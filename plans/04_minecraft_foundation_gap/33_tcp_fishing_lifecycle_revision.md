# Plan33 — TCP fishing lifecycle revision boundary

## 定位

- Plan30 的真 TCP vector 能穩定取得 fishing cast ACK 與 owner hook projection，
  但 fixed-tick hook state 在 socket request 到達前會分配新的 session/world
  revision。
- 這不是 Plan30 harness 可以用 fixture 清除 reservation 來掩飾的通過項；本計劃
  只追查 revision hand-off，不新增釣魚內容或放寬 stale gate。

## 精確 acceptance

- [x] NetworkClient 在收到 cast ACK/session update 後能以 latest owner revision
  發送合法 reel；server 不因 transport delay 把仍有效的 hook request 誤判
  `InvalidRevision`。
- [x] cast/reel/duplicate/cancel 的 fixed-tick state、inventory/drop/XP 與
  owner-private projection 在 Listen TCP、Dedicated TCP 和 embedded lane 一致。
- [x] stale/out-of-order request 仍拒絕，且不以接受過期 revision 或跳過 sequence
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

## 完成紀錄（2026-08-12）

- `NetworkClient` 將 `client_sequence == 0` 定義為尚未送出的新 gameplay input，
  在真正寫入 TCP frame 前配置 sequence 並綁定當下最新 owner revision；明確非零
  sequence/revision 的 duplicate、stale 與 out-of-order probe 保持原值。
- 本機 `PlayerSessionUpdate` 通過 replication gate 後也更新 revision high-water，
  因此 cast ACK 後到達的 fixed-tick owner projection 可供下一個新 input 使用。
- authority fixed tick 只推進 `gameplay.revision`，不再把自主 hook/cooldown/brew
  presentation tick 當成 client-authored `last_revision` baseline；已接受的 request 與
  durable mutation 仍照常推進 anti-stale baseline，未放寬 revision/sequence gate。
- `tests/plan33_tcp_fishing_lifecycle.rs` 在 Embedded、Listen TCP、Dedicated TCP 驗證
  cast→nibble→reel、loot/XP/釣竿耐久、cached duplicate、cancel、owner privacy、
  stale 與 out-of-order。Plan30 的 reel expectation 已由精確 `InvalidRevision` blocker
  更新為成功 lifecycle；protocol 維持 v19，沒有 bump。

## Fresh revision compatibility correction（2026-08-13）

Plan33 的 fresh-input rebase 取 supplied `client_revision` 與 client-side
high-water 的較大值，並將 high-water 更新至該值。這保留 stale/out-of-order
probe 的非零 sequence/revision 原值，同時避免 legacy fresh envelopes 在
high-water 尚為 0 時把明確 revision（例如 17）降成 0。

Focused debug and release checks passed after this correction: the fresh-revision unit
regression, `legacy_client_inputs_are_single_gameplay_envelopes`, the three Plan33
topology tests, and the two Plan30 regression tests. The existing Plan33 and Plan30
topology vectors remain the behavioral acceptance lanes; no new numbered plan is
required. Repository-wide full-suite evidence is intentionally not claimed here.

