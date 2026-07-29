# Performance Optimization Track

> 更新日期：2026-07-30
> 來源計畫：`14_performance_optimization.md`（同目錄）  
> 原則：每個任務獨立量測、驗證及回退；沒有改善 p95/p99 或記憶體的高複雜度改動不繼續擴大。  
> 詳細計畫：本目錄下 `01_*.md` ~ `14_*.md`，每個任務一份獨立 plan（含子任務、驗收條件、風險）。
> 審核基線：[`15_performance_audit_repair_plan.md`](15_performance_audit_repair_plan.md)；已知失敗見 [`repro/README.md`](repro/README.md)。

## 任務總覽

| # | 任務 | 詳細計畫 | 狀態 | 審核修復輪次 | Commit | 驗證 |
|---|---|---|---|---|---|---|
| 0 | 火把索引與週期掃描移除 | （不在 01–14 審核回退範圍） | Complete | — | - | `cargo test --release`（366 unit + 1 integration）；torch index focused tests 2 passed |
| 1 | 補完 Phase 0 可觀測性與固定基線 | [01_observability_baseline.md](01_observability_baseline.md) | Partial | R5、R9 | - | GPU/window 與固定場景 artifact 缺失 |
| 2 | 增量 prioritized Chunk queues | [02_streaming.md](02_streaming.md) | Partial | R1（已完成） | - | R1 correctness 已驗收；整體 Complete 仍受第 6 節 artifact/clippy gate 約束 |
| 3 | 真正的背景存檔 | [03_save.md](03_save.md) | Partial | R2（已完成） | - | durability/ACK/fault-injection 已驗收；固定場景 autosave p95 artifact 仍缺 |
| 4 | 多人 catch-up streaming | [04_network.md](04_network.md) | Partial | R3、R5 | - | backpressure/order/bounded drain 未驗收 |
| 5 | 固定 simulation tick | [05_simulation_tick.md](05_simulation_tick.md) | Partial | R4、R5、R9 | - | 只有合成測試；缺 world checksum |
| 6 | 紅石 dirty worklist 與 sleeping | [06_redstone.md](06_redstone.md) | Partial | R5 | - | sleep fast-path 與獨立 reference 未驗收 |
| 7 | Entity ID/type/spatial indexes | [07_entity.md](07_entity.md) | Partial | R5 | - | index 增量維護與主要消費路徑未驗收 |
| 8 | 重用 frame scratch 與靜態快取 | [08_render_scratch.md](08_render_scratch.md) | Partial | R7 | - | 穩態 allocation 與 hand cache 未驗收 |
| 9 | Entity、item 與 particle instancing | [09_render_instancing.md](09_render_instancing.md) | Partial | R7、R9 | - | ring completion 與視覺/性能 parity 缺失 |
| 10 | Region GPU arena | [10_render_gpu_arena.md](10_render_gpu_arena.md) | Partial | R6 | - | lifecycle、handle safety、runtime compact 未驗收 |
| 11 | Packed TerrainVertex 與 section meshing | [11_render_packed_vertex.md](11_render_packed_vertex.md) | Partial | R6 | - | AO decode 錯誤；section ownership 未完成 |
| 12 | Paletted ChunkSection | [12_memory_paletted.md](12_memory_paletted.md) | Partial | R7、R9 | - | storage 不 demote；memory accounting/microbench 缺失 |
| 13 | Section visibility 與 Entity occlusion | [13_culling.md](13_culling.md) | Partial | R6 | - | async LOS 永遠 visible；section culling 未完成 |
| 14 | Release、PGO 與 frame pacing | [14_build_release.md](14_build_release.md) | Partial | R8、R9 | - | FPS cap、真正 dynamic resolution、PGO A/B 缺失 |

狀態用語：

- `Pending`：尚未開始，或尚無足以確認有效實作的證據。
- `Partial`：已有部分實作，但仍有 correctness、durability、parity 或驗收 artifact 缺口。
- `Complete`：必須同時滿足 [`15_performance_audit_repair_plan.md`](15_performance_audit_repair_plan.md) 第 6 節全部完成定義；僅有編譯或單元測試通過不足以宣告完成。

## 實作順序與依賴

```
[已完成] 火把索引
   │
   ▼
1. 可觀測性與基線 ◄── 所有後續任務的量測前提
   │
   ├──► 2. streaming ──► 4. network
   │       │
   ├──► 3. save
   │
   ├──► 5. simulation tick ──► 6. redstone
   │                          └─► 7. entity
   │
   ├──► 8. render scratch ──► 9. instancing ──► 10. GPU arena
   │                                              └─► 11. packed vertex + section meshing
   │                                                    └─► 12. paletted storage
   │                                                          └─► 13. culling
   │
   └──► 14. build/release（最後，需穩定 workload 後才做 PGO）
```

