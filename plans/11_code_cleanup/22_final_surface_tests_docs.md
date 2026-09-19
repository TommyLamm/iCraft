# 22 — 公開面、測試與文件總驗收

狀態：已完成。基線：`83e751d`，2026-09-17。
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

- 實作完成於 2026-09-19。
- 刪除無 caller 死 API 與死原型測試：
  - 刪除 `src/mob.rs` 中無引用之 `calculate_explosion_damage` 與 `explode` 函式及對應原型測試。
  - 刪除 `src/voxel_shape.rs` 中無外部呼叫之 `get_connections` 包裝函式，呼叫點直接調用 `get_connections_sampled`。
  - 刪除 `src/world/mesh/section.rs` 中無 caller 之內部包裝 `mesh_section_lod_from_halo`。
- 收窄跨 crate 與跨模組可見性，清理過期 imports 與失效 allow：
  - 將各模組中僅內部或測試使用的 API 收窄為 `pub(crate)` 或 `#[cfg(test)]`。
  - 清理跨模組之 unused imports（包含 `tests/review_hardening_invariants.rs`、`tests/waterlogging_authority.rs`、`src/physics.rs`、`src/worldgen/mod.rs` 等），並在對應測試模組中補全具體引用，嚴格保持 pub 收窄原則，不隨意放寬。
- 測試強固化與 Windows 檔案系統競態防護：
  - 修復 `src/save/region.rs` 中 Windows 平臺原子替換檔案 `replace_file_atomically`：加入針對 `ERROR_ACCESS_DENIED (5)` 與 `ERROR_SHARING_VIOLATION (32)` 的短暫鎖定重試機制，消除在 Windows 檔案系統並發及防毒/索引掃描時的瞬間競爭。
  - 修復 `src/server_runtime/tests.rs` 中 `runtime_instances_have_isolated_worldgen_channels` 在高並發全套測試負載下的 Rayon worker 排程等待逾時 flake。
- 執行並全數通過 6 項驗證命令：
  - `cargo fmt --all -- --check`：通過（exit 0）。
  - `cargo check --all-targets --all-features`：通過（exit 0）。
  - `cargo check --no-default-features --bin icraft-server`：通過（exit 0，零編譯警告）。
  - `cargo test --all-targets --all-features`：通過（722 lib tests、228 bin tests、2 server tests 及全部 integration tests 通過）。
  - `cargo test --no-default-features --lib --tests`：通過（722 unit tests 及全數 headless 整合測試通過）。
  - `cargo tree --no-default-features -e normal`：通過，驗證無 winit/wgpu/rodio/image/pollster 桌面依賴進入 headless 目標。
- 文件更新：
  - 更新 `ARCHITECTURE.md`，同步記錄 dead helpers (`calculate_explosion_damage`, `explode`, `get_connections`) 移除。
  - 更新 `plans/11_code_cleanup/README.md`，將 22 號計劃狀態標記為「已完成」。
- 淨增刪統計（全代碼庫格式化與清理）：143 files changed, 3568 insertions(+), 3311 deletions(-)。

