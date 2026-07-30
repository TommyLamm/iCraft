# 任務 15-R3：network catch-up backpressure、order 與持久來源

> 對應計畫：`15_performance_audit_repair_plan.md` 第 2.3 節
> 狀態：完成（2026-07-30）
> 前置：R0（文件狀態回退）
> 目標：以 `(player, dimension, chunk, revision)` 管理傳輸所有權、server 明確接受後才 dequeue、mailbox full 要 retry 不可 silent drop、flatten/Zlib 移到 bounded worker、用持久可回收 mutation/revision index 建立 catch-up，並定義 snapshot 與後續 BlockChange 順序。
> Commit 訊息：`fix(perf): reliable network catch-up with backpressure and persistent revision index`

## 完成摘要

- Protocol v6 為 `ChunkData`／`BlockChange` 加入 dimension + revision，並加入 `ChunkAck`。
- State 以 ACK-owned `(player, dimension, chunk, revision)` entry 管理傳輸；timeout 會 retry，超過上限套用可觀測的 slow-client disconnect policy。
- 每個 client 使用獨立 bounded mailbox；full 會回報 `CatchupBackpressured` 與累積 counter，不再 silent drop。
- snapshot flatten/Zlib 與 unloaded persisted-chunk load 移到 capacity 8 worker；主線程每幀全域最多提交 2 筆。
- `mutation_revisions.bin` 只保留每 chunk 最新 revision；chunk 存檔也帶實際 revision，避免尚未落盤的舊 snapshot 被錯標為新版本。
- client revision gate 會先緩衝跨 channel gap，再以 snapshot base revision 釋放連續 BlockChange；過期 snapshot/change 直接丟棄。

## 相關程式碼位置（已核對）

- `src/state.rs:4647-4703` - catch-up mailbox 與 `pop_front`/`CatchupMailbox::replace` 路徑。
- `src/network/server.rs:175-203` - join payload 處理與 `ChunkSaveData::from_chunk`。
- `src/network/server.rs:634-652` - mailbox drain 排序與丟棄。
- `src/network/client.rs` - client 接收與套用。
- `src/network/protocol.rs` - 協定封包定義。
- `src/save.rs` - flatten/Zlib 與 region 來源。
- `src/state.rs:2378-2555` - network drain budget 路徑。

## 已確認的基線風險

- State 在 server 接受前已 `pop_front`；`CatchupMailbox::replace` 滿時回 `false`，caller 忽略結果，造成 Chunk 永久缺漏。
- mailbox drain 依 `(cx, cz)` 排序，破壞近距優先。
- join payload 在主線程呼叫 `ChunkSaveData::from_chunk` 並 Zlib 壓縮。
- catch-up 來源只看當下 loaded mutated chunks；已 unload 的歷史 mutation 不會補給新玩家。
- reliable FIFO 與 catch-up mailbox 是不同 select branch，缺少跨 channel revision/order 規則。

## 子任務清單

### 3.1 傳輸所有權與 server ACK dequeue
- [x] 檔案：`src/network/server.rs`、`src/state.rs`
- 步驟：
  1. 每筆 catch-up payload 標記 `(player, dimension, chunk, revision)` 作為傳輸所有權。
  2. server 確認 client 已接受（ack 封包或 in-flight 上限內）後才從 queue dequeue。
  3. `src/state.rs:4647-4703` 的 `pop_front` 延後到 ACK 後執行。
  4. 未 ACK 項保留在 queue，並有重傳/超時策略。
- 驗收：mailbox capacity=1、慢 client 仍可最終收斂，無 Chunk 永久缺漏。

### 3.2 mailbox full retry 不可 silent drop
- [x] 檔案：`src/network/server.rs`、`src/state.rs`
- 步驟：
  1. `CatchupMailbox::replace` 滿時不再回 `false` 被忽略；改為保留/retry 並記錄 `catchup_mailbox_full` counter。
  2. 定義 slow client policy：降速、暫停新 snapshot、或斷線，必須明確且可觀測。
  3. 多 client 場景各自獨立 backpressure，互不餓死。
  4. `src/network/server.rs:634-652` 的 drain 改為近距 priority，不依 `(cx, cz)` 純排序。
- 驗收：mailbox full 時不丟 Chunk；slow client policy 可觀測。

