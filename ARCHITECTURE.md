# Architecture

> Last verified: 2026-07-22. This is a navigation map, not a replacement for
> source code. Read it first, then inspect only the symbols named for the task.
>
> Git baseline: branch `master`, commit
> `f8037dd6a633afe63a58990312a659bbc181dec5` (`f8037dd`). This identifies the
> committed revision on which the verified working tree is based; it is not a
> self-reference to the commit that may later include this file.
>
> Maintenance rule: whenever this architecture map is updated, refresh the
> verification date, branch, and baseline commit together.

## Project at a glance

`iCraft` is a single-binary Rust desktop voxel game. It runs a `winit`
event loop, keeps the simulation on the main thread, and renders through `wgpu`.
Terrain, the texture atlas, and fallback sounds are generated procedurally.

There is currently no integrated server or database. A self-contained networking
foundation (versioned `Packet` protocol and async TCP `Connection` transport
under `src/network/`) exists but is not yet wired into the game loop.
Display/input/audio
settings persist in `settings.txt`, while each world's data (including seed,
metadata, game time, player status, inventory, current dimension, advancement
progress, and dimension-namespaced chunks) is stored under its own
`saves/<world>/` directory. Entity state, including bosses and projectiles, is
still transient in-memory.

## How agents should navigate

1. Use the routing tables below to select a module and exact symbol.
2. Query CodeGraph with those exact names and a small file cap (usually 2-5).
   For example: `main App`, `State::update State::render`, or
   `ChunkManager::set_block tick_fluids`.
3. Avoid repository-wide CodeGraph questions such as "explain the architecture".
   Large generic symbols in `state.rs` (`new`, `update`, `render`) can dominate
   the response with verbatim source while omitting other relevant modules.
4. Treat source returned by CodeGraph as already read. If it reports a stale
   file, read only that file directly. Read configs, docs, assets, and WGSL
   directly when graph indexing does not cover the needed detail.

## Entrypoints and ownership

```text
src/main.rs
  -> App (src/app.rs): winit lifecycle and input/event translation
     -> Menu (src/menu.rs): startup UI, settings, and world management
     -> State (src/state.rs): selected world, simulation systems, and GPU state
        -> update(dt): advances simulation and produces dirty CPU/GPU state
        -> render(): builds transient meshes/UI, records passes, presents frame
```

- `src/main.rs::main` declares every crate module and starts `EventLoop::run_app`.
- `src/app.rs::App` owns an optional `Menu` / `Game` runtime and frame timing.
  `resumed` creates the window and calls `Menu::new`. A `WorldLaunch` is queued
  from `window_event` and applied from `about_to_wait`, after the native input
  callback has returned, so a menu surface is never destroyed and replaced for
  the same window re-entrantly. On Windows both runtimes explicitly use wgpu's
  DX12 backend; other platforms use the normal primary native backends.
  `window_event` routes configurable keyboard input, mouse, resize, pause,
  inventory, the advancement screen, menu actions, and redraw. Input priority
  while playing is death screen -> pause menu -> advancements -> inventory ->
  world interaction.
- On `RedrawRequested`, `App` caps `dt` at 0.1 seconds, updates the active
  runtime, requests the next redraw, then renders its current surface.
- `src/state.rs::State` is the composition root and principal coupling hotspot.
  It owns the window/GPU resources, camera, chunks and mesh cache, player,
  inventory/crafting, enchanting/brewing workstations, potion effects,
  entities, lightweight particles, audio, UI state, timers, the weather state
  machine, advancement manager/GUI, and the 20 Hz redstone scheduler.

## Runtime data flows

### Startup

