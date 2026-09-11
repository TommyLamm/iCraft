# Plan04 — 權威／世界／玩法死 API 掃除

## 定位

跨八份掃描彙整、grep 確認 live caller 為零（或僅測試）的符號。09 波 02 已列的 `AuthorityTopology`／`within_reach`／`ServerWorld::dispatch` 這裡不重複（但 `dispatch` 的**測試改寫**在此）。

### 權威／runtime

| 符號 | 位置 | 狀態 |
| :--- | :--- | :--- |
| `ServerWorld::place_bonus_chest` | `server_world.rs` 1107 | 0 live caller |
| `AuthorityCore::session_gameplay` | `authority/mod.rs` 380–386 | 0 live caller |
| `close_container_viewers`（非 `_forced`） | `server_world.rs` 454–458 | 0 caller；live 全用 `_forced` |
| `AUTHORITY_CONTRACT_VERSION` | `contract.rs` 16；re-export `mod.rs` 29 | 從未讀取 |
| `common_vector_snapshot`、`cache_len` | `mod.rs` 437–444；`contract.rs` 653 | tests only |
| `ServerWorld::new` | `server_world.rs` 90–107 | 生產只用 `new_with_difficulty` |
| `HostToServer::{BroadcastSleepStateSync, BroadcastPlayerHealth}` | `channels.rs` 176–186、279–282 | 無 `server_runtime` producer（若 Plan 07 先落地則自然消失） |
| **`sleeping_players`** | `server_world.rs` 70、139、1450 | **只 insert，永不讀取／清除**：第一次 `/sleep` 成功，第二次永遠 `InvalidState`。這是 bug，不只是死碼 |
| `ServerWorld::dispatch`（`#[cfg(test)]`） | `server_world.rs` 1486–1535 | 第二份 operation match；caller 只在 2698–2849 測試 |
| `ensure_villager`／`ensure_vehicle`（`#[cfg(test)] pub`） | 1106–1192 | 若 Plan32 seeding 可用 `set_block`／`entities.spawn` 取代則刪 |

### 世界

| 符號 | 位置 | 狀態 |
| :--- | :--- | :--- |
| `Chunk::generate_mesh`／`generate_mesh_inner`／`generate_mesh_bundle*`／`generate_surface_mesh*`／`ChunkMeshBundle` | `mesh.rs` 1292–1410、1703+；`chunk_render.rs` 284–287 | 0 生產 caller；live 是 `generate_section_mesh_bundle_from_halo_with_registry_for_lods`（`state.rs` 5971） |
| `ChunkStreamingScheduler` 欄位 `dirty_chunk_meshes`／`dirty_mesh_priority` 與 `pop_nearest_dirty`／`reprioritize_dirty`／`dirty_len` | `chunk_schedule.rs` 221–250、377–407 | 註解自述「只有單元測試」；生產用 `SectionMeshScheduler` |
| `CHUNK_HEIGHT`、`SECTION_COUNT`、測試樹 `place_oak_tree`／`place_spruce_tree` | `block.rs` 19、51–153；`section.rs` 7 | `SECTION_COUNT` 全 repo 零引用；樹 helper 與 `worldgen/feature.rs` 327–414 重複 |
| `find_safe_spawn_position` | `block.rs` 1857–1897 | 只有自身測試 2064 |
| `block_occlusion_shape` | `voxel_shape.rs` 645–688 | 只有自身測試 803–807；mesh 用 `face_should_render` |
| `NETHER_HEIGHT` | `dimension.rs` 11、323 | 與 `WorldHeight::NETHER` 重複 |

### 玩法域

| 符號 | 位置 | 狀態 |
| :--- | :--- | :--- |
| redstone `#[allow(dead_code)]` impl 內：`charge_at`、`snapshot`（alias）、`checksum`（alias）、`Direction::{dx,dy,dz}` | `redstone.rs` 310、394、404、415、96–104 | 0 caller |
| `EntityManager::{get_index_by_id, clear, update_spatial_indexes}`；`get_entities_in_chunk` | `entity.rs` 808–813、829–832、921–928、1096 | 0 caller／tests only |
| `MutationCause::{PlayerPlace,PlayerBreak,Redstone,Explosion}`、`BlockMutationRequest.new_entity` | `world_tick.rs` 15–21；所有 request 都是 `System`／`None` | 只 `sim_harness.rs` 541 讀 |
| `sample_all_loaded_random_ticks`、`tick_all_loaded_fluids`、`tick_all_loaded_hoppers(_with_entities)` | `world_tick.rs` 353–372、483+；`fluid.rs` 19–29 | 只測試與 `sim_harness`；`server_world.rs` 1625 明令禁用 |
| `LootTableId::StrongholdLibrary` | `loot.rs` 10、299–334 | 結構生成從不傳入 |
| `WeatherSystem::authority_random_offset`／`authority_random_seed` | `weather.rs` 231–237 | tests only |
| `match_recipe`（alias） | `recipes.rs` 960 | 一行 alias |
| `get_highest_solid_y`（alias） | `mob.rs` 115–117 | 一行 alias |

## 前置

09 波 02（`AuthorityTopology`／`dispatch` 判定）、09 波 14（允許刪 `tick_all_loaded_*` wrapper）。Plan 01 先落地可少掉 `sim_harness` 例外。

## 精確 acceptance

- [ ] 上三表符號刪除；`cargo check --all-targets` 無 `dead_code` 警告來自這些位置。
- [ ] `sleeping_players`：實作醒來／跳夜（離床或天亮 `clear`），**或**刪欄位只留床方塊檢查；新增測試「連續兩次 `/sleep` 都不是 `InvalidState`」。
- [ ] `server_world.rs` 測試改走 `AuthorityCore::submit_request`；`dispatch` 刪除。
- [ ] `world_tick`／`fluid` 測試改呼叫 `*_in_columns(..., Some(all_loaded))`；wrapper 刪除。
- [ ] `BlockMutationRequest` 縮為 `pos`＋`new_block`＋`new_state`（+ 必要 fluid 欄位）。
- [ ] `mesh.rs` 測試改走 `mesh_l0_volume`／section halo 入口。

## 預計檔案與測試

- 改：`src/authority/{mod,contract}.rs`、`src/server_world.rs`、`src/network/channels.rs`、`src/world/{mesh,block,section}.rs`、`src/chunk_render.rs`、`src/chunk_schedule.rs`、`src/voxel_shape.rs`、`src/dimension.rs`、`src/redstone.rs`、`src/entity.rs`、`src/world_tick.rs`、`src/fluid.rs`、`src/loot.rs`、`src/weather.rs`、`src/recipes.rs`、`src/mob.rs`
- 驗證：`cargo test --lib`；`tests/headless_server_authority.rs`；`tests/authority_gameplay_domains.rs`（sleep）

## 建議階段

1. 純刪（alias、常數、tests-only helper）。
2. `sleeping_players` 修 bug + 測試。
3. `ServerWorld::dispatch` 測試改寫。
4. `tick_all_loaded_*` 與 `BlockMutationRequest` 收縮。
5. mesh 死 API 與測試改寫。

## 不在本計劃

- 渲染／UI／存檔／資源側死 API（Plan 05）。
- `active_dimension`／`world()`（Plan 11）。
- error enum 塌縮（Plan 10）。
