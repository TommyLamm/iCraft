# 12 — 村莊、POI、村民交易、繁殖、鐵傀儡與襲擊

## 執行包

- 優先級：P2
- 前置條件：04、05、10、11 完成
- 後續解鎖：完整 Adventure/Husbandry 進度、13 的村民交通場景
- 建議提交上限：3
- 禁止順帶實作：流浪商人、全部職業／交易、聲望完整原版數值

## 目標

讓 10 生成的村莊不再是空建築：村民能認領床與工作站、按作息尋路、以 Emerald 交易、
繁殖和受鐵傀儡保護；加入一個可完成的最小襲擊循環。

官方 [Village](https://www.minecraft.net/en-us/article/village)說明 biome-specific 村莊、
多種職業、以 Emerald 交易、交易升級與鐵傀儡保護，作為本計劃的最低語義。

## 實作步驟

### A. POI 與村莊身份

- [ ] 新增 `src/village/poi.rs`，從載入 Chunk 的 Bed/工作站註冊 POI，卸載時保留索引摘要。
- [ ] POI claim 有 owner、lease/retry、dimension/position；方塊被破壞立即解除。
- [ ] 村莊由床、meeting point、居民範圍聚類，不以生成結構 ID 永久綁死。
- [ ] POI save 和重建可兼容舊世界，不每次載入全世界掃描。

### B. 村民、職業與作息

- [ ] 新增 Villager entity metadata：profession、level、XP、offers、home/job/meeting POI、age。
- [ ] 最小職業集合：Unemployed、Farmer、Librarian、Armorer、Cleric。
- [ ] 白天工作／集合、夜間回床、危險時逃跑；navigation 使用 11 budget。
- [ ] 無職業村民認領可達工作站；失去未鎖定工作站可轉職，已交易者保留職業。

### C. 交易

- [ ] 定義資料化 TradeOffer：買入 A/B、賣出、uses/max、villager XP、價格修正。
- [ ] 交易 UI 使用 02 container transaction pattern；host 驗證 offer、庫存和 session。
- [ ] Emerald 作貨幣並加入礦物／loot／交易可達鏈；不得用 Gold 代替。
- [ ] 村民升級解鎖新 offers；每日工作補貨有上限。
- [ ] 兩 client 同時交易最後一次庫存，只能一個成功。

### D. 繁殖、農夫與鐵傀儡

- [ ] 食物意願、可用床、人口上限和 baby 成長。
- [ ] Farmer 可收割／補種 05 的代表作物，遵守 mob griefing world rule 預留接口。
- [ ] 達到居民／驚慌條件生成 Iron Golem；仇恨與玩家聲望採簡化、可測規則。

### E. 最小襲擊循環

- [ ] 加 Pillager/Captain/Ravager 或首版 Pillager+Captain；Bad Omen 狀態來源明確。
- [ ] 帶 Bad Omen 進入有效村莊啟動多 wave 襲擊，波次與難度資料化。
- [ ] 成功給 Hero of the Village 和交易折扣；失敗／Chunk unload／無玩家的暫停策略明確。
- [ ] Advancement trigger 只在 host 權威事件點觸發。

## 主要文件

- 新增：`src/village/{mod,poi,trade,raid}.rs`
- 修改：`src/entity.rs`、`src/ai/*`、`src/spawning.rs`、`src/state.rs`
- 修改：`src/inventory.rs`、`src/player.rs`、`src/save.rs`、`src/network/*`
- 修改：`src/advancements.rs`、`src/mob_renderer.rs`、`src/audio.rs`

## 驗收

- [ ] POI 認領、釋放、Chunk unload/reload、跨 Chunk 尋路和舊世界重建。
- [ ] 職業鎖定、升級、補貨、uses 用盡、價格、雙 client 競爭。
- [ ] 床不足不繁殖，增加床後可繁殖；baby 長大不複製 POI。
- [ ] 農夫收割補種不生成免費物品。
- [ ] 襲擊開始、wave 生成、勝敗、斷線、保存重載和折扣清理。
- [ ] 人工：找到程序化村莊→交易升級→擴床繁殖→觸發並守住一次襲擊。

## 完成閘門

交易、POI 和 raid 均需保存並由 host 權威；只加入 Villager 模型或固定商店 UI 不算完成。

