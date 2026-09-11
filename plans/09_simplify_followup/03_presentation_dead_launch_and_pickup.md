# Plan03 — 刪桌面不可達啟動與 Pickup 死分支

## 定位

所有 live launch 要麼 `is_client`（Join），要麼 `in_process_authority`（Singleplayer／Host）。`State::new` 裡 `!is_client && !in_process_authority` 永遠 false。該分支仍含同步 worldgen、bonus chest、spawn lighting、`pending_redstone_metadata`（從未 push，後段迴圈永遠空）。

`bootstrap.rs` 的 `has_save` 在 `load_launch_world_state` 恆為 `false`。

永遠不執行的本地 Pickup／XP 區塊（`State::update`）：條件要求 `inventory_decision(Pickup) == LocalMutate`，但 `PresentationTopology::inventory_decision` 對 `Pickup` **永遠** `Reject`。註解已寫「wait for authority pickup」。

其它低風險死欄位／變體（同檔、可一併刪）：

- `furnace_tick_timer`：只 init，從不遞增。
- `autosave_timer`：只 assign。
- `StationKind::Furnace`：match 臂 no-op；熔爐走 container，不是 station kind。
- `open_chest`：`has_in_process_runtime()` 立即 return；剩餘 body 僅 Join submit。可改名／內聯到 join 分支。

## 前置

無。05 必須等本計劃刪掉 `pending_redstone_metadata` 之後，才清 presentation `RedstoneSystem`。

## 精確 acceptance

- [x] `State::new` 沒有 `!in_process_authority` 的同步 worldgen／bonus chest／spawn mesh 路徑。
- [x] `pending_redstone_metadata` 與永遠空的 restore 迴圈消失。
- [x] 本地 Pickup／XP 收集區塊刪除；拾取只走 authority。
- [x] 刪 `furnace_tick_timer`、`autosave_timer`（若仍無 reader）。
- [x] 刪 `StationKind::Furnace` 與 `frame.rs` 對應 no-op 臂。
- [x] `open_chest` 不再假裝 generic；Join-only 路徑命名或內聯清楚。
- [x] 確認沒有測試／harness 以「無 embedded runtime 的非 client」啟動 `State`。
- [x] `cargo test --bin icraft -- presentation_inventory_policy` 與既有 embedded presentation 測試通過。

## 預計檔案與測試

- `src/state.rs`、`src/presentation/bootstrap.rs`、`src/presentation/frame.rs`
- `src/presentation_inventory_policy.rs`（若刪 Pickup 本地路徑後 enum 仍給測試用，可留到 15）
- 驗證：`tests/review_hardening_embedded_presentation.rs`；`cargo test --bin icraft -- presentation`

## 建議階段

1. 讀 `State::new` 的 `in_process_authority`／`is_client` 兩臂，刪不可達臂並編譯。
2. 刪 Pickup 區塊。
3. 刪 timer／`StationKind::Furnace`／精簡 `open_chest`。

## 不在本計劃

- 刪 presentation `RedstoneSystem` 欄位（05）。
- 合併 embedded 雙 block 投影（`project_authority_mutations` vs `BlockChange`）。
- 補線 `handle_inventory_click` 的 `LocalMutate`（那是行為缺口，不是死碼；本計劃不改 click 語意）。
- 刪 `dynamic_resolution` 設定（04）。

## 實作與證據

### 改了什麼

- `State::new`：刪掉不可達的 `!is_client && !in_process_authority` 同步 worldgen、bonus chest、spawn lighting／spawn mesh，以及從未 push 的 `pending_redstone_metadata` restore 迴圈。連帶刪 `INITIAL_WORLD_CHUNK_RADIUS`／`initial_chunk_radius`（只服務該死路徑）。
- `bootstrap.rs`：刪恆為 `false` 的 `LaunchWorldState.has_save`。
- `State::update`：刪本地 Pickup／XP 收集。`inventory_decision(Pickup)` 仍永遠 `Reject`；拾取只走 authority。`PresentationInventoryTarget::Pickup` 留到 Plan 15。
- 刪無 reader 的 `furnace_tick_timer`、`autosave_timer`。
- 刪 `StationKind::Furnace` 與 `frame.rs`／slot 列表的 no-op 臂。熔爐仍是 container。
- `open_chest` 改名為 `submit_join_container_open`：呼叫端已是 Join-only，函式不再假裝 generic（不再對 embedded 立即 return）。
- `ARCHITECTURE.md`：寫明 presentation 不本地 worldgen／bonus chest／pickup。

### 測試

- `cargo test --offline presentation_inventory_policy`：4 passed（lib；`--bin icraft` 過濾器 0 tests，該模組在 lib）。
- `cargo test --offline --bin icraft -- presentation`：11 passed。
- `cargo test --offline --test review_hardening_embedded_presentation`：3 passed。
- `cargo test --offline --bin icraft -- embedded_`：6 passed（含 `embedded_runtime_*`／`embedded_inventory_writeback_*`）。
- `inventory_slot_hits_map_to_inventory_decision_targets`：ok。

### 確認

- `State::new` 只由 `app.rs` 以 menu `WorldLaunch` 呼叫（Singleplayer／Host／Client）。測試不建構無 embedded runtime 的非 client `State`；embedded 測試走 `EmbeddedRuntimeBridge` 或 policy gate。

### 剩餘缺口

- presentation `RedstoneSystem` 空殼仍在（Plan 05）。
- `handle_inventory_click` 的 Embedded `LocalMutate` 行為缺口未改（刻意排除）。
- `dynamic_resolution`／`render_scale` 未動（Plan 04）。
- Packet／`PROTOCOL_VERSION` 未改。

