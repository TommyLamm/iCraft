# Plan14 — inventory click 走既有政策 helper

## 定位

`PresentationTopology::inventory_decision` 與 `collect_inventory_ui_hits` / `merchant_offer_at` / `enchant_option_at` 已存在。`handle_inventory_click`（`state.rs` ~9804）用 topology match 重寫 Join／Embedded／Workstation／Reject。`presentation_inventory_click_target` 重寫商人／附魔／配方書座標。`InventoryUiRect` 與 `MenuRect` 同欄位、同 `contains`。

Pickup／耕地已走 `inventory_decision`。

## 前置

06（enum 二值後 match 更短）。

## 精確 acceptance

- [x] `handle_inventory_click` 的拓撲決策呼叫 `inventory_decision(target)`（merchant／container 特例須在證據對照現有行為）。
- [x] `presentation_inventory_click_target` 改呼叫 `collect_inventory_ui_hits`（slot 仍可留在 State，因其需 live layout）。
- [x] `InventoryUiRect` 變成 `MenuRect` 或 type alias。
- [x] 現有 inventory／presentation_click 測試通過，期望值不變。

## 預計檔案與測試

- `src/state.rs`、`src/presentation_click.rs`、`src/presentation_inventory_policy.rs`、`src/menu.rs`
- 驗證：`cargo test --lib presentation_click::`；`presentation_inventory_policy::`；state inventory 測試

## 建議階段

1. hit-test 表合併。
2. `handle_inventory_click` 對照 `inventory_decision` 逐臂。
3. `InventoryUiRect` alias。

## 不在本計劃

- 讓 UI 點擊變成權威 commit。
- 刪 leftover inventory 路徑（應已在 01 消失）。

## 實作與證據

### 改了什麼

- `handle_inventory_click` 不再手寫 Join／Embedded match。slot／附魔／配方書先映到 `PresentationInventoryTarget`，再呼叫 `inventory_decision`。
- **Container**：兩邊拓撲都是 `SendAuthorityOp`，仍送 `ContainerClick`（與改前相同）。
- **Merchant**：不是 policy target。若當成 `Workstation` 會 `Reject`，但 live 路徑兩邊都送 `GameplayOperation::Trade`，因此維持特例、不走 `inventory_decision`。
- `presentation_inventory_click_target` 的商人／附魔／配方書座標改呼叫 `collect_inventory_ui_hits`（writeback 無滑鼠鍵，`is_left = true` 對齊舊的 always-on overlay）。slot 仍用 State live layout。
- `InventoryUiRect` 改成 `MenuRect` type alias。`menu.rs` 無需改動。

### 測了什麼

- `cargo test --lib presentation_click::`：10 passed。
- `cargo test --lib presentation_inventory_policy::`：4 passed（含 container = SendAuthorityOp、workstation = Reject 兩邊拓撲）。
- `cargo test --bin icraft inventory`：4 passed（含 slot→target 對照、既有 writeback／overflow／session inventory）。
- `cargo test --bin icraft creative_`：10 passed（rect alias 後 layout 不重疊）。

### 留下的缺口

- Merchant 仍不是 `PresentationInventoryTarget`；click 送 Trade，writeback 仍把它當 Workstation（不 writeback）。
- Embedded `LocalMutate` 仍是 no-op 本體；writeback 由 `app` 在 click 後依 `should_writeback_after_inventory_click` 觸發。UI 點擊不是權威 commit。
- leftover inventory 路徑已在 01 消失，本計劃未再刪。
- `NetworkHandle::Host` 仍在（03）。
