# 任務 30：渲染優化

> **複雜度**: ⭐⭐⭐⭐  
> **涉及面**: 渲染管線、多線程、算法優化  
> **前置條件**: P0 任務 1 (多 Chunk 渲染)

---

## 30.1 視錐剔除 (Frustum Culling)
- [x] 從相機矩陣提取 6 個視錐平面
- [x] 每個 Chunk 與視錐做 AABB 相交測試
- [x] 不在視野內的 Chunk 不提交 draw call

---

## 30.2 Greedy Meshing
- [x] 對每個面方向，將相同材質/光照的相鄰面合併為更大的矩形
- [x] 顯著減少頂點數和 draw call

---

## 30.3 多線程 Mesh 生成
- [x] 使用 `rayon` 並行生成多個 Chunk 的 mesh
- [x] 主線程只負責上傳到 GPU
- [x] 新 Chunk 加載和 mesh 重建在後台線程完成

---

## 30.4 Chunk 排序與批次渲染
- [x] 不透明 Chunk 從前到後排序 (利用 early-z)
- [x] 透明 Chunk 從後到前排序 (正確的 alpha blending)
- [x] 減少 Pipeline/BindGroup 切換

---

## 30.5 LOD (Level of Detail)
- [x] 遠處 Chunk 使用簡化 mesh (例如只渲染地表面)
- [x] 超遠處用低解析度的地形輪廓

---

## 驗證
- [ ] Render distance = 16 時 FPS 保持 60+
- [x] 背對的 Chunk 不渲染 (驗證方法: 減少渲染三角形數)
- [x] Mesh 生成不導致畫面卡頓

> `60+ FPS` 是硬件與視窗場景相關的人工效能驗收，本次環境未自動操作遊戲
> 世界進行可靠 FPS 量測。其餘兩項由可見 draw plan 統計測試、bounded
> Rayon worker、過期結果丟棄測試與 Release 編譯／測試覆蓋。
