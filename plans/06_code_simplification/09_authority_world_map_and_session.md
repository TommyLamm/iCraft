# Plan09 — 權威 world map 與 session 影子欄位

## 定位

`AuthorityCore`（`src/authority/mod.rs` ~79）同時有：

- `world: ServerWorld` — 「相容用的 active slot」
- `worlds: BTreeMap<Dimension, ServerWorld>` — parked 維度

`activate_dimension`（~350）把活世界 `mem::replace` 進 `self.world`。`with_world` 換進去跑 closure 再換回來。`tick` 與 `ServerRuntime::route_authority_snapshot`（~2930）做同一支舞再 restore `active_before`。讀者無法判斷 `core.world` 當下是 session 維度、spawn 維度，還是上次 `with_world` 的殘留。

每位連線玩家還有兩份紀錄：

- `SessionContract`（`src/authority/contract.rs` ~607）：pose、gamemode、inventory、sequence
- `PlayerSessionState`（`src/server_runtime.rs` ~754）：再存一份 pose／dimension／sequence，外加 interest

`update_interest`（~3082）把 `session.interest` clone 進五個 public 影子集合：`interest_chunks`、`simulation_chunks`、`entity_interest`、`simulation_entity_interest`、`container_viewers`。註解已寫 routing 用 `interest`；測試仍 assert 影子欄位。漏一次 copy 會看起來像維度不同步。

`AuthorityBoundary`（`src/authority/mod.rs` ~115–309）註解仍寫 desktop Singleplayer／Host 在用。`ARCHITECTURE.md` 與呼叫點都已改走 `ServerRuntime`。非測試建構只有兩個單元測（~3875、~3898）。幾乎每個方法都是 `self.core.foo(...)`。`sync_villager`／`sync_vehicle`／`sync_entity`／`seed_bonus_chest` 在 `submit_request` 之外灌實體，生產未用。

`set_dimension`／`set_session_dimension` 對 `Dimension::from_wire` 檢查兩次。

## 前置

- 03 已合併：請求只有一棵 dispatch。本計劃改的是世界 **儲存** 與 session **欄位**，不要同時重寫 match。

若 03 未合併：停止。不要在三層 dispatch + 搬移 world slot 上同時動刀。

## 精確 acceptance

- [x] `AuthorityCore` 的維度世界只存在 `BTreeMap<Dimension, ServerWorld>`（或等價有序 map）。`active_dimension: Dimension` 是 **key**，不是被搬來搬去的值。
- [x] `world()`／`world_mut()`／`revision_for_dimension` 是 map lookup。`with_world` 若還存在，不得再 swap 整顆 `ServerWorld`。
- [x] 可觀察語意保留：`set_session_dimension` 之後，讀「active world」得到目標維度的那顆 `ServerWorld`。既有 `dimension_transfer_updates_session_and_world_contract`、`dimension_worlds_are_parked_without_chunk_aliasing` 必須改寫成測 **key + map**，不得再要求 `mem::replace`。
- [x] Tick、snapshot fanout、save／metrics 仍用排序過的維度迭代（`Ord` 已是 `as u8`）。不得改成 `HashMap` 迭代。
- [x] `PlayerSessionState` 刪除五個影子 interest 集合。讀取改 `session.interest.*`。測試改 assert `interest`。
- [x] `PlayerSessionState` 不再複製權威已擁有、且 runtime 只是為了投影而 mirror 的 gameplay 欄位——**僅限**確認沒有 TCP／save 讀取之後。pose clock、teleport allowance、pending chunks、projected revision、storage 留下。不確定就留，並在證據列出。
- [x] `AuthorityBoundary` 標 `#[cfg(test)]`，或刪除並讓那兩個測試直接建 `AuthorityCore`。刪未使用的 `sync_*`／`seed_bonus_chest`。`set_dimension` 與 `set_session_dimension` 若重複，留一個。
- [x] 維度在 runtime 是 `Dimension`、在 wire 是 `u8` 的轉換仍只發生在邊界，不進 tick。
- [x] Plan32 travel、session lifecycle、residency、invariants 期望值不變（除了明確改去讀影子欄位的 assert）。

## 預計檔案與測試

