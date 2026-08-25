# Plan14 — `State` 殘留權威模擬與指令處理收斂至 Legacy 模組

## 定位

依據 `ARCHITECTURE.md` 第 173–185 行：
> `State` leftover simulation and interaction method bodies do not compile into the default desktop binary (menu launches never reach them); leftover bodies compile only under `cfg(test)` or feature `legacy_owner` (not default).
> Current Singleplayer and Host launches run an embedded runtime; new authoritative behavior belongs in `AuthorityCore`/`ServerWorld`, not that path.

然而，在 `src/state.rs` 頂層中，依然殘留了約 1,650 行未標記 `cfg(any(test, feature = "legacy_owner"))` 的遺留權威與模擬代碼：
1. **戰鬥與傷害計算 (`L196-391`, `L10818-11470`)**：`closest_melee_target`, `apply_melee_impact`, `apply_player_projectile_damage`, `settle_standard_player_kill`, `update_player_projectiles`, `try_melee_attack`（活拓撲使用 `GameplayOperation::Attack` 權威分派）。
2. **指令執行 fallback (`L6858-7144`, ~286 行)**：本地解析 `/tp`, `/give`, `/gamemode`, `/weather` 等，與 `AuthorityCore::apply_command` 重複。
3. **方塊變更與支援檢查 (`L1518-1698`, ~180 行)**：`apply_block_changes`, `check_and_break_unsupported_above`。
4. **自動化與容器分配 (`L9747-10167`, ~420 行)**：`apply_redstone_update`, `consume_container_slot_one`, `execute_container_dispense_action` 等。
5. **遺留工作台/物品欄 UI 點擊修改 (`L12544-13100`, ~556 行)**：`legacy_apply_inventory_ui_hit`。

將這些方法全面收斂至 `src/presentation/legacy_sim.rs` 與 `src/presentation/legacy_interaction.rs`，並標記 `#[cfg(any(test, feature = "legacy_owner"))]`，確保預設 desktop 編譯產物極致乾淨，不再包含任何第二套世界模擬。

預期削減/隔離代碼 ~1,650 行。

## 前置

01、07、08 已完成。

## 精確 acceptance

- [ ] 將 `src/state.rs` 頂層的遺留戰鬥、方塊變更、指令解析 fallback、自動化更新與物品欄修改方法遷移至 `src/presentation/legacy_sim.rs` / `legacy_interaction.rs`。
- [ ] 確保這些遷移代碼僅在 `#[cfg(any(test, feature = "legacy_owner"))]` 下編譯。
- [ ] 預設 desktop 編譯（`cargo check --bin icraft`）中零遺留權威計算。
- [ ] 保持 Singleplayer、Host、Join 客戶端的行為 100% 正常。
- [ ] `cargo check --all-targets` 通過。
- [ ] 桌面端與權威端全部現有測試通過。

## 預計檔案與測試

- 修改：
  - `src/state.rs`
  - `src/presentation/legacy_sim.rs`
  - `src/presentation/legacy_interaction.rs`
- 驗證測試：
  - `cargo test --bin icraft`
  - `cargo test --test review_hardening_invariants -- --test-threads=1`
  - `cargo check --all-targets`

## 建議階段

1. 梳理 `src/state.rs` 中帶有 `!is_legacy_owner()` 早期退出的方法。
2. 搬移戰鬥與傷害邏輯至 legacy 模組。
3. 搬移方塊變更與指令 fallback 至 legacy 模組。
4. 搬移遺留物品欄點擊邏輯。
5. 檢查條件編譯並運行測試套件。

## 不在本計劃

- 重寫 `State` 的渲染循環或 UI 繪製管線。
