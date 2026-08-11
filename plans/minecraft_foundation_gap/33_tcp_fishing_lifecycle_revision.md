# Plan33 — TCP fishing lifecycle revision boundary

## 定位

- Plan30 的真 TCP vector 能穩定取得 fishing cast ACK 與 owner hook projection，
  但 fixed-tick hook state 在 socket request 到達前會分配新的 session/world
  revision。
- 這不是 Plan30 harness 可以用 fixture 清除 reservation 來掩飾的通過項；本計劃
  只追查 revision hand-off，不新增釣魚內容或放寬 stale gate。

## 精確 acceptance

- [ ] NetworkClient 在收到 cast ACK/session update 後能以 latest owner revision
  發送合法 reel；server 不因 transport delay 把仍有效的 hook request 誤判
  `InvalidRevision`。
- [ ] cast/reel/duplicate/cancel 的 fixed-tick state、inventory/drop/XP 與
  owner-private projection 在 Listen TCP、Dedicated TCP 和 embedded lane 一致。
- [ ] stale/out-of-order request 仍拒絕，且不以接受過期 revision 或跳過 sequence
  gate 來修正 race；補 deterministic transport test 不用 unbounded sleep。

## 預計檔案與測試

- `src/network/client.rs`（latest revision projection/egress seam）、必要時
  `src/server_runtime.rs`/`src/network/server.rs` 的 bounded ACK ordering；不預設
  要 protocol bump。
- `tests/common/tcp_harness.rs`、`tests/plan30_real_transport_acceptance.rs`、
  `tests/plan33_tcp_fishing_lifecycle.rs`。

## Plan30 證據邊界

- 2026-08-12 真 TCP run：cast expected revision = authority world revision at
  request construction (fixture-dependent 4–5)，ACK accepted at next revision
  (5–6)，owner hook projection present；在 response wait 的 fixed ticks 後，
  session revision 已前進（約 9–10）。reel 使用當下 expected revision，但
  server 回 `GameplayOutcome::Rejected { reason: InvalidRevision }`，因 hook
  tick 在 request ingress 前再次更新 session revision。
- 因此 Plan30 只勾 cast/duplicate evidence；reel lifecycle 明確 blocked by
  Plan33，fixture teardown 只清除下一 lane 的 reservation，不能改寫上述結果。

## 不在本計劃

- 新魚類/戰利品、renderer/GPU、manual QA、transport metrics race、或任何放寬
  anti-stale/sequence/security gate 的旁路。

