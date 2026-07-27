# 實作計畫 11：Host 遠端方塊請求 Reach 驗證

> 來源：Task 10 後續項 N3。

## 狀態

⏳ 待實作

## 目標

Host 收到 joined client 的方塊放置／破壞請求時，必須驗證目標方塊在請求
玩家的權威位置的可達距離內，防止惡意或不同步的 client 修改遠處方塊。

## 已確認根因

- `State::set_block_and_broadcast`（`src/state.rs:6324`）是 host 處理
  `ClientBlockChange` 的唯一入口。
- 目前只驗證：(1) requester 是已知 remote player、(2) `can_place_block_at`
  放置碰撞、(3) chunk 已載入、(4) `can_place_block_with_support` 支撐。
- **沒有** reach 距離檢查。client 可以對任意座標送 `RequestBlockChange`，
  host 會直接採用。
- 本地玩家的 raycast 使用 `5.0` 格的最大距離（`src/state.rs:7085`、
  `7346`）。joined client 的 `handle_click` 也以 `5.0` 做 raycast
  （`src/state.rs:7082`），但這只是 client 端 UX，host 不重驗。
- Host 擁有每個 remote player 的最新權威快照
  (`remote_players[&id].snapshots.back()`)，可用於 reach 計算。

## 實作步驟

1. 在 `src/state.rs` 定義常數 `BLOCK_REACH: f32 = 5.0`（與 raycast 一致），
   並加一小段容差（例如 `+ 1.5`）以涵蓋玩家半寬（0.3 × 2）與方塊中心
   偏移（最多 √3/2 ≈ 0.87），最終 reach 上限約 `6.5`。
2. 在 `set_block_and_broadcast` 中，`can_place_block_at` 之前加入 reach 檢查：
   - 從 `self.remote_players[&requester]` 取得最新權威位置
     (`snapshots.back()`)。若無快照，拒絕請求。
   - 計算玩家 AABB 中心到目標方塊 AABB 中心的距離。
   - 超過 `BLOCK_REACH + tolerance` 則靜默拒絕（return），不廣播、不修改。
3. 破壞請求（`block == Air`）同樣適用 reach 檢查；放置與破壞共用同一條驗證。
4. 將 reach 檢查抽成獨立純函式 `block_within_reach(
   player_pos: Vec3, block_pos: (i32,i32,i32)) -> bool`，方便單元測試。
5. 更新 `ARCHITECTURE.md` 多人段落，說明 host 對遠端方塊請求做 reach 驗證。
6. 更新 `track.md`、`plans/progress.md`、`plans/implementation/10_bug_audit.md`
   的 N3 狀態。

## 驗證

- [ ] reach 邊界：玩家中心恰在 5.0 格內可通過；超過上限被拒。
- [ ] 無快照的 requester 一律拒絕。
- [ ] 破壞（Air）與放置請求都受 reach 限制。
- [ ] 拒絕時不修改 world、不廣播、不扣物品、不播音效。
- [ ] 既有 placement collision、support 及 authenticated-id 測試不回歸。
- [ ] `cargo fmt --all -- --check`、`cargo test --release`、
      `cargo check --release`。
- [ ] 人工 Host + Join 嘗試在 reach 邊緣放置／破壞（需互動式雙視窗）。

## Commit

單一功能 commit：`fix(network): validate reach for remote block requests`
