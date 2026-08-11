# Architecture

> Last verified: 2026-08-11
> Git baseline: tommy-dev
>
> This document is a concise navigation map. Source code remains authoritative.

## System overview

`iCraft` is a Rust voxel game plus a headless dedicated-server binary:

- `winit` owns the desktop event loop and input.
- `wgpu` renders the menu, terrain, entities, particles, and immediate-mode UI.
- `authority::AuthorityCore` owns the GPU-independent gameplay contract; the
  desktop `State` is a presentation/input composition root and projects
  Singleplayer/Host authority snapshots into renderer caches.
- Rayon workers generate/load chunks and build terrain meshes.
- Dedicated Tokio threads run TCP host/client networking.
- `src/lib.rs` exposes the simulation/network contract to `icraft-server`.
- `authority::{AuthorityCore, contract}` and `server_world.rs` own the
  headless fixed tick, authenticated request sequencing, world mutations,
  rules, commands, entities, block entities, and automation. `server_runtime.rs`
  owns transport/session scheduling, save and metrics boundaries without
  constructing a GPU, window, or audio device.
- A background save worker handles autosaves and chunk-unload writes.
- Terrain, the texture atlas, and missing audio assets can be generated
  procedurally. `resources.rs` discovers the built-in `assets/` pack and
  workspace-relative `resourcepacks/` entries, validates manifests/dependency
  order and bounded ZIP contents, and resolves selected textures, sounds,
  language, model, and font descriptors. Selected model descriptors are captured
  in an immutable registry shared by background L0/L1/L2 mesh jobs; selected
  bitmap glyphs feed Menu and State text renderers. `ICRAFT_RESOURCE_PACK` is an
  explicit development/test override only. Missing assets retain procedural or
  built-in fallbacks and are diagnosed once; shader overrides are not supported.

There is no database. Multiplayer supports both the existing listen-server
model and `icraft-server`: the host/runtime owns authoritative simulation,
while joining clients render a synchronized local copy.

## Entrypoints and ownership

```text
src/main.rs
  -> App (src/app.rs)
     -> Menu (src/menu.rs)
     -> State (src/state.rs)
        -> update(dt): networking + simulation + streaming
        -> render(): visibility + GPU passes + UI
```

`src/bin/icraft-server.rs` loads `server.properties`, starts
`server_runtime::ServerRuntime`, and runs the same network authority without
the desktop composition root. Ctrl-C and console `save`/`stop` synchronously
flush level and per-player files.

- `main` declares modules and starts `EventLoop::run_app`.
- `App` owns the `Menu`/`Game` runtime transition, frame timing, OS events,
  input priority, cursor mode, resize, and surface-error handling.
- `Menu` owns world discovery/creation, settings, controls, and multiplayer
  launch options.
- `State` is the composition root and main coupling hotspot. It owns GPU
  resources, camera, loaded chunks, mesh/render caches, player/inventory,
  entities, dimensions, weather, redstone, advancements, audio, networking
  bridges, and in-game UI.

`AuthorityBoundary` is the in-process bridge used by Singleplayer and the
listen-server host. It registers a local pseudo-session and submits the same
`GameplayRequest`/fixed-tick path as `AuthorityCore` in the dedicated binary.
Accepted request mutations and fixed-tick snapshots are projected one-way into
`State` for lighting, mesh invalidation, block-entity UI, and local input
feedback. Boundary worlds disable the renderer's legacy redstone, fluid,
random-tick, furnace, mob, vehicle, village, and dropped-entity authority
paths; unsupported presentation interactions are rejected by the core. `SaveManager`
v2 now owns dedicated player current-dimension/effects files, checked authority
chunk/entity restore, and atomic region-cache commit/retry semantics. The runtime
maintains dimension-aware view/simulation/container interest and exposes a bounded
`RoutedInterestUpdate` ledger for the transport owner. Targeted wire-packet encoding
and concurrent login reservation remain Plan 18 B/D follow-up work; this ledger must
not be presented as a completed network E2E migration.

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

Joining clients wait for a successful protocol-v17 login before using the host's
seed and synchronized world state.

### Per-frame update

`State::update` drains network events first, then advances the major systems:

1. Fixed/budgeted authority ticks are scheduled by `AuthorityCore` (or
   `ServerRuntime` for dedicated) and mutation/snapshot deltas are projected
   into renderer caches.
2. Player local input/physics presentation and chunk streaming.
3. Camera/uniform synchronization, UI, and continuous mining.

