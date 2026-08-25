# iCraft 代碼簡化路線

> 來源：2026-08-16 全專案 `code-simplification` 只讀掃描（對照 `ARCHITECTURE.md`）。
> 代碼基線：`tommy-dev`（審查當日 `ARCHITECTURE.md` 標為 `4e552ed`）。
> 這不是新玩法，也不是效能優化。目標是在**不改行為**的前提下，讓新同事不再修到已旁路的第二套實作。
>
> 第二波（11–20）來源：同一日 01–10 落地後的分區複掃。活拓撲已經是
> `ServerRuntime → AuthorityCore → ServerWorld`；剩下的是 leftover 仍編在活型別旁邊、
> 以及 3k–20k 合成根還沒按責任拆完。
>
> 第三波（21–30）來源：2026-08-19 在 01–20 全部落地後對照 `ARCHITECTURE.md`
> （當時標為 `e907760`）與活源碼的再掃描。活權威路徑已經清楚；剩餘是
> leftover 仍編進 desktop、未接線原型仍編進 lib、以及幾個 3k–17k 合成根。

## 1. 怎麼用

一次只執行一份編號計劃。把 [prompt.md](prompt.md) 裡的 `{{PLAN_FILE}}` 換成該檔路徑。
完成並留下窄測試證據後，另開任務再做下一份。

依賴只在各計劃的「前置」寫明。沒寫前置的計劃可以與其他同級項並行。

第二波建議並行組（仍是一次一個 agent／一個 PR）：

- 11、12、13 彼此無依賴，可同時開三條分支。
- 14 建議等 11；15／16 可並行；17 建議等 15。
- 18、19、20 在 07／05／08 已完成的前提下可並行；20 最好等 17，避免跟送出改動搶 `server.rs`。

第三波建議並行組（`state.rs` 上 21 → 25 → 28 必須串行）：

- 21、22、23、24 彼此無檔案衝突，可同時開四條分支。25／28 不要跟 21 同時改 `state.rs`。
- 25 建議等 21；28 建議等 21（以及 25，若 inbound 還有 `is_authoritative()`）。
- 26、27、29、30 彼此無依賴，可與 22–24 並行。

## 2. 這條路線在修什麼

權威遷移已經完成：Singleplayer / Host 走 `ServerRuntime` → `AuthorityCore` → `ServerWorld`。
01–20 之後，活啟動路徑不再經過 leftover；第三波掃的是「死路徑仍編譯」與「合成根還沒按責任切開」：

- leftover 三檔仍無條件編進 desktop，`tick_simulation`／`handle_click` 仍混著第二套世界。
- `Brain`／`SpawningSystem` 仍編進 `icraft-server`；`SoundMaterial` 把 rodio 拉進 lib。
- `is_authoritative()` 與 `PresentationTopology` 重疊；`authority/mod.rs`、`server_runtime.rs`、`menu.rs`、`inventory.rs` 仍是 3k–17k 合成根。

`ARCHITECTURE.md` 仍是權威描述。計劃與源碼衝突時以源碼為準，並在該計劃驗收裡更新架構句。

## 3. 優先級

| 級 | 意思 | 計劃 |
| --- | --- | --- |
| P0 | 刪／隔離已證實死碼，或收斂會讓人改錯入口的 API | 01–04、11–13、21–22 |
| P1 | 去掉雙寫與重複控制流；行為必須逐測鎖定 | 05–08、14–17、23–25 |
| P2 | 結構搬移；不改契約，但 blast radius 大 | 09–10、18–20、26–30 |

## 4. 執行包索引

