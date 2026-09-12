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

- [x] 測試／microbench／MeshSnapshot 走訪使用 chunk 的 `min_section_y` + `sections.len()` 或 `height.contains_y`。
- [x] Nether／End 生成節數用該維度 `height().section_count()`。
- [x] 手寫 `* 16` 節→世界 Y 改 helper（至少生產路徑）。
- [x] 存檔遷移測試仍鎖舊 Y 語意。

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

## 實作與證據

工作樹 `C:\Users\Tommy\Desktop\iCraft-wt-08-12`，分支 `plan/08-12-signed-y-height`。

### 改了什麼

高度政策改走 chunk 節範圍與 signed-Y helper，不再把 `CHUNK_HEIGHT`／`0..256` 當活世界邊界：

- `Chunk::{min_world_y, max_world_y_exclusive, world_y_range}`；`section_and_local_y_to_world_y` 用於節→世界 Y。
- MeshSnapshot、microbench、client checksum、dimension／lighting／interaction／sim_harness／boss 測試走訪改 `world_y_range()`。
- Nether／End 生成節數用 `height().section_count()`；Nether 不再 pad 到 `SECTION_COUNT`。
- 生產路徑 `chunk.rs`、`world_tick.rs`、`mesh.rs` 高度走訪改 helper；未重寫 mesh／光照熱路徑。
- 未改 `save/format.rs` 歷史 `0..256` 遷移。`CHUNK_HEIGHT`／`SECTION_COUNT` 留下並註明是 legacy dense 常數。

ARCHITECTURE.md 補上 `section_and_local_y_to_world_y`、`Chunk::world_y_range()`，以及 Nether／End 用 `height().section_count()`。

### 測了什麼

- `cargo test --lib world::` — 73 passed
- `network::client::tests::tcp_capacity_one_retries_without_starving_second_client_and_converges` — ok
- `network::client::tests::revision_gate_multi_client_checksum_converges` — ok
- `save::tests::legacy_u8_redstone_y_236_stays_236_not_negative_twenty` — ok（236 仍是 236）
- `cargo test --lib dimension::` — 11 passed
- `cargo test --bin icraft mesh_snapshot` — `mesh_snapshot_owns_the_neighbor_halo` ok
- `cargo test --lib lighting::` — 5 passed

### 留下的缺口

`save/format.rs` 現行編碼仍有 `min_section_y * 16`（非遷移路徑）。leftover `legacy_interaction` 仍用 `CHUNK_HEIGHT`。稠密測試陣列 `[[[BlockType; 16]; 256]; 16]` 未改。光照 packing 的 `* 16` 不是節→世界 Y。

