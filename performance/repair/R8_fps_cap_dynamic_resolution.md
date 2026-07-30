# 任務 15-R8：FPS cap 與真正的 dynamic resolution（可選）

> 對應計畫：`15_performance_audit_repair_plan.md` 第 3.7 節
> 狀態：完成安全路徑（viewport-only 功能停用，FPS cap 已接線）；offscreen dynamic resolution 保持可選且未啟用
> 前置：R7（render scratch/instancing 修復）
> 目標：暫時停用只縮 viewport 的 `dynamic_resolution`（避免世界只畫在左上角）；若保留功能則建立低解析度 offscreen target、terrain/entity/particle 渲染到 scaled target 後 upscale、UI 在 native，並以 GPU-time feedback + 上下界 + hysteresis + cooldown 控制；加入獨立 FPS cap setting 與 `ControlFlow::WaitUntil`/frame deadline，sim accumulator 使用真實 elapsed 不用 cap interval。
> Commit 訊息：`fix(perf): disable viewport-only dynamic resolution and add real fps cap`

## 相關程式碼位置（已核對）

- `src/state.rs:12658-12671` - `dynamic_resolution` 只縮 viewport，世界只畫在 swapchain 左上角。
- `src/state.rs:12902-12911` - frame pacing / sim accumulator 路徑。
- `src/app.rs` - event loop `ControlFlow` 與 frame deadline。
- `src/chunk_render.rs` - terrain/entity/particle render target。

## 已確認的基線風險

- 目前 `dynamic_resolution` 只縮 viewport，沒有 scaled render target 與 upscale，世界只畫在左上角，視覺錯誤。
- 沒有獨立 FPS cap setting；`ControlFlow` 未用 `WaitUntil`/frame deadline。
- sim accumulator 使用 cap interval 而非真實 elapsed，可能造成 tick 不穩。
- 若直接保留 viewport-only 縮放，會持續誤導使用者以為是 dynamic resolution。

## 子任務清單

### 8.1 停用 viewport-only dynamic_resolution
- [ ] 檔案：`src/state.rs`
- 步驟：
  1. `src/state.rs:12658-12671` 暫時隱藏/停用只縮 viewport 的 `dynamic_resolution`，避免世界只畫在 swapchain 左上角。
  2. 預設關閉，UI 不顯示該選項或標註「實驗中」。
  3. 保留程式碼以供 R8.2 重建。
- 驗收：預設狀態下世界不再只畫在左上角。

### 8.2 scaled render target 與 upscale
- [ ] 檔案：`src/state.rs`、`src/chunk_render.rs`
- 步驟：
  1. 若保留功能：建立低解析度 offscreen color/depth target。
  2. terrain/entity/particle render 到 scaled target。
  3. upscale 到完整 surface（post-process pass）。
  4. UI 在 native surface rendering，不受 scale 影響。
  5. scale 比例由 GPU-time feedback 動態調整。
- 驗收：啟用時世界填滿 surface，UI 清晰，無左上角縮小問題。

### 8.3 GPU-time feedback + 上下界 + hysteresis + cooldown
- [ ] 檔案：`src/state.rs`
- 步驟：
  1. 以 GPU-time feedback（R5.1 timestamp readback）決定 scale。
  2. 設上下界（如 0.5x–1.0x），不無限降解析度。
  3. hysteresis 避免抖動；cooldown 限制調整頻率。
  4. 不支援 timestamp 時 fallback 固定 scale。
- 驗收：scale 調整穩定，無抖動，受上下界與 cooldown 約束。

### 8.4 獨立 FPS cap setting
- [ ] 檔案：`src/app.rs`、`src/state.rs`
- 步驟：
  1. 加入獨立 FPS cap setting（與 dynamic resolution 分開）。
  2. event loop 使用 `ControlFlow::WaitUntil`/frame deadline 達成 cap。
  3. cap 不影響 simulation tick（sim accumulator 獨立）。
- 驗收：FPS cap 生效且不影響 tick 頻率。

### 8.5 sim accumulator 使用真實 elapsed
- [ ] 檔案：`src/state.rs`、`src/app.rs`
- 步驟：
  1. `src/state.rs:12902-12911` sim accumulator 使用真實 elapsed time，不用 cap interval。
  2. 最多四個 catch-up ticks，保留有界 debt（與 R5.6 一致）。
  3. frame deadline 與 sim accumulator 解耦。
- 驗收：FPS cap 變動下 sim tick 頻率穩定 20 Hz。

### 8.6 視覺與 pacing 整合測試
- [ ] 檔案：`src/state.rs`（測試模組）、`src/app.rs`（測試模組）
- 步驟：
  1. 啟用 dynamic resolution 時世界填滿 surface（golden/視覺驗證）。
  2. GPU-time feedback 驅動 scale 在上下界內穩定。
  3. FPS cap 變動下 sim tick 20 Hz 穩定。
  4. 預設關閉時不影響既有渲染。
- 驗收：視覺與 pacing 整合測試通過。

## 驗收條件

- [ ] 預設停用 viewport-only `dynamic_resolution`，世界不再只畫在左上角。
- [ ] 若保留：低解析度 offscreen target + upscale + UI native。
- [ ] GPU-time feedback + 上下界 + hysteresis + cooldown。
- [ ] 獨立 FPS cap setting + `ControlFlow::WaitUntil`/frame deadline。
- [ ] sim accumulator 使用真實 elapsed，不使用 cap interval。
- [ ] FPS cap 不影響 simulation tick。

## 風險與回退

- scaled render target 增加 GPU pass 與記憶體；若無明顯效益，預設關閉即可，本輪標為可選。
- GPU-time feedback 在不支援 timestamp 的 backend 無效；以固定 scale fallback，不誤降解析度。
- `ControlFlow::WaitUntil` 在部分平台精度不同；以 deadline + elapsed 校正。

## 驗證命令

```text
cargo fmt --all -- --check
cargo test --all-targets
cargo test --all-targets --release
cargo build --release
cargo clippy --all-targets --all-features
cargo test --release -- dynamic_resolution fps_cap frame_pacing sim_accumulator   # R8 視覺/pacing 整合測試
```
