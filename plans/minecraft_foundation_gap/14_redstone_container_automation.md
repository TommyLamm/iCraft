# 14 — 容器紅石自動化與真實 Dispenser／Dropper

## 執行包

- 優先級：P2
- 前置條件：02、03、06 完成
- 後續解鎖：最終紅石驗收
- 建議提交上限：3
- 禁止順帶實作：Crafter、完整所有可發射物品行為、準連接等高階 Java quirks

> 2026-08-06 狀態：核心玩法、存檔、協議與 headless 驗證已完成；GPU/window
> 與真實 Host+Join Client 場景不在目前環境內，對應實機驗收項目保持未勾選，
> 不宣稱已通過。

## 目標

在現有 dust/repeater/comparator/piston/door/TNT 基礎上，補足 Minecraft 最常用的容器
自動化：Hopper、容器 fullness comparator、帶庫存的 Dispenser/Dropper，以及 Observer。

## 現況問題

- `Dispenser`／`Dropper` 沒有容器；紅石 action 固定生成 Arrow 或 Redstone。
- Comparator 有模式但不能讀真實箱子／熔爐 fullness。
- 沒有 Hopper，箱子和熔爐不能形成物品傳輸鏈。
- 紅石 tick 有 budget／睡眠機制，新容器 tick 必須保持相同有界性。

## 實作步驟

### A. Container capability

- [x] 為 Block Entity 定義 `ContainerAccess`：slot count、可插入／可抽出面、revision。
- [x] Chest/Furnace 暴露 sided rules；UI click 與 automation 共用 transaction，不各自改 slot。
- [x] fullness signal 按非空比例與 stack fullness 計算，空=0、非空最少=1、滿=15。
- [x] Comparator 讀容器時在容器 revision 改變後被標 dirty，不每 tick 掃整個世界。

### B. Hopper

- [x] Hopper block entity 5 格、facing、cooldown、enabled state。
- [x] 每次 transfer 至多移動一個 item，先驗證目的容量後原子提交。
- [x] 上方吸入 DroppedItem 和 container pull、朝向 container push；metadata 合併規則沿用 Inventory。
- [x] 紅石 powered 時停用；Chunk 邊界目的未載入時不抽出來源。
- [x] Hopper chain 使用 queue/budget，禁止同 tick 無限循環或依 HashMap 次序改變結果。

### C. Dispenser／Dropper

- [x] 兩者各有 9 格 container，觸發時用決定性 RNG 選非空 slot。
- [x] Dropper 只掉物／插入前方 container；Dispenser 對 Arrow、Splash Potion、Bucket、
  Flint and Steel 實作代表性行為，其餘退回掉物。
- [x] 上升沿觸發一次，不在持續供電每 redstone tick 連發。
- [x] 發射成功才消耗；前方不可用時保持物品或按明確 fallback。

### D. Observer

- [x] Observer facing 與 pulse state；監視前方 block/state/entity revision 的合法變化。
- [x] 自身輸出引起的鄰居更新不產生無限遞迴；使用 scheduled tick queue。
- [x] 跨 Chunk 缺失時不假報變化，載入後建立 baseline。

### E. 存檔／網路／性能

- [x] 所有 container slot、cooldown、facing、pending pulse 保存並同步。
- [x] host 唯一 tick；client 接收 slot delta（僅觀看者）和 block state delta。
- [x] F3/perf 顯示 hopper moves、container checks、observer pulses 和 backlog。

## 主要文件

- `src/block_entity.rs`、`src/redstone.rs`、`src/world_tick.rs`、`src/world.rs`
- `src/state.rs`、`src/inventory.rs`、`src/entity.rs`、`src/save.rs`、`src/network/*`

## 驗收

- [x] Chest→Hopper→Furnace→Hopper→Chest 自動熔煉，物品與燃料總量守恆。
- [x] Hopper 跨 Chunk unload 不吞物；循環鏈 obey budget 且結果可重現。
- [x] Comparator 空、半滿、滿、double chest 和 furnace 不同 slot fullness。
- [ ] Dispenser／Dropper 代表物品、空容器、上升沿、保存重載（headless 可驗證資料/上升沿/存檔；State+GPU 實機場景待執行）。
- [x] Observer 監視放置／破壞／state 改變且不自激振盪。
- [ ] Host+Client 觀看同一自動化容器，UI 不倒退或複製。

## 完成閘門

刪除現有固定 Arrow/Redstone 假輸出路徑；所有輸出必須來自相應 Block Entity 的真實 slot。

> 完成閘門的程式碼條件已滿足：Dispenser/Dropper 只從自身 9 格實際 slot
> 選擇與消耗物品，Dropper 的前方容器寫入與來源扣除為同一個原子結果；固定
> Arrow/Redstone 輸出路徑已不存在。完整實機驗收仍需在可用 GPU 與 Host+Join
> Client 環境執行，故上方未勾選項目維持真實狀態。

