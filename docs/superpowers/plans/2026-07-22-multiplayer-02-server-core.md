# Multiplayer Sub-task 2: Integrated Server Core

> **Parent task:** [plans/p3/25_multiplayer.md](../../../plans/p3/25_multiplayer.md) (任務 25 - 多人遊戲)
> **Sub-task:** 2 of 6 - the authority/host side.
> **Depends on:** Sub-task 1 (Protocol & Transport). **Blocks:** Sub-tasks 4, 5.
>
> **For agentic workers:** Implement task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement an **integrated (listen) server** that runs inside the host's process and owns the authoritative world state. It accepts TCP connections, performs the handshake/login flow, manages connected client sessions, and relays world mutations and player state between clients. The host's existing `State`/`ChunkManager` remains the source of truth — the server simply fans the host's authoritative changes out to clients and relays client inputs back to the host.

**Architecture:** Create `src/network/server.rs` containing a `NetworkServer` struct. The server runs its `tokio` accept + per-client relay loops on a **dedicated background thread** (created in Sub-task 3 alongside the host bridge). It communicates with the main thread through a pair of `std::sync::mpsc` channels — `ServerToHost` (events for the host's `State` to consume) and `HostToServer` (commands the host emits) — exactly mirroring the `SaveManager` background-thread pattern in `src/save.rs`. This keeps all simulation on the main thread and avoids `Send` problems with GPU/winit types.

**Tech Stack:** Rust, `tokio`, `bincode`, `std::sync::mpsc`.

**Key design decisions:**
- **Listen-server, not dedicated binary:** reuses all existing world generation, lighting, fluids, and redstone code on the host. No headless fork required. Matches the "basic multiplayer" scope.
- **Host is player zero:** the host never opens a TCP socket to itself; its own player state is injected directly into the broadcast set as `PlayerId(0)`.
- **Channel bridge, not shared state:** the server never touches `State` or `ChunkManager` directly. It receives `HostToServer` commands (e.g. "broadcast this block change", "send chunk X,Z to client Y") and emits `ServerToHost` events (e.g. "client Z broke block at P"). The host applies events during its normal `State::update`.
- **Per-client send queue:** each connected client has a `tokio::sync::mpsc::Receiver<Packet>` so a slow client cannot block the broadcast loop; the send task drops packets if the queue is full (lagging client).

---

### Task 1: Define Server Channel Message Types (`src/network/server.rs`)

**Files:**
- Create: `src/network/server.rs`
- Modify: `src/network/mod.rs`

- [ ] **Step 1: Declare the submodule**
  Add `pub mod server;` to `src/network/mod.rs`.

- [ ] **Step 2: Define `ServerToHost` (events the host consumes)**
  ```rust
  pub enum ServerToHost {
      ClientJoined { id: PlayerId, username: String },
      ClientLeft { id: PlayerId },
      ClientPosition { id: PlayerId, x: f32, y: f32, z: f32, yaw: f32, pitch: f32 },
      ClientAction { id: PlayerId, action: Action },
      ClientBlockChange { id: PlayerId, x: i32, y: i32, z: i32, block: u32 },
      ChatFromClient { id: PlayerId, message: String },
  }
  ```

- [ ] **Step 3: Define `HostToServer` (commands the host emits)**
  ```rust
  pub enum HostToServer {
      BroadcastBlockChange { x: i32, y: i32, z: i32, block: u32 },
      SendChunk { cx: i32, cz: i32, blocks: Vec<u8>, to: PlayerId },
      BroadcastPlayerPosition { id: PlayerId, x: f32, y: f32, z: f32, yaw: f32, pitch: f32 },
      BroadcastChat { sender: String, message: String },
      NotifyPlayerJoin { id: PlayerId, username: String },
      NotifyPlayerLeave { id: PlayerId },
      Stop,
  }
  ```

- [ ] **Step 4: Verify it compiles**
  Run: `cargo check`

---

### Task 2: Implement Client Session Management (`src/network/server.rs`)

**Files:**
- Modify: `src/network/server.rs`

- [ ] **Step 1: Define `ClientSession`**
  ```rust
  struct ClientSession {
      id: PlayerId,
      username: String,
      out_tx: tokio::sync::mpsc::Sender<Packet>,  // feed to the send task
  }
  ```
  Hold all sessions in a `HashMap<PlayerId, ClientSession>` inside `NetworkServer`.

- [ ] **Step 2: Implement `NetworkServer::spawn`**
  ```rust
  pub fn spawn(
      bind_addr: String,
      seed: u64,
      gamemode: u8,
      host_to_server: std::sync::mpsc::Receiver<HostToServer>,
      server_to_host: std::sync::mpsc::Sender<ServerToHost>,
  ) -> std::thread::JoinHandle<()>
  ```
  Create a `tokio::runtime::Runtime::new()` on this background thread, then block_on the run loop. Return the `JoinHandle` so the host can join on shutdown.

- [ ] **Step 3: Implement the accept loop**
  Bind a `TcpListener` on `bind_addr`. For each accepted stream, spawn a tokio task that:
  1. Wraps the stream in `transport::Connection`.
  2. Awaits a `Packet::Handshake`; validates `protocol_version == PROTOCOL_VERSION`. On mismatch, send `Packet::Disconnect { reason }` and drop.
  3. Allocates a fresh `PlayerId` (monotonic counter starting at 1).
  4. Sends `Packet::LoginSuccess { player_id, seed, gamemode }`.
  5. Registers the session, emits `ServerToHost::ClientJoined`, and starts two tasks (recv loop + send loop) below.

- [ ] **Step 4: Implement the per-client recv loop**
  Loop on `Connection::recv`:
  - `PlayerPosition` -> forward as `ServerToHost::ClientPosition`.
  - `PlayerAction` -> forward as `ServerToHost::ClientAction`.
  - `BlockChange` -> forward as `ServerToHost::ClientBlockChange`.
  - `ChatMessage` -> forward as `ServerToHost::ChatFromClient`.
  - `Keepalive` -> reset the client's timeout timer.
  - `Err` / `Disconnect` / EOF -> break, triggering cleanup in Step 5.

- [ ] **Step 5: Implement the per-client send loop & cleanup**
  Loop on the client's `out_rx`:
  - `Connection::send(packet).await`; on error, break.
  - On break: remove the session from the map, emit `ServerToHost::ClientLeft`, broadcast `Packet::PlayerLeave` to remaining clients.

---

### Task 3: Implement Host Command Processing (`src/network/server.rs`)

**Files:**
- Modify: `src/network/server.rs`

- [ ] **Step 1: Implement the `HostToServer` drain loop**
  Run concurrently with the accept loop (e.g. `tokio::select!` over `host_to_server.recv()` and the accept future, or a dedicated task using `tokio::task::spawn_blocking` to bridge the std channel). For each command:
  - `BroadcastBlockChange` -> `Packet::BlockChange` to **every** client's `out_tx`.
  - `SendChunk` -> `Packet::ChunkData` to the single `to` client.
  - `BroadcastPlayerPosition` -> `Packet::PlayerPosition` to every client (used for the host's own position).
  - `BroadcastChat` -> `Packet::ChatMessage` to every client.
  - `NotifyPlayerJoin` / `NotifyPlayerLeave` -> `Packet::PlayerJoin` / `Packet::PlayerLeave` to every client.
  - `Stop` -> shut down the runtime and return.

- [ ] **Step 2: Bridge the std channel into async safely**
  Because `host_to_server` is a blocking `std::sync::mpsc::Receiver`, use `tokio::task::spawn_blocking` to read it in a blocking manner and forward via a `tokio::sync::mpsc` to the async select loop, OR poll it with `try_recv` on a `tokio::time::interval`. Document the chosen approach in a comment.

- [ ] **Step 3: Implement keepalive & timeout**
  Every 5 s, each client session sends a `Packet::Keepalive`. If no packet (including keepalive) is received from a client within 15 s, treat it as a timeout disconnect (run the Step 5 cleanup).

- [ ] **Step 4: Verify it compiles**
  Run: `cargo check`

---

### Task 4: Add Server Integration Tests

**Files:**
- Modify: `src/network/server.rs`

- [ ] **Step 1: Write a connect + login test**
  In a `#[cfg(test)] mod tests` block with `#[tokio::test]`:
  - Spawn `NetworkServer::spawn` on `127.0.0.1:0` (use `TcpListener::bind` first to learn the port, then hand the addr in).
  - Connect a client `Connection`, send `Handshake`, assert receipt of `LoginSuccess` with a nonzero `player_id` and the expected seed.
  - Drain `server_to_host` and assert a `ClientJoined` event arrived.

- [ ] **Step 2: Write a broadcast relay test**
  - Connect two clients. Have the server receive a `PlayerPosition` from client A (via `ServerToHost::ClientPosition`) and, when the host sends `HostToServer::BroadcastPlayerPosition`, assert client B receives the corresponding `Packet::PlayerPosition`.

- [ ] **Step 3: Write a disconnect cleanup test**
  - Drop client A's connection; assert `ServerToHost::ClientLeft` is emitted and client B receives `Packet::PlayerLeave`.

- [ ] **Step 4: Run tests**
  Run: `cargo test network::server`
  Expected: All server tests pass.

---

### Task 5: Verification

- [ ] **Step 1: Full check**
  Run: `cargo fmt --check && cargo check --release && cargo test`
  Expected: All existing + new tests pass. No game module is modified yet — the server is driven only by its channel interface.

---

## Affected Files Summary

- **[NEW]** `src/network/server.rs`
- **[MODIFY]** `src/network/mod.rs`

## Verification Gate

Before starting Sub-task 4 / 5 (which wire the server into the host `State`):
- `cargo test network::server` passes (login, broadcast relay, disconnect cleanup).
- The server compiles and runs entirely on its background thread with the channel bridge; the main thread is never async.
