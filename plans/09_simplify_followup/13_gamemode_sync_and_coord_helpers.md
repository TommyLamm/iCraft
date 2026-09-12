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

- [x] `session_sync` 有 `sync_game_mode(id)`（或 gameplay projection 的 contract→runtime 方向包含 mode）；join／respawn／`/gamemode`／save 前都走它。
- [x] save codec 讀到的 `PlayerData.game_mode` 與 `SessionContract.game_mode` 在 round-trip 後一致。
- [x] 單一 `position_to_milli`（建議 `authority` 或 `contract` 子模組）+ 統一 milli 上限；fishing `validate()` 用同一 helper。
- [x] `PLAYER_REACH`／`player_reach_squared` 單一來源；`validate_request`、mining、trade／mount 的「方塊 reach」改用它。**不要**強行統一「眼睛 raycast」與「實體中心」語意，只消掉魔術數字。
- [x] `tests/review_hardening_session_lifecycle.rs`、fishing／combat bounds 測試通過。

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

## 實作與證據

### 改了什麼

- `session_sync::sync_game_mode`：contract → `PlayerData.game_mode`。
- `sync_gameplay_projection` 一律先呼叫 `sync_game_mode`（覆蓋 `/gamemode`、hardcore→spectator 經 projection）。
- join 註冊後、leave／`save_all` 前、以及 respawn（去掉 ad-hoc 寫入）都走 session_sync。
- `authority::contract`：單一 `position_to_milli`／`position_to_milli_opt`、`POSITION_ABS_LIMIT`／`POSITION_MILLI_ABS_LIMIT`、`milli_within_abs_limit`；dispatch／server_world／fishing 共用。
- `interaction::PLAYER_REACH`／`player_reach_squared`：`validate_request`、mining eye check、trade／mount、LoS raycast、boss 近距 spawn 門檻改用常數（數值仍為 8.0）。
- `ARCHITECTURE.md` 補上 `sync_game_mode` 與座標／reach 單一來源。

### 測了什麼

- `cargo test --lib creative_world_meta_seeds_new_player` — ok
- `cargo test --lib authority_gameplay_round_trips` — ok（Adventure mode round-trip）
- `cargo test --lib fishing` — 10 ok
- `cargo test --lib -- trade mount mining` — 20 ok
- `cargo test --test review_hardening_session_lifecycle` — 3 ok
- `cargo test --test authority_persistence` — 4 ok
- `cargo test --test plan33_tcp_fishing_lifecycle` — 3 ok
- `cargo test --lib combat` — 8 ok
- `cargo test --lib -- game_mode` — 2 ok（persisted_player_game_mode + typed_mining_game_modes）

### 留下的缺口

- LoginSuccess 的 world-default `gamemode` 與 per-session `SessionContract.game_mode` 仍是兩條通道；本計劃只收斂 runtime／authority 雙寫，不改 wire LoginSuccess。
- 眼睛 raycast 與實體中心距離仍用不同測點；僅常數來源統一。
- 未合併 `PlayerData`／`SessionGameplayState`（計劃排除）。
