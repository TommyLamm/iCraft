# Plan03 — 刪 `NetworkHandle::Host` 與桌面 catch-up

## 定位

Singleplayer / Host 明確設成 `NetworkHandle::None`；第二個 GPU 擁有的 `NetworkServer` 會變成第二條權威。`Host { ... }` 只出現在 `state.rs` 單元測試，以及 `network_inbound.rs` 一長串廣播方法。

Listen-host 輸出已由 `ServerRuntime` 走 TCP。`process_join_catchups` 在有 runtime 時直接 return，函式體是空的，但仍每 tick 呼叫。

## 前置

01（避免 leftover 仍走 `NetworkHandle::Host` 廣播）。

## 精確 acceptance

- [ ] `NetworkHandle` 只留 `None | Client`。
- [ ] 刪所有 `if let NetworkHandle::Host` 廣播／catch-up 方法。
- [ ] 刪 `CatchupStatus`、`pending_player_catchups`、空的 `process_join_catchups` 與其每 tick 呼叫。
- [ ] 單元測試改成 `None`／`Client` 或不測 Host 傳輸。
- [ ] `cargo check --all-targets` 通過。

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
