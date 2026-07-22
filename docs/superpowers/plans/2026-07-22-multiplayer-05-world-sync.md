# Multiplayer Sub-task 5: World (Block) Synchronization

> **Parent task:** [plans/p3/25_multiplayer.md](../../../plans/p3/25_multiplayer.md) (任務 25 - 多人遊戲)
> **Sub-task:** 5 of 6 - keeps the shared world's block mutations in sync.
> **Depends on:** Sub-task 1 (Protocol), Sub-task 2 (Server), Sub-task 3 (Client Bridge). **Blocks:** nothing strictly, but pairs with Sub-task 4/6 for the full experience.
>
> **For agentic workers:** Implement task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Synchronize all authoritative block mutations across host and clients. Because both sides generate terrain deterministically from the **shared seed** (sent in `LoginSuccess`), only **mutations** - player place/break, fluid flow results, redstone actuators, explosions, weather fire/snow - need to be transmitted. The host is the sole authority: it applies mutations locally (through the existing `ChunkManager::set_block` + lighting + mesh-invalidation + redstone path) and broadcasts each resulting `BlockChange`. Clients never mutate the world directly; they translate inbound `BlockChange` packets into the **same** apply path so lighting, meshes, and redstone stay consistent.

**Architecture:** Add a single `State::apply_remote_block_change(x, y, z, block)` that performs the full mutation sequence the architecture mandates: `ChunkManager::set_block`, then `lighting::update_*_after_placed/removed`, then `chunk_manager::mark_block_mesh_dependencies` dirtying `State::chunk_meshes`, then `RedstoneSystem::on_block_changed` (host-side redstone still runs only on the host). The host calls the existing local mutation helpers and additionally emits `HostToServer::BroadcastBlockChange`; clients call `apply_remote_block_change` from the network drain. A `ChunkData` path exists for catch-up when a client joins mid-game or a chunk's stored mutations are missing.

**Tech Stack:** Rust, existing `chunk_manager`, `lighting`, `redstone`, `fluid` modules.

**Key design decisions:**
- **Shared-seed deterministic generation + mutation-only sync:** avoids shipping entire chunk payloads every frame. `ChunkData` is reserved for join-time catch-up of chunks the host has mutated but the client hasn't loaded yet.
- **One canonical apply path:** both local (host) and remote (client) block changes funnel through the same lighting + mesh-dependency + redstone invalidation so the architecture's invariants (noted in `ARCHITECTURE.md` under "Block mutation and remeshing") are never violated by the network path.
- **Host-side derived mutations are broadcast too:** fluid flow, redstone actuators, explosions, and weather placements on the host call `set_block`; those calls are intercepted (see Task 2) so their results propagate to clients. This keeps clients as pure renderers of the host's world.
- **Clients suppress local simulation of fluids/redstone:** when role is `Client`, the fluid tick and redstone 20 Hz scheduler are skipped in `State::update`, because the host already ran them and will broadcast the resulting block changes. This prevents double-simulation divergence.

---

### Task 1: Add a `BlockType` <-> `u32` Conversion (`src/world.rs`)

**Files:**
- Modify: `src/world.rs`

- [x] **Step 1: Add discriminant conversion helpers**
  Add `BlockType::to_wire(&self) -> u32` and `BlockType::from_wire(u32) -> Option<BlockType>` based on the enum's `as u32` discriminant (ensure `BlockType` derives or has a stable numeric mapping). Document that adding a new `BlockType` must not reuse an existing wire value.

- [x] **Step 2: Add a roundtrip unit test**
  Assert every `BlockType` variant round-trips through `to_wire`/`from_wire`.

- [x] **Step 3: Verify it compiles**
  Run: `cargo test world`

---

### Task 2: Centralize Host Block Broadcast (`src/state.rs`, `src/chunk_manager.rs`)

**Files:**
- Modify: `src/state.rs`

- [x] **Step 1: Add `State::set_block_and_broadcast`**
  A host-side helper that wraps the existing authoritative mutation:
  1. Call `ChunkManager::set_block` (or the existing `State` mutation entry that already does set_block + lighting + mesh dirty + redstone).
  2. If `NetworkHandle` is `Host`, emit `HostToServer::BroadcastBlockChange { x, y, z, block: block.to_wire() }`.
  This becomes the single call site for player-driven and host-derived mutations that should be visible to clients.

- [x] **Step 2: Route player place/break through the broadcaster**
  In `State::break_block` / `State::handle_click` (placement), when `is_authoritative()`, call `set_block_and_broadcast` instead of the raw `set_block` path. (Client-side these already send `RequestBlockChange` per Sub-task 3 and do nothing locally.)

- [x] **Step 3: Route host-derived mutations through the broadcaster**
  Identify the host-side mutation call sites that produce visible block changes and ensure they go through `set_block_and_broadcast` (or emit `BroadcastBlockChange` directly if they use a batch path). Key sites from `ARCHITECTURE.md`:
  - `fluid.rs` returned dirty chunks already call `set_block`; ensure fluid-driven `set_block` on the host also broadcasts (or batch-broadcast at end of the fluid tick to limit packet volume).
  - `State::apply_redstone_update` actuator mutations (piston moves, lamp toggle, TNT explosion block damage, dispenser output).
  - `boss.rs` / `mob.rs` Creeper explosions and Wither skull block damage.
  - `weather.rs` fire placement and snow-layer accumulation.
  For high-frequency sources (fluids), consider coalescing changes per chunk per tick to avoid packet storms.

- [x] **Step 4: Verify it compiles**
  Run: `cargo check`

---

### Task 3: Implement Client-Side `apply_remote_block_change` (`src/state.rs`)

**Files:**
- Modify: `src/state.rs`

