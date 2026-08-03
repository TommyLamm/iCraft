# 05 — 耕作、食物使用與通用 Random Tick

## 執行包

- 優先級：P0
- 前置條件：01、03 完成
- 後續解鎖：09、11、12
- 建議提交上限：3
- 禁止順帶實作：村民農夫、蜂蜜、完整所有作物與食物

## 目標

建立 Minecraft 式可持續食物循環：製作鋤頭、耕地、灌溉、播種、自然生長、骨粉、收割、
熔爐烹飪和有使用時長的進食；同時提供可供草、樹苗、火等後續共用的有界 random tick。

官方 [耕作指南](https://help.minecraft.net/hc/en-us/articles/360046311411-A-Beginner-s-Guide-to-Farming-in-Minecraft)
列出種子／可直接種植作物、農田和骨粉生長，應作為本計劃的玩法口徑。

## 現況問題

- `world.rs` 有 `random_tick_count` 計數，但沒有通用作物狀態／生長調度。
- 有 Seeds、Wheat、Carrot、肉類等 Item，卻沒有 Farmland 或 Crop block。
- 只有 Apple／Bread 在右鍵分支能立即食用，其餘食物只有名稱與貼圖。
- 食物、燃料和配方屬性散在 match 中，繼續擴充會增加 `State` 耦合。

## 實作步驟

### A. 有界 Random Tick

- [ ] 新增 `src/world_tick.rs`，host 每個 fixed tick 只抽樣已載入且具 random-tick 方塊的 section。
- [ ] 抽樣由 world seed、dimension、game tick、section identity 決定，測試可重現且不依 HashMap 順序。
- [ ] 每 tick 設硬 budget；超量延後，不可因 render distance 線性拖垮一幀。
- [ ] 事件輸出 `WorldMutation`，client 不自行生長。
- [ ] F3/perf 增加抽樣數、mutation 數和 backlog；空世界成本接近零。

### B. 農田與作物

- [ ] 新增木／石／鐵／鑽石鋤（最少石、鐵兩級可先完成）及耐久規則。
- [ ] 新增 Farmland moisture 0..7、WheatCrop age 0..7、CarrotCrop age 0..7；需要時再加 Potato。
- [ ] 鋤對 Dirt/Grass 使用變為 Farmland；上方被實心方塊覆蓋或缺水會退化。
- [ ] 4 格水平範圍水源使農田逐步濕潤；跑跳踩踏依機率恢復 Dirt。
- [ ] Seeds/Carrot 只能種在 Farmland；光照不足不生長。
- [ ] 生長率考慮周圍濕潤農田，並避免把所有作物同 tick 同步成熟。
- [ ] 成熟／未成熟破壞給不同掉落；Fortune 只影響額外種子／作物數。
- [ ] 新增 Bone Meal item 與 Bone→Bone Meal 配方；使用後隨機推進作物年齡但不超過成熟，
  且只在成功時消耗。

### C. 食物使用狀態

- [ ] 為 `ItemProperties` 增加可選 `FoodProperties`：hunger、saturation、use duration、效果與容器返還。
- [ ] 將 Apple、Bread、熟／生肉、Rotten Flesh、Golden Carrot 等代表性物品資料化。
- [ ] 新增按住使用→進度→完成／中斷狀態；切 slot、受 UI gate、死亡時安全取消。
- [ ] 飢餓滿時普通食物不可開始，允許例外必須由資料標記。
- [ ] 03 的 Furnace 加入生肉→熟肉；刪除剩餘的不合理食物替代配方。

### D. 存檔與多人

- [ ] crop age/moisture 走 block state，既有舊存檔 state=0 合法。
- [ ] host 同步生長 mutation；client 的使用進度可預測顯示，但完成與消耗由 host ACK。
- [ ] Chunk unload/reload 不應重置作物年齡；離線生長首版明確不支援。

## 主要文件

- 新增：`src/world_tick.rs`
- 修改：`src/world.rs`、`src/inventory.rs`、`src/crafting.rs`／`recipes.rs`
- 修改：`src/state.rs`、`src/player.rs`、`src/save.rs`、`src/network/*`

## 驗收

- [ ] random tick 在固定 seed 下可重現，budget 永不超限。
- [ ] 濕潤／乾燥、遮擋、踩踏、低光、骨粉、成熟掉落分支有測試。
- [ ] 作物跨 Chunk 邊界找水且缺鄰居時 fail-safe，不錯誤退化。
- [ ] 所有標記為 food 的物品可完成或正確拒絕使用，取消不消耗。
- [ ] Host/Client 同時收割同一作物，只成功一次且掉落不重複。
- [ ] 人工完成「鋤地→引水→種植→等待→骨粉→收割→烹飪→進食」。

## 完成閘門

食物 item 出現在 Creative 目錄不等於完成；至少上述農業循環及 raw→cooked→eat 必須
從全新 Survival 世界可達。
