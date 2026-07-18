# 任務 30：渲染優化

> **複雜度**: ⭐⭐⭐⭐  
> **涉及面**: 渲染管線、多線程、算法優化  
> **前置條件**: P0 任務 1 (多 Chunk 渲染)

---

## 30.1 視錐剔除 (Frustum Culling)
- [ ] 從相機矩陣提取 6 個視錐平面
- [ ] 每個 Chunk 與視錐做 AABB 相交測試
- [ ] 不在視野內的 Chunk 不提交 draw call

---

## 30.2 Greedy Meshing
- [ ] 對每個面方向，將相同材質/光照的相鄰面合併為更大的矩形
- [ ] 顯著減少頂點數和 draw call

---

## 30.3 多線程 Mesh 生成
- [ ] 使用 `rayon` 並行生成多個 Chunk 的 mesh
- [ ] 主線程只負責上傳到 GPU
- [ ] 新 Chunk 加載和 mesh 重建在後台線程完成

---

## 30.4 Chunk 排序與批次渲染
- [ ] 不透明 Chunk 從前到後排序 (利用 early-z)
- [ ] 透明 Chunk 從後到前排序 (正確的 alpha blending)
- [ ] 減少 Pipeline/BindGroup 切換

---

## 30.5 LOD (Level of Detail)
- [ ] 遠處 Chunk 使用簡化 mesh (例如只渲染地表面)
- [ ] 超遠處用低解析度的地形輪廓

---

## 驗證
- [ ] Render distance = 16 時 FPS 保持 60+
- [ ] 背對的 Chunk 不渲染 (驗證方法: 減少渲染三角形數)
- [ ] Mesh 生成不導致畫面卡頓
