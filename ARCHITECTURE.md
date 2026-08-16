# Architecture

> Last verified: 2026-08-16 at `tommy-dev`.
> Source code is authoritative; `plans/`, `docs/superpowers/`, and most of
> `plans/performance/` are design/history records, not a description of the live runtime.
>
> Review scope: Cargo targets, both entrypoints, desktop lifecycle, embedded and
> dedicated runtimes, authority/world ownership, TCP protocol, persistence,
> resource loading, rendering, background work, and integration-test topology.

## System overview

iCraft is a Rust voxel game with a desktop client and a headless dedicated server.
The desktop uses `winit`, `wgpu`, and `rodio`; shared simulation uses a deterministic
20 Hz authority; networking uses Tokio TCP; chunk generation and meshing use Rayon.

| Target | Entrypoint | Responsibility |
| --- | --- | --- |
| `icraft` | `src/main.rs` | Desktop menu/game, input, presentation, and optional embedded server. `--microbench` runs the built-in microbench instead. |
| `icraft-server` | `src/bin/icraft-server.rs` | Headless fixed-tick server, TCP sessions, console commands, metrics, and synchronous save/shutdown. |
| `icraft` library | `src/lib.rs` | Exposes shared authority, world, network, persistence, and test APIs to the server and integration tests. |

The desktop binary declares its module tree directly instead of importing the
library crate. Shared source files are therefore compiled once for the desktop
target and again through `src/lib.rs` for the server/tests. Presentation modules
(`menu`, `camera`, `texture`) stay desktop-only so `icraft-server` does not
compile the wgpu menu. `#[global_allocator]` is installed only in `src/main.rs`.

## Runtime topologies

```text
Singleplayer
  App -> State -> EmbeddedRuntimeBridge -> ServerRuntime (no socket)

Listen host
  App -> State -> EmbeddedRuntimeBridge -> ServerRuntime -> NetworkServer
                                                        ^
Join client                                             | TCP
  App -> State -> NetworkClient ------------------------+

Dedicated
  icraft-server -> ServerRuntime -> NetworkServer

All server-side paths -> AuthorityCore -> one ServerWorld per loaded dimension
```

- `App` owns the `Menu`/`State` transition, OS events, frame deadlines, resize,
  cursor state, and surface-error handling.
- `State` is the desktop composition root. It owns GPU/audio resources, input,
  camera, UI, chunk/render caches, client interpolation, and presentation effects.
- Singleplayer and listen-host use `ServerRuntime::new_embedded`; local input and
  socket input share the same bounded FIFO and authority path.
- A joining client never runs world authority. It sends input to the server and
  applies revision-gated projections to local render caches.
- `AuthorityBoundary` is a thin `AuthorityCore` helper retained for unit tests;
  it is not the current desktop Singleplayer/Host runtime path.

## Authority and ownership

| Owner | Canonical state |
| --- | --- |
| `ServerRuntime` | Transport/session lifecycle, player files, interest sets, tick scheduling, projection routing, saves, and metrics. |
| `AuthorityCore` | Authenticated sessions, request sequencing/idempotency, gameplay state, dimension routing, fixed-tick ordering, and global authority entity IDs. |
| `ServerWorld` | Chunks, blocks, raw fluid, block entities, entities, revisions, time, redstone, hoppers, fluids, random ticks, furnaces, spawning, and entity AI/physics. |
| `State` / renderer `ChunkManager` | Presentation copies only: streamed chunks, meshes, GPU allocations, particles, UI, interpolation, and feedback. |
| `SaveManager` | Durable level, player, chunk, entity, dimension, and mutation-revision data. |

The normal mutation flow is:

```text
input
  -> bounded runtime/network queue
  -> authenticated GameplayRequest
  -> session + sequence + dimension + revision + interest/reach/state validation
  -> atomic AuthorityCore/ServerWorld mutation
  -> dimension-scoped revision and cached GameplayResponse
  -> AuthoritySnapshot / targeted presentation event
  -> embedded State projection or TCP client projection
```

Important rules:

- Authority requests are validated before presentation state changes. Rejection
  must not consume inventory, create drops, or partially mutate the world.
- Revisions are dimension-scoped. Network and save gates use
  `(dimension, revision)`; the aggregate snapshot revision is only a summary.
- Sessions and dimensions use stable sorted iteration so topology and transport
  arrival order do not change fixed-tick checksums.
