# Plan27 — `ServerRuntime` 投影／ingress 拆檔

## 定位

`src/server_runtime.rs` 約 4,622 行。`session_sync.rs` 已抽出（16）。剩下合成根
仍同時擁有建構、tick 編排、inbound `ServerToHost`、以及 interest 投影：

| 約略行 | 內容 |
| --- | --- |
| 878–1070 | `ServerRuntime` 欄位、`new`／`new_embedded`／`construct` |
| 1071–1492 | `tick_with_output` 編排 + dedicated console |
| 1493–2864 | `handle_event`（ingress） |
| 2873– | `route_authority_snapshot`（投影） |

`tick_with_output` 必須留在根檔當短編排器（drain → `authority.tick` → transfer →
route → evict → metrics → autosave）。不要把 tick 順序打散到三個檔各寫一段。

## 前置

16、17 已完成。可與 26、29、30 並行。不要跟 16 再改 `session_sync.rs` 的寫入口。
不要跟 20 搶 `network/server.rs`（20 已完成；本計劃只動 runtime 側）。

## 精確 acceptance

- [x] 至少拆出（名稱可微調，責任不可混）：
      - `src/server_runtime/ingress.rs`：`handle_event` 與 join／leave／pose／
        gameplay 入隊的私有 helper
      - `src/server_runtime/projection.rs`：`route_authority_snapshot` 與
        interest-routed fanout
- [x] `tick_with_output` 留在 `server_runtime.rs`，仍依序：
      bounded drain → `authority.tick` → dimension transfer → route →
      evict uninteresting → container closures → metrics → autosave。
      不得重排，不得把 autosave 失敗改成中止 tick。
- [x] 公開方法留在 `ServerRuntime`。子檔是 `impl ServerRuntime`，不是新 runtime 型別。
- [x] 不得把 `InterestSet` 搬進 `AuthorityCore`。不得合併兩個 session 型別。
- [x] pose／chat／gameplay ingress 預算與 `QueueFull` 語意不變。
- [x] reliable 投影 250 ms 視窗與驅逐不變。
- [x] 既有測試期望值不變。

## 預計檔案與測試

- 新增：`src/server_runtime/ingress.rs`、`src/server_runtime/projection.rs`。
- 修改：`src/server_runtime.rs`（`mod` + 短 `tick_with_output`）。
- 測試：
  - `cargo test --lib server_runtime::`
  - `cargo test --test review_hardening_session_lifecycle -- --test-threads=1`
  - `cargo test --test review_hardening_ingress -- --test-threads=1`
  - `cargo test --test review_hardening_chunk_residency -- --test-threads=1`
  - `cargo test --test runtime_topology_parity -- --test-threads=1`
  - `cargo test --test headless_server_authority -- --test-threads=1`

## 建議階段

1. 搬 `handle_event` 整段到 `ingress.rs`。跑 session lifecycle 與 ingress。
2. 搬 `route_authority_snapshot`。跑 residency 與 topology parity。
3. 確認根檔 `tick_with_output` 仍是短編排器。

## 不在本計劃

- 塌縮 `HostToServer` 成 `Packet`。
- 統一三套指令語言。
- 把 dedicated console 改走 `commands::parse`。
- leftover cfg、desktop `State` 拆檔。

## 實作與證據

### 改動摘要

1. **新增 `src/server_runtime/ingress.rs`**：
   - 抽出 `ServerRuntime::handle_event`、`handle_join`、`handle_join_with_storage`、`handle_leave`、`handle_position`、`handle_block_change`、`handle_gameplay_request`。
   - 抽出 session 請求輔助：`legacy_request`、`send_legacy_rejection`、`session_revision`、`session_request_id`、`default_player_payload`。
   - 抽出公用 session 請求與傳送入口：`submit_request`、`login_session`、`logout_session`、`set_session_dimension`、`transfer_session_dimension`、`apply_authority_dimension_transfer`、`teleport_session`。
   - 抽出 transport 發送與指標同調輔助：`enqueue_host`、`enqueue_stop`、`sync_network_metrics`。

2. **新增 `src/server_runtime/projection.rs`**：
   - 抽出 `ServerRuntime::route_authority_snapshot`、`route_container_result`、`drain_initial_chunk_projections`。
   - 抽出 interest set 相關更新與查詢：`update_interest`、`update_interest_for`、`update_interest_for_at`、`record_interest_update`、`queue_interest_update`、`queue_block_change`。
   - 抽出各類投影推播 helper：`send_response`、`send_respawn_result`、`send_block_entity_delta`、`send_entity_spawn`、`send_entity_state`、`send_entity_despawn`、`send_session_update`、`send_player_effects`、`send_container_slot_update`、`send_container_close`、`close_runtime_container`、`force_close_player_containers`、`send_chunk_projection`、`push_presentation_event`、`drain_routed_updates`。
   - 抽出 `entity_state_wire` 輔助函式。

3. **修改 `src/server_runtime.rs`**：
   - 宣告 `mod ingress;` 與 `mod projection;`。
   - 保留簡潔的 `tick_with_output` 編排器（bounded drain → `authority.tick` → dimension transfer → route → evict uninteresting → container closures → metrics → autosave）。
   - 保留型別定義（`ServerRuntime`、`PlayerSessionState`、`ServerProperties`、`ServerMetrics`、`RuntimeInput`、`RuntimePresentationEvent` 等）、建構／生命週期／存檔／console 指令邏輯與單元測試。

### 驗證證據

- `cargo check --all-targets` 通過。
- `cargo test --lib server_runtime::`（26 passed; 0 failed）。
- `cargo test --test review_hardening_session_lifecycle -- --test-threads=1`（3 passed; 0 failed）。
- `cargo test --test review_hardening_ingress -- --test-threads=1`（2 passed; 0 failed）。
- `cargo test --test review_hardening_chunk_residency -- --test-threads=1`（4 passed; 0 failed）。
- `cargo test --test runtime_topology_parity -- --test-threads=1`（6 passed; 0 failed）。
- `cargo test --test headless_server_authority -- --test-threads=1`（2 passed; 0 failed）。

### 留下的缺口

- 無（本計劃 scope 完全閉環，未動 wire enum 或改變任何既有行為）。

