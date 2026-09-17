# 17 — desktop feature 與 dedicated 建置邊界

狀態：待執行。基線：`83e751d`，2026-09-17。
前置：15；本包不必等待所有 gameplay 刪碼。

## 定位與證據

`Cargo.toml` 的 winit、wgpu、rodio、image、pollster 都是無條件 dependencies。實測 `cargo tree --depth 1` 均列為直接依賴。目前 shared `resources.rs` 仍引用 image／rodio；15 是真實前置。shared `chunk_render.rs:17` 使用 bytemuck，不能一併當成桌面依賴移除。

## 實作步驟

1. 新增預設啟用的 `desktop` feature，將 winit／wgpu／rodio／image／pollster 改 optional；`microbench = ["desktop"]`。
2. 明列 `icraft` binary 的 `required-features = ["desktop"]`；dedicated binary 不設這個 requirement。
3. 核對 shared reachable tree，不用空函式替代缺少 codec 的 shared API；依賴由 15 的消費者邊界消除。
4. 用 `cargo tree --no-default-features -e normal` 核對正常 build 的依賴邊。shared bytemuck／zip／rayon 有實際使用，保留。
5. resources 單元測試的 image／sound 消費者案例移到 desktop 測試邊界，headless 保留 generic resolver、ZIP、路徑與診斷測試；不把 rodio 用 dev-dependency 偷帶回每次 headless 測試。
6. 更新 README 與 ARCHITECTURE 的啟動／feature 指令。直接 pins 由 18 單獨處理。

## 驗證

```powershell
cargo check --all-targets --all-features
cargo check --no-default-features --bin icraft-server
cargo test --no-default-features --lib --tests
cargo check --features microbench --bin icraft
cargo tree --no-default-features -e normal
```

## 驗收

default desktop 及 microbench 保持可用；headless 正常依賴圖不再因本 package 無條件依賴而包含 image／rodio／winit／wgpu／pollster。這是減少建置範圍，不宣稱改善 FPS。

## 實作紀錄

尚未執行；完成時填寫改動、實際命令／結果、淨刪碼和文件更新。

