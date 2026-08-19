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

- [ ] `src/inventory.rs` 變成模組根（或 `src/inventory/mod.rs`），並 `pub use`
      舊路徑：`GameMode`、`Item`、`ItemStack`、`Inventory`、`apply_stack_click`、
      `ContainerInventory`、`CreativeTab`、`ToolType`、`ToolMaterial`。
      現有 `use crate::inventory::Item` **零改**。
- [ ] 至少拆出（名稱可微調，責任不可混）：
      - catalog：`Item` enum + `ItemProperties`／`item.properties()`
      - click：`apply_stack_click`、`StackClickResult`
      - 容器／玩家欄：`ContainerInventory`、`Inventory`
- [ ] 不得改 `Item` 的 enum 順序或 serde／bincode 形狀。
- [ ] 不得改 `apply_stack_click` 的守恒／空槽／拖曳語意。
- [ ] 不得把 `Item` 與 `BlockType` 合成。不得「順便」改 creative 網格常數。
- [ ] 既有測試期望值不變。

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
