# 13 — 刪未接線村民交易生成與升級算法

狀態：已完成。基線：`83e751d`，2026-09-17。
前置：01 建議先完成（同檔）。

## 定位與判定

village/trade.rs:93–334 generate_offers_for_level 唯一 caller 是同檔 test_offer_generation；VillagerLevel::xp_threshold／next_level（:28/38）僅 test_villager_level_thresholds 使用。可刪約 260 行舊算法。

VillagerLevel／TradeOffer 仍由 entity.rs:331、save/format.rs:427/480/533、state.rs:5317 使用。effective_cost_a／is_out_of_stock 是正式 authority／UI 路徑。

## 實作步驟

1. 刪 generate_offers_for_level、xp_threshold、next_level 和只驗舊算法的兩個測試。
2. 刪失去用途的 VillagerProfession import。
3. TradeOffer::new 若刪 generator 後只服務有效交易測試，改成測試 fixture，再刪正式 constructor；保留交易斷言。
4. 保留 VillagerLevel、TradeOffer 及正式價格／庫存邏輯。
5. price_multiplier 即使 gameplay 未讀，也是 serde 結構欄位，不在本包當純死欄位移除；不改存檔布局。
6. 更新 ARCHITECTURE，避免把未接線 generator 描述為現行玩法。

## 驗證與驗收

`cargo test --lib trade_conserves_items_and_mount_projects_session_state`、`cargo test --lib trade_second_cost_failure_rolls_back_first_cost`、`cargo test --lib test_trade_discount`，以及受影響 save roundtrip。

generator／升級方法消失；正式 trade 庫存守恆、第二成本 rollback 與折扣仍被測試。

## 實作紀錄

- 改動細節：
  1. `src/village/trade.rs`：刪除 `generate_offers_for_level`、`VillagerLevel::xp_threshold`、`VillagerLevel::next_level` 以及舊算法測試 `test_villager_level_thresholds`、`test_offer_generation`。
  2. `src/village/trade.rs`：刪除頂層未使用的 `VillagerProfession` import；將未在生產代碼使用的 `Item` import 移入 `#[cfg(test)] mod tests`。
  3. `src/village/trade.rs`：為 `TradeOffer::new` 標註 `#[cfg(test)]`，從正式 public constructor 收窄為測試 fixture，專供 `authority::tests`、`server_world::tests` 及同檔折扣測試構造交易數據。
  4. 完整保留 `VillagerLevel`（含 `from_u8`）、`TradeOffer`、`effective_cost_a`、`is_out_of_stock` 及 `price_multiplier` 序列化欄位，不破壞存檔與網絡契約。
  5. `ARCHITECTURE.md`：在核心模組說明中補充標註未接線村民交易生成算法與升級門檻已刪除，`TradeOffer` 與 `VillagerLevel` 保持為正式權威契約。
- 實際命令與驗證結果：
  - `cargo test --lib trade_conserves_items_and_mount_projects_session_state`：通過（1 passed; 0 failed）。
  - `cargo test --lib trade_second_cost_failure_rolls_back_first_cost`：通過（1 passed; 0 failed）。
  - `cargo test --lib test_trade_discount`：通過（1 passed; 0 failed）。
  - `cargo test --lib roundtrip`：34 個 roundtrip 測試全數通過（含 `test_entity_save_data_roundtrip` 與 `test_serialization_roundtrips`）。
  - `cargo check --all-targets --all-features`：成功（exit code 0）。
- 淨刪碼量與保留原因：
  - 淨刪碼量：`src/village/trade.rs` 從 370 行精簡為 89 行（刪除 284 行，新增 3 行，淨刪除 281 行）。
  - 保留原因：`VillagerLevel` 與 `TradeOffer` 為 `entity.rs`、`save/format.rs`、`state.rs`、`authority/dispatch` 等活躍資料流核心欄位；`price_multiplier` 為 serde 存檔結構欄位，保留以維持存檔相容性；`effective_cost_a` 與 `is_out_of_stock` 為權威交易扣款與庫存檢查的核心計算。

