# Plan07 — 世界 tick helper 與 signed-Y 掃描

## 定位

權威 tick 已正確走 simulation union：`ServerWorld` 呼叫 `tick_fluids_in_columns`／`sample_random_ticks_in_columns`／`tick_hoppers_in_columns`。但公開 API 仍留 `columns: None` 的 unbounded wrapper（`src/fluid.rs` ~26、`src/world_tick.rs` ~324／~445）。`state.rs` leftover 與 `sim_harness` 走 wrapper。新系統很容易 `tick_fluids(...)` 就繞過 residency。

同一檔裡還有可安全抽取、不改玩法的重複：

- Hopper 面向轉移（~520）與上方容器轉移（~544）都是 clone → `transfer_one` → write-back。
- `evaluate_random_tick_at` 裡仙人掌（~227）與甘蔗（~246）除了方塊型別幾乎相同。農田濕度寫死 `7u8`（~127）——**本計劃不得「修好」它**，那會改生長率。
- `redstone.rs` `apply_component_transitions`（~1194）裡 torch／comparator／lamp／door／trapdoor 都是 `power > 0` 選 On／Off。

Signed-Y：`ARCHITECTURE.md` 要求用 `Dimension::height()`／`world.rs` helper，不要寫死 `0..256`。仍用 `1..CHUNK_HEIGHT` 的掃描會漏 Overworld `y ≤ 0` 與 `y ≥ 256`：

- `chunk_manager.rs` `check_and_break_unsupported_for_loaded_chunk` ~554
- `boss.rs` `open_surface_y` ~973
- `mob.rs` `is_under_sun` 用 `(my + 1)..320`，在 Nether／End 會 overscan

`BlockType::support_status_at` 的 `if y <= 0`（`world.rs` ~716）是**現役語意**，本計劃不得改成 `!height.contains_y(y - 1)`。

01 已處理／將處理死的 `place_*_tree` 與 carve 臂。本計劃不要重做那些刪除。

## 前置

無。不要跟 01 搶刪 `world.rs` 樹 helper。若 01 未合併，本計劃不要刪那些函式。

## 精確 acceptance

- [ ] `*_in_columns`（或要求 `&BTreeSet<(i32,i32)>`）是流體／隨機刻／漏斗的公開權威入口。`columns == None` 走遍所有已載入 column 的路徑改名為 `tick_all_loaded_*`（或 `#[cfg(test)]`），rustdoc 寫明「測試／leftover renderer；權威禁止」。
- [ ] `ServerWorld` 繼續傳 simulation union，不得改成 `tick_all_loaded_*`。
- [ ] Hopper 兩段 container 轉移共用一個 helper（例如 `try_container_transfer`）。Item pickup 仍分開。面向、冷卻、cooldown 數字不變。
- [ ] 仙人掌／甘蔗抽 `try_grow_column(block, max_h, rng)`。農田濕度那兩行保持原樣（含寫死 7）。
- [ ] 紅石 powered 方塊用表或 `powered_variant(block, powered)`。Piston／dispenser／TNT 上升緣保持獨立。
- [ ] `1..CHUNK_HEIGHT` 的 **掃描迴圈**改走該維度的 `WorldHeight`（`min_y+1 .. max_y` 或既有 helper），謂語不變。`is_under_sun` 用 `height.max_y_exclusive()`，不要寫死 320。
- [ ] **不得**改 `support_status_at` 的 `y <= 0` 判斷。
- [ ] **不得**合併 `physics::resolve_collisions` 與 `entity::resolve_collisions`（實體仍是整方塊 AABB）。
- [ ] **不得**合併 `spawn_mobs` 與 `SpawningSystem` 的距離／上限。
- [ ] `tests/review_hardening_chunk_residency.rs`、waterlogging、難度權威測試期望值不變。

## 預計檔案與測試

- 修改：`src/world_tick.rs`、`src/fluid.rs`、`src/redstone.rs`、`src/chunk_manager.rs`、`src/boss.rs`、`src/mob.rs`、`src/server_world.rs`（只改呼叫名稱／註解）、`src/world.rs`（只加 height iterator，若尚未存在）。
- 測試：
  - `cargo test --lib world_tick::`
  - `cargo test --lib fluid::`
  - `cargo test --lib redstone::`
  - `cargo test --lib chunk_manager::`
  - `cargo test --test review_hardening_chunk_residency -- --test-threads=1`
  - `cargo test --test waterlogging_authority -- --test-threads=1`
  - `cargo test --test difficulty_authority -- --test-threads=1`
  - `cargo check --all-targets`

## 建議階段

1. 確認 `WorldHeight`／signed-Y helper 現有 API，缺 iterator 再補，不要新發明第二套高度型別。
2. 把 unbounded wrapper 改名並收緊 rustdoc；改 `ServerWorld` 以外的呼叫點名稱。
3. 抽 hopper helper、生長 column、powered_variant。
4. 換掃描迴圈。每個迴圈改完跑相關單元測。
5. 跑 residency／waterlogging。

## 不在本計劃

- 修好農田濕度（那是行為變更，另開玩法計劃）。
- 實體改用 voxel shape 碰撞。
- 把 `Brain` 接到 tick。
- 刪 `world_mutation::apply_batch`（leftover renderer 仍用）。
- lighting sky／block 泛型合併。
