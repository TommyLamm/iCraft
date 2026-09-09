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

- [ ] `gpu_frame_resources`、`presentation_click` 改為 desktop binary-only `mod`（或確認 server 不編譯它們）。
- [ ] `microbench` 對齊架構：`cfg(test)` / feature `harness`；桌面 `--microbench` 仍用 `src/main.rs` 自己的模組。
- [ ] 共享、server 不需要的模組能 `pub(crate)` 就不要 `pub`。
- [ ] 刪或內聯永遠 `None` 的 translucent cull helper；未接 GPU 的 `dynamic_resolution` 不要編進 default desktop（或明確接上，不要 `dead_code` 空模組）。
- [ ] `cargo check --bin icraft-server` 編譯集變小或至少不再 export 那些符號；`cargo check --all-targets` 通過。

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
