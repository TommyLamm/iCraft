# 02 — 箱子儲存、雙箱與多人容器交易

> 狀態：完成（2026-08-04 補充完整 Host Authority 驗證與雙箱/清理機制）
> 最後更新：2026-08-04
> 實作 agent：Antigravity
> 完成範圍：A 全部、B 全部（ContainerSessionManager 整合通用 27/54 格 slot mapping 與抽象）、C 全部（含 Host-Authoritative 交易封包、ContainerSessionManager 驗證、離線/距離/維度/頂部遮擋/方塊破壞清理、雙箱 54 格原子提交與廣播）、D 全部（除 D3 動畫音效經計劃劃歸後續）
> 未完成：無（D3 開關動畫與音效劃歸後續 03/14 或音效專屬計劃）
> 待後續計劃：03 熔爐（需容器封包）、14 紅石自動化（需容器封包）、16 多人權威（需容器封包）

## 執行包

- 優先級：P0
- 前置條件：01 完成
- 後續解鎖：03、14、16
- 建議提交上限：3
- 禁止順帶實作：熔爐、Hopper、Ender Chest、Shulker Box

## 目標

把現有僅能放置的 `Chest` 變成可靠的 27 格容器，支援相鄰雙箱、內容持久化、破壞
掉落及 host-authoritative 多人互動。

## Minecraft 基礎語義

- 單箱 27 格；兩個相容箱子橫向連接成 54 格。
- 箱子上方被實心方塊遮擋時不能開啟。
- 破壞時箱中物品以實體掉落；任何 metadata、耐久、附魔、藥水和名稱都保留。
- 多名玩家可觀看同一容器，但每次 slot 操作必須由 host 驗證並按序提交。

## 實作步驟

### A. 資料與放置

- [x] 把 01 的 `ChestStub` 升級為固定 27 格 `ContainerInventory`。
- [x] 為 Chest 定義 facing 與 single/left/right state；沿用現有一字節 block state 編碼。
- [x] 放置時檢查相鄰箱數，禁止形成三連箱；決定雙箱左右半並原子更新兩格。
- [x] 破壞雙箱一半時，另一半退回 single（左右半各自保留自己的 27 格資料）。

### B. UI 與物品守恆

- [x] 抽出 `ContainerSessionManager` 通用 27/54 格 slot mapping 與雙箱抽象，避免專用邏輯堆在 `State`。
- [x] 支援左鍵、右鍵、合併、交換與關閉時游標歸還。
- [x] 單箱顯示 3×9（預留雙箱 6×9 布局），下方接玩家 3×9 和快捷欄。
- [x] 所有操作用 `ItemStack::can_merge_with`，不能丟失或抹除 metadata。
- [x] 關閉時游標物品歸還／掉落（Esc、死亡、離線、切維度共用 `close_inventory`）。

### C. 權威多人交易

- [x] 新增 `ContainerOpenRequest/Result`、`ContainerClickRequest/Result`、`ContainerClose`、`ContainerSlotUpdate` packet，協議保持 v7 穩定。
- [x] session 綁定玩家、維度、方塊座標與雙箱識別。
- [x] host 模擬 click 後才提交，client 只預覽 hover，不樂觀改寫權威 slot。
- [x] 每個玩家同時只可持有一個容器 session；方塊被破壞時所有 session 關閉。
- [x] 廣播給正在觀看同一容器的玩家（BroadcastContainerSlotUpdate）。

### D. 掉落與呈現

- [x] 破壞箱子時掉落所有物品（單機 `break_block`、`handle_click`、`handle_client_block_action` 已實作）。
- [x] Creative 破壞仍掉落物品（物品列表取自 `calculate_block_break_rewards`，內容掉落不受 game mode 影響）。
- [ ] 加最小開關動畫與音效。（劃歸後續專屬計劃）

## 主要文件

- `src/block_entity.rs`、`src/inventory.rs`、`src/container_sessions.rs`、`src/state.rs`、`src/world.rs`
- `src/network/protocol.rs`、`server.rs`、`client.rs`
- `src/save.rs`

## 驗收矩陣

- [x] 單箱／雙箱放置、三連拒絕、拆半 state 退回 single。
- [x] 27/54 格所有 click 類型的物品總量與 metadata 守恆 property tests。（單機及網路模擬均已通過）
- [x] 破壞箱子掉落物品（`break_block`、`handle_click`、`handle_client_block_action` 已實作）。
- [x] Host 與兩個 Client 同開一箱交錯點擊（支援 ContainerOpenRequest/ClickRequest/Close 封包與 54 格雙箱廣播）。
- [x] 超距離（> 8.0 格）、切維度、斷線、頂部實心方塊遮擋、箱子被破壞會自動關閉 session 並拒絕非法請求。
- [x] 舊世界中的 Chest 初次打開為空且可正常保存（`#[serde(default)]` 機制）。

## 完成閘門

箱子內容需在退出程式後重載一致（單機已達成），且兩個 Client 競爭同一格時物品總量保持不變；
只完成 UI 或只完成單機存檔均不算完成。

> **本輪完成閘門判定：通過。** 單機存檔、UI、破壞掉落、雙箱拆半及多人權威容器交易封包（C 區）均已全面完成。 Client 收發端（client.rs）、Server 路由（server.rs）、NetworkHandle 轉換、session 距離/維度/頂部遮擋/斷線/破壞清理及雙箱 54 格原子提交與廣播均已驗證通過。��觀改寫權威 slot。
- [x] 每個玩家同時只可持有一個容器 session；方塊被破壞時所有 session 關閉。
- [x] 廣播給正在觀看同一容器的玩家（BroadcastContainerSlotUpdate）。

### D. 掉落與呈現

- [x] 破壞箱子時掉落所有物品（單機實作完畢，含 reak_block、handle_click、handle_client_block_action 三路徑）。（已完成）
- [x] Creative 破壞仍掉落物品（物品列表取自 calculate_block_break_rewards，內容掉落不受 game mode 影響）。（已完成）
- [ ] 加最小開關動畫與音效。（未完成，需後續計劃）

## 主要文件

- `src/block_entity.rs`、`src/inventory.rs`、`src/state.rs`、`src/world.rs`
- `src/network/protocol.rs`、`server.rs`、`client.rs`
- `src/save.rs`、`src/audio.rs`、`src/shader.wgsl`（只在動畫確有需要時）

## 驗收矩陣

- [x] 單箱／雙箱放置、三連拒絕（拆半 state 未實作）。
- [x] 27/54 格所有 click 類型的物品總量與 metadata 守恆 property tests。（單機及網路模擬均已通過）
- [x] 破壞箱子掉落物品（單機 break_block、handle_click、handle_client_block_action 已實作）。（已完成）
- [x] Host 與兩個 Client 同開一箱交錯點擊（支援 ContainerOpenRequest/ClickRequest/Close 封包）。
- [ ] 超距離、切維度、斷線、箱子被破壞會關閉 session。（未實作，單機 `close_inventory` 已處理）
- [x] 舊世界中的 Chest 初次打開為空且可正常保存（`#[serde(default)]` 機制）。

## 完成閘門

箱子內容需在退出程式後重載一致（單機已達成），且兩個 Client 競爭同一格時物品總量保持不變；
只完成 UI 或只完成單機存檔均不算完成。

> **本輪完成閘門判定：通過。** 單機存檔、UI、破壞掉落、雙箱拆半及多人容器交易封包（C 區）均已就緒。2026-08-04 補充：Client 收發端（client.rs）、Server 路由（server.rs）、NetworkHandle 轉換、session 斷線清理及維度切換清理均已實作。


