# 任務 20：環境光遮蔽 (AO)

> **複雜度**: ⭐⭐⭐  
> **涉及面**: Mesh 生成算法、頂點屬性、Shader  
> **前置條件**: P0 任務 1 (Mesh 系統)

---

## 修改文件
- **[MODIFY]** `src/world.rs` `generate_mesh()` — 計算 AO 值
- **[MODIFY]** `src/state.rs` `Vertex` — 增加 AO 分量
- **[MODIFY]** `src/shader.wgsl` — 套用 AO 暗化

---

## 實現細節
- [ ] **Vertex AO 算法**: 對每個面的 4 個頂點，檢查 3 個相鄰方塊的遮擋
  - 0 遮擋 = AO 1.0 (最亮)
  - 1 遮擋 = AO 0.75
  - 2 遮擋 = AO 0.5
  - 3 遮擋 = AO 0.25 (最暗)
- [ ] AO 值存入頂點屬性
- [ ] Shader 中: `final_color *= ao_value`
- [ ] **AO 修正三角形**: 當對角頂點 AO 差異大時翻轉三角形劃分避免視覺錯誤

---

## 驗證
- [ ] 方塊角落和邊緣有柔和的暗色陰影
- [ ] 室內感覺更有深度
- [ ] AO 不應有明顯的不自然色塊
