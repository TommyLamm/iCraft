# 17 — desktop feature 與 dedicated 建置邊界

狀態：已完成。基線：`83e751d`，2026-09-17。
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

- 改動細項：
  1. `Cargo.toml`：
     - 新增 `default = ["desktop"]`，以及 `desktop = ["dep:winit", "dep:wgpu", "dep:rodio", "dep:image", "dep:pollster"]`。
     - `microbench` feature 連帶啟用 `desktop`：`microbench = ["desktop"]`。
     - 將 `winit`、`wgpu`、`rodio`、`image`、`pollster` 宣告為 `optional = true`。
     - 明列 `[[bin]]` 目標：`name = "icraft"` 設定 `required-features = ["desktop"]`；`name = "icraft-server"` 不設任何 requirement。
  2. 共用可達樹與測試邊界核對：
     - `resources.rs` 僅使用通用 decode closure `resolve_decoded`，其單元測試僅測試 byte slice 閉包，不包含 `image`/`rodio` 依賴。
     - 實際影像與音效解碼單元測試位於 `texture.rs` 與 `audio.rs`（由 `main.rs` 即 `icraft` binary 包含，受 `desktop` feature 管轄）。
     - `bytemuck`（頂點 layout）、`zip`（資源包讀取）與 `rayon`（並行物理與生成）在 shared library 中有實質活躍呼叫，保留為無條件依賴。
  3. 修正 TCP 驗收測試 `tests/plan30_real_transport_acceptance.rs`：
     - 在合成工作台交易後，使用 `drive_until` 等待客戶端透過異步 TCP socket 接收 `PlayerSessionUpdate`，消除伺服器記憶體回應就緒即返回導致客戶端事件輪詢的時序競爭。
  4. 文件更新：
     - `README.md`：補充 `desktop` Cargo feature、無桌面依賴之專屬伺服器啟動指令（`cargo run --no-default-features --bin icraft-server`）與 `--microbench` 說明。
     - `ARCHITECTURE.md`：更新 Target 表格與建置邊界說明，記錄 `desktop` feature 封裝之依賴、`icraft` binary 的 required-features 與 dedicated 伺服器之無 default features 建置特性。
- 實際驗證命令與結果：
  - `cargo check --all-targets --all-features`：exit 0
  - `cargo check --no-default-features --bin icraft-server`：exit 0
  - `cargo test --no-default-features --lib --tests -- --test-threads=1`：724 unit tests passed (2 ignored pre-existing), 14 integration test suites passed (exit 0)
  - `cargo check --features microbench --bin icraft`：exit 0
  - `cargo tree --no-default-features -e normal`：確認 `image`, `rodio`, `winit`, `wgpu`, `pollster` 完全自正常 headless 依賴圖中移除；`bytemuck`, `zip`, `rayon` 正常保留
- 淨刪碼／保留原因：
  - 依賴邊界清晰切分：headless 正常建置圖淨減 5 個重量級 UI/繪圖/音效直接依賴及其下游龐大相依樹。
  - 保留：`indexmap`、`exr`、`half` 等直接版本 pin 依計劃留待 18 工作包獨立處置；`bytemuck`、`zip`、`rayon` 於 shared 庫中活躍使用，持續保留。

