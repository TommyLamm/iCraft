# 18 — 清理只作版本限制的直接依賴

狀態：待執行。基線：`83e751d`，2026-09-17。
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

尚未執行；完成時填寫改動、實際命令／結果、淨刪碼和文件更新。