| # | 單獨執行文件 | 優先 | 狀態 | 前置 |
| --- | --- | --- | --- | --- |
| 01 | [隔離已證實死路徑](01_quarantine_dead_paths.md) | P0 | 已完成 | 無 |
| 02 | [表現層拓撲改成單一 enum](02_presentation_topology_enum.md) | P0 | 已完成 | 無 |
| 03 | [權威請求單一 dispatch](03_authority_single_dispatch.md) | P0 | 已完成 | 無 |
| 04 | [TCP 測試 helper 收斂](04_tcp_test_helpers.md) | P0 | 已完成 | 無 |
| 05 | [Embedded 不再建 desktop 存檔工人](05_embedded_save_ownership.md) | P1 | 已完成 | 無 |
| 06 | [點擊／物品欄／挖掘 cancel 去重](06_presentation_click_and_inventory.md) | P1 | 已完成 | 02 |
| 07 | [世界 tick helper 與 signed-Y 掃描](07_world_helpers_and_signed_y.md) | P1 | 已完成 | 無 |
| 08 | [網路 leftover 只收不發](08_network_legacy_egress.md) | P1 | 已完成 | 無 |
| 09 | [權威 world map 與 session 影子欄位](09_authority_world_map_and_session.md) | P2 | 已完成 | 03 |
| 10 | [`state.rs` 模組拆分](10_state_module_split.md) | P2 | 已完成 | 02（建議 06 已合併） |
| 11 | [Library 面縮小與 harness cfg](11_lib_surface_and_harness_cfg.md) | P0 | 已完成 | 無 |
| 12 | [活型別 leftover 衛生](12_live_type_leftover_hygiene.md) | P0 | 已完成 | 無 |
| 13 | [TCP 測試 helper 長尾](13_tcp_helper_long_tail.md) | P0 | 已完成 | 04 |
| 14 | [Desktop 改依賴 library](14_desktop_depends_on_lib.md) | P1 | 已完成 | 建議 11 |
| 15 | [leftover 互動抽出 `state.rs`](15_legacy_interaction_extract.md) | P1 | 已完成 | 02、10（建議 06） |
| 16 | [Session 單一同步 helper](16_session_sync_helpers.md) | P1 | 已完成 | 09 |
| 17 | [leftover 進程內通道縮小](17_legacy_inprocess_channels.md) | P1 | 已完成 | 06、08（建議 15） |
| 18 | [`world.rs` 機械拆檔](18_world_module_split.md) | P2 | 已完成 | 07（建議 14） |
| 19 | [`save.rs` 子模組拆分](19_save_module_split.md) | P2 | 已完成 | 05 |
| 20 | [`network/server.rs` 拆檔](20_network_server_split.md) | P2 | 已完成 | 08（建議 17） |
| 21 | [leftover 模擬改為 test／feature 才編譯](21_legacy_owner_cfg.md) | P0 | 已完成 | 15 |
| 22 | [未接線 AI／刷怪原型改 cfg(test)](22_unused_prototypes_cfg.md) | P0 | 已完成 | 無 |
| 23 | [`SoundMaterial` 抽出，audio 移出 library](23_sound_material_and_audio_fence.md) | P1 | 已完成 | 建議 21 |
| 24 | [culling 連通／LOS 與 frustum 拆檔](24_culling_connectivity_split.md) | P1 | 已完成 | 無 |
| 25 | [`is_authoritative()` 收成拓撲謂詞](25_topology_predicates.md) | P1 | 已完成 | 02（建議 21） |
| 26 | [`AuthorityCore` tick／dispatch／portals 拆檔](26_authority_module_split.md) | P2 | 已完成 | 03、09、16 |
| 27 | [`ServerRuntime` 投影／ingress 拆檔](27_server_runtime_split.md) | P2 | 已完成 | 16、17 |
| 28 | [`EmbeddedRuntimeBridge` 與 inbound 拆檔](28_embedded_bridge_extract.md) | P2 | 已完成 | 15（建議 21、25） |
| 29 | [`menu.rs` 按鈕座標單一 MenuRect 表](29_menu_rect_table.md) | P2 | 已完成 | 無 |
| 30 | [`inventory.rs` 目錄／click 拆檔](30_inventory_module_split.md) | P2 | 已完成 | 18 |

## 5. 與其他路線的關係

