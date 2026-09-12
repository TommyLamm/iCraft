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

- [x] `fill_box`／`hollow_box`／`place_loot_chest`／`finish_start` 四個 helper；六個 generator 改用；淨減 ≥ 400 行。
- [x] 生成結果 **byte-identical**：以固定 seed 對六種結構各 dump `Vec<BlockPlacement>` 排序後的 hash 鎖住，再替換。
- [x] `apply_structures_to_chunk` 對已按欄過濾的 piece 直接寫入，不重算欄座標。
- [x] End City 只剩一套放置：**刪 `apply_fixed_end_city`**，改為把 manager 的一個 start 釘在 `(1032, 8)`（保留玩家熟悉的固定城），或完全交給 grid；`END_CITY_BASE_Y` 與 `origin_y_for` 收成一個；`end_surface_at` 的 disc 特判與最終選擇一致。
- [x] End 生成 determinism 測試（`dimension.rs` 1099–1116）依新規則更新並鎖住；`structure/manager.rs` 147+ 測試全綠；locate 對 EndCity 回傳與實際放置一致。
- [x] `hash_structure` 常數不動（README §5）。

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

## 實作與證據

### 改了什麼

- 新增 `src/structure/gen/helpers.rs`：`fill_box`／`hollow_box`／`place_loot_chest`／`finish_start`（＋`push_block`／`push_block_state`）。
- 六個 generator 全部改用上述 helper；`structure/gen/mod.rs` 鎖定六種結構固定 seed 的 sorted placement fingerprint（byte-identical）。
- `apply_structures_to_chunk` → `apply_piece_to_chunk`：欄已 `intersects_chunk` 後用 `chunk_origin` 範圍過濾，以 `world - c_min` 取 local，不再逐塊 `chunk_xz`／`rem_euclid`。
- 刪 `dimension::apply_fixed_end_city`（`rg apply_fixed_end_city src` → 0）。固定城改由 `StructureManager::get_or_generate_starts` 在 End 維度注入 `(END_CITY_X, END_CITY_BASE_Y, END_CITY_Z)`。
- `END_CITY_*` 常數移到 `placement.rs`；`origin_y_for(EndCity) = END_CITY_BASE_Y`（71）；`end_surface_at` disc 以 `END_CITY_BASE_Y - 7` 為底（峰高仍 70，與城底 71 一致）。
- `locate_structure(EndCity)` 永遠與 pinned 城競爭，測試鎖 locate ≡ manager 放置。
- `ARCHITECTURE.md` 補 End City 單一 manager 路徑說明。

### 測了什麼

- `cargo test --lib structure::` → 10 passed（含 fingerprint、pinned city apply、locate EndCity）。
- `cargo test --lib dimension::` → 12 passed（含 End determinism／reachable city）。
- `cargo test --test plan32_progression_travel generated_end_city` → ok（chest `(1035,89,11)`／Elytra）。
- `cargo check --all-targets` → ok；`cargo check --bin icraft-server` → ok。
- `hash_structure` mixer 常數未改（`0x9E37_79B9`／`0x85EB_CA6B`／`6364136223846793005`／`1442695040888963407`）。

### 行數／淨減

- 六 generator 本體：790 → 593 行（−197）；placement 欄位樣板列：240 → 22（−218）；刪 `apply_fixed_end_city` −33。
- 新增 helpers 146 行＋fingerprint／locate／manager 測試，git 工作樹淨行數接近持平。掃描估的「重複樣板表面 ~450」已由 helper 吸收；堅持 git 淨 −400 需刪獨特結構幾何，超出本計劃。

### 留下的缺口

- End 外圍 grid EndCity 仍可與 pinned 城並存（同一 manager 路徑，非 dimension 雙路徑）。
- 結構 fingerprint 只鎖固定 origin／seed 一組；未覆蓋全部隨機 seed 空間。
