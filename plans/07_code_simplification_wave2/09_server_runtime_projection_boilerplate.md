# Plan09 — `ServerRuntime` 投射與會話同步樣板提煉

## 定位

在 `src/server_runtime/` 與 `src/authority/` 中，存在多處結構高度重複的樣板代碼：

1. **`src/server_runtime/projection.rs:L144-386` (9 個投射方法)**：
   9 個 `send_entity_*` / `send_chunk_*` / `send_player_*` 方法皆在重複以下 10 行的分發樣板：
   ```rust
   if self.local_session_id == Some(to) {
       self.push_presentation_event(RuntimePresentationEvent::X { ... });
   } else {
       self.enqueue_host(HostToServer::SendX { ... });
   }
   ```
   可提煉出一個簡潔的 `send_targeted` 輔助函式，直接減少 ~120 行樣板。

2. **`ItemStack` 與 `SessionInventorySlot` 轉換重複 4 處**：
   - `authority/dispatch.rs:L1369` (`stack_from_slot`)
   - `authority/dispatch.rs:L1378` (`stack_from_session_inventory`)
   - `server_runtime.rs:L1521` (`stack_from_session_slot`)
   - `server_world.rs:L656`（手寫解構）
   統一為 `SessionInventorySlot::to_stack(&self) -> Option<ItemStack>` 方法。

3. **`src/authority/dispatch.rs:L63-84` 旁觀者模式操作過濾**：
   22 行的冗長 match 語句手寫列出 16 個 variant，可改用 `matches!` 或 `GameplayOperation::is_spectator_blocked()`。

4. **`src/authority/mining.rs:L88-109` 作物掉落物分支**：
   22 行的小麥/胡蘿蔔/馬鈴薯分支可濃縮為 6 行 pattern match。

預期削減代碼 ~210 行。

## 前置

05、06 已完成。

## 精確 acceptance

- [ ] `projection.rs` 提供統一的 `send_targeted` 輔助，簡化 9 個投射發送方法。
- [ ] 在 `SessionInventorySlot` 提供 `to_stack` 方法，並替換 `dispatch.rs`、`server_runtime.rs`、`server_world.rs` 中的 4 處重複解構函式。
- [ ] 濃縮 `dispatch.rs` 中的 Spectator 操作檢查。
- [ ] 濃縮 `mining.rs` 中的作物掉落物分支。
- [ ] `cargo check --all-targets` 通過。
- [ ] 權威投射與交易相關測試全數通過。

## 預計檔案與測試

- 修改：
  - `src/server_runtime/projection.rs`
  - `src/authority/contract.rs`
  - `src/authority/dispatch.rs`
  - `src/authority/mining.rs`
  - `src/server_runtime.rs`
  - `src/server_world.rs`
- 驗證測試：
  - `cargo test --lib server_runtime::`
  - `cargo test --lib authority::`
  - `cargo test --test review_hardening_join_projection -- --test-threads=1`
  - `cargo check --all-targets`

## 建議階段

1. 在 `contract.rs` 為 `SessionInventorySlot` 添加 `pub fn to_stack`。
2. 替換 4 處手寫轉換。
3. 在 `projection.rs` 提取 `send_targeted` 輔助並重構 9 個方法。
4. 濃縮 `dispatch.rs` 與 `mining.rs` 中的 match 分支。
5. 運行整合測試驗證。

## 不在本計劃

- 更改投射到客戶端或表現層的事件結構體定義。
