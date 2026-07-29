# 任務 15-R0：回退 01–14 文件狀態並保存失敗 reproduction

> 對應計畫：`15_performance_audit_repair_plan.md` 第 5 節 R0 與第 4.3 節第 1 點
> 狀態：Complete（2026-07-29；既有 fmt/clippy gate 缺口見下方執行紀錄）
> 前置：無（審核修復第一輪）
> 目標：依審核結論把任務 01–14 的文件狀態由虛假的 Complete 改回 Partial/Pending，保存目前已知失敗的 reproduction 與 artifact，並記錄驗證缺口，作為後續 R1–R9 修復的誠實基線。
> Commit 訊息：`docs(perf): revert 01-14 status to partial and save reproductions`

## 相關程式碼位置（已核對）

- `performance/performance_track.md:10-26` - 任務總覽表，目前 01–11 多數標為 Complete。
- `performance/performance_track.md:24-26` - 任務 12/13/14 已標 Pending，需核對其餘任務狀態。
- `performance/performance_track.md:270-273` - 已知驗證限制：尚未執行 GPU/window 視覺驗證、尚未建立固定場景或 before/after 報告。
- `performance/15_performance_audit_repair_plan.md:13-28` - 審核結論表，逐項 Fail/Partial 結論。
- `performance/baselines/2026-07-28_windows_dx12.md:7-61` - 既有基線 artifact 範圍。
- `performance/01_observability_baseline.md` 至 `performance/14_build_release.md` - 各任務詳細計畫內的勾選狀態需回退。
- `ARCHITECTURE.md` - 權威模型與架構描述，需與文件狀態一致。

## 已確認的基線風險

- 01–14 多數文件把子任務與驗收條件全勾 `[x]`，但審核發現 GPU timestamp readback 次序錯誤、mesh dirty 漏排程、save 非 atomic、network mailbox silent drop、async LOS 永遠 visible 等多個 correctness 缺陷，等於宣告完成但實際未驗收。
- 沒有可重播的固定場景 before/after artifact，視距 16 的 claims 用視距 8 baseline 充數。
- 若不先回退狀態，後續 R1–R9 修復將建立在虛假基線上，無法區分「新修復」與「從未完成」。

## 子任務清單

### 0.1 依審核結論回退 01–14 文件狀態
- [x] 檔案：`performance/performance_track.md`、`performance/01_*.md`–`performance/14_*.md`
- 步驟：
  1. 逐項對照 `15_performance_audit_repair_plan.md:13-28` 審核結論表，把 Fail 的任務（01/02/03/04/06/07/08/10/11/13/14）狀態改為 `Partial`，並在對應詳細計畫把驗收條件 `[x]` 改回 `[ ]`。
  2. Partial 的任務（05/09/12）保持 `Partial`，但補註審核指出的剩餘缺口（如 05 的合成測試、09 的 ring 無 completion 保護、12 的 storage 不 demote）。
  3. 在 `performance_track.md` 任務總覽表「狀態」欄統一用 `Partial`/`Pending`，移除未經 artifact 證明的 `Complete`。
  4. 在每份 01–14 詳細計畫開頭 metadata 加註 `審核回退：見 15_performance_audit_repair_plan.md`。
- 驗收：01–14 文件狀態與審核結論表完全一致，無殘留虛假 `Complete`。

### 0.2 保存現有失敗 reproduction 與 artifact
- [x] 檔案：`performance/baselines/`、`performance/repro/`（新建）
- 步驟：
  1. 建立 `performance/repro/` 目錄，收納目前可重現失敗的測試名稱、輸入 seed 與重現步驟。
  2. 把審核發現的失敗路徑（mesh mark_dirty 漏排程、save 非 atomic、mailbox drop、async LOS 永遠 visible、AO decode 不一致）逐項寫成 repro 條目，記錄對應 `file:line` 與觸發條件。
  3. 保留 `performance/baselines/2026-07-28_windows_dx12.md` 既有 raw data，標註其視距 8 限制，不可作為視距 16 claims 證據。
  4. 每條 repro 註明預期修復輪次（R1–R9）與目前驗證缺口。
- 驗收：`performance/repro/` 下每個審核發現都有對應條目，可由他人獨立重現。

