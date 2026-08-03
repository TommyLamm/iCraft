# 02 — 箱子儲存、雙箱與多人容器交易

## 執行包

- 優先級：P0
- 前置條件：01 完成
- 後續解鎖：03、14、16
- 建議提交上限：3
- 禁止順帶實作：熔爐、Hopper、Ender Chest、Shulker Box

## 目標

把現有僅能放置的 `Chest` 變成可靠的 27 格容器，支援相鄰雙箱、內容持久化、破壞
掉落及 host-authoritative 多人互動。

## Minecraft 基礎語義

- 單箱 27 格；兩個相容箱子橫向連接成 54 格。
- 箱子上方被實心方塊遮擋時不能開啟。
- 破壞時箱中物品以實體掉落；任何 metadata、耐久、附魔、藥水和名稱都保留。
- 多名玩家可觀看同一容器，但每次 slot 操作必須由 host 驗證並按序提交。

## 實作步驟

### A. 資料與放置

- [ ] 把 01 的 `ChestStub` 升級為固定 27 格 `ContainerInventory`。
- [ ] 為 Chest 定義 facing 與 single/left/right state；沿用現有一字節 block state 編碼。
- [ ] 放置時檢查相鄰箱數，禁止形成三連箱；決定雙箱左右半並原子更新兩格。
- [ ] 破壞雙箱一半時，另一半退回 single，原有 54 格資料依左右半無損拆分。

### B. UI 與物品守恆

- [ ] 抽出通用 `ContainerView`／slot mapping，避免再把專用 hitbox 堆入 `State`。
- [ ] 支援左鍵、右鍵、shift-click、合併、交換與關閉時游標歸還。
- [ ] 單箱顯示 3×9，雙箱顯示 6×9，下方接玩家 3×9 和快捷欄。
- [ ] 所有操作用 `ItemStack::can_merge_with`，不能丟失或抹除 metadata。
- [ ] 關閉、Esc、死亡、離線、切維度、Chunk unload 都有明確游標與 session 清理策略。

### C. 權威多人交易

- [ ] 新增 `ContainerOpenRequest/Opened/Rejected`、`ContainerClickRequest`、
  `ContainerSnapshot/Delta/Closed` packet，協議升版。
- [ ] session 綁定玩家、維度、方塊座標、容器 revision 和距離；每次 click 重新驗證。
- [ ] host 模擬 click 後才提交，client 只預覽 hover，不樂觀改寫權威 slot。
- [ ] 每個玩家同時只可持有一個容器 session；方塊被破壞時所有 session 收到關閉原因。
- [ ] 廣播給正在觀看同一容器的玩家，未觀看者不接收無關 slot 流量。

### D. 掉落與呈現

- [ ] 破壞箱子時以 01 的 transaction 產生完整內容掉落，再掉落箱子本身。
- [ ] Creative 破壞仍應處理內容，不能無聲刪除玩家物品。
- [ ] 加最小開關動畫與音效；模型可先 single chest，雙箱貼圖／模型可在同一計劃尾段完成。

## 主要文件

- `src/block_entity.rs`、`src/inventory.rs`、`src/state.rs`、`src/world.rs`
- `src/network/protocol.rs`、`server.rs`、`client.rs`
- `src/save.rs`、`src/audio.rs`、`src/shader.wgsl`（只在動畫確有需要時）

## 驗收矩陣

- [ ] 單箱／雙箱放置、三連拒絕、遮擋拒開、拆半 state 正確。
- [ ] 27/54 格所有 click 類型的物品總量與 metadata 守恆 property tests。
- [ ] 破壞、爆炸、支撐連鎖、Chunk unload、保存失敗重試不丟物品。
- [ ] Host 與兩個 Client 同開一箱交錯點擊，不重複、不覆蓋、不死鎖。
- [ ] 超距離、切維度、斷線、箱子被破壞會關閉 session。
- [ ] 舊世界中的 Chest 初次打開為空且可正常保存。

## 完成閘門

箱子內容需在退出程式後重載一致，且兩個 Client 競爭同一格時物品總量保持不變；
只完成 UI 或只完成單機存檔均不算完成。

