# 05 — 桌面無 producer 狀態與 menu 殘留

狀態：已完成。基線：`83e751d`，2026-09-17。
前置：無；可早於取消 worldgen。

## 定位與判定

- state.rs:1354 pending_chunk_payloads 全倉只有初始化、clear、remove，沒有 insert；:3240 的套用分支沒有 producer。
- chunk_manager/presentation.rs:17 load_generation 只 bump、無 reader。WorldColumns 同名 counter 被 redstone/runtime 使用，必須保留。
- State 的 hud_str_scratch／recipe_book_search／world_spawn／difficulty／bonus_chest 只有宣告／初始化。
- presentation_click.rs:172 同一 matches! 重複 Furnace。
- menu/settings.rs 的 load／from_file_contents 重複 sanitize 流程。

## 實作步驟

1. 刪 pending_chunk_payloads 及清空／remove／不可能的 Loaded payload restore；不刪 Join 仍使用的 pending_block_changes。
2. 刪僅 presentation (src/chunk_manager/presentation.rs) 中的 load_generation 欄位、accessor、bump wrapper；authority counter 不動。
3. 刪無用 State 欄位，連 bootstrap.rs:142/146/187/198/212/216 的無用資料計算／傳遞一起清；launch.difficulty 給 runtime 的用途保留。
4. 核對 State::new 建立空 chunk_meshes 後的首次 invalidation loop；確認中間 set_game_mode 無 column producer 後刪空操作。
5. 刪 menu 的 dead hit-test／options 常數／無 caller constructor。settings 載入與測試解析共用一次 sanitize 結束步驟。
6. 刪重複 Furnace match arm。world_seed／world_type／generate_structures 此時仍被桌面 worldgen 使用，由 11 再刪。

## 驗證與驗收

desktop cargo check、menu／bootstrap 現有測試；不為單純無 producer 欄位新增鏡像測試。確認 authority load_generation 的 redstone／eviction 測試不受影響。刪除的狀態不搬到新的未使用 struct。

## 實作紀錄

已完成：
1. **pending_chunk_payloads 清理**：
   - 刪除 `src/state.rs` 中的 `pending_chunk_payloads` 定義、清空調用、HashMap 初始化及 worker result 中的 remove / restore 假分支。
   - 刪除 `src/presentation/network_event.rs` 中的 `self.pending_chunk_payloads.clear();`。
   - 保留 Join 仍正常使用的 `pending_block_changes` 及其 revision 映射。
2. **presentation load_generation 清理**：
   - 刪除 `src/chunk_manager/presentation.rs` 中的 `load_generation` 欄位、`load_generation(&self)` accessor、`bump_load_generation(&mut self)` 及其在 `insert_resident_chunk` / `remove_resident_chunk` / `restore_chunk_data` 中的呼叫。
   - 嚴格保留 `src/chunk_manager/mod.rs` 中 `WorldColumns` 的 `load_generation` 及其在 redstone / eviction 流程中的所有調用。
3. **無用 State 欄位與 bootstrap 計算清理**：
   - 刪除 `State` 結構體中的 `hud_str_scratch`、`recipe_book_search`、`world_spawn`、`difficulty`、`bonus_chest`。
   - 刪除 `src/presentation/bootstrap.rs` 中 `LaunchWorldState` 對應的 `world_spawn` 與 `bonus_chest` 欄位、無用預算邏輯與傳遞。
   - 保留 `launch.difficulty` 傳遞至 `EmbeddedRuntimeBridge::new` 的 runtime 使用路徑。
4. **State::new 空操作清理**：
   - 核對確認 `State::new` 中建立空 `chunk_meshes` 後，`set_game_mode` 無任何 column producer，直接刪除對空集合做 `invalidate_chunk_meshes` 的無操作代碼。
5. **Menu 殘留死碼與 settings sanitize 去重**：
   - 刪除 `src/menu/mod.rs` 中無用常數 `OPTIONS_ROW_TOPS`、`SETTINGS_FILE`、`CONTROLS_FILE`。
   - 刪除無 caller 的 `Menu::new` constructor（`App` 統一走 `Menu::from_gpu`）。
   - 刪除 dead hit-test 函式（`menu::mod` 的 `options_row_at`、`hit`，`widgets` 的 `Screen::hit_index`、`focus_count`、`focus_rect`、`widget`）。
   - 在 `src/menu/settings.rs` 抽出 `sanitize()` 方法，讓 `load()` 與測試專用 `from_file_contents()` 共用，並移除 `allow(dead_code)`。
   - 遷移 `src/menu/tests.rs` 中使用 `options_row_at` / `hit` 的過時測試至正式 `options_button_rects()` 入口，並移除無意義的重疊斷言。
6. **重複 Furnace match arm**：
   - 刪除 `src/presentation_click.rs` 中 `is_authority_container` matches 內的重複 `BlockType::Furnace`。
   - `world_seed`、`world_type`、`generate_structures` 目前在桌面 worldgen 仍有活躍讀取，按計劃留待 11 刪除。

驗證與驗收：
- `cargo check --all-targets --all-features`：通過（exit 0）
- `cargo test --bin icraft menu::`：通過（36 passed; 0 failed）
- `cargo test --lib redstone::`：通過（31 passed; 0 failed）
- `cargo test --bin icraft presentation`：通過（10 passed; 0 failed）
- 淨刪碼量：9 files changed, 21 insertions(+), 116 deletions(-)，淨刪除 95 行。
