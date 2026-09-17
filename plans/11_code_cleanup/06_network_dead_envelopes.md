# 06 — network 死封套與傳送支線

狀態：待執行。基線：`83e751d`，2026-09-17。
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

尚未執行；完成時記錄實際修改、驗證結果、刪碼量及文件更新。

