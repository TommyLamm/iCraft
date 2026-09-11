# Plan09 — v20 附帶項：`ContainerAction` 進 enum、剝每封包 `protocol_version`

## 定位

09 波 01 是本輪唯一允許的 handshake bump（v19 → v20）。兩個 wire 層面的殘留若不搭這班車，就要等 v21：

### `Container { action: u8 }` wire gap

- `ContainerAction::from_wire`（`protocol.rs` 172–177）只接受 `0`／`2`；`1` 是 leftover Click。
- `GameplayOperation::Container { action: u8 }`（437–443）直接存 wire byte；`validate_bounds` 靠 `from_wire` 失敗拒絕（698）。
- live producer：`state.rs` 4735–4739、8033、8622；`authority/dispatch.rs` 98–102，全部用 `to_wire()`。
- 測試：`leftover_container_click_wire_is_rejected`（`protocol.rs` 2073–2091）、`server_world.rs` 3113（`action: 1`）。

### 每封包 `protocol_version: u32`

- `Packet`（`protocol.rs` 1184–1455）**40 個變體**每個都帶 `protocol_version`。
- accessor 是 113 行 or-pattern match（1458–1571）。
- handshake 已拒舊版；內層 `GameplayRequest`／`Response` 不重複帶版號。
- 每個 send site（`client.rs` keepalive／chat／action、`egress.rs` 所有建構子）都要寫 `protocol_version: PROTOCOL_VERSION`。

## 前置

**必須與 09 波 01 同一個 PR／同一次 bump**。09 波 01 開始實作前先把本計劃併入其執行清單；若 09 波 01 已獨立落地成 v20，本計劃改為 v21 並在 README 註明。

## 精確 acceptance

- [ ] `GameplayOperation::Container { action: ContainerAction }`，`ContainerAction` 為 `#[repr(u8)]` Open=0、Close=1；未知值在 decode 失敗。
- [ ] `from_wire`／`to_wire`／wire gap 註解／`leftover_container_click_wire_is_rejected` 刪除；改為「未知 enum 值 decode 失敗」測試。
- [ ] `protocol_version` 只留在 `Handshake`／`LoginSuccess`／`ServerListPing*`；其餘 39 個變體刪欄位。
- [ ] `Packet::protocol_version()` accessor 刪除；連線在 handshake 後持有版本。
- [ ] 所有 send site 不再寫 `protocol_version:`。
- [ ] `PROTOCOL_VERSION == 20`（或與 09 波 01 一致）；roundtrip 測試更新；對抗性手組 frame 測試維持手組並更新 layout。
- [ ] `ARCHITECTURE.md` Network 段：`Container` wire 值改 Open=0／Close=1；刪「leftover Click=1 fails bounds validation」句。

## 預計檔案與測試

- 改：`src/network/{protocol,client,egress,ingress,server}.rs`、`src/authority/dispatch.rs`、`src/state.rs`、`src/server_world.rs`（測試）、`ARCHITECTURE.md`
- 驗證：`cargo test --lib network::`；`tests/review_hardening_adversarial_frames.rs`；`tests/review_hardening_container_click.rs`；`tests/plan30_real_transport_acceptance.rs`

## 建議階段

1. 在 09 波 01 的 bump commit 之後、同 PR 內：先做 `ContainerAction` enum（小）。
2. 剝 `protocol_version`（機械，用編譯錯誤當清單）。
3. 更新手組 frame 測試與 ARCHITECTURE。

## 不在本計劃

- 刪 leftover inbound 變體／catch-up（09 波 01 本體）。
- `Packet` 變體再重排（本波之後不得再動）。
