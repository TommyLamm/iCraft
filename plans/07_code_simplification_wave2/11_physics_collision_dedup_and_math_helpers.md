# Plan11 — 物理與實體軸向碰撞去重及 PRNG 整合

## 定位

在物理引擎、實體系統與世界模擬中，存在多處顯著的重複實現與自造輪子：

1. **軸向分離碰撞邏輯重複 (`src/entity.rs:L620-668` vs `src/physics.rs:L430-481`)**：
   `Entity::resolve_axis_collision` 與 `PlayerPhysics::resolve_axis_collision` 對 X、Y、Z 三軸的 AABB 掃描、邊界分離與位置校正邏輯重複率達 90%。
   應在 `src/physics.rs` 抽取共用的 `resolve_axis_box_collision` 輔助函式。

2. **未使用的相容包裝 `src/physics.rs:L90-101` (`block_aabb`)**：
   碰撞判定已全面改用 `block_shape` (`VoxelShape`)，刪除該 12 行殘留包裝。

3. **散落 7 處的重複 PRNG / SplitMix64 / Hash 常數**：
   - `src/boss.rs:L995-1006`
   - `src/world_tick.rs:L18-26`
   - `src/dimension.rs:L304-314`（`hash3` 是 `worldgen::hash_coord` 的完全克隆）
   - `src/worldgen/mod.rs:L70-82`
   - `src/mob.rs:L227-236`
   - `src/passive_mob.rs:L339-348`
   - `src/loot.rs:L496-515`
   重複定義了 SplitMix64 常數（`0xbf58476d1ce4e5b9`, `0x94d049bb133111eb`）與 LCG 隨機算法，應統一由 `src/world_tick.rs` 或 shared PRNG 工具提供。

4. **重複的難度定義 (`src/game_rules.rs:L46-140`)**：
   `Difficulty` 與 `ServerDifficulty` 枚舉及其字串解析 100% 重複，完全應合併為單一 `Difficulty` 枚舉。

5. **`src/crafting.rs` 3 行純別名模組**：
   刪除 `src/crafting.rs`，所有地方直接使用 `crate::recipes::*`。

預期削減代碼 ~180 行。

## 前置

無。可與 07–10、12–13 並行。

## 精確 acceptance

- [ ] 在 `src/physics.rs` 抽取共用的 `resolve_axis_box_collision`，並在 `entity.rs` 與 `physics.rs` 複用。
- [ ] 刪除 `physics.rs` 中的 `block_aabb`。
- [ ] 統一 7 個模組中的 SplitMix64 / Hash 演算法，刪除 `dimension.rs` 中的 `hash3`，直接調用 `worldgen::hash_coord`。
- [ ] 將 `game_rules.rs` 中的 `ServerDifficulty` 合併進 `Difficulty`。
- [ ] 刪除 `src/crafting.rs` 並在 `src/lib.rs` / `src/main.rs` 移除對應宣告。
- [ ] `cargo check --all-targets` 通過。
- [ ] 物理與合成相關測試全數通過。

## 預計檔案與測試

- 刪除：
  - `src/crafting.rs`
- 修改：
  - `src/physics.rs`
  - `src/entity.rs`
  - `src/game_rules.rs`
  - `src/boss.rs`
  - `src/dimension.rs`
  - `src/lib.rs`
  - `src/main.rs`
- 驗證測試：
  - `cargo test --lib physics::`
  - `cargo test --lib entity::`
  - `cargo test --lib recipes::`
  - `cargo check --all-targets`

## 建議階段

1. 抽取 `physics.rs` 碰撞輔助並重構 `entity.rs`。
2. 合併 `Difficulty` 與 `ServerDifficulty`。
3. 統一 PRNG 常數與方法。
4. 刪除 `crafting.rs` 並更新引用的模組路徑。
5. 運行物理與遊戲規則測試。

## 不在本計劃

- 更改實體 AABB 盒尺寸或重力加速度常數。
