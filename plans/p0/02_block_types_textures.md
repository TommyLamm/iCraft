# 任務 2：更多方塊類型 + 真實紋理

> **複雜度**: ⭐⭐⭐⭐  
> **涉及面**: 方塊定義、紋理系統、渲染管線、世界生成  
> **前置條件**: 無（可與任務 1 並行部分工作）

---

## 2.1 擴展方塊定義
**目標**: 從 4 種方塊擴展到至少 30 種基礎方塊

### 修改文件
- **[MODIFY]** `src/world.rs` — 擴展 `BlockType` 枚舉
- **[MODIFY]** `src/texture.rs` — 擴展紋理圖集
- **[MODIFY]** `src/world.rs` `generate_mesh()` — 擴展 UV 映射

### 新增方塊清單（第一批）
```rust
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u8)]
pub enum BlockType {
    Air = 0,
    Grass = 1,
    Dirt = 2,
    Stone = 3,
    Sand = 4,
    Gravel = 5,
    OakLog = 6,
    OakPlanks = 7,
    OakLeaves = 8,
    Cobblestone = 9,
    Bedrock = 10,
    Water = 11,
    CoalOre = 12,
    IronOre = 13,
    GoldOre = 14,
    DiamondOre = 15,
    RedstoneOre = 16,
    Glass = 17,
    Brick = 18,
    StoneBrick = 19,
    Snow = 20,
    Ice = 21,
    Clay = 22,
    Sandstone = 23,
    Obsidian = 24,
    CraftingTable = 25,
    Furnace = 26,
    Chest = 27,
    TNT = 28,
    Bookshelf = 29,
    Torch = 30,
}
```

### 子任務清單
- [ ] 擴展 `BlockType` 枚舉至 30+ 種
- [ ] 為每種方塊定義屬性 (硬度、透明度、是否可燃、採集工具)
- [ ] 方塊屬性表：`BlockProperties { hardness, transparent, light_level, tool_type }`

---

## 2.2 紋理圖集升級
**目標**: 生成 Minecraft 風格的 16×16 像素藝術紋理

- [ ] 擴展紋理圖集至 256×256 (16×16 的 16×16 格子)
- [ ] 為每種方塊生成程序化紋理 (仿 Minecraft 風格)
- [ ] 每種方塊最多 3 種面紋理 (top/side/bottom)
- [ ] 建立 `BlockTexture` 查找表：方塊ID + 面方向 → UV 座標
- [ ] 修改 `generate_mesh()` 使用新的 UV 查找表

---

## 2.3 透明方塊渲染
**目標**: 玻璃、樹葉、水等透明/半透明方塊的正確渲染

- [ ] 區分不透明方塊與透明方塊的 mesh
- [ ] 透明面片排序 (Back-to-Front) 或使用 Alpha Test
- [ ] 新增透明方塊渲染 pipeline (alpha blending)
- [ ] 樹葉方塊使用 Alpha Test (cutout)

---

## 2.4 世界生成改進
- [ ] 基岩層生成 (Y=0~4 隨機基岩)
- [ ] 地下礦脈生成 (煤、鐵、金、鑽石按層級分佈)
- [ ] 地形帶沙灘 (水面附近的沙子替換)

---

## 驗證
- [ ] 世界中可見到 15+ 種不同方塊
- [ ] 每種方塊的紋理清晰可辨
- [ ] 透明方塊 (玻璃/樹葉) 渲染正確
- [ ] 地下可見不同礦物
