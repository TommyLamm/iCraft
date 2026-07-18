# 任務 1：多 Chunk 支持 + 動態加載

> **複雜度**: ⭐⭐⭐⭐⭐ (最高)  
> **涉及面**: 架構級重構 — 觸及 world、state、physics、interaction、rendering  
> **前置條件**: 無

---

## 1.1 Chunk 管理器
**目標**: 替換單一 `Chunk` 為多 Chunk 世界管理系統

### 修改文件
- **[NEW]** `src/chunk_manager.rs` — Chunk 管理器
- **[MODIFY]** `src/world.rs` — Chunk 增加座標、支持帶偏移的噪聲生成
- **[MODIFY]** `src/state.rs` — 替換 `chunk: Chunk` 為 `chunk_manager: ChunkManager`
- **[MODIFY]** `src/physics.rs` — 碰撞檢測改為查詢 ChunkManager
- **[MODIFY]** `src/interaction.rs` — Raycast 改為查詢 ChunkManager

### 實現細節
```rust
// src/chunk_manager.rs
use std::collections::HashMap;

pub struct ChunkManager {
    pub chunks: HashMap<(i32, i32), Chunk>,  // (chunk_x, chunk_z) -> Chunk
    pub render_distance: i32,                 // 預設 8
}

impl ChunkManager {
    /// 根據玩家位置，加載/卸載 Chunk
    pub fn update_loaded_chunks(&mut self, player_chunk_x: i32, player_chunk_z: i32) { ... }

    /// 查詢世界座標 (wx, wy, wz) 對應的方塊
    pub fn get_block(&self, wx: i32, wy: i32, wz: i32) -> BlockType { ... }

    /// 設置世界座標的方塊
    pub fn set_block(&mut self, wx: i32, wy: i32, wz: i32, block: BlockType) { ... }
}
```

### 子任務清單
- [ ] 建立 `ChunkManager` 結構體與 `HashMap<(i32, i32), Chunk>`
- [ ] 為 `Chunk` 新增 `chunk_x`, `chunk_z` 欄位
- [ ] `Chunk::new(cx, cz, seed)` — 帶世界座標偏移的噪聲生成
- [ ] `ChunkManager::get_block(wx, wy, wz)` — 世界座標→Chunk 座標轉換
- [ ] `ChunkManager::set_block(wx, wy, wz, block)` — 同上，設置方塊
- [ ] `ChunkManager::update_loaded_chunks()` — 根據玩家位置加載/卸載
- [ ] 修改 `physics.rs` 的碰撞檢測：所有 `chunk.get_block()` → `chunk_manager.get_block()`
- [ ] 修改 `interaction.rs` 的 raycast：同上
- [ ] 修改 `state.rs`：持有 `ChunkManager` 替代 `Chunk`

---

## 1.2 跨 Chunk Face Culling
**目標**: Chunk 邊界方塊的 Face Culling 需查詢相鄰 Chunk

- [ ] `generate_mesh()` 接受一個回調/閉包用於查詢跨 Chunk 邊界的鄰居方塊
- [ ] 邊界 (x=0/15, z=0/15) 的面片依據相鄰 Chunk 的方塊決定是否繪製

---

## 1.3 每 Chunk 獨立 GPU 緩衝區
**目標**: 修改方塊時僅重建受影響 Chunk 的 mesh

```rust
pub struct ChunkMesh {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub num_indices: u32,
    pub dirty: bool,  // 需要重建 mesh
}
```

- [ ] 為每個 Chunk 維護獨立的 `ChunkMesh`
- [ ] `render()` 中遍歷所有已加載 Chunk 的 mesh 進行繪製
- [ ] 方塊修改後僅標記對應 Chunk 為 dirty，下一幀重建

---

## 1.4 渲染距離設定
- [ ] 在暫停選單中增加 Render Distance 調整按鈕 (2~16)
- [ ] 持久化到 `settings.txt`

---

## 驗證
- [ ] 能在多個 Chunk 之間自由行走，地形無縫銜接
- [ ] 跨 Chunk 邊界放置/挖掘方塊正常
- [ ] Chunk 動態加載/卸載無卡頓 (至少 render distance = 8)
- [ ] 碰撞檢測在 Chunk 邊界正常工作
