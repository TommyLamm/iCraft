# Track 12：Joined Client 權威方塊 Action-Result 流程

> 對應計畫：`plans/implementation/12_block_action_result.md`（Task 10 後續項 G1）
> 相依：建議在計畫 11（N3 reach 驗證）之後實作，兩者改同一 host 入口 `set_block_and_broadcast`。
> 狀態：⏳ 待實作
> 目標：Joined client 生存放置／破壞經 host 權威驗證，host 回 ACK，client 依結果
> 消耗物品、生成掉落物、損耗工具、獲得經驗。消除 client 無限放置／不掉物。
> Commit：`feat(network): authoritative block action-result for joined clients`

## 協議版本協調（重要）
- 計畫 12 與 13 都把 `PROTOCOL_VERSION` 4->5，且都動 `BlockChange`。
- **建議順序**：先做 12（升 v5 + `BlockActionRequest`/`BlockActionResult` + `ItemWire`）；13 再於同一 v5 為 `BlockChange`/`ChunkData` 附加 `state`。
- 若先做 13，則 12 不再升版，僅在既有 v5 上加變體。**只升一次版**。

## 相關程式碼位置（已核對）
- `src/network/protocol.rs:5` `PROTOCOL_VERSION=4`；`Packet`(23-94)；`BlockChange`(63-69)；`ChunkData`(70-75)；舊版拒絕測試(316-328)。
- `src/network/server.rs:46` `ServerToHost::ClientBlockChange`；`:61` `HostToServer::BroadcastBlockChange`；`:466` BlockChange->ClientBlockChange；`:592` Broadcast->Packet::BlockChange。
- `src/network/client.rs:82` `RequestBlockChange`；`:42` `ClientToGame::BlockChange`；`:311` 送 BlockChange；`:254` 收 BlockChange。
- `src/state.rs:6324` `set_block_and_broadcast`；`:7069` `handle_click`（client 分支 7070-7109）；`:6486` `break_block`（本地 canonical break，survival 6539-6650）；`:2207` `request_block_change`；`:4034` host 處理 ClientBlockChange；`:4341` `trigger_advancement`；`:265` `spawn_dropped_item`。
- `src/inventory.rs:392` `ItemStack{item,count,durability,enchantments,potion,custom_name}`；`:1980` `add_stack`；`:2041` `use_selected_item`。
- `src/player.rs:84` `add_exhaustion`；player `add_experience`。
- `src/entity.rs:16` `EntityType::DroppedItem`。

## 子任務清單

### 階段一：協議擴充

#### 12.1 定義 `ItemWire` 與雙向轉換
- [ ] 檔案：`src/network/protocol.rs`（或新 `src/network/wire_item.rs`）
- 步驟：
  1. `ItemWire{ item:u32, count:u16, durability:u16, enchantments:[u8;6], potion:Option<PotionWire>, custom_name:[u8;24] }`（`Serialize/Deserialize/Clone/PartialEq`）。
  2. `PotionWire` 對應 `brewing::PotionData`。
  3. `from_stack(&ItemStack)` / `to_stack() -> Option<ItemStack>`，enchantments↔`EnchantmentSet`、potion↔`PotionData`、custom_name↔`ItemName`。
  4. 空手用 `item==Air wire` 或 `count==0`。
- 驗證：轉換函式對齊 `ItemStack` 欄位。

#### 12.2 `ItemWire <-> ItemStack` roundtrip 測試
- [ ] 檔案：`src/network/protocol.rs` 測試區
- 步驟：roundtrip 含普通方塊、附魔工具（鋒利V/效率IV/絲綢/時運III）、帶耐久、Potion/SplashPotion、自訂名稱（邊界長度）、Air/count 0。
- 驗證：`cargo test --release item_wire` 全綠。

#### 12.3 升 `PROTOCOL_VERSION` 為 5
- [ ] 檔案：`src/network/protocol.rs:5`
- 步驟：改 `= 5`；舊版拒絕測試(316-328)用 `PROTOCOL_VERSION-1` 自動測 v4 被拒。
- 驗證：`server.rs:288`、`client.rs:198/241` 拒絕 v4。

#### 12.4 新增 `BlockActionRequest` 變體
- [ ] 檔案：`src/network/protocol.rs`
- 步驟：`BlockActionRequest{ protocol_version, action:Action, x,y,z, block:u32, held_item:Option<ItemWire> }`；`action`=Place/Break；`block` 放置=目標 wire、破壞=Air。更新 `protocol_version()`。
- 驗證：編譯通過。

#### 12.5 新增 `BlockActionResult` 變體
- [ ] 檔案：`src/network/protocol.rs`
- 步驟：`BlockActionResult{ protocol_version, x,y,z, success:bool, consumed_item:bool, drops:Vec<ItemWire> }`。更新 `protocol_version()`。
- 驗證：編譯通過。

