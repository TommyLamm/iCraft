# Plan21 — `catalog.rs`／`recipes.rs` 資料表化

## 定位

### `src/inventory/catalog.rs`（~2,544 行）

同一 `Item` enum 被六個平行 match 重描：`creative_tab`（730–947）、`tool_properties`（949–1140）、`armor_properties`（1146–1262）、`food_properties`（1279–1402）、`properties`（1415–2430，含 2070+ 巢狀群組 match）、`from_block`（2432–end）。`properties()` 幾乎每個方塊物品都重複 `max_stack: 64`／`is_block: true`／`block_type: Some(BlockType::X)`；工具列重複 material／durability／speed 於 sword／pick／axe／shovel。新增一個物品要改 4–6 個臂。

### `src/recipes.rs`（~1,152 行）

`RecipeManager::new`（103–~950）是 85 次 `add_shaped`；木材三連複製貼上：planks（110–130）、sticks（142–162）、crafting table（165–185）、chest（188–208）。`match_crafting_recipe`（964–1037）配 `active_items: Vec`、sort，再線性走**全部** recipes（shapeless 再 shaped）；`find_smelting_recipe`（953–957）線性；`match_recipe`（960）一行 alias。

## 前置

無。Plan 20 若先落地，`from_block` 的 lit／powered alias 列會消失。

## 精確 acceptance

- [x] `static ITEM_DEFS: [ItemDef; N]`（name／max_stack／block／atlas／tab／tool／armor／food）以 discriminant 索引；六個方法改欄位讀取；`from_block` 由表反向生成一次（`OnceLock` 或 `const`）。
- [x] 表長度與 enum 變體數 `const` assert；每變體一列測試。
- [x] 所有屬性值 snapshot 對比 **byte-identical**。
- [x] recipes：pattern 字串表 + `for wood in [Oak, Birch, Spruce]` 展開 plank-family；`RecipeManager::new` 縮到 ~200 行以內。
- [x] shaped 以 (寬, 高, 第一格 item) 或 key 索引；smelting 以 `HashMap<Item, _>`；`match_recipe` alias 刪。
- [x] 現有 crafting／furnace 測試（`recipes.rs` 1070+）全綠；新增「所有現有配方輸出不變」的 golden 測試（先由舊實作 dump）。

## 預計檔案與測試

- 改：`src/inventory/catalog.rs`（可能拆 `catalog/{items,defs}.rs`）、`src/recipes.rs`
- 驗證：`cargo test --lib inventory:: recipes::`；`tests/plan34_container_break_inventory_conservation.rs`；`tests/review_hardening_container_click.rs`

## 建議階段

1. catalog snapshot 測試 → 建表 → 六方法改查表。
2. recipes golden dump → 表化 + 木材展開。
3. recipe 索引。

## 不在本計劃

- `catalog.rs` 檔案切分之外的 inventory 模組重構。
- 新增配方或改玩法數值。

## 實作與證據

### 改了什麼

- 新增 `src/inventory/item_table.rs`：`ItemDef` + `ITEM_DEFS: [ItemDef; ITEM_COUNT]`（214 列，與 `Item::Observer as usize + 1` 對齊）。
- `catalog.rs`：六個屬性方法改讀 `ITEM_DEFS`；`from_block` 以 `OnceLock<[Item; BLOCK_TYPE_COUNT]>` 反向生成（first-wins）並覆寫非 1:1 別名（`WheatCrop→Wheat`、`SnowLayer→Snow`、`Farmland→Dirt` 等）。
- 鎖定 snapshot：`item_property_snapshot.txt`、`from_block_snapshot.txt`。
- `recipes.rs`：`SHAPED`／`SHAPELESS`／`SMELTING` 靜態表；`WOODS` 展開 planks／sticks／crafting table／chest；`RecipeManager::new` ≈ 134 行；shaped 索引 `(w,h,pattern[0][0])`；smelting `HashMap<Item, usize>`。
- 刪除死 alias `match_smelting`（計劃文中的 `match_recipe`；基線僅 `match_smelting` → `find_smelting_recipe`）。
- `ARCHITECTURE.md` 補 `ITEM_DEFS`／recipe 索引契約。

### 測了什麼

- `cargo test --lib inventory::` → 26 ok（含 `item_defs_cover_every_variant`、兩份 snapshot）。
- `cargo test --lib recipes::` → 6 ok（含 golden、`new` ≤200 行）。
- `cargo test --test plan34_container_break_inventory_conservation` → 4 ok。
- `cargo test --test review_hardening_container_click` → 9 ok。
- `cargo check --all-targets`、`cargo check --bin icraft-server` → ok。

### 死路徑證據（`match_smelting`）

- 刪除前：`git show HEAD:src/recipes.rs` 僅定義 `match_smelting`（一行轉發 `find_smelting_recipe`）。
- 全樹 `rg match_smelting src tests`：除該定義外 **0 callers**（無 live producer／cfg）。
- 刪除後同搜尋：0 hits。

### 留下的缺口

- `Item::Wool` 歷史上 `block_type: Some(Snow)` 仍保留（byte-identical；未改玩法數值）。
- recipe golden 以 id 排序比對（不鎖定註冊順序）；shapeless 仍線性掃描（集合很小）。
- `catalog.rs` 未再拆成 `catalog/{items,defs}.rs`（計劃允許、非必須）。
