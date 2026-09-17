# 01 — 刪除完整原型與測試專用玩法殼

狀態：已完成。基線：`83e751d`，2026-09-17。
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

- 改動項目：
  1. 完整刪除 5 個無 producer 的原型與測試檔：`src/container_sessions.rs`、`src/vehicle.rs`、`src/rail.rs`、`src/navigation.rs`、`src/village/raid.rs`。
  2. 清理 `src/lib.rs`（移除 `container_sessions`、`navigation`、`vehicle`、`rail` 的 mod 宣告）、`src/main.rs`（移除 `navigation` 的 re-export）、`src/village/mod.rs`（移除 `raid` 模組宣告與 `PoiManager`、`Village`、`PoiType`、`RaidManager`、`RaidStatus`、`MerchantSessionManager` 的 re-export）。
  3. 清理 `src/fishing.rs`：刪除 test-only 的 `FishingHook`、`FishingResult`、`FishingManager` 及其單元測試與未用 imports，保留正式 wire 階段 `FishingHookStage`、`authoritative_launch_velocity_milli`、`deterministic_fishing_roll` 與常數。
  4. 清理 `src/village/poi.rs`：刪除第 49 行起的 `PoiType`、`PoiEntry`、`Village`、`PoiManager` 舊系統與專屬測試，保留正式 `VillagerProfession` 列舉及其 wire / display 方法。
  5. 清理 `src/village/trade.rs`：刪除 `ActiveMerchantSession`、`MerchantSessionManager` 與未用 `HashMap` import，保留 `TradeOffer`、`VillagerLevel` 及相關正式方法（生成算法留待 13 處理）。
  6. 更新 `ARCHITECTURE.md`：移除為測試保留 presentation shells 的過時描述，更新模組地圖 Gameplay 項目。
- 實際命令與結果：
  - `cargo test --lib authority::fishing::tests`: 7 passed; 0 failed
  - `cargo test --lib trade_conserves_items_and_mount_projects_session_state`: 1 passed; 0 failed
  - `cargo test --test plan33_tcp_fishing_lifecycle`: 1 passed; 0 failed; 2 ignored (既有 pre-existing network flood flake)
  - `cargo test --test plan34_container_break_inventory_conservation --test review_hardening_container_click`: 12 passed; 0 failed
  - `cargo check --all-targets --all-features`: exit code 0
- 淨刪碼／保留原因：
  - 淨刪除 1,813 行（12 files changed, 7 insertions(+), 1,820 deletions(-)）。
  - 保留原因：`VillagerProfession`、`VillagerLevel`、`TradeOffer` 仍為 entity/save/authority 契約；fishing wire stages 與 authority launch/roll 算法仍為 fishing lifecycle 核心；authority container 與 mount 機制已由 `ServerWorld` / session overlay 實現，刪除的原型皆為無任何正式 caller 的死碼。


