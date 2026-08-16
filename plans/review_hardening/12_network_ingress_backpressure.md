# Plan12 — 網路入口背壓與可靠容器廣播

## 定位

- 只有 `GameplayRequest` 限 120/s。`ChatMessage` 與 `PlayerPosition` 直接進
  `HOST_EVENT_QUEUE_CAPACITY = 1024`。`MeteredHostEventSender::try_send` 在 Full 時
  當成 host channel 關閉，**斷開送出者**；一個 flood 會讓其他客戶端的下一筆 pose／chat
  也 `Err` 被踢。
- Chat 在 enqueue 之後才 `message.chars().take(256)`。2 MiB 字串可先佔 queue
  （高水位約 1024 × 2 MiB）。
- `BroadcastContainerSlotUpdate` 不在 `reliable_broadcast`，走 `best_effort_send`，
  64 槽客戶端 queue 滿就丟。Listen-host 箱子同步靠這條；dedicated 的 targeted
  `SendContainerSlotUpdate` 是可靠的。
- `NetworkClient` 用 unbounded `std::sync::mpsc`，`let _ = send`。惡意伺服器可灌到
  join client OOM。
- `listener.accept` 無條件 spawn `run_client`；`max_players` 在握手後才執行。
  無預認證連線上限。

## 前置

- 02 應已合併，否則「可靠廣播容器 slot」會把**錯誤的** click 結果送得更可靠。
  若 02 未合併：本計劃只做 chat／pose 背壓與 client 有界 channel，不要先改容器廣播。

## 精確 acceptance

- [ ] Chat 在 `run_client` enqueue **之前**拒 >256 字元（或 bytes，與現有顯示 cap 一致）
  的訊息，不進 host queue。
- [ ] Chat 與 pose 有獨立速率上限。`TrySendError::Full` 對該連線做入口背壓
  （丟 pose、拒 chat），**不得**把其他客戶端當死 host 踢掉。
- [ ] `BroadcastContainerSlotUpdate` 走可靠路徑，或對每個 viewer 送 targeted 可靠更新。
  enqueue 失敗的處理與其他可靠包相同（短等後踢慢客戶端），不得默默丟 slot delta。
- [ ] `NetworkClient` 改有界 channel；持續溢位斷線。非本機 `player_id` 的
  `PlayerSessionUpdate` 在進 queue 前丟掉。
- [ ] 未完成握手的並發連線有上限（建議 `2 * max_players`），超過停止 accept 或立即關
  新 socket。握手超時可短於現有 15s（寫進測試能接受的值）。
- [ ] 測試：單 client 灌 pose／超長 chat，第二 client 仍能完成 GameplayRequest；
  超長 chat 不在 host queue；容器 slot 在 viewer queue 壓力下仍送達或該 viewer 被踢
  （不得靜默與主機箱子分叉）。

## 預計檔案與測試

- 修改：`src/network/server.rs`、`src/network/client.rs`、`src/server_runtime.rs`
  （若 host queue 型別／計量要改）。
- 測試：`src/network/server.rs` 單元測 + `tests/review_hardening_ingress.rs`（真 socket）。

## 建議階段

1. Chat 長度在 enqueue 前拒絕 + 測試。
2. Full 不再當死 host。
3. 容器 slot 改可靠。
4. Client 有界 channel。
5. 預認證連線 cap。

## 不在本計劃

- 有界 bincode（03）。
- Handshake 身份（04）。
- TLS。
