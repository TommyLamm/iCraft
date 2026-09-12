# iCraft 第五波簡化路線 — 無視風險版 (Simplify Follow-up Wave 10)

> 來源：2026-09-11 全專案 `/simplify` 唯讀掃描（八路 Grok 4.6 子代理並行：桌面 State／渲染 UI／權威 runtime／網路存檔／世界生成／玩法域／測試與跨模組重用／tick 與 frame 熱路徑）。
> 代碼基線：`tommy-dev` @ `4432cf1`（Wave 08 全部落地；**Wave 09 十六份計劃尚未開始**）。
> 核心目標：這一波 **明確無視風險**。Wave 06–09 在「明確不在本路線」裡刻意擋下的大刀（三層事件 enum 合併、雙 session 記錄、雙 response cache、SimHarness 整包、tick 執行緒 I/O、dense chunk grid、renderer 資料表化）全部納入。

與 Wave 09 的關係：

- Wave 09 的 16 份計劃 **不重複列入**。本波多份計劃的「前置」指向 09 的編號；建議先做完 09 的 01／02／07／08／13／14 再開本波對應項。
- 本波 09 號（原 v20 附帶項）原須與 09 的 01 **同一個 handshake bump**；09 波 01 已獨立落地成 v20，故本項改走 **v21**（見該計劃檔註記）。
- 09 §5 列的排除項，本波只保留四條硬約束（見 §5）。

---

## 1. 怎麼用

一次只執行一份編號計劃。把 [prompt.md](prompt.md) 裡的 `{{PLAN_FILE}}` 換成該檔路徑。
完成並留下窄測試證據後，另開任務再做下一份。

- **P0 刪死殼與死 API**（01–06）：彼此檔案重疊小，可並行。02／03 都動 `state.rs`，不要同一 PR。
- **P1 契約塌縮**（07–11）：07 與 08 是同一件事的 server 半邊與 join 半邊，可並行；09 綁 09 波的 01；10 建議在 07 之後；11 可獨立。
- **P1 熱路徑**（12–18）：12 改執行緒模型，單獨 PR；13／14 可並行；15 建議在 07 之後；16／17 可並行；18 只動桌面。
- **P2 資料表與去重**（19–26）：19 → 20 必須串行；21／22／23／24／25 彼此獨立；26 是本波 blast radius 最大的一份，放最後。
- **P2 結構與測試**（27–28）：27 建議在其他計劃把死碼刪完後再切檔；28 建議在 09 波的 02／16 之後。

---

## 2. 本波掃描確認的問題

