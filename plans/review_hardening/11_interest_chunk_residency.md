# Plan11 — Interest 驅動 chunk 駐留

## 定位

- `ARCHITECTURE.md`：per-session interest 同時是投影邊界**與** chunk 物質化門檻。
- 實作：`ServerWorld::ensure_chunk` 只 insert，伺服器路徑沒有 `chunks.remove`。
  `set_block`、`safe_spawn_y`、Nether 傳送門連結、加入時每 tick 16 個 chunk 的投影器
  都會呼叫它。`tick` 對**所有**已載入 column 跑紅石／流體／隨機刻／熔爐／實體。
- 探索過的世界永遠留在 RAM，每 6000 tick 整包重存。`loaded_chunks` 報的是這個無界集合。
- 既有測試證明可視邊界**可以** on-demand 載入，不證明視野外**不會**物質化。

## 前置

無。建議 05 已合併，這樣 evict 前 flush 不會把「restore 失敗的生成 chunk」寫盤。
若 05 未合併：evict 只允許 flush **成功 restore 過或本 session 新生成且 dirty** 的 column。

## 精確 acceptance

- [x] 離開**所有** session 的 simulation／view 集（加上與客戶端相同的滯後）的 column，
  在 flush dirty 之後從 `ServerWorld.chunks` 移除。Spawn `(0,0)` 可保留一圈，但必須有上限。
- [x] 隨機刻／流體／hopper／熔爐只走 `simulation_chunks`（或等價的 interest 聯集），
  不走 `chunks.keys()` 全表。
- [x] `ensure_chunk` 不得在 interest／budget 之外被 tick 熱路徑呼叫，除了傳送門連結
  需要的成對 column（寫明例外並測：連結後若無人在場，仍應進入 evict 候選）。
- [x] 測試：玩家在原點 view=2，N tick 後 chunk (8,0) 不在 `world.chunks`；對該處
  `BlockAction` reject 且仍不 `ensure_chunk`。玩家走 32 chunk 後，原點 column 在
  下一次 evict／save 後離開 map。
- [x] `loaded_chunks` 指標與駐留集一致。

## 預計檔案與測試

- 修改：`src/server_world.rs`（`ensure_chunk`、tick 迴圈、新 evict）、
  `src/server_runtime.rs`（interest 更新之後呼叫 evict）、`src/authority/interest.rs`
  僅在需要「simulation 聯集」helper 時。
- 測試：`src/server_runtime.rs`／`src/authority/interest.rs` 單元測；
  `tests/review_hardening_chunk_residency.rs`。

## 建議階段

1. 先加「視野外不 ensure」負向測試（15 也列了；本計劃擁有它）。
2. Tick 改讀 simulation set。
3. Evict + flush。
4. 傳送門例外。
5. 指標。

## 不在本計劃

- 桌面 mesh 佇列（14）。
- 改世界生成內容（10）。

## 實作與證據

### 行為

- `InterestSet` 新增與客戶端相同的 Chebyshev 滯後（`view + UNLOAD_HYSTERESIS`）以及有上限的 spawn 圈（半徑 1、最多 9 格）。有 session 的維度不釘死 spawn；無人時 evict 只保留該圈。
- `ServerWorld::tick` 用玩家位置 × `ChunkManager.render_distance`（即 simulation distance）組成 simulation 聯集。hopper／流體／隨機刻／熔爐只走這個集合；tick 內的 `set_block` 也拒絕為集合外 column 生成地形。
- `ServerRuntime::tick_with_output` 在 interest 更新與 16-column 投影之後呼叫 `evict_uninteresting_chunks`。dirty 且本 session 成功 restore／新生成的 column 先 flush，失敗則留在 RAM；非 dirty 直接卸下。
- 傳送門連結仍可 `ensure_chunk` 目的地成對 column（`tick_portal_travel` 註明例外）。連結後無人在場即進入一般 evict 候選，沒有 portal pin。
- `loaded_chunks` 仍是各維度 `world.chunks.chunks.len()` 之和，現在等於 eviction 後的駐留集。

### 測試

- `cargo test --lib authority::interest`：5 passed（含 `residency_hysteresis_matches_client_chebyshev_ring`、`spawn_residency_is_capped_and_simulation_union_is_per_session`）。
- `cargo test --lib evict` / `tick_automation_walks` / `empty_dimension_keeps`：`tick_automation_walks_simulation_columns_not_residency`、`evict_flushes_dirty_then_removes_unkept_columns`、`interest_evict_drops_origin_after_long_walk_and_metrics_match`、`empty_dimension_keeps_only_capped_spawn_ring` 通過。
- `cargo test --test review_hardening_chunk_residency`：4 passed（視野外不 ensure、走 32 chunk 後 origin flush+evict、傳送門 column 可 evict、無人時 spawn 圈有上限）。
- `cargo test --lib fixed_tick_checksum_is_deterministic`：通過。
- `cargo fmt --all`：通過。

### 留下的缺口

- `restore_authority_state` 仍一次載入磁碟上全部已存 column，第一個 evict 才卸下；大世界啟動仍可能有一次尖峰。
- 實體不隨 column evict；紅石 `sync_loaded_chunks` 仍掃駐留集（已有界）而不是更小的 simulation 集。
- `set_block`／`safe_spawn_y` 在 gameplay 與測試路徑仍可 `ensure_chunk`（tick 熱路徑已過濾）。
