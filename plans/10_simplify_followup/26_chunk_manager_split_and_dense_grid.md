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

- [ ] 兩個型別：`WorldColumns`（chunks + height + fluid queues + save dirty + simulation distance）給 `ServerWorld`；`PresentationChunks`（chunks + section mesh dirty + view radius）給 `State`。共用的純查詢（`get_block`／`highest_solid_y`／`column_neighborhood`）放 trait 或共用 inner struct。
- [ ] 權威 `set_block` 不再維護 mesh invalidation；presentation 套用不 enqueue fluid／不跑權威 lighting。
- [ ] `render_distance` 拆成 `view_distance`（presentation）與 `simulation_distance`（authority）。
- [ ] **dense grid**：以 `(cx - origin_x, cz - origin_z)` 索引的滑動 2D `Vec<Option<Chunk>>`（大小 `2*(distance+RESIDENCY_HYSTERESIS)+1`）+ generation counter；HashMap 只留罕見出窗 debug 路徑（或完全移除，出窗即未載入）。
- [ ] `chunk_schedule.rs` 16–24 的 hysteresis 與 grid origin 使用同一個中心；多玩家 server 的 grid 覆蓋所有 session union（或 per-session 視窗 + 共用 `Chunk` `Arc`——擇一並寫入 ARCHITECTURE）。
- [ ] 「所有 loaded chunks」迭代順序不影響 RNG／checksum（authority 已用顯式 interest set；加測試鎖住）。
- [ ] 所有 `ChunkManager::new(8)` 測試改對新型別；插入任意座標（`(-1,-1)`、`(4,-3)`）的測試改為窗內座標或顯式擴窗。
- [ ] `ARCHITECTURE.md` Ownership／World 段改寫。

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
