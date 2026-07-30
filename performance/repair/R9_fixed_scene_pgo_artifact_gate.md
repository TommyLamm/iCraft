# 任務 15-R9：固定場景 before/after、PGO A/B 與 artifact gate

> 對應計畫：`15_performance_audit_repair_plan.md` 第 4.3 節與第 6 節完成定義
> 狀態：待修復
> 前置：R8（FPS cap/dynamic resolution）
> 目標：建立 8 個固定場景的 before/after artifact（含 CPU/GPU p50/p95/p99、1% low、working set、upload bytes、draw calls、buffer objects、save/network queue depth 與 delay、correctness checksum），視距 16 claims 用視距 16 量測，PGO 同 workload A/B 未達門檻不納入 pipeline，每份報告含 raw data 與完整硬體資訊，並重新驗收 01–14 文件狀態與 `performance_track.md`/`ARCHITECTURE.md` 一致。
> Commit 訊息：`fix(perf): fixed-scene before/after artifacts, pgo a/b and document re-acceptance`

## 相關程式碼位置（已核對）

- `performance/performance_track.md:10-26` - 任務總覽表，R9 完成後重新驗收狀態。
- `performance/performance_track.md:270-273` - 已知驗證限制，R9 補完。
- `performance/baselines/2026-07-28_windows_dx12.md:7-61` - 既有視距 8 baseline，不可作為視距 16 證據。
- `performance/15_performance_audit_repair_plan.md:335-346` - 第 6 節完成定義。
- `ARCHITECTURE.md` - 架構描述，需與文件狀態一致。
- `src/state.rs:3137-3179, 12963-12997` - GPU timestamp 量測來源（R5 修復後可用）。
- `Cargo.toml` / release profile - PGO 與 release 設定。

## 已確認的基線風險

- 沒有 8 個固定場景的 before/after artifact，無法審計性能百分比。
- 視距 16 的 claims 用視距 8 baseline 充數，證據不足。
- PGO 未做 non-PGO/PGO A/B，改善未達門檻就納入 pipeline 風險。
- 報告只有手寫摘要，缺 raw data 與完整硬體資訊。
- 01–14 文件狀態在 R0 回退為 Partial/Pending，需於 R9 重新驗收為 Complete（須滿足第 6 節全部條件）。

## 子任務清單

### 9.1 8 個固定場景 before/after
- [ ] 檔案：`performance/baselines/`、`performance/reports/`（新建）
- 步驟：
  1. 定義 8 個固定場景：開放地形、遮擋室內、快速飛行、紅石、流體、1,000 entities、autosave、多人加入。
  2. 每場景錄製 before/after：CPU/GPU p50/p95/p99、1% low、working set、upload bytes、draw calls、buffer objects。
  3. 每場景錄製 save/network queue depth 與 delay。
  4. 每場景錄製 correctness checksum（blocks/light/fluid/redstone/entities/player/world-time）。
  5. before 為 R0 回退後基線，after 為 R1–R8 修復後結果。
- 驗收：8 個固定場景均有可重播 before/after artifact。

### 9.2 視距 16 claims 用視距 16 量測
- [ ] 檔案：`performance/reports/`
- 步驟：
  1. 視距 16 的 claims 一律以視距 16 量測，不可用視距 8 baseline 充數。
  2. 標註既有 `performance/baselines/2026-07-28_windows_dx12.md` 為視距 8，不可作為視距 16 證據。
  3. 視距 16 量測涵蓋 8 個固定場景。
- 驗收：視距 16 claims 全部以視距 16 artifact 證明。

### 9.3 PGO 同 workload A/B
- [ ] 檔案：`performance/reports/`、`Cargo.toml`/build 設定
- 步驟：
  1. PGO 使用相同 workload 做 non-PGO/PGO A/B。
  2. 比較 CPU/GPU frame time p50/p95/p99 與 working set。
  3. 改善未達既定門檻就不納入 pipeline。
  4. A/B 結果含 raw data，記錄 profile collection 流程。
- 驗收：PGO A/B 有 raw data；未達門檻不納入 pipeline。

### 9.4 每份報告含 raw data
- [ ] 檔案：`performance/reports/`
- 步驟：
  1. 每份報告包含 raw data 或可重播輸出，不只手寫摘要。
  2. raw data 含每幀時間序列、counter 數列與 checksum。
  3. 提供重播指令與 seed。
- 驗收：每份報告可由他人重播重算。

### 9.5 完整硬體資訊
- [ ] 檔案：`performance/reports/`
- 步驟：
  1. 每份報告保存完整硬體資訊：CPU、GPU、RAM、driver、OS、wgpu backend、commit、settings、resolution、render distance。
  2. 硬體資訊與量測同時記錄，不可事後補猜。
  3. 不同硬體的報告分開標註。
- 驗收：每份報告硬體資訊完整且可追溯。

