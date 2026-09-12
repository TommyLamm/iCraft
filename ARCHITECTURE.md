# Architecture

iCraft is a Rust voxel game: desktop client (`icraft`) plus headless server
(`icraft-server`). Shared simulation is a deterministic 20 Hz authority over
Tokio TCP. Source and tests are the contract; `plans/` is history.

## Where to put code

| Target | Entrypoint | Owns |
| --- | --- | --- |
| `icraft` | `src/main.rs` | winit/wgpu/rodio loop, menu, input, presentation. `--microbench` is feature-gated (`microbench`). |
| `icraft-server` | `src/bin/icraft-server.rs` | Headless `ServerRuntime`, TCP, console, autosave/shutdown. |
| `icraft` lib | `src/lib.rs` | Shared authority, world, network, persistence. |

Desktop `main.rs` re-exports the library (`pub use icraft::{world, …}`) so
desktop files keep `crate::world` paths. Shared source compiles once.

**Do not add to `lib.rs`:** `menu`, `camera`, `audio`, `texture`,
`gpu_frame_resources`, `presentation_click`, `src/presentation/`,
`accessibility`, `localization`, `advancements`, `weather`, or desktop
section-visibility traversal (`presentation/visibility.rs`). That would
compile wgpu/UI/lang / presentation climate into `icraft-server`.
Lib `culling::is_los_blocked` stays shared for authority; the old desktop
`EntityLosManager` worker is gone.
`presentation_inventory_policy` is the thin, GPU-free policy cut used by
server and tests. Save keeps only `AdvancementProgressData` (unlock set);
the advancement tree UI is desktop-only. Presentation weather is a
`TimeSync.weather`-driven enum (hosts currently always send Clear) — not a
second `ClimateSystem` authority.

`lib.rs` has two `pub` layers: the server/tests contract (authority, world,
network, save, `presentation_inventory_policy`, …) and extra `pub` modules so
the desktop crate can re-export them. `loot`, `voxel_shape`, `worldgen`,
`fluid`, `mob`, and `world_tick` are `pub(crate)`. `rail` and presentation
shells (`vehicle`, `container_sessions`, POI/raid managers, map manager,
presentation `FishingManager`) are `cfg(test)` only — desktop `State` no
longer owns them; live container viewers and fishing hooks live on
`ServerWorld` / session overlay. `recipes` stays `pub` because desktop
`State` and `ServerWorld` expose `RecipeManager`.
Desktop `--microbench` is `src/main.rs`'s `mod microbench` behind feature
`microbench` (`cargo run --features microbench -- --microbench`); it is not
compiled into the library or `icraft-server`. Settings keys
`dynamic_resolution` and `render_scale` were removed; leftover lines in old
`settings.txt` are ignored on load. Leftover renderer-owned world simulation
(`legacy_sim` / `legacy_interaction` / `legacy_systems`) and feature
`legacy_owner` are gone. The old empty `harness` feature and recipe/physics
smoke modules `sim_harness` / `final_acceptance` are gone.

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
  Join/embedded block facing comes from projected columns / mutations /
  block-entity events, not client-side redstone restore. Embedded columns
  arrive as in-process `PresentationEvent::ChunkColumn(Arc<Chunk>)`; join
  clients still decode revision-gated `ChunkData`.
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
  (`inventory_decision(Pickup)` is always `Reject`). Presentation does not
  Q-drop ghost entities, scan void/lava/cactus for local damage, tick
  brew/effects/`world_time` on join, or worldgen on dimension switch —
  health/time/effects arrive from session projection; portal/respawn teardown
  is `reset_presented_dimension` only. F3 no longer hard-zeros absent
  desktop save-queue counters.
- `NetworkHandle` is `None` (embedded singleplayer / listen-host) or `Client`
  (join). Listen-host TCP is owned by `ServerRuntime`, not a GPU-thread server.
- `State::tick_authority_boundary` is the embedded 20 Hz tick. There is no
  separate `AuthorityBoundary` type.

`PresentationTopology` is `Embedded` or `JoinClient`, derived from role
(Join wins) plus in-process runtime. Non-join launches are Embedded.
There is no `LegacyOwner`, no `is_authoritative()`, and no
`AuthorityTopology`. Listen vs embedded is `TransportMode::{Disabled, Listen}`.
Chunk load policy is only `PresentationTopology::chunk_load_policy()`
(Join awaits `ChunkData`; Embedded may generate locally). Presentation never
mutates authority-owned world/container slots locally; `set_item_at_slot`
no-ops `ContainerSlot` and all join-client writes. Inventory click writeback
uses `resolve_inventory_hit` (shared probe) so Embedded player-inventory
`LocalMutate` and Join `Reject` stay on one hit path. `Pickup` remains an
inventory target that is always `Reject`; farmland-trample / unsupported-break
policy variants are gone.

