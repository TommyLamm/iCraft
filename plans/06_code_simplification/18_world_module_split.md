# Plan18 — `world.rs` 機械拆檔

## 定位

`src/world.rs` 約 7,000 行，四個不相關產品擠在一個 crate 檔：

| 約略行 | 內容 | 應去處 |
| --- | --- | --- |
| 1–16 | `FLUID_*`、`CHUNK_*` | `world/` 根或 `fluid_bits` |
| 19–248 | `Biome` + 過期 `Biome::get_biome` shim | 可留 `block`／`biome`；活選擇已在 `worldgen/climate.rs` |
| 250–1667 | `BlockType`、`BlockState`、`properties()` | `world/block.rs` |
| 1669–2510、4625–5270 | atlas UV、greedy／特殊 mesh | `chunk_render` 或 `world/mesh.rs` |
| 2512–3483 | `SectionKey`、palette、`ChunkSection` | `world/section.rs` |
| 3486–4620 | `Chunk` 欄 API + `new_with_seed` | `world/chunk.rs`；gen 本體可繼續呼叫 `worldgen` |
| 5396+ | 測試（含 mesh） | 跟著被測型別走 |

`CHUNK_HEIGHT = 256` 不是 Overworld 高度。活欄位用 `Dimension::height()`（07 已掃 signed-Y）。
本計劃是 **move + `pub use`**，不改 wire discriminant、不改 greedy 合併、不改 `TerrainVertex` layout。

## 前置

07 已完成（signed-Y helper、`tick_*_in_columns`）。可與 16、17 並行。
不要跟 15 同時大改 `state.rs` 對 `crate::world::*` 的 import（15 只搬 leftover；若衝突以 re-export 消化）。

**建議** 14 已合併：desktop 已 `use icraft::world`，拆檔後 `pub use` 一次就能餵兩邊。未合併時 `main.rs` 與 `lib.rs` 都要能看到新子模組。

## 精確 acceptance

- [ ] `src/world.rs` 變成模組根（或改名 `src/world/mod.rs`），並 `pub use` 舊路徑：
      `BlockType`、`BlockState`、`BlockProperties`、`Biome`、`Chunk`、`ChunkSection`、
      `SectionKey`、`SectionIdentity`、`MeshVoxel`、`FLUID_*`、`CHUNK_WIDTH`／`HEIGHT`／`DEPTH`、
      `find_safe_spawn_position`。現有 `use crate::world::BlockType` **零改**（測試與生產）。
- [ ] 至少拆出（名稱可微調，責任不可混）：
      - `block`：`BlockType`／`BlockState`／`properties`／support／harvest
      - `section`：palette、`LightStorage`、`ChunkSection`、`SectionKey`
      - `chunk`：column API、heightmap、BE map、`empty_in_dimension`
      - mesh：`generate_mesh*`、AO、火把／門／仙人掌等特殊 mesh，搬到 `chunk_render` 或 `world/mesh.rs`
- [ ] `Chunk::new_with_seed` 仍是公開入口，內部繼續呼叫 `worldgen`。不得改噪音鹽、carve、樹跨欄。
- [ ] 不得改 `BlockType`／`Biome` 的 enum 順序或 `to_wire`／`from_wire`。
- [ ] 不得改 `TerrainVertex` stride、greedy merge 規則、AO 公式。mesh 單元測期望值不變。
- [ ] 不得刪 `Biome::get_biome` shim（只服務 `world` 測試）。不得「修好」`CHUNK_HEIGHT = 256` 或農田 `7u8`。
- [ ] 不得把 `Item` 與 `BlockType` 合成一個 enum。
- [ ] `cargo test --lib world::` 與依賴 `BlockType`／`Chunk` 的權威測通過。

## 預計檔案與測試

- 新增：`src/world/mod.rs`（或保留 `world.rs` 當根 + `#[path]`；二選一，證據寫明）、
  `src/world/block.rs`、`src/world/section.rs`、`src/world/chunk.rs`、mesh 目的地。
- 修改：`src/lib.rs`／`src/main.rs` 只在模組路徑改變時改 `mod world`；呼叫端靠 `pub use`。
- 測試：
  - `cargo test --lib world::`
  - `cargo test --lib chunk_render::`
  - `cargo test --lib block_model::`
  - `cargo test --test review_hardening_invariants -- --test-threads=1`
  - `cargo test --test waterlogging_authority -- --test-threads=1`
  - `cargo test --test review_hardening_chunk_restore -- --test-threads=1`
  - `cargo check --all-targets`

## 建議階段

1. 先把 `block` 型別移出，根檔 `pub use`。`cargo test --lib world::`。
2. 再搬 `section`／`chunk` 儲存。
3. 最後搬 `generate_mesh*`。每步確認 vertex 測。
4. 跑 restore／waterlogging（存檔與 fluid byte 契約）。

## 不在本計劃

- 把 `Chunk::new_with_seed` 整段搬進 `worldgen/`（可列後續；本計劃只拆檔）。
- 刪 `Biome::get_biome`、改 Nether／End 的 256 陣列。
- 拆 `inventory.rs` 目錄（README §7）。
- 合併兩個 `ray_intersects_aabb`、改實體碰撞。
- 開 workspace `icraft-world` crate。
