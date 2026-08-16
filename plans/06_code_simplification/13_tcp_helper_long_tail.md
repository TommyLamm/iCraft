# Plan13 — TCP 測試 helper 長尾

## 定位

04 已把 Plan30–34、`review_hardening_block_use_rejected`、`review_hardening_session_lifecycle`、
`headless_server_authority` 的 listen bind 改成 `HeldLoopback`，並在
`tests/common/tcp_harness.rs` 提供 `temp_world`／`loopback_properties`／`gameplay_request`／`session_slot`。

長尾仍在（掃描當日，執行前再 grep）：

| Helper | 仍本地複製 |
| --- | --- |
| `reserve_port()`（drop-then-rebind TOCTOU） | `tests/review_hardening_ingress.rs:28` |
| `temp_world` | `review_hardening_ingress.rs`、`review_hardening_chunk_residency.rs`、`difficulty_authority.rs`、`runtime_topology_parity.rs` |
| `session_slot` | `authority_gameplay_domains.rs`、`review_hardening_container_click.rs`、`runtime_topology_parity.rs` |

`review_hardening_adversarial_frames.rs` 與 ingress **flooder** 必須繼續走 raw `TcpStream`。
04 已允許 `headless_server_authority` 保留 `HeadlessClient`——本計劃也不合併它。

## 前置

04 已完成。可與 11、12 並行。不改生產碼。

## 精確 acceptance

- [ ] `tests/review_hardening_ingress.rs` 刪本地 `reserve_port`。需要 listen 的測試改 `HeldLoopback::bind()`，`release()` 後立刻讓 `ServerRuntime` bind。flooder／raw frame IO（`write_packet`／`read_packet`／handshake）留下。
- [ ] 上表 `temp_world`／`session_slot` 複本改呼叫 `tests/common`。語意必須與 common 現況一致（`temp_world(prefix)` → `icraft-{prefix}-{nanos}`；`session_slot` → `ItemWire::from_stack`）。若某檔刻意用不同 path 規則（pid+底線），**留下**並在證據寫明。
- [ ] 各檔本地 `properties()`／種子／`view=4`／`max_players=2` **不**收進 common。04 的契約：share builders，不 share scenarios。
- [ ] `review_hardening_adversarial_frames.rs` 不改。
- [ ] 不合併 `HeadlessClient` 與 `TcpClient`。不把 `sim_harness` 收進 `tests/common`。
- [ ] 測試期望值、timeout、assertion 不變。

## 預計檔案與測試

- 修改：`tests/review_hardening_ingress.rs`、`tests/review_hardening_chunk_residency.rs`、
  `tests/difficulty_authority.rs`、`tests/runtime_topology_parity.rs`、
  `tests/authority_gameplay_domains.rs`、`tests/review_hardening_container_click.rs`。
  必要時只加 `tests/common` 的 re-export，不改 builder 語意。
- 測試（各檔 `--test-threads=1`，Windows 一次一個）：
  - `cargo test --test review_hardening_ingress`
  - `cargo test --test review_hardening_chunk_residency`
  - `cargo test --test difficulty_authority`
  - `cargo test --test runtime_topology_parity`
  - `cargo test --test authority_gameplay_domains`
  - `cargo test --test review_hardening_container_click`
  - `cargo test --test review_hardening_adversarial_frames`

## 建議階段

1. 對每個本地 helper 做 diff：與 `tcp_harness.rs` bit-identical 才替換。
2. 先改 ingress 的 `reserve_port` → `HeldLoopback`，跑該檔（含 flooder）。
3. 再替換 `temp_world`／`session_slot`。
4. 跑表列測試。

## 不在本計劃

- 改生產網路、ingress 預算、handshake。
- 開 `icraft-test-support` crate。
- 把 `sim_harness`／`final_acceptance` 接到 `ServerRuntime`。
- 讓 adversarial／flood 走 `TcpClient`。
