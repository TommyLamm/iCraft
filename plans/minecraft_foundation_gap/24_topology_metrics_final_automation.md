# 24 — 三拓撲對照、Metrics 穩定化與最終自動驗收

## 定位

- 優先級：P3，承接 Plan23 未勾選的自動驗收；前置提交：`42cff75`
- 本計劃只補三拓撲 runtime parity、workstation/session projection 證據、既有 TCP metrics race、
  完整 debug/release regression 與 30 分鐘 dedicated headless soak
- 不新增玩法內容、不重構 renderer；GPU/window/audio/DPI 人工 artifact 仍獨立記錄
- 建議提交上限：1

## A. Plan22 三拓撲 runtime vector

- [x] 以同一 `ServerRuntime` scheduling/request/ACK/snapshot harness 在 Singleplayer、
  ListenServer、Dedicated 執行 fishing、furnace/craft/enchant/brew/anvil、combat/death/respawn。
- [x] accepted/rejected、duplicate/out-of-order/stale 的 outcome、session inventory/health/revision、
  world/entity/container delta 在三拓撲一致；fixture 可建立初始 world/session，但不得 direct core
  mutation 代替被驗證的 request/tick/result。
- [x] owner `PlayerSessionUpdate` 足以還原 craft/enchant/anvil 結果與 rich metadata；這些是瞬時
  transaction，若無獨立 progress state，文件明確記錄以 ACK + session snapshot 為完成契約。
- [x] dimension transfer/reconnect 不洩漏其他 dimension 或其他玩家的 workstation/session payload。

## B. TCP transport metrics race

- [x] `outbound_packets/outbound_bytes` 對成功 frame 在 peer 可觀察該 frame 前已發布；write 失敗會
  rollback，不能把失敗 frame 計入成功數。
- [x] 保持 inbound/outbound exact counts、queue gauges 與 relaxed/ordering 語義清楚；不以 sleep/yield
  掩蓋 race。
- [x] `transport_metrics_count_exact_successful_tcp_frames` release isolated 重跑至少 50 次全通過，
  並覆蓋 write failure/rollback。

## C. 完整自動 regression 與 soak

- [x] `cargo fmt --all -- --check`、`git diff --check`。
- [x] `cargo test --locked --no-fail-fast` 與 `cargo test --release --locked --no-fail-fast` 全通過
  （允許既有明確 ignored tests，但不得有 failure）。
- [x] `cargo check --all-targets`、`cargo check --release --locked` 全通過。
- [x] `icraft-server --once`、短跑與 30 分鐘 dedicated headless soak 有 dated command/log/metrics；
  soak 無 crash、panic、queue growth、save failure 或 tick stall。
- [x] 更新 Plan18/19/21/23 的交接狀態與 README 總狀態，只修改由本 plan 真正關閉的自動項目。

## 不在本計劃

- GPU Host+Join、視窗比例/DPI、audio device、實機輸入與截圖 artifact。
- 新玩法、內容、相容格式或與本 plan failure 無關的 warning 清理。

## 驗證紀錄（2026-08-11）

- `tests/runtime_topology_parity.rs` 的
  `plan24_plan22_gameplay_vectors_match_all_runtime_topologies` 通過（1 test，
  3 topology iterations，0 failed）。同一 fixed-tick request/ACK/snapshot lane
  覆蓋 fishing cast/reel（duplicate 不產生第二 hook）、furnace output、2x2
  craft、enchant、anvil rename、brew ready/take（ready tick 不自動 debit，
  duplicate take 不重複產物）、entity/player combat、death inventory clear、
  respawn health/velocity reset、stale/out-of-order rejection、dimension
  transfer 與 named-session reconnect。owner-private `PlayerSessionUpdate` 與
  workstation rich metadata assertions 均在每個 topology 執行。
- `cargo test --lib network::server::tests`：35 passed；其中 outbound reservation
  fault-injection test 先觀察成功 reservation，再確認 write failure rollback。
  生產 writer/connection send path 均經 `send_with_outbound_metrics`，沒有以
  sleep/yield 掩蓋 ordering。release isolated
  `transport_metrics_count_exact_successful_tcp_frames` 連續 50/50 通過。
- `cargo fmt --all -- --check`、`git diff --check`、`cargo check --all-targets`、
  `cargo check --release --locked` 通過。完整 debug/release
  `cargo test --locked --no-fail-fast` 與 `cargo test --release --locked
  --no-fail-fast` 均零 failure（library targets 分別 656 passed/3 ignored、
  787 passed/3 ignored，其餘 integration targets 亦全通過）。
- `icraft-server --once`（ticks=1）及 `--ticks 100` 短跑均 exit 0，metrics
  `queue_depth=0`, `queue_full=0`, `saves=1`。新增的 bounded
  `--duration-seconds` 只供既有 headless soak CLI，以固定 50ms tick cadence
  執行 dated 1800 秒 dedicated run；完整 command、PID、health samples 與
  metrics 保存在
  `artifacts/plan24_20260811_verification.md` 及
  `artifacts/headless_wall_soak_20260811_220121.log`。該 run 由
  `22:01:21.899+08` 至 `22:31:22.016+08` exit 0，final metrics
  `ticks=35842`, `queue_depth=0`, `queue_full=0`, `saves=6`,
  `max_tick_us=28547`, `last_save_ms=26`，無 panic/error/save failure。

Plan24 自動驗收因此完成；GPU/window/audio/DPI、實機 Host+Join 與其他 manual
visual artifacts 仍明確不在本計劃，不能由 headless 結果代替。
