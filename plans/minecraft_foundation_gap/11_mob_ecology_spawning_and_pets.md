# 11 — 生物生態、生成容量、消失與寵物

## 執行包

- 優先級：P2
- 前置條件：05、07、09、10 完成
- 後續解鎖：12、13
- 建議提交上限：3
- 禁止順帶實作：村民職業／交易／襲擊、全部原版生物

## 目標

先把「生物如何生成、活動、尋路、消失和保存」做成可擴充生態，再加入缺口最大的代表性
生物與馴養閉環；避免繼續在 `mob.rs`／`passive_mob.rs` 以大型 match 堆內容。

## 內容邊界

本計劃的最小代表集合：Spider、Slime、Witch、Drowned、Ghast、Magma Cube、Wither
Skeleton、Wolf、Cat、Horse、Bat、Squid/Fish 中至少一種水生物。超出集合另建內容包。

## 實作步驟

### A. 生成分類與容量

- [ ] 定義 Monster/Creature/Ambient/WaterCreature 類別、每維度／玩家周邊 mob cap。
- [ ] 生成候選檢查 biome、light、surface/fluid、碰撞、與玩家距離、世界難度。
- [ ] Peaceful 清除／禁止 hostile；Easy/Normal/Hard 的 spawn/damage 差異資料化。
- [ ] natural spawn 與 structure spawn 分開標記，避免結構怪物吃掉全部自然 cap。
- [ ] 非持久化遠距 mob 有 despawn；命名、馴養、繁殖、持有物等使其 persistent。

### B. AI 與導航

- [ ] 抽出 goal/brain scheduler：swim、panic、wander、look、tempt、follow owner、melee、ranged。
- [ ] 導航使用有界 node budget 和 timeout；不能每隻 mob 每幀全域 A*。
- [ ] 支援門、窄形狀、水、掉落風險的基本 path policy；Chunk 缺失時停止而非穿越。
- [ ] 攻擊 cooldown、LOS、目標選擇和仇恨由 host tick，client 只插值呈現。

### C. 代表生物

- [ ] 每種新 mob 定義尺寸、生命、速度、掉落、生成規則、AI、音效與最小模型。
- [ ] Spider 攀爬／跳撲、Slime 分裂、Witch 喝／擲藥、Drowned 水陸切換。
- [ ] Ghast 遠程火球、Magma Cube 跳躍、Wither Skeleton 近戰 Wither effect。
- [ ] 水生／ambient 類展示 category cap 與專用移動，不需一次補齊所有魚種。

### D. 馴養與寵物

- [ ] Wolf/Cat 有 owner UUID、tamed/sitting/health/collar state，保存與網路同步。
- [ ] 馴養 item、機率、坐下／跟隨、傳送防卡、owner-only command。
- [ ] Horse 的馴服與基礎屬性在此完成；實際騎乘控制由 13 接手。
- [ ] 寵物死亡／跨維度／owner 離線有明確策略，不跟錯同名玩家。

### E. 實體資料與協議

- [ ] 擴充 `EntityStateWire` 或加 typed metadata delta，設每種 payload 上限。
- [ ] 使用 stable entity ID/owner ID，不將本地 vector index 當身份。
- [ ] persistent entity save 加資料版本和未知種類容錯。

## 主要文件

- 建議新增：`src/ai/*`、`src/spawning.rs`
- 修改：`src/entity.rs`、`src/mob.rs`、`src/passive_mob.rs`、`src/boss.rs`
- 修改：`src/mob_renderer.rs`、`src/state.rs`、`src/save.rs`、`src/network/*`

## 驗收

- [ ] 各 category cap、玩家距離、光照、biome、難度與 despawn 的決定性測試。
- [ ] 200+ mob 固定場景遵守 AI node/tick budget，無幀率相關行為。
- [ ] Host/Client mob spawn/state/despawn 不亂序復活或重複掉落。
- [ ] 寵物 owner 保存重載、斷線重連、跨維度和死亡一致。
- [ ] 每個代表 mob 至少一條完整「生成→攻擊／互動→死亡→掉落」測試。

## 完成閘門

新增下一種普通 goal-based mob 時，不應再修改 `State::update` 主流程；若仍需在 State 加多個
專用 tick 分支，先完成 AI/metadata 抽象再交付。

