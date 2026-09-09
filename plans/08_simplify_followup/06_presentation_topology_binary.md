# Plan06 — `PresentationTopology` 收成二值

## 定位

生產路徑 `debug_assert!(false, "LegacyOwner topology is unreachable...")`。`from_bools` 與 deprecated wrapper 只為測試相容。點擊／背包／chunk load 因此永遠帶第三個不該發生的世界擁有者模式。

## 前置

01。

## 精確 acceptance

- [ ] enum 只留 `Embedded | JoinClient`。
- [ ] 刪 `is_legacy_owner`、`from_bools`、四個 deprecated bool wrapper。
- [ ] 政策函式吃 enum，不要 `(has_runtime, is_client)`。
- [ ] `tests/review_hardening_embedded_presentation.rs` 改呼叫 enum 方法。
- [ ] `handle_click` / inventory / chunk load 不再有 LegacyOwner 臂。

## 預計檔案與測試

- `src/presentation_inventory_policy.rs`、`src/state.rs`、相關 tests
- 驗證：`cargo test --lib presentation_inventory_policy::`；review_hardening_embedded_presentation

## 建議階段

1. 把測試改成 `Embedded`／`JoinClient`。
2. 刪 variant 與 wrapper。
3. 清 match 窮盡性。

## 不在本計劃

- inventory click 改呼叫 `inventory_decision`（14）。
- 改權威拓撲。
