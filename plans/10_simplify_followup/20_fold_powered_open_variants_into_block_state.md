# Plan20 — 13 對 powered／open `BlockType` 折進 `BlockState`

## 定位

以下變體是「另一個狀態的同一種方塊」：

`RedstoneTorchOff`、`RepeaterPowered`、`ComparatorPowered`、`StoneButtonPressed`、`LeverOn`、`PressurePlatePowered`、`PistonExtended`、`StickyPistonExtended`、`RedstoneLampLit`、`OakDoorOpen`、`OakTrapdoorOpen`、`FurnaceLit`、`EndPortalFrameFilled`。

`BlockState.is_open` 已存在（`block.rs` 324；encode bit 4 在 349），箱子用它（`server_world.rs` 766）。門 **同時用兩軌**：redstone 寫 `BlockType::OakDoorOpen`（`redstone.rs` 1339–1341；測試 2049），mesh 又讀 `state.is_open`（`mesh.rs` 585、632；`voxel_shape.rs` 430–434）。`is_passable: self == BlockType::OakDoorOpen`（`block.rs` 1190）存在的原因就是型別與 state 都編了 open。

每對變體出現在 catalog、mesh、voxel_shape、redstone、inventory map、texture、worldgen（`FurnaceLit`／`EndPortalFrameFilled`）——13 discriminant × 6 張表的 match 臂。

## 前置

Plan 19（屬性表先就位；折變體只改表列與 state bit）。

## 精確 acceptance

- [x] 13 個變體從 `BlockType` 刪除；powered／open／lit／filled／extended 由 `BlockState` bit（沿用 `is_open` 或新增 `is_powered`／`is_lit` bit）表示。
- [x] redstone／mesh／voxel_shape／catalog／texture／worldgen／inventory 的對應臂改讀 state。
- [x] **相容**：`BlockType::from_wire` 對 13 個舊 discriminant 建 alias 表 → (base type, state bits)；存檔載入時映射；wire 上 `BlockChange`／`ChunkData` 由 server 直接送新 id + state（client 也是新版，Plan 09 的 v20 之後）。舊 discriminant 保留為 reserved hole 直到下一個 bump。
- [x] `is_passable` 門特例刪除（改讀 `state.is_open`）。
- [x] 現有 redstone 門／燈／活塞／中繼器／比較器測試改 assert state 而非型別，全綠。
- [x] 存檔 roundtrip 測試：舊格式含 `OakDoorOpen` 的欄載入後 mesh／redstone 行為一致。

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

## 實作與證據

### 改了什麼

- 13 個 powered／open／lit／extended／filled 變體改名為 `Reserved50`…`Reserved90`（保留 discriminant hole）；遊戲路徑經 `canonicalize()`／`migrate_saved()` 映回 base + `BLOCK_STATE_OPEN_BIT`。
- 共用 bit 4 `is_open`：門／活板開、燈／熔爐 lit、活塞 extended、末地門框 filled、拉桿／按鈕／踏板／中繼器／比較器 powered；紅石火把例外（bit set = 熄滅）。
- Helpers：`light_emission_for`／`face_tex_for`／`is_solid_for`／`is_passable_for`；門 passable 不再靠型別特例。
- redstone／mesh／voxel_shape／catalog／authority／structure／save／UI 呼叫點改讀 state。
- `BlockPlacement.block_state`（serde default 0）；Stronghold 預填框寫 open bit。
- `ensure_chest_loot` 標記 chunk dirty（讓 loot 生成後能存檔；解阻 plan32 持久化斷言）。
- plan32 loot 測試：`ensure_chunk` → `materialize_chunk`（Async worldgen 後 `ensure_chunk` 不再同步落地）。
- 整合測試 `LeverOn` fixture 改為 `Lever` + open bit。

### 死路徑證據

```text
rg "BlockType::(OakDoorOpen|FurnaceLit|…|LeverOn|…)" src tests --glob "*.rs"
→ 僅註解／Reserved 說明／save 測試 legacy id 68；無 live producer。
```

### 測了什麼

| 命令 | 結果 |
| :--- | :--- |
| `cargo test --lib redstone::` | 31 ok |
| `cargo test --lib world::` | 88 ok |
| `cargo test --lib save::` | 50 ok（2 ignored） |
| `cargo test --lib inventory::` | 23 ok |
| `cargo test --test waterlogging_authority` | 5 ok |
| `cargo test --test review_hardening_chunk_restore` | 5 ok |
| `cargo test --test plan32_progression_travel`（除 dragon） | 4 ok |
| `cargo check --all-targets` | ok |
| `cargo check --bin icraft-server` | ok |

### 留下的缺口

- `dedicated_tcp_combat_completes_generated_dragon_lifecycle` 在 **Plan 20 前後（含 clean HEAD `421ae92`）** 均於等待 gameplay response 2（`/gamemode creative`）逾時；與本折疊無關的既有 TCP harness／End tick 負載問題。
- Reserved hole 仍佔 `BLOCK_TABLE` 槽位（註解列）；下一個 wire bump 才可真正刪 discriminant。
- `SoundId::FurnaceLit` 為音效 ID，刻意未改。
