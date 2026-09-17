# 14 — Surface API 簡化與每欄取樣一次

狀態：待執行。基線：`83e751d`，2026-09-17。
前置：03 建議先完成。

## 定位與判定

worldgen/surface.rs:122 block_for_column 的 _ctx／_wx／_wz 完全未用；WorldGenContext::block_at_sampled 只是代傳六個參數。BiomeSurfaceData::is_dry（:12）僅初始化。

刪 is_dry 後 Plains／Forest／BirchForest／Meadow／Savanna／Jungle 的表項相同，Desert／Badlands 相同。正式 chunk.rs:112 的每 column 已有 biome，可把 surface 表項取一次。

## 實作步驟

1. 刪 is_dry 與未使用函式參數，合併相同 biome match arms。
2. 選一個正式 block-for-surface 入口；刪沒有語意的代傳 wrapper，不新增同義抽象。
3. 在 column 外層取得 BiomeSurfaceData，每個 Y 只使用 wy／surface_y／該表項，不反覆重建同一資料。
4. 保留 bedrock、sea level、水體及不同 biome 的真實差异；hash constants／生成 seed 語意不變。
5. 若需要調整 ARCHITECTURE 的 sampled fill 描述，同包更新。

## 驗證與驗收

既有：every_biome_has_surface_data、overworld_floor_is_bedrock_at_min_y、block_for_column_handles_water、dimension::tests::fixed_seed_column_fingerprints_are_stable。

比較修改前後相同 seed、正負座標的正式 chunk 輸出；states 若新增比較須記錄為新增覆蓋。函式簽名不再含三個 unused 參數，surface 表項只在每 column 求值一次。

## 實作紀錄

尚未執行；完成時記錄實際修改、驗證結果、刪碼量及文件更新。

