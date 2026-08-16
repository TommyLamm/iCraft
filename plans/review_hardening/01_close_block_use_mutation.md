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

- [ ] `AuthorityCore::submit_request` 對任何已認證 session 的 `GameplayOperation::BlockUse`
  回 `RejectReason::Unsupported`（或等價的「不再是 mutation」），世界方塊與物品欄不變。
- [ ] `Packet::BlockActionRequest` 與 join-client `RequestBlockAction` 只產生
  `GameplayOperation::BlockAction`，帶 action、held、face、look；不得再合成 `BlockUse`。
- [ ] `ServerRuntime::handle_block_change` 與 `ServerToHost::ClientBlockAction` 不再呼叫
  `set_block`。殘留 arm 拒絕或刪除，不得預設寫 Air。
- [ ] 頭測與拓撲 fixture 改用 `#[cfg(test)]` 的 `ServerWorld::set_block`／save 種子種世界，
  不再用客戶端 `BlockUse` 種 Chest／Glass／Obsidian。
- [ ] 新增負向測試：TCP 或 embedded 送出 `BlockUse { block: DiamondOre }`（或 Air 清箱子）
  → reject，目標格不變，物品欄不變，無 DroppedItem。
- [ ] Plan31 既有 `BlockAction` TCP 矩陣仍通過。

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
