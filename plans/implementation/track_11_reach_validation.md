# Track 11：Host 遠端方塊請求 Reach 驗證

> 對應計畫：`plans/implementation/11_reach_validation.md`（Task 10 後續項 N3）
> 狀態：⏳ 待實作
> 目標：Host 收到 joined client 的方塊放置／破壞請求時，驗證目標方塊在請求
> 玩家權威位置的可達距離內，防止惡意或不同步的 client 修改遠處方塊。
> Commit：`fix(network): validate reach for remote block requests`

## 相關程式碼位置（已核對）

- `src/state.rs:6324` `set_block_and_broadcast` — host 處理 remote block request 的唯一入口。
- `src/state.rs:6336` 目前只驗 `remote_players.contains_key(&requester)` + `can_place_block_at`。
- `src/state.rs:1743` `RemotePlayer.snapshots: VecDeque<PlayerSnapshot>`；`snapshots.back()` 為最新權威位置。
- `src/state.rs:7085` 本地玩家 raycast 最大距離 `5.0`；`src/state.rs:7082` joined client `handle_click` 也以 `5.0` raycast（僅 client UX）。
- `src/state.rs:2207` `request_block_change`（client→host 送出點）。
- `src/state.rs:6384` `apply_remote_block_change`（client 套用權威變更）。
- `src/physics.rs:44-53` `player_aabb` 公式（feet + standing height/2），reach 中心計算沿用。

## 子任務清單

### 11.1 定義 BLOCK_REACH 常數與容差
- [ ] 檔案：`src/state.rs`
- 步驟：
  1. 在 `src/state.rs` 頂部常數區定義 `const BLOCK_REACH: f32 = 5.0;`（與 raycast 一致）。
  2. 定義容差 `const BLOCK_REACH_TOLERANCE: f32 = 1.5;`，涵蓋玩家半寬（0.3×2）與方塊中心偏移（最多 √3/2 ≈ 0.87）。
  3. 最終 reach 上限 = `BLOCK_REACH + BLOCK_REACH_TOLERANCE` ≈ 6.5 格。
- 驗證：常數存在且數值正確。

### 11.2 實作 `block_within_reach` 純函式
- [ ] 檔案：`src/state.rs`
- 步驟：
  1. 新增獨立純函式（不取 `&self`），計算玩家 AABB 中心到目標方塊 AABB 中心的距離：
     - 方塊中心 = `(x+0.5, y+0.5, z+0.5)`。
     - 比較 `(player_pos - block_center).length() <= BLOCK_REACH + BLOCK_REACH_TOLERANCE`。
  2. 簽名：`fn block_within_reach(player_pos: Vec3, block_pos: (i32,i32,i32)) -> bool`。
  3. `player_pos` 由呼叫端傳入玩家 AABB 中心（非 feet）。
- 驗證：函式可獨立編譯、無副作用、可單測。

### 11.3 `block_within_reach` 單元測試
- [ ] 檔案：`src/state.rs`（`#[cfg(test)]` 區段）
- 步驟：新增測試：
  1. 玩家中心恰在 5.0 格內（距離 5.0）→ 通過。
  2. 距離 6.0（介於 5.0 與 6.5）→ 通過（容差允許）。
  3. 距離 6.5（邊界）→ 通過。
  4. 距離 6.51 → 拒絕。
  5. 距離 10.0 → 拒絕。
  6. 對角方塊（√3 ≈ 1.73）→ 通過。
- 驗證：`cargo test --release block_within_reach` 全綠。

### 11.4 在 `set_block_and_broadcast` 加入 reach 檢查
- [ ] 檔案：`src/state.rs:6324`
- 步驟：
  1. 在 `let block = ...from_wire` 之後、`can_place_block_at`（6336 行）之前插入 reach 檢查。
  2. 從 `self.remote_players.get(&requester)` 取得 `RemotePlayer`（用 `get` 避免 contains + 重取）。
  3. 取 `remote.snapshots.back()`；若 `None`（無快照）→ `return`（靜默拒絕）。
  4. 計算玩家 AABB 中心：`snapshot.position + Vec3::new(0.0, PLAYER_STANDING_HEIGHT * 0.5, 0.0)`（standing height 1.8，沿用 `physics.rs` `player_aabb` 公式）。
  5. 呼叫 `block_within_reach(center, (x, y, z))`；若 false → `return`（不廣播、不修改、不扣物品、不播音效）。
