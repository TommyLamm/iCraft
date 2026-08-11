# 29 — Locale layers 與 bounded visible labels

## 定位

- 優先級：P3，Plan 17/19 的本地化 consumer follow-up
- 前置條件：Plan 17 的 resource-pack resolver、`TranslationCatalog`、Accessibility settings
  與 Plan 19 的 selected-pack consumer fixtures
- 建議提交上限：3 個功能提交；本文件與回歸測試可併入對應提交
- 不處理：GPU/window/audio/DPI 或人工視覺證據、三拓撲 gameplay E2E、原版完整 pack
  format、parser grammar/help、debug/F3、branding、raw user input 與動態 item/entity names

## 目標與邊界

Plan 17 原本的 locale resolver 只取第一個合法 payload，無法表達 selected pack 的 partial
locale；Plan 19 也把 consumer 接線描述成 source-wide 完成。Plan 29 將這兩件事收斂成
可測的 bounded contract：selected packs 依高到低疊層，builtin 是最低層；每一層先通過
UTF-8/JSON `HashMap<String, String>` validator，壞層跳過且一個 logical path 只留一筆
diagnostic。既有 `resolve_locale` 的第一個合法結果語義保留。

`TranslationCatalog` 以低到高 merge layers；active language 缺 key 時查 merged English，
English 仍由 merged English 提供。這保證 selected pack 只覆蓋部分 `de_de` 或 `en_us` 時，
不會吞掉較低層或 builtin sentinel。

本計劃的 `VISIBLE_REQUIRED_KEYS` 是刻意有限的可見字串契約，不追求 source-wide zero
literal。涵蓋 menu/world/create/options/accessibility/resource-packs/controls/delete、
save/connection/death/pause HUD、inventory/station，以及 command prefix 與穩定執行 status；
parser grammar、help/error 原文、debug/F3、branding、raw input 和動態名稱仍屬非目標。

## 實作範圍

- `src/resources.rs`
  - 新增 `resolve_locale_layers`，輸出高到低合法 layer，validator 與 diagnostic 去重
    共用既有資源包安全邊界。
- `src/localization.rs`
  - 新增 bounded `VISIBLE_REQUIRED_KEYS`、低到高 merge、English fallback、immutable
    `format_lookup` 與 coverage helper。
- `src/menu.rs`
  - 將穩定的 menu、world/create/options/accessibility/resource-pack/controls/delete labels
    接到 catalog；動態使用者輸入與 metadata 不在契約內。
- `src/state.rs`
  - 將 save/connection/death/pause HUD、inventory/station labels，以及 command prefix/
    stable status 接到 immutable catalog lookup；保留 parser/help/debug/raw dynamic literals。
- `assets/lang/en_us.json`、`assets/lang/de_de.json`
  - 為 bounded key set 提供 UTF-8、key-complete English/German values。

## 驗收與測試

- locale layer unit：高到低順序、partial selected EN/DE merge、invalid UTF-8/JSON skip 與
  diagnostic 去重、selected-pack sentinel、builtin fallback。
- catalog unit：`VISIBLE_REQUIRED_KEYS` 在 EN/DE 均有值、immutable format lookup、EN↔DE
  switch 只改 presentation catalog。
- targeted lanes：`cargo test --lib resources:: --no-fail-fast`、`localization::`、`menu::`；
  `state::` 若沒有純測試須記錄 0 matched，而不是宣稱 GPU render evidence。
- standard gates：`cargo fmt --all -- --check`、debug/release tests、`cargo check --all-targets`
  與 `git diff --check`。GPU/window/audio/DPI、clean-checkout asset startup、三拓撲 E2E
  仍保留給 Plan 19/人工 QA 的 `[ ]` evidence。

Final serial record (2026-08-12, `--test-threads=1`): debug and release each
passed 1,532 tests with 0 failures and 6 ignored (688 library, 819 client
binary, 2 server binary, 23 integration, 0 doc-tests). Targeted resource,
localization, and menu lanes passed 18, 10, and 30 tests; the state-only filter
matched 0 pure tests and therefore supplies no GPU/render evidence. Release
check, all-target check, fmt check, and diff check all passed.

## 完成判定

完成只表示上述 bounded headless contract 與 consumers 有 code/test evidence；不改寫
Plan 17/19 對人工視覺、GPU/音效、DPI、clean checkout 或 listen/dedicated topology 的
未完成狀態。若未來要擴大 locale 到 parser/debug 或做完整 source audit，另開後續 plan。
