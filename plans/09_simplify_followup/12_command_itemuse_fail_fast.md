# Plan12 — Command／ItemUse fail-fast

## 定位

`GameplayOperation::ItemUse`：僅 `item_kind.food_properties()` 成功；劍／工具等進 `dispatch.rs` 才 `RejectReason::Unsupported`。`validate_bounds` 只查 item／count。Client 可能反覆送必 reject 的 op，浪費 sequencing／cache。測試 `unsupported_tool_item_use` 鎖的是 dispatch 後的 Unsupported。

`apply_command` 大段永遠 Unsupported：`Help`、`Difficulty`、`Weather`、`Kill`、`SpawnPoint`、`SetWorldSpawn`、`Locate`、`Seed`、`SaveAll`。Desktop `state.rs` 本地處理 Help UI；dedicated console 是另一入口。Gameplay 路徑只實作 `GameRule`／`Time`／`Give`／`GameMode` 等。

行為應保持「仍 reject」，只把失敗前移並縮小 match。**不 bump protocol**。

## 前置

無。若 02 已刪 `ServerWorld::dispatch`，不要再加回 command 分叉。

## 精確 acceptance

- [x] `GameplayRequest::validate_bounds`：`ItemUse` 在非食物時回 `InvalidState` 或既有 reject（更新 `unsupported_tool_item_use` 期望）。
- [x] `commands::parse` 能區分 `ConsoleOnly` vs `GameplayAllowed`，或 `validate_bounds` 拒絕 console-only 字串。
- [x] `apply_command` 只留已實作 arm；Help 等不再佔 match。
- [x] Desktop 本地 Help UI 不變；`/give` 等 live command 行為不變。
- [x] `cargo test --lib authority::` 通過。

## 預計檔案與測試

- `src/authority/dispatch.rs`、`src/network/protocol.rs`（`validate_bounds`）、`src/commands/mod.rs`
- `src/state.rs`（僅確認 Help 仍本地，不改協議）
- 驗證：`cargo test --lib -- unsupported_tool_item_use`；command 相關 authority 測試

## 建議階段

1. ItemUse：validate_bounds 收窄，更新測試期望。
2. Command：parser 或 bounds 標 console-only，縮小 `apply_command`。

## 不在本計劃

- 實作 Weather／Kill 等新 gameplay 命令。
- 刪 `Action::Use` wire。
- 改 `Container { action: 1 }` gap。

## 實作與證據

### 改了什麼

- `GameplayRequest::validate_bounds`：非食物 `ItemUse` → `InvalidState`（在 sequencing 之前）。
- `commands::CommandSurface::{GameplayAllowed, ConsoleOnly}` + `Command::surface()`；console-only 字串在 `validate_bounds` 回 `Unsupported`（`/respawn` 仍為 lifecycle 特例）。
- `AuthorityCore::apply_command`：只保留 `GameMode`／`Teleport`／`Give`／`GameRule`／`Time`；Help 等不再佔具名 arm（防禦性 `_`）。
- Desktop `state.rs` `execute_command_line`／Help UI 未改；`ARCHITECTURE.md` 補 fail-fast 契約一句。
- 需過 bounds 才能測 revision／sequence 的 fixture 改用 `Item::Bread`（authority／server_runtime／network／integration）。

### 測了什麼

- `cargo test --lib -- unsupported_tool_item_use` → ok（期望 `InvalidState`，且不消耗 sequence）。
- `cargo test --lib -- console_only_command parser_marks_console_only` → ok。
- `cargo test --lib authority::` → 66 passed。
- 相關窄測：`duplicate_and_stale_revision`、`gameplay_requests_are_idempotent`、`request_rate_limit`、`headless_two_sessions` → ok。

### 留下的缺口

- Weather／Kill／Seed 等仍未實作 gameplay；僅提早 reject。
- `apply_item_use`／`apply_command` 仍保留非食物／ConsoleOnly 的防禦性 reject（正常路徑已不會到達）。
- 未跑完整 integration suite（只改了會被 fail-fast 誤傷的 fixture）。
