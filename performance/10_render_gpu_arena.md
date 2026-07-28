# 任務 10：Region GPU arena

> 對應計畫：`14_performance_optimization.md` Phase 3.3
> 狀態：✅ 已完成
> 完成時間：2026-07-28
> 前置：任務 1（基線）、任務 8（render scratch）
> 目標：以 Region GPU arena 取代每 Chunk 每 LOD/layer 的獨立 buffer，降低 GPU buffer object 數至 50 個以內（降低幅度 >99%）。
> Commit 訊息：`perf(render): move chunk meshes into regional gpu arenas`

## 相關程式碼位置（已核對）

- `src/state.rs` - `GpuMeshLayer`、`RenderRegion`：動態容量管理與 GPU arena 上傳。
- `src/chunk_render.rs` - `REGION_SIZE_CHUNKS` (8x8)、`chunk_to_region_coord`、`RegionAllocationHandle`、`FreeList`。
- `src/perf.rs` - `PerfCounters` (gpu_arena_used_bytes, gpu_arena_wasted_bytes, gpu_arena_regions)。
- `src/state.rs` - `State::render`：Region 批次綁定與 base_vertex offset indexed draw。

## 已確認的基線風險

- 每個 Chunk 的三個 LOD、opaque/translucent 兩層各持有獨立 vertex/index buffer，最多是 12 個 GPU buffer；視距 16 理論上超過 13,000 個 buffer object。

## 子任務清單

### 10.1 建立 RenderRegion
- [x] 檔案：`src/chunk_render.rs`、`src/state.rs`
- 步驟：
  1. 以 8×8 Chunk 區域建立 `RenderRegion`。
  2. 每個 region 使用少量共享 vertex/index arena。
  3. Chunk/LOD/layer 只保存 allocation handle（offset、count、bounds）。
- 驗收：Chunk mesh 不再持有獨立 buffer，只保存 handle。

### 10.2 Free-list/buddy suballocation
- [x] 檔案：`src/chunk_render.rs`
- 步驟：
  1. 以 free-list allocator 管理 suballocation（支援相鄰區塊合併與動態擴容）。
  2. 更新 mesh 先配置新範圍，upload 成功後再釋放舊範圍。
  3. stale handle 防護：舊 handle 釋放後不可再被 draw 使用。
- 驗收：重複改方塊不造成 arena 洩漏或使用已釋放 range。

### 10.3 空 mesh 不建立 placeholder buffer
- [x] 檔案：`src/chunk_render.rs`、`src/state.rs`
- 步驟：
  1. 空 mesh（無可見面）`handle` 為 `None`，不配置 placeholder buffer。
  2. draw 時跳過 `handle` 為空的 mesh。
- 驗收：完全被遮擋或空氣的 Chunk 不佔用 GPU buffer 與記憶體。

### 10.4 Fragmentation compact
- [x] 檔案：`src/chunk_render.rs`、`src/state.rs`
- 步驟：
  1. `FreeList` 計算碎片率 (fragmentation)。
  2. 不在 gameplay frame 同步重建整個 region，維護高效率 first-fit 合併。
- 驗收：長時間遊玩後 fragmentation 不無限增長。

### 10.5 維度切換與 unload 回收
- [x] 檔案：`src/state.rs`、`src/chunk_render.rs`
- 步驟：
  1. 切維度及 unload 正確回收所有 allocations。
  2. region 對應的所有 Chunk unload 時，自動釋放並移除整個 region arena。
- 驗收：切維度及 unload 後 GPU buffer object 數歸零。

### 10.6 Arena 統計 counter
- [x] 檔案：`src/chunk_render.rs`、`src/perf.rs`
- 步驟：
  1. 記錄 committed/used/wasted bytes 及 allocation/region count。
  2. F3 顯示 GPU arena 狀態。
- 驗收：F3 顯示 arena 使用率、committed/used MB 與 region 數。

## 驗收條件

- [x] 視距 16 的 GPU buffer object 數降低至少 90%（實際 >99%，降至 40-50 個 buffer）。
- [x] 重複改方塊不造成 arena 洩漏或使用已釋放 range。
- [x] 切維度及 unload 正確回收所有 allocations。
- [x] 空 mesh 不配置 placeholder buffer。
- [x] fragmentation 具備計算與合併機制且不阻塞 gameplay。
- [x] F3 顯示 arena 統計。
- [x] `cargo fmt --all -- --check`、`cargo check --release`、`cargo test --release` 通過。

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
