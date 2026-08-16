# Plan14 — Desktop 改依賴 library

## 定位

`ARCHITECTURE.md` 寫：desktop binary 自己宣告模組樹，共享源碼編兩次。
Plan 10 把「desktop `use icraft::*`」列為不在計劃內，理由是「架構刻意雙編譯」。

真正需要的籬笆是：**`presentation/`、`menu`、`state`、`camera`、`texture` 不進 `lib.rs`**，
這樣 `icraft-server` 才不會編譯 wgpu 選單。這個籬笆 10 已經做到。雙編譯不是它的前提。

現況：`src/main.rs` 與 `src/lib.rs` 各宣告近 50 個相同 `mod`。
`cargo test --bin icraft` 會再跑一遍 `world.rs`／`inventory.rs` 單元測。

同一 package 的 bin 依賴 lib 是不同 crate，看不見 `pub(crate)`。掃描當日跨界 helper：

| 符號 | 檔案 |
| --- | --- |
| `mark_block_mesh_dependencies`／`mark_section_mesh_dependencies`／`surrounding_chunk_coords` | `src/chunk_manager.rs` |
| `BlockStorage`／`LightStorage` | `src/world.rs` |
| `apply_stack_click`／`StackClickResult` | `src/inventory.rs` |
| `is_component` | `src/redstone.rs` |
| `sound_bytes_are_decodable` | `src/resources.rs` |

desktop-only 的 `pub(crate)`（`presentation/*`）與本計劃無關。

## 前置

**建議** 11 已合併：`lib.rs` 的 `pub` 白名單已穩定，本計劃只 promote helper，不再縮模組。
若 11 未合併：仍可做，但 `main.rs` 的 `pub use icraft::{…}` 清單要以當時 `lib.rs` 為準。

不要跟 15 同時大搬 `state.rs`。

## 精確 acceptance

- [x] `src/main.rs` **不再** `mod` 那些已由 `lib.rs` 提供的共享模組。改 `use icraft::…` 或
      `pub use icraft::{world, inventory, server_runtime, …}`，讓既有 `crate::world` 路徑在
      desktop 樹裡仍然能解析（選一種，整棵樹一致；寫進證據）。
- [x] desktop-only 模組仍由 `main.rs` 宣告：`app`、`camera`、`dynamic_resolution`、
      `hand_renderer`、`menu`、`mob_renderer`、`particles`、`presentation`、`state`、`texture`，
      以及 `#[path]` 掛在 `state` 下的 `legacy_sim`／`frame`。
- [x] `lib.rs` **沒有** `pub mod presentation`／`menu`／`state`／`camera`／`texture`。
      `cargo check --bin icraft-server` 不得突然編譯 `src/menu.rs`。
- [x] 上表 `pub(crate)` helper 升成 `pub`，或加一個窄 facade。不得為了省事把整個
      `chunk_manager`／`world` 內部型別全部公開。
- [x] `#[global_allocator]` 仍只在 `src/main.rs`。server／tests 不得安裝它。
- [x] `cargo test --bin icraft` **不再**重跑 `world::`／`inventory::` 那些 lib 單元測
      （它們留在 `cargo test --lib`）。`state.rs` 裡的 interpolation／pause／GPU timestamp 測仍走 `--bin icraft`。
- [x] 不得改 tick／render／wire 行為。既有測試期望值不變。

## 預計檔案與測試

- 修改：`src/main.rs`、被 promote 的 helper 所在檔、desktop 檔裡若需要的 `use` 路徑。
- 測試：
  - `cargo check --bin icraft`
  - `cargo check --bin icraft-server`
  - `cargo test --lib world:: inventory:: -- --test-threads=1`
  - `cargo test --bin icraft interpolation_midpoint_and_clamps gpu_timestamp_state_tests mesh_invalidation -- --test-threads=1`
  - `cargo test --bin icraft multiplayer_host_keeps_world_ticks singleplayer_pause -- --test-threads=1`
  - `cargo test --test review_hardening_embedded_presentation -- --test-threads=1`

## 建議階段

1. 對上表每個 `pub(crate)` 確認 desktop 與 lib-internal 呼叫點。升可見性或加 facade。
2. `main.rs` 刪共享 `mod`，改 re-export／`use icraft`。`cargo check --bin icraft`。
3. 修殘留 `crate::` 路徑。確認 server 仍不編 `menu.rs`。
4. 跑 `--lib` 與 `--bin icraft` 窄測試，確認測沒有雙跑錯位。

