# Plan17 — leftover 進程內通道縮小

## 定位

08 停了 **新的** leftover 方塊／action 送出，並把三份適配器收成 `protocol::wrap_legacy`。
Wire 上的 leftover `Packet` variant **必須留下**（bincode discriminant）。

08 自己留下的進程內缺口（掃描當日）：

| 送出點 | 現況 | 為什麼 08 沒改 |
| --- | --- | --- |
| `NetworkHandle::send_sleep_request`（`network_inbound.rs:1458`） | 仍發 `GameToClient::SleepRequest` | 當時沒有 dimension；亂填 0 會改地獄／終界睡眠。client send loop 編成 `Packet::GameplayRequest` 並蓋 `current_dimension` |
| `state.rs` 容器 click／close（~16160、~17060） | 仍發 `GameToClient::ContainerClickRequest`／`ContainerClose` | 08 禁止改 `handle_click`；06 已抽政策，但 egress 型別沒改 |
| `GameToClient::RequestBlockChange` | client send loop 與 **單元測** 仍接受 | 生產 desktop 08 已改走 `GameplayRequest` |
| `ServerToHost::ClientSleepRequest`／`Container*` | TCP 不再產生；leftover presentation + `ServerRuntime::handle_event` 仍處理 | 08 不塌通道 |

活契約已經是「一張 `GameplayRequest`」。本計劃把 **desktop／Join 新送出** 收成信封，
並在確認沒有生產呼叫後刪進程內 leftover **送出** 臂。不刪 `Packet` variant。

## 前置

- 08 已完成：`wrap_legacy` 存在；`Action::Use` → `None`；reserved `Packet` 已加註。
- 06 已完成：不要重寫 click 命中邏輯，只改送出型別。
- **建議** 15 已合併：leftover `handle_click` 已不在 `state.rs` 主檔，比較好搜生產送出點。
  未合併也能做，但必須把 Join 活路徑與 `legacy_handle_click` 分開列在證據裡。

不要跟 20（`network/server.rs` 拆檔）搶同一段 match。

## 精確 acceptance

- [x] Join／desktop **新送出**的睡眠、容器 click、容器 close 只走 `GameToClient::GameplayRequest`
      （或 embedded 的 `RuntimeInput::submit_request`）。內部用既有 `wrap_legacy` 或直接組
      `GameplayOperation::Sleep`／`ContainerClick`／`Container`。
- [x] 睡眠請求必須帶 **正確維度**（session／`current_dimension`），不得寫死 0。
      既有「client send loop 蓋 dimension」的語意要保持：玩家在哪個維度睡，信封就是哪個維度。
- [x] 容器 leftover 的 `dragged.is_some() || !is_left` 切分仍只存在 `wrap_legacy`，bit-identical。
- [x] `Action::Use` 仍不合成 `BlockUse`。
- [x] `Packet::{SleepRequest, ContainerClickRequest, ContainerClose, BlockChange, BlockActionRequest, …}`
      **留在 enum**。client／server **仍接受** leftover inbound（舊客戶端／單元測會發）。
- [x] `Packet::{OpenTradeWindow, ExecuteTrade*, CloseTradeWindow, RaidStatusSync}` 不碰。
- [x] 生產碼不再 **建構** `GameToClient::{SleepRequest, ContainerClickRequest, ContainerClose, RequestBlockChange, RequestBlockAction}`。
      單元測若仍要測 inbound-compat，改從測試 socket 送 leftover `Packet`，或把那些測留在
      `#[cfg(test)]` 並在證據列出。
- [x] 不得把 `HostToServer` 塌成 `Packet`。不得 protocol bump。
- [x] `wrap_legacy_use_stays_none_and_container_split_is_bit_identical` 與
      `legacy_sleep_and_container_fields_are_preserved_in_envelopes`（或同等 inbound 測）仍過。
- [x] Plan30–34、container conservation、adversarial、ingress 期望值不變。

## 預計檔案與測試

- 修改：`src/presentation/network_inbound.rs`、`src/state.rs`（只動送出）、
  必要時 `src/network/client.rs` 的 send loop（可刪已無生產發送者的 leftover 臂，**recv** leftover `Packet` 必須留）。
- 測試：
  - `cargo test --lib network::`
  - `cargo test --test plan30_real_transport_acceptance -- --test-threads=1`
  - `cargo test --test plan33_tcp_fishing_lifecycle -- --test-threads=1`
  - `cargo test --test plan34_container_break_inventory_conservation -- --test-threads=1`
  - `cargo test --test review_hardening_container_click -- --test-threads=1`
  - `cargo test --test review_hardening_block_use_rejected -- --test-threads=1`
  - `cargo test --test review_hardening_adversarial_frames -- --test-threads=1`
  - `cargo check --all-targets`

## 建議階段

1. 列出所有 `GameToClient::SleepRequest`／`Container*`／`RequestBlock*` 的 **生產** 建構點（排除 `#[cfg(test)]`）。
2. 睡眠改 `GameplayRequest`，先跑 Plan33／sleep 相關。確認維度不是 0。
3. 容器 click／close 改信封。跑 Plan34 與 container_click。
4. 刪已無呼叫的生產送出臂。recv／`wrap_legacy`／reserved `Packet` 留下。
5. 跑 TCP 矩陣。

## 不在本計劃

