# 任務 21：附魔 / 釀造系統

> **複雜度**: ⭐⭐⭐⭐⭐ (最高)  
> **涉及面**: 附魔機制、釀造機制、GUI、效果系統  
> **前置條件**: P1 任務 6 (背包) + 任務 7 (工具) + 任務 8 (生命)

---

## 21.1 附魔系統

### 新增文件
- **[NEW]** `src/enchantment.rs` — 附魔系統

### 實現細節
```rust
pub enum Enchantment {
    // 工具附魔
    Efficiency(u8),     // 效率 I~V: 加快挖掘速度
    Unbreaking(u8),     // 耐久 I~III: 降低耐久消耗概率
    SilkTouch,          // 絲綢之觸: 方塊直接掉落本體
    Fortune(u8),        // 時運 I~III: 增加掉落數量
    // 武器附魔
    Sharpness(u8),      // 鋒利 I~V: 增加攻擊傷害
    Knockback(u8),      // 擊退 I~II: 擊退距離
    FireAspect(u8),     // 火焰附加 I~II: 點燃目標
    Looting(u8),        // 搶奪 I~III: 增加掉落物
    // 盔甲附魔
    Protection(u8),     // 保護 I~IV: 減少傷害
    FeatherFalling(u8), // 摔落保護 I~IV: 減少摔落傷害
    Respiration(u8),    // 水下呼吸 I~III: 延長水下氧氣
    // 弓附魔
    Power(u8),          // 力量 I~V: 增加弓箭傷害
    Infinity,           // 無限: 不消耗箭矢
}
```

### 子任務清單
- [ ] 定義附魔枚舉及其效果
- [ ] 附魔台方塊 + GUI:
  - 3 個附魔選項 (隨機生成)
  - 消耗經驗等級 + 青金石
  - 書架數量影響附魔等級上限 (最多 15 個書架)
- [ ] 附魔效果套用:
  - 挖掘時套用效率/絲綢之觸/時運
  - 攻擊時套用鋒利/擊退/火焰附加
  - 受傷時套用保護/摔落保護
- [ ] 附魔光澤效果: 附魔物品有紫色光澤動畫
- [ ] 鐵砧: 合併附魔 + 修復工具 + 重命名

---

## 21.2 釀造系統

### 新增文件
- **[NEW]** `src/brewing.rs` — 釀造系統

### 實現細節
```rust
pub enum PotionEffect {
    Speed { level: u8, duration: f32 },
    Strength { level: u8, duration: f32 },
    Healing { level: u8 },
    Regeneration { level: u8, duration: f32 },
    NightVision { duration: f32 },
    Invisibility { duration: f32 },
    FireResistance { duration: f32 },
    WaterBreathing { duration: f32 },
    Poison { level: u8, duration: f32 },
    Slowness { level: u8, duration: f32 },
}
```

### 子任務清單
- [ ] 釀造台方塊 + GUI (水瓶 + 原料 → 藥水)
- [ ] 釀造配方系統
- [ ] 藥水效果套用:
  - 速度: 修改移動速度
  - 力量: 修改攻擊傷害
  - 治療: 立即回血
  - 再生: 持續回血
  - 夜視: 在暗處看清
  - 隱形: 生物不主動攻擊
- [ ] 效果持續時間 HUD 顯示
- [ ] 噴濺藥水: 投擲 + 範圍效果

---

## 驗證
- [ ] 附魔台可使用, 消耗經驗和青金石
- [ ] 附魔效果生效 (效率鎬挖得更快)
- [ ] 藥水釀造流程正確
- [ ] 喝下藥水後效果 HUD 顯示且生效
