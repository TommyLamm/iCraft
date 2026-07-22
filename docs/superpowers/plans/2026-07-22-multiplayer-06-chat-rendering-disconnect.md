# Multiplayer Sub-task 6: Chat, Remote Player Rendering & Disconnect Handling

> **Parent task:** [plans/p3/25_multiplayer.md](../../../plans/p3/25_multiplayer.md) (任務 25 - 多人遊戲)
> **Sub-task:** 6 of 6 (final) - the user-facing surface: chat UI, remote-player avatars, name tags, and graceful disconnect.
> **Depends on:** Sub-task 3 (Client Bridge), Sub-task 4 (Player Sync data). **Blocks:** nothing - completes task 25.
>
> **For agentic workers:** Implement task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the visible multiplayer experience. Add an in-game **chat box** (open with `T`, send with `Enter`, close with `Esc`, display recent messages) wired to the `ChatMessage` packet path. Render **remote players** as a simple block-figure avatar (reusing the existing `mob_renderer` cuboid pipeline) with interpolated pose from Sub-task 4, plus floating **name tags**. Finally, handle **disconnect** gracefully: detect host/client loss, clean up remote-player entities, and surface a non-destructive "connection lost" message with a return path to the menu.

**Architecture:** Chat state lives in `State` (a ring buffer of recent messages + an input string + an `is_chat_open` flag); `T`/`Enter`/`Esc` routing is added to `App::window_event`. Remote-player rendering extends `mob_renderer::render_mobs` with an `EntityType::RemotePlayer` branch that assembles a Steve-like cuboid figure (head, body, arms, legs) from the existing `add_cuboid` helper and applies yaw. Name tags reuse the existing vector-text UI path in `State::render`. Disconnect handling extends `State::drain_network_events` (the `Disconnected` case from Sub-task 3) and the shutdown path.

**Tech Stack:** Rust, `wgpu`, `winit`, existing `mob_renderer.rs`, existing vector-font UI.

**Key design decisions:**
- **Chat via existing UI primitives:** no new GPU pipeline. Chat input + history are drawn with the same textured/colored UI quads and vector text already used by the inventory and F3 overlay.
- **Remote avatar = cuboids:** matches the rest of the entity rendering (skeletons, dropped items already use `add_cuboid`). No new textures required beyond a simple skin tone; optionally read a `player_skin.png` later.
- **Name tags as screen-space text:** project the remote player's world position to screen space (using the camera view-projection already on `State`), then draw the username with the vector font, clamped to screen bounds. Skip the tag if the player is behind the camera or occluded (basic depth test optional).
- **Non-destructive disconnect:** on connection loss, freeze the local world (do not corrupt saves), remove all `RemotePlayer` entities, set a `connection_lost` flag that overlays a message, and offer "Return to Menu" - which performs the normal save-and-quit flow for the host's world.

---

### Task 1: Chat UI State & Input Routing (`src/state.rs`, `src/app.rs`)

