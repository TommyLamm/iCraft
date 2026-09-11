# Plan22 — 渲染資料表：mob 部件、atlas paint-on-miss、單一 cube emitter、model 路徑、shader 殘留

## 定位

### `mob_renderer.rs`：1,800 行逐型別 cuboid 腳本

`render_mobs` 從 `EntityType::Zombie`（303）到 `IronGolem`／`Ravager`（1940–2084）約 40 個型別，每個手呼叫 `add_cuboid`（202）／`add_flat_sprite`（225）寫死尺寸／UV／肢體角度。`ExperienceOrb | Boat | Minecart | FishingHook`（2110–2113）是空臂。GPU 已 instance 單位 cuboid（`build_unit_cuboid_prototype` 123；上傳 `state.rs` 3573）。

### `texture.rs`：先畫滿再被覆蓋

`new_procedural_with_manager`（~1758–2712）用 `draw_noise`／`draw_ore`／`draw_planks`／~40 個 icon painter 填每個 16×16 tile，然後 `apply_resource_pack_with_manager`（1592–1610）把 `PACK_TILES`（~1322–1558，~100 條）蓋在上面。註解 1318–1321 承認 pack + vanilla 是真正來源。每個 painter 各自複製同一個 LCG `next_rand` closure（12–21、62–70、96–104、132–140…）。啟動固定光柵一張 256×256 atlas 兩次。

### 四份 cube emitter

- `block_model.rs` `push_quad`／`append_box*`（245–315）
- `world/mesh.rs` `BLOCK_FACES` + `append_box_mesh`（72–80、534–568）
- `mob_renderer.rs` `build_unit_cuboid_prototype`（123）+ CPU instance list
- `hand_renderer.rs` `add_cuboid_view`（680–682）**自述與 `mob_renderer::add_cuboid` 相同**，加第二份 24 角表（695+）

同一組 6 面 UV／winding 數學在四檔（~250–400 行）；手在 CPU 展成 `state::Vertex`，mob 在 GPU instance 同一形狀。

### `MODEL_PATHS` 平行表與假 model 格式

`MODEL_PATHS`（`block_model.rs` 98–220）是 `BlockType::Observer as usize + 1` 條路徑，必須按 discriminant 順序（測試 1278）；`ModelDescriptor.parent` 解析後從未讀；live mesh 仍走 `append_custom_block_mesh_impl` 大 match（508–1074），registry 只提供 `atlas_tile`（74–81）。

## 前置

無（Plan 05 先刪 `new_procedural`／shader 殘留可避免衝突）。

## 精確 acceptance

- [ ] `MobPart { size, offset, tex_cols, tex_row, limb }` 資料表 per `EntityType`；`render_mobs` 變成表驅動 + 小 animator；只有 dragon／wither／item 保留特例；空臂刪。
- [ ] `PACK_TILES` 成為 atlas 定義；tile 只在 pack miss 時呼叫 painter（或 painter 全刪改 1×1 debug 色）；painter 的 LCG 收成一份。
- [ ] 一個 `emit_box` 給 `TerrainVertex`（mesh + block_model 共用）；hand 改 instance mob 單位 cuboid（或至少共用角表）。
- [ ] `models/block/{snake}.json` 路徑由 `BlockType` 名稱衍生；`MODEL_PATHS` 與 `ModelDescriptor.parent` 刪；`canonical_block_model_paths_cover_every_wire_variant` 改為衍生規則測試。
- [ ] mob／hand／block 幾何測試（`mob_renderer.rs` 2309–2687；`hand_renderer.rs` ~1274；mesh／block_model 幾何）全綠；桌面手動確認外觀。
- [ ] 啟動 atlas 建構時間前後對比記錄。

## 預計檔案與測試

- 改：`src/mob_renderer.rs`、`src/hand_renderer.rs`、`src/texture.rs`、`src/block_model.rs`、`src/world/mesh.rs`、`src/state.rs`（pipeline／upload 微調）
- 驗證：`cargo test --bin icraft`；`cargo test --lib world::mesh:: block_model::`；桌面截圖對比

## 建議階段

1. 單一 `emit_box`（mesh + block_model）。
2. hand 共用 mob cuboid。
3. mob 部件表。
4. atlas paint-on-miss。
5. `MODEL_PATHS` 衍生。

## 不在本計劃

- menu（Plan 23）。
- shader `vs_main`／crosshair 清理（Plan 05）。
- frame 上傳批次（Plan 18）。