## Ownership

| Owner | Canonical state |
| --- | --- |
| `ServerRuntime` | Transport, sessions, interest, tick schedule, projection, saves, metrics. |
| `AuthorityCore` | Authenticated sessions, request sequencing, gameplay, dimension routing, fixed-tick order, `AuthoritySnapshot`, global entity IDs. |
| `ServerWorld` | Chunks, blocks, fluids, block entities, entities, revisions, time, redstone, hoppers, random ticks, furnaces, spawning, AI/physics. `tick` returns a snapshot; `AuthorityCore` stores it. |
| `State` / renderer `ChunkManager` | Presentation copies: streamed chunks, meshes, GPU, particles, UI, interpolation. |
| `SaveManager` | Durable level, player, chunk, entity, dimension, mutation-revision data. |

Worlds live in a `BTreeMap` keyed by `Dimension`. Callers pass an explicit
`Dimension` (or `&mut ServerWorld`); there is no ambient active-world pointer.

One contract plus a runtime overlay stay separate because interest, Instant
pose clocks, and the save codec cannot enter the deterministic core:

- `SessionContract` in `AuthorityCore` owns username, pose, dimension,
  game mode, the accepted client sequence, and the single 128-deep
  `GameplayResponse` cache.
- `PlayerSessionState` in `ServerRuntime` is keyed by `PlayerId` and owns
  interest, the save codec (`PlayerData`), Instant pose clocks
  (`last_pose_position` / teleport allowance), and projection scratch.
  It does not mirror id / username / live pose / dimension / game_mode.

Pose / dimension / game mode / gameplay overlays go only through
`write_pose`, `sync_pose_from_authority`, `sync_dimension`, `sync_game_mode`,
and `sync_gameplay_projection` in `src/server_runtime/session_sync.rs`.
`write_pose` updates the contract (and pose clocks / interest); it does not
dual-write live pose into `PlayerData`. `sync_gameplay_projection` overlays
game_mode, gameplay, and pose onto `PlayerData` for projection/save.
`teleport_session` grants `teleport_allowance` before `write_pose`.

TCP ingress keeps rate-limit, in-flight dedupe, a completed-request-id set
(for forwarding retransmits to the authority cache), and a sequence watermark
filter. It does not store `GameplayResponse` bodies. Bounds / revision /
reach / spectator gates live in authority `preflight` (`dispatch.rs`).
Block / combat handlers use `SessionActionView` (`Copy`) instead of cloning
the full contract.

Float→milli pose conversion and the milli abs bound live in
`authority::contract` (`position_to_milli` / `POSITION_MILLI_ABS_LIMIT`).
Scalar health / hunger milli helpers (`scalar_to_milli`, `milli_to_scalar`,
`quantize_health`, `milli_to_vec3`) share that module — NaN policies differ
on purpose (`quantize_health` → `u32::MAX`, scalars → `0`).
Block↔chunk XZ helpers are `world::chunk_xz` / `local_xz` / `chunk_origin`.
Six-neighbor offsets come from `redstone::Direction::ALL` /
`Direction::all_deltas` (lighting / redstone). Shared FNV-1a and LCG live in
`rng`; bool flag parsing is `game_rules::parse_bool_flag`.
Block / interaction reach is `interaction::PLAYER_REACH` (still 8.0) with
`player_reach_squared()` for squared comparisons.

## Mutation path

```text
input
  -> bounded runtime/network queue
  -> TCP: rate-limit + in_flight + sequence watermark (no response cache)
  -> authenticated GameplayRequest
  -> authority preflight (bounds + sequence + dimension + revision +
     spectator + reach/state) then single response-cache lookup
  -> atomic AuthorityCore / ServerWorld mutation
  -> dimension-scoped revision + cached GameplayResponse (authority only)
  -> AuthoritySnapshot / targeted presentation event
  -> embedded State: `ChunkColumn(Arc<Chunk>)` + snapshot `WorldMutation`
     (TCP/join: `ChunkData` / `BlockChange`)
  -> presentation apply (no `ChunkManager::set_block` fluid side effects)
```

- Reject before mutating. A rejection must not consume inventory, spawn
  drops, or partially write the world.