1. **測試專用世界仍編進每次 `cargo test`**：`sim_harness.rs` + `final_acceptance.rs` ≈ 2,090 行自帶第二套世界（自己的 `ChunkManager`／redstone／entities／trades）；`harness` feature 零使用者；桌面 `main.rs` 無條件編 `mod microbench`。
2. **桌面 `State` 仍掛著沒有權威的玩法殼**：`PoiManager`／`RaidManager`／`MapManager`／`MountManager`／`FishingManager`／`ContainerSessionManager`／`MinecartState` 只在 `State` 與 `sim_harness` 建構，`ServerWorld::tick` 從不進入；Q 鍵仍本地生成幽靈掉落；每 tick 仙人掌／岩漿／虛空 AABB 掃描在 join 端是 `let _ =`；`switch_dimension` 仍本地 worldgen；join 端本地跑 brew／effects／時間，F 鍵 60× 時間加速編進生產。
3. **同一則 server→client 訊息被型別化五次**：`Packet` → `HostToServer`（28 變體）／`RuntimePresentationEvent`（19 變體）→ `ClientToGame`（~30）→ `NetworkInbound`；`egress.rs` 515 行 1:1 match、`drain_inbound` 287 行 1:1 map、`project_runtime_presentation_event` 再一份。合計 ~1,500–2,000 行鏡像。
4. **雙 session／雙 cache／三重 gate**：`SessionContract` 與 `PlayerSessionState` 鏡像 `id`／`username`／`position`／`dimension`／`game_mode`；transport 與 authority 各一個 128 深 `VecDeque<GameplayResponse>`；bounds／sequence／revision 在 TCP ingress、`submit_request`、`validate_request` 各檢一次；`apply_block_action`／combat 每次 `.cloned()` 整個含 128 筆 cache 的 `SessionContract`。
5. **20 Hz tick 執行緒仍做同步 worldgen 與磁碟 I/O**：`drain_initial_chunk_projections` 每 tick 最多 16 欄同步 `ensure_chunk`（噪聲＋結構）＋ 98k voxel flatten；eviction／autosave 在 tick 內 zlib＋region 重寫（cache 整份 `clone()`）；`save_all` 每次重寫所有玩家＋全部實體。
6. **09 未涵蓋的 20 Hz 全表掃**：漏斗 tick 走每個 simulation 欄的全部 block entity（熔爐已有索引，漏斗沒有）；`ServerWorld::tick` 每 tick 重建 simulation union（`BTreeSet` ~3–4k insert／玩家）；`residency_keep_set` 每 tick 重建；紅石 sleep 前仍 O(resident) `sync_loaded_chunks`；awake 紅石每 tick 全 components 兩次 collect+sort；`next_unique_entity_id` 每個 id 掃全部維度實體；dispense 後 `rebuild_indexes()`。
7. **世界表示與生成走「dense 再 palette」**：`Chunk::new_with_seed` 先配 `Vec<Vec<[BlockType;16]>>`×384 高＋兩份光照，再拷進 24 個 section；Superflat 先跑完整 Overworld 再覆寫；Nether 自帶第五份光照 BFS；embedded 單機把 paletted → dense → paletted → 全欄 lighting 走一遍才進 presentation。
8. **手寫巨表**：`BlockType::properties()` 845 行 121 臂 match（lighting／mesh 每 voxel 重建 struct）；`catalog.rs` 六份平行 per-item match（~1,200–1,600 行）；`recipes.rs` 85 個 `add_shaped`（三種木材複製貼上）；`mob_renderer.rs` 1,800 行逐型別 cuboid 腳本；`texture.rs` 先程序化畫滿整張 atlas 再被 `PACK_TILES` 覆蓋；`menu.rs` 九個畫面各自 rect／click／draw／focus 四份表。
9. **13 對 powered／open `BlockType` 與 `BlockState.is_open` 雙軌**：`OakDoorOpen` 同時是型別又是 state bit；redstone 寫型別、mesh 讀 state。
10. **跨模組重複 helper**：`session_slot_from_stack` ×3；六鄰居表在 lighting 內聯七次；兩份 `ray_intersects_aabb`（entity 版除以零）；`div_euclid(16)` 在 `server_world.rs` 22 處無 helper；FNV-1a ×4、SplitMix64 ×2、LCG ×3；`parse_bool` ×4；hostile／passive 生成同骨架。

---

## 3. 優先級劃分

| 級別 | 意義 | 計劃編號 |
| :--- | :--- | :--- |
| **P0** | 刪測試世界、桌面玩法殼、leftover 本地模擬、死 API、lib 圍籬 | 01 – 06 |
| **P1** | 塌縮三層事件 enum／雙 session／雙 cache／`active_dimension` | 07 – 11 |
| **P1** | tick 執行緒卸載、tick 索引、embedded 零拷貝、worldgen／lighting／frame 熱路徑 | 12 – 18 |
| **P2** | 資料表化與跨模組去重、ChunkManager 拆分與 dense grid | 19 – 26 |
| **P2** | 機械切檔、測試瘦身 | 27 – 28 |

---

## 4. 執行包索引

