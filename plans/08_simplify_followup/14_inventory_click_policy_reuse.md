# Plan14 — inventory click 走既有政策 helper

## 定位

`PresentationTopology::inventory_decision` 與 `collect_inventory_ui_hits` / `merchant_offer_at` / `enchant_option_at` 已存在。`handle_inventory_click`（`state.rs` ~9804）用 topology match 重寫 Join／Embedded／Workstation／Reject。`presentation_inventory_click_target` 重寫商人／附魔／配方書座標。`InventoryUiRect` 與 `MenuRect` 同欄位、同 `contains`。

Pickup／耕地已走 `inventory_decision`。

## 前置

06（enum 二值後 match 更短）。

## 精確 acceptance

- [ ] `handle_inventory_click` 的拓撲決策呼叫 `inventory_decision(target)`（merchant／container 特例須在證據對照現有行為）。
- [ ] `presentation_inventory_click_target` 改呼叫 `collect_inventory_ui_hits`（slot 仍可留在 State，因其需 live layout）。
- [ ] `InventoryUiRect` 變成 `MenuRect` 或 type alias。
- [ ] 現有 inventory／presentation_click 測試通過，期望值不變。

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
