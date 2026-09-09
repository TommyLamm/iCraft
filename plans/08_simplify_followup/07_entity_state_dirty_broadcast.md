# Plan07 — 實體狀態 dirty／批次廣播

## 定位

`ServerRuntime::route_authority_snapshot` 每個 tick 對每個維度 `collect` 全部實體，再對每個興趣目標 `send_entity_state`（`projection.rs` ~724–751）。20 Hz × 全部實體 × 全部觀看者，靜止掉落物也每 50 ms 編碼一次。10 名玩家 × 200 實體 ≈ 每秒 4 萬包。

Interest 已有 `simulation_entities`，不必再掃全表當唯一來源。

## 前置

無。會動投影／協定 payload 形狀時保持 checksum 決定性。

## 精確 acceptance

- [x] 只送 pose／health／anim 有變的實體，或每玩家一包 `EntityStates { items: Vec<...> }`。
- [x] 靜止且未髒的實體不再每 tick encode。
- [x] Join 客戶端仍能看到進入／離開興趣集的實體（entered 全量、其後 delta）。
- [x] 現有 TCP／投影測試通過；不得為了簡化改 checksum 期望值，除非本計劃同步改 checksum 輸入並更新測試說明。

## 預計檔案與測試

- `src/server_runtime/projection.rs`、必要時 `protocol.rs`
- 驗證：`cargo test --lib server_runtime::`；TCP harness 實體可見性

## 建議階段

1. 量測或計數目前每 tick `send_entity_state` 次數（可用現有 metrics）。
2. 先做 per-entity dirty，再考慮批次封包。
3. 興趣 entered 必須全量。

## 不在本計劃

- 重寫實體 ECS。
- 平行化 `AuthorityCore::tick`。
- 容器 slot 扇出（可順手記在證據，不當成本計劃）。

## 實作與證據

- `route_authority_snapshot` 不再掃整張實體表、也不再每 tick 對每個觀看者 `entity_state_wire`。只走各 session 的 `simulation_entities`；pose／health／anim fingerprint 沒變就跳過 encode／send。
- 興趣 `entered` 仍是全量：view 進入送 `EntitySpawn`（只 encode 新進入的實體），simulation 進入因沒有上次 fingerprint 而送一次完整 `EntityState`。離開 simulation 會清 fingerprint，再進入仍是全量。
- 未新增 `EntityStates { items }` 批次封包：dirty 已把靜止實體的 20 Hz encode 砍掉；改 wire 形狀要升 protocol，本計劃用既有 `EntityState` latest-wins。Checksum 輸入未改。
- 容器 slot 扇出仍是「每個 viewer × 每個 slot 一包」，未動。

### 測試

```
cargo test --lib server_runtime:: -- --test-threads=1
  27 passed（含 entity_state_broadcasts_dirty_or_entered_only）

cargo test --test headless_server_authority -- --test-threads=1
  tcp_dispenser_drop_projection_converges_complete_item_metadata passed
  two_clients_share_headless_authority_with_revision_interest_and_reconnect passed

cargo test --test plan31_authoritative_block_actions -- --test-threads=1
  3 passed（含 TCP typed block action 投影）

cargo test --test plan34_container_break_inventory_conservation tcp_listen_and_dedicated -- --test-threads=1
  passed（先前與其它 TCP 套件並行時 30s harness 超時一次，單獨重跑通過）

cargo test --test runtime_topology_parity -- --test-threads=1
  6 passed（含 plan28_dispenser_item_projection_matches_all_runtime_topologies）
```
