# Plan04 — 刪 presentation 存檔與 `world_mutation`

## 定位

非 `legacy_owner` 時 `bootstrap.rs` 永遠 `save_manager = None`。`trigger_background_save` 對非 LegacyOwner 立刻 `Ok(())`。`world_mutation::apply_batch` 只服務 leftover renderer（`state.rs` cfg 與 `legacy_sim.rs`）。架構已寫它不是 headless 權威突變根。

Embedded 存檔只應留 `ServerRuntime`。

## 前置

01、03。

## 精確 acceptance

- [ ] 刪 `State.save_manager`、`mutation_revisions`、`pending_player_catchups`（若 03 未刪完）。
- [ ] 刪 bootstrap leftover `SaveManager` 建構。
- [ ] 刪 `src/world_mutation.rs` 與 `lib.rs` 的 `pub mod world_mutation`。
- [ ] 權威繼續只用 `ServerWorld` / `AuthorityCore`。
- [ ] `cargo test --lib` 中 `apply_batch` 測試遷走或刪除。

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
