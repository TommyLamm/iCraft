# Performance Optimization Track

> 更新日期：2026-07-28  
> 來源計畫：`plans/implementation/14_performance_optimization.md`  
> 原則：每個階段獨立量測、驗證及回退；沒有改善 p95/p99 或記憶體的高複雜度改動不繼續擴大。

## 目前進度

### 已完成：火把索引與週期掃描移除

- `Chunk` 使用緊湊 `u16` 保存普通火把的本地座標。
- Chunk 生成、維度生成、存檔／網路 payload restore 後會建立或重建索引。
- `ChunkManager::set_block` 增量新增或移除火把位置。
- 火把煙霧改為遍歷索引，不再每 0.4 秒掃描所有載入 Chunk 的 voxel arrays。
- 保留舊行為：只對偶數 Y 的普通 `BlockType::Torch` 產生煙霧。
- 已覆蓋負世界座標、重複寫入、替換、移除及存檔恢復測試。

### 部分完成：Phase 0 可觀測性

已完成：

- 固定容量 256-sample ring buffer。
- 熱路徑記錄不建立 `String`、`Vec` 或集合。
- F3 顯示最近窗口的 average、p95、p99 與樣本數。
- 已接入 17 個 CPU scopes：
  - `network_drain`
  - `world_tick`
  - `player_physics`
  - `chunk_schedule`
  - `terrain_result_integrate`
  - `lighting`
  - `redstone`
  - `hostile_mobs`
  - `passive_mobs`
  - `particles_update`
  - `render_prepare_terrain`
  - `render_prepare_entities`
  - `render_prepare_particles`
  - `render_prepare_ui`
  - `gpu_upload`
  - `render_encode`
  - `present`
- 已加入 loaded/visible chunks、terrain candidates/triangles/draw calls、GPU mesh
  bytes、GPU buffer objects、worker in-flight、stale results 與 tracked upload
  bytes 等 counters。

仍需補完：

- Adapter timestamp query 支援與 GPU pass timings。
- `lighting` 目前主要量測 Chunk load propagation；還需涵蓋 block、fluid 及其他
  runtime lighting mutation。
- `gpu_upload` 目前是 CPU enqueue/create-buffer 量測，尚未完整涵蓋 particle、
  UI、camera 及 crack buffer writes。
- Entity rendered/frustum-culled/occlusion-culled counters。
- Save/network queue depth、region cache bytes、cancelled worker 等 counters。
- 固定 seed 場景、before/after 報告及正式硬件基線。

## 剩餘實作順序

### 1. 補完 Phase 0 可觀測性與固定基線

- 完成 GPU timestamps 與缺少的 counters。
- 建立開放地形、遮擋室內、快速飛行、紅石、流體、1,000 entities、
  autosave 與多人加入固定場景。
- 記錄 CPU/GPU frame time p50、p95、p99、1% low、working set、queue depth
  與 upload bytes。

### 2. `perf(streaming)`：增量 prioritized Chunk queues

- 預計算近到遠 spiral offsets。
- 只在跨 Chunk、切換維度或視距變更時更新 target。
- Load/dirty mesh queue 去重與優先級排序。
- 加入 unload hysteresis。
- 依時間、結果數及 upload bytes 設定每幀整合預算。
- 保留 generation/lifetime/revision stale-result 驗證。

### 3. `perf(save)`：真正的背景存檔

- 建立 `DirtyChunkSet`，autosave 只 snapshot dirty chunks。
- Flatten、Bincode、Zlib 及 region I/O 全部移至 save worker。
- Bounded、latest-revision-wins queue。
- 按 region batch，一次 autosave 每個 region 只重寫一次。
- Temporary file、flush、atomic rename。
- Region cache byte/entry 上限與 LRU eviction。

### 4. `perf(network)`：多人 catch-up streaming

- PlayerJoin 不同步壓縮全部 mutated chunks。
- 背景建立 payload，依玩家距離與 revision 排序並分幀傳送。
- 大型 payload bounded backpressure。
- Network event drain 加入時間及事件數預算。
- Pose/time 可 coalesce；可靠 block/chat/control 封包保持語義。

### 5. `perf(sim)`：固定 simulation tick

- Frame update 與 20 Hz authoritative world simulation 分離。
- 最多四個 catch-up ticks，保留有界 debt。
- Player physics 使用固定 substep。
- AI、spawning、random tick、redstone、fluid、weather 改為 tick/秒語義。
- 驗證 30/60/144/240 FPS 下世界 checksum 一致。

### 6. `perf(redstone)`：dirty worklist 與 sleeping

- Mutation、scheduled tick、pressure plate change 推入去重 worklist。
- 只重新計算 changed node 與鄰接元件。
- 無 dirty/scheduled/active device 時 sleep。
- 移除完整 component `HashMap` clone。
- 加入跨 Chunk、大型線路及 loop budget parity tests。

