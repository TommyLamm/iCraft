# 任務 15-R5：instrumentation、network budget 與 sim/redstone/entity 修復

> 對應計畫：`15_performance_audit_repair_plan.md` 第 3.1、3.2、3.3 節
> 狀態：待修復
> 前置：R1、R2、R3、R4（P0 correctness 已修復）
> 目標：修復 GPU timestamp readback state machine、補完 lighting/gpu_upload scope 與 queue telemetry、令 network drain 真正跨幀 bounded、以 headless world harness 取代合成 fixed-tick test、修正 redstone sleep occupant 判斷與 entity spatial/type index 增量維護。
> Commit 訊息：`fix(perf): correct instrumentation, bounded network drain and sim/redstone/entity`

## 相關程式碼位置（已核對）

- `src/state.rs:3137-3179` - GPU timestamp readback `map_async`/`get_mapped_range` 路徑。
- `src/state.rs:12963-12997` - GPU timestamp query 建立/drain。
- `src/state.rs:2378-2555` - network drain budget 與 `try_iter().collect()` 全量 drain。
- `src/state.rs:4733-4803` - reliable/catch-up mailbox select branch。
- `src/state.rs:4968` - `update_chunks` 與 tick 排程。
- `src/culling.rs`、`src/mob.rs`、`src/passive_mob.rs`、`src/boss.rs` - entity AI/pickup/projectile 查詢。
- `src/world.rs` - redstone/pressure plate 與 component 掃描。

## 已確認的基線風險

- GPU timestamp readback 在 `map_async` 未完成或未 `device.poll` 前就 `get_mapped_range`，次序錯誤。
- `TIMESTAMP_QUERY` 與 `TIMESTAMP_QUERY_INSIDE_PASSES` 未區分，不支援時顯示假零值。
- lighting scope 只量測 load propagation，未涵蓋 block/fluid/weather/redstone mutation；gpu_upload 未涵蓋 particle/UI/camera/crack。
- queue counters 低報，未分 inbound/outbound/reliable/catch-up/save。
- `try_iter().collect()` 全量 drain 破壞跨幀 budget；pose/time burst 後最後值可能丟失。
- fixed-tick 驗收只是合成位移，不是完整 world checksum；redstone sleep 因永遠存在的 player occupant 失效；entity spatial index 每輪全量重建。

## 子任務清單

### 5.1 GPU timestamp readback state machine
- [ ] 檔案：`src/state.rs`
- 步驟：
  1. `src/state.rs:3137-3179` 建立 readback state machine：`map_async` 成功且 `device.poll` 完成後才呼叫 `get_mapped_range`。
  2. 追蹤每個 query set 的 map 狀態（`Unmapped`/`Mapping`/`Mapped`/`Consumed`），不可跨狀態搶讀。
  3. `src/state.rs:12963-12997` 的 query 建立/drain 配合 state machine。
  4. 不支援 timestamp 的 adapter 進入 `Unsupported` 狀態，顯示 N/A。
- 驗收：timestamp readback 不再次序錯誤；不支援時顯示 N/A 而非假零值。

### 5.2 區分 TIMESTAMP_QUERY 與 INSIDE_PASSES
- [ ] 檔案：`src/state.rs`
- 步驟：
  1. 查詢 adapter `TIMESTAMP_QUERY` 與 `TIMESTAMP_QUERY_INSIDE_PASSES` feature 支援。
  2. render/compute pass 內 timestamp 僅在 `INSIDE_PASSES` 支援時使用，否則該 pass timing 顯示 N/A。
  3. 不支援的 pass path 不可顯示假零值。
  4. 加入 adapter supported/unsupported state-machine 測試。
- 驗收：不支援的 pass timing 顯示 N/A，不誤報零值。

### 5.3 lighting/gpu_upload scope 補完
- [ ] 檔案：`src/state.rs`、`src/world.rs`
- 步驟：
  1. lighting scope 涵蓋 load、block、fluid、weather、redstone 等所有 mutation 路徑。
  2. gpu_upload scope 涵蓋 camera、UI、crack、particle、entity、terrain writes。
  3. 確認 scope 進入/退出配對，無洩漏。
  4. F3 顯示各子 scope 時間。
- 驗收：lighting/gpu_upload scope 涵蓋所有列舉 mutation/upload 路徑。

### 5.4 queue telemetry 分類
- [ ] 檔案：`src/state.rs`、`src/network/server.rs`、`src/save.rs`
- 步驟：
  1. 分開 inbound/outbound/reliable/catch-up/save producer/worker queue depth、bytes、drop/retry/cancel counters。
  2. F3 顯示各 queue 的 depth/bytes/drop。
  3. counters 與真實 backlog 一致，不再低報。
  4. 跨幀保留累計值供 p95/p99 計算。
