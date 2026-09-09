# Architecture

iCraft is a Rust voxel game: desktop client (`icraft`) plus headless server
(`icraft-server`). Shared simulation is a deterministic 20 Hz authority over
Tokio TCP. Source and tests are the contract; `plans/` is history.

## Where to put code

| Target | Entrypoint | Owns |
| --- | --- | --- |
| `icraft` | `src/main.rs` | winit/wgpu/rodio loop, menu, input, presentation. `--microbench` calls `icraft::microbench::run()`. |
| `icraft-server` | `src/bin/icraft-server.rs` | Headless `ServerRuntime`, TCP, console, autosave/shutdown. |
| `icraft` lib | `src/lib.rs` | Shared authority, world, network, persistence. |

Desktop `main.rs` re-exports the library (`pub use icraft::{world, …}`) so
desktop files keep `crate::world` paths. Shared source compiles once.

**Do not add to `lib.rs`:** `menu`, `camera`, `audio`, `texture`,
`src/presentation/`. That would compile wgpu/audio into `icraft-server`.
`presentation_inventory_policy` is the thin, GPU-free policy cut used by
server and tests.

`lib.rs` has two `pub` layers: the server/tests contract (authority, world,
network, save, `presentation_inventory_policy`, …) and extra `pub` modules so
the desktop crate can re-export them. `loot`, `voxel_shape`, and `worldgen`
are `pub(crate)`. `sim_harness` / `final_acceptance` compile only under
`cfg(test)` or feature `harness`. Leftover renderer-owned world simulation
(`legacy_sim` / `legacy_interaction` / `legacy_systems`) and feature
`legacy_owner` are gone.

New gameplay belongs in `AuthorityCore` / `ServerWorld`. Start in the narrow
domain module, then check projection, save, and protocol. Do not add
cross-domain logic to `state.rs` or `server_runtime.rs`.

## Runtimes

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

All server paths -> AuthorityCore -> BTreeMap<Dimension, ServerWorld>
```

- `App` owns the menu/game transition and the OS/window loop.
- `State` is the desktop composition root: GPU, input, camera, UI, render
  caches, interpolation. It is not the world authority.
- Singleplayer and listen-host use `ServerRuntime::new_embedded`. Local and
  socket input share one bounded FIFO.
- A join client never runs authority, worldgen, or `SaveManager`. It sends
  `GameplayRequest`s and applies revision-gated projections.
- Embedded presentation peeks `dimension.dat` so the first projected columns
  are not dropped. Player and terrain arrive from `ServerRuntime`.
- Production `NetworkHandle` is `None` (embedded) or `Client` (join).
  `NetworkHandle::Host` is leftover in-process transport. Listen-host TCP is
  owned by `ServerRuntime`, not a second GPU-thread server.
- `State::tick_authority_boundary` is the embedded 20 Hz tick. There is no
  separate `AuthorityBoundary` type.

`PresentationTopology` is `Embedded` or `JoinClient`, derived from role
(Join wins) plus in-process runtime. Non-join launches are Embedded.
There is no `LegacyOwner` and no `is_authoritative()`.

## Ownership

| Owner | Canonical state |
| --- | --- |
| `ServerRuntime` | Transport, sessions, interest, tick schedule, projection, saves, metrics. |
| `AuthorityCore` | Authenticated sessions, request sequencing, gameplay, dimension routing, fixed-tick order, `AuthoritySnapshot`, global entity IDs. |
| `ServerWorld` | Chunks, blocks, fluids, block entities, entities, revisions, time, redstone, hoppers, random ticks, furnaces, spawning, AI/physics. `tick` returns a snapshot; `AuthorityCore` stores it. |
| `State` / renderer `ChunkManager` | Presentation copies: streamed chunks, meshes, GPU, particles, UI, interpolation. |
| `SaveManager` | Durable level, player, chunk, entity, dimension, mutation-revision data. |

Worlds live in a `BTreeMap` keyed by `Dimension`. `active_dimension` is a
compatibility key, not a moved slot.

Two session records stay separate because interest and the save codec cannot
enter the deterministic core:

- `SessionContract` in `AuthorityCore`
- `PlayerSessionState` in `ServerRuntime`

Pose / dimension / gameplay overlays go only through
`write_pose`, `sync_pose_from_authority`, `sync_dimension`, and
`sync_gameplay_projection` in `src/server_runtime/session_sync.rs`.
`teleport_session` grants `teleport_allowance` before `write_pose`.

## Mutation path

```text
input
  -> bounded runtime/network queue
  -> authenticated GameplayRequest
  -> session + sequence + dimension + revision + interest/reach/state checks
  -> atomic AuthorityCore / ServerWorld mutation
  -> dimension-scoped revision + cached GameplayResponse
  -> AuthoritySnapshot / targeted presentation event
  -> embedded State projection or TCP client projection
