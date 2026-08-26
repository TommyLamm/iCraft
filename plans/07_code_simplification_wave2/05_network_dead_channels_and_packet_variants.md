# Plan05 — 刪除網路 dead 通道變體與未使用的 Packet 變體

## 定位

在引入統一的 `GameplayRequest`（帶 sequence、dimension、revision 與 typed `GameplayOperation`）後，客戶端所有方塊變更、方塊操作、睡覺、容器點擊/開關均已由 `network/ingress.rs` 統一包裝為 `ServerToHost::GameplayRequest` 發送。

然而，`ServerToHost` 枚舉與伺服器 runtime 處理層依然殘留了 6 個永不被產生的死變體及 150+ 行分支代碼：
1. `src/network/channels.rs`:
   - `ClientBlockChange`
   - `ClientBlockAction`
   - `ClientSleepRequest`
   - `ContainerOpenRequest`
   - `ContainerClickRequest`
   - `ContainerClose`
2. `src/server_runtime/ingress.rs:L51-58, L126-269`:
   - 對上述 6 個死變體的冗餘分派與處理代碼（150+ 行）。
3. `src/network/protocol.rs:L1590-1626`:
   - `Packet` 枚舉中明確標註為 `v19 reserved / unused` 的 5 個交易與突襲變體（`OpenTradeWindow`, `ExecuteTradeRequest`, `ExecuteTradeResult`, `CloseTradeWindow`, `RaidStatusSync`）。

刪除這些死變體與分支可直接減少 ~375 行死代碼，同時消除維護舊通道變體的心理負擔。

## 前置

無。可與 01–04、06 並行。

## 精確 acceptance

- [x] 從 `src/network/channels.rs` 的 `ServerToHost` 枚舉中刪除上述 6 個未使用的變體。
- [x] 從 `src/server_runtime/ingress.rs` 中刪除處理這 6 個變體的全部 `match` 分支及輔助方法。
- [x] 從 `src/network/protocol.rs` 的 `Packet` 枚舉中安全刪除未分配/未使用的交易與突襲變體（不影響其他 wire 變體的解析）。
- [x] `cargo check --all-targets` 通過。
- [x] 所有網路整合測試（`tests/review_hardening_network_ingress.rs`、`tests/plan30_real_transport_acceptance.rs` 等）全部通過。

## 預計檔案與測試

- 修改：
  - `src/network/channels.rs`
  - `src/server_runtime/ingress.rs`
  - `src/network/protocol.rs`
  - `src/network/server.rs`
  - `src/presentation/network_event.rs`
  - `src/presentation/network_inbound.rs`
- 驗證測試：
  - `cargo test --lib network::`
  - `cargo test --lib server_runtime::`
  - `cargo test --test review_hardening_ingress -- --test-threads=1`
  - `cargo test --test plan30_real_transport_acceptance -- --test-threads=1`
  - `cargo check --all-targets`

## 建議階段

1. 檢查 `ServerToHost` 在 `network/ingress.rs`、`server_runtime/` 中的全部使用點，確認無其他地方產生這 6 個變體。
2. 刪除 `ServerToHost` 中的 6 個死變體。
3. 刪除 `server_runtime/ingress.rs` 中的對應 `match` 分支。
4. 清理 `protocol.rs` 中未使用的 Packet 變體。
5. 運行網路整合測試套件驗證。

## 不在本計劃

- 變更任何現有 `GameplayRequest` / `GameplayOperation` 的 wire 格式。
- 修改 `Packet` 的 2 MiB 長度限制或 bincode decode 策略。

## 實作與證據

### 修改內容
1. **`src/network/channels.rs`**：
   - 從 `ServerToHost` 枚舉中刪除 6 個未使用的死變體：`ClientBlockChange`、`ClientBlockAction`、`ClientSleepRequest`、`ContainerOpenRequest`、`ContainerClickRequest`、`ContainerClose`。
2. **`src/server_runtime/ingress.rs`**：
   - 刪除 `handle_event` 中上述 6 個死變體的全部 `match` 分支。
   - 刪除不再使用的輔助方法：`handle_block_change`、`legacy_request`、`send_legacy_rejection`、`session_request_id`。
3. **`src/network/protocol.rs`**：
   - 從 `Packet` 枚舉及其 `protocol_version()` 匹配中刪除未使用的交易與突襲變體：`OpenTradeWindow`、`ExecuteTradeRequest`、`ExecuteTradeResult`、`CloseTradeWindow`、`RaidStatusSync`。
4. **`src/network/server.rs`**：
   - 更新單元測試中對 `ServerToHost` 事件類型的斷言匹配。
5. **`src/presentation/network_event.rs` & `src/presentation/network_inbound.rs`**：
   - 清理表現層對應的死事件變體分派與轉換。

### 驗證證據
- `cargo test --lib network::` (101 passed; 0 failed)
- `cargo test --lib server_runtime::` (26 passed; 0 failed)
- `cargo test --test review_hardening_ingress -- --test-threads=1` (2 passed; 0 failed)
- `cargo test --test plan30_real_transport_acceptance -- --test-threads=1` (2 passed; 0 failed)
- `cargo check --all-targets` (通過)

