# 23 — State／Runtime 權威接線與三拓撲收斂

## 定位

- 優先級：P3，承接 Plan21 Phase C；前置提交：`dfdf1d8`
- 本計劃只收斂既有 State input、NetworkClient request、ServerRuntime presentation 與
  singleplayer/listen/dedicated common vectors；不新增玩法內容、不處理 metrics race、GPU 或 soak
- 建議提交上限：1；完成後才可開始最終 regression/soak 計劃

## 現況證據

- Singleplayer/Host 已建立 `EmbeddedRuntimeBridge`，但 `State::submit_authority_request` 在沒有
  embedded runtime 時直接回 `None`，Join Client 尚未共用 typed `GameplayRequest` 出站路徑。
- `use_fishing_rod`、workstation、combat/item-use 等多個輸入只以 `has_in_process_runtime()`
  區分；Join Client 仍可能落入舊本地 mutation 或靜默 no-op。
- `project_authority_container` 仍明示缺 typed presentation payload；三拓撲 parity 尚未覆蓋
  Plan22 gameplay domains 與 dimension/reconnect/fault matrix。

## A. 統一 request egress

- [x] `State` 對 mutating gameplay input 只產生一份 typed operation：Singleplayer/Host 送入
  `RuntimeInput`，Join Client 送入 `GameToClient::GameplayRequest`；不得先改 renderer cache。
- [x] request id、client sequence、dimension revision 各自單調且 bounded；client 不自動抬高
  server-rejected stale revision/sequence，也不重送已 rejected operation。
- [x] fishing、furnace/craft/enchant/brew/anvil、combat、UseState、container/trade/mount、sleep、
  respawn/dimension 等現有入口在 Join Client 不得落入本地 gameplay mutation。

## B. 權威 presentation projection

- [x] embedded 與 socket client 共用 ACK、`PlayerSessionUpdate`、world/entity/container delta 的
  typed projector；只在 authority revision 前進時更新 State cache。
- [ ] inventory rich metadata、health/hunger/effects/death/velocity/fishing hook/mount/use-state、
  workstation progress 與 container slots 可由 snapshot/delta 還原，不從舊 State 猜測成功結果。
  已覆蓋 inventory metadata、health/death/velocity/fishing/mount/shield/brew 與 container slots；
  craft/enchant/anvil 的 station-specific progress wire projection 仍是後續 plan 候選，Join Client
  只送 typed ContainerClick，不做本地成功猜測。
- [x] wrong target、wrong dimension、duplicate/out-of-order/stale projection 被丟棄；owner-private
  session payload 不可廣播給其他 client。
- [x] dimension transfer 先切 authority session/interest，再重建 renderer cache；reconnect 不跨維度
  洩漏 chunk/entity/container/session update。

## C. 三拓撲與 fault vectors

- [ ] Plan22 gameplay operation vector 在 Singleplayer、ListenServer、Dedicated 使用同一
  `ServerRuntime` scheduling/request/ACK/snapshot 路徑，結果與 reject reason 一致。
  現有 parity vector 覆蓋 disabled singleplayer/listen FIFO；完整 Plan22 gameplay matrix 的
  dedicated-vs-embedded 對照仍留後續 plan。
- [x] 覆蓋 duplicate、out-of-order、stale revision、queue full/slow consumer、disconnect/reconnect、
  dimension transfer 與 owner-targeted session projection。
- [x] headless 真 transport 至少以 listen + 2 clients 驗證 request delivery 與 interest isolation；
  測試不得以 direct core mutation 代替被驗證的 network/runtime 行為。

## 驗證閘門

- [x] `cargo test --test runtime_topology_parity` (4 passed)
- [x] `cargo test --test headless_server_authority` (1 passed; listen + 2 clients, owner-private session)
- [x] 新增或擴充 request/projection integration tests，覆蓋 A–C。
  `network::client` typed egress/gate unit vector 與 headless owner projection 已擴充。
- [x] `cargo test --lib server_runtime::tests`、`cargo test --lib network::client::tests`、
  `cargo test --lib network::server::tests`
- [x] `cargo fmt --all -- --check`、`cargo check --all-targets`、`cargo check --lib --bins`、
  `cargo check --release --locked`、`git diff --check`

## 驗證紀錄

- `server_runtime::tests`: 13 passed；`network::client::tests`: 16 passed；
  `network::server::tests`: 34 passed。
- `ReplicationGate` 以 `(dimension, sequence, revision)` gate session snapshots；State 的
  `reset_presented_dimension` 只清 presentation cache，後續 ChunkData/Entity delta 仍由 authority
  projection 重建。Join Client bed/container right-click 已改送 typed Sleep/Container envelope。
- 本計劃未執行 GPU/window/audio/DPI artifact、30-minute soak 或 metrics publication-race；這些
  保留原有 QA/後續 plan 邊界。craft/enchant/anvil station progress projection 與完整三拓撲
  Plan22 gameplay 對照亦列為後續 plan 候選，未在本計劃擴張 wire/gameplay scope。

## 不在本計劃

- `transport_metrics_count_exact_successful_tcp_frames` publication race。
- GPU/window/audio/DPI 人工 artifact、30 分鐘 soak。
- 新玩法、內容目錄、renderer 重構或 protocol binary compatibility 之外的擴張。
