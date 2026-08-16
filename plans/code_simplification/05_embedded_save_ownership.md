# Plan05 — Embedded 不再建 desktop 存檔工人

## 定位

`ARCHITECTURE.md`：當 in-process `ServerRuntime` 擁有世界時，它是 `mutation_revisions.bin` 與世界寫入的唯一作者。`save.rs` 的 `SaveQueue` 是 desktop bounded latest-wins **leftover** worker。

現況：`State::new`（約 6277–6306）在 Singleplayer／Host 仍建構：

- presentation `SaveManager`
- `SaveQueue` + 背景 worker
- `NetworkSnapshotWorker`

然後 `trigger_background_save`／相關路徑在 `has_in_process_runtime()` 時立刻 no-op。`save_synchronously` 有 runtime 時已經轉呼叫 `embedded_runtime.save_all()`。

Join client 已經不建這套工人。Embedded 卻建了再停用，讀 `save.rs` 的人會以為 desktop worker 是活的權威路徑。

Listen-host catch-up 仍可能用 `network_snapshot_worker`（`state.rs` ~9153，`allow_disk_fallback = !has_in_process_runtime()`）。**本計劃不得在該呼叫點改掉之前刪 snapshot worker。**

## 前置

無。不要跟 10 一起搬 `State::new` 整段；本計劃只改「建不建工人」與對應 drop／shutdown。

## 精確 acceptance

- [ ] `has_in_process_runtime()` 將為真的啟動路徑（Singleplayer、Listen host）**不** `spawn_save_worker`、不為 presentation 建可寫入世界的 `SaveManager`、不啟動會寫 `mutation_revisions.bin` 的第二個 index。
- [ ] Join client 維持現況：不建權威存檔工人。
- [ ] 有 embedded runtime 時，手動存檔／結束遊戲／autosave UI 只走 `EmbeddedRuntimeBridge::save_all`（或等價的 runtime `save_all`）。不得再 enqueue 到空的 `SaveQueue`。
- [ ] `NetworkSnapshotWorker`：
  - 若 Host catch-up 在 `has_in_process_runtime()` 時已不再讀它：不要建。
  - 若仍有呼叫點：保留，並在 rustdoc 寫「這是 catch-up 編碼，不是權威存檔」。
- [ ] 無 runtime 的 leftover `LegacyOwner` 路徑若測試仍建構：可繼續建 `SaveQueue`。不要順便刪 leftover 存檔語意。
- [ ] shutdown 仍做一次同步 `save_all`；失敗要表面化（現有 `save_error`／log 契約），不得改成吞掉。
- [ ] `tests/authority_persistence.rs`、`tests/runtime_topology_parity.rs` 的存檔／重開期望不變。

## 預計檔案與測試

- 修改：`src/state.rs`（`State::new` 工人建構、`trigger_background_save`、`save_synchronously`、drop／shutdown）、必要時 `src/save.rs` rustdoc。
- 測試：
  - `cargo test --test authority_persistence -- --test-threads=1`
  - `cargo test --test runtime_topology_parity -- --test-threads=1`
  - `cargo test --test review_hardening_embedded_presentation -- --test-threads=1`
  - `cargo test --lib save::`
  - `cargo check --all-targets`

若 `runtime_topology_parity` 整檔過長，至少跑與 save／restart／legacy player.dat 相關的測試名稱，並在證據列出跑了哪些。

## 建議階段

1. 列出 `SaveQueue`、`SaveManager`、`spawn_save_worker`、`spawn_network_snapshot_worker`、`trigger_background_save`、`save_synchronously` 的所有呼叫點，標每個是 embedded／join／legacy。
2. 把 embedded 啟動的工人建構收到與 Join 相同的閘門後面。
3. 確認 quit／autosave UI 在 embedded 只呼叫 runtime `save_all`。
4. snapshot worker 依呼叫點去留，寫進證據。
5. 跑 persistence／topology 窄測試。

## 不在本計劃

- 把 `spawn_network_snapshot_worker` 從 `save.rs` 搬到 `network/`（證據可列，不要做）。
- 改 region 格式、inflate 上限、atomic replace。
- 刪 leftover `LegacyOwner` 存檔。
- 改 `ServerRuntime::save_all` 的 6,000-tick autosave 週期。
