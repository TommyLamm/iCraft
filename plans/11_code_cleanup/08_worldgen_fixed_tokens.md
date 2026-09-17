# 08 — Runtime worldgen 固定 token 與空 metrics

狀態：已完成。基線：`83e751d`，2026-09-17。
前置：無；20 另處理真正的工作生命週期。

## 定位與判定

worldgen_worker.rs:49/50 將 generation／lifetime 初始化為 1；bump_generation／bump_lifetime（:54/59）全倉無 caller，也沒有其他 assignment。runtime.rs:947/948 帶入 job、worker :88/89 帶回、:111 is_current 永遠比較相同值。

每個 Runtime 新建獨立 channel；舊 Runtime drop 後 send 失敗，不會進新實例。正式「不要覆蓋已 materialize／restore failed 欄位」由 server_world/columns.rs:71 apply_generated_chunk 負責。

## 實作步驟

1. 刪 runtime worker/job/result 固定 generation／lifetime、unused bump／is_current，以及沒有可達 token 變化的 stale-discard 分支。
2. 刪對應固定零 worldgen_stale_discarded metrics 和測試預期／文件欄位。
3. Sender 本身可 clone，去掉 Arc<Sender<_>> 的多餘一層。
4. 只處理 runtime worldgen，桌面 mesh generation／lifetime 會真實改變，不在本包刪除。
5. 修正 ARCHITECTURE 對 runtime stale token 的錯誤敘述；需求撤回與 completed 排程另見 20，不以刪 token 假裝已解決。

## 驗證與驗收

本次全倉未找到直接驗證這套 token 的測試。新增真正的行為案例：同步 materialize 並改動的欄位不被晚到 result 覆蓋；failed restore 欄位拒絕生成結果；不同 runtime channel 不串結果。

沿用 chunk restore／residency 與 dimension interest 測試。完成後沒有恆定 token 及永不變化的 stale 指標；仍保留已載入欄位保護。

## 實作紀錄

- 修改內容：
  - `src/server_runtime/worldgen_worker.rs`：移除 `WorldgenJob` / `WorldgenResult` / `WorldgenWorker` 上的 `generation` 與 `lifetime` 欄位；移除無 caller 的 `bump_generation`、`bump_lifetime` 與恆為真的 `is_current`；將 `result_tx` 由 `Arc<mpsc::Sender<WorldgenResult>>` 簡化為 `Sender<WorldgenResult>`（利用其內建的 `Clone`）；新增 `worker_channels_are_isolated_between_instances` 單元測試。
  - `src/server_runtime.rs`：自 `ServerMetrics` 移除固定為零的 `worldgen_stale_discarded`；在 `schedule_pending_worldgen` 移除 job 上的 generation/lifetime 賦值；在 `collect_worldgen_results` 移除無效的 `!is_current` 丟棄檢查與指標累計。
  - `ARCHITECTURE.md`：修正 runtime worldgen 與 network/mesh 的描述，指出 runtime worldgen 不攜帶 generation/lifetime，其防止過期覆蓋的保護由 `apply_generated_chunk` 依欄位是否已常駐或 failed restore 處理；保留桌面 mesh 的 SectionIdentity generation/lifetime/revision 機制。
  - `src/server_world/tests.rs`：新增 `materialized_mutated_column_rejects_late_worldgen_result` 與 `failed_restore_column_rejects_worldgen_result` 測試。
  - `src/server_runtime/tests.rs`：新增 `runtime_instances_have_isolated_worldgen_channels`、`materialized_mutated_column_survives_runtime_worldgen_collection` 與 `failed_restore_column_rejects_runtime_worldgen_collection` 測試。
- 實際命令與結果：
  - `cargo test --lib server_runtime::`：exit 0，36 passed, 0 failed。
  - `cargo test --lib server_world::`：exit 0，31 passed, 0 failed。
  - `cargo check --all-targets --all-features`：exit 0。
- 淨刪碼／保留原因：
  - 產品代碼淨刪除 39 行死 token、無用方法與 Arc 包裝。桌面 mesh 相關的 generation、lifetime 與 SectionIdentity 因在渲染生命週期中具備動態演進與淘汰邏輯，完全保留不更動。新增共 6 個保護行為測試覆蓋隔離與覆寫保護。

