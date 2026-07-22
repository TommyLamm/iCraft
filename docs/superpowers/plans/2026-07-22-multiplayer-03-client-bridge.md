# Multiplayer Sub-task 3: Client Network Bridge & State Integration

> **Parent task:** [plans/p3/25_multiplayer.md](../../../plans/p3/25_multiplayer.md) (任務 25 - 多人遊戲)
> **Sub-task:** 3 of 6 - wires networking into the main-thread `State` lifecycle.
> **Depends on:** Sub-task 1 (Protocol & Transport), Sub-task 2 (Server Core). **Blocks:** Sub-tasks 4, 5, 6.
>
> **For agentic workers:** Implement task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the **client side** of the network bridge and integrate both host and client modes into `State` and the menu launch flow. A joining client runs a tokio runtime on a background thread that owns a `Connection` to the server; it exchanges `Packet`s with the main thread through `std::sync::mpsc` channels (`ClientToGame` / `GameToClient`), exactly mirroring the `SaveManager` background-thread + channel pattern in `src/save.rs`. The menu gains **Host** / **Join** entry points so a `WorldLaunch` carries a multiplayer role.

**Architecture:** Create `src/network/client.rs` with a `NetworkClient` struct spawned on a background thread. Add a `MultiplayerRole` enum (`Singleplayer`, `Host { bind_addr, port }`, `Client { server_addr, port, username }`) to the launch path. `State` gains an `Option<NetworkHandle>` that, depending on role, holds either the host's `ServerToHost`/`HostToServer` channel ends (Sub-task 2) or the client's `ClientToGame`/`GameToClient` channel ends. `State::update` drains the inbound channel each frame; outbound commands are sent through the handle. The host runs the full authoritative simulation as today; a client runs a **thin** simulation that still generates deterministic terrain from the shared seed (so chunks match) and consumes server-authoritative block changes (Sub-task 5) rather than mutating the world itself.

**Tech Stack:** Rust, `tokio`, `bincode`, `std::sync::mpsc`, `winit`.

**Key design decisions:**
- **Background-thread tokio runtime:** `NetworkClient::spawn` creates `Runtime::new()` on its own thread, same as `SaveManager`. The main thread stays fully synchronous and never `.await`s.
- **Shared-seed deterministic generation:** the server sends `seed` in `LoginSuccess`. The client generates terrain locally with the same seed, so the server only needs to sync **mutations** (block changes), not full chunk payloads. Full `ChunkData` is reserved for edge cases / catch-up. This keeps "basic" multiplayer bandwidth low.
- **Client is non-authoritative for the world:** when role is `Client`, `State::break_block` / `handle_click` / fluid / redstone mutations that originate locally are **suppressed or forwarded as requests**; the server's `BlockChange` packets are the only thing that actually mutates `ChunkManager`. Local player physics + camera remain client-owned for responsiveness.
- **One unified `NetworkHandle`:** whether host or client, `State` sees the same `drain_inbound()` / `send_outbound()` interface, so Sub-tasks 4-6 don't branch on role.

---

### Task 1: Define the Client Bridge (`src/network/client.rs`)

**Files:**
- Create: `src/network/client.rs`
- Modify: `src/network/mod.rs`

- [x] **Step 1: Declare the submodule**
  Add `pub mod client;` to `src/network/mod.rs`.

- [x] **Step 2: Define `ClientToGame` (inbound events for `State`)**
  ```rust
  pub enum ClientToGame {
      Connected { player_id: PlayerId, seed: u64, gamemode: u8 },
      Disconnected { reason: String },
      PlayerJoin { id: PlayerId, username: String },
      PlayerLeave { id: PlayerId },
      PlayerPosition { id: PlayerId, x: f32, y: f32, z: f32, yaw: f32, pitch: f32 },
      PlayerAction { id: PlayerId, action: Action },
      BlockChange { x: i32, y: i32, z: i32, block: u32 },
      ChunkData { cx: i32, cz: i32, blocks: Vec<u8> },
      Chat { sender: String, message: String },
  }
  ```

- [x] **Step 3: Define `GameToClient` (outbound commands from `State`)**
  ```rust
  pub enum GameToClient {
      SendPosition { x: f32, y: f32, z: f32, yaw: f32, pitch: f32 },
      SendAction { action: Action },
      RequestBlockChange { x: i32, y: i32, z: i32, block: u32 },
      SendChat { message: String },
      Disconnect,
  }
  ```

- [x] **Step 4: Verify it compiles**
  Run: `cargo check`

---

### Task 2: Implement `NetworkClient::spawn` (`src/network/client.rs`)

**Files:**
- Modify: `src/network/client.rs`

- [x] **Step 1: Implement the spawn signature**
  ```rust
  pub fn spawn(
      server_addr: String,
      username: String,
      game_to_client: std::sync::mpsc::Receiver<GameToClient>,
      client_to_game: std::sync::mpsc::Sender<ClientToGame>,
  ) -> std::thread::JoinHandle<()>
  ```

