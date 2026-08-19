# Plan27 — `ServerRuntime` 投影／ingress 拆檔

## 定位

`src/server_runtime.rs` 約 4,622 行。`session_sync.rs` 已抽出（16）。剩下合成根
仍同時擁有建構、tick 編排、inbound `ServerToHost`、以及 interest 投影：

| 約略行 | 內容 |
| --- | --- |
| 878–1070 | `ServerRuntime` 欄位、`new`／`new_embedded`／`construct` |
| 1071–1492 | `tick_with_output` 編排 + dedicated console |
| 1493–2864 | `handle_event`（ingress） |
| 2873– | `route_authority_snapshot`（投影） |

`tick_with_output` 必須留在根檔當短編排器（drain → `authority.tick` → transfer →
route → evict → metrics → autosave）。不要把 tick 順序打散到三個檔各寫一段。

## 前置

16、17 已完成。可與 26、29、30 並行。不要跟 16 再改 `session_sync.rs` 的寫入口。
不要跟 20 搶 `network/server.rs`（20 已完成；本計劃只動 runtime 側）。

## 精確 acceptance

- [ ] 至少拆出（名稱可微調，責任不可混）：
      - `src/server_runtime/ingress.rs`：`handle_event` 與 join／leave／pose／
        gameplay 入隊的私有 helper
      - `src/server_runtime/projection.rs`：`route_authority_snapshot` 與
        interest-routed fanout
- [ ] `tick_with_output` 留在 `server_runtime.rs`，仍依序：
      bounded drain → `authority.tick` → dimension transfer → route →
      evict uninteresting → container closures → metrics → autosave。
      不得重排，不得把 autosave 失敗改成中止 tick。
- [ ] 公開方法留在 `ServerRuntime`。子檔是 `impl ServerRuntime`，不是新 runtime 型別。
- [ ] 不得把 `InterestSet` 搬進 `AuthorityCore`。不得合併兩個 session 型別。
- [ ] pose／chat／gameplay ingress 預算與 `QueueFull` 語意不變。
- [ ] reliable 投影 250 ms 視窗與驅逐不變。
- [ ] 既有測試期望值不變。

## 預計檔案與測試

- 新增：`src/server_runtime/ingress.rs`、`src/server_runtime/projection.rs`。
- 修改：`src/server_runtime.rs`（`mod` + 短 `tick_with_output`）。
- 測試：
  - `cargo test --lib server_runtime::`
  - `cargo test --test review_hardening_session_lifecycle -- --test-threads=1`
  - `cargo test --test review_hardening_ingress -- --test-threads=1`
  - `cargo test --test review_hardening_chunk_residency -- --test-threads=1`
  - `cargo test --test runtime_topology_parity -- --test-threads=1`
  - `cargo test --test headless_server_authority -- --test-threads=1`

## 建議階段

1. 搬 `handle_event` 整段到 `ingress.rs`。跑 session lifecycle 與 ingress。
2. 搬 `route_authority_snapshot`。跑 residency 與 topology parity。
3. 確認根檔 `tick_with_output` 仍是短編排器。

## 不在本計劃

- 塌縮 `HostToServer` 成 `Packet`。
- 統一三套指令語言。
- 把 dedicated console 改走 `commands::parse`。
- leftover cfg、desktop `State` 拆檔。