### 9.6 重新驗收 01–14 文件狀態
- [ ] 檔案：`performance/performance_track.md`、`performance/01_*.md`–`performance/14_*.md`、`performance/15_performance_audit_repair_plan.md`、`ARCHITECTURE.md`
- 步驟：
  1. 依第 6 節完成定義逐項驗收：P0/P1 問題全部修復並有防回歸測試。
  2. release 與 debug 全套測試穩定通過；`cargo fmt --check` 與 clippy 通過。
  3. 實際 GPU/window、host/client、slow-client、I/O fault-injection 場景完成。
  4. 存檔與 multiplayer checksum 無分歧。
  5. shader/CPU packed data 與視覺 golden parity 通過。
  6. 8 個固定場景具可重播 before/after artifacts。
  7. 所有數值改善宣稱能從 raw artifact 重算；沒有證據的百分比不得勾選。
  8. 滿足全部條件者才標 Complete，否則保持 Partial/Pending。
  9. 確認 `performance_track.md`、01–14 plans、總計畫與 `ARCHITECTURE.md` 狀態一致。
- 驗收：01–14 文件狀態與第 6 節完成定義一致，無虛假 Complete。

### 9.7 完成定義一致性檢查
- [ ] 檔案：`performance/15_performance_audit_repair_plan.md`、`performance/performance_track.md`
- 步驟：
  1. 對照總計畫第 6 節完成定義逐條勾稽。
  2. 更新總計畫第 1 節審核結論表為最終狀態。
  3. 更新 `performance_track.md` 任務總覽表狀態與驗證摘要。
  4. 記錄任何仍存在的限制（如未支援平台）。
- 驗收：總計畫、`performance_track.md`、01–14 與 `ARCHITECTURE.md` 完全一致。

## 驗收條件

- [ ] 8 個固定場景具可重播 before/after artifacts（CPU/GPU p50/p95/p99、1% low、working set、upload bytes、draw calls、buffer objects、save/network queue depth 與 delay、correctness checksum）。
- [ ] 視距 16 的 claims 用視距 16 量測。
- [ ] PGO 同 workload A/B；未達門檻不納入 pipeline。
- [ ] 每份報告含 raw data 或可重播輸出。
- [ ] 硬體資訊完整（CPU/GPU/RAM/driver/OS/wgpu backend/commit/settings/resolution/render distance）。
- [ ] 01–14 文件狀態依第 6 節完成定義重新驗收，無虛假 Complete。
- [ ] `performance_track.md`、01–14 plans、總計畫與 `ARCHITECTURE.md` 狀態一致。

## 本輪已產出的可審計 tooling（不等同於量測完成）

- `performance/benchmarks/r9-scenes.json` 固定八個 scene ID、seed、重播步驟與
  render distance 16。
- `performance/tools/Validate-R9Jsonl.ps1`、`Measure-R9Runs.ps1`、
  `Invoke-R9Matrix.ps1`、`New-R9Manifest.ps1` 與 `Compare-R9Pgo.ps1` 已通過
  `performance/tools/Test-R9Tools.ps1`；測試只驗證 schema、dry-run 與 fail-closed
  gate，沒有執行 GPU/window workload。
- `performance/reports/README.md` 與 `r9-report-template.md` 定義 raw JSONL、
  contemporaneous hardware manifest、重播指令及 PGO admission gate。
- `performance/baselines/README.md` 明確標註既有 2026-07-28 報告為視距 8
  歷史資料，不可支持視距 16 claim。

上述 tooling 不會填入或推導性能數字。本機續作已安裝 Rust
`llvm-tools-preview`，並產出
`performance/reports/2026-07-30-local-manifest.json`（DX12、1280×720、
RD16）。因目前尚未取得八場景 before/after、實際 GPU/window 或 PGO A/B raw
output，本輪所有驗收 checkbox 及 01–14 任務狀態仍維持
`Partial`/`Pending`，不得宣稱改善百分比。

## 風險與回退

- 固定場景量測受硬體/驅動變動影響；以同一硬體、同一 commit 量測，並記錄環境。
- PGO profile collection 可能不穩定；未達門檻則不納入 pipeline，保持 non-PGO release。
- 重新驗收可能發現部分任務仍無法 Complete；保持 Partial/Pending 並記錄缺口，不虛假勾選。
- 本輪不改 src 程式碼，僅量測與文件驗收；若有新缺陷，回到對應 R1–R8 輪次修復。

## 驗證命令

```text
cargo fmt --all -- --check
cargo test --all-targets
cargo test --all-targets --release
cargo build --release
cargo clippy --all-targets --all-features
# R9 gate 6 為 8 個固定場景 before/after 重播與文件一致性檢查：
cargo run --release   # 固定場景 1-8 before/after 量測，輸出 raw data 至 performance/reports/
# PGO A/B（視改善門檻決定是否納入 pipeline）
git diff --check      # 確認 performance/ 與 ARCHITECTURE.md 狀態一致、src/ 未動
```
