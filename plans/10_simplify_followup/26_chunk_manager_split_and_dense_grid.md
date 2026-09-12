# Plan26 — `ChunkManager` 拆 presentation／authority，dense chunk grid

## 定位

### 一個型別、兩個擁有者、互相背著對方的欄位

`ChunkManager`（`chunk_manager.rs` 128–137）同時是 `ServerWorld.chunks`（`server_world.rs` 55）與 `State.chunk_manager`（`state.rs` 2311）：

| 欄位／行為 | 誰需要 | 另一邊付的成本 |
| :--- | :--- | :--- |
| `pending_mesh_invalidations`／`pending_section_mesh_invalidations` | presentation（只有 `state.rs` 6009–6015 drain） | 權威每次 `set_block` 都 `record_mesh_invalidation`（157–159）填兩個沒人 drain 的 HashSet |
| `dirty_chunks`（存檔）、`water_updates`／`lava_updates`（`fluid.rs` 61） | authority | presentation `apply_synced_block_change`（`state.rs` 205–236）呼叫 `set_block` 會 **enqueue fluids 並重跑 lighting** 在 GPU 執行緒副本上 |
| `render_distance` | 桌面＝view（`state.rs` 3237）；server＝simulation distance（`server_runtime.rs` 1024、`server_world.rs` 1592） | 同名不同義 |

### `HashMap<(i32,i32), Chunk>`

`ChunkManager.chunks`（129）與桌面第二份 map。熱 `get_block`／lighting／physics／meshing halo 全部 hash。view 16 ≈ 1,089 欄；lighting BFS／physics 每 voxel 查。Wave 09 §5 排除 dense grid；本波解除。

## 前置

Plan 15（投影不再走 `set_block`）、Plan 17（lighting 走鄰域快取，dense grid 的收益才能落在 BFS 上）。建議 Plan 14 的 physics 鄰域也先做。

## 精確 acceptance

- [x] 兩個型別：`WorldColumns`（chunks + height + fluid queues + save dirty + simulation distance）給 `ServerWorld`；`PresentationChunks`（chunks + section mesh dirty + view radius）給 `State`。共用的純查詢（`get_block`／`highest_solid_y`／`column_neighborhood`）放 trait 或共用 inner struct。
- [x] 權威 `set_block` 不再維護 mesh invalidation；presentation 套用不 enqueue fluid／不跑權威 lighting。
- [x] `render_distance` 拆成 `view_distance`（presentation）與 `simulation_distance`（authority）。
- [x] **dense grid**：以 `(cx - origin_x, cz - origin_z)` 索引的滑動 2D `Vec<Option<Chunk>>`（大小 `2*(distance+RESIDENCY_HYSTERESIS)+1`）+ generation counter；HashMap 只留罕見出窗 debug 路徑（或完全移除，出窗即未載入）。
- [x] `chunk_schedule.rs` 16–24 的 hysteresis 與 grid origin 使用同一個中心；多玩家 server 的 grid 覆蓋所有 session union（或 per-session 視窗 + 共用 `Chunk` `Arc`——擇一並寫入 ARCHITECTURE）。
- [x] 「所有 loaded chunks」迭代順序不影響 RNG／checksum（authority 已用顯式 interest set；加測試鎖住）。
- [x] 所有 `ChunkManager::new(8)` 測試改對新型別；插入任意座標（`(-1,-1)`、`(4,-3)`）的測試改為窗內座標或顯式擴窗。
- [x] `ARCHITECTURE.md` Ownership／World 段改寫。

## 實作與證據

### 改了什麼

- `src/chunk_manager/` 拆模組：`DenseColumnGrid`、`WorldColumns`、`PresentationChunks`、`ColumnQuery`／`LightColumnHost`。
- 權威 `set_block`／fluid／light 只 mark `dirty_chunks`；presentation `apply_presentation_cell`／light 只 mark mesh dirty。
- `render_distance` → `view_distance`（presentation）／`simulation_distance`（`WorldColumns`＋`AuthorityConfig`）。
- Dense grid 以 `distance + RESIDENCY_HYSTERESIS` 定窗；presentation `recenter`；authority `cover_session_centers` 覆蓋 session union。出窗欄進 overflow，等 eviction flush 後再移除。
- BlockAction on-demand 載入改 `materialize_chunk`（Async `ensure_chunk` 無法當 tick 結算）。
- `ARCHITECTURE.md` Ownership 段改寫。

### 測了什麼

| 命令 | 結果 |
| --- | --- |
| `cargo test --lib chunk_manager::` | 26 passed |
| `cargo test --lib lighting::` | 9 passed |
| `cargo test --lib fluid::` | 9 passed |
| `cargo test --lib` | 726 passed, 2 ignored |
| `cargo test --test review_hardening_chunk_residency` | 4 passed |
| `cargo check --all-targets` | ok |
| `cargo check --bin icraft-server` | ok |

決定性：`loaded_column_iteration_order_is_deterministic`；lighting checksum `fixed_seed_place_break_lighting_checksum_stable`；worldgen byte-identity 仍在 lib 測試內。

### 死路徑證據

- 權威 mesh invalidation：`WorldColumns` 無 `pending_mesh_*`／`record_mesh_invalidation`；`authority_set_block_marks_save_dirty_without_mesh_queue` 斷言只 dirty＋fluid。
- presentation fluid：`apply_presentation_cell` 無 `schedule_fluid_neighbors`；`presentation_cell_marks_mesh_not_fluids`。

### 留下的缺口

- overflow 仍是 HashMap（出窗／測試／flush 前暫存）；熱路徑在窗內 dense 索引。
- container 槽位 API 在 `WorldColumns`／`PresentationChunks` 各有一份（未再抽共用 helper）。
- `settings.render_distance` UI 鍵名未改（仍對應 presentation `view_distance`）。


## 預計檔案與測試

- 改：`src/chunk_manager.rs`（拆兩檔）、`src/server_world.rs`、`src/state.rs`、`src/fluid.rs`、`src/lighting.rs`、`src/physics.rs`、`src/chunk_schedule.rs`、`src/authority/interest.rs`、`src/server_runtime.rs`、`ARCHITECTURE.md`
- 驗證：`cargo test --lib`；`cargo test --bin icraft`；`tests/review_hardening_chunk_residency.rs`；checksum 決定性測試；worldgen byte-identity

## 建議階段

1. 型別拆分（不換容器；純責任切分）。
2. `render_distance` 改名。
3. dense grid 給 presentation（單玩家視窗最簡單）。
4. dense grid 給 authority（多 session union 視窗）。

## 不在本計劃

- ECS。
- 投影路徑不走 `set_block`（Plan 15 已做）。
