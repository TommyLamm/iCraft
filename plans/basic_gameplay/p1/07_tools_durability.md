# 任務 7：工具與耐久度

> **複雜度**: ⭐⭐⭐  
> **涉及面**: 物品系統、挖掘速度計算、UI  
> **前置條件**: 任務 6 (背包系統)

---

## 7.1 工具系統

### 新增/修改文件
- **[NEW]** `src/item.rs` — 物品定義
- **[MODIFY]** `src/interaction.rs` — 挖掘速度計算
- **[MODIFY]** `src/state.rs` — 挖掘進度 UI

### 實現細節
```rust
pub enum ToolType { None, Pickaxe, Axe, Shovel, Hoe, Sword }
pub enum ToolMaterial { Wood, Stone, Iron, Gold, Diamond }

pub struct ToolProperties {
    pub tool_type: ToolType,
    pub material: ToolMaterial,
    pub mining_speed: f32,
    pub durability: u32,
    pub damage: f32,
}
```

### 子任務清單
- [ ] 定義 `ToolType` 和 `ToolMaterial` 枚舉
- [ ] 每種方塊需要的工具類型 + 最低工具等級
- [ ] 挖掘速度計算：`base_time / (tool_speed_multiplier)`
- [ ] 手動挖掘：不使用正確工具時速度極慢
- [ ] 挖掘進度系統：長按左鍵持續挖掘，進度滿後方塊被破壞

---

## 7.2 耐久度系統
- [ ] 每次使用工具消耗 1 耐久
- [ ] 耐久度歸零時工具損壞（播放音效，從背包移除）
- [ ] 快捷欄/背包中顯示耐久條 (彩色漸變: 綠→黃→紅)

---

## 7.3 挖掘進度渲染
- [ ] 長按左鍵時，方塊表面覆蓋 10 級破壞紋理
- [ ] 破壞進度隨時間增加
- [ ] 鬆開左鍵或切換目標時重置進度
- [ ] 進度滿後方塊被移除 + 掉落物品

---

## 驗證
- [ ] 用鎬挖石頭比用手快很多
- [ ] 工具使用後耐久度減少
- [ ] 耐久度歸零工具消失
- [ ] 挖掘有漸進破壞效果
