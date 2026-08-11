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

## WIP 交接（2026-08-12）

本輪依使用者要求停止，功能尚未完成；提交基線為 Plan30 `f171fa4`，目前 checkpoint
已保存以下內容：

- protocol v18 的 typed `BlockAction`（StartBreak／CancelBreak／Place）與 owner-private
  `MiningProgressWire`；舊 v17 handshake 由既有 exact-version gate 拒絕。
- `AuthorityCore` fixed-tick mining、range／LOS／loaded chunk／expected block-state／exact held
  slot gate、Creative／Survival／Adventure policy、單次 block/drop/XP/tool durability commit。
- authority place 的 item-to-block exact mapping、Adventure `can_place_on`、inventory debit，
  以及 block-entity create/remove seam。
- State 已開始改走 typed Start／Cancel／Place，並投影 authority mining progress。

已驗證：

- `cargo fmt --all -- --check`：pass。
- `cargo check --all-targets`：pass（僅既有 warnings）。
- authority mining 5 tests、place 2 tests：7/7 pass。
- protocol typed block-action/mining projection roundtrip：1/1 pass。
- `tests/plan31_authoritative_block_actions.rs`：1/3 pass；embedded pass，Listen TCP 與
  Dedicated TCP 均在等待 request id 1 的 client-visible `GameplayResponse` 時 timeout。

下一位 agent 應先定位 TCP response timeout（不要用 direct authority response 取代），再完成：

1. owner-only progress／XP session projection與 observer no-leak；兩 client block/drop/BE delta。
2. duplicate、stale、out-of-order 真 TCP ingress；多 tick 後 Cancel 不被 progress revision
   自己擋成 stale。
3. reconnect 清空未完成 progress，以及 final block save→shutdown→reload。
4. integration 的 Place＋BlockEntityDelta create/remove；container inventory break conservation
   若不納入本批，新增精確後續 plan，不能宣稱已完成。
5. 更新本文件、README、ARCHITECTURE，並跑 serial debug/release full suites、release check、
   fmt/diff gates 後才可把 Plan31 標為完成。

Plan32 只讀審計已確認應拆為 portal/dimension authority（32A）與 progression
loot/completion（32B；dragon lifecycle 過大時再拆 32C）；不得用
`set_session_dimension`／fixture entity 冒充玩家 E2E。Plan33 fishing lifecycle 仍未開始。
