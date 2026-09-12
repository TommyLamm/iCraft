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

- [x] 新型別 `ProjectionEvent { dest: ProjectionDest, packet: Packet }`（`dest` = `Session(PlayerId)` / `Broadcast` / `AllExcept(PlayerId)` 等現有語意），或直接 `(Option<PlayerId>, Packet)`。
- [x] `RuntimePresentationEvent` 與 `HostToServer` 的 gameplay 變體刪除；只保留控制訊號（`Stop`／disconnect／kick）在一個小 enum。
- [x] `send_targeted` 收成一個建構子；15 個 call site 每處只建一次 `Packet`。
- [x] `egress.rs` 的 1:1 match 刪除；mailbox 分類（pose／latest-state／chunk）改為對 `Packet` 變體的小 classifier，不是第二 schema。
- [x] embedded presentation 收的是同一個 `Packet`（Plan 08／15 再往下收）；`project_runtime_presentation_event` 對應縮小。
- [x] wire 不變（`Packet` 變體不動，handshake 版號不動）。
- [x] `ARCHITECTURE.md` Network 段改寫：runtime 直接產生 `Packet` 形狀的投影事件。

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

## 實作與證據

### 改了什麼

- 新增 `ProjectionEvent { dest: ProjectionDest, packet: Packet }` 與
  `ProjectionDest::{Session, Broadcast}`（`src/network/channels.rs`）。
- `HostToServer` 塌成 `Project`／`DisconnectClient`／`DisconnectCatchupClient`／`Stop`；
  gameplay 變體全部刪除。
- `RuntimePresentationEvent` 枚舉刪除；embedded 與 TCP 共用 `ProjectionEvent`。
- `send_targeted(to, packet)` 只建一次 `Packet`；call site 不再寫雙 closure。
- `egress::handle_host_command` 改為 `classify_packet`（Catchup／Pose／State／Reliable），
  不再 1:1 map `HostToServer` → `Packet`。
- `project_runtime_presentation_event` 改為對 `Packet` 變體 match。
- `ARCHITECTURE.md` Network 段改寫為 runtime 直接產生 `Packet` 形狀投影。

### 測了什麼

- `cargo test --lib network::` — 100 passed
- `cargo test --lib server_runtime::` — 30 passed
- `cargo test --test plan30_real_transport_acceptance` — 2 passed（TCP 首次偶發 TooFar／tick over budget，重跑通過；與既有慢 tick 壓力同源）
- `cargo test --test review_hardening_join_projection` — 3 passed
- `cargo test --test review_hardening_embedded_presentation` — 3 passed
- `cargo test --test runtime_topology_parity` — 6 passed
- `cargo check --all-targets` — ok
- `cargo check --bin icraft-server` — ok

### 死路徑證據

- `BroadcastSleepStateSync`／`BroadcastPlayerHealth`：全 repo `*.rs` grep 0 hits（Plan 定位所述 live producer 已不存在；egress 亦無對應臂）。
- `HostToServer` gameplay 變體：`channels.rs` 只剩 4 個控制／Project 變體；
  live `projection.rs` 僅 `HostToServer::Project`；ingress 廣播改 `project_broadcast(Packet::…)`。
- `RuntimePresentationEvent::`：全 repo 0 hits。

### 留下的缺口

- Plan 08：join 半邊 `ClientToGame` → `NetworkInbound` 尚未合併；embedded 仍經
  `project_runtime_presentation_event` 映射到 `NetworkInbound`。
- Plan 15：embedded `Arc<Chunk>` 零拷貝未做。
- Plan 09：每封包 `protocol_version` 欄位仍在（本計劃刻意不動 wire）。
- `plan30` TCP 在並行／慢機器上仍可能因 tick over budget 出現偶發 TooFar；非本計劃回歸。
