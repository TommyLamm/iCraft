# 任務 5：固定 simulation tick

> 對應計畫：`14_performance_optimization.md` Phase 2.1
> 狀態：✅ 已完成
> 前置：任務 1（基線）
> 目標：將 frame update 與 20 Hz 權威 world simulation 分離，確保遊戲行為不隨 FPS 改變。
> Commit 訊息：`perf(sim): split frame updates from fixed world and physics ticks`

## 相關程式碼位置（已核對）

- `src/app.rs` - `App::about_to_wait`：frame `dt` 來源，目前 cap 在 0.1 秒。
- `src/state.rs:5179` - `State::update`：目前以 frame `dt` 直接驅動所有系統。
- `src/physics.rs:205` - `PlayerPhysics::update`：目前以 frame `dt` 驅動。
- `src/mob.rs:287` - `update_mobs`：hostile AI/projectile。
- `src/passive_mob.rs` - passive mob AI。
- `src/redstone.rs:261` - `RedstoneSystem`：20 Hz scheduler（已部分固定）。
- `src/fluid.rs` - water 0.25s / lava 1.5s tick（已部分固定）。
- `src/weather.rs:56` - `WeatherSystem`：world tick 驅動。

## 子任務清單

### 5.1 Simulation accumulator
- [x] 檔案：`src/state.rs`、`src/app.rs`
- 步驟：
  1. `App` 保留真實 frame `dt`；`State` 維護 simulation accumulator。
  2. 權威 world simulation 固定 20 Hz（50 ms/tick）。
  3. 最多四個 catch-up tick；超出時保留有界 debt 而非無限追趕。
  4. `State::update` 拆分為 `State::tick_simulation`（固定 20 Hz）與 `State::update_frame`（每幀）。
- 驗收：低 FPS 時不無限追趕；高 FPS 時不過快執行模擬。

### 5.2 Player physics 固定 substep
- [x] 檔案：`src/physics.rs`、`src/state.rs`
- 步驟：
  1. Player physics 使用固定 60 Hz（或經基準選定的）substep。
  2. render/camera 使用 previous/current snapshot 插值，保持視覺平滑。
  3. 高速移動時確認碰撞 substep 不穿透。
- 驗收：30/60/144/240 FPS 下碰撞結果一致。

### 5.3 AI/spawning/random tick 改為 tick/秒語義
- [x] 檔案：`src/mob.rs`、`src/passive_mob.rs`、`src/boss.rs`、`src/weather.rs`、`src/state.rs`
- 步驟：
  1. AI、spawning、leaf random tick、redstone、fluid、weather accumulation 改為 tick/秒語義。
  2. 不再依 FPS 執行次數。
  3. 確認所有 timer/cooldown 以 tick 計算，不以 frame `dt` 累加。
- 驗收：leaf decay、mob attack、spawning、redstone 和流體速度不隨 FPS 改變。

### 5.4 純呈現工作保留 frame update
- [x] 檔案：`src/state.rs`、`src/particles.rs`、`src/camera.rs`
- 步驟：
  1. particles、camera、remote interpolation 等純呈現工作保留 frame update（用 frame `dt`）。
  2. 確認這些系統不影響權威世界狀態。
- 驗收：視覺平滑度不受 tick 分離影響。

### 5.5 Pause/death/network tick policy
- [x] 檔案：`src/state.rs`
- 步驟：
  1. pause 時停止 simulation tick 但保留 frame update（UI 動畫）。
  2. death screen 時停止 simulation tick。
  3. network-not-ready 時停止 simulation tick。
  4. 明確化並加入測試。
- 驗收：pause/death/network 斷線時模擬正確暫停。

### 5.6 World checksum 一致性測試
- [x] 檔案：`src/state.rs`（測試區段）
- 步驟：
  1. 建立固定 seed + 固定輸入序列的測試場景。
  2. 以 30/60/144/240 FPS 執行相同輸入，記錄 world checksum（block/light/entity 狀態雜湊）。
  3. 確認所有 FPS 下 checksum 一致。
- 驗收：30/60/144/240 FPS 下 world checksum 完全一致。

## 驗收條件

- [x] 30/60/144/240 FPS 下，同一輸入與 seed 的 world checksum 一致。
- [x] leaf decay、mob attack、spawning、redstone 和流體速度不隨 FPS 改變。
- [x] pause/death/network 斷線時模擬正確暫停。
- [x] 低 FPS 時有界 debt，不無限追趕。
- [x] `cargo fmt --all -- --check`、`cargo check --release`、`cargo test --release` 通過。

## 風險與回退

- 固定 tick 可能改變既有遊戲手感；以 world checksum 測試確保行為一致。
- physics substep 增加 CPU 成本；若基線顯示退化，調整 substep 頻率。
- 這是行為敏感改動，必須完整回歸測試。

## 驗證命令

```text
cargo fmt --all -- --check
cargo check --release
cargo test --release
cargo run --release   # 多 FPS 下視覺與行為驗證
```
