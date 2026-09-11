# iCraft 第四波簡化與熱路徑收益路線 (Simplify Follow-up Wave 09)

> 來源：2026-09-10 全專案 `/simplify` 審查（品質／效能／重用／協定／死碼／桌面／權威 八路並行，只看收益）。
> 代碼基線：`tommy-dev` @ `4432cf1`（Wave 08 十六份計劃全部落地後）。
> 核心目標：刪掉仍在編譯的 leftover 協定與桌面死路徑、讓 20 Hz tick 不再對靜止玩家做全量掃描／全量 clone。

已落地、不重複列入本路線（Wave 08）：

- leftover 模擬／`legacy_owner`、`BlockUse`／`wrap_legacy`、`NetworkHandle::Host`、presentation `SaveManager`。
- `PresentationTopology` 二值、entity dirty broadcast、region cache／dirty chunk autosave。
- checksum **block revision** 增量、漏斗 cooldown dirty、熔爐索引、紅石 sleep。
- 載入光照邊界入隊、mesh halo／selected LOD、signed-Y helper、TcpClient／`temp_world` 合併。
- inventory click 走 `inventory_decision`、session pose／dimension／gameplay overlay 走 `session_sync`。

本波審查已確認：**Plan 09 的 checksum 增量只覆蓋 block revisions**；實體仍每 tick 全表排序。**entity dirty broadcast 只減少靜止重送**，interest 集合與 session snapshot 仍每 tick 全重建。

---

## 1. 怎麼用

一次只執行一份編號計劃。把 [prompt.md](prompt.md) 裡的 `{{PLAN_FILE}}` 換成該檔路徑。
完成並留下窄測試證據後，另開任務再做下一份。

- **P0 死路徑刪除**（01–05）：01 是唯一的協議 bump（v20）。02–05 不改 handshake 數字，但 03／04 動 `state.rs`／`menu.rs`，不要同一 PR 混在一起。
- **P1 熱路徑**（06–11、14）：06 改 snapshot 形狀；07 必須在 11 之前。08／09／10／14 可並行，但不要同一 PR 混 encode 與 fanout。
- **P2 契約收斂**（12、13、15、16）：12 不改 wire 佈局。15 建議在 03 之後。16 僅測試。

---

## 2. 本路線聚焦解決的問題

1. **Leftover 協定仍決定 enum**：六個 inbound `Packet` 變體 decode 後丟棄；`BlockActionResult` 整條 egress／presentation 鏈無 live producer；catch-up（`ChunkAck`／`Catchup*`）在 `ServerRuntime` 是 no-op。
2. **20 Hz 對靜止世界仍全掃**：每個 dimension 四次 session filter；每 tick clone 全部 `SessionGameplayState`；interest `mem::take` 重建兩個 `HashSet`；`ServerWorld::tick` 與 `AuthorityCore::tick` 雙重 checksum，實體全表 `sort_unstable`。
3. **桌面死路徑仍編譯**：`!in_process_authority` 啟動分支不可達；Pickup 本地拾取被 policy 永遠 `Reject`；`dynamic_resolution`／`render_scale` 寫 settings 但不進 GPU；presentation `RedstoneSystem` 只 restore 空 Vec。
4. **契約分叉**：`game_mode` 未走 `session_sync`；`position_to_milli`／`PLAYER_REACH` 三處邊界；測試 `request()` 與 `TestServer` 複本；`ServerWorld::dispatch` 測試專用第二份 operation match。

---

## 3. 優先級劃分

| 級別 | 意義 | 計劃編號 |
| :--- | :--- | :--- |
| **P0** | 刪 leftover 協定／死 enum／桌面死路徑／幽靈設定 | 01 – 05 |
| **P1** | tick／投影／encode 高槓桿 | 06 – 11、14 |
| **P2** | fail-fast、session_sync、helper 合併、測試腳手架 | 12、13、15、16 |

---

## 4. 執行包索引

