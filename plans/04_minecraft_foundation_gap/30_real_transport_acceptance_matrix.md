# Plan30 — real-transport acceptance matrix

## 定位與邊界

- 基線：`185247d`；本計劃只把已有 authority gameplay domains 接到真實
  `NetworkClient` TCP ingress/egress，並修正驗收文件的拓撲聲明。
- Singleplayer 仍使用 embedded `RuntimeInput`。ListenServer 必須有 local host
  並至少一個（本 vector 使用兩個）真 TCP remote；Dedicated 必須有兩個真 TCP
  clients。
- authority seam 只用來建立起始 world/session fixture 或清除跨 lane 的長生命
  reservation；每個 mutating request、ACK、private projection、reconnect assertion
  都經 socket。fixture 不稱為玩家 E2E。
- 不升 protocol v17、不新增玩法，不納入 GPU/window/audio/DPI/soak 或 network
  metrics race。

## A. 共用 driver 與 bounded domain vector

- [x] `tests/common/tcp_harness.rs` 集中 `NetworkClient` command/event queue、
  fixed-tick wait、response gate、owner-private session projection。避免每個
  topology 複製等待邏輯。
- [x] `tests/plan30_real_transport_acceptance.rs` 在 Listen（local host + 2 TCP
  remotes）與 Dedicated（2 TCP remotes）使用同一 vector：
  fishing cast/duplicate、furnace output、2x2 craft、enchant、anvil rename、
  brew start/fixed ticks/ready/take、player combat/death/legacy respawn、
  out-of-order/stale、owner-private `PlayerSessionUpdate` 與 TCP
  disconnect/reconnect。
- [x] sequence/revision gates 保留。duplicate 不重新執行；domain reject 後下一
  request 使用下一 sequence。fixed-tick fishing reservation 在獨立 lane teardown
  後才進 workstation lane，避免把長生命 hook 的 revision churn 當成另一個
  request 的成功證據；TCP duplicate 的 real ingress 以 duplicate metric 加上
  authority response-cache 中完全相同的 ACK/outcome/revision 證明，因
  `NetworkClient` response gate 會刻意丟棄重複 response。
- [x] fishing reel 的 TCP ingress/egress 已精確驗證為
  `Rejected(InvalidRevision)`（fixed-tick hook 使 delayed reel revision 過期），
  因此只宣稱 cast + cached duplicate pass；完整 reel lifecycle 轉 Plan33。
  Dimension travel 尚無 player-facing seam，轉 Plan32。不得用 direct
  `set_session_dimension` 冒充 E2E。

## B. final acceptance matrix

| Scenario | Singleplayer | Listen TCP | Dedicated + 2 TCP clients |
| --- | --- | --- | --- |
| Foundation | pass（既有 Plan19 harness） | blocked：缺 canonical block-action/mining ingress（Plan31） | 同上 |
| Progression | pass（既有 bounded SimHarness） | blocked：缺 player travel/completion ingress（Plan32） | 同上 |
| SocialAutomation | pass（既有 bounded SimHarness） | blocked：缺 player-authored block/automation ingress（Plan31） | 同上 |

Plan30 的 TCP domain vector 證明 authority/transport parity 的可達 subset，
不會把三條 SimHarness scenario 重新標成網路 E2E。`src/final_acceptance.rs`
對 Listen/Dedicated 維持明確 per-scenario blocked reason。

## C. 後續計劃

- Plan31：authoritative block actions/mining、progress/drop/XP 與跨拓撲 projection。
- Plan32：portal/dimension travel、progression completion、dragon/End City 的
  真 ingress/egress。兩者本計劃只建文件，不實作 mechanics 或 protocol bump。
- Plan33：以最新 owner revision 完成 TCP fishing cast→reel lifecycle；本計劃只
  保留 reel 的精確 InvalidRevision evidence，不繞過 anti-stale gate。

## 驗證紀錄（2026-08-12）

- `cargo fmt --all` 通過。
- `cargo test --test plan30_real_transport_acceptance -- --nocapture`：2 tests
  通過（embedded Singleplayer contract 1、real TCP topology vector 1）；TCP
  test 依序跑 Listen（local host + 2 remotes）與 Dedicated（2 remotes），並
  完成上述 bounded vector、reconnect、owner-private projection、duplicate
  cached-response comparison 與 reel `Rejected(InvalidRevision)` evidence。
  運行時僅有既有 tick-over-budget/writer-close 日誌，無測試 failure。
- `runtime_topology_parity` 的三 topology Plan22 vector 仍是 direct embedded
  `RuntimeInput` parity fixture，不能作真 TCP E2E 證據；Plan30 文件與 Plan24
  文字已分開這兩種證據。
- reconnect review fix：TCP reconnect 的 bounded wait 現在同時等待新
  `player_id`、runtime session count 與 owner-private `PlayerSessionUpdate`，
  不再在 wait 返回後以 timing-sensitive 的立即 assertion 讀 queue；修正後
  debug targeted 10/10、release targeted 10/10 均通過（每次 2 tests）。
- 修正前 release serial 曾在 reconnect assertion 遇到 1 次 event-order race；
  這不是 production failure。修正後的 final full debug/release serial 以
  下方 aggregate 為 gate，兩者均要求 0 failures。
- final full debug serial（`cargo test --locked --no-fail-fast -- --test-threads=1`）：
  lib 688 passed/0 failed/3 ignored（691 total），client binary 819/0/3，
  server binary 2/0/0；authority domains 3/3、persistence 3/3、difficulty
  3/3、headless 2/2、Plan30 2/2、runtime topology 6/6、waterlogging 5/5、
  passive placeholder 1/1、doc-tests 0/0。
- final full release serial（`cargo test --release --locked --no-fail-fast -- --test-threads=1`）：
  同一 aggregate 全數通過（lib 688/0/3、client 819/0/3、server 2/0/0；
  integrations 3/3、3/3、3/3、2/2、2/2、6/6、5/5、1/1；doc-tests 0）。

## 不在本計劃

- canonical block/mining（Plan31）、progression travel/completion（Plan32）、
  village/minecart social network scenario、GPU/window/audio/DPI、metrics race、
  dedicated release-binary/manual-soak artifact（Plan24）或任何新
  protocol/content；release regression 只作本計劃的驗證 gate。
