# Multiplayer Connection Debug Information Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add detailed console debug logging, real-time UI connection status updates, F3 HUD network debug info, and in-game chat system notifications during multiplayer game connection and session lifecycle.

**Architecture:** Extend `ClientToGame` channel with a `StatusUpdate` variant. In `src/network/client.rs` and `src/network/server.rs`, output tagged console logs (`[NetworkClient]`, `[NetworkServer]`) at each connection step. In `src/state.rs`, handle inbound status updates to refresh UI overlay text, output system chat log entries on join/leave/disconnect events, and display network status on the F3 overlay screen.

**Tech Stack:** Rust (std::thread, std::sync::mpsc, tokio::net::TcpStream/TcpListener, wgpu/ui vector-line rendering).

## Global Constraints

- Preserve protocol version compatibility (`PROTOCOL_VERSION`).
- Maintain existing non-blocking channel polling & thread safety.
- Console output should be prefixed with clear tags (`[NetworkClient]`, `[NetworkServer]`).
- Do not break existing network unit tests.

---

### Task 1: Client-Side Connection Logging & StatusUpdate Channel Event

**Files:**
- Modify: `src/network/client.rs`
- Modify: `src/state.rs`

**Interfaces:**
- Consumes: `ClientToGame` channel
- Produces: `ClientToGame::StatusUpdate { message: String }`

- [ ] **Step 1: Add StatusUpdate variant to ClientToGame enum in `src/network/client.rs`**

Add `StatusUpdate { message: String }` to `ClientToGame`.

- [ ] **Step 2: Add console logging and status updates to `run_client` in `src/network/client.rs`**

Update `run_client` to log `[NetworkClient]` console messages and send `ClientToGame::StatusUpdate`:
1. When starting: `eprintln!("[NetworkClient] Connecting to {server_addr}...");` and send `StatusUpdate("CONNECTING TO {server_addr}...")`.
2. On TCP connect success: `eprintln!("[NetworkClient] TCP connection established to {server_addr}");` and send `StatusUpdate("TCP CONNECTED. HANDSHAKING...")`.
3. On Handshake send: `eprintln!("[NetworkClient] Sent Handshake (user: {username}, v{PROTOCOL_VERSION})");` and send `StatusUpdate("HANDSHAKE SENT. WAITING FOR SERVER...");`.
4. On `LoginSuccess`: `eprintln!("[NetworkClient] Login success! Assigned Player ID: {player_id}, Seed: {seed}, Gamemode: {gamemode}");` and send `StatusUpdate("LOGIN SUCCESS. LOADING WORLD...");`.
5. On connection failure / error: `eprintln!("[NetworkClient] Connection failed: {error}");`.

- [ ] **Step 3: Handle StatusUpdate variant in `src/state.rs`**

In `src/state.rs` where `ClientToGame` is mapped to `NetworkInbound`:
Add `NetworkInbound::StatusUpdate(String)` and handle `ClientToGame::StatusUpdate { message }`.

- [ ] **Step 4: Check compilation**

Run: `cargo check`
Expected: PASS

- [ ] **Step 5: Commit changes**

```bash
git add src/network/client.rs src/state.rs
git commit -m "feat(network): add ClientToGame::StatusUpdate and client connection debug logging"
```

---

### Task 2: Server-Side Connection Debug Logging

**Files:**
- Modify: `src/network/server.rs`

**Interfaces:**
- Consumes: `NetworkServer::spawn`, `NetworkServer::run`, `NetworkServer::run_client`
- Produces: Console `[NetworkServer]` debug output

- [ ] **Step 1: Add server startup and client connection debug logging in `src/network/server.rs`**

In `NetworkServer::spawn`:
`eprintln!("[NetworkServer] Listening on {bind_addr} (Seed: {seed}, Gamemode: {gamemode})");`

In `NetworkServer::run`:
When a TCP stream is accepted:
Obtain peer address `let peer_addr = stream.peer_addr().ok();`
`eprintln!("[NetworkServer] Accepted TCP connection from {peer_addr:?}");`

In `Self::run_client`:
Log handshake reception:
`eprintln!("[NetworkServer] Received Handshake from {peer_addr:?}: username='{username}', protocol_version={protocol_version}");`
If version mismatch:
`eprintln!("[NetworkServer] Handshake rejected for {peer_addr:?}: version mismatch (expected {PROTOCOL_VERSION}, got {protocol_version})");`
On `LoginSuccess`:
`eprintln!("[NetworkServer] Sent LoginSuccess to '{username}' (Player ID: {player_id}) from {peer_addr:?}");`
On disconnection / timeout / drop:
`eprintln!("[NetworkServer] Client '{username}' (Player ID: {player_id}) from {peer_addr:?} disconnected: {reason}");`

- [ ] **Step 2: Check compilation**

Run: `cargo check`
Expected: PASS

- [ ] **Step 3: Commit changes**

```bash
git add src/network/server.rs
git commit -m "feat(network): add server connection lifecycle debug logging"
```

---

### Task 3: State UI Status Updates, F3 HUD Debug Info, and In-Game Chat System Logs

**Files:**
- Modify: `src/state.rs`

**Interfaces:**
- Consumes: `NetworkInbound`, `show_debug`, `chat_messages`
- Produces: Dynamic `network_status` rendering, System chat log messages, F3 network status line

- [ ] **Step 1: Update `drain_network_events` in `src/state.rs`**

1. On `NetworkInbound::StatusUpdate(msg)`:
   Update `self.network_status = Some(msg);`.
2. On `NetworkInbound::Connected`:
   Set `self.network_status = None;` and push system chat: `"[Network] Connected to server as player #<id>"`.
3. On `NetworkInbound::Disconnected(reason)`:
   Push system chat: `"[Network] Disconnected: <reason>"`.
4. On `NetworkInbound::PlayerJoin { id, username }`:
   Push system chat: `"[Network] <username> joined the game"`.
5. On `NetworkInbound::PlayerLeave(id)`:
   Push system chat: `"[Network] Player #<id> left the game"`.

- [ ] **Step 2: Add Network Info line to F3 Debug Screen in `src/state.rs`**

In `State::render` under `if self.show_debug`:
Add a `net_str` string:
- If `MultiplayerRole::Host { port }`:
  `format!("NET: HOST ON PORT {} | CLIENTS: {}", port, self.remote_players.len())`
- If `MultiplayerRole::Client { server_addr, port, .. }`:
  `format!("NET: CLIENT @ {}:{} | LOCAL ID: {} | PLAYERS: {}", server_addr, port, self.local_player_id.unwrap_or(0), self.remote_players.len() + 1)`
- If `MultiplayerRole::Singleplayer`:
  `"NET: SINGLEPLAYER".to_string()`

Include `net_str` in `debug_lines`.

- [ ] **Step 3: Check compilation and unit tests**

Run: `cargo test`
Expected: PASS

- [ ] **Step 4: Commit changes**

```bash
git add src/state.rs
git commit -m "feat(ui): update dynamic network status, chat system logs, and F3 debug HUD"
```

---

### Task 4: Verification & Test Coverage

**Files:**
- Modify: `src/network/client.rs`

- [ ] **Step 1: Update client unit tests to handle `ClientToGame::StatusUpdate`**

In `src/network/client.rs` tests (e.g. `wait_for_event`), filter out or accept `ClientToGame::StatusUpdate` events so existing tests pass cleanly without being broken by intermediate status updates.

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: PASS

- [ ] **Step 3: Commit unit test fixes**

```bash
git add src/network/client.rs
git commit -m "test(network): update client unit test event filtering for StatusUpdate"
```
