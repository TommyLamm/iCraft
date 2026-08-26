# Plan06 — 刪除權威、世界與會話中的死函式

## 定位

在權威架構與會話協議收斂過程中，多個模組留下了零調用的舊介面、假函式以及僅在舊單元測試中殘留的包裝結構體：

| 檔案 | 殘留項目 | 行數 | 原因 |
| :--- | :--- | :--- | :--- |
| `src/container_sessions.rs:L369-423` | `get_furnace_slots`, `set_furnace_slots` | ~54 行 | 熔爐槽位操作已由 `ServerWorld::take_furnace_output` 與 `transactions::execute_furnace_take_output` 接管 |
| `src/container_sessions.rs:L244-252, L359-367` | `get_chest_inventory`, `set_chest_inventory` | ~18 行 | 無人使用的單行轉發別名 |
| `src/container_sessions.rs:L130-132` | `close_all_for_player` | ~3 行 | `close_by_player` 的完全重複包裝 |
| `src/server_world.rs:L997-1009` | `spawn_experience_orb` | ~13 行 | 權威經驗球已統一走 `spawn_authority_experience`（帶 xp_value 與 index rebuild） |
| `src/authority/contract.rs:L30-32, L532-553` | `AuthorityTopology::is_headless`, `SessionGameplayState::add_item` | ~26 行 | 零調用死方法（全庫均使用 `add_slot`） |
| `src/authority/mod.rs:L117-164, L497-499` | `AuthorityBoundary`, `AuthorityCore::world_mutations` | ~50 行 | 零調用或僅供 2 個舊測試使用的膠水結構體 |
| `src/server_runtime.rs:L1473-1476` | `gamemode_wire` | ~4 行 | 接收 `LevelData` 後忽略並硬編碼回傳 0 的假函式 |
| `src/world/block.rs:L50-152` | `Biome::get_biome`, `ALL`, `terrain_params`, `is_snowy`, `is_dry` | ~103 行 | 舊生物群系查詢表，全庫世界生成均已遷移至 `worldgen::climate::ClimateSystem` |

清理這些死方法能直接消除維護雜音，預計減少 ~270 行死代碼。

## 前置

無。可與 01–05 並行。

## 精確 acceptance

- [x] 刪除 `src/container_sessions.rs` 中的 `get_furnace_slots`、`set_furnace_slots`、`get_chest_inventory`、`set_chest_inventory`、`close_all_for_player`。
- [x] 刪除 `src/server_world.rs` 中的 `spawn_experience_orb`。
- [x] 刪除 `src/authority/contract.rs` 中的 `SessionGameplayState::add_item` 與 `AuthorityTopology::is_headless`。
- [x] 刪除 `src/authority/mod.rs` 中的 `AuthorityBoundary` 結構體與 `world_mutations` 方法，將調用 `AuthorityBoundary` 的 2 個單元測試改為直接構建 `AuthorityCore`。
- [x] 刪除 `src/server_runtime.rs` 中的 `gamemode_wire`。
- [x] 刪除 `src/world/block.rs` 中的舊 `Biome` 查詢表與常數（保留純 enum 定義）。
- [x] `cargo check --all-targets` 通過。
- [x] 權威與世界測試全數通過。

## 預計檔案與測試

- 修改：
  - `src/container_sessions.rs`
  - `src/server_world.rs`
  - `src/authority/contract.rs`
  - `src/authority/mod.rs`
  - `src/server_runtime.rs`
  - `src/world/block.rs`
- 驗證測試：
  - `cargo test --lib authority::`
  - `cargo test --lib server_world::`
  - `cargo test --lib container_sessions::`
  - `cargo check --all-targets`

## 建議階段

1. 逐一刪除各檔案中的死方法。
2. 在 `src/authority/mod.rs` 中重構 2 個舊單元測試，移除 `AuthorityBoundary`。
3. 運行單元與整合測試驗證。

## 不在本計劃

- 解耦 `container_sessions.rs` 與 `ServerWorld` 的架構關係（此為 Plan 10）。
- 更改任何權威交易或掉落經驗的邏輯。

## 實作與證據

### 修改內容
1. **`src/container_sessions.rs`**：
   - 刪除 `get_furnace_slots`、`set_furnace_slots`（已由 `ServerWorld::take_furnace_output` 與 `transactions::execute_furnace_take_output` 接管）。
   - 刪除 `get_chest_inventory`、`set_chest_inventory`（無人使用的單行轉發別名）。
   - 刪除 `close_all_for_player`（`close_by_player` 的重複包裝）。
2. **`src/server_world.rs`**：
   - 刪除 `spawn_experience_orb`（經驗球生成統一走 `spawn_authority_experience`）。
3. **`src/authority/contract.rs`**：
   - 刪除 `AuthorityTopology::is_headless` 與 `SessionGameplayState::add_item`（全庫均使用 `add_slot`）。
4. **`src/authority/mod.rs`**：
   - 刪除 `AuthorityBoundary` 結構體與 `AuthorityCore::world_mutations` 方法。
   - 重構調用 `AuthorityBoundary` 的 2 個單元測試（`dimension_transfer_updates_session_and_world_contract`、`dimension_worlds_are_parked_without_chunk_aliasing`），改為直接構建與操作 `AuthorityCore`。
5. **`src/server_runtime.rs`**：
   - 刪除假函式 `gamemode_wire`，呼叫處直接傳入 `0`。
6. **`src/world/block.rs`**：
   - 刪除舊 `Biome` 查詢表與輔助常數/函式（`ALL`、`get_biome`、`terrain_params`、`is_snowy`、`is_dry`）及其測試，保留純 `enum Biome` 定義。

### 驗證證據
- `cargo test --lib authority::` (56 passed; 0 failed)
- `cargo test --lib server_world::` (18 passed; 0 failed)
- `cargo test --lib container_sessions::` (9 passed; 0 failed)
- `cargo test --test headless_server_authority` (2 passed; 0 failed)
- `cargo check --all-targets` (通過)