The legacy renderer simulation remains available only for a compatibility path
without an `AuthorityBoundary`; Singleplayer and listen-host worlds do not
enter it.

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

`ServerWorld::chunks` is authoritative for the headless authority path. The
renderer `ChunkManager::chunks` is a one-way presentation projection; terrain
meshes, visibility sets, GPU allocations, and particle vertices are always
derived caches.

`AuthorityCore` owns an active compatibility `ServerWorld` plus a
dimension-keyed map of independent headless worlds, a deterministic 20 Hz tick,
sorted session/entity iteration, per-dimension `RevisionClock`s, bounded response
cache, and the `SessionContract` dimension/position/permission gates. Requests
route by the authenticated session dimension; `world_ref`/`world_mut` read a
dimension without switching the compatibility view and `with_world` restores the
caller's active view after a bounded operation. `ServerRuntime` submits
authenticated envelopes and schedules/saves/metrics the core; it does not
mutate a second authoritative block or entity map.

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

`ServerWorld::chunks` is authoritative world state, including per-chunk
`block_entities` keyed by `(u8, i16, u8)` local coordinates. The renderer
`ChunkManager::chunks` carries a projection for mesh and UI consumption; it is
not a second authority. Terrain meshes, visibility sets, GPU allocations, and
particle vertices are derived caches.
`chunk_manager::mark_block_mesh_dependencies` is the shared mesh dependency rule.
Redstone returns `BlockMutation` records and side-effect actions applied via host transaction handlers.

Container automation keeps `BlockEntity` slots, revisions, hopper cooldown/power,
facing, and observer baselines as authoritative state. `ContainerAccess` is the
shared sided-capability gate for UI clicks and hopper transfers; complete slot
vectors are committed atomically. A host redstone tick runs observers, bounded
hopper work (one item per transfer), furnace progression, and dispenser/dropper
actions. Joining clients never simulate these mutations; they only apply the
host's revision-gated `BlockEntityDelta`/slot updates. Comparator dependencies are
woken by container revision notifications rather than a world-wide per-tick scan.

`BlockState` encodes facing (2 bits), is_top (1), is_right_hinge (1), is_open (1), and chest_type (2 bits: Single/Left/Right) in a single byte. Bit 7 is reserved. For Farmland and Crops (Wheat, Carrot, Potato), state byte `u8` stores moisture level (0..7) and crop growth age (0..7) in bits `0..2`.

Plan26 completes the container lifecycle seam without changing protocol v16:
`ContainerSessionManager::close_by_block` is dimension-scoped and
`close_exact` is player/position-scoped; `ServerWorld` emits closure intents for
block replacement and `ServerRuntime` routes targeted `ContainerClose` packets
for distance/invalid-dimension rejection, interest departure, transfer, logout,
and disconnect. `NetworkClient` accepts a forced close only when its active
`(dimension, x, y, z)` key matches and bypasses the normal container revision
gate. `State::force_close_inventory` is an idempotent transient-UI cleanup path
that never submits another close or returns/drops a cursor item. First/last
viewers toggle both halves of a double chest through `is_open`; the renderer
uses deterministic binary chest geometry and `audio.rs` emits one deterministic
ChestOpen/ChestClose edge fallback. The v16 packet has no epoch/reason/cursor,
so same-key stale-close disambiguation remains a v17 follow-up; smooth lid,
GPU/audio-device, and Host+Join visual evidence remain outside headless claims.

Plan27 adds the minimal waterlogging authority lane without changing the
`BlockState` bit layout. Only `OakSlab` and `CobblestoneSlab` interpret bit 7 of
the chunk raw-fluid byte as `WATERLOGGED`; level/falling bits and reserved bits
retain their existing masks. `GameplayOperation::FluidUse` validates the exact
selected `SlotRefWire`, hand, face, reach, and dimension before atomically
committing a `WaterBucket`/`Bucket` transaction through `ServerWorld`. A
waterlogged slab remains solid while the fixed fluid tick may source adjacent
air, including across chunk boundaries; raw-only fluid transitions still carry
their own revision-bearing `WorldMutation`. Save v3 persists the byte unchanged,
and protocol v17 carries it through `BlockChange.raw_fluid` and
`ChunkData.fluid_levels` in both embedded and socket projection paths. The
complement translucent slab mesh reuses the existing halo/translucent pass while
collision, voxel shape, and light values stay on the host slab semantics.

