# 15 — 資源解碼只做一次，移出 shared 的桌面 codec

狀態：待執行。基線：`83e751d`，2026-09-17。
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

尚未執行；完成時填寫改動、實際命令／結果、淨刪碼和文件更新。