| # | 單獨執行文件 | 優先級 | 預估淨收益 | 前置依賴 | 狀態 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 01 | [刪 `SimHarness`／`final_acceptance`／`harness` feature，gate 桌面 microbench](01_delete_sim_harness_and_harness_feature.md) | P0 | ~-2,400 行；每次 `cargo test --lib` 少編第二套世界 | 無 | 已完成 |
| 02 | [刪桌面 `State` 上無權威的玩法殼](02_presentation_gameplay_shells.md) | P0 | ~-1,800 行 lib+State；lib 契約少 `container_sessions` | 無 | 已完成 |
| 03 | [刪 `State` leftover 本地突變與 join 端本地模擬](03_state_leftover_mutation_and_join_local_sim.md) | P0 | ~-450 行；每 tick 少一次仙人掌 AABB；join 不再 split-brain | 09 波 03 | 已完成 |
| 04 | [權威／世界／玩法死 API 掃除](04_dead_api_authority_world_gameplay.md) | P0 | ~-900 行；修 `/sleep` 一次性閂鎖 bug | 09 波 02、14 | 已完成 |
| 05 | [渲染／UI／存檔／資源死 API 與手寫 ZIP](05_dead_api_render_save_resources_zip.md) | P0 | ~-800 行 + `unicode-segmentation` | 無 | 已完成 |
| 06 | [lib 圍籬：桌面模組移出、weather／advancements 收桌面](06_lib_fence_desktop_only_modules.md) | P0 | `icraft-server` 少編 ~3,000 行 UI／lang／wgpu layout | 無 | 已完成 |
| 07 | [一種投影事件：`HostToServer`＋`RuntimePresentationEvent` → `Packet`](07_one_projection_event_type.md) | P1 | ~-700–900 行鏡像 enum 與雙 closure | 09 波 01 | 已完成 |
| 08 | [`ClientToGame` 併入 `NetworkInbound`](08_client_to_game_into_network_inbound.md) | P1 | ~-450–800 行 1:1 map | 09 波 01 | 已完成 |
| 09 | [v20 附帶項：`ContainerAction` 進 enum、剝每封包 `protocol_version`](09_v20_container_action_and_strip_packet_version.md) | P1 | -40 struct 欄位、-113 行 accessor、關 wire gap | 09 波 01（已單獨 v20 → **改走 v21**） | 已完成（v21） |
| 10 | [單一 session 記錄、單一 response cache、單一 preflight](10_single_session_cache_preflight.md) | P1 | ~-400 行 sync／cache／gate；每請求少兩次 cache 掃 | 07、09 波 13 | 已完成 |
| 11 | [刪 `active_dimension`／`world()`／`world_mut_active()`，dispatch 帶 `Dimension`](11_thread_dimension_drop_active_dimension.md) | P1 | tick 少 3–4 次 `Vec` 配置與 restore；`current_revision` 不再看錯維度 | 無 | 已完成 |
| 12 | [tick 執行緒卸載：worldgen worker、非同步存檔、dirty 玩家／實體](12_offload_worldgen_and_save_from_tick.md) | P1 | 走路時 tick 少數十 ms；autosave 不再 50–500 ms 卡 | 無 | 已完成 |
| 13 | [世界 tick 索引：漏斗、紅石 generation、dispense、metadata、boss](13_world_tick_indexes_hopper_redstone_dispense.md) | P1 | 安靜世界 hopper／redstone tick 接近零；busy 紅石少兩次全表 sort | 09 波 08、09 | 已完成 |
| 14 | [runtime 每 tick 快取：union／keep-set、entity id、`SessionContract` clone、metrics](14_runtime_per_tick_caches_and_clones.md) | P1 | 靜止玩家每 tick 少 ~8k BTree insert 與 O(resident) 掃 | 09 波 07 | 已完成 |
| 15 | [embedded 零拷貝投影：`Arc<Chunk>`、突變只套一次](15_embedded_zero_copy_projection.md) | P1 | 單機每欄少 ~3×288 KiB 拷貝＋palette 重建＋全欄 lighting | 07 | 已完成 |
| 16 | [worldgen 直接填 paletted section、Superflat、Nether 光、flatten SoA](16_worldgen_paletted_fill_and_flatten.md) | P1 | 每欄少 3×98k 陣列＋24 section 拷貝；Superflat 不再跑完整 Overworld | 無 | 已完成 |
| 17 | [lighting BFS 去重、鄰域快取、載入只 seed 邊界](17_lighting_bfs_dedupe_and_neighborhood.md) | P1 | 放／破方塊光照少 2 次 HashMap／cell；載入少 1–8 ms | 無 | 已完成 |
| 18 | [frame 熱路徑：只走 visible set、上傳批次、字型快取、刪 entity LOS worker](18_frame_hot_path.md) | P1 | 每 frame 少 O(sections) AABB、~17 次 `write_buffer`、一條 LOS 執行緒 | 09 波 04 | 已完成 |
| 19 | [`BlockType` 靜態屬性表](19_block_type_static_property_table.md) | P2 | ~-1,200 行 match；lighting／mesh 每 voxel 不再 121 臂 | 無 | 已完成 |
| 20 | [13 對 powered／open `BlockType` 折進 `BlockState`](20_fold_powered_open_variants_into_block_state.md) | P2 | 少 13 discriminant × 6 張表的 match 臂 | 19 | 已完成 |
| 21 | [`catalog.rs`／`recipes.rs` 資料表化](21_item_catalog_and_recipes_tables.md) | P2 | ~-2,000 行 match 與 `add_shaped` | 無 | 未開始 |
| 22 | [渲染資料表：mob 部件、atlas paint-on-miss、單一 cube emitter、shader 殘留](22_render_data_tables_mob_texture_cube.md) | P2 | ~-2,500 行；啟動不再畫兩次 atlas | 無 | 未開始 |
| 23 | [`menu.rs` widget 表、共用 `GpuContext`、controls 單表](23_menu_widget_table_and_shared_gpu.md) | P2 | ~-1,500 行；menu↔game 不再重建 device | 無 | 未開始 |
| 24 | [跨模組 helper 合併：slot／鄰居／ray／座標／milli／RNG／parse_bool／spawn](24_cross_cutting_helpers.md) | P2 | ~-300 行；修 entity ray 除零 | 09 波 13 | 未開始 |
| 25 | [結構生成 helper 與雙 End City](25_structure_gen_helpers_and_end_city.md) | P2 | ~-450 行 box／chest／start 樣板；一套 End City 放置 | 無 | 未開始 |
| 26 | [`ChunkManager` 拆 presentation／authority，dense chunk grid](26_chunk_manager_split_and_dense_grid.md) | P2 | 權威 `set_block` 不再維護兩個無人 drain 的 mesh set；每 `get_block` 少 hash | 15、17 | 未開始 |
| 27 | [巨檔機械切分：server_world／server_runtime／dispatch／network／mesh／block／redstone／catalog／state](27_giant_file_splits.md) | P2 | 導航與編譯隔離；~9,000 行 inline 測試移出 | 01–06 | 未開始 |
| 28 | [測試瘦身：拓撲三重跑、leftover fixture、sleep、roundtrip 表、存檔 fixture](28_test_slimming.md) | P2 | ~-1,500 行測試；少 50–300 ms sleep padding | 09 波 02、16 | 未開始 |

