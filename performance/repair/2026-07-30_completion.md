# 2026-07-30 performance repair continuation

> 基準 commit：`b13bbbb74325397d4a9334ba6158d895f222fbe7`
> 範圍：接續該 checkpoint 的 R5–R9 剩餘 runtime 與驗證工作。

## 已完成

- R5：修正同步與 Tokio channel 的 pre-account/rollback，統一
  inbound/outbound/reliable/catch-up/save queue telemetry；entity radius query
  直接使用座標 bucket，不再掃描所有 bucket。GPU timestamp 改為雙槽、帶
  submission tag 的 readback state machine；network tail 改為跨幀
  latest-wins staging。
- R6：runtime terrain ownership 切成 16³ section，worker 使用 18³ immutable
  halo，結果以 dimension generation、chunk lifetime 與 section revision
  驗證；draw/culling 以 section 為單位。LOS worker 使用相同 immutable
  snapshot 與 world/camera/entity identity，stale/timeout/overflow fail-open。
  Region free/draw 驗證 instance/token，active chunk 計數只接受 resident
  allocation；空且曾擴容的 arena 以每幀一個的上限安全重建。
- R7：terrain/entity instance slots 由 GPU completion 回收，避免覆寫
  in-flight buffer；held-item base mesh 依 item/model key 快取，walk/swing
  只更新 uniform；section storage 每幀最多 compact 四個，F3 使用實際
  representation memory，microbench 可由 `--microbench` 執行。
- R8：FPS cap/deadline 與 simulation elapsed 解耦。未實作 offscreen upscale
  前，viewport-only dynamic resolution 強制使用 native scale，避免只畫在左上角。
- R9：建立八場景 RD16 matrix、JSONL schema/summary/manifest/PGO fail-closed
  工具與報告模板，並加入 PowerShell 自測；已產出本機硬件 manifest，
  安裝 `llvm-tools-preview`，完成 CPU microbench 的 profile-generate、
  `llvm-profdata` merge 與 profile-use build。
- 相容性：程式可由目前 Rust 1.78 toolchain 編譯，不依賴較新的
  `Option::is_none_or` 或 const mutation。

## 驗證

- `cargo test --all-targets`：496 passed、3 ignored；integration 1 passed。
- `pwsh -NoProfile -File performance/tools/Test-R9Tools.ps1`：PASS。
- `cargo test --all-targets --release`：496 passed、3 ignored；integration
  1 passed。
- `cargo clippy --all-targets --all-features`：PASS（專案既有 warnings 未設
  deny，無 clippy error）。
- `cargo run --release -- --microbench`：PASS，輸出 12 項可重播 JSONL，
  覆蓋 storage representations、lighting、collision、section mesh、
  save flatten 與 network chunk flatten。
- `cargo fmt --all -- --check`、`cargo check --all-targets` 與
  `git diff --check`：PASS。

## 保留的外部 artifact gate

01–14 的狀態仍為 `Partial`。本機硬件 manifest 與 CPU microbench PGO profile
已存在，但尚未執行實機 GPU/window 固定八場景 before/after，也沒有符合
R9 schema 的 PGO A/B frame summary。R9 工具會對缺失資料 fail-closed；在這些
artifact 完成前不得填寫效能百分比或把整體狀態改為 `Complete`。
