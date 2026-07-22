# Multiplayer Sub-task 4: Player State Synchronization

> **Parent task:** [plans/p3/25_multiplayer.md](../../../plans/p3/25_multiplayer.md) (任務 25 - 多人遊戲)
> **Sub-task:** 4 of 6 - keeps every connected player's avatar position/action in sync.
> **Depends on:** Sub-task 1 (Protocol), Sub-task 2 (Server), Sub-task 3 (Client Bridge). **Blocks:** Sub-task 6 (rendering consumes this).
>
> **For agentic workers:** Implement task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Synchronize each player's position, orientation, and simple actions across all clients. The host broadcasts its own authoritative position (as `PlayerId(0)`) and relays every client's position to the others. Clients send their local position each tick and render remote players from the server's `PlayerPosition` broadcasts. Remote avatars are stored as **interpolated** snapshots so movement looks smooth despite 20 Hz network updates and variable latency.

**Architecture:** Reuse the existing `EntityManager` in `src/entity.rs` to hold remote players as a new `EntityType::RemotePlayer` variant (kept separate from hostile/passive mobs so their AI is skipped). `State` gains a `remote_players: HashMap<PlayerId, RemotePlayerState>` map holding the last received snapshot plus interpolation buffers. `State::update` advances interpolation toward the latest snapshot; `State::render` delegates to `mob_renderer` for the actual cuboid avatar (the detailed model lives in Sub-task 6, but the data + position are owned here).

**Tech Stack:** Rust, `glam`, existing `entity.rs` / `mob_renderer.rs`.

**Key design decisions:**
- **20 Hz send, client-side interpolation:** each side sends its position at ~50 ms intervals (already throttled in Sub-task 3). Receivers interpolate between the two most recent snapshots with a ~100 ms artificial delay, smoothing jitter without large perceived lag.
- **No client-side prediction of remote players:** remote avatars move purely on interpolated server data. Only the local player is predicted (it already is - local physics runs immediately).
- **Reuse `EntityManager` + `mob_renderer`:** remote players piggyback on the existing entity render path (`mob_renderer::render_mobs` already draws arbitrary cuboids from `Entity` data), avoiding a new GPU pipeline. Their AI tick is skipped by branching on `EntityType::RemotePlayer` in `mob::update_mobs`.
- **Host is authoritative for player-vs-player effects:** actions (e.g. a remote player breaking a block) arrive as `PlayerAction`; the host validates and broadcasts the resulting `BlockChange` (see Sub-task 5). A client never applies another player's action locally except for cosmetic cues.

---

### Task 1: Add `RemotePlayer` Representation (`src/entity.rs`)

**Files:**
- Modify: `src/entity.rs`

- [ ] **Step 1: Add `EntityType::RemotePlayer`**
  Add the variant to the `EntityType` enum. Give it a human-sized AABB in `Entity::new` (e.g. `0.6 x 1.8 x 0.6`, matching `PlayerPhysics`).

- [ ] **Step 2: Add remote-player fields to `Entity`**
  Add `pub player_id: u64` (0 = none) and `pub username: String` (fixed-length `ArrayString` or a small heapless buffer to keep `Entity` movable; if a `String` is unavoidable, document the allocation). Default both for non-remote entities.

- [ ] **Step 3: Add a unit test**
  Verify an `Entity::new(EntityType::RemotePlayer, ...)` has the expected AABB and default fields.

- [ ] **Step 4: Verify it compiles**
  Run: `cargo check`

---

### Task 2: Maintain a Remote-Player Snapshot Map in `State` (`src/state.rs`)

**Files:**
- Modify: `src/state.rs`

- [ ] **Step 1: Define `RemotePlayerState`**
  ```rust
  struct RemotePlayerState {
      entity_index: usize,                 // index into EntityManager
      prev: PlayerSnapshot,                // previous received snapshot
      latest: PlayerSnapshot,              // most recent received snapshot
      latest_time: f32,                    // sim time of latest snapshot
      username: String,
  }
  struct PlayerSnapshot { x: f32, y: f32, z: f32, yaw: f32, pitch: f32 }
  ```
  Add `remote_players: std::collections::HashMap<PlayerId, RemotePlayerState>` to `State`.