#### 12.6 封包 roundtrip 測試
- [ ] 檔案：`src/network/protocol.rs` 測試區
- 步驟：`BlockActionRequest`(含 held_item 與 None)、`BlockActionResult`(success/fail、consumed_item、多 drops) roundtrip；`protocol_version()`==v5。
- 驗證：`cargo test --release block_action` 全綠。

#### 12.7 v4 client handshake 被拒測試
- [ ] 檔案：`src/network/server.rs` 測試區（參考 `:1003`）
- 步驟：v4 handshake 被拒、回 `Disconnect`、不建 session。
- 驗證：`cargo test --release` 通過。

### 階段二：Server relay

#### 12.8 `ServerToHost::ClientBlockAction`
- [ ] 檔案：`src/network/server.rs`
- 步驟：新增 `ClientBlockAction{ id, action, x,y,z, block, held_item }`；server 收 `BlockActionRequest` 以 authenticated `id` 替換送出（參考 `:466`）。
- 驗證：server 編譯；轉發正確。

#### 12.9 `HostToServer::SendBlockActionResult`
- [ ] 檔案：`src/network/server.rs`
- 步驟：新增 `SendBlockActionResult{ to, x,y,z, success, consumed_item, drops }`；server 封為 `Packet::BlockActionResult` **只發給 `to`**（非廣播）。保留 `BroadcastBlockChange` 廣播 world state。
- 驗證：定向發送邏輯正確。

#### 12.10 Server relay 測試
- [ ] 檔案：`src/network/server.rs` 測試區
- 步驟：`BlockActionRequest`->`ClientBlockAction`(id 鑑權)；`SendBlockActionResult`->只目標 client 收 `BlockActionResult`。
- 驗證：`cargo test --release server` 全綠。

### 階段三：Client 端

#### 12.11 Client bridge 事件
- [ ] 檔案：`src/network/client.rs`
- 步驟：`GameToClient::RequestBlockAction{ action,x,y,z,block,held_item }`；`ClientToGame::BlockActionResult{ x,y,z,success,consumed_item,drops }`；收 Request->送 `Packet::BlockActionRequest`；收 `Packet::BlockActionResult`->送 `ClientToGame::BlockActionResult`。
- 驗證：client 編譯；映射正確。

#### 12.12 `handle_click` client 分支改送 `RequestBlockAction`
- [ ] 檔案：`src/state.rs:7070-7109`
- 步驟：
  1. 左鍵：raycast->`request_block_action(Break,pos,Air,held_item)`。
  2. 右鍵：raycast->`get_selected_block()`->本地 `can_place_block_at` UX 預檢->`request_block_action(Place,target,block,held_item)`。
  3. `held_item` 取自 `hotbar[selected]` 轉 `ItemWire`（空手 None）。
  4. **不**本地消耗／掉物-等 ACK。保留 `send_action(Place/Break)` cosmetic。移除舊 `request_block_change` 呼叫。
- 驗證：client 不再本地扣物品／掉物。

#### 12.13 State 處理 `BlockActionResult`
- [ ] 檔案：`src/state.rs`（drain 區，約 `:2140`/`:4034`）
- 步驟：
  1. `ClientToGame::BlockActionResult`->`NetworkInbound` 變體或直接處理。
  2. `success&&consumed_item`：`inventory.use_selected_item(creative)`。
  3. `success&&!drops.is_empty()`：逐筆 `add_stack`，滿時 `spawn_dropped_item`。
  4. `!success`：不動作（由後續 `AuthoritativeBlockChange` 修正）。
  5. break `success` 時 client 自行 `trigger_advancement(MineBlock(...))`。
- 驗證：ACK 驅動消耗／掉落／成就。

#### 12.14 Client 端測試
- [ ] 檔案：`src/network/client.rs` + `src/state.rs`
- 步驟：Request->Packet roundtrip；Packet->ClientToGame；`success+consumed_item` 消耗一個；`success+drops` 入背包／滿 spawn；`!success` 不消耗不掉落。
- 驗證：`cargo test --release` 全綠。

### 階段四：Host 權威處理

#### 12.15 抽出 canonical break 生存側效應
- [ ] 檔案：`src/state.rs`（重構 `break_block:6486`）
- 步驟：將 survival 區塊（6539-6650：harvest eligibility via held_item、fortune/silk、drops、XP、exhaustion、tool durability）抽成函式 `(old_block,pos,held_item:&ItemStack,game_mode)->(drops:Vec<Item>,xp:u32,exhaustion:f32,tool_damage:bool)`；本地 `break_block` 改呼叫並 spawn drops/add xp/exhaustion/damage tool。本地行為不變。
- 驗證：本地 break 行為與測試不回歸。

