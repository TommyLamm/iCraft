# Multiplayer Sub-task 1: Network Protocol & Transport Layer

> **Parent task:** [plans/p3/25_multiplayer.md](../../../plans/p3/25_multiplayer.md) (任務 25 - 多人遊戲)
> **Sub-task:** 1 of 6 — foundation layer with **no game-logic dependencies**.
> **Depends on:** nothing. **Blocks:** Sub-tasks 2, 3, 4, 5.
>
> **For agentic workers:** Implement task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish a self-contained, testable networking foundation: a versioned `Packet` enum, length-prefixed binary framing over TCP, and an async connection abstraction built on `tokio`. This layer must compile and round-trip packets in unit tests without touching any game module.

**Architecture:** Create a new `src/network/` module group. `protocol.rs` owns the `Packet` enum and its (de)serialization via the existing `bincode` crate. `transport.rs` owns a `Connection` struct wrapping a `tokio` TCP stream with manual 4-byte big-endian length framing (`read_exact` / `write_all`) so no extra codec crate is required. The tokio runtime is **not** started here; `Connection` is runtime-agnostic and driven by whoever owns it (the server in Sub-task 2, the client in Sub-task 3).

**Tech Stack:** Rust, `tokio` (new), `bincode` (existing), `serde` (existing).

**Key design decisions:**
- **Length-prefixed framing** (4-byte BE length + bincode payload): simple, allocation-free reads into a reused buffer, and safe over TCP's stream boundary.
- **Packet version field**: every packet carries a `protocol_version: u32` so future schema changes can reject mismatched clients cleanly.
- **No `#[tokio::main]`**: the runtime is created on a dedicated background thread later (Sub-task 3), mirroring the proven `SaveManager` background-thread + `mpsc` pattern in `src/save.rs`. This keeps the main winit thread fully synchronous.
- **Send/Sync boundaries**: `Connection` holds a `tokio::sync::mpsc::Sender<Packet>` for outbound and exposes `recv()` for inbound, decoupling producers/consumers from the socket.

---

### Task 1: Add Dependencies & Declare the Network Module

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/main.rs`
- Create: `src/network/mod.rs`

- [ ] **Step 1: Add `tokio` to `Cargo.toml`**
  Under `[dependencies]`, add only the features needed for TCP + a multi-thread runtime:
  ```toml
  tokio = { version = "1", features = ["rt-multi-thread", "net", "io-util", "sync", "macros", "time"] }
  ```

- [ ] **Step 2: Create `src/network/mod.rs`**
  Declare the submodules and re-export the public surface:
  ```rust
  pub mod protocol;
  pub mod transport;
  ```

- [ ] **Step 3: Declare the `network` module in `src/main.rs`**
  Add `mod network;` to `src/main.rs` (keep alphabetical-ish ordering near the other modules).

- [ ] **Step 4: Verify it compiles**
  Run: `cargo check`
  Expected: Success (empty submodules).

---

### Task 2: Define the Packet Protocol (`src/network/protocol.rs`)

**Files:**
- Create: `src/network/protocol.rs`

- [ ] **Step 1: Define shared wire types**
  Define `PlayerId` (`pub type PlayerId = u64;`) and a `PROTOCOL_VERSION: u32 = 1` constant. Define a small `Action` enum (`Place`, `Break`, `Use`) used by `PlayerAction`.

- [ ] **Step 2: Define the `Packet` enum**
  All variants carry `protocol_version: u32` so the framing layer can reject mismatches. Use `#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]`:
  ```rust
  pub enum Packet {
      // Connection lifecycle
      Handshake { protocol_version: u32, username: String },
      LoginSuccess { protocol_version: u32, player_id: PlayerId, seed: u64, gamemode: u8 },
      Disconnect { protocol_version: u32, reason: String },
      // Player state (broadcast by server, sent by client)
      PlayerPosition { protocol_version: u32, id: PlayerId, x: f32, y: f32, z: f32, yaw: f32, pitch: f32 },
      PlayerAction { protocol_version: u32, id: PlayerId, action: Action },
      PlayerJoin { protocol_version: u32, id: PlayerId, username: String },
      PlayerLeave { protocol_version: u32, id: PlayerId },
      // World mutations (server-authoritative)
      BlockChange { protocol_version: u32, x: i32, y: i32, z: i32, block: u32 },
      ChunkData { protocol_version: u32, cx: i32, cz: i32, blocks: Vec<u8> },
      // Chat
      ChatMessage { protocol_version: u32, sender: String, message: String },
      // Keepalive / timeout
      Keepalive { protocol_version: u32 },
  }
  ```
  Note: `BlockType` is serialized as `u32` (its discriminant) to keep the wire format compact and decoupled from the in-game enum's internal layout. A conversion helper to/from `BlockType` is added in Sub-task 5.

- [ ] **Step 3: Add (de)serialization helpers**
  Implement `Packet::encode(&self) -> Vec<u8>` (bincode serialize) and `Packet::decode(bytes: &[u8]) -> Result<Packet, bincode::Error>`.

- [ ] **Step 4: Add unit tests for roundtrip serialization**
  In a `#[cfg(test)] mod tests` block, assert every variant encodes and decodes back to an equal value, and that a mismatched `protocol_version` can be detected by the caller.

- [ ] **Step 5: Run tests**
  Run: `cargo test network::protocol`
  Expected: All roundtrip tests pass.

---

### Task 3: Implement Length-Prefixed Framing & `Connection` (`src/network/transport.rs`)

**Files:**
- Create: `src/network/transport.rs`

- [ ] **Step 1: Implement the `Connection` struct**
  ```rust
  pub struct Connection {
      stream: tokio::net::TcpStream,
      read_buf: Vec<u8>,   // reused for payload reads
  }
  ```
  Provide `Connection::new(stream: TcpStream) -> Self`.

- [ ] **Step 2: Implement `Connection::recv`**
  ```rust
  pub async fn recv(&mut self) -> std::io::Result<Packet>
  ```
  Read 4 bytes (big-endian `u32` length), cap it at a sane maximum (e.g. 2 MiB) to guard against malformed peers, `read_exact` the payload into `read_buf`, then `Packet::decode`. Return a distinct error on truncation/oversize.

- [ ] **Step 3: Implement `Connection::send`**
  ```rust
  pub async fn send(&mut self, packet: &Packet) -> std::io::Result<()>
  ```
  `Packet::encode`, prepend 4-byte BE length, `write_all`. Use `flush` after writing.

- [ ] **Step 4: Add a framed roundtrip integration test**
  In a `#[cfg(test)] mod tests` using `#[tokio::test]`, bind a `TcpListener` on `127.0.0.1:0`, accept one connection, connect a peer, and assert a `Handshake` packet sent from one side is received identically on the other.

- [ ] **Step 5: Run tests**
  Run: `cargo test network::transport`
  Expected: The framed roundtrip test passes.

---

### Task 4: Verification

- [ ] **Step 1: Full check**
  Run: `cargo fmt --check && cargo check --release && cargo test`
  Expected: All existing tests plus the new protocol/transport tests pass; no game module is touched yet.

---

## Affected Files Summary

- **[NEW]** `src/network/mod.rs`
- **[NEW]** `src/network/protocol.rs`
- **[NEW]** `src/network/transport.rs`
- **[MODIFY]** `Cargo.toml`
- **[MODIFY]** `src/main.rs`

## Verification Gate

Before starting Sub-task 2:
- `cargo test network::protocol` passes (all variants roundtrip).
- `cargo test network::transport` passes (framed TCP roundtrip).
- `cargo check --release` is clean.
