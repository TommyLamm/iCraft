# Plan16 — 測試 `request()`／`TestServer` 合併

## 定位

Plan 13 合併了 integration `TcpClient`／`temp_world`，但：

- `tests/review_hardening_chunk_residency.rs`、`review_hardening_ingress.rs`、`review_hardening_session_lifecycle.rs`、`review_hardening_container_click.rs`、`headless_server_authority.rs`、`authority_gameplay_domains.rs`、`waterlogging_authority.rs` 各自有 ~15–25 行本地 `fn request()`，與 `tests/common/tcp_harness.rs` 的 `gameplay_request()` 重複。
- `src/network/server.rs` 的 `#[cfg(test)] TestServer`（bind／connect／handshake／`next_event_matching`）與 harness 的 Windows self-connect workaround 再複製一份。

`waterlogging_authority.rs` 走 `AuthorityCore` 而非 TCP，需要 `authority_request(core, session_id, ...)` 而非 TcpClient。

## 前置

無。純測試。Windows ephemeral port 重試必須保留。

## 精確 acceptance

- [x] TCP 測試改用 `tcp_harness::{gameplay_request, current_revision, session_slot}`（或同等），刪本地 `fn request()`。
- [x] `AuthorityCore` 測試改用共用 `authority_request` helper（可放 `tests/common/` 或 `authority` 測試模組）。
- [x] `TestServer` 併入 `tests/common/` 或 `network` test helper；`server.rs` 單元測試改呼叫共用型別。
- [x] Windows loopback self-connect 重試邏輯只留一份。
- [x] `cargo test --test review_hardening_ingress`、`--test review_hardening_session_lifecycle`、`cargo test --lib network::server` 通過。

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

## 實作與證據

- 新增 `tests/common/authority_harness.rs`：`authority_request` 依 live session dimension／revision 組 `GameplayRequest`（對齊 `gameplay_request`）。
- TCP 測試刪本地 `fn request()`：`review_hardening_ingress`／`session_lifecycle`／`chunk_residency`／`headless_server_authority` 改呼叫 `tcp_harness::gameplay_request`（stale revision 用 struct update 覆寫）。ingress flood 測試改等 peer 進入 `runtime.players`＋authority session 後再 stamp。
- AuthorityCore 測試：`authority_gameplay_domains`／`review_hardening_container_click`／`waterlogging_authority` 改共用 `authority_request`（waterlogging 保留薄 `fluid_request` 只包固定 FluidUse 座標）。
- `src/network/loopback_test.rs`：抽出 `LoopbackTestServer`＋`connect_loopback_stream`；`server.rs` 單元測試改用共用型別；`host_queue_full_*` 不再內嵌第二份 self-connect。
- sync 路徑：`tcp_harness::connect_loopback_std`；`review_hardening_adversarial_frames` 改用之（手組 frame 未改）。
- `ARCHITECTURE.md` Tests／Network 列補上 harness／`loopback_test`。
- 未刪 `SimHarness`／`final_acceptance`；未改 production handshake；未去重 `microbench`。`runtime_topology_parity` 的 harness method `request` 與 `server_runtime` 單元測試專用 container-open helper 不在本計劃檔案清單內，保留。

### 測試

```
cargo test --lib network::server -- --test-threads=1
  32 passed

cargo test --test review_hardening_ingress -- --test-threads=1
  2 passed

cargo test --test review_hardening_session_lifecycle -- --test-threads=1
  3 passed

cargo test --test review_hardening_chunk_residency --test review_hardening_container_click --test authority_gameplay_domains --test waterlogging_authority --test headless_server_authority -- --test-threads=1
  各檔 test result ok（chunk_residency 3、container_click 2、authority_gameplay_domains 9、waterlogging 5、headless 依編譯順序其一為 4）
```