`src/voxel_shape.rs` defines `VoxelShape` (holding up to 8 AABBs without heap allocation) to provide unified `block_collision_shape`, `block_selection_shape`, and `block_occlusion_shape`. Player physics (`physics.rs`) iterates over all constituent AABBs for movement collision and ladder climbing. DDA raycasting (`interaction.rs`) queries `block_selection_shape.ray_intersects` at each voxel step. Non-full blocks (Slabs, Stairs, Fences, Fence Gates, Walls, Panes, Ladders, Signs) bypass greedy meshing via `is_greedy_cube` and generate faces via `src/block_model.rs` (`append_custom_block_mesh`).

`SignBlockEntity` in `src/block_entity.rs` provides text storage (4 lines of up to 15 UTF-8 characters) with full save persistence and backward compatibility.

`src/world_tick.rs` implements the `RandomTickEngine` and deterministic random tick sampling (`sample_random_ticks`). On each simulation tick, loaded chunk sections with `random_tick_count > 0` are sampled (3 voxels/section/tick standard). Sampling uses a deterministic PRNG seeded by `(world_seed, game_tick, dimension, section_id)` with a maximum section budget (512 sections/tick) to prevent frame drops at large render distances. Random ticks update farmland hydration/degradation and crop age progression.

Food items define `FoodProperties` (hunger, saturation, eating duration ticks, always_edible, return_item). Hold-to-eat right click state machine tracks continuous usage duration, supporting item/slot/death cancellation and triggering `AdvancementTrigger::EatFood` on completion.

## Multiplayer authority

The shared headless authority lives in `authority::AuthorityCore` and
`ServerWorld`:

- Singleplayer and listen-server hosts use `AuthorityBoundary` in-process;
  dedicated mode constructs the same `AuthorityCore` without `State`.
- `SessionContract` validates authenticated identity, dimension, reach,
  permissions, client sequence/revision, and the bounded response cache.
- Each loaded dimension ticks in stable wire order with its own revision/time
  namespace; snapshots aggregate mutations/checksums deterministically while
  clients gate deltas by `(dimension, revision)`, never by the aggregate max.
- `SaveManager` persists dedicated player payloads (current dimension separate
  from spawn dimension), authoritative chunk/block-entity/entity snapshots, and
  a dimension-scoped mutation-revision index. `ServerRuntime` traverses all
  loaded dimensions, merges the revision index once, and restores the active
  compatibility dimension. Region caches are committed only after an atomic
  replacement succeeds, preserving the previous snapshot for a retry on failure.
- `InterestSet` tracks per-session view/simulation chunks, simulation entities,
  and open container viewers. `ServerRuntime::drain_routed_updates` exposes the
  bounded, dimension-checked routing ledger; the network packet adapter remains
  a separate Plan 18 B/D seam.
- `authority::{fishing,transactions,combat}` are the gameplay-domain seams:
  fishing and brewing advance exactly one fixed 20 Hz step, rich workstation
  sources are compare-and-committed atomically, and combat derives damage from
  authenticated pose/cooldown/equipment before publishing session/entity death,
  drops, XP, shield durability, and respawn deltas. Brew action `2` is the
  explicit ready-output take operation in protocol v16; fixed ticks never debit
  reserved inputs on their own. `GameplayOperation::FluidUse` is the protocol
  v17 typed water-bucket seam for the bounded slab waterlogging set.
- `ServerRuntime` is transport/session/scheduling/save/metrics glue. It does
  not maintain a parallel authoritative block/entity map.

The Phase A boundary cutover and Phase C persistence/interest seams are covered by
headless authority/projection tests, including simultaneous Overworld/Nether
sessions and active-view restoration. Atomic concurrent login, bounded transport,
fault injection and metrics have automated evidence; Plan25 now also supplies the
server-owned difficulty consumer (strict properties parse/persistence, Peaceful
hostile cleanup, and Easy/Normal/Hard chase policy). GPU Host+Join, complete
multi-dimension reconnect/failure matrices, and remaining topology acceptance
are still manual or follow-up work. Headless tests must not be presented as a
GPU/manual pass.

`src/network/` contains a versioned bincode protocol over length-prefixed TCP:

- `NetworkServer` and `NetworkClient` each run Tokio on a background thread.
- Main-thread `State` communicates with them through synchronous channels.
- Player poses are sequenced, timestamped, coalesced, and rendered from a
  bounded interpolation buffer.
- Reliable queues carry login, chat, chunk, block, container transactions (open/click/close/slot update), and time/weather state.
- `GameplayRequest`/`GameplayResponse` is the common request/ACK envelope for
  block, container, item-use, combat, sleep, trade, mount, and command
  operations. It carries request ID, client sequence, authenticated session,
  dimension/revision and bounded rejection reasons; the server keeps a bounded
  idempotency cache and per-session request rate limiter.
