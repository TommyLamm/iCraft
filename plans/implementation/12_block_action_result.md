# 實作計畫 12：Joined Client 權威方塊 Action-Result 流程

> 來源：Task 10 後續項 G1。
> 相依：建議在計畫 11（N3 reach 驗證）之後實作，因兩者修改同一 host 驗證
> 入口 `set_block_and_broadcast`。

## 狀態

⏳ 待實作

## 目標

Joined client 的生存模式放置／破壞經過 host 權威驗證，host 回傳成功／失敗
結果（ACK），client 依結果消耗物品、生成掉落物、損耗工具耐久、獲得經驗。
不再出現 client 無限放置或不掉物的情況。

## 已確認根因

### 現況資料流

1. Joined client `handle_click`（`src/state.rs:7070-7109`）：
   - 左鍵破壞：raycast → `request_block_change(Air)` → `send_action(Break)`。
   - 右鍵放置：raycast → `get_selected_block()` → 本地 `can_place_block_at`
     UX 預檢 → `request_block_change(block)` → `send_action(Place)`。
   - **不消耗物品、不生成掉落物、不損耗工具**——只送請求並等待。
2. Client → Server：`RequestBlockChange { x, y, z, block }`（`src/network/
   client.rs:82`）。協議 `BlockChange` 封包（`src/network/protocol.rs:63`）
   **沒有** held item、enchantment 或 action type 欄位。
3. Server → Host：`ServerToHost::ClientBlockChange { id, x, y, z, block }`
   （`src/network/server.rs:46`），保留 authenticated `id`，但 **不攜帶**
   client 的持有物品資訊。
4. Host `set_block_and_broadcast`（`src/state.rs:6324`）：只做 block storage
   + lighting + mesh invalidation + redstone + broadcast。**不消耗物品、
   不生成掉落物、不損耗工具、不給經驗、不觸發成就、不增加 exhaustion**。
5. Host broadcast `BlockChange` → Client `apply_remote_block_change`
   （`src/state.rs:6384`）：只做 block storage + lighting + mesh。**無 ACK、
   無失敗回報、無物品消耗、無掉落物**。

### 核心問題

- 協議層沒有 `BlockActionResult` / ACK 變體，client 無法得知請求被接受或拒絕。
- Host 不知道 client 持有什麼物品，無法做生存側效應（消耗、掉落、耐久、經驗）。
- Client 不消耗物品 → 多人生存可無限放置方塊。
- Client 破壞方塊不掉物 → 多人生存無法收集資源。
- DroppedItem 是暫時性實體，不在網路上同步，client 端看不到 host 的掉落物。

## 實作步驟

### 階段一：協議擴充

1. 在 `src/network/protocol.rs` 升 `PROTOCOL_VERSION` 為 5。
2. 新增 `BlockActionRequest` 封包變體：
   ```text
   BlockActionRequest { protocol_version, action: Action, x, y, z,
                        block: u32, held_item: Option<ItemWire> }
   ```
   - `action`：`Place` 或 `Break`。
   - `block`：放置時為目標方塊 wire 值；破壞時為 `Air`。
   - `held_item`：client 當前 selected hotbar slot 的物品 wire 表示
     （item kind + enchantments + potion + durability + custom_name），
     讓 host 能計算 harvest eligibility、fortune、silk touch、tool damage。
3. 新增 `BlockActionResult` 封包變體：
   ```text
   BlockActionResult { protocol_version, x, y, z, success: bool,
                        consumed_item: bool, drops: Vec<ItemWire> }
   ```
   - `success`：host 是否接受並執行了該請求。
   - `consumed_item`：放置成功時為 true，指示 client 消耗一個 selected item。
   - `drops`：破壞成功時的掉落物品清單（host 權威計算），client 直接加入
     backpack（或本地 spawn DroppedItem entity 供拾取）。
4. 定義 `ItemWire` 結構體：`item: u32`（Item enum wire 值）、`count: u16`、
   `durability: u16`、`enchantments: [u8; 6]`（固定六槽 encoding）、
   `potion: Option<PotionWire>`、`custom_name: [u8; 24]`。提供
   `ItemStack -> ItemWire` / `ItemWire -> ItemStack` 轉換。
5. 舊版 client（protocol < 5）在 handshake 階段被拒絕。

### 階段二：Server relay

6. `src/network/server.rs`：`ServerToHost` 新增 `ClientBlockAction { id,
   action, x, y, z, block, held_item }`。Server 從 `BlockActionRequest`
   封包取出欄位，替換 authenticated `id`，轉發給 host。