`main` -> `App::resumed` -> `Menu::new` first creates only the lightweight menu
surface and procedural rotating panorama. Selecting or creating a world yields a
`WorldLaunch`; `App` queues the transition and, from `about_to_wait`, drops the
menu surface before calling `State::new` with the selected directory, seed, mode,
difficulty, and current `GameSettings`. `State::new` initializes wgpu
pipelines/buffers (including the dedicated crack pipeline and particle buffers)
-> builds the texture atlas -> restores the saved dimension -> restores or creates
only the 3×3 spawn area from the selected world's seed -> propagates initial
lighting -> generates its chunk meshes -> initializes gameplay, entity, particle,
UI, advancement, and audio state. Existing `player.dat` data restores completed
advancement IDs along with player/inventory state; new and legacy saves start
with default progress. `State::update_chunks` streams the remainder of the
selected render distance after the first frame. Crack tiles prefer external
`destroy_stage_*.png` files and fall back to procedural generation when those
assets are unavailable.

### Per-frame update

`State::update` performs these major stages:

1. Accumulate debug/autosave timing, then tick water every 0.25 s and lava every
   1.5 s; returned chunk coordinates mark cached meshes dirty. Autosave and fluid
   ticks occur **before** the dead/paused early return.
2. Handle portal contact/dimension switches, then tick redstone at 20 Hz (up to
   four catch-up steps per frame), including pressure-plate occupancy, delayed
   updates, bounded signal settling, actuator mutations, TNT fuses, dispenser
   actions, and note sounds.
3. Tick brewing and active potion/Wither effects, advance advancement toasts and
   advancement-GUI dragging, update particle physics, and derive sprint/FOV state.
4. Advance world time and, in the Overworld only, weather. Weather emits
   roof-clipped rain/snow particles, maintains the rain loop, accumulates thin
   snow in cold biomes, and dispatches thunder strikes before translating
   `KeyState` into movement.
5. Run `PlayerPhysics::update`, then `State::update_chunks`; apply landing,
   fall, void, lava, cactus, hunger, oxygen, and audio effects. Emit footstep
   dust and periodic torch smoke, collect eligible dropped items, and run
   bounded leaf-decay random ticks.
6. Enforce difficulty rules, then ensure/update dimension-specific mobs and
   bosses through `boss.rs`. Resolve player-owned projectiles before ordinary
   hostile AI/projectiles and dropped-item physics; update passive mobs and
   spawn them only in the Overworld. Creative mode suppresses hostile targeting,
   Creeper ignition, boss attacks/movement, and boss-projectile player hits.
7. Sync camera and `CameraUniform`, upload the uniform, and advance continuous
   survival-mode mining through `interaction::raycast`.

`State::update_chunks` unloads out-of-range chunks, creates at most one missing
chunk per frame, propagates its boundary lighting, and rebuilds at most four
nearby dirty chunk meshes per frame.

### Block mutation and remeshing

```text
input / mob / fluid behavior
  -> ChunkManager::set_block
     -> Chunk.blocks + heightmap
     -> enqueue water/lava neighbor updates
  -> lighting::{update_*_after_placed, update_*_after_removed}
  -> mark affected ChunkMesh entries dirty (including boundary neighbors)
  -> State::update_chunks
  -> Chunk::generate_mesh(neighbor lookup closure)
  -> opaque/translucent wgpu buffers
  -> State::render
```

Important invariant: `ChunkManager::set_block` does **not** update lighting or
mark `State::chunk_meshes` dirty. Any non-fluid mutation path must explicitly do
both. Mutations on a chunk edge require the adjacent chunk mesh; mutations on a
chunk corner also require the diagonal mesh because vertex AO samples diagonal
blocks. `chunk_manager::mark_block_mesh_dependencies` is the shared source of
truth for this invalidation, while chunk load/unload invalidates all eight
surrounding loaded meshes.

Redstone is the other major mutation path. `RedstoneSystem` returns explicit
`BlockMutation` records plus side-effect actions instead of touching renderer,
audio, or entity state. `State::apply_redstone_update` uses those records to
update sky/block light and the shared mesh dependency set before executing
explosions, dispenser/dropper output, and note sounds. Player placement/removal
must call `RedstoneSystem::on_block_changed`; newly loaded chunks are scanned
once to recover saved circuits.

