# 22 — 公開面、測試與文件總驗收

狀態：待執行。基線：`83e751d`，2026-09-17。
前置：本輪選定的 01–21 全部完成後。

## 實作步驟

1. 將死原型測試隨實作刪除；有效產品 assertions 已按各包移到真正入口。核對沒有為了讓刪碼通過而改弱斷言。
2. 收窄跨 crate 不需要的 pub、清過期 imports／unused／unreachable 與失效 allow。桌面和 integration tests 真正需要的 API 保留。
3. 補齊 06 原計劃中的其他 gameplay 死 API尾項：mob::calculate_explosion_damage／explode、voxel_shape::get_connections 等先核對真正引用再刪；若範圍超過小型尾項，另立有證據的包，不順手改遊戲規則。
4. 同步 ARCHITECTURE 的模組地圖、唯一 session、player同步寫入、runtime job lifecycle、desktop projection、surface/index 和 Cargo feature 說明。
5. 在每份計劃填實作與實際命令結果，README 更新狀態；歷史計劃保留為歷史，不當現況契約。
6. 輸出 source 淨增刪、刪除模組／API、警告變化、依賴圖及驗證結果；機械搬檔、壓縮排版不當主要精簡收益。
7. 所有必要窄測試後跑一次完整驗證，只有新變更／失敗／未解問題才重跑。

## 最終命令

```powershell
cargo fmt --all -- --check
cargo check --all-targets --all-features
cargo check --no-default-features --bin icraft-server
cargo test --all-targets --all-features
cargo test --no-default-features --lib --tests
cargo tree --no-default-features -e normal
```

17 尚未完成時，不以 no-default-features 成功宣稱已完成 dependency fence。基線已有格式／測試問題須與修改所致失敗分開記錄。

## 驗收

- 舊玩法和無 caller API 真正消失，不改名移到新 cfg/test 殼。
- 桌面不再 worldgen；視距同步、projection commit、mesh identity 都由真實行為驗證。
- worldgen work 不因 completed backlog 重複生成；chunk 索引只有一份重建掃描。
- 解碼與音訊資料複製減少，有產品 fallback／bytes／計量測試。
- headless build dependency graph 符合 feature 邊界。
- 新行為測試未執行前不標已驗證；不存在虛構 FPS／latency 百分比。
- 任何實作改變 architecture/data contract 都已有同包文件更新。

## 實作紀錄

尚未執行；完成時記錄實際修改、驗證結果、刪碼量及文件更新。

