# Plan17 — lighting BFS 去重、鄰域快取、載入只 seed 邊界

## 定位

Wave 08 讓載入光照的 **seed** 用 `column_neighborhood`（`lighting.rs` 585；`chunk_manager.rs` 290–296 註解「hot loop never does a per-voxel HashMap get」）。BFS 本體沒有跟上：

- `propagate_sky_light`（27–96）、`remove_sky_light`（98–161）、`propagate_block_light`（163+）、`remove_block_light` 四份幾乎相同的 walk，每個鄰居都 `chunk_manager.get_block` + `get_sky_light`／`get_block_light`（各一次 `HashMap<(i32,i32),Chunk>` 查找，`get_loaded_block` 341–344）。
- 四份各自內聯 `LIGHT_DIRS`（本檔 5–12 已定義，卻在 32、104、168、238、388、490 重抄）。
- 同一段 15 行「標記本欄 + 4 邊鄰居 dirty」複製四次（68–86、128–144、206–219、264–277）。
- Nether 生成有第五份 BFS（`dimension.rs` 447–505；Plan 16 刪）。

載入路徑：`TerrainWorkerResult::Loaded` 對中心 + 4 鄰欄呼叫 `propagate_chunk_lighting`（`state.rs` 5791–5811）；`lighting.rs` 572–648 對每個 section／x／z／ly 都掃兩個鄰居探針，Overworld ≈ 98k × 5 欄 ≈ 0.5M cell，GPU 執行緒 1–8 ms／欄（被 `MAX_INTEGRATE_TIME_MS` 切片，變成 hitch 叢）。ARCHITECTURE 描述的「只 seed 面／發光體／較暗鄰居」在 seed 階段成立，但 volume scan 還在。

放／破方塊在 view 16 觸及數千 cell，每 cell 2 次 HashMap × 6 鄰居。

## 前置

無。可與 Plan 16 並行（16 刪 Nether BFS 時直接呼叫本計劃的入口）。

## 精確 acceptance

- [x] 一個泛型 `propagate`／`remove_light`（sky／block 以 `LightKind` 參數化），在 3×3 欄暫取鄰域上運作；每 cell 零 HashMap 查找。
- [x] `mark_light_dirty(dirty, nx, nz)` 一份；`LIGHT_DIRS` 只在一處。
- [x] 載入光照只從 seed 集合起 BFS（中心：face／emitter／lit-border；鄰欄：只 shared face），不做 4 鄰欄全 volume scan；`state.rs` 載入改為單次 `propagate_chunk_lighting`。
- [x] 光照值與現在一致：`fixed_seed_place_break_lighting_checksum_stable` 鎖 checksum `0xf9bf_c8d6_aab7_4170`；既有 lighting／Nether 測試全綠。
- [x] `lighting.rs` 測試全綠；`dimension::`（含 Nether glowstone 光）全綠。
- [x] 載入一欄光照時間記錄於「實作與證據」。

## 預計檔案與測試

- 改：`src/lighting.rs`、`src/chunk_manager.rs`（`note_light_cell_change`）、`src/state.rs`（載入單次呼叫）、`ARCHITECTURE.md`
- 驗證：`cargo test --lib lighting::`；`cargo test --lib dimension::`；`cargo test --bin icraft`；固定序列光照 checksum

## 建議階段

1. 抽 `mark_light_dirty` 與 `LIGHT_DIRS` 單一來源（純去重）。
2. 四份 BFS 合一份泛型（先保留 HashMap 查找，鎖 checksum）。
3. 換成鄰域快取。
4. 載入只 seed 邊界。

## 不在本計劃

- 生成階段光照（Plan 16）。
- dense chunk grid（Plan 26）。

## 實作與證據

### 改了什麼

- `src/lighting.rs`：`LightKind` + `propagate`／`remove_light`；`LightNeighborhood` 暫取 3×3 `Chunk`；BFS 每 cell 走鄰域陣列而非 `HashMap`；`mark_light_dirty`／`LIGHT_DIRS` 單一來源；`propagate_chunk_lighting` 中心 smart seed + 四鄰 shared-face seed，一次 BFS。
- `src/chunk_manager.rs`：`note_light_cell_change`（restore 後套用 save-dirty + mesh invalidation）。
- `src/state.rs`：欄載入／restore 路徑由 5 次 `propagate_chunk_lighting` 改為 1 次（函式內部已處理鄰面）。
- `ARCHITECTURE.md`：更新 load lighting 契約描述。

### 測了什麼

| 命令 | 結果 |
| --- | --- |
| `cargo test --lib lighting::` | 9 passed（含 checksum + face-only + timing smoke） |
| `cargo test --lib dimension::` | 12 passed（含 Nether glowstone／block light 15） |
| `cargo test --bin icraft` | 221 passed |
| `cargo check --all-targets` | ok |
| `cargo check --bin icraft-server` | ok |

Checksum：`fixed_seed_place_break_lighting_checksum_stable` → `0xf9bf_c8d6_aab7_4170`。

載入 timing（`load_lighting_timing_smoke`，中心+4 鄰已生成欄，單次 `propagate_chunk_lighting`）：

| 建置 | 耗時 |
| --- | --- |
| debug | ~67 ms |
| release | ~7.3 ms |

對照：舊路徑對中心+4 鄰各做一次全 volume seed（≈5× Overworld 98k cell 探針），GPU 執行緒常見 1–8 ms／欄 hitch 叢。新路徑一次呼叫、鄰欄只掃 shared face；release 單次 ~7 ms 覆蓋五欄邊界（不再乘 5 次 volume）。

### 留下的缺口

- 中心欄 seed 仍會掃 lit cell（`light > 1` 或 face）；未改成純「只掃發光體+面」——水平洞穴入口仍需 lit-border seed（既有 cave 測試依賴）。
- place／break 的 remove→propagate 仍各 take／restore 一次鄰域（正確但可再合併）。
- Plan 26 dense grid 未做。