`Chunk::generate_mesh` reads neighboring block, sky-light, block-light, fluid
level, and falling state through a closure supplied by `State`. It emits separate
opaque/cutout and translucent vertex/index sets. Each visible face receives
four-level vertex ambient occlusion from three exterior neighbor samples per
corner; the darker diagonal distribution selects the quad triangulation to
avoid interpolation seams. Only solid opaque blocks cast AO, while every Chunk
surface type can receive it.

### Rendering

`State::render` first generates mob mesh data, camera-facing particle quads, and
all immediate-mode UI vertices (including advancement toasts/screen) on the CPU.
The render pass order is: sky ->
opaque/cutout chunks -> mobs (including dropped items) -> translucent chunks ->
alpha-blended particles -> multiply-blended mining crack overlay -> textured UI
-> colored UI -> crosshair -> line/text UI -> present. The shader entrypoints
and packed camera, lighting, fog, time, underwater, and damage behavior are in
`src/shader.wgsl`. Terrain `Vertex` data carries AO as a smooth location-3
attribute; packed sky/block/face lighting remains flat and the fragment shader
multiplies both contributions before hurt tint and fog.

### Particles and dropped items

Block breaks call `particles::spawn_block_debris` and, for eligible survival
drops, `State::spawn_dropped_item`. `ParticleSystem` is a bounded (4,096 entry),
transient CPU simulation: `State::update` advances position, gravity, age, and
expiry; emitter helpers assign small atlas UV sub-rects; `State::render` calls
`compile_mesh` to write billboard vertices/indices into preallocated dynamic GPU
buffers. Footstep dust reuses the block below the player, while torch smoke is
emitted by a periodic loaded-chunk scan and shrinks over its lifetime. Rain uses
vertically stretched water-textured billboards, snow uses drifting snow
billboards, and lightning chains short-lived fully lit stretched billboards.
Precipitation lifetime is capped at each loaded column's heightmap, so it stops
at terrain, foliage, and player-built roofs.

Dropped items deliberately use `EntityManager`, not `ParticleSystem`. They carry
an `inventory::Item`, use normal entity gravity/collision, skip hostile and
passive AI, and remain authoritative until collection. `mob_renderer::render_mobs`
draws each as a small atlas-textured cuboid with time-based yaw and vertical
bobbing. Both particles and dropped-item entities remain transient and are not
included in world saves.

### Combat projectiles and mob animation

Arrows are ordinary transient `Entity` records. `Entity::update_physics` applies
gravity and derives yaw/pitch from velocity. Skeleton AI in `mob::update_mobs`
keeps an 8-12 block firing distance, continuously aims at the player's upper
body, and emits hostile arrows on a two-second cooldown. `mob_renderer` uses
`target_player`, `pitch`, and `action_cooldown` to assemble the skeleton's arms,
hand-anchored 3D bow, drawn string, and nocked arrow from cuboids.

Right-clicking a bow spawns a friendly arrow from the camera. Survival consumes
an arrow unless Infinity is present; Power increases the stored projectile
damage. `State::update_player_projectiles` resolves friendly-arrow/entity hits
and splash-potion effects, while `mob::update_mobs` advances all arrows, removes
block impacts, and handles hostile-arrow/player hits. Dispensers also spawn
friendly arrows through the redstone action bridge. The same entity collection
and `mob_renderer::render_mobs` path therefore owns arrows from players,
skeletons, and dispensers.

### Weather

`weather.rs::WeatherSystem` owns a deterministic clear -> rain -> thunder ->
clear state machine. Each state lasts 12,000-24,000 world ticks. Its cached
temperature/moisture/ocean Perlin samplers classify precipitation: deserts are
dry, taiga and mountains receive snow, and other biomes receive rain. `State`
applies the resulting effects: weather brightness scales sky colors and global
sky light, `AudioManager` maintains an infinite rain loop, and thunder produces
a white UI flash, a world-space bolt, positional thunder, nearby player/entity
damage, burning, and an emissive fire block. Cold-biome accumulation places a
passable `SnowLayer`; mesh generation lowers only its top vertices to 1/8 block.
Weather timing itself is transient, while placed fire and snow are ordinary
chunk blocks and therefore persist through chunk saves.

