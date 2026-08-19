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

- [ ] 每個 `MenuScreen` 有一張按鈕表（名稱可微調，例如 `MenuRect { left, right, top, bottom, id }`）。
      focus 導航、`handle_click`、draw **都讀這張表**，不得再各寫一份字面量。
- [ ] 數字 bit-identical。允許 `hit(x,y,l,r,b,t)` 改成 `rect.contains(x,y)`，
      矩形本身不得平移。
- [ ] 動態列（world list、server list）仍用現有 scroll／`top - 0.15` 公式，
      但公式只出現一次。
- [ ] 不得改 `MenuAction`、`MultiplayerRole` 切換、world create／delete、
      controls 綁定語意。
- [ ] 不得開始拆 `menu.rs` 成 ECS 或把選單搬進 `presentation/`。
      本計劃成功標準是座標單一來源，不是 4.7k 行變 200 行。
- [ ] 既有 `presentation_inventory_policy`／menu 單元測期望值不變。

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
