# Plan15 — policy 幽靈 API 與 inventory hit 去重

## 定位

`presentation_inventory_policy.rs`：

- `should_mutate_world()`：Embedded／Join 皆 `false`；live 僅 `set_item_at_slot` 讀一次。
- `presentation_chunk_load_policy(role)` 與 `PresentationTopology::chunk_load_policy()` 重複。
- `FarmlandTrample`／`UnsupportedBreak` 無 `state.rs` 引用（Pickup 的 **本地收集** 已在 03 刪；enum 仍給測試）。

`state.rs` 的 `presentation_inventory_click_target` 與 `probe_inventory_click` 都跑 `collect_inventory_ui_hits` + `get_inventory_slots`；前者固定 `is_left = true`。

`MultiplayerRole` 與 `PresentationTopology` 實質只有 Join vs Embedded；Host／Singleplayer 差異在 port／username，不在 inventory 政策。

## 前置

03（Pickup 死分支已刪，才能安全考慮是否保留 `Pickup` enum 給測試）。

## 精確 acceptance

- [x] 刪 `should_mutate_world` 或內聯唯一呼叫點。
- [x] 只留一套 chunk load policy（`PresentationTopology` 或 role helper，不是兩套）。
- [x] `FarmlandTrample`／`UnsupportedBreak` 若仍無 production 引用則刪 variant 與測試。
- [x] 單一 `resolve_inventory_hit(mouse, is_left) -> Option<(PresentationInventoryTarget, InventoryHit)>`；`presentation_inventory_click_target`／`probe_inventory_click` 變薄包裝。
- [x] Embedded player inventory `LocalMutate` 與 Join reject 語意不變；merchant／recipe-book 測試通過。
- [x] 不把 `MultiplayerRole` 刪掉（menu join form 仍需要 addr／port／username）。

## 預計檔案與測試

- `src/presentation_inventory_policy.rs`、`src/state.rs`、`src/presentation_click.rs`
- 驗證：`cargo test --lib presentation_inventory_policy`；`cargo test --bin icraft -- presentation_click`

## 建議階段

1. 刪幽靈 fn／重複 chunk policy。
2. 合併 hit probe。
3. 刪未用 target enum（確認 grep）。

## 不在本計劃

- 補線 `handle_inventory_click` 的 `LocalMutate` no-op（行為決策，不是刪除）。
- 合併 `NetworkInbound`／`ClientToGame`／`RuntimePresentationEvent`。
- 拆 `state.rs` god file。

## 實作與證據

### 改了什麼

- 刪 `PresentationTopology::should_mutate_world`；`State::set_item_at_slot` 對 `ContainerSlot` 直接 early-return（語意等同「永遠 false」）。
- 刪 free fn `presentation_chunk_load_policy`；只留 `PresentationTopology::chunk_load_policy()`。menu／join-projection 測試改走 topology。
- 刪 `PresentationInventoryTarget::{FarmlandTrample, UnsupportedBreak}` 與對應測試；保留 `Pickup`（永遠 Reject，給 policy／hardening 測試）。
- `State::resolve_inventory_hit(is_left)` 統一 `probe` → `authority_hit` → target；`presentation_inventory_click_target` 變薄包裝（`is_left = true`）。`probe_inventory_click` 仍是唯一幾何 probe 實作（merchant 需完整 probe，無法只從 `Option<(Target, Hit)>` 還原）。
- `ARCHITECTURE.md`：補 chunk-load／resolve hit／刪 trample／unsupported 變體說明。
- `MultiplayerRole` 未刪。

### 測了什麼

- `cargo test --lib presentation_inventory_policy` → 4 passed
- `cargo test --bin icraft -- presentation_click` → 10 passed（含 merchant／recipe-book）
- `cargo test --test review_hardening_embedded_presentation --test review_hardening_join_projection` → 3+3 passed

### 留下的缺口

- `handle_inventory_click` 的 Embedded `LocalMutate` 仍是刻意 no-op（計劃排除補線）。
- `resolve_inventory_hit` 參數是 `&self` + `is_left`（mouse 來自 `self.mouse_ndc`），不是獨立 `mouse` 參數；語意與計劃一致。
- `presentation_click.rs` 本計劃未改（merchant／recipe-book 測試已覆蓋現有 probe 幾何）。
