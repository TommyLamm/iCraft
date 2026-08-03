# 03 — 熔爐、熔煉配方與生存資源鏈修正

## 執行包

- 優先級：P0
- 前置條件：01、02 完成
- 後續解鎖：05、14
- 建議提交上限：3
- 禁止順帶實作：Blast Furnace、Smoker、Crafter、完整資料包格式

## 目標

將 `Furnace` 從裝飾方塊變成 3 格、可燃料驅動、可持久化的工作站，並移除目前為了
快速可玩而加入的錯誤配方捷徑，使木材→工具→採礦→熔煉→鐵裝備成為真實閉環。

官方 [第一夜指南](https://www.minecraft.net/en-us/article/how-survive-your-first-night-minecraft)
把熔爐加工材料和烹飪食物列為管理飢餓的基礎環節；[合成指南](https://www.minecraft.net/en-us/article/how-craft)
也區分 crafting 與 furnace recipe book。

## 現況問題

- `RecipeManager` 把 Iron Ore／Gold Ore 寫成 shapeless crafting conversion。
- Bread 配方目前使用三個 Apple，不是三個 Wheat。
- 多種釀造／維度材料使用「替代配方」憑空轉換，破壞探索與維度進度。
- `Furnace` 可合成／放置，但右鍵沒有工作站或燃燒狀態。

## 實作步驟

### A. 配方領域分離

- [ ] 新增 `src/recipes.rs` 或在 `crafting.rs` 分離 `CraftingRecipe`、
  `SmeltingRecipe`、`FuelDefinition`。
- [ ] 配方使用穩定 ID；輸入／輸出保留 count 和 metadata 規則。
- [ ] 修正 Bread 為三個 Wheat；刪除礦石 shapeless conversion。
- [ ] 逐項審核「Substitute recipes」，刪除可由現有 Nether／mob 正常取得者；暫時不可取得者
  必須在 UI 標記 unavailable，不再用無關材料偽造。
- [ ] 提供配方唯一性、可達性和輸出非 Air 的資料驗證測試。

### B. Furnace Block Entity

- [ ] 把 `FurnaceStub` 升級為 input/fuel/output、burn time、burn total、cook progress、
  recipe ID、累積 XP 和 revision。
- [ ] 固定 tick 推進，不以渲染幀率計時；Chunk unload 後以保存時間戳安全補算或明確暫停。
- [ ] 只有輸入、燃料和輸出容量同時合法時才消耗燃料／推進。
- [ ] 燃料燒完、輸入改變、輸出堵塞、配方切換的進度語義有單元測試。
- [ ] lit/unlit 使用 block state 或配對變體，不以另一份權威資料造成分歧。

### C. UI、掉落與 XP

- [ ] 使用 02 的通用容器 UI，增加 input/fuel/output 與兩條進度圖示。
- [ ] output slot 禁止放入；shift-click 根據物品角色路由。
- [ ] 加最小 Recipe Book：只顯示已解鎖 crafting/smelting recipe，可搜尋並在材料足夠時填格；
  unlock 條件保存於玩家資料並由 host 觸發。
- [ ] 取出成品時發放累積 XP；破壞時掉落三格內容但不直接發放未領 XP。
- [ ] 加燃燒／完成音效和最小火焰視覺，不擴大到煙囪粒子系統重做。

### D. 多人與持久化

- [ ] 容器 click 沿用 02 session；host tick 並發布進度／slot delta。
- [ ] client 不自行消耗燃料；斷線重連以 snapshot 收斂。
- [ ] 保存所有計時欄位；舊 Furnace 方塊載入為空工作站。

## 主要文件

- 新增或重構：`src/recipes.rs`、`src/block_entity.rs`
- 修改：`src/crafting.rs`、`src/inventory.rs`、`src/state.rs`、`src/world.rs`
- 修改：`src/save.rs`、`src/network/*`、`src/audio.rs`

## 驗收

- [ ] Coal 與木製燃料有不同且可預期的 burn time。
- [ ] Iron/Gold Ore 不能在 crafting grid 直接變成 ingot。
- [ ] Iron/Gold、沙→玻璃、原肉→熟肉的代表配方可熔煉。
- [ ] 關閉 UI、離開 Chunk、保存重載、Host/Client 重連後進度與 slot 一致。
- [ ] output 滿、錯誤輸入、無燃料時不消耗任何物品。
- [ ] 取出多批成品的 XP 不重發。
- [ ] Recipe Book 解鎖、搜尋、材料不足、auto-fill 與保存重載。
- [ ] 從空背包開始能正常完成「木鎬→石鎬→鐵礦→熔煉→鐵鎬」。

## 完成閘門

不得保留任何可跳過熔爐或維度進度的臨時替代配方，除非在本計劃的「已知例外」中
逐條記錄原因、移除條件和所有者。
