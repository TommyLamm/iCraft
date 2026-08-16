# 任務 19：F3 Debug 畫面

> **複雜度**: ⭐⭐ (小)  
> **涉及面**: 文字渲染、遊戲狀態讀取  
> **前置條件**: 無（讀取現有狀態即可）

---

## 修改文件
- **[MODIFY]** `src/state.rs` — F3 切換 debug overlay

---

## 子任務清單
- [x] F3 鍵切換 Debug HUD 顯示/隱藏
- [x] 顯示內容 (文字渲染在左上角):
  - [x] FPS (每秒幀數) 和幀時間 (ms)
  - [x] 玩家座標 (X, Y, Z) 精確到小數點 3 位
  - [x] 玩家面朝方向 (yaw / pitch in degrees)
  - [x] 所在 Chunk 座標 (Chunk X, Z)
  - [x] 所在生態群系名稱
  - [x] 已加載 Chunk 數量
  - [x] 實體數量
  - [x] 渲染的三角形/頂點數
  - [x] 記憶體使用量 (估算)
  - [x] 遊戲時間 / Day count

---

## 驗證
- [x] F3 正確切換 debug overlay
- [x] FPS 數值合理且實時更新
- [x] 座標隨移動實時變化
