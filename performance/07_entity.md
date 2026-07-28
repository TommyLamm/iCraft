# 任務 7：Entity ID/type/spatial indexes

> 對應計畫：`14_performance_optimization.md` Phase 2.3
> 狀態：⏳ 待實作
> 前置：任務 1（基線）、任務 5（固定 tick）
> 目標：為 `EntityManager` 加入 ID/type/spatial index，消除全表遍歷與線性 `.find()`。
> Commit 訊息：`perf(entity): add id, type and spatial indexes`

## 相關程式碼位置（已核對）

- `src/entity.rs:102` - `Entity` 結構。
- `src/entity.rs:389` - `EntityManager` 結構。
- `src/entity.rs:395` - `EntityManager::new`。
- `src/entity.rs:156` - `Entity::new`。
- `src/mob.rs:287` - `update_mobs`：遍歷 entity 並做 AI/combat。
- `src/passive_mob.rs` - passive mob AI。
- `src/boss.rs` - boss 行為。
- `src/state.rs:7120` - `spawn_dropped_item`：spawn 點。
- `src/state.rs:7258` - `update_player_projectiles`：projectile 查找。
- `src/mob_renderer.rs:170` - `render_mobs`：渲染遍歷。

## 已確認的基線風險

- Mob、passive mob、dropped item、remote player 和 rendering 共用單一 `Vec<Entity>`，存在多次全表遍歷、按 ID 線性 `.find()` 及按類型過濾。

## 子任務清單

### 7.1 EntityId -> dense index
- [ ] 檔案：`src/entity.rs`
- 步驟：
  1. 增加 `EntityId -> dense index` 映射（`HashMap<u64, usize>`）。
  2. `swap_remove` 後更新 index（被 swap 的 entity 的 index 更新）。
  3. 保留 cache-friendly dense storage。
- 驗收：Entity spawn/remove/restore/remote leave 後 ID index 永遠有效。

### 7.2 EntityType 與 Chunk/section bucket
- [ ] 檔案：`src/entity.rs`
- 步驟：
  1. 增加按 `EntityType` 的 bucket（hostile/passive/projectile/drop/remote_player）。
  2. 增加按 Chunk/section 的 spatial bucket。
  3. 位置跨區時增量更新 bucket 歸屬。
- 驗收：按類型/區域查找不遍歷全部 Entity。

### 7.3 Nearby 查詢先查 bucket
- [ ] 檔案：`src/mob.rs`、`src/passive_mob.rs`、`src/boss.rs`、`src/state.rs`、`src/mob_renderer.rs`
- 步驟：
  1. nearby collision、pickup、melee、projectile、spawning 和 rendering 先查相關 bucket。
  2. 不遍歷全部 Entity。
  3. 確認所有 `.find()` 與全表 `iter().filter()` 都改為 bucket 查詢。
- 驗收：壓力場景查找複雜度隨附近實體數，而不是全世界實體數增長。

### 7.4 Distance-squared 取代 sqrt
- [ ] 檔案：`src/entity.rs`、`src/mob.rs`、`src/passive_mob.rs`、`src/state.rs`
- 步驟：
  1. 使用 `distance_squared` 取代不需要真實距離的 `sqrt`。
  2. 比較時用 `d_sq <= range * range`。
- 驗收：無不必要的 `sqrt` 呼叫。

### 7.5 重用事件 scratch Vec
- [ ] 檔案：`src/entity.rs`、`src/state.rs`、`src/mob.rs`
- 步驟：
  1. 重複使用事件 scratch Vec（`State` 持有持久欄位，逐幀 `clear()` 重用 capacity）。
  2. 移除每 frame 的短命 allocations。
- 驗收：穩態 entity 更新接近零 heap allocation。

### 7.6 AoS 拆分（條件性）
- [ ] 檔案：`src/entity.rs`
- 步驟：
  1. 只有性能基線顯示 `Entity` AoS 大小是問題時，才將 dropped item、projectile、passive/hostile payload 拆成 enum/component side storage。
  2. 若無明顯退化，保持現有 AoS。
- 驗收：基線證明才執行；不做無證據的重構。

## 驗收條件

- [ ] Entity spawn/remove/restore/remote leave 後 ID index 永遠有效。
- [ ] 壓力場景查找複雜度隨附近實體數，而不是全世界實體數增長。
- [ ] 無不必要的 `sqrt` 呼叫。
- [ ] 穩態 entity 更新接近零 heap allocation。
- [ ] `cargo fmt --all -- --check`、`cargo check --release`、`cargo test --release` 通過。

## 風險與回退

- ID index 若維護錯誤會導致 use-after-swap；必須有完整的 spawn/remove/move 測試。
- bucket 更新若過於頻繁會增加 CPU；增量更新只在跨區時觸發。
- AoS 拆分是可選的，基線證明才執行。

## 驗證命令

```text
cargo fmt --all -- --check
cargo check --release
cargo test --release
cargo run --release   # 固定場景 6（1,000 entities）before/after 比較
```