---

## 5. 本波仍保留的硬約束（其餘 06–09 的排除項全部解除）

- `save/format.rs` 歷史存檔 `0..256` 遷移語意不得改成 signed-Y 重解讀（新格式可以加 `data_version` 雙讀）。
- 對抗性畸形封包測試必須維持手組 frame。
- 不合併 `AuthorityCore` 與 `ServerRuntime` 兩個 **型別**（掃描結論：收益在 session 記錄與 cache，不在擁有者；合併會把 `Instant`／`SaveManager` 拖進 checksum 核心）。
- `worldgen::hash_coord`／`structure::hash_structure` 的 mixer 常數不動（改了就換世界）；結構 helper 重構必須 byte-identical。

---

## 6. 掃描有、但不下單的項目

| 項目 | 為什麼不下單 |
| --- | --- |
| `tokio_util::codec` 取代 `ConnectionReader` | 只省 ~80 行，且 header／body 間 cancellation latch 有明確測試 |
| `thiserror` 取代九個手寫 `Display` | ~80–100 行；新依賴，等有第二個理由再加 |
| Rayon 平行整個 `AuthorityCore::tick` | 14 只做 entity physics 平行（sorted `moved_ids`）；redstone／fluid／random tick 共用同一張 map |
| `ChunkSaveData` 六串 zlib → 單 blob | 需要 region 格式 bump 與雙讀；先做 16 的 flatten SoA，格式另案 |
| 三套指令語言合併 | console 是無 session 的 admin 面；live 動詞除了 `save-all` 名字沒有重疊 |
| `worldgen` 三份 hash 合併 | 見 §5 |

---

## 7. 審查來源（唯讀，2026-09-11）

八路 Grok 4.6 子代理全部成功：桌面 State／presentation、渲染 UI 媒體、權威 runtime server_world、網路存檔資源、世界生成結構光照、玩法域、測試與跨模組重用、tick／frame 熱路徑。
父代理抽查：`sleeping_players` 只 insert 無讀取；`harness` feature 無 `--features` 使用者；`unicode-segmentation` 只在 `Cargo.toml`；列出的死 API 定義存在。
