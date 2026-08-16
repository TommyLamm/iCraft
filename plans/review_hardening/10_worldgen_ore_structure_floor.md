# Plan10 — 礦脈、結構 cache、基岩與樹冠

## 定位

- `place_ores`（`src/worldgen/ore.rs` 約 75）：`blocks` 形狀是 `[x][local_y][z]`，
  `blocks.len()` 是 16（寬）不是 384（高）。`ly = wy - min_y_offset` 又把符號用反。
  結果：只有世界 Y∈[64,80) 過關，卻寫到 local 0..16（世界 Y -64..-49）；
  鑽石／金／紅石基本上不生成。`_chunk_z` 沒進 hash。測試只比 config 符號與 bit 恆等。
- `StructureManager` 放在 process-global `OnceLock`，key 只有 `(Dimension, region_x, region_z)`，
  **沒有 world seed**。同一行程第二個世界會重放第一個世界的村莊／要塞。
- 預設 Overworld 基岩在 Y=-60，Y=-64..-61 是石頭；superflat 才是 Y=-64。
- `feature.rs` 鄰 chunk 樹迴圈：origin 在 3×3 取樣後，樹幹不在本 chunk 就 `continue`，
  跨界樹葉從不寫入 → 16 格樹縫。註解寫的是相反行為。
- 村莊／要塞 Y 是常數，與 `/locate` 的地牢 Y 公式不一致。

## 前置

無。不要在本計劃改光照或實體碰撞（09）。

## 精確 acceptance

- [ ] `place_ores` 用 `local_y = wy + min_y_offset`（或與 `Chunk::new_with_seed` 同一轉換），
  邊界檢查 `ly >= blocks[lx].len()`。鑽石出現在設定的世界 Y（<16），煤在海平面附近，
  且位置是世界 Y 不是 -64 附近的錯位。hash 含 `chunk_z`。
- [ ] 結構 cache key 含 `seed`（或每個 `ServerWorld`／`WorldGenContext` 自有 manager，
  去掉跨世界 `OnceLock`）。兩個不同 seed 的世界在同一行程不得共用 region 結果。
- [ ] 預設 Overworld `get_block_local(x, min_y, z) == Bedrock`。不得在基岩下留可挖石頭。
- [ ] 樹：鄰 chunk 樹幹產生的落在本 chunk 的葉子必須寫入。測試：樹在 local X=15，
  鄰 chunk X=0 看得到葉子。
- [ ] `/locate` 與實際 placement 共用同一個 `origin_y_for(id, seed, chunk)`
  （至少地牢；村莊改採 `surface_height` 並 clamp 到 `Dimension::height()`）。

## 預計檔案與測試

- 修改：`src/worldgen/ore.rs`、`src/worldgen/feature.rs`、`src/worldgen/surface.rs`、
  `src/dimension.rs`、`src/structure/manager.rs`、`src/structure/locate.rs`、
  `src/structure/placement.rs`。
- 測試：礦脈「石質 384 高 column 在設定 Y 真的有礦」；結構 cache 兩 seed；
  預設 chunk Y=min_y 基岩；兩 chunk 樹冠。

## 建議階段

1. 修 `place_ores` + 寫會失敗的高度測試（現況兩份空結果也會過）。
2. 拆 `OnceLock` 或把 seed 加進 key。
3. 基岩改 `height.min_y()`。
4. 樹冠跨界。
5. locate／place 共用 origin Y。

## 不在本計劃

- Superflat 仍先跑完整 density（可選清理，不要綁 acceptance）。
- 興趣驅動 evict（11）。
- 結構密度對齊完整 vanilla spacing。
