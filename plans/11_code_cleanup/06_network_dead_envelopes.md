# 06 — network 死封套與傳送支線

狀態：已完成。基線：`83e751d`，2026-09-17。
前置：02 建議先完成。

## 定位與判定

- network/session.rs:166 EncodedPacket.protocol_version 無正式讀取，僅 encoded_packet_fanout_shares_one_payload_arc 的 assertion 使用。
- TrackedPacket::try_from_packet（:233）、into_packet（:259）僅 server_tests.rs 使用。
- egress.rs:295 broadcast_to → session.rs:384 best_effort_send_encoded 無正式根入口；best_effort_send／send_with_outbound_metrics 是死支線。
- QueuedPacket::Outbound **仍有正式 producer**（ingress.rs:48）與 consumer（:346），不可隨 helper 一起刪。

## 實作步驟

1. 刪 EncodedPacket 的版本副本／getter；握手版本檢查保留於正式 protocol 路徑。
2. 測試改由 EncodedPacket::new／TrackedPacket::new 建構，以 packet() 觀察 queue 結果，刪只服務舊測試的正式轉換 API。
3. 刪上述無根傳送鏈，以及 client::authoritative_weather_event 舊 helper；清失去用途的 imports／constants。
4. CatchupMailbox::len 若只有測試需求，移到 test-only 邊界；其 lifecycle 測試留存。
5. 不改正常 reliable／pose／state／catchup delivery，也不改 wire format。inbound 計量另由 09 處理。

## 驗證與驗收

`cargo test --lib network::`；關注：
encoded_packet_fanout_shares_one_payload_arc、queue_metrics_track_backlog_replacement_drain_and_saturation、full_reliable_queue_evicts_slow_client_without_ghost_session、unsent_pose_updates_are_latest_wins_per_player。

刪掉 metadata/assertion 後 Arc 共用、bytes 內容、背壓與 latest-wins 行為 assertions 仍在。

## 實作紀錄

- 修改內容：
  1. `src/network/session.rs`：
     - 刪除 `EncodedPacket` 的 `protocol_version` 欄位與 `protocol_version(&self)` getter、`into_packet(self)`。
     - 刪除 `TrackedPacket` 的 `try_from_packet` 與 `into_packet` 測試專用轉換 API。
     - 刪除無根傳送函式 `best_effort_send`、`best_effort_send_encoded`、`send_with_outbound_metrics`。
     - 將 `CatchupMailbox::len` 的 `#[allow(dead_code)]` 改為 `#[cfg(test)]`。
     - 清理重複與未使用常數 `HANDSHAKE_TIMEOUT`、`DEFAULT_POSE_RATE_PER_SECOND`、`DEFAULT_CHAT_RATE_PER_SECOND`（正式定義保留在 `channels.rs`）。
     - 清理測試模組中未使用的 `PROTOCOL_VERSION` import，並移除 `encoded_packet_fanout_shares_one_payload_arc` 中對已刪除 `protocol_version()` 的 assertion。
  2. `src/network/server.rs`：
     - 從 re-exports 中移除 `broadcast_to`、`best_effort_send`、`send_with_outbound_metrics`，補入 `EncodedPacket` 供內部測試使用。
  3. `src/network/egress.rs`：
     - 刪除無根入口 `broadcast_to`。
     - 清理未使用的 `PROTOCOL_VERSION` 與 `best_effort_send_encoded` imports。
  4. `src/network/client.rs`：
     - 刪除無 caller 的舊 helper `authoritative_weather_event`。
  5. `src/network/client_tests.rs`：
     - 刪除針對已刪 helper 的單元測試 `authoritative_weather_packets_map_to_pure_client_events`（真實天氣封包分發在 `connects_and_receives_world_init` 等整合測試中已覆蓋）。
  6. `src/network/server_tests.rs`：
     - 調整 `reliable_join_and_leave_wait_for_bounded_queue_capacity` 與 `full_reliable_queue_evicts_slow_client_without_ghost_session`，改用 `EncodedPacket::new` + `TrackedPacket::new` 建構，透過 `packet.packet().clone()` 觀察佇列封包。
- 保留原因：
  - `QueuedPacket::Outbound` 在 `ingress.rs:48`（forward_to_outbound）具有正式 producer，且在 `ingress.rs:346` 有 consumer，維持正常轉發流程未刪除。
  - 握手與連線協議正規版本檢查維持在 `protocol.rs`、`client.rs`、`ingress.rs` 正式路徑。
  - `packet_bytes` 用於 outbound/inbound metrics 保留。
- 實際驗證命令與結果：
  - `cargo check --all-targets --all-features`：exit 0，無 network 相關警告。
  - `cargo test --lib network::`：93 passed, 0 failed（關注之 4 個核心測試全數通過）。
  - `cargo test --test review_hardening_adversarial_frames --test review_hardening_session_lifecycle`：5 passed, 0 failed。
- 淨刪碼量：
  - 6 個檔案變更，17 行新增，145 行刪除，淨刪除 128 行。


