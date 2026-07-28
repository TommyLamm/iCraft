# 任務 4：多人 catch-up streaming

> 對應計畫：`14_performance_optimization.md` Phase 1.4
> 狀態：✅ 已完成
> 前置：任務 1（基線）、建議任務 2（streaming queue 基礎）
> 目標：PlayerJoin 不同步壓縮全部 mutated chunks，改為背景建立 payload 並分幀傳送，消除加入期間的主線程長幀。
> Commit 訊息：`perf(network): stream join catch-up with bounded backpressure`

## 相關程式碼位置（已核對）

- `src/state.rs` - `schedule_player_catchup`, `process_join_catchups`, `drain_network_events`
- `src/network/server.rs` - `CatchupMailbox`, `ClientSession`, `handle_host_command`

## 子任務清單

### 4.1 PlayerJoin 不同步壓縮
- [x] 檔案：`src/state.rs`、`src/network/server.rs`
- 步驟：
  1. `schedule_player_catchup` 不在 PlayerJoin callback 同步壓縮全部 Chunk。
  2. 改為標記需要傳送的 Chunk 清單，交給背景建立 payload。
- 驗收：PlayerJoin 不造成主線程長幀。

### 4.2 背景建立 payload 並分幀傳送
- [x] 檔案：`src/network/server.rs`、`src/state.rs`
- 步驟：
  1. 背景建立 payload，依玩家距離與 Chunk revision 排序。
  2. 分幀發送，每幀只傳送有限數量的 payload。
  3. 同一 Chunk payload latest-wins；較舊 revision 被較新取代時丟棄。
- 驗收：加入期間主線程 `network_drain` p95 在預算內。

### 4.3 可靠佇列保留順序
- [x] 檔案：`src/network/server.rs`
- 步驟：
  1. 可靠 block/chat/control queue 保留順序，不被 catch-up payload 干擾。
  2. catch-up payload 使用獨立 bounded queue (`CatchupMailbox`)，與可靠佇列分離。
- 驗收：block change 與 chat 順序正確；catch-up 不阻塞可靠封包。

### 4.4 drain_network_events 預算
- [x] 檔案：`src/state.rs`
- 步驟：
  1. `drain_network_events` 加入時間預算與事件數預算。
  2. pose/time 可 coalesce（latest-wins），達到預算時保留未處理的 coalescable 事件。
  3. authoritative block result 不可丟失，必須在預算內處理完畢或提高預算。
- 驗收：大量網路事件不造成單幀 `network_drain` 超過預算。

### 4.5 大型 payload bounded backpressure
- [x] 檔案：`src/network/server.rs`、`src/network/client.rs`
- 步驟：
  1. 大型 payload 使用 bounded backpressure，避免主線程和 Tokio thread 同時累積無上限 Vec。
  2. queue 滿時暫停 payload 建立，等待 consumer 消化。
- 驗收：主線程與 Tokio thread 的 queue 深度有上限（F3 counter 顯示）。

## 驗收條件

- [x] PlayerJoin 不在主線程同步壓縮全部 Chunk。
- [x] catch-up payload 依距離排序且分幀傳送。
- [x] 可靠 block/chat/control 封包順序不受影響。
- [x] `drain_network_events` 有時間/事件數預算。
- [x] queue 深度有上限。
- [x] 加入期間主線程 p95 改善（與任務 1 基線比較）。
- [x] `cargo fmt --all -- --check`、`cargo check --release`、`cargo test --release` 通過。

## 風險與回退

- backpressure 過激會延長加入時間；預算由基線決定。
- 可靠封包語義不可削弱；若有疑慮，先只做 payload 分幀，不動可靠佇列。
- 不改變 Protocol 版本（除非需要新封包類型）。

## 驗證命令

```text
cargo fmt --all -- --check
cargo check --release
cargo test --release
cargo run --release   # 固定場景 8（Host + client 加入）before/after 比較
```
