# Plan11 — Library 面縮小與 harness cfg

## 定位

`src/lib.rs` 有 57 個無條件 `pub mod`，沒有 `#[cfg]`、沒有 `pub(crate)`。
`icraft-server` 只 `use icraft::server_runtime`，integration test 也只用其中約 20 個模組，
但 library 仍編譯：

| 模組 | 為什麼不該是預設 public API |
| --- | --- |
| `sim_harness` | Plan 19 煙霧；`ARCHITECTURE.md` 寫明不是權威閉環。沒有 `tests/` import |
| `final_acceptance` | 只走 `SimHarness`。Listen／Dedicated 列是 `blocked_reason` |
| `microbench` | desktop `main.rs --microbench` 用的是 **binary 自己的** `mod microbench`，不是 lib |
| `gpu_frame_resources` | `register_submission_completion` 吃 `&wgpu::Queue`。只有 `State` 用 |
| `presentation_click` | 純 CPU 政策，但沒有 integration test import。`presentation_inventory_policy` 才是該留在 lib 的薄切面 |
| `audio`、`block_model` | server tick 不跑；因 leftover／mesh 耦合而留下來 |

`Cargo.toml` 沒有 `[features]`。本計劃**不**把 `wgpu`／`rodio` 改成 optional
（那要先解 `world.rs → chunk_render` 與 `mob::update_mobs → AudioManager`，見 README §7）。
本計劃只讓預設 library 面不再假裝 harness／GPU hook 是 server API。

## 前置

無。可與 12、13 並行。不要跟 14 搶 `lib.rs` 的 `pub` 清單——14 會再 promote 幾個 `pub(crate)` helper；本計劃先縮模組可見性。

## 精確 acceptance

- [x] `Cargo.toml` 增加 feature `harness`（不要放進 `default`）。
- [x] `src/lib.rs` 對 `sim_harness`、`final_acceptance`、`microbench` 加
      `#[cfg(any(test, feature = "harness"))]`。`icraft-server` 與未開 feature 的
      `cargo check --lib` **不得**編譯這三個模組。
- [x] desktop `src/main.rs` 的 `mod microbench` 與 `--microbench` 入口保持可用，不依賴 lib feature。
      `main.rs` 的 `pub mod sim_harness`／`final_acceptance` 同樣改 `cfg`，或刪掉 binary 上多餘的
      `pub`（binary 沒有外部 crate 連它）。
- [x] `lib.rs` 只對 `tests/` 與 `src/bin/icraft-server.rs` **實際** `use icraft::…` 的模組保持 `pub mod`。
      執行前必須再 grep `use icraft::`。其餘改 `pub(crate) mod`。
      掃描當日確定要留 `pub` 的集合（再核一次）：
      `authority`、`block_entity`、`brewing`、`chunk_manager`、`container_sessions`、
      `dimension`、`enchantment`、`entity`、`fishing`、`game_rules`、`inventory`、
      `network`、`passive_mob`、`player`、`presentation_inventory_policy`、`redstone`、
      `save`、`server_runtime`、`server_world`、`structure`、`world`。
      若 grep 發現新的 `use icraft::foo`，該 `foo` 留下 `pub`，寫進證據。
- [x] `lib.rs` 檔頭 rustdoc 分成「server／tests 契約」與「crate-internal／desktop-adjacent」。
      明確寫：`presentation/` **不得**加進 lib（10 的籬笆）。
- [x] `gpu_frame_resources`、`presentation_click` 改 `pub(crate)`（或同等），除非 grep 證明
      `tests/` 有 import。不得把它們搬進 `presentation/`（那是 desktop-only 目錄，lib 測會斷）。
- [x] 不得把 `wgpu`／`winit`／`rodio`／`image` 改 optional。不得開 workspace crate。
- [x] 既有測試期望值不得改。`cargo test --lib` 仍要編到 harness（因為 `cfg(test)`）。

## 預計檔案與測試

