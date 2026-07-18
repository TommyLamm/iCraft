# 2026-07-18 Minecraft wgpu 複製版設計規格書 (Design Spec)

本規格書詳細定義基於 Rust + wgpu + winit 實現的高還原度 Minecraft 核心引擎的設計細節。

---

## 1. 專案概述 (Project Overview)
本專案旨在利用 Rust 語言與底層繪圖 API `wgpu`，從零開始建置一個 3D 沙盒方塊遊戲。我們將以 Minecraft 經典玩法為藍本，實現地形生成、網格最佳化、物理碰撞與方塊互動等核心系統，並兼顧代碼的可擴充性與高效能。

---

## 2. 系統架構 (System Architecture)

專案將分為以下幾個主要模組：
1.  **Application (應用主體與視窗)**：使用 `winit` 建立視窗並管理輸入事件，驅動遊戲核心更新與渲染循環。
2.  **Renderer (渲染模組)**：基於 `wgpu` 管理 GPU 狀態、著色器 (WGSL)、頂點與索引緩衝區、紋理圖集 (Texture Atlas) 與相機 Uniform。
3.  **World (世界與區塊數據)**：管理網格化世界。世界由多個 `Chunk`（$16 \times 256 \times 16$ 方塊）組成，包含區塊噪聲生成與 Face Culling（面片剪裁）網格構建器。
4.  **Physics (物理與相機控制)**：管理玩家的位置、速度與 AABB 碰撞體，處理基於重力與輸入的位移，並預防方塊穿透。
5.  **Interaction (互動模組)**：使用 DDA (Digital Differential Analysis) 3D 射線檢測演算法，定位玩家准星指向的方塊，以執行挖掘與放置。

---

## 3. 詳細模組設計 (Detailed Module Design)

### 3.1. 渲染模組 (Renderer)
-   **顯示 API**：`wgpu` (自動切換至 Vulkan, DX12, Metal)。
-   **著色器語言**：WGSL (WebGPU Shading Language)。
-   **頂點格式 (Vertex Layout)**：
    ```rust
    struct CornerVertex {
        position: [f32; 3],  // 3D 座標
        tex_coords: [f32; 2], // 紋理 UV 座標
        normal: [f32; 3],     // 法線向量（用於簡單光照計算）
        ao: f32,             // 環境光遮蔽強度值 (0.0 ~ 1.0)
    }
    ```
-   **紋理圖集 (Texture Atlas)**：
    -   使用單張大圖（例如 256x256 像素），將草地、泥土、石頭、木頭、葉子等材質切割成 16x16 的子圖。
    -   計算 UV 時，根據方塊類型與面朝向，動態計算頂點對應圖集中的偏移量，以避免在渲染時重置 Texture Bindings。
-   **相機 (Camera)**：
    -   維持 View 矩陣與 Projection 矩陣。
    -   透過 `wgpu::Buffer` 作為 Uniform 變數上傳至 GPU，並於 Shader 進行頂點變換。

### 3.2. 世界管理與網格最佳化 (World & Chunking)
-   **區塊大小**：單個 Chunk 為 $16 \times 256 \times 16$ 的方塊矩陣。
-   **方塊定義 (Block Type)**：
    -   `Air` (0), `Grass` (1), `Dirt` (2), `Stone` (3), `Wood` (4), `Leaves` (5), `Sand` (6), `Glass` (7)。
-   **面片剪裁最佳化 (Face Culling)**：
    -   如果一個方塊面與相鄰的非透明方塊相貼，則該面**不生成**頂點。
    -   僅在方塊面向 `Air`（或水體、玻璃等透明方塊）時才將該面的 4 個頂點和 6 个索引寫入網格緩衝區。
-   **地形噪聲生成 (Terrain Generation)**：
    -   使用 2D Perlin 噪聲產生高度圖，定義地表高度。
    -   地表以下填充 `Stone`，表層填充 `Dirt`，最上一層若無遮蔽則改為 `Grass`。
    -   引入種子碼 (Seed) 以支援隨機且可重現的地貌。

### 3.3. 物理與第一人稱控制 (Physics & Collision)
-   **玩家狀態**：
    -   `position`: 3D 向量。
    -   `velocity`: 3D 向量。
    -   `yaw` & `pitch`: 視角旋轉。
    -   `AABB` (軸對齊包圍盒)：寬度 0.6，高度 1.8（模擬 Minecraft 玩家尺寸）。
-   **重力與阻力**：
    -   每幀應用向下的重力加速度（如 $-32.0 \text{ m/s}^2$）。
    -   應用空氣阻力與地面摩擦力，確保鬆開按鍵時玩家能自然停下。
-   **AABB 碰撞檢測與修正**：
    -   在更新玩家位置時，沿 X、Y、Z 軸分步移動。
    -   在每一步移動後，檢測玩家 AABB 是否與周圍非空氣方塊的 AABB 相交。
    -   若相交，將玩家座標沿移動的反方向推回至邊界，並將該軸的速度歸零（例如落地時 Y 軸速度歸零，允許再次跳躍）。

### 3.4. 互動與射線檢測 (Interaction & Raycasting)
-   **准星射線 (Raycast)**：
    -   從玩家眼睛位置沿視線方向延伸一條長度限制為 5.0 個單位的射線。
    -   使用 DDA (Digital Differential Analysis) 演算法，以極低開銷步進網格，找出射線碰撞的第一個非空氣方塊。
-   **動作響應**：
    -   **挖掘 (滑鼠左鍵)**：將射線命中的方塊修改為 `Air`，並標記所屬 Chunk 重新構建 GPU 網格緩衝區。
    -   **放置 (滑鼠右鍵)**：取得命中方塊的表面法線，在相鄰的空氣方塊位置放置當前手持的方塊（如 `Stone`），並更新 Chunk 網格。

---

## 4. 驗證與測試計劃 (Verification & Test Plan)

### 4.1. 自動編譯與程式碼靜態分析
-   執行 `cargo check` 與 `cargo clippy` 確保代碼無警告與錯誤。
-   執行 `cargo test` 驗證 AABB 碰撞演算法、DDA 射線檢測算法與噪聲生成模組的正確性。

### 4.2. 手動測試場景 (Manual Test Matrix)
-   **視窗與相機**：啟動後滑鼠游標成功鎖定，移動滑鼠視角平滑旋轉，無方向反轉與死角。
-   **地形生成**：生成出有起伏的綠色地表，底層為泥土和岩石。
-   **移動與物理**：玩家會受重力掉落，按空白鍵可跳躍，且不能穿透地面或周圍方塊。
-   **挖掘與放置**：對著方塊左鍵點擊可使其消失；右鍵點擊可在其表面外側產生新方塊，且新產生的方塊同樣具有物理碰撞判定。
