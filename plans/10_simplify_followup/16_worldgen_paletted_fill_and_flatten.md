# Plan16 — worldgen 直接填 paletted section、Superflat、Nether 光、flatten SoA、`BlockStorage` 去重

## 定位

### dense-then-convert

- `Chunk::new_with_seed`（`chunk.rs` 83–227）配 `Vec<Vec<[BlockType;16]>>` × 384 高＋兩份光照陣列，填 Y／雕洞／礦／feature 後再拷進 24 個 `ChunkSection`。
- `generate_overworld_chunk`（`dimension.rs` 289–295）是 wrapper，**重建 `chunk.rs` 210–215 已建好的** torch／redstone／furnace 索引。
- `generate_superflat_chunk`（265–287）先呼叫 `Chunk::new_with_seed`（**完整 Overworld 洞穴／礦／樹**）再覆寫每格。
- Nether（308–557）dense `[[[BlockType;16];128];16]` + **自己的光照 BFS**（447–505），再同樣拷 section；`NETHER_HEIGHT`（11）重複 `WorldHeight::NETHER`。
- End（723–798）已直接填 section——正確模式。

每欄 ~3 × 98k 陣列 + 24 section 拷貝；Superflat 多付一次完整 Overworld。

### flatten 每 voxel 隨機存取

`flatten_column_voxels`（`format.rs` 821–854）三層迴圈 x × height × z 每格呼叫 `get_block_local`／`get_block_state`／`get_sky_light`／`get_block_light`／`get_fluid_level`，再壓縮；`network_terrain_payload`（1205–1221）共用。section 已有 palette。這是 `from_chunk`／投影 payload 的主要 CPU。

### `BlockStorage` 四份 promote 路徑

`section.rs` `BlockStorage::get` 171–201、`set` 203–342：Paletted1／2／4／8 各自實作「在 palette → pack；有空位 → push；否則升寬重寫 4096」。`LightStorage::set_sky`／`set_block`（486–517）是同一 uniform→packed 分叉兩份。~170 行。

## 前置

無。

## 精確 acceptance

- [x] Overworld／Nether 生成直接對 `ChunkSection` `set_block_local`（或每 section `from_dense`），不再配整欄 dense；`generate_overworld_chunk` wrapper 的索引重建刪除。
- [x] Superflat 從 `Chunk::empty_in_dimension` 開始填，不呼叫 `new_with_seed`。
- [x] Nether 光照走 `recompute_direct_column_lighting`／`propagate_chunk_lighting`；私有 BFS 刪除；`NETHER_HEIGHT` 改 `WorldHeight::NETHER.height()`。
- [x] `flatten_column_voxels` 改 per-section SoA 一次走（輸出 bytes 與現在 **byte-identical**，磁碟格式不變）。
- [x] `BlockStorage` get／set／promote 以 bits-per-index 參數化成一份；`LightStorage::set_nibble(kind)` 一份。
- [x] worldgen byte-identity 測試（`worldgen/mod.rs` 116；`dimension.rs` 1184–1200）**維持 byte-identical**；Superflat 輸出與現在一致（現在的覆寫是完整覆寫，應等價——加一個固定 seed 對比測試鎖住）。
- [x] `WorldHeight` 欄位改私有，只留 `min_y()`／`max_y_exclusive()`／`section_count()`（順手收斂 `physics.rs` 427、`dimension.rs` 835 的直接欄位存取）。

## 預計檔案與測試

- 改：`src/world/{chunk,section}.rs`、`src/dimension.rs`、`src/worldgen/mod.rs`、`src/save/format.rs`、`src/lighting.rs`、`src/physics.rs`
- 驗證：`cargo test --lib worldgen:: dimension:: world:: save::`；`tests/review_hardening_chunk_restore.rs`；固定 seed 生成 checksum 對比（Overworld／Nether／End／Superflat 各一）

## 建議階段

1. `BlockStorage`／`LightStorage` 去重（有 section 測試鎖住）。
2. flatten SoA（bytes 對比測試）。
3. Superflat 空欄起手；刪 Overworld wrapper 索引重建。
4. Overworld 直接填 section。
5. Nether 直接填 section + 走 lighting.rs。
6. `WorldHeight` 私有欄位。

## 不在本計劃

- 磁碟 `ChunkSaveData` 六串 zlib → 單 blob（README §6，另案）。
- 結構生成 helper（Plan 25）。
- lighting BFS 本體去重（Plan 17）。

## 實作與證據

### 改了什麼

- `section.rs`：`BlockStorage` Paletted1/2/4 get／set／promote 收成 bits-per-index helper；`LightStorage::set_nibble` 統一 sky／block。
- `save/format.rs`：`flatten_column_voxels` 改 per-section SoA 走訪；oracle 測試鎖住與舊 `get_*` 走訪 byte-identical。
- `chunk.rs`：`new_with_seed` 改 `empty_in_dimension` + `set_block_local` 填 section，再 `recompute_direct_column_lighting`；不再配 3×98k dense。
- `worldgen/ore.rs`／`feature.rs`：改寫入 `Chunk`（去掉 dense 陣列與 `min_y_offset`）。
- `dimension.rs`：Superflat 自空欄起填；Overworld wrapper 刪重複 index rebuild；Nether 刪私有 BFS，改 `recompute_direct_column_lighting` + `propagate_chunk_lighting`；`WorldHeight` 欄位私有。
- `physics.rs`／`mob.rs`：改走 `min_y()` accessor。
- `ARCHITECTURE.md`：記錄 paletted fill／SoA flatten／私有 `WorldHeight`。

### 測了什麼

- `cargo test --lib -- worldgen:: dimension:: world::section:: world::chunk:: flatten_soa ore:: feature::`（58 passed）
- `cargo test --lib world::section::`（10 passed，含於上列）
- `dimension::tests::fixed_seed_column_fingerprints_are_stable`（OW／Nether／End／Superflat 固定 seed 指紋）
- `tests/review_hardening_chunk_restore.rs`（5 passed）
- `cargo check --all-targets`／`cargo check --bin icraft-server`（通過）

### 留下的缺口

- Superflat 光照改為對平坦地形 `recompute_direct_column_lighting`（舊路徑繼承被覆寫前 Overworld 的錯光）；方塊佈局以既有／新指紋測試鎖定。
- Nether 塊光改共用 `propagate_chunk_lighting`，單欄結果應等價舊 BFS，但跨欄鄰接行為與 Plan 17 的 BFS 去重仍分開。
- ore／feature 熱路徑改 `set_block_local`（palette 增量），生成 CPU 可能暫高於舊 dense 再 `from_dense`；未做 microbench。
- `lighting.rs` 本體未改（Plan 17）。
