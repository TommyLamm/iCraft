# iCraft 第三波簡化與熱路徑收益路線 (Simplify Follow-up)

> 來源：2026-09-09 全專案 `/simplify` 審查（品質／效能／重用三路並行，只看收益）。
> 代碼基線：`tommy-dev` @ `28ded83`（worldgen 欄採樣、漏斗空間撿物、heightmap 生成、封包 framing、卸載滯後 helper、signed-Y 光照還原已落地）。
> 核心目標：刪掉已不是執行路徑的 leftover、收斂重複 helper、砍掉 20 Hz / 進圈幀上的二次掃描與整檔 I/O。

已落地、不重複列入本路線：

- Overworld 每欄只採樣一次 `surface_height_at` / `biome_at`，洞穴只雕到地表。
- 漏斗撿物走 `query_radius_types`。
- `ChunkManager::highest_solid_y`；敵對／被動生成共用；拿掉 spawn `println!`。
- `Packet::encode_frame`；`within_unload_hysteresis`。
- `EntityScratch` 只留 `id_list`；刪永遠 `false` 的 `project_authority_container`。
- 區塊可見性 BFS 吃當前維度高度；天氣用 `dimension.height()`。
- 網路還原後 `recompute_direct_column_lighting`；天空光改看 `is_opaque()`。

---

## 1. 怎麼用

一次只執行一份編號計劃。把 [prompt.md](prompt.md) 裡的 `{{PLAN_FILE}}` 換成該檔路徑。
完成並留下窄測試證據後，另開任務再做下一份。

- **P0 死路徑刪除**（01–06）：檔案重疊少，01 會動 `state.rs` 大量 `cfg`，06 應在 01 之後。
- **P1 熱路徑**（07–11）：07／08 影響 tick 與 I/O，可並行但不要同一 PR 混協定與存檔。
- **P2 重用**（12–16）：12 可隨時做；15 建議在 02、06 之後。

---

## 2. 本路線聚焦解決的問題

1. **Leftover 仍決定編譯圖**：`legacy_sim` / `legacy_interaction` / `legacy_systems` 約 5600 行綁在 `cfg(test)`，每次 `cargo test` 都編。`BlockUse`、`wrap_legacy`、`NetworkHandle::Host`、presentation `SaveManager` 都不是 live 路徑。
2. **20 Hz 成本隨實體／欄數線性膨脹**：每 tick 對每個觀看者廣播每個實體；autosave 逐 chunk 重寫整個 region；checksum 掃完整 `block_revisions`。
3. **進圈幀 hitch**：載入光照全欄掃描；mesh 主執行緒 per-voxel HashMap halo；永遠打三層 LOD。
4. **重複政策**：`CHUNK_HEIGHT` / `0..256`、測試 `TcpClient`／`temp_world` 複本、inventory click 不走 `inventory_decision`、session pose／dimension 雙寫。

---

## 3. 優先級劃分

| 級別 | 意義 | 計劃編號 |
| :--- | :--- | :--- |
| **P0** | 刪 leftover／永遠失敗的協定信封／熱路徑 debug IO | 01 – 06 |
| **P1** | tick／存檔／光照／mesh 高槓桿 | 07 – 11 |
| **P2** | helper 合併與契約面縮小 | 12 – 16 |

---

## 4. 執行包索引

| # | 單獨執行文件 | 優先級 | 預估淨收益 | 前置依賴 | 狀態 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 01 | [刪 leftover 模擬與 `legacy_owner`](01_remove_legacy_owner_simulation.md) | P0 | ~-5600 行模組 + `state.rs` 死分支 | 無 | 待執行 |
| 02 | [刪 `BlockUse`／`wrap_legacy`／舊 inbound](02_remove_blockuse_and_legacy_packets.md) | P0 | 少一條永遠被拒的平行協定 | 無 | 已完成 |
| 03 | [刪 `NetworkHandle::Host` 與 catch-up](03_remove_network_handle_host_and_catchup.md) | P0 | 刪桌面假 listen-host 傳輸樹 | 01 | 待執行 |
| 04 | [刪 presentation 存檔與 `world_mutation`](04_remove_presentation_save_and_world_mutation.md) | P0 | 少第二份 mutation index | 01, 03 | 待執行 |
| 05 | [刪熱路徑 `debug-879839.log`](05_remove_hotpath_debug_logs.md) | P0 | 主執行緒／還原不再開檔 | 無 | 待執行 |
| 06 | [`PresentationTopology` 收成二值](06_presentation_topology_binary.md) | P0 | 點擊／背包少一個不可達模式 | 01 | 待執行 |
| 07 | [實體狀態 dirty／批次廣播](07_entity_state_dirty_broadcast.md) | P1 | 多人 tick + encode 數量級下降 | 無 | 待執行 |
| 08 | [Region cache、批次寫、dirty-only autosave](08_region_cache_and_dirty_autosave.md) | P1 | 消滅 N×整檔 rewrite 與 tick zlib | 無 | 待執行 |
| 09 | [Checksum 改增量](09_incremental_world_checksum.md) | P1 | 每 tick 每維度少掃全表 | 無 | 待執行 |
| 10 | [漏斗 cooldown dirty、熔爐索引、紅石 sleep](10_hopper_furnace_redstone_tick.md) | P1 | 安靜世界的 tick 接近「沒事」 | 無 | 待執行 |
| 11 | [載入光照邊界入隊與 mesh halo／LOD](11_lighting_and_mesh_hotpath.md) | P1 | 走路 hitch 與進圈 CPU | 無 | 待執行 |
| 12 | [剩餘 `CHUNK_HEIGHT`／`0..256` 改 signed-Y](12_signed_y_and_height_helpers.md) | P2 | 高度政策單一來源 | 無 | 待執行 |
| 13 | [測試 `TcpClient`／`temp_world` 合併](13_test_harness_reuse.md) | P2 | 少約 150 行等待複本 | 無 | 已完成 |
| 14 | [inventory click 走既有政策](14_inventory_click_policy_reuse.md) | P2 | 政策只活在 policy 模組 | 06 | 待執行 |
| 15 | [session 鏡像欄位與 Container 雙信封](15_session_mirror_and_container_envelopes.md) | P2 | 少 dual-write 與兩種 click wire | 02 | 已完成 |
| 16 | [縮小 `lib.rs` 桌面契約與死 helper](16_lib_surface_and_dead_helpers.md) | P2 | server 少編永不呼叫的桌面 API | 04 | 待執行 |

---

## 5. 明確不在本路線

- 不把 `AuthorityCore` 與 `ServerRuntime` 合成一個型別。
- 不重寫 renderer／wgpu／compute meshing。
- 不把 `HashMap<(i32,i32), Chunk>` 換成 dense grid／ECS。
- 不平行化 `AuthorityCore::tick`（checksum 是單執行緒決定性的）。
- 不優化 container click 的 `Copy` 固定陣列。
- 不把 `SessionContract` 與 `PlayerSessionState` 合併成一個 struct（interest／存檔不能進確定性核心）。
- 對抗性畸形封包測試必須維持手組 frame，不得改走 `Packet::encode`。
- `save/format.rs` 歷史存檔 `0..256` 遷移語意不得改成 signed-Y 重解讀。
