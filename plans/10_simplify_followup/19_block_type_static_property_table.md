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

- [ ] `static BLOCK_TABLE: [BlockDef; N]` 以 discriminant 索引（`BlockType` 已 `#[repr(u8)]`）；`BlockDef` 含 name／hardness／render_type／is_solid／is_passable／light_emission／face tex／sound／preferred tool／min harvest／cross-model。
- [ ] `properties()`／`get_face_tex_index()`／`sound_material()`／`preferred_tool()`／`min_harvest_material()`／`is_cross_model()` 全部變成 `&BLOCK_TABLE[self as usize]` 欄位讀取（回 `&'static`，不再建 struct）。
- [ ] 行為式方法（`can_stay_on`、`support_status_at`、`is_passable` 對門的特例——Plan 20 之後消失）保留為程式碼。
- [ ] 表長度與 enum 變體數以 `const` assert 鎖定；每個變體有且只有一列（測試 `canonical_block_table_covers_every_variant`）。
- [ ] 所有數值 **byte-identical**：以現有 `properties()` 對每個變體 snapshot 出的 JSON／debug 字串測試鎖住後再替換。
- [ ] wire／save 值不變。
- [ ] `mining.rs` 140、mesh／lighting hot loop 改讀 `&'static` 表。

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
