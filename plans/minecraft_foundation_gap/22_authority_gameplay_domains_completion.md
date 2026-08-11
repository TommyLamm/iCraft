# 22 — Plan21 玩法權威域完成

## 定位

- 優先級：P3，承接 Plan21 Phase B；前置提交：`78bd2f5`
- 本計劃只完成既有 fishing、workstation transaction 與 combat lifecycle 的 headless authority 閉環；
  不處理 State/Join Client UI 接線、GPU、30 分鐘 soak 或新的玩法內容
- 建議提交上限：1；完成後才可開始 listen/State/topology 後續計劃

## 完成證據

- protocol v16 已有 `Fishing`、`FurnaceTakeOutput`、`Craft`、`Enchant`、`Brew`、`Anvil`、
  `Combat` 與 `UseState` typed operations。
- `authority/{fishing,transactions,combat}.rs` 已提供純規則與 rollback 測試，
  `AuthorityCore` 也已接入基本 dispatch。
- `tests/authority_gameplay_domains.rs` 以共同 dedicated headless vector 證明 fishing fixed-tick
  lifecycle、brew ready/take、combat shield/armor durability、knockback、death drop/XP/
  keep-inventory 與 respawn 全部寫回 session/world delta。

## A. Fishing lifecycle

- [x] cast/reel/cancel 只由已認證 session 的 `GameplayOperation::Fishing` 變更；驗證 hand、rod、
  look、維度、距離與狀態，拒絕不得消耗耐久或產生 hook/loot。
- [x] fixed tick 驅動 hook 落水、等待、bite、timeout；reel 的 loot、XP、rod durability、hook despawn
  與 session revision 原子提交並可由 snapshot 觀察。
- [x] duplicate/out-of-order/stale request 不得重複 loot 或耐久消耗。

## B. Workstation transactions

- [x] furnace output+XP、craft、enchant、brew start/tick/take/cancel、anvil 都從 exact rich
  `ItemStack` identity 驗證後原子提交；count、durability、enchantment、potion、custom name、
  can-break/can-place-on metadata 守恆。
- [x] station block、距離、slot bounds、brew locks、inventory capacity、levels/XP 任一失敗時，
  session、block entity、revision 均不變。
- [x] fixed tick 會推進 brew 到 ready，完成後才可 take；disconnect/reconnect 的 session snapshot
  不會複製或遺失 reserved inputs。

## C. Combat/death/respawn lifecycle

- [x] server 根據 authoritative pose/cooldown/held equipment 計算攻擊；client 不可自報 damage。
- [x] armor/shield mitigation 與 durability、knockback、damage source、health/effects 寫入 session delta；
  entity target 寫入 headless world delta。
- [x] 玩家死亡依 `keep_inventory` 原子產生 item/XP drops 或保留 inventory，並記錄 death source；
  respawn 恢復 health/hunger/velocity、清理死亡狀態且不重複 drops。
- [x] PVP、Creative/Spectator、wrong dimension、out-of-range、duplicate/stale 拒絕無部分副作用。

## 驗證閘門

- [x] `cargo test --lib authority::fishing::tests`
- [x] `cargo test --lib authority::transactions::tests`
- [x] `cargo test --lib authority::combat::tests`
- [x] `cargo test --lib authority::tests`
- [x] 新增 `tests/authority_gameplay_domains.rs` headless integration vector，覆蓋 A–C 的
  accepted/rejected/duplicate/stale 與 snapshot delta（3 tests）。
- [x] `cargo fmt --all -- --check`、`cargo check --lib --bins`、
  `cargo check --release --locked`、`git diff --check`

## 驗證紀錄

- 2026-08-11：authority fishing 7/7、transactions 8/8、combat 6/6、authority 14/14；
  `cargo test --test authority_gameplay_domains` 3/3 通過。
- 2026-08-11：`cargo fmt --all -- --check`、`cargo check --lib --bins`、
  `cargo check --release --locked` 與 `git diff --check` 通過（既有編譯 warnings 無失敗）。
- 同一 headless vector 驗證固定 20 Hz fishing bite/reel、furnace/craft/enchant/anvil、
  brew 200 tick ready 後 action=2 take、disconnect reservation cleanup、shield durability、
  entity/player death drops/XP、keep-inventory、respawn velocity/death-source clear 與
  duplicate/stale 無二次副作用。

## 不在本計劃

- State/NetworkClient 的 UI/input request 接線與 presentation projection。
- 三拓撲真 transport delivery、GPU/window/audio/DPI、30 分鐘 soak。
- 新增魚種、配方、裝備、附魔或其他內容目錄。
