# 04 — 死亡掉落、床、出生點與物品生命週期

## 執行包

- 優先級：P0
- 前置條件：01 完成
- 後續解鎖：15、16
- 建議提交上限：3
- 禁止順帶實作：Hardcore UI、重生錨、完整 gamerule 指令

## 目標

修正目前「死亡直接清空背包、永遠在固定座標重生」的核心偏差，加入可恢復的死亡掉落、
安全世界出生點、床的雙方塊／睡眠／個人重生點，以及多人跳夜規則。

官方 [出生、死亡與重生指南](https://www.minecraft.net/en-us/article/spawning-and-dying)
說明死亡物品留在原地、載入區域中約 5 分鐘後消失、預設回到世界出生點，並可由床設定
個人重生點。

## 現況問題

- `State::take_damage` 在死亡時呼叫 `inventory.clear()`，不生成掉落。
- `State::respawn` 強制切回 Overworld 並設到 `(8, 80, 8)`。
- `PlayerState::reset_for_respawn` 保留 XP，沒有死亡 XP 規則。
- 沒有 Bed block/item、睡眠狀態、出生點安全查找或多人睡眠投票。

## 實作步驟

### A. 掉落與 XP 實體

- [ ] 將死亡 inventory、armor、副手（若 07 尚未完成則預留接口）原子轉成 DroppedItem。
- [ ] 每個掉落保留完整 `ItemStack`；合併只在 metadata 相同時發生。
- [ ] 掉落物在其 Chunk 載入並權威 tick 的累計 5 分鐘後消失；卸載時間不錯誤扣減。
- [ ] 加 `ExperienceOrb` 或等價可收集 XP bundle；按明確公式掉落部分等級並清理玩家 XP。
- [ ] 火／熔岩、仙人掌、爆炸對掉落物的處理有一致 damage policy。

### B. 世界出生點

- [ ] `LevelData` 增加 world spawn 維度、位置、yaw/pitch 與資料版本。
- [ ] 新世界依種子選擇安全地表；重生時搜尋可站立、頭部無碰撞、已載入位置。
- [ ] 無安全個人出生點時回退 world spawn；仍失敗才使用受控 emergency platform。
- [ ] Join Client 的重生由 host 指定，client 不自行猜座標。

### C. 床與睡眠

- [ ] 新增 Bed item/block；使用 block state 表達 facing、head/foot、occupied。
- [ ] 放置／破壞兩格原子化；模型、碰撞和 mesh 走 06 的 shape 接口預留，06 未完成時先用
  專用形狀實作。
- [ ] Overworld 夜間且附近無敵對生物時可睡；白天只設定 spawn 並提示不能睡。
- [ ] 睡眠完成將時間推至清晨並清除對應天氣；醒來尋找床邊安全格。
- [ ] 床被移除或周圍堵塞時，個人 spawn 失效並回退 world spawn。
- [ ] Host 計算睡眠比例；首版預設所有在線、存活、同 Overworld 玩家都睡才跳夜。

### D. 存檔與協議

- [ ] Player save 增加個人 spawn、睡眠狀態不持久化。
- [ ] 協議新增 death drops、respawn result、sleep request/state/time skip；所有請求驗證距離與維度。
- [ ] 玩家斷線時若正在睡眠，立即退出投票；死亡畫面不能保留容器 session。

## 主要文件

- `src/world.rs`、`src/inventory.rs`、`src/entity.rs`、`src/player.rs`
- `src/state.rs`、`src/save.rs`、`src/dimension.rs`、`src/physics.rs`
- `src/network/protocol.rs`、`server.rs`、`client.rs`、`src/mob_renderer.rs`

## 驗收

- [ ] 滿背包死亡後，每件物品與 metadata 的總量守恆；重拾可恢復。
- [ ] 掉落物只在載入區域累計到期，保存重載不重置或重複計時。
- [ ] 床 head/foot 跨 Chunk 邊界放置、破壞、保存、同步均原子。
- [ ] 白天、夜晚、附近怪物、床被堵、床被破壞的分支符合規則。
- [ ] Host+2 Client 的睡眠進入／離開／斷線不會卡死時間。
- [ ] 死亡後容器、聊天、飛行、移動輸入和游標狀態安全清理。

## 完成閘門

單人與多人都必須能完成「設床→遠行死亡→掉落留在死亡點→床邊重生→取回物品」；
任何物品直接消失或複製都屬阻塞缺陷。

