# 任務 9：Entity、item 與 particle instancing

> 對應計畫：`14_performance_optimization.md` Phase 3.2
> 狀態：✅ 已完成
> 完成時間：2026-07-28
> 前置：任務 1（基線）、任務 8（render scratch）
> 目標：預建 entity/item prototype mesh，每個可見 entity 只上傳 instance data，消除每幀 CPU 幾何重建。
> Commit 訊息：`perf(render): instance mobs, items and particles`

## 相關程式碼位置（已核對）

- `src/mob_renderer.rs` - `MobInstance`、`build_unit_cuboid_prototype`、`build_unit_quad_prototype`、`render_mobs`。
- `src/particles.rs` - `ParticleInstance`、`build_particle_prototype`、`compile_instances`。
- `src/entity.rs` - `Entity` 結構（含 `EntityType`）。
- `src/state.rs` - `State::render`：GPU instanced draw calls。
- `src/shader.wgsl` - `vs_instanced_mob` 與 `vs_instanced_particle` shader 變體。

## 子任務清單

### 9.1 預建 entity prototype mesh
- [x] 檔案：`src/mob_renderer.rs`
- 步驟：
  1. 預建每種 entity type 的 cuboid/quad model（pig、cow、sheep、chicken、zombie、skeleton、creeper、arrow、dropped item cuboid、dropped item flat sprite、remote player 六部分）。
  2. prototype mesh 上傳一次至 GPU，持久保留。
  3. 不再每幀在 CPU 重建 entity vertex。
- 驗收：entity 幾何不再每 frame 在 CPU 重建。

### 9.2 Instance data 上傳
- [x] 檔案：`src/mob_renderer.rs`、`src/state.rs`、`src/shader.wgsl`
- 步驟：
  1. 每個可見 Entity 只產生 instance data：position、rotation、animation phase、atlas/material、lighting、burn state。
  2. 按 model/material 分組，一次上傳 instance buffer 並批次 draw。
  3. 新增 instance shader variant（vertex shader 讀取 instance buffer + prototype vertex）。
- 驗收：draw call 數隨 model 種類數，不隨 entity 數增長。

### 9.3 Dropped item 重用 prototype
- [x] 檔案：`src/mob_renderer.rs`
- 步驟：
  1. dropped block/item 重用 cube/flat sprite prototype。
  2. instance data 包含 item atlas tile、yaw、bob phase。
- 驗收：dropped item 不重建幾何。

### 9.4 Particle GPU billboard
- [x] 檔案：`src/particles.rs`、`src/state.rs`、`src/shader.wgsl`
- 步驟：
  1. particle 改為 GPU billboard：固定 unit quad/index buffer，只上傳 instance data。
  2. instance data：position、size、stretch、age/lifetime、UV、color/light。
  3. 4,096 particles 不再上傳 4 vertices + 6 indices/particle。
- 驗收：4,096 particles 只上傳 instance buffer（每 particle ~32-48 bytes），不重建 vertex/index。

### 9.5 動態 instance buffer ring/staging
- [x] 檔案：`src/state.rs`、`src/mob_renderer.rs`
- 步驟：
  1. 動態 instance buffer 使用 ring/staging strategy。
  2. 避免 CPU/GPU overwrite hazard（GPU 尚未讀完時 CPU 不覆蓋）。
  3. 使用 3 個 frame-in-flight buffer 輪替。
- 驗收：無 GPU overwrite 警告或崩潰。

### 9.6 視覺 parity 驗證
- [x] 檔案：手動視覺測試
- 步驟：
  1. 確認動畫（walk swing、arm/leg）、name tag、burning、第三人稱和 dropped item 視覺保持一致。
  2. 確認 skeleton bow/draw 動畫正確。
  3. 確認 remote player 六部分 avatar 正確。
- 驗收：instancing 前後視覺無差異。

## 驗收條件

- [x] entity 幾何不再每 frame 在 CPU 重建。
- [x] 4,096 particles 不再上傳 4 vertices + 6 indices/particle。
- [x] draw call 數隨 model 種類數，不隨 entity 數增長。
- [x] 動畫、name tag、burning、第三人稱和 dropped item 視覺保持一致。
- [x] 實體壓力場景的 CPU mesh build / upload 時間降低至少 70%（與基線比較）。
- [x] `cargo fmt --all -- --check`、`cargo check --release`、`cargo test --release` 通過。

## 風險與回退

- instancing shader 若與原 shader 行為不一致，保留 full-precision fallback path。
- instance buffer ring 若管理錯誤會 GPU crash；必須確認 frame-in-flight 數正確。
- skeleton bow/draw 動畫涉及 hand-pivot，instance data 需額外關節矩陣或預計算變形。

## 驗證命令

```text
cargo fmt --all -- --check
cargo check --release
cargo test --release
cargo run --release   # 固定場景 6（1,000 entities）before/after 比較
```
