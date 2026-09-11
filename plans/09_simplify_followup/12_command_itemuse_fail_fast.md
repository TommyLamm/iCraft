# Plan12 — Command／ItemUse fail-fast

## 定位

`GameplayOperation::ItemUse`：僅 `item_kind.food_properties()` 成功；劍／工具等進 `dispatch.rs` 才 `RejectReason::Unsupported`。`validate_bounds` 只查 item／count。Client 可能反覆送必 reject 的 op，浪費 sequencing／cache。測試 `unsupported_tool_item_use` 鎖的是 dispatch 後的 Unsupported。

`apply_command` 大段永遠 Unsupported：`Help`、`Difficulty`、`Weather`、`Kill`、`SpawnPoint`、`SetWorldSpawn`、`Locate`、`Seed`、`SaveAll`。Desktop `state.rs` 本地處理 Help UI；dedicated console 是另一入口。Gameplay 路徑只實作 `GameRule`／`Time`／`Give`／`GameMode` 等。

行為應保持「仍 reject」，只把失敗前移並縮小 match。**不 bump protocol**。

## 前置

無。若 02 已刪 `ServerWorld::dispatch`，不要再加回 command 分叉。

## 精確 acceptance

- [ ] `GameplayRequest::validate_bounds`：`ItemUse` 在非食物時回 `InvalidState` 或既有 reject（更新 `unsupported_tool_item_use` 期望）。
- [ ] `commands::parse` 能區分 `ConsoleOnly` vs `GameplayAllowed`，或 `validate_bounds` 拒絕 console-only 字串。
- [ ] `apply_command` 只留已實作 arm；Help 等不再佔 match。
- [ ] Desktop 本地 Help UI 不變；`/give` 等 live command 行為不變。
- [ ] `cargo test --lib authority::` 通過。

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
