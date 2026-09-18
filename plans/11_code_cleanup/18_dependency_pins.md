# 18 — 清理只作版本限制的直接依賴

狀態：已完成。基線：`83e751d`，2026-09-17。
前置：17 建議先完成，便於分別檢查 desktop／headless。

## 定位與證據

`Cargo.toml` 直接宣告 indexmap、exr、half、rayon-core，註解說明是 image／rayon 的版本 pin。source 無對應直接 import，但這本身不足以判定 dependency 無用途。

本次 `cargo tree -i` 確認：
- exr 由 image 及本 package 直接引入。
- indexmap 由 naga／wgpu-core／zip 及本 package 引入；即使 headless 移除 GPU，zip 仍可能需要它。

## 實作步驟

1. 逐項記錄 direct pin 和反向依賴，不把傳遞依賴誤列為可從 lockfile 整包刪除。
2. 移除沒有 source 用途、且現有 Cargo.lock 已可維持解析結果的 direct pins；每次比較 lockfile，避免順手 update 整個依賴樹。
3. 若某 pin 仍影響可建置性／版本統一，就留一處明確理由，不增加 package metadata 假用途來迴避 dependency 檢查。
4. 只刪已脫離完整圖的 lockfile packages；不在本包裁減目前資源格式支援。
5. 以實際刪除的直接宣告與正常建置依賴圖作為成果，不用「少 import」估算二進位大小。

## 驗證

```powershell
cargo check --locked --all-targets --all-features
cargo check --locked --no-default-features --bin icraft-server
cargo tree --no-default-features -e normal
```

修改 manifest 後如 Cargo 要求先更新 root lock entry，先正常解析一次，再以 --locked 驗收；只接受與本包相符的 lock 差異。

## 驗收

直接 dependency 清單代表實際 source 或仍必要的解析限制。留下每一個未刪 pin 的具體原因，沒有無關版本升級。

## 實作紀錄

- 改動細項：
  1. `Cargo.toml`：
     - 移除無 source 引用之直接 pins：`indexmap = "=2.2.6"`、`exr = "=1.71.0"`、`half = "=2.2.1"`、`rayon-core = "=1.12.1"`，以及註解 `# Pinned so image / rayon keep the versions this workspace validated.`。
     - 將活躍使用的直接依賴 `rayon = "=1.10.0"` 改為標準 semver 宣告 `rayon = "1.10"`，符合全專案依賴宣告慣例。
  2. `Cargo.lock`：
     - 僅移除 root package `icraft` 的 4 個無 source direct pins（`exr`, `half`, `indexmap`, `rayon-core`）。
     - 依賴項套件解析版本完全不變：`indexmap 2.2.6`（由 `zip` / `naga` / `wgpu-core` 引入）、`exr 1.71.0`（由 `image` 引入）、`half 2.2.1`（由 `exr` 引入）、`rayon-core 1.12.1`（由 `rayon` / `exr` 引入）、`rayon 1.10.0`（由 `icraft` 引入），皆由 `Cargo.lock` 精確鎖定。無任何不相關依賴升級或版本變動。
  3. Headless 依賴圖精簡效果：
     - 在 `--no-default-features` 下，`image` 關閉使得 `exr` 不再被任何套件引入。
     - 清理直接 pin 後，`icraft-server` 正常依賴圖直接移除了 `exr`、`half` 及 `exr` 引入的 8 個傳遞套件（`bit_field`, `flume`, `spin`, `lock_api`, `scopeguard`, `lebe`, `miniz_oxide 0.7.4`, `smallvec`, `zune-inflate`），消除了 headless 建置中重複的 `miniz_oxide` 舊版本（`0.7.4` vs `flate2` 的 `0.8.9`）。
     - `indexmap` 在 headless 下僅由 `zip` 引入，不再作為 root 直接依賴重複暴露；`rayon-core` 僅由 `rayon` 引入。
- 實際驗證命令與結果：
  - `cargo tree -i indexmap` / `exr` / `half` / `rayon` / `rayon-core`：確認反向依賴關係與無 source 直接 import 特性。
  - `cargo check --locked --all-targets --all-features`：exit 0
  - `cargo check --locked --no-default-features --bin icraft-server`：exit 0
  - `cargo tree --no-default-features -e normal`：確認 headless 正常依賴圖完全移除了 `exr`, `half` 及其傳遞依賴；`indexmap` 僅屬 `zip`；`rayon-core` 僅屬 `rayon`。
- 淨刪碼／保留原因：
  - 淨刪碼：`Cargo.toml` 淨刪 5 行（4 個 pins + 1 行註解）；`Cargo.lock` 淨刪 4 行直接依賴條目。
  - 保留原因：
    - `rayon`：於 `src/server_runtime/worldgen_worker.rs`、`src/state.rs`、`src/server_world/mod.rs` 活躍使用（並行物理、世界生成、方塊搜尋），為實質 direct dependency，轉為常規 semver `"1.10"` 保留。
    - `indexmap`, `exr`, `half`, `rayon-core`：在 `Cargo.lock` 中作為傳遞依賴保留（版本分別為 2.2.6, 1.71.0, 2.2.1, 1.12.1），現有 lockfile 已維持完全一致之解析結果，無需也不應在 root `Cargo.toml` 中增加假用途宣告。

