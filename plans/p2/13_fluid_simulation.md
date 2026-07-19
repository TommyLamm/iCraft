# 任務 13：水 / 岩漿流體模擬

> **複雜度**: ⭐⭐⭐⭐⭐ (最高)  
> **涉及面**: 流體物理、渲染管線、玩家交互、水下效果  
> **前置條件**: P0 (多 Chunk + 透明渲染)
> 
> **設計規格書**: [fluid-simulation-design.md](file:///f:/Desktop/MC/docs/superpowers/specs/2026-07-19-fluid-simulation-design.md)
> **實作計畫**: [fluid-simulation.md](file:///f:/Desktop/MC/docs/superpowers/plans/2026-07-19-fluid-simulation.md)

---

## 13.1 流體數據結構與模擬

### 新增/修改文件
- **[NEW]** `src/fluid.rs` — 流體擴散與更新模擬系統
- **[MODIFY]** `src/world.rs` — Chunk 結構與 `BlockType` 增設 `fluid_levels` 及 `BlockType::Lava`
- **[MODIFY]** `src/chunk_manager.rs` — 增加流體等級與 waterfall 狀態的 bit 讀寫介面
- **[MODIFY]** `src/shader.wgsl` — 水下深度藍霧、UV 紋理動畫滾動

### 流體模擬核心設計
- **流體狀態編碼**:
  - `Chunk` 新增 `fluid_levels: Box<[[[u8; 16]; 256]; 16]>`，使用 1 byte 儲存流體資訊：
    - `Bits 0..3`: `level` (0 = 滿/源頭, 1~7 = 流動遞減級別，7 為最薄)
    - `Bit 3 (0x08)`: `falling` 是否為垂直下落狀態
- **流體 Tick 計算**:
  - 水每 5 ticks 更新一次，岩漿每 30 ticks 更新一次。
  - 下方為 passable 方塊時垂直流動且 `level` 保持 0，標記 `falling = true`。
  - 下方為 solid 時向 4 個水平方向以 `level + 1` 擴散。
  - 缺乏來源時 `level` 逐 tick 遞減，直至 `level > 7` 回復為 `Air`。
- **水源生成**:
  - 空氣方塊下方為固體且水平方向有 2 個以上水水源（`level == 0`）時，自動轉化為新水源。
- **水+岩漿交互**:
  - 水流入岩漿源（level 0）-> **黑曜石 (Obsidian)**
  - 水流入流動岩漿（level > 0）-> **圓石 (Cobblestone)**
  - 岩漿流入水源（level 0）-> **石頭 (Stone)**
  - 岩漿流入流動水（level > 0）-> **圓石 (Cobblestone)**

---

## 13.2 流體渲染與玩家交互

- **動態流體高度**:
  - `generate_mesh()` 中如果為流體，調整頂面 Y 座標為 `world_y + (8 - level)/8 * 0.9`。
- **UV 流動動畫**:
  - 在 `CameraUniform` 傳入全域 `total_time`，並在 shader 中為流體 UV 加上滾動偏移量。
- **玩家物理**:
  - 進入水/岩漿時加速度減弱、終端速度設限、水平運動受阻尼。
  - 按住 Space 鍵在水中提供向上浮力以進行游泳。
  - 處於岩漿中時每秒造成 `4.0` 點燃傷害。
- **水中呼吸與溺水**:
  - 頭部高於 eye level（Y+1.62）處於水中時扣除氧氣（最高 300 刻/15 秒）。
  - 氧氣耗盡時每秒造成 `2.0` 傷害，傷害來源標記為 `DamageSource::Drowning`。
  - 當氧氣不足時，在 HUD 上以泡沫（Bubbles）進度條渲染。
- **水下視覺**:
  - 當頭部處於水中，shader 大幅拉近 fog_end 並混合深藍色濾鏡。

---

## 驗證

- [ ] 放置水桶後水能自然流動並依距離變薄
- [ ] 3x1 水槽兩端放水，中間能生成無限水
- [ ] 水和岩漿相遇時能正確轉化為黑曜石、圓石和石頭
- [ ] 玩家進入水中能藉由 Space 鍵游泳，並受流體摩擦力減速
- [ ] 頭部浸入水中後氣泡條減少，氧氣歸零後扣血，浮出水面後迅速恢復
- [ ] 水下視覺呈現深藍色霧效，且視線受限
