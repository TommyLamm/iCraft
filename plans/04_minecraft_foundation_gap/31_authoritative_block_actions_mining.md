# Plan31 — authoritative block actions and mining completion

## 定位

- 這是 Plan30 驗收矩陣留下的後續缺口；本計劃尚未開始，不能由 fixture 或
  `set_block` 結果代替玩家輸入。
- 目標是讓 block-use／挖掘的請求、權威驗證、破壞進度、掉落與 owner/interest
  projection 都能由 Singleplayer、Listen TCP 與 Dedicated TCP 的同一條 ingress/egress
  路徑觀察。

## 精確 acceptance

- [x] 為可表達的 block action 建立 typed request contract（座標、面、持有物、pose、
  client sequence/revision），保留 anti-stale、range、LOS、權限與 duplicate 語義。
- [x] 權威固定 tick 管理 mining progress；取消、換手、離開 range、重複或 out-of-order
  不得重複破壞、掉落、XP 或 revision。
- [x] 真 TCP Listen（local host + remote）與 Dedicated（兩個 remote clients）各跑同一
  bounded vector，確認 block/BlockEntity/drop/XP 的 owner-private 與 interest projection。
- [x] save/reload、跨 chunk、重連後的 block state 與 progress 有 dated headless artifact。

## 預計檔案與測試

- `src/network/protocol.rs`、`src/authority/contract.rs`、`src/authority/mod.rs`、
  `src/server_world.rs`、`src/server_runtime.rs`（只在既有 seam 不足時）。
- `tests/common/tcp_harness.rs`、`tests/plan31_authoritative_block_actions.rs`、
  `tests/runtime_topology_parity.rs` 的共用 assertion。

## 不在本計劃

- 新方塊內容、renderer/voxel mesh、GPU/window/audio/DPI、完整 vanilla tool table，
  以及任何未被 Plan30 blocker 指出的 protocol bump。
- 含 inventory 的 Chest／Furnace／Hopper／Dispenser／Dropper 被破壞時之內容物掉落守恆；
  現有 block-entity removal 只保證資料與 projection 移除，完整內容物守恆由 Plan34
  精確追蹤，本計劃不宣稱已完成。

## 完成記錄（2026-08-12）

本計劃已完整執行並全數通過驗證：

- protocol v18 的 typed `BlockAction`（StartBreak／CancelBreak／Place）與 owner-private
  `MiningProgressWire`；舊 v17 handshake 由既有 exact-version gate 拒絕。
- `AuthorityCore` fixed-tick mining、range／LOS／loaded chunk／expected block-state／exact held
  slot gate、Creative／Survival／Adventure policy、單次 block/drop/XP/tool durability commit。
- 修正 `commit_mining_break` 誤將 `spawn_dropped_item` 包裹在 release 模式下會被優化掉的 `debug_assert!` BUG，確保 Release 模式下挖掘掉落物 conservation 正常。
- 修正 `network/client.rs` 單元測試事件過濾器，忽略 `PlayerSessionUpdate` 以避免消耗非 typed event。
- 2026-08-13 corrective：恢復 Plan30 已定義的 duplicate response 邊界。Server 仍以
  byte-identical cached ACK 回應重送並計入 duplicate metric；`NetworkClient` 在同一
  request id 已成功進入可靠 app queue 後抑制 replay/rewrite。Plan31 TCP duplicate
  驗收改以 authority cache + metric 證明 idempotency，不再等待應被 gate 丟棄的第二份
  client-visible ACK。
- authority place 的 item-to-block exact mapping、Adventure `can_place_on`、inventory debit，
  以及 block-entity create/remove seam。
- State 已完整接入 typed Start／Cancel／Place，並正確投影 authority mining progress。

已驗證：

- `cargo fmt --all -- --check`：pass。
- `cargo check --release --all-targets`：pass。
- `git diff --check`：pass。
- `cargo test --release --test plan31_authoritative_block_actions -- --test-threads=1`：3/3 pass（Embedded, Listen TCP, Dedicated TCP 均通過）。
- `cargo test --release --lib -- --test-threads=1`：698/698 unit tests pass。
- 2026-08-13 corrective gates：response-gate lib/bin unit 各 1/1、
  `headless_server_authority` debug/release 各 2/2、Plan31 debug/release 各 3/3、
  Plan30 debug/release 各 2/2 通過。

Corrective acceptance 補驗證同一 bounded vector 中玩家位於 chunk `(0,0)`、mutation 位於
chunk `(1,0)`，以及未完成 Obsidian mining 經真 TCP disconnect 後，以同 username
reconnect 得到新 session 且 `mining=None`；詳見 dated artifact
`artifacts/plan31_20260812_verification.md`。