- 修改：`Cargo.toml`、`src/lib.rs`、必要時 `src/main.rs`、被降可見性模組的 rustdoc。
- 測試：
  - `cargo check --bin icraft-server`
  - `cargo check --lib`
  - `cargo check --bin icraft`
  - `cargo test --lib presentation_inventory_policy::`
  - `cargo test --test review_hardening_embedded_presentation -- --test-threads=1`
  - `cargo test --test review_hardening_invariants -- --test-threads=1`
  - `cargo test --test headless_server_authority -- --test-threads=1`
  - `rg "use icraft::" tests src/bin`（證據裡列出最終 `pub` 集合）

## 建議階段

1. 對 `tests/` 與 `src/bin/` 跑 `use icraft::` grep，寫下真實 `pub` 白名單。
2. 加 `harness` feature；cfg 三個 harness 模組。`cargo check --bin icraft-server`。
3. 非白名單模組改 `pub(crate)`。修編譯錯誤（通常是某測試漏列）。
4. rustdoc。跑窄測試。

## 不在本計劃

- desktop `use icraft::*`（14）。
- `wgpu`／`rodio` optional、`client` feature、workspace crates。
- 解 `world.rs → chunk_render` 或 `update_mobs → AudioManager`（README §7）。
- 刪 `sim_harness`／`final_acceptance` 內容。
- 把 `presentation/` 加進 `lib.rs`。

## 實作與證據

實作當日再掃一次 `tests/` + `src/bin` 的 `icraft::<first>`（含 `use icraft::` 與路徑寫法）：

```
authority
block_entity
brewing
chunk_manager
container_sessions
dimension
enchantment
entity
fishing
game_rules
inventory
network
passive_mob
player
presentation_inventory_policy
redstone
save
server_runtime
server_world
structure
world
```

`src/bin/icraft-server.rs` 只 `use icraft::server_runtime::{ServerProperties, ServerRuntime}`。
沒有新的 `use icraft::foo`。`gpu_frame_resources`／`presentation_click`／`sim_harness`／`final_acceptance`／`microbench` 都沒有 integration import。

改了什麼：

- `Cargo.toml`：新增 `[features] harness = []`，沒有 `default`，wgpu／winit／rodio／image 仍是普通依賴，沒有 workspace crate。
- `src/lib.rs`：21 個契約模組維持 `pub mod`；其餘改 `pub(crate) mod`。`sim_harness`／`final_acceptance`／`microbench` 加 `#[cfg(any(test, feature = "harness"))]`。檔頭 rustdoc 分成 server／tests 契約與 crate-internal／desktop-adjacent，並寫明 `presentation/` 不得加進 lib。
- `src/main.rs`：`mod microbench` 與 `--microbench` 仍無條件可用；`sim_harness`／`final_acceptance` 改 `#[cfg(any(test, feature = "harness"))] mod`（拿掉 binary 上多餘的 `pub`）。
- rustdoc：`gpu_frame_resources`、`presentation_click`、三個 harness 模組標成 crate-internal／cfg 範圍。沒有搬進 `presentation/`。

測了什麼（全部通過；未改任何測試期望值）：

```
cargo check --bin icraft-server
cargo check --lib
cargo check --bin icraft
cargo test --lib presentation_inventory_policy::
cargo test --test review_hardening_embedded_presentation -- --test-threads=1
cargo test --test review_hardening_invariants -- --test-threads=1
cargo test --test headless_server_authority -- --test-threads=1
```

- `cargo check --lib`／`--bin icraft-server` 的 rustc JSON 沒有 `sim_harness`／`final_acceptance`／`microbench` 檔案。
- `cargo test --lib presentation_inventory_policy::`：4 passed（cfg(test) 仍編到 harness）。
- `review_hardening_embedded_presentation`：4 passed。
- `review_hardening_invariants`：6 passed。
- `headless_server_authority`：2 passed。

留下的缺口：

- 降可見性後，`cargo check --lib` 對 leftover `pub` 項出現大量 `dead_code`／`unused_imports`（約 234）。這是 Plan 12 的活型別 leftover 衛生，本計劃不收。
- desktop 仍雙編譯自己的模組樹（Plan 14）。
- `wgpu`／`rodio` 仍是 library 依賴（README §7）。
