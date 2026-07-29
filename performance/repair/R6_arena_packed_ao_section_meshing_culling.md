# 任務 15-R6：GPU arena lifecycle、packed AO、section meshing 與 culling

> 對應計畫：`15_performance_audit_repair_plan.md` 第 3.4、3.5、3.6 節
> 狀態：待修復
> 前置：R5（instrumentation 與 sim 修復）
> 目標：統一維度 teardown `render_regions`、為 allocation handle 加 region identity/slot/generation/owner 防止 stale/double-free、修正 AO decode 與 CPU 一致、落實真正 16³ section meshing，並重做 conservative section/entity culling（含真實 async LOS snapshot 與 identity 驗證）。
> Commit 訊息：`fix(perf): arena lifecycle, packed AO parity, section meshing and conservative culling`

## 相關程式碼位置（已核對）

- `src/state.rs:1551-1559` - dimension reset 未清 `render_regions`。
- `src/state.rs:4830-4837` - network reset 有清除 `render_regions`（參考）。
- `src/state.rs:1047-1052` - section meshing 所有權。
- `src/state.rs:1115-1167` - section mesh/revision/connectivity 結構。
- `src/state.rs:5725-5747` - section mesh worker 驗證。
- `src/chunk_render.rs:55-60` - AO packing codes。
- `src/chunk_render.rs:113-120` - AO decode 路徑。
- `src/world.rs:1539-1545` - CPU AO mapping。
- `src/world.rs:3691-3702` - section storage/meshing。
- `src/shader.wgsl:162-163` - shader AO decode `ao_raw / 3.0` 錯誤。
- `src/culling.rs:347-394` - `is_section_occluder` 與 occluder 判定。
- `src/culling.rs:406-493` - async LOS worker 永遠 visible。

## 已確認的基線風險

- 維度切換時 `render_regions` 未清（`src/state.rs:1551-1559`），舊 region allocations/buffers 洩漏。
- allocation handle 無 owner/generation，stale/double-free 可破壞 allocator。
- shader AO decode 用 `ao_raw / 3.0`（`src/shader.wgsl:162-163`），與 CPU packed codes `3/2/1/0 -> 1.0/0.75/0.5/0.25` 不一致。
- section meshing 實際仍是整個 `16×256×16` Chunk，未真正切 16³ section。
- async LOS worker（`src/culling.rs:406-493`）沒有地形資料且永遠回 visible；stale graph 未 fail-open；透明/非完整模型可被錯當 occluder。

## 子任務清單

### 6.1 統一維度 teardown render_regions
- [ ] 檔案：`src/state.rs`
- 步驟：
  1. dimension switch、disconnect、world reset、unload 時統一 teardown `render_regions`。
  2. `src/state.rs:1551-1559` 的 reset 路徑補上 `render_regions` 清除，與 `src/state.rs:4830-4837` 一致。
  3. teardown 釋放所有 region buffer object 與 allocation handle。
  4. teardown 後 F3 buffer objects 歸零。
- 驗收：維度往返後舊 region allocations/buffers 歸零。

### 6.2 allocation handle region identity/generation
- [ ] 檔案：`src/state.rs`、`src/chunk_render.rs`
- 步驟：
  1. allocation handle 增加 region identity、slot、generation/owner 欄位。
  2. `free` 驗證 bounds、generation、overlap；double-free 返回錯誤而非損壞 allocator。
  3. stale handle（generation 不符）free 視為 no-op 並記錄。
  4. handle 驗證失敗不破壞 allocator 內部結構。
- 驗收：stale/double-free/out-of-bounds handle 測試不破壞 allocator。

### 6.3 used/free checked arithmetic + compact
- [ ] 檔案：`src/state.rs`、`src/chunk_render.rs`
- 步驟：
  1. used/free counters 使用 checked arithmetic，溢位回錯而非 wrap。
  2. fragmentation threshold 達到時觸發低優先 compact。
  3. compact 不可在 gameplay frame 同步重建整個 region；排到低優先背景。
  4. 隨機 allocate/free/compact property test 保持無重疊、used+free=capacity。
- 驗收：property test 無重疊、used+free=capacity 恆成立。

### 6.4 F3 顯示實際 buffer objects
- [ ] 檔案：`src/state.rs`
- 步驟：
  1. F3 顯示實際建立的 buffer objects，而非 `render_regions.len() * 2` 估算。
  2. 從 allocator 統計實際 alive buffer 數。
  3. 維度往返後 buffer objects 歸零。
- 驗收：F3 buffer objects 反映實際 allocation。

### 6.5 AO decode 與 CPU 一致
- [ ] 檔案：`src/shader.wgsl`、`src/chunk_render.rs`、`src/world.rs`
- 步驟：
  1. `src/shader.wgsl:162-163` 移除 `ao_raw / 3.0`，改用離散 mapping：packed codes `3/2/1/0` -> `1.0/0.75/0.5/0.25`。
  2. 確認 `src/chunk_render.rs:55-60` packing 與 `src/world.rs:1539-1545` CPU mapping 一致。
  3. `src/chunk_render.rs:113-120` decode 路徑對齊。
  4. 加入 CPU packing ↔ WGSL decode parity test/golden render。
- 驗收：CPU packing 與 WGSL decode parity test/golden 通過。

