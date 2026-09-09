# Plan01 — 刪 leftover 模擬與 `legacy_owner`

## 定位

Live Singleplayer / Host 從不進入 leftover 世界擁有者方法，但 `#[cfg(any(test, feature = "legacy_owner"))]` 讓 `cargo test` 仍編譯約 5600 行舊模擬：

| 檔案 | 規模 | 原因 |
| :--- | :--- | :--- |
| `src/presentation/legacy_sim.rs` | ~2164 行 | 只給 leftover 世界 tick；掛在 `state.rs` 的 `cfg` |
| `src/presentation/legacy_interaction.rs` | ~2874 行 | leftover 點擊／物品使用／方塊突變 |
| `src/presentation/legacy_systems.rs` | ~594 行 | leftover 村莊／突襲／載具／釣魚／熔爐 |
| `Cargo.toml` `legacy_owner` | 非 default feature | 生產不可達 |
| `src/state.rs` | ~60 處同 cfg、~40 處 `is_legacy_owner()` | 三分支強迫全樹 |

模組自己寫「Live Singleplayer / Host never enter these methods」。選單啟動也不會走到。

## 前置

無。06 必須等本計劃完成。

## 精確 acceptance

- [ ] 刪除三個 leftover 模組檔與 `state.rs` 的 `#[path]` 掛載。
- [ ] 刪除 feature `legacy_owner`。
- [ ] 生產 `handle_click` 只 match Embedded / Join。
- [ ] 所有 `is_legacy_owner()` 生產分支消失；測試改走 Embedded runtime 或刪只鎖 leftover 的用例。
- [ ] `cargo check --all-targets` 與 `cargo test` 通過。

## 預計檔案與測試

- 修改：`src/state.rs`、`src/presentation/mod.rs`、`src/presentation/bootstrap.rs`、`src/presentation/network_event.rs`、`Cargo.toml`、相關 `tests/`
- 刪除：`legacy_sim.rs`、`legacy_interaction.rs`、`legacy_systems.rs`
- 驗證：`cargo check --all-targets`；`cargo test --lib` 中仍引用 leftover 的測試改寫或刪除

## 建議階段

1. 列出所有 `legacy_owner` / `is_legacy_owner` 引用。
2. 先刪 feature 與模組掛載，用編譯錯誤當清單。
3. 把測試改成 embedded `ServerRuntime` 或刪除。
4. 清 `state.rs` 死分支。

## 不在本計劃

- 刪 `NetworkHandle::Host`（03）、presentation `SaveManager`（04）、`PresentationTopology::LegacyOwner` 型別（06）。
- 改協定或存檔格式。
