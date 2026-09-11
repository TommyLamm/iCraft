# Plan20 — 13 對 powered／open `BlockType` 折進 `BlockState`

## 定位

以下變體是「另一個狀態的同一種方塊」：

`RedstoneTorchOff`、`RepeaterPowered`、`ComparatorPowered`、`StoneButtonPressed`、`LeverOn`、`PressurePlatePowered`、`PistonExtended`、`StickyPistonExtended`、`RedstoneLampLit`、`OakDoorOpen`、`OakTrapdoorOpen`、`FurnaceLit`、`EndPortalFrameFilled`。

`BlockState.is_open` 已存在（`block.rs` 324；encode bit 4 在 349），箱子用它（`server_world.rs` 766）。門 **同時用兩軌**：redstone 寫 `BlockType::OakDoorOpen`（`redstone.rs` 1339–1341；測試 2049），mesh 又讀 `state.is_open`（`mesh.rs` 585、632；`voxel_shape.rs` 430–434）。`is_passable: self == BlockType::OakDoorOpen`（`block.rs` 1190）存在的原因就是型別與 state 都編了 open。

每對變體出現在 catalog、mesh、voxel_shape、redstone、inventory map、texture、worldgen（`FurnaceLit`／`EndPortalFrameFilled`）——13 discriminant × 6 張表的 match 臂。

## 前置

Plan 19（屬性表先就位；折變體只改表列與 state bit）。

## 精確 acceptance

- [ ] 13 個變體從 `BlockType` 刪除；powered／open／lit／filled／extended 由 `BlockState` bit（沿用 `is_open` 或新增 `is_powered`／`is_lit` bit）表示。
- [ ] redstone／mesh／voxel_shape／catalog／texture／worldgen／inventory 的對應臂改讀 state。
- [ ] **相容**：`BlockType::from_wire` 對 13 個舊 discriminant 建 alias 表 → (base type, state bits)；存檔載入時映射；wire 上 `BlockChange`／`ChunkData` 由 server 直接送新 id + state（client 也是新版，Plan 09 的 v20 之後）。舊 discriminant 保留為 reserved hole 直到下一個 bump。
- [ ] `is_passable` 門特例刪除（改讀 `state.is_open`）。
- [ ] 現有 redstone 門／燈／活塞／中繼器／比較器測試改 assert state 而非型別，全綠。
- [ ] 存檔 roundtrip 測試：舊格式含 `OakDoorOpen` 的欄載入後 mesh／redstone 行為一致。

## 預計檔案與測試

- 改：`src/world/{block,block_table,mesh}.rs`、`src/voxel_shape.rs`、`src/redstone.rs`、`src/inventory/catalog.rs`、`src/texture.rs`／`PACK_TILES`、`src/dimension.rs`（FurnaceLit／EndPortalFrame 放置）、`src/save/format.rs`（載入 alias）、`src/network/protocol.rs`（`from_wire` alias）
- 驗證：`cargo test --lib redstone:: world:: save:: inventory::`；`tests/waterlogging_authority.rs`；`tests/plan32_progression_travel.rs`（End portal frame）；`tests/review_hardening_chunk_restore.rs`

## 建議階段

1. 門／活板門（`is_open` 已在，只刪型別雙軌）。
2. 燈／熔爐（lit bit）。
3. 火把／中繼器／比較器／按鈕／拉桿／踏板（powered bit）。
4. 活塞（extended bit）與 End portal frame（filled bit）。
5. alias 表 + 舊存檔測試。

## 不在本計劃

- 重排 `BlockType` discriminant（保留 hole）。
- 新增更多 state 位元給非本計劃用途。