### Inventory and crafting

`App::window_event` selects the interaction mode (death, pause menu, inventory,
or world). `State::{open_inventory, handle_inventory_click, close_inventory}`
owns UI slot behavior. Data lives in `inventory::Inventory`; recipe definitions
and matching live in `crafting::RecipeManager`. World interactions are handled by
`State::{handle_click, break_block}` and `interaction::raycast`.

### Advancements

`advancements.rs` defines a fixed 50-node tree split across Minecraft, Nether,
The End, Adventure, and Husbandry. `AdvancementManager::check_trigger` enforces
parent completion, records completed string IDs, queues three-second toasts, and
returns newly completed IDs so `State::trigger_advancement` can grant XP and play
the UI sound. Trigger variants exist for obtaining/crafting/eating items, mining,
kills, dimension entry, brewing, enchanting, breeding, and category roots.

Current runtime dispatch is narrower than the trigger model: `State` emits only
`MineBlock`, `CraftItem`, and `EnchantItem`. Adding another advancement event
requires an explicit `State::trigger_advancement` call at its authoritative
mutation path. `AdvancementProgressData` is serialized inside `player.dat`;
toast timers and `AdvancementGui` pan/zoom/category state are transient. The
fixed `L` key opens the screen, mouse drag pans, the wheel zooms, and Escape
closes it before affecting inventory or pause state.

### Enchanting, anvils, brewing, and effects

`inventory::ItemStack` keeps a fixed six-entry `EnchantmentSet`, optional
`PotionData`, and a fixed 24-byte custom name, so stacks remain `Copy` and still
fit the existing immediate-mode drag/drop UI. `enchantment.rs` owns option
generation and all stat helpers. An enchanting table scans a two-block ring for
up to 15 bookshelves, derives three deterministic offers, and charges experience
levels plus lapis in Survival. Anvils combine enchantments, repair equal tools,
and accept keyboard text for renaming.

`brewing.rs` owns potion recipes, ten effect variants, the 10-second brewing
state machine, and `EffectManager`. State applies effects to movement, melee,
regeneration/poison, night brightness, hostile targeting, lava damage, and
underwater oxygen. Splash potions are transient projectile entities and apply
within four blocks. Closing any workstation returns authoritative slot contents
to the player inventory; workstation progress and active effects are transient.

### Redstone

`redstone.rs` owns coordinate-indexed component metadata, 0-15 power,
weak/strong charge classification, facing, repeater delay, comparator mode,
note pitch, scheduled ticks, and loaded-chunk discovery. Dust loses one power per
wire hop; direct sources and directional repeater/comparator outputs can strongly
charge solid blocks. Torch inversion reads the strong charge of its support.

Each 20 Hz step applies due events, updates player/entity pressure plates,
iterates the component graph to a fixed point with a 64-pass safety cap, then
processes device transitions. Repeaters delay 1-4 ticks and restore power to 15;
buttons release after 20 ticks; primed TNT explodes after 80 ticks. Pistons move
one block, sticky pistons also pull one block, lamps change light emission,
powered doors/trapdoors become passable, dispensers fire arrows, droppers emit
items, and note blocks play one of 25 synthesized pitches. Dynamic metadata is
rebuilt when chunks load; block variants remain part of normal chunk saves.

### Dimensions, portals, and bosses

`dimension.rs::Dimension` is the dimension identity used by generation, chunk
loading, sky light, ambient light, portal scaling, and save paths. Overworld
chunks still use the legacy `saves/<world>/regions/` directory; Nether and End
chunks use `saves/<world>/dimensions/{nether,end}/regions/`. The active
dimension is persisted separately in `dimension.dat`.

`dimension::generate_chunk` dispatches to the Overworld generator, a deterministic
Nether generator with bedrock roof/floor, netherrack caves, lava sea, soul sand,
and glowstone, or an End generator with void islands and a reachable End City.
The Overworld generator embeds a compact stronghold room with twelve empty End
portal frames. Nether portal detection accepts either X/Z-oriented obsidian
frames; Overworld <-> Nether switches apply the vanilla 8:1 horizontal scale and
build a linked portal at the destination. Completed End portal frames switch the
player to a safe End spawn near the origin.

