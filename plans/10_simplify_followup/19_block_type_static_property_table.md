# Plan19 — `BlockType` 靜態屬性表

## 定位

`src/world/block.rs` `impl BlockType` 用六個平行 `match` 描述同一組 121 個變體：

| 方法 | 行 | 規模 |
| :--- | :--- | :--- |
| `properties()` | 726–1571 | ~845 行；每臂重複 `name`／`hardness`／`render_type`／`is_solid`／`is_passable`／`light_emission` |
| `get_face_tex_index()` | 1580–1780 | ~200 行 atlas 座標 |
| `sound_material()` | 682–724 | — |
| `preferred_tool()` | 1782–1826 | — |
| `min_harvest_material()` | 1829–1853 | — |
| `is_cross_model()` | 477–487 | — |

熱 caller 每 voxel 重建整個 `BlockProperties` struct：`lighting.rs` 58、194（`neighbor_block.properties().render_type`）；`chunk.rs` 169、173；`mesh.rs` `is_greedy_cube` 655–656。121 臂 match 在 meshing／lighting／生成的每個 voxel 上跑。

## 前置

無。Plan 20 依賴本計劃。

## 精確 acceptance

- [x] `static BLOCK_TABLE: [BlockDef; N]` 以 discriminant 索引（`BlockType` 已 `#[repr(u8)]`）；`BlockDef` 含 name／hardness／render_type／is_solid／is_passable／light_emission／face tex／sound／preferred tool／min harvest／cross-model。
- [x] `properties()`／`get_face_tex_index()`／`sound_material()`／`preferred_tool()`／`min_harvest_material()`／`is_cross_model()` 全部變成 `&BLOCK_TABLE[self as usize]` 欄位讀取（回 `&'static`，不再建 struct）。
- [x] 行為式方法（`can_stay_on`、`support_status_at`、`is_passable` 對門的特例——Plan 20 之後消失）保留為程式碼。
- [x] 表長度與 enum 變體數以 `const` assert 鎖定；每個變體有且只有一列（測試 `canonical_block_table_covers_every_variant`）。
- [x] 所有數值 **byte-identical**：以現有 `properties()` 對每個變體 snapshot 出的 JSON／debug 字串測試鎖住後再替換。
- [x] wire／save 值不變。
- [x] `mining.rs` 140、mesh／lighting hot loop 改讀 `&'static` 表。

## 預計檔案與測試

- 改：`src/world/block.rs`（可能新檔 `src/world/block_table.rs`）、`src/lighting.rs`、`src/world/{mesh,chunk}.rs`、`src/authority/mining.rs`
- 驗證：`cargo test --lib world::block:: world::mesh:: lighting:: authority::mining::`；屬性 snapshot 對比測試；worldgen byte-identity 測試

## 建議階段

1. 先寫 snapshot 測試（每變體所有屬性 dump）。
2. 建表，`properties()` 改查表，跑 snapshot。
3. 其餘五個方法改查表。
4. hot caller 改讀 `&'static`。

## 不在本計劃

- 折 powered／open 變體（Plan 20）。
- `block.rs` 切檔（Plan 27）。

## 實作與證據

### 改了什麼

- 新增 `src/world/block_table.rs`：`BlockDef` + `BLOCK_TABLE: [BlockDef; 121]`（`BLOCK_TYPE_COUNT`），以 `BlockType as usize` 索引；`const` assert 鎖表長。
- `BlockType::{def,properties,get_face_tex_index,sound_material,preferred_tool,min_harvest_material,is_cross_model}` 改為讀靜態表（`properties()` 回 `&'static BlockProperties`）。
- `can_stay_on`／`support_status_at` 仍為行為式 match；門／活板門的 solid／passable 差已落成表中獨立列（不再 runtime `self ==`）。
- 熱路徑改讀 `block.def()`：`lighting.rs`、`world/mesh.rs`、`world/chunk.rs`、`world/section.rs`、`authority/mining.rs`。
- 鎖定 golden：`src/world/block_property_snapshot.txt` + 測試 `block_static_property_snapshot_is_byte_identical`／`canonical_block_table_covers_every_variant`。
- `ARCHITECTURE.md` 記錄 `BLOCK_TABLE` 契約；README #19 → 已完成。

### 測了什麼

- `cargo test --lib world::block::` → 10 passed（含 snapshot／coverage）
- `cargo test --lib world::mesh::` → 27 passed
- `cargo test --lib lighting::` → 9 passed
- `cargo test --lib authority::mining::` → 4 passed
- `cargo check --all-targets` → ok
- `cargo check --bin icraft-server` → ok

### 留下的缺口

- Plan 20：powered／open 變體尚未折進 `BlockState`。
- `Dirt` 等地塊的 footstep 聲仍走原 match 的 `_ => Stone`（byte-identical 保留，非本計劃改語意）。
- 非熱路徑 caller 仍可走薄 `properties()` accessor；未全面改寫。
