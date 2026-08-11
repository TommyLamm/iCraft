# 26 — 容器生命週期強制關閉與箱子回饋

> 狀態：已實作（2026-08-12；以 v16 wire contract 為邊界）
> 前置：02、16、23、24
> 本計劃不取代 02 的容器交易語義；只補生命週期清理，以及箱子開關的最小 deterministic 回饋。

## 目標

- 任一權威路徑（超距離、維度轉移、興趣範圍離開、斷線/登出、箱子被破壞、非法 session）都能只關閉受影響的 player+dimension+position session。
- 強制關閉可重複、不可遞迴送出 close request，也不可重複歸還/掉落游標物品；權威 projection 是網路 client 的唯一 inventory 來源。
- 沿用現有 v16 `Packet::ContainerClose` wire shape 作 targeted server→client close；不加入 reason/epoch/cursor 欄位。
- 箱子以既有 `BlockState::is_open` bit 表示 binary open/closed pose；首位 viewer 開、末位 viewer 關，雙箱兩半同步。
- 以既有 procedural mesh 路徑提供 deterministic binary chest pose；不新增 smooth animation timeline 或 renderer pipeline。
- 新增 ChestOpen/ChestClose 音效 ID 與可解碼的 synthesized fallback；只在 block-state edge 播放一次。

## v16 邊界與非目標

- 不升 `PROTOCOL_VERSION`，不新增 wire packet 欄位/enum variant。現有 v16 client 若沒有新 inbound `ContainerClose` handler，只能維持 decode 相容，不能保證生命週期行為；v17（reason/epoch/cursor 或 authority-owned cursor）另列後續計劃。
- 不把 cursor stack 塞入 `ContainerClose`。使用者主動關閉沿用既有 authoritative return/drop；forced close 清理 transient cursor，不在非權威 client 產生物品或改寫 server inventory。
- 不做平滑 lid 插值、shader、GPU timeline 或實機 Host+Join 證據；GPU/Host+Join 仍是手動 C 類證據。
- 不改 unrelated gameplay、automation、fluid、save schema 或既有警告。

## 驗收閘門

1. close-by-block 以 dimension scope 並返回精確 session records；duplicate/late close 不會關閉同 player 的新 session。
2. NetworkClient 對 forced close 繞過 container revision gate，只接受 active `(dimension,x,y,z)`；State cleanup 不遞迴送 close、不重複歸還/掉落。
3. TooFar/InvalidDimension/transfer、interest departure、break、logout/disconnect 均清 viewer/session/UI；modern runtime 與 legacy host 路徑一致。
4. Chest first/last viewer transitions update both halves' `is_open`; mesh state and audio edge are deterministic and idempotent.
5. Relevant unit/headless/topology tests pass; no GPU artifact is claimed by automated tests.

## 主要檔案

`src/container_sessions.rs`, `src/network/protocol.rs`, `src/network/client.rs`, `src/network/server.rs`, `src/state.rs`, `src/authority/interest.rs`, `src/authority/mod.rs`, `src/server_world.rs`, `src/server_runtime.rs`, `src/world.rs`, `src/block_model.rs`, `src/audio.rs`, optional `assets/sounds/chest_open.wav` and `chest_close.wav`, plus focused tests in those modules and `tests/headless_server_authority.rs` / `tests/runtime_topology_parity.rs`.

## 實作與驗證紀錄

- `ContainerSessionManager` now has dimension-scoped `close_by_block` and exact
  player/position close; `NetworkClient` accepts the existing v16 targeted
  `ContainerClose` only for the active key and bypasses the normal revision gate.
  Mismatched dimension/position and late duplicates are ignored.
- `ServerRuntime`/`ServerWorld` route close intents for distance and invalid
  dimension rejection, interest departure, transfer, block replacement, logout,
  and disconnect.  Legacy host and modern runtime paths send targeted closes;
  `State::force_close_inventory` clears transient UI/cursor state without
  submitting another close or returning/dropping items.
- Chest first/last viewer transitions update both halves' `BlockState::is_open`;
  the custom mesh is deterministic binary open/closed geometry and the
  `ChestOpen`/`ChestClose` procedural fallback is deterministic and edge-gated.
- Focused evidence: `network::client` targeted-close integration (1),
  `network::server::tests` (35), `server_runtime::tests` (13),
  `headless_server_authority` (1), `runtime_topology_parity` (5), plus the
  container-session, server-world chest, protocol, block-model, and audio unit
  tests.  After the review fixes, the final
  `cargo test --release --locked --no-fail-fast` suite passed: library **670
  passed / 3 ignored**, main target **801 passed / 3 ignored**, server bin 2,
  and every integration/doc-test lane had zero failures.  The focused final
  gates also passed: `container_sessions` (9), chest and forced-viewer
  `server_world` tests (3), `server_runtime::tests` (14),
  `network::server::tests` (35), `headless_server_authority` (1), and
  `runtime_topology_parity` (5).  `cargo check --release`,
  `cargo fmt --all -- --check`, and `git diff --check` passed after the fixes.
- No direct `State` GPU-constructor unit test is claimed: the cleanup contract is
  covered through the client/runtime/headless routes above.  No smooth lid
  interpolation, GPU/window, audio-device, or Host+Join visual evidence is
  claimed.  The v16 packet has no close epoch/reason/cursor, so stale same-key
  close disambiguation remains a v17 follow-up.
- `02_chest_storage.md` is intentionally unchanged: it contains a pre-existing
  invalid UTF-8 control byte, so its historical duplicate checkbox block was not
  safely rewritten.  This Plan26 record is authoritative for the D3/lifecycle
  follow-up status.
