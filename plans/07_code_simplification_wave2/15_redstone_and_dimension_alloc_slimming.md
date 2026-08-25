# Plan15 — 紅石排程與維度堆配置優化

## 定位

在紅石系統與多維度生成中，存在以下過度工程與不必要的巨型堆分配：

1. **`src/redstone.rs:L201-260` 鏡像枚舉 `SavedDirection` 與 `SavedComparatorMode` (~60 行)**：
   `Direction` 與 `ComparatorMode` 本身已可直接 derive `Serialize, Deserialize`，卻額外手寫了完全鏡像的 `SavedDirection`、`SavedComparatorMode` 以及大量 `From` / `into_direction` 轉換樣板。
   直接在核心枚舉 derive serde，省去 60 行膠水代碼。

2. **`src/redstone.rs:L416-441` 快照序列化 match 壓縮**：
   25 行的手動 push discriminant byte 可直接利用 `scheduled.kind.encode(&mut bytes)` 簡化。

3. **`src/dimension.rs:L627-726` 地獄/終界生成 65k 元素巨型陣列堆分配 (~70 行)**：
   `generate_nether_chunk` 與 `generate_end_chunk` 每次生成區塊均在堆上分配 5 個 `Box<[[[BlockType; 16]; 256]; 16]>`（每區塊 >300KB），隨後在 `finish_chunk` 用 4 重循環搬移至 16 個 `ChunkSection`。
   改為直接按 Section 填充，消除巨型中間緩衝區的分配與拷貝開銷。

預期削減代碼 ~150 行。

## 前置

無。可與 07–13、16 並行。

## 精確 acceptance

- [ ] `redstone.rs` 移除鏡像的 `SavedDirection` 與 `SavedComparatorMode`，直接對 `Direction` / `ComparatorMode` 進行序列化。
- [ ] 簡化 `redstone.rs` 排程任務的序列化 match 邏輯。
- [ ] 消除 `dimension.rs` 中地獄/終界生成時的巨型 3D 陣列分配，改為直接填充 `ChunkSection`。
- [ ] 保持紅石時序、持久化格式與地獄/終界地形生成結果 100% 確定性一致。
- [ ] `cargo check --all-targets` 通過。
- [ ] 紅石與維度測試全數通過。

## 預計檔案與測試

- 修改：
  - `src/redstone.rs`
  - `src/dimension.rs`
- 驗證測試：
  - `cargo test --lib redstone::`
  - `cargo test --lib dimension::`
  - `cargo check --all-targets`

## 建議階段

1. 簡化 `redstone.rs` 中的方向/模式枚舉與序列化。
2. 重構 `dimension.rs` 中的地獄/終界區塊填充流程。
3. 運行紅石與維度單元測試驗證。

## 不在本計劃

- 更改紅石更新順序或強弱充能規則。
- 更改地獄/終界的地形雜訊或結構生成演算法。