`boss.rs` owns dimension-specific hostile mobs and boss state transitions. It
spawns bounded Nether mobs, the Ender Dragon plus healing crystals, End City
shulkers, and Withers summoned from a soul-sand/skull T pattern. The module
returns `BossEvents` instead of mutating `State` directly; `State::apply_boss_events`
applies player damage, Wither effects, explosions, block placements, drops, and
dragon completion XP. Boss HUD data is pulled from `boss::active_boss_hud` during
UI construction. `GameMode` is passed into boss updates: Creative freezes the
player-seeking movement of Blaze/Piglin/Husk/Wither actors, keeps the Dragon in
its orbit phase, and suppresses player damage/projectile hits while preserving
entity physics and world-side lifecycle events.

## Source routing table

### Runtime and rendering

| File | Responsibility / key symbols |
| --- | --- |
| `src/main.rs` | Crate module list and binary entrypoint `main`. |
| `src/app.rs` | `winit::ApplicationHandler`; owns the `Menu` / `Game` runtime state machine, OS events, configurable key/mouse routing, redraw loop, resize and surface-error policy. |
| `src/menu.rs` | Main-menu renderer and UI state; procedural panorama, world discovery/create/delete metadata, `GameSettings`, key bindings, localization choices, and `WorldLaunch`. |
| `src/state.rs` | `State`, `ChunkMesh`, `KeyState`, `SlotType`; selected-world GPU setup, frame ordering, in-game/advancement UI, mining/placement, friendly projectiles, particle emitters, dropped-item collection, damage/respawn, and chunk streaming. Start with the exact method, not the whole file. |
| `src/camera.rs` | `Camera`, `CameraUniform`, `WorldTime`; matrices, fog/sky uniform data, day/night clock and sky light. |
| `src/shader.wgsl` | Terrain/sky/UI shader entrypoints; lighting packing, fog, animated fluids, underwater and hurt effects. |
| `src/texture.rs` | `TextureAtlas::new_procedural` and all 16x16 tile/icon drawing, including external-or-procedural 10-stage crack tiles and solid bow/string tiles. Writes `assets/texture_atlas.png`, then uploads it to the GPU. |
| `src/audio.rs` | `SoundId`, `SoundMaterial`, `AudioManager`; load/cache WAV files, synthesize missing sounds, 2D/approximate 3D playback. |
| `src/weather.rs` | `Weather`, `Precipitation`, `WeatherSystem`; timed transitions, biome precipitation, lightning/flash scheduling, and bounded effect budgets. |

### World and simulation

| File | Responsibility / key symbols |
| --- | --- |
| `src/world.rs` | `BlockType`, `BlockProperties`, `RenderType`, `Chunk`; 16x256x16 storage, deterministic terrain/caves/ores, block metadata/atlas coordinates, heightmap, CPU mesh generation. |
| `src/chunk_manager.rs` | Loaded-chunk map, world/local coordinate conversion, block/light/fluid accessors, heightmap updates, deduplicated water/lava work queues. |
| `src/dimension.rs` | `Dimension`, dimension-specific chunk generators, sky-light/ambient rules, Overworld-Nether coordinate scaling, Nether portal frame detection, End portal completion detection, and End exit fountain generation. |
| `src/lighting.rs` | Cross-chunk BFS propagation/removal for sky and emissive block light; initial chunk lighting and post-mutation updates. |
| `src/fluid.rs` | Budgeted event-driven water/lava cells, falling/level propagation, draining, infinite water, and water/lava solidification. Returns dirty chunk coordinates. |
| `src/redstone.rs` | 20 Hz redstone graph, 0-15 weak/strong power, component index, delayed ticks, comparator/repeater logic, actuator mutations, TNT/dispense/note actions, and loop protection. |
| `src/physics.rs` | `AABB`, `PlayerPhysics`; movement, gravity, jumping/swimming, axis collision resolution, fall-distance result. |
| `src/interaction.rs` | Grid DDA block `raycast` and `RaycastResult`; read-only world targeting. |
| `src/save.rs` | `LevelData`, `PlayerData`, `ChunkSaveData`, `SaveManager`; Bincode serialization of player/inventory/advancement data, Zlib chunk arrays, Region file management, legacy-player upgrade, and thread-based background saving. |

