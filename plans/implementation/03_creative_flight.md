# 實作計畫 03：Creative Minecraft 式飛行

## 目標

Creative 模式雙擊 Jump 切換飛行；WASD 水平移動、Space 上升、Shift
下降，保留實體碰撞，不穿牆；切 Survival、死亡/重生與切維度時安全退出。

## 實作步驟

1. `PlayerPhysics` 增加 transient `is_flying` 與統一 setter；切換時清垂直
   速度並重設 fall-distance 基準，不寫入存檔。
2. 在 State 增加可測的 300 ms `DoubleTapTracker`，只接受 non-repeat 的
   Jump press；Survival 不會預先武裝。
3. `App::handle_game_keyboard` 將 Jump press edge 交給 State；`G` 模式切換
   也改為 non-repeat 並走 `State::set_game_mode`。
4. 飛行 movement：Space/Shift 分別為 +Y/-Y，同時按為 0；WASD 保持相機
   yaw 水平向量。
5. `PlayerPhysics::update` 飛行分支忽略重力、流體浮力/阻力與 fall damage，
   但沿用三軸 solid collision。
6. 向下飛到地面時退出；撞天花板只清 Y 速度。再次雙擊 Jump 主動退出。
7. Creative → Survival、死亡、respawn、dimension switch 清飛行與 pending
   tap；暫停/背包/聊天/失焦只清按鍵並保持 hover。
8. 更新 F3 狀態提示、架構與進度文件。

## 驗證

- Double tap window、repeat、reset、模式切換單元測試。
- hover、上升、下降、雙鍵、牆/頂/地碰撞、水/熔岩與 fall damage 測試。
- Survival 原有 jump/gravity/fall damage 不回歸。
- `cargo fmt -- --check`、`cargo test --release`、`cargo check --release`。
- 人工第一/第三人稱、聯機位置同步與模式切換。

## Commit

單一功能 commit：`feat(creative): add minecraft-style flight`