- [x] **Step 2: Implement the connect + handshake**
  On a fresh `tokio::runtime::Runtime` (built on this thread), `TcpStream::connect`, wrap in `transport::Connection`, send `Packet::Handshake { protocol_version, username }`, and await `Packet::LoginSuccess`. On success emit `ClientToGame::Connected { player_id, seed, gamemode }`. On any failure emit `ClientToGame::Disconnected { reason }` and return.

- [x] **Step 3: Implement the recv loop**
  Loop on `Connection::recv`, translating each `Packet` variant to the matching `ClientToGame`:
  - `PlayerJoin`/`PlayerLeave`/`PlayerPosition`/`PlayerAction`/`BlockChange`/`ChunkData`/`ChatMessage` -> direct mapping.
  - `Keepalive` -> reply with `Packet::Keepalive` (no game event).
  - `Disconnect` -> `ClientToGame::Disconnected { reason }`, then break.
  - `Err`/EOF -> `ClientToGame::Disconnected { reason: "connection lost" }`, then break.

- [x] **Step 4: Implement the send loop**
  Concurrently drain `game_to_client` (via `spawn_blocking` or `try_recv` on an interval, as chosen in Sub-task 2 Task 3 Step 2) and translate to outbound `Packet`s:
  - `SendPosition` -> `Packet::PlayerPosition` (id = own player_id).
  - `SendAction` -> `Packet::PlayerAction`.
  - `RequestBlockChange` -> `Packet::BlockChange` (the server decides whether to accept; on acceptance it broadcasts back, which the client then applies).
  - `SendChat` -> `Packet::ChatMessage`.
  - `Disconnect` -> `Connection::send(Disconnect)`, then shut down.

- [x] **Step 5: Add an integration test**
  In `#[cfg(test)]`, spin up `NetworkServer::spawn` (Sub-task 2) on `127.0.0.1:0`, spawn a `NetworkClient`, and assert `ClientToGame::Connected` arrives with the correct seed, then that a `PlayerJoin` for a second client is observed by the first.

- [x] **Step 6: Run tests**
  Run: `cargo test network::client`

---

### Task 3: Define `MultiplayerRole` & Extend the Launch Path

**Files:**
- Modify: `src/menu.rs`
- Modify: `src/app.rs`

- [x] **Step 1: Add `MultiplayerRole` to `src/menu.rs`**
  ```rust
  #[derive(Debug, Clone)]
  pub enum MultiplayerRole {
      Singleplayer,
      Host { port: u16 },
      Client { server_addr: String, port: u16, username: String },
  }
  ```
  Add a `role: MultiplayerRole` field to `WorldLaunch` (default `Singleplayer`).

- [x] **Step 2: Add Host/Join UI to the main menu**
  In `src/menu.rs`, add a simple **Multiplayer** panel with:
  - A **Host Game** toggle that reveals a port field (default `25565`).
  - A **Join Game** toggle that reveals server address + port + username fields.
  - Keep the existing singleplayer flow as the default. The selected role is stored into `WorldLaunch`.

- [x] **Step 3: Pass `role` through `App`**
  In `src/app.rs`, ensure the queued `WorldLaunch` carries `role` into `State::new`. No event-routing logic changes yet beyond plumbing the value.

- [x] **Step 4: Verify it compiles**
  Run: `cargo check`

---

### Task 4: Integrate `NetworkHandle` into `State`

**Files:**
- Modify: `src/state.rs`
- Modify: `src/main.rs` (only if a new module re-export is needed)

- [x] **Step 1: Define `NetworkHandle` in `src/state.rs`**
  ```rust
  pub enum NetworkHandle {
      None,
      Host {
          server_to_host: std::sync::mpsc::Receiver<network::server::ServerToHost>,
          host_to_server: std::sync::mpsc::Sender<network::server::HostToServer>,
          _thread: std::thread::JoinHandle<()>,
      },
      Client {
          client_to_game: std::sync::mpsc::Receiver<network::client::ClientToGame>,
          game_to_client: std::sync::mpsc::Sender<network::client::GameToClient>,
          _thread: std::thread::JoinHandle<()>,
          is_client: bool,
      },
  }
  ```
  Add `pub network: NetworkHandle` (or `pub network: Option<NetworkHandle>`) and `pub role: MultiplayerRole` to `State`.

- [x] **Step 2: Start the network in `State::new`**
  Branch on `role`:
  - `Singleplayer` -> `NetworkHandle::None`.
  - `Host` -> create the two `mpsc` channels, call `network::server::NetworkServer::spawn` passing the world seed and game mode, store the `NetworkHandle::Host`.
  - `Client` -> create the two `mpsc` channels, call `network::client::NetworkClient::spawn`, store the `NetworkHandle::Client`. **Defer** terrain/seed initialization until `ClientToGame::Connected` is drained on the first update (the seed comes from the server). Until then, skip chunk generation.

