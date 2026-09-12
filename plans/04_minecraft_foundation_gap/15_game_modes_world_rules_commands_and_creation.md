# 15 — 遊戲模式、World Rules、管理指令與世界建立選項

## 執行包

- 優先級：P3
- 前置條件：04、07、09、10 完成
- 後續解鎖：16
- 建議提交上限：3
- 禁止順帶實作：完整原版 Brigadier、Command Block、資料包函數

## 目標

補齊 Adventure、Spectator、Hardcore，建立可持久化且由 host 權威的 world rule 層，
交付一組足以管理／測試世界的基本指令與更完整的新世界選項。

官方 [All Game Modes](https://help.minecraft.net/hc/en-us/articles/360058743992-Minecraft-Differences-Between-Creative-Survival-and-Hardcore-Game-Modes)
把 Survival、Creative、Adventure、Spectator 和 Hardcore 都列為可玩模式，應作為本計劃口徑。

## 實作步驟

### A. 模式政策

- [x] `GameMode` 增加 Adventure、Spectator；Hardcore 作 world flag + Hard difficulty，不偽裝成普通模式。
- [x] 集中 `GameModePolicy`：碰撞、傷害、飢餓、飛行、穿牆、放置／破壞、拾取、mob targeting。
- [x] Adventure 預設不能改方塊；為 ItemStack 預留 can_break/can_place_on 條件。
- [x] Spectator 可穿方塊飛行、無碰撞／傷害／拾取／world mutation，能觀看但不能操作容器。
- [x] Hardcore 死亡後世界不可再以 Survival 重生；可轉 Spectator 或回標題。

### B. World Rules

- [x] 新增可序列化 `WorldRules`，首批包含：keep_inventory、mob_griefing、do_mob_spawning、
  do_daylight_cycle、do_weather_cycle、do_fire_tick、do_insomnia、sleeping_percentage、pvp。
- [x] 所有系統讀同一 runtime snapshot；禁止各自從 menu setting 猜規則。
- [x] host 修改後廣播；client 顯示但不能提交未授權改動。
- [x] 舊世界用明確 default；規則改動寫入 level data 且原子保存。

### C. 指令與權限

- [x] 新增最小 parser/dispatcher、typed arguments、錯誤位置和 help，不需要複製 Brigadier wire format。
- [x] 指令集合：`help`、`gamemode`、`difficulty`、`gamerule`、`time`、`weather`、`tp`、
  `give`、`kill`、`spawnpoint`、`setworldspawn`、`locate`、`seed`、`save-all`。
- [x] 單人開啟 cheats 或 host operator 才有管理權；普通 client chat 不可執行。
- [x] 所有數值、字串和 selector 數量設上限；命令結果走系統 chat。

### D. 世界建立與管理

- [x] Create World 加 game mode、Hardcore、difficulty、seed、generate structures、bonus chest、cheats。
- [x] world type 最小提供 Default、Superflat；Large Biomes 等列為後續內容。
- [x] 世界列表顯示版本、模式、Hardcore、最後遊玩、是否需要升級；危險升級先備份。
- [x] 加刪除確認、複製／備份世界；所有路徑保持在 `saves/` 解析後根目錄內。

## 主要文件

- 建議新增：`src/game_rules.rs`、`src/commands/*`
- 修改：`src/inventory.rs`、`src/player.rs`、`src/state.rs`、`src/physics.rs`
- 修改：`src/menu.rs`、`src/save.rs`、`src/network/*`、`src/app.rs`

## 驗收

- [x] 五種玩法政策 truth table；切模式時飛行、碰撞、輸入、UI 安全重設。
- [x] Hardcore 死亡、重開世界、備份／升級流程不可繞過永久死亡標記。
- [x] 每項 world rule 均接入 host runtime consumer（時間、天氣、生成、火焰、睡眠、戰鬥、死亡）；序列化、規則同步與 consumer 分支由 release 測試覆蓋。
- [x] 未授權 client 的每個管理指令都被 host 拒絕且不改狀態。
- [x] 指令極端輸入、長字串、NaN/Inf、越界座標、未知 ID 不 panic。
- [x] Superflat 和 Default 同 seed 可重現，generate structures flag 真正控制結構生成。

## 完成閘門

新增模式／規則後，權威判斷應集中於 policy/rules；若各系統仍以大量
`game_mode == Creative` 特例決定行為，先完成收斂再標完成。

## 驗證紀錄

- `cargo fmt --all -- --check`
- `cargo test --release`（645 passed, 3 ignored；另有 placeholder integration test 1 passed）
- `cargo check --release`
- `git diff --check`
- Plan 15 窄測：`game_rules`、typed command parser、Superflat generation、world-rule sync、world path guard、Plan 04 save round-trip 均通過。

尚未執行需要 GPU/window 的 Host + Join Client 實機場景；`do_insomnia` 目前以既有 hostile 夜間生成 gate 實作，專案尚無 Phantom entity registry，待後續內容計劃補齊。

