# Plan02 — 刪除未接線的 AI 與生成系統原型

## 定位

在伺服器權威架構落地後，實體行為與生成已由 `ServerWorld::tick_entities`、`mob::spawn_mobs` 及 `passive_mob::spawn_passive_mobs` 統一接管。
早期開發的原型系統殘留在代碼庫中，僅在 `#[cfg(any(test, feature = "harness"))]` 下編譯或僅供自身單元測試調用，從未接入權威主循環：

| 檔案 / 目錄 | 行數 | 內容 |
| :--- | :--- | :--- |
| `src/ai/brain.rs`, `goal.rs`, `navigation.rs`, `mod.rs` | 510 行 | 基于 Goal-stack 的 AI 原型與 A* 尋路。權威端使用直接向量追逐 |
| `src/spawning.rs` | 315 行 | `SpawningSystem`, `MobCategory`, `SpawnReason` 類別上限生成原型，僅供其自身的單元測試調用 |

正如 `ARCHITECTURE.md` 第 43–44 行與 `plans/06_code_simplification/README.md` §7 所記錄，這些原型未上線且合併會更改既有玩法，應予徹底清理。

## 前置

無。可與 01、03–06 並行。

## 精確 acceptance

- [x] 徹底刪除整個 `src/ai/` 目錄（`brain.rs`, `goal.rs`, `navigation.rs`, `mod.rs`）。
- [x] 徹底刪除 `src/spawning.rs` 文件。
- [x] 在 `src/lib.rs` 中移除 `pub(crate) mod ai;` 與 `pub(crate) mod spawning;` 模組宣告。
- [x] `cargo check --all-targets` 通過。
- [x] 權威端生物模擬測試與整合測試不受影響。

## 預計檔案與測試

- 刪除：
  - `src/ai/brain.rs`
  - `src/ai/goal.rs`
  - `src/ai/navigation.rs`
  - `src/ai/mod.rs`
  - `src/spawning.rs`
- 修改：
  - `src/lib.rs`
- 驗證測試：
  - `cargo test --lib entity::`
  - `cargo test --lib mob::`
  - `cargo test --lib passive_mob::`
  - `cargo check --all-targets`

## 建議階段

1. 檢查是否有任何非測試代碼 `use crate::ai` 或 `use crate::spawning`（確認全為死碼）。
2. 刪除 `src/ai/` 與 `src/spawning.rs`。
3. 從 `src/lib.rs` 移除模組導出。
4. 運行編譯與單元測試。

## 不在本計劃

- 修改 `ServerWorld::tick_entities` 中的權威生物追逐邏輯。
- 修改 `mob::spawn_mobs` 或 `passive_mob::spawn_passive_mobs` 的生成機率與常數。

## 實作與證據

### 修改內容
1. 徹底刪除 `src/ai/` 目錄（`brain.rs`, `goal.rs`, `navigation.rs`, `mod.rs`，510 行）。
2. 徹底刪除 `src/spawning.rs`（315 行）。
3. 從 `src/lib.rs` 移除 `pub(crate) mod ai;` 與 `pub(crate) mod spawning;` 宣告。

### 驗證證據
- `cargo test --lib entity::` (26 passed; 0 failed)
- `cargo test --lib mob::` (5 passed; 0 failed)
- `cargo test --lib passive_mob::` (2 passed; 0 failed)
- `cargo check --all-targets` (通過)
