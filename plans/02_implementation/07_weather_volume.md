# 實作計畫 07：可調雨聲音量

## 狀態

✅ 已完成（2026-07-24）

## 目標

增加獨立的 Minecraft 式 Weather Volume，立即控制 Rain 與 Thunder；
預設降低連續雨聲，並與 Master × Sound 聯乘及持久化。

## 現況

雨聲目前已受 `master_volume * sound_volume` 控制，但無法單獨調低。
`AudioManager` 只有一個平坦 gain；pause menu 又反推 master，存在雙資料源。

## 實作步驟

1. `GameSettings` 增加 `weather_volume`，預設 0.4，load/save/clamp 並向下兼容
   缺少新 key 的舊 settings。
2. `AudioManager` 保存 category multiplier 與 loop 的 `SoundId`；`gain_for`
   對 Rain/Thunder 使用 `master * sound * weather`，其他 SFX 不乘 weather。
3. `set_weather_volume` 即時更新已播放的 rain loop。
4. 主選單 Options 增加 Weather；pause menu 也增加 Weather，調節立即生效
   並寫回 settings。
5. 收斂音量 source-of-truth：State 修改 `self.settings` 後統一 sync 到 audio，
   不再由當前 mixer 值反推 master。
6. 更新架構與進度文件。

## 驗證

- [x] 舊 settings 缺 key 使用 0.4；超界/NaN 安全正規化；save 包含新 key。
- [x] Rain/Thunder gain 乘 weather，BlockBreak/footstep 不乘。
- [x] loop 播放中調 0 可即時靜音，再調高可恢復。
- [x] 主選單 Options 與 pause menu 都有不重疊的 Weather 控制。
- [x] `GameSettings` 為 source-of-truth，保存時同步 mixer，不反推 master。
- [x] `cargo fmt -- --check`、`cargo test --release`、`cargo check --release`。
- [ ] 人工雨天主選單/暫停調節與重啟持久化。

自動驗證結果：226 項單元測試與 1 項整合測試全部通過；唯一編譯
警告是既有未使用的 `hand_camera_buffer`。

## Commit

單一功能 commit：`feat(audio): add weather volume control`