依賴說明：
- **任務 1（基線）是所有後續任務的前提**：沒有可重現的 before/after 量測，任何改動都無法證明有效。
- **任務 5（固定 tick）是任務 6、7 的前提**：redstone/entity 優化建立在 tick 語義明確化之後。
- **任務 8->9->10->11->12->13 為渲染/記憶體路線的漸進鏈**：先消除穩態配置，再做 instancing，然後才動 GPU arena、packed vertex、paletted storage 與 occlusion。
- **任務 14（build）最後執行**：PGO 需要穩定 workload 才有意義。

## 目前進度

### 已完成：火把索引與週期掃描移除（不在 01–14 審核範圍）

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

> 每個任務的詳細子任務清單、驗收條件與風險評估見同目錄對應檔案。

### 1. 補完 Phase 0 可觀測性與固定基線

> 詳細計畫：[`01_observability_baseline.md`](01_observability_baseline.md)

- 完成 GPU timestamps 與缺少的 counters。
- 建立開放地形、遮擋室內、快速飛行、紅石、流體、1,000 entities、
  autosave 與多人加入固定場景。
- 記錄 CPU/GPU frame time p50、p95、p99、1% low、working set、queue depth
  與 upload bytes。

### 2. `perf(streaming)`：增量 prioritized Chunk queues

> 詳細計畫：[`02_streaming.md`](02_streaming.md)

- 預計算近到遠 spiral offsets。
- 只在跨 Chunk、切換維度或視距變更時更新 target。
- Load/dirty mesh queue 去重與優先級排序。
- 加入 unload hysteresis。
- 依時間、結果數及 upload bytes 設定每幀整合預算。
- 保留 generation/lifetime/revision stale-result 驗證。

### 3. `perf(save)`：真正的背景存檔

> 詳細計畫：[`03_save.md`](03_save.md)

- 建立 `DirtyChunkSet`，autosave 只 snapshot dirty chunks。
- Flatten、Bincode、Zlib 及 region I/O 全部移至 save worker。
- Bounded、latest-revision-wins queue。
- 按 region batch，一次 autosave 每個 region 只重寫一次。
- Temporary file、flush、atomic rename。
- Region cache byte/entry 上限與 LRU eviction。

### 4. `perf(network)`：多人 catch-up streaming

> 詳細計畫：[`04_network.md`](04_network.md)

- PlayerJoin 不同步壓縮全部 mutated chunks。
- 背景建立 payload，依玩家距離與 revision 排序並分幀傳送。
- 大型 payload bounded backpressure。
- Network event drain 加入時間及事件數預算。
- Pose/time 可 coalesce；可靠 block/chat/control 封包保持語義。

### 5. `perf(sim)`：固定 simulation tick

> 詳細計畫：[`05_simulation_tick.md`](05_simulation_tick.md)

- Frame update 與 20 Hz authoritative world simulation 分離。
- 最多四個 catch-up ticks，保留有界 debt。
- Player physics 使用固定 substep。
- AI、spawning、random tick、redstone、fluid、weather 改為 tick/秒語義。
- 驗證 30/60/144/240 FPS 下世界 checksum 一致。

### 6. `perf(redstone)`：dirty worklist 與 sleeping

> 詳細計畫：[`06_redstone.md`](06_redstone.md)

- Mutation、scheduled tick、pressure plate change 推入去重 worklist。
- 只重新計算 changed node 與鄰接元件。
- 無 dirty/scheduled/active device 時 sleep。
- 移除完整 component `HashMap` clone。
- 加入跨 Chunk、大型線路及 loop budget parity tests。

### 7. `perf(entity)`：ID/type/spatial indexes

> 詳細計畫：[`07_entity.md`](07_entity.md)

- `EntityId -> dense index`，`swap_remove` 後修正 index。
- EntityType 及 Chunk/section buckets。
- Nearby collision、pickup、melee、projectile、spawn、render 先查 bucket。
- 使用 distance-squared 並重用 scratch vectors。

### 8. `perf(render)`：重用 frame scratch 與靜態快取

> 詳細計畫：[`08_render_scratch.md`](08_render_scratch.md)

- 重用 terrain candidates、draw plan、LOD、mob、particle、UI storage。
- `DrawCandidate` 攜帶 LOD 與 distance key。
- 快取 debug/UI labels 及靜態 geometry。
- 手持模型只在 held item/model 改變時重建。
- 加入穩態 allocation counter。

### 9. `perf(render)`：Entity、item 與 particle instancing

> 詳細計畫：[`09_render_instancing.md`](09_render_instancing.md)

- 預建 entity/item prototype meshes。
- 每個可見 entity 只上傳 instance data。
- 依 model/material 分組批次繪製。
- Particle 改為固定 unit quad 加 instance buffer。
- 動態 instance buffer 使用 ring/staging 策略。

### 10. `perf(render)`：Region GPU arena

> 詳細計畫：[`10_render_gpu_arena.md`](10_render_gpu_arena.md)

- 以 8×8 Chunk 或 benchmark 選定大小建立 `RenderRegion`。
- Region 共用少量 vertex/index arenas。
- Free-list/buddy suballocation、stale handle 防護與低優先 compact。
- 空 mesh 不建立 placeholder buffer。
- 目標：視距 16 的 GPU buffer object 數降低至少 90%。

### 11. `perf(render)`：Packed TerrainVertex 與 section meshing

