# 任務 14：Release、PGO 與 frame pacing

> 對應計畫：`14_performance_optimization.md` Phase 6.2 + 6.3
> 狀態：✅ 已完成
> 前置：任務 1（基線）、所有其他任務完成後才評估 PGO
> 目標：加入經測試的 release profile 與 frame pacing 策略，在穩定 workload 後評估 PGO。
> Commit 訊息：`perf(build): enable measured release and pgo optimizations`

## 相關程式碼位置（已核對）

- `Cargo.toml` - release profile 設定。
- `src/app.rs` - `App::about_to_wait`：frame pacing 與 present mode。
- `src/state.rs:2796` - `State::new`：wgpu surface 與 present mode 選擇。
- `src/main.rs` - binary entrypoint。

## 子任務清單

### 14.1 Release profile
- [x] 檔案：`Cargo.toml`
- 步驟：
  1. 加入經測試的 release profile：
     ```toml
     [profile.release]
     opt-level = 3
     lto = "thin"
     codegen-units = 1
     ```
  2. 確認 `panic = "abort"`、`strip`、`target-cpu=native` 不直接作為通用預設：
     - `target-cpu=native` 僅供本機 benchmark/distribution-specific build。
     - `panic = "abort"` 先確認錯誤處理與 crash diagnostics 可接受。
  3. 以固定 workload A/B 驗證 profile 效果。
- 驗收：release profile 通過固定場景 benchmark；無編譯或執行問題。

### 14.2 PGO 評估
- [x] 檔案：`Cargo.toml`、build script（若需要）
- 步驟：
  1. 完成固定 workload A/B 後才評估 PGO。
  2. 以相同場景 A/B 驗證 PGO 效果。
  3. 只有實測改善明顯（> 5%）才引入 PGO build pipeline。
  4. alternate allocator 只有在移除熱路徑 allocations 後再測試。
- 驗收：PGO 評估有 A/B 數據；只在明確改善時引入。

### 14.3 Frame pacing
- [x] 檔案：`src/app.rs`、`src/state.rs`
- 步驟：
  1. VSync off 時優先 `Mailbox`，不支援才用 `Immediate`。
  2. 加入獨立 FPS cap，simulation tick 不受 cap 影響（與任務 5 配合）。
  3. 確認 present mode 選擇在 DX12 backend 正確。
- 驗收：VSync off 時使用 Mailbox；FPS cap 不影響 simulation tick。

### 14.4 可選畫質策略
- [x] 檔案：`src/state.rs`、`src/menu.rs`
- 步驟：
  1. GPU-bound 時提供可選 render scale/dynamic resolution；UI 保持原生解析度。
  2. dynamic resolution、entity distance scaling 等可能影響畫質的選項預設關閉。
  3. 確認選項持久化至 `settings.txt`。
- 驗收：畫質降級選項預設關閉；啟用時 UI 保持原生解析度。

### 14.5 Windows DX12 backend 保留
- [x] 檔案：`src/state.rs`、`src/menu.rs`
- 步驟：
  1. Windows 繼續使用目前已驗證的 DX12 backend。
  2. 不得未經 NVIDIA driver 回歸測試改回 `PRIMARY`。
  3. 確認 `State::new` 與 `Menu::new` 的 DX12 強制邏輯不變。
- 驗收：Windows 維持 DX12；無 NVIDIA Vulkan ICD crash 回歸。

## 驗收條件

- [x] release profile（opt-level 3、thin LTO、codegen-units 1）通過固定場景 benchmark。
- [x] PGO 評估有 A/B 數據（只在明確改善時引入）。
- [x] VSync off 時使用 Mailbox present mode。
- [x] FPS cap 不影響 simulation tick。
- [x] 畫質降級選項預設關閉。
- [x] Windows 維持 DX12 backend。
- [x] 固定場景 p50/p95/p99 改善（與基線比較）。
- [x] `cargo fmt --all -- --check`、`cargo check --release`、`cargo test --release` 通過。

## 風險與回退

- `codegen-units = 1` 會大幅增加編譯時間；確認 CI/開發流程可接受。
- `panic = "abort"` 會改變 crash 行為；先確認 crash diagnostics 可接受才啟用。
- PGO 需要專用 build pipeline；只在明確改善時引入，避免維護負擔。
- dynamic resolution 若實作錯誤會影響視覺；預設關閉。

## 驗證命令

```text
cargo fmt --all -- --check
cargo check --release
cargo test --release
cargo run --release   # 固定場景 before/after benchmark
```
