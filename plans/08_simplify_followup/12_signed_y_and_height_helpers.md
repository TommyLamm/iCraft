# Plan12 — 剩餘 `CHUNK_HEIGHT`／`0..256` 改 signed-Y helper

## 定位

既有來源：`Dimension::height()`、`WorldHeight::{contains_y, min_y, section_count}`、`section_and_local_y_to_world_y`。生產路徑仍有硬編碼 256：

- `src/world/block.rs` / `section.rs`：`CHUNK_HEIGHT = 256`、`SECTION_COUNT = 16`（常數可留作「單節高度」語意時須在註解寫清）
- `src/state.rs` `MeshSnapshot`（`cfg(test)`）仍用 `0..CHUNK_HEIGHT` 且 `world_y < 0` 當空氣
- `src/dimension.rs` Nether／End pad 到 `SECTION_COUNT`
- `src/network/client.rs` 測試 checksum `for y in 0..256`
- `src/microbench.rs` `(0..256).contains(&y)`
- 多處手寫 `min_section_y as i32 * 16` 應改 `section_and_local_y_to_world_y`

天氣與可見性 BFS 已在前一輪改過。

**不要改** `save/format.rs` 歷史存檔 `0..256` 遷移（236 仍是 236）。

## 前置

無。

## 精確 acceptance

- [ ] 測試／microbench／MeshSnapshot 走訪使用 chunk 的 `min_section_y` + `sections.len()` 或 `height.contains_y`。
- [ ] Nether／End 生成節數用該維度 `height().section_count()`。
- [ ] 手寫 `* 16` 節→世界 Y 改 helper（至少生產路徑）。
- [ ] 存檔遷移測試仍鎖舊 Y 語意。

## 預計檔案與測試

- `src/state.rs` MeshSnapshot、`dimension.rs`、`network/client.rs` 測試、`microbench.rs`、`chunk.rs`／`mesh.rs` 零星 `* 16`
- 驗證：`cargo test --lib world::`；client checksum 測試；save 遷移測試

## 建議階段

1. 測路徑 checksum／MeshSnapshot。
2. 生成 Nether／End。
3. 生產 `* 16` 零星點。

## 不在本計劃

- 刪 `CHUNK_WIDTH`／`CHUNK_DEPTH`（16 是欄尺寸，不是世界高）。
- 存檔遷移重解讀。
