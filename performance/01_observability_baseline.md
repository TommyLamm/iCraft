# 任務 1：補完 Phase 0 可觀測性與固定基線

> 對應計畫：`14_performance_optimization.md` Phase 0
> 狀態：⏳ 待實作（部分已完成）
> 目標：完成 GPU timestamp、缺少的 counters 與固定場景基線，為所有後續性能任務提供可重現的 before/after 量測。
> Commit 訊息：`perf(instrument): complete gpu timestamps, counters and fixed benchmarks`

## 相關程式碼位置（已核對）

- `src/perf.rs:1` - `FrameScopes`、ring buffer、`ScopeId`、`record_nanos`、`snapshot`。
- `src/perf.rs:222` - `PerfCounters`（loaded_region_cache_bytes 等）。
- `src/state.rs:9003` - `State::render`：GPU pass 錄製點。
- `src/state.rs:5179` - `State::update`：CPU scope 錄製點。
- `src/state.rs:4481` - `trigger_background_save`：save queue depth 量測點。
- `src/state.rs:4005` - `drain_network_events`：network event 量測點。
- `src/chunk_render.rs:18` - `TerrainVertex`、mesh bounds。
- `src/app.rs` - `App::about_to_wait`：frame pacing 與 present scope。

## 已完成部分

- [x] 固定容量 256-sample ring buffer。
- [x] 熱路徑記錄不建立 `String`、`Vec` 或集合。
- [x] F3 顯示最近窗口的 average、p95、p99 與樣本數。
- [x] 已接入 17 個 CPU scopes（network_drain、world_tick、player_physics、chunk_schedule、terrain_result_integrate、lighting、redstone、hostile_mobs、passive_mobs、particles_update、render_prepare_terrain、render_prepare_entities、render_prepare_particles、render_prepare_ui、gpu_upload、render_encode、present）。
- [x] 已加入 loaded/visible chunks、terrain candidates/triangles/draw calls、GPU mesh bytes、GPU buffer objects、worker in-flight、stale results 與 tracked upload bytes 等 counters。

## 子任務清單

### 1.1 Adapter timestamp query 支援
- [ ] 檔案：`src/perf.rs`、`src/state.rs`
- 步驟：
  1. 在 `State::new` 的 wgpu device 初始化後，檢查 `wgpu::Features::TIMESTAMP_QUERY`（及 `TIMESTAMP_QUERY_INSIDE_PASSES`）。
  2. 若支援，啟用 feature 並建立 `wgpu::QuerySet`（足夠數量的 timestamp query slots）。
  3. 在 render pass 的 sky、opaque terrain、entity/mob、translucent terrain、particles、crack overlay、UI 各階段前後寫入 timestamp。
  4. 使用 `wgpu::Buffer` + `map_async` 在 frame 結束後非同步讀回 timestamp；不可阻塞 render loop。
  5. 將 GPU pass timings 寫入 `PerfCounters`，F3 顯示。
  6. 不支援時自動停用，不影響遊戲啟動（`State::new` 不因缺少 feature 而 panic）。
- 驗收：支援的 adapter 下 F3 顯示各 GPU pass 時間；不支援時靜默停用。

### 1.2 補完 lighting scope 涵蓋範圍
- [ ] 檔案：`src/lighting.rs`、`src/state.rs`、`src/fluid.rs`
- 步驟：
  1. 目前 `lighting` scope 主要量測 Chunk load propagation；確認 block mutation 後的 `update_sky_light_after_removed` / `update_block_light_after_placed` 等也包在同一 scope。
  2. 流體固化／流動造成的 lighting 更新加入 scope。
  3. 確認 scope 是巢狀累加（不重複計算），在 F3 標註為「含 chunk load + mutation」。
- 驗收：`lighting` p95 反映真實 lighting 工作量。

