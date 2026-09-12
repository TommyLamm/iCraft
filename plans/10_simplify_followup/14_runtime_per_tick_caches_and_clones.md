# Plan14 — runtime 每 tick 快取：union／keep-set、entity id、`SessionContract` clone、metrics、平行 physics

## 定位

09 波 07 只跳過**runtime interest 的兩個 HashSet 重建**。同一 tick 內還有三處對靜止玩家重算的集合，以及幾個 O(all) 的小熱點：

| 問題 | 證據 | 成本 |
| :--- | :--- | :--- |
| `ServerWorld::tick` 每 tick 重建 simulation union | `server_world.rs` 1580–1598：配 `occupants: Vec`、sort／dedup、對**每個玩家** `chunks_around`（`interest.rs` 160–177，HashSet）再倒進 `BTreeSet`；`InterestSet.simulation_chunks`（`interest.rs` 99）與 `union_simulation_chunks`（196–205）**已存在但沒用** | sim 32 ≈ 3–4k BTree insert／玩家／維度／tick |
| `residency_keep_set` 每 tick 重建 + 全 resident 掃 | `server_runtime.rs` 1410–1431：新 `BTreeSet` extend view+sim，再 `residency_hysteresis_chunks`（新 HashSet，`interest.rs` 180–192）；`evict_unkept_chunks` filter **全部** `chunks.keys()`（`server_world.rs` 358–364） | ~4k insert + O(resident) contains，通常零 evict |
| `next_unique_entity_id` 掃所有維度與 session | `authority/mod.rs` 242–259：每個候選 id 跑 `worlds.values().any(get_by_id)` + 每個 session 的 hook；caller：dispense（`tick.rs` 48）、mining 每個 drop（441）、fishing（`dispatch.rs` 727）、死亡 XP／drops（1192、1199）；`claim_entity_id`（262–265）本來就單調 | 開箱 27 drop = O(27 × entities × dimensions) |
| metrics 走全維度兩次；`eprintln!` 在 tick 執行緒 | `server_runtime.rs` 1119–1131 兩次 `dimensions()`（每次配 Vec）數 chunks／entities；1146–1148 over-budget `eprintln!`（正是超時時輸出） | O(dimensions) alloc + 可能阻塞的 stderr |
| `ServerWorld::tick` 回傳假 `AuthoritySnapshot` | `server_world.rs` 1684–1690：`session_updates: Vec::new()`、`tick: self.time`（不是 fixed_tick）、checksum 算完被 `AuthorityCore::tick` 丟掉再算（09 波 08 刪雙 checksum） | 多一個錯型別的配置 |
| 每 tick physics 每 voxel HashMap | `resolve_axis_box_collision`（`physics.rs` 457–479）每 AABB cell × 3 軸 `chunks.get`（`chunk_manager.rs` 341–344）；`aabb_touches_unloaded_column`（`entity.rs` 657–668）再一次 | 移動實體 ~80–150 probe／tick |
| entity physics 單執行緒 | `server_world.rs` 2041–2068 單一 `&mut` loop；`chunks` 只讀 | busy sim 半徑內 physics 是 hopper／fluid 之後最大塊 |

## 前置

09 波 07（interest skip；本計劃直接消費它保留的 set）。

## 精確 acceptance

- [x] `ServerWorld::tick` 收 `&BTreeSet<(i32,i32)>`（或可重用 buffer）作為 simulation union，由 runtime interest 提供；`occupants` 只在玩家跨欄時重建。
- [x] `residency_keep_set` 快取直到任一 session 跨欄／改距離／換維度；resident generation counter 讓「keep == resident」O(1) 短路。
- [x] `next_unique_entity_id` 信任單調計數器，只 probe **目標** world 與該 session hook；全掃降為 `debug_assert`。
- [x] `loaded_chunks`／`entities` metrics 在 tick 已走過的 world 上累加；over-budget 只進 `last_tick_time_us`／`max_tick_time_us`。
- [x] `ServerWorld::tick` 回 `Vec<WorldMutation>`（+ 待處理 dispenser actions），不再建 `AuthoritySnapshot`。
- [x] `update_physics` 收 3×3 `&Chunk` 鄰域（與 lighting／mesh 同模式），不再每 voxel `chunks.get`。
- [x] （最後階段，可拆）entity physics 用 Rayon 對 slice 平行：只讀 `chunks`，收集 `moved_ids` **排序後**才 `sync_entity_positions`；redstone／fluid／random tick 維持序列；checksum 測試前後一致。
- [x] `ARCHITECTURE.md` Tick vs frame：union 由 interest 提供、物理鄰域、平行 physics 的排序套用規則。

## 預計檔案與測試

- 改：`src/server_world.rs`、`src/server_runtime.rs`、`src/authority/{mod,tick,interest}.rs`、`src/physics.rs`、`src/entity.rs`、`src/chunk_manager.rs`、`ARCHITECTURE.md`
- 驗證：`cargo test --lib server_world:: server_runtime:: authority:: physics:: entity::`；`tests/review_hardening_chunk_residency.rs`（hysteresis 必須一致）；checksum 決定性測試（`host_client_sixty_second_entity_checksum_*`）；mining drops／fishing／dispense 測試

## 建議階段

1. metrics／`eprintln!`／假 snapshot（純清理）。
2. `next_unique_entity_id`。
3. union 與 keep-set 快取。
4. physics 鄰域。
5. 平行 physics（獨立 commit，可單獨 revert）。

## 不在本計劃

- 世界 tick 內部索引（Plan 13）。
- dense chunk grid（Plan 26）。

## 實作與證據

### 改了什麼

- `ServerWorld::tick` 改收 interest 提供的 `&BTreeSet` simulation union，回傳 `Vec<WorldMutation>`；plate occupants 的 column key 只在跨欄時重建。
- `ServerRuntime` 快取 simulation union（chunk-anchor fingerprint）與 residency keep-set；`load_generation` 覆蓋時 O(1) 跳過 eviction；eviction 後用 `resident_metrics()` 刷新 counters。
- `next_unique_entity_id(dimension, owner)` 只 probe 目標 world／可選 hook；全表掃改 `debug_assert`。
- `ColumnNeighborhood`（3×3）供 entity／player collision；`update_physics` 不再每 voxel `chunks.get`。
- Entity physics Rayon 平行，`moved_ids` 排序後再 `sync_entity_positions`。
- Over-budget 只記 timing metrics（無 tick-thread stderr）。
- `ARCHITECTURE.md` Tick vs frame 補上 union／鄰域／平行排序契約。

### 測了什麼

- `cargo test --lib server_world::` — 29 passed（含 checksum 決定性）。
- `cargo test --lib authority::` — 66 passed。
- `cargo test --lib physics::` / `entity::` — 通過。
- `cargo test --lib server_runtime::` — 29 passed；既有失敗 `embedded_block_action_loads_an_interested_boundary_chunk_on_demand`（在 HEAD `456b918` 亦失敗，非本計劃回歸）。
- `cargo test --test review_hardening_chunk_residency` — 4 passed。

### 留下的缺口

- 既有 flaky／環境失敗：`embedded_block_action_loads_an_interested_boundary_chunk_on_demand`（LOS／chunk materialize，與本計劃無關）。
- Plate occupants 跨欄後仍每 tick 更新 block cell（壓力板正確性）；column key 才是跨欄快取。
- SessionContract clone 已由 Wave 10 Plan 10 的 `SessionActionView` 處理；本計劃未再動 request clone 路徑。
