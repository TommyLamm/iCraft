# Architecture

> Last verified: 2026-08-04
> Git baseline: tommy-dev
>
> This document is a concise navigation map. Source code remains authoritative.

## System overview

`iCraft` is a single-binary Rust voxel game:

- `winit` owns the desktop event loop and input.
- `wgpu` renders the menu, terrain, entities, particles, and immediate-mode UI.
- The main thread owns authoritative gameplay `State`.
- Rayon workers generate/load chunks and build terrain meshes.
- Dedicated Tokio threads run TCP host/client networking.
- A background save worker handles autosaves and chunk-unload writes.
- Terrain, the texture atlas, and missing audio assets can be generated
  procedurally. On startup the atlas is overlaid with the Stay True resource
  pack (`F:\Desktop\Stay True 1.21.5`, override via `ICRAFT_RESOURCE_PACK`),
  with vanilla 1.21.5 fallback textures under `assets/vanilla/textures` for
  everything the pack does not override.

There is no database or separate dedicated-server binary. Multiplayer uses a
listen-server model: the host runs the authoritative world simulation, while
joining clients render a synchronized local copy.

## Entrypoints and ownership

```text
src/main.rs
  -> App (src/app.rs)
     -> Menu (src/menu.rs)
     -> State (src/state.rs)
        -> update(dt): networking + simulation + streaming
        -> render(): visibility + GPU passes + UI
```

- `main` declares modules and starts `EventLoop::run_app`.
- `App` owns the `Menu`/`Game` runtime transition, frame timing, OS events,
  input priority, cursor mode, resize, and surface-error handling.
- `Menu` owns world discovery/creation, settings, controls, and multiplayer
  launch options.
- `State` is the composition root and main coupling hotspot. It owns GPU
  resources, camera, loaded chunks, mesh/render caches, player/inventory,
  entities, dimensions, weather, redstone, advancements, audio, networking
  bridges, and in-game UI.

On Windows, menu and game GPU initialization intentionally select DX12 because
the primary Vulkan path has caused a verified NVIDIA driver crash.

## Runtime flow

### Startup

`App::resumed` creates the menu. A selected `WorldLaunch` is applied from
`about_to_wait`, after the window callback returns. `State::new` then:

1. Creates wgpu pipelines, buffers, atlas, and audio state.
2. Loads world/player/dimension data and eligible saved entities.
3. Restores or generates the initial chunks and lighting.
4. Builds initial terrain meshes and starts background services.
5. Streams the remaining render distance incrementally.

Joining clients wait for a successful protocol-v7 login before using the host's
seed and synchronized world state.

### Per-frame update

`State::update` drains network events first, then advances the major systems:

1. Autosave and fixed/budgeted simulation work.
2. Portals, redstone, brewing, effects, advancements, particles, and weather.
3. Player input/physics, damage/survival state, interactions, and chunk
   streaming.
4. Projectiles, hostile/passive mobs, bosses, dropped items, and entity cleanup.
5. Camera/uniform synchronization and continuous mining.

Paused/dead/UI states gate gameplay input, but maintenance work that must remain
safe across pauses is handled before the relevant early return. Inspect
`State::update` before changing ordering.

### Rendering

Terrain CPU work is separated from GPU ownership:

```text
ChunkManager chunks
  -> SectionIdentity + owned 18³ halo snapshot
  -> Rayon 16³ section mesh job
  -> dimension/lifetime/section-revision validation
  -> per-section RenderRegion GPU arena upload
  -> frustum/section visibility + LOD draw plan
  -> wgpu render passes
```

The renderer then generates mob mesh data, camera-facing particle quads, and all
immediate-mode UI vertices (including remote-player name tags, chat, disconnect
UI, and advancement toasts/screen) on the CPU.
The render pass order is: sky ->
opaque/cutout chunks -> mobs (including dropped items) -> translucent chunks ->
alpha-blended particles -> multiply-blended mining crack overlay -> colored UI ->
textured UI -> crosshair -> line/text UI -> present. The shader entrypoints
and packed camera, lighting, fog, time, underwater, and damage behavior are in
`src/shader.wgsl`. Terrain uses the separate `TerrainVertex` layout and
`vs_terrain`/`fs_terrain`; AO remains smooth, while atlas tile and packed
sky/block/face lighting remain flat. Mob, hand, particle, and UI geometry keep
their existing vertex layouts.

