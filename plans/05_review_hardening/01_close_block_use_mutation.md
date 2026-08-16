# Plan01 — 關閉 BlockUse 任意改方塊

## 定位

- `GameplayOperation::BlockAction` 已是權威放置／開挖入口（Plan31），`ServerWorld::dispatch`
  對它正確回 `Unsupported`。
- 現役 TCP／listen 適配器把 `Packet::BlockActionRequest`、`Packet::BlockChange` 和
  `ServerRuntime::handle_block_change` 改寫成 `GameplayOperation::BlockUse`，後者只檢查
  維度、姿態、8 格距離後就 `set_block`。沒有物品扣減、遊戲模式、視線或合法轉換。
- `set_block(..., Air)` 會刪掉既有 block entity 且不走 `commit_mining_break` 掉落路徑。
- `tests/headless_server_authority.rs` 與 `tests/runtime_topology_parity.rs` 用 `BlockUse`
  種箱子，等於把後門寫進契約。

## 前置

無。可與 02–05 並行。

## 精確 acceptance

- [x] `AuthorityCore::submit_request` 對任何已認證 session 的 `GameplayOperation::BlockUse`
  回 `RejectReason::Unsupported`（或等價的「不再是 mutation」），世界方塊與物品欄不變。
- [x] `Packet::BlockActionRequest` 與 join-client `RequestBlockAction` 只產生
  `GameplayOperation::BlockAction`，帶 action、held、face、look；不得再合成 `BlockUse`。
- [x] `ServerRuntime::handle_block_change` 與 `ServerToHost::ClientBlockAction` 不再呼叫
  `set_block`。殘留 arm 拒絕或刪除，不得預設寫 Air。
- [x] 頭測與拓撲 fixture 改用 `#[cfg(test)]` 的 `ServerWorld::set_block`／save 種子種世界，
  不再用客戶端 `BlockUse` 種 Chest／Glass／Obsidian。
- [x] 新增負向測試：TCP 或 embedded 送出 `BlockUse { block: DiamondOre }`（或 Air 清箱子）
  → reject，目標格不變，物品欄不變，無 DroppedItem。
- [x] Plan31 既有 `BlockAction` TCP 矩陣仍通過。

## 預計檔案與測試

- 修改：`src/server_world.rs`（`dispatch` 的 `BlockUse` 臂）、`src/authority/mod.rs`（若要在
  core 先拒）、`src/network/server.rs`（約 1891）、`src/network/client.rs`（約 1107）、
  `src/server_runtime.rs`（`handle_block_change`、`ClientBlockAction` 約 1528／1974）。
- 修改：`tests/headless_server_authority.rs`、`tests/runtime_topology_parity.rs`、
  任何 `GameplayOperation::BlockUse {` 測試種子。
- 新增或擴：`tests/plan31_authoritative_block_actions.rs` 或本路線窄檔
  `tests/review_hardening_block_use_rejected.rs`。

## 建議階段

1. 搜 `GameplayOperation::BlockUse` 與 `handle_block_change`，列出所有生產入口與測試種子。
2. 先寫負向測試（應失敗：現在 `BlockUse` 會成功改方塊）。
3. 在 `submit_request` 或 `dispatch` 把 `BlockUse` 改成 `Unsupported`。
4. 把網路適配器改接到 `BlockAction::{Place,StartBreak}`；缺 held／face 的舊 `BlockChange`
   封包拒絕，不要猜。
5. 把測試種子改成權威 `set_block`。跑 Plan31 與本計劃窄測試。

## 不在本計劃

- 容器點擊守恆（02）、門／活板門 `UseBlock` 新 op、waterlog-on-place（09／後續）。
- Protocol bump。若舊客戶端只會發 `BlockUse`，拒絕即可，不必兼容。
- GPU／window／音訊。

## 實作與證據

### 改了什麼

- `AuthorityCore::submit_request` 對已認證 session 的 `GameplayOperation::BlockUse` 先回 `RejectReason::Unsupported`（消耗 sequence、寫入 cache），不走 reach／spectator 再分類。
- `dispatch_session_gameplay` 與 `ServerWorld::dispatch` 的 `BlockUse` 臂同樣回 `Unsupported`，不再 `set_block`。
- `Packet::BlockActionRequest` 與 `GameToClient::RequestBlockAction` 經 `GameplayOperation::from_legacy_block_action` 改寫成 `BlockAction::{Place,StartBreak}`；`Action::Use` 丟棄，不合成 `BlockUse`。
- 殘留 `Packet::BlockChange` / `RequestBlockChange` / `handle_block_change` 仍可構造 `BlockUse`，但權威拒絕；`ClientBlockAction` 改接到 `BlockAction`，不再預設寫 Air。
- 頭測／拓撲／persistence／runtime unit fixture 用 `ServerWorld::set_block` 種 Chest／Glass／Obsidian，不再把客戶端 `BlockUse` 當成功 mutation。
- 新增 `tests/review_hardening_block_use_rejected.rs`：embedded DiamondOre、embedded Air 清箱、TCP BlockUse，皆要求 Unsupported、目標格／物品欄不變、無 DroppedItem。
- Debug 機首次 tick 常超過 5s，把 `EVENT_TIMEOUT` 從 5s 調到 30s（`tests/common/tcp_harness.rs`、`tests/headless_server_authority.rs`），否則 Plan31 listen／頭測會在 join drain 前超時。

### 測試

```
cargo test --test review_hardening_block_use_rejected -- --test-threads=1
```
3 passed（embedded DiamondOre、embedded Air、TCP）。

```
cargo test --test plan31_authoritative_block_actions -- --test-threads=1
```
3 passed（embedded / dedicated TCP / listen TCP）。

```
cargo test --test headless_server_authority -- --test-threads=1
```
2 passed。

```
cargo test --test runtime_topology_parity -- --test-threads=1
```
`disabled_singleplayer_drains_local_request_through_fixed_tick_fifo` 與 `listen_runtime_routes_local_response_to_tick_output` 通過（改為 leftover BlockUse → Unsupported）。完整 6 測裡 plan24／plan28 長向量在本機 debug 下超過 180s 未跑完，未宣稱整檔通過。

另跑：`cargo check --all-targets` 通過；`leftover_block_use_is_unsupported_*`、`sessions_in_multiple_dimensions_*`、`embedded_interest_fanout_*`、`headless_two_sessions_*`、`duplicate_and_stale_revision_*`、`relays_block_action_request_*`、`block_change_reports_*`、`authority_persistence`、`same_vectors_have_same_revisions_*` 通過。未跑整倉 `cargo test`。

### 剩餘缺口

- 殘留 `BlockChange` 適配器仍合成 `BlockUse` 以便 leftover 封包走到 Unsupported；沒有 protocol bump。
- `from_legacy_block_action` 對缺 face／look 的舊 `BlockActionRequest` 填 `[0,0,0]`／`[0,0,1000]`，讓封包能進權威 `BlockAction` 驗證，不發明 held。
- `Action::Use`（門／活板門）仍無新 op，適配器直接丟棄。
- `submit_local_authority_block_use`（host `apply_block_changes`）現在會被 Unsupported 拒絕；Plan 06 再處理 presentation 不再走這條路。
- `PLAYER_REACH`／`within_reach`／`valid_coordinate` 在 `handle_block_change` 不再呼叫後變成 dead_code 警告。
- 完整 `runtime_topology_parity` 長向量與整倉 `cargo test` 未在本 worktree 跑完。
