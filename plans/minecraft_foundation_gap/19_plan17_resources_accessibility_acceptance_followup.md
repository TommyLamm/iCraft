# 19 — Plan 17 資源包、本地化、無障礙與真驗收補齊

## 定位

- 優先級：P3，Plan 17 follow-up
- 前置條件：Plan 17 的基礎 resolver、locale catalog、Accessibility settings、singleplayer harness 已存在；Plan 18 僅是 A 項網路拓撲的前置條件
- 後續解鎖：Plan 17 的真實完成閘門，以及整條 foundation-gap route 的最終驗收
- 建議提交上限：3 個功能提交；文件與測試接線可併入對應功能提交
- 本計劃只補 Plan 17 的可證明缺口，不擴張 vanilla 完整 pack format、Marketplace、shader override、mod loader 或內容目錄

Plan 17 的程式骨架與 headless 基礎測試保留。本計劃把目前的 fixture assertions、未接線的
resource-pack consumers，以及尚未完成的視覺／輸入證據補成可重現的驗收；不能以 enum 數量、
存在性 anchor 或單純 `tick_count` 取代真實行為。

## A. 真端到端 gameplay harness

### 目標

把 `src/final_acceptance.rs` 的三條 singleplayer 場景改為可執行的最小 gameplay workflow，
每一步都透過公開的 simulation API／command／inventory／block-entity seam 完成，再檢查狀態、
持久化與重載結果。禁止直接 `set_block`／`spawn` 只建立目標 enum 作為通過條件；測試 fixture
若必要只能用於建立起始地形，不能代替互動和交易結果。

### Foundation

- 建立新世界並取得木材，透過合成 API 製作工具。
- 以工具採礦並驗證 harvest、耐久與掉落；把礦石送入真正熔爐，驗證燃料、進度、輸出、XP。
- 開啟箱子、放入／取出物品，保存世界後重載，驗證箱子內容仍在且沒有複製或遺失。
- 鋤地、供水、種植、收割、烹飪／進食，並驗證床睡眠跳夜與重生點。
- 觸發死亡，驗證死亡點掉落、重生後回收和物品生命週期規則。

### Progression

- 由有效資源鏈建造並啟動 Nether portal，再在目標維度完成一次可觀察的轉移。
- 取得 Fortress／必要進度後完成 End portal，驗證維度轉移、Dragon encounter state、死亡／
  重生邊界，以及 End City loot table 的實際容器物品，而非只檢查方塊或 entity 存在。
- assertions 必須覆蓋輸入、權威狀態變更、掉落／容器結果與保存後重建；固定座標只可作為
  確定性測試起點。

### SocialAutomation

- 建立村民交易 session，完成至少一筆交易並檢查雙方物品、貨幣與交易冷卻。
- 將物品放入 Hopper，驗證跨容器輸送、熔爐燃料／輸入／輸出和完成後物品數量守恆。
- 以 minecart／其他已支援載具運輸物品或玩家，檢查位置、載荷與保存／重載結果。

### 拓撲與保存

- Singleplayer：A–C 三場景必須在同一 harness 中完成，並在 save→reload 後重跑關鍵 assertions。
- Listen-server、dedicated + 2 clients：沿用相同 scenario/assertion 套件；該部分依賴 Plan 18
  authority unification，Plan 19 不在 Plan 18 完成前宣稱通過。
- 報告應列出每個 step 的輸入、權威結果、保存結果與失敗原因；禁止只輸出 enum assertion 名稱。

## B. Resource-pack consumers、locale 與安全邊界

### Consumer 接線

- `ResourcePackManager` 必須成為 texture、item/block model descriptor、sound、font、lang 的
  唯一解析入口；每個 consumer 都要有覆蓋測試，並保留 built-in fallback。
- 將 `TranslationCatalog::from_resource_packs` 接到 menu、HUD、chat、death、command、
  disconnect、advancement 與 item/block/entity name 的實際渲染路徑；不要讓 static built-in
  `translate` 繞過已選 pack。
- 語言切換要即時更新 menu/HUD/chat 並保留輸入、世界與選取 pack；至少測試參數化消息、最小
  plural、UTF-8、German→English missing-key fallback。
- `ICRAFT_RESOURCE_PACK` 只能是明確設定的開發／測試 override，需在 runtime resolver 中有
  可測試的注入點；預設 discovery 不得回到任何特定外部磁碟路徑。

### Invalid／fallback diagnostics

- 壞 PNG、不可解碼 sound、壞 font/model descriptor、無效 UTF-8/JSON locale 都要回退到 built-in
  或 procedural asset，且每個 logical path 在一個 runtime session 只產生一筆診斷。
- 測試 fallback 不得只檢查「沒有 panic」；要檢查實際 consumer 收到 built-in/procedural 結果、
  diagnostics key 去重，以及 valid override 仍優先於 fallback。

### Manifest、依賴與 ZIP

- manifest 欄位（id/name/version/format/description/dependencies）要有格式、長度、dependency
  ID 與 duplicate/self-reference 驗證；missing dependency 與 cycle 必須拒絕且可在 menu 看到原因。
- `apply_enabled_order` 要維持 dependency topological order；錯序請求應正規化或拒絕，不能讓
  dependency 反過來覆蓋 dependent pack。
- 建立自動測試覆蓋：`../`、反斜線／絕對／drive path、symlink、單 entry > 8 MiB、總包 > 128 MiB、
  entry count 上限、high-ratio deflate、ZIP64／截斷 central directory、manifest invalid、missing
  dependency、cycle、CRC／size mismatch。ZIP 不得落地解壓。
