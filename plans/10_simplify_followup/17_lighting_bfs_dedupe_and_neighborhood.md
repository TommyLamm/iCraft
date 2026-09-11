# Plan17 — lighting BFS 去重、鄰域快取、載入只 seed 邊界

## 定位

Wave 08 讓載入光照的 **seed** 用 `column_neighborhood`（`lighting.rs` 585；`chunk_manager.rs` 290–296 註解「hot loop never does a per-voxel HashMap get」）。BFS 本體沒有跟上：

- `propagate_sky_light`（27–96）、`remove_sky_light`（98–161）、`propagate_block_light`（163+）、`remove_block_light` 四份幾乎相同的 walk，每個鄰居都 `chunk_manager.get_block` + `get_sky_light`／`get_block_light`（各一次 `HashMap<(i32,i32),Chunk>` 查找，`get_loaded_block` 341–344）。
- 四份各自內聯 `LIGHT_DIRS`（本檔 5–12 已定義，卻在 32、104、168、238、388、490 重抄）。
- 同一段 15 行「標記本欄 + 4 邊鄰居 dirty」複製四次（68–86、128–144、206–219、264–277）。
- Nether 生成有第五份 BFS（`dimension.rs` 447–505；Plan 16 刪）。

載入路徑：`TerrainWorkerResult::Loaded` 對中心 + 4 鄰欄呼叫 `propagate_chunk_lighting`（`state.rs` 5791–5811）；`lighting.rs` 572–648 對每個 section／x／z／ly 都掃兩個鄰居探針，Overworld ≈ 98k × 5 欄 ≈ 0.5M cell，GPU 執行緒 1–8 ms／欄（被 `MAX_INTEGRATE_TIME_MS` 切片，變成 hitch 叢）。ARCHITECTURE 描述的「只 seed 面／發光體／較暗鄰居」在 seed 階段成立，但 volume scan 還在。

放／破方塊在 view 16 觸及數千 cell，每 cell 2 次 HashMap × 6 鄰居。

## 前置

無。可與 Plan 16 並行（16 刪 Nether BFS 時直接呼叫本計劃的入口）。

## 精確 acceptance

- [ ] 一個泛型 `propagate<Kind>`／`remove<Kind>`（sky／block 以 trait 或 enum 參數化），在 3×3 欄 `&mut Chunk` 鄰域快取上運作；每 cell 零 HashMap 查找。
- [ ] `mark_light_dirty(dirty, nx, nz)` 一份；`LIGHT_DIRS` 只在一處。
- [ ] 載入光照只從 seed 集合起 BFS，不做全 volume scan；`propagate_chunk_lighting` 對 4 鄰欄的呼叫改為只處理邊界面。
- [ ] 光照值與現在 **完全一致**（用固定 seed 欄 + 固定放／破序列的光照 checksum 測試鎖住）。
- [ ] `lighting.rs` 測試（659–833）全綠；Nether glowstone 測試（`dimension.rs` 1056+）全綠。
- [ ] 載入一欄的光照時間（microbench 或 `perf.rs` 計數）前後對比記錄在「實作與證據」。

## 預計檔案與測試

- 改：`src/lighting.rs`、`src/chunk_manager.rs`（鄰域借用 helper）、`src/state.rs`（載入呼叫）、`src/dimension.rs`（Nether 入口，若 16 未先做）
- 驗證：`cargo test --lib lighting:: dimension::`；`cargo test --bin icraft`；固定序列光照 checksum

## 建議階段

1. 抽 `mark_light_dirty` 與 `LIGHT_DIRS` 單一來源（純去重）。
2. 四份 BFS 合一份泛型（先保留 HashMap 查找，鎖 checksum）。
3. 換成鄰域快取。
4. 載入只 seed 邊界。

## 不在本計劃

- 生成階段光照（Plan 16）。
- dense chunk grid（Plan 26）。
