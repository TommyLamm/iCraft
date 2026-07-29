# 性能優化 01–14 審核修復計畫

> 審核日期：2026-07-29  
> 審核基線：`master` / `5f1ee4d`  
> 狀態：待修復  
> 範圍：`performance/01`–`14`、`performance/14_performance_optimization.md`、`ARCHITECTURE.md` 與其對應實作  
> 原則：先修資料遺失、世界分歧與畫面錯誤，再補完未落實的優化，最後才重新宣告性能成果。

## 1. 審核結論

目前不能判定 14 項性能優化已正確完成。自動測試大致通過，但測試沒有覆蓋多個實際控制流錯誤；性能百分比也沒有可審計的 before/after artifact。

| 任務 | 結論 | 主要原因 |
|---|---|---|
| 01 可觀測性 | Fail | GPU timestamp readback 次序錯誤；lighting / upload scopes 不完整；queue counters 低報；基線 artifact 不足。 |
| 02 Streaming | Fail | 多數 mutation 只 `mesh.mark_dirty()`，沒有加入 scheduler，mesh 可永久 stale。 |
| 03 Save | Fail | queue 無界、沒有 revision/ACK、I/O error 被吞、Windows replace 非 atomic。 |
| 04 Network | Fail | join payload 仍在主線程壓縮；mailbox full 永久丟 Chunk；距離排序被破壞；drain budget 無效。 |
| 05 Fixed tick | Partial | 20 Hz accumulator 已存在，但驗收測試只是合成位移，不是完整 world checksum；host pause/death policy 錯誤。 |
| 06 Redstone | Fail | State 永遠傳入 player occupant，sleep fast-path 在正常 gameplay 不會生效；differential test 不是獨立 reference。 |
| 07 Entity index | Fail | spatial index 每輪全量重建，主要 AI、pickup、projectile、render 查詢仍掃描完整 `Vec<Entity>`。 |
| 08 Render scratch | Fail | 每幀仍建立 entity/UI/String buffers；hand animation 仍重建並上傳 CPU geometry。 |
| 09 Instancing | Partial | 基本 instancing 已存在，但 ring 沒有 GPU completion 保護，粒子 instance 缺計畫中的 color/light，性能與視覺 parity 無 artifact。 |
| 10 GPU arena | Fail | 維度切換未清 arena；handle 無 owner/generation 防 stale/double-free；compact 未接入 runtime。 |
| 11 Packed vertex/section mesh | Fail | shader AO decode 與 CPU 不一致；section meshing 實際仍是整個 `16×256×16` Chunk。 |
| 12 Paletted memory | Partial | 基本 palette/packed light 已存在，但 storage 不 demote，外部仍可直接 match storage，memory counter 不反映實際 representation，沒有 microbenchmark。 |
| 13 Culling | Fail | async LOS worker 沒有地形資料且永遠回 visible；stale graph 未 fail-open；透明/非完整模型可被錯當 occluder；沒有 section mesh 可供 section draw culling。 |
| 14 Build/release | Fail | release profile 與 Mailbox/DX12 已落實；FPS cap、真正 dynamic resolution、PGO A/B 與固定場景結果不存在。 |

跨任務另發現 multiplayer entity authority 與 `ARCHITECTURE.md` 不一致：joining client 仍自行 spawn/模擬 living entities 並承受本地 AI 傷害，而 protocol 沒有 entity/health replication。這不一定由性能改動引入，但違反本輪「不得改變多人權威模型」的前提，必須作為 release blocker。

### 關鍵證據索引

