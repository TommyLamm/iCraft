# Architecture

iCraft is a Rust voxel game: desktop client (`icraft`) plus headless server
(`icraft-server`). Shared simulation is a deterministic 20 Hz authority over
Tokio TCP. Source and tests are the contract; `plans/` is history.

## Where to put code

| Target | Entrypoint | Owns |
| --- | --- | --- |
| `icraft` | `src/main.rs` | winit/wgpu/rodio loop, menu, input, presentation. `--microbench` calls this crate's `mod microbench`. |
| `icraft-server` | `src/bin/icraft-server.rs` | Headless `ServerRuntime`, TCP, console, autosave/shutdown. |
| `icraft` lib | `src/lib.rs` | Shared authority, world, network, persistence. |

Desktop `main.rs` re-exports the library (`pub use icraft::{world, …}`) so
desktop files keep `crate::world` paths. Shared source compiles once.

**Do not add to `lib.rs`:** `menu`, `camera`, `audio`, `texture`,
`gpu_frame_resources`, `presentation_click`, `src/presentation/`. That would
compile wgpu/audio/click policy into `icraft-server`.
`presentation_inventory_policy` is the thin, GPU-free policy cut used by
server and tests.

`lib.rs` has two `pub` layers: the server/tests contract (authority, world,
network, save, `presentation_inventory_policy`, …) and extra `pub` modules so
the desktop crate can re-export them. `loot`, `voxel_shape`, `worldgen`,
`fluid`, `mob`, `rail`, and `world_tick` are `pub(crate)`. `recipes` stays
`pub` because desktop `State` and `ServerWorld` expose `RecipeManager`.
`sim_harness` / `final_acceptance` / `microbench` compile only under
`cfg(test)` or feature `harness`. Desktop `--microbench` is `src/main.rs`'s
own `mod`. Settings keys `dynamic_resolution` and `render_scale` were
removed; leftover lines in old `settings.txt` are ignored on load.
Leftover renderer-owned world simulation (`legacy_sim` /
`legacy_interaction` / `legacy_systems`) and feature `legacy_owner` are gone.

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
  caches, interpolation. It is not the world authority. Desktop `State` does
  not hold `RedstoneSystem`; live redstone ticks only in `ServerWorld`.
  Join/embedded block facing comes from `ChunkData` / `BlockChange` /
  block-entity projection, not client-side redstone restore.
- Singleplayer and listen-host use `ServerRuntime::new_embedded`. Local and
  socket input share one bounded FIFO.
- A join client never runs authority, worldgen, or `SaveManager`. It sends
  `GameplayRequest`s and applies revision-gated projections.
- Desktop `State` has no `SaveManager`. Embedded saves go through
  `ServerRuntime`; join clients persist nothing locally.
- Embedded presentation peeks `dimension.dat` so the first projected columns
  are not dropped. Player and terrain arrive from `ServerRuntime`.
  `State::new` does not generate spawn chunks, place a bonus chest, or collect
  dropped items / XP locally. Pickup is authority-only
  (`inventory_decision(Pickup)` is always `Reject`).
- `NetworkHandle` is `None` (embedded singleplayer / listen-host) or `Client`
  (join). Listen-host TCP is owned by `ServerRuntime`, not a GPU-thread server.
- `State::tick_authority_boundary` is the embedded 20 Hz tick. There is no
  separate `AuthorityBoundary` type.

`PresentationTopology` is `Embedded` or `JoinClient`, derived from role
(Join wins) plus in-process runtime. Non-join launches are Embedded.
There is no `LegacyOwner`, no `is_authoritative()`, and no
`AuthorityTopology`. Listen vs embedded is `TransportMode::{Disabled, Listen}`.

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

- `SessionContract` in `AuthorityCore` owns pose, dimension, and the accepted
  client sequence.
- `PlayerSessionState` in `ServerRuntime` owns interest, the save codec,
  `Instant` pose clocks, and teleport allowance.

