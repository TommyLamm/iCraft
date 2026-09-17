# 13 — 刪未接線村民交易生成與升級算法

狀態：待執行。基線：`83e751d`，2026-09-17。
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

尚未執行；完成時記錄實際修改、驗證結果、刪碼量及文件更新。