- Place/break is `GameplayOperation::BlockAction`.
- Container open/close is `GameplayOperation::Container` with typed
  `ContainerAction` (Open / Close). Clicks are `ContainerClick` only.
  Clicks are conserving session transactions: clone player +
  container, verify the claimed cursor, apply brew/viewer locks, commit both
  sides or roll back. Client-supplied item data is never echoed as truth.
- Revisions are `(dimension, revision)`. The aggregate snapshot revision is
  a summary only.
- `AuthorityCore::tick` walks worlds by `BTreeMap` key. Each dimension uses an
  explicit `world_mut(dimension)` lookup; there is no tick-end restore of an
  active key. Request routing (`submit_request`, session dimension change,
  respawn) also takes an explicit `Dimension`.
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
  so a hopper may wait up to 8 extra ticks. Hoppers are discovered from a
  compact per-chunk `hopper_positions` index (same encoding as furnaces /
  torches), so zero-hopper simulation columns do not walk `block_entities`.
  Furnaces are ticked from a compact
  per-chunk index with the same encoding as torches. Random ticks sample from a
  per-chunk ascending `section_y` index (`random_tick_sections`) maintained on
  load / `set_block_local` / unload; authority walks the simulation-union
  columns and takes at most 128 already-ordered eligible sections without
  rescanning empty sections or sorting each tick. Sleeping redstone skips
  comparator/observer refresh until a container mutation, plate occupancy
  change, scheduled/dirty work, or loaded-chunk set change wakes it.
  `ChunkManager` carries a monotonic `load_generation` bumped on resident
  insert/remove; sleeping redstone compares that counter instead of probing
  every known chunk key. Awake redstone keeps comparator and transition-
  capable component indexes, runs transitions only for settle-evaluated /
  due positions, and collects persistent metadata from a per-column sidecar.
  Block mutation revisions are stored per-column so eviction is O(that
  column). Grounded
  dropped items with near-zero velocity skip XYZ physics until the support
  block changes or an external push applies velocity. Living entities that are
  sitting, anchored, grounded (or flying with zero velocity), and not in
  hostile chase skip `update_physics` and `ai_phase` bumps; hostiles only write
  chase velocity when a player is within range and the desired speed differs.
  Dimension boss / Enderman updates use nearest-player pose plus actual look
  direction and skip entirely when the dimension has no players.
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