- `plans/05_review_hardening/` 修的是「已宣稱契約與現役適配器不一致」。本路線假設那些契約已經成立，只做行為不變的簡化。
- `plans/04_minecraft_foundation_gap/` 是玩法閉環。不要把本路線的搬移寫回 31/34 當「未完成」。
- `plans/03_performance/` 是量測後的速度工作。本路線禁止以「看起來比較快」為由加複雜度。

## 6. 明確不在本路線

- 合併刷怪規則（`SpawningSystem` vs `spawn_mobs` 數字不同，合併會改玩法）。
- 把 `update_mobs` 接到未上線的 `Brain`。
- 實體碰撞改走玩家的 `block_collision_shape`（mob 會開始卡門／半磚）。
- 從 wire enum 刪除或重排 `GameplayOperation::BlockUse`、未使用的 `Packet` variant。
- 把維度 / session 迭代改成 `HashMap`（checksum）。
- 重開 offscreen dynamic resolution。
- 一次把 25k／19k 行 `state.rs` 重寫或拆成 ECS（10／15／28 只做檔案邊界；21 只 cfg leftover，不拆 300 欄位）。
- 開 `icraft-core`／`icraft-world`／`icraft-net`／`icraft-client` workspace（先做 14、23、24；`rodio`／`wgpu` optional 仍等 23／24 落地後另開 31+）。
- GPU／window／audio-device／DPI／Host+Join 實機畫面。

## 7. 掃描有、但不開獨立計劃的項目

這些是中低優先、會改玩法、或要等 21–30 之後才有乾淨縫。需要時另開 31+，不要塞進執行中的編號。

| 項目 | 為什麼不下單 |
| --- | --- |
| `SpawningSystem` 接到 `ServerWorld` | 距離／上限與 `spawn_mobs` 不同，合併改玩法。22 只 cfg，不接線 |
| `Brain` 取代 `update_mobs` | 權威現在是追最近玩家；接上會改 AI。22 只 cfg，不接線 |
| 實體碰撞改用 `block_collision_shape` | mob 會開始卡門／半磚 |
| 兩個 `ray_intersects_aabb` 合併 | 平行軸／盒內命中契約不同 |
| 農田隨機刻寫死濕度 `7u8` | 「修好」會改生長率 |
| 三套指令語言（session／world／console） | 03 已隔離 dispatch；console 是另一個 admin enum |
| `ContainerSessionManager` 與 slot helper 拆檔 | 權威只用 `simulate_container_click`；30 先拆 `inventory.rs` |
| `HostToServer` 塌縮成 `Packet` | 17 只停 desktop 新送出；整包塌縮 blast radius 太大 |
| `wgpu`／`rodio` optional、workspace crates | 11 明確不做；23 只把 `audio.rs` 移出 lib，24 只拆 culling 檔。optional 等 23／24 落地後另開 |
| 把 culling visibility 半邊改 desktop-only | 24 先拆檔；搬出 lib 才能談 `client` feature |
| 合併 `SessionContract` 與 `PlayerSessionState` | interest／save codec 不能進 checksum 核心；16 已收斂寫口 |
| 兩份 `microbench` 合併 | desktop `--microbench` 不能依賴 lib `harness` |
| 刪 wire 上的 `BlockUse`／`wrap_legacy` | protocol break；08／17 已停新送出 |
| `app.rs` 的 `time_speed` 同時換副手又加速時間 | 行為正確但命名誤導；改名即可，不擋本路線 |
| `CameraUniform` 與 `update_frame` 各算一次天空色 | 兩個「far」含義不同，合併容易改霧 |
| `crafting.rs` 兩行 `pub use` | 純別名，改名噪音大於收益 |
| `dynamic_resolution` 設定項 | 效能 Plan14 已決定編進 desktop；重開 upscale 另案 |
| `navigation.rs` 改名為 maps | 與 `ai/navigation.rs` 撞名；22 把 AI 側 cfg 掉之後純改名可另開 |
