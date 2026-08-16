# Plan06 — Embedded 表現層停止改世界

## 定位

- 現役 Singleplayer／Listen Host 都有 `embedded_runtime`。架構要求 renderer 只吃投影。
- `State::is_authoritative()` 只是 `!Client`。物品欄 UI（`handle_inventory_click`、
  `set_item_at_slot`）在 SP／Host 仍改 `ChunkManager` 容器槽與 XP，再呼叫
  `sync_authority_gameplay_from_local`，而後者**只**回寫 inventory + hotbar。
  箱子會被權威 snap-back → 刪物品或複製。
- 撿取（`state.rs` 約 12710）只看 `can_pickup`，會 `inventory.add_stack`、
  `add_experience`、刪本地 entity。換 hotbar 時把多出來的物品寫進權威，權威掉落物還在。
- Farmland 踐踏走 `apply_block_changes` → 對 embedded 變成玩家 `BlockUse`（01 關掉後
  會變 reject 或錯誤的 BlockUse）；join client 直接 `set_block` Dirt。
- `process_terrain_worker_results` 一律 `check_and_break_unsupported_for_loaded_chunk`，
  可在 presentation 生成掉落。
- `State::new` 對非 client 仍從磁碟物化 spawn halo + `player.dat`，與
  `ServerRuntime::new_embedded` 搶同一 `world_dir`。
- `sync_authority_gameplay_from_local` 穿過 `EmbeddedRuntimeBridge` 摸 `.authority`。

## 前置

- 01：物品欄以外的方塊寫入必須是 `BlockAction`，不能再靠 `BlockUse`。
- 02：容器 UI 必須能送會守恆的 `ContainerClick`。

若 01／02 未合併：停止實作，不要在 State 裡再做一套「本地模擬後再 sync」。

## 精確 acceptance

- [ ] `has_in_process_runtime()` 為真時：
  - 物品欄／容器／工作站 click 只送權威 op，等投影。不得 `set_item_at_slot` 寫
    chest／furnace／hopper，不得 `spend_levels`。
  - 撿取不得改 inventory／XP／entity。權威收集後投影 `SessionGameplayUpdate` +
    `EntityDespawn`。
  - 不得 `apply_block_changes`／`chunk_manager.set_block` 做 farmland 或 unsupported-break。
- [ ] Embedded 啟動：`ChunkManager` 與玩家表現從空開始，只靠 runtime 投影填入。
  不再在 `State::new` 對同一 `world_dir` `load_chunk_in`／`player.dat`（bonus chest
  寫入已跳過；連讀取也要停）。
- [ ] `sync_authority_gameplay_from_local` 改經 bridge 方法，參數只有 inventory +
  selected hotbar。刪除 health／hunger／mount 複製。`State` 不得再碰 `.authority`。
- [ ] Join client 路徑維持「不跑這條 writeback」（回歸：沒有 embedded 時不得呼叫它）。
- [ ] 測試（headless 或把 click／pickup 抽到可測 helper）：embedded `State` 縫上
  開箱 shift-click 與踩過掉落物之後，權威容器／entity／XP 與「只送 op」一致，
  且本地 `ChunkManager` 在投影前不變容器。若完整 `State` 構不過（wgpu），
  抽 `presentation_inventory_policy` helper 測閘門，並在 15 補真正 join／embedded 縫。

## 預計檔案與測試

- 修改：`src/state.rs`（`handle_inventory_click`、`set_item_at_slot`、撿取、farmland、
  `process_terrain_worker_results`、`State::new`、`sync_authority_gameplay_from_local`）、
  `src/app.rs`（若它無條件呼叫 writeback）、embedded bridge 所在檔
  （`state.rs` 或 `server_runtime.rs` 附近）。
- 測試：優先 `src/state.rs` 的 `#[cfg(test)]` 閘門；必要時新檔
  `tests/review_hardening_embedded_presentation.rs`（不建 wgpu）。

## 建議階段

1. 把所有 `is_authoritative()` 且會改世界／容器／物品的呼叫點列成表，逐條改成
   `has_in_process_runtime()` 則送 op。
2. 收窄 bridge API。
3. 拿掉 embedded 的磁碟 bootstrap。
4. 測閘門 + 跑既有 embedded FIFO／pause 測試。

## 不在本計劃

- Join client 停 worldgen（07）。
- 休眠的 Host catch-up／`NetworkHandle` 雙路徑（14）。
- 把 `state.rs` 拆檔。
