# Plan13 — 網路伺服器非同步原生化與通道目標整合

## 定位

在網路伺服器與連線管理模組中，存在以下非同步樣板與成對通道過度工程：

1. **`src/network/server.rs:L171-249` 定時器輪詢 `std::sync::mpsc`**：
   伺服器主非同步迴圈中使用 `time::interval(10ms)` 輪詢 `host_to_server.try_recv()`，既消耗額外 CPU 喚醒，又給 host 指令帶來最高 10ms 的無謂延遲。
   改為 `tokio::sync::mpsc` 通道，在 `tokio::select!` 中直接 `cmd = host_to_server.recv() => { ... }` 原生掛起等待，消除定時器與排空樣板。

2. **`src/server_address_book.rs:L267-284, L412-465` 檔案重命名與臨時 Runtime**：
   - 檔案儲存手寫了非原子的 remove 再 rename，改為複用 `save::region::atomic_write`。
   - 單次 ping 動態構造並銷毀 Tokio runtime，改用標準帶超時的 `std::net::TcpStream` 或複用已有 runtime。

3. **`src/network/channels.rs` 與 `egress.rs` 中成對的 `BroadcastX` / `SendX`**：
   許多網路指令成對出現（一個廣播全體、一個單發目標），可透過 `to: Option<PlayerId>` 統一結構，減少 ~150 行枚舉定義與 match 處理。

預期削減代碼 ~240 行。

## 前置

05（網路 dead 通道變體清理）。

## 精確 acceptance

- [ ] `network/server.rs` 移除 10ms 定時器輪詢，改為基於非同步 channel 的原生 `recv().await`。
- [ ] `server_address_book.rs` 複用原子檔案寫入工具，消除臨時 Tokio runtime 建立。
- [ ] 簡化 `HostToServer` 成對的廣播/單發指令結構。
- [ ] 保持伺服器 TCP 監聽、封包收發與 Ping 檢測行為完全不變。
- [ ] `cargo check --all-targets` 通過。
- [ ] 網路通訊與伺服器測試全數通過。

## 預計檔案與測試

- 修改：
  - `src/network/server.rs`
  - `src/network/channels.rs`
  - `src/network/egress.rs`
  - `src/server_address_book.rs`
- 驗證測試：
  - `cargo test --lib network::`
  - `cargo test --test review_hardening_network_ingress -- --test-threads=1`
  - `cargo test --test plan30_real_transport_acceptance -- --test-threads=1`
  - `cargo check --all-targets`

## 建議階段

1. 重構 `network/server.rs` 的指令接收為 tokio mpsc async recv。
2. 整合 `server_address_book.rs` 的檔案寫入與 ping。
3. 簡化成對的 `HostToServer` 指令。
4. 運行真實 TCP 整合測試。

## 不在本計劃

- 修改 TCP 封包長度頭（4-byte big-endian）或 bincode 編解碼規則。
