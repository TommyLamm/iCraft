# Plan15 — embedded 零拷貝投影：`Arc<Chunk>`、突變只套一次

## 定位

單機／listen-host 的 embedded presentation 與 TCP join 走同一條 wire 形狀的路徑，即使兩端在同一個進程：

### ChunkData：paletted → dense → paletted → 全欄 lighting

`send_chunk_projection`（`projection.rs` 496–541）對 embedded 也產生 dense `Vec<u8>` 串流；client `restore_network_payload`（`format.rs` 1327–1342）`decode_network_required_voxels` `to_vec()`（889）→ `apply_decoded_column`（943+）重建 palette → `recompute_direct_column_lighting`（1341）。每欄 ~288 KiB × 3 份拷貝 + palette 重建 + 全欄光照，在套用 tick 的那一 frame 付。

### 每個突變套兩次

`tick_authority_boundary`（`state.rs` 4106–4110）先 drain `BlockChange` 事件 → `apply_remote_block_change`（`network_event.rs` 236–248；`state.rs` 7240–7278），再 `project_authority_mutations`（4616–4650）→ `apply_synced_block_change` **再套一次**。第二次通常 `previous == block` no-op，但仍做 HashMap 查找＋lighting 屬性檢查。註解 4643 自述 runtime 輸出「尚未帶 block-entity payload」。09 §5 因 fluid／revision 語意差異保留兩路；本波解除。

### presentation `set_block` 仍跑權威副作用

`apply_synced_block_change`（`state.rs` 205–236）呼叫 `ChunkManager::set_block`，會 **enqueue fluids 並重跑 lighting**——這是 GPU 執行緒上的第二份世界副作用（Plan 26 會拆型別；本計劃先讓投影路徑不走 `set_block`）。

## 前置

Plan 07（投影事件單一型別後，embedded 可以夾帶非 wire 的 payload 變體）。

## 精確 acceptance

- [x] embedded 投影新增 `ChunkColumn(Arc<Chunk>)`（或 section-wise clone）變體，**不經** dense 串流；TCP 維持 `ChunkData`。
- [x] embedded 套用 `Arc<Chunk>` 時直接用 authority 已算好的光照，不再 `recompute_direct_column_lighting`；join 路徑不變。
- [x] embedded 只套一次突變：走 snapshot `WorldMutation`（含 `raw_fluid`／revision）；`BlockChange` 事件只給 TCP／join。revision gate 不刪。
- [x] 投影套用不呼叫 `ChunkManager::set_block`；改為寫 payload + 標 section mesh dirty + 只在 light emission 變時做局部光照。
- [x] 單機走路 frame 上的 `to_vec`／palette 重建計數測試（或 microbench）前後對比。
- [x] `ARCHITECTURE.md` Runtimes／Network 段：embedded 投影是進程內 `Arc<Chunk>`；「不合併 `project_authority_mutations` 與 `BlockChange`」排除句刪除並改寫。

## 預計檔案與測試

- 改：`src/server_runtime/projection.rs`、`src/presentation/{embedded_runtime,network_event}.rs`、`src/state.rs`、`src/save/format.rs`（只讀路徑不變）、`ARCHITECTURE.md`
- 驗證：`cargo test --bin icraft`；`tests/review_hardening_embedded_presentation.rs`；`tests/waterlogging_authority.rs`（fluid 語意）；`tests/review_hardening_join_projection.rs`（join 不變）；lighting 測試

## 建議階段

1. 投影套用不走 `set_block`（獨立）。
2. embedded 單一突變路徑。
3. `Arc<Chunk>` 變體與套用。
4. ARCHITECTURE。

## 不在本計劃

- `ChunkManager` 拆型別（Plan 26）。
- TCP `ChunkData` 壓縮／格式（09 波 10、README §6）。

## 實作與證據

### 改了什麼

- 新增 `PresentationEvent::{Packet, ChunkColumn{Arc<Chunk>}}`；embedded 佇列可夾帶進程內欄，TCP 仍只走 `ProjectionEvent`／`ChunkData`。
- `drain_initial_chunk_projections`：local session → `send_embedded_chunk_column`（`Arc::new(chunk.clone())`）；遠端 TCP → 既有 dense `network_terrain_payload`。
- `queue_block_change` 跳過 `local_session_id`；embedded 突變只經 snapshot `WorldMutation`（`project_authority_mutations` 含 `raw_fluid` + revision gate）。
- `ChunkManager::apply_presentation_cell`：寫 block／state／fluid + mesh dirty，**不** `schedule_fluid_neighbors`；`apply_synced_block_change` 改走此路徑，僅在 opacity／emission 變時做局部光照。
- `apply_embedded_chunk_column`：`insert_resident_chunk` 套用 Arc 欄，不呼叫 `recompute_direct_column_lighting`。
- `ARCHITECTURE.md` Runtimes／Network／Persistence：記錄 `ChunkColumn` 與單一突變路徑。
- 順手修正 `embedded_runtime_poses_use_monotonic_sender_time`：改讀 authority pose（Plan 10 後 `PlayerData.position` 不再 dual-write；HEAD 上此測已失敗）。

### 測了什麼

- `cargo test --bin icraft` — 221 passed
- `cargo test --lib server_runtime::tests::embedded_chunk_projection_uses_arc_column_not_dense_stream` — ok（無 `ChunkData`、有 `ChunkColumn`）
- `cargo test --bin icraft remote_block_change_updates_light_and_boundary_mesh_dependencies` — ok（投影不 enqueue fluid）
- `cargo test --test review_hardening_embedded_presentation` — 3 passed
- `cargo test --test review_hardening_join_projection` — 3 passed
- `cargo test --test waterlogging_authority` — 5 passed
- `cargo test --lib lighting::` — 6 passed
- `cargo test --test runtime_topology_parity` — 6 passed
- `cargo check --all-targets` — ok
- `cargo check --bin icraft-server` — ok

### 死路徑證據

- embedded local `BlockChange`：`queue_block_change` 對 `local_session_id` `continue`；plan31 embedded 向量斷言 owner 不再收到 `BlockChange`。
- embedded dense `ChunkData`：`embedded_chunk_projection_uses_arc_column_not_dense_stream` 斷言 local session 事件中無 `Packet::ChunkData`。
- 投影 `set_block`：`apply_synced_block_change` 只呼叫 `apply_presentation_cell`；單元測試確認 `pop_fluid_update` 為空。
- `recompute_direct_column_lighting`：仍只在 `restore_network_payload`（join／disk）；`apply_embedded_chunk_column` 路徑 0 呼叫。

### 留下的缺口

- Presentation 仍 `(*arc).clone()` 進 `ChunkManager`（可變 CPU 拷貝）；真正共享 `Arc` 駐留留給 Plan 26 拆型別。
- TCP `ChunkData` 仍走 dense 串流（本計劃排除）。
- `plan31_listen_tcp_typed_block_action_projection` 在乾淨 HEAD 亦失敗（時序／mining session 投影），非本計劃引入。
