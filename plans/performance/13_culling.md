# 任務 13：Section visibility 與 Entity occlusion

> 對應計畫：`14_performance_optimization.md` Phase 5.1 + 5.2 + 5.3
> 狀態：Partial
> 審核回退：見 [`15_performance_audit_repair_plan.md`](15_performance_audit_repair_plan.md)；async LOS worker 無地形資料且永遠回 visible，stale graph/occluder/section ownership 亦未達驗收，待 R6 修復。
> 前置：任務 1（基線）、任務 11（section meshing）、任務 12（section storage）
> 目標：加入便宜的剔除階層與保守遮擋剔除，跳過被地形遮擋的 terrain section 與 entity 渲染。
> Commit 訊息：`perf(culling): add section visibility and conservative entity occlusion`

## 相關程式碼位置（已核對）

- `src/state.rs` - `State::render`：實作 4 階層剔除（Distance -> Frustum -> Section Vis -> LOS）。
- `src/culling.rs` - 包含 `SectionConnectivity` flood fill、Bounded Graph Traversal 與 non-blocking `EntityLosManager`。
- `src/chunk_render.rs` - `ChunkMeshBundle` 包含 `section_connectivity`。
- `src/world.rs` - `generate_mesh_bundle` 在背景 Rayon 執行 section connectivity 運算。
- `src/mob_renderer.rs` - `render_mobs` 僅渲染通過剔除階層之實體。

## 子任務清單

### 13.1 便宜剔除階層
- [ ] 檔案：`src/state.rs`、`src/chunk_render.rs`
- 步驟：
  1. 所有 terrain section 與 entity submission 依序執行：
     1. render distance
     2. section/entity AABB frustum
     3. section visibility（occlusion graph）
     4. optional asynchronous entity LOS
  2. 只有通過前一階段才執行下一階段。
  3. 確認 frustum culling 保留現有行為。
- 驗收：剔除階層按順序執行，前一階段不通過即跳過後續。

### 13.2 Section occlusion graph
- [ ] 檔案：`src/chunk_render.rs`、`src/world.rs`、`src/state.rs`
- 步驟：
  1. meshing worker 對每個 section 的透明/可通行 voxel 做 flood fill。
  2. 建立六個 section faces 的 pairwise connectivity bitmask。
  3. 從 camera section 做 bounded graph traversal，只訪問能經可見 face 到達的 section。
  4. 完整 opaque block 才能作可靠 occluder；leaves、glass、fluid、cutout、translucent 和未載入資料保守視為可見。
  5. camera teleport、進入未完成 section 或 stale graph 時 fail-open。
- 驗收：洞穴/建築內場景 terrain draw call 顯著下降。

### 13.3 非同步 Entity LOS
- [ ] 檔案：`src/state.rs`、`src/mob_renderer.rs`
- 步驟：
  1. 對仍可能可見而且 mesh 成本較高的 entity，從 camera 到 AABB center/corners 做 bounded voxel LOS。
  2. 使用獨立低優先級 queue，不能與 terrain meshing 爭奪全部 Rayon threads。
  3. 結果保存 world/chunk revision、camera cell、TTL 和 last-visible hysteresis。
  4. stale、超時或 queue overflow 時一律 render（fail-open）。
  5. 下列類型預設 bypass/白名單：
     - camera 附近 entities
     - projectiles
     - bosses
     - remote players 在近距離
     - model 超出標準 AABB
     - lightning/critical effects
  6. 只跳過渲染和純 client visual animation；權威 AI、physics、pickup、damage、drops、network state 一律繼續。
- 驗收：在牆後 1,000 entities 場景，render submission 和 upload 顯著下降。

### 13.4 快速恢復驗證
- [ ] 檔案：手動視覺測試
- 步驟：
  1. 快速轉身時沒有明顯 pop-in。
  2. 開門、拆牆後 occlusion 正確恢復。
  3. 所有不確定狀態均 fail-open，不會出現實體永久消失。
- 驗收：快速轉身及拆牆時無明顯 pop-in；無實體永久消失。

### 13.5 Counter 整合
- [ ] 檔案：`src/perf.rs`、`src/state.rs`
- 步驟：
  1. 確認任務 1 的 `entities_occlusion_culled` counter 接入。
  2. 加入 `sections_occlusion_culled` counter。
  3. F3 顯示。
- 驗收：F3 顯示 section 與 entity occlusion culling 計數。

## 驗收條件

- [ ] 剔除階層按順序執行（distance -> frustum -> section visibility -> LOS）。
- [ ] 洞穴/建築內場景 terrain draw call 下降。
- [ ] 在牆後 1,000 entities 場景，render submission 和 upload 顯著下降。
- [ ] 快速轉身、開門、拆牆時沒有明顯 pop-in。
- [ ] 所有不確定狀態均 fail-open，不會出現實體永久消失。
- [ ] 權威 AI、physics、pickup、damage 不受 occlusion 影響。
- [ ] F3 顯示 culling counters。
- [ ] `cargo fmt --all -- --check`、`cargo check --release`、`cargo test --release` 通過。

## 風險與回退

- occlusion graph 是高複雜度改動；必須有基線證明被遮擋的 section/entity 渲染是瓶頸。
- fail-open 是最高原則；任何不確定狀態一律 render，避免實體消失。
- LOS queue 不能餓死 terrain meshing；使用獨立低優先級 thread。
- camera teleport 時必須 fail-open 並重建 visibility。

## 驗證命令

```text
cargo fmt --all -- --check
cargo check --release
cargo test --release
cargo run --release   # 固定場景 2（室內遮擋）與 6（1,000 entities）before/after
```