`world.rs` produces terrain mesh data; `chunk_render.rs` defines terrain
vertices, bounds, LOD data, draw planning, and region allocations.
`culling.rs` performs bounded section visibility traversal and conservative
snapshot-based entity LOS. Dirty connectivity fails open until the matching
world revision is available.
`chunk_schedule.rs` prioritizes bounded load/mesh work.
`State::render` owns final submission. Per-section terrain allocations carry
exact generation/lifetime/revision identity; instance buffers use a bounded
completion-protected frame-resource pool. The held-item base mesh is cached by
item/model key and walk/swing animation is applied through a uniform instead of
rebuilding CPU geometry.

The event loop owns the optional FPS deadline while simulation consumes real
elapsed time. The former viewport-only dynamic-resolution path is forced to
native scale; it must not be re-enabled without an offscreen render target,
upscale pass, and native-resolution UI.

The high-level pass order is sky, opaque terrain, entities, translucent terrain,
particles/effects, mining overlay, UI, and present.

## World mutation rules

`ChunkManager::chunks` is authoritative world state. Terrain meshes, visibility
sets, GPU allocations, and particle vertices are derived caches.

For authoritative block mutations:

```text
world_mutation::apply_batch / BlockMutationRequest
  -> validate positions, loaded chunks, and block entity types
  -> atomic commit of block, state, and BlockEntity
  -> update sky/block lighting
  -> perform support cascade (unsupported block break)
  -> invalidate dependent meshes (boundary/AO)
  -> trigger redstone notifications & bump chunk mutation revision
  -> broadcast authoritative BlockChange & BlockEntityDelta when hosting
```

`ChunkManager::chunks` is authoritative world state, including per-chunk `block_entities` keyed by `(u8, i16, u8)` local coordinates. Terrain meshes, visibility sets, GPU allocations, and particle vertices are derived caches.
`chunk_manager::mark_block_mesh_dependencies` is the shared mesh dependency rule.
Redstone returns `BlockMutation` records and side-effect actions applied via host transaction handlers.

`BlockState` encodes facing (2 bits), is_top (1), is_right_hinge (1), is_open (1), and chest_type (2 bits: Single/Left/Right) in a single byte. Bit 7 is reserved. For Farmland and Crops (Wheat, Carrot, Potato), state byte `u8` stores moisture level (0..7) and crop growth age (0..7) in bits `0..2`.

`src/voxel_shape.rs` defines `VoxelShape` (holding up to 8 AABBs without heap allocation) to provide unified `block_collision_shape`, `block_selection_shape`, and `block_occlusion_shape`. Player physics (`physics.rs`) iterates over all constituent AABBs for movement collision and ladder climbing. DDA raycasting (`interaction.rs`) queries `block_selection_shape.ray_intersects` at each voxel step. Non-full blocks (Slabs, Stairs, Fences, Fence Gates, Walls, Panes, Ladders, Signs) bypass greedy meshing via `is_greedy_cube` and generate faces via `src/block_model.rs` (`append_custom_block_mesh`).

`SignBlockEntity` in `src/block_entity.rs` provides text storage (4 lines of up to 15 UTF-8 characters) with full save persistence and backward compatibility.

`src/world_tick.rs` implements the `RandomTickEngine` and deterministic random tick sampling (`sample_random_ticks`). On each simulation tick, loaded chunk sections with `random_tick_count > 0` are sampled (3 voxels/section/tick standard). Sampling uses a deterministic PRNG seeded by `(world_seed, game_tick, dimension, section_id)` with a maximum section budget (512 sections/tick) to prevent frame drops at large render distances. Random ticks update farmland hydration/degradation and crop age progression.

Food items define `FoodProperties` (hunger, saturation, eating duration ticks, always_edible, return_item). Hold-to-eat right click state machine tracks continuous usage duration, supporting item/slot/death cancellation and triggering `AdvancementTrigger::EatFood` on completion.

## Multiplayer authority

`src/network/` contains a versioned bincode protocol over length-prefixed TCP:

- `NetworkServer` and `NetworkClient` each run Tokio on a background thread.
- Main-thread `State` communicates with them through synchronous channels.
- Player poses are sequenced, timestamped, coalesced, and rendered from a
  bounded interpolation buffer.
- Reliable queues carry login, chat, chunk, block, container transactions (open/click/close/slot update), and time/weather state.

