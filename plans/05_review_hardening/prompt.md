# iCraft 審查硬化路線 — 單 Agent 執行 Prompt

## 使用方法

將下面 Prompt 中的 `{{PLAN_FILE}}` 替換成**一份且僅一份**編號計劃，例如：

```text
plans/05_review_hardening/01_close_block_use_mutation.md
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
   - plans/05_review_hardening/README.md
   - {{PLAN_FILE}}
   - workspace 中適用的 AGENTS.md（如存在）
3. 以當前源碼為準，逐項核對計劃的「定位」和「前置」。不要只相信舊進度文檔。
   若前置尚未完成：停止實作，列出具體缺失、代碼證據和最小解阻方案；不要建立臨時旁路。
4. 工作樹可能已有使用者或其他 agent 的修改。先執行 git status，保留所有無關變更；不得 reset、checkout、
   覆蓋或回退不屬於本任務的內容。遇到重疊時調整自己的實作以兼容現況。
5. 先建立簡短執行清單，再按計劃中的階段逐步完成。每完成一個可驗證階段就運行對應的窄測試，
   不要等所有修改結束才首次編譯。
6. 從相關 symbol 開始閱讀。需要改 State 時，先定位精確資料流、authority gate、UI gate、
   更新順序和現有測試，避免通讀整個 src/state.rs。

不可破壞的架構約束：

- Host／server 是所有 gameplay mutation 的唯一權威。Join Client 不得自行結算世界、容器、物品消耗。
- 拒絕不得消耗物品、產生掉落或部分變更世界。
- Revision 以 (dimension, revision) 為單位。
- 新權威行為寫進 AuthorityCore／ServerWorld，不要加回 renderer 遺留模擬。
- 測試必須鎖契約，不得再批准客戶端作者化的 BlockUse／ItemWire。

完成定義：

- 計劃「精確 acceptance」每一項都有代碼或測試證據。
- 計劃列出的窄測試通過。不要宣稱 repo-wide full suite，除非計劃明確要求。
- 在計劃檔補「實作與證據」段落：改了什麼、測了什麼、留下的缺口。
- 把 plans/05_review_hardening/README.md 該列狀態改成「已完成」或「已實作（列出剩餘）」。
```
