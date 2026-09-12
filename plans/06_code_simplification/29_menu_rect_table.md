# Plan29 — `menu.rs` 按鈕座標單一 `MenuRect` 表

## 定位

`src/menu.rs` 約 4,757 行。每個 `MenuScreen` 的按鈕 NDC 矩形至少寫三次：
focus 列表、`handle_click` 的 `hit(x, y, …)`、draw。掃描當日 Main 為例：

| 用途 | 約略位置 |
| --- | --- |
| focus rects | `:1888` 附近 `[[-0.34, 0.34, 0.21, 0.34], …]` |
| click | `:1906` `hit(x, y, -0.34, 0.34, 0.21, 0.34)` |
| draw | 另處再寫一次相同數字 |

改一個按鈕要對三份座標。10／15 明確不拆選單。本計劃只收座標表，不改畫面、
不改 `MenuAction`、不改 world discovery。

## 前置

無。可與 22–24、26、27、30 並行。不要跟任何人搶 `menu.rs` 以外的檔。

## 精確 acceptance

- [x] 每個 `MenuScreen` 有一張按鈕表（名稱可微調，例如 `MenuRect { left, right, top, bottom, id }`）。
      focus 導航、`handle_click`、draw **都讀這張表**，不得再各寫一份字面量。
- [x] 數字 bit-identical。允許 `hit(x,y,l,r,b,t)` 改成 `rect.contains(x,y)`，
      矩形本身不得平移。
- [x] 動態列（world list、server list）仍用現有 scroll／`top - 0.15` 公式，
      但公式只出現一次。
- [x] 不得改 `MenuAction`、`MultiplayerRole` 切換、world create／delete、
      controls 綁定語意。
- [x] 不得開始拆 `menu.rs` 成 ECS 或把選單搬進 `presentation/`。
      本計劃成功標準是座標單一來源，不是 4.7k 行變 200 行。
- [x] 既有 `presentation_inventory_policy`／menu 單元測期望值不變。

## 預計檔案與測試

- 修改：`src/menu.rs`（可在同檔加 `MenuRect`／`fn screen_buttons`）。
- 測試：
  - `cargo check --bin icraft`
  - `cargo test --bin icraft` 裡現有 `menu` 單元測（執行前 `rg "mod tests" src/menu.rs` 列出）
  - `cargo test --lib presentation_inventory_policy::`
  - `cargo check --bin icraft-server`（仍不得編 `menu.rs`）

## 建議階段

1. 先抽 Main／Options 這種固定四顆鈕。對一下 focus 與 click 的舊數字。
2. 再收 Worlds／Multiplayer 的動態列公式。
3. ConfirmDelete 等小畫面。`cargo check --bin icraft`。

## 不在本計劃

- 重做選單視覺、加新畫面。
- 把 `WorldLaunch` 搬出 `menu.rs`。
- leftover／authority 檔。

## 實作與證據

### 1. 變更概述
- 在 `src/menu.rs` 定義了輕量 `MenuRect` 結構體（含 `x0`, `x1`, `y0`, `y1`，並提供 `contains(x, y)` 與 `as_array()` 方法）。
- 建立了各畫面的單一真實來源表與動態項目生成函式：
  - `MAIN_BUTTON_RECTS`: Main 畫面的 4 顆按鈕矩形。
  - `CONFIRM_DELETE_BUTTON_RECTS`: 確認刪除畫面的 2 顆按鈕矩形。
  - `CREATE_WORLD_RECTS`: 建立世界畫面的 11 個輸入框與選項按鈕矩形。
  - `OPTIONS_ROW_TOPS`, `OPTIONS_BOTTOM_RECTS`, `options_button_rects()`: Options 畫面的 15 顆按鈕矩形。
  - `CONTROLS_SENSITIVITY_RECT`, `CONTROLS_DONE_RECT`, `control_button_rect()`, `controls_button_rects()`: Controls 畫面的 10 顆按鈕矩形。
  - `ACCESSIBILITY_DONE_RECT`, `accessibility_button_rect()`, `accessibility_button_rects()`: Accessibility 畫面的 11 顆按鈕矩形。
  - `RESOURCE_PACKS_BOTTOM_RECTS`, `resource_pack_item_rect()`: ResourcePacks 畫面列表項目與底部按鈕矩形。
  - `WORLDS_BOTTOM_RECTS`, `world_item_rect()`: Worlds 畫面列表項目與底部按鈕矩形。
  - `MULTIPLAYER_MODE_RECTS`, `MULTIPLAYER_HOST_PORT_RECT`, `MULTIPLAYER_JOIN_FIELD_RECTS`, `MULTIPLAYER_PING_RECT`, `MULTIPLAYER_BOTTOM_RECTS`, `recent_server_item_rect()`, `multiplayer_focus_rects()`: Multiplayer 畫面所有模式、輸入框、歷史伺服器與按鈕矩形。
- 統一了 `focus_count(&self)`、`focus_rect(&self)`、`handle_click(&mut self)`（以及附屬點擊處理器 `handle_options_click`、`handle_accessibility_click`、`handle_resource_pack_click`、`select_recent_server`）與所有 `draw_*` 渲染函式，全數改由上述 `MenuRect` 表／函式讀取，消除所有重複座標字面量。
- `hit(...)` 委派給 `MenuRect::new(x0, x1, y0, y1).contains(x, y)` 保持向前相容。
- 新增單元測試 `menu_rect_tables_are_valid_and_consistent` 驗證所有表與生成器的 NDC 邊界合法性與包含邏輯。

### 2. 驗證證據
1. `cargo test --bin icraft menu::`：34 項 menu 單元測試全數通過（0 failed）。
2. `cargo test --lib presentation_inventory_policy::`：4 項測試全數通過。
3. `cargo check --bin icraft-server`：編譯檢查成功（無 `menu.rs` 依賴污染）。
4. `cargo test --bin icraft`：全數 205 項測試（含 smoke checksum 與 remote sync）全數通過（0 failed, 1 ignored benchmark）。