```

- Reject before mutating. A rejection must not consume inventory, spawn
  drops, or partially write the world.
- Place/break is `GameplayOperation::BlockAction`. `BlockUse` is a leftover
  envelope and is always `Unsupported`.
- Container clicks are conserving session transactions: clone player +
  container, verify the claimed cursor, apply brew/viewer locks, commit both
  sides or roll back. Client-supplied item data is never echoed as truth.
- Revisions are `(dimension, revision)`. The aggregate snapshot revision is
  a summary only.
- `AuthorityCore::tick` walks worlds by `BTreeMap` key. It does not
  `activate_dimension`. After the pass, `active_dimension` is restored to the
  value from the start of the tick. `activate_dimension` is request routing
  (submit, session dimension change, respawn).
- Session chat commands: `AuthorityCore::apply_command`. Dedicated console
  is a separate admin surface.
- Per-session interest is both the projection boundary and the chunk
  materialization gate. Random ticks, fluids, hoppers, and furnaces walk the
  simulation union, not the unbounded residency map. Columns that leave every
  session's view/simulation sets (plus `interest::RESIDENCY_HYSTERESIS`, same
  Chebyshev ring as client unload) are flushed if dirty and evicted.
- `EmbeddedRuntimeBridge::sync_local_inventory` may write back only
  inventory, cursor, and selected hotbar. Health, hunger, XP, mining, and
  mounts stay server-owned. Join clients never use this path.
- Inventory UI topology is `PresentationTopology::inventory_decision`.
  Container slots send `ContainerClick`. Merchant offers submit `Trade` on
  both topologies (they are not `Workstation`, which would Reject). A UI
  hit is never itself an authoritative commit.

Leftover renderer-owned world simulation is gone. World mutation belongs in
`AuthorityCore` / `ServerWorld`. Presentation `SaveManager` and
`world_mutation::apply_batch` are leftover persistence helpers, not the
authority mutation root.

## Tick vs frame

`ServerRuntime::tick_with_output` (50 ms):

1. Drain at most the bounded inbound budget.
2. `AuthorityCore::tick` every loaded dimension (session domains, mining,
   portals, then world time/redstone/hoppers/fluids/random ticks/furnaces/
   spawning/entities; then deferred dispenser/dropper actions).
3. Apply dimension transfers, route snapshots by interest, evict uninteresting
   columns, close invalid containers, update metrics, autosave every 6,000
   ticks (log errors; shutdown still `save_all`).

Desktop: `App -> State::update(dt) -> State::render()`. Drain events, run
capped 20 Hz catch-up (listen-host keeps ticking in pause/death UI;
Singleplayer pauses), then interpolation / particles / streaming / one main
pass.

Terrain is derived only:

```text
ChunkManager -> section halo snapshot -> Rayon mesh -> identity check
  -> GPU region upload -> visibility + LOD -> wgpu