| # | 單獨執行文件 | 優先級 | 預估淨收益 | 前置依賴 | 狀態 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 01 | [協定 v20：刪 leftover inbound 與死 ACK／catch-up](01_protocol_v20_leftover_packets.md) | P0 | ~-400 行協定／通道／handler | 無 | 已完成 |
| 02 | [刪 `AuthorityTopology` 與死 helper](02_authority_dead_topology_and_helpers.md) | P0 | 少一個從未讀的 enum + 測試 drift | 無 | 已完成 |
| 03 | [刪桌面不可達啟動與 Pickup 死分支](03_presentation_dead_launch_and_pickup.md) | P0 | ~-180 行 `state.rs` 死路徑 | 無 | 已完成 |
| 04 | [刪幽靈 `dynamic_resolution`／`render_scale`](04_ghost_dynamic_resolution_settings.md) | P0 | ~-300 行模組 + settings 鍵 | 無 | 已完成 |
| 05 | [刪 presentation `RedstoneSystem` 空殼](05_presentation_redstone_shell.md) | P0 | ~-100 行 restore 空管線 | 03 | 未開始 |
| 06 | [session 維度索引 + dirty `session_updates`](06_session_index_and_dirty_updates.md) | P1 | 20 Hz 少 O(sessions×dimensions) 與全量 clone | 無 | 未開始 |
| 07 | [interest 未跨欄時跳過重建](07_interest_skip_stationary.md) | P1 | 靜止玩家每 tick 少兩次 `query_radius` | 無 | 未開始 |
| 08 | [拿掉雙重 checksum 與實體全表排序](08_checksum_drop_double_and_entity_sort.md) | P1 | idle tick 不再對全部實體 sort+hash | 無 | 未開始 |
| 09 | [實體 idle skip 與增量 `sync_positions`](09_entity_idle_and_incremental_sync.md) | P1 | 靜止生物不再每 tick 寫 velocity／掃空間索引 | 無 | 未開始 |
| 10 | [出站封包只 encode 一次](10_encode_once_shared_bytes.md) | P1 | ChunkData／EntityState 尖峰少 2–3 次 bincode | 無 | 未開始 |
| 11 | [mutation／block-entity fanout 反向索引](11_mutation_fanout_and_be_encode.md) | P1 | 紅石／流體 busy 時少 O(mutations×players) | 07 | 未開始 |
| 12 | [Command／ItemUse fail-fast](12_command_itemuse_fail_fast.md) | P2 | 縮小永遠 Unsupported 的 dispatch | 無 | 未開始 |
| 13 | [`game_mode` 納入 session_sync 與座標常數](13_gamemode_sync_and_coord_helpers.md) | P2 | 少 split-brain 與三份 `position_to_milli` | 無 | 未開始 |
| 14 | [random-tick eligible 索引](14_random_tick_eligible_index.md) | P1 | simulation union 不再每 tick 掃+sort | 無 | 未開始 |
| 15 | [policy 幽靈 API 與 inventory hit 去重](15_policy_ghost_and_inventory_hit.md) | P2 | 少重複 chunk policy 與 hit probe | 03 | 未開始 |
| 16 | [測試 `request()`／`TestServer` 合併](16_test_request_and_loopback_server.md) | P2 | 少約 8 份 fixture 複本 | 無 | 未開始 |

---

## 5. 明確不在本路線

- 不把 `AuthorityCore` 與 `ServerRuntime` 合成一個型別。
- 不把 `SessionContract` 與 `PlayerSessionState` 合併。
- 不平行化 `AuthorityCore::tick`。
- 不重寫 renderer／wgpu／compute meshing。
- 不把 `HashMap<(i32,i32), Chunk>` 換成 dense grid／ECS。
- 不把 `HostToServer` 整層刪成直接 `Packet`（與 embedded `RuntimePresentationEvent` 三層合併留給後續；本波 01 只刪死變體）。
- 不把 container open／click／slot 收進 `PlayerSessionUpdate`（Plan 15 刻意保留的 live 投影）。
- 不合併 embedded `project_authority_mutations` 與 `BlockChange` 事件鏈（雙路徑語意不同：fluid／revision）。
- 不把 autosave／eviction 移出 tick 執行緒、不改啟動全量 restore（crash 完整性；Wave 08 只做了 dirty **chunk**）。
- 不刪 `Container { action: 1 }` 的 wire gap（除非未來再 bump）。
- 不刪 `SimHarness`／`final_acceptance` 整包（~2000 行測試遷移，另開波次）。
- 不統一 dual response cache（transport vs `SessionContract`；idempotency 風險高）。
- 對抗性畸形封包測試必須維持手組 frame。
- `save/format.rs` 歷史存檔 `0..256` 遷移語意不得改成 signed-Y 重解讀。

---

## 6. 審查來源（唯讀，2026-09-10）

成功：權威／runtime、效能（兩份）、代碼品質、協定殘留、死碼、桌面 presentation、重用模式。
五份補充掃描因用量上限中斷；與成功報告主題重疊，未重跑。
