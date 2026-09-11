# Plan11 — mutation／block-entity fanout 反向索引

## 定位

`route_authority_snapshot` 每個 `WorldMutation` 掃 interest 找目標；每次 `get_block_entity(...).cloned()`，再對每個 target `entity.clone()`。Container 路徑 `slots × container_targets` 逐格 `send_container_slot_update`。

紅石／流體 busy tick 的成本是 O(mutations × players)，不是 O(dirty chunks)。Wave 08 EntityState 有 fingerprint；**container／BE 沒有**。

07 之後靜止玩家不再重建 interest，本計劃才能安全維護 `(dimension, chunk) → session_ids` 而不每 tick 重建索引。

## 前置

07（interest skip）。索引必須在 chunk enter／depart 時更新；07 的 early-out 不能漏掉真正的進出。

## 精確 acceptance

- [x] 維護 `(dimension, ChunkCoord) → session_ids` 反向索引，隨 interest enter／depart 更新。
- [x] mutation fanout 用該索引，不再對每個 mutation 掃全部 `players`。
- [x] 同一 block entity 每 tick 最多編碼一次，再按 session 發送。
- [x] 無 viewer 的 BE 不 clone、不送 slot 更新。
- [x] 非 viewer 仍不洩漏私有庫存；container revision 閘門不變。
- [x] `tests/plan34_container_break_inventory_conservation.rs`、`tests/review_hardening_container_click.rs` 通過。

## 預計檔案與測試

- `src/server_runtime/projection.rs`、`src/authority/interest.rs`（若 delta 需更明確）
- 驗證：container／block-entity 投影測試；redstone 造成的 BlockChange 仍到達有 interest 的 session

## 建議階段

1. 由既有 interest delta 建反向索引；對照 residency 測試。
2. mutation 迴圈改查索引；BE encode 提升到迴圈外。
3. container slot 只對 viewer；能 diff 就不要全槽 fanout。

## 不在本計劃

- 把 container open／click／slot 合併進 `PlayerSessionUpdate`。
- 改 `flatten_column_voxels`／ChunkData 壓縮。
- 改 embedded 雙 block 投影路徑。

## 實作與證據

### 改了什麼

- `ServerRuntime::chunk_interest_index`：`(Dimension, ChunkCoord) → BTreeSet<session_id>`。
- `PlayerSessionState::chunk_index_dimension`：記錄索引實際登記的維度，避免
  `sync_dimension` 先改 `interest.dimension` 時几何 enter／depart 為空卻留下
  舊維度鍵。
- Join／leave／`update_interest_for_at` 重建時維護索引；靜止 early-out 不碰索引。
- `queue_interest_update`（Block／BlockEntity／Chunk／Container）改查反向索引；
  Container 再以 `wants_container` 過濾 viewer。
- `route_authority_snapshot`：先查 BE／container 目標；無目標不 `cloned()` BE、
  不拉 slots；有目標時 BE 只從 world clone 一次再 fanout。
- `ARCHITECTURE.md` 補上 reverse-index fanout 契約。

### 測了什麼

- `cargo test --lib interest` — 14 passed（含
  `chunk_interest_index_tracks_join_move_and_leave`、stationary skip、
  dimension／fanout isolation）。
- `cargo test --test review_hardening_chunk_residency` — 4 passed。
- `cargo test --test plan34_container_break_inventory_conservation` — 4 passed。
- `cargo test --test review_hardening_container_click` — 9 passed。

### 留下的缺口

- Container 全槽 fanout 仍無 per-slot／fingerprint diff（計劃「能 diff 就」；
  目前只做到無 viewer 不取 slots、有 viewer 才送）。
- `handle_position` 的遠端玩家 pose fanout 仍掃全部 `players`（非本計劃）。
- Entity／EntityState 的 `queue_interest_update` 路徑仍全表 filter（mutation
  熱路徑不走該分支）。