Container operations (open/click/close) use host-authoritative transactions with `ContainerOpenRequest`/`SendContainerOpenResult`, `ContainerClickRequest`/`SendContainerClickResult`, `BroadcastContainerSlotUpdate`, and `ContainerClose` packets over protocol v7.
`ContainerSessionManager` tracks player ID, dimension, block position, and combined single (27)/double (54) chest slot mapping.
The host validates reach distance (<= 8.0 blocks), dimension, top-block solid obstruction, and container block presence before committing slot mutations.
Rejected requests return `success: false` without partial side effects. When a chest is broken, destroyed, or a player disconnects/switches dimensions, all associated sessions close automatically.

The host is the sole authority for world mutations. Remote break/place requests
are validated against authenticated player state, reach, loaded chunks,
placement support, and player collision. Rejected requests must not consume
inventory, create drops, play action sounds, or mutate/broadcast the world.
Clients apply inventory/tool/advancement side effects only after a successful
host result.

Clients apply synchronized blocks through the storage/light/mesh path only.
Redstone, fluids, weather placement, random ticks, explosions, mob world
changes, and unsupported-block cascades remain host-side. Unloaded-chunk changes
are deferred and replayed after stream-in.

## Persistence and configuration

| Path | Authoritative contents |
| --- | --- |
| `settings.txt` | Display, audio, difficulty, language, view, and related `GameSettings` values. |
| `controls.config` | Configurable key bindings; loaded and saved by `GameSettings`. |
| `saves/<world>/world.meta` | World-list name, seed, game mode, difficulty, and last-played time. |
| `saves/<world>/level.dat` | Bincode `LevelData`: seed, game time, world spawn coordinates, dimension, yaw, and version. |
| `saves/<world>/player.dat` | Bincode player, inventory/item metadata, game mode, XP, spawn point, spawn dimension, unlocked_recipes, and advancement progress. |
| `saves/<world>/dimension.dat` | Active dimension; missing legacy files default to Overworld. |
| `saves/<world>/entities.dat` | Persistent Overworld living/persistent/dropped entities. |
| `saves/<world>/regions/` | Overworld region data. |
| `saves/<world>/regions/` (block_entities) | Per-chunk `BlockEntity` data (chest inventories, furnace block entities with 3 slots/burn/cook progress/accumulated XP, sign text) serialized via `ChunkSaveData`; `LegacyBlockEntity` fallback guarantees backward compatibility. |
| `saves/<world>/dimensions/{nether,end}/` | Dimension-specific entities and regions. |

`SaveManager` owns serialization, legacy-player upgrades, atomic sidecar writes,
compressed chunk data, region caching, and dimension-aware paths. Five-minute
autosaves and unload saves use a bounded latest-wins queue with per-Chunk
dirty/in-flight/persisted revisions. Worker ACKs carry real save errors; failed
snapshots remain retryable. Window close and “Save and Quit” flush synchronously,
and a failed flush stays in-game with retry/abandon controls.

Transient state includes projectiles, particles, remote-player snapshots,
workstation progress, active effects, advancement UI state, and Creative flight.

## Module map

| Area | Primary files |
| --- | --- |
| App lifecycle and menu | `main.rs`, `app.rs`, `menu.rs` |
| Composition, simulation, UI, GPU submission | `state.rs` |
| World/chunks/generation | `world.rs`, `chunk_manager.rs`, `dimension.rs` |
| Lighting, fluids, block targeting | `lighting.rs`, `fluid.rs`, `interaction.rs` |
| Terrain scheduling/rendering | `chunk_schedule.rs`, `chunk_render.rs`, `culling.rs`, `shader.wgsl` |
| Player, recipes, gameplay data | `physics.rs`, `player.rs`, `inventory.rs`, `recipes.rs`, `crafting.rs` |
| Equipment and effects | `enchantment.rs`, `brewing.rs`, `hand_renderer.rs` |
| Entities and AI | `entity.rs`, `mob.rs`, `passive_mob.rs`, `boss.rs`, `mob_renderer.rs` |
| Container & workstation system | `block_entity.rs` (ChestBlockEntity, FurnaceBlockEntity), `inventory.rs` (ContainerInventory), `recipes.rs` (CraftingRecipe, SmeltingRecipe, FuelDefinition, RecipeManager), `state.rs` (SlotType::ContainerSlot, container_target, open_chest, recipe_book_open, update_furnaces) |
| Networking | `network/{protocol,transport,server,client}.rs` |
| Persistence and assets | `save.rs`, `texture.rs`, `audio.rs` |
| Performance instrumentation | `perf.rs`, `performance/` |

