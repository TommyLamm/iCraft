# Plan04 — 刪 presentation 存檔與 `world_mutation`

## 定位

非 `legacy_owner` 時 `bootstrap.rs` 永遠 `save_manager = None`。`trigger_background_save` 對非 LegacyOwner 立刻 `Ok(())`。`world_mutation::apply_batch` 只服務 leftover renderer（`state.rs` cfg 與 `legacy_sim.rs`）。架構已寫它不是 headless 權威突變根。

Embedded 存檔只應留 `ServerRuntime`。

## 前置

01、03。

## 精確 acceptance

- [x] 刪 `State.save_manager`、`mutation_revisions`、`pending_player_catchups`（若 03 未刪完）。
- [x] 刪 bootstrap leftover `SaveManager` 建構。
- [x] 刪 `src/world_mutation.rs` 與 `lib.rs` 的 `pub mod world_mutation`。
- [x] 權威繼續只用 `ServerWorld` / `AuthorityCore`。
- [x] `cargo test --lib` 中 `apply_batch` 測試遷走或刪除。

## 預計檔案與測試

- `src/state.rs`、`src/presentation/bootstrap.rs`、`src/lib.rs`
- 刪：`src/world_mutation.rs`
- 驗證：`cargo check --all-targets`；權威 persistence 測試仍過

## 建議階段

1. 確認預設桌面啟動 `save_manager: None`。
2. 刪欄位與空存檔路徑。
3. 刪 `world_mutation` 模組與測試。

## 不在本計劃

- 改 `SaveManager` region 格式（08）。
- 改 `lib.rs` 其他桌面模組可見性（16）。

## 實作與證據

### 改了什麼

- 刪 `State.save_manager`、`mutation_revisions` 與 leftover presentation 存檔路徑（bootstrap 不再建 `SaveManager`；`trigger_background_save` 與 presentation entity/chunk restore 一併拿掉）。`pending_player_catchups` 已在 03 刪完。
- 刪 `src/world_mutation.rs` 與 `lib.rs` / `main.rs` 的 `world_mutation` 模組。隨機 tick 仍用的 `BlockMutationRequest` / `MutationCause` 遷到 `world_tick`；權威繼續只經 `ServerWorld::set_block` 套用。
- `ARCHITECTURE.md`：桌面 `State` 沒有 `SaveManager`；`mutation_revisions.bin` 只由 `ServerRuntime` 寫。

### 測了什麼

- `cargo check --all-targets`：通過。
- `cargo test --test authority_persistence -- --test-threads=1`：4 passed。
- `cargo test --lib apply_batch -- --list`：0 tests（leftover `apply_batch` 測試已刪）。
- `cargo test --lib world_tick::tests -- --test-threads=1`：18 passed。

### 留下的缺口

- `PresentationTopology::LegacyOwner` 仍在型別上（06）。
- 不可達 leftover spawn 路徑仍會在 GPU 執行緒 worldgen，但不讀不寫存檔。
- `SaveManager::save_mutation_revision_index_unless_runtime` 與 region 格式未改（08）。
- `lib.rs` 其他桌面模組可見性未改（16）。