- `ServerListPingRequest`/`ServerListPingResponse` reports protocol version,
  MOTD, and online/max player counts.

Container operations (open/click/close) use host-authoritative transactions with `ContainerOpenRequest`/`SendContainerOpenResult`, `ContainerClickRequest`/`SendContainerClickResult`, `BroadcastContainerSlotUpdate`, and `ContainerClose` packets over protocol v17 (the container fields retain their v16 shape). Slot updates carry the container entity revision; duplicate, stale, wrong-dimension, or out-of-range updates are discarded before any local mutation. The click result updates only the cursor; the authoritative slot value arrives through the revision-bearing update/delta. `WorldRulesSync` carries the host's serialized `WorldRules` snapshot to clients; clients apply it for display/runtime policy and cannot submit rule mutations. Trading and raid packets remain versioned under the same protocol.
`ContainerSessionManager` and `MerchantSessionManager` track player ID, dimension, villager ID, and active trade offers.
`PoiManager` (`src/village/poi.rs`) indexes Bed and JobSite POIs by chunk with max-distance spatial hashing, maintaining spatial village clusters for villager assignment and bed count tracking.
`RaidManager` (`src/village/raid.rs`) tracks active village raids, wave progression (Pillager/Ravager counts), Bad Omen triggers, and raid victory/defeat states.
`Villager` entities execute profession claiming, food harvest, restocking, bed sleeping, breeding, and level progression based on trade XP.
The host validates reach distance (<= 8.0 blocks), dimension, top-block solid obstruction, and container block presence before committing slot mutations.
Rejected requests return `success: false` without partial side effects. When a chest is broken, destroyed, too far away, leaves the interest set, or a player disconnects/switches dimensions, only the matching player/dimension/position sessions close automatically; a late close cannot tear down a different active key. Forced client cleanup is deliberately non-recursive and does not mint or duplicate cursor items.

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

## Modes, world rules, and commands (Plan 15)

`game_rules.rs` is the single runtime policy layer. `GameModePolicy` derives
collision, phase/noclip, damage, hunger, flight, interaction, pickup, and mob
targeting decisions from the player's `GameMode` and the host's `WorldRules`.
Adventure item stacks carry compact `can_break`/`can_place_on` block masks;
Spectator is read-only and cannot open or mutate containers. Hardcore is a
persisted world property (with Hard difficulty) and a persisted player death
marker, so reconnect/respawn cannot silently return a dead player to Survival.

`commands/` contains the bounded typed parser and dispatcher used by host
commands. Commands are accepted only from the host/authorized operator or when
single-player cheats are enabled; ordinary client chat is never interpreted as
an administrative command. Rule changes are saved with `LevelData` and sent to
clients through `WorldRulesSync`. The menu persists Default/Superflat creation
options and validates world copy/backup/delete paths beneath the canonical
`saves/` root.

## Persistence and configuration

| Path | Authoritative contents |
| --- | --- |
| `settings.txt` | Display, audio, difficulty, language, view, and related `GameSettings` values. |
| `controls.config` | Configurable key bindings; loaded and saved by `GameSettings`. |
| `saves/<world>/world.meta` | World-list name, seed, game mode, difficulty, world type, structure/bonus/cheat flags, Hardcore flag, and last-played time. |
| `saves/<world>/level.dat` | Bincode `LevelData`: seed, game time, world spawn coordinates, dimension, yaw, format version, world type/structure flags, Hardcore flag, and the serialized `WorldRules` snapshot. |
| `saves/<world>/player.dat` | Bincode player, inventory/item metadata (including Adventure break/place masks), game mode, XP, spawn point, spawn dimension, unlocked_recipes, advancement progress, and the persistent death marker used by Hardcore. |
| `world/players/<username>.dat` | Dedicated-runtime v2 per-player atomic save: current dimension plus the complete `PlayerData` payload (including separate spawn dimension) and active effects. Version-1 files migrate using the saved spawn dimension; username is sanitized and duplicate logins are explicitly rejected. |
| `server.properties` | MOTD, bind/port, player cap, difficulty, online-mode placeholder, whitelist/operators, view/simulation distance, PvP, world path, and seed. |
| `saves/<world>/dimension.dat` | Active dimension; missing legacy files default to Overworld. |
| `saves/<world>/entities.dat` | Persistent Overworld living/persistent/dropped entities. |
| `saves/<world>/regions/` | Overworld region data; authoritative chunk payloads include block entities and per-chunk mutation revisions. Failed replacements leave the prior region/cache snapshot available for retry. |
| `saves/<world>/regions/` (block_entities) | Per-chunk `BlockEntity` data (chest/furnace inventories and progress, hopper 5-slot transfer state, dispenser/dropper 9-slot inventories, observer baseline/pulse state, sign text) serialized via `ChunkSaveData`; legacy block-entity decoding and serde defaults preserve older saves. |
| `saves/<world>/dimensions/{nether,end}/` | Dimension-specific entities and regions. |