### 6.6 section meshing 所有權與 halo
- [ ] 檔案：`src/state.rs`、`src/world.rs`、`src/chunk_render.rs`
- 步驟：
  1. mesh ownership 改成 section：16³ section mesh/revision/connectivity，18³ halo snapshot。
  2. mutation 只 dirty 本 section 及必要 halo neighbors（與 R1.2 一致）。
  3. `src/state.rs:1047-1052, 1115-1167` 結構改為以 section 為單位。
  4. LOD residency 與 priority 以 section/revision 管理。
  5. `src/state.rs:5725-5747` worker result 驗證 dimension generation、chunk lifetime、section revision。
  6. `src/world.rs:3691-3702` section storage/meshing 落實 16³。
  7. 未完成 section storage 前，不得在任務 11/13 文件宣稱 section remesh/draw culling 完成。
- 驗收：meshing 以 16³ section 為單位，mutation 只 dirty 受影響 section 與 halo。

### 6.7 conservative is_section_occluder
- [ ] 檔案：`src/culling.rs`
- 步驟：
  1. `src/culling.rs:347-394` `is_section_occluder` 只接受完整、實心、opaque cube。
  2. glass、ice、fluid、leaves、cutout、cross/thin/custom model 一律 fail-open。
  3. 每種 translucent/cutout/special model 都不造成 false cull。
- 驗收：透明/非完整模型不被錯當 occluder，無 false cull。

### 6.8 mesh dirty 立即 invalid connectivity
- [ ] 檔案：`src/culling.rs`、`src/state.rs`
- 步驟：
  1. mesh dirty 時立即 invalid 對應 section connectivity graph（與 R1.3 一致）。
  2. 新 revision graph 回來前視為全可見（fail-open）。
  3. stale dimension/revision result 不可寫 cache。
- 驗收：拆牆/開門後同一幀先 fail-open，之後更新 graph。

### 6.9 async LOS snapshot 與 identity 驗證
- [ ] 檔案：`src/culling.rs`
- 步驟：
  1. `src/culling.rs:406-493` async LOS request 必須攜帶最小 immutable voxel snapshot、dimension/generation/chunk revisions、camera cell 與 entity identity。
  2. worker 使用 snapshot 做真實 LOS，不再永遠回 visible。
  3. poll 只接受全部 identity 相符結果；stale/timeout/overflow 一律 visible。
  4. 快速轉身、teleport、queue overflow、未載入 section 均不會永久消失。
- 驗收：牆後 entity 穩定 cull；stale/timeout/overflow 一律 visible，無永久消失。

### 6.10 section-level mesh 存在才宣稱 section skip
- [ ] 檔案：`src/culling.rs`
- 步驟：
  1. 只有 section-level mesh/handle 存在後才宣稱 terrain section 被 skip。
  2. 否則只能算 whole-chunk coarse culling。
  3. 與 R6.6 section meshing 落實同步。
- 驗收：section skip 僅在 section mesh/handle 存在時生效。

### 6.11 culling counters
- [ ] 檔案：`src/culling.rs`、`src/state.rs`
- 步驟：
  1. 加入 culling counters：distance、frustum、section、LOS、fail-open、stale result。
  2. F3 顯示各類 culling 計數。
- 驗收：culling counters 齊全且可觀測。

### 6.12 culling/arena 整合測試
- [ ] 檔案：`src/culling.rs`（測試模組）、`src/state.rs`（測試模組）
- 步驟：
  1. 牆後 entity 穩定 cull；拆牆後 fail-open 再更新 graph。
  2. stale dimension/revision result 不可寫 cache。
  3. 每種 translucent/cutout/special model 不造成 false cull。
  4. 維度往返後 buffer objects 歸零；allocator property test。
  5. AO parity golden test。
- 驗收：全部 culling/arena 整合測試通過。

## 驗收條件

- [ ] 維度往返後舊 region allocations/buffers 歸零。
- [ ] stale/double-free/out-of-bounds handle 測試不破壞 allocator；used+free=capacity。
- [ ] F3 顯示實際 buffer objects。
- [ ] AO decode 與 CPU 一致；CPU packing ↔ WGSL decode parity test/golden 通過。
- [ ] meshing 以 16³ section 為單位，mutation 只 dirty 受影響 section 與 halo。
- [ ] `is_section_occluder` 只接受完整實心 opaque cube，其餘 fail-open。
- [ ] async LOS 攜帶 snapshot 與 identity，stale/timeout/overflow 一律 visible。
- [ ] section-level skip 僅在 section mesh/handle 存在時生效。
- [ ] culling counters 齊全。

## 風險與回退

- section meshing 改動範圍大；可先以 section 結構並存，逐步遷移 worker，再移除整 Chunk 路徑。
- AO parity 若 shader 離散 mapping 影響視覺，以 golden render 比對新舊，確認僅修正錯誤而非改變美術。
- async LOS snapshot 增加記憶體；snapshot 採最小 voxel 集合，用完即釋。
- compact 同步風險；先以背景排程，必要時退回延遲 compact 並標記 fragmentation。

## 驗證命令

```text
cargo fmt --all -- --check
cargo test --all-targets
cargo test --all-targets --release
cargo build --release
cargo clippy --all-targets --all-features
cargo test --release -- arena occluder los culling section_meshing ao_parity   # R6 fault-injection/golden/整合測試
```
