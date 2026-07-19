# Architecture

> Last verified: 2026-07-19. This is a navigation map, not a replacement for
> source code. Read it first, then inspect only the symbols named for the task.

## Project at a glance

`minecraft_clone` is a single-binary Rust desktop voxel game. It runs a `winit`
event loop, keeps the simulation on the main thread, and renders through `wgpu`.
Terrain, the texture atlas, and fallback sounds are generated procedurally.

There is currently no server, networking layer, database, or world save/load.
Only display/input/audio settings persist in `settings.txt`; world, player,
inventory, and entity state live in memory for the process lifetime.

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
     -> State (src/state.rs): owns the game, simulation systems, and GPU state
        -> update(dt): advances simulation and produces dirty CPU/GPU state
        -> render(): builds transient meshes/UI, records passes, presents frame
```

- `src/main.rs::main` declares every crate module and starts `EventLoop::run_app`.
- `src/app.rs::App` owns `Option<State>` and frame timing. `resumed` creates the
  window and calls `State::new`; `device_event` handles raw mouse motion;
  `window_event` routes keyboard, mouse, resize, pause, inventory, and redraw.
- On `RedrawRequested`, `App` caps `dt` at 0.1 seconds, calls `State::update`,
  requests the next redraw, then calls `State::render`.
- `src/state.rs::State` is the composition root and principal coupling hotspot.
  It owns the window/GPU resources, camera, chunks and mesh cache, player,
  inventory/crafting, entities, audio, UI state, and timers.

## Runtime data flows

### Startup

`main` -> `App::resumed` -> `State::new` -> load `GameSettings` -> initialize
wgpu pipelines/buffers -> build procedural texture atlas -> create spawn chunks
-> propagate initial lighting -> generate chunk meshes -> initialize gameplay,
entity, UI, and audio state.

### Per-frame update

`State::update` performs these major stages:

1. Tick water every 0.25 s and lava every 1.5 s; returned chunk coordinates mark
   cached meshes dirty. These ticks occur **before** the paused/dead early return.
2. Advance world time and translate `KeyState` into movement.
3. Run `PlayerPhysics::update`, then `State::update_chunks`.
4. Apply landing/fall/void/lava/hunger/drowning state and audio effects.
5. Spawn/update hostile mobs, then update/spawn passive mobs.
6. Sync camera and `CameraUniform`, upload the uniform, and advance continuous
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
both. Mutations on a chunk edge may require the adjacent chunk mesh as well.

`Chunk::generate_mesh` reads neighboring block, sky-light, block-light, fluid
level, and falling state through a closure supplied by `State`. It emits separate
opaque/cutout and translucent vertex/index sets.

### Rendering

`State::render` first generates mob mesh data and all immediate-mode UI vertices
on the CPU. The render pass order is: sky -> opaque/cutout chunks -> mobs ->
translucent chunks -> mining crack overlay -> textured UI -> colored UI ->
crosshair -> line/text UI -> present. The shader entrypoints and packed camera,
lighting, fog, time, underwater, and damage behavior are in `src/shader.wgsl`.

### Inventory and crafting

`App::window_event` selects the interaction mode (death, pause menu, inventory,
or world). `State::{open_inventory, handle_inventory_click, close_inventory}`
owns UI slot behavior. Data lives in `inventory::Inventory`; recipe definitions
and matching live in `crafting::RecipeManager`. World interactions are handled by
`State::{handle_click, break_block}` and `interaction::raycast`.

## Source routing table

### Runtime and rendering

| File | Responsibility / key symbols |
| --- | --- |
| `src/main.rs` | Crate module list and binary entrypoint `main`. |
| `src/app.rs` | `winit::ApplicationHandler`; OS events, key/mouse routing, redraw loop, resize and surface-error policy. |
| `src/state.rs` | `State`, `ChunkMesh`, `KeyState`, `SlotType`, `GameSettings`; GPU setup, frame ordering, UI, mining/placement, damage/respawn, chunk streaming. Start with the exact method, not the whole file. |
| `src/camera.rs` | `Camera`, `CameraUniform`, `WorldTime`; matrices, fog/sky uniform data, day/night clock and sky light. |
| `src/shader.wgsl` | Terrain/sky/UI shader entrypoints; lighting packing, fog, animated fluids, underwater and hurt effects. |
| `src/texture.rs` | `TextureAtlas::new_procedural` and all 16x16 tile/icon drawing. Writes `assets/texture_atlas.png`, then uploads it to the GPU. |
| `src/audio.rs` | `SoundId`, `SoundMaterial`, `AudioManager`; load/cache WAV files, synthesize missing sounds, 2D/approximate 3D playback. |

### World and simulation

| File | Responsibility / key symbols |
| --- | --- |
| `src/world.rs` | `BlockType`, `BlockProperties`, `RenderType`, `Chunk`; 16x256x16 storage, deterministic terrain/caves/ores, block metadata/atlas coordinates, heightmap, CPU mesh generation. |
| `src/chunk_manager.rs` | Loaded-chunk map, world/local coordinate conversion, block/light/fluid accessors, heightmap updates, deduplicated water/lava work queues. |
| `src/lighting.rs` | Cross-chunk BFS propagation/removal for sky and emissive block light; initial chunk lighting and post-mutation updates. |
| `src/fluid.rs` | Budgeted event-driven water/lava cells, falling/level propagation, draining, infinite water, and water/lava solidification. Returns dirty chunk coordinates. |
| `src/physics.rs` | `AABB`, `PlayerPhysics`; movement, gravity, jumping/swimming, axis collision resolution, fall-distance result. |
| `src/interaction.rs` | Grid DDA block `raycast` and `RaycastResult`; read-only world targeting. |

### Gameplay and entities

| File | Responsibility / key symbols |
| --- | --- |
| `src/inventory.rs` | `GameMode`, `Item`, tool/material metadata, `ItemStack`, `Inventory`; stacks, durability, hotbar/backpack/armor/craft slots, block-item mapping. |
| `src/crafting.rs` | `Recipe`, `RecipeManager`; shaped/shapeless recipe definitions and grid matching. |
| `src/player.rs` | `PlayerState`, `DamageSource`; health, hunger, saturation/exhaustion, regeneration, invulnerability, oxygen/drowning, death state. |
| `src/entity.rs` | `EntityType`, `Entity`, `EntityManager`; shared hostile/passive/projectile/particle data, AABBs, basic entity physics, IDs and spawn storage. |
| `src/mob.rs` | Hostile spawn/AI/combat, arrows, sunlight burning, creeper explosion and associated world/lighting/mesh mutations. |
| `src/passive_mob.rs` | Pig/cow/sheep/chicken wandering, cliff avoidance, breeding/young, drops and species-specific behavior. |
| `src/mob_renderer.rs` | CPU cuboid mesh construction for all entity types; output is uploaded and drawn by `State::render`. |

## Data and configuration

| Path | Role |
| --- | --- |
| `Cargo.toml` | Rust package and graphics/window/audio/noise dependencies. |
| `settings.txt` | Working-directory-relative `key:value` settings. Defaults and parser/writer are `state.rs::GameSettings`; settings are saved by pause-menu adjustments. |
| `assets/texture_atlas.png` | Generated diagnostic/runtime atlas output; `texture.rs` is the source of truth. |
| `assets/sounds/*.wav` | Loaded by `AudioManager`; missing files are synthesized and written at startup. |
| `plans/progress.md` | Feature roadmap/status. Useful for intent, not current runtime truth. |
| `docs/superpowers/{specs,plans}/` | Historical design and implementation notes by feature. Confirm behavior against source. |

## Tests and verification

Most behavioral tests are inline `#[cfg(test)]` unit tests beside their modules,
especially in `world.rs`, `lighting.rs`, `fluid.rs`, `physics.rs`,
`interaction.rs`, `inventory.rs`, `crafting.rs`, `player.rs`, `entity.rs`,
`mob.rs`, and `audio.rs`.

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
  and passive mob code can manipulate `state::ChunkMesh`. Changes to render data
  can therefore affect nominally simulation-only modules.
- Block changes have distributed follow-up work (lighting + dirty meshes). Search
  all callers of `ChunkManager::set_block` before changing mutation semantics.
- Chunk meshes and mob meshes are derived caches, not authoritative state. The
  authoritative world is `ChunkManager::chunks`; authoritative entities are in
  `EntityManager::entities`.
- Save/load is a planned feature (`plans/p2/16_save_load.md`), not an existing
  persistence boundary.
