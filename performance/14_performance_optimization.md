# 實作計畫 14：全棧性能優化

> 狀態：待實作  
> 優先級：P0 / P1  
> 原則：先量測、再消除卡頓尖峰，最後才做高複雜度渲染與資料結構改造。  
> 相容性：不得改變既有遊戲規則、存檔語義或多人權威模型；可選的畫質降級功能必須預設關閉。

## 目標

在保留目前視覺效果與遊戲行為的前提下，盡可能降低：

- 主線程 p95 / p99 frame time 與 micro-stutter。
- Chunk streaming、lighting、mesh upload、自動存檔與多人加入造成的長幀。
- 穩態渲染的 CPU 配置、幾何重建、GPU upload 與 draw-call 成本。
- 視距 16 時 Chunk、mesh、region cache 與實體的 CPU / GPU 記憶體。
- 紅石、AI、物理、隨機 tick 和實體查找的無效工作。

目前已完成的 Greedy Meshing、視錐剔除、距離排序、Rayon 背景 mesh 生成與
三級 LOD 保留，不重複實作。本計畫建立在現有渲染優化之上。

## 外部參考與移植原則

本計畫參考下列 Minecraft 優化 mod，但只移植可泛化的設計原則，不直接複製
Java/Minecraft 實作：

| 參考 | 公開方向 | iCraft 對應 |
|---|---|---|
| [Sodium](https://modrinth.com/mod/sodium) / [source](https://github.com/CaffeineMC/sodium) | 重寫高效 chunk renderer，提升 FPS 並減少 micro-stutter | Region GPU arena、緊湊頂點、批次/間接繪製、優先級 worker、低配置 render preparation |
| [EntityCulling](https://modrinth.com/mod/entityculling) / [source](https://github.com/tr7zw/EntityCulling) | 非同步可見性判定，跳過被 terrain/structure 遮擋的實體渲染 | 保守的 section occlusion graph、非同步 entity LOS、白名單與 fail-open |
| [FerriteCore](https://modrinth.com/mod/ferrite-core) / [technical summary](https://github.com/malte0811/FerriteCore/blob/main/summary.md) | 壓縮、特化與去重長期駐留資料 | Empty/Uniform/Paletted section、nibble light、optional state/fluid、共用 immutable 資料 |
| [Lithium](https://modrinth.com/mod/lithium) / [optimization list](https://github.com/CaffeineMC/lithium/blob/develop/lithium-fabric-mixin-config.md) | AI、物理、block tick、集合與事件驅動優化，保持遊戲機制 | 固定 tick、sleep/wake、dirty worklist、section metadata、entity spatial/type index |

## 已確認的基線風險

以下是目前程式碼中可直接確認的高收益熱點：

1. `State::update_chunks` 每幀重新建立及排序完整載入範圍。視距 16 的方形範圍
   是 33×33，共 1,089 個 Chunk。
2. 火把煙霧每 0.4 秒遍歷所有載入 Chunk，對每個 Chunk 掃描
   `16×16×128` 個位置；視距 16 最多約 3,570 萬次 voxel 檢查。
3. `trigger_background_save` 在主線程將所有 Chunk 的五組 64 KiB voxel
   陣列轉換並壓縮後才送入 save worker。視距 16 單次需處理約 357 MB 原始
   資料。
4. `SaveManager::save_chunk_in` 每存一個 Chunk 都重新序列化並覆寫整個 region。
   全量 autosave 會對同一 region 進行大量重複寫入。
5. `RedstoneSystem::settle_power` 在每個 20 Hz tick 最多執行 64 輪，
   每輪 clone 完整 component `HashMap`，即使沒有實際紅石變更也會進入 settle。
6. `State::render` 每幀重新建立 terrain candidates、LOD map、mob mesh、
   particle mesh、hand mesh 和多組 UI Vec。
7. 每個 Chunk 的三個 LOD、opaque/translucent 兩層各持有獨立 vertex/index
   buffer，最多是 12 個 GPU buffer；視距 16 理論上超過 13,000 個 buffer
   object。
8. `TerrainVertex` 為 36 bytes；大量欄位可以用 chunk/region 相對座標與整數
   packed 格式表示。
9. `Chunk` 為每個 voxel 固定保存 block、block state、sky light、block
   light 和 fluid level，共約 320 KiB；1,089 個 Chunk 僅原始 voxel storage
   約 340 MiB。
10. Mob、passive mob、dropped item、remote player 和 rendering 共用單一
    `Vec<Entity>`，存在多次全表遍歷、按 ID 線性 `.find()` 及按類型過濾。

## 全局驗收指標

Phase 0 完成後記錄正式硬件基線；以下是目標而非未量測的保證：

- 視距 16、1080p、固定 seed 測試場景達到穩定 60+ FPS。
- 主要場景記錄 CPU/GPU frame time 的 p50、p95、p99 和 1% low。
- 自動存檔、快速飛行 streaming 與多人加入期間，不出現超過 33 ms 的
  主線程工作尖峰；單幀背景整合工作預算目標為 2–4 ms。
- 穩態 gameplay render 接近零 heap allocation。
- GPU buffer object 數相對目前設計降低至少 90%。
- 視距 16 的 Chunk CPU 記憶體降低至少 40%，以實測 working set 與分項
  counter 驗證。
- 實體壓力場景的 CPU mesh build / upload 時間降低至少 70%。
- 所有優化通過 `cargo test --release`、`cargo check --release`、固定世界
  checksum、渲染 golden screenshots、舊存檔與 multiplayer 回歸。

## Phase 0：可觀測性與可重現基線

### 0.1 CPU frame scopes

新增 `src/perf.rs`，以低開銷 RAII scope 或明確 timestamp 記錄：

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

使用固定容量 ring buffer 保存最近樣本，F3 顯示最近窗口的平均、p95 和 p99；
不得在量測熱路徑建立 `String` 或配置集合。

### 0.2 GPU timestamp 與資源 counter

若 adapter 支援 timestamp queries，記錄：

- sky
- opaque terrain
- entity/mob
- translucent terrain
- particles
- crack overlay
- UI

不支援時自動停用，不影響遊戲啟動。另加入：

- CPU/GPU mesh bytes
- GPU buffer object count
- loaded / visible / occluded chunks
- terrain candidates、triangles、draw calls
- rendered / frustum-culled / occlusion-culled entities
- worker queue、in-flight、cancelled、stale results
- upload bytes/frame
- save/network queue depth
- loaded region cache bytes

### 0.3 固定場景

建立可人工重播或 headless 執行的固定 seed 場景：

1. 開放地形，視距 8/16，靜止及快速旋轉。
2. 洞穴或建築內，大量 Chunk 在視錐內但被遮擋。
3. 以固定速度直線/對角飛行，持續載入和卸載 Chunk。
4. 高密度紅石線路、repeaters、pistons 和 scheduled ticks。
5. 大量流體、爆炸及 lighting 變更。
6. 1,000 dropped items / mobs，其中大部分在牆後。
7. 五分鐘 autosave 與連續 dirty-chunk save。
8. Host + client 加入，包含大量 mutated chunks。

每次性能改動保存 before/after 報告；沒有改善 p95/p99 或記憶體的高複雜度
改動不得合併。

## Phase 1：P0 卡頓尖峰

### 1.1 火把與特殊方塊索引

涉及：

- `src/world.rs`
- `src/chunk_manager.rs`
- `src/state.rs`
- `src/dimension.rs`

步驟：

1. 為每個 Chunk/section 保存 torch positions 或特殊方塊類型索引。
2. World generation/load worker 在建立 Chunk 時同時產生索引。
3. `ChunkManager::set_block` 或統一 mutation pipeline 增量更新索引。
4. 火把煙霧只遍歷索引，並依相機距離及 particle budget 採樣。
5. 後續 leaf/random tick、redstone component discovery 共用 section metadata。

驗收：

- 火把效果位置及密度與目前一致。
- 視距 16 的 torch update 不再掃描完整 voxel arrays。

### 1.2 增量 Chunk streaming queue

涉及：

- `src/state.rs`
- `src/chunk_manager.rs`
- `src/chunk_render.rs`

步驟：

1. 依 render distance 預計算穩定的近到遠螺旋 offset。
2. 只有玩家跨越 Chunk、切換維度或更改視距時更新 load/unload target。
3. load queue、dirty mesh queue 使用可去重 priority queue。
4. dirty mutation 直接 push affected Chunk/section，不再每幀掃描全部 mesh。
5. 加入一圈保留 hysteresis，降低邊界往返造成的 unload/reload thrash；
   實際圈數由記憶體基線決定。
6. 每幀按 CPU 時間、結果數與 upload bytes 三重預算整合 worker result。
7. result 保持 generation/lifetime/revision 驗證；取消或丟棄過期工作。

驗收：

- 玩家停在同一 Chunk 時，streaming scheduler 不建立 O(render_distance²)
  的臨時 Vec 或排序。
- 快速飛行時近距離 Chunk 優先完成，無單幀集中 upload。

### 1.3 真正的背景存檔

涉及：

- `src/save.rs`
- `src/state.rs`
- `src/menu.rs`

步驟：

1. 建立 `DirtyChunkSet`，所有權威 block/state/light/fluid/redstone mutation
   統一標記。
2. autosave 只 snapshot dirty chunks；完整 flush 只在明確 Save and Quit。
3. 主線程 snapshot 保持最小、無壓縮；flatten、Bincode、Zlib 和 region I/O
   全部移入專用 save worker。
4. `SaveCommand` 改為可合併、bounded queue；同一 Chunk 僅保留最新 revision。
5. worker 按 region 分組，一個 batch 只序列化和寫入 region 一次。
6. 使用同目錄 temporary file + flush + atomic rename，避免中途中斷毀損存檔。
7. region cache 加入 byte/entry 上限與 LRU eviction。
8. 關閉遊戲時等待 queued revisions flush，並顯示現有 saving overlay。

驗收：

- autosave 不在主線程壓縮 Chunk。
- 多個同 region Chunk 的單次 autosave 只產生一次 region rewrite。
- 快速修改同一 Chunk 時舊 revision 不覆蓋新 revision。
- 現有存檔格式保持可讀；若版本化格式，必須具備向後讀取。

### 1.4 多人 catch-up streaming

涉及：

- `src/state.rs`
- `src/network/client.rs`
- `src/network/server.rs`
- `src/network/protocol.rs`

步驟：

1. `send_mutated_chunks_to` 不在 PlayerJoin callback 同步壓縮全部 Chunk。
2. 背景建立 payload，按玩家距離、Chunk revision 排序並分幀發送。
3. 同一 Chunk payload latest-wins；可靠 block/chat/control queue 保留順序。
4. `drain_network_events` 加入時間/事件數預算；pose/time 可 coalesce，
   authoritative block result 不可丟失。
5. 大型 payload 使用 bounded backpressure，避免主線程和 Tokio thread
   同時累積無上限 Vec。

## Phase 2：Lithium 路線——固定 Tick、事件驅動與空間索引

### 2.1 分離 frame update 與 simulation tick

涉及：

- `src/app.rs`
- `src/state.rs`
- `src/physics.rs`
- `src/mob.rs`
- `src/passive_mob.rs`
- `src/redstone.rs`
- `src/fluid.rs`
- `src/weather.rs`

步驟：

1. `App` 保留真實 frame `dt`，`State` 維護 simulation accumulator。
2. 權威 world simulation 固定 20 Hz，最多四個 catch-up tick，超出時保留
   有界 debt 而非無限追趕。
3. Player physics 使用固定 60 Hz 或經基準選定的 substep；render/camera 使用
   previous/current snapshot 插值。
4. AI、spawning、leaf random tick、redstone、fluid、weather accumulation 改為
   tick/秒語義，不再依 FPS 執行次數。
5. particles、camera、remote interpolation 等純呈現工作保留 frame update。
6. pause/death/network-not-ready 的 tick policy 明確化並加入測試。

驗收：

- 30/60/144/240 FPS 下，同一輸入與 seed 的 world checksum 一致。
- leaf decay、mob attack、spawning、redstone 和流體速度不隨 FPS 改變。

### 2.2 紅石 dirty worklist 與 sleeping

涉及：

- `src/redstone.rs`
- `src/state.rs`
- `src/chunk_manager.rs`

步驟：

1. block mutation、scheduled tick、pressure-plate occupant change 將受影響節點
   加入去重 worklist。
2. 只重新計算 changed node 及其鄰接元件，不 clone 完整 component map。
3. 無 dirty node、無 scheduled tick、無 active fuse/device 時進入 sleep。
4. Chunk load worker 直接回傳 component metadata/index，避免首次 20 Hz tick
   掃描完整 Chunk。
5. 保留 loop/overflow protection；計數改為每次事件的 node budget。
6. 加入大型線路與跨 Chunk 邊界 propagation parity tests。

驗收：

- 靜止紅石世界的 `settle_power` 工作量接近零。
- 現有紅石單元測試及新舊實作 differential tests 結果一致。

### 2.3 Entity ID/type/spatial index

涉及：

- `src/entity.rs`
- `src/mob.rs`
- `src/passive_mob.rs`
- `src/boss.rs`
- `src/state.rs`
- `src/mob_renderer.rs`

步驟：

1. 增加 `EntityId -> dense index`，以 `swap_remove` 後更新 index 的方式保留
   cache-friendly dense storage。
2. 增加按 EntityType 與 Chunk/section 的 bucket，位置跨區時增量更新。
3. nearby collision、pickup、melee、projectile、spawning 和 rendering
   先查相關 bucket，不遍歷全部 Entity。
4. 使用 distance-squared 取代不需要真實距離的 `sqrt`。
5. 重複使用事件 scratch Vec；移除每 frame 的短命 allocations。
6. 只有性能基線顯示 `Entity` AoS 大小是問題時，才將 dropped item、
   projectile、passive/hostile payload 拆成 enum/component side storage。

驗收：

- Entity spawn/remove/restore/remote leave 後 ID index 永遠有效。
- 壓力場景查找複雜度隨附近實體數，而不是全世界實體數增長。

## Phase 3：Sodium 路線——渲染資料與提交架構

### 3.1 穩態零配置 render preparation

涉及：

- `src/state.rs`
- `src/chunk_render.rs`
- `src/particles.rs`
- `src/hand_renderer.rs`
- `src/mob_renderer.rs`
- `src/menu.rs`

步驟：

1. 將 terrain candidates、draw plan、selected LOD、mob/particle/UI scratch
   storage 變成 `State` 持久欄位，逐幀 `clear()` 重用 capacity。
2. `DrawCandidate` 直接攜帶 LOD 和預先計算 distance key，移除
   `selected_lods HashMap` 及 sort comparator 的重複距離計算。
3. UI 文本 uppercase、debug labels 和靜態 UI geometry 按 dirty flag cache。
4. first-person hand 只在 held item/model 改變時重建基礎 mesh；動畫改為
   transform/uniform。
5. 加入 allocation counter，穩態 render 出現配置即在 F3 標記。

### 3.2 Entity、item 與 particle instancing

步驟：

1. 預建每種 entity type 的 cuboid/quad model。
2. 每個可見 Entity 只產生 instance data：position、rotation、animation、
   atlas/material、lighting、burn state。
3. 按 model/material 分組，一次上傳 instance buffer 並批次 draw。
4. dropped block/item 重用 cube/flat sprite prototype。
5. particle 改為 GPU billboard：固定 unit quad/index buffer，只上傳
   position、size/stretch、age/lifetime、UV 和 color/light instance。
6. 動態 instance buffer 使用 ring/staging strategy，避免 CPU/GPU overwrite
   hazard。

驗收：

- 實體幾何不再每 frame 在 CPU 重建。
- 4,096 particles 不再上傳 4 vertices + 6 indices/particle。
- 動畫、name tag、burning、第三人稱和 dropped item 視覺保持一致。

### 3.3 Region GPU arena

涉及：

- `src/state.rs`
- `src/chunk_render.rs`
- `src/world.rs`

步驟：

1. 以 8×8 或經基準選定的 Chunk 區域建立 `RenderRegion`。
2. 每個 region 使用少量共享 vertex/index arena；Chunk/LOD/layer 只保存
   allocation handle、offset、count 和 bounds。
3. 以 free-list/buddy allocator 管理 suballocation；更新 mesh 先配置新範圍，
   upload 成功後再釋放舊範圍。
4. 空 mesh 不配置 4-byte placeholder buffer。
5. fragmentation 超過門檻時低優先級 compact；不得在 gameplay frame
   同步重建整個 region。
6. 記錄 committed/used/wasted bytes 及 allocation count。

驗收：

- 視距 16 的 GPU buffer object 數降低至少 90%。
- 重複改方塊不造成 arena 泄漏或使用已釋放 range。
- 切維度及 unload 正確回收所有 allocations。

### 3.4 緊湊 TerrainVertex

目前 `TerrainVertex` 為：

- `position: [f32; 3]`
- `local_uv: [f32; 2]`
- `atlas_tile: [f32; 2]`
- `light_level: f32`
- `ao: f32`

共 36 bytes。目標格式約 16–20 bytes：

- region-relative fixed-point/u16 position
- packed local UV
- integer atlas tile
- packed sky/block light
- packed AO/face flags

Shader 以 region origin 重建 world position。必須驗證：

- Chunk/region 邊界沒有裂縫。
- 大座標沒有明顯精度退化。
- Greedy UV repeat、AO triangulation、fluid、snow、door/trapdoor 與 cross-model
  結果一致。
- 若緊湊格式無法安全表示特殊模型，提供 full-precision fallback stream。

### 3.5 Section 級增量 meshing

步驟：

1. 將 Chunk 垂直切為 16³ sections。
2. block/light mutation 只 dirty 所在 section 與需要 halo 的鄰接 section。
3. MeshSnapshot 只保存一次 packed 18³ halo，不再 clone 完整 Chunk 並重複
   保存中心 voxel。
4. worker request 以 section revision、Chunk lifetime、dimension generation
   做失效判定。
5. 專用 bounded worker pool 保留一個 CPU core 給主線程；近距離、視錐內、
   無可用 mesh 的工作優先。
6. 送入昂貴 generator 前再次檢查 cancellation token。
7. L0/L1/L2 可分開 residency/build priority；遠距先生成 L2/L1，接近前預取 L0。

驗收：

- 單格 block mutation 不再重建完整 16×256×16 Chunk mesh。
- 跨 section/Chunk AO、lighting、fluid 邊界正確。
- 快速連續修改同一位置時，只上傳最新 revision。

### 3.6 Multi-draw indirect

1. adapter 支援 `MULTI_DRAW_INDIRECT` 時，region/layer 建立 indirect command
   buffer，一次提交多個 draw。
2. opaque 保持 front-to-back；translucent 保持 back-to-front。
3. 不支援時使用同一 arena 的普通 `draw_indexed` loop，功能與畫面一致。
4. 只有 CPU render submission 確認為瓶頸時才啟用，避免增加不必要複雜度。

## Phase 4：FerriteCore 路線——Chunk 與長期記憶體

### 4.1 ChunkSection storage abstraction

涉及：

- `src/world.rs`
- `src/chunk_manager.rs`
- `src/save.rs`
- `src/lighting.rs`
- `src/fluid.rs`
- `src/redstone.rs`
- `src/dimension.rs`

先建立 access abstraction，再替換 storage；外部系統不得直接索引固定
`chunk.blocks[x][y][z]`。

每個 16³ section 的 block storage 支援：

- `Empty`
- `Uniform(BlockType)`
- `Paletted { palette, packed_indices }`
- `Global` fallback

palette bits 依實際種類數選擇，避免為簡單 stone/air section 固定支付 4 KiB。

### 4.2 Light、state 與 fluid packing

1. sky light 和 block light 各為 0–15，合併進同一 byte 的兩個 nibble。
2. 全零/全 15 light section 使用 uniform representation。
3. block state 與 fluid level 全零時不配置 storage；首次非零 mutation
   才建立 optional packed array。
4. section 保存 non-air、opaque、random-tick、fluid、emitter、redstone
   component counts，支援 O(1) early-out。
5. heightmap 保持現有語義，並以 mutation 增量更新。

### 4.3 相容與性能保護

1. save/network wire format初期保持不變，在 serialization 邊界 flatten。
2. 對現有世界做 load -> save -> load checksum。
3. microbenchmark `get_block`、`set_block`、lighting、physics collision 和 meshing；
   palette 不能以明顯 CPU regression 換取未使用的記憶體節省。
4. 熱 Chunk/section 可在必要時使用較寬但更快的 representation；冷資料偏向壓縮。
5. internal integer-coordinate maps A/B 測試 fast hasher 或 dense/toroidal
   chunk grid，只有實測改善才引入依賴。

## Phase 5：EntityCulling 路線——保守遮擋剔除

### 5.1 便宜剔除階層

所有 entity submission 依序執行：

1. render distance
2. entity AABB frustum
3. chunk/section visibility
4. optional asynchronous LOS

只有通過前一階段才執行下一階段。

### 5.2 Section occlusion graph

涉及：

- `src/chunk_render.rs`
- `src/world.rs`
- `src/state.rs`

1. meshing worker 對每個 section 的透明/可通行 voxel 做 flood fill。
2. 建立六個 section faces 的 pairwise connectivity bitmask。
3. 從 camera section 做 bounded graph traversal，只訪問能經可見 face 到達的
   section。
4. 完整 opaque block 才能作可靠 occluder；leaves、glass、fluid、cutout、
   translucent 和未載入資料保守視為可見。
5. camera teleport、進入未完成 section 或 stale graph 時 fail-open。

此結果可同時剔除 terrain section 與其中 entities，但不得影響 world tick。

### 5.3 非同步 Entity LOS

1. 對仍可能可見而且 mesh 成本較高的 entity，從 camera 到 AABB center/corners
   做 bounded voxel LOS。
2. 使用獨立低優先級 queue，不能與 terrain meshing 爭奪全部 Rayon threads。
3. 結果保存 world/chunk revision、camera cell、TTL 和 last-visible hysteresis。
4. stale、超時或 queue overflow 時一律 render。
5. 下列類型預設 bypass/白名單：
   - camera 附近 entities
   - projectiles
   - bosses
   - remote players在近距離
   - model 超出標準 AABB
   - lightning/critical effects
6. 只跳過渲染和純 client visual animation；權威 AI、physics、pickup、damage、
   drops、network state 一律繼續。

驗收：

- 在牆後 1,000 entities 場景，render submission 和 upload 顯著下降。
- 快速轉身、開門、拆牆時沒有明顯 pop-in。
- 所有不確定狀態均 fail-open，不會出現實體永久消失。

## Phase 6：網路、編譯與可選 GPU-bound 策略

### 6.1 Network serialization

- pose 已有 latest-wins，保留。
- block changes 依 Chunk/position coalesce，但 ACK/chat/order-sensitive packet
  不合併。
- catch-up payload 背景壓縮並重用 save-compatible raw snapshot。
- 記錄 bytes/sec、queue delay、payload count 和 main-thread integrate time。
- 只有基準證明值得時才引入新版 chunk delta/palette wire format；必須 bump
  protocol version。

### 6.2 Release build

在 `Cargo.toml` 加入經測試的 release profile：

```toml
[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
```

`panic = "abort"`、`strip`、`target-cpu=native` 不直接作為通用預設：

- `target-cpu=native` 僅供本機 benchmark/distribution-specific build。
- `panic = "abort"` 先確認錯誤處理與 crash diagnostics 可接受。
- 完成固定 workload 後再做 PGO，並以相同場景 A/B 驗證。
- alternate allocator 只有在移除熱路徑 allocations 後再測試。

### 6.3 Frame pacing 與可選畫質策略

- VSync off 時優先 `Mailbox`，不支援才用 `Immediate`。
- 加入獨立 FPS cap，simulation tick 不受 cap 影響。
- GPU-bound 時提供可選 render scale/dynamic resolution；UI 保持原生解析度。
- dynamic resolution、entity distance scaling 等可能影響畫質的選項預設關閉。
- Windows 繼續使用目前已驗證的 DX12 backend；不得未經 NVIDIA driver
  回歸測試改回 `PRIMARY`。

## 測試矩陣

### 自動測試

- Fixed-tick determinism at 30/60/144/240 render FPS。
- Streaming spiral/priority/hysteresis。
- Save revision coalescing、region batch、atomic replacement、legacy load。
- Section palette roundtrip、uniform transition、nibble light。
- Dirty section/halo mesh invalidation。
- Region arena allocate/free/compact/stale handle。
- Packed vertex encode/decode boundary and precision。
- Redstone event-driven parity and loop bounds。
- Entity spatial index spawn/move/remove/restore。
- Section visibility connectivity。
- Entity culling TTL/hysteresis/fail-open/whitelist。
- Network event budget and reliable packet preservation。

### 視覺測試

- Greedy UV repeat、AO、lighting。
- Water/ice/translucent sorting。
- Door、trapdoor、thin snow、cross model、torch。
- LOD transition及 skirts。
- Entity animation、remote player、burning、projectiles、dropped items。
- Particle billboard、rain/snow/lightning。
- 快速轉身及拆牆後 occlusion 恢復。

### 驗證命令

```text
cargo fmt -- --check
cargo test --release
cargo check --release
cargo run --release
```

另外執行 Phase 0 固定場景，保存 before/after CPU、GPU、記憶體與 queue 報告。

## 實作與提交順序

每一階段必須可獨立 benchmark、review 和回退，不做單一巨型重構。

1. `perf: add frame, subsystem, gpu and memory instrumentation`
2. `perf(world): index torches and remove periodic voxel scans`
3. `perf(streaming): add incremental prioritized chunk queues and budgets`
4. `perf(save): move chunk compression off-thread and batch region writes`
5. `perf(network): stream join catch-up with bounded backpressure`
6. `perf(sim): split frame updates from fixed world and physics ticks`
7. `perf(redstone): replace full-map settling with dirty propagation`
8. `perf(entity): add id, type and spatial indexes`
9. `perf(render): reuse frame scratch buffers and cache static ui/hand data`
10. `perf(render): instance mobs, items and particles`
11. `perf(render): move chunk meshes into regional gpu arenas`
12. `perf(render): pack terrain vertices and add section remeshing`
13. `perf(memory): introduce paletted chunk sections and packed lighting`
14. `perf(culling): add section visibility and conservative entity occlusion`
15. `perf(build): enable measured release and pgo optimizations`

## 停止條件

下列情況不得繼續擴大優化：

- 基線顯示該系統不是瓶頸。
- 改動提高平均 FPS但惡化 p95/p99 或 streaming stutter。
- 記憶體節省導致熱路徑 CPU 明顯退化。
- 行為、存檔、多人權威或視覺 parity 無法可靠驗證。
- adapter-specific 快路徑沒有跨硬件 fallback。

最終交付應以「量測結果與回歸證據」判斷，而不是以完成優化清單判斷。