- 驗收：F3 queue counters 與真實 backlog 一致，分類齊全。

### 5.5 移除全量 drain + reliable FIFO + per-key mailbox
- [ ] 檔案：`src/state.rs`
- 步驟：
  1. `src/state.rs:2378-2555` 移除 `try_iter().collect()` 全量 drain，改為 bounded per-frame drain。
  2. 可靠事件保存於跨幀 FIFO；pose/time 使用 per-key latest-wins mailbox。
  3. 超 budget 後 pose/time 最後值不可丟失（保留 latest）。
  4. budget 同時限制 events、bytes 與 elapsed time。
- 驗收：大 burst 下單幀處理量有界；所有 reliable packet 最終按序完成；pose/time 每個 key 最後值必定保留。

### 5.6 每幀先 network state 再 authoritative ticks
- [ ] 檔案：`src/state.rs`
- 步驟：
  1. 每幀先處理必要 network state，再進行 authoritative ticks（與 R4.6 一致）。
  2. network drain budget 與 tick 排程順序明確。
  3. budget 用完時 tick 仍可跨幀補，保留有界 debt。
- 驗收：network/tick 順序正確，budget 有界。

### 5.7 headless world harness 取代合成 fixed-tick test
- [ ] 檔案：`src/state.rs`（測試模組）、新增 harness 模組
- 步驟：
  1. 建立 headless world harness：固定 seed/input，無 GPU/window。
  2. 以 30/60/144/240 render FPS 驅動，比較 blocks/light/fluid/redstone/entities/player/world-time checksum。
  3. 取代目前只做合成位移的 fixed-tick test。
  4. 明確 player physics substep 頻率與 collision 上限，不以單次 50 ms step 代替驗收。
- 驗收：30/60/144/240 FPS 下世界 checksum 一致。

### 5.8 redstone sleep occupant set
- [ ] 檔案：`src/world.rs`、`src/state.rs`
- 步驟：
  1. redstone sleep 判斷只追蹤 pressure plate occupant set 的變化，不因永遠存在的 player occupant 失效。
  2. idle 時不掃全部 plate/component。
  3. differential test 使用獨立 reference implementation/fixture，非自身。
  4. sleep fast-path 在正常 gameplay 能生效。
- 驗收：redstone sleep 在有 player 時仍能 sleep；differential test 用獨立 reference。

### 5.9 entity spatial/type index 增量維護
- [ ] 檔案：`src/state.rs`、`src/mob.rs`、`src/passive_mob.rs`、`src/boss.rs`
- 步驟：
  1. entity spatial/type index 改為跨 bucket 移動的增量維護，entity 移動時更新 bucket。
  2. pickup、AI、partner search、projectile、melee、spawn 與 render 必須消費 bucket query。
  3. 禁止同一 tick 在 mob/passive/boss/state 多處全量 rebuild index。
  4. 使用 distance-squared 並重用 scratch vectors。
- 驗收：主要查詢走 bucket query，無每輪全量 rebuild。

### 5.10 instrumentation/預算整合測試
- [ ] 檔案：`src/state.rs`（測試模組）
- 步驟：
  1. timestamp supported/unsupported adapter state-machine 測試。
  2. network burst 下單幀處理量有界、reliable 保序、pose/time 最後值保留測試。
  3. headless harness 30/60/144/240 FPS checksum 一致測試。
  4. redstone sleep 與 entity index 增量測試。
- 驗收：全部 instrumentation/預算整合測試通過。

## 驗收條件

- [ ] GPU timestamp readback state machine 正確，不支援時顯示 N/A。
- [ ] lighting/gpu_upload scope 涵蓋所有 mutation/upload 路徑。
- [ ] queue telemetry 分 inbound/outbound/reliable/catch-up/save 且與真實 backlog 一致。
- [ ] 移除全量 drain；大 burst 下單幀處理量有界；reliable 保序；pose/time 最後值保留。
- [ ] headless world harness 30/60/144/240 FPS checksum 一致。
- [ ] redstone sleep 在 player 存在時仍生效；differential test 用獨立 reference。
- [ ] entity spatial/type index 增量維護，主要查詢走 bucket query。

## 風險與回退

- readback state machine 若增加幀延遲，以雙 buffer query set 輪替吸收。
- headless harness 需重現完整 tick 語義；若某些系統依賴 GPU，提供 CPU fallback 或標記為 harness 限制。
- entity index 增量維護若引入 bucket 一致性 bug，以全量 rebuild 作為測試 oracle 比對。

## 驗證命令

```text
cargo fmt --all -- --check
cargo test --all-targets
cargo test --all-targets --release
cargo build --release
cargo clippy --all-targets --all-features
cargo test --release -- timestamp scope telemetry network_budget headless redstone entity_index   # R5 instrumentation 與整合測試
```
