# Plan09 — 實體 idle skip 與增量 `sync_positions`

## 定位

`ServerWorld::tick_entities` 對每個非 hook 實體：扣 cooldown、敵對每 tick 寫 `velocity`、`ai_phase++`、`update_physics`，最後 `sync_positions()` 掃 **全部 ID**。落地物 grounded skip 已做；活體／敵對／空間索引未增量。

副作用：敵對每 tick 寫 velocity 會讓 Wave 08 的 EntityState fingerprint **幾乎每 tick dirty**，把 dirty broadcast 的收益吃掉。

`entity.rs` 已有 `sync_entity_positions(moved_ids)`，tick 仍呼叫全量 `sync_positions()`。

## 前置

無。08 若已加 entity dirty 指紋，本計劃應複用，不要第二套。

## 精確 acceptance

- [x] 靜止被動／睡眠／零速度且非敵對追擊的實體跳過 `update_physics`（未載入欄凍結語意不變）。
- [x] 敵對只在目標／距離／difficulty 需要時寫 `velocity`；沒有玩家或不在追擊距離時不要每 tick 賦值相同速度。
- [x] `tick_entities` 累積 `moved_ids`，呼叫 `sync_entity_positions`；靜止實體不進列表。
- [x] Peaceful 清敵對、未載入欄凍結、boss `ensure_dimension_entities` 不變。
- [x] 漏記 mover 會讓 combat／撿取空間 query 漏實體 — 必須有測試：移動後 `query_radius` 仍找得到。

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

## 實作與證據

### 改了什麼

- `ServerWorld::tick_entities`：累積 `moved_ids`，改呼叫 `sync_entity_positions`；靜止 living（sitting／anchored／grounded／零速飛行）且非追擊時跳過 `update_physics` 與 `ai_phase++`／`ai_timer`。
- 敵對 chase：僅在 `HOSTILE_CHASE_RANGE`（40）內寫水平速度，且 desired 與現值不同才賦值；超出距離／無玩家清除 `target_player`，不每 tick 重寫相同速度。
- Cooldown／invuln／fire-aspect 計時器仍每 tick 遞減；DroppedItem 仍走既有 `update_physics` 內 grounded skip（pickup cooldown 不變）。
- 未改 `physics.rs` 碰撞形狀；Peaceful 清敵對、`ensure_dimension_entities`／boss update 路徑不變。
- `ARCHITECTURE.md`：補 living idle skip 與增量 spatial sync 契約。

### 測了什麼

- `cargo test --lib entity::` — 28 passed（含 `moved_entity_remains_findable_via_query_radius_after_incremental_sync`）。
- `cargo test --lib -- tick_entities` — 1 passed（`tick_entities_keeps_moved_entity_findable_via_query_radius`）。
- `cargo test --lib -- checksum` — 13 passed（含 stationary living fingerprint reuse）。
- 新增：`stationary_living_entity_skips_physics_and_reuses_checksum_fingerprint`、`sitting_living_entity_skips_physics_while_airborne_velocity_is_zero`、`hostile_out_of_chase_range_does_not_rewrite_velocity_each_tick`；既有 `difficulty_controls_hostile_policy_*`（近距 chase）仍通過。

### 留下的缺口

- 敵對仍線性掃全部玩家找最近目標（計劃明示不做空間 bucket）。
- 非 living（載具／投射物／掉落物）不走 tick 層 idle skip；掉落物繼續靠內部 grounded skip。
- Boss／Enderman 專用 AI 仍在 `boss::update_dimension_entities`；與 generic hostile chase 是分開路徑。
- `ai_timer` 在 idle skip 時不推進；若未來被動 AI 依賴 timer 喚醒，需另開計劃。