## 不在本計劃

- 開 workspace crate（`icraft-core`／`world`／`net`／`client`）。
- `wgpu` optional／`client` feature。
- 拆 `state.rs`（15）或 `world.rs`（18）。
- 把 `presentation/` 加進 lib。
- 刪 `legacy_sim`。

## 實作與證據

**策略（整棵樹一致）：** `src/main.rs` 用 `pub use icraft::{world, inventory, server_runtime, …}`
re-export 共享模組，desktop 檔維持既有 `crate::world` 路徑。沒有把 desktop 樹改成 `use icraft::…`。

**desktop-only 仍由 `main.rs` 宣告：** `app`、`camera`、`dynamic_resolution`、`hand_renderer`、
`menu`、`mob_renderer`、`particles`、`presentation`、`state`、`texture`、`microbench`。
`legacy_sim`／`frame` 仍由 `state.rs` 的 `#[path]` 掛載。`#[global_allocator]` 只在 `main.rs`。

**`lib.rs` 籬笆：** 沒有 `pub mod presentation`／`menu`／`state`／`camera`／`texture`。
`cargo check --bin icraft-server` 通過，輸出不含 `src/menu.rs`。

為了讓 bin crate 能 `pub use icraft::audio` 這類路徑，desktop 實際 `use crate::` 的共享模組
從 `pub(crate) mod` 升成 `pub mod`（desktop-shared，不是 server／tests 契約）。仍保持
`pub(crate)` 的是 desktop 樹沒有 `crate::` 引用的：`ai`、`loot`、`recipes`、`spawning`、
`voxel_shape`、`worldgen`，以及 cfg 的 `sim_harness`／`final_acceptance`／`microbench`。

**升成 `pub` 的 helper（窄，沒有整包公開內部型別）：**

| 符號 | 檔案 | 原因 |
| --- | --- | --- |
| `mark_block_mesh_dependencies`／`mark_section_mesh_dependencies`／`surrounding_chunk_coords` | `chunk_manager.rs` | 計劃表；desktop `state.rs` 呼叫 |
| `acknowledge_mesh_invalidation`／`drain_mesh_invalidations`／`acknowledge_section_mesh_invalidation`／`drain_section_mesh_invalidations` | `chunk_manager.rs` | 當日掃描漏列；`state.rs` 跨 crate 呼叫 |
| `ChunkManager::new` | `chunk_manager.rs` | 拿掉 `#[cfg(test)]`。bin 測編譯 lib 時沒有 `cfg(test)`，否則 `ChunkManager::new` 消失 |
| `BlockStorage`／`LightStorage` | `world.rs` | 計劃表；desktop `microbench.rs` 使用 |
| `apply_stack_click`／`StackClickResult` | `inventory.rs` | 計劃表 |
| `is_component` | `redstone.rs` | 計劃表 |
| `sound_bytes_are_decodable` | `resources.rs` | 計劃表 |

`TerrainVertex::desc` 從 `state.rs` 的 inherent impl 搬到 `chunk_render.rs`（跨 crate 不能
對 lib 型別寫 inherent impl）。沒有改 leftover 互動、沒有改 tick／render／wire。

**窄測試（Windows，`--test-threads=1`；Cargo 只收一個 TESTNAME，bin／lib 過濾器分開跑）：**

- `cargo check --bin icraft` — 通過
- `cargo check --bin icraft-server` — 通過（未編譯 `menu.rs`）
- `cargo test --lib world::` — 73 passed
- `cargo test --lib inventory::` — 23 passed
- `cargo test --bin icraft interpolation_midpoint_and_clamps` — 1 passed
- `cargo test --bin icraft gpu_timestamp_state_tests` — 3 passed
- `cargo test --bin icraft mesh_invalidation` — 1 passed
- `cargo test --bin icraft multiplayer_host_keeps_world_ticks` — 1 passed
- `cargo test --bin icraft singleplayer_pause` — 1 passed
- `cargo test --test review_hardening_embedded_presentation` — 4 passed

`cargo test --bin icraft -- --list` 不含 `world::`／`inventory::`。

**留下的缺口：** 未開 workspace crate；`ai`／`worldgen` 等仍是 crate-internal；未改
`ARCHITECTURE.md`（父任務統一更新）。未跑 repo-wide full suite。
