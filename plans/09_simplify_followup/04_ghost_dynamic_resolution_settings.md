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

- [x] `GameSettings` 不再有 `dynamic_resolution`／`render_scale` 欄位；load 時忽略舊 `settings.txt` 這兩行（不要 fail）。
- [x] 刪 `src/dynamic_resolution.rs` 與 `main.rs`／`lib.rs` 的模組掛載（含 harness cfg）。
- [x] `frame.rs` 不再保留「將來 scale≠1」的死 viewport 分支；scale 就是 1.0 或刪該區域。
- [x] harness／`#[cfg(test)]` 不再依賴該模組；若有單元測試，刪或改成「settings 忽略未知鍵」。
- [x] `ARCHITECTURE.md` 刪「模組仍在 harness 編譯」的描述，改為設定鍵已移除。
- [x] `cargo check --bin icraft` 與 `cargo test --bin icraft -- menu` 通過。

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

## 實作與證據

### 改了什麼

- `GameSettings` 刪除 `dynamic_resolution`／`render_scale`。load 原本就有 `_ => {}` 忽略未知鍵；舊 `settings.txt` 這兩行改走該路徑，不再 fail。
- 刪 `src/dynamic_resolution.rs`。`main.rs` 拿掉 `#[cfg(any(test, feature = "harness"))] mod dynamic_resolution;`。`lib.rs` 本來就沒掛這個模組。
- `presentation/frame.rs` 刪掉硬編碼 `effective_scale = 1.0` 與兩段永遠不進的 viewport 分支。`entity_distance_scale` 仍被 entity render 距離讀取。
- 原模組內 7 個 controller 單元測試隨檔刪除；改加 `leftover_dynamic_resolution_settings_keys_are_ignored`：舊鍵 + 未知鍵不 fail，known keys 仍 round-trip，save 不再寫那兩行。
- `ARCHITECTURE.md`：不再寫「模組仍在 harness 編譯」；改為設定鍵已移除、舊行忽略。

### 測了什麼

- `cargo check --bin icraft`：通過。
- `cargo test --bin icraft -- menu`：35 passed（含 `leftover_dynamic_resolution_settings_keys_are_ignored` 與 settings round-trip）。
- `cargo test --bin icraft -- settings`：8 passed。
- `cargo check --bin icraft-server`：通過。`target/debug/deps/icraft*.d` 不含 `dynamic_resolution.rs`。

### 剩餘缺口

- 沒有實作真正的 dynamic resolution upscale（刻意排除）。
- GPU timestamp／perf HUD 未動。
- `entity_distance_scale` 保留。
- Packet／`PROTOCOL_VERSION` 未改。
- Plan 05 未開始。
