# Plan01 — 協定 v20：刪 leftover inbound 與死 ACK／catch-up

## 定位

Protocol v19 仍帶六個 **decode 後丟棄** 的 inbound 變體，以及一條 **從未由 `ServerRuntime` 送出** 的 ACK 鏈。Plan 03 刪了 desktop catch-up，但 TCP 層 `ChunkAck` → `CatchupAck` 在 `server_runtime/ingress.rs` 仍是 `Ok(())` no-op。

Leftover inbound（`src/network/ingress.rs` catch-all `Ok(Ok(_))`）：

- `Packet::BlockActionRequest`
- `Packet::SleepRequest`
- `Packet::ContainerOpenRequest`
- `Packet::ContainerClickRequest`
- client→server `ContainerClose`
- client→server `BlockChange`（與 **server→client `BlockChange` 投影** 不同方向）

死 egress：

- `Packet::BlockActionResult`／`HostToServer::SendBlockActionResult`：全 repo 僅 `network/server.rs` 測試 emit。
- Join handler `NetworkInbound::BlockActionResult`（`presentation/network_event.rs`）永遠收不到 live 封包。

Catch-up 假管線：

- `Packet::ChunkAck`；`ServerToHost::{CatchupAck, CatchupAccepted, CatchupBackpressured}`
- `state.rs`：`MAX_CATCHUP_SUBMITS_PER_FRAME`、`CATCHUP_ACK_TIMEOUT`、`MAX_CATCHUP_RETRIES`（`#[allow(dead_code)]`）

刪 enum 變體平移 bincode discriminant。本計劃 **明確允許** `PROTOCOL_VERSION` 升至 **20**。舊 client 握手失敗是預期。

## 前置

無。本計劃是本波唯一 handshake bump。後續計劃不得再改 `Packet` 變體順序。

## 精確 acceptance

- [ ] `PROTOCOL_VERSION == 20`；handshake 拒絕 v19。
- [ ] 上列 leftover inbound 變體與 `BlockActionResult` 從 `Packet`／`HostToServer`／`ClientToGame`／`NetworkInbound` 消失。
- [ ] Ingress 不再有「legacy drop」catch-all；未知／非該方向的 decode 失敗即斷線。
- [ ] **保留** server→client `BlockChange` 投影與 server→client `ContainerClose`。
- [ ] 刪 `ChunkAck` 與三個 `Catchup*` 通道；client 不再 ack chunk；`ServerRuntime` 不再 ignore 這些事件。
- [ ] 刪 `state.rs` 三個 catch-up 死常數。
- [ ] `leftover_inbound_packets_are_dropped_not_wrapped` 與 round-trip 測試改寫或刪除；對抗性手組 frame 測試仍手組。
- [ ] `ARCHITECTURE.md` 改寫 v20：不再描述 decode-then-drop leftover。

## 預計檔案與測試

- `src/network/protocol.rs`、`channels.rs`、`ingress.rs`、`egress.rs`、`client.rs`、`server.rs`
- `src/server_runtime/ingress.rs`、`src/presentation/network_inbound.rs`、`network_event.rs`、`src/state.rs`
- 驗證：`cargo test --lib network::`；`tests/review_hardening_ingress.rs`；`tests/review_hardening_adversarial_frames.rs`；`tests/plan30_real_transport_acceptance.rs`

## 建議階段

1. 列出每個目標變體的 live caller（必須為零，或僅測試）。
2. 先刪 `BlockActionResult` 鏈（無 live producer）。
3. 刪 leftover inbound 變體與 ingress catch-all。
4. 刪 catch-up 封包／通道／client ack／死常數。
5. bump v20，更新 ARCHITECTURE 與握手測試。

## 不在本計劃

- 刪 `Container { action: 1 }` wire gap。
- 刪 `Action::Use`。
- 刪 live `ContainerOpenResult`／`ContainerClickResult`／`ContainerSlotUpdate`。
- 把 `HostToServer` 整層改成直接 enqueue `Packet`。
- 簡化 `Packet::protocol_version()` 的 exhaustive match（可順便用 helper，但不是目標）。