- 修改：`src/authority/mod.rs`、`src/authority/contract.rs`（若 interest 讀取型別要調）、`src/server_runtime.rs`、必要時 `tests/runtime_topology_parity.rs`、`tests/review_hardening_invariants.rs`。
- 測試：
  - `cargo test --lib authority::`
  - `cargo test --test review_hardening_session_lifecycle -- --test-threads=1`
  - `cargo test --test review_hardening_invariants -- --test-threads=1`
  - `cargo test --test review_hardening_chunk_residency -- --test-threads=1`
  - `cargo test --test plan32_progression_travel -- --test-threads=1`
  - `cargo test --test runtime_topology_parity -- --test-threads=1`
  - `cargo check --all-targets`

## 建議階段

1. 列出所有 `core.world` 讀寫與 `activate_dimension`／`with_world` 呼叫點。
2. 先加 `world_ref(dim)` lookup，讓新碼走 map，舊 `self.world` 仍在。跑 `authority::`。
3. 拿掉搬移，改 active key。改那兩個 Boundary／park 測試。
4. 刪影子 interest；改測試 assert。
5. `AuthorityBoundary` 收進 `#[cfg(test)]`。
6. 跑 travel／session／residency。

## 不在本計劃

- 合併 `SessionContract` 與 `PlayerSessionState` 成單一型別（interest 影子可刪；transport 狀態留下）。
- 改 checksum 聚合順序。
- 把 `impl Ord for Dimension` 搬到 `dimension.rs`（可做，但是可選；搬的話比較鍵必須仍是 `as u8`）。
- 拆 `handle_event`／`handle_join_with_storage` 大函式（證據可列切點）。
- 改 container click 的 clone-then-commit。

## 實作與證據

`AuthorityCore` 不再有被 `mem::replace` 搬移的 `world: ServerWorld` slot。每個已載入維度都住在 `worlds: BTreeMap<Dimension, ServerWorld>`，`active_dimension` 只是 map key。

- `world()` / `world_mut_active()` 查目前 key；`world_ref(dim)` / `world_mut(dim)` 查任意已載入維度。
- `activate_dimension` 只改 key（必要時 `ensure_dimension` 插入新世界）。
- `with_world` 直接 `worlds.get_mut`，不 swap、不改 active key。
- `tick` 仍依 `dimensions()`（BTreeMap 鍵序，`Dimension` as `u8`）迭代，結束後把 active key 設回 `active_before_tick`。
- `route_authority_snapshot` 改走 `world_ref` / `world_mut(dim)`，不再 activate/restore 整顆世界。
- `dimension_transfer_updates_session_and_world_contract` 與 `dimension_worlds_are_parked_without_chunk_aliasing` 改 assert `active_dimension()` + `world_ref(dim)`。

`PlayerSessionState` 刪除五個影子集合：`interest_chunks`、`simulation_chunks`、`entity_interest`、`simulation_entity_interest`、`container_viewers`。routing / 測試改讀 `session.interest.chunks`、`simulation_chunks`、`entities`、`simulation_entities`、`open_containers`。`set_session_dimension` 成功後清 `session.interest.open_containers`。

留下的 `PlayerSessionState` gameplay / transport 欄位（確認仍有 TCP 或 save 讀取）：

- `data: PlayerData` — `save_player` clone 後寫 player 檔；pose / interest / respawn 也讀 `data.position`。
- `effects` — join 投影 `send_player_effects`，dedicated save 寫入 effect vector。
- `dimension` — runtime 路由與 save 的 current dimension fallback。
- `last_client_sequence` — ingress 序號。
- pose clock（`last_pose_sequence` / `last_pose_sender_time_millis` / `last_pose_received_at`）、`teleport_allowance`、`pending_initial_chunks`、`last_projected_session_revision`、`storage` 依計劃留下。

沒有再刪其他 mirror 欄位。沒有合併 `SessionContract` 與 `PlayerSessionState`。

`AuthorityBoundary` 標 `#[cfg(test)]`，只留測試用的 `new` / `set_position` / `set_dimension`。刪除 `sync_villager` / `sync_vehicle` / `sync_entity` / `seed_bonus_chest` 與重複的 `set_session_dimension`。`ARCHITECTURE.md` 已寫它是 unit-test helper，無需再改。

驗收（全部通過）：

```
cargo test --lib authority::                                          # 56 passed
cargo test --test review_hardening_session_lifecycle -- --test-threads=1  # 3 passed
cargo test --test review_hardening_invariants -- --test-threads=1         # 6 passed
cargo test --test review_hardening_chunk_residency -- --test-threads=1    # 4 passed
cargo test --test plan32_progression_travel -- --test-threads=1           # 5 passed
cargo test --test runtime_topology_parity -- --test-threads=1             # 6 passed
cargo check --all-targets
```

未跑 repo-wide full suite。
