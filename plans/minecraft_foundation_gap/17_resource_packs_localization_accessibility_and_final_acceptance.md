# 17 — 資源包、結構化本地化、無障礙與總體驗收

## 執行包

- 優先級：P3，最後收斂
- 前置條件：01–16 完成
- 後續解鎖：本路線關閉
- 建議提交上限：3
- 禁止順帶實作：原版完整 pack format、core shader override、Marketplace／Mod loader

## 目標

移除對特定磁碟資源包路徑的依賴，讓資產／文字／音效可驗證地覆蓋；建立最低可用的
Accessibility 面板；最後以跨系統場景而非 enum 數量驗收整條路線。

官方 [Accessibility](https://www.minecraft.net/en-us/accessibility)把可導覽選單、旁白與聊天
顯示調整列為核心工具；本計劃先交付 iCraft 架構可合理支援的子集。

## 實作步驟

### A. 資源包管理

- [x] 定義 iCraft pack manifest：id、name、version、format、description、dependencies。
- [x] 從專案 `assets/` 內建包 + 使用者 `resourcepacks/` 掃描；移除外部特定資源包的
  默認依賴，環境變數只作顯式開發 override。
- [x] Menu 提供可用／啟用順序、套用／重載、錯誤詳情；zip 解壓採路徑穿越與大小上限保護。
- [x] 首版覆蓋 texture、item/block model descriptor、sound、font、lang；shader 不列為支援面。
- [x] missing/invalid asset 回退內建資產並產生一次性診斷，不在每幀刷 log。

### B. 結構化本地化

- [x] 把 UI、item/block/entity、death、command、disconnect、advancement 字串換成 translation key。
- [x] 內建 `en_us` 與一個完整第二語言（可延續 `de_de`）；缺 key 回退 en_us 並記錄測試失敗清單。
- [x] 支援參數化消息、複數的最小策略和 UTF-8；不拼接依賴英文語序的片段。
- [x] 語言切換即時刷新 menu/HUD，不重建世界或丟失輸入。

### C. 無障礙與 UI

- [x] 新增 Accessibility 面板：UI scale、chat scale/opacity、subtitles、high contrast、
  reduce flashing、toggle sprint/sneak、camera bobbing、damage tilt。
- [x] 音效事件提供 subtitle key、方向箭頭和短時隊列；隊列有上限。
- [x] reduce flashing 影響 lightning/End flash/damage overlay，不改 gameplay timing。
- [x] 所有 menu 可只用鍵盤導航，focus 可見且順序穩定；滑鼠仍可用。
- [x] 字體／UI 在 4:3、16:9、21:9 和高 DPI scale 不截斷關鍵操作。

### D. 最終矩陣與文檔

- [x] 新增 headless end-to-end 測試：新世界→工具→礦→熔爐→箱子→農場→床→死亡回收。
- [x] 新增 progression 測試：結構→Nether→Fortress→End→Dragon→End City loot。
- [x] 新增 social/automation 測試：村民交易→Hopper furnace→載具運輸。
- [ ] 每條場景在 single、listen-server、dedicated+2 clients 三種拓撲跑同一 assertions。Plan 18 authority hand-off；singleplayer harness 已通過，兩個網路 topology 未宣稱通過。
- [x] 建人工 QA checklist：視覺、音效、輸入、無障礙、GPU 性能、保存／重啟。
- [x] 更新 `README.md`、`ARCHITECTURE.md`、`plans/progress.md`，刪除／標記已過時舊計劃聲明。
- [x] 產生已知差異表，按「基礎缺口」「內容差異」「明確不支援」分類，禁止用百分比自評替代。

## 主要文件

- 建議新增：`src/resources/*`、`src/localization.rs`、`src/accessibility.rs`
- 修改：`src/texture.rs`、`src/audio.rs`、`src/menu.rs`、`src/state.rs`、所有顯示文字來源
- 修改：`README.md`、`ARCHITECTURE.md`、`plans/progress.md`、測試 harness

## 驗收

- [ ] 刪除／改名外部資源路徑後，乾淨 checkout 仍能以內建資產啟動。需人工 GPU/音效啟動證據。
- [ ] 惡意 zip path、壓縮炸彈、錯誤 manifest、缺依賴、循環依賴被安全拒絕。path/manifest/dependency/cycle 自動測試通過；壓縮炸彈與 live ZIP 手測仍待 QA。
- [x] 所有內建 translation key 在 en_us 有值；第二語言覆蓋率達計劃門檻。
- [ ] subtitles、reduce flashing、UI scale、鍵盤導航有自動布局／狀態測試和人工證據。狀態/queue 自動測試通過；視覺人工證據待 QA。
- [ ] 三條端到端場景與三種網路拓撲全部通過，保存後重啟再驗一次。
- [ ] 固定視角 GPU／CPU 性能與記憶體不低於既有 performance artifact 的核准門檻。

## 完成閘門

只有當 `README.md` 所列六個端到端完成定義均有測試或人工證據、且已知差異表沒有未分流
的 P0/P1 基礎缺口時，才可把整個 Minecraft 基礎缺口路線標為完成。

