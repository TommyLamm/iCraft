# Plan16 — 測試腳手架與驗收測試收斂

## 定位

在測試系統與腳手架中，存在大量重複與死拓撲分支：

1. **`src/sim_harness.rs` (1,403 行) 與 `src/final_acceptance.rs` (861 行) 死分支清理**：
   - `final_acceptance.rs:L30-84`：`AcceptanceTopology::ListenServer` 與 `DedicatedTwoClients` 僅返回寫死的 blocked reason（`"Listen/Dedicated recipe rows are not this CPU physics smoke. Real TCP coverage lives in Plan30-34..."`）。
   - 真實的 listen/dedicated 網路覆蓋已完全由 `tests/plan30_real_transport_acceptance.rs` 與 `tests/review_hardening_*.rs` 接管。
   - 清理死拓撲變體與相關無效的分派路徑。

2. **`tests/` 跨檔案重複的 TCP 測試 Helper 收斂 (~100 行)**：
   - `plan30_real_transport_acceptance.rs`..`plan34` 中重複手寫了 `properties()`、`current_revision()`、`source()` 等測試 helper。
   - 統一收斂至 `tests/common/tcp_harness.rs`。

3. **`src/microbench.rs` 重複編譯聲明**：
   - `microbench.rs` 在 `src/lib.rs:L85` 與 `src/main.rs:L25` 各自被宣告為模組編譯了兩次。
   - 統一為單一導出路徑。

預期削減代碼 ~950 行。

## 前置

02、04 已完成。

## 精確 acceptance

- [ ] 移除 `final_acceptance.rs` 中永遠回傳 blocked 的死拓撲變體（`ListenServer`, `DedicatedTwoClients`）。
- [ ] 簡化 `sim_harness.rs`，消除未使用的輔助方法。
- [ ] 收斂 `tests/` 下重複的測試 helper 至 `tests/common/tcp_harness.rs`。
- [ ] 統一 `microbench.rs` 的模組宣告路徑。
- [ ] `cargo check --all-targets` 通過。
- [ ] 驗收測試與整合測試全數通過。

## 預計檔案與測試

- 修改：
  - `src/final_acceptance.rs`
  - `src/sim_harness.rs`
  - `src/lib.rs`
  - `src/main.rs`
  - `tests/common/tcp_harness.rs`
  - `tests/plan30_real_transport_acceptance.rs`
- 驗證測試：
  - `cargo test --test review_hardening_invariants -- --test-threads=1`
  - `cargo test --test plan30_real_transport_acceptance -- --test-threads=1`
  - `cargo check --all-targets`

## 建議階段

1. 移除 `final_acceptance.rs` 的死拓撲分支。
2. 統一 `tests/common/tcp_harness.rs` 的 helper。
3. 清理 `microbench.rs` 的雙重聲明。
4. 運行完整測試套件驗證。

## 不在本計劃

- 刪除真實的 TCP 整合測試用例。
