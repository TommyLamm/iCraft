# 09 — 收包按實際 frame bytes 計量

狀態：待執行。基線：`83e751d`，2026-09-17。
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

尚未執行；完成時記錄實際修改、驗證結果、刪碼量及文件更新。