> 詳細計畫：[`11_render_packed_vertex.md`](11_render_packed_vertex.md)

- TerrainVertex 壓縮至約 16–20 bytes。
- Shader 從 region origin 重建 world position。
- Chunk 垂直切為 16³ sections。
- Mutation 只 dirty 所在 section 及必要 halo。
- Worker request 使用 section revision、Chunk lifetime、dimension generation。
- L0/L1/L2 分開 residency 與 priority。

### 12. `perf(memory)`：Paletted ChunkSection

> 詳細計畫：[`12_memory_paletted.md`](12_memory_paletted.md)

- 先建立 storage access abstraction。
- Block storage 支援 Empty、Uniform、Paletted、Global。
- Sky/block light 合併為 nibble-packed byte。
- 全零 state/fluid 使用 optional storage。
- 保存 non-air、opaque、random-tick、fluid、emitter、redstone counts。
- Save/network wire format初期保持不變，在邊界 flatten。

### 13. `perf(culling)`：Section visibility 與 Entity occlusion

> 詳細計畫：[`13_culling.md`](13_culling.md)

- 剔除順序：distance、frustum、section visibility、optional async LOS。
- Meshing worker 建立 section face connectivity。
- Camera section 執行 bounded visibility graph traversal。
- Entity LOS 使用低優先 queue、TTL 與 last-visible hysteresis。
- Boss、projectile、近距 remote player 等類型 bypass。
- 所有 stale、timeout、overflow 狀態 fail-open。

### 14. `perf(build)`：Release、PGO 與 frame pacing

> 詳細計畫：[`14_build_release.md`](14_build_release.md)

- 加入經測試的 release profile：`opt-level = 3`、thin LTO、
  `codegen-units = 1`。
- 固定 workload A/B 後才評估 PGO。
- VSync off 優先 Mailbox，fallback Immediate。
- FPS cap 不影響 simulation tick。
- Dynamic resolution/entity distance scaling 預設關閉。
- Windows 保留目前已驗證的 DX12 backend。

## 驗證紀錄

審核前曾執行：

- `cargo test --release`：主程式 368 tests + integration 1 test，全數通過。
- `cargo fmt --all -- --check`：全數通過。
- `cargo check --release`：無 error / warning 阻擋，編譯通過。
- Perf ring focused tests：3 passed。
- Torch index focused release tests：2 passed。
- Dimension focused release tests：9 passed。
- `git diff --check`：通過。

已知驗證限制：

| 缺口 | 影響 | 補完輪次 |
|---|---|---|
| 未執行實際 GPU/window 視覺驗證 | GPU timestamp、AO、instancing、arena、culling 與 dynamic resolution 的畫面正確性未知 | R5、R6、R7、R8、R9 |
| 未建立 8 個固定 seed 場景及可重播 raw artifact | 不能審計 p50/p95/p99、1% low、working set 或 before/after 百分比 | R9 |
| 現有基線只使用 render distance 8 | 不得作為 render distance 16 的 buffer/memory/performance claims 證據 | R9 |
| 缺少 host/client world、entity、health checksum | multiplayer authority 與 catch-up 最終收斂未驗證 | R3、R4、R9 |
| 缺少 slow-client、capacity=1 與多 client backpressure 場景 | catch-up Chunk 可能永久缺漏，可靠封包順序亦未驗證 | R3、R9 |
| 缺少 CPU packing ↔ WGSL decode parity/golden | packed AO 與其他 shader decode 不能宣告視覺等價 | R6、R9 |
| 缺少 30/60/144/240 FPS headless full-world checksum | fixed tick 目前只由合成位移測試支撐 | R5、R9 |
| 缺少 non-PGO/PGO 相同 workload A/B | PGO 與 release 性能改善不可宣告 | R9 |

目前可重播的已知失敗控制流與缺口，集中記錄於 [`repro/README.md`](repro/README.md)。

## 持續風險

- `Chunk::blocks` 仍是 public；目前 runtime direct writes 均在生成／restore 後
  rebuild index，但未來新增 direct writer 可能令火把索引失效。長期應收斂至
  統一 mutation/storage access API（任務 12）。
- `world_tick` 是總時間，包含 redstone、mob、chunk schedule 等子 scopes；
  這些數字是巢狀關係，不可相加。
- F3 snapshot 每 0.5 秒更新，顯示的是最近窗口摘要，不是即時單幀值。
- 高複雜度的 GPU arena（任務 10）、packed vertex（任務 11）、paletted storage
  （任務 12）與 occlusion（任務 13）必須先有基線證明是瓶頸，才進入實作。

## 提交規範

每個任務完成後：

1. 更新本檔案的任務總覽表狀態欄、commit hash 與驗證摘要。
2. 在對應的詳細計畫檔案中勾選所有子任務與驗收條件。
3. 執行 `cargo fmt --all -- --check`、`cargo check --release`、`cargo test --release`。
4. 保存 before/after 性能報告（任務 1 基線建立後）。
5. 單一功能 commit，訊息格式：`perf(<area>): <description>`。