| 問題 | 主要證據 |
|---|---|
| Mesh dirty 漏排程 | `src/state.rs:5486-5491, 5861-5888`，以及直接 `mesh.mark_dirty()` 的 `1372-1375, 6020-6038, 7403-7406, 7777-7780` 等路徑。 |
| Save ownership/atomicity | `src/state.rs:3231-3330, 5281-5295, 5784-5799`；`src/save.rs:509-530, 650-690`。 |
| Catch-up 漏 Chunk | `src/state.rs:4647-4703`；`src/network/server.rs:175-203, 634-652`。 |
| Network budget 無效 | `src/state.rs:2378-2555, 4733-4803`。 |
| GPU timestamp readback | `src/state.rs:3137-3179, 12963-12997`。 |
| GPU arena 維度洩漏 | dimension reset `src/state.rs:1551-1559` 未清 `render_regions`；network reset `4830-4837` 有清除。 |
| Packed AO mismatch | `src/chunk_render.rs:55-60, 113-120`；`src/world.rs:1539-1545`；`src/shader.wgsl:162-163`。 |
| Section meshing 未落實 | `src/state.rs:1047-1052, 1115-1167, 5725-5747`；`src/world.rs:3691-3702`。 |
| Async LOS 永遠 visible | `src/culling.rs:347-394, 406-493`。 |
| Dynamic resolution 畫面錯誤 | `src/state.rs:12658-12671, 12902-12911`；沒有 scaled render target/upscale。 |
| Multiplayer authority 分歧 | `src/state.rs:6517-6528, 6598-6606, 6848-6865`；`src/mob.rs:617-633`；`src/network/protocol.rs:210-302`。 |
| 驗收 artifact 不足 | `performance/performance_track.md:24-26, 270-273`；`performance/baselines/2026-07-28_windows_dx12.md:7-61`。 |

## 2. P0：先修 correctness / durability blockers

### 2.1 統一所有 mesh mutation 的 dirty enqueue

涉及：

- `src/state.rs`
- `src/chunk_schedule.rs`
- `src/chunk_manager.rs`
- mutation producers：fluid、lighting、weather、redstone、network、break/place、mob/boss

問題：

- `State::mark_chunk_dirty` 同時更新 mesh revision 與 scheduler，但大量路徑直接呼叫 `mesh.mark_dirty()`。
- `update_chunks` 只消費 `scheduler.dirty_chunk_meshes`，因此這些 mutation 不會重新排程 mesh。

修復：

1. 建立唯一 API，例如 `invalidate_chunk_mesh(coord, dependency_reason)`。
2. API 必須同時：
   - 遞增 chunk/section revision；
   - enqueue 去重的 priority work；
   - 套用 boundary/diagonal AO dependency；
   - 將 stale connectivity 立即標為 invalid/fail-open。
3. 禁止 runtime 直接呼叫 `ChunkMesh::mark_dirty`；以 visibility 或 lint/test 約束。
4. dirty queue 改為持久 priority queue，不要每次 dispatch 重建及排序完整臨時 `Vec`。

驗收：

- place、break、fluid、light、weather、redstone、remote block change、Chunk boundary 與 diagonal AO mutation 都會排入一次工作。
- 玩家停在原 Chunk 時也會整合新 revision；舊 revision 結果必須丟棄。
- 加入「mutation → scheduler queued → worker result → visible mesh revision」整合測試。

### 2.2 重做 save ownership、ACK 與 atomic replacement

涉及：

- `src/save.rs`
- `src/state.rs`
- Windows platform replacement helper

問題：

- `std::sync::mpsc::channel` 無界，F3 counter 不含 channel backlog。
- `SaveCommand`/snapshot 沒有 revision；dirty set 在 enqueue 前已 drain，send/I/O error 又被忽略。
- `Flush` 即使寫入失敗仍 ACK success。
- `atomic_write` 在 Windows 先刪舊檔再 rename，有 crash/data-loss window。
- 讀既有 region 失敗時可建立空 region 再覆寫，風險是丟失同 region 其他 Chunk。

修復：

1. 改為 bounded queue；每項用 `(dimension, chunk, revision)` latest-wins。
2. dirty 狀態拆成 `dirty / in_flight / persisted_revision`：
   - enqueue 成功後保留 in-flight；
   - worker 成功持久化並 ACK 才清除對應 revision；
   - 新 mutation revision 不得被舊 ACK 清除；
   - enqueue、serialize、open、read、deserialize、write 或 replace 失敗都要 requeue/保留 dirty。
3. `Flush` 回傳 `Result`，UI/quit path 必須能顯示失敗而不是假成功。
4. Windows 使用 `ReplaceFileW` 或 `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`；temp 檔同目錄且名稱唯一。
5. region file 存在但讀取/反序列化失敗時停止覆寫並回報 corruption；不可 fallback 成空 region。
6. queue depth 包含 producer backlog、worker pending 與 in-flight bytes；修正 phantom LRU key 累積。

