# 實作計畫 08：Creative 原版式物品選擇界面

> 狀態：已完成（2026-07-24）；自動驗證已通過，GPU 畫面人工驗收待辦。

## 目標

Creative 按 E 顯示可瀏覽所有非 Air 物品的 9×5 無限供應目錄、分類頁籤、
滾動條與底部快捷欄；Survival 與工作站介面保持原樣。

## 現況

- `Item` 有 145 variants，扣掉 Air 應有 144 個 Creative catalog items。
- `Inventory::new_creative` 目前只預置 36 種，遺漏 108 種。
- `Item::properties` 已是名稱、堆疊、圖示與 block mapping 的完整資料源。

## 實作步驟

1. [x] 在 `inventory.rs` 定義完整且唯一的 `CREATIVE_ITEMS` 與 `CreativeTab`：
   All、Blocks、Tools、Combat、Food & Brewing、Redstone、Misc。
2. [x] `Inventory` 增加 tab/scroll UI 狀態與純 helper：分類、可見 45 格、最大
   scroll、clamp、逐列滾動。
3. [x] 修正 SplashPotion 由 `ItemStack::new` 建立正確 water splash metadata。
4. [x] `SlotType` 增加只讀虛擬 `Creative(Item)`；目錄物品不寫入 main inventory。
5. [x] Creative 且非工作站時，`get_inventory_slots` 生成 9×5 catalog + 9 hotbar；
   左鍵取 max stack，右鍵取 1，hotbar 仍重用現有交換/合併操作。
6. [x] render 加分類 tabs、scrollbar、目錄背景與 tooltip；隱藏 Survival armor/
   2×2 crafting 元素。
7. [x] inventory-open 的 MouseWheel 優先滾 catalog，不切 hotbar。
8. [x] 關閉時處理 catalog 生成的 dragged stack，不造成真實物品遺失或重複。
9. [x] 更新架構與進度文件。

## 驗證

- [x] Catalog 恰好包含每個非 Air Item 一次，屬性與 atlas 座標有效。
- [x] tabs partition 無遺漏/重複；9×5 window 與 scroll clamp 正確。
- [x] 左/右鍵供應量、虛擬槽不被清空、物品可放進 hotbar。
- [x] Survival、Crafting Table、Enchanting、Brewing、Anvil 由 layout gate
  保持標準介面。
- [x] `cargo fmt --all -- --check`、`cargo test --release`、
  `cargo check --release`；238 項單元測試與 1 項整合測試通過。
- [ ] 人工瀏覽 144 種物品、分類、滾動、tooltip、拖到快捷欄。

## Commit

單一功能 commit：`feat(creative): add item catalog inventory`
