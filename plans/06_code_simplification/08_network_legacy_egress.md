# Plan08 — 網路 leftover 只收不發

## 定位

真契約是「一張 `GameplayRequest` 信封」。現役仍有五層平行型別：`Packet`、`ServerToHost`、`HostToServer`、`ClientToGame`、`GameToClient`。同一欄位加一次要改五處。

更危險的是 **egress 還在發 leftover，ingress 再包回 `GameplayRequest`**：

- `src/network/client.rs` `legacy_gameplay_request` ~647；send loop ~1159 把 `RequestBlockChange` 編成 `BlockUse`，把 `RequestBlockAction` 編成 typed op
- `src/network/server.rs` `legacy_gameplay_request` ~1431；recv 仍接受舊 `ContainerClick`／`BlockChange` 再包
- `src/server_runtime.rs` `legacy_request` ~2784；`handle_event` 對 block action／sleep／container 各包一次
- `src/state.rs` `NetworkHandle::request_block_change`／`request_block_action` ~5164

`GameplayOperation::from_legacy_block_action` 對 `Action::Use` 回 `None`——這必須保留。

未使用但佔 discriminant 的 `Packet` variant（`OpenTradeWindow`、`ExecuteTradeRequest`、`ExecuteTradeResult`、`CloseTradeWindow`、`RaidStatusSync`）只出現在 `protocol.rs`。client recv 對未知包是 `Ok(_) => {}`。**刪或重排會弄壞 bincode，本計劃禁止。**

`NetworkClient` send 路徑對每個失敗都複製 `eprintln!` + `Disconnected { "connection lost" }` + `return`（~1141–1338）。

## 前置

無。不要跟 03 搶刪 `handle_gameplay_request` 的 BlockUse arm；03 管權威拒絕，本計劃管「桌面還發不發 leftover」。

## 精確 acceptance

- [x] Desktop／Join **新送出**的方塊／容器／睡眠／戰鬥請求只走 `GameToClient::GameplayRequest`（或等價、已經是 typed `GameplayRequest` 的通道）。`State::request_block_change`／`request_block_action` 若還存在，必須在內部改建成 `GameplayRequest`，不得再送 `RequestBlockChange`。
- [x] 伺服器／client **仍接受** leftover inbound 封包，並包成同一 `GameplayRequest`（測試與舊客戶端仍會發）。三份 `legacy_gameplay_request` 收成 **一個** `fn wrap_legacy(...)`（放 `protocol.rs` 或小模組），欄位填法 bit-identical。
- [x] `Action::Use` 仍然不映射、不合成 `BlockUse`。
- [x] 容器 leftover 的 `dragged.is_some() || !is_left` 切分保持 bit-identical。
- [x] `Packet::{OpenTradeWindow, ExecuteTrade*, CloseTradeWindow, RaidStatusSync}` 留在 enum。可加註「v19 reserved／unused」。不得刪、不得重排、不得改 discriminant。
- [x] `NetworkClient` 送出失敗抽 `send_or_die`（或等價）。斷線 reason 字串保持 `"connection lost"`，除非現有測試鎖的是別的字——以測試為準。
- [x] `ConnectionReader::recv` 的 header／body 填滿可抽 `read_exact_into`，但必須維持「header 與 body 之間可取消」的測試（`recv_survives_cancellation_between_header_and_body`）。
- [x] 不得把 host／client channel 整包改成直接傳 `Packet`（那是後續大 PR）。本計劃只停 **發出** leftover、共用 wrap、抽 send／read helper。
- [x] Plan30–34、adversarial frames、ingress、BlockUse rejected 期望值不變。

## 預計檔案與測試

- 修改：`src/network/protocol.rs`、`src/network/client.rs`、`src/network/server.rs`、`src/network/transport.rs`、`src/state.rs`（只動 `NetworkHandle` 送出 helper）、`src/server_runtime.rs`（改呼叫共用 wrap，不改 join 順序）。
- 測試：
  - `cargo test --lib network::`
  - `cargo test --test plan30_real_transport_acceptance -- --test-threads=1`
  - `cargo test --test plan31_authoritative_block_actions -- --test-threads=1`
  - `cargo test --test review_hardening_block_use_rejected -- --test-threads=1`
  - `cargo test --test review_hardening_adversarial_frames -- --test-threads=1`
  - `cargo test --test review_hardening_ingress -- --test-threads=1`
  - `cargo check --all-targets`

