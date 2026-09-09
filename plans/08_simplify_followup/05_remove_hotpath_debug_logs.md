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

- [ ] `rg "debug-879839|#region agent log"` 在 `src/` 與 `tests/` 為零。
- [ ] 只寫 log、沒有 assert 的 `debug_*` 測試刪除或改成真正斷言。
- [ ] 有真實斷言的測試保留斷言、刪 log 區塊。
- [ ] `cargo test --lib culling::`、`chunk_render`、`lighting`、hopper／fluid／mining 相關測試通過。

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