驗收：

- fault injection 覆蓋 enqueue failure、worker panic、serialize/I/O failure、replace 前/中/後 crash。
- 重啟後只可讀到完整舊版或完整新版，不能缺檔、部分檔或遺失同 region 其他 Chunk。
- 暫停 worker 並持續 mutation/unload 時記憶體仍有上限，counter 與真實 backlog 一致。
- 快速重複修改同一 Chunk 時只允許最高 revision 成為 persisted。

### 2.3 令 join catch-up 具備可靠 backpressure 與持久來源

涉及：

- `src/state.rs`
- `src/network/server.rs`
- `src/network/client.rs`
- `src/network/protocol.rs`
- `src/save.rs`

問題：

- State 在 server 接受前已 `pop_front`；`CatchupMailbox::replace` 滿時回 `false`，caller 忽略結果。
- mailbox drain 依 `(cx, cz)` 排序，破壞近距優先。
- join payload 在主線程呼叫 `ChunkSaveData::from_chunk` 並 Zlib 壓縮。
- catch-up 來源只看當下 loaded mutated chunks；已 unload 的歷史 mutation 不會補給新玩家。
- reliable FIFO 與 catch-up mailbox 是不同 select branch，缺少跨 channel revision/order 規則。

修復：

1. 以 `(player, dimension, chunk, revision)` 管理傳輸所有權；server 明確接受後才 dequeue。
2. mailbox full 要保留/retry，不得 silent drop；slow client policy 必須明確且可觀測。
3. 把 snapshot flatten/compress 移到 bounded worker，主線程只提交 immutable request。
4. 從持久、可回收的 mutation/revision index 建立 catch-up，不依賴 host 當下 loaded set；取代目前只增不減、join 時全量排序的 `mutated_chunks` HashSet。
5. 保留距離 priority；同一 Chunk latest revision wins。
6. 定義 Chunk snapshot 與其後 BlockChange 的順序：client 不得用較舊 snapshot 覆寫新 mutation。

驗收：

- mailbox capacity=1、慢 client、多 client、unloaded mutated Chunk 均可最終收斂 checksum。
- 沒有 Chunk 永久缺漏；近距 Chunk 先到；可靠 chat/control/block packet 保序。
- catch-up 壓力場景中主線程不執行 flatten/Zlib，且每幀總 budget 不隨 client 數線性放大。

### 2.4 修正 multiplayer authority 與 host pause policy

涉及：

- `src/state.rs`
- `src/mob.rs`
- `src/passive_mob.rs`
- `src/boss.rs`
- `src/network/protocol.rs`
- `src/app.rs`
- `ARCHITECTURE.md`

修復：

1. 採用文件既定的 host-authoritative 模型：
   - living entity spawn、AI、damage、drops、breeding、despawn、persistence 只在 host；
   - protocol 增加 entity spawn/state/despawn 與 player health/effect replication；
   - client 只插值或做可回滾的純視覺預測。
2. Host 開 pause menu或死亡時只 gate 本機 controller/UI；只在 singleplayer 暫停權威 world clock。
3. network state 要在 catch-up simulation ticks 前套用，避免用舊 remote pose/action 驗證。

驗收：

- host/client 分處不同位置 60 秒，client 不自行生成 living entity。
- host entity/world/health checksum 與 client replicated state 收斂。
- host pause/death 時 world time、redstone、fluid、autosave 與 remote action 仍前進，本機 movement 停止。

## 3. P1：修復錯誤的 instrumentation、render 與 culling

### 3.1 GPU timestamp、scope 與 queue telemetry

1. `map_async` 成功且 `device.poll` 完成後才呼叫 `get_mapped_range`；建立 readback state machine。
2. 區分 `TIMESTAMP_QUERY` 與 `TIMESTAMP_QUERY_INSIDE_PASSES`；不支援的 pass path 顯示 N/A，不顯示假零值。
3. lighting scope 涵蓋 load、block、fluid、weather、redstone 等所有 mutation。
4. gpu_upload scope 涵蓋 camera、UI、crack、particle、entity、terrain writes。
5. 分開 inbound/outbound/reliable/catch-up/save producer/worker queue depth、bytes、drop/retry/cancel counters。
6. 加入 timestamp supported/unsupported adapter state-machine tests。

