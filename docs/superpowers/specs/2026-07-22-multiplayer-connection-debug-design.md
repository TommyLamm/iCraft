# Multiplayer Connection Debug Information Design

## Overview

This document specifies the design for adding detailed debug and status information throughout the multiplayer connection lifecycle in `iCraft`. It enhances developer troubleshooting via tagged console logging (`[NetworkClient]`, `[NetworkServer]`), provides real-time connection progress feedback on the client UI screen, expands the F3 debug HUD with network metadata, and logs join/leave/connection events into the in-game chat system.

## Components & Changes

### 1. `src/network/client.rs` & `src/network/protocol.rs`

- **ClientToGame Channel**:
  - Add `StatusUpdate { message: String }` variant to `ClientToGame` enum to inform the main thread `State` of fine-grained connection progress.

- **Console Debug Logging & Progress Updates**:
  - `[NetworkClient] Initiating connection to <server_addr>...`
  - Send status update: `"CONNECTING TO <server_addr>..."`
  - On TCP success: `[NetworkClient] TCP connection established to <server_addr>`
  - Send status update: `"TCP CONNECTED. SENDING HANDSHAKE..."`
  - On Handshake send: `[NetworkClient] Sent Handshake (version: <v>, username: <user>)`
  - Send status update: `"HANDSHAKE SENT. WAITING FOR SERVER..."`
  - On `LoginSuccess`: `[NetworkClient] Login success! Assigned Player ID: <id>, Seed: <seed>, Gamemode: <gamemode>`
  - Send status update: `"LOGIN SUCCESS. LOADING WORLD..."`
  - On Disconnect / Timeout / Error: `[NetworkClient] Connection failed / disconnected: <reason>`

### 2. `src/network/server.rs`

- **Server Debug Console Logging**:
  - `[NetworkServer] Listening on <bind_addr> (Seed: <seed>, Gamemode: <gamemode>)`
  - `[NetworkServer] Accepted TCP connection from <peer_addr>`
  - `[NetworkServer] Received Handshake from <peer_addr>: username='<username>', protocol_version=<v>`
  - If protocol version mismatch: `[NetworkServer] Handshake rejected for <peer_addr>: protocol mismatch (expected <v1>, got <v2>)`
  - On `LoginSuccess`: `[NetworkServer] Sent LoginSuccess to '<username>' (Player ID: <id>) from <peer_addr>`
  - On Client disconnect / timeout: `[NetworkServer] Client '<username>' (Player ID: <id>) from <peer_addr> disconnected: <reason>`

### 3. `src/state.rs`

- **NetworkInbound Handling**:
  - Add `StatusUpdate(String)` to internal `NetworkInbound` enum.
  - In `drain_network_events`:
    - On `StatusUpdate(msg)`: update `self.network_status = Some(msg)`.
    - On `Connected`: clear `self.network_status = None` and add system chat message `"[Network] Connected to server as player #<id>"`.
    - On `Disconnected`: update `self.network_status = Some(...)` and add system chat message `"[Network] Disconnected: <reason>"`.
    - On `PlayerJoin`: add system chat message `"[Network] <username> joined the game"`.
    - On `PlayerLeave`: add system chat message `"[Network] Player #<id> left the game"`.

- **F3 Debug Screen Extension**:
  - When `self.show_debug` is active:
    - If `MultiplayerRole::Host { port }`:
      - Append `NET: HOST ON PORT <port> | CLIENTS: <num_remote_players>`
    - If `MultiplayerRole::Client { server_addr, port, username }`:
      - Append `NET: CLIENT @ <server_addr>:<port> | ID: <local_id> | PLAYERS: <num_remote_players + 1>`

## Verification Plan

### Automated Tests
- Run `cargo check` to verify type checking.
- Run `cargo test` to ensure existing network & unit tests pass.

### Manual Verification
- Test joining server / host connection flow with logging enabled.
- Verify console output tags `[NetworkClient]` and `[NetworkServer]`.
- Verify dynamic connection status messages on loading overlay.
- Press F3 in multiplayer game to verify network debug line display.
