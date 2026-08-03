# 10 — 程序化結構、Loot Table 與維度進度

## 執行包

- 優先級：P1
- 前置條件：08、09 完成
- 後續解鎖：11、12、13
- 建議提交上限：3；若內容量超出，先完成引擎＋三個代表結構後停止
- 禁止順帶實作：村民 AI、襲擊、完整所有原版結構

## 目標

取代固定 Chunk 要塞與固定座標末地城，建立跨 Chunk、與生成順序無關、可保存 start/
reference 的結構系統和資料化戰利品；用正常探索資源鏈完成 Nether 與 End 進度。

官方 [Tricky Trials](https://www.minecraft.net/en-us/updates/tricky-trials)把程序化大型結構、
戰鬥遭遇和獎勵視為探索核心；本計劃只建立通用結構／戰利品底座及代表內容。

## 實作步驟

### A. 結構底座

- [ ] 新增 `src/structure/`：`StructureId`、start placement、piece、bounding box、rotation/mirror。
- [ ] region-based 候選座標由 world seed + structure salt 決定，與 Chunk 生成順序無關。
- [ ] 一個 start 可產生多 piece 並跨 Chunk；每個 Chunk 只裁切寫入自身範圍。
- [ ] 保存 structure start/reference，避免重載後重抽或重複放置戰利品。
- [ ] template 格式至少支援 block/state、block entity marker、entity marker 和 processor。

### B. Loot Table

- [ ] 新增 `src/loot.rs`：pool、roll、weighted entry、count range、condition、item modifier。
- [ ] loot seed 綁定 world seed + container position + table ID；容器首次打開或首次生成時只 roll 一次。
- [ ] 戰利品使用 01/02 ContainerInventory，不能以打破特殊 Chest 直接生成固定 Elytra。
- [ ] Fortune/Looting 等條件接口可預留，不在本計劃重做所有 mob drops。
- [ ] 驗證空表、權重溢出、無效 Item ID、極端 roll count，並設生成上限。

### C. 代表性 Overworld 結構

- [ ] Dungeon：房間、spawner 占位、1–2 箱；先以 Zombie/Skeleton 隨機生成。
- [ ] Mineshaft：走廊、支撐、交叉口、Chest；洞穴裁切合理。
- [ ] Village layout：道路／房屋／床／工作站／箱；本計劃只放建築和 POI marker，12 接管居民。
- [ ] Stronghold：多房間、portal room、Eye of Ender 定位；刪除 `(2,2)` 固定房間。
- [ ] 每個結構有 spacing/separation、biome whitelist 和 locate API。

### D. Nether／End 進度

- [ ] Nether Fortress：最小走廊、Blaze spawn marker、Nether Wart farm、loot。
- [ ] Bastion 可列為本計劃尾段；若超出提交上限，明確移到內容擴充，不阻塞基礎進度。
- [ ] End City 用結構 start 生成 tower/ship/loot；Elytra 放真正 loot container。
- [ ] End Gateway 在龍死亡後決定性生成並可往返；Boss 重生／多次龍戰可列清楚狀態。
- [ ] Respawn Anchor 加入 Nether 個人重生點，與 04 spawn API 整合。

### E. 權威性與串流

- [ ] 結構只在 host/worldgen worker 產生；client 接收普通 Chunk snapshot。
- [ ] structure placement 和玩家 mutation revision 合併規則明確，絕不覆蓋已保存玩家修改。
- [ ] 首次生成、載入舊 Chunk、跨版本新增結構採 region status，避免已探索區域突然重生建築。

## 主要文件

- 新增：`src/structure/*`、`src/loot.rs`
- 修改：`src/dimension.rs`、`src/world.rs`、`src/save.rs`、`src/chunk_manager.rs`
- 修改：`src/block_entity.rs`、`src/state.rs`、`src/inventory.rs`、`src/entity.rs`

## 驗收

- [ ] 同 seed／不同生成順序得到相同 structure starts、pieces 和 loot seed。
- [ ] 跨 Chunk 結構無接縫、無重疊重放，卸載重載不重 roll 箱子。
- [ ] 舊已探索 Chunk 不因升級被新結構覆蓋。
- [ ] `locate`／Eye of Ender 返回最近合法 Stronghold，而非固定座標。
- [ ] Survival 可由正常資源鏈找到 Fortress、取得 Blaze/Wart、進入 End、取得 Elytra。
- [ ] Host/Client 同開 structure chest 不生成兩份 loot。

## 完成閘門

必須移除 `dimension.rs` 內固定座標 Stronghold／End City 的權威生成路徑；僅把座標換掉或
多寫幾個固定結構不算完成。

