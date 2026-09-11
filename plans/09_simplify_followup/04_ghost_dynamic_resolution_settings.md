# Plan04 — 刪幽靈 `dynamic_resolution`／`render_scale`

## 定位

ARCHITECTURE 已寫：`dynamic_resolution` 不在 default desktop compile，無 live upscale pass。Plan 16 把模組改成 `cfg(test)`／`harness`，但 **settings 仍持久化兩個 GPU 從不讀的鍵**。

- `GameSettings.dynamic_resolution`／`render_scale`：只在 `menu.rs` load／save／serialize。
- `src/dynamic_resolution.rs`：~259 行，僅 `#[cfg(any(test, feature = "harness"))]`。
- `presentation/frame.rs`：`effective_scale = 1.0_f32` 硬編碼；無 offscreen upscale target。

`entity_distance_scale` **有**被 `frame.rs` 讀取，不要刪。

## 前置

無。不要改協定。

## 精確 acceptance

- [ ] `GameSettings` 不再有 `dynamic_resolution`／`render_scale` 欄位；load 時忽略舊 `settings.txt` 這兩行（不要 fail）。
- [ ] 刪 `src/dynamic_resolution.rs` 與 `main.rs`／`lib.rs` 的模組掛載（含 harness cfg）。
- [ ] `frame.rs` 不再保留「將來 scale≠1」的死 viewport 分支；scale 就是 1.0 或刪該區域。
- [ ] harness／`#[cfg(test)]` 不再依賴該模組；若有單元測試，刪或改成「settings 忽略未知鍵」。
- [ ] `ARCHITECTURE.md` 刪「模組仍在 harness 編譯」的描述，改為設定鍵已移除。
- [ ] `cargo check --bin icraft` 與 `cargo test --bin icraft -- menu` 通過。

## 預計檔案與測試

- `src/menu.rs`、`src/dynamic_resolution.rs`（刪）、`src/main.rs`、`src/lib.rs`、`src/presentation/frame.rs`
- 驗證：`cargo test --bin icraft -- settings` 或 menu settings roundtrip；`cargo check --bin icraft-server`（不該再看到該檔）

## 建議階段

1. 確認 `state.rs`／render 路徑零讀取（grep）。
2. settings load 改忽略未知鍵（若尚未）。
3. 刪欄位、模組、harness 測試。

## 不在本計劃

- 實作真正的 dynamic resolution upscale。
- 刪 GPU timestamp／perf HUD（live）。
- 刪 `entity_distance_scale`。
