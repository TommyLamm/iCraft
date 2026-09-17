# 04 — 字體、手部及小型渲染死路徑

狀態：已完成。基線：`83e751d`，2026-09-17。
前置：無；16 負責音效 bytes，15 負責資源 codec。

## 定位與判定

- 字體死鏈：state.rs:6270 add_string_textured_with_source → :6215 add_char_textured_with_source → glyph_atlas.rs:126 push_glyph_quad → uv_for；menu/mod.rs:3329 draw_text_with_font_textured 也沒有正式 caller。
- glyph_atlas.rs:87 build_rgba 只有自身測試使用。正式 font 仍走 state.rs:6179 add_char_lines_with_source、menu/mod.rs:3294 draw_text_with_font。
- 手部正式入口：presentation/frame.rs:350 prepare_hand → :374 build_first_person_hand_base_mesh；:425 animation_for_hand_mesh 上傳 uniform。舊 apply_hand_animation／build_first_person_hand_mesh_into 只服務測試。
- particles.rs:286 spawn_block_debris 無 caller；texture.rs:34 draw_redstone_torch 只有測試 caller。

## 實作步驟

1. 刪 textured font 死鏈、atlas UV helper／常數／ATLAS_CHARS／char_index；保留 glyph 與 FontSource glyph_override 的有效 line-font 路徑。
2. 手部幾何測試改調 base mesh；動畫測試使用正式 animation uniform／矩陣驗證必要採樣點，再刪 CPU animation 舊包裝。
3. 保留 minecraft_tools_render_as_closed_extruded_models、tool_models_have_minecrafts_one_pixel_depth、tools_follow_the_hand_animation 的產品斷言；不要因 helper test-only 而刪掉行為覆蓋。
4. 刪無 caller 的 debris helper；將 torch 視覺測試改驗正式資源 atlas 後刪程序化舊繪製。
5. 清過期 allow(dead_code)；GPU 持有資源需根據 lifetime 判斷。get_entities_by_type 有 boss 正式 caller，只移過期抑制。
6. UI 狀態／menu 清理由 05 處理，音效 fallback 測試由 16 遷移。

## 驗證與驗收

`cargo test --bin icraft hand_renderer::`、`cargo test --bin icraft menu::`、`cargo test --bin icraft texture::`；保留 menu_text_uses_selected_font_and_builtin_fallback。

人工檢查 menu/HUD、自訂字體、空手、工具、方塊物品。舊渲染鏈消失且實際幾何／UV／動畫仍有測試。

## 實作紀錄

- 實作改動：
  1. **字體死鏈清理**：
     - 在 `src/state.rs` 刪除無 caller 的 `add_char_textured_with_source` 與 `add_string_textured_with_source`。
     - 在 `src/menu/mod.rs` 刪除無 caller 的 `draw_text_with_font_textured`。
     - 在 `src/glyph_atlas.rs` 刪除 `push_glyph_quad`, `uv_for`, `build_rgba`, `CELL_W`, `CELL_H`, `COLS`, `ROWS`, `ATLAS_W`, `ATLAS_H`, `ATLAS_CHARS`, `char_index` 及未使用的 `FontSource` import；保留核心 `glyph` 點陣資料函數並補齊其單元測試。
     - 完整保留 line-font 與 solid-glyph 有效路徑（`add_char_lines_with_source`, `add_string_lines_with_source`, `draw_text_with_font`, `glyph_override`）。
  2. **手部渲染清理**：
     - 將手部幾何測試（`hand_mesh_contains_right_arm_and_held_block`, `hand_mesh_omits_item_when_slot_is_empty`, `hand_mesh_renders_flat_item_as_sprite_quad`, `minecraft_tools_render_as_closed_extruded_models`, `tool_models_have_minecrafts_one_pixel_depth`）改調 `build_first_person_hand_base_mesh`。
     - 動畫測試 `tools_follow_the_hand_animation` 改用正式 `animation_for_hand_mesh` 與 `matrix()` 驗證必要頂點採樣點變換。
     - 完整保留 `minecraft_tools_render_as_closed_extruded_models`、`tool_models_have_minecrafts_one_pixel_depth`、`tools_follow_the_hand_animation` 的完整斷言。
     - 刪除舊 CPU animation 封裝：`apply_hand_animation`, `build_first_person_hand_mesh_into`, `build_first_person_hand_mesh`。
  3. **粒子清理**：
     - 刪除 `src/particles.rs` 中無 caller 的 `spawn_block_debris` helper；保留有 caller 的 `block_debris_uv` 與 `spawn_footstep_dust`。
  4. **貼圖清理**：
     - 刪除 `src/texture.rs` 中程序化舊繪製 `draw_redstone_torch`。
     - 更新 `redstone_torch_sprite_has_transparent_background_and_thin_artwork` 測試改直接驗證正式資源 atlas 中的紅石火把 tile（`PACK_TILES` 中 `(col: 6, row: 2)`）。
  5. **過期 allow(dead_code) 清理**：
     - 移除 `src/entity.rs` 中 `get_entities_by_type` 上的過期 `#[allow(dead_code)]`（已有 boss 模組正式 caller）。
     - 保留因 GPU lifetime 持有資源的抑制（如 `TextureAtlas.texture`）。
- 驗證命令與結果：
  - `cargo test --bin icraft hand_renderer::`：16 passed; 0 failed
  - `cargo test --bin icraft menu::`：36 passed; 0 failed
  - `cargo test --bin icraft texture::`：8 passed; 0 failed
  - `cargo test --bin icraft glyph_atlas::`：1 passed; 0 failed
  - `cargo check --all-targets --all-features`：exit 0
- 淨刪碼統計：7 files changed, 72 insertions(+), 306 deletions(-)，淨刪 234 行。


