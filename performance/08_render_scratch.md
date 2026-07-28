# 任務 8：重用 frame scratch 與靜態快取

> 對應計畫：`14_performance_optimization.md` Phase 3.1
> 狀態：✅ 已完成
> 前置：任務 1（基線）
> 目標：消除穩態渲染的每幀 heap allocation，重用 terrain/draw/mob/particle/UI scratch buffer 並快取靜態資料。
> Commit 訊息：`perf(render): reuse frame scratch buffers and cache static ui/hand data`

## 相關程式碼位置（已核對）

- `src/state.rs:9003` - `State::render`：每幀重建 terrain candidates、LOD map、mob/particle/hand/UI mesh。
- `src/chunk_render.rs:303` - `DrawPlan`：draw 排序。
- `src/particles.rs:97` - `compile_mesh`：particle mesh 編譯。
- `src/mob_renderer.rs:170` - `render_mobs`：mob mesh 建構。
- `src/hand_renderer.rs` - first-person hand mesh。
- `src/menu.rs` - UI 文本與靜態 geometry。

## 已確認的基線風險

- `State::render` 每幀重新建立 terrain candidates、LOD map、mob mesh、particle mesh、hand mesh 和多組 UI Vec。

## 子任務清單

### 8.1 持久化 terrain/draw scratch storage
- [x] 檔案：`src/state.rs`、`src/chunk_render.rs`
- 步驟：
  1. 將 terrain candidates、draw plan、selected LOD scratch storage 變成 `State` 持久欄位。
  2. 逐幀 `clear()` 重用 capacity，不重新配置。
  3. 確認 `DrawPlan` 相關 Vec 都改為持久重用。
- 驗收：穩態渲染 terrain/draw 路徑零 allocation。

### 8.2 DrawCandidate 攜帶 LOD 與 distance key
- [x] 檔案：`src/chunk_render.rs`、`src/state.rs`
- 步驟：
  1. `DrawCandidate` 直接攜帶 LOD 和預先計算的 distance key。
  2. 移除 `selected_lods HashMap` 及 sort comparator 的重複距離計算。
  3. sort 只讀取 `DrawCandidate` 內的 key。
- 驗收：draw 排序不重複計算距離；`selected_lods HashMap` 移除。

### 8.3 持久化 mob/particle/UI scratch
- [x] 檔案：`src/state.rs`、`src/particles.rs`、`src/mob_renderer.rs`、`src/menu.rs`
- 步驟：
  1. mob/particle/UI vertex/index scratch storage 變成持久欄位。
  2. 逐幀 `clear()` 重用 capacity。
  3. 確認 `compile_mesh`（`particles.rs:97`）與 `render_mobs`（`mob_renderer.rs:170`）的輸出 Vec 改為傳入重用。
- 驗收：穩態 mob/particle/UI 路徑零 allocation。

### 8.4 快取 UI 文本與 debug labels
- [x] 檔案：`src/state.rs`、`src/menu.rs`
- 步驟：
  1. UI 文本 uppercase、debug labels 和靜態 UI geometry 按 dirty flag cache。
  2. 只有內容變更時重建。
  3. F3 debug 文本只在 counter 值變化時重建。
- 驗收：穩態 UI 文本不每幀重建。

### 8.5 Hand mesh 只在 held item 改變時重建
- [x] 檔案：`src/hand_renderer.rs`、`src/state.rs`
- 步驟：
  1. first-person hand 只在 held item/model 改變時重建基礎 mesh。
  2. 動畫（swing、位置偏移）改為 transform/uniform，不重建 vertex。
  3. 記錄 last held item，比較後決定是否重建。
- 驗收：手持物品不變時 hand mesh 不重建。

### 8.6 Allocation counter
- [x] 檔案：`src/perf.rs`、`src/state.rs`
- 步驟：
  1. 加入 allocation counter（穩態 render 出現配置即在 F3 標記）。
  2. 可用 `global_allocator` wrapper 或在關鍵路徑前後比較計數。
  3. F3 顯示 per-frame allocation 數。
- 驗收：穩態 gameplay render 接近零 allocation（F3 標記）。

## 驗收條件

- [x] 穩態 gameplay render 接近零 heap allocation。
- [x] terrain/draw/mob/particle/UI scratch 重用 capacity。
- [x] `DrawCandidate` 攜帶 LOD 與 distance key，移除 `selected_lods HashMap`。
- [x] hand mesh 只在 held item 改變時重建。
- [x] UI 文本按 dirty flag cache。
- [x] F3 allocation counter 顯示穩態接近零。
- [x] `cargo fmt --all -- --check`、`cargo check --release`、`cargo test --release` 通過。

## 風險與回退

- persistent scratch 若 `clear()` 後遺留舊資料，可能渲染錯誤幾何；必須確認所有 push 路徑正確。
- hand mesh cache 若動畫狀態判定錯誤，可能導致 hand 不更新；以 held item + animation phase 雙條件判斷。
- 本任務不改變視覺或遊戲行為。

## 驗證命令

```text
cargo fmt --all -- --check
cargo check --release
cargo test --release
cargo run --release   # 穩態場景 F3 allocation counter 驗證
```
