# Plan34 — container break inventory conservation

## 定位

- Plan31 完成 authoritative block break 與 block-entity create/remove，但實際審計確認
  `commit_mining_break` 只生成方塊本身的 drop；`set_block(..., Air)` 隨後移除 block entity，
  因而會遺失非空容器內容。
- 本計劃只補齊這個已確認的守恆缺口，不重做 Plan02／Plan26 的容器 UI、viewer 或雙箱生命週期。

## 精確 acceptance

- [x] 權威破壞 Chest、Furnace、Hopper、Dispenser、Dropper 前，先以完整 `ItemStack`
  metadata 擷取所有非空 slots；每個 stack 恰好轉為一個或多個總量相等的 DroppedItem，
  durability、enchantments、potion data、custom name 與 can-break/place metadata 不變。
- [x] 方塊 drop、容器內容 drops、block mutation 與 block-entity removal 形成單一可驗證 commit；
  失敗不得部分清空，duplicate/retry/stale 不得重複掉落。
- [x] owner 與 interest observers 收到一致的 item entity 與 `BlockEntityDelta(None)`；
  observer 不得收到 owner-private session payload。
- [x] 真 TCP Listen 與 Dedicated 各驗證至少一個非空容器，並以 authority 單元矩陣涵蓋
  Chest／Furnace／Hopper／Dispenser／Dropper；save→shutdown→reload 不得復活已掉落內容，
  reconnect／cached duplicate 不得改變總量。

## 預計檔案與測試

- `src/authority/mod.rs`、`src/authority/mining.rs`、`src/server_world.rs`（只在 atomic seam
  不足時）。
- `tests/plan34_container_break_inventory_conservation.rs` 與既有 TCP harness。

## 實作與證據（2026-08-13）

- `AuthorityCore::commit_mining_break` 在所有 session/tool/XP preflight 成功後，
  snapshot 目標 Chest/Furnace/Hopper/Dispenser/Dropper 的非空完整 `ItemStack`；
  block drops 與 container drops 一起預約全數 entity IDs，先 prepare 全部
  `DroppedItem`；任一 prepare 或後續 `set_block(..., Air)` 失敗都 rollback 已建立的
  entities 並保留來源 block entity，只有 Air mutation 成功後才提交 pending mutation
  與 session 結果。來源 block entity 未在 mutation 前 drain/clear。
- `tests/plan34_container_break_inventory_conservation.rs` 的 authority matrix 驗證
  五種容器、完整 metadata/count、BE removal、cached duplicate、stale request 與
  無重複掉落；同檔真 TCP Listen/Dedicated 驗證 owner/observer 的 EntitySpawn/State、
  BlockEntityDelta(None)、owner-private session isolation、reconnect 與 save/reload。
- Focused debug/release 結果記錄於
  `artifacts/plan34_20260813_verification.md`；未宣稱 repo-wide full suite。

## 不在本計劃

- 新容器種類、完整 vanilla 隨機散落速度、容器 UI／音效／動畫、hopper rewrite、
  renderer/GPU/window/audio/DPI、或與 container-break conservation 無關的 protocol bump。