Start with the exact symbol related to the task; avoid reading all of
`state.rs`.

## Signed vertical world (Plan 08)

The world now uses a signed `min_y=-64, height=384` scheme for the Overworld
(`dimension.rs:38-101`). `WorldHeight` provides `contains_y`, `section_index`,
`section_y_at_index`, `min_section_y`, `max_section_y_exclusive` helpers.

`Chunk` (`world.rs:3279`) stores sparse `Vec<Option<ChunkSection>>` indexed by
`min_section_y` rather than a dense `[ChunkSection; 16]`. Empty sections are
`None` and consume no storage. The `SectionKey.section_y` field is `i8`.

`Chunk` methods (`get_block_local`, `set_block_local`, `get_sky_light`, etc.)
accept `wy: i32` world-Y and use `world_y_to_section_y`/`world_y_to_local_y`
checked helpers. The chunk `heightmap` uses `i16` with `NO_HEIGHT = -9999`
sentinel.

All dimension-bound systems (physics collision, void damage, mob spawn, portal
placement, fluid tick, light propagation, world mutation validation) now use
`dimension.height()` / `WorldHeight::contains_y` instead of hardcoded
`0..CHUNK_HEIGHT`.

Network protocol v10: `ChunkData` packet carries explicit `min_section_y: i8`
and `section_count: u16`. Save format v2: `ChunkSaveData::data_version = 2`
with height-aware flat array serialization.

```rust
// Safe checked helpers in world.rs:
world_y_to_section_y(y: i32) -> i8;        // y >> 4
world_y_to_local_y(y: i32) -> u8;          // y.rem_euclid(16)
section_and_local_y_to_world_y(sy: i8, ly: u8) -> i32;
```

## Architectural invariants and hotspots

- `State` is intentionally central but mixes simulation, GPU setup, networking,
  UI, and interactions; preserve ordering and authority boundaries.
- Chunk and entity collections are authoritative; meshes and render data are
  disposable caches.
- Background chunk/mesh results carry generation/revision identity. Discard
  stale results rather than uploading them.
- GPU buffers and wgpu submission stay on the main thread.
- Host-only systems must not run authoritatively on joining clients.
- Entity persistence includes living, explicitly persistent, dropped-item (with full `ItemStack` metadata and 5-minute loaded-chunk despawn timer), and experience-orb entities; remote players and short-lived projectiles are not saved.
- Advancement definitions do not subscribe automatically. New event producers
  must call `State::trigger_advancement` at the authoritative mutation point.
- Dimension switches must keep chunk/entity saving, runtime reset, portal
  placement, and `dimension.dat` updates together.
- Redstone component metadata is stored with chunk data; legacy saves may not
  contain it.
- `Inventory` and `InventoryData` feature `offhand: Option<ItemStack>`, with `#[serde(default)]` backward compatibility for legacy save formats. F key swaps selected hotbar item with offhand item.
- Combat calculation (`calculate_damage_reduction`, `can_shield_block`, `calculate_attack_damage`, `calculate_bow_shot`) is host-authoritative and uses pure functions in `player.rs`.
- Shield blocking (180° facing arc) reduces damage by 100% for blockable sources and degrades shield durability; Axe attacks trigger a 5-second (100 ticks) shield disable.
- `settings.txt` and `controls.config` are working-directory-relative. Keep
  parsing defaults and sanitization backward compatible.

## Verification

Behavioral tests are mostly inline `#[cfg(test)]` tests. The package has no
`src/lib.rs`, so `tests/passive_mob_tests.rs` cannot directly exercise internal
modules and remains a placeholder.

The performance plans 01–14 remain `Partial` until their fixed-scene GPU/window
and PGO artifacts exist; this status is not a claim that their runtime repair
work is absent. The authoritative status and outstanding artifact gates are
tracked in
[`performance/performance_track.md`](performance/performance_track.md) and
[`performance/15_performance_audit_repair_plan.md`](performance/15_performance_audit_repair_plan.md).
The host-authoritative model above remains the invariant; R4 repaired the
joining-client simulation/replication and pause/death-policy gaps.

Use:

```text
cargo test
cargo check --release
cargo run
```

`cargo run` requires a window/GPU and optionally an audio device; audio can
degrade to silent operation.