### Gameplay and entities

| File | Responsibility / key symbols |
| --- | --- |
| `src/inventory.rs` | `GameMode`, `Item`, tool/material metadata, `ItemStack`, `Inventory`; stacks, durability, hotbar/backpack/armor/craft slots, block-item mapping, and creative redstone components. |
| `src/crafting.rs` | `Recipe`, `RecipeManager`; shaped/shapeless recipe definitions and grid matching, including the redstone component crafting chain. |
| `src/enchantment.rs` | `Enchantment`, `EnchantmentSet`, `EnchantingState`, `AnvilState`; offer generation, compatibility, stat modifiers, repair/combine/rename rules. |
| `src/brewing.rs` | `PotionKind`, `PotionData`, `PotionEffect`, `EffectManager`, `BrewingStandState`; recipes, timed brewing and active-effect queries. |
| `src/player.rs` | `PlayerState`, `DamageSource`; health, hunger, saturation/exhaustion, regeneration, invulnerability, oxygen/drowning, death state. |
| `src/entity.rs` | `EntityType`, `Entity`, `EntityManager`; shared hostile/passive/projectile/particle/drop data, AABBs, gravity/flying physics, velocity-derived orientation, friendly-projectile metadata, IDs, and spawn storage. |
| `src/boss.rs` | Dimension mob population, Ender Dragon, Wither, End Crystal, Blaze/Piglin/Husk/Shulker behavior, Creative-mode attack suppression, boss deaths, drops, block-placement events, and Boss HUD summaries. |
| `src/mob.rs` | Hostile spawn/AI/combat, skeleton aiming/arrows, Creative-mode targeting suppression, sunlight burning, Creeper explosion and associated world/lighting/mesh mutations; advances dropped-item physics but skips hostile AI for them. |
| `src/passive_mob.rs` | Pig/cow/sheep/chicken wandering, cliff avoidance, breeding/young, drops and species-specific behavior. |
| `src/mob_renderer.rs` | CPU cuboid mesh construction for all entity types, including velocity-oriented arrows, the skeleton's hand-pivoted bow/draw animation, and rotating/bobbing dropped items; output is uploaded and drawn by `State::render`. |
| `src/particles.rs` | `Particle`, `ParticleSystem`, `MAX_PARTICLES`, emitter/atlas helpers; bounded particle physics and camera-facing billboard mesh compilation. |
| `src/advancements.rs` | `AdvancementProgressData`, `AdvancementManager`, `AdvancementTree`, `AdvancementGui`; 50-node/5-category tree, parent-gated trigger evaluation, persisted completion IDs, transient toasts, and interactive GUI state. Rendering and event dispatch remain in `State`. |

### Networking

| File | Responsibility / key symbols |
| --- | --- |
| `src/network/mod.rs` | Module root; re-exports `protocol` and `transport`. |
| `src/network/protocol.rs` | `PlayerId`, `PROTOCOL_VERSION`, `Action`, `Packet`; bincode `encode`/`decode` of the 11-variant versioned wire enum (each packet carries `protocol_version: u32`). No game-module dependencies. |
| `src/network/transport.rs` | `Connection`; async `tokio` TCP stream with 4-byte big-endian length-prefixed framing, a 2 MiB packet cap, and `recv`/`send`. Runtime-agnostic; not yet driven by the game loop. |

## Data and configuration