Pose / dimension / gameplay overlays go only through
`write_pose`, `sync_pose_from_authority`, `sync_dimension`, and
`sync_gameplay_projection` in `src/server_runtime/session_sync.rs`.
`teleport_session` grants `teleport_allowance` before `write_pose`.
TCP ingress still rejects out-of-order sequences before they cross the host
channel (the network thread has no `AuthorityCore`); that watermark is not a
second accepted-sequence source.

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
- Place/break is `GameplayOperation::BlockAction`.
- Container open/close is `GameplayOperation::Container`. Clicks are
  `ContainerClick` only; leftover `Container { action: 1 }` is rejected.
  Clicks are conserving session transactions: clone player +
  container, verify the claimed cursor, apply brew/viewer locks, commit both
  sides or roll back. Client-supplied item data is never echoed as truth.
- Revisions are `(dimension, revision)`. The aggregate snapshot revision is
  a summary only.
- `AuthorityCore::tick` walks worlds by `BTreeMap` key. It does not
  `activate_dimension`. After the pass, `active_dimension` is restored to the
  value from the start of the tick. `activate_dimension` is request routing
  (submit, session dimension change, respawn).
- Session chat commands: `AuthorityCore::apply_command`. Dedicated console
  is a separate admin surface. `commands::Command::surface` marks
  `GameplayAllowed` vs `ConsoleOnly`; console-only strings and non-food
  `ItemUse` fail in `GameplayRequest::validate_bounds` before sequencing
  (desktop Help UI stays local via `commands::parse` / `help_text`).
- Per-session interest is both the projection boundary and the chunk
  materialization gate. Random ticks, fluids, hoppers, and furnaces walk the
  simulation union, not the unbounded residency map. Columns that leave every
  session's view/simulation sets (plus `interest::RESIDENCY_HYSTERESIS`, same
  Chebyshev ring as client unload) are flushed if dirty and evicted.
- Interest caches a column key
  `(dimension, chunk_x, chunk_z, view_distance, simulation_distance)`. When a
  session stays on that key, chunk HashSets are not rebuilt and
  `chunks_around` is not re-run. Entity interest skips both `query_radius`
  calls while that key and `EntityManager::spatial_revision` are unchanged;
  otherwise it diffs ids into the live sets without `mem::take`. Teleport
  (`write_pose` with refresh), dimension change, and view/simulation distance
  changes invalidate the anchor and force a full refresh.
- `ServerRuntime` keeps a reverse map
  `(dimension, ChunkCoord) → session_ids`, updated on chunk enter/depart and
  join/leave. Mutation / block-entity / container fanout looks up that map
  instead of scanning every player. Block-entity payloads are cloned only when
  at least one interested session exists (encode once per mutation, then
  fan out). Container slot updates still require an open viewer; non-viewers
  with only chunk interest never receive private inventory slots.
- `AuthorityCore` keeps a `BTreeMap<u8, Vec<PlayerId>>` session index, updated
  on register / remove / `set_session_dimension`. The four per-dimension tick
  phases look it up instead of filtering the full session table. Snapshot
  `session_updates` lists only sessions whose gameplay changed this tick
  (join, dimension change, and mining / brew / fishing / cooldown revision
  bumps). `last_snapshot` is replaced in place so the previous session vector
  is not cloned.
- Entity spawn/despawn follows view-distance interest. `EntityState` follows
  simulation-distance and is sent only when pose, health, or animation
  changed, or when the entity newly entered that session's simulation set.
  Stationary entities are not re-encoded every tick.
- Hopper `transfer_cooldown` countdown is memory-only. A column is marked dirty
  only when hopper slots change or cooldown is armed `0→N` after a transfer.
  Reload restores the last persisted cooldown (typically 8 after a transfer),
  so a hopper may wait up to 8 extra ticks. Furnaces are ticked from a compact
  per-chunk index with the same encoding as torches. Sleeping redstone skips
  comparator/observer refresh until a container mutation, plate occupancy
  change, scheduled/dirty work, or loaded-chunk set change wakes it. Grounded
  dropped items with near-zero velocity skip XYZ physics until the support
  block changes or an external push applies velocity. Living entities that are
  sitting, anchored, grounded (or flying with zero velocity), and not in
  hostile chase skip `update_physics` and `ai_phase` bumps; hostiles only write
  chase velocity when a player is within range and the desired speed differs.
  `tick_entities` syncs spatial buckets via a mover id list
  (`sync_entity_positions`), not a full-table `sync_positions` scan.
