# Plan07 — interest 未跨欄時跳過重建

## 定位

`route_authority_snapshot` 開頭對 **每個** player 呼叫 `update_interest_for_at`，即使站著不動。`InterestSet::update_position` 每次 `mem::take` 兩個 chunk `HashSet` 再 `chunks_around`；同 tick 兩次 `query_radius`（view + simulation），再 `old_entities.clone()`／重建 entity set。

Wave 08 entity dirty broadcast 已減少「靜止 EntityState 重送」，但 **interest 集合本身**仍 20 Hz 全重建。

## 前置

無。11 的 mutation 反向索引依賴「進出 chunk 時索引正確」，建議 11 在本計劃之後。

## 精確 acceptance

- [x] session 快取 `(dimension, chunk_x, chunk_z, view_distance, simulation_distance)`（或 pose 量化鍵）。未跨欄且 dimension／距離不變時，**不**重建 chunk HashSet、**不**重跑 `chunks_around`。
- [x] 未跨欄時 entity interest 改增量：只在 spatial bucket dirty 或既有 simulation 集合可能進出時 `query_radius`；禁止每 tick `mem::take` 整包 entity set。
- [x] 傳送、跨欄、dimension 變更、view／simulation distance 變更必須強制全量更新。
- [x] enter／depart 時序與 `RESIDENCY_HYSTERESIS` 不變；`tests/review_hardening_chunk_residency.rs` 通過。
- [x] 靜止單人 tick 不再對該 session 分配新的 chunk HashSet（可用測試計數或 debug assert）。

## 預計檔案與測試

- `src/authority/interest.rs`、`src/server_runtime/projection.rs`
- 驗證：`tests/review_hardening_chunk_residency.rs`；`cargo test --lib interest`；必要時加「stationary player 兩 tick 無 chunk enter／depart」單元測試

## 建議階段

1. 在 `update_position` 加 early-out；補跨欄／傳送測試。
2. entity `update_entities` 改 dirty／diff，不要每 tick 重建。
3. 對照 hysteresis 與 unload 測試。

## 不在本計劃

- 改 `ensure_chunk` 同步 worldgen（不在本波）。
- mutation fanout 反向索引（11）。
- 改 `residency_keep_set` 的每 tick `BTreeSet` 重建（可順便快取，但不是必須）。

## 實作與證據

### 改了什麼

- `InterestSet` 快取 `chunk_anchor = (dimension, cx, cz, view, sim)`；`update_position` 同鍵 early-out，不 `mem::take`／不跑 `chunks_around`。
- `set_distances`／`invalidate_anchor`：距離變更與 `write_pose(..., refresh_interest=true)`（含 teleport）強制下一次全量刷新。
- `update_entities`／`update_simulation_entities` 改成對 live HashSet 做差集 insert／remove，不再 `mem::take` 整包替換。
- `EntityManager::spatial_revision`：spawn／despawn／跨欄 sync／rebuild／clear 時 bump；projection 在「同欄 + revision 未變」時跳過兩次 `query_radius`。
- `ARCHITECTURE.md` 補上 stationary interest skip 契約。

### 測了什麼

- `cargo test --lib interest` — 12 passed（含 stationary skip、跨欄／維度／距離強制重建、entity incremental）。
- `cargo test --lib stationary_session_skips_chunk_interest_rebuild_across_ticks` — ok。
- `cargo test --test review_hardening_chunk_residency` — 4 passed。

### 留下的缺口

- `spatial_revision` 只在 **bucket 成員變更** 時 bump。實體在同一 chunk 內移動但仍跨越連續 `query_radius` 球面邊界時，靜止玩家可能延遲進出直到下次 bucket 變更或玩家跨欄／傳送。Plan 09 的 moved-id sync 可再加細粒度 dirty。
- `residency_keep_set` 仍每 tick 重建 `BTreeSet`（本計劃明確排除）。