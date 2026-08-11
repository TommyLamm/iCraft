# Plan31 — authoritative block actions and mining completion

## 定位

- 這是 Plan30 驗收矩陣留下的後續缺口；本計劃尚未開始，不能由 fixture 或
  `set_block` 結果代替玩家輸入。
- 目標是讓 block-use／挖掘的請求、權威驗證、破壞進度、掉落與 owner/interest
  projection 都能由 Singleplayer、Listen TCP 與 Dedicated TCP 的同一條 ingress/egress
  路徑觀察。

## 精確 acceptance

- [ ] 為可表達的 block action 建立 typed request contract（座標、面、持有物、pose、
  client sequence/revision），保留 anti-stale、range、LOS、權限與 duplicate 語義。
- [ ] 權威固定 tick 管理 mining progress；取消、換手、離開 range、重複或 out-of-order
  不得重複破壞、掉落、XP 或 revision。
- [ ] 真 TCP Listen（local host + remote）與 Dedicated（兩個 remote clients）各跑同一
  bounded vector，確認 block/BlockEntity/drop/XP 的 owner-private 與 interest projection。
- [ ] save/reload、跨 chunk、重連後的 block state 與 progress 有 dated headless artifact。

## 預計檔案與測試

- `src/network/protocol.rs`、`src/authority/contract.rs`、`src/authority/mod.rs`、
  `src/server_world.rs`、`src/server_runtime.rs`（只在既有 seam 不足時）。
- `tests/common/tcp_harness.rs`、`tests/plan31_authoritative_block_actions.rs`、
  `tests/runtime_topology_parity.rs` 的共用 assertion。

## 不在本計劃

- 新方塊內容、renderer/voxel mesh、GPU/window/audio/DPI、完整 vanilla tool table，
  以及任何未被 Plan30 blocker 指出的 protocol bump。