- `EmbeddedRuntimeBridge::sync_local_inventory` may write back only
  inventory, cursor, and selected hotbar. Health, hunger, XP, mining, and
  mounts stay server-owned. Join clients never use this path.
- Inventory UI topology is `PresentationTopology::inventory_decision`.
  Container slots send `ContainerClick`. Merchant offers submit `Trade` on
  both topologies (they are not `Workstation`, which would Reject). A UI
  hit is never itself an authoritative commit.

Leftover renderer-owned world simulation is gone. World mutation belongs in
`AuthorityCore` / `ServerWorld`. Desktop `State` has no `SaveManager` and no
presentation mutation index. Random ticks emit `world_tick::BlockMutationRequest` `{ pos, new_block, new_state }`,
which `ServerWorld` applies; durable writes stay on `ServerRuntime`.

## Tick vs frame

`ServerRuntime::tick_with_output` (50 ms):

1. Drain at most the bounded inbound budget.
2. `AuthorityCore::tick` every loaded dimension (session domains, mining,
   portals, then world time/redstone/hoppers/fluids/random ticks/furnaces/
   spawning/entities; then deferred dispenser/dropper actions).
3. Apply dimension transfers, route snapshots by interest, evict uninteresting
   columns, close invalid containers, update metrics, autosave every 6,000
   ticks (log errors; shutdown still `save_all`).
  `ServerWorld::checksum` is computed only by `AuthorityCore` after pending
  redstone dispense mutations are folded in; `ServerWorld::tick` leaves
  snapshot `checksum` at 0. The hash mixes a running XOR of block-revision
  fingerprints (updated on mutation/evict) plus a cached sorted-entity
  fingerprint. Idle ticks skip the resident-map scan and, when there is no
  entity spawn / despawn / pose / `ai_phase` change, reuse the prior entity
  fingerprint instead of sorting and re-hashing the full table.

Desktop: `App -> State::update(dt) -> State::render()`. Drain events, run
capped 20 Hz catch-up (listen-host keeps ticking in pause/death UI;
Singleplayer pauses), then interpolation / particles / streaming / one main
pass.

Terrain is derived only:

