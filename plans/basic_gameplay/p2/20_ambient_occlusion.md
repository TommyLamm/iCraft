# 任務 20：環境光遮蔽 (AO)

> **狀態**: 🟢 已完成
>
> **複雜度**: ⭐⭐⭐
>
> **完成日期**: 2026-07-20
>
> **涉及面**: Mesh 生成算法、頂點屬性、Shader、跨 Chunk 網格失效

---

## 實作摘要

在 `Chunk::generate_mesh()` 的 CPU 網格生成階段，為每個可見方塊面四角取樣外側的兩個側邊格與一個對角格，將 0、1、2、3 個遮擋者依序映射為 `1.0`、`0.75`、`0.5`、`0.25`。AO 透過獨立的頂點屬性傳入 WGSL，與既有天空光、方塊光及面向亮度相乘。

每個四邊形會比較兩組對角 AO 總和，自動選擇 `0–2` 或 `1–3` 對角線，避免明暗分佈不均時產生明顯的三角形割裂。跨 Chunk 方塊修改、流體、爆炸與 Chunk 串流也會正確讓對角相鄰網格失效。

## 實際修改文件

- `src/world.rs`
  - 新增 `BlockType::is_ao_occluder()`，只有 solid 且 opaque 的完整方塊遮擋 AO。
  - 將六面定義抽為共用常數，加入 AO 取樣、四級亮度映射及三角線選擇純函式。
  - opaque、cutout、translucent Chunk 表面均寫入逐頂點 AO。
- `src/state.rs`
  - 共用 `Vertex` 新增 `ao: f32` 與 shader location 3，stride 由 24 bytes 變為 28 bytes。
  - 準星與破壞裂紋使用 `ao = 1.0`。
  - Chunk 載入／卸載會讓周圍八個已載入鄰居網格失效。
  - 玩家放置／破壞、持續採礦與葉片衰減改用統一網格依賴 helper。
- `src/shader.wgsl`
  - AO 以平滑插值 varying 傳遞，既有打包光照維持 `flat`。
  - fragment shader 在受傷混色與霧效之前套用 `clamp(ao, 0.25, 1.0)`。
- `src/chunk_manager.rs`
  - 新增支援負座標、Chunk 邊與 Chunk 角的網格依賴收集 helper。
  - 新增八方向 Chunk 鄰居座標 helper。
- `src/fluid.rs`, `src/mob.rs`, `src/passive_mob.rs`
  - 流體更新、爆炸破壞及羊吃草改用統一 dirty Chunk 邏輯。
- `src/mob_renderer.rs`, `src/particles.rs`
  - 生物、掉落物與粒子使用 `ao = 1.0`，維持原有外觀。

## 關鍵決策

- [x] 每個頂點從 `p + n` 外側平面取樣 `side_u`、`side_v`、`corner` 三格。
- [x] 嚴格使用四級線性 AO，不加入「兩側同時遮擋即最暗」特例。
- [x] Air、Water、Lava、Glass、Leaves、Torch 與植被不投下實心 AO。
- [x] 未載入 Chunk 與世界高度外沿用 Air 語義，不遮擋 AO。
- [x] `ao0 + ao2 > ao1 + ao3` 時使用 `[0,1,3, 1,2,3]`，其餘使用既有 `[0,1,2, 0,2,3]`。
- [x] 非 Chunk 幾何固定使用 `ao = 1.0`。
- [x] 不增加 Chunk 常駐 AO 陣列、存檔欄位、uniform 或遊戲設定。
- [x] 保留每幀最多重建四個 Chunk 網格的既有節流策略。

## 自動驗證

- [x] AO 0／1／2／3 遮擋映射測試。
- [x] solid opaque 與非遮擋材質分類測試。
- [x] 六個面、每面四角的法線及切線取樣方向測試。
- [x] 孤立方塊與人工遮擋的實際 `generate_mesh()` 頂點 AO 測試。
- [x] 一般、翻轉、相等分佈的索引結果及六面繞序測試。
- [x] Chunk 四邊、四角、對角與負座標 dirty 集合測試。
- [x] `cargo fmt -- --check`
- [x] `cargo check --release`
- [x] `cargo test --release`：56 項單元測試及 1 項整合測試通過。

## 遊戲內視覺抽查

以下項目需要在具備圖形介面的實際遊戲執行環境確認：

- [ ] 日間露天平面無棋盤格或對角亮暗線。
- [ ] 方塊內角、牆地交界、洞穴與屋頂下方呈現柔和深度。
- [ ] 玻璃、樹葉、植被與水岸沒有不自然實心黑影。
- [ ] Chunk 邊界及角落放置／破壞方塊後無舊 AO 接縫。
- [ ] Chunk 載入／卸載後無舊 AO 或缺面。
- [ ] 準星、生物、粒子、裂紋、受傷閃爍、日夜光照與霧效無視覺回歸。
- [ ] Release 模式連續跨 Chunk 與修改方塊時無持續卡頓。

## 範圍外

- Greedy meshing
- SSAO
- 陰影貼圖
- AO 設定開關
