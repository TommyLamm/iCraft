# 07 — Save worker 無 producer 封套與多餘 job id

狀態：已完成。基線：`83e751d`，2026-09-17。
前置：02。

## 定位與判定

SavePayload::PlayerFile（save_worker.rs:33）無 producer，worker :218、ack :63、runtime.rs:1046 消費分支僅構成死鏈。

實際 player save 在 runtime.rs:818 save_all_inner 同步執行 save_player 並清 dirty。一般 Chunks／Entities／SidecarGroup job id 只透傳，consumer 用 .. 忽略；Barrier id 在 save_worker.rs:133 有真實匹配。

## 實作步驟

1. 刪 PlayerFile payload、ack、worker match、runtime 消費分支及僅服務此路徑的程式。
2. 刪一般 Chunks／Entities／SidecarGroup 的 job_id 配置與透傳；保留 Barrier job_id。
3. 保留 chunk dirty revision、entity epoch 確認，以及真實同步 player persistence。
4. 修正「player dirty 於 worker ack 清除」錯誤註解；不在刪碼包偷偷把 player 改成非同步。
5. 刪 clear_block_revision 等無 caller helper 前再核對測試；session_revision 已由 02 處理，不能誤刪產品測試。
6. 更新 ARCHITECTURE 的 worker ack 職責。

## 驗證與驗收

既有：save_all_persists_only_dirty_resident_columns、save_failure_is_reported_and_retry_keeps_original_path、autosave_error_does_not_skip_shutdown_flush、leave_save_failure_still_releases_identity、authority_gameplay_round_trips_through_dedicated_player_save、save::tests::stale_ack_cannot_clear_a_newer_dirty_revision。

本次未找到專門的 worker barrier／entity-epoch race 測試；實作時若改到相關確認流程，補真實 producer→worker→ack 案例，不能稱已有覆蓋。完成後每個 payload variant 均有正式 producer。

wait_barrier timeout／disconnect 的回傳語意另記於 audit.md，屬持久化正確性後續，不混計成刪碼收益。

## 實作紀錄

- 狀態：已完成。
- 實際修改：
  1. `src/server_runtime/save_worker.rs`：刪除 `SavePayload::PlayerFile` 與 `SaveAck::PlayerFile` 變體；從 `Chunks`、`Entities`、`SidecarGroup` 中移除無用途的透傳 `job_id`，僅在 `Barrier` 保留真實匹配用的 `job_id`；更新 `save_thread_main` 處理分支。
  2. `src/server_runtime.rs`：移除 `save_authority_state` 中對一般 payloads 生成 `next_job_id()` 的冗餘呼叫；更新 `save_all_inner` 中 `wait_barrier` 的失敗比對（移除 `PlayerFile`，收斂 `SidecarGroup`）；更新 `apply_save_acks`（刪除 `PlayerFile` 分支，去除無效的 `..` 通配符）。
  3. `src/server_world/columns.rs`：刪除 `#[allow(dead_code)]` 且無任何 caller 的單點 `clear_block_revision`。
  4. `ARCHITECTURE.md`：更新 tick 階段與 Persistence 說明，明確記錄 worker ack 僅確認 chunk revision 與 entity epoch，player persistence 仍為 tick thread 同步寫入並立即清 dirty，一般 payload 不再帶 job id。
  5. 新增測試：在 `save_worker.rs` 補全 `worker_persists_chunks_entities_sidecars_and_barrier_in_order`，端到端覆蓋 `Chunks`、`Entities`、`SidecarGroup`、`Barrier` 每個變體的真實背景持久化、順序保證與 barrier 匹配。
- 驗證結果：
  - `cargo test --lib server_runtime::tests`：31 passed, 0 failed
  - `cargo test --lib server_runtime::save_worker::tests`：1 passed, 0 failed
  - `cargo test --lib save::`：50 passed, 0 failed, 2 ignored
  - `cargo test --test authority_persistence`：4 passed, 0 failed
  - `cargo check --all-targets --all-features`：exit 0，編譯警告由 30 降至 25（消除 5 個 `PlayerFile` 及未讀 `job_id` 警告）。
- 淨刪碼／保留原因：
  - 產品代碼淨刪除 58 行（死封套、無用途欄位與 dead API），新增 109 行真實 background worker 單元測試。
  - 保留 `SavePayload::Barrier { job_id }` 與 `wait_barrier` 供 shutdown / save_all 精確同步；保留同步 player persistence 保證玩家存檔可靠性。
