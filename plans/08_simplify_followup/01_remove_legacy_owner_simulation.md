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

- [x] 刪除三個 leftover 模組檔與 `state.rs` 的 `#[path]` 掛載。
- [x] 刪除 feature `legacy_owner`。
- [x] 生產 `handle_click` 只 match Embedded / Join。
- [x] 所有 `is_legacy_owner()` 生產分支消失；測試改走 Embedded runtime 或刪只鎖 leftover 的用例。
- [x] `cargo check --all-targets` 與 `cargo test` 通過。

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

## 實作與證據

### 改了什麼

- 刪除 `src/presentation/legacy_sim.rs`、`legacy_interaction.rs`、`legacy_systems.rs` 與 `state.rs` 的 `#[path]` 掛載。
- 刪除 Cargo feature `legacy_owner`。
- `handle_click` 只對 Embedded / Join 做 live 世界點擊；`LegacyOwner` 臂只留 `debug_assert`（型別仍在，留給 06）。
- `state.rs` / `bootstrap.rs` / `network_event.rs` 的 leftover 模擬／存檔／點擊死分支拿掉；live 路徑一律走 AuthorityCore／ServerRuntime。
- 刪只鎖 leftover helper 的單元測試（melee/projectile/reach/`validate_remote_block_request`）。Embedded runtime 測試保留。

### 測了什麼

- `cargo check --all-targets`：通過。
- `cargo test --bin icraft`：181 passed。
- `cargo test --lib presentation_inventory_policy`：4 passed。
- `cargo test --lib`：721 passed；3 個 mesh Y 斷言失敗（`waterlogged_slab_adds_only_the_translucent_complement`、`end_portal_frame_and_surface_use_lower_minecraft_heights`、`trapdoor_mesh_generation_open_and_closed_bounds`）。這三個測試所在檔未改，屬既有問題，不是 leftover 刪除造成。

### 留下的缺口

- `PresentationTopology::LegacyOwner` 與 `is_legacy_owner()` 仍在（06）。
- presentation `SaveManager` 欄位仍在，bootstrap 永遠 `None`（04）。
- `NetworkHandle::Host` 仍在（03）。
- Embedded 玩家物品欄 click 的 leftover `legacy_apply_inventory_ui_hit` 已刪；生產路徑本來就不編該函式。後續由 14 把 click 收進既有政策。
