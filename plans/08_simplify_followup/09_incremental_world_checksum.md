# Plan09 — `ServerWorld::checksum` 改增量

## 定位

每個 loaded 維度、每個 tick 掃完整 `block_revisions` HashMap，再 collect + sort 實體兩次（`server_world.rs` ~2074–2170）。註解寫「revisions are sorted first」，程式是 HashMap 直接迭代。`AuthorityCore::tick` 在世界 tick 之後又做一次。欄 resident 期間 revision map 只增不縮。

## 前置

無。07 若改實體廣播，checksum 仍必須是決定性的。

## 精確 acceptance

- [ ] checksum 改 running／mutation 時更新，或至少 revision 用排序切片、實體走一趟。
- [ ] 相同世界狀態（含空 tick）產生相同 checksum。
- [ ] `tests/review_hardening_invariants.rs` 到達順序 checksum 穩定測試通過。
- [ ] 不得為了讓測試過關而放寬期望值。

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