1. Drain save-worker acks (clear dirty only after successful persist).
2. Schedule pending async worldgen and collect completed Rayon results.
3. Drain at most the bounded inbound budget.
4. Build per-dimension simulation unions from cached interest
   `simulation_chunks` (rebuilt only when a session's chunk anchor changes),
   then `AuthorityCore::tick` applies up to `MAX_INITIAL_CHUNK_PROJECTIONS_PER_TICK`
   completed columns (session-id / nearest-first), then every loaded dimension
   (session domains, mining, portals, then world time/redstone/hoppers/fluids/
   random ticks/furnaces/spawning/entities; then deferred dispenser/dropper
   actions). `ServerWorld::tick` takes the interest union as `&BTreeSet` and
   returns `Vec<WorldMutation>` (dispenser actions stay on the pending queue);
   plate occupants rebuild their column key only when a player crosses a chunk.
5. Apply dimension transfers, route snapshots by interest, evict uninteresting
   columns (residency keep-set is cached until anchors/distances change; when
   every resident is inside keep and `load_generation` is unchanged, eviction
   is an O(1) skip), close invalid containers, update metrics from the walked
   worlds (refreshed after eviction), enqueue autosave every 6,000 ticks
   (failures increment metrics; shutdown / console `save-all` still block on a
   save-worker barrier). Over-budget ticks only bump timing counters — no
   tick-thread `eprintln!`.
  `ServerWorld::checksum` is computed only by `AuthorityCore` after pending
  redstone dispense mutations are folded in. The hash mixes a running XOR of
  block-revision fingerprints (updated on mutation/evict) plus a cached
  sorted-entity fingerprint. Idle ticks skip the resident-map scan and, when
  there is no entity spawn / despawn / pose / `ai_phase` change, reuse the
  prior entity fingerprint instead of sorting and re-hashing the full table.
  Entity physics samples a 3×3 `ColumnNeighborhood` (same halo pattern as
  lighting/mesh) instead of per-voxel `chunks.get`. Movable entities run
  `update_physics` in parallel via Rayon over read-only chunk refs; collected
  `moved_ids` are sorted before `sync_entity_positions`. Redstone / fluid /
  random tick stay sequential. Authority entity ids trust the monotonic
  counter and only probe the target world plus the optional owner fishing
  hook (`debug_assert` keeps the old full scan).

Worldgen for interest projection is off the tick thread: `ensure_chunk` in
`WorldgenMode::Async` only registers demand; Rayon workers generate; results
carry `(dimension, generation, lifetime)` and are discarded when stale.
Gameplay mutations that need a missing column (`set_block`, fluid use, spawn
Y, spawn bootstrap) still call `materialize_chunk` synchronously.

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
least 1×1. `App` creates one `GpuContext` (`presentation/bootstrap.rs`:
device / queue / surface / config / present-mode policy) and menu↔game
transitions move it through `Menu::from_gpu` / `into_gpu_context` and
`State::new` / `into_gpu_context` — no second `request_adapter`. Menu UI
uses `shader.wgsl` `vs_ui` / `fs_ui` (no duplicate menu `UI_SHADER`).
Desktop menu lives under `src/menu/` (`mod.rs`, `widgets.rs` screen
tables, `controls.rs` binding table, `settings.rs`).

## World

A chunk is a 16×16 column of sparse 16-high paletted `ChunkSection`s. Block
entities live in the owning chunk. Use signed-Y helpers in `src/world/`
(`world_y_to_section_y`, `section_and_local_y_to_world_y`,
`Chunk::world_y_range()`, `Dimension::height()`), not hard-coded `0..256`.
`BlockType` is `#[repr(u8)]` with stable wire/save discriminants; static
gameplay/render fields live in `BLOCK_TABLE` (`src/world/block_table.rs`),
indexed by discriminant after `canonicalize()`. `BlockType::def()` /
`properties()` return `&'static` rows (no per-voxel struct rebuild).
Behavioral helpers (`can_stay_on`, `support_status_at`) stay as code.
`Item` static fields (name / stack / block / atlas / creative tab / tool /
armor / food) live in `ITEM_DEFS` (`src/inventory/item_table.rs`), indexed by
discriminant. `Item::def()` and the former six property match arms read that
table; `Item::from_block` builds a reverse map once (`OnceLock`) from
`ITEM_DEFS` plus a small set of non-1:1 overrides. Crafting / smelting recipes
are pattern tables in `src/recipes.rs` (wood-family expansion for planks /
sticks / table / chest); shaped lookup is keyed by `(width, height,
pattern[0][0])` and smelting by `HashMap<Item, _>`.

Open / powered / lit / extended / filled no longer use paired `BlockType`
variants. Bit 4 of `BlockState` (`is_open` / `BLOCK_STATE_OPEN_BIT`) carries
that flag for doors, trapdoors, lamps, furnaces, pistons, end-portal frames,
levers, buttons, plates, repeaters, and comparators. Redstone torches invert
the bit (set = extinguished) so legacy id 49 stays lit. The thirteen former
variant discriminants remain as `Reserved50`…`Reserved90` holes;
`from_wire` / `migrate_saved` map them to the base type plus the open bit.
State-aware helpers: `light_emission_for`, `face_tex_for`, `is_solid_for`,
`is_passable_for`.
Nether and End generation fill paletted `ChunkSection`s directly (no
full-column dense scratch). Overworld and Superflat do the same via
`set_block_local`; Superflat starts from `Chunk::empty_in_dimension` instead
of running full Overworld gen. Nether block light uses
`recompute_direct_column_lighting` plus `propagate_chunk_lighting`.
`WorldHeight` fields are private; callers use `min_y()` /
`max_y_exclusive()` / `section_count()`. Column save/network flatten walks
section storage in SoA order (byte-identical to the old per-voxel `get_*`
walk).

| Dimension | `WorldHeight` |
| --- | --- |
| Overworld | `-64 .. 320` |
| Nether | `0 .. 128` |
| End | `0 .. 256` |

Unloaded columns are not air: entity physics freezes for a tick if the
current or predicted AABB touches missing terrain.

Load lighting (`propagate_chunk_lighting`) seeds the center column from faces,
emitters, and lit cells that border darker neighbors, then seeds only the shared
faces of the four cardinal neighbors — no per-neighbor volume scan. BFS runs on a
temporarily taken 3×3 `&mut Chunk` neighborhood so each cell does zero `HashMap`
lookups. Section mesh halos still copy from the immutable `column_neighborhood`
refs. Runtime meshing generates only the currently selected LOD; coarser
LODs are filled the first time the camera selects them.

- `dimension.rs` picks generation per dimension.
- `worldgen/` owns climate, density, surfaces, caves, ores, features.
- `structure/` owns villages, strongholds, fortresses, End cities, dungeons,
  mineshafts. Structure-start caches are `(seed, dimension, region_x,
  region_z)`. Generators share `fill_box` / `hollow_box` / `place_loot_chest`
  / `finish_start`. The End pins one familiar city at
  `(END_CITY_X, END_CITY_BASE_Y, END_CITY_Z)` inside `StructureManager`
  (same Y as `origin_y_for(EndCity)`); there is no parallel
  `dimension::apply_fixed_end_city` path.
- Column fill samples surface/biome once per (x, z), then `block_at_sampled`
  per Y. Ambient spawn uses `ChunkManager::highest_solid_y`.

Join `ChunkData` that omits light streams zeros them then
`Chunk::recompute_direct_column_lighting`. Disk restore of a full
`ChunkSaveData` is fail-closed.

## Network

Protocol v21: bincode over TCP, 4-byte big-endian length, 2 MiB cap.
`Packet::encode_payload` / `encode_frame` build the wire body. Outbound
queues hold `EncodedPacket` (`Arc<[u8]>` payload plus the logical `Packet`):
metering, mailbox replace, and `ConnectionWriter::send_payload` share one
encode. Broadcast fanout clones the `Arc` so N connections do not
re-serialize. Authenticated sessions speak a single `PROTOCOL_VERSION`
(handshake rejects others), so shared payload Arcs are never mixed across
protocol versions. Older versions fail handshake. Malformed pre-auth frames
close that connection only. Unknown or wrong-direction post-auth packets
close that connection; there is no decode-then-drop leftover path.
`protocol_version` is carried only on `Handshake`, `LoginSuccess`, and
`ServerListPing*`; after handshake the connection holds the negotiated
version and other packets omit the field.

Server→client projection is one schema end-to-end for wire gameplay:
`ServerRuntime` builds a wire `Packet` once inside
`ProjectionEvent { dest, packet }` (`ProjectionDest::Session` /
`Broadcast`). Embedded presentation drains `PresentationEvent`: either
`Packet(ProjectionEvent)` (same shape as TCP) or
`ChunkColumn { Arc<Chunk>, revision, … }` for the local session — never
dense `ChunkData` streams or a second palette rebuild / full-column
lighting pass. TCP listen/dedicated wraps wire events as
`HostToServer::Project` only; `ChunkColumn` never leaves the process.
`HostToServer` keeps only control variants
(`DisconnectClient` / `DisconnectCatchupClient` / `Stop`). Egress classifies
mailbox delivery (catch-up / pose / state / reliable) from the `Packet`
variant — it is not a second payload enum. Embedded block deltas apply once
from snapshot `WorldMutation` (including `raw_fluid` + revision gate);
`BlockChange` packets are TCP/join only. Join-client inbound is the same
`Packet` after one protocol-version check: `ClientToGame` is only
`StatusUpdate` (local connection-progress text) or `Packet`; presentation
`NetworkInbound` is that thin type, and `NetworkStaging` / handlers classify
`Packet` variants directly (Plan 08).

Player identity is `normalize_player_identity`: lowercase ASCII
`[a-z0-9_-]`, 1–16 bytes, no Windows reserved stems. `online-mode=true`
fails startup (credentials not implemented). Handshake never self-grants
operator.

`GameplayRequest` carries request id, client sequence, session, dimension,
revision, and a typed operation. The bounded response cache makes retries
idempotent. Live egress for sleep / container click / close is a
`GameplayRequest`. `Container` uses typed `ContainerAction` (Open=`0`,
Close=`1`); unknown discriminants fail decode. Live desktop send uses pose /
chat / disconnect / `GameplayRequest` / respawn. Server→client `BlockChange`
projection remains for TCP/join; embedded applies the same cells from
snapshot `WorldMutation` only. Clients do not ACK
chunks; the join-client inbound queue is 1024 events so one presentation
tick can enqueue without ACK pacing. Deleting leftover inbound `Packet`
variants shifts later discriminants; handshake is protocol v21.

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
| `assets/`, `resourcepacks/` | Built-in pack plus optional directory/ZIP packs (`zip` crate; entries stored as `Arc<[u8]>`). |

Writes are atomic. Chunk restore is fail-closed: corrupt/empty/oversized/
dimension-inconsistent streams error; the column is never generated or
saved over (`ServerWorld::failed_restore_chunks`). `ServerRuntime` is the
sole `SaveManager` owner for loads and the sole writer of
`mutation_revisions.bin` (via the save worker). Desktop `State` does not keep
a second mutation index. Autosave and shutdown flush only `dirty_chunks`
(plus eviction of unkept dirty columns), batched per region file so one
region is rewritten once. Region write hits take the cache entry by move
(`remove` → mutate → reinsert) and trust a write-generation stamp instead of
re-statting with `fs::metadata`; a cold load of a truncated or corrupt region
still fail-closes and leaves `.bin.bak` semantics unchanged. Tick enqueues
`SavePayload`s (flattened chunk payloads, dirty entity dumps, sidecar groups);
zlib/region bincode/atomic write run on the save thread; dirty bits clear only
after ack. Sidecar batches share one `sync_all`. Players persist only when
their per-session dirty bit is set; entities skip rewrite while their checksum
epoch matches the last persisted watermark. Disk chunk streams use zlib level 1
(`Compression::fast`); the wrapper is unchanged so older level-6 payloads
still inflate. Historical save payloads still treat Y as `0..256` world Y
and must not be reinterpreted as signed-Y. Live `ChunkData` projection
sends uncompressed terrain streams to TCP/join clients instead of the disk
`ChunkSaveData` envelope. Embedded local sessions receive
`PresentationEvent::ChunkColumn(Arc<Chunk>)` and keep authority lighting.
Desktop world paths go through `validated_world_path` (no symlink escape
from `saves/`).

## Code map

| Area | Files |
| --- | --- |
| Desktop loop | `src/main.rs` (`mod accessibility` / `localization` / `advancements` / `weather`; `culling` facade), `src/app.rs`, `src/menu/` (widget screens + shared `GpuContext`), `src/state.rs`, `src/audio.rs` |
| Presentation (desktop-only) | `src/presentation/` — `embedded_runtime.rs`, `network_event.rs`, `frame.rs` are `#[path]` children of `state`. `visibility.rs` (section visibility BFS only; entity LOS worker removed) is loaded via `main.rs`. `gpu_frame_resources` / `presentation_click` are `mod` in `main.rs`; `microbench` is the same behind feature `microbench`. |
| Authority | `src/authority/` (`tick.rs`, `portals.rs`, `dispatch.rs`, `combat.rs`, `contract.rs`, `fishing.rs`, `interest.rs`, `mining.rs`, `transactions.rs`) |
| Runtime | `src/server_runtime.rs` plus `ingress.rs`, `projection.rs`, `session_sync.rs`; `src/server_world.rs`; `src/bin/icraft-server.rs` |
| World | `src/world/` (`block.rs`, `block_table.rs`, `section.rs`, `chunk.rs`, `mesh.rs`), `src/chunk_manager.rs`, `src/dimension.rs`, `src/worldgen/`, `src/structure/` |
| Gameplay | `src/player.rs`, `src/physics.rs`, `src/inventory/`, `src/block_entity.rs`, `src/redstone.rs`, `src/fluid.rs`, `src/world_tick.rs`, `src/entity.rs`, `src/mob.rs`, `src/passive_mob.rs`, `src/village/` (`VillagerProfession` / `TradeOffer`; POI/raid/merchant-session managers are `cfg(test)` only), `src/fishing.rs` (wire stages + authority helpers; presentation `FishingManager` is `cfg(test)` only) |
| Render | `src/chunk_schedule.rs`, `src/chunk_render.rs` (CPU mesh data; wgpu vertex layout lives next to desktop pipelines), `src/culling/` (`los` + `connectivity` in lib), `src/block_model.rs` (`emit_box` shared terrain box emitter; model paths derived from `BlockType` snake_case), desktop `src/mob_renderer.rs` + `src/mob_parts.rs` (table-driven `MobPart` + animator; dragon/wither/item specials), `src/hand_renderer.rs` (shares `UNIT_CUBOID_CORNERS`), `src/texture.rs` (`PACK_TILES` atlas definition with paint-on-miss), `src/shader.wgsl` |
| Network | `src/network/` (`protocol.rs`, `transport.rs`, `server.rs`, `client.rs`, `ingress.rs`, `egress.rs`; `loopback_test.rs` is `cfg(test)` only) |
| Save / assets | `src/save/` (`format.rs` includes `AdvancementProgressData`, `region.rs`, `player.rs`, `index.rs`), `src/resources.rs` |
| Tests | inline `#[cfg(test)]`, `tests/` (`tests/common/tcp_harness.rs`, `authority_harness.rs`) |
