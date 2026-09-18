# 15 — 資源解碼只做一次，移出 shared 的桌面 codec

狀態：已完成。基線：`83e751d`，2026-09-17。
前置：無；完成後再做 16、17。

## 定位與證據

- `src/resources.rs:368`：`resolve_texture` 呼叫 `image::load_from_memory`，僅留下成功／失敗，丟掉解碼結果。
- `src/texture.rs:496–502`：`apply_resource_pack_with_manager` 取得 bytes 後再次 `load_from_memory`。
- `src/resources.rs:527`：`resolve_validated_asset` 先建立所有候選的 `Vec<Arc<[u8]>>`，再逐一 validator。
- `src/resources.rs:967`：聲音 validator 依賴 rodio 並 `bytes.to_vec()`；因此目前只將 Cargo dependencies 設 optional 並不足以讓 shared 在無桌面模式編譯。
- `src/block_model.rs:39` 的正式 model registry 使用 `read_asset + parse_model_descriptor`，不經 `resolve_model`；後者目前僅由 resources 自身測試引用。

## 實作步驟

1. 將資源覆蓋順序、路徑處理、診斷去重保留在 ResourcePackManager；將 typed decode 放在消費者提供的 closure。建議單一 `resolve_decoded<T>`，回傳第一個成功解析的值，而非先 bool 驗證再回 bytes。
2. 讓 texture 消費者直接取得已解碼影像；同一 atlas 建構若多 tile 使用相同來源，可用該次建構內的 path cache 重用影像，完成後釋放。
3. 移除 resources 對 image／rodio 的直接依賴；音效解碼驗證移到 audio 端，透過同一候選遍歷 API 保留壞 override → 有效 fallback 行為。
4. 若 borrow 邊界允許，逐候選解碼成功即返回，移除整份候選 Vec；不要為省這個短 Vec 引入複雜 trait 層。
5. 刪只有舊測試使用的 `resolve_model` 包裝；將有效資源測試改走正式 registry。不要順便改 `ModelRegistry` 對 unsupported descriptor 使用 procedural fallback 的既有語意。
6. font 的正式輸出是 bitmap glyphs；將 magic-only 驗證與真正 bitmap parse 合成一次。現有缺省 font 安靜回退及 invalid payload 診斷去重行為保持可測。
7. 不改目前支援的圖片／音訊格式；codec feature 裁減不混入本包。

## 驗證

既有：`resources::tests::typed_consumers_skip_invalid_override_and_deduplicate_diagnostics`、`bitmap_font_source_is_parsed_and_invalid_payload_falls_back_once`、`texture::tests::resource_pack_atlas_applies_real_textures`、`paint_on_miss_atlas_build_is_pack_first`、`block_model::tests::selected_model_descriptor_reaches_mesh_consumer`。

新增一個可計數 decoder 的測試，覆蓋 invalid override → valid fallback，確認成功候選只解析一次；另檢查重複 atlas source 不重複 decode（若加入建構期 cache）。這些是新增驗證，不是已存在的結果。

## 驗收

- 正式 texture 載入不再 bool decode 後重做 decode。
- shared resources 不 import image／rodio；診斷、資源優先序和 fallback 有行為測試。
- 純 wrapper 減少，不增加同功能第二套 resolver。

## 實作紀錄

- 改動細節：
  - `src/resources.rs`：
    - 徹底移除 `image` 與 `rodio` 依賴，解除 shared 對桌面 codec 的直接綁定。
    - 引入單一消費端解碼 API `pub fn resolve_decoded<T, F>(&mut self, relative: &str, kind: &str, mut decode: F) -> Option<T>`：由最高優先序已啟用 pack 逆序遍歷至 `BUILTIN_PACK_ID`，逐一使用 caller 提供的 decode closure 解碼，解碼成功即立即返回；失敗記錄去重診斷並繼續 fallback，完全消除中間 `Vec<Arc<[u8]>>` 候選集合分配。
    - 刪除所有舊 bool 驗證與 test-only 轉接層：`resolve_texture`、`resolve_sound`、`resolve_model`、`resolve_font`、`resolve_validated_asset`、`font_bytes_are_decodable`、`sound_bytes_are_decodable`。
    - `resolve_font_source` 將 magic 檢驗與 bitmap 解析整合為 `parse_bitmap_font`，保留缺省 quiet fallback 與格式異常診斷去重。
    - 既有測試 `typed_consumers_skip_invalid_override_and_deduplicate_diagnostics` 遷移至 `resolve_decoded`、`ModelRegistry::from_resource_packs` 與 `resolve_font_source` 正式入口。
    - 新增回歸測試 `counted_decoder_only_decodes_successful_candidate_once`，驗證 corrupt override -> valid fallback 流程下有效候選僅被解碼一次。
  - `src/texture.rs`：
    - 抽取 `apply_resource_pack_tiles_with_decode`，呼叫 `resolve_decoded` 直接獲取已解碼之 `image::DynamicImage`，移除 `image::load_from_memory` 二次解碼。
    - 在單次 atlas 建構過程中維護 `decoded_cache: HashMap<&'static str, Option<image::DynamicImage>>`，相同來源路徑的 tile 僅解碼一次，建構完畢隨函式退出即釋放。
    - 更新 `compose_player_head_tiles_with_manager` 與 `compose_enderman_eyes_with_manager` 改走 `resolve_decoded`。
    - 新增回歸測試 `repeated_atlas_sources_are_decoded_only_once_during_atlas_build`，驗證多次參照相同路徑的 tile 僅觸發單次解碼。
  - `src/audio.rs`：
    - 將音效 bytes 驗證邏輯收斂至 `src/audio.rs` 的 `sound_bytes_are_decodable`，透過 `resolve_decoded(&logical_path, "sound", |bytes| sound_bytes_are_decodable(bytes).then(|| bytes.to_vec()))` 保留 invalid override -> fallback 行為。
  - 文件更新：
    - `ARCHITECTURE.md`：記錄資產解碼邊界（`resolve_decoded`）、shared 不依賴桌面 codec、以及 atlas 建構期紋理快取架構。
    - `plans/11_code_cleanup/README.md`：更新工作包 15 狀態為「已完成」。
- 驗證命令與結果：
  - `cargo check --all-targets`（通過，0 errors）
  - `cargo test --lib resources`（通過，17 passed, 0 failed）
  - `cargo test --bin icraft texture`（通過，11 passed, 0 failed）
  - `cargo test --bin icraft audio`（通過，7 passed, 0 failed）
  - 專項測試全數通過：
    - `resources::tests::typed_consumers_skip_invalid_override_and_deduplicate_diagnostics`
    - `bitmap_font_source_is_parsed_and_invalid_payload_falls_back_once`
    - `texture::tests::resource_pack_atlas_applies_real_textures`
    - `paint_on_miss_atlas_build_is_pack_first`
    - `block_model::tests::selected_model_descriptor_reaches_mesh_consumer`
    - `counted_decoder_only_decodes_successful_candidate_once`
    - `repeated_atlas_sources_are_decoded_only_once_during_atlas_build`
- 淨碼統計：
  - 修改 3 處核心檔案，淨更動 +209 / -124（淨增加為 2 項新增單元測試與計數驗證，徹底移除 bool 重複解碼與純轉接 wrapper）。

