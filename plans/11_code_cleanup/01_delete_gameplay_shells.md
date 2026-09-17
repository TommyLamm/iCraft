# 01 — 刪除完整原型與測試專用玩法殼

狀態：待執行。基線：`83e751d`，2026-09-17。
前置：無。

## 定位與判定

直接刪碼。全倉符號搜尋已確認以下型別只在自身原型及其測試使用；不代表正式遊戲相同概念沒有實作。

- 整檔：`src/container_sessions.rs`、`src/vehicle.rs`、`src/rail.rs`、`src/navigation.rs`、`src/village/raid.rs`，共 1,174 行。
- `src/lib.rs:37/75/84` gate container／vehicle／rail；`src/village/mod.rs:2` gate raid。navigation 外部只剩 lib mod 與 main re-export。
- `src/fishing.rs:102–309`：FishingHook／FishingResult／FishingManager，只有 test_fishing_cast_and_reel 使用。
- `src/village/poi.rs:49` 起的 PoiManager／Village 舊系統；`trade.rs:336–380` 的 ActiveMerchantSession／MerchantSessionManager。

## 實作步驟

1. 刪五個整檔及 mod／re-export／專屬測試。
2. 刪 fishing、POI、merchant 的 test-only shell 和無用 imports；不再換一個 feature 留存。
3. 保留 fishing 前 100 行正式階段／launch velocity／deterministic roll，保留 VillagerProfession／VillagerLevel／TradeOffer。
4. 保留 authority mount/container/fishing、EntityType::FishingHook 及正式 block/entity discriminants。同名玩法不等於對被刪模組的依賴。
5. 村民交易生成算法另由 13 處理，避免刪資料契約。
6. 更新 ARCHITECTURE 模組地圖，刪除「僅為測試保留」描述。

## 驗證與驗收

執行 `cargo test --lib authority::fishing::tests`、`cargo test --lib trade_conserves_items_and_mount_projects_session_state`、`cargo test --test plan33_tcp_fishing_lifecycle`，以及受影響 container 測試。

plan33 現有 embedded／listen／dedicated fishing lifecycle 案例繼續通過。舊原型名字從 source 宣告與引用消失；不新增替代 manager。plan32 的 portal／dragon 套件放完整回歸，不作每個原型刪除包的必要窄測試。

## 實作紀錄

尚未執行；完成時記錄實際修改、驗證結果、刪碼量及文件更新。