### 3.2 Network drain 真正跨幀 bounded

1. 移除 `try_iter().collect()` 全量 drain。
2. 可靠事件保存在跨幀 FIFO；pose/time 使用 per-key latest-wins mailbox，但超 budget 後最後值不能丟失。
3. budget 同時限制 events、bytes 與 elapsed time。
4. 每幀先處理必要 network state，再進行 authoritative ticks。

驗收：

- 大 burst 下單幀處理量有界；所有 reliable packet 最終按序完成。
- pose/time burst 後每個 key 的最後值必定保留。

### 3.3 完成 fixed tick、redstone sleep 與 entity index

1. 將 synthetic fixed-tick test 換成 headless world harness：
   - 固定 seed/input；
   - 30/60/144/240 render FPS；
   - 比較 blocks/light/fluid/redstone/entities/player/world-time checksum。
2. 明確 player physics substep 頻率與 collision 上限，不以單次 50 ms step 代替驗收。
3. Redstone sleep 判斷不能因永遠存在的 player occupant 失效：
   - 只追蹤 pressure plate occupant set 的變化；
   - idle 時不掃全部 plate/component；
   - differential test 使用獨立 reference implementation/fixture。
4. Entity spatial/type index改為跨 bucket 移動的增量維護；pickup、AI、partner search、projectile、melee、spawn 與 render 必須消費 bucket query。
5. 禁止同一 tick 在 mob/passive/boss/state 多次全量 rebuild index。

### 3.4 修復 GPU arena lifecycle

1. dimension switch、disconnect、world reset、unload 時統一 teardown `render_regions`。
2. allocation handle 增加 region identity、slot、generation/owner；free 驗證 bounds、generation、overlap，double-free 返回錯誤。
3. used/free counters 使用 checked arithmetic。
4. fragmentation threshold 接入低優先 compact；compact 不可在 gameplay frame 同步重建整個 region。
5. F3 顯示實際建立的 buffer objects，而非 `render_regions.len() * 2` 估算。

驗收：

- 維度往返後舊 region allocations/buffers 歸零。
- stale/double-free/out-of-bounds handle 測試不破壞 allocator。
- 隨機 allocate/free/compact property test 保持無重疊、used+free=capacity。

### 3.5 修正 packed vertex 與真正 section meshing

1. AO decode 與 CPU mapping一致：
   - packed codes `3/2/1/0` 對應 `1.0/0.75/0.5/0.25`；
   - shader 不可用 `ao_raw / 3.0` 取代離散 mapping。
2. 加入 CPU packing ↔ WGSL decode parity test/golden render。
3. 將 mesh ownership 改成 section：
   - 16³ section mesh/revision/connectivity；
   - 18³ halo snapshot；
   - mutation 只 dirty 本 section及必要 halo neighbors；
   - LOD residency與 priority 以 section/revision 管理；
   - worker result驗證 dimension generation、chunk lifetime、section revision。
4. 未完成 section storage 前，不得在任務 11/13 文件宣稱 section remesh/draw culling 完成。

### 3.6 重做 conservative section/entity culling

1. `is_section_occluder` 只接受完整、實心、opaque cube；glass、ice、fluid、leaves、cutout、cross/thin/custom model 一律 fail-open。
2. mesh dirty 時立即 invalid connectivity；新 revision graph 回來前視為全可見。
3. async LOS request 必須攜帶最小 immutable voxel snapshot、dimension/generation/chunk revisions、camera cell 與 entity identity。
4. worker 使用 snapshot 做真實 LOS；poll 只接受全部 identity 相符結果，stale/timeout/overflow 一律 visible。
5. 只有 section-level mesh/handle 存在後才宣稱 terrain section 被 skip；否則只能算 whole-chunk coarse culling。
6. 加入 culling counters：distance、frustum、section、LOS、fail-open、stale result。

驗收：

- 牆後 entity 穩定 cull；拆牆/開門後同一幀先 fail-open，之後更新 graph。
- stale dimension/revision result不能寫 cache。
- 每種 translucent/cutout/special model 都不能造成 false cull。
- 快速轉身、teleport、queue overflow、未載入 section 均不會永久消失。