## 建議階段

1. 列出所有 `GameToClient::Request*`／`Packet::BlockChange`／`ContainerClickRequest` 的 **生產送出點**（不是測試）。
2. 抽 `wrap_legacy`，讓三份適配器改呼叫它，先不改誰送什麼。跑 `network::` 單元測。
3. 把 `State`／Join egress 改成只送 `GameplayRequest`。
4. `send_or_die`、`read_exact_into`。
5. reserved packet 加註。
6. 跑 TCP 矩陣。

## 不在本計劃

- 把 `HostToServer`／`ServerToHost` 塌縮成 `Packet`。
- Protocol bump 或刪 variant。
- 改 pose／chat／gameplay ingress 預算（20／8／120）。
- 改 250 ms reliable enqueue 或 256 事件隊列。
- 把 `u8` 的 Container／Fishing／Brew action 改成 enum 欄位（那是 wire 變更）。新建構點可用 `to_wire()`，但本計劃不強制。

## 實作與證據

執行前列出生產 leftover 送出點（測試除外）：

| 位置 | 之前 | 之後 |
| --- | --- | --- |
| `NetworkHandle::request_block_change` | `GameToClient::RequestBlockChange`（零呼叫） | `wrap_legacy(BlockChange)` → `request_gameplay` → `GameToClient::GameplayRequest` |
| `NetworkHandle::request_block_action` | `GameToClient::RequestBlockAction`（零呼叫） | `wrap_legacy(BlockAction)` → `request_gameplay`；`Action::Use` 仍 `None`、不送 |
| `submit_authority_request`（Join 線上入口） | 已是 `GameToClient::GameplayRequest` | 未改 |
| `NetworkHandle::send_sleep_request` | `GameToClient::SleepRequest` | **仍 leftover**。`NetworkHandle` 沒有 dimension；改成 `GameplayRequest{dimension:0}` 會改地獄／終界睡眠。client send loop 仍把它編成 `Packet::GameplayRequest` 並蓋上 `current_dimension` |
| `handle_click` 容器 click／close | `GameToClient::ContainerClickRequest`／`ContainerClose` | **未動**（本計劃禁止改 `handle_click`；06 範圍）。wire 仍是 `Packet::GameplayRequest` |
| `NetworkClient` leftover send loop | leftover `GameToClient` → `Packet::GameplayRequest` | 仍接受 leftover inbound，改走共用 `wrap_legacy` |
| `Packet::BlockChange` 伺服器送出 | 世界投影（不是請求） | 未改 |

改動：

- `protocol.rs` 新增 `LegacyGameplay` + `fn wrap_legacy(...)`。`request_id`／`client_sequence` 保持 0，三份適配器再用各自的 prepare／revision 填法（bit-identical）。`Action::Use` 走既有 `from_legacy_block_action` → `None`。容器 leftover 的 `dragged.is_some() || !is_left` 切分只在這一個函式。單元測 `wrap_legacy_use_stays_none_and_container_split_is_bit_identical` 鎖住 Use=None 與切分。
- `client.rs`／`server.rs`／`server_runtime.rs` 的 leftover 適配器改呼叫 `wrap_legacy`。host leftover `ContainerClickRequest` 仍一律建 `ContainerClick`（原本就沒走切分）。
- `NetworkClient` send 失敗抽 `send_or_die`；reason 仍是 `"connection lost"`。
- `ConnectionReader::recv` 抽 `read_exact_into`；header 與 body 仍是兩個 await，`recv_survives_cancellation_between_header_and_body` 通過。
- reserved `Packet` variant 加註「v19 reserved/unused」；未刪、未重排。
- 沒有把 host／client channel 改成直接傳 `Packet`。沒有 protocol bump。

測試（`CARGO_TARGET_DIR=F:\Desktop\iCraft\target`）：

- `cargo test --lib network::` → 101 passed
- `cargo test --test plan30_real_transport_acceptance -- --test-threads=1` → 2 passed
- `cargo test --test plan31_authoritative_block_actions -- --test-threads=1` → 3 passed
- `cargo test --test review_hardening_block_use_rejected -- --test-threads=1` → 3 passed
- `cargo test --test review_hardening_adversarial_frames -- --test-threads=1` → 2 passed
- `cargo test --test review_hardening_ingress -- --test-threads=1` → 2 passed
- `cargo check --all-targets` → ok

