# Plan03 — 刪除 `mob` 與 `passive_mob` 遺留渲染器更新循環

## 定位

在早期架構中，生物實體狀態更新由桌面端渲染主循環驅動（`update_mobs` 與 `update_passive_mobs`）。
在權威化完成後，20Hz 固定刻度下的生物移動、AI、攻擊、擊退、受傷、掉落與繁殖均由 `ServerWorld::tick_entities` 統一進行（見 `ARCHITECTURE.md` §3）。

目前殘留在 `src/mob.rs` 與 `src/passive_mob.rs` 中的舊更新循環：
- `src/mob.rs:L293-490`（198 行）：`update_mobs`
- `src/passive_mob.rs:L41-323`（283 行）：`update_passive_mobs`

僅由 `legacy_sim.rs`（桌面遺留模擬）與模組內部的遺留單元測試調用。活體 Singleplayer、Listen Host、Join Client 及 Dedicated Server 均不走此路徑。

## 前置

無。可與 01、02、04–06 並行。

## 精確 acceptance

- [x] 移除 `src/mob.rs` 中的 `update_mobs` 函式及依賴其運作的遺留單元測試（或遷移至權威測試）。
- [x] 移除 `src/passive_mob.rs` 中的 `update_passive_mobs` 函式及相關遺留更新輔助。
- [x] 若 `src/presentation/legacy_sim.rs` 調用了這兩個函式，替換為空實現或直接移除該調用。
- [x] 保持 `src/mob.rs` 中的 `spawn_mobs`、`Mob` 定義與渲染屬性完整。
- [x] 保持 `src/passive_mob.rs` 中的 `spawn_passive_mobs`、`PassiveMob` 定義與渲染屬性完整。
- [x] `cargo check --all-targets` 通過。
- [x] 權威端生物模擬與集成測試全數通過。

## 預計檔案與測試

- 修改：
  - `src/mob.rs`
  - `src/passive_mob.rs`
  - `src/presentation/legacy_sim.rs`（若有引用）
- 驗證測試：
  - `cargo test --lib mob::`
  - `cargo test --lib passive_mob::`
  - `cargo test --lib server_world::`
  - `cargo check --all-targets`

## 建議階段

1. 檢查 `update_mobs` 與 `update_passive_mobs` 的全庫調用點。
2. 刪除 `src/mob.rs` 中 `update_mobs` 及相關舊單元測試。
3. 刪除 `src/passive_mob.rs` 中 `update_passive_mobs` 及相關舊單元測試。
4. 清理 `legacy_sim.rs` 中的調用點。
5. 運行編譯與單元測試。

## 不在本計劃

- 修改 `ServerWorld::tick_entities` 的生物更新邏輯。
- 變更生物渲染插值（`mob_renderer.rs`）。

## 實作與證據

### 修改內容
1. **`src/mob.rs`**：
   - 刪除遺留渲染更新循環 `update_mobs`、`MobSoundEvent` 列舉、`should_retain_after_health_cleanup`、`PlayerHitSource`、`PlayerHitEvent`、`is_under_sun`。
   - 保留權威生成 `spawn_mobs`、幾何運算 `calculate_explosion_damage`、`explode` 與 `get_highest_solid_y`。
   - 清理依賴 `update_mobs` 的舊單元測試，保留爆炸傷害與面向運算單元測試（`test_explosion_damage`、`explosion_reports_authoritative_block_removals_and_can_be_visual_only`、`test_mob_yaw_faces_player`）。
   - 代碼淨減：~851 行。
2. **`src/passive_mob.rs`**：
   - 刪除遺留渲染更新循環 `update_passive_mobs` 及私有輔助 `store_or_drop_chicken_egg`、`check_cliff_ahead`。
   - 保留權威生成 `spawn_passive_mobs` 以及對應的群體生成與半徑查詢測試（`fresh_loaded_spawn_region_establishes_passive_population`、`partner_query_matches_independent_radius_oracle`）。
   - 代碼淨減：~398 行。
3. **`src/presentation/legacy_sim.rs`**：
   - 移除呼叫 `crate::mob::update_mobs` 與 `crate::passive_mob::update_passive_mobs` 的遺留邏輯，清理未使用的局部變數（`yaw_sin`, `yaw_cos`, `right`, `is_raining`）。
   - 保留白天生物生成 `crate::passive_mob::spawn_passive_mobs` 與玩家投射物更新。

### 驗證與測試證據
- `cargo test --lib mob::`：5/5 全部通過。
- `cargo test --lib passive_mob::`：2/2 全部通過。
- `cargo test --lib server_world::`：18/18 全部通過。
- `cargo test --lib`：714 測試通過（實體、物理、世界生成、權威刻度無回退）。

### 留下的缺口
- 無。`ServerWorld::tick_entities` 統一驅動所有生物移動、AI、物理與生成，桌面渲染器循環已完全解耦。
