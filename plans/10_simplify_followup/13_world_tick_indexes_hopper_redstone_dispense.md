# Plan13 — 世界 tick 索引：漏斗、紅石 generation、dispense、metadata、boss

## 定位

Wave 08 已把熔爐／火把改成 compact per-chunk index、紅石可 sleep。以下是同一模式沒有套到的地方：

| 問題 | 證據 | 每 tick 成本 |
| :--- | :--- | :--- |
| **漏斗走全部 block entity** | `tick_hoppers_in_columns`（`world_tick.rs` 510–547）迭代 `simulation_chunks` 後掃每個 `chunk.block_entities` filter `Hopper`；`chunk.rs` 只有 `furnace_positions()`（338）沒有 hopper 版；cooldown 中仍 `get_block_entity_mut`（557–559）；transfer `clone()` 兩個完整 `BlockEntity`（658–676、707–708）；撿物再 clone 一次（616） | O(sim 欄 × BE) + sort，sim 32 ≈ 4,225 欄，零漏斗也付 |
| **紅石 sleep 前仍 O(resident)** | `RedstoneSystem::tick`（`redstone.rs` 679–691）early-out **之前**先 `tick += 1`、`sync_loaded_chunks`（850–858：長度相同時 `known_chunks.iter().all(contains_key)`）、`normalize_plate_occupants`（新 `HashSet`） | ~4k `contains_key` + 一次 alloc，睡著也付 |
| **awake 紅石兩次全表 collect+sort** | `refresh_container_revisions`（712–743）配 `Vec` filter 全部 components 找 comparator 再 sort；`apply_component_transitions`（1137–1249）`components.keys().collect()` + `sort_unstable` 再逐個 match 全部型別 | busy 紅石世界 O(components) × 2 + 2 sort |
| **dispense 後全量 `rebuild_indexes`** | `execute_redstone_dispense`（`server_world.rs` 1951–1952）`entities.push` 後 `rebuild_indexes()`（`entity.rs` 729–751 清空重建 id／type／spatial）；`spawn()` 本來就增量（834–845） | O(all entities)／成功 dispense，且 dirty 全部 fingerprint |
| **存檔 metadata 掃全部 components；evict 掃全部 revisions** | `collect_chunk_metadata`（`redstone.rs` 475–513）每存一欄迭代 **全域** `components`；`remove_resident_chunk`（`server_world.rs` 384–396）filter **全部** `block_revisions` keys | evict N 欄 = O(N×components) + O(N×revisions) |
| **boss 只看玩家 0、視線固定 −Z；`ai_phase` 每 tick 遞增** | `update_dimension_entities`（`server_world.rs` 2080–2088）用 `player_positions.first()`（2071）與 `Vec3::NEG_Z`；hostile loop 2041–2068 每個實體 `ai_phase += 1`，被 checksum 雜湊（2155） | 多人 boss／enderman 凝視錯；idle 實體每 tick dirty checksum，與 09 波 08／09 的 idle skip 打架 |

## 前置

09 波 08（雙 checksum／實體排序）、09 波 09（實體 idle skip）。

## 精確 acceptance

- [ ] `Chunk::hopper_positions()` 與熔爐同編碼；`tick_hoppers_in_columns` 只走索引；cooldown 中不取 `&mut`；只在真的嘗試 transfer 時 clone hopper + 目的地。
- [ ] `ChunkManager` 帶 load-generation counter；`sync_loaded_chunks` 比 generation，不比 key set；`normalize_plate_occupants` 用 scratch `HashSet`；sleep early-out 移到最前。
- [ ] comparator 與 transition-capable 元件各自索引；`apply_component_transitions` 只跑本次 settle 改變 power 的位置 + 到期排程。
- [ ] dispense 用增量 insert（與 `spawn`／`add_restored_entity` 同）。
- [ ] 紅石 persistent metadata 以 `(cx,cz)` sidecar 儲存；`block_revisions` 改 per-column map，evict 為 O(該欄)。
- [ ] boss update 收「最近玩家 + 實際視線」；該維度無玩家時跳過；`ai_phase` 與 physics 一起被 idle skip。
- [ ] 漏斗／紅石／dispenser／boss 現有測試全綠；新增「零漏斗 sim 欄不掃 BE」「sleep 紅石不 `contains_key`」計數測試。
- [ ] `ARCHITECTURE.md` Mutation path 段補 hopper index、redstone generation counter。

## 預計檔案與測試

- 改：`src/world/chunk.rs`、`src/world_tick.rs`、`src/redstone.rs`、`src/chunk_manager.rs`、`src/server_world.rs`、`src/entity.rs`、`src/boss.rs`、`ARCHITECTURE.md`
- 驗證：`cargo test --lib world_tick:: redstone:: entity:: boss:: server_world::`；`tests/plan32_progression_travel.rs`（dragon／wither）；`tick_automation_walks_simulation_columns`；hopper cooldown dirty 測試

## 建議階段

1. hopper index（與熔爐同模式，最小風險）。
2. redstone generation counter + sleep 前移。
3. dispense 增量 + `next_unique` 不在此（Plan 14）。
4. awake 紅石索引。
5. metadata／revision per-column。
6. boss 最近玩家 + `ai_phase` skip。

## 不在本計劃

- runtime 側 union／keep-set 快取（Plan 14）。
- 隨機刻 eligible 索引（09 波 14）。
