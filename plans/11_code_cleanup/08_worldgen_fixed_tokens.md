# 08 — Runtime worldgen 固定 token 與空 metrics

狀態：待執行。基線：`83e751d`，2026-09-17。
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

尚未執行；完成時記錄實際修改、驗證結果、刪碼量及文件更新。

