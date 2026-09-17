# 03 — worldgen 舊密度／洞穴算法與無用狀態

狀態：待執行。基線：`83e751d`，2026-09-17。
前置：無。

## 定位與判定

`src/worldgen/density.rs:86/103` 的 density_at／is_cave 只由自身測試使用；正式 `src/world/chunk.rs:132/133` 使用 CaveCarver。四個舊 Perlin 欄位仍於 constructor 建構。

`WorldGenContext::block_at` 唯一 caller 是名字為 chunk_generation_is_byte_identical_across_threads 的測試；該測試實際只比較 y=64 的 16×16 Option<BlockType>，沒有完整生成 chunk。

## 實作步驟

1. 刪 density_at／is_cave 及只驗證舊算法的 density_is_deterministic／cave_detection_is_bounded；保留 surface_height_within_world_bounds。
2. 刪 continent_noise／cavern_noise／tunnel_noise／ravine_noise 及初始化，留下 detail／ridge／river。
3. 刪 density::surface_height 自由函式、WorldGenContext::seed／supports_dimension、salted_seed／RIVER_BED_DEPTH／BEACH_BAND。
4. 將上述名字誇大的測試改走真正 chunk 生成、跨執行緒比較，再刪 block_at wrapper；14 負責正式 surface API 收斂。
5. NETHER_COLUMN_HEIGHT 並非無引用：dimension.rs:941 的有效測試改用 Dimension::Nether.height().max_y_exclusive()-1，再刪常數；保留 nether_has_roof_lava_features_and_no_sky_light。

## 驗證與驗收

`cargo test --lib worldgen::`、`cargo test --lib world::chunk::tests`、`cargo test --lib dimension::tests::fixed_seed_column_fingerprints_are_stable`。

現有 golden fingerprints 涵蓋 Overworld／Nether／End／Superflat 和正負座標，但只 hash blocks／heightmap；新增 states 比對才可聲稱覆蓋 states。正式生成值維持一致，少建四個不用的 Perlin；不承諾未量測的速度百分比。

## 實作紀錄

尚未執行；完成時記錄實際修改、驗證結果、刪碼量及文件更新。