### 0.3 記錄已知驗證缺口
- [x] 檔案：`performance/performance_track.md`、`performance/15_performance_audit_repair_plan.md`
- 步驟：
  1. 在 `performance_track.md`「已知驗證限制」小節擴充：列出缺少 GPU/window 視覺驗證、缺少固定場景、缺少 host/client checksum、缺少 fault-injection 等缺口。
  2. 對應每個缺口標註將由哪一輪（R5/R9 等）補完。
  3. 在總計畫第 6 節完成定義旁，補一個「目前缺口清單」對照表。
  4. 確認 `ARCHITECTURE.md` 的 host-authoritative 描述與文件狀態描述不衝突（multiplayer authority 缺口指向 R4）。
- 驗收：驗證缺口清單與審核結論一一對應，且每項有負責輪次。

### 0.4 統一狀態用語與交叉引用
- [x] 檔案：`performance/performance_track.md`、`ARCHITECTURE.md`
- 步驟：
  1. 統一狀態用語為 `Pending`/`Partial`/`Complete`，並定義：Complete 須同時滿足總計畫第 6 節全部條件。
  2. 在 `performance_track.md` 任務總覽表新增「審核修復輪次」欄，標註每個任務對應的 R1–R9。
  3. 確認 01–14 詳細計畫與總計畫的任務編號對應一致。
  4. 不改動任何 `src/` 程式碼，僅產出文件。
- 驗收：狀態用語一致、交叉引用雙向可追溯。

## 驗收條件

- [x] 01–14 文件狀態全數回退為 Partial/Pending，與審核結論表一致。
- [x] 各詳細計畫內虛假 `[x]` 全改回 `[ ]`。
- [x] `performance/repro/` 收納所有審核發現的重現條目。
- [x] 已知驗證缺口清單建立且對應到修復輪次。
- [x] `performance_track.md`、01–14 詳細計畫、總計畫與 `ARCHITECTURE.md` 狀態一致。
- [x] 未修改任何 `src/` 程式碼。

## 風險與回退

- 回退狀態可能讓既有 commit 顯得「倒退」；以審核結論為依據，誠實標註優於虛假 Complete。
- repro 條目若描述不夠精確，後續輪次難以驗證修復；每條必須含 `file:line` 與觸發步驟。
- 本輪純文件，無執行期風險。

## 驗證命令

```text
cargo fmt --all -- --check
cargo test --all-targets
cargo test --all-targets --release
cargo build --release
cargo clippy --all-targets --all-features
# R0 為純文件回退，gate 6 以 git diff 確認僅 performance/ 與 ARCHITECTURE.md 變動、src/ 未動：
git diff --stat -- src
git diff --stat -- performance ARCHITECTURE.md
```

## R0 執行紀錄（2026-07-29）

| 驗證 | 結果 | 備註 |
|---|---|---|
| 01–14 metadata/checkbox 結構檢查 | Pass | 14 份狀態均為 `Partial`、無殘留 `[x]`、都有審核回退連結 |
| 本地 Markdown links | Pass | `ARCHITECTURE.md` 與 `performance/**/*.md` 相對連結均可解析 |
| `git diff --check` | Pass | 僅有 Git 的 LF→CRLF working-copy 提示，無 whitespace error |
| `git diff --stat -- src` | Pass | 無輸出；本輪未修改 `src/` |
| `cargo test --all-targets` | Pass | 371 unit + 1 integration |
| `cargo test --all-targets --release -j 1` | Pass | 371 unit + 1 integration；平行首次編譯曾在 `flume` 無診斷退出，單 job 重跑通過 |
| `cargo build --release -j 1` | Pass | 完成，只有既有 dead-code warnings |
| `cargo fmt --all -- --check` | Fail（既有） | 未修改的 `src/culling.rs`、`main.rs`、`menu.rs`、`mob_renderer.rs`、`state.rs` 不符合 rustfmt；R0 不得改 `src/` |
| `cargo clippy --all-targets --all-features -j 1` | Fail（既有） | 5 errors：`src/network/transport.rs:58` 的 `never_loop`；`src/texture.rs:1235,1237,1513,1739` 的永遠為零運算，另有 268 warnings |

fmt/clippy 失敗不由 R0 文件變更引入，也不在本輪純文件授權範圍內；
它們保留為後續修復前的已知 gate 缺口，不得據此宣告 01–14 `Complete`。