- Per-session interest is both the projection boundary and the chunk
  materialization gate. View, simulation, entity, and open-container interest
  are tracked separately and processed with bounded budgets. Columns that leave
  every session's view and simulation sets (plus the same hysteresis the client
  uses) are flushed if dirty and evicted; a dimension with no sessions may keep
  a capped spawn ring. Random ticks, fluids, hoppers, and furnaces walk the
  simulation union, not the unbounded residency map.
- `State::sync_authority_gameplay_from_local` is a narrow transition exception:
  embedded inventory UI may write back only inventory and selected hotbar slot.
  Health, hunger, XP, mining, mounts, and other authority-owned fields must stay
  server-owned. Joining clients never use this path.
- `State` still contains a large renderer-side legacy simulation path. Current
  Singleplayer and Host launches disable it by having an embedded runtime; new
  authoritative behavior belongs in `AuthorityCore`/`ServerWorld`, not that path.
- `world_mutation::apply_batch` is an atomic helper used by the legacy renderer
  path. It is not the primary headless authority mutation root.

## Tick, frame, and render flow

`ServerRuntime::tick_with_output` performs one 50 ms server step:

1. Drain at most the bounded inbound-event budget.
2. Tick `AuthorityCore` once for every loaded dimension.
3. Within each dimension, advance session domains, mining and portal travel,
   then `ServerWorld` time, redstone, hoppers, fluids, random ticks, furnaces,
   spawning, entities, and deferred dispenser/dropper actions.
4. Apply dimension transfers and container closures, route snapshots by
   interest, update metrics, and autosave every 6,000 ticks.

The desktop redraw path is `App -> State::update(dt) -> State::render()`:

1. Drain network/runtime events first.
2. Run fixed 20 Hz simulation with a capped catch-up accumulator. A listen host
   continues ticking while its pause/death UI is open; Singleplayer pauses.
3. Update frame-only interpolation, particles, UI, camera uniforms, continuous
   input, and bounded streaming/mesh integration.
4. Encode and submit one main render pass.

Terrain data moves through derived caches only:

```text
ChunkManager chunks
  -> 18^3 section halo snapshot
  -> Rayon 16^3 mesh job
  -> generation/lifetime/revision validation
  -> per-region GPU arena upload
  -> section visibility + LOD draw plan
  -> wgpu submission
```

GPU objects and submission stay on the main thread. The pass order is sky,
opaque/cutout terrain, entities, translucent terrain, particles, mining overlay,
first-person hand, colored/textured UI, crosshair, and text/lines.

On Windows, menu and game initialization force DX12 because the Vulkan path has
a known NVIDIA driver crash. The desktop compiles `dynamic_resolution` for
scale control; it currently still renders at native scale. Re-enabling a scaled
offscreen target requires an upscale pass and native UI. Lost/Outdated surfaces
resize from `window.inner_size()` (at least 1×1); Timeout skips present.

## World model

- A chunk is a 16x16 column of sparse 16-high `ChunkSection`s. Sections use
  paletted block/light storage and optional state/fluid arrays; block entities
  are stored inside their owning chunk.
- World heights come from `Dimension::height()`: Overworld `-64..320`, Nether
  `0..128`, and End `0..256`. Code must use the signed-Y helpers in `world.rs`
  rather than hard-coded `0..256` bounds.
- `dimension.rs` selects deterministic generation per dimension.
  `worldgen/` owns Overworld climate, density, surfaces, caves, ores, and
  features; `structure/` owns villages, strongholds, fortresses, End cities,
  dungeons, and mineshafts.
- Meshes, visibility, GPU allocations, network snapshots, particles, and UI are
  derived. Only authority chunks/entities/session state are canonical.

## Networking and concurrency

- Protocol v19 is bincode over TCP with a four-byte big-endian frame length and
  a 2 MiB packet cap. Older protocol versions are rejected at handshake.
- `GameplayRequest` carries request ID, client sequence, session, dimension,
  revision, and a typed operation. The bounded response cache makes retries
  idempotent; clients independently gate chunk, entity, session, block-entity,
  and container revisions.
- `NetworkServer` and `NetworkClient` each run a Tokio runtime on a background
  thread and communicate with the authority/presentation roots through bounded
  or metered channels. Pose traffic may be coalesced; reliable gameplay and
  lifecycle events are not silently replaced.