- 驗證：reach 超標時函式在 reach 檢查點提前 return。

### 11.5 破壞請求（Air）同樣適用 reach
- [ ] 檔案：`src/state.rs:6324`
- 步驟：
  1. 確認 reach 檢查位於 `can_place_block_at` 之前，對 `block == Air`（破壞）與放置共用同一條驗證路徑。
  2. 不為破壞另開路徑；放置與破壞都先過 reach。
- 驗證：破壞遠端方塊被拒；破壞近處方塊不受影響。

### 11.6 無快照 requester 一律拒絕
- [ ] 檔案：`src/state.rs`
- 步驟：
  1. 確認 11.4 步驟 3 的 `None` 分支直接 `return`。
  2. 新增測試：requester 存在於 `remote_players` 但 `snapshots` 為空 → 請求被拒、world 不變、無廣播。
- 驗證：`cargo test --release` 該測試通過。

### 11.7 Host reach 驗證整合測試
- [ ] 檔案：`src/state.rs`（測試區段）
- 步驟：以既有 host 測試模式（參考 `state.rs:12053` 附近的 `ClientBlockChange` 測試）建立：
  1. requester 在 `remote_players` 中，給近距離 snapshot → 放置成功、廣播發出。
  2. 同一 requester 給遠距離 snapshot（> 6.5 格）→ 放置被拒、`broadcast_block_change` 未被呼叫、world 不變。
  3. 破壞（Air）遠距離 → 被拒。
  4. 破壞近距離 → 成功。
  5. 未知 requester（不在 `remote_players`）→ 被拒（既有行為，確認未回歸）。
- 驗證：`cargo test --release reach` 全綠。

### 11.8 既有測試不回歸
- [ ] 步驟：
  1. 確認既有 placement collision、`can_place_block_with_support`、authenticated-id、multiplayer block sync 測試仍通過。
  2. 特別檢查 `state.rs:12065`、`12095` 既有 `ClientBlockChange` / `AuthoritativeBlockChange` 測試。
- 驗證：`cargo test --release` 全綠（預期 ≥ 310 unit + 1 integration）。

### 11.9 格式／編譯／測試閘門
- [ ] 步驟：依序執行
  1. `cargo fmt --all -- --check`
  2. `cargo check --release`
  3. `cargo test --release`
- 驗證：三者皆通過。

### 11.10 更新文件
- [ ] 檔案：`ARCHITECTURE.md`、`track.md`、`plans/progress.md`、`plans/implementation/10_bug_audit.md`
- 步驟：
  1. `ARCHITECTURE.md` 多人段落說明 host 對遠端方塊請求做 reach 驗證（常數值、容差、拒絕策略）。
  2. `track.md` 加入 Task 11 列與 working notes。
  3. `plans/progress.md` 更新日誌新增一條。
  4. `10_bug_audit.md` 將 N3 標記為已修復。
- 驗證：文件與實作一致。

### 11.11 人工驗收
- [ ] 步驟：互動式雙視窗 Host + Join，嘗試在 reach 邊緣放置／破壞。
- 驗證：邊緣內行為正常、超出被拒、無世界不同步。

### 11.12 Commit
- [ ] 步驟：單一功能 commit `fix(network): validate reach for remote block requests`，只 stage 本任務檔案。
- 驗證：`git diff --check` 通過；commit 訊息符合 repo 風格。

## 驗收條件（對應計畫驗證清單）
- [ ] reach 邊界：玩家中心恰在 5.0 格內可通過；超過上限被拒。
- [ ] 無快照的 requester 一律拒絕。
- [ ] 破壞（Air）與放置請求都受 reach 限制。
- [ ] 拒絕時不修改 world、不廣播、不扣物品、不播音效。
- [ ] 既有 placement collision、support 及 authenticated-id 測試不回歸。
- [ ] `cargo fmt --all -- --check`、`cargo test --release`、`cargo check --release`。
- [ ] 人工 Host + Join 嘗試在 reach 邊緣放置／破壞。
