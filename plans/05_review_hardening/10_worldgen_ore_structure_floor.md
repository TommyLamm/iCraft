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

- [x] `place_ores` 用 `local_y = wy + min_y_offset`（或與 `Chunk::new_with_seed` 同一轉換），
  邊界檢查 `ly >= blocks[lx].len()`。鑽石出現在設定的世界 Y（<16），煤在海平面附近，
  且位置是世界 Y 不是 -64 附近的錯位。hash 含 `chunk_z`。
- [x] 結構 cache key 含 `seed`（或每個 `ServerWorld`／`WorldGenContext` 自有 manager，
  去掉跨世界 `OnceLock`）。兩個不同 seed 的世界在同一行程不得共用 region 結果。
- [x] 預設 Overworld `get_block_local(x, min_y, z) == Bedrock`。不得在基岩下留可挖石頭。
- [x] 樹：鄰 chunk 樹幹產生的落在本 chunk 的葉子必須寫入。測試：樹在 local X=15，
  鄰 chunk X=0 看得到葉子。
- [x] `/locate` 與實際 placement 共用同一個 `origin_y_for(id, seed, chunk)`
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

## 實作與證據

### 行為

- `place_ores`：`local_y = wy + min_y_offset`（與 `Chunk::new_with_seed` 的 `wy - min_y` 同一轉換），邊界改查 `blocks[lx].len()`。vein 鄰居同樣用高度而不是 `blocks.len()`（寬 16）。hash 納入 `chunk_z`。
- `StructureManager` cache key 改為 `(seed, Dimension, region_x, region_z)`。process-global `OnceLock` 仍在，但兩 seed 不再共用 region starts。
- 預設 Overworld 基岩改 `Dimension::Overworld.height().min_y()`（-64）。`wy <= min_y` 一律 Bedrock，基岩下不再留石頭。Superflat 原本就是 Y=-64，未改。
- `place_trees` 不再在樹幹落在鄰 chunk 時 `continue`。`place_oak` 等改吃 signed local XZ，只寫入落在本 column 的葉子。
- 新增 `origin_y_for(id, seed, chunk_x, chunk_z)`，locate 與 `StructureManager` 共用。地牢維持 `20 + (seed + chunk_x) % 30`；村莊改 `surface_height` 並 clamp 到 Overworld `height()`。

### 測試

- `cargo test --lib worldgen::`：24 passed（含 `ores_place_at_configured_world_y`、`ore_hash_includes_chunk_z`、`neighbor_trunk_at_x15_writes_leaves_into_chunk_x0`、`overworld_floor_is_bedrock_at_min_y`）。`ores_place_at_configured_world_y` / `ore_hash_includes_chunk_z` 修前失敗、修後通過。
- `cargo test --lib structure::`：7 passed（含 `structure_cache_does_not_share_across_seeds`、`locate_dungeon_y_matches_placement`、`village_origin_y_uses_surface_height`）。
- `cargo test --lib dimension::`：11 passed（含 `default_overworld_floor_is_bedrock_at_min_y`、`superflat_generation_is_signed_height_and_deterministic`、`fresh_overworld_spawn_region_contains_trees_and_ground_flora`）。
- `cargo fmt`：通過。
- `cargo check --all-targets`：通過（C: 空間不足，改用 `CARGO_TARGET_DIR=F:\tmp\icraft-plan10-target`）。