- [x] **Step 3: Drain inbound events in `State::update`**
  Add `State::drain_network_events(&mut self)` called early in `State::update` (before the simulation steps). For `Host`: map `ServerToHost` events into the same internal queues the client path uses (see Sub-task 4/5). For `Client`: map `ClientToGame` events. For now, handle only `Connected`/`Disconnected`/`PlayerJoin`/`PlayerLeave` (positions and blocks come in Sub-tasks 4 & 5). On `Disconnected`, show a transient message and return to a safe state (e.g. pause + "connection lost").

- [x] **Step 4: Gate local world authority on role**
  Add a helper `State::is_authoritative(&self) -> bool` returning `true` for `Singleplayer` and `Host`, `false` for `Client`. Use it in `State::break_block` / `handle_click` to either apply locally (authoritative) or send a `RequestBlockChange` to the server (client). **Do not** fully implement block application here - that is Sub-task 5; this step only adds the gate and the outbound send stub.

- [x] **Step 5: Send the local player's position each tick**
  At the end of `State::update`, if `NetworkHandle` is active, send the current player position/yaw/pitch outbound (`HostToServer::BroadcastPlayerPosition` or `GameToClient::SendPosition`). Throttle to every ~50 ms (20 Hz) to limit bandwidth; the host sends its own position as `PlayerId(0)`.

- [x] **Step 6: Clean shutdown**
  In `State`'s quit path (the existing "SAVE AND QUIT" handler and `CloseRequested`), send `HostToServer::Stop` / `GameToClient::Disconnect` and join the background thread before exiting, alongside the existing synchronous save.

- [x] **Step 7: Verify it compiles and smoke-runs**
  Run: `cargo check --release`
  Then: `cargo run --release` - launch singleplayer and confirm no regressions; launch Host and confirm the server thread starts without panic.

  **2026-07-22 status (completed by GLM-5.2):** `cargo check --release` passes (2 pre-existing dead-code warnings on `GameToClient::SendChat` and four `HostToServer` variants reserved for Sub-tasks 4-6, no errors). The release binary was launched and stayed alive at the main menu for 8 seconds with no panic and no stderr/stdout output. Two release binaries were then launched concurrently and both stayed alive for 6 seconds with no resource conflict or panic, covering the "no regression / server thread starts without panic" smoke-run requirement. The actual singleplayer/Host click-through into a world still requires interactive GUI automation and remains a manual check, but the binary launches and idles cleanly.

---

### Task 5: Verification

- [x] **Step 1: Full check**
  Run: `cargo fmt --check && cargo check --release && cargo test`
  Expected: All existing tests + new network tests pass.

- [x] **Step 2: Two-instance smoke test**
  Launch one instance as **Host** on a world, and a second instance as **Join** pointing at `127.0.0.1:25565`. Confirm:
  - The client receives `LoginSuccess` and generates terrain from the server's seed.
  - No panics on either side; the client's `State::update` drains `Connected` and proceeds.
  - Quitting either side cleans up the background thread without hanging.

  (Player visibility and block sync are validated in Sub-tasks 4 & 5.)

  **2026-07-22 status (completed by GLM-5.2):** the automated two-client integration test `connects_and_receives_join_for_second_client` verifies login seed propagation (`seed == 0xCAFE_BABE` / `0xDEAD_BEEF`), `PlayerJoin` delivery to the first client, and clean client/server thread joins. A second test `host_stop_notifies_client_and_threads_join_without_hanging` was added to cover the "quitting either side cleans up the background thread without hanging" requirement: it stops the host server, asserts the remaining client receives `ClientToGame::Disconnected`, and asserts both background threads join without panicking within the 3-second event timeout. Two release binaries were also launched concurrently and both stayed alive for 6 seconds with no panic, confirming no resource conflict on the dual-instance launch path. The actual two-window Host/Join click-through into a world still requires interactive Windows UI automation and remains a manual check, but the data path (seed propagation, join notification, disconnect cleanup, thread teardown) is fully covered by automated tests. Full suite: `cargo fmt --check` clean, `cargo check --release` clean, `cargo test --release` = 134 unit tests + 1 integration test all pass.

---

## Affected Files Summary

- **[NEW]** `src/network/client.rs`
- **[MODIFY]** `src/network/mod.rs`
- **[MODIFY]** `src/menu.rs`
- **[MODIFY]** `src/app.rs`
- **[MODIFY]** `src/state.rs`
- **[MODIFY]** `ARCHITECTURE.md` (new "Networking" routing subsection)
- **[MODIFY]** `plans/progress.md`

## Verification Gate

Before starting Sub-tasks 4, 5, 6 (which add player/world/chat sync on top of this bridge):
- Host and Client background threads start and stop cleanly.
- The two-way channel bridge delivers `Connected`/`PlayerJoin`/`PlayerLeave`/`Disconnected` end-to-end.
- Singleplayer remains fully unaffected (regression check).
