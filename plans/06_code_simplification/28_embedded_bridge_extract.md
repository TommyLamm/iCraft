# Plan28 — `EmbeddedRuntimeBridge` 與 inbound 拆檔

## 定位

Plan 15 刻意留下兩塊在 `state.rs`：

| 符號 | 約略行 | 為什麼 15 不搬 |
| --- | --- | --- |
| `EmbeddedRuntimeBridge` | `state.rs` 2577–2792 | 不要跟 leftover 互動綁在一起 |
| `handle_single_network_event` | `state.rs` 6732 起整段 match | 碰大量 private 欄位 |

`network_inbound.rs` 已有 `NetworkInbound`／drain。`handle_single_network_event`
仍是 `State` 上的巨型 match。Bridge 是活路徑唯一 in-process 入口，卻埋在
16k 行檔案中間。

沿用 10／15 的 `#[path]` 模式（要碰 `State` 私有欄位）。成功標準是檔案邊界，
不是刪欄位、不是改 FIFO／revision 閘。

## 前置

15 已完成。

**建議** 21 已合併：leftover 不再跟活 `impl State` 搶同一個檔，搬 bridge 比較乾淨。
**建議** 25 已合併：inbound 裡的 `is_authoritative()` 已收成拓撲謂詞。

不要跟 21／25 同時改 `state.rs`。

## 精確 acceptance

- [ ] 新增 `src/presentation/embedded_runtime.rs`（或同等），以 `#[path]` 掛在
      `state` 下。整段搬走 `EmbeddedRuntimeBridge` 與其 `impl`（`new`、`queue_request`、
      `queue_position`、`tick`、`sync_local_inventory`、`save_all`／`shutdown`）。
- [ ] 新增 `src/presentation/network_event.rs`（或同等），以 `#[path]` 掛在 `state`
      下。搬走 `handle_single_network_event` 全文。`State::update` 的 drain 呼叫點
      只留一行。
- [ ] 這兩個檔**不是** `presentation/mod.rs` 的 `pub(crate) mod`（與 leftover 相同：
      它們是 `state` 的孩子，避免把 private 欄位改 `pub`）。
- [ ] `State` 欄位留在原 struct。不得開始拆 300 欄位。
- [ ] Bridge 語意不變：Join 仍拒絕 `new`；request id／sequence 仍由 bridge 分配；
      `sync_local_inventory` 仍只寫 inventory／cursor／hotbar。
- [ ] inbound `GameplayRequest` 臂仍是 no-op（embedded listen 由 `ServerRuntime`
      擁有）。不得把它接回第二條權威路徑。
- [ ] 不得把 leftover 方法搬進這兩個檔。
- [ ] `cargo check --bin icraft`；desktop 既有 `state` 單元測與 embedded
      presentation 通過。

## 預計檔案與測試

- 新增：`src/presentation/embedded_runtime.rs`、`src/presentation/network_event.rs`。
- 修改：`src/state.rs`（`#[path]` + 呼叫點）、`src/presentation/mod.rs` 註解。
- 測試：
  - `cargo check --bin icraft`
  - `cargo check --bin icraft-server`（仍不得編這兩個檔，除非走 `state`）
  - `cargo test --bin icraft interpolation_midpoint_and_clamps gpu_timestamp_state_tests -- --test-threads=1`
  - `cargo test --bin icraft embedded_runtime_uses_world_player_profile_and_fifo_ack embedded_runtime_poses_use_monotonic_sender_time -- --test-threads=1`
  - `cargo test --test review_hardening_embedded_presentation -- --test-threads=1`
  - `cargo test --test review_hardening_join_projection -- --test-threads=1`

## 建議階段

1. 先搬 `EmbeddedRuntimeBridge`（型別邊界清楚）。`cargo check --bin icraft`。
2. 再搬 `handle_single_network_event`。每搬完確認 Join health／entity 臂仍編譯。
3. 跑 embedded／join 窄測試。證據列出未搬的 leftover（若 21 已 cfg，寫明）。

## 不在本計劃

- 刪 `LegacyOwner`。
- 拆 `menu.rs`。
- 把 Bridge 改成 `presentation/mod.rs` 的獨立模組（那要公開 `State` 欄位）。
- 重排 drain／tick／render 順序。

## 實作與證據

### 1. 搬移項目與代碼結構
- 新建 `src/presentation/embedded_runtime.rs`：完整承載 `EmbeddedRuntimeBridge` 定義與其實作方法（`new`, `session_id`, `session_game_mode`, `topology`, `revision_for_dimension`, `queue_request`, `queue_position`, `tick`, `save_all`, `shutdown`, `set_session_dimension`, `sync_local_inventory`）。
- 新建 `src/presentation/network_event.rs`：完整承載 `State::handle_single_network_event` 的 `NetworkInbound` 事件分發邏輯（含狀態更新、連線/斷線、玩家進出與位置校驗、區塊與實體投影同步、容器互動等）。
- 修改 `src/state.rs`：透過 `#[path = "presentation/embedded_runtime.rs"] mod embedded_runtime;` 與 `#[path = "presentation/network_event.rs"] mod network_event;` 引用子模組，使行為與欄位可見性完全一致，無任何行為變更。
- 更新 `src/presentation/mod.rs`：補充模組說明註解，明確標示 `embedded_runtime.rs` 與 `network_event.rs` 屬於 `state` 的 `#[path]` 子模組。

### 2. 測試驗證證據
- `cargo check --bin icraft`：編譯通過。
- `cargo check --bin icraft-server`：編譯通過（不依賴 desktop presentation）。
- `cargo test --bin icraft embedded_runtime -- --test-threads=1`：2 passed（`embedded_runtime_poses_use_monotonic_sender_time`, `embedded_runtime_uses_world_player_profile_and_fifo_ack`）。
- `cargo test --bin icraft interpolation_midpoint_and_clamps -- --test-threads=1`：1 passed（`interpolation_midpoint_and_clamps`）。
- `cargo test --bin icraft gpu_timestamp_state_tests -- --test-threads=1`：3 passed（`gpu_timestamp_state_tests`）。
- `cargo test --test review_hardening_embedded_presentation -- --test-threads=1`：4 passed（`embedded_gate_sends_container_op_and_rejects_world_mutations`, `embedded_player_inventory_writeback_is_the_only_exception`, `join_client_never_calls_inventory_writeback`, `no_runtime_must_not_invoke_writeback`）。
- `cargo test --test review_hardening_join_projection -- --test-threads=1`：3 passed（`chunk_data_inserts_column_matching_payload_without_prior_worldgen`, `join_client_must_not_mutate_presentation_chunks`, `schedule_chunk_load_does_not_insert_generated_column_for_join_client`）。

