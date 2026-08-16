# Plan02 — 容器點擊改為 session 守恆交易

## 定位

- `GameplayOperation::ContainerClick` 的 `dragged: Some(item)` 把客戶端 `ItemWire`
  直接寫進 Chest／Furnace／Hopper／Dispenser／Dropper；`dragged: None` 抽出一格後丟棄。
- 沒有對 `SessionGameplayState` 借貸，沒有 brew-lock，沒有和游標比對。
- `container_sessions.rs` 已有 `simulate_container_click`，權威 dispatch 沒用它。
- Plan34 鎖的是**破壞**容器的掉落守恆，不是 click。`tests/headless_server_authority.rs`
  還用 dragged Stone 當成功案例。

## 前置

無。建議與 01 分開提交，避免同一 PR 同時改兩個 envelope。

## 精確 acceptance

- [ ] Click 只接受「目前 session 游標／熱鍵欄上已存在、metadata 完全相同」的 stack。
  客戶端自撰 `ItemWire`（例如鑽石）→ `InvalidState` 或 `PermissionDenied`，容器與物品欄不變。
- [ ] 一次 click 是單一交易：複製 session 物品欄 + 複製容器槽，跑 `simulate_container_click`
  （或等價），兩邊都成功才 `set_session_gameplay` + 寫回 block entity 並 bump 容器 revision。
- [ ] `dragged: None` 的抽出必須進 session 游標或物品欄；不得把物品從世界裡蒸發。
- [ ] Brew 鎖定的瓶子／材料不得被 click 抽走或覆寫（沿用 `ItemUse` 已有的 lock 謂詞）。
- [ ] 非 viewer → `PermissionDenied`，容器不變（既有檢查保留）。
- [ ] 測試：`player + container + cursor` 在 accept 與 reject 後總量不變；
  含左鍵、堆疊交換、空槽放入、滿物品欄抽出失敗。
- [ ] Plan34 破箱守恆仍通過。頭測改為伺服器先種箱子內容，再讓客戶端 click 真實持有物。

## 預計檔案與測試

- 修改：`src/server_world.rs`（`dispatch_container` Click 臂、`replace_container_slot`、
  `extract_container_slot`）、`src/authority/mod.rs`（若交易應升到 core，與 mining／craft 同級）、
  `src/container_sessions.rs`（只在現有 simulate 不夠時）。
- 修改：`src/server_runtime.rs` 的 `route_container_result`——必須把 session 物品欄投影
  與容器 slot 一起送出，不得只 echo 客戶端 payload。
- 測試：擴 `tests/plan34_container_break_inventory_conservation.rs` **或** 新檔
  `tests/review_hardening_container_click.rs`。必須走與 `NetworkServer` 相同的
  `ContainerClick` envelope，不能只測 typed 內部 helper。

## 建議階段

1. 讀 `simulate_container_click` 與 `SessionGameplayState::transact`，確認可復用的 commit 點。
2. 先寫「偽造鑽石寫入箱子」負向測試（現在會過，修完應 reject）。
3. 把 Click 臂改成 clone-commit；刪除或降為 test-only 的裸 `replace_container_slot`。
4. 改頭測種子。跑 Plan34 + 本計劃矩陣。

## 不在本計劃

- 新容器種類、雙箱 UI、shift-click 配方移動的完整 vanilla 矩陣（可先支援現有 simulate 行為）。
- Listen-host `BroadcastContainerSlotUpdate` 丟包（12）。
- Embedded 物品欄 UI 改送 click（06，依賴本計劃）。

## 實作與證據

- Click 改由 `AuthorityCore::apply_container_click` 做單一 clone-commit：複製
  `SessionGameplayState` + 容器槽，用 `simulate_container_click`，兩邊都成功才
  `set_session_gameplay` 並 `commit_container_item_slots`（寫回 block entity、
  bump 容器與世界 revision）。
- 客戶端 `dragged: Some(item)` 只當聲稱：必須與 session cursor 或熱鍵欄上
  metadata 完全相同的 stack 相符；否則 `RejectReason::InvalidState`。偽造鑽石
  不會寫進箱子。
- `dragged: None` 抽出進 session 物品欄；物品欄滿則整筆 reject，容器不變。
- Brew 鎖定沿用 `transactions::brew_locks_slot` / `preserves_brew_locks`。
  Brewing stand 不是 `ContainerAccess` 容器，對它 click 已 fail-closed
  （`InvalidState`）。
- 非 viewer 仍 `PermissionDenied`。`ServerWorld::dispatch` 的 Click 臂改回
  `Unsupported`，刪除生產路徑上的 `replace_container_slot` /
  `extract_container_slot`。
- `route_container_result` 投影權威容器槽 + session cursor，並送出
  `PlayerSessionUpdate`，不再 echo 客戶端 payload。
- 頭測改為伺服器先種箱子內容，再讓客戶端 click 真實持有物。

驗證：

```
cargo test --test review_hardening_container_click -- --nocapture
  9 passed

cargo test --test plan34_container_break_inventory_conservation -- --nocapture
  debug：3/4（authority 守恆三則通過）
  debug TCP listen 第一個 tick 6–11s > harness 5s，未進破箱斷言
  release：4 passed

cargo test --test headless_server_authority -- --nocapture
  debug：tcp_dispenser passed；two_clients 在認證等待超時
  release：two_clients passed（含伺服器種箱後 click 真實持有物與重啟守恆）

cargo check --all-targets
  ok
```
