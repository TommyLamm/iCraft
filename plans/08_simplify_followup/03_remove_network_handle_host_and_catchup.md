# Plan03 — 刪 `NetworkHandle::Host` 與桌面 catch-up

## 定位

Singleplayer / Host 明確設成 `NetworkHandle::None`；第二個 GPU 擁有的 `NetworkServer` 會變成第二條權威。`Host { ... }` 只出現在 `state.rs` 單元測試，以及 `network_inbound.rs` 一長串廣播方法。

Listen-host 輸出已由 `ServerRuntime` 走 TCP。`process_join_catchups` 在有 runtime 時直接 return，函式體是空的，但仍每 tick 呼叫。

## 前置

01（避免 leftover 仍走 `NetworkHandle::Host` 廣播）。

## 精確 acceptance

- [x] `NetworkHandle` 只留 `None | Client`。
- [x] 刪所有 `if let NetworkHandle::Host` 廣播／catch-up 方法。
- [x] 刪 `CatchupStatus`、`pending_player_catchups`、空的 `process_join_catchups` 與其每 tick 呼叫。
- [x] 單元測試改成 `None`／`Client` 或不測 Host 傳輸。
- [x] `cargo check --all-targets` 通過。

## 預計檔案與測試

- `src/presentation/network_inbound.rs`、`network_event.rs`、`src/state.rs`
- 驗證：`cargo test --bin icraft` 中 network handle 相關測試

## 建議階段

1. 確認生產建構點只有 `None`／`Client`（`state.rs` 約 4001）。
2. 刪 enum 臂，用編譯錯誤清方法。
3. 刪 catch-up 欄位與空函式。

## 不在本計劃

- 刪 presentation `SaveManager`（04）。
- 改 `ServerRuntime` TCP listen-host。

## 實作與證據

### 改了什麼

- `NetworkHandle` 只留 `None | Client`；刪 Host drain 與所有 Host 廣播／catch-up 方法。
- 刪 `CatchupStatus`、`PlayerCatchupEntry`、`pending_player_catchups`、空的 `process_join_catchups` 與每 tick 呼叫。
- 清掉只服務 leftover Host 傳輸的 inbound 臂（catch-up ACK、`ClientRespawnRequest`、`ChatFromClient`、Host `GameplayRequest`）與桌面側 Host 廣播呼叫。
- `host_inbound_gameplay_request_preserves_authenticated_player_id` 改成 `network_handle_none_drains_no_inbound_events`；Client drain 測試保留。
- `ARCHITECTURE.md`：`NetworkHandle` 契約改成只有 `None`／`Client`。

### 測了什麼

- `cargo check --all-targets`：通過。
- `cargo test --bin icraft -- network_handle`：2 passed（`network_handle_none_drains_no_inbound_events`、`network_handle_preserves_client_chat_and_disconnect_payloads`）。
- `cargo test --bin icraft -- client_block_change_is_classified_as_host_authority`：1 passed。
- `cargo test --bin icraft`：181 passed。

### 留下的缺口

- presentation `SaveManager`、`mutation_revisions` 仍在（04）。
- `ServerRuntime` TCP listen-host 與 `ServerToHost`／`HostToServer` 通道未改。
- `PresentationTopology::LegacyOwner` 仍在（06）。
- `RemotePlayerState` 上部分 Host 廣播用欄位現在只寫不讀；debug overlay 的 `network_catchup_mailbox_full` 永遠為 0。
