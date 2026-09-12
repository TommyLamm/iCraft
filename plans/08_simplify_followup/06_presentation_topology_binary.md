# Plan06 — `PresentationTopology` 收成二值

## 定位

生產路徑 `debug_assert!(false, "LegacyOwner topology is unreachable...")`。`from_bools` 與 deprecated wrapper 只為測試相容。點擊／背包／chunk load 因此永遠帶第三個不該發生的世界擁有者模式。

## 前置

01。

## 精確 acceptance

- [x] enum 只留 `Embedded | JoinClient`。
- [x] 刪 `is_legacy_owner`、`from_bools`、四個 deprecated bool wrapper。
- [x] 政策函式吃 enum，不要 `(has_runtime, is_client)`。
- [x] `tests/review_hardening_embedded_presentation.rs` 改呼叫 enum 方法。
- [x] `handle_click` / inventory / chunk load 不再有 LegacyOwner 臂。

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

## 實作與證據

### 改了什麼

- `PresentationTopology` 只留 `Embedded | JoinClient`。非 Join 一律 `Embedded`；`from` 對非 Join 仍 `debug_assert` 必須有 in-process runtime。
- 刪 `is_legacy_owner`、`from_bools`，以及 `should_mutate_presentation_world`、`presentation_inventory_decision`、`should_sync_authority_inventory_from_local`、`should_writeback_after_inventory_click` 四個 bool wrapper。
- `inventory_decision` / `should_mutate_world` / writeback / chunk-load 政策只吃 enum。世界突變永遠 false；容器 click 送權威、工作站／撿物／踩田／無支撐破壞一律 Reject。
- `handle_click` 直接走 live 路徑；`handle_inventory_click` 窮盡 Embedded／Join；chunk load 本來就只看 `chunk_load_policy()`。
- `review_hardening_embedded_presentation` 改呼叫 enum 方法；刪 leftover「無 runtime 仍本地 mutate」用例。

### 測了什麼

- `cargo test --lib presentation_inventory_policy::`：3 passed。
- `cargo test --test review_hardening_embedded_presentation`：3 passed。

### 留下的缺口

- `handle_inventory_click` 仍手寫 Join／Embedded 臂，未改呼叫 `inventory_decision`（14）。
- `NetworkHandle::Host` 仍在（03）。
- presentation `SaveManager` 仍在（04）。
