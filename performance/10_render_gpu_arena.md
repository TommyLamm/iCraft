# 任務 10：Region GPU arena

> 對應計畫：`14_performance_optimization.md` Phase 3.3
> 狀態：⏳ 待實作
> 前置：任務 1（基線）、任務 8（render scratch）
> 目標：以 Region GPU arena 取代每 Chunk 每 LOD/layer 的獨立 buffer，降低 GPU buffer object 數至少 90%。
> Commit 訊息：`perf(render): move chunk meshes into regional gpu arenas`

## 相關程式碼位置（已核對）

- `src/state.rs:794` - `ChunkMesh`：目前每 LOD/layer 持有獨立 vertex/index buffer。
- `src/chunk_render.rs:18` - `TerrainVertex`。
- `src/chunk_render.rs:119` - `ChunkMeshData`。
- `src/chunk_render.rs:173` - `ChunkMeshBundle`。
- `src/world.rs:2956` - `generate_mesh_bundle`：mesh 生成。
- `src/state.rs:9003` - `State::render`：draw 提交。

## 已確認的基線風險

- 每個 Chunk 的三個 LOD、opaque/translucent 兩層各持有獨立 vertex/index buffer，最多是 12 個 GPU buffer；視距 16 理論上超過 13,000 個 buffer object。

## 子任務清單

### 10.1 建立 RenderRegion
- [ ] 檔案：`src/chunk_render.rs`、`src/state.rs`
- 步驟：
  1. 以 8×8（或經基準選定大小）Chunk 區域建立 `RenderRegion`。
  2. 每個 region 使用少量共享 vertex/index arena。
  3. Chunk/LOD/layer 只保存 allocation handle（offset、count、bounds）。
- 驗收：Chunk mesh 不再持有獨立 buffer，只保存 handle。

### 10.2 Free-list/buddy suballocation
- [ ] 檔案：`src/chunk_render.rs`
- 步驟：
  1. 以 free-list 或 buddy allocator 管理 suballocation。
  2. 更新 mesh 先配置新範圍，upload 成功後再釋放舊範圍。
  3. stale handle 防護：舊 handle 釋放後不可再被 draw 使用。
- 驗收：重複改方塊不造成 arena 洩漏或使用已釋放 range。

### 10.3 空 mesh 不建立 placeholder buffer
- [ ] 檔案：`src/chunk_render.rs`、`src/state.rs`
- 步驟：
  1. 空 mesh（無可見面）不配置 4-byte placeholder buffer。
  2. draw 時跳過 handle 為空的 mesh。
- 驗收：完全被遮擋或空氣的 Chunk 不佔用 GPU buffer。

### 10.4 Fragmentation compact
- [ ] 檔案：`src/chunk_render.rs`、`src/state.rs`
- 步驟：
  1. fragmentation 超過門檻時低優先級 compact。
  2. 不得在 gameplay frame 同步重建整個 region。
  3. compact 在閒置或低幀時間執行。
- 驗收：長時間遊玩後 fragmentation 不無限增長。

### 10.5 維度切換與 unload 回收
- [ ] 檔案：`src/state.rs`、`src/chunk_render.rs`
- 步驟：
  1. 切維度及 unload 正確回收所有 allocations。
  2. region 對應的所有 Chunk unload 時，釋放整個 region arena。
- 驗收：切維度及 unload 後 GPU buffer object 數歸零（或降至背景值）。

### 10.6 Arena 統計 counter
- [ ] 檔案：`src/chunk_render.rs`、`src/perf.rs`
- 步驟：
  1. 記錄 committed/used/wasted bytes 及 allocation count。
  2. F3 顯示。
- 驗收：F3 顯示 arena 使用率與 fragmentation。

## 驗收條件

- [ ] 視距 16 的 GPU buffer object 數降低至少 90%（與基線比較）。
- [ ] 重複改方塊不造成 arena 洩漏或使用已釋放 range。
- [ ] 切維度及 unload 正確回收所有 allocations。
- [ ] 空 mesh 不配置 placeholder buffer。
- [ ] fragmentation 有 compact 機制且不阻塞 gameplay。
- [ ] F3 顯示 arena 統計。
- [ ] `cargo fmt --all -- --check`、`cargo check --release`、`cargo test --release` 通過。

## 風險與回退

- arena 管理是高複雜度改動；必須有基線證明 buffer object 數是瓶頸。
- suballocation bug 可能導致 GPU crash 或渲染錯誤；stale handle 防護必須完備。
- compact 若在 gameplay frame 同步執行會造成長幀；嚴格限制為低優先級。
- 若 adapter 對大 buffer 有大小限制，需確認 arena 不超限。

## 驗證命令

```text
cargo fmt --all -- --check
cargo check --release
cargo test --release
cargo run --release   # 視距 16 場景 F3 GPU buffer object count before/after
```
