# 任務 15-R7：render scratch、hand、instancing 與 paletted demotion

> 對應計畫：`15_performance_audit_repair_plan.md` 第 4.1、4.2 節
> 狀態：待修復
> 前置：R6（arena/section meshing/culling 修復）
> 目標：把 `visible_entities`、UI textured vertices、debug labels、uppercase-format 移到可重用 scratch/cache 並以實際 instrumentation 驗證零配置；hand 基礎 mesh 只在 held item/model 改變時重建；instance ring 加 GPU completion 保護；paletted storage 適時 demote 並以真實 representation 計算 memory counter；建立可執行 microbench。
> Commit 訊息：`fix(perf): reuse render scratch, hand mesh timing, instance fence and paletted demotion`

## 相關程式碼位置（已核對）

- `src/state.rs` - `visible_entities`、render prepare、UI textured vertices、debug labels 路徑。
- `src/state.rs:12963-12997` - render encode 與 allocation 相關路徑。
- `src/chunk_render.rs` - draw plan 與 instance ring。
- `src/world.rs` - `ChunkSection` representation、Block/Light storage。
- `src/render.rs`（或對應 hand render 模組） - hand mesh 重建與動畫。
- `src/particle.rs`（或對應模組） - `ParticleInstance` 結構。

## 已確認的基線風險

- 每幀仍建立 entity/UI/String buffers；`visible_entities`、debug labels、uppercase/format results 無可重用 scratch。
- 無實際 allocation instrumentation，只靠手工 counter 推定零配置。
- hand animation 仍重建並上傳 CPU geometry，而非只動 transform/uniform。
- instance ring 只固定三槽輪轉，無 queue completion/fence 保護，可能覆寫 in-flight buffer。
- `ParticleInstance` 缺計畫中的 color/light。
- paletted storage 不 demote，全零 state/fluid 仍佔 allocation；`ChunkSection` representation 外部可直接 match；memory counter 不反映實際 representation；無 microbench。

## 子任務清單

### 7.1 render scratch 重用
- [ ] 檔案：`src/state.rs`、`src/chunk_render.rs`
- 步驟：
  1. `visible_entities`、UI textured vertices、debug labels、uppercase/format results 移到可重用 scratch/cache，每幀 `clear` 重用。
  2. scratch 容量隨峰值成長，不每幀重新分配。
  3. 確認 scratch 不跨幀殘留邏輯資料（只重用容量）。
  4. terrain candidates、draw plan、LOD、mob、particle、UI storage 一併重用。
- 驗收：穩態畫面下 render prepare 路徑零配置（以 instrumentation 驗證）。

### 7.2 實際 allocation instrumentation
- [ ] 檔案：`src/state.rs`、測試 allocator
- 步驟：
  1. 建立實際 allocation instrumentation 或測試 allocator，不能只靠手工 counter 推定零配置。
  2. 在熱路徑包裝 `Vec`/`String` 配置計數，超過門檻記錄配置點。
  3. 穩態畫面 instrumentation 斷言零（或 bounded）配置。
  4. F3 可顯示每幀配置計數。
- 驗收：穩態畫面配置計數為零（或 bounded），有可重現量測。

### 7.3 hand mesh 重建時機
- [ ] 檔案：hand render 模組、`src/state.rs`
- 步驟：
  1. hand 基礎 mesh 只在 held item/model 改變時重建。
  2. walk/attack swing 動畫移到 transform/uniform，不重建 CPU geometry。
  3. 動畫資料以 uniform/update 上傳，不重新產生頂點。
  4. 切換手持物品才觸發 mesh 重建。
- 驗收：揮動/走路時 hand mesh 不重建 CPU geometry。

### 7.4 instance ring GPU completion
- [ ] 檔案：`src/chunk_render.rs`、`src/state.rs`
- 步驟：
  1. instance ring 使用 queue completion/fence 或足夠大的 staging belt，不只固定三槽輪轉。
  2. 覆寫 ring slot 前確認該 slot GPU 已完成。
  3. 多 entity/particle instance 共用 ring，依 completion 回收 slot。
  4. ring 上限由 GPU frame 數決定，避免無限成長。
- 驗收：instance ring 不覆寫 in-flight buffer；高 entity 數穩定。