`SaveManager` owns serialization, legacy-player upgrades, dedicated player files,
atomic sidecar writes, compressed chunk data, region caching, and dimension-aware
paths. It also exposes checked authority restore and a bounded mutation-revision
index for `ServerRuntime`. Five-minute
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
| Composition, presentation/input, UI, GPU submission | `state.rs` |
| Headless authority, fixed tick, sessions, revisions, interest routing | `authority/{mod,contract,interest,combat,fishing,transactions}.rs`, `server_world.rs`, `server_runtime.rs` |
| World/chunks/generation & structures | `world.rs`, `chunk_manager.rs`, `dimension.rs`, `worldgen/{mod, climate, density, surface, carver, ore, feature}.rs`, `structure/{types, placement, gen/*, manager, locate}.rs`, `loot.rs` |
| Lighting, fluids, block targeting | `lighting.rs`, `fluid.rs`, `interaction.rs` |
| Terrain scheduling/rendering | `chunk_schedule.rs`, `chunk_render.rs`, `culling.rs`, `shader.wgsl` |
| Player, recipes, gameplay data | `physics.rs`, `player.rs`, `inventory.rs`, `recipes.rs`, `crafting.rs` |
| Equipment and effects | `enchantment.rs`, `brewing.rs`, `hand_renderer.rs` |
| Entities and AI | `entity.rs`, `spawning.rs`, `ai/{mod, goal, brain, navigation}.rs`, `mob.rs`, `passive_mob.rs`, `boss.rs`, `mob_renderer.rs` |
| Container & automation system | `block_entity.rs` (ContainerAccess, Chest/Furnace/Hopper/Dispenser/Dropper/Observer entities), `container_sessions.rs` (atomic UI transactions), `inventory.rs` (ContainerInventory/ItemStack), `world_tick.rs` (bounded hopper transfers), `redstone.rs` (comparators/observers/actions), `recipes.rs` (CraftingRecipe, SmeltingRecipe, FuelDefinition, RecipeManager), `state.rs` (host furnace/dispense loop and revision-gated replication) |
| Transport, mounts, navigation & fishing | `vehicle.rs` (MountManager, BoatState), `rail.rs` (MinecartState, RailShape), `navigation.rs` (Compass, Clock, MapData), `fishing.rs` (FishingManager, loot rolling) |
| Networking and dedicated runtime | `network/{protocol,transport,server,client}.rs`, `server_runtime.rs`, `bin/icraft-server.rs` |
| Persistence and assets | `save.rs`, `texture.rs`, `audio.rs`, `resources.rs` |
| Localization and accessibility | `localization.rs`, `accessibility.rs`, `menu.rs`, `state.rs` |
| Modes, rules, commands | `game_rules.rs`, `commands/`, `state.rs`, `menu.rs` |
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

Network protocol v17: `ChunkData` packet carries explicit `min_section_y: i8`,
`section_count: u16`, and raw `fluid_levels`; `BlockChange` carries the raw
fluid byte alongside block/state. Block-entity variants and container updates
carry stable revisions. Save format v3: `ChunkSaveData::data_version = 3` with
height-aware flat arrays, compressed block entities, and redstone metadata.
Legacy 0..255 format (data_version 0/1/2)
maps into Y=0..255 with Y<0 and Y>=256 remaining empty/Air. SaveManager creates `.bin.bak`
backups before modifying existing region files and aborts on deserialization corruption without overwriting.

```rust
// Safe checked helpers in world.rs:
world_y_to_section_y(y: i32) -> i8;        // y >> 4
world_y_to_local_y(y: i32) -> u8;          // y.rem_euclid(16)
section_and_local_y_to_world_y(sy: i8, ly: u8) -> i32;
```

## Architectural invariants and hotspots

