# Plan05 — 刪 presentation `RedstoneSystem` 空殼

## 定位

桌面 `State` 仍持有 `redstone: RedstoneSystem` 與 `redstone_tick_timer`。Live 紅石只在 `ServerWorld` tick（`server_world.rs`）。Presentation 路徑：

- `self.redstone` 僅 init／reset + `restore_chunk_metadata`。
- `redstone.tick` **從不**在 `state.rs` 呼叫。
- Worker／chunk load 永遠送 `redstone_metadata: Vec::new()`。
- Plan 03 刪掉 `pending_redstone_metadata` 空迴圈後，這整條 restore 管線沒有輸入。

`ChunkLoadResult.redstone_metadata` 與 `schedule_chunk_load` 相關欄位是空殼。

## 前置

03（不可達啟動與 `pending_redstone_metadata` 已刪）。不要在 03 未完成時砍 `RedstoneSystem`，以免漏掉啟動路徑仍寫 metadata 的錯覺。

## 精確 acceptance

- [ ] `State` 不再持有 `RedstoneSystem`／`redstone_tick_timer`。
- [ ] `ChunkLoadResult` 不再帶 `redstone_metadata`；worker 不再分配空 Vec。
- [ ] `restore_chunk_metadata` 若只被 presentation 呼叫，改 `pub(crate)` 測試專用或刪 presentation 呼叫點（權威仍用 `RedstoneSystem`）。
- [ ] Join／embedded 方塊 facing 仍只靠 `ChunkData`／`BlockChange`／block-entity 投影，不依賴 client 端 redstone restore。
- [ ] `cargo test --bin icraft` 中 chunk load／mesh 相關測試通過。

## 預計檔案與測試

- `src/state.rs`、`src/redstone.rs`（只動 presentation 呼叫點，不改權威 tick）
- `src/chunk_schedule.rs` 若承載 metadata
- 驗證：`tests/review_hardening_embedded_presentation.rs`；`cargo test --lib redstone::`

## 建議階段

1. Grep `restore_chunk_metadata`／`redstone_metadata` 的 **presentation** caller。
2. 刪 State 欄位與 load result 欄位。
3. 確認 `RedstoneSystem` 仍由 `ServerWorld` 使用。

## 不在本計劃

- 改權威紅石 sleep／comparator index（Wave 08 已做 sleep；awake 全表掃描留給後續）。
- 為 client mesh 重接 facing metadata sidecar。
