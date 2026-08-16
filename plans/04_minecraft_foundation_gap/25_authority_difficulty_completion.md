# 25 — 權威難度消費與持久化完成

## 定位

- 優先級：P3，承接 Plan18 留下的「完整 difficulty consumer」缺口；前置提交：`1291183`
- 本計劃只補 server-owned difficulty 的 canonical parse、持久化與既有 hostile AI
  consumer；不新增生物、傷害、飢餓或生成系統。
- `menu::Difficulty` 仍是 presentation/world-creation 型別；伺服器使用獨立的
  `ServerDifficulty`，避免 UI 與 headless authority 互相推導。
- 建議提交上限：1

## A. Server-owned difficulty contract

- [x] `server.properties` 嚴格接受 `peaceful|easy|normal|hard`；未知值在
  `SaveManager`/authority world 建立前 rejected。四值以 typed `ServerDifficulty` 傳入
  `AuthorityConfig`，由 embedded、listen 和 dedicated 共用。
- [x] difficulty 持久化沿用既有 `server.properties`（`save_all` 寫入 world directory）；
  舊 `level.dat` 的 bincode 欄位順序不變，避免為新增 policy 破壞 legacy level decode。
- [x] `pvp` 保持獨立 operator rule。Peaceful 不再偷偷把 `pvp` 改成 false；請求 gate
  仍只讀 `WorldRules::pvp`。

## B. Existing hostile policy consumer

- [x] `ServerWorld::allows_hostile_spawning` 明確表示
  `do_mob_spawning && difficulty != Peaceful`。目前 engine 沒有 autonomous hostile
  spawn generator；此 gate 是既有 authority contract，未藉本計劃新增內容。
- [x] Peaceful 在 fixed tick 明確移除已載入 hostile entity，避免既有怪物繞過難度；
  `do_mob_spawning=false` 不會凍結已載入怪物的既有 AI。
- [x] Easy/Normal/Hard 只消費既有 hostile chase lane，速度倍率分別為 `0.9/1.0/1.1`；
  這是目前 engine 可表達且可在 headless 觀察的差異。完整 vanilla hostile damage、
  hunger/starvation、spawn tables、despawn radius 與其他未存在系統明確列為 non-goal。
- [x] difficulty 納入 deterministic world checksum，確保不同 authority policy 不會被
  誤視為同一 snapshot。

## C. 驗證向量

- [x] typed parse、未知值 fail-before-world、四難度 policy 及 `pvp` independence。
- [x] Peaceful existing-hostile despawn；Easy/Normal/Hard chase speed monotonicity；
  `do_mob_spawning=false` 下既有 hostile 仍 tick。
- [x] `server.properties` save/reload，embedded ListenServer 與 dedicated runtime
  使用同一 difficulty。
- [x] 明確排除 GPU/window/audio/DPI/Host+Join visual 與 30 分鐘 soak；Plan24 的
  headless soak artifact 不因本計劃重跑。

## 不在本計劃

- 新 hostile spawn/despawn tables、mob AI、combat damage path、hunger/starvation
  simulation 或完整 Java difficulty parity。
- protocol version bump、State/menu/renderer/UI 重構、GPU/window/audio/DPI 手動驗收。
- 任何與 difficulty failure 無關的 warning、metrics race、topology 或 soak 修正。

## 驗證紀錄（2026-08-11）

- `cargo test --test difficulty_authority -- --nocapture`：3 passed，覆蓋 strict
  invalid config、Peaceful/pvp、Peaceful despawn、Easy/Normal/Hard chase、save/reload、
  embedded ListenServer 與 dedicated parity。
- `cargo test --lib game_rules::tests -- --nocapture`：4 passed；
  `cargo test --lib server_world::tests -- --nocapture`：6 passed；
  `cargo test --lib server_runtime::tests -- --nocapture`：13 passed。
- `cargo check --all-targets`、`cargo check --release --locked`、
  `cargo fmt --all -- --check` 與 `git diff --check`：通過（僅既有 warnings）。
- `cargo test --release --locked --no-fail-fast`：全套通過；lib `658 passed / 3
  ignored`、main `789 passed / 3 ignored`、server bin `2 passed`，
  `authority_gameplay_domains` `3`、`authority_persistence` `3`、
  `difficulty_authority` `3`、`headless_server_authority` `1`、
  `passive_mob_tests` `1`、`runtime_topology_parity` `5`，doctests `0`。

Plan25 的 authority difficulty consumer 只宣稱上述 headless contract；未實作的
vanilla difficulty systems 與 GPU/manual evidence 必須交由後續計劃，不得以本向量冒充。