- `State` is intentionally central for presentation/input but still mixes the
  legacy renderer simulation with GPU setup, networking, UI, and interactions
  during the Plan 18 cutover; preserve ordering and authority boundaries.
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
- Hopper/dispenser/dropper/observer fields are part of the v3 block-entity
  payload; missing fields use serde defaults, and legacy `LegacyBlockEntity`
  entries are migrated before insertion. Slot metadata (count, durability,
  enchantments, potion data, and custom names) is copied unchanged by every
  automation transfer.
- The host is the only authority for automation, comparator output, observer
  pulses, item consumption, drops, and furnace progression. Hopper work is
  capped per tick and observer scheduling is bounded; stale chunk/network
  revisions are ignored rather than partially committed.
- `Inventory` and `InventoryData` feature `offhand: Option<ItemStack>`, with `#[serde(default)]` backward compatibility for legacy save formats. F key swaps selected hotbar item with offhand item.
- Combat calculation (`calculate_damage_reduction`, `can_shield_block`, `calculate_attack_damage`, `calculate_bow_shot`) is host-authoritative and uses pure functions in `player.rs`.
- Shield blocking (180° facing arc) reduces damage by 100% for blockable sources and degrades shield durability; Axe attacks trigger a 5-second (100 ticks) shield disable.
- `settings.txt` and `controls.config` are working-directory-relative. Keep
  parsing defaults and sanitization backward compatible.

Plan 17 settings also persist `ui_scale`, `chat_scale`, `chat_opacity`,
`subtitles`, `high_contrast`, `reduce_flashing`, `toggle_sprint`,
`toggle_sneak`, `camera_bobbing`, `damage_tilt`, and selected
`resource_packs` IDs. `ResourcePackManager` keeps pack bytes in bounded maps;
ZIPs are never extracted to disk and unsafe paths, symlinks, oversized entries,
compression bombs, missing dependencies, and cycles are rejected.

## Overworld terrain & biomes (Plan 09)

The terrain generator uses continuous 2D climate noise and 3D density sampling encapsulated in `src/worldgen/`:

- `climate.rs`: `ClimateSystem` samples `temperature`, `humidity`, `continentalness`, `erosion`, and `weirdness` to continuously select 16 Overworld biomes (`Plains`, `Forest`, `BirchForest`, `Taiga`, `SnowyPlains`, `Desert`, `Savanna`, `Swamp`, `Jungle`, `Badlands`, `Meadow`, `WindsweptHills`, `River`, `Beach`, `Ocean`, `DeepOcean`). `WeatherSystem` shares this `ClimateSystem` for unified rain/snow precipitation queries.
- `density.rs` & `carver.rs`: 3D density fields combine continental landmass, erosion, ridges, and cave carvers (`cheese/cavern`, `tunnel`, `ravine`) with deep lava lake thresholds (`y <= 0`).
- `surface.rs` & `ore.rs`: Surface layers (top, filler, underwater) are data-driven per biome. Ores (`Coal`, `Iron`, `Gold`, `Redstone`, `Diamond`) distribute across negative Y Y-ranges using deterministic per-chunk vein algorithms.
- `feature.rs`: Tree and plant feature placer handles multi-chunk tree boundary placement and column flora.
- `world_tick.rs`: Evaluates natural block simulation (grass spread/decay, leaf decay based on log proximity, sapling growth, cactus/sugar cane growth, ice/snow melt, fire spread, falling sand/gravel step movement).

## Mob ecology, spawning & pets (Plan 11)

- `spawning.rs`: `MobCategory` caps (`Monster`: 70, `Creature`: 10, `Ambient`: 15, `WaterCreature`: 5 per player), natural spawn checks (biome, light <= 7 for monsters, surface, fluid, distance 24..128 blocks from player), difficulty rules (Peaceful despawns/prevents hostile), and despawn evaluation (>128 blocks instant despawn unless persistent; >32 blocks soft despawn after 30s).
- `ai/`: Priority-sorted `Brain` scheduler managing modular goals (`SwimGoal`, `SitGoal`, `FollowOwnerGoal`, `MeleeAttackGoal`, `WanderGoal`) and `BoundedPathfinder` with node evaluation caps to prevent A* frame spikes.
- Representative Mobs & Pets: `Spider` (climbing/leaping), `Slime` / `MagmaCube` (size-based splitting on death), `Witch` (potion drinking & splash potion throwing), `Drowned` (water/land toggle), `Ghast` (flying & fireball), `WitherSkeleton` (Wither effect), `Wolf` (tameable with bone, standing follow/sit toggle, attacks targets, dye collar), `Cat` (tameable with fish, Creeper repulsion within 8 blocks), `Horse` (tameable/rideable attributes), `Bat` (ambient cavern flight), `Squid` (water swimming).
- Persistence: `EntitySaveData` serialized with `#[serde(default)]` backward compatibility for `owner_id`, `owner_uuid`, `is_tamed`, `is_sitting`, `collar_color`, `slime_size`, and `is_persistent`.


