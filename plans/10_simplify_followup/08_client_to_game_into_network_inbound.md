# Plan08 — `ClientToGame` 併入 `NetworkInbound`

## 定位

join 半邊同樣是三層鏡像：

| 層 | 位置 | 規模 |
| :--- | :--- | :--- |
| client decode：`Packet` → `ClientToGame` | `network/client.rs` 558–1067 | ~510 行 |
| `ClientToGame` enum | `client.rs` 114–286 | ~30 變體 |
| `NetworkHandle::drain_inbound`：`ClientToGame` → `NetworkInbound` | `presentation/network_inbound.rs` 478–765 | ~287 行純 1:1 map（差異只有 `BlockChange` vs `AuthoritativeBlockChange`、`{ reason }` vs `Disconnected(String)`） |
| `NetworkInbound` enum | `network_inbound.rs` 33–195 | 與 `ClientToGame` 同構 |
| embedded：`RuntimePresentationEvent` → `NetworkInbound` | `state.rs` 4118–4300+ | 第二份 map，目標同一 enum |
| `handle_single_network_event` | `network_event.rs` | 第三次 match |

grep：`ClientToGame::` 101、`NetworkInbound::` 73。

## 前置

09 波 01（`Packet` 定案）。與 Plan 07 可並行；07 落地後 embedded 側的 map 直接消失。

## 精確 acceptance

- [ ] 遊戲執行緒 drain 的是 `Packet`（或 `NetworkInbound` = `ClientToGame`，二選一並刪另一個）。
- [ ] `drain_inbound` 的 1:1 map 刪除；`protocol_version` 在 decode 處剝一次（Plan 09 之後這個欄位本身消失）。
- [ ] `state.rs` `project_runtime_presentation_event` 的 map 刪除（embedded 與 join 都送同一型別進 `handle_single_network_event`）。
- [ ] `NetworkStaging` 保留（它以變體分類，不依賴第二 enum）。
- [ ] wire 不變。
- [ ] `network_inbound` 單元測試改對新型別；分類測試（pose／latest-state 覆寫語意）保留。

## 預計檔案與測試

- 改：`src/network/client.rs`、`src/presentation/{network_inbound,network_event,embedded_runtime}.rs`、`src/state.rs`
- 驗證：`cargo test --lib network::client::`；`cargo test --bin icraft presentation::`；`tests/review_hardening_join_projection.rs`；`tests/plan30_real_transport_acceptance.rs`

## 建議階段

1. 選定目標型別（建議：直接 `Packet`，因為 Plan 07 讓 server 也產 `Packet`）。
2. client decode 只做 `Packet::decode` + 版本檢查；刪 `ClientToGame`。
3. 刪 `drain_inbound` map；`handle_single_network_event` 改 match `Packet`。
4. 刪 `state.rs` 的 embedded map。

## 不在本計劃

- server 半邊（Plan 07）。
- `client.rs` 測試模組搬家（Plan 27）。
