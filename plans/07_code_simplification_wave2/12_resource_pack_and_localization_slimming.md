# Plan12 — 資源包解析與本地化邏輯精簡

## 定位

在資源包與本地化模組中，存在多處過度設計與別名重複：

1. **`src/resources.rs` 複雜的依賴拓撲排序與別名重複 (~440 行)**：
   - `L650-757`：110 行複雜的資源包依賴拓撲排序、有向圖 DFS 循環檢測。本地資源包清單已有自然順序，直接採用確定性線性覆蓋（清單後者覆蓋前者），可大幅精簡邏輯。
   - `L404-471`：4 對完全重複的轉發別名方法（`resolve_texture`/`texture_bytes`、`resolve_model`/`model_bytes`、`resolve_font`/`font_bytes`、`resolve_sound`/`sound_bytes`），應統一保留單一方法。

2. **`src/localization.rs` 冗餘查找與字串替換 (~60 行)**：
   - `L499-524`：頂層 `translate` / `format` 函式使用 `OnceLock` 重新解析內建 JSON 並重複了 `TranslationCatalog` 的查找邏輯。應統一透過 `TranslationCatalog` 查詢。
   - `L372-391`：`format` 與 `format_lookup` 複製了兩次完全相同的 `{token}` 字串替換迴圈，抽取為共用 4 行 helper。
   - `L467-493`：26 行手寫字元解析函式 `key_component`，改用標準字元迭代器與 `to_ascii_lowercase` 簡化。

預期削減代碼 ~500 行。

## 前置

無。可與 07–11、13 並行。

## 精確 acceptance

- [ ] 移除 `src/resources.rs` 中 4 對重複的方法別名，統一使用 `resolve_*`（或 `*_bytes`）。
- [ ] 簡化 `src/resources.rs` 的資源包載入順序邏輯。
- [ ] 統一 `src/localization.rs` 的字串替換與 JSON 翻譯查找路徑。
- [ ] 確保多語言翻譯與資源包材質/音效覆蓋功能運作完全正常。
- [ ] `cargo check --all-targets` 通過。
- [ ] 資源包與本地化測試全數通過。

## 預計檔案與測試

- 修改：
  - `src/resources.rs`
  - `src/localization.rs`
- 驗證測試：
  - `cargo test --lib resources::`
  - `cargo test --lib localization::`
  - `cargo check --all-targets`

## 建議階段

1. 清理 `resources.rs` 中的重複別名並更新調用點。
2. 精簡 `resources.rs` 的拓撲排序為線性覆蓋。
3. 精簡 `localization.rs` 中的重複格式化與解析。
4. 運行資源與本地化單元測試。

## 不在本計劃

- 更改資源包目錄結構或 ZIP 安全限制。
- 更改任何現有語言檔案的 key/value 內容。
