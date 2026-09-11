# Plan16 — 測試 `request()`／`TestServer` 合併

## 定位

Plan 13 合併了 integration `TcpClient`／`temp_world`，但：

- `tests/review_hardening_chunk_residency.rs`、`review_hardening_ingress.rs`、`review_hardening_session_lifecycle.rs`、`review_hardening_container_click.rs`、`headless_server_authority.rs`、`authority_gameplay_domains.rs`、`waterlogging_authority.rs` 各自有 ~15–25 行本地 `fn request()`，與 `tests/common/tcp_harness.rs` 的 `gameplay_request()` 重複。
- `src/network/server.rs` 的 `#[cfg(test)] TestServer`（bind／connect／handshake／`next_event_matching`）與 harness 的 Windows self-connect workaround 再複製一份。

`waterlogging_authority.rs` 走 `AuthorityCore` 而非 TCP，需要 `authority_request(core, session_id, ...)` 而非 TcpClient。

## 前置

無。純測試。Windows ephemeral port 重試必須保留。

## 精確 acceptance

- [ ] TCP 測試改用 `tcp_harness::{gameplay_request, current_revision, session_slot}`（或同等），刪本地 `fn request()`。
- [ ] `AuthorityCore` 測試改用共用 `authority_request` helper（可放 `tests/common/` 或 `authority` 測試模組）。
- [ ] `TestServer` 併入 `tests/common/` 或 `network` test helper；`server.rs` 單元測試改呼叫共用型別。
- [ ] Windows loopback self-connect 重試邏輯只留一份。
- [ ] `cargo test --test review_hardening_ingress`、`--test review_hardening_session_lifecycle`、`cargo test --lib network::server` 通過。

## 預計檔案與測試

- `tests/common/tcp_harness.rs`、`tests/common/mod.rs`
- 上列 `tests/*.rs`、`src/network/server.rs`
- 驗證：上述窄測試；必要時 `cargo test --test plan30_real_transport_acceptance`

## 建議階段

1. 擴展 harness helper（revision／dimension stamp）。
2. 遷移 hardening 測試。
3. 抽 `LoopbackTestServer`，刪 `TestServer` 複本。

## 不在本計劃

- 刪 `SimHarness`／`final_acceptance`。
- 改 production 網路握手。
- 把 `microbench.rs` 去重（lib vs bin 雙編譯；低優先，可順手 `#[path]` 但不必須）。
