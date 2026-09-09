# Plan11 — 載入光照邊界入隊與 mesh halo／LOD

## 定位

載入整合（`state.rs` ~6573–6598）對 self + 四鄰呼叫 `propagate_chunk_lighting`。該函式對每個體素 × 6 鄰做 `get_block`（HashMap 欄查找）。Overworld 每欄約 59 萬次查找；×5 欄 × 每幀最多 2 個載入結果。生成時已寫過單欄 light。

`schedule_section_mesh` 在 spawn Rayon 之前於主執行緒 `capture_section_halo`：5832 次 `chunks.get`。`generate_section_mesh_bundle_from_halo_inner` 永遠打 L0+L1+L2。`mesh_l0_volume` 四個 `Vec::new()`。

## 前置

無。05 若未做，頂點路徑仍可能開 debug 檔，本計劃不要依賴它。

## 精確 acceptance

- [x] 載入光照只從邊界體素／光源／chunk 面入隊，或熱迴圈鎖住 `&Chunk` 再掃 local。
- [x] halo 從最多 9 個 `&Chunk` 一次抓齊，不要 per-voxel HashMap。
- [x] L1／L2 延到第一次被選到該 LOD，或視距內只 mesh L0。
- [x] mesh `Vec` 預留容量（非空氣數或合理上界）。
- [x] 現有 mesh／lighting／halo 測試通過。

## 預計檔案與測試

- `src/lighting.rs`、`src/state.rs`、`src/chunk_manager.rs` `capture_section_halo`、`src/world/mesh.rs`、`src/presentation/gpu_terrain.rs`
- 驗證：`cargo test --lib lighting::`；`world::mesh::`

## 建議階段

1. 光照：邊界／光源入隊。
2. halo：9-chunk 一次抓。
3. LOD 延後 + Vec capacity。

## 不在本計劃

- 重寫 greedy mesh 或 wgpu 管線。
- compute meshing。
- 為 `get_block` 做全域 voxel 快取。

## 實作與證據

- 光照：`propagate_chunk_lighting` 一次抓 3×3 `&Chunk`，只掃已配置 section，鄰居讀 local 而非 per-voxel HashMap。洞穴水平傳播與跨 chunk 面漏光測試覆蓋。
- Halo：`ChunkManager::column_neighborhood` + `capture_section_halo` 從最多 9 欄填 18³。
- LOD：runtime 只 mesh 當前選到的層；`prepare_terrain_draw_plan` 在首次選到缺失 LOD 時入隊補齊，繪製可 fallback。公開 `generate_section_mesh_bundle*` 仍打滿 L0–L2。
- Mesh `Vec`：`mesh_l0_volume_with_registry` 依非空氣數 `reserve` 24 verts / 36 indices。
- 測試：`cargo test --lib lighting::` 6 passed；`world::mesh::` 26 passed（`halo_bundle_can_defer_coarse_lods`、`section_halo_occludes_boundary_neighbor`）；`capture_section_halo_uses_neighbor_columns` passed。
  兩個既有失敗（`trapdoor_mesh_generation_open_and_closed_bounds`、`end_portal_frame_and_surface_use_lower_minecraft_heights`）是 packed Y 對 `REGION_ORIGIN_Y` 的語意，屬另開 bugfix，本計劃未改。
