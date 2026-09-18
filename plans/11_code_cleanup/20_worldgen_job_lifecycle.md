# 20 — Worldgen demand／generating／completed 排程收斂

狀態：已完成。基線：`83e751d`，2026-09-17。
前置：08；建議 02 已完成。

## 定位與推論

worker 同時上限 32（worldgen_worker.rs:10）；authority 每 tick 最多套用 16（runtime.rs:69）。poll_completed（worker :97–103）立即移除 in_flight，但 demand 到 apply_generated_chunk（columns.rs:73）才移除。未套用結果仍留 AuthorityCore.pending_worldgen；下一 tick 又先 schedule 再 collect（runtime.rs:496/497）。

由控制流可推導：已完成但等待 apply 的欄位仍有 demand 卻不在 in_flight，可能重複生成。這是待確定性重現的推論，本次沒有執行負载量測。

## 實作步驟

1. 先建立超過 apply limit 的可控制完成序列，確認 completed backlog 是否被重新 schedule。
2. 設計一個以 (dimension,cx,cz) 為 key 的已提交生命週期，覆蓋 generating→completed→applied/discarded；只在最終消費才釋放 key，或讓 schedule 同時排除 completed backlog。選一份權威狀態，不維護三份互相修補的 set。
3. 需求撤回／session 離開後，generation 可以繼續完成但 result 不應重新 materialize 已無需求的欄位；於最終 apply 核對當前需求，明列與同步 materialize 的優先關係。
4. 同步 materialize／failed restore 的拒絕分支也要結束 job lifecycle；不因拒絕而永久佔用排程容量。
5. authority/mod.rs:175 apply_pending_worldgen 將 index vector＋Vec<Option<...>>＋deferred slots 掃描改為直接 stable sort owned columns 後消費；維持 dimension/session/nearest ordering、同 key 原順序、apply budget 和 world 不存在時的原處理語意。
6. 刪重複 bookkeeping 和誤導的 still-demanded 註解，更新 ARCHITECTURE。
7. 08 移除的固定 token 不補回假防護；若需求生命周期需要識別重開任務，使用由真實狀態轉移推進的 identity。

## 驗證與驗收

新增確定性測試：超過16結果跨 tick 不重複 schedule；同步 materialize 修改不被晚到結果蓋掉；撤回需求不重新生成；跨維度同座標分開；apply 預算與穩定順序不變；拒絕結果釋放排程容量。

既有回歸：review_hardening_chunk_residency、review_hardening_chunk_restore、dimension_interest_and_session_transfer_are_isolated。

最終應能由測試追蹤每個 demand 至多一個未終結 job，並區分 queued／applied／discarded；只保留實際需要的指標。

## 實作紀錄

已完成：
1. `src/server_world/columns.rs` & `src/server_world/mod.rs`:
   - 定義 `WorldgenApplyOutcome { Applied, Discarded }`。
   - `apply_generated_chunk` 嚴格核對並移除 pending demand，若無 demand、已駐留或屬 failed restore 則安全返回 `Discarded`，不再強行 materialize 無需求欄位。
   - 提供 `is_chunk_demand_pending`、`withdraw_chunk_demand`、`prune_unkept_demands`。
2. `src/server_runtime/worldgen_worker.rs`:
   - 新增 `is_in_flight` 與 `in_flight_count` 查詢介面。
3. `src/authority/mod.rs`:
   - 重構 `apply_pending_worldgen`: 刪除 `order: Vec<usize>`、`slots: Vec<Option<...>>` 及二次掃描，直接以 `pending.sort_by_key` 進行穩定排序並原地消費，維持 apply budget (16)、session 距離優先級、同鍵 FIFO 穩定性與 missing world 延遲語意。
   - 新增 `is_worldgen_pending` 與 `pending_worldgen_count`。
4. `src/server_runtime.rs` & `src/server_runtime/ingress.rs`:
   - `schedule_pending_worldgen` 排除 `self.authority.is_worldgen_pending`，徹底解決超過 16 筆的 completed backlog 跨 tick 重複生成問題。
   - `evict_uninteresting_chunks` 與 `handle_leave` 在玩家離開或視野移出時立即呼叫 `prune_unkept_demands` 清理無需求 demand。
5. 文件更新：
   - `ARCHITECTURE.md`: 補充世界生成完整生命週期、backlog 防重排程與需求撤回丟棄機制。
6. 測試覆蓋：
   - 新增 7 個確定性測試（`authority::tests` 2 個，`server_runtime::tests` 5 個），涵蓋超過 16 筆結果跨 tick 不重複排程、同步 materialize 優先不被覆蓋、需求撤回不重新生成且晚到結果丟棄、跨維度同座標隔離、apply 預算與穩定排序、拒絕分支釋放容量。
   - 既有回歸測試全部通過：`review_hardening_chunk_residency` (4 passed)、`review_hardening_chunk_restore` (5 passed)、`dimension_interest_and_session_transfer_are_isolated` (1 passed)、`cargo test --lib authority` (76 passed)、`cargo test --lib server_runtime` (46 passed)、`cargo test --lib server_world` (31 passed)。