- Rayon workers generate/load chunks and mesh owned section snapshots. Results
  carry dimension, generation, lifetime, and revision identities and are
  discarded if stale.
- `save.rs` provides the desktop bounded latest-wins save worker with retryable
  failures. The active `ServerRuntime` authority performs its own autosave and
  synchronous shutdown flush through `SaveManager`.

## Persistence, settings, and assets

All default paths are relative to the process working directory. Desktop menu
worlds live under `saves/`; the dedicated server uses its configured `world_dir`
(default `world/`). In the table below, `<world>` means either location.

| Path | Contents |
| --- | --- |
| `settings.txt`, `controls.config` | Graphics/audio/accessibility/multiplayer preferences and key bindings. |
| `<world>/world.meta` | Human-readable world discovery and creation metadata (format v3). |
| `<world>/level.dat`, `player.dat`, `dimension.dat` | Level/rules, local player, and current dimension. |
| `<world>/players/<name>.dat` | Sanitized named player state for remote/dedicated sessions. |
| `<world>/regions/r.*.*.bin` | Overworld region files containing compressed chunk v3 payloads and block entities. |
| `<world>/dimensions/{nether,end}/` | Per-dimension regions and `entities.dat`; Overworld entities live at the world root. |
| `<world>/mutation_revisions.bin` | Latest dimension/chunk mutation revisions. |
| `server.properties` | Default dedicated-server config; effective server policy is also persisted inside the world directory. |
| `assets/`, `resourcepacks/` | Built-in pack plus optional validated directory/ZIP packs. |

Save payloads use serde/bincode and zlib-compressed chunk arrays. Writes use
atomic replacement; existing region files receive `.bin.bak` backups, and the
in-memory region cache is updated only after a successful replacement.

`ResourcePackManager` validates manifests, dependencies, paths, entry counts,
and byte budgets without extracting ZIPs. Selected packs layer textures, sounds,
models, bitmap fonts, and locale catalogs over the built-in pack. Missing assets
fall back to built-in or procedural data. `ICRAFT_RESOURCE_PACK` is only an
explicit development/test override.

## Code map

| Area | Primary files |
| --- | --- |
| Desktop lifecycle and UI | `src/main.rs`, `src/app.rs`, `src/menu.rs`, `src/state.rs` |
| Authority and dedicated runtime | `src/authority/`, `src/server_world.rs`, `src/server_runtime.rs`, `src/bin/icraft-server.rs` |
| World storage and generation | `src/world.rs`, `src/chunk_manager.rs`, `src/dimension.rs`, `src/worldgen/`, `src/structure/`, `src/loot.rs` |
| Gameplay systems | `src/player.rs`, `src/physics.rs`, `src/inventory.rs`, `src/recipes.rs`, `src/block_entity.rs`, `src/container_sessions.rs`, `src/redstone.rs`, `src/fluid.rs`, `src/world_tick.rs`, `src/entity.rs`, `src/mob.rs`, `src/passive_mob.rs`, `src/boss.rs`, `src/ai/` |
| Rendering | `src/chunk_schedule.rs`, `src/chunk_render.rs`, `src/culling.rs`, `src/block_model.rs`, `src/mob_renderer.rs`, `src/hand_renderer.rs`, `src/particles.rs`, `src/texture.rs`, `src/shader.wgsl` |
| Networking | `src/network/{protocol,transport,server,client}.rs` |
| Persistence and resources | `src/save.rs`, `src/resources.rs`, `src/localization.rs`, `src/audio.rs`, `src/accessibility.rs` |
| Tests and performance | inline `#[cfg(test)]`, `tests/`, `src/sim_harness.rs`, `src/final_acceptance.rs`, `src/microbench.rs`, `plans/performance/` |

`state.rs` is the largest coupling hotspot and mixes presentation with legacy
simulation. `server_runtime.rs` is the transport/session/save composition root.
Start changes at the narrow domain module, then verify the projection and save/
protocol boundaries rather than adding more cross-domain logic to either root.

## Verification

Most automated coverage is headless: inline unit tests plus integration tests
for persistence, authority parity, real TCP, block actions, travel, fishing,
waterlogging, and container conservation. Rendering, window, audio-device, DPI,
and fixed-scene GPU performance still require manual or artifact-based checks.

```text
cargo fmt --all -- --check
cargo check --all-targets
cargo test
cargo test --release
cargo run
cargo run --bin icraft-server -- --once --world <path>
```
