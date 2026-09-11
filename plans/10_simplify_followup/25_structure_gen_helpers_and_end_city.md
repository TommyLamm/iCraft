# Plan25 — 結構生成 helper 與雙 End City

## 定位

### 六個 generator 複製同一套樣板

`structure/gen/{dungeon,village,mineshaft,stronghold,nether_fortress,end_city}.rs` 都以相同的 `BoundingBox::new` + `StructureStart { pieces: vec![StructurePiece { blocks, bounding_box }] }` 結尾（dungeon 101–112、village 117–128、mineshaft 114–125、stronghold 181–192、fortress 109–120、end city 87–98）。內部迴圈都是 `for dx/dy/dz { is_wall = …; push BlockPlacement }`。箱子都是 `ChestBlockEntity { inventory: ContainerInventory::new(), loot_table: Some(...), loot_seed: Some(seed as u64 ^ CONST) }`。

`apply_structures_to_chunk`（`manager.rs` 103–123）在 `intersects_chunk` 之後又逐 placement 重算 `div_euclid(16)`。

### 兩套 End City 放置

`generate_structures`（`dimension.rs` 216–225）：
1. `StructureManager` 在 16-chunk grid 放 `StructureId::EndCity`，離原點 45 欄內跳過（`placement.rs` 70–110）。
2. `apply_fixed_end_city` **永遠**把 `generate_end_city(END_CITY_X=1032, END_CITY_Z=8, y=71)` 蓋在相交的欄（230–262）。

`end_surface_at` 又對同一點周圍 46 格 disc 特判（568–572）。`origin_y_for` EndCity = 64 vs `END_CITY_BASE_Y` = 71，兩套 Y 規則。

## 前置

無。Plan 24 先落地可直接用 `chunk_xz` helper。

## 精確 acceptance

- [ ] `fill_box`／`hollow_box`／`place_loot_chest`／`finish_start` 四個 helper；六個 generator 改用；淨減 ≥ 400 行。
- [ ] 生成結果 **byte-identical**：以固定 seed 對六種結構各 dump `Vec<BlockPlacement>` 排序後的 hash 鎖住，再替換。
- [ ] `apply_structures_to_chunk` 對已按欄過濾的 piece 直接寫入，不重算欄座標。
- [ ] End City 只剩一套放置：**刪 `apply_fixed_end_city`**，改為把 manager 的一個 start 釘在 `(1032, 8)`（保留玩家熟悉的固定城），或完全交給 grid；`END_CITY_BASE_Y` 與 `origin_y_for` 收成一個；`end_surface_at` 的 disc 特判與最終選擇一致。
- [ ] End 生成 determinism 測試（`dimension.rs` 1099–1116）依新規則更新並鎖住；`structure/manager.rs` 147+ 測試全綠；locate 對 EndCity 回傳與實際放置一致。
- [ ] `hash_structure` 常數不動（README §5）。

## 預計檔案與測試

- 改：`src/structure/gen/*.rs`、`src/structure/{manager,placement,locate,types}.rs`、`src/dimension.rs`
- 驗證：`cargo test --lib structure:: dimension::`；`tests/plan32_progression_travel.rs`（End 進度）；固定 seed 結構 hash 對比

## 建議階段

1. 結構 placement hash 測試（六種）。
2. helper 抽出，一個 generator 一個 commit。
3. apply path。
4. End City 單一放置（獨立 commit；會改 End 世界，需更新 determinism 測試）。

## 不在本計劃

- worldgen 欄填法（Plan 16）。
- 新結構或 loot 表內容。
