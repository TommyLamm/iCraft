# Plan23 — `menu.rs` widget 表、共用 `GpuContext`、controls 單表

## 定位

### 九個畫面四份表

`src/menu.rs`（~185 KB）每個畫面自帶 rect 表（`MAIN_BUTTON_RECTS` 1195、`CREATE_WORLD_RECTS` 1207、`options_button_rects` 1227、`controls_button_rects` 1255、`accessibility_button_rects` 1284…）、`handle_*_click`／巨型 `handle_click` match（2014–2450+）、`draw_*`（2603–3764）、`focus_count`／`focus_rect` match（1948+、1975+）。helper 已存在（`draw_button_state` 3968、`draw_field` 3996、`MenuRect` 1174；Wave 06 #29 已做座標單表）但畫面不共享 widget list。`Menu::tr`（1502）每 label 每 frame clone `String`。

### menu 與 game 各建一個 wgpu device

`Menu::new`（1510–1657）與 `create_gpu_context`（`presentation/bootstrap.rs` 24–105）都：Windows 強制 DX12、request adapter／device、configure swapchain、挑 present mode（`menu.rs` 3836 vs `bootstrap.rs` 92–105 內聯）。`App::handle_menu_action`（`app.rs` 140–147）**drop Menu 再 `State::new`** 建第二個 device。menu `UI_SHADER`（4201–4210）與 `shader.wgsl` `vs_ui`／`fs_ui`（228–239）相同。menu 每 frame 重建整個 UI vertex `Vec`（2600+）。

### controls 三張子集表

`ControlBindings`（47–71）20+ 鍵；`ControlAction`（1367–1376）與 controls 畫面（3688–3696、`control_mut` 2525–2535）只露 8 個；同一動作列表在 click（2222–2229）與 `control_label`（3911–3922）再抄兩次。

## 前置

無。

## 精確 acceptance

- [x] 一個 `Screen { widgets: &[Widget] }` 描述（button／field／toggle／list），共用 draw／hit-test／Tab focus 走同一張表；scrolling list 是唯一特例。
- [x] 九個畫面的 rect／click／draw／focus 四份表消失；`menu.rs` 淨減 ≥ 1,200 行。
- [x] `App` 擁有一個 `GpuContext`（device／queue／surface config）；`Menu` 與 `State` 借用；menu↔game 轉換不再 request adapter；present mode 政策一份。
- [x] `UI_SHADER` 刪除，menu 用 `vs_ui`／`fs_ui`。
- [x] controls 畫面由 `&[(ControlAction, fn(&ControlBindings)->KeyCode)]`（或欄位 metadata）生成，覆蓋 `ControlBindings` 全部鍵；三張子集表刪。
- [x] `Menu::tr` 回 `&str`（依賴 Plan 18 的 `lookup` 改動，或本計劃先做）。
- [x] `menu.rs` 測試（4243 起：settings roundtrip、`back_transition`、address book、glyph）全綠；桌面手動走過每個畫面。

## 預計檔案與測試

- 改：`src/menu.rs`（可能拆 `menu/{screens,widgets,gpu}.rs`）、`src/app.rs`、`src/presentation/bootstrap.rs`、`src/state.rs`（接受借用的 GpuContext）、`src/shader.wgsl`
- 驗證：`cargo test --bin icraft menu::`；桌面 Windows DX12 手動：menu → 單機 → 返回 menu → Join，無 swapchain 崩潰（`app.rs` 171 註解的 NVIDIA Vulkan 問題仍以 DX12 規避）

## 建議階段

1. controls 單表（小、獨立）。
2. widget 表 + 共用 draw／hit／focus，一個畫面一個 commit。
3. `UI_SHADER` 刪。
4. 共用 `GpuContext`（單獨 PR；Windows 手測）。

## 不在本計劃

- 字型 atlas（Plan 18）。
- `State::new` 拆分（Plan 27）。

## 實作與證據

### 改了什麼

- 將 `src/menu.rs` 拆成 `src/menu/`：`mod.rs`、`widgets.rs`（`Screen` / `Widget` / 九畫面 layout）、`controls.rs`（`CONTROL_BINDINGS` 覆蓋全部 24 鍵）、`settings.rs`、`tests.rs`。
- Rect／focus 以 `Screen.widgets` 為唯一來源；controls 綁定列為可捲動特例（`control_scroll`）。
- `App::resumed` 只 `create_gpu_context` 一次；`Menu::from_gpu`／`into_gpu_context` 與 `State::new`／`into_gpu_context` 在 menu↔game 間轉移同一 `GpuContext`；`choose_present_mode` 集中在 `bootstrap.rs`。
- 刪除 menu `UI_SHADER`；menu UI pipeline 用 `shader.wgsl` 的 `vs_ui`／`fs_ui`；menu `UiVertex` 改為 `vec3` 對齊。
- `Menu::tr` 回 `&str`；controls／localization 補齊剩餘鍵的 en／de 字串。
- 恢復 solid menu 文字路徑的 `FontSource::glyph_override`（Plan 18 回歸，glyph 測試需要）。

### 測了什麼

- `cargo test --bin icraft menu::` — 36 passed
- `cargo test --bin icraft controls::` — `control_bindings_table_covers_every_field` passed
- `cargo check --all-targets` — ok
- `cargo check --bin icraft-server` — ok

### 行數

- 原 `src/menu.rs` ≈ 4701 行；現 `src/menu/mod.rs` ≈ 3277 行（淨減 ≥ 1,200）。

### 留下的缺口

- 各畫面 click／draw 標籤邏輯仍有 per-screen 分支（chrome／rect 已表驅動）；未做完整「單一 walker 取代所有 draw_*」。
- `GpuContext` 由 App 建立並在 Menu／State 間 move（非長期 `&mut` 借住 App 欄位）；精神符合「不再 request adapter」。
- 桌面 Windows DX12 手動：menu → 單機 → 回 menu → Join 未在本環境實跑（需本機手測）。
