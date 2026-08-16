# 任務 5：快捷欄 + 方塊選擇

> **複雜度**: ⭐⭐⭐  
> **涉及面**: 數據結構、UI 渲染、輸入綁定  
> **前置條件**: 任務 2 (需有多種方塊可選)

---

## 5.1 快捷欄數據

### 修改文件
- **[NEW]** `src/inventory.rs` — 物品/背包系統基礎
- **[MODIFY]** `src/state.rs` — 集成快捷欄
- **[MODIFY]** `src/app.rs` — 數字鍵和滾輪輸入

### 實現細節
```rust
pub struct ItemStack {
    pub block_type: BlockType,
    pub count: u32,
}

pub struct Hotbar {
    pub slots: [Option<ItemStack>; 9],
    pub selected: usize,  // 0~8
}
```

### 子任務清單
- [x] 定義 `ItemStack` 和 `Hotbar` 結構體
- [x] 創造模式：快捷欄預填 9 種常用方塊
- [x] 數字鍵 1~9 切換選中格
- [x] 滑鼠滾輪切換選中格
- [x] 右鍵放置使用當前選中方塊類型
- [x] 左鍵挖掘回收方塊到背包（生存模式）

---

## 5.2 快捷欄 HUD 渲染
- [x] 底部居中繪製 9 個格子
- [x] 選中格有高亮邊框
- [x] 每個格子內顯示方塊的縮略紋理
- [x] 顯示堆疊數量

---

## 驗證
- [x] 按 1~9 可切換選中格
- [x] 滾輪可切換選中格
- [x] 右鍵放置的方塊類型與選中格一致
- [x] 底部快捷欄清晰可見
