# 實作計畫 09：背包打開時禁止視角轉動

## 目標

背包、進度、死亡、聊天、暫停、斷線或失焦時禁止 raw mouse look，但仍讓
`CursorMoved` 更新 UI hover；關閉 UI 後正確恢復 grab/hide。

## 已確認根因

`App::device_event` 直接處理 `DeviceEvent::MouseMotion`，gate 只有 paused、
chat、connection lost、focus，漏了 `inventory.is_open`。釋放 cursor grab
不會阻止 raw mouse event，所以視角仍改變。

## 實作步驟

1. 抽純 `allows_camera_look` predicate，統一檢查 pause、inventory、
   advancements、chat、connection lost、dead 與 focus。
2. `device_event` 只有 predicate 為 true 才套 sensitivity 並 clamp pitch。
3. 保留 `WindowEvent::CursorMoved -> handle_mouse_move`，UI hover 不受影響。
4. 集中 cursor mode 同步：純 gameplay 嘗試 Locked（fallback Confined）並隱藏；
   任何 blocker 則 None 並顯示。
5. E toggle 改為 `pressed && !event.repeat`，避免長按反覆開關。
6. 防止 inventory 與 advancement 同時打開；close UI 時依完整狀態決定是否
   重抓鼠標。
7. 更新架構與進度文件。

## 驗證

- predicate truth table：每個 blocker 單獨出現都禁止 look。
- disabled MouseMotion 不改 yaw/pitch；enabled 正確套 sensitivity/clamp。
- inventory open 時 CursorMoved 仍改 `mouse_ndc`。
- key repeat 不反覆 toggle；inventory/advancement 不會重疊。
- `cargo fmt -- --check`、`cargo test --release`、`cargo check --release`。
- 人工 E 開啟後大幅移動鼠標、點 UI、E/Esc 關閉再測 gameplay look。

## Commit

單一功能 commit：`fix(input): lock camera while inventory is open`

