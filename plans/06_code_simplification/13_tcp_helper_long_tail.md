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

- [x] `tests/review_hardening_ingress.rs` 刪本地 `reserve_port`。需要 listen 的測試改 `HeldLoopback::bind()`，`release()` 後立刻讓 `ServerRuntime` bind。flooder／raw frame IO（`write_packet`／`read_packet`／handshake）留下。
- [x] 上表 `temp_world`／`session_slot` 複本改呼叫 `tests/common`。語意必須與 common 現況一致（`temp_world(prefix)` → `icraft-{prefix}-{nanos}`；`session_slot` → `ItemWire::from_stack`）。若某檔刻意用不同 path 規則（pid+底線），**留下**並在證據寫明。
- [x] 各檔本地 `properties()`／種子／`view=4`／`max_players=2` **不**收進 common。04 的契約：share builders，不 share scenarios。
- [x] `review_hardening_adversarial_frames.rs` 不改。
- [x] 不合併 `HeadlessClient` 與 `TcpClient`。不把 `sim_harness` 收進 `tests/common`。
- [x] 測試期望值、timeout、assertion 不變。

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

## 實作與證據

執行前再掃一次本地 helper。`tests/common/tcp_harness.rs` 現況：

- `temp_world(prefix)` → `icraft-{prefix}-{nanos}`
- `session_slot(stack)` → `SessionInventorySlot::from_wire(ItemWire::from_stack(...), can_break, can_place_on)`
- listen bind → `HeldLoopback::bind()` / `release()`

替換：

| Helper | 檔 | 決定 |
| --- | --- | --- |
| `reserve_port` | `review_hardening_ingress.rs` | 刪。listen 測試改 `HeldLoopback`；`release()` 後立刻 `ServerRuntime::new`。flooder／`write_packet`／`read_packet`／handshake 留下。 |
| `temp_world` | `review_hardening_ingress.rs` | 與 common 同 nanos 規則，改 `temp_world(&format!("plan12-ingress-{label}"))`，目錄仍是 `icraft-plan12-ingress-{label}-{nanos}`。 |
| `session_slot` | `authority_gameplay_domains.rs`、`review_hardening_container_click.rs`、`runtime_topology_parity.rs` | bit-identical，改 `common::tcp_harness::session_slot`。 |

留下（path 規則不同，不是 common 的 `icraft-{prefix}-{nanos}`）：

| 檔 | 本地格式 |
| --- | --- |
| `review_hardening_chunk_residency.rs` | `icraft_plan11_{label}_{pid}_{nonce}`（pid+底線） |
| `difficulty_authority.rs` | `icraft-difficulty-{label}-{pid}-{nonce}-{suffix}`（pid + 原子計數） |
| `runtime_topology_parity.rs` `temp_world` | `icraft_runtime_topology_{label}_{unique}`（底線，無 pid） |

未收進 common：各檔 `properties()`、種子、`view=4`、`max_players=2`。未改 `review_hardening_adversarial_frames.rs`。未合併 `HeadlessClient`／`TcpClient`。未改生產碼或測試期望值。

測了什麼（合併後在主工作樹再跑；未改期望值）：

| 指令 | 結果 |
| --- | --- |
| `cargo test --test review_hardening_ingress -- --test-threads=1` | 2 passed |
| `cargo test --test review_hardening_chunk_residency -- --test-threads=1` | 4 passed |
| `cargo test --test difficulty_authority -- --test-threads=1` | 3 passed |
| `cargo test --test authority_gameplay_domains -- --test-threads=1` | 3 passed |
| `cargo test --test review_hardening_container_click -- --test-threads=1` | 9 passed |
| `cargo test --test review_hardening_adversarial_frames -- --test-threads=1` | 2 passed |
| `cargo test --test runtime_topology_parity -- --test-threads=1 --skip plan24_plan22_gameplay_vectors_match_all_runtime_topologies` | 5 passed, 1 filtered |

`plan24_plan22_gameplay_vectors_match_all_runtime_topologies` 在此機 debug 下每個 `ServerRuntime` tick 約數秒 × 200 brew ticks × 3 topology，前景過慢，未等完。該測項未改代碼路徑（只換 `session_slot` 別名），期望值未動。

留下的缺口：

- 三個不同 path 規則的 `temp_world` 仍本地複製。
- `runtime_topology_parity` 的 plan24 未在前景跑完。
- 未開 `icraft-test-support` crate。
