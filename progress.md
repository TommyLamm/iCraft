# Minecraft wgpu 複製版開發進度追蹤 (progress.md)

本文件用於記錄專案開發進度，以防長對話上下文壓縮導致開發細節丟失。

---

## 1. 系統環境與技術棧
*   **語言**：Rust (1.78.0) 與 Cargo (1.78.0)
*   **視窗與輸入**：`winit`
*   **底層圖形庫**：`wgpu` (自動切換至 Vulkan / DX12 / Metal)
*   **數學運算**：`glam` (高效能 3D 向量與矩陣運算庫)
*   **地圖生成**：`noise` (用於 Perlin / Simplex 噪聲產生高度圖)
*   **材質加載**：`image` (用於加載 Texture Atlas PNG)

---

## 2. 當前進度狀態
*   **階段**：實作計劃完成，準備進入 Cargo 初始化與程式碼編寫階段。
*   **已完成工作**：
    *   [x] 系統開發工具檢測（確定 Rust 已安裝，缺少 C++ 編譯器，因此選擇 Rust）。
    *   [x] 方案討論與確立（採用 **方案 A：Rust + wgpu + winit**）。
    *   [x] 系統架構設計與模組拆分討論。
    *   [x] 撰寫正式設計規格書並提交至 git [2026-07-18-minecraft-design.md](file:///f:/Desktop/MC/docs/superpowers/specs/2026-07-18-minecraft-design.md)。
    *   [x] 撰寫詳細實作計劃並提交至 git [2026-07-18-minecraft-implementation.md](file:///f:/Desktop/MC/docs/superpowers/plans/2026-07-18-minecraft-implementation.md)。
    *   [x] 初始化 Git 儲存庫並完成初次提交。
    *   [x] 建立 `progress.md` 進度追蹤文件。

---

## 3. 開發路線圖與待辦清單 (TODO)

### 3.1. 實作計劃與初始化
*   [x] 撰寫詳細實作計劃 `implementation_plan.md`
*   [x] 建立 `cargo` 專案結構與配置 `Cargo.toml` 依賴

### 3.2. 第一階段：視窗與 wgpu 渲染管線
*   [x] 初始化 `winit` 視窗與基本的遊戲主循環 (Game Loop)
*   [x] 初始化 `wgpu` 設備 (Device)、佇列 (Queue) 與著色器 (WGSL)
*   [x] 實作基本的 3D 相機 (Camera) 與矩陣變換上傳 (Uniform Buffer)
*   [x] 載入方塊紋理圖集 (Texture Atlas) 並設置採樣器 (Sampler)

### 3.3. 第二階段：世界數據與網格最佳化 (Mesh)
*   [x] 實作 `Chunk` 數據結構 ($16 \times 256 \times 16$) 與方塊定義
*   [x] 實作 **Face Culling** (面片剪裁) 演算法以大量精簡 GPU 頂點數
*   [x] 實作 2D/3D Noise 地形生成，產生基本的隨機山丘地貌
*   [x] 完成單個與多個 Chunk 的靜態渲染與材質映射

### 3.4. 第三階段：玩家控制與物理碰撞
*   [x] 實作第一人稱相機旋轉（鎖定滑鼠、讀取相對位移）
*   [x] 實作玩家鍵盤移動（WASD、空格跳躍）
*   [x] 實作重力與加速度公式
*   [x] 實作 **AABB 軸對齊包圍盒** 碰撞偵測與位置推回修正，防穿牆與掉落

### 3.5. 第四階段：方塊互動與射線檢測
*   [x] 實作 DDA 3D 射線檢測演算法，取得玩家眼睛指向的方塊位置
*   [x] 實作滑鼠左鍵挖掘（方塊設為 Air，觸發 Chunk 網格重新生成）
*   [x] 實作滑鼠右鍵放置（在相鄰法線方向放置方塊，觸發網格更新）
*   [ ] 實作准星 (Reticle) 或是選中方塊的外框標記

### 3.6. 驗證與最佳化
*   [x] 撰寫單元測試驗證 AABB 碰撞與 DDA 射線正確性
*   [x] 整合測試所有模組，在 Windows 本地編譯執行，確認無記憶體洩漏與渲染掉幀
*   [x] 整理開發 Walkthrough 並交付

---

## 4. 設計決策摘要
*   *2026-07-18*: 考慮到平台相容性與學習深度，決定使用 raw `wgpu` 而非進階引擎 `Bevy`，完全自訂頂點格式 `CornerVertex`、面片裁剪 (Face Culling) 邏輯與著色器以達到最高的渲染掌控力。
*   *2026-07-18*: 新建 `progress.md` 以追蹤完整研發細節，避免上下文受限時的重複設計或狀態遺失。