### 1.3 補完 gpu_upload scope 涵蓋範圍
- [ ] 檔案：`src/state.rs`、`src/particles.rs`、`src/mob_renderer.rs`
- 步驟：
  1. 目前 `gpu_upload` 只量測 terrain buffer enqueue/create；加入 particle、UI、camera uniform 及 crack overlay buffer write 的時間。
  2. 確認所有 `queue.write_buffer` 呼叫都在 scope 內。
- 驗收：`gpu_upload` 涵蓋所有 per-frame GPU buffer write。

### 1.4 Entity rendered/culled counters
- [ ] 檔案：`src/state.rs`、`src/mob_renderer.rs`
- 步驟：
  1. 在 `State::render` 的 entity 提交路徑加入三個 counter：`entities_rendered`、`entities_frustum_culled`、`entities_occlusion_culled`（occlusion 尚未實作時為 0）。
  2. F3 顯示。
- 驗收：F3 顯示實體渲染/剔除計數。

### 1.5 Save/network queue depth 與 region cache counters
- [ ] 檔案：`src/save.rs`、`src/state.rs`、`src/network/server.rs`、`src/network/client.rs`
- 步驟：
  1. `SaveManager` 加入 `pending_save_queue_depth` 與 `region_cache_bytes` counter（`save.rs:477` 的 `region_cache` HashMap）。
  2. `NetworkServer` / `NetworkClient` 加入 inbound/outbound queue depth counter。
  3. `cancelled_worker` counter 加入。
  4. 主線程每幀從 background thread 的共享 counter 讀取（用 `Arc<AtomicU32>` 或既有的同步通道）。
  5. F3 顯示。
- 驗收：autosave 與多人加入時 F3 顯示 queue 深度變化。

### 1.6 建立固定 seed 場景
- [ ] 檔案：`performance/benchmarks/`（新增目錄與場景描述檔）
- 步驟：建立以下可重播場景的描述與 seed：
  1. 開放地形，視距 8/16，靜止及快速旋轉。
  2. 洞穴或建築內，大量 Chunk 在視錐內但被遮擋。
  3. 以固定速度直線/對角飛行，持續載入和卸載 Chunk。
  4. 高密度紅石線路、repeaters、pistons 和 scheduled ticks。
  5. 大量流體、爆炸及 lighting 變更。
  6. 1,000 dropped items / mobs，其中大部分在牆後。
  7. 五分鐘 autosave 與連續 dirty-chunk save。
  8. Host + client 加入，包含大量 mutated chunks。
- 驗收：每個場景有明確 seed、座標、操作步驟與預期瓶頸。

### 1.7 記錄正式硬件基線
- [ ] 檔案：`performance/baselines/`（新增目錄）
- 步驟：
  1. 在固定場景下執行 `cargo run --release`，記錄 CPU/GPU frame time 的 p50、p95、p99 和 1% low。
  2. 記錄 working set、queue depth 與 upload bytes。
  3. 保存為 `performance/baselines/<date>_<hardware>.md`。
- 驗收：每個場景有 before 基線報告，供後續任務比較。

## 驗收條件

- [ ] 支援的 adapter 下 F3 顯示 7 個 GPU pass timings。
- [ ] `lighting` 與 `gpu_upload` scope 完整涵蓋各自工作。
- [ ] Entity rendered/culled counters 顯示。
- [ ] Save/network queue depth 與 region cache counters 顯示。
- [ ] 8 個固定 seed 場景已建立且可重播。
- [ ] 正式硬件基線報告已保存。
- [ ] `cargo fmt --all -- --check`、`cargo check --release`、`cargo test --release` 通過。

## 風險與回退

- GPU timestamp 讀回若造成 frame stall，改為每 N 幀取樣一次。
- 固定場景若無法自動重播，先以人工操作手冊 + 錄製座標序列替代。
- 本任務純量測，不改變遊戲行為，回退風險極低。

## 驗證命令

```text
cargo fmt --all -- --check
cargo check --release
cargo test --release
cargo run --release   # 手動驗證 F3 顯示與基線記錄
```
