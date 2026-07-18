# 任務 9：日夜循環

> **複雜度**: ⭐⭐⭐  
> **涉及面**: 時間系統、天空渲染、光照更新  
> **前置條件**: P0 任務 3 (光照) + 任務 4 (天空盒)

---

## 9.1 時間系統

### 新增/修改文件
- **[MODIFY]** `src/sky.rs` — 天空顏色隨時間變化
- **[MODIFY]** `src/lighting.rs` — 天空光照隨時間變化
- **[MODIFY]** `src/state.rs` — 時間系統集成

### 實現細節
```rust
pub struct WorldTime {
    pub ticks: u64,           // 每秒 20 ticks
    pub day_length: u64,      // 24000 ticks = 20 分鐘
}

impl WorldTime {
    pub fn time_of_day(&self) -> f32 { (self.ticks % self.day_length) as f32 / self.day_length as f32 }
    pub fn sun_angle(&self) -> f32 { self.time_of_day() * 2.0 * PI }
    pub fn sky_light_level(&self) -> u8 { /* 0~15 基於太陽位置 */ }
}
```

### 子任務清單
- [ ] 定義 `WorldTime` 結構體，每幀累加 ticks
- [ ] 天空顏色隨時間平滑變化：日出(橙)→白天(藍)→日落(橙紅)→夜晚(深藍)
- [ ] 全局天空光照等級隨時間變化 (白天15，夜晚4)
- [ ] 太陽/月亮位置隨時間繞圈
- [ ] 夜晚天空顯示星星（隨機點）
- [ ] F3 Debug 信息顯示當前遊戲時間

---

## 驗證
- [ ] 天空顏色 20 分鐘內完成一個完整循環
- [ ] 日出/日落時天空有漸變效果
- [ ] 夜晚世界明顯變暗
- [ ] 太陽和月亮位置正確交替
