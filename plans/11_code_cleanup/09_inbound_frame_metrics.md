# 09 — 收包按實際 frame bytes 計量

狀態：已完成。基線：`83e751d`，2026-09-17。
前置：06 建議先完成；不改 wire format。

## 定位與判定

活躍流程優化。network/session.rs:71 NetworkMetrics::record_inbound → :283 packet_bytes → packet.encode().len()；ingress.rs:191/525 每次握手及 post-auth 收包都會重新序列化已解碼 Packet。

transport reader 本已知道 frame_len。Packet::decode 允許 trailing bytes，因此 re-encode 長度亦不一定等於收到的 bytes。

## 實作步驟

1. 正式成功收包出口提供本次 frame 的實際 byte count（4-byte header 加 body）。選擇回傳小型 received 結果或同一次接收的 metadata，不建立第二份 Packet。
2. record_inbound 改接收數值，移除 production inbound 的 packet.encode／packet_bytes 需求。
3. 調整 handshake／post-auth 兩個 caller；只對成功接收的 frame 按目前 metrics 契約記錄。
4. cancellation 後續讀、同 segment 多 frame、oversize header 的控制流維持原樣，避免以重新包裝 decoder 改變 framing。
5. packet_bytes 若仍被測試使用，遷移測試 fixture 的預期 frame 長度來源後刪 helper。

## 驗證與驗收

既有：transport_metrics_count_exact_successful_tcp_frames、outbound_metrics_publish_before_write_and_rollback_on_failure、recv_survives_cancellation_between_header_and_body、recv_multiple_frames_in_one_segment、recv_rejects_length_header_above_max_packet_size。

新增合法 trailing bytes frame 的計量案例，斷言包含實際收到的 header＋body；多 frame 分別記錄，取消／失敗不雙計。正式 inbound metrics 不再序列化 Packet。

## 實作紀錄

- 改動細節：
  1. `src/network/transport.rs`：新增 `ReceivedPacket { pub packet: Packet, pub frame_bytes: u64 }`（實作 `Deref<Target = Packet>` 與 `From<ReceivedPacket> for Packet`）。`Connection::recv` 與 `ConnectionReader::recv` 成功解碼時直接提供 4-byte header + body 的實際 wire bytes（`(LEN_HEADER + need) as u64`），避免建立第二份 Packet。
  2. `src/network/session.rs`：`NetworkMetrics::record_inbound` 改為接收 `bytes: u64` 數值（`inbound_bytes += bytes`），正式移除 inbound 重複序列化已解碼封包的開銷。刪除無 production caller 的 `packet_bytes(&Packet)` helper 與 `NetworkMetrics::reserve_outbound(&Packet)`；移除 `src/network/server.rs` 中的 `packet_bytes` re-export。
  3. `src/network/ingress.rs`：修改 handshake 與 post-auth 兩個 caller，改以 `received.frame_bytes` 呼叫 `record_inbound`；完全保留 cancellation、同 segment 多 frame、oversize 檢查等控制流與 wire format。
  4. `src/network/client.rs`、`src/network/loopback_test.rs`、`src/server_address_book.rs`、`src/network/server_tests.rs`：適配 `ReceivedPacket` 解構。
  5. 測試覆蓋：
     - `src/network/session.rs`：新增 `inbound_frame_metrics_include_trailing_bytes_and_avoid_duplicate_counting`，驗證含合法 trailing bytes 的 frame 計量精確包含 4-byte header + body（高於 re-encode 長度），驗證 cancellation 逾時不雙計／不誤計，且同 segment 連續多 frame 分別正確記錄。
     - `src/network/server_tests.rs`：新增 `server_ingress_records_actual_frame_bytes_with_trailing_data`，驗證端到端經由 `NetworkServer` / TCP socket 收到含 trailing data 的握手封包時，server inbound metrics 精確記錄實際收到的 wire bytes。
  6. 文件更新：更新 `ARCHITECTURE.md` Network 段落說明收包按傳輸層實際 wire bytes 計量、不再 re-serialize 的契約。
- 實際命令與結果：
  - `cargo check --all-targets --all-features`: exit 0 (10.46s)
  - `cargo test --lib network::`: 95 passed; 0 failed; finished in 2.15s
  - `cargo test --test review_hardening_ingress`: 2 passed; 0 failed
  - `cargo test server_address_book`: 6 passed; 0 failed
- 淨刪碼／保留原因：
  - 刪除 `packet_bytes(&Packet)` 與 `reserve_outbound(&Packet)`：正式 production inbound 已改按實際長度計量，outbound 一律使用 `reserve_outbound_bytes`，兩者已無正式 caller，遵守不加相容殼原則積極刪除。
  - 保留 `frame_byte_len`：供 outbound `EncodedPacket`、`send_writer_packet` 及測試計算 frame 長度共用。