### 3.3 flatten/Zlib 移到 bounded worker
- [x] 檔案：`src/network/server.rs`、`src/save.rs`、`src/state.rs`
- 步驟：
  1. join payload 的 `ChunkSaveData::from_chunk` 與 Zlib 壓縮（`src/network/server.rs:175-203`）移到 bounded worker thread。
  2. 主線程只提交 immutable request（chunk 座標、revision、Arc 參考）。
  3. worker 輸出已 flatten/compress 的 payload，主線程只負責發送。
  4. 每幀總 budget 不隨 client 數線性放大。
- 驗收：catch-up 壓力場景中主線程不執行 flatten/Zlib。

### 3.4 持久可回收 mutation/revision index
- [x] 檔案：`src/state.rs`、`src/save.rs`
- 步驟：
  1. 建立持久 mutation/revision index，記錄每個 Chunk 的歷史 mutation revision，不依賴 host 當下 loaded set。
  2. 取代目前只增不減、join 時全量排序的 `mutated_chunks` HashSet。
  3. index 可回收：已傳給所有在線/未來 player 的舊 revision 可清理。
  4. 已 unload 的歷史 mutation 仍能補給新玩家。
- 驗收：新玩家加入時可取得已 unload Chunk 的最新 snapshot。

### 3.5 距離 priority + 同 Chunk latest revision wins
- [x] 檔案：`src/network/server.rs`
- 步驟：
  1. catch-up queue 以 `(distance_to_player, dimension, chunk)` 排序，近距優先。
  2. 同一 Chunk 多個 revision 只保留最新 revision 傳輸。
  3. 距離 priority 在玩家移動時動態更新。
  4. reliable chat/control/block packet 不受 catch-up priority 影響，保序。
- 驗收：近距 Chunk 先到；同 Chunk 舊 revision 不覆蓋新 revision。

### 3.6 snapshot 與後續 BlockChange 順序
- [x] 檔案：`src/network/protocol.rs`、`src/network/client.rs`
- 步驟：
  1. 定義 Chunk snapshot 與其後 BlockChange 的全域順序：snapshot 帶 base revision，後續 change 帶遞增 revision。
  2. client 不得用較舊 snapshot 覆寫新 mutation；以 revision 比較丟棄過期 snapshot/change。
  3. reliable FIFO 與 catch-up mailbox 間加入跨 channel revision/order 規則。
  4. client 套用前驗證 revision 單調性。
- 驗收：client checksum 與 host 收斂，無舊 snapshot 覆蓋。

### 3.7 catch-up 收斂整合測試
- [x] 檔案：`src/network/server.rs`（測試模組）、`src/network/client.rs`（測試模組）
- 步驟：
  1. mailbox capacity=1、慢 client、多 client、unloaded mutated Chunk 場景各自斷言 checksum 收斂。
  2. 近距 Chunk 先到；可靠 chat/control/block packet 保序。
  3. 主線程在壓力場景每幀總 budget 不隨 client 數線性放大。
  4. 跨 channel revision 順序測試：snapshot 後 BlockChange 不可被舊 snapshot 覆蓋。
- 驗收：所有 catch-up 場景 checksum 收斂，無永久缺漏。

## 驗收條件

- [x] mailbox capacity=1、慢 client、多 client、unloaded mutated Chunk 均可最終收斂 checksum。
- [x] 沒有 Chunk 永久缺漏；近距 Chunk 先到。
- [x] 可靠 chat/control/block packet 保序。
- [x] catch-up 壓力場景中主線程不執行 flatten/Zlib，每幀總 budget 不隨 client 數線性放大。
- [x] server 明確接受後才 dequeue；mailbox full 不 silent drop。
- [x] snapshot 與後續 BlockChange 順序正確，無舊 snapshot 覆蓋。

## 風險與回退

- 持久 mutation/revision index 可能增加記憶體；以可回收策略與上限控制，回收已傳給所有在線 player 的舊 revision。
- ACK-based dequeue 在 packet loss 下需重傳，重傳計入 budget counter 避免無限重試。
- 多 client backpressure 若實作複雜，先以單一 global budget 收斂，再拆分 per-client。

## 驗證命令

```text
cargo fmt --all -- --check
cargo test --all-targets
cargo test --all-targets --release
cargo build --release
cargo clippy --all-targets --all-features
cargo test --release -- catchup mailbox backpressure network_order revision   # R3 整合與 fault-injection 測試
```
