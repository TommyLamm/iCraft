# 01 — 權威世界變更與 Block Entity 基礎

## 執行包

- 優先級：P0
- 前置條件：無
- 後續解鎖：02、03、04、05、06、14
- 建議提交上限：3
- 禁止順帶實作：箱子 UI、熔爐配方、Hopper、村民或完整資料包系統

## 目標

在不破壞現有 `ChunkManager::chunks` 權威性的前提下，加入可承載箱子、熔爐、
告示牌等動態方塊資料的 Block Entity 儲存，並把散落於 `State` 的方塊副作用收斂為
可測試的權威變更交易。

## 現況與缺口

- `Chunk` 已保存 `BlockType`、`block_state`、流體與光照，但沒有每格動態物件。
- `ChunkSaveData` 和 `Packet::ChunkData` 能保存／同步方塊與一字節狀態。
- `State` 內多條路徑各自處理光照、mesh invalidation、紅石、掉落和廣播，新增容器
  很容易漏掉其中一環。
- `ARCHITECTURE.md` 明確規定 GPU 只在主線程、host 是唯一世界權威、舊存檔需兼容。

## 交付範圍

### A. Block Entity 模型

- [ ] 新增 `src/block_entity.rs`，定義穩定版本的 `BlockEntity` enum；本計劃只需
  `ChestStub`、`FurnaceStub`、`SignStub` 三個可序列化占位型別，不實作玩法。
- [ ] 使用 Chunk-local 線性索引或 `(u8, i16, u8)` 作 key；禁止以世界座標重複保存。
- [ ] 在 `Chunk` 提供 `get/insert/remove/iter_block_entity`，驗證方塊種類與 entity 種類匹配。
- [ ] 方塊被替換為不相容種類時自動移除舊 Block Entity；同種類 state 改變不得誤刪。
- [ ] `Chunk::memory_usage` 計入動態資料；空集合不應為每個 section 分配固定大陣列。

### B. 權威變更交易

- [ ] 新增 `src/world_mutation.rs`，定義 `MutationCause`、`BlockMutationRequest`、
  `BlockMutationOutcome` 和批次變更 API。
- [ ] 交易一次性處理：block/state/entity、光照、支撐連鎖、mesh 依賴、紅石通知、
  dirty revision、音效／掉落事件和網路廣播描述。
- [ ] 將至少「普通放置」「普通破壞」「紅石 mutation」遷移到同一入口；流體與天氣可
  暫留舊路徑，但要產生明確後續清單。
- [ ] 批次變更先驗證後提交，避免門、結構或爆炸只完成一半。

### C. 存檔與網路

- [ ] `ChunkSaveData` 增加 `#[serde(default)] block_entities` 與資料版本；舊存檔讀取為空。
- [ ] 保存時只寫當前 Chunk 的 entity，載入時拒絕越界、重複 key 和不匹配種類。
- [ ] 協議升版；`ChunkData` 加 Block Entity snapshot，另加 host→client 的增量 packet。
- [ ] 設定每個 Chunk 和每個 packet 的 entity 數量／payload 上限，超限要明確斷線或拒絕，
  不可無界配置記憶體。
- [ ] client 只套用 host snapshot/delta，不自行 tick 權威資料。

## 主要文件

- 新增：`src/block_entity.rs`、`src/world_mutation.rs`
- 修改：`src/main.rs`、`src/world.rs`、`src/chunk_manager.rs`、`src/save.rs`
- 修改：`src/network/protocol.rs`、`server.rs`、`client.rs`、`src/state.rs`
- 文檔：`ARCHITECTURE.md`、本計劃狀態

## 測試與驗收

- [ ] Block Entity 插入、替換、刪除、越界與種類匹配單元測試。
- [ ] 新格式 round-trip；舊 Chunk fixture 載入為空 entity 集合。
- [ ] Chunk unload/save/reload 保持資料，失敗保存仍可重試。
- [ ] 全量 snapshot 後增量更新有單調 revision；舊 revision 不覆蓋新資料。
- [ ] 跨 Chunk 批次變更要麼全提交，要麼完全不改。
- [ ] 既有門、紅石、爆炸、天氣、流體和多人方塊測試不回歸。
- [ ] 手工：Host 放置／破壞帶 stub entity 的測試方塊，Join Client 重連後狀態一致。

## 完成閘門

02 可以只新增 `Chest` 行為而不再修改 Chunk 存檔格式；若仍需自行建立另一套容器
sidecar，本計劃視為未完成。

