# Plan07 — 一種投影事件：`HostToServer`＋`RuntimePresentationEvent` → `Packet`

## 定位

一則 server→client gameplay 事件目前在 server 半邊被型別化三次：

| 層 | 位置 | 規模 |
| :--- | :--- | :--- |
| `RuntimePresentationEvent`（embedded 路徑） | `server_runtime.rs` 216–346 | 19 變體，欄位與 `Packet` 相同，只多 `target` |
| `HostToServer`（TCP 路徑） | `network/channels.rs` 107–293 | 28 變體，欄位與 `Packet` 相同，只多 `to` |
| `handle_host_command`：`HostToServer` → `Packet` | `network/egress.rs` 36–550 | ~515 行 1:1 match（37 個 `HostToServer::` 臂） |
| `ServerRuntime::send_targeted` | `projection.rs` 187–199 | 每次呼叫要寫**兩個 closure** 建同一 payload；15 個 call site（container open／click／close／slot、gameplay response、respawn、BE delta、entity spawn／state／despawn、session、effects、chunk、block change、world-rules／time／pose／dimension） |

grep：`HostToServer::` 78 hits、`RuntimePresentationEvent::` 33 hits。Live `ServerRuntime` 從不 emit `BroadcastSleepStateSync`／`BroadcastPlayerHealth`（只有 egress 臂與 `network/client.rs` 測試）。

Embedded 與 TCP 共用同一個 authority；只有最後一跳不同（`push_presentation_event` vs `enqueue_host`）。

## 前置

09 波 01（v20；`Packet` 變體順序定案後再以它為唯一 schema）。

## 精確 acceptance

- [ ] 新型別 `ProjectionEvent { dest: ProjectionDest, packet: Packet }`（`dest` = `Session(PlayerId)` / `Broadcast` / `AllExcept(PlayerId)` 等現有語意），或直接 `(Option<PlayerId>, Packet)`。
- [ ] `RuntimePresentationEvent` 與 `HostToServer` 的 gameplay 變體刪除；只保留控制訊號（`Stop`／disconnect／kick）在一個小 enum。
- [ ] `send_targeted` 收成一個建構子；15 個 call site 每處只建一次 `Packet`。
- [ ] `egress.rs` 的 1:1 match 刪除；mailbox 分類（pose／latest-state／chunk）改為對 `Packet` 變體的小 classifier，不是第二 schema。
- [ ] embedded presentation 收的是同一個 `Packet`（Plan 08／15 再往下收）；`project_runtime_presentation_event` 對應縮小。
- [ ] wire 不變（`Packet` 變體不動，handshake 版號不動）。
- [ ] `ARCHITECTURE.md` Network 段改寫：runtime 直接產生 `Packet` 形狀的投影事件。

## 預計檔案與測試

- 改：`src/server_runtime.rs`、`src/server_runtime/{projection,ingress}.rs`、`src/network/{channels,egress,server}.rs`、`src/presentation/embedded_runtime.rs`、`src/state.rs`（`project_runtime_presentation_event`）
- 驗證：`cargo test --lib network:: server_runtime::`；`tests/plan30_real_transport_acceptance.rs`；`tests/review_hardening_join_projection.rs`；`tests/review_hardening_embedded_presentation.rs`；`tests/runtime_topology_parity.rs`

## 建議階段

1. 引入 `ProjectionEvent`；`send_targeted` 先同時輸出新舊型別（過渡一個 commit）。
2. 逐個 call site 改成只建 `Packet`。
3. 刪 `HostToServer` gameplay 變體與 egress match；mailbox classifier 改對 `Packet`。
4. 刪 `RuntimePresentationEvent` gameplay 變體。
5. 更新 ARCHITECTURE。

## 不在本計劃

- join 半邊 `ClientToGame` → `NetworkInbound`（Plan 08）。
- embedded `Arc<Chunk>` 零拷貝（Plan 15）。
- `Packet` 每變體 `protocol_version` 剝除（Plan 09）。
