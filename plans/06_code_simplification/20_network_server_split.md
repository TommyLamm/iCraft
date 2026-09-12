# Plan20 — `network/server.rs` 按既有型別拆檔

## 定位

`src/network/server.rs` 約 5,500 行，約 43% 是測試。Interest **不**在這個檔
（`authority/interest.rs` + `ServerRuntime` 路由）。真正縫已經是型別：

| 約略行 | 內容 |
| --- | --- |
| 開頭–360 | metrics、queue、`reliable_send` |
| 332–719 | `ServerConfig`、`ServerToHost`、`HostToServer` |
| 720–1203 | handshake identity、`ClientSession`、rate limiter、三個 mailbox、pre-auth |
| 1215–1547 | spawn／run、`prepare`／`wrap_legacy` 呼叫、route |
| 1549–2268 | `run_client`：handshake + leftover ingress + 活信封 |
| 2320–3121 | `handle_host_command`：`HostToServer` → `Packet` |
| 3143+ | 測試 |

08 把 `wrap_legacy` 放在 `protocol.rs`——留下。本計劃不發明 network 內的 interest 模組，
也不把 `HostToServer` 塌成 `Packet`（README 已寫 blast radius 太大；17 只縮小 desktop 送出）。

## 前置

08 已完成。**建議** 17 已合併：生產碼不再建構 leftover `GameToClient`，拆 egress 時比較不會
跟送出改動搶同一段 match。未合併也能做——只搬家，不改 match 臂語意。

不要跟 17 同一 PR。

## 精確 acceptance

- [ ] 在 `src/network/` 下拆出（名稱可微調，責任不可混）：
      - `channels.rs`：`HostToServer`、`ServerToHost`、`HostEventSender`
      - `session.rs`（或 `server_session.rs`，避免與權威 session 撞名）：`ClientSession`、
        `GameplaySessionState`、`RequestRateLimiter`、`PreAuthSlot`、三個 mailbox
      - `egress.rs`：`handle_host_command` + `send_to`／`broadcast_*`／evict
      - `ingress.rs`：`run_client`、handshake、leftover inbound match、`route_gameplay_request`
- [ ] `server.rs` 留下 `NetworkServer::spawn`／`run` 與 `pub use`。
      `use crate::network::server::{HostToServer, ServerToHost, NetworkServer}` 零改。
- [ ] `wrap_legacy` 留在 `protocol.rs`。不得複製第三份適配器。
- [ ] 不得改 handshake 逾時、pre-auth cap（`2 * max_players`）、ingress 預算（pose 20／chat 8／gameplay 120）、
      250 ms reliable enqueue、256 事件隊列、2 MiB 幀上限。
- [ ] 不得刪、重排 `Packet` variant。不得 protocol bump。
- [ ] leftover inbound 仍接受並包成 `GameplayRequest`。`Action::Use` 仍 `None`。
- [ ] 單元測跟著被測型別走（handshake／rate limit／mailbox 可進子檔的 `#[cfg(test)]`），
      期望值不變。
- [ ] `client.rs` **不**在本計劃拆。

## 預計檔案與測試

- 新增：`src/network/channels.rs`、`src/network/session.rs`（或 `server_session.rs`）、
  `src/network/egress.rs`、`src/network/ingress.rs`。
- 修改：`src/network/mod.rs`、`src/network/server.rs`。
- 測試：
  - `cargo test --lib network::`
  - `cargo test --test plan30_real_transport_acceptance -- --test-threads=1`
  - `cargo test --test review_hardening_adversarial_frames -- --test-threads=1`
  - `cargo test --test review_hardening_ingress -- --test-threads=1`
  - `cargo test --test review_hardening_session_lifecycle -- --test-threads=1`
  - `cargo check --bin icraft-server`

## 建議階段

1. 先搬 `HostToServer`／`ServerToHost` 到 `channels.rs` + re-export。`cargo test --lib network::`。
2. 搬 mailbox／session／rate limiter。
3. 剪 `run_client` → `ingress`、`handle_host_command` → `egress`。每步 check。
4. 跑 TCP／adversarial／ingress。

## 不在本計劃

- `HostToServer` → `(Option<PlayerId>, Packet)` 塌縮。
- 拆 `client.rs` recv／send。
- 改 `protocol.rs` 的 wire enum。
- 把 interest 搬進 `network/`。
- 刪 leftover inbound 臂。
