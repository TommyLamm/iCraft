# 06 — 通用 VoxelShape、方塊狀態與基礎建築件

## 執行包

- 優先級：P1
- 前置條件：01 完成
- 後續解鎖：07、14
- 建議提交上限：3
- 禁止順帶實作：所有木材／顏色變體、完整原版 blockstate JSON

## 目標

停止為每個非整方塊在渲染、碰撞、射線和放置各寫一份特例，建立單一 `VoxelShape`
語義，再交付最常用的半磚、樓梯、柵欄／門、牆、梯子、玻璃片和告示牌。

## 現況問題

- Door/Trapdoor 已有 state 與專用模型／碰撞，是可複用先例。
- 大部分方塊仍假設一個單位 AABB；raycast 主要按 voxel 命中，不理解複合形狀空隙。
- `BlockState` 名稱是通用的，但欄位只服務門／活板門；增加樓梯等會迅速塞滿條件分支。
- 缺 Slab、Stair、Fence、Wall、Ladder、Pane、Fence Gate、Sign 等基本建築能力。

## 實作步驟

### A. Shape 與 state API

- [ ] 新增 `VoxelShape { boxes: SmallVec<[Aabb; N]> }` 或無新依賴的固定小集合表示。
- [ ] 為每個 block 提供 `collision_shape`、`selection_shape`、`occlusion_shape`，三者不可混用。
- [ ] `physics.rs`、placement overlap、DDA 精確命中和 entity LOS 改讀同一 shape API。
- [ ] 對空 shape、薄片、多盒形狀、邊界接觸建立精確測試。
- [ ] 把 state codec 改為按 block 類型的 typed decode/encode；未知 bit 需 sanitize。

### B. 渲染與鄰接

- [ ] mesh builder 接受 shape elements 或專用 model descriptor，統一 outward winding、UV、AO、light。
- [ ] 非滿方塊不參與錯誤 greedy merge；occlusion 只移除真正被覆蓋的面。
- [ ] 鄰接狀態（Fence/Wall/Pane）由 authoritative neighbor update 計算並保存／同步。
- [ ] 跨 Chunk 邊界鄰居未知時保守呈現，載入後重新連接並 invalidation。
- [ ] 水浸首版至少支援 slab/stairs，流體 level 與 block state 不互相覆蓋。

### C. 最小建築內容

- [ ] Oak/Cobblestone Slab：bottom/top/double。
- [ ] Oak/Cobblestone Stair：facing、half；首版可先 straight，inner/outer 作本計劃尾段。
- [ ] Oak Fence/Fence Gate、Cobblestone Wall、Glass Pane：四向連接與正確碰撞。
- [ ] Ladder：面向、攀爬物理、支撐移除。
- [ ] Oak Sign：站立／壁掛、文字 Block Entity、基本編輯 UI 與存檔；不含富文本。
- [ ] 為上述內容補 item、recipe、atlas mapping、creative tab 和掉落。

### D. 交易與相容性

- [ ] 多方塊／多 state 放置使用 01 transaction；Host 驗證支撐、玩家碰撞和 item 消耗。
- [ ] Chunk snapshot/delta 帶 state；舊存檔零 state 對每種新方塊都有合法 default。
- [ ] 既有 Door/Trapdoor/Torch/Cactus/Portal 遷移到 shape API，不改變已驗證 bounds。

## 主要文件

- 建議新增：`src/voxel_shape.rs`、`src/block_model.rs`
- 修改：`src/world.rs`、`src/physics.rs`、`src/interaction.rs`、`src/culling.rs`
- 修改：`src/chunk_manager.rs`、`src/state.rs`、`src/inventory.rs`、`src/crafting.rs`
- 修改：`src/save.rs`、`src/network/*`、`src/block_entity.rs`

## 驗收

- [ ] 每種 shape 的 AABB、selection hit 和 mesh bounds 精確測試。
- [ ] 玩家可站在 slab/stair 上、穿過 fence gap 不可、沿 ladder 攀爬。
- [ ] AO／光照不在半磚內部產生錯誤黑面，透明片排序不回歸。
- [ ] Fence/Wall/Pane 跨 Chunk 放置、載入、卸載和破壞會重算連接。
- [ ] 水浸方塊保存重載與 Host/Client 一致，破壞後水行為明確。
- [ ] Sign 文字有長度／UTF-8 限制，惡意 client 不能提交無界字串。

## 完成閘門

後續新增一種簡單複合形狀方塊時，只需提供 state codec、shape/model 與資料，不應再修改
玩家碰撞、DDA 和網路核心邏輯。

