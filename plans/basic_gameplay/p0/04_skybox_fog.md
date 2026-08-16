# 任務 4：天空盒 + 霧效

> **複雜度**: ⭐⭐ (較小)  
> **涉及面**: Shader 渲染、視覺效果  
> **前置條件**: 無（可獨立實現）

---

## 4.1 天空盒渲染

### 修改文件
- **[NEW]** `src/sky.rs` — 天空渲染模組
- **[MODIFY]** `src/shader.wgsl` — 天空著色器
- **[MODIFY]** `src/state.rs` — 集成天空渲染

### 子任務清單
- [ ] 使用全屏四邊形 (fullscreen quad) + 片段著色器生成天空漸變
- [ ] 天空頂部：深藍色 → 地平線：淺藍色/白色
- [ ] 天空在地形之前渲染 (depth test disabled)
- [ ] 太陽/月亮 billboard (簡單白色圓形)

---

## 4.2 霧效 (Distance Fog)
- [ ] Shader 中計算片段到相機的距離
- [ ] 距離超過一定閾值時，顏色漸變混入天空色
- [ ] `fog_factor = clamp((distance - fog_start) / (fog_end - fog_start), 0, 1)`
- [ ] `final_color = mix(fragment_color, fog_color, fog_factor)`
- [ ] 霧效距離與渲染距離掛鉤

---

## 驗證
- [ ] 天空不再是純色，有漸變效果
- [ ] 遠處地形自然淡出消失
- [ ] 天空與地形邊界不再有生硬切割
