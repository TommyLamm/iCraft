# Plan21 — `catalog.rs`／`recipes.rs` 資料表化

## 定位

### `src/inventory/catalog.rs`（~2,544 行）

同一 `Item` enum 被六個平行 match 重描：`creative_tab`（730–947）、`tool_properties`（949–1140）、`armor_properties`（1146–1262）、`food_properties`（1279–1402）、`properties`（1415–2430，含 2070+ 巢狀群組 match）、`from_block`（2432–end）。`properties()` 幾乎每個方塊物品都重複 `max_stack: 64`／`is_block: true`／`block_type: Some(BlockType::X)`；工具列重複 material／durability／speed 於 sword／pick／axe／shovel。新增一個物品要改 4–6 個臂。

### `src/recipes.rs`（~1,152 行）

`RecipeManager::new`（103–~950）是 85 次 `add_shaped`；木材三連複製貼上：planks（110–130）、sticks（142–162）、crafting table（165–185）、chest（188–208）。`match_crafting_recipe`（964–1037）配 `active_items: Vec`、sort，再線性走**全部** recipes（shapeless 再 shaped）；`find_smelting_recipe`（953–957）線性；`match_recipe`（960）一行 alias。

## 前置

無。Plan 20 若先落地，`from_block` 的 lit／powered alias 列會消失。

## 精確 acceptance

- [ ] `static ITEM_DEFS: [ItemDef; N]`（name／max_stack／block／atlas／tab／tool／armor／food）以 discriminant 索引；六個方法改欄位讀取；`from_block` 由表反向生成一次（`OnceLock` 或 `const`）。
- [ ] 表長度與 enum 變體數 `const` assert；每變體一列測試。
- [ ] 所有屬性值 snapshot 對比 **byte-identical**。
- [ ] recipes：pattern 字串表 + `for wood in [Oak, Birch, Spruce]` 展開 plank-family；`RecipeManager::new` 縮到 ~200 行以內。
- [ ] shaped 以 (寬, 高, 第一格 item) 或 key 索引；smelting 以 `HashMap<Item, _>`；`match_recipe` alias 刪。
- [ ] 現有 crafting／furnace 測試（`recipes.rs` 1070+）全綠；新增「所有現有配方輸出不變」的 golden 測試（先由舊實作 dump）。

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
