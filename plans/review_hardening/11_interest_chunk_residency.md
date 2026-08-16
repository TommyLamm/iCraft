# Plan11 — Interest 驅動 chunk 駐留

## 定位

- `ARCHITECTURE.md`：per-session interest 同時是投影邊界**與** chunk 物質化門檻。
- 實作：`ServerWorld::ensure_chunk` 只 insert，伺服器路徑沒有 `chunks.remove`。
  `set_block`、`safe_spawn_y`、Nether 傳送門連結、加入時每 tick 16 個 chunk 的投影器
  都會呼叫它。`tick` 對**所有**已載入 column 跑紅石／流體／隨機刻／熔爐／實體。
- 探索過的世界永遠留在 RAM，每 6000 tick 整包重存。`loaded_chunks` 報的是這個無界集合。
- 既有測試證明可視邊界**可以** on-demand 載入，不證明視野外**不會**物質化。

## 前置

無。建議 05 已合併，這樣 evict 前 flush 不會把「restore 失敗的生成 chunk」寫盤。
若 05 未合併：evict 只允許 flush **成功 restore 過或本 session 新生成且 dirty** 的 column。

## 精確 acceptance

- [ ] 離開**所有** session 的 simulation／view 集（加上與客戶端相同的滯後）的 column，
  在 flush dirty 之後從 `ServerWorld.chunks` 移除。Spawn `(0,0)` 可保留一圈，但必須有上限。
- [ ] 隨機刻／流體／hopper／熔爐只走 `simulation_chunks`（或等價的 interest 聯集），
  不走 `chunks.keys()` 全表。
- [ ] `ensure_chunk` 不得在 interest／budget 之外被 tick 熱路徑呼叫，除了傳送門連結
  需要的成對 column（寫明例外並測：連結後若無人在場，仍應進入 evict 候選）。
- [ ] 測試：玩家在原點 view=2，N tick 後 chunk (8,0) 不在 `world.chunks`；對該處
  `BlockAction` reject 且仍不 `ensure_chunk`。玩家走 32 chunk 後，原點 column 在
  下一次 evict／save 後離開 map。
- [ ] `loaded_chunks` 指標與駐留集一致。

## 預計檔案與測試

- 修改：`src/server_world.rs`（`ensure_chunk`、tick 迴圈、新 evict）、
  `src/server_runtime.rs`（interest 更新之後呼叫 evict）、`src/authority/interest.rs`
  僅在需要「simulation 聯集」helper 時。
- 測試：`src/server_runtime.rs`／`src/authority/interest.rs` 單元測；
  `tests/review_hardening_chunk_residency.rs`。

## 建議階段

1. 先加「視野外不 ensure」負向測試（15 也列了；本計劃擁有它）。
2. Tick 改讀 simulation set。
3. Evict + flush。
4. 傳送門例外。
5. 指標。

## 不在本計劃

- 桌面 mesh 佇列（14）。
- 改世界生成內容（10）。
