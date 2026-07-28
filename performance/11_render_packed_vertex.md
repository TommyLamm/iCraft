# 任務 11：Packed TerrainVertex 與 section meshing

> 對應計畫：`14_performance_optimization.md` Phase 3.4 + 3.5
> 狀態：⏳ 待實作
> 前置：任務 1（基線）、任務 10（GPU arena）
> 目標：壓縮 TerrainVertex 至約 16-20 bytes，並將 Chunk 垂直切為 16³ sections，實現 section 級增量 meshing。
> Commit 訊息：`perf(render): pack terrain vertices and add section remeshing`

## 相關程式碼位置（已核對）

- `src/chunk_render.rs:18` - `TerrainVertex`：目前 36 bytes（position f32x3、local_uv f32x2、atlas_tile f32x2、light_level f32、ao f32）。
- `src/world.rs:1931` - `Chunk` 結構。
- `src/world.rs:2552` - `generate_mesh`：完整 Chunk mesh 生成。
- `src/world.rs:2956` - `generate_mesh_bundle`。
- `src/state.rs:861` - `MeshSnapshot`：目前 clone 完整 Chunk。
- `src/state.rs:952` - `ChunkMeshResult`：generation/lifetime/revision 驗證。
- `src/state.rs:4968` - `update_chunks`：mesh 重建調度。
- `src/shader.wgsl` - terrain shader（需修改 position 重建）。

## 子任務清單

### 11.1 Packed TerrainVertex 格式
- [ ] 檔案：`src/chunk_render.rs`、`src/shader.wgsl`
- 步驟：
  1. 壓縮 `TerrainVertex` 至約 16-20 bytes：
     - region-relative fixed-point/u16 position（3 × u16 = 6 bytes）
     - packed local UV（1 u32 或 2 u16）
     - integer atlas tile（2 u16 或 1 u32）
     - packed sky/block light（2 nibble = 1 byte）
     - packed AO/face flags（1 byte）
  2. Shader 以 region origin 重建 world position。
  3. 確認 vertex layout 與 WGSL attribute 對齊。
- 驗收：TerrainVertex 從 36 bytes 降至約 16-20 bytes；shader 正確重建 position。

### 11.2 邊界與精度驗證
- [ ] 檔案：`src/chunk_render.rs`（測試區段）
- 步驟：
  1. 驗證 Chunk/region 邊界沒有裂縫。
  2. 驗證大座標沒有明顯精度退化。
  3. 驗證 Greedy UV repeat、AO triangulation、fluid、snow、door/trapdoor 與 cross-model 結果一致。
  4. 若緊湊格式無法安全表示特殊模型，提供 full-precision fallback stream。
- 驗收：packed 與原格式視覺與幾何 parity 一致。

### 11.3 Section 級切分
- [ ] 檔案：`src/world.rs`、`src/chunk_render.rs`、`src/state.rs`
- 步驟：
  1. 將 Chunk 垂直切為 16³ sections（16 個 section/Chunk）。
  2. block/light mutation 只 dirty 所在 section 及需要 halo 的鄰接 section。
  3. MeshSnapshot 只保存一次 packed 18³ halo，不再 clone 完整 Chunk 並重複保存中心 voxel。
- 驗收：單格 block mutation 不再重建完整 16×256×16 Chunk mesh。

### 11.4 Section revision 失效判定
- [ ] 檔案：`src/state.rs`、`src/chunk_render.rs`
- 步驟：
  1. worker request 以 section revision、Chunk lifetime、dimension generation 做失效判定。
  2. 確認 stale section result 被正確丟棄。
- 驗收：快速連續修改同一位置時，只上傳最新 revision。

### 11.5 Worker pool 優先級
- [ ] 檔案：`src/state.rs`
- 步驟：
  1. 專用 bounded worker pool 保留一個 CPU core 給主線程。
  2. 近距離、視錐內、無可用 mesh 的工作優先。
  3. 送入昂貴 generator 前再次檢查 cancellation token。
- 驗收：主線程不被 worker 餓死；近距 mesh 優先完成。

### 11.6 LOD 分開 residency
- [ ] 檔案：`src/state.rs`、`src/chunk_render.rs`
- 步驟：
  1. L0/L1/L2 可分開 residency/build priority。
  2. 遠距先生成 L2/L1，接近前預取 L0。
- 驗收：快速飛行時遠距先有粗 LOD，近距接近時升級 L0。

### 11.7 跨 section/Chunk 邊界正確性
- [ ] 檔案：`src/world.rs`（測試區段）
- 步驟：
  1. 確認跨 section/Chunk AO、lighting、fluid 邊界正確。
  2. halo 包含鄰接 section 的邊界 voxel。
- 驗收：section 邊界無視覺裂縫或 AO/lighting 錯誤。

## 驗收條件

- [ ] TerrainVertex 壓縮至約 16-20 bytes。
- [ ] 單格 block mutation 不再重建完整 Chunk mesh。
- [ ] 跨 section/Chunk AO、lighting、fluid 邊界正確。
- [ ] 快速連續修改同一位置時，只上傳最新 revision。
- [ ] Greedy UV repeat、AO triangulation、fluid、snow、door/trapdoor 與 cross-model 結果一致。
- [ ] GPU vertex upload bytes 降低（與基線比較）。
- [ ] `cargo fmt --all -- --check`、`cargo check --release`、`cargo test --release` 通過。

## 風險與回退

- packed vertex 精度不足會在邊界產生裂縫；必須有完整的邊界 parity test。
- section meshing 是高複雜度改動；必須有基線證明完整 Chunk remesh 是瓶頸。
- 特殊模型（torch、cross、fluid）若無法安全 packed，保留 full-precision fallback stream。
- shader 修改需同步驗證 DX12 backend。

## 驗證命令

```text
cargo fmt --all -- --check
cargo check --release
cargo test --release
cargo run --release   # 視覺 parity 驗證 + GPU upload bytes before/after
```
