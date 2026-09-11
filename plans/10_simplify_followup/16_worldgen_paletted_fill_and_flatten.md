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

- [ ] Overworld／Nether 生成直接對 `ChunkSection` `set_block_local`（或每 section `from_dense`），不再配整欄 dense；`generate_overworld_chunk` wrapper 的索引重建刪除。
- [ ] Superflat 從 `Chunk::empty_in_dimension` 開始填，不呼叫 `new_with_seed`。
- [ ] Nether 光照走 `recompute_direct_column_lighting`／`propagate_chunk_lighting`；私有 BFS 刪除；`NETHER_HEIGHT` 改 `WorldHeight::NETHER.height()`。
- [ ] `flatten_column_voxels` 改 per-section SoA 一次走（輸出 bytes 與現在 **byte-identical**，磁碟格式不變）。
- [ ] `BlockStorage` get／set／promote 以 bits-per-index 參數化成一份；`LightStorage::set_nibble(kind)` 一份。
- [ ] worldgen byte-identity 測試（`worldgen/mod.rs` 116；`dimension.rs` 1184–1200）**維持 byte-identical**；Superflat 輸出與現在一致（現在的覆寫是完整覆寫，應等價——加一個固定 seed 對比測試鎖住）。
- [ ] `WorldHeight` 欄位改私有，只留 `min_y()`／`max_y_exclusive()`／`section_count()`（順手收斂 `physics.rs` 427、`dimension.rs` 835 的直接欄位存取）。

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