- 刪或重排任何 `Packet`／`GameplayOperation` variant（含 `BlockUse`）。
- 把 `HostToServer`／`ServerToHost` 整包改成傳 `Packet`。
- 刪 `ServerRuntime::handle_event` 的 leftover `Client*` 臂——只要 leftover presentation 或測試還會 `try_send` 它們。本計劃只停 desktop **新送出**。若 grep 證明零生產建構，可在證據裡列「可另案刪 handle_event 臂」，本計劃不刪。
- 改 ingress 預算或 reliable enqueue 視窗。
- 拆 `server.rs`（20）。

## 實作與證據

執行前列出生產 leftover `GameToClient` 建構點（排除 `#[cfg(test)]`）：

| 位置 | 之前 | 之後 |
| --- | --- | --- |
| `NetworkHandle::send_sleep_request`（`network_inbound.rs`） | `GameToClient::SleepRequest { x, y, z }`（無維度） | **刪除 helper**。唯一呼叫在 leftover `legacy_handle_click` |
| leftover `legacy_handle_click` 床（`legacy_interaction.rs`） | 經 `send_sleep_request` 發 leftover | `submit_local_authority_operation(Sleep)` → Join 走 `request_gameplay(GameplayRequest)`，維度 = `self.current_dimension as u8` |
| Join 活路徑 `handle_join_world_click` Sleep | 已是 `submit_local_authority_operation(Sleep)` | 未改 |
| Embedded 活路徑 `handle_authority_click` Sleep | 已是 `submit_authority_request` + `current_dimension` | 未改 |
| `legacy_apply_inventory_ui_hit` 容器 click（`state.rs`） | `GameToClient::ContainerClickRequest` | `wrap_legacy(ContainerClick)` + `network.request_gameplay`；維度／revision 仍是 `current_dimension` 與 block-entity revision |
| Join／Embedded 活路徑容器 click | 已是 `submit_inventory_container_click` → `GameplayOperation::ContainerClick` | 未改 |
| leftover `close_inventory` Client 臂（`state.rs`） | `GameToClient::ContainerClose` | `wrap_legacy(ContainerClose)` + `request_gameplay`；維度 = `current_dimension` |
| Join 活路徑 close | `!is_authoritative()` 已走 `submit_local_authority_operation(Container { Close })` 並 return | 未改 |
| `request_block_change`／`request_block_action` | Plan 08 已是 `wrap_legacy` + `GameplayRequest` | 未改 |

睡眠維度：

- leftover／Join 新送出都經 `submit_local_authority_operation` 或既有 `submit_authority_request`，信封 `dimension` 是 `self.current_dimension as u8`，不是寫死 0。
- `prepare_gameplay_request` **不蓋** dimension。client send loop 對 leftover `SleepRequest` 臂仍用自己的 `current_dimension`（inbound-compat）；生產碼不再走該臂。
- 玩家在哪個維度睡，信封就是哪個維度。

刪／留：

- **刪**：`NetworkHandle::send_sleep_request`（已無生產呼叫）。
- **留 enum／recv**：`GameToClient::{SleepRequest, ContainerClickRequest, ContainerClose, RequestBlockChange, RequestBlockAction}` 仍在 enum；client send loop leftover 臂仍接受（給 `#[cfg(test)]`）。
- **留 Packet inbound**：`Packet::{SleepRequest, ContainerClickRequest, ContainerClose, BlockChange, BlockActionRequest, …}` 未刪未重排。`legacy_sleep_and_container_fields_are_preserved_in_envelopes` 仍從測試 socket 送 leftover `Packet`。
- **留 reserved**：`OpenTradeWindow`／`ExecuteTrade*`／`CloseTradeWindow`／`RaidStatusSync` 未碰。
- **留 handle_event**：`ServerRuntime::handle_event` 的 `ClientSleepRequest`／`Container*`／`ClientBlock*` 臂未刪。TCP leftover inbound 08 已改 `wrap_legacy` → `GameplayRequest`，生產碼不再建構那些 `ServerToHost` leftover 變體（單元測 `host_inbound_block_request_preserves_authenticated_player_id` 仍 `send(ClientBlockChange)`）。**可另案刪 handle_event 臂**。
- `HostToServer` 未塌成 `Packet`。無 protocol bump。
- `wrap_legacy` 仍是 `dragged.is_some() || !is_left` 唯一切分點；`Action::Use` 仍 `None`。

`#[cfg(test)]` leftover `GameToClient` 建構（inbound-compat，未改期望）：

- `network::client::tests::legacy_client_inputs_are_single_gameplay_envelopes` 仍 `send(RequestBlockChange / RequestBlockAction / SleepRequest / ContainerClickRequest)`。

測試：

- `cargo test --lib network:: -- --test-threads=1` → 101 passed（含 `wrap_legacy_use_stays_none_and_container_split_is_bit_identical`、`legacy_sleep_and_container_fields_are_preserved_in_envelopes`）
- `cargo test --test plan30_real_transport_acceptance -- --test-threads=1` → 2 passed
- `cargo test --test plan33_tcp_fishing_lifecycle -- --test-threads=1` → 3 passed
- `cargo test --test plan34_container_break_inventory_conservation -- --test-threads=1` → 4 passed
- `cargo test --test review_hardening_container_click -- --test-threads=1` → 9 passed
- `cargo test --test review_hardening_block_use_rejected -- --test-threads=1` → 3 passed
- `cargo test --test review_hardening_adversarial_frames -- --test-threads=1` → 2 passed
- `cargo check --all-targets` → ok

缺口：

- leftover `GameToClient` send-loop 臂與 enum variant 仍在（測試 inbound-compat）。
- `ServerRuntime::handle_event` leftover `Client*` 臂仍在；可另案刪。
- `ContainerOpenRequest` 進程內 variant 不在本計劃範圍。
