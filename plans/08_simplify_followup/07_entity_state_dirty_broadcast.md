# Plan07 — 實體狀態 dirty／批次廣播

## 定位

`ServerRuntime::route_authority_snapshot` 每個 tick 對每個維度 `collect` 全部實體，再對每個興趣目標 `send_entity_state`（`projection.rs` ~724–751）。20 Hz × 全部實體 × 全部觀看者，靜止掉落物也每 50 ms 編碼一次。10 名玩家 × 200 實體 ≈ 每秒 4 萬包。

Interest 已有 `simulation_entities`，不必再掃全表當唯一來源。

## 前置

無。會動投影／協定 payload 形狀時保持 checksum 決定性。

## 精確 acceptance

- [ ] 只送 pose／health／anim 有變的實體，或每玩家一包 `EntityStates { items: Vec<...> }`。
- [ ] 靜止且未髒的實體不再每 tick encode。
- [ ] Join 客戶端仍能看到進入／離開興趣集的實體（entered 全量、其後 delta）。
- [ ] 現有 TCP／投影測試通過；不得為了簡化改 checksum 期望值，除非本計劃同步改 checksum 輸入並更新測試說明。

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