| Path | Role |
| --- | --- |
| `Cargo.toml` | Rust package and graphics/window/audio/noise/networking dependencies. |
| `settings.txt` | Working-directory-relative `key:value` settings. Defaults and parser/writer are `menu.rs::GameSettings`; includes display, three audio levels, difficulty, language, sensitivity, and key bindings. |
| `saves/<world>/world.meta` | Human-readable world-list metadata: display name, generation seed, game mode, difficulty, and last-played timestamp. Legacy `world_001` saves are inferred and upgraded when selected. |
| `saves/<world>/level.dat` | Bincode `LevelData`: authoritative world seed and game-time ticks. |
| `saves/<world>/player.dat` | Bincode `PlayerData`: transform, survival state, XP, game mode, inventory/item metadata, and advancement progress. Pre-enchanting player schemas are upgraded in memory with default newer fields. |
| `saves/<world>/dimension.dat` | One-byte active-dimension sidecar. Missing files default to Overworld for old saves. |
| `saves/<world>/regions/` | Legacy and current Overworld Region files. |
| `saves/<world>/dimensions/{nether,end}/regions/` | Nether and End Region files, using the same compressed chunk payload format as Overworld regions. |
| `assets/texture_atlas.png` | Generated diagnostic/runtime atlas output; `texture.rs` is the source of truth. |
| `assets/sounds/*.wav` | Loaded by `AudioManager`; missing files are synthesized and written at startup. |
| `plans/progress.md` | Feature roadmap/status. Useful for intent, not current runtime truth. |
| `docs/superpowers/{specs,plans}/` | Historical design and implementation notes by feature. Confirm behavior against source. |

## Tests and verification

Most behavioral tests are inline `#[cfg(test)]` unit tests beside their modules,
especially in `world.rs`, `lighting.rs`, `fluid.rs`, `physics.rs`,
`interaction.rs`, `inventory.rs`, `crafting.rs`, `enchantment.rs`, `brewing.rs`,
`player.rs`, `entity.rs`, `particles.rs`, `weather.rs`, `mob.rs`, `boss.rs`,
`dimension.rs`, `redstone.rs`, `advancements.rs`, `audio.rs`, and `menu.rs`.

`tests/passive_mob_tests.rs` is currently only a placeholder. Because the package
has no `src/lib.rs`, integration tests cannot directly import the internal
modules; add a library boundary before expecting meaningful external integration
coverage.

Use:

```text
cargo test
cargo check --release
cargo run
```

`cargo run` needs a graphics adapter/window and optionally an audio device; audio
initialization degrades to silent operation when no default output device exists.

## Known architectural hotspots

- `src/state.rs` mixes composition, simulation orchestration, GPU setup, UI layout,
  and interactions. Locate an exact method before reading it.
- Rendering types leak downward: `world.rs` imports `state::Vertex`, while hostile
  and passive mob code can manipulate `state::ChunkMesh`, and `particles.rs`
  imports `state::Vertex`. Changes to render data can therefore affect nominally
  simulation-only modules.
- Block changes have distributed follow-up work (lighting + dirty meshes). Search
  all callers of `ChunkManager::set_block` before changing mutation semantics.
- Advancement definitions do not subscribe to gameplay events automatically.
  Every new trigger producer must call `State::trigger_advancement` at the
  authoritative mutation path; otherwise the definition remains unreachable.
- Chunk meshes and mob meshes are derived caches, not authoritative state. The
  authoritative world is `ChunkManager::chunks`; authoritative entities are in
  `EntityManager::entities`. Particle vertices are also derived each frame from
  `ParticleSystem::particles`.
- On Windows, `Menu::new` and `State::new` intentionally force DX12 rather than
  the usual primary-backend selection. This avoids a verified NVIDIA Vulkan ICD
  crash (`nvoglv64.dll`) during the menu-to-world transition; do not switch this
  back to `PRIMARY` without testing the affected driver path.
- Save/load is managed by `SaveManager` in `src/save.rs` using Bincode and Zlib
  compression. `State` spawns a background thread listening on `SaveCommand` for
  non-blocking five-minute autosaves and chunk unloads, while window close and
  "Save and Quit" flush synchronously.
- Dimension switching rebuilds chunk, mesh, entity, particle, and redstone runtime state around the target dimension. Keep portal placement, chunk saves, and `dimension.dat` updates together when changing this flow.
