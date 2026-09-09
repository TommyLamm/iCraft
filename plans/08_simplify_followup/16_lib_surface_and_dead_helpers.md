# Plan16 — 縮小 `lib.rs` 桌面契約與死 helper

## 定位

`lib.rs` 為了桌面 `pub use` 把只被 `state.rs` 用的模組編進 server 函式庫。`icraft-server` 只 `use icraft::server_runtime`。架構寫 `microbench` 應 `cfg(test)`／`harness`；`recipes` 應 `pub(crate)`。

死／空 API：

- `terrain_translucent_cull_mode` 永遠 `None`
- `dynamic_resolution`：`main.rs` `allow(dead_code)`，未接 upscale pass
- `egress::broadcast_pose` 的 `allow(dead_code)` 複本

## 前置

04（`world_mutation` 已不在 lib 之後，契約更小）。

## 精確 acceptance

- [x] `gpu_frame_resources`、`presentation_click` 改為 desktop binary-only `mod`（或確認 server 不編譯它們）。
- [x] `microbench` 對齊架構：`cfg(test)` / feature `harness`；桌面 `--microbench` 仍用 `src/main.rs` 自己的模組。
- [x] 共享、server 不需要的模組能 `pub(crate)` 就不要 `pub`。
- [x] 刪或內聯永遠 `None` 的 translucent cull helper；未接 GPU 的 `dynamic_resolution` 不要編進 default desktop（或明確接上，不要 `dead_code` 空模組）。
- [x] `cargo check --bin icraft-server` 編譯集變小或至少不再 export 那些符號；`cargo check --all-targets` 通過。

## 預計檔案與測試

- `src/lib.rs`、`src/main.rs`、`src/state.rs`、`src/dynamic_resolution.rs`、`src/network/egress.rs`
- 驗證：`cargo check --bin icraft-server`；`cargo check --bin icraft`；`cargo test --lib`

## 建議階段

1. 列出 `lib.rs` 每個 `pub mod` 的真實呼叫者（server／tests／desktop）。
2. GPU／click 政策移出 lib。
3. 刪空 helper。

## 不在本計劃

- 拆 workspace crates。
- 把 presentation／audio 加進 `lib.rs`。

## 實作與證據

### 改了什麼

- `gpu_frame_resources`、`presentation_click` 從 `lib.rs` 移到 `src/main.rs` 的 desktop-only `mod`。預設 library／`icraft-server` 不再編譯這兩個檔。
- `microbench` 在 library 改 `#[cfg(any(test, feature = "harness"))] pub(crate)`。桌面 `--microbench` 用 `main.rs` 自己的 `mod microbench`。
- `fluid`、`mob`、`rail`、`world_tick` 改 `pub(crate)`（tests／server binary 沒有 `use icraft::…`；desktop 也不走 `crate::`）。`network::{channels,egress,ingress,session}` 改 `pub(crate)`；`transport` 仍 `pub`（desktop address-book 測試用 `Connection`）。
- `recipes` 維持 `pub`：desktop `State` 與 `ServerWorld` 公開 `RecipeManager`，無法在不改公開欄位型別的前提下改 `pub(crate)`。
- 內聯 translucent terrain `cull_mode: None`，刪永遠回 `None` 的 helper。`dynamic_resolution` 改 `#[cfg(any(test, feature = "harness"))]`，預設 desktop 不編；設定項仍持久化。
- 刪 `egress::broadcast_pose` 的 `allow(dead_code)` 包裝；測試改呼叫 `broadcast_pose_inner`。

### 測了什麼

- `cargo check --bin icraft-server`：通過。deps `icraft-1a956145570ac11b.d`（lib rlib）不含 `gpu_frame_resources.rs` / `presentation_click.rs` / `microbench.rs` / `dynamic_resolution.rs`。對照 Plan 04 同檔含前三者。
- `cargo check --bin icraft`：通過。desktop deps 含 `gpu_frame_resources.rs`、`presentation_click.rs`、`microbench.rs`，不含 `dynamic_resolution.rs`。
- `cargo check --all-targets`：通過。
- `cargo test --lib`：706 passed；3 failed（`waterlogged_slab_adds_only_the_translucent_complement`、`end_portal_frame_and_surface_use_lower_minecraft_heights`、`trapdoor_mesh_generation_open_and_closed_bounds`）在起點 `4f11e8c` 已失敗，非本計劃迴歸。`microbench::smoke_checksums_are_stable` passed。
- `cargo test --bin icraft -- presentation_click gpu_frame_resources dynamic_resolution microbench_flag`：20 passed。

### 留下的缺口

- `recipes` 仍 `pub`（見上）。
- 桌面仍 `pub use` 多數 GPU-adjacent 共享模組（`chunk_render`、`culling`、`lighting`…），因為 `State` 走 `crate::`；沒拆 workspace crate。
- 未接 GPU 的 dynamic-resolution upscale 仍未接上；設定 bool 仍寫 `settings.txt`。
