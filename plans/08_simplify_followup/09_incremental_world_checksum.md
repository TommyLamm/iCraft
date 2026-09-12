# Plan09 — `ServerWorld::checksum` 改增量

## 定位

每個 loaded 維度、每個 tick 掃完整 `block_revisions` HashMap，再 collect + sort 實體兩次（`server_world.rs` ~2074–2170）。註解寫「revisions are sorted first」，程式是 HashMap 直接迭代。`AuthorityCore::tick` 在世界 tick 之後又做一次。欄 resident 期間 revision map 只增不縮。

## 前置

無。07 若改實體廣播，checksum 仍必須是決定性的。

## 精確 acceptance

- [x] checksum 改 running／mutation 時更新，或至少 revision 用排序切片、實體走一趟。
- [x] 相同世界狀態（含空 tick）產生相同 checksum。
- [x] `tests/review_hardening_invariants.rs` 到達順序 checksum 穩定測試通過。
- [x] 不得為了讓測試過關而放寬期望值。

## 預計檔案與測試

- `src/server_world.rs`、`src/authority/tick.rs`
- 驗證：review_hardening checksum；`cargo test --lib server_world::`

## 建議階段

1. 先修「HashMap 迭代卻聲稱 sorted」：要嘛排序，要嘛增量。
2. 實體 payload 合進同一迴圈。
3. 對拍舊 checksum 一組固定種子世界。

## 不在本計劃

- 換加密 hash 演算法。
- 平行化 tick。

## 實作與證據

工作樹 `C:\Users\Tommy\Desktop\iCraft-wt-08-09`，分支 `plan/08-09-incremental-checksum`，起點 `plan/08-05-remove-debug-logs` @ `497ee60`。

### 改了什麼

`ServerWorld::checksum` 不再每 tick 掃完整 `block_revisions`，也不再 collect + sort 實體兩次：

- `block_revisions` 插入／覆寫／欄驅逐時維護 running XOR（每筆仍是 FNV-1a fingerprint）。idle tick 只寫入 8-byte 聚合。
- 實體改成排序切片後走一趟：同一迴圈寫 pose、type、dropped／potion payload。
- 仍用 FNV-1a；未改 `authority/tick.rs` 呼叫點、未平行化 tick、未改實體廣播協定。

未改 ARCHITECTURE.md（snapshot 仍帶 checksum，演算法是 `ServerWorld` 內部細節）。

### 測了什麼

- `cargo test --lib server_world::` — 22 passed（含 `fixed_tick_checksum_is_deterministic`、`running_revision_checksum_matches_sorted_map_fold`、`checksum_is_independent_of_revision_insert_order`、`empty_ticks_keep_matching_checksums_for_identical_worlds`、`checksum_distinguishes_entity_type_at_same_pose`、`checksum_distinguishes_raw_fluid_mutations`）
- `cargo test --test review_hardening_invariants fixed_tick_checksum_is_independent_of_inbound_arrival_order -- --exact` — 1 passed

未放寬既有期望：流體 raw 差異、同 pose 不同實體型別、到達順序獨立性仍 `assert_ne!` / `assert_eq!`。

### 留下的缺口

`mutation_revision_index` / `chunk_revision` 後備路徑仍會掃 `block_revisions`（存檔／卸載，不是每 tick checksum）。實體位置每 tick 都會變，checksum 仍必須走一趟實體。