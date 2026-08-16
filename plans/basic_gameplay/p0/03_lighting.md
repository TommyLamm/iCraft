# 任務 3：光照系統

> **複雜度**: ⭐⭐⭐⭐  
> **涉及面**: 數據結構、BFS 傳播算法、頂點屬性、Shader  
> **前置條件**: 任務 1 (多 Chunk — 光照需跨 Chunk 傳播)

---

## 3.1 光照數據結構
**目標**: 每個方塊存儲 sky light 和 block light 等級 (0~15)

### 修改文件
- **[NEW]** `src/lighting.rs` — 光照計算模組
- **[MODIFY]** `src/world.rs` — Chunk 增加光照數據
- **[MODIFY]** `src/state.rs` — Vertex 增加光照分量
- **[MODIFY]** `src/shader.wgsl` — 片段著色器套用光照

### 實現細節
```rust
pub struct Chunk {
    pub blocks: [[[BlockType; 16]; 256]; 16],
    pub sky_light: [[[u8; 16]; 256]; 16],    // 0~15
    pub block_light: [[[u8; 16]; 256]; 16],  // 0~15
}
```

### 子任務清單
- [ ] Chunk 新增 `sky_light` 和 `block_light` 陣列
- [ ] 光照等級定義：0 = 全暗，15 = 全亮

---

## 3.2 天空光傳播
- [ ] 從最高非透明方塊往下填充 sky_light = 15
- [ ] BFS 向四周擴散，每擴散一格衰減 1
- [ ] 透明方塊 (玻璃/空氣) 不阻擋光照
- [ ] 非透明方塊完全阻擋光照

---

## 3.3 方塊光傳播
- [ ] 定義光源方塊清單 (Torch=14, Glowstone=15, Lava=15)
- [ ] 從光源方塊開始 BFS 擴散，每格衰減 1
- [ ] 方塊放置/移除時增量更新光照

---

## 3.4 光照渲染
- [ ] Vertex 新增 `light_level: f32` 屬性
- [ ] `generate_mesh()` 時查詢每個面的光照等級
- [ ] 面的光照 = max(sky_light, block_light) / 15.0
- [ ] 不同面方向的基礎光照修正 (top=1.0, sides=0.8, bottom=0.5)
- [ ] Shader 中 `final_color = texture_color * light_level`

---

## 驗證
- [ ] 洞穴內部明顯變暗
- [ ] 放置火把可照亮周圍
- [ ] 在方塊上方蓋屋頂，內部會變暗
- [ ] 光照過渡自然無突變
