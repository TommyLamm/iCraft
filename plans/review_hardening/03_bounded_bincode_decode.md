# Plan03 — 有界 bincode 解碼與對抗性 TCP

## 定位

- 協定 v19 在 transport 用 4-byte BE 長度 + 2 MiB frame cap（`MAX_PACKET_SIZE`）。
- `Packet::decode` 是 `bincode::deserialize`，預設選項。bincode 1.3.3 的 `deserialize_seq`
  會依 `u64` 長度 `Vec::with_capacity`，**先於**讀元素。`SliceReader` 限制 String／bytes，
  不限制 `Vec<T>`（含未標 `serde_bytes` 的 `Vec<u8>`）。
- `recv()` 在 handshake match 之前就 decode 完整 `Packet`。一個約 40-byte、自稱超長
  `ChunkData.blocks` 的 frame 可在登入前 OOM。
- 既有測試只有 `invalid_bytes_rejected`（三個 `0xFF`）與 16 KiB `MAX_REQUEST_BYTES`，
  沒有「合法 frame + 謊報 Vec 長度」，也沒有 `length > 2 MiB` 的 length header 測試。

## 前置

無。

## 精確 acceptance

- [ ] 任何 `Packet::decode` 路徑對「frame ≤ 2 MiB、內含 `Vec` 長度遠大於剩餘 bytes」
  回 `InvalidData`（或自訂錯誤），行程不 abort、不 `handle_alloc_error`。
- [ ] 2 MiB **length header**（`0x00200001` + 短 body）在 `ConnectionReader::recv` 被拒，
  不建立 session，伺服器仍能接受下一個合法客戶端。
- [ ] `Vec<u8>` 熱欄位（至少 `ChunkData.blocks`、聊天字串、效果 payload）用
  `serde_bytes` 或自訂 visitor，capacity 不得超過剩餘 slice。
- [ ] Handshake 完成前 decode 失敗只關閉該連線，不影響其他 client。
- [ ] 新增 raw-TCP fixture（不要只靠 `NetworkClient`）：寫 crafted frame，assert 伺服器存活。

## 預計檔案與測試

- 修改：`src/network/protocol.rs`（`Packet::decode`、相關 `Vec` 欄位）、
  `src/network/transport.rs`（可把 decode 限額與 frame cap 綁在一起）。
- 測試：`src/network/protocol.rs` 單元測 + `src/network/transport.rs` 長度 header；
  integration 放 `tests/review_hardening_adversarial_frames.rs` 或擴
  `tests/common/tcp_harness.rs`。

## 建議階段

1. 用最小 crafted `ChunkData` 在單元測試重現「小 slice、大 len」；確認現況是 panic／超大 alloc
   或 Err（以實際 bincode 行為為準，不要只抄審查假設）。
2. 包一層 decode：先拒 `len > remaining`，再 `deserialize`。
3. 加 `> 2 MiB` length-header 測試。
4. 跑 network 單元 + 新對抗檔；不要為此升 protocol 版本。

## 不在本計劃

- TLS、handshake 身份（04）、chat／pose 速率（12）。
- 改 bincode major 版（除非現有 1.3.x 無法安全包一層；預設不升級）。
