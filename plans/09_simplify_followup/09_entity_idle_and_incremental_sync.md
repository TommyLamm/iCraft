# Plan09 — 實體 idle skip 與增量 `sync_positions`

## 定位

`ServerWorld::tick_entities` 對每個非 hook 實體：扣 cooldown、敵對每 tick 寫 `velocity`、`ai_phase++`、`update_physics`，最後 `sync_positions()` 掃 **全部 ID**。落地物 grounded skip 已做；活體／敵對／空間索引未增量。

副作用：敵對每 tick 寫 velocity 會讓 Wave 08 的 EntityState fingerprint **幾乎每 tick dirty**，把 dirty broadcast 的收益吃掉。

`entity.rs` 已有 `sync_entity_positions(moved_ids)`，tick 仍呼叫全量 `sync_positions()`。

## 前置

無。08 若已加 entity dirty 指紋，本計劃應複用，不要第二套。

## 精確 acceptance

- [ ] 靜止被動／睡眠／零速度且非敵對追擊的實體跳過 `update_physics`（未載入欄凍結語意不變）。
- [ ] 敵對只在目標／距離／difficulty 需要時寫 `velocity`；沒有玩家或不在追擊距離時不要每 tick 賦值相同速度。
- [ ] `tick_entities` 累積 `moved_ids`，呼叫 `sync_entity_positions`；靜止實體不進列表。
- [ ] Peaceful 清敵對、未載入欄凍結、boss `ensure_dimension_entities` 不變。
- [ ] 漏記 mover 會讓 combat／撿取空間 query 漏實體 — 必須有測試：移動後 `query_radius` 仍找得到。

## 預計檔案與測試

- `src/server_world.rs`、`src/entity.rs`、`src/physics.rs`（僅 caller，不改碰撞形狀）
- 驗證：`cargo test --lib entity::`；`cargo test --lib -- tick_entities`；相關 mob 測試

## 建議階段

1. 把 `sync_positions` 換成 moved list；補「移動後 spatial query」測試。
2. 靜止被動 skip physics。
3. 敵對速度 dirty-write。

## 不在本計劃

- 碰撞 fast-path（fence／stair 穿模風險）。
- 敵對改空間 bucket 找最近玩家（可做小優化，但本計劃以 skip／sync 為主）。
- 每玩家每 tick spawn 改 round-robin（另題）。