## Verification

Behavioral tests are mostly inline `#[cfg(test)]` tests. The library target
now exposes the headless authority and protocol to multiplayer-core tests;
those tests do not construct a wgpu device/window/audio graph. The passive-mob
placeholder remains unrelated to the dedicated binary.

The performance plans 01–14 remain `Partial` until their fixed-scene GPU/window
and PGO artifacts exist; this status is not a claim that their runtime repair
work is absent. The authoritative status and outstanding artifact gates are
tracked in
[`performance/performance_track.md`](performance/performance_track.md) and
[`performance/15_performance_audit_repair_plan.md`](performance/15_performance_audit_repair_plan.md).
The host-authoritative model above remains the invariant; R4 repaired the
joining-client simulation/replication and pause/death-policy gaps.

Plan 14 headless verification covers the hopper smelting chain, unloaded-chunk
atomicity, sided capability/transaction validation, comparator revision wake-up,
observer edge/budget behavior, container revision ordering, and v3 save/snapshot
round trips. A GPU/window Host+Join-client scene still requires manual execution
outside the headless test environment.

Plan 17/19 adds `final_acceptance.rs`, a deterministic headless harness for real
foundation, progression, and social/automation workflows, including inventory,
block-entity and save/reload assertions. Singleplayer runs in CI. A separate
Plan18 harness now runs a real dedicated authority with two TCP clients for
block/container/revision/interest/reconnect coverage; the complete three-scenario
listen/dedicated rows remain explicit Plan18 hand-offs. Resource
packs, localization, subtitles, reduced motion, and keyboard-focus behavior
have unit coverage. Visual 4:3/16:9/21:9/high-DPI, audio-device,
GPU-performance, 30-minute soak, and three-topology acceptance still require
the manual steps in `plans/minecraft_foundation_gap/17_qa_checklist.md`.
Plan21 Phase A additionally keeps simultaneous dimension worlds and per-session
interest/revision routing isolated in headless tests, with persistence and
topology/reconnect regression coverage. Its fishing/furnace and listen-State
follow-up phases remain unchecked.
Plan22 adds `tests/authority_gameplay_domains.rs`, a dedicated headless vector
for fishing fixed-tick cast/bite/reel idempotency, furnace/craft/enchant/anvil
transactions, explicit brew-ready take and reconnect cleanup, and combat
shield/knockback/death/keep-inventory/respawn behavior. The vector and the
authority domain suites pass; GPU/window, transport-topology, and 30-minute
soak artifacts remain outside this plan.
Plan23 completes the State/NetworkClient authority cutover for Join Client
inputs: each mutating input emits one typed `GameplayRequest`, while embedded
and socket roots share the ACK/session/world/entity/container projection lane.
Session snapshots are gated by dimension, sequence, and revision; a dimension
transfer clears presentation caches before ordered ChunkData/entity deltas
repopulate them. The listen + two-client headless vector verifies request
delivery, interest isolation, duplicate/stale rejection, and owner-private
session payloads. Station-specific craft/enchant/anvil progress projection and
GPU/window artifacts remain explicit follow-up boundaries; Plan24 records the
automated topology, transport-metrics, and dedicated-headless-soak closures.

Plan24 closes the automated portions of those follow-ups. The shared
`ServerRuntime` fixed-tick/request/ACK/snapshot vector now runs the fishing,
workstation, brew, combat/death/respawn, stale/duplicate, dimension-transfer,
and reconnect cases in Singleplayer, ListenServer, and Dedicated topologies;
owner-private session projections carry the workstation results and metadata.
Outbound network counters reserve publication before a frame write and roll
back on write failure, with all production writer paths using the same guard;
the TCP metrics test passed 50 isolated release runs. Debug/release suites and
checks are green, and the dated dedicated headless 1800-second soak ended with
zero queue depth, zero queue-full events, six saves, and no panic/error. The
raw soak log and command/metrics record live under
`plans/minecraft_foundation_gap/artifacts/`. GPU/window/audio/DPI and real
Host+Join visual evidence remain manual and are intentionally not claimed.

