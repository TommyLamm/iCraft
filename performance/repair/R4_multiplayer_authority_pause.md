# 任務 15-R4：multiplayer authority 與 host pause policy

> 對應計畫：`15_performance_audit_repair_plan.md` 第 2.4 節與跨任務發現
> 狀態：待修復（release blocker，跨任務）
> 前置：R0（文件狀態回退）
> 目標：採用文件既定的 host-authoritative 模型，living entity 生命週期只在 host 執行，protocol 增加 entity spawn/state/despawn 與 player health/effect replication，client 只插值或做可回滾純視覺預測，pause 只 gate 本機 controller/UI，network state 在 catch-up simulation ticks 前套用。
> Commit 訊息：`fix(perf): host-authoritative entity lifecycle and pause policy`

## 相關程式碼位置（已核對）

- `src/state.rs:6517-6528` - client 自行 spawn/模擬 living entity 路徑。
- `src/state.rs:6598-6606` - client 承受本地 AI 傷害路徑。
- `src/state.rs:6848-6865` - pause/world clock 處理。
- `src/mob.rs:617-633` - mob AI/damage 在 client 執行。
- `src/network/protocol.rs:210-302` - 協定缺少 entity/health replication。
- `src/passive_mob.rs` - passive mob AI。
- `src/boss.rs` - boss AI。
- `src/app.rs` - pause menu 與 controller gating。
- `ARCHITECTURE.md` - host-authoritative 模型定義。

## 已確認的基線風險

- joining client 仍自行 spawn/模擬 living entities 並承受本地 AI 傷害，違反 `ARCHITECTURE.md` host-authoritative 前提。
- protocol 沒有 entity spawn/state/despawn 與 player health/effect replication。
- pause menu 或 host 死亡時暫停權威 world clock，導致多人 world time/redstone/fluid 停滯。
- network state 在 catch-up sim ticks 後才套用，可能用舊 remote pose/action 驗證。

## 子任務清單

### 4.1 host-authoritative entity lifecycle
- [ ] 檔案：`src/state.rs`、`src/mob.rs`、`src/passive_mob.rs`、`src/boss.rs`
- 步驟：
  1. living entity spawn、AI、damage、drops、breeding、despawn、persistence 只在 host `State` 執行；client 路徑（`src/state.rs:6517-6528`）移除本地 spawn/模擬。
  2. `src/mob.rs:617-633` 的 AI/damage 邏輯以 `if is_host` gate，client 不執行權威計算。
  3. passive/boss mob 同樣以 host-only gate 包住 spawn/AI/despawn。
  4. entity 生命週期事件改為經 protocol 同步給 client。
- 驗收：host/client 分處不同位置 60 秒，client 不自行生成 living entity。

### 4.2 protocol entity spawn/state/despawn replication
- [ ] 檔案：`src/network/protocol.rs`、`src/network/server.rs`、`src/network/client.rs`
- 步驟：
  1. `src/network/protocol.rs:210-302` 新增 `EntitySpawn`、`EntityState`、`EntityDespawn` 封包。
  2. EntityState 攜帶 entity id、型別、position、velocity、facing、health、animation state。
  3. host 在 spawn/despawn/state 變更時廣播；client 收到後建立/更新/移除純視覺 entity。
  4. EntityState 以 per-entity latest-wins mailbox 傳輸，避免亂序。
- 驗收：host entity/world checksum 與 client replicated state 收斂。

### 4.3 player health/effect replication
- [ ] 檔案：`src/network/protocol.rs`、`src/network/server.rs`、`src/network/client.rs`、`src/state.rs`
- 步驟：
  1. protocol 新增 `PlayerHealth`、`PlayerEffect` 封包，由 host 廣播權威值。
  2. `src/state.rs:6598-6606` client 承受本地 AI 傷害改為只接收 host 傷害結果，client 不可本地扣血。
  3. client 顯示的 health/effect 一律來自 host replication。
  4. 死亡判定只在 host 進行，再同步給 client。
- 驗收：client 顯示的 health/effect 與 host 一致，本地傷害不生效。

### 4.4 client 純視覺預測/插值
- [ ] 檔案：`src/network/client.rs`、`src/state.rs`
- 步驟：
  1. client 對 remote entity 只做插值或可回滾純視覺預測，不影響權威 state。
  2. client 本機 player movement 可預測，但收到 host 校正時回滾至權威值。
  3. 預測誤差超過門檻時立即 snap 到 host state，記錄 `prediction_rollback` counter。
  4. client 不可用預測結果驗證 remote action（如攻擊命中）。
- 驗收：預測回滾後 client state 與 host 收斂，無權威分歧。

### 4.5 pause 只 gate 本機 controller/UI
- [ ] 檔案：`src/state.rs`、`src/app.rs`
- 步驟：
  1. host 開 pause menu 或死亡時只 gate 本機 controller/UI（`src/app.rs`），不暫停權威 world clock。
  2. 只在 singleplayer 才暫停 world clock；多人時 world time、redstone、fluid、autosave 與 remote action 仍前進。
  3. `src/state.rs:6848-6865` 的 pause 邏輯區分 singleplayer/multiplayer。
  4. pause 期間 host 仍處理 network state 與權威 tick。
- 驗收：host pause/death 時 world time、redstone、fluid、autosave 與 remote action 仍前進，本機 movement 停止。

### 4.6 network state 在 catch-up sim ticks 前套用
- [ ] 檔案：`src/state.rs`、`src/network/client.rs`
- 步驟：
  1. 每幀先處理必要 network state（entity/health/block replication），再進行 authoritative catch-up ticks。
  2. 避免用舊 remote pose/action 驗證新 tick 結果。
  3. network state 套用與 tick 順序在整合測試斷言。
  4. catch-up sim ticks 最多四個，保留有界 debt。
- 驗收：network state 套用順序正確，無用舊 state 驗證新 tick。

### 4.7 host/client 收斂整合測試
- [ ] 檔案：`src/state.rs`（測試模組）、`src/network/`（測試模組）
- 步驟：
  1. host/client 分處不同位置 60 秒，斷言 client 不自行生成 living entity。
  2. host entity/world/health checksum 與 client replicated state 收斂。
  3. host pause/death 場景斷言 world clock 行為正確。
  4. network state 套用順序斷言。
- 驗收：全部 host/client 收斂測試通過。

## 驗收條件

- [ ] host/client 分處不同位置 60 秒，client 不自行生成 living entity。
- [ ] host entity/world/health checksum 與 client replicated state 收斂。
- [ ] host pause/death 時 world time、redstone、fluid、autosave 與 remote action 仍前進，本機 movement 停止。
- [ ] protocol 含 entity spawn/state/despawn 與 player health/effect replication。
- [ ] client 只插值或可回滾純視覺預測，不做權威計算。
- [ ] network state 在 catch-up simulation ticks 前套用。

## 風險與回退

- host-authoritative 改動範圍大，可能影響 singleplayer 行為；singleplayer 視為 host+client 同機，邏輯不變。
- protocol 新增封包需向後相容；舊 client 收到未知封包應忽略而非崩潰。
- pause policy 區分 single/multi 若誤判，可能讓多人世界停滯；以 `is_singleplayer` 明確判斷並測試。

## 驗證命令

```text
cargo fmt --all -- --check
cargo test --all-targets
cargo test --all-targets --release
cargo build --release
cargo clippy --all-targets --all-features
cargo test --release -- authority entity_replication pause host_client   # R4 host/client 收斂整合測試
```