```text
ChunkManager -> 9-column halo snapshot -> Rayon mesh (the currently
  selected LOD; L1/L2 wait until first selected) -> identity check
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
(`world_y_to_section_y`, `section_and_local_y_to_world_y`,
`Chunk::world_y_range()`, `Dimension::height()`), not `CHUNK_HEIGHT` (256,
legacy dense constant) and not hard-coded `0..256`. Nether and End generation
allocate `height().section_count()` sections.

| Dimension | `WorldHeight` |
| --- | --- |
| Overworld | `-64 .. 320` |
| Nether | `0 .. 128` |
| End | `0 .. 256` |

Unloaded columns are not air: entity physics freezes for a tick if the
current or predicted AABB touches missing terrain.

Load lighting (`propagate_chunk_lighting`) seeds from the locked column and
up to eight neighbors (faces, emitters, and local darker neighbors) instead of
a per-voxel HashMap lookup. Section mesh halos copy from those same column
refs. Runtime meshing generates only the currently selected LOD; coarser
LODs are filled the first time the camera selects them.

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

Protocol v20: bincode over TCP, 4-byte big-endian length, 2 MiB cap.
`Packet::encode_payload` / `encode_frame` build the wire body. Outbound
queues hold `EncodedPacket` (`Arc<[u8]>` payload plus the logical `Packet`):
metering, mailbox replace, and `ConnectionWriter::send_payload` share one
encode. Broadcast fanout clones the `Arc` so N connections do not
re-serialize. Authenticated sessions speak a single `PROTOCOL_VERSION`
(handshake rejects others), so shared payload Arcs are never mixed across
protocol versions. Older versions fail handshake. Malformed pre-auth frames
close that connection only. Unknown or wrong-direction post-auth packets
close that connection; there is no decode-then-drop leftover path.

Player identity is `normalize_player_identity`: lowercase ASCII
`[a-z0-9_-]`, 1–16 bytes, no Windows reserved stems. `online-mode=true`
fails startup (credentials not implemented). Handshake never self-grants
operator.

`GameplayRequest` carries request id, client sequence, session, dimension,
revision, and a typed operation. The bounded response cache makes retries
idempotent. Live egress for sleep / container click / close is a
`GameplayRequest`. `Container` wire values are Open=`0` and Close=`2`;
leftover Click=`1` fails bounds validation. Live desktop send uses pose /
chat / disconnect / `GameplayRequest` / respawn. Server→client `BlockChange`
projection and server→client `ContainerClose` remain. Clients do not ACK
chunks; the join-client inbound queue is 1024 events so one presentation
tick can enqueue without ACK pacing. Deleting leftover inbound `Packet`
variants shifts later discriminants; handshake is protocol v20.

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
saved over (`ServerWorld::failed_restore_chunks`). `ServerRuntime` is the
sole `SaveManager` owner and the sole writer of `mutation_revisions.bin`.
Desktop `State` does not keep a second mutation index. Autosave and shutdown
flush only `dirty_chunks` (plus eviction of unkept dirty columns), batched
per region file so one region is rewritten once. `SaveManager` reuses
`region_cache` on write when the on-disk length still matches the last
observed snapshot; a truncated or corrupt region still fail-closes and
leaves `.bin.bak` semantics unchanged. Disk chunk streams use zlib level 1
(`Compression::fast`); the wrapper is unchanged so older level-6 payloads
still inflate. Historical save payloads still treat Y as `0..256` world Y
and must not be reinterpreted as signed-Y. Live `ChunkData` projection
sends uncompressed terrain streams instead of the disk `ChunkSaveData`
envelope. Desktop world paths go through `validated_world_path` (no
symlink escape from `saves/`).

## Code map

| Area | Files |
| --- | --- |
| Desktop loop | `src/main.rs`, `src/app.rs`, `src/menu.rs`, `src/state.rs`, `src/audio.rs` |
| Presentation (desktop-only) | `src/presentation/` — `embedded_runtime.rs`, `network_event.rs`, `frame.rs` are `#[path]` children of `state`. `gpu_frame_resources` / `presentation_click` / `microbench` are `mod` in `main.rs`. |
| Authority | `src/authority/` (`tick.rs`, `portals.rs`, `dispatch.rs`, `combat.rs`, `contract.rs`, `fishing.rs`, `interest.rs`, `mining.rs`, `transactions.rs`) |
| Runtime | `src/server_runtime.rs` plus `ingress.rs`, `projection.rs`, `session_sync.rs`; `src/server_world.rs`; `src/bin/icraft-server.rs` |
| World | `src/world/` (`block.rs`, `section.rs`, `chunk.rs`, `mesh.rs`), `src/chunk_manager.rs`, `src/dimension.rs`, `src/worldgen/`, `src/structure/` |
| Gameplay | `src/player.rs`, `src/physics.rs`, `src/inventory/`, `src/block_entity.rs`, `src/container_sessions.rs`, `src/redstone.rs`, `src/fluid.rs`, `src/world_tick.rs`, `src/entity.rs`, `src/mob.rs`, `src/passive_mob.rs` |
| Render | `src/chunk_schedule.rs`, `src/chunk_render.rs`, `src/culling/`, `src/block_model.rs`, `src/shader.wgsl` |
| Network | `src/network/` (`protocol.rs`, `transport.rs`, `server.rs`, `client.rs`, `ingress.rs`, `egress.rs`) |
| Save / assets | `src/save/` (`format.rs`, `region.rs`, `player.rs`, `index.rs`), `src/resources.rs` |
| Tests | inline `#[cfg(test)]`, `tests/` (`tests/common/tcp_harness.rs`), `src/sim_harness.rs` / `src/final_acceptance.rs` (harness/`cfg(test)` only) |
