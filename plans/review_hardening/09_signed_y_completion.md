# Plan09 — Signed-Y 殘留收斂

## 定位

- `Dimension::height()`：Overworld `-64..320`、Nether `0..128`、End `0..256`。
  `world.rs` 的 signed-Y helper 是對的；多個模擬域仍當世界是 `0..256`。
- 已確認殘留：
  - `entity.rs` 約 596：碰撞 clamp `0..255`。未載入 column 經 `get_block` 變成 Air
    → 掉落物穿地（挖礦／破箱戰利品消失）。
  - `redstone.rs` 約 264／561：sidecar `local_y: u8`，Y<0 或 Y≥256 存檔丟或 wrap。
  - `fluid.rs` 約 250：`wy == 0` 當基岩；Overworld Y=0 是可玩層，會錯誤形成無限水源。
  - `lighting.rs` 增量光：`0..CHUNK_HEIGHT`、頂當 255、鄰接取樣不跨 Y<0／Y≥256。
  - `world_tick.rs` 農田含水、`passive_mob.rs` 生成掃描仍是 `0..CHUNK_HEIGHT`。
  - `camera.rs` far plane 與 cactus AABB 仍用 `CHUNK_HEIGHT = 256`。
- 玩家物理、流體 tick、敵對地表掃描已改用 `Dimension::height()`——對齊它們。

## 前置

無。不要在本計劃修礦脈／結構（10）。

## 精確 acceptance

- [ ] 實體碰撞 Y 範圍 = 該維度 `min_y .. max_y_exclusive`。未載入 column 不得當空氣：
  該實體本 tick 跳過物理或凍結（選一種，測試釘死）。
- [ ] 紅石 sidecar 存 signed world Y（`i16`）或 section+local，與 block entity 一致。
  舊 `u8` payload 按「當時當 0..256」遷移，不得把 Y=-20 讀成 236。
- [ ] 流體 `is_supported` 刪除 `wy == 0` 特例。支撐 = 下方實心且非本流體。
- [ ] 增量 sky／block light 的 seed／column-fill 全部改 `height.min_y()..max_y_exclusive()`。
  測試：在 Y=5 放不透光，Y=-8 的 sky 變 0；在 Y=256 拆方塊，可從 Y=257 得 sky。
- [ ] 農田含水與被動生成掃描用 `dimension.height().contains_y`。
- [ ] Presentation cactus／far plane 用當前維度高度，不是常數 256。
- [ ] 公開 `CHUNK_HEIGHT` 不再被這些路徑當世界邊界。可暫時留著給 Nether／End 的 256
  高 dense 遺留，但 Overworld 路徑不得再讀它當 max Y。

## 預計檔案與測試

- 修改：`src/entity.rs`、`src/chunk_manager.rs`（`get_block` 的 Air fallback 只在
  「確認已載入」時使用）、`src/redstone.rs`、`src/fluid.rs`、`src/lighting.rs`、
  `src/world_tick.rs`、`src/passive_mob.rs`、`src/camera.rs`、`src/state.rs` cactus
  （約 12880）。
- 測試：各模組單元測；掉落物在 Y=-10 的實心底上不得穿過。

## 建議階段

1. 實體碰撞 + 未載入凍結（守恆風險最高）。
2. 光照 seed／fill。
3. 紅石 persist 遷移。
4. 流體／農田／被動生成／camera。
5. 搜 `0..CHUNK_HEIGHT`、`CHUNK_HEIGHT as i32 - 1`、`wy == 0`，把本計劃範圍內的清掉。

## 不在本計劃

- `place_ores`、structure `OnceLock`、預設基岩 Y、樹冠跨 chunk（10）。
- Superflat 仍先跑完整 gen（10／可選）。
- 新光源模型。
