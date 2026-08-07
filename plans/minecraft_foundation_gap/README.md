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
| P3 | 多人／獨立伺服器 | listen-server；新系統尚無權威協議 | 每玩家狀態、容器交易、獨立無 GPU server | 16 |
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
| 16 | [多人權威與獨立伺服器](16_multiplayer_dedicated_server_and_authority_completion.md) | 已完成（headless dedicated/runtime 通過；30 分鐘 soak 與 GPU Host+Join 實機待執行） |
| 17 | [資源包、本地化、無障礙與總驗收](17_resource_packs_localization_accessibility_and_final_acceptance.md) | 待執行 |

官方資料也佐證上述族群屬於基礎體驗：

- [合成指南](https://www.minecraft.net/en-us/article/how-craft)包含配方書與熔爐配方。
- [出生、死亡與重生](https://www.minecraft.net/en-us/article/spawning-and-dying)包含死亡掉落、世界出生點、床與重生錨。
- [耕作指南](https://help.minecraft.net/hc/en-us/articles/360046311411-A-Beginner-s-Guide-to-Farming-in-Minecraft)列出農田、種子、作物生長與骨粉。
- [村莊](https://www.minecraft.net/en-us/article/village)包含職業、綠寶石交易、等級與鐵傀儡。
- [所有遊戲模式](https://help.minecraft.net/hc/en-us/articles/360058743992-Minecraft-Differences-Between-Creative-Survival-and-Hardcore-Game-Modes)定義 Survival、Creative、Adventure、Spectator 和 Hardcore。
- [獨立伺服器說明](https://help.minecraft.net/hc/en-us/articles/4408873961869-Minecraft-Dedicated-and-Featured-Servers-FAQ-)確認 Java Edition 的獨立伺服器是正式玩法面。
- [Accessibility](https://www.minecraft.net/en-us/accessibility)把選單導覽、旁白與聊天顯示列為核心無障礙工具。

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

15 需 04、07；16 需 02–15 的網路契約已穩定；17 最後執行。
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

