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

- [x] `has_in_process_runtime()` 將為真的啟動路徑（Singleplayer、Listen host）**不** `spawn_save_worker`、不為 presentation 建可寫入世界的 `SaveManager`、不啟動會寫 `mutation_revisions.bin` 的第二個 index。
- [x] Join client 維持現況：不建權威存檔工人。
- [x] 有 embedded runtime 時，手動存檔／結束遊戲／autosave UI 只走 `EmbeddedRuntimeBridge::save_all`（或等價的 runtime `save_all`）。不得再 enqueue 到空的 `SaveQueue`。
- [x] `NetworkSnapshotWorker`：
  - 若 Host catch-up 在 `has_in_process_runtime()` 時已不再讀它：不要建。
  - 若仍有呼叫點：保留，並在 rustdoc 寫「這是 catch-up 編碼，不是權威存檔」。
- [x] 無 runtime 的 leftover `LegacyOwner` 路徑若測試仍建構：可繼續建 `SaveQueue`。不要順便刪 leftover 存檔語意。
- [x] shutdown 仍做一次同步 `save_all`；失敗要表面化（現有 `save_error`／log 契約），不得改成吞掉。
- [x] `tests/authority_persistence.rs`、`tests/runtime_topology_parity.rs` 的存檔／重開期望不變。

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

## 實作與證據

執行前呼叫點：`State::new` 對非 Join 一律 `SaveManager` + `spawn_save_worker` + `spawn_network_snapshot_worker`。`trigger_background_save` 在 `has_in_process_runtime()` 立刻 no-op；`save_synchronously` 有 runtime 時已轉 `embedded_runtime.save_all()`。Join 已不建工人。`schedule_player_catchup`／`process_join_catchups` 在 `has_in_process_runtime()` 已 early-return（`state.rs` ~8944／8969），`allow_disk_fallback = !has_in_process_runtime()` 對 embedded 不可達。

改動：

- `State::new` 把工人建構收到與 Join 相同的閘門：`is_client || in_process_authority` 時 `save_manager`／`save_tx`／`network_snapshot_worker` 皆 `None`。`current_dimension`／`mutation_revisions` 初值對 embedded 不再從 presentation `SaveManager` 讀。
- leftover `LegacyOwner`（`!is_client && !in_process_authority`）仍建 `SaveQueue` + snapshot worker。leftover 存檔語意未刪。
- 手動存檔／QUIT／視窗關閉／`Command::SaveAll` 仍走 `save_synchronously` → `EmbeddedRuntimeBridge::save_all`。autosave UI 只在 `is_legacy_owner()` 呼叫 `trigger_background_save`；embedded 不會 enqueue 到空 queue。
- `NetworkSnapshotWorker`：**不**在 Singleplayer／Listen host 建構。型別與 leftover Host catch-up 呼叫點留下；rustdoc 寫明是 catch-up 編碼，不是權威存檔。未搬到 `network/`。
- leftover `switch_dimension` 讀 chunk 改 `save_manager.as_ref().and_then(...)`，避免 embedded 無 manager 時 panic。有 manager 時行為不變。
- `shutdown_network` 仍呼叫 `runtime.shutdown()`（內部一次同步 `save_all`）；失敗寫入既有 `save_error`。未吞掉。
- `ARCHITECTURE.md` 補上 embedded 不建 presentation 存檔工人；desktop `SaveQueue` 標 leftover。

測試（期望值未改）：

- `cargo test --lib save::` — 51 passed, 2 ignored
- `cargo test --test authority_persistence -- --test-threads=1` — 4 passed
- `cargo test --test runtime_topology_parity -- --test-threads=1` — 整檔在 `plan24_plan22_gameplay_vectors_match_all_runtime_topologies` 超時。已通過：`disabled_singleplayer_drains_local_request_through_fixed_tick_fifo`、`legacy_constructor_remains_dedicated_listen_runtime`、`listen_runtime_routes_local_response_to_tick_output`、`plan28_dispenser_item_projection_matches_all_runtime_topologies`、`world_player_storage_loads_and_rewrites_legacy_player_dat`（5 passed）。未跑完長測 `plan24_plan22_gameplay_vectors_match_all_runtime_topologies`。
- `cargo test --test review_hardening_embedded_presentation -- --test-threads=1` — 4 passed
- `cargo check --all-targets` — ok

缺口：leftover `LegacyOwner` `SaveQueue`／catch-up worker 仍在。`spawn_network_snapshot_worker` 未搬到 `network/`。長測 `plan24_plan22_gameplay_vectors_match_all_runtime_topologies` 未在本工作樹跑完。
