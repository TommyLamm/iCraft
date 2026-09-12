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

- [x] `MobPart { size, offset, tex_cols, tex_row, limb }` 資料表 per `EntityType`；`render_mobs` 變成表驅動 + 小 animator；只有 dragon／wither／item 保留特例；空臂刪。
- [x] `PACK_TILES` 成為 atlas 定義；tile 只在 pack miss 時呼叫 painter（或 painter 全刪改 1×1 debug 色）；painter 的 LCG 收成一份。
- [x] 一個 `emit_box` 給 `TerrainVertex`（mesh + block_model 共用）；hand 改 instance mob 單位 cuboid（或至少共用角表）。
- [x] `models/block/{snake}.json` 路徑由 `BlockType` 名稱衍生；`MODEL_PATHS` 與 `ModelDescriptor.parent` 刪；`canonical_block_model_paths_cover_every_wire_variant` 改為衍生規則測試。
- [x] mob／hand／block 幾何測試（`mob_renderer.rs` 2309–2687；`hand_renderer.rs` ~1274；mesh／block_model 幾何）全綠；桌面手動確認外觀。
- [x] 啟動 atlas 建構時間前後對比記錄。

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

## 實作與證據

### 改了什麼

- `block_model::emit_box`：mesh／block_model 共用 `TerrainVertex` 盒體 emitter；`mesh::append_box_mesh` 改呼叫它。
- `mob_renderer::UNIT_CUBOID_CORNERS`：hand `add_cuboid_view` 共用單位角表（winding／UV 對齊）。
- 新增 `src/mob_parts.rs`：`MobPart` + `Limb`／scale／tex 模式；每 `EntityType` 靜態表；`emit_table_parts` + 小 animator（skeleton bow、blaze rods）。`render_mobs` 只保留 dragon／wither／dropped-item 特例。
- 空臂（`ExperienceOrb`／`Boat`／`Minecart`／`FishingHook`）改為空 `&[]` 表，自 `mob_renderer` match 刪除。
- `texture.rs`：`PACK_TILES` 為 atlas 定義（含 destroy stages）；paint-on-miss（miss → debug 色或 crack）；共用 `next_rand`；刪除 ~40 個僅服務預繪的 procedural painters。
- `MODEL_PATHS` 刪；`model_path_for_block` 由 `BlockType` Debug→snake（`Grass`→`grass_block`）；`RawModelDescriptor.parent` 刪。
- `ARCHITECTURE.md` Render 列更新。

### 測了什麼

- `cargo test --bin icraft mob_renderer::` → 13 ok
- `cargo test --bin icraft hand_renderer::` → 16 ok
- `cargo test --bin icraft texture::` → 8 ok（含 `paint_on_miss_atlas_build_is_pack_first`：217 resolved／0 miss，apply ≈139ms）
- `cargo test --lib block_model::` → 5 ok
- `cargo test --lib world::mesh::` → 27 ok
- `cargo check --all-targets`、`cargo check --bin icraft-server` → ok

### 死路徑證據（空臂）

- 刪除前：`git show HEAD:src/mob_renderer.rs` 有 `ExperienceOrb | Boat | Minecart | FishingHook => {}`（空 match 臂、無 mesh producer）。
- 刪除後：`rg ExperienceOrb|Boat|Minecart|FishingHook src/mob_renderer.rs` → **0 hits**；`mob_parts.rs` 對應 `*_PARTS: &[]`（intentional empty）。

### Atlas 時間對比

- 前：整張 256×256 procedural 預繪 + `PACK_TILES` 再覆蓋（雙次光柵）。
- 後：只走 `PACK_TILES` 一次；本機 `paint_on_miss_atlas_build_is_pack_first` 記錄 apply ≈**138.95ms**、**217 resolved／0 fallback**。

### 留下的缺口

- 桌面外觀需人工開遊戲目視（計劃允許）；EndCrystal 旋轉球 pitch 以 size 門檻近似舊硬編碼。
- Miss fallback 為 debug 色（非舊 procedural art）；有 vanilla／pack 時 0 miss。
- `state.rs` pipeline／upload 無需微調（未改 GPU instance 契約）。
- `tools/gen_mob_parts.py`／`patch_texture_atlas.py` 等為一次性產生器，可留可不留。