#### 12.16 新增 `handle_client_block_action`
- [ ] 檔案：`src/state.rs`
- 步驟：取代 `set_block_and_broadcast` 為 remote request 入口：
  1. 驗證 requester 存在 + reach（計畫 11 `block_within_reach`）。
  2. 驗證 chunk 已載入。
  3. **Break**：目標非 Air、hardness≥0；canonical break path（set Air、redstone `on_block_changed`、lighting、mesh、12.15 側效應用 `held_item`）；廣播 `BlockChange(Air)`；回 `success:true, drops`。
  4. **Place**：`can_place_block_at`+`can_place_block_with_support`；canonical place path；廣播 `BlockChange(block)`；回 `success:true, consumed_item:true`。
  5. 失敗：回 `success:false`，不改 world。
- 驗證：host 正確分支 place/break。

#### 12.17 Host drops spawn + ACK 回傳
- [ ] 檔案：`src/state.rs`
- 步驟：host drops 也 spawn DroppedItem（host 玩家可拾取）；透過 `HostToServer::SendBlockActionResult{to:requester,...}` 回 ACK；host 不為 remote 觸發成就。
- 驗證：host spawn drops；ACK 送出。

#### 12.18 接線 `ClientBlockAction`->`handle_client_block_action`
- [ ] 檔案：`src/state.rs:4034` drain 區
- 步驟：`ServerToHost::ClientBlockAction`->呼叫 `handle_client_block_action`，取回 result，送 `HostToServer::SendBlockActionResult`。舊 `ClientBlockChange` 路徑改為只 host 仍可用的權威廣播（或移除，見 12.20）。
- 驗證：end-to-end 接線正確。

#### 12.19 Host 權威測試
- [ ] 檔案：`src/state.rs` 測試區
- 步驟：
  1. Break：harvest eligibility（正確工具／材質）、fortune、silk touch、XP、exhaustion、tool durability 正確。
  2. Place：reach+collision+support 驗證；成功 `consumed_item`。
  3. 失敗：`success:false`，不改 world、不廣播。
  4. 廣播 `BlockChange` 仍到所有 client（含非請求者）。
- 驗證：`cargo test --release` 全綠。

### 階段五：收尾

#### 12.20 移除／重塑舊 `RequestBlockChange` 路徑
- [ ] 檔案：`src/state.rs`、`src/network/*`
- 步驟：client->host 改用 `BlockActionRequest`；舊 `RequestBlockChange`/`ClientBlockChange` 移除，或保留作 host->client 權威 `BlockChange` 廣播。確認無死碼警告。
- 驗證：`cargo check --release` 無新警告。

#### 12.21 既有測試不回歸
- [ ] 步驟：確認 placement collision、support、authenticated-id、multiplayer block sync 測試仍通過（`state.rs:12065/12095` 系列）。
- 驗證：`cargo test --release` 全綠。

#### 12.22 格式／編譯／測試閘門
- [ ] 步驟：`cargo fmt --all -- --check`、`cargo check --release`、`cargo test --release`。
- 驗證：三者通過。

#### 12.23 更新文件
- [ ] 檔案：`ARCHITECTURE.md`、`track.md`、`plans/progress.md`、`plans/implementation/10_bug_audit.md`
- 步驟：多人段落說明權威 action-result 流程、`ItemWire`、ACK 機制；G1 標記已修復。
- 驗證：文件與實作一致。

#### 12.24 人工驗收
- [ ] 步驟：互動式雙視窗 Host+Join 生存放置（消耗物品）與破壞（獲得掉落物）。
- 驗證：放置扣物品、破壞掉物、工具耐久損耗、經驗獲得、失敗不副作用。

#### 12.25 Commit
- [ ] 步驟：單一 commit `feat(network): authoritative block action-result for joined clients`，只 stage 本任務檔案。
- 驗證：`git diff --check` 通過。

## 驗收條件（對應計畫驗證清單）
- [ ] `ItemWire <-> ItemStack` roundtrip（含附魔、藥水、耐久、自訂名稱）。
- [ ] Protocol v5 `BlockActionRequest`/`BlockActionResult` roundtrip。
- [ ] 舊協議（v4）client handshake 被拒。
- [ ] Host break：harvest eligibility、fortune、silk touch、XP、exhaustion、tool durability 正確。
- [ ] Host place：reach + collision + support 驗證；成功 `consumed_item`。
- [ ] Host 驗證失敗：`success:false`，不修改 world、不廣播。
- [ ] Client 收到 `success+consumed_item`：消耗一個 selected item。
- [ ] Client 收到 `success+drops`：drops 入背包；滿時 spawn 本地 drop。
- [ ] Client 收到 `!success`：不消耗物品、不生成掉落物。
- [ ] Host 廣播 `BlockChange` 仍正確到達所有 client（含非請求者）。
- [ ] 既有 placement collision、support、authenticated-id 及 multiplayer block sync 測試不回歸。
- [ ] `cargo fmt --all -- --check`、`cargo test --release`、`cargo check --release`。
- [ ] 人工 Host + Join 生存模式放置（消耗物品）與破壞（獲得掉落物）。
