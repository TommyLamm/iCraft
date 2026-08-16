# Architecture

> Last verified: 2026-08-16 at `4e552ed` (`tommy-dev`).
> Change range reviewed: the 20 commits after `b3912c6` through `4e552ed`
> (inclusive of the current `HEAD`).
> Source code is authoritative; `plans/`, `docs/superpowers/`, and most of
> `plans/03_performance/` are design/history records, not a description of the live runtime.
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
compile the wgpu menu. Small transport-independent presentation policies live in
`presentation_inventory_policy` so the server and headless tests can verify the
boundary without importing the UI. `chunk_render` and `perf` remain shared due
to world mesh types and queue metrics. `#[global_allocator]` is installed only
in `src/main.rs`; the dynamic-resolution controller is compiled only into the
desktop tree.

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
  (worlds live in a BTreeMap keyed by Dimension; active_dimension is a key)
```

- `App` owns the `Menu`/`State` transition, OS events, frame deadlines, resize,
  cursor state, and surface-error handling.
- `State` is the desktop composition root. It owns GPU/audio resources, input,
  camera, UI, chunk/render caches, client interpolation, and presentation effects.
- Singleplayer and listen-host use `ServerRuntime::new_embedded`; local input and
  socket input share the same bounded FIFO and authority path.
- A joining client never runs world authority. It sends input to the server and
  applies revision-gated projections to local render caches. It does not run
  local world generation or construct `SaveManager`, chunk-save, or network-
  snapshot workers; a missing/corrupt authoritative payload leaves the column
  absent.
- Embedded presentation starts without loading `player.dat` or pre-materializing
  a spawn halo, and does not construct a presentation `SaveManager`, chunk-save
  worker, or network-snapshot worker. It may peek `dimension.dat` so the first
  projected columns are not dropped. Player state and terrain arrive from
  `ServerRuntime` projections.
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
- `GameplayOperation::BlockUse` remains only as a compatibility envelope and is
  always rejected as `Unsupported`; it is never a raw `set_block` capability.
  Placement and mining enter through typed `BlockAction` validation. Legacy
  block-change adapters may reach the rejection path but cannot mutate terrain.
- A container click is a conserving session transaction. The authority clones
  the player's gameplay state and container slots, verifies that any claimed
  dragged stack matches the session cursor/hotbar, applies brew locks and viewer
  permissions, then commits both sides. A failed container write rolls back the
  session. Successful routing projects the authoritative slot, cursor, and
  `PlayerSessionUpdate`; client-supplied item data is never echoed as truth.
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
  `EmbeddedRuntimeBridge::sync_local_inventory` may write back only inventory,
  cursor, and selected hotbar slot. Health, hunger, XP, mining, mounts, and other
  authority-owned fields must stay server-owned. Joining clients never use this
  path, and embedded/container UI waits for authoritative projections instead of
  writing block entities, dropped entities, XP, farmland, or support breaks.
- Session lifecycle changes are commit ordered. Same-dimension gateway travel
  uses the portal-transfer path and teleport allowance; living players cannot
  respawn; portal ignition/Ender Eye insertion debit a cloned inventory before
  mutating blocks. Leave-save failure still releases the normalized identity.
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
4. Apply dimension transfers, route snapshots by interest, flush/evict columns
   outside the residency set, close invalid containers, update metrics, and
   autosave every 6,000 ticks. Autosave errors are logged rather than aborting
   the tick; shutdown still attempts its own synchronous `save_all` flush.

The desktop redraw path is `App -> State::update(dt) -> State::render()`:

1. Drain network/runtime events first.
2. Run fixed 20 Hz simulation with a capped catch-up accumulator. A listen host
   continues ticking while its pause/death UI is open; Singleplayer pauses.
3. Update frame-only interpolation, particles, UI, camera uniforms, continuous
   input, and bounded streaming/mesh integration. Load integration admits at
   most two payloads and 2 MiB inside the shared 3 ms terrain-result budget per
   frame; remaining results stay queued before lighting work begins.
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

The persistent section-mesh scheduler is latest-wins per section and caps its
pending queue at 16,384 entries, evicting the farthest pending section on
overflow. GPU objects and submission stay on the main thread. The pass order is
sky, opaque/cutout terrain, entities, translucent terrain, particles, mining
overlay, first-person hand, colored/textured UI, crosshair, and text/lines.

On Windows, menu and game initialization force DX12 because the Vulkan path has
a known NVIDIA driver crash. The desktop compiles `dynamic_resolution` for
scale control; it currently still renders at native scale. Re-enabling a scaled
offscreen target requires an upscale pass and native UI. Swapchains are created
at least 1×1; Lost/Outdated surfaces resize from `window.inner_size()` (also
clamped to 1×1), while Timeout skips the present without retry/log churn.

## World model

- A chunk is a 16x16 column of sparse 16-high `ChunkSection`s. Sections use
  paletted block/light storage and optional state/fluid arrays; block entities
  are stored inside their owning chunk.
- World heights come from `Dimension::height()`: Overworld `-64..320`, Nether
  `0..128`, and End `0..256`. Code must use the signed-Y helpers in `world.rs`
  rather than hard-coded `0..256` bounds.
- Collision, lighting, fluids, redstone sidecars, farmland hydration, passive
  spawning, camera range, and entity bounds all use that signed world height.
  Redstone persistence stores world Y as `i16` and migrates legacy `u8` values
  as the old `0..256` coordinate space. Entity physics freezes for a tick when
  its current or predicted AABB touches an unloaded column; missing terrain is
  not interpreted as air for collision/support.
- `dimension.rs` selects deterministic generation per dimension.
  `worldgen/` owns Overworld climate, density, surfaces, caves, ores, and
  features; `structure/` owns villages, strongholds, fortresses, End cities,
  dungeons, and mineshafts.
- Overworld generation maps configured ore world Y through the signed minimum,
  includes both chunk axes in placement hashes, places bedrock at `min_y`, and
  evaluates neighboring tree origins so canopies cross chunk boundaries.
  Structure-start caches are keyed by `(seed, dimension, region_x, region_z)`,
  and locate/placement share one `origin_y_for` policy.
- Meshes, visibility, GPU allocations, network snapshots, particles, and UI are
  derived. Only authority chunks/entities/session state are canonical.

## Networking and concurrency

- Protocol v19 is bincode over TCP with a four-byte big-endian frame length and
  a 2 MiB packet cap shared by transport and `Packet::decode`. The reader rejects
  an oversized length header before allocating a body; bounded byte/vector
  visitors reject claimed lengths beyond the remaining frame before reserving.
  Older protocol versions are rejected at handshake, and malformed pre-auth
  frames close only that connection.
- Login, whitelist, operator, and `players/<identity>.dat` keys all use
  `normalize_player_identity`: lowercase ASCII `[a-z0-9_-]`, 1–16 bytes, excluding
  Windows reserved stems. Invalid names are rejected rather than rewritten onto
  another identity. `online-mode=true` fails configuration/startup because real
  credential verification is not implemented; offline/LAN names are accounts
  without passwords. A handshake never self-grants operator status.
- `GameplayRequest` carries request ID, client sequence, session, dimension,
  revision, and a typed operation. The bounded response cache makes retries
  idempotent; clients independently gate chunk, entity, session, block-entity,
  and container revisions.
- `NetworkServer` and `NetworkClient` each run a Tokio runtime on a background
  thread and communicate with the authority/presentation roots through bounded
  or metered channels. Pose, chat, and gameplay use separate default ingress
  budgets (20/s, 8/s, and 120/s); chat is rejected above 256 characters before
  host enqueue. A full host queue backpressures only the offending connection:
  pose may be dropped, chat rejected, and gameplay receives `QueueFull`.
- Reliable gameplay/lifecycle output and container-slot deltas are never
  silently replaced. A client that cannot accept them inside the 250 ms reliable
  enqueue window is evicted. The client-to-presentation event queue is bounded
  at 256 and disconnects after 16 sustained overflows; private session updates
  for another player are discarded before enqueue.
- Pre-authenticated sockets are capped at twice `max_players` and use a 5-second
  handshake timeout. Post-authentication idle timeout and per-client outbound
  queues are separate controls.
- Rayon workers generate/load chunks and mesh owned section snapshots. Results
  carry dimension, generation, lifetime, and revision identities and are
  discarded if stale.
- `save.rs` provides the leftover desktop bounded latest-wins save worker for
  `LegacyOwner` construction and unit tests. The active `ServerRuntime`
  authority performs its own autosave and synchronous shutdown flush through
  `SaveManager`. Embedded Singleplayer / listen-host presentation does not
  spawn that worker.

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

Chunk restore is fail-closed. Required or present optional streams with corrupt,
empty, oversized, or dimension-inconsistent data return an error. Decode occurs
into an empty column and is inserted only after full success. Failed coordinates
enter `ServerWorld::failed_restore_chunks`, are neither regenerated nor saved
over, and remain absent from projections; one bad column does not abort loading
the rest of a world. All save inflates use an expected-size `take(expected + 1)`
limit (with an 8 MiB sidecar ceiling) before allocating the restored payload.

While an in-process `ServerRuntime` owns a world, it is the sole writer of
`mutation_revisions.bin`; presentation catch-up neither persists a second index
nor falls back to its independent region cache. Desktop discovery, creation,
upgrade, and launch pass through `validated_world_path`, which rejects symlinks,
Windows reparse points, and canonical paths escaping `saves/`. Settings and
controls also use atomic replacement. `server.properties` validates MOTD as
1–256 bytes. Region `.bin.bak` files are compatibility backups, not a rotating
last-known-good history.

`ResourcePackManager` validates manifests, dependencies, paths, entry counts,
and byte budgets without extracting ZIPs. Selected packs layer textures, sounds,
models, bitmap fonts, and locale catalogs over the built-in pack. Missing assets
fall back to built-in or procedural data. `ICRAFT_RESOURCE_PACK` is only an
explicit development/test override.

## Code map

| Area | Primary files |
| --- | --- |
| Desktop lifecycle and UI | `src/main.rs`, `src/app.rs`, `src/menu.rs`, `src/state.rs`, `src/presentation_inventory_policy.rs` |
| Authority and dedicated runtime | `src/authority/`, `src/server_world.rs`, `src/server_runtime.rs`, `src/bin/icraft-server.rs` |
| World storage and generation | `src/world.rs`, `src/chunk_manager.rs`, `src/dimension.rs`, `src/worldgen/`, `src/structure/`, `src/loot.rs` |
| Gameplay systems | `src/player.rs`, `src/physics.rs`, `src/inventory.rs`, `src/recipes.rs`, `src/block_entity.rs`, `src/container_sessions.rs`, `src/redstone.rs`, `src/fluid.rs`, `src/world_tick.rs`, `src/entity.rs`, `src/mob.rs`, `src/passive_mob.rs`, `src/boss.rs`, `src/ai/` |
| Rendering | `src/chunk_schedule.rs`, `src/chunk_render.rs`, `src/culling.rs`, `src/block_model.rs`, `src/mob_renderer.rs`, `src/hand_renderer.rs`, `src/particles.rs`, `src/texture.rs`, `src/shader.wgsl` |
| Networking | `src/network/{protocol,transport,server,client}.rs` |
| Persistence and resources | `src/save.rs`, `src/resources.rs`, `src/localization.rs`, `src/audio.rs`, `src/accessibility.rs` |
| Tests and performance | inline `#[cfg(test)]`, `tests/`, `src/sim_harness.rs`, `src/final_acceptance.rs`, `src/microbench.rs`, `plans/03_performance/` |

`state.rs` is the largest coupling hotspot and mixes presentation with legacy
simulation. `server_runtime.rs` is the transport/session/save composition root.
Start changes at the narrow domain module, then verify the projection and save/
protocol boundaries rather than adding more cross-domain logic to either root.

## Verification

Most automated coverage is headless: inline unit tests plus integration tests
for persistence, authority parity, real TCP, block actions, travel, fishing,
waterlogging, container conservation, adversarial frames, ingress pressure,
session lifecycle, corrupt restore, chunk residency, and projection-only joins.
`tests/review_hardening_invariants.rs` additionally locks arrival-order checksum
stability, invalid-dimension non-mutation, dimension-local revisions, stale-place
conservation, and join projection behavior. `final_acceptance` / `sim_harness`
are explicitly recipe/physics smoke over `ChunkManager`, not an authority closed
loop; Listen/Dedicated TCP is covered by Plan30–34 and the review-hardening
suites. Rendering, window, audio-device, DPI, Host+Join visuals, long soak, and
fixed-scene GPU performance still require manual or artifact-based checks.

```text
cargo fmt --all -- --check
cargo check --all-targets
cargo test
cargo test --release
cargo run
cargo run --bin icraft-server -- --once --world <path>
```
