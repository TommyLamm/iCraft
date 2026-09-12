# Plan02 — 刪 `BlockUse`、`wrap_legacy` 與舊 inbound 封包

## 定位

`GameplayOperation::BlockUse` 永遠 `Unsupported`（`dispatch.rs` 早退與 match arm 拒絕兩次），卻仍是協定與測試夾具。`wrap_legacy` 把 leftover `BlockChange` 映成 `BlockUse` 只為了再被拒。Live egress 已是 `GameplayRequest`。

| 符號 | 位置 | 原因 |
| :--- | :--- | :--- |
| `GameplayOperation::BlockUse` | `src/network/protocol.rs` | 永遠 `Unsupported` |
| `LegacyGameplay` + `wrap_legacy` | `protocol.rs` ~588–687 | 只服務舊 inbound |
| leftover 客戶端封包適配 | `network/ingress.rs` ~588–846 | live 不建構 |
| leftover `GameToClient` 變體 | `network/client.rs` ~302–338 | 只有 client 單元測試送 |
| `common_gameplay_vectors` 第一筆 | `authority/contract.rs` | 鎖「拒絕且不突變」 |

## 前置

無。15 的 Container 雙信封收斂建議在本計劃之後。

## 精確 acceptance

- [x] 刪 `GameplayOperation::BlockUse`。
- [x] 刪 `LegacyGameplay`、`wrap_legacy`、`from_legacy_block_action`。
- [x] Ingress 對 leftover `Packet::{BlockChange-as-request, BlockActionRequest, SleepRequest, ContainerOpenRequest, ContainerClickRequest, ContainerClose}` 不再包成 request（可丟／斷線）。伺服器→客戶端的 `BlockChange` **投影**保留。
- [x] `GameToClient` live 送出路徑只留 pose / chat / disconnect / `GameplayRequest` / respawn。
- [x] `tests/review_hardening_block_use_rejected.rs` 與 `common_gameplay_vectors` 的 BlockUse 夾具改成真實 `BlockAction` 或刪除。
- [x] Protocol 版本若因此不相容舊測試客戶端：在計劃證據寫明；live 桌面不得再送舊變體。

## 預計檔案與測試

- `src/network/protocol.rs`、`ingress.rs`、`client.rs`、`src/authority/dispatch.rs`、`contract.rs`
- `tests/review_hardening_block_use_rejected.rs` 及 network leftover 測試
- 驗證：`cargo test --lib network::`；相關 review_hardening

## 建議階段

1. 確認桌面 live send 只走 `NetworkHandle::request_gameplay`。
2. 刪 variant 與 wrap，用編譯錯誤清測試。
3. 對抗性 frame 測試維持手組 header。

## 不在本計劃

- 改 Protocol v19 的 handshake 或 2 MiB cap。
- 刪 `NetworkHandle::Host`（03）。
- 合併 `Container` / `ContainerClick`（15）。

## 實作與證據

- `GameplayOperation::BlockUse` 已從 `src/network/protocol.rs` 刪除；`dispatch.rs` 的早退／match arm 一併拿掉。place/break 只走 `BlockAction`。
- `LegacyGameplay`、`wrap_legacy`、`from_legacy_block_action` 已刪。Ingress catch-all 對 leftover `Packet::{BlockChange-as-request, BlockActionRequest, SleepRequest, ContainerOpenRequest, ContainerClickRequest, ContainerClose}` **decode 後丟棄**，不再包成 `GameplayRequest`。`GameToClient` 只剩 pose / action / chat / disconnect / respawn / `GameplayRequest`。
- 伺服器→客戶端 `Packet::BlockChange` 投影仍由 `egress.rs` 送出；`client.rs` 仍套 revision gate。`NetworkHandle::Host` 與 Container 雙信封未動。Handshake 仍是 Protocol v19，`MAX_PACKET_SIZE` 仍是 2 MiB。對抗性 oversized header 仍手組 `0x0020_0001`（`transport.rs::recv_rejects_length_header_above_max_packet_size`）。
- 舊 BlockUse 夾具改成 `BlockAction::Place`：缺 `held` 或物品／目標不合法時權威回 `InvalidState`，不突變世界／物品欄、不產掉落。`common_gameplay_vectors[0]` 同此。
- **Protocol 相容性**：handshake 版本未 bump。刪 `GameplayOperation::BlockUse` 會平移後續 bincode discriminant，舊測試客戶端若仍送 `BlockUse` 會解成別的 operation 或 decode 失敗，不再保證 `Unsupported`。leftover inbound `Packet` 變體仍可 decode（未重排），只是不再轉成 request。live 桌面只送 `GameplayRequest`。

### 測試

```
cargo test --lib network:: -- --test-threads=1
  100 passed (含 leftover_inbound_packets_are_dropped_not_wrapped、
  inbound_block_change_is_not_wrapped_as_a_request、
  live_client_inputs_are_typed_gameplay_requests、
  recv_rejects_length_header_above_max_packet_size、
  rejects_old_protocol_during_handshake)

cargo test --test review_hardening_block_use_rejected --test review_hardening_container_click --test review_hardening_invariants -- --test-threads=1
  3 + 9 + 6 passed

cargo test --test authority_persistence --test headless_server_authority --test runtime_topology_parity -- --test-threads=1
  4 + 2 + 6 passed
```
