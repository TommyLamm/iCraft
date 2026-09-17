# 07 — Save worker 無 producer 封套與多餘 job id

狀態：待執行。基線：`83e751d`，2026-09-17。
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

尚未執行；完成時記錄實際修改、驗證結果、刪碼量及文件更新。