### 3.7 修正 render-scale 視覺錯誤

1. 暫時隱藏/停用目前只縮 viewport 的 `dynamic_resolution`，避免世界只畫在 swapchain 左上角。
2. 若保留功能：
   - 建立低解析度 offscreen color/depth；
   - terrain/entity/particle render 到 scaled target；
   - upscale 到完整 surface；
   - UI 在 native surface rendering；
   - GPU-time feedback、上下界、hysteresis、cooldown。
3. 加入獨立 FPS cap setting與 `ControlFlow::WaitUntil`/frame deadline；simulation accumulator 使用真實 elapsed，不使用 cap interval。

## 4. P2：補完未達成的性能承諾

### 4.1 Render scratch / hand / instancing

1. 將 `visible_entities`、UI textured vertices、debug labels、uppercase/format results 移到可重用 scratch/cache。
2. 建立實際 allocation instrumentation或測試 allocator；不能只靠手工 counter 推定零 allocation。
3. hand 基礎 mesh只在 held item/model 改變時重建；walk/attack swing移到 transform/uniform。
4. instance ring 使用 queue completion/fence 或足夠的 staging belt，不只固定三槽輪轉。
5. ParticleInstance 補齊 color/light 或明確修訂 plan 與視覺基準。

### 4.2 Paletted storage demotion 與真實 memory accounting

1. state/fluid 全部歸零後釋放 optional allocation。
2. Block/Light storage在適當時機由 Global/Paletted/Packed demote回 Uniform/Empty；避免每次 set 做昂貴掃描，可用 counts/dirty compaction policy。
3. 將 `ChunkSection` representation 收斂為私有，外部只用 access/query API。
4. memory counter按實際 palette、packed indices、optional arrays、light storage與 container overhead計算。
5. 建立可執行 microbench，覆蓋 `get/set`、lighting、collision、meshing與 save/network flatten。

### 4.3 PGO、benchmark 與 artifact gate

1. 暫時把 01–14 未驗收項目改回 `Partial`/`Pending`；不要保留全 `[x]`。
2. 保存完整硬件資訊：CPU、GPU、RAM、driver、OS、wgpu backend、commit、settings、resolution、render distance。
3. 8 個固定場景各自保存 before/after：
   - CPU/GPU p50/p95/p99；
   - 1% low；
   - working set；
   - upload bytes、draw calls、buffer objects；
   - save/network queue depth與delay；
   - correctness checksum。
4. 視距 16 的 claims 必須用視距 16量測；目前視距 8 baseline不能作為證據。
5. PGO 使用相同 workload做 non-PGO/PGO A/B；改善未達既定門檻就不納入 pipeline。
6. 每份報告包含 raw data 或可重播輸出，不只手寫摘要。

## 5. 實作順序與 gate

```text
R0 先把文件狀態改回 Partial/Pending，保存目前失敗 reproductions
 |
 +-- R1 mesh dirty correctness
 +-- R2 save durability/ACK/atomic replace
 +-- R3 network catch-up/backpressure/order
 +-- R4 multiplayer authority/pause
 |
 +-- R5 instrumentation + network budget + sim/redstone/entity
 |
 +-- R6 arena lifecycle -> packed AO -> section meshing
                              |
                              +-> conservative section/entity culling
 |
 +-- R7 render scratch/hand/instancing + paletted demotion
 |
 +-- R8 FPS cap / real dynamic resolution（可選）
 |
 +-- R9 fixed-scene before/after + PGO A/B + 文件重新驗收
```

### 分輪詳細計畫

