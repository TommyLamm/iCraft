# Plan30 — `inventory.rs` 目錄／click 拆檔

## 定位

`src/inventory.rs` 約 3,484 行，三件產品擠在一個 crate 檔：

| 約略行 | 內容 |
| --- | --- |
| 4–10 | `GameMode` |
| 12–640 | `Item` 巨 enum |
| 641–735 | Creative tab、tool／armor／food properties |
| 736–908 | `ItemStack`、`ContainerInventory` |
| 909–1005 | `apply_stack_click` |
| 1006–2829 | `ItemProperties` 目錄表 |
| 2830– | `Inventory` |

18 已拆 `world.rs` 並明確留下本項。`Item` 與 `BlockType` **不得**合成一個 enum
（wire／存檔／properties 都分開）。權威容器路徑用 `simulate_container_click`／
`apply_stack_click`，不要在拆檔時改 click 守恒。

## 前置

18 已完成。可與 26、27、29 並行。不要跟 23 搶 `world/block.rs`（本計劃不改
`BlockType`）。

## 精確 acceptance

- [x] `src/inventory.rs` 變成模組根（`src/inventory/mod.rs`），並 `pub use`
      舊路徑：`GameMode`、`Item`、`ItemStack`、`Inventory`、`apply_stack_click`、
      `ContainerInventory`、`CreativeTab`、`ToolType`、`ToolMaterial`。
      現有 `use crate::inventory::Item` **零改**。
- [x] 至少拆出（名稱可微調，責任不可混）：
      - catalog：`Item` enum + `ItemProperties`／`item.properties()`（`src/inventory/catalog.rs`）
      - click：`apply_stack_click`、`StackClickResult`（`src/inventory/click.rs`）
      - 容器／玩家欄：`ContainerInventory`、`Inventory`（`src/inventory/container.rs`，附 `src/inventory/stack.rs`）
- [x] 不得改 `Item` 的 enum 順序或 serde／bincode 形狀。
- [x] 不得改 `apply_stack_click` 的守恒／空槽／拖曳語意。
- [x] 不得把 `Item` 與 `BlockType` 合成。不得「順便」改 creative 網格常數。
- [x] 既有測試期望值不變。

## 預計檔案與測試

- 新增：`src/inventory/mod.rs`（或根檔 + 子檔）、catalog／click／container 目的地。
- 修改：`src/lib.rs`／`src/main.rs` 只在 `mod inventory` 路徑改變時改一行。
- 測試：
  - `cargo test --lib inventory::`
  - `cargo test --test review_hardening_container_click -- --test-threads=1`
  - `cargo test --test plan34_container_break_inventory_conservation -- --test-threads=1`
  - `cargo test --lib presentation_inventory_policy::`
  - `cargo check --all-targets`

## 建議階段

1. 先搬 `Item` + properties 目錄，根檔 `pub use`。`cargo test --lib inventory::`。
2. 搬 `apply_stack_click`。跑 container conservation。
3. 搬 `Inventory`／`ContainerInventory`。

## 不在本計劃

- 拆 `ContainerSessionManager`（仍在 §7）。
- 改 creative 物品欄 UI（那是 `state.rs`）。
- leftover 物品欄路徑。
- workspace `icraft-item` crate。

## 實作與證據

### 1. 改動內容

將原本 3,628 行的單一檔案 `src/inventory.rs` 依責任拆分為子模組目錄 `src/inventory/`：
- [`src/inventory/mod.rs`](file:///F:/Desktop/iCraft/src/inventory/mod.rs)：模組根，宣告子模組並以 `pub use` 重新匯出所有 public 型別與函式，維持既有 `crate::inventory::*` 路徑零破壞。
- [`src/inventory/catalog.rs`](file:///F:/Desktop/iCraft/src/inventory/catalog.rs)：`Item` enum（155 個 variant 順序及 discriminant 100% 保持）、`ALL_ITEMS`、`ItemProperties`、`CreativeTab`、`CREATIVE_*` 常數、`ToolType`、`ToolMaterial`、`ToolProperties`、`ArmorSlot`、`ArmorProperties`、`FoodProperties` 及 `impl Item` 方法（`properties`、`tool_properties`、`armor_properties`、`food_properties`、`renders_flat`、`from_block` 等）。
- [`src/inventory/stack.rs`](file:///F:/Desktop/iCraft/src/inventory/stack.rs)：`GameMode`、`CreativeDragOrigin`、`ItemStack` 結構體與 `impl ItemStack`。
- [`src/inventory/click.rs`](file:///F:/Desktop/iCraft/src/inventory/click.rs)：`StackClickResult` 結構體與 `apply_stack_click` 堆疊點擊合併／分割邏輯。
- [`src/inventory/container.rs`](file:///F:/Desktop/iCraft/src/inventory/container.rs)：`ContainerInventory`（方塊實體容器）與 `Inventory`（玩家物品欄）及其所有操作方法。
- [`src/inventory/tests.rs`](file:///F:/Desktop/iCraft/src/inventory/tests.rs)：23 個單元測試原汁原味遷移至子模組測試。

### 2. 測試證據

- `cargo test --lib inventory::`：23 passed, 0 failed
- `cargo test --test review_hardening_container_click -- --test-threads=1`：9 passed, 0 failed
- `cargo test --test plan34_container_break_inventory_conservation -- --test-threads=1`：4 passed, 0 failed
- `cargo test --lib presentation_inventory_policy::`：4 passed, 0 failed
- `cargo check --all-targets`：0 errors
- `cargo test --lib`：737 passed, 0 failed, 3 ignored

### 3. 留下的缺口

無本計劃範疇內的缺口。`ContainerSessionManager` 屬於 §7 明確不在本路線的延伸拆分。