Plan25 makes `ServerDifficulty` a server-owned value parsed from
`server.properties` and passed through `AuthorityConfig` to every
`ServerWorld`, without changing the existing binary `level.dat` layout or
protocol version. The existing hostile AI lane consumes it deterministically:
Peaceful removes loaded hostiles on the next fixed tick, while Easy/Normal/Hard
use bounded `0.9/1.0/1.1` chase multipliers. `do_mob_spawning=false` remains a
spawn gate and does not freeze already-loaded hostiles; PvP remains an
independent `WorldRules` setting. Tests cover strict fail-before-world config,
server.properties save/reload, checksum/policy observability, and embedded vs
dedicated parity. There is no autonomous spawn-table, vanilla damage, hunger,
GPU, audio, or visual implementation claim in this plan.

Plan26 adds the remaining container lifecycle and chest feedback contract. The
pre-review baseline debug library suite passed 665 tests (3 ignored), the
release library suite passed 666 (3 ignored), and the complete pre-review
`cargo test --release` lanes passed, including the 797-test binary lane plus
all server/integration/doc-test lanes. Review-fix narrow gates then passed
`container_sessions` (9), chest/forced-viewer `server_world` tests (3),
`server_runtime::tests` (14), `headless_server_authority` (1), and
`runtime_topology_parity` (5); `cargo check --release`, `cargo fmt --all
-- --check`, and `git diff --check` also passed. No direct `State`
GPU-constructor unit test is claimed; client, runtime, server, and headless
vectors cover the forced-close routing. Smooth lid interpolation, audio-device
and Host+Join visual evidence, and v17 same-key close epoch/reason/cursor
fields remain explicit follow-ups.

Plan27 adds a bounded slab-waterlogging vector. `fluid::tests` (6),
`chunk_manager::tests` (15), `block_model::tests` (5), and
`server_world::tests` (11) cover raw bit masks, v3 save carriers, fixed-tick
same-block mutations, cross-chunk source flow, mesh complement/invalidation,
and atomic world checksums. Protocol/client/server/runtime lanes pass 28/17/35/14
tests respectively; authority/persistence/headless/topology/waterlogging
integration lanes pass 3/3/1/5/5. The v17 packet carries raw fluid bytes and
rejects the prior handshake version, while `RevisionGate` preserves latest-wins
ordering for raw-fluid block/chunk projections. Debug/release/check and diff
gates all pass as recorded in Plan27; GPU/window/audio/DPI, full vanilla
waterlogging parity, and a 30-minute soak are explicitly outside this plan.

Plan28 closes the narrow authoritative Dispenser/Dropper lane without another
protocol bump: Plan27+28 finalize the same unpublished development v17
sequence, while the intermediate Plan27 `b77c38f` EntityStateWire shape is not
claimed binary-compatible. `RedstoneAction::Dispense` is drained in sorted
position/facing order on a rising edge only; source/front chunks and matching
block entities must be loaded before an atomic source/target/entity commit.
Dispenser behavior is deliberately limited to Arrow, SplashPotion, source-only
Water/Lava buckets, Flint and Steel fire placement, and metadata-preserving
ordinary DroppedItem fallback. Dropper insertion is merge-first/lowest-empty,
otherwise one DroppedItem is spawned. The authority allocator owns global
entity ids; `EntityStateWire.item` carries complete ItemStack metadata so
embedded, TCP, and three-topology projections converge. Redstone `last_powered`
and block/entity payloads retain their save/reload semantics, and powered reload
does not phantom-fire. Targeted evidence is 10 `authoritative_` unit tests,
2 bucket atomicity/flow tests, one latch roundtrip, one wire roundtrip, one real
TCP two-client projection, and one Singleplayer/ListenServer/Dedicated topology
projection; full vanilla dispenser behavior, cauldron/waterlogging integration,
hopper rewrites, GPU/window/audio, and manual visual acceptance remain outside
this plan. Final serial debug/release suites each pass 1,524 tests (684 library,
815 client binary, 2 server binary, 23 integration, zero doc-tests) with six
ignored benchmark/crash-child tests; `cargo check --all-targets` and
`cargo check --release --locked` pass. The serial test setting isolates an
existing process-local save-failure injection race; it does not change
production save behavior.

Use:

```text
cargo test
cargo test --release --lib server_runtime network::protocol
cargo run --bin icraft-server -- --once --world /tmp/icraft-world
cargo check --release
cargo run
```

`cargo run` requires a window/GPU and optionally an audio device; audio can
degrade to silent operation.