| 輪次 | 詳細計畫連結 |
|---|---|
| R0 回退文件狀態與保存 reproduction | [`repair/R0_revert_status_and_reproductions.md`](repair/R0_revert_status_and_reproductions.md) |
| R1 mesh dirty correctness | [`repair/R1_mesh_dirty_correctness.md`](repair/R1_mesh_dirty_correctness.md) |
| R2 save durability/ACK/atomic | [`repair/R2_save_durability_ack_atomic.md`](repair/R2_save_durability_ack_atomic.md) |
| R3 network catch-up/backpressure/order | [`repair/R3_network_catchup_backpressure_order.md`](repair/R3_network_catchup_backpressure_order.md) |
| R4 multiplayer authority/pause | [`repair/R4_multiplayer_authority_pause.md`](repair/R4_multiplayer_authority_pause.md) |
| R5 instrumentation + network budget + sim | [`repair/R5_instrumentation_network_budget_sim.md`](repair/R5_instrumentation_network_budget_sim.md) |
| R6 arena/packed AO/section meshing/culling | [`repair/R6_arena_packed_ao_section_meshing_culling.md`](repair/R6_arena_packed_ao_section_meshing_culling.md) |
| R7 render scratch/hand/instancing/paletted | [`repair/R7_render_scratch_hand_instancing_paletted.md`](repair/R7_render_scratch_hand_instancing_paletted.md) |
| R8 FPS cap / dynamic resolution（可選） | [`repair/R8_fps_cap_dynamic_resolution.md`](repair/R8_fps_cap_dynamic_resolution.md) |
| R9 fixed-scene + PGO + artifact gate | [`repair/R9_fixed_scene_pgo_artifact_gate.md`](repair/R9_fixed_scene_pgo_artifact_gate.md) |

每一階段合併前必須：

1. `cargo fmt --all -- --check`
2. `cargo test --all-targets`
3. `cargo test --all-targets --release`
4. `cargo build --release`
5. `cargo clippy --all-targets --all-features`（先安裝與 toolchain 相符的 component）
6. 該階段的 fault-injection、headless integration 或 GPU golden tests

## 6. 完成定義

只有同時符合下列條件，01–14 才可重新標為 Complete：

- P0/P1 問題全部修復並有防回歸測試。
- release 與 debug 全套測試穩定通過；`cargo fmt --check` 與 clippy 通過。
- 實際 GPU/window、host/client、slow-client、I/O fault-injection 場景完成。
- 存檔與 multiplayer checksum 無分歧。
- shader/CPU packed data與視覺 golden parity 通過。
- 8 個固定場景具可重播 before/after artifacts。
- 所有數值改善宣稱能從 raw artifact重算；沒有證據的百分比不得勾選。
- `performance_track.md`、01–14 plans、總計畫與 `ARCHITECTURE.md` 狀態一致。

### 目前缺口清單

| 目前缺口 | 對應審核結論 | 負責輪次 | 關閉證據 |
|---|---|---|---|
| mesh revision 已變更但 scheduler 未必 enqueue | 任務 02 Fail | R1 | mutation → queue → worker → visible revision 整合測試 |
| save 無 bounded ownership/revision ACK，Windows replace 非 atomic | 任務 03 Fail | R2 | fault-injection、crash/restart 與 highest-revision persistence 測試 |
| catch-up mailbox full 可 silent drop，排序及跨 channel revision 未定義 | 任務 04 Fail | R3 | capacity=1、slow/multi-client 與 unloaded Chunk checksum |
| joining client 仍自行模擬 living entities；host pause/death policy 錯誤 | multiplayer release blocker、任務 05 Partial | R4 | host/client entity/world/health checksum 與 pause/death 場景 |
| GPU timestamp/readback、queue telemetry、bounded drain、完整 fixed-tick/redstone/entity harness 缺失 | 任務 01/05/06/07 Fail 或 Partial | R5 | supported/unsupported timestamp tests、burst budget、30/60/144/240 FPS checksum |
| arena lifecycle/handle safety、AO decode、section ownership、conservative culling 未完成 | 任務 10/11/13 Fail | R6 | allocator property tests、CPU/WGSL golden、section revision/culling tests |
| render scratch/hand/ring completion/paletted demotion 未完成 | 任務 08 Fail、09/12 Partial | R7 | allocation instrumentation、GPU completion、representation/microbench tests |
| FPS cap 與真正 dynamic resolution 缺失或畫面錯誤 | 任務 14 Fail | R8 | frame-deadline test與 scaled target/upscale 視覺驗證 |
| 缺實際 GPU/window、host/client、fault-injection、固定場景 before/after、PGO A/B raw artifacts | 全部任務的 artifact gate | R9 | 8 場景可重播報告、checksums、raw output 與 non-PGO/PGO A/B |

R0 保存的現行失敗 reproduction 索引見
[`repro/README.md`](repro/README.md)；在上述關閉證據齊全前，01–14 維持
`Partial`。
