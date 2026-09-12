# 09 — Overworld 地形、生態群系與自然方塊模擬

## 執行包

- 優先級：P1
- 前置條件：05、08 完成
- 後續解鎖：10、11、12、13
- 建議提交上限：3
- 禁止順帶實作：村莊、地下城、完整 1.21.5 biome 清單

## 目標

用連續氣候與三維密度取代目前 7 類、二維高度為主的地形，使探索至少具備山谷、河流、
海岸、深淺海、地下洞穴／含水層、積雪線和決定性自然方塊行為。

## 基礎內容目標

不是追求原版全部 biome，而是交付可辨識且連續的代表集合：Plains、Forest、Birch
Forest、Taiga、Snowy Plains、Desert、Savanna、Swamp、Jungle、Badlands、Meadow、
Windswept Hills、River、Beach、Ocean、Deep Ocean，加現有 Nether/End 的獨立處理。

## 實作步驟

### A. 決定性噪聲場

- [x] 新增 climate sample：temperature、humidity、continentalness、erosion、weirdness。
- [x] 地形 density 由低頻大陸、侵蝕、山脊和洞穴組合，不以單一 biome base/scale 跳變。
- [x] 所有 noise seed 從 world seed + 明確 salt 派生；不能依線程或生成順序。
- [x] 建 seam tests：相鄰 Chunk 邊界高度、洞穴和水面連續。

### B. 地表與水系

- [x] biome selection 使用 climate lookup；海岸／河流作 overlay，而不是孤立方格。
- [x] 海平面、river bed、beach、deep ocean 分層；水體不生成懸空牆。
- [x] 每個 biome 資料化 top/filler/underwater blocks、植被、溫度與降水。
- [x] weather 的雨／雪查詢改讀同一 climate/biome source。

### C. 洞穴、含水層與礦物

- [x] 分離 cheese/cavern、tunnel 和 ravine 類洞穴，保留地表入口但避免過度破壞海床。
- [x] 簡化 aquifer：同一局部水位連續填水，深層 lava 有明確閾值。
- [x] Ore distribution 按 Y 範圍、暴露修正、vein size 資料化；Diamond/Redstone 適配負 Y。
- [x] 生成測試用統計範圍，不對單一 seed/Chunk 寫死精確數量。

### D. 自然方塊 tick

- [x] 接入 05 random tick：grass spread/decay、leaf decay、sapling growth、cactus/sugar cane growth。
- [x] falling sand/gravel 使用 FallingBlock entity 或有界 scheduled move；不能瞬間遞迴整柱。
- [x] fire 支援燃燒時間、有限蔓延、雨熄滅與 Nether 行為；現有 lightning/TNT 入口共用。
- [x] snow/ice formation/melt 使用 biome temperature、light 和 weather。

### E. 串流與性能

- [x] Chunk 生成保持 worker 可執行，不接觸 GPU 或全局可變 State。
- [x] 初始 terrain、surface、carver、feature 分 stage 計時；每 stage 可取消 stale job。
- [x] 記錄固定種子 9×9 Chunk 的時間、峰值記憶體和 block distribution 基準。

## 主要文件

- 建議新增：`src/worldgen/{mod,climate,density,surface,carver,feature,ore}.rs`
- 修改：`src/world.rs`、`src/dimension.rs`、`src/weather.rs`、`src/world_tick.rs`
- 修改：`src/chunk_schedule.rs`、`src/state.rs`、`src/microbench.rs`

## 驗收

- [x] 同 seed、不同線程數、不同 Chunk 請求順序產生 byte-identical 結果。
- [x] 至少 16 個目標 biome 在種子掃描中可達，轉場不出現垂直斷牆。
- [x] 河流能連續跨越多 Chunk，海岸不大面積漏水。
- [x] Ore Y 分布與設計表一致，Bedrock/void 邊界安全。
- [x] tree/plant/fire/falling block 跨 Chunk 邊界遵守 budget 和 authority。
- [x] release benchmark 不比遷移前固定場景慢超過預設門檻；超出須有數據和批准。

## 完成閘門

10 應只消費 worldgen stage API 放置結構，不能再次把固定世界座標的特殊房間寫回
`dimension.rs`。