- [x] **Step 1: Implement `State::apply_remote_block_change`**
  ```rust
  fn apply_remote_block_change(&mut self, x: i32, y: i32, z: i32, block_wire: u32) {
      let block = match BlockType::from_wire(block_wire) { Some(b) => b, None => return };
      let prev = self.chunk_manager.get_block(x, y, z);
      self.chunk_manager.set_block(x, y, z, block);
      // Lighting: remove then place, matching the existing local path.
      if block == BlockType::Air {
          crate::lighting::update_after_removed(&mut self.chunk_manager, x, y, z, prev);
      } else {
          crate::lighting::update_after_placed(&mut self.chunk_manager, x, y, z, block);
      }
      // Mesh dependency invalidation (handles edge/corner neighbors).
      crate::chunk_manager::mark_block_mesh_dependencies(&mut self.chunk_meshes, x, y, z);
      // Do NOT run redstone on the client; the host runs it and broadcasts its effects.
  }
  ```
  Adjust the exact helper names/signatures to match the current source (`lighting` / `chunk_manager` APIs).

- [x] **Step 2: Wire it into the network drain**
  In `State::drain_network_events`, on `BlockChange { x, y, z, block }` (client) / `ServerToHost::ClientBlockChange` (host), call the appropriate path:
  - **Client:** `apply_remote_block_change(...)` directly.
  - **Host:** validate the requesting client's action is legal (basic sanity: position within loaded range, not obviously malformed), apply locally via `set_block_and_broadcast`, which re-broadcasts to all clients including the originator.

- [x] **Step 3: Add a unit test**
  Using a minimal `ChunkManager`/`State` harness (or the existing test pattern in `src/lighting.rs`), apply a `BlockChange { Air -> Stone }` and assert the block, a boundary mesh dirty flag, and lighting all update correctly.

- [x] **Step 4: Verify it compiles**
  Run: `cargo test`

---

### Task 4: Suppress Client-Side World Simulation (`src/state.rs`)

**Files:**
- Modify: `src/state.rs`

- [x] **Step 1: Gate fluid/redstone/weather world mutation on authority**
  In `State::update`, when `!is_authoritative()` (Client role):
  - Skip the fluid tick (water/lava) and its returned dirty-chunk handling.
  - Skip the 20 Hz redstone scheduler and `apply_redstone_update`.
  - Skip weather-driven block placement (fire/snow accumulation) and Creeper/boss explosion block damage (those arrive as `BlockChange` from the host instead).
  - Keep weather **visuals** (particles, sky darkening, audio) running locally since they're derived from the shared seed/time and don't mutate blocks on the client.
  - Keep local player physics, particles, and rendering fully active.

- [x] **Step 2: Reconcile time**
  Ensure world time stays in sync: the host's game-time ticks should drive the clients. Add a lightweight `TimeSync` packet or piggyback the current game tick on `Keepalive`/`PlayerPosition` so the client can correct drift. (For "basic" scope, piggyback on an existing packet to avoid a new variant; if a new variant is cleaner, add `Packet::TimeSync` to `protocol.rs`.)

- [x] **Step 3: Verify it compiles**
  Run: `cargo check --release`

---

### Task 5: Chunk Catch-Up on Join (`src/network/server.rs`, `src/state.rs`)

**Files:**
- Modify: `src/network/server.rs`
- Modify: `src/state.rs`

- [x] **Step 1: Send mutated chunks to a newly joined client**
  When a client joins, the host should send `ChunkData` for any loaded chunks whose stored state differs from freshly-generated terrain (i.e. chunks with player/derived mutations). For "basic" scope, implement a simple version: on `ClientJoined`, the host iterates currently-loaded chunks and sends `ChunkData` for those that have been mutated (track a `mutated: bool` per chunk, or compare against a fresh generation). Full per-block diffing is out of scope.

- [x] **Step 2: Apply `ChunkData` on the client**
  In `State::drain_network_events`, on `ChunkData { cx, cz, blocks }`: decompress/decode the block array (reuse the existing `save.rs` chunk payload format if compatible), overwrite the local chunk's blocks, rebuild its heightmap, re-propagate boundary lighting, and mark the mesh dirty. If the chunk isn't loaded yet, buffer the payload and apply it when `update_chunks` loads that coordinate.

- [x] **Step 3: Verify it compiles**
  Run: `cargo check`

---

### Task 6: Verification

- [x] **Step 1: Full check**
  Run: `cargo fmt --check && cargo check --release && cargo test`

- [ ] **Step 2: Two-client block sync test**
  Host + 2 Clients:
  - Host places a block; both clients see it appear (lighting + mesh update).
  - Host breaks a block; both clients see it removed.
  - Client A places a block (sends `RequestBlockChange`); host validates, applies, and broadcasts; client B and client A both see it.
  - Host triggers a fluid flow / Creeper explosion; clients observe the resulting block changes without running the sim themselves.

- [ ] **Step 3: Join-mid-game test**
  Host mutates several chunks, then a client joins; confirm the client's loaded chunks reflect the host's mutations via `ChunkData` catch-up (not bare terrain).

---

## Affected Files Summary

- **[MODIFY]** `src/world.rs`
- **[MODIFY]** `src/state.rs`
- **[MODIFY]** `src/network/server.rs` (chunk catch-up dispatch)
- **[MODIFY]** `src/network/protocol.rs` (optional `TimeSync` variant)
- **[MODIFY]** `ARCHITECTURE.md`

## Verification Gate

Before Sub-task 6 (chat + remote-player rendering + disconnect UI):
- All block mutations on the host propagate to clients with correct lighting/mesh/redstone-free application.
- Clients run no world mutation simulation; they only render host-driven changes.
- Mid-game joins receive correct chunk state.
