# Plan13 — 測試 `TcpClient`／`temp_world` 合併

## 定位

`tests/common/tcp_harness.rs` 已有 `TcpClient`、`HeldLoopback`、`temp_world`、`loopback_properties`、`drive_until`、`wait_for_response`。複本：

- `tests/headless_server_authority.rs`：近乎完整複製 `TcpClient` + 本地 `wait_for_response`
- `tests/difficulty_authority.rs`、`runtime_topology_parity.rs`：本地 `temp_world` + bind 立刻 drop
- `review_hardening_chunk_residency.rs`、`plan32_progression_travel.rs`、`review_hardening_session_lifecycle.rs`、`review_hardening_chunk_restore.rs`、`authority_persistence.rs`：各寫一份 `temp_dir().join(format!(icraft-…))`

`headless_server_authority.rs` 的 `TempWorld` Drop 清目錄比 harness 多清理；路徑仍可呼叫 `temp_world`，Drop 可留。

對抗性 `TcpStream` 手組 frame **不要**改走 `Packet::encode`。

## 前置

無。

## 精確 acceptance

- [x] 刪 `HeadlessClient`，改用 harness `TcpClient`。
- [x] 普通 temp 路徑改 `tcp_harness::temp_world(prefix)`；port 改 `HeldLoopback`。
- [x] 場景專屬 `properties()` 維持本地。
- [x] 合法 round-trip 測試可用 `encode_frame`；對抗性畸形 header 不動。
- [x] 現有 TCP 整合測試通過。

## 預計檔案與測試

- `tests/common/tcp_harness.rs` 與上表 tests
- 驗證：`cargo test --test headless_server_authority`；`runtime_topology_parity`；chunk_residency；session_lifecycle

## 建議階段

1. headless 客戶端合併。
2. temp_world／HeldLoopback 替換。
3. 合法 write_packet 改 frame helper（可與 02 無關）。

## 不在本計劃

- 對抗性 frame 測試改走 `NetworkClient`。
- 改 `src/network/client.rs` 兩份 checksum 閉包以外的產品碼（checksum 可順手抽測試 helper）。

## 實作與證據

- `tests/headless_server_authority.rs` 刪本地 `HeadlessClient`。客戶端改 harness `TcpClient`；`drive_pair_until`／`drive_one_until`／`wait_for_pair_response` 只包 `drive_until`／`wait_for_response`。`TempWorld` 路徑改 `temp_world("headless-authority")`，Drop 清目錄仍留。場景 `properties()`（`max_players=2`、種子）仍本地。
- harness `TcpClient` 補 `connected`／container open／click helpers，讓 headless 投影斷言不必再複製一份 socket 客戶端。
- 普通 temp 路徑改 `tcp_harness::temp_world(prefix)`：`difficulty_authority`、`runtime_topology_parity`、`review_hardening_chunk_residency`、`plan32_progression_travel`、`review_hardening_session_lifecycle`、`review_hardening_chunk_restore`、`authority_persistence`。Listen／dedicated bind 改 `HeldLoopback`（`release()` 後立刻 `ServerRuntime` bind）；embedded-only 用 `HeldLoopback` 取代 hardcoded／bind-立刻-drop port。
- `review_hardening_ingress.rs` 合法 `write_packet` 改 `Packet::encode_frame`。`review_hardening_adversarial_frames.rs` 未改（手組畸形 header 仍走 `write_frame`／`encode()`，不走 `NetworkClient`／`Packet::encode_frame`）。
- 未改產品碼，未抽 `src/network/client.rs` 兩份 checksum 閉包。未改 `ARCHITECTURE.md`。

### 測試

```
cargo test --test headless_server_authority
  2 passed

cargo test --test runtime_topology_parity --test review_hardening_chunk_residency --test review_hardening_session_lifecycle -- --test-threads=1
  6 + 4 + 3 passed

cargo test --test difficulty_authority --test authority_persistence
  3 + 4 passed

cargo test --test plan32_progression_travel --test review_hardening_ingress --test review_hardening_chunk_restore -- --test-threads=1
  5 + 2 + 5 passed
```
