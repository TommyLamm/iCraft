# 2026-07-30 HKT 17:30 performance repair checkpoint

> 歷史 checkpoint：已由
> [`2026-07-30_completion.md`](2026-07-30_completion.md) 所記錄的後續工作取代。
> 本文只保留當時狀態，不應作為目前完成度判定。
>
> 範圍：任務 15 的 R5–R9
> 截止：依使用者要求，香港時間 2026-07-30 17:30 停止繼續實作並提交當時已有進度。

## R5：instrumentation、network budget、sim/redstone/entity

狀態：主要 runtime 修復完成；5.10 整合測試仍為 Partial，因此未將 R5 repair 文件標記為完成。

- GPU timestamp readback 使用明確狀態機與 capability gate；不支援時顯示 N/A。
- lighting 與 GPU upload 依來源分類，upload 記錄實際 bytes 與 elapsed time。
- queue telemetry 分 inbound、outbound、reliable、catch-up、save producer、save worker，並保留 240 幀 p95/p99。
- network drain 同時受 event、byte、elapsed-time budget 限制；reliable 使用跨幀 FIFO，pose/time/entity/health/effect 使用 latest-wins mailbox。
- headless harness 以 30/60/144/240 FPS 驅動同一固定輸入，校驗 blocks/state/light/fluid/redstone/entities/player/world time，並覆蓋高速碰撞。
- redstone sleep 追蹤 pressure-plate occupant set；differential oracle 不呼叫 production propagation。
- entity spatial/type index 增量維護；candidate query 改走 bucket/radius/type index，允許的全域 lifecycle loop 明確分類。
- 已有 timestamp state-machine、network burst/FIFO/latest、headless checksum、redstone sleep/oracle、entity index oracle 測試；尚缺實際 adapter/device `map_async` readback 測試，以及直接驅動完整 `State::drain_network_events` 的 burst 整合 artifact。

## R6：terrain arena、section mesh 與 culling

狀態：部分完成，未達 R6 驗收。

- 已有 generation/owner 驗證的 allocation token、free-list invariant/隨機測試、18³ section halo、section scheduler、保守 occluder allow-list、LOS snapshot DDA 與 fail-open 基礎。
- 尚未完成 runtime 全面切換至 section GPU mesh、frame-boundary staged compaction swap、完整 LOS identity/timeout 接線、section draw 與 F3 culling counters。

## R7：steady-state allocation 與 memory

狀態：部分完成，未達 R7 驗收。

- 已有 completion-protected bounded GPU frame-resource pool、hand base-mesh/uniform helper、particle RGBA/light instance、paletted storage demotion與六項 microbench 基礎。
- 尚未完成 State runtime 接線、實際 allocator instrumentation、hand shader uniform 路徑、safe-point demotion/F3 memory breakdown 與正式 microbench artifact。

## R8：frame pacing 與 dynamic resolution

狀態：部分完成，未達 R8 驗收。

- FPS cap/deadline 與 cap-change reset 已實作並有單元測試。
- dynamic-resolution controller 已有演算法基礎，但尚未接到 runtime；offscreen color/depth、upscale 與 native-resolution UI 尚未完成。

## R9：measurement、PGO 與文件

狀態：部分完成，未達 R9 驗收。

- 已建立 JSONL 驗證、manifest、run matrix 與 PGO 比較 PowerShell 工具。
- 尚未產出固定八場景 before/after raw artifact、完整 hardware manifest、RD16 報告與 PGO A/B 結果，因此不得宣稱效能提升百分比。

## 截止前可執行 gate

- `cargo fmt --all -- --check`：PASS。
- `cargo check --all-targets`：PASS（R6–R8 未接線基礎產生 dead-code warnings）。
- `cargo test --all-targets`：PASS；主程式 459 passed、2 ignored，整合測試 1 passed。
- `cargo test --all-targets --release`：PASS；主程式 459 passed、2 ignored，整合測試 1 passed。
- `cargo build --release`：由 release test 完成同一 release binary build/link。
- `cargo clippy --all-targets --all-features`：本機 rustup 顯示 clippy component 已安裝，但 binary 回報不適用於目前 stable toolchain；屬工具鏈限制，非程式碼 gate 通過。
