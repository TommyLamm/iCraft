# 13 — 船、礦車、騎乘、導航工具與釣魚

## 執行包

- 優先級：P2
- 前置條件：06、07、09、10、11 完成
- 後續解鎖：16 的載具多人收斂場景
- 建議提交上限：3
- 禁止順帶實作：所有木種載具、鞘翅煙火重做、Happy Ghast 等後續版本內容

## 目標

補齊 Minecraft 探索最基本的水上、鐵路和騎乘移動，並加入 Compass/Map/Clock 與釣魚，
使世界尺度擴大後仍有合理導航和資源回收方式。

## 實作步驟

### A. 通用騎乘關係

- [x] Entity 增加 vehicle/passenger stable ID 和 seat transform；保存前驗證無環。
- [x] mount/dismount request 由 host 驗證距離、占用、維度和生存狀態。
- [x] 乘客位置由 vehicle 權威派生，不能同時跑獨立玩家碰撞。
- [x] 下車尋找安全位置；vehicle 被破壞、切維度、死亡、斷線時原子解除。

### B. Boat

- [x] 補齊 sprint-swim／游泳姿態、上浮下潛與一格高空間碰撞，保持氧氣／水流現有語義。
- [x] Oak Boat item/entity、放置水面、雙槳輸入、轉向、浮力、陸地阻力與碰撞。
- [x] 支援一名駕駛和一個額外乘客；先不做 Chest Boat。
- [x] 破壞／撞擊掉落規則、Bubble Column 可列後續但水流影響需明確。

### C. Minecart 與 Rail

- [x] Rail、Powered Rail、Detector Rail、Activator Rail block state 與連接 shape。
- [x] Minecart 沿 rail graph 投影移動，坡道、彎道、交叉連接由鄰接 state 決定。
- [x] Powered Rail 加速／制動、Detector 輸出紅石；跨 Chunk rail 查詢缺失時安全停車。
- [x] 首版只做普通 Minecart，Chest/Furnace/Hopper Minecart 留到內容擴充。

### D. Horse 騎乘

- [x] 接管 11 的 tamed Horse，加入 Saddle item/slot、jump charge、速度／跳躍屬性。
- [x] 玩家控制、碰撞、落下傷害、下車和保存；Horse Armor 可列為可選尾段。

### E. 導航與釣魚

- [x] Compass 指向 world spawn；Clock 顯示 Overworld 時間，在其他維度有不穩定呈現。
- [x] 最小 Map：固定 scale、探索像素、玩家 marker、持久 ID；分批更新避免每幀掃區域。
- [x] Fishing Rod 使用 item-use 狀態生成 hook；等待、咬鉤、收線由 host 狀態機決定。
- [x] fishing loot 使用 10 Loot Table，區分 fish/junk/treasure 的最小集合。

## 主要文件

- 建議新增：`src/vehicle.rs`、`src/rail.rs`、`src/navigation.rs`、`src/fishing.rs`
- 修改：`src/entity.rs`、`src/physics.rs`、`src/world.rs`、`src/state.rs`
- 修改：`src/inventory.rs`、`src/save.rs`、`src/network/*`、`src/mob_renderer.rs`
- 修改：`src/redstone.rs`、`src/loot.rs`、`src/hand_renderer.rs`

## 驗收

- [x] vehicle/passenger 不可形成環，斷線／破壞／切維度不留幽靈乘客。
- [x] Boat 水面、岸邊、碰撞、兩乘客與 Host/Client 高延遲場景。
- [x] Rail 直線、彎道、坡道、Chunk 邊界、powered/detector redstone。
- [x] Horse 馴服→裝鞍→騎乘→跳躍→保存重載完整場景。
- [x] Compass/Clock 在三維度行為明確；Map 重載保持像素且有更新 budget。
- [x] 釣魚取消、成功、過早收線、玩家離開 Chunk 和兩 client 同步。

## 完成閘門

載具位置和釣魚 loot 必須由 host 結算；僅在 client 移動模型或隨機給物品不算完成。
