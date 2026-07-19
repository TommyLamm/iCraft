# 任務 15：樹木 + 生態群系

> **複雜度**: ⭐⭐⭐⭐  
> **涉及面**: 世界生成、噪聲算法、邊界鄰居投影、流動植物、模擬更新、碰撞傷害  
> **前置條件**: P0 任務 1 (多 Chunk) + 任務 2 (方塊類型)
> 
> **設計規格書**: [trees-biomes-design-spec.md](file:///f:/Desktop/MC/docs/superpowers/specs/2026-07-19-trees-biomes-design-spec.md)
> **實作計畫**: [trees-biomes.md](file:///f:/Desktop/MC/docs/superpowers/plans/2026-07-19-trees-biomes.md)

---

## 15.1 生態群系系統與地形生成

### 新增/修改文件
- **[MODIFY]** `src/world.rs` — 實現 `Biome` 分配、高度插值平滑地形生成、橡/樺/杉樹結構生成及植物生成
- **[MODIFY]** `src/inventory.rs` — 擴充 `Item` 及 properties/from_block 支持 Birch/Spruce log/planks/leaves、Tall Grass、Flowers、Cactus、Sugar Cane、Pumpkin、Melon
- **[MODIFY]** `src/crafting.rs` — 增加樺木/雲杉木板、木棍、合成箱及箱子的合成配方
- **[MODIFY]** `src/texture.rs` — 繪製 Row 12 所有新方塊之 16x16 程序化紋理
- **[MODIFY]** `src/state.rs` — 串接 `State::update` 中的隨機 Tick 樹葉衰敗與仙人掌接觸傷害，並更新 `break_block` 的掉落邏輯

### 生態群系分佈設計
- **Plains (平原)**: 平坦草地（高度 ~65，起伏 4.0），稀疏橡木，隨機 Tall Grass 及黃/紅花。
- **Forest (森林)**: 起伏丘陵（高度 ~66，起伏 6.0），密集橡木與白樺木。
- **Desert (沙漠)**: 起伏沙地（高度 ~65，起伏 5.0），表層為沙子，下層為砂岩，生成仙人掌，無地表水源。
- **Taiga (針葉林)**: 寒冷林地（高度 ~68，起伏 8.0），地表覆雪，生成錐形雲杉木。
- **Swamp (沼澤)**: 低窪濕地（高度 ~62，起伏 1.5），高地表水比例，生長橡木與甘蔗。
- **Mountains (山地)**: 險峻山脈（高度 ~82，起伏 22.0），高海拔露石/雪頂，極少雲杉。
- **Ocean (海洋)**: 深海（高度 ~50，起伏 6.0），地表低於 62 處全為水，海床為沙子或碎石。

---

## 15.2 樹木生成與邊界投影
- **鄰居投影 (Neighbor Projection)**: 解決跨 Chunk 樹木截斷問題。生成目前 Chunk 時，遍歷周圍 3x3 鄰域，根據鄰居的 Chunk 座標 deterministic 播種隨機數，計算該鄰居生成的每棵樹，並將落入目前 Chunk 範圍內的 log 和 leaves 寫入目前 Chunk 區塊。
- **Oak Tree (橡木)**: 4~6 格原木，頂部 4 層球狀 canopy 葉冠（橡木樹葉）。
- **Birch Tree (白樺)**: 5~7 格白樺原木（白皮黑紋），頂部 4 層窄圓柱葉冠。
- **Spruce Tree (雲杉)**: 6~10 格深褐雲杉原木，頂部呈錐形/塔狀漸細的針葉樹冠。

---

## 15.3 裝飾與模擬更新
- **Tall Grass (高草) & Flowers (黃蒲公英 / 紅罌粟)**: 渲染為 Cutout 穿透方塊，無物理碰撞。
- **Cactus (仙人掌)**: 生成於沙漠沙地，1~3 格高。玩家 AABB 與其相交時，每 0.5 秒扣除 0.5 顆心（1.0 點傷害）。
- **Sugar Cane (甘蔗)**: 生成於臨水的水邊沙地或草地，2~4 格高，Cutout 穿透。
- **Pumpkin / Melon (南瓜/西瓜)**: 罕見隨機放置的 solid opaque 方塊。
- **樹葉衰敗 (Leaf Decay)**: 每幀在已載入 Chunk 中隨機抽選 30 個方塊。若為葉子方塊，則以其為起點進行最大距離 4 的 BFS 搜尋（僅穿越葉子）。若搜尋範圍內無任何 Log 原木，則將葉子替換為 Air 並增量更新光照與標記重繪。

---

## 驗證

- [ ] 運行 `cargo check` 可順利編譯通過
- [ ] 運行 `cargo test` 可通過所有單元測試（包含生態群系分佈及樹木放置邊界測試）
- [ ] 運行 `cargo run` 進入遊戲，地形呈現 Plains、Forest、Desert、Taiga、Swamp、Mountains、Ocean 等生態且過渡平滑自然
- [ ] 地圖上自然分佈並生成橡木、白樺木、雲杉木，形狀各異且跨邊界無生硬截斷
- [ ] 沙漠中生成仙人掌，碰撞時玩家閃紅受傷；水邊生成甘蔗；草地散落高草、黃花與紅花
- [ ] 砍伐樹幹原木後，周圍懸空的樹葉在數秒內藉由隨機 Tick 自動衰敗消失
- [ ] 破壞 Tall Grass 有 12.5% 機率掉落 `Seeds`，破壞樺木/雲杉葉有機率掉落 `Apple`
