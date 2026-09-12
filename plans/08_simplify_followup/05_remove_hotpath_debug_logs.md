# Plan05 — 刪熱路徑 `debug-879839.log` 探測碼

## 定位

多個熱路徑與測試仍寫 `debug-879839.log`（`#region agent log`）。`TerrainVertex::new`、LOS `make_snapshot`、`restore_network_payload`、漏斗／流體／挖掘在生產或測試中開檔 append。這是同步磁碟 I/O，不是遊戲邏輯。

目前已知位置（以源碼為準再掃一遍）：

- `src/chunk_render.rs`（頂點編碼 + 測試）
- `src/culling/visibility.rs`（LOS 拒絕 + 測試）
- `src/save/format.rs` `restore_network_payload`
- `src/world_mutation.rs`、`src/lighting.rs`、`src/world/mesh.rs`
- `src/world_tick.rs`、`src/fluid.rs`、`src/authority/mining.rs`

工作樹根目錄的 `debug-879839.log` 不得進 git。

## 前置

無。可與任何計劃並行。

## 精確 acceptance

- [x] `rg "debug-879839|#region agent log"` 在 `src/` 與 `tests/` 為零。
- [x] 只寫 log、沒有 assert 的 `debug_*` 測試刪除或改成真正斷言。
- [x] 有真實斷言的測試保留斷言、刪 log 區塊。
- [x] `cargo test --lib culling::`、`chunk_render`、`lighting`、hopper／fluid／mining 相關測試通過。

## 預計檔案與測試

- 上表各檔；必要時 `.gitignore` 加入 `debug-879839.log`
- 驗證：`rg` 為空；窄單元測試

## 建議階段

1. 全庫搜尋確認清單。
2. 生產路徑先刪（頂點、LOS、restore）。
3. 測試路徑刪 log；空測試刪或補 assert。

## 不在本計劃

- 修負 Y 頂點編碼語意（若 clamp 仍錯，另開 bugfix，不要混進本清理）。
- 改光照演算法（11）。

## 實作與證據

工作樹 `C:\Users\Tommy\Desktop\iCraft-wt-08-05`，分支 `plan/08-05-remove-debug-logs`。

### 改了什麼

生產熱路徑刪除同步 append `debug-879839.log`：

- `TerrainVertex::new`（`src/chunk_render.rs`）
- LOS `make_snapshot` Y 範圍拒絕（`src/culling/visibility.rs`）
- `ChunkSaveData::restore_network_payload`（`src/save/format.rs`）
- leftover `apply_batch` 不透明變化探測（`src/world_mutation.rs`）
- `desired_flow` 非 source 瀑布探測（`src/fluid.rs`）
- hopper budget skip（`src/world_tick.rs`）
- `calculate_block_break_rewards` XP 探測（`src/authority/mining.rs`）

測試路徑：所有 `debug_*` 測試本來就有真實 assert，故保留測試、只刪 `#region agent log`。沒有「只寫 log、沒有 assert」的空測試可刪。

`.gitignore` 加入 `debug-879839.log`。未改 ARCHITECTURE.md（契約不依賴這些 log）。未改負 Y 頂點 clamp 語意。

### 測了什麼

`rg "debug-879839|#region agent log"` 在 `src/` 與 `tests/` 為零。

- `cargo test --lib culling::` — 12 passed
- `cargo test --lib chunk_render` — 28 passed（含 `debug_negative_world_y_encoding`）
- `cargo test --lib lighting::` — 5 passed（含 `debug_network_restore_cannot_reseed_sky`）
- hopper 相關 — 8 passed（含 `debug_hopper_budget_skips_cooldown`）
- `cargo test --lib fluid::` — 9 passed（含 `debug_waterfall_and_horizontal_spread`）
- mining 相關 — 11 passed（含 `debug_ore_xp_and_crop_drops`）
- `debug_negative_section_mesh_bounds`、`debug_stone_to_glass_skips_sky_update` — 各 1 passed

### 留下的缺口

`TerrainVertex::new` 仍對 `rel_y = (y - REGION_ORIGIN_Y).max(0.0)` clamp；若負 Y 編碼仍錯，需另開 bugfix，不在本清理。光照演算法留給計劃 11。
