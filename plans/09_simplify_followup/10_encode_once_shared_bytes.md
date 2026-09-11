# Plan10 — 出站封包只 encode 一次

## 定位

Wave 08 加了 `Packet::encode_frame()`，但沒快取 payload：

- `packet_bytes` 呼叫 `packet.encode()`（session 計量／reserve）。
- enqueue 後 `TrackedPacket` 仍持有 `Packet`。
- `ConnectionWriter`／`encode_frame` 再 `encode()` 一次。

多人時同一 pose／state／chunk 事件可 serialize N 次。ChunkData／EntityState 尖峰最痛。

## 前置

無。不要 bump protocol。01 若尚未做，本計劃仍只改內部 bytes 快取，不刪變體。

## 精確 acceptance

- [x] 熱路徑：一次 `encode()`（或 `encode_frame`），`packet_bytes`、queue 計量、真正寫 socket **共用**同一 payload。
- [x] `TrackedPacket`（或同等）可持有 `Arc<[u8]>`；broadcast 對多個 connection 只 serialize 一次。
- [x] 2 MiB cap、4-byte BE length prefix 不變。
- [x] 對抗性畸形封包測試仍手組 frame，不改走 `Packet::encode`。
- [x] 不同協議版本的 session 不得誤共享 bytes（若目前只有單一 `PROTOCOL_VERSION`，寫明假設）。
- [x] `cargo test --lib network::` 與 `tests/review_hardening_adversarial_frames.rs` 通過。

## 預計檔案與測試

- `src/network/session.rs`、`transport.rs`、`protocol.rs`、`egress.rs`
- 驗證：既有 encode_frame／max size 測試；egress broadcast 測試

## 建議階段

1. `packet_bytes` 改吃已編碼長度，避免第二次 serialize。
2. `TrackedPacket` 存 bytes；writer 只寫 length+payload。
3. broadcast 路徑提升到「encode once, fanout Arc」。

## 不在本計劃

- 改 bincode schema 或壓縮 ChunkData。
- 刪 `HostToServer` 鏡像層。
- 改 inbound decode。

## 實作與證據

### 改了什麼

- `Packet::encode_payload`：2 MiB 上限的 bincode body；`encode_frame` 改走它。
- `EncodedPacket`：持有邏輯 `Packet` + `Arc<[u8]>` payload；`frame_bytes()` =
  payload + 4-byte BE length，供 queue／metrics 與寫 socket 共用同一 encode。
- `TrackedPacket` 改存 `EncodedPacket`；writer（`send_payload`／
  `send_encoded_packet`）只寫 length+payload，不再第二次 `encode`。
- `broadcast_to`／`broadcast_reliably`／`broadcast_pose_inner`／`broadcast_state`
  先 encode 一次，再 `clone` Arc fanout。
- 假設：handshake 後只有單一 `PROTOCOL_VERSION`；`EncodedPacket` 記錄版本並
  `debug_assert`，避免日後多版本誤共享 bytes。

### 測了什麼

- `cargo test --lib network::` → 100 passed（含
  `encoded_packet_fanout_shares_one_payload_arc`、encode_frame／mailbox／
  transport framing）。
- `cargo test --test review_hardening_adversarial_frames` → 2 passed（手組
  frame，未改走 `Packet::encode`）。

### 留下的缺口

- 單目標 `send_to`／mailbox `replace(Packet)` 仍各自 encode 一次（非 N 倍
  fanout）；inbound `packet_bytes(&Packet)` 仍可能再 serialize 一次做計量。
- Plan 11（mutation fanout 反向索引）未做。
