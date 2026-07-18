# 任務 28：成就 / 進度系統

> **複雜度**: ⭐⭐⭐  
> **涉及面**: 事件系統、UI 通知、數據持久化  
> **前置條件**: 大部分遊戲內容完成（觸發條件依賴各系統）

---

## 28.1 進度追蹤

### 新增文件
- **[NEW]** `src/advancements.rs` — 進度系統

### 實現細節
```rust
pub struct Advancement {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub parent: Option<&'static str>,
    pub trigger: AdvancementTrigger,
    pub completed: bool,
}

pub enum AdvancementTrigger {
    ObtainItem(Item),
    EnterBiome(Biome),
    KillMob(EntityType),
    CraftItem(Item),
}
```

### 子任務清單
- [ ] 定義進度樹 (約 50 個進度)
  - 石器時代 (獲取石鎬)
  - 開始了 (獲取工作台)
  - 穫取鑽石
  - 進入地獄
  - 終局之戰 (擊敗末影龍)
- [ ] 觸發條件檢測: 在物品獲取/生物擊殺/位置進入等事件中檢查
- [ ] Toast 通知: 完成進度時螢幕右上角彈出提示
- [ ] 進度界面: 可查看進度樹圖

---

## 驗證
- [ ] 首次獲取木材時彈出進度通知
- [ ] 進度界面可查看已完成/未完成項目
- [ ] 進度正確保存/載入