**Files:**
- Modify: `src/state.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Add chat state to `State`**
  ```rust
  pub chat_messages: std::collections::VecDeque<(String, String)>,  // (sender, message), cap ~50
  pub chat_input: String,
  pub is_chat_open: bool,
  ```
  Initialize empty in `State::new`.

- [ ] **Step 2: Route `T` / `Enter` / `Esc` in `App::window_event`**
  - `T` (only when playing, no other screen open): set `is_chat_open = true`, clear `chat_input`.
  - While `is_chat_open`: capture `KeyboardInput` characters into `chat_input` (reuse the text-input logic already present for the anvil rename field in `src/enchantment.rs`); `Enter` sends the message (see Step 3); `Esc` closes without sending.
  - While chat is open, suppress world-interaction input (movement may stay enabled or be disabled - choose disabled for clarity, matching inventory-open behavior).

- [ ] **Step 3: Send a chat message**
  On `Enter` (if `chat_input` non-empty): emit the outbound chat command (`HostToServer::BroadcastChat { sender: local_username, message }` for host; `GameToClient::SendChat { message }` for client). The server prepends/uses the sender's username. Clear input and close the box.

- [ ] **Step 4: Receive chat into the ring buffer**
  In `State::drain_network_events`, on `Chat { sender, message }` (client) / `ServerToHost::ChatFromClient` (host): push `(sender, message)` to `chat_messages`, evicting the oldest beyond the cap. On host, also broadcast to all clients.

- [ ] **Step 5: Verify it compiles**
  Run: `cargo check`

---

### Task 2: Render Chat Box & History (`src/state.rs`)

**Files:**
- Modify: `src/state.rs`

- [ ] **Step 1: Render recent messages**
  In `State::render` (UI pass), draw the last ~8 `chat_messages` lines bottom-left, fading older entries (optional alpha). Use the existing vector-text rendering used by F3/inventory.

- [ ] **Step 2: Render the input box when open**
  When `is_chat_open`, draw a translucent input rectangle at the bottom of the screen with the current `chat_input` text and a cursor (simple blinking underscore). Reuse the colored-UI quad path.

- [ ] **Step 3: Verify it renders**
  Run: `cargo run --release` -> press `T`, type, `Enter`; confirm the message appears in history. (Two-instance test for cross-client chat is in Task 5.)

---

### Task 3: Render Remote Player Avatars (`src/mob_renderer.rs`, `src/texture.rs`)

**Files:**
- Modify: `src/mob_renderer.rs`
- Modify: `src/texture.rs` (optional skin tile)
- Modify: `src/state.rs`

- [ ] **Step 1: Add a skin tile to the texture atlas**
  In `src/texture.rs`, add a simple procedurally-drawn skin-tone tile (or leave a placeholder UV and document that `assets/textures/player_skin.png` can override later). Record its atlas UVs.

- [ ] **Step 2: Add the `RemotePlayer` branch in `mob_renderer::render_mobs`**
  Using the entity's interpolated `position`/`yaw` (set in Sub-task 4), assemble a block figure from `add_cuboid`:
  - Head (0.5×0.5×0.5) on top.
  - Body (0.5×0.75×0.25) below the head.
  - Two arms (0.25×0.75×0.25) at the sides.
  - Two legs (0.25×0.75×0.25) below the body.
  Apply yaw rotation around the vertical axis. Use the entity's `walk`/time for a subtle idle arm swing if desired (optional for "basic").

- [ ] **Step 3: Ensure remote players are included in the mob mesh data**
  In `State::render`, the mob-mesh data collection already iterates `EntityManager`; confirm `RemotePlayer` entities are passed to `render_mobs` and drawn in the opaque pass.

- [ ] **Step 4: Verify it renders**
  Run: `cargo run --release` with Host + 1 Client; move the client - confirm the host sees a block-figure avatar at the client's position, and vice versa.

---

### Task 4: Render Name Tags (`src/state.rs`)

**Files:**
- Modify: `src/state.rs`

- [ ] **Step 1: Project remote-player positions to screen space**
  For each `RemotePlayerState`, take the entity's interpolated world position + a head offset (e.g. +1.8 y), multiply by the camera's view-projection matrix, and convert NDC to screen pixels. Skip if the point is behind the camera (`w <= 0`).

- [ ] **Step 2: Draw the username**
  Using the existing vector-text UI path, draw the `username` centered above the projected point, with a small translucent background quad for readability. Clamp the x-position to screen bounds so off-screen-edge players show a partial tag.

- [ ] **Step 3: Verify it renders**
  Run: `cargo run --release`; confirm the remote player's name floats above its avatar and tracks it as it moves.

---

### Task 5: Graceful Disconnect & Cleanup (`src/state.rs`, `src/app.rs`)

**Files:**
- Modify: `src/state.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Handle `Disconnected` from the network drain**
  In `State::drain_network_events`, on `Disconnected { reason }`:
  - Set `connection_lost: bool = true` and store the reason.
  - Remove all `RemotePlayer` entities from `EntityManager` and clear `remote_players`.
  - Pause local simulation input (freeze the world; do not corrupt the save).
  - For a client, stop sending outbound commands; the background thread has already exited.

- [ ] **Step 2: Render the disconnect overlay**
  In `State::render`, if `connection_lost`, draw a centered "CONNECTION LOST: <reason>" message and a "RETURN TO MENU" button (reuse the existing button rendering from the pause menu).

- [ ] **Step 3: Wire "Return to Menu"**
  In `App::window_event`, clicking "RETURN TO MENU" triggers the normal save-and-quit-to-menu flow (host saves the world; client discards local state). Ensure the network background thread is joined before returning to `Menu`.

- [ ] **Step 4: Handle host-side client disconnect**
  Already mostly handled in Sub-task 2 (server emits `PlayerLeave`). Confirm that on `ServerToHost::ClientLeft`, `State` removes the corresponding remote-player entity and that remaining clients see the avatar disappear.

- [ ] **Step 5: Verify it compiles**
  Run: `cargo check --release`

---

### Task 6: Verification & Task 25 Sign-off

- [ ] **Step 1: Full check**
  Run: `cargo fmt --check && cargo check --release && cargo test`

- [ ] **Step 2: End-to-end multiplayer smoke test (matches task 25 acceptance)**
  - Two clients connect to the same host. (Meets: "兩個客戶端可連線同一伺服器".)
  - Players are mutually visible and positions sync smoothly with interpolation. (Meets: "玩家互相可見且位置同步".)
  - One player places/breaks a block; the other sees it. (Meets: "一個玩家放置的方塊另一個能看到".)
  - Chat: `T` opens the box, `Enter` sends, both clients see the message. (Meets: "聊天功能正常".)
  - Kill the host process / disconnect a client; the other side shows "CONNECTION LOST" and can return to the menu without panicking or corrupting saves.

- [ ] **Step 3: Update tracking docs**
  - Mark task #25 complete in `plans/progress.md` (status 🟢, completion date, changelog entry summarizing the 6 sub-tasks, new files, modified files, key decisions, and verification).
  - Refresh `ARCHITECTURE.md`: add a "Networking" subsection under "Runtime data flows" and a `src/network/` row in the source routing table; note the listen-server model, the background-thread + mpsc bridge, shared-seed deterministic generation, and host-authoritative block sync.

---

## Affected Files Summary

- **[MODIFY]** `src/state.rs`
- **[MODIFY]** `src/app.rs`
- **[MODIFY]** `src/mob_renderer.rs`
- **[MODIFY]** `src/texture.rs`
- **[MODIFY]** `ARCHITECTURE.md`
- **[MODIFY]** `plans/progress.md`

## Verification Gate (Task 25 Complete)

All four acceptance criteria from `plans/p3/25_multiplayer.md` are met:
- [ ] Two clients connect to one server.
- [ ] Players are mutually visible with synced positions.
- [ ] Block placement is visible to the other client.
- [ ] Chat works.
