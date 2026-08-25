# Plan01 — `State` 廢棄 GPU 管線與頂點緩衝清理

## 定位

在引入分區 GPU 地形管線管理器（`GpuTerrainPipelineManager`）和實例化生物/粒子渲染器後，`State` 結構體與初始化流程中殘留了大量不再使用的 WGPU 渲染管線、非實例化頂點/索引緩衝區及 scratch 向量：

| 檔案 | 殘留項目 | 行數 | 原因 |
| :--- | :--- | :--- | :--- |
| `src/state.rs:L3038-3039, L3710-3871` | `render_pipeline`, `trans_pipeline` | ~90 行 | 地形渲染已全由 `GpuTerrainPipelineManager` 管理，這兩條管線從未在任何 render pass 中被 bind 或 draw |
| `src/state.rs:L3142-3155, L4402-4482` | `mob_vertex_buffer`, `mob_index_buffer`, `particle_vertex_buffer`, `particle_index_buffer` | ~60 行 | 生物走 `mob_renderer.rs` 實例化繪製，粒子走 `particles.rs` 實例化繪製，舊頂點緩衝區從未綁定 |
| `src/state.rs:L3219-3222, L4875-4878` | `mob_vertices_scratch`, `mob_indices_scratch`, `particle_vertices_scratch`, `particle_indices_scratch` | ~20 行 | 實例化渲染直接構建 instance buffer，舊 scratch 向量已廢棄 |
| `src/mob_renderer.rs:L202-390` | `expand_mob_instances`, `render_mobs_legacy`, `render_local_player_legacy` | ~142 行 | CPU 端 quad 展開函式已由頂點著色器實例化完全取代，無運行時調用 |
| `src/presentation_inventory_policy.rs:L154-161` | `presentation_may_generate_chunks`, `presentation_may_mutate_chunks` | ~10 行 | 零調用相容別名 |

本計劃為純死碼清理，不影響任何現行渲染輸出與 GPU 提交。

## 前置

無。可與 02–06 並行。

## 精確 acceptance

- [ ] `State` 結構體中移除 `render_pipeline` 與 `trans_pipeline` 欄位及其在 `State::new` 中的管線編譯代碼。
- [ ] `State` 結構體中移除 `mob_vertex_buffer`、`mob_index_buffer`、`particle_vertex_buffer`、`particle_index_buffer` 及其在 `State::new` 中的 GPU 緩衝區分配。
- [ ] `State` 結構體中移除 `mob_vertices_scratch`、`mob_indices_scratch`、`particle_vertices_scratch`、`particle_indices_scratch` 欄位。
- [ ] 移除 `mob_renderer.rs` 中的 `expand_mob_instances`、`render_mobs_legacy`、`render_local_player_legacy`。
- [ ] 移除 `presentation_inventory_policy.rs` 中的 `presentation_may_generate_chunks` 與 `presentation_may_mutate_chunks`。
- [ ] `cargo check --all-targets` 通過，無編譯錯誤。
- [ ] 現有渲染測試與整合測試通過。

## 預計檔案與測試

- 修改：
  - `src/state.rs`
  - `src/mob_renderer.rs`
  - `src/presentation_inventory_policy.rs`
- 驗證測試：
  - `cargo test --lib presentation_inventory_policy::`
  - `cargo test --lib mob_renderer::`
  - `cargo check --all-targets`

## 建議階段

1. 檢查 `src/state.rs` 中 `render_pipeline`、`trans_pipeline` 的所有引用，確認無 render pass 引用後刪除欄位與 `State::new` 構建。
2. 檢查並刪除 `mob_*` 與 `particle_*` 緩衝區欄位與分配。
3. 清理 `mob_renderer.rs` 與 `presentation_inventory_policy.rs` 的死函式。
4. 運行 `cargo check --all-targets` 驗證。

## 不在本計劃

- 修改地形著色器或實例化著色器（`shader.wgsl`）。
- 更改任何現行渲染通道的順序。
