# iCraft Minecraft 基礎缺口計劃 — 單 Agent 執行 Prompt

## 使用方法

將下面 Prompt 中的 `{{PLAN_FILE}}` 替換成**一份且僅一份**編號計劃，例如：

```text
plans/minecraft_foundation_gap/02_chest_storage.md
```

不要一次填入多份計劃。完成該計劃並驗收後，另開新任務執行下一份。

## Prompt

```text
你是 iCraft 專案的實作 agent。請在目前 workspace 中完整執行以下唯一計劃：

{{PLAN_FILE}} = 

工作原則：

1. 本次只執行上述一份計劃。不得順手開始後續編號、相鄰功能或計劃中明確排除的內容。
2. 開始修改前，必須完整閱讀：
   - ARCHITECTURE.md
   - plans/minecraft_foundation_gap/README.md
   - {{PLAN_FILE}}
   - workspace 中適用的 AGENTS.md（如存在）
3. 以當前源碼為準，逐項核對計劃的「現況」和「前置條件」，不要只相信舊進度文檔。
   若前置條件尚未完成：停止實作，列出具體缺失、代碼證據和最小解阻方案；不要建立臨時旁路。
4. 工作樹可能已有使用者或其他 agent 的修改。先執行 git status，保留所有無關變更；不得 reset、checkout、
   覆蓋或回退不屬於本任務的內容。遇到重疊時調整自己的實作以兼容現況。
5. 先建立簡短執行清單，再按計劃中的階段逐步完成。每完成一個可驗證階段就運行對應的窄測試，
   不要等所有修改結束才首次編譯。
6. 從相關 symbol 開始閱讀，避免無目的地通讀 18,000 行的 src/state.rs。需要改 State 時，先定位精確資料流、
   authority gate、UI gate、更新順序和現有測試。

不可破壞的架構約束：

- Chunk／entity collections 是權威世界狀態；mesh、visibility、GPU allocation 和粒子是可重建 cache。
- Host／server 是所有 gameplay mutation 的唯一權威。Join Client 不得自行結算世界、容器、物品消耗、
  傷害、掉落、交易、睡眠、AI、載具或隨機 tick。
- 新增狀態時必須同時考慮：建立、變更、破壞、Chunk unload/reload、存檔失敗重試、舊存檔遷移、
  network snapshot/delta、斷線重連和 stale revision。
- 所有可靠請求需驗證已認證玩家、維度、距離、當前狀態、權限及 revision；拒絕不得產生部分副作用。
- 跨多格或多 slot 操作必須先驗證後原子提交，不能留下半扇門、半個雙箱或已扣物品但未完成的世界狀態。
- 背景工作結果必須攜帶 generation/lifetime/revision identity；stale result 應丟棄。
- GPU resource 建立、buffer upload 和 queue submission 保持在主線程；headless gameplay 不得依賴 GPU。
- 為 queue、packet、解壓資料、字串、collection、每 tick 工作量和遞迴／連鎖更新設明確上限。
- 不可為了快速通過流程加入錯誤替代配方、固定座標獎勵、client-side 權威或無持久化的假資料。

實作品質要求：

- 優先抽取純函數、交易結果和 typed state，為權威規則寫單元測試。
- 新 wire/save 格式使用穩定 ID、明確版本和向後兼容 default/migration；需要時提升 protocol version。
- ItemStack 的 count、durability、enchantment、potion、custom name 等 metadata 必須守恆。
- 世界 mutation 要沿用統一的光照、mesh dependency、紅石通知、dirty revision、保存和廣播路徑。
- 不要只新增 enum、貼圖或 UI 外殼；必須完成計劃要求的端到端玩法閉環。
- 不要把所有新邏輯繼續塞入 State；按照計劃抽出有清晰 ownership 的模組。
- 新增或修改的失敗路徑不得 panic、靜默丟資料或宣稱保存成功。

驗證要求：

1. 完成計劃內列出的全部自動測試與驗收矩陣。
2. 至少運行：
   - cargo fmt --all -- --check
   - cargo test --release
   - cargo check --release
   - git diff --check
3. 對多人功能，加入或運行 Host + Join Client 的 authority、重複請求、亂序、拒絕、斷線重連測試。
4. 對存檔功能，驗證新格式 round-trip、舊格式 migration、Chunk unload/reload 和保存失敗重試。
5. 對性能敏感功能，驗證計劃指定的 budget、queue bound、固定場景時間或記憶體指標。
6. 需要 GPU／視窗／多人實機操作而當前環境無法完成時，不得假稱通過；列出精確人工步驟和未驗證風險。

文檔與收尾：

- 實作完成後更新 ARCHITECTURE.md 中受影響的 ownership、runtime flow、authority、persistence、protocol、
  invariants 和 verification 說明。
- 將 {{PLAN_FILE}} 的狀態與 checkbox 更新為真實結果；未完成項保持未勾選並說明原因。
- 更新 plans/minecraft_foundation_gap/README.md 的對應狀態，但不要改動其他計劃的狀態。
- 如既有 plans/progress.md 或 track.md 對本功能有直接聲明，同步修正；不要重寫無關歷史。
- 除非使用者明確要求，不要自行 commit、push 或建立 PR。若有 commit 要求，整份計劃最多 3 個聚焦 commit。
- 完成本計劃後立即停止，不要開始下一份。

最終回報必須包含：

- 實際完成的玩法結果，而不只是修改步驟。
- 主要修改文件與關鍵架構決策。
- 新增／更新的 save 和 protocol version，以及兼容策略。
- 已運行的測試、測試數量／結果及人工驗收結果。
- 尚未完成或無法驗證的項目與風險。
- git status 摘要，清楚區分本任務修改和原有無關修改。

只有當 {{PLAN_FILE}} 的「完成閘門」確實滿足、所有必要工作完成且沒有未分流的阻塞缺陷時，
才可以宣告該計劃完成。
```

