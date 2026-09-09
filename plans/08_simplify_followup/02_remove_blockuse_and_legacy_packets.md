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

- [ ] 刪 `GameplayOperation::BlockUse`。
- [ ] 刪 `LegacyGameplay`、`wrap_legacy`、`from_legacy_block_action`。
- [ ] Ingress 對 leftover `Packet::{BlockChange-as-request, BlockActionRequest, SleepRequest, ContainerOpenRequest, ContainerClickRequest, ContainerClose}` 不再包成 request（可丟／斷線）。伺服器→客戶端的 `BlockChange` **投影**保留。
- [ ] `GameToClient` live 送出路徑只留 pose / chat / disconnect / `GameplayRequest` / respawn。
- [ ] `tests/review_hardening_block_use_rejected.rs` 與 `common_gameplay_vectors` 的 BlockUse 夾具改成真實 `BlockAction` 或刪除。
- [ ] Protocol 版本若因此不相容舊測試客戶端：在計劃證據寫明；live 桌面不得再送舊變體。

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
