# iCraft 代碼簡化路線

> 來源：2026-08-16 全專案 `code-simplification` 只讀掃描（對照 `ARCHITECTURE.md`）。
> 代碼基線：`tommy-dev`（審查當日 `ARCHITECTURE.md` 標為 `4e552ed`）。
> 這不是新玩法，也不是效能優化。目標是在**不改行為**的前提下，讓新同事不再修到已旁路的第二套實作。
>
> 第二波（11–20）來源：同一日 01–10 落地後的分區複掃。活拓撲已經是
> `ServerRuntime → AuthorityCore → ServerWorld`；剩下的是 leftover 仍編在活型別旁邊、
> 以及 3k–20k 合成根還沒按責任拆完。

## 1. 怎麼用

一次只執行一份編號計劃。把 [prompt.md](prompt.md) 裡的 `{{PLAN_FILE}}` 換成該檔路徑。
完成並留下窄測試證據後，另開任務再做下一份。

依賴只在各計劃的「前置」寫明。沒寫前置的計劃可以與其他同級項並行。

第二波建議並行組（仍是一次一個 agent／一個 PR）：

- 11、12、13 彼此無依賴，可同時開三條分支。
- 14 建議等 11；15／16 可並行；17 建議等 15。
- 18、19、20 在 07／05／08 已完成的前提下可並行；20 最好等 17，避免跟送出改動搶 `server.rs`。

## 2. 這條路線在修什麼

權威遷移已經完成：Singleplayer / Host 走 `ServerRuntime` → `AuthorityCore` → `ServerWorld`。
舊的「renderer 擁有世界」路徑幾乎整套還在編譯。掃描的主因是：

- 活路徑與 leftover 疊在同一個函式／型別裡。
- 設計好的模組（`SpawningSystem`、`Brain`、desktop `SaveQueue`）看起來像線上入口，實際沒接上。
- 同一契約在三到五處各寫一次（dispatch、點擊、interest 影子欄位、封包適配器）。

`ARCHITECTURE.md` 仍是權威描述。計劃與源碼衝突時以源碼為準，並在該計劃驗收裡更新架構句。

## 3. 優先級

| 級 | 意思 | 計劃 |
| --- | --- | --- |
| P0 | 刪／隔離已證實死碼，或收斂會讓人改錯入口的 API | 01–04、11–13 |
| P1 | 去掉雙寫與重複控制流；行為必須逐測鎖定 | 05–08、14–17 |
| P2 | 結構搬移；不改契約，但 blast radius 大 | 09–10、18–20 |

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
| 11 | [Library 面縮小與 harness cfg](11_lib_surface_and_harness_cfg.md) | P0 | 待執行 | 無 |
| 12 | [活型別 leftover 衛生](12_live_type_leftover_hygiene.md) | P0 | 待執行 | 無 |
| 13 | [TCP 測試 helper 長尾](13_tcp_helper_long_tail.md) | P0 | 待執行 | 04 |
| 14 | [Desktop 改依賴 library](14_desktop_depends_on_lib.md) | P1 | 待執行 | 建議 11 |
| 15 | [leftover 互動抽出 `state.rs`](15_legacy_interaction_extract.md) | P1 | 待執行 | 02、10（建議 06） |
| 16 | [Session 單一同步 helper](16_session_sync_helpers.md) | P1 | 待執行 | 09 |
| 17 | [leftover 進程內通道縮小](17_legacy_inprocess_channels.md) | P1 | 待執行 | 06、08（建議 15） |
| 18 | [`world.rs` 機械拆檔](18_world_module_split.md) | P2 | 待執行 | 07（建議 14） |
| 19 | [`save.rs` 子模組拆分](19_save_module_split.md) | P2 | 待執行 | 05 |
| 20 | [`network/server.rs` 拆檔](20_network_server_split.md) | P2 | 待執行 | 08（建議 17） |

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
- 一次把 25k／19k 行 `state.rs` 重寫或拆成 ECS（10／15 只做檔案邊界，不刪 leftover、不拆 300 欄位）。
- 開 `icraft-core`／`icraft-world`／`icraft-net`／`icraft-client` workspace（先做 14 與 §7 的 audio／culling 解耦）。
- GPU／window／audio-device／DPI／Host+Join 實機畫面。

## 7. 掃描有、但不開獨立計劃的項目

這些是中低優先、會改玩法、或要等 11–20 之後才有乾淨縫。需要時另開 21+，不要塞進執行中的編號。

| 項目 | 為什麼不下單 |
| --- | --- |
| `SpawningSystem` 接到 `ServerWorld` | 距離／上限與 `spawn_mobs` 不同，合併改玩法 |
| `Brain` 取代 `update_mobs` | 權威現在是追最近玩家；接上會改 AI |
| 實體碰撞改用 `block_collision_shape` | mob 會開始卡門／半磚 |
| 兩個 `ray_intersects_aabb` 合併 | 平行軸／盒內命中契約不同 |
| 農田隨機刻寫死濕度 `7u8` | 「修好」會改生長率 |
| 三套指令語言（session／world／console） | 03 已隔離 dispatch；12 只修過期 rustdoc；console 是另一個 admin enum |
| `ContainerSessionManager` 與 slot helper 拆檔 | 權威只用 `simulate_container_click`；可跟 leftover `state.rs` 一起做 |
| `HostToServer` 塌縮成 `Packet` | 17 只停 desktop 新送出；整包塌縮 blast radius 太大 |
| `menu.rs` 按鈕座標三份複製 | 10／15 不拆選單；另開時用一張 `MenuRect` 表餵 focus／click／draw |
| `handle_single_network_event`／`EmbeddedRuntimeBridge` 拆檔 | 15 刻意留下；`#[path]` 另案，不要跟 leftover 互動綁在一起 |
| `update_mobs` 與 `AudioManager` 解耦、`culling` LOS／frustum 拆檔 | 為了讓 server 不再編 rodio／GPU 可見性；等 11／18 後另開，才能談 `client` feature |
| `wgpu`／`rodio` optional、workspace crates | 11 明確不做；依賴上一列解耦 |
| `inventory.rs` 目錄／click 拆檔 | 18 先拆 `world.rs`；Item／BlockType 不得合成 enum |
| `authority/mod.rs` tick／dispatch／portals 拆檔 | 12／16 先清 leftover 與雙寫；公開方法留在 `AuthorityCore` |
| `server_runtime.rs` 投影／ingress 拆檔 | 16／17 之後另開；不要跟 session sync 綁在一起 |
| `app.rs` 的 `time_speed` 同時換副手又加速時間 | 行為正確但命名誤導；改名即可，不擋本路線 |
| `CameraUniform` 與 `update_frame` 各算一次天空色 | 兩個「far」含義不同，合併容易改霧 |
| `crafting.rs` 兩行 `pub use` | 純別名，改名噪音大於收益 |
| `dynamic_resolution` 設定項 | 效能 Plan14 已決定編進 desktop；重開 upscale 另案 |
| `navigation.rs` 改名為 maps | 與未接線的 `ai/navigation.rs` 撞名；純改名可另開 |
