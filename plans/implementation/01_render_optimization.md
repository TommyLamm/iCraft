# 實作計畫 01：完成渲染優化

> 狀態：已完成（Release 自動驗證通過；視距 16 的 60+ FPS 留待實機量測）

## 目標

完成 `plans/p3/30_render_optimization.md` 的視錐剔除、Greedy Meshing、背景
Mesh 生成、Chunk 排序與三級 LOD，並讓 F3 顯示實際提交的 Chunk、Draw Call
和三角形數。

## 現況與風險

- `Chunk::generate_mesh` 逐方塊逐面生成，每個可見面固定 4 vertices / 6 indices。
- `State::update_chunks` 在主線程同步生成 Chunk 與最多四個 mesh。
- `State::render` 直接遍歷 `HashMap`，沒有視錐剔除或排序。
- 相機 far plane 固定 100 blocks，Render Distance 16 時不足。
- Atlas UV 不能直接被 Greedy quad 拉伸；AO 的內部頂點也不能任意消失。

## 實作步驟

1. 新增 `src/chunk_render.rs`，集中純 CPU 的 `TerrainVertex`、
   `ChunkMeshData`、Frustum/AABB、DrawPlan、LOD 與 worker request/result。
2. 將 terrain shader 改為 `local_uv + atlas_tile`，以 `fract(local_uv)` 在同一
   atlas tile 內重複紋理；mob/hand/particle 保持現有 `Vertex`。
3. 對完整立方體面做 Greedy Meshing；key 包含方向、材質、tile、render
   class、packed light 與 AO。只有四角一致的 uniform AO 面可以合併；
   fluid、SnowLayer、cross-model 與特殊模型保持獨立路徑。
4. 由 view-projection 提取六個視錐平面；修正 far plane，使其覆蓋方形
   render distance 最遠角。每個 `ChunkMesh` 保存實際 bounds。
5. 建立可測的 visible draw plan：opaque 近到遠、transparent 遠到近，
   穩定以座標 tie-break；空 mesh 與視錐外 mesh 不提交。
6. 使用既有 Rayon 加 bounded worker queue。主線程建立含一格 halo 的
   owned mesh input，worker 只回 CPU vectors，wgpu buffer 只在主線程上傳。
7. 為每個 Chunk 維護 revision/lifetime token 和 dimension generation；
   mutation、卸載重載或切維度後丟棄過期 worker result。
8. 每個 worker 同時產生 L0 完整 greedy、L1 16×16 surface heightfield、
   L2 4×4 coarse outline；遠距 surface mesh 加 skirts，避免 LOD 接縫。
9. F3 改顯示實際 visible chunks、submitted terrain triangles 與 draw calls。
10. 勾選 `30_render_optimization.md`，更新 `ARCHITECTURE.md` 與進度記錄。

## 驗證

- 單元測試：Frustum 前/後/near/far AABB、opaque/transparent 排序。
- 單元測試：2×2 greedy 合併、不同 tile/light/AO 不合併、UV 重複正確。
- 單元測試：worker revision/dimension/lifetime 過期結果被丟棄。
- 單元測試：L0 > L1 > L2 的幾何量、heightmap 與 skirts 正確。
- `cargo fmt -- --check`
- `cargo test --release`
- `cargo check --release`
- 人工：Render Distance 16 快速旋轉、連續改方塊，F3 背對地形時提交三角形
  明顯下降且沒有主線程 mesh 卡頓。

## Commit

單一功能 commit：`feat(render): complete chunk rendering optimizations`
