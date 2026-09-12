# Plan08 — 拿掉雙重 checksum 與實體全表排序

## 定位

Wave 08 Plan 09 讓 **block revisions** 走 XOR 增量，idle 不再掃 resident map。剩餘成本：

1. `ServerWorld::tick` 算一次 `checksum` 放進自己的 snapshot（`server_world.rs`），`AuthorityCore::tick` **丟棄**該值，再對同一世界呼叫 `world.checksum(entries)`。
2. `ServerWorld::checksum` 每次分配 `order: Vec<usize>` 並對 **全部實體** `sort_unstable_by_key` 再 hash，即使實體集合與位姿相對上一個 checksum tick 沒變。

20 Hz × 每維度。安靜世界的實體排序是現在 checksum 的主成本。

## 前置

無。不要與 06 同一 PR 混 session snapshot 形狀。

## 精確 acceptance

- [x] `ServerWorld::tick` 不再為丟棄的 snapshot 算 checksum（或該欄位刪除／固定 0，由 `AuthorityCore` 唯一計算）。
- [x] 實體雜湊：idle（無 entity spawn／despawn／moved／ai_phase 變化）跳過 sort+全表 hash，重用上一 tick 的實體指紋。
- [x] 有實體變更時仍決定性：同一集合同一順序得到同一 u64；既有 checksum 測試期望不變。
- [x] `aggregate_dimension_checksums` 語意不變。
- [x] `cargo test --lib -- checksum` 與 `src/microbench.rs` smoke（若有）通過。

## 預計檔案與測試

- `src/server_world.rs`、`src/authority/tick.rs`、`src/entity.rs`（若加 dirty／fingerprint）
- 驗證：既有 `checksum` 單元測試；`cargo test --lib server_world::`；`cargo test --bin icraft -- microbench`（若編譯）

## 建議階段

1. 刪 `ServerWorld::tick` 內 checksum；確認沒有測試讀那個 snapshot.checksum。
2. 為 entity 集合加 generation／XOR 指紋；idle 重用。
3. 對照決定性測試（同 seed 兩次 tick 序列）。

## 不在本計劃

- 平行化 tick。
- 改 block revision XOR（已落地）。
- 改實體物理／AI（09）。

## 實作與證據

### 改了什麼

- `ServerWorld::tick`：snapshot `checksum` 固定為 `0`；權威 checksum 只由 `AuthorityCore::tick` 在 fold pending dispense／mutations 後呼叫 `world.checksum`。
- `EntityManager`：新增 `checksum_epoch` + `cached_entity_fingerprint`；spawn／despawn／rebuild／chunk-cross 與 `mark_checksum_inputs_changed` 使指紋失效。
- `ServerWorld::checksum`：實體改為獨立 sorted FNV 指紋（與 block-revision XOR 同型），idle 時重用；`AuthorityCore` 改 `world_mut` 以便寫入快取。
- `tick_entities`：偵測 pose／`ai_phase` 變化；boss 路徑用 before／after key 比對，避免空白世界被誤標記 dirty。
- `ARCHITECTURE.md`：說明單一 checksum 路徑與實體指紋快取。

### 測了什麼

- `cargo test --lib -- checksum` — pass（含既有決定性／區分測試與新 idle reuse）
- `cargo test --lib server_world::` — 24 passed
- `cargo test --lib smoke_checksums_are_stable` — pass
- `cargo test --test review_hardening_invariants fixed_tick_checksum` — pass
- 新增：`idle_entity_fingerprint_skips_sort_and_hash_rebuild`、`empty_world_checksum_reuses_entity_fingerprint_across_idle_ticks`

### 留下的缺口

- 目前 `tick_entities` 對非 FishingHook 實體每 tick `ai_phase += 1`，有實體在場時指紋仍每 tick 重建；Plan 09 idle AI／physics skip 落地後，安靜實體世界才會穩定命中快取。
- 實體指紋改為先獨立 hash 再混入父 FNV（8-byte），絕對 checksum 數值與舊「直接 inline 混入」不同；相對決定性／區分語意不變，無 golden 常數依賴。
- `dropped_stack` 原地變更未列入 idle dirty 訊號（計劃只要求 spawn／despawn／moved／ai_phase）；若未來有該路徑需補 mark。
