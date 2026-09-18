# 14 — Surface API 簡化與每欄取樣一次

狀態：已完成。基線：`83e751d`，2026-09-17。
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

- 依步驟 1 刪除 `BiomeSurfaceData` 中無人讀取的死欄位 `is_dry`。合併相同表項的 match 分支：Plains / Forest / BirchForest / Meadow / Savanna / Jungle 合併為一組，Desert / Badlands 合併為一組，其餘保持 Taiga、SnowyPlains、Swamp、WindsweptHills、River、Beach、Ocean/DeepOcean。
- 依步驟 2 刪除無語意代傳 wrapper `WorldGenContext::block_at_sampled`。以 `surface::block_for_column(wy, surface_y, &surface)` 為正式入口，簽名移除未使用的 `_ctx`、`_wx`、`_wz` 三個參數。
- 依步驟 3 在 `Chunk::new_with_seed`（`src/world/chunk.rs`）每 column 外層只取樣一次 `let surface = BiomeSurfaceData::for_biome(biome);`，每 Y 僅傳入 `wy`、`surface_y` 與 `&surface`，不再每 Y 重複求值。
- 依步驟 4 保留了 bedrock floor（Overworld min_y 處及以下為 Bedrock）、sea level、水體填補、高山雪頂（`wy > 80 && is_snowy`）與各 biome 真實差異；未修改任何 hash 常數或種子計算。
- 依步驟 5 同步更新 `ARCHITECTURE.md` 第 446 行的世界生成 sampled fill 描述為 `BiomeSurfaceData` 每 column 採樣一次與 `block_for_column` 每 Y 填充。
- 新增測試覆蓋：
  - `worldgen::surface::tests::merged_biome_surface_data_matches`：驗證合併後 biome surface 表項的一致性。
  - `worldgen::surface::tests::block_for_column_handles_snow_filler_and_stone`：驗證高地雪頂、雪地低地草皮、表層 3 格內 filler 及深層 stone 的選塊行為。
  - `dimension::tests::fixed_seed_overworld_coordinates_fingerprints_are_stable`：驗證正正、負正、正負、負負四象限座標的正式 chunk 輸出指紋一致性與彼此互異性。
- 驗證命令與結果：
  - `cargo test --lib worldgen`：31 passed, 0 failed（22 既有 + 9 通過，消除 `is_dry` 未讀取編譯警告）。
  - `cargo test dimension::tests::fixed_seed_column_fingerprints_are_stable`：1 passed, 0 failed（指紋 `6656363810664605235` 完全穩定一致）。
  - `cargo test dimension::tests::fixed_seed_overworld_coordinates_fingerprints_are_stable`：1 passed, 0 failed（正負座標指紋確定性驗證通過）。
  - `cargo check --all-targets`：exit 0，無任何新增警告或錯誤。