- [ ] **Step 2: Handle join/leave from the network drain**
  In `State::drain_network_events` (added in Sub-task 3), on `PlayerJoin { id, username }`:
  - Spawn an `Entity` of type `RemotePlayer` in `EntityManager` with `player_id = id`, store its index and username into `remote_players`.
  - On `Host`: also broadcast `HostToServer::NotifyPlayerJoin` so all clients learn of the newcomer (and the newcomer learns of existing players via the same path).
  On `PlayerLeave { id }`: remove the `Entity` and drop the map entry. On `Host`: broadcast `HostToServer::NotifyPlayerLeave`.

- [ ] **Step 3: Record position snapshots**
  On `PlayerPosition { id, ... }` (inbound), update `remote_players[id]`: shift `latest` into `prev`, set `latest` from the packet, stamp `latest_time`. If `id` is unknown (race), lazily create the entry.

- [ ] **Step 4: Verify it compiles**
  Run: `cargo check`

---

### Task 3: Interpolate & Advance Remote Players (`src/state.rs`)

**Files:**
- Modify: `src/state.rs`

- [ ] **Step 1: Implement interpolation in `State::update`**
  After draining network events and before rendering, for each `RemotePlayerState` compute an interpolated pose at `sim_time - INTERP_DELAY` (e.g. 0.1 s) by `lerp`-ing between `prev` and `latest` based on `(target - latest_time + dt) / (latest_time - prev_time)`. Clamp to `[0,1]`; if only one snapshot exists yet, hold `latest`. Write the interpolated `pos`/`yaw`/`pitch` back into the `Entity`.

- [ ] **Step 2: Cap stale remote players**
  If no snapshot has arrived for a remote player within e.g. 10 s, freeze it in place (do not remove - removal comes from `PlayerLeave`/disconnect in Sub-task 6). This prevents a frozen avatar from drifting due to extrapolation.

- [ ] **Step 3: Add an interpolation unit test**
  Construct two snapshots 50 ms apart and assert the interpolated position at the midpoint equals the average, and that clamping holds at/beyond the endpoints.

- [ ] **Step 4: Verify it compiles**
  Run: `cargo test` (new interpolation test passes).

---

### Task 4: Send Local Position & Actions (`src/state.rs`)

**Files:**
- Modify: `src/state.rs`

- [ ] **Step 1: Confirm the 20 Hz position send**
  Sub-task 3 already sends local position each tick (throttled to ~50 ms). Extend it to also include `yaw`/`pitch` from `self.camera`/`self.player_physics`. For the host, use `PlayerId(0)`.

- [ ] **Step 2: Forward local actions**
  When the local player performs an action that should be visible to others (initial scope: block place/break, which is wired in Sub-task 5), also emit the corresponding `PlayerAction` outbound so other clients can play a cosmetic cue (e.g. arm swing). Keep the action set minimal for "basic" multiplayer.

- [ ] **Step 3: Skip remote-player AI**
  In the entity update dispatch (where `mob::update_mobs` / `passive_mob` ticks are called), skip any entity whose `EntityType == RemotePlayer`. Remote players are driven only by the interpolation above.

- [ ] **Step 4: Verify it compiles**
  Run: `cargo check --release`

---

### Task 5: Verification

- [ ] **Step 1: Full check**
  Run: `cargo fmt --check && cargo check --release && cargo test`

- [ ] **Step 2: Two-client position test**
  With Host + 2 Clients connected:
  - Move client A; confirm client B's `remote_players` map receives `PlayerPosition` updates for A's `PlayerId` within ~100 ms.
  - Confirm the interpolated `Entity` position updates smoothly (no teleport snapping under normal latency).
  - Confirm the host sees both clients move.

  (Visual confirmation of the avatar is validated in Sub-task 6; here we verify the data path and `Entity` pose.)

---

## Affected Files Summary

- **[MODIFY]** `src/entity.rs`
- **[MODIFY]** `src/state.rs`
- **[MODIFY]** `src/mob.rs` (skip `RemotePlayer` in AI tick)
- **[MODIFY]** `ARCHITECTURE.md`

## Verification Gate

Before Sub-task 6 (visual rendering of remote avatars + chat + disconnect UI):
- `remote_players` map is populated/cleared on join/leave.
- Interpolated `Entity` poses update each frame from server snapshots.
- Host relays positions between clients; no remote-player entity is run through mob AI.