7. `HostToServer` 新增 `SendBlockActionResult { to: PlayerId, x, y, z,
   success, consumed_item, drops }`。Server 將其封裝為 `BlockActionResult`
   封包，**只發送給該 `PlayerId`**（不是廣播）。
8. 保留既有 `BroadcastBlockChange` 用於 world state 廣播（所有 client）。

### 階段三：Client 端

9. `src/network/client.rs`：`GameToClient` 新增 `RequestBlockAction { action,
   x, y, z, block, held_item }`。`ClientToGame` 新增 `BlockActionResult { x,
   y, z, success, consumed_item, drops }`。
10. `src/state.rs` joined client `handle_click`（`src/state.rs:7070`）：
    - 不再送 `RequestBlockChange`，改送 `RequestBlockAction`，附帶 held item。
    - **不**在本地消耗物品或生成掉落物——等 ACK。
    - 保留 `send_action(Place/Break)` 做手臂擺動 cosmetic。
11. `State` 處理收到的 `BlockActionResult`：
    - `success && consumed_item`：`inventory.use_selected_item(creative)`。
    - `success && !drops.is_empty()`：將 drops 加入 backpack（`add_stack`），
      滿時 spawn 本地 DroppedItem entity。
    - `!success`：不做任何事（client 預測狀態由後續 `AuthoritativeBlockChange`
      自動修正）。

### 階段四：Host 權威處理

12. `src/state.rs` 新增 `handle_client_block_action` 取代
    `set_block_and_broadcast` 作為 remote block request 的入口：
    - 驗證 requester 存在 + reach（計畫 11）。
    - 驗證 chunk 已載入。
    - **Break**：驗證目標非 Air、hardness ≥ 0。執行 canonical break path：
      set_block(Air)、redstone `on_block_changed`、lighting、mesh、
      **生存側效應**（harvest eligibility via held_item、fortune/silk touch、
      drops 計算、XP、exhaustion、tool durability）。廣播 `BlockChange(Air)`。
      回傳 `BlockActionResult { success: true, drops }`。
    - **Place**：驗證 `can_place_block_at` + `can_place_block_with_support`。
      執行 canonical place path：set_block、redstone、lighting、mesh。
      廣播 `BlockChange(block)`。回傳 `BlockActionResult { success: true,
      consumed_item: true }`。
    - 驗證失敗：回傳 `BlockActionResult { success: false, ... }`，不修改 world。
13. Host 的 drops 在 host 端也 spawn 為 DroppedItem entity（host 自己的玩家
    可拾取）。Client 端的 drops 由 ACK 直接入背包或本地 spawn。
14. Host 不再為 remote client 觸發成就（成就只在本地 player 的
    `trigger_advancement` 路徑）；remote client 的成就由 client 自行在收到
    `success` ACK 後觸發本地 `trigger_advancement(MineBlock/CraftItem)`。

### 階段五：收尾

15. 移除舊的 `RequestBlockChange` / `ClientBlockChange` 路徑（或保留作為
    host→client 的 authoritative `BlockChange` 廣播，但 client→host 改用
    `BlockActionRequest`）。
16. 更新 `ARCHITECTURE.md` 多人段落、`track.md`、`plans/progress.md`、
    `plans/implementation/10_bug_audit.md` 的 G1 狀態。

## 驗證

- [ ] `ItemWire <-> ItemStack` roundtrip（含附魔、藥水、耐久、自訂名稱）。
- [ ] Protocol v5 `BlockActionRequest` / `BlockActionResult` roundtrip。
- [ ] 舊協議（v4）client 在 handshake 被拒絕。
- [ ] Host break：harvest eligibility（正確工具／材質）、fortune、silk touch、
      XP、exhaustion、tool durability 計算正確。
- [ ] Host place：reach + collision + support 驗證；成功時 `consumed_item`。
- [ ] Host 驗證失敗：回傳 `success: false`，不修改 world、不廣播。
- [ ] Client 收到 `success + consumed_item`：消耗一個 selected item。
- [ ] Client 收到 `success + drops`：drops 入背包；背包滿時 spawn 本地 drop。
- [ ] Client 收到 `!success`：不消耗物品、不生成掉落物。
- [ ] Host 廣播 `BlockChange` 仍正確到達所有 client（含非請求者）。
- [ ] 既有 placement collision、support、authenticated-id 及 multiplayer
      block sync 測試不回歸。
- [ ] `cargo fmt --all -- --check`、`cargo test --release`、
      `cargo check --release`。
- [ ] 人工 Host + Join 生存模式放置（消耗物品）與破壞（獲得掉落物）（需
      互動式雙視窗）。

## Commit

單一功能 commit：`feat(network): authoritative block action-result for joined clients`