- 目錄 pack 也必須在讀取 manifest 前受大小上限約束；所有拒絕路徑不得修改工作區或建立輸出檔。

## C. Accessibility、字幕與跨比例布局

### Presentation behavior

- subtitle direction 必須以 listener 的 camera-relative forward/right basis 計算，測試不同 yaw、
  front/back/left/right、center、queue cap、dedup、expiry 與 direction localization。
- reduce flashing 明確覆蓋 lightning、End flash（若該效果存在則接入同一 presentation helper）與
  damage overlay，不改 tick、damage、authority 或網路結果；補上 End flash 的最小可測試效果／
  hand-off，不能把不存在的效果標成通過。
- camera bobbing 要控制 camera／視角 presentation（不是只有手部 mesh swing）；damage tilt 要有
  真正可觀察且可關閉的 tilt／overlay 行為，並以狀態測試保證不改 gameplay。

### HUD、menu 與 input

- ui scale、chat scale/opacity、high contrast 必須套用 menu、HUD、chat、subtitle、death screen；
  高比例設定不得透過 NDC clamp 把文字或操作按鈕截掉。
- Accessibility、Options、Resource Packs、Worlds、Create World、Controls、Multiplayer、
  Confirm Delete 每一頁都要有 layout bounds test（4:3、16:9、21:9、DPI／scale），並檢查 focus ring
  與可啟動區域仍在畫面內。
- Resource Packs／Worlds 等動態清單需要鍵盤可捲動、focus 順序穩定且可見；Tab、Shift+Tab、Enter、
  Escape 從文字欄位切換時不能吞掉焦點或遺失輸入。滑鼠命中區需與鍵盤 activation 共用 bounds。
- 測試所有 settings 的保存／重啟（含 subtitles、contrast、reduce flashing、toggle sprint/sneak、
  camera bobbing、damage tilt、resource-pack IDs），以及 live language switch 不重建世界。

## D. 文件、證據與 checkbox 真實性

- Plan 17 的 `[x]` 只代表基礎程式骨架或已通過的 headless unit；真 E2E、consumer 接線、視覺／
  音效／DPI／30 分鐘 soak 與網路拓撲需在本計劃或人工 QA 中有 dated artifact 才可勾選。
- 更新 `17_qa_checklist.md`、`17_known_differences.md`、`plans/progress.md` 與 `ARCHITECTURE.md`，
  把 fixture harness、未接線 consumer、Accessibility presentation 缺口和 Plan 18 hand-off 分流。
- 新增測試報告格式，對每一場景、拓撲、保存重載與人工 QA artifact 記錄 pass／blocked／failed，
  不得用 enum 數量或百分比自評替代。
- 若實作中發現與 Plan 17 無關的內容差異，另建後續 plan；本計劃不順手修復其他玩法系統。

## 主要檔案與責任邊界

預期修改範圍：

- `src/final_acceptance.rs`、`src/sim_harness.rs` 與必要的 gameplay test seams（A）
- `src/resources.rs`、`src/localization.rs`、`src/texture.rs`、`src/audio.rs`、model/font consumers
  與對應 tests（B）
- `src/accessibility.rs`、`src/menu.rs`、`src/state.rs`、`src/audio.rs` 與 layout/state tests（C）
- `plans/minecraft_foundation_gap/17_qa_checklist.md`、`17_known_differences.md`、
  `plans/progress.md`、`ARCHITECTURE.md`（D）

不得在本計劃直接改動 Plan 18 的 authority protocol／server migration；只接入其公開 contract
來執行 topology harness。

## 驗收與完成閘門

- [ ] Foundation、Progression、SocialAutomation 每一步都是可觀察 gameplay 操作，singleplayer
  三場景均通過，並在保存後重載重跑關鍵 assertions。
- [ ] B 的五類 asset consumer 實際使用 selected pack；locale 即時切換、fallback、diagnostics、
  env override、manifest/dependency/ZIP security tests 全通過。
- [ ] C 的 direction、flash、camera/tilt、HUD/menu scale/contrast、dynamic-list keyboard/layout
  tests 全通過，並附 4:3、16:9、21:9、DPI 的人工 artifact。
- [ ] QA checklist 每項有日期、平台、操作步驟與 log／screenshot／performance artifact；未完成項保留
  `[ ]` 並標明 blocker。
- [ ] Listen-server 與 dedicated+2 clients 三場景只有在 Plan 18 authority unification 完成後才可
  由同一 assertion suite 宣稱通過；在此之前標記 `blocked by Plan 18`。
- [ ] `cargo fmt --all -- --check`、`cargo test --release`、`cargo check --release`、
  `git diff --check` 及 Plan 17/19 targeted tests 通過。

### 與 Plan 18 的並行邊界

- Plan 19 A 的 singleplayer harness、B 的 resource/locale consumers 與 C 的 Accessibility/layout
  可立即並行，彼此以 logical asset、settings 與 simulation test contract 對接。
- Plan 19 A 的 listen-server／dedicated+2 clients execution 依賴 Plan 18 authority unification；
  在 Plan 18 完成前只能建立 adapter、scenario vectors 與 blocked report，不可修改或複製其權威
  遷移實作。
- B/C 不應等待 Plan 18；若需要跨域 authority 行為，只記錄 hand-off，不擴大 Plan 19 範圍。
