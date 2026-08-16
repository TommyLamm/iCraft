# Plan01 — 隔離已證實死路徑

## 定位

全專案掃描確認下列符號**沒有生產呼叫**，但仍然看起來像線上 API。新同事會在這裡「修刷怪／AI／mesh／戰鬥」而碰不到活路徑。

掃描當日證據（執行前必須再 grep 一次，以源碼為準）：

| 符號 | 檔案 | 生產呼叫 |
| --- | --- | --- |
| `ParticleSystem::compile_mesh` | `src/particles.rs` ~247 | 零。線上是 `compile_instances`（`state.rs` ~20847） |
| `ServerWorld::apply_combat` | `src/server_world.rs` 1159–1181 | 零。線上戰鬥在 `src/authority/combat.rs` |
| `traverse_section_visibility`（會配置 scratch 的 wrapper） | `src/culling.rs` ~286 | 零。線上是 `traverse_section_visibility_with_scratch` |
| `ChunkStreamingScheduler::enqueue_dirty` | `src/chunk_schedule.rs` ~234 | 只有本檔單元測。`State::update_chunks` 仍對永遠空的 dirty map 做 `remove_dirty`／`reprioritize_dirty` |
| `SpawningSystem` | `src/spawning.rs` | 只有本檔測試。線上是 `mob::spawn_mobs`／`passive_mob::spawn_passive_mobs` |
| `Brain` | `src/ai/brain.rs` | 只在 `src/ai/mod.rs` re-export，沒有 `new_for_entity`／`tick` 呼叫 |
| `get_interpolated_height`、`place_birch_tree` | `src/world.rs` | 零 |
| `place_oak_tree`、`place_spruce_tree` | `src/world.rs` | 只在 `test_tree_placement_bounds` ~6620 |
| `Chunk::new_with_seed` carve | `src/world.rs` ~3644 | `wy > 8` 與 else 都寫 `Air` |

`ARCHITECTURE.md` 已寫：Singleplayer／Host 走 embedded runtime；section mesh scheduler 才是現役。

## 前置

無。可與 02–05、07、08 並行。不要跟 09／10 搶同一段 `update_chunks` 大搬移。

## 精確 acceptance

- [ ] `compile_mesh` 從 `src/particles.rs` 刪除。`compile_instances` 與 `compile_instances_preserves_legacy_white_full_light_math` 仍在。
- [ ] `ServerWorld::apply_combat` 刪除。`authority/combat.rs` 路徑與既有戰鬥測試不變。
- [ ] `culling::traverse_section_visibility` 無呼叫則刪除。`*_with_scratch` 與 fail-open 測試仍在。
- [ ] `State::update_chunks` 不再維護永遠空的 `dirty_chunk_meshes`（`remove_dirty`／`reprioritize_dirty`／`drain_mesh_invalidations` 當 live work）。`SectionMeshScheduler` 仍是唯一生產 enqueue。`enqueue_dirty` 若只剩測試，改 `#[cfg(test)]` 或留在測試模組；**不要**改 `SectionMeshScheduler` 的 16384 cap／latest-wins。
- [ ] `SpawningSystem` 與 `Brain` 檔頭 rustdoc 寫明「未接到權威 tick；線上入口是 …」。不得把 `spawn_mobs` 接到 `SpawningSystem`（距離／上限不同，合併會改玩法）。
- [ ] `get_interpolated_height`、`place_birch_tree` 刪除。`place_oak_tree`／`place_spruce_tree` 若只服務一個測試：把該測試改測 `worldgen::feature`，或把兩個 helper 標 `#[cfg(test)]`。不得刪 `Biome::terrain_params`／`is_snowy`。
- [ ] carve 兩個非熔岩臂收成一個 `Air`。`let _biome = ctx.biome_at(...)` 若仍未使用則刪，不要開始用它改地表。
- [ ] 既有測試期望值不得改。

## 預計檔案與測試

- 修改：`src/particles.rs`、`src/server_world.rs`、`src/culling.rs`、`src/chunk_schedule.rs`、`src/state.rs`（只動 dirty-queue 呼叫）、`src/spawning.rs`、`src/ai/brain.rs`、`src/world.rs`。
- 測試：
  - `cargo test --lib particles::`
  - `cargo test --lib culling::`
  - `cargo test --lib chunk_schedule::`
  - `cargo test --lib spawning::`
  - `cargo test --lib world::`（含 `test_tree_placement_bounds` 若仍存在）
  - `cargo test --test plan31_authoritative_block_actions -- --test-threads=1`
  - `cargo check --all-targets`

## 建議階段

1. 對上表每個符號再 grep 一次。有新的生產呼叫就從本計劃拿掉該項，寫進證據「未刪原因」。
2. 先刪零呼叫函式（`compile_mesh`、`apply_combat`、`traverse_section_visibility`、height／birch）。
3. 停掉 `update_chunks` 對空 dirty map 的維護；跑 `chunk_schedule` 單元測。
4. rustdoc 隔離 `SpawningSystem`／`Brain`。
5. 收 carve 死臂；必要時搬樹測試。
6. 跑窄測試，補「實作與證據」。

## 不在本計劃

- 把 `SpawningSystem` 接到 `ServerWorld::tick_entities`。
- 把 `update_mobs` 改成 `Brain`。
- 刪 `dynamic_resolution` 或改設定 UI（Plan14 已決定編進 desktop；重開 upscale 另案）。
- 刪 wire 上的 `BlockUse` 或未使用 `Packet` variant。
- 拆 `state.rs`（10）、改 dispatch（03）、改存檔工人（05）。