### 7. `perf(entity)`：ID/type/spatial indexes

- `EntityId -> dense index`，`swap_remove` 後修正 index。
- EntityType 及 Chunk/section buckets。
- Nearby collision、pickup、melee、projectile、spawn、render 先查 bucket。
- 使用 distance-squared 並重用 scratch vectors。

### 8. `perf(render)`：重用 frame scratch 與靜態快取

- 重用 terrain candidates、draw plan、LOD、mob、particle、UI storage。
- `DrawCandidate` 攜帶 LOD 與 distance key。
- 快取 debug/UI labels 及靜態 geometry。
- 手持模型只在 held item/model 改變時重建。
- 加入穩態 allocation counter。

### 9. `perf(render)`：Entity、item 與 particle instancing

- 預建 entity/item prototype meshes。
- 每個可見 entity 只上傳 instance data。
- 依 model/material 分組批次繪製。
- Particle 改為固定 unit quad 加 instance buffer。
- 動態 instance buffer 使用 ring/staging 策略。

### 10. `perf(render)`：Region GPU arena

- 以 8×8 Chunk 或 benchmark 選定大小建立 `RenderRegion`。
- Region 共用少量 vertex/index arenas。
- Free-list/buddy suballocation、stale handle 防護與低優先 compact。
- 空 mesh 不建立 placeholder buffer。
- 目標：視距 16 的 GPU buffer object 數降低至少 90%。

### 11. `perf(render)`：Packed TerrainVertex 與 section meshing

- TerrainVertex 壓縮至約 16–20 bytes。
- Shader 從 region origin 重建 world position。
- Chunk 垂直切為 16³ sections。
- Mutation 只 dirty 所在 section 及必要 halo。
- Worker request 使用 section revision、Chunk lifetime、dimension generation。
- L0/L1/L2 分開 residency 與 priority。

### 12. `perf(memory)`：Paletted ChunkSection

- 先建立 storage access abstraction。
- Block storage 支援 Empty、Uniform、Paletted、Global。
- Sky/block light 合併為 nibble-packed byte。
- 全零 state/fluid 使用 optional storage。
- 保存 non-air、opaque、random-tick、fluid、emitter、redstone counts。
- Save/network wire format初期保持不變，在邊界 flatten。

### 13. `perf(culling)`：Section visibility 與 Entity occlusion

- 剔除順序：distance、frustum、section visibility、optional async LOS。
- Meshing worker 建立 section face connectivity。
- Camera section 執行 bounded visibility graph traversal。
- Entity LOS 使用低優先 queue、TTL 與 last-visible hysteresis。
- Boss、projectile、近距 remote player 等類型 bypass。
- 所有 stale、timeout、overflow 狀態 fail-open。

### 14. `perf(build)`：Release、PGO 與 frame pacing

- 加入經測試的 release profile：`opt-level = 3`、thin LTO、
  `codegen-units = 1`。
- 固定 workload A/B 後才評估 PGO。
- VSync off 優先 Mailbox，fallback Immediate。
- FPS cap 不影響 simulation tick。
- Dynamic resolution/entity distance scaling 預設關閉。
- Windows 保留目前已驗證的 DX12 backend。

## 驗證紀錄

本輪完成：

- `cargo test`：主程式 358 tests + integration 1 test，全數通過。
- `cargo test --release`：主程式 358 tests + integration 1 test，全數通過。
- 最終 `cargo check --release`：通過。
- Perf ring focused tests：3 passed。
- Torch index focused release tests：2 passed。
- Dimension focused release tests：9 passed。
- `git diff --check`：通過。

已知驗證限制：

- `cargo fmt -- --check` 仍被倉庫既有的 `mob.rs`、`mob_renderer.rs`、
  `save.rs` 與部分 `state.rs` 格式差異阻擋。
- 尚未執行需要實際 GPU/window 的 `cargo run --release` 視覺驗證。
- 尚未建立 Phase 0 固定場景或正式 before/after 性能報告。

## 持續風險

- `Chunk::blocks` 仍是 public；目前 runtime direct writes 均在生成／restore 後
  rebuild index，但未來新增 direct writer 可能令火把索引失效。長期應收斂至
  統一 mutation/storage access API。
- `world_tick` 是總時間，包含 redstone、mob、chunk schedule 等子 scopes；
  這些數字是巢狀關係，不可相加。
- F3 snapshot 每 0.5 秒更新，顯示的是最近窗口摘要，不是即時單幀值。
- 高複雜度的 GPU arena、packed vertex、paletted storage 與 occlusion 必須先有
  基線證明是瓶頸，才進入實作。
