# Plan11 — mutation／block-entity fanout 反向索引

## 定位

`route_authority_snapshot` 每個 `WorldMutation` 掃 interest 找目標；每次 `get_block_entity(...).cloned()`，再對每個 target `entity.clone()`。Container 路徑 `slots × container_targets` 逐格 `send_container_slot_update`。

紅石／流體 busy tick 的成本是 O(mutations × players)，不是 O(dirty chunks)。Wave 08 EntityState 有 fingerprint；**container／BE 沒有**。

07 之後靜止玩家不再重建 interest，本計劃才能安全維護 `(dimension, chunk) → session_ids` 而不每 tick 重建索引。

## 前置

07（interest skip）。索引必須在 chunk enter／depart 時更新；07 的 early-out 不能漏掉真正的進出。

## 精確 acceptance

- [ ] 維護 `(dimension, ChunkCoord) → session_ids` 反向索引，隨 interest enter／depart 更新。
- [ ] mutation fanout 用該索引，不再對每個 mutation 掃全部 `players`。
- [ ] 同一 block entity 每 tick 最多編碼一次，再按 session 發送。
- [ ] 無 viewer 的 BE 不 clone、不送 slot 更新。
- [ ] 非 viewer 仍不洩漏私有庫存；container revision 閘門不變。
- [ ] `tests/plan34_container_break_inventory_conservation.rs`、`tests/review_hardening_container_click.rs` 通過。

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
