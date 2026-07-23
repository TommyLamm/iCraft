# 實作計畫 04：禁止方塊放進玩家碰撞體

## 目標

任何 solid 方塊都不能放在本地或遠端玩家 AABB 內；剛好面接觸允許；
client 可預檢，但 host 必須做最終權威驗證，避免放置後把玩家擠開。

## 已確認根因

- 本地 `handle_click`、client request 與 host `set_block_and_broadcast` 都沒有
  occupancy 驗證。
- host inbound 轉換丟失原本已驗證的 player id。
- solid block 出現在玩家 AABB 中後，`resolve_collisions` 會把玩家推出。

## 實作步驟

1. 在 `physics.rs` 抽 `player_aabb_at` 和 `unit_block_aabb`，統一玩家尺寸。
2. 增加純 policy：只有 `properties().is_solid` 且與任何 player AABB 有
   正體積重疊時拒絕；面/邊/角剛好接觸不拒絕。
3. `State::can_place_block_at` 使用 local current AABB 與 remote
   `latest.position`，不能使用延遲插值的 render entity position。
4. local authoritative 放置在扣物品/播放聲音/改 world 前驗證。
5. joined client 發 request 前做同 policy 的 UX 預檢。
6. 保留 `ClientBlockChange` 的 authenticated player id，host 再做權威 occupancy
   驗證；client 套用 host block change 時不重驗，以免分歧。
7. 不把 Torch 等 non-solid 方塊錯誤擋掉。
8. 更新架構與進度文件。

## 驗證

- local/remote overlap 拒絕；feet 正好在 block top、側面接觸允許。
- non-solid Torch 同格允許。
- host inbound player id 不再遺失。
- rejected placement 不修改、不扣物品、不 broadcast；移開後成功一次。
- `cargo fmt -- --check`、`cargo test --release`、`cargo check --release`。
- 人工單人與 Host/Join 嘗試放腳下、頭部、另一玩家身上。

## Commit

單一功能 commit：`fix(blocks): reject placement inside players`

