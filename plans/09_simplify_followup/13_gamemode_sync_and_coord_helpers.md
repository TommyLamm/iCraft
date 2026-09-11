# Plan13 — `game_mode` 納入 session_sync 與座標常數

## 定位

ARCHITECTURE 規定 pose／dimension／gameplay 只走 `write_pose`／`sync_dimension`／`sync_gameplay_projection`。`game_mode` 是例外：

- `session_sync.rs` 不含 game_mode。
- `ingress.rs` join／respawn ad-hoc 寫 `PlayerData.game_mode`。
- `SessionContract.game_mode` 與 `PlayerData.game_mode` 雙份；`apply_gameplay_to_player_data` **不** sync game_mode。
- `/gamemode`、hardcore→spectator、save 還原任一漏寫即 split-brain。

座標／距離常數同樣分裂：

- `authority/dispatch.rs`：`position_to_milli` → `Result`，上限 `2_000_000.0`
- `server_world.rs`：`Option`，同上限
- `authority/fishing.rs`：`MAX_ABS_MILLI = 2_000_000_000`
- `PLAYER_REACH`／字面 `8.0`：`server_runtime.rs`（死 `within_reach`，02 應已刪）、`server_world.rs`、`authority/tick.rs` mining、`boss.rs`

## 前置

無。02 若已刪 `within_reach`，本計劃只抽常數，不要把死函式加回。

## 精確 acceptance

- [ ] `session_sync` 有 `sync_game_mode(id)`（或 gameplay projection 的 contract→runtime 方向包含 mode）；join／respawn／`/gamemode`／save 前都走它。
- [ ] save codec 讀到的 `PlayerData.game_mode` 與 `SessionContract.game_mode` 在 round-trip 後一致。
- [ ] 單一 `position_to_milli`（建議 `authority` 或 `contract` 子模組）+ 統一 milli 上限；fishing `validate()` 用同一 helper。
- [ ] `PLAYER_REACH`／`player_reach_squared` 單一來源；`validate_request`、mining、trade／mount 的「方塊 reach」改用它。**不要**強行統一「眼睛 raycast」與「實體中心」語意，只消掉魔術數字。
- [ ] `tests/review_hardening_session_lifecycle.rs`、fishing／combat bounds 測試通過。

## 預計檔案與測試

- `src/server_runtime/session_sync.rs`、`src/server_runtime/ingress.rs`、`src/server_runtime.rs`
- `src/authority/dispatch.rs`、`src/authority/fishing.rs`、`src/authority/contract.rs`
- `src/server_world.rs`、`src/interaction.rs` 或 `src/physics.rs`（reach 常數）
- 驗證：`tests/authority_persistence.rs`；`tests/plan33_tcp_fishing_lifecycle.rs`；`cargo test --lib -- game_mode`

## 建議階段

1. `sync_game_mode`；grep 所有 `game_mode =` 寫入點。
2. 抽 `position_to_milli`。
3. 抽 `PLAYER_REACH`，只替換方塊距離魔術數字。

## 不在本計劃

- 合併 `PlayerData` 與 `SessionGameplayState` 成單一 live struct。
- 合併 dual response cache。
- 改 reach 數值（保持 8.0）。