### 7.5 ParticleInstance color/light
- [ ] 檔案：particle 模組、`src/chunk_render.rs`
- 步驟：
  1. `ParticleInstance` 補齊 color/light 欄位，或明確修訂 plan 與視覺基準。
  2. 粒子 instance 携带 color/light，與計畫一致。
  3. 若不補 color/light，修訂計畫並記錄視覺基準差異。
- 驗收：`ParticleInstance` 與計畫一致或已修訂 plan 並記錄基準。

### 7.6 paletted demotion 釋放
- [ ] 檔案：`src/world.rs`、`src/state.rs`
- 步驟：
  1. state/fluid 全部歸零後釋放 optional allocation。
  2. Block/Light storage 在適當時機由 Global/Paletted/Packed demote 回 Uniform/Empty。
  3. 避免每次 set 做昂貴掃描，用 counts/dirty compaction policy。
  4. demotion 觸發由 counts 變化決定，非每 set 掃描。
- 驗收：全零 state/fluid 釋放 allocation；storage 適時 demote。

### 7.7 ChunkSection representation 私有
- [ ] 檔案：`src/world.rs`
- 步驟：
  1. 將 `ChunkSection` representation 收斂為私有，外部只用 access/query API。
  2. 移除外部直接 match storage kind 的路徑。
  3. 提供 `get_block`/`set_block`/`get_light`/`set_light` 等統一 API。
  4. 內部 representation 變更不影響外部呼叫端。
- 驗收：外部僅透過 access/query API 使用 `ChunkSection`。

### 7.8 memory counter 真實 representation
- [ ] 檔案：`src/world.rs`、`src/state.rs`
- 步驟：
  1. memory counter 按實際 palette、packed indices、optional arrays、light storage 與 container overhead 計算。
  2. demotion 後 counter 反映釋放。
  3. F3 顯示實際 representation 記憶體。
- 驗收：memory counter 反映實際 representation，demotion 後下降。

### 7.9 可執行 microbench
- [ ] 檔案：新增 microbench 模組（`benches/` 或測試模組）
- 步驟：
  1. 建立 microbench 覆蓋 `get/set`、lighting、collision、meshing 與 save/network flatten。
  2. 比較 Global/Paletted/Packed/Uniform representation 各路徑耗時。
  3. microbench 結果可重現，納入報告。
- 驗收：microbench 可執行且結果可重現。

### 7.10 scratch/instancing/paletted 整合測試
- [ ] 檔案：`src/state.rs`（測試模組）、`src/world.rs`（測試模組）
- 步驟：
  1. 穩態畫面 allocation instrumentation 斷言零/bounded 配置。
  2. hand 動畫不重建 mesh 斷言。
  3. instance ring completion 保護測試。
  4. paletted demotion 釋放與 memory counter 一致測試。
- 驗收：全部整合測試通過。

## 驗收條件

- [ ] `visible_entities`、UI textured vertices、debug labels、uppercase/format 移到可重用 scratch。
- [ ] 實際 allocation instrumentation 驗證穩態零/bounded 配置。
- [ ] hand 基礎 mesh 只在 held item/model 改變時重建；動畫移到 transform/uniform。
- [ ] instance ring 使用 queue completion/fence 或足夠 staging belt，不覆寫 in-flight。
- [ ] `ParticleInstance` 補 color/light 或修訂 plan 並記錄基準。
- [ ] state/fluid 全零釋放 allocation；Block/Light storage 適時 demote。
- [ ] `ChunkSection` representation 私有，外部只用 access/query API。
- [ ] memory counter 反映實際 representation。
- [ ] microbench 覆蓋 get/set、lighting、collision、meshing、flatten 且可重現。

## 風險與回退

- scratch 重用若殘留邏輯資料造成跨幀 bug；以 `clear` 不 `truncate` 並測試斷言長度歸零。
- instance ring completion 需 GPU fence 支援；若 backend 不支援，以足夠大 staging belt 保守輪轉。
- paletted demotion 觸發頻繁可能反向增加 CPU；以 counts/dirty policy 控制觸發時機。
- `ChunkSection` 私有化影響面廣；先加 API，逐步遷移呼叫端，最後改 visibility。

## 驗證命令

```text
cargo fmt --all -- --check
cargo test --all-targets
cargo test --all-targets --release
cargo build --release
cargo clippy --all-targets --all-features
cargo test --release -- allocation_scratch hand_mesh instance_ring paletted_demotion microbench   # R7 與 microbench 測試
```
