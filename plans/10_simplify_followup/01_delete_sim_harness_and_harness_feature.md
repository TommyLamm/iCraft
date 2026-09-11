# Plan01 — 刪 `SimHarness`／`final_acceptance`／`harness` feature，gate 桌面 microbench

## 定位

`cargo test --lib` 每次都編一套與 `AuthorityCore` 無關的第二世界：

| 檔案 | 規模 | 證據 |
| :--- | :--- | :--- |
| `src/sim_harness.rs` | ~1,331 行 | `SimHarness` at 61；檔頭 1–8 自述「不是 AuthorityCore」；自帶 `ChunkManager`、redstone、entities、trades、`MerchantSessionManager`、minecart／mount tick |
| `src/final_acceptance.rs` | ~762 行 | 檔頭 1–8 同上；唯一 caller 是 `sim_harness` 自己的 `#[cfg(test)]`（~1302 起）與 `run_for_fps` |
| `Cargo.toml` `harness = []` | 第 10 行 | 全 repo（`*.rs`／`*.toml`／`*.ps1`／`*.yml`／`*.md`）**沒有任何 `--features harness` 使用者**；`cfg(test)` 已足以編這些模組 |
| `src/main.rs:27` | `mod microbench;` **無條件** | `--microbench` 在 50–52；每個桌面 release binary 都編一份 bench；lib 的 `microbench.rs`（87–88）再編一份 |

`sim_harness` 也是 leftover `tick_all_loaded_fluids`／`tick_all_loaded_hoppers`／`MinecartState::tick`／`MountManager` 唯一的非測試 caller（`sim_harness.rs` 1086、1098–1159、1177–1178）——刪掉它，Plan 02／04 的死殼判定就沒有例外。

Live 覆蓋已由 Plan30–34 + `review_hardening_*` 承擔（全部走 `ServerRuntime`／TCP harness）。

## 前置

無。Wave 09 §5「不刪 `SimHarness`／`final_acceptance` 整包」在本波解除。

## 精確 acceptance

- [ ] `src/sim_harness.rs`、`src/final_acceptance.rs` 刪除；`lib.rs` 82–90 與 `main.rs` 22 的對應 `cfg` 掛載消失。
- [ ] `Cargo.toml` 刪 `harness` feature，或僅保留給 `main.rs` `mod microbench` 用（擇一，README 註明）。
- [ ] `main.rs` 的 `mod microbench` 不再無條件編譜：`#[cfg(feature = "microbench")]`（或沿用 `harness`），`--microbench` 在未開 feature 時印出提示退出。
- [ ] lib `microbench.rs` 與 `main.rs` 版本二選一；另一份刪除或改 `#[path]` 共用。
- [ ] `cargo test --lib` 不再編任何 `sim_harness::`／`final_acceptance::` 符號；`cargo check --all-targets` 通過。
- [ ] `ARCHITECTURE.md` 刪 `sim_harness` / `final_acceptance` / `harness` 段落與「Tests」列的引用。

## 預計檔案與測試

- 刪：`src/sim_harness.rs`、`src/final_acceptance.rs`（可能連 lib `src/microbench.rs`）
- 改：`src/lib.rs`、`src/main.rs`、`Cargo.toml`、`ARCHITECTURE.md`
- 驗證：`cargo check --all-targets`；`cargo check --bin icraft-server`；`cargo test --lib`（總數下降但無新失敗）；`cargo run --features microbench -- --microbench` 仍可跑

## 建議階段

1. grep `sim_harness::`／`final_acceptance::`／`SimHarness`，確認除自身測試外零 caller。
2. 刪兩檔與 `cfg` 掛載，用編譯錯誤當清單；把仍被 harness 鎖定的 `tick_all_loaded_*` 引用交給 Plan 04。
3. 處理 microbench 雙份與 feature gate。
4. 更新 ARCHITECTURE 與 README 狀態。

## 不在本計劃

- 刪 `tick_all_loaded_*` wrapper 本體與 `world_tick`／`fluid` 單元測試改寫（Plan 04）。
- 刪 `MinecartState`／`MountManager`（Plan 02）。
- 測試瘦身（Plan 28）。