```

GPU objects stay on the main thread. Terrain vertex Y is relative to
`REGION_ORIGIN_Y` (`WorldHeight::OVERWORLD.min_y`, `-64`). Visibility uses
the current dimension `WorldHeight`. Client unload uses
`chunk_schedule::within_unload_hysteresis`.

Windows menu/game init forces DX12 (Vulkan NVIDIA crash). Swapchains are at
least 1×1.

## World

A chunk is a 16×16 column of sparse 16-high paletted `ChunkSection`s. Block
entities live in the owning chunk. Use signed-Y helpers in `src/world/`
(`world_y_to_section_y`, `Dimension::height()`), not `CHUNK_HEIGHT` (256,
legacy dense constant) and not hard-coded `0..256`.

| Dimension | `WorldHeight` |
| --- | --- |
| Overworld | `-64 .. 320` |
| Nether | `0 .. 128` |
| End | `0 .. 256` |

Unloaded columns are not air: entity physics freezes for a tick if the
current or predicted AABB touches missing terrain.

- `dimension.rs` picks generation per dimension.
- `worldgen/` owns climate, density, surfaces, caves, ores, features.
- `structure/` owns villages, strongholds, fortresses, End cities, dungeons,
  mineshafts. Structure-start caches are `(seed, dimension, region_x,
  region_z)`.
- Column fill samples surface/biome once per (x, z), then `block_at_sampled`
  per Y. Ambient spawn uses `ChunkManager::highest_solid_y`.

Join `ChunkData` that omits light streams zeros them then
`Chunk::recompute_direct_column_lighting`. Disk restore of a full
`ChunkSaveData` is fail-closed.

## Network

Protocol v19: bincode over TCP, 4-byte big-endian length, 2 MiB cap.
`Packet::encode_frame` is the length-prefix helper. Older versions fail
handshake. Malformed pre-auth frames close that connection only.

Player identity is `normalize_player_identity`: lowercase ASCII
`[a-z0-9_-]`, 1–16 bytes, no Windows reserved stems. `online-mode=true`
fails startup (credentials not implemented). Handshake never self-grants
operator.

`GameplayRequest` carries request id, client sequence, session, dimension,
revision, and a typed operation. The bounded response cache makes retries
idempotent. Live egress for sleep / container click / close is a
`GameplayRequest`. Leftover `Packet` / `GameToClient` variants exist for
inbound compatibility and tests.

`NetworkServer` / `NetworkClient` run Tokio on a background thread with
bounded/metered channels. Reliable gameplay/lifecycle output is never
silently replaced; a client that cannot accept it is evicted. Rayon
generate/mesh results carry dimension/generation/lifetime/revision and are
discarded if stale.

## Persistence

Default paths are relative to cwd. Desktop worlds: `saves/`. Dedicated:
configured `world_dir` (default `world/`).

| Path | Contents |
| --- | --- |
| `settings.txt`, `controls.config` | Preferences and key bindings. |
| `<world>/world.meta` | Discovery/creation metadata. |
| `<world>/level.dat`, `player.dat`, `dimension.dat` | Level, local player, current dimension. |
| `<world>/players/<name>.dat` | Named remote/dedicated players. |
| `<world>/regions/r.*.*.bin` | Overworld compressed chunk payloads. |
| `<world>/dimensions/{nether,end}/` | Per-dimension regions and `entities.dat`. |
| `<world>/mutation_revisions.bin` | Latest dimension/chunk revisions. |
| `server.properties` | Dedicated config; effective policy also lives in the world dir. |
| `assets/`, `resourcepacks/` | Built-in pack plus optional directory/ZIP packs. |

Writes are atomic. Chunk restore is fail-closed: corrupt/empty/oversized/
dimension-inconsistent streams error; the column is never generated or
saved over (`ServerWorld::failed_restore_chunks`). While `ServerRuntime`
owns a world it is the sole writer of `mutation_revisions.bin`. Desktop
world paths go through `validated_world_path` (no symlink escape from
`saves/`).

## Code map

| Area | Files |
| --- | --- |
| Desktop loop | `src/main.rs`, `src/app.rs`, `src/menu.rs`, `src/state.rs`, `src/audio.rs` |
| Presentation (desktop-only) | `src/presentation/` — `embedded_runtime.rs`, `network_event.rs`, `frame.rs` are `#[path]` children of `state`. |
| Authority | `src/authority/` (`tick.rs`, `portals.rs`, `dispatch.rs`, `combat.rs`, `contract.rs`, `fishing.rs`, `interest.rs`, `mining.rs`, `transactions.rs`) |
| Runtime | `src/server_runtime.rs` plus `ingress.rs`, `projection.rs`, `session_sync.rs`; `src/server_world.rs`; `src/bin/icraft-server.rs` |
| World | `src/world/` (`block.rs`, `section.rs`, `chunk.rs`, `mesh.rs`), `src/chunk_manager.rs`, `src/dimension.rs`, `src/worldgen/`, `src/structure/` |
| Gameplay | `src/player.rs`, `src/physics.rs`, `src/inventory/`, `src/block_entity.rs`, `src/container_sessions.rs`, `src/redstone.rs`, `src/fluid.rs`, `src/world_tick.rs`, `src/entity.rs`, `src/mob.rs`, `src/passive_mob.rs` |
| Render | `src/chunk_schedule.rs`, `src/chunk_render.rs`, `src/culling/`, `src/block_model.rs`, `src/shader.wgsl` |
| Network | `src/network/` (`protocol.rs`, `transport.rs`, `server.rs`, `client.rs`, `ingress.rs`, `egress.rs`) |
| Save / assets | `src/save/` (`format.rs`, `region.rs`, `player.rs`, `index.rs`), `src/resources.rs` |
| Tests | inline `#[cfg(test)]`, `tests/` (`tests/common/tcp_harness.rs`), `src/sim_harness.rs` / `src/final_acceptance.rs` (harness/`cfg(test)` only) |
