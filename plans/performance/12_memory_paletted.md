# 任務 12：Paletted ChunkSection

> 對應計畫：`14_performance_optimization.md` Phase 4.1 + 4.2 + 4.3
> 狀態：Partial
> 審核回退：見 [`15_performance_audit_repair_plan.md`](15_performance_audit_repair_plan.md)；基本 palette/packed light 已存在，但 storage 不 demote、外部仍可 match representation、memory counter 不反映實際 representation，且缺 microbenchmark，待 R7、R9 修復。
> 前置：任務 1（基線）、建議任務 11（section 切分基礎）
> 目標：以 Empty/Uniform/Paletted/Global section storage 取代固定 voxel 陣列，降低 Chunk CPU 記憶體至少 40%。
> Commit 訊息：`perf(memory): introduce paletted chunk sections and packed lighting`

## 相關程式碼位置（已核對）

- `src/world.rs:1931` - `Chunk` 結構：目前每 voxel 固定保存 block、block state、sky light、block light 和 fluid level（約 320 KiB/Chunk）。
- `src/chunk_manager.rs:210` - `ChunkManager::set_block`：mutation 入口。
- `src/chunk_manager.rs` - block/light/fluid accessors。
- `src/lighting.rs` - sky/block light propagation。
- `src/fluid.rs` - fluid level storage。
- `src/redstone.rs:261` - `RedstoneSystem`：component discovery。
- `src/dimension.rs` - chunk generation。
- `src/save.rs` - chunk 序列化。

## 已確認的基線風險

- `Chunk` 為每個 voxel 固定保存 block、block state、sky light、block light 和 fluid level，共約 320 KiB；1,089 個 Chunk 僅原始 voxel storage 約 340 MiB。

## 子任務清單

### 12.1 Storage access abstraction
- [ ] 檔案：`src/world.rs`、`src/chunk_manager.rs`
- 步驟：
  1. 先建立 storage access abstraction（trait 或方法），外部系統不得直接索引固定 `chunk.blocks[x][y][z]`。
  2. 所有 `chunk.blocks[x][y][z]` 直接存取改為通過 abstraction。
  3. 確認所有 caller（lighting、fluid、redstone、physics、interaction、meshing、save、dimension）都改用 abstraction。
- 驗收：外部系統不直接索引 `chunk.blocks`；`Chunk::blocks` 可改為 private。

### 12.2 Block storage 支援 Empty/Uniform/Paletted/Global
- [ ] 檔案：`src/world.rs`
- 步驟：
  1. 每個 16³ section 的 block storage 支援：
     - `Empty`（全空氣）
     - `Uniform(BlockType)`（單一類型，如全石頭）
     - `Paletted { palette, packed_indices }`（多類型）
     - `Global` fallback（滿 palette，直接陣列）
  2. palette bits 依實際種類數選擇，避免為簡單 stone/air section 固定支付 4 KiB。
  3. mutation 時自動在 representation 間轉換。
- 驗收：簡單 section 不固定支付 4 KiB；mutation 正確轉換 representation。

### 12.3 Light nibble packing
- [ ] 檔案：`src/world.rs`、`src/lighting.rs`
- 步驟：
  1. sky light 和 block light 各為 0-15，合併進同一 byte 的兩個 nibble。
  2. 全零/全 15 light section 使用 uniform representation。
  3. lighting propagation 使用 nibble read/write。
- 驗收：light section 記憶體減半；lighting 結果不變。

### 12.4 Optional state/fluid storage
- [ ] 檔案：`src/world.rs`、`src/fluid.rs`
- 步驟：
  1. block state 與 fluid level 全零時不配置 storage。
  2. 首次非零 mutation 才建立 optional packed array。
- 驗收：無 fluid/state 的 section 不佔用額外記憶體。

### 12.5 Section metadata counts
- [ ] 檔案：`src/world.rs`
- 步驟：
  1. section 保存 non-air、opaque、random-tick、fluid、emitter、redstone component counts。
  2. 支援 O(1) early-out（例如全 air section 跳過 meshing/lighting）。
  3. heightmap 保持現有語義，並以 mutation 增量更新。
- 驗收：O(1) early-out 減少無效工作。

### 12.6 Save/network wire format 相容
- [ ] 檔案：`src/save.rs`、`src/network/protocol.rs`
- 步驟：
  1. save/network wire format 初期保持不變，在 serialization 邊界 flatten。
  2. 對現有世界做 load -> save -> load checksum。
  3. 確認舊存檔可正常載入。
- 驗收：存檔格式向後相容；load -> save -> load checksum 一致。

### 12.7 Microbenchmark 防退化
- [ ] 檔案：`src/world.rs`（測試區段）
- 步驟：
  1. microbenchmark `get_block`、`set_block`、lighting、physics collision 和 meshing。
  2. palette 不能以明顯 CPU regression 換取未使用的記憶體節省。
  3. 熱 Chunk/section 可在必要時使用較寬但更快的 representation；冷資料偏向壓縮。
- 驗收：microbenchmark 無明顯退化（< 5% regression）。

## 驗收條件

- [ ] 視距 16 的 Chunk CPU 記憶體降低至少 40%（以實測 working set 與分項 counter 驗證）。
- [ ] 外部系統不直接索引 `chunk.blocks`。
- [ ] light section 記憶體減半。
- [ ] 無 fluid/state 的 section 不佔用額外記憶體。
- [ ] O(1) early-out 生效。
- [ ] 存檔格式向後相容；load -> save -> load checksum 一致。
- [ ] microbenchmark 無明顯退化。
- [ ] `cargo fmt --all -- --check`、`cargo check --release`、`cargo test --release` 通過。

## 風險與回退

- paletted storage 是最高複雜度改動；必須有基線證明記憶體是瓶頸。
- access abstraction 是大範圍 refactor；必須逐步替換並持續測試。
- microbenchmark 若顯示明顯退化，熱路徑保留直接陣列存取。
- wire format 不變；若未來需版本化，必須 bump protocol version。

## 驗證命令

```text
cargo fmt --all -- --check
cargo check --release
cargo test --release
cargo run --release   # 視距 16 working set before/after
```
