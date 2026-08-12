# iCraft 對照 Minecraft 的基礎功能缺口與執行路線

> 審計日期：2026-08-03
>
> 代碼基線：`2a810842ed5dc20e0567867341ec673423963bdd`
>
> 主要依據：`ARCHITECTURE.md`、當前 `src/`、既有 `plans/` 與官方 Minecraft 資料。

## 1. 比較口徑

本路線以 **Minecraft: Java Edition 1.21.5 的核心玩法族群**作為固定參照，原因是
目前資產覆蓋已明確以 1.21.5 為基線。目標不是一次複製該版本的全部方塊、物品、
生物和技術格式，而是補齊玩家會視為「Minecraft 基礎體驗」的閉環：採集、加工、
儲存、生存、建造、探索、戰鬥、交通、社會系統和多人權威性。

官方資料把 Minecraft 的基本體驗概括為收集資源、合成、建造、探索、生存與戰鬥；
Creative 還包含無限資源、無敵與飛行。參見 [How to Minecraft](https://www.minecraft.net/en-us/article/how-minecraft)、
[What is Minecraft](https://www.minecraft.net/en-us/about-minecraft) 和
[Java 1.21.5 發佈說明](https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21-5)。

### 不納入本輪完成定義

- 逐項複製 1.21.5 的完整內容目錄、所有裝飾變體或其後版本的新內容。
- Mojang 帳戶、Realms、官方聊天簽名、Marketplace、Bedrock 跨平台相容。
- 與原版 Java 的封包、存檔 NBT 或 Mod API 二進制相容。
- 光線追蹤、Vibrant Visuals 或原版渲染器逐像素一致。

這些內容可在本路線完成後另建「內容擴充／相容性」路線，不應阻塞基礎閉環。

## 2. 已確認的現況

當前專案已具備可觀的骨架，不應重做：Chunk 串流與光照、挖掘／放置、基本物理、
Survival/Creative、生命飢餓氧氣、日夜天氣、流體、基礎敵對／被動生物、附魔釀造、
紅石、三維度與 Boss、進度、存檔、listen-server 多人遊戲及 Creative 物品目錄。

代碼層面的容量約為：

| 類別 | 現況 | 判讀 |
| --- | ---: | --- |
| `BlockType` | 89 個（含 Air 與狀態變體） | 數量尚可，但大量 Minecraft 基礎形狀／功能方塊缺失 |
| `Item` | 145 個（含 Air） | 多個物品只有目錄／貼圖，未形成使用閉環 |
| `EntityType` | 22 個 | Boss 有了，但常見生態、NPC、載具不足 |
| `Biome` | 7 個 | 缺現代高度、河流／海洋層次、地下生態和大部分地貌 |
| `GameMode` | 2 個 | 缺 Adventure、Spectator、Hardcore |
| 網路協議 | v7 | 有方塊／實體同步，沒有容器交易、睡眠、載具等權威交易 |

## 3. 主要缺口

| 優先級 | 功能族群 | 現況證據 | 需要的完成標準 | 對應計劃 |
| --- | --- | --- | --- | --- |
| P0 | 動態方塊資料 | Chunk 有 block/state，沒有通用 block entity | 箱子／熔爐等可持久化、卸載、同步 | 01 |
| P0 | 儲存容器 | `Chest` 只是方塊／物品 | 單箱、雙箱、內容掉落、多人互斥與同步 | 02 |
| P0 | 熔煉與正確配方 | 礦物被當作無序合成；部分配方是替代配方 | 燃料、進度、輸出、XP、配方資料化 | 03 |
| P0 | 死亡／睡眠／重生 | 死亡直接清背包；固定 `(8,80,8)` 重生 | 掉落、5 分鐘時限、床、世界出生點 | 04 |
| P0 | 耕作與食物 | 有種子／小麥等物品，沒有農田／作物；只有蘋果和麵包能吃 | 從鋤地到收割、烹飪與進食的完整循環 | 05 |
| P1 | 建造形狀 | 主要是整方塊，僅少數特殊模型 | 半磚、樓梯、柵欄、梯子、告示牌、複合碰撞 | 06 |
| P1 | 戰鬥／裝備 | 即時射箭、只有鐵甲、沒有副手／盾牌 | 使用時長、攻擊冷卻、副手、盾、裝備等級 | 07 |
| P1 | 現代世界高度 | 世界固定 0..255 | `-64..319`、有符號 section、舊存檔遷移 | 08 |
| P1 | 地形／生態群系 | 7 種二維噪聲地貌 | 河流、海岸、深淺海、地下與氣候連續性 | 09 |
| P1 | 結構／戰利品 | 固定座標要塞和末地城 | 跨 Chunk 決定性結構、Loot Table、定位與進度 | 10 |
| P2 | 生物生態／寵物 | 22 種實體且 AI 類型有限 | 生成分類、容量、消失、代表性怪物與馴養 | 11 |
| P2 | 村莊／交易／襲擊 | 完全缺失 | POI、職業、交易、繁殖、鐵傀儡、襲擊 | 12 |
| P2 | 載具／導航／釣魚 | 完全缺失 | 船、礦車、騎乘、地圖／指南針、釣魚 | 13 |
| P2 | 紅石自動化 | 有線路和元件，但容器類輸出是固定假資料 | Hopper、容器比較器、真正發射器／投擲器 | 14 |
| P3 | 模式／規則／指令 | 只有 Survival/Creative 與固定世界建立選項 | Adventure/Spectator/Hardcore、gamerule、管理指令 | 15 |
| P3 | 多人／獨立伺服器 | listen-server + dedicated/runtime 基礎；`State` 權威尚未遷移 | 每玩家狀態、容器交易、獨立無 GPU server 的共同權威核心 | 16、18 |
| P3 | 資源包／語言／無障礙 | 硬編碼資產疊加；英／德設定；無專用 Accessibility | 可選資源包、結構化 locale、字幕與 UI 縮放 | 17 |

## 4. 執行包索引

執行任一編號計劃時，使用 [單 Agent 執行 Prompt](prompt.md)，並只替換其中一個
`{{PLAN_FILE}}`；完成後另開任務再處理下一份。

| # | 單獨執行文件 | 狀態 |
| --- | --- | --- |
| 01 | [權威世界變更與 Block Entity](01_world_state_and_block_entities.md) | 已完成 |
| 02 | [箱子儲存與多人容器交易](02_chest_storage.md) | 已完成 |
| 03 | [熔爐、熔煉與配方進度](03_furnace_smelting_and_recipe_progression.md) | 已完成 |
| 04 | [死亡、睡眠、出生點與掉落物](04_death_sleep_spawn_and_item_lifecycle.md) | 已完成 |
| 05 | [耕作、食物與 Random Tick](05_farming_food_and_random_ticks.md) | 已完成 |
| 06 | [VoxelShape 與基礎建築件](06_voxel_shapes_and_building_blocks.md) | 已完成 |
| 07 | [戰鬥、裝備、副手與蓄力使用](07_combat_equipment_offhand_and_item_use.md) | 已完成 |
| 08 | [有符號垂直世界遷移](08_signed_vertical_world_migration.md) | 已完成 |
| 09 | [Overworld 地形、生態與自然模擬](09_overworld_terrain_biomes_and_block_simulation.md) | 已完成 |
| 10 | [程序化結構、戰利品與維度進度](10_structures_loot_and_dimension_progression.md) | 已完成 |
| 11 | [生物生態、生成與寵物](11_mob_ecology_spawning_and_pets.md) | 已完成 |
| 12 | [村莊、交易、POI 與襲擊](12_villages_trading_poi_and_raids.md) | 已完成 |
| 13 | [載具、騎乘、導航與釣魚](13_transport_mounts_navigation_and_fishing.md) | 已完成 |
| 14 | [紅石容器自動化](14_redstone_container_automation.md) | 已實作（headless 通過；實機驗收待執行） |
| 15 | [遊戲模式、規則、指令與世界建立](15_game_modes_world_rules_commands_and_creation.md) | 已完成（headless 通過；Host+Join GPU 實機待執行） |
| 16 | [多人權威與獨立伺服器](16_multiplayer_dedicated_server_and_authority_completion.md) | 已實作基礎；核心權威遷移缺口轉 18（headless dedicated/runtime 短跑通過；30 分鐘 soak 與 GPU Host+Join 實機待執行） |
| 17 | [資源包、本地化、無障礙與總驗收](17_resource_packs_localization_accessibility_and_final_acceptance.md) | 已實作基礎；bounded visible consumer 與 locale layer 缺口轉 Plan29；真 E2E、Accessibility presentation、GPU、30 分鐘 soak、三拓撲實機仍待 |
| 18 | [Plan16 核心權威遷移與獨立伺服器補齊](18_server_authority_unification_followup.md) | authority/network/persistence/management foundation、真 TCP 雙客戶端 harness、fault/metrics、三拓撲 parity vectors、dedicated 30 分鐘 headless soak 與 difficulty consumer 已通過；完整玩家三場景 TCP E2E 由 Plan30/31/32 分流，GPU Host+Join 仍待 |
| 19 | [Plan17 資源包、本地化、無障礙與真驗收補齊](19_plan17_resources_accessibility_acceptance_followup.md) | Singleplayer 真 workflow、selected-pack texture/sound/lang/model/font consumers 與 bounded visible consumer 基礎已通過；Plan30 證明 bounded TCP domains，完整 Foundation/Social block/automation ingress 仍 Plan31、Progression travel/completion 仍 Plan32，GPU/DPI/音效 artifact 仍待 |
| 20 | [Plan01–17 回歸硬化](20_plan01_17_regression_hardening.md) | 已完成；debug/release 結構 seed、Plan15 legacy metadata／備份與 `mob_griefing` consumer 語義均有回歸測試 |
| 21 | [多維度權威拓撲與 Plan18 既有缺口](21_multidimension_authority_topology_followup.md) | Phase A 多維度 headless authority、session routing、interest/persistence foundation、玩法域、三拓撲回歸與 reconnect/soak 自動證據已通過；Plan30 補 bounded 真 TCP domains，dimension travel/full scenario 與 listen State/C GPU 驗收仍待後續 |
| 22 | [Plan21 玩法權威域完成](22_authority_gameplay_domains_completion.md) | 已完成；fishing、workstation transaction、brew ready/take 與 combat/death/respawn headless authority vector 通過；GPU/transport/soak 不在本計劃 |
| 23 | [State／Runtime 權威接線與三拓撲收斂](23_state_runtime_topology_completion.md) | 核心完成；Join Client typed egress、owner-private projection、listen + 2 clients embedded parity gates 通過；Plan30 另補真 TCP bounded domains（含 workstation），dimension/full scenario 與 manual GPU/visual artifacts 仍後續；Plan24 已補 runtime parity、metrics race、全套 regression 與 dedicated soak |
| 24 | [三拓撲對照、Metrics 穩定化與最終自動驗收](24_topology_metrics_final_automation.md) | 已完成；三拓撲 embedded authority parity、TCP metrics publication/rollback、debug/release/check、50x isolated release 與 dated 30 分鐘 dedicated headless soak 均有 artifact；Plan30 true TCP 僅關閉 bounded domain subset，GPU/window/audio/DPI/Host+Join visual 仍明確排除 |
| 25 | [權威難度消費與持久化完成](25_authority_difficulty_completion.md) | 已完成 headless contract；四難度 strict parse、server.properties persistence、Peaceful hostile despawn、Easy/Normal/Hard chase consumer、pvp independence 與 embedded/dedicated parity 通過；完整 vanilla difficulty systems 與 GPU/manual evidence 明確排除 |
| 26 | [容器生命週期強制關閉與箱子回饋](26_container_lifecycle_chest_feedback.md) | 已實作 v16 targeted forced-close；dimension/player/session 精確清理涵蓋超距離、非法維度、transfer、interest departure、break、logout/disconnect；雙箱 `is_open` 首末 viewer、deterministic binary mesh、ChestOpen/Close edge audio 與 client/runtime/headless tests 通過；State 直接 GPU ctor、smooth lid、audio-device、Host+Join visual、v17 epoch/reason/cursor 明確排除 |
| 27 | [半磚 Waterlogging 權威閉環](27_slab_waterlogging_authority.md) | 已完成；OakSlab/CobblestoneSlab raw-fluid bit7、v3 save、FluidUse 原子 bucket、fixed-tick 跨 Chunk flow、v17 BlockChange/ChunkData 與 embedded/listen/dedicated headless projection 及 debug/release/check gates 通過；GPU/window/audio/DPI、完整原版 parity、30 分鐘 soak 明確不在本計劃 |
| 28 | [權威 Dispenser／Dropper](28_authoritative_dispenser_dropper.md) | 已完成 headless/runtime 核心；紅石上升沿 deterministic action、loaded-front guard、Arrow/Potion/Bucket/Flint/普通掉物窄矩陣、Dropper merge/fallback、metadata/save/reload、全球 entity id 與 v17 EntityStateWire 及 TCP 雙客戶端／三拓撲 projection 通過；完整 vanilla 行為、cauldron/waterlogging、hopper rewrite、GPU/window/audio/manual visual 明確不在本計劃 |
| 29 | [Locale layers 與 bounded visible labels](29_plan17_locale_visible_labels.md) | 已完成 bounded headless contract；selected EN/DE partial layers、invalid diagnostics、bounded menu/HUD/inventory/station/command consumers 與 EN/DE coverage 通過；GPU/window/audio/DPI、clean-checkout startup、三拓撲 E2E 仍不在本計劃 |
| 30 | [真實 TCP 拓撲驗收矩陣](30_real_transport_acceptance_matrix.md) | 已完成 bounded evidence；Singleplayer embedded 與 Listen（local host+2 TCP remotes）/Dedicated（2 TCP clients）共用 domain assertions，含 fishing cast+cached duplicate、Furnace/Craft/Enchant/Anvil/Brew、combat/respawn、stale/out-of-order、owner-private projection、reconnect；reel 精確 `InvalidRevision` blocker 轉 Plan33，完整 Foundation/Social block/automation 轉 Plan31、Progression travel/completion 轉 Plan32 |
| 31 | [權威方塊操作與採礦](31_authoritative_block_actions_mining.md) | 已完成；typed BlockAction v18、owner-private progress wire、Singleplayer/Listen/Dedicated TCP 三拓撲 Block/Drop/XP projection 均通過 |
| 32 | [進度旅行與完成閉環](32_progression_travel_completion.md) | 已完成；typed portal/dimension travel、dragon completion、fortress/End City loot 與 Singleplayer/Listen/Dedicated 真 ingress/egress 均通過 |
| 33 | [TCP 釣魚生命週期 revision](33_tcp_fishing_lifecycle_revision.md) | 待執行；修正最新 owner revision 下 cast→reel 的真 TCP lifecycle；Plan30 保留 reel `Rejected(InvalidRevision)` evidence，不繞過 anti-stale gate |
| 34 | [容器破壞內容物守恆](34_container_break_inventory_conservation.md) | 待執行；補非空 Chest/Furnace/Hopper/Dispenser/Dropper 被權威破壞時完整 ItemStack 掉落、duplicate/reconnect/save 守恆；Plan31 只完成 BE removal，不宣稱內容物守恆 |

官方資料也佐證上述族群屬於基礎體驗：

- [合成指南](https://www.minecraft.net/en-us/article/how-craft)包含配方書與熔爐配方。
- [出生、死亡與重生](https://www.minecraft.net/en-us/article/spawning-and-dying)包含死亡掉落、世界出生點、床與重生錨。
- [耕作指南](https://help.minecraft.net/hc/en-us/articles/360046311411-A-Beginner-s-Guide-to-Farming-in-Minecraft)列出農田、種子、作物生長與骨粉。
- [村莊](https://www.minecraft.net/en-us/article/village)包含職業、綠寶石交易、等級與鐵傀儡。
- [所有遊戲模式](https://help.minecraft.net/hc/en-us/articles/360058743992-Minecraft-Differences-Between-Creative-Survival-and-Hardcore-Game-Modes)定義 Survival、Creative、Adventure、Spectator 和 Hardcore。
- [獨立伺服器說明](https://help.minecraft.net/hc/en-us/articles/4408873961869-Minecraft-Dedicated-and-Featured-Servers-FAQ-)確認 Java Edition 的獨立伺服器是正式玩法面。
- [Accessibility](https://www.minecraft.net/en-us/accessibility)把選單導覽、旁白與聊天顯示列為核心無障礙工具。

Plan26 的自動證據以 v16/headless 邊界為準：pre-review baseline debug `cargo test --lib` 為 665 passed、3 ignored，release `cargo test --release --lib` 為 666 passed、3 ignored；完整 pre-review `cargo test --release` 的各 binary/integration/doc-test lanes 均通過。review fix 後窄閘門亦通過：`container_sessions` 9、`server_world` chest/forced-viewer 3、`server_runtime::tests` 14、`headless_server_authority` 1、`runtime_topology_parity` 5；`cargo check --release`、`cargo fmt --all -- --check` 與 `git diff --check` 亦通過。State 直接 GPU 建構、audio-device、smooth lid、Host+Join visual 仍需人工 C 類驗收，v17 epoch/reason/cursor 不在本計劃。舊 Plan02 含 pre-existing invalid UTF-8 control byte，未安全回填其歷史重複 checkbox；Plan26 文件是 D3/lifecycle follow-up 的狀態來源。

Plan28 的 authority/transport 自動證據以同一未發佈 v17 development sequence 為準：
`cargo test --lib authoritative_` 10 passed、`cargo test --lib bucket_` 2 passed，
redstone latch roundtrip 1、EntityStateWire metadata roundtrip 1；真 TCP
`headless_server_authority` Dispenser projection 1 passed，且
`runtime_topology_parity` Dispenser projection 1 passed across
Singleplayer/ListenServer/Dedicated。DroppedItem 的 `ItemWire` 同步攜帶
count、durability、enchantments、custom_name、`can_break` 與 `can_place_on`，
source/target revisions 與 saved redstone latch 由 host authority 提交；中間
Plan27 `b77c38f` 的 EntityStateWire 形狀不宣稱 binary compatibility，Plan27+28
只在未發佈 sequence 內 finalize v17。完整 release/check/fmt/diff gate 由本批
整合收尾執行，Plan14 的 GPU/manual Host+Join、完整 vanilla dispenser 行為和
waterlogging/cauldron/hopper 擴充仍維持明確 non-goal。舊 Plan02 含
pre-existing invalid UTF-8 control byte，未安全回填其歷史重複 checkbox。

Plan28 final serial full-suite record (2026-08-12, `--test-threads=1`) is
1,524 passed, 0 failed, and 6 ignored in both debug and release (library
684/3 ignored, client binary 815/3 ignored, server binary 2, integrations
3/3/3/2/1/6/5, doc-tests 0). `cargo check --all-targets` and
`cargo check --release --locked` pass; `cargo fmt --all -- --check` and
`git diff --check` pass. The serial setting avoids the pre-existing
process-local save serialization-injection race; no production save behavior
was changed for it.

Plan29 final serial full-suite record (2026-08-12, `--test-threads=1`) is
1,532 passed, 0 failed, and 6 ignored in both debug and release (library
688/3 ignored, client binary 819/3 ignored, server binary 2, integrations
3/3/3/2/1/6/5, doc-tests 0). The targeted localization/resource/menu lanes
reported 10, 18, and 30 tests respectively; the state-only filter matched 0
pure tests and is not GPU evidence. `cargo check --all-targets`,
`cargo check --release --locked`, `cargo fmt --all -- --check`, and
`git diff --check` pass. This closes only the bounded headless locale/visible
consumer contract; manual presentation and topology evidence remain open.

Plan31 final serial full-suite record (2026-08-12, `--test-threads=1`) is
completed with protocol v18 typed BlockActions (StartBreak/CancelBreak/Place),
owner-private MiningProgressWire, fixed-tick mining progress, and single-commit
block drop & XP conservation. All 3 tests in `tests/plan31_authoritative_block_actions.rs`
(Embedded, Listen TCP, Dedicated TCP) pass in release mode. Library suite 698/698 tests pass.
`cargo check --release --all-targets`, `cargo fmt --all -- --check`, and `git diff --check` pass cleanly.

## 5. 執行規則

每個編號文件是一個**單獨代理任務**。執行代理必須遵守：

1. 一次只接一份編號計劃；不得順手開始下一份。
2. 先檢查「前置條件」；未達成就停止並回報，不以臨時旁路掩蓋。
3. 僅修改該計劃列出的主要模組及必要接線；發現跨域需求寫入交接，不擴大範圍。
4. 每份最多 3 個功能 commit；若超過，應把剩餘工作拆成續篇，而不是壓成巨型提交。
5. 所有世界變更都保持 host authoritative；新狀態必須同時考慮存檔、卸載、網路和舊檔遷移。
6. 完成後更新 `ARCHITECTURE.md` 與本目錄狀態；不得只以「能編譯」宣告完成。
7. 通用最低驗證：`cargo fmt --all -- --check`、`cargo test --release`、
   `cargo check --release`、`git diff --check`，再執行計劃列出的人工場景。

## 6. 建議次序與並行邊界

```text
01 ─┬─> 02 ─> 03 ─┬─> 05
    │              └─> 14
    ├─> 04
    ├─> 06 ─> 07
    └─> 08 ─> 09 ─> 10 ─> 11 ─┬─> 12
                                └─> 13

04 + 07 + 09 + 10 ─> 15
02–15 contracts stable ─> 16 ─> 17

15 需 04、07；16 需 02–15 的網路契約已穩定；17 最後執行；19 收斂 17 的真驗收，
其中 A 的 listen/dedicated 拓撲依賴 18，B/C 可與 18 並行。
```

- 嚴格串行：`01 → 02 → 03`、`08 → 09 → 10`、`11 → 12`。
- 可在 01 完成後分支：04、06、08。
- 06 完成後可做 07；10 完成後可分別做 11 與 13。
- 14 在 02、03、06 完成後執行。
- 15、16、17 是收斂階段，不應提前混入核心玩法開發。

## 7. 完成定義

本路線完成不是「擁有更多 enum」，而是下列端到端場景在單人、Host、Join Client
三種模式都成立：

1. 新世界出生後採木、合成工具、採礦、用熔爐加工、用箱子保存。
2. 耕種、收割、烹飪、進食，並能睡覺跳夜和設定重生點。
3. 死亡後物品在死亡點掉落、按規則消失，玩家在有效出生點重生。
4. 探索可重現的地貌、結構和戰利品，能靠正常資源鏈進入 Nether 與 End。
5. 村民交易、代表性怪物、寵物與交通形成可持續探索循環。
6. 建築形狀、碰撞、光照、流體和紅石自動化能跨 Chunk 正確工作。
7. 伺服器權威、存檔重載和版本遷移不產生複製、丟失或客戶端分歧。

Plan23 核心接線已完成：Join Client 的 State inputs 走 typed
`GameplayRequest`，embedded/socket 共用權威 projection，listen + 2 clients
headless `RuntimeInput` parity request/interest/owner-private session vector
通過。Plan30 再以真 TCP 覆蓋 bounded fishing cast/duplicate、Furnace/Craft/
Enchant/Anvil/Brew、combat/respawn、stale/out-of-order、reconnect；reel
lifecycle 轉 Plan33，完整 Foundation/Social block/automation ingress 轉 Plan31，
Progression travel/completion 轉 Plan32。GPU/visual artifacts 仍明確保留給
後續 plan；Plan24 已以相同 fixed-tick runtime lane 完成完整 Plan22 三拓撲
parity、transport metrics publication/rollback、完整 debug/release/check 與 dated dedicated 30 分鐘 headless
soak。Plan25 再將 server-owned difficulty 以 strict `ServerDifficulty` 接入既有
hostile AI lane，並完成 `server.properties` save/reload、Peaceful despawn、
Easy/Normal/Hard chase 與 pvp-independent authority parity。headless 結果不取代
GPU/window/audio/DPI 或實機 Host+Join 證據。

