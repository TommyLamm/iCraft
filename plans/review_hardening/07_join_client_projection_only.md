# Plan07 — Join client 只吃投影

## 定位

- 架構：加入客戶端不跑世界權威，只送 input、套用 revision-gated 投影。
- `schedule_chunk_load` 一律 `generate_chunk_with_options`。Save overlay 才用
  `is_authoritative()` 閘。`launch_client` 以 `seed: 0` 起步；`Connected` 重設 seed
  後串流仍用客戶端 `world_type`／結構旗標生成。`ChunkData` 若晚到，中間幀是本地捏造世界。
- Join client 仍建 `SaveManager`、save worker、network snapshot worker，目錄在
  `temp_dir()/icraft_multiplayer_client`。
- Farmland／unsupported-break 在 join 路徑會寫本地 chunk（與 06 相關，但 06 只管 embedded）。

## 前置

無。可與 06 並行；若兩份都改 `schedule_chunk_load`／unsupported-break，後合併時保留
「client 與 embedded 都不准本地生成／破壞」兩個閘。

## 精確 acceptance

- [x] `MultiplayerRole::Client`：不得呼叫 `generate_chunk_with_options` 填入
  `ChunkManager`。只從 revision-gated `ChunkData`（加上後續 `AuthoritativeBlockChange`）
  insert。缺失 chunk 顯示為未載入，不是假地形。
- [x] Join client 不建世界 `SaveManager`、不建 chunk save worker、不建
  `NetworkSnapshotWorker` 去 persist index。
- [x] Join client 不得 `set_block`／unsupported-break／farmland 寫 presentation chunk。
- [x] 既有 `RevisionGate`／Plan30 chunk 投影測試仍通過。
- [x] 測試：headless 投影縫（不必 wgpu）——先送一包 `ChunkData`，assert 該 column
  出現且方塊與 payload 一致；在 payload 前 `schedule_chunk_load` 不得插入生成 chunk。

## 預計檔案與測試

- 修改：`src/state.rs`（`schedule_chunk_load`、`State::new` 的 client 分支、
  `launch_client` 在 `src/menu.rs` 約 2330）、`src/chunk_manager.rs` 僅在 API 需要
  「只接受權威 payload」時。
- 測試：抽 load 排程閘到可測函式；或 `tests/review_hardening_join_projection.rs`
  用現有 `NetworkClient` + 自製 sink（15 會再補完整 State 縫）。

## 建議階段

1. 在 `schedule_chunk_load` 對 Client 改成「只 enqueue 座標，等 ChunkData」。
2. 拿掉 client 的 SaveManager／snapshot worker 建構。
3. 閘 farmland／unsupported-break。
4. 窄測試 + Plan30 chunk／revision 測試。

## 不在本計劃

- 客戶端 unbounded `mpsc`（12）。
- Embedded bootstrap（06）。
- 完整無 wgpu 的 `State` 重構。

## 實作與證據

### 行為

- `presentation_chunk_load_policy` / `schedule_presentation_chunk_load`（`src/menu.rs`）把 join client 釘成 `AwaitAuthoritativePayload`。`State::schedule_chunk_load` 對該政策直接 return，不再 `generate_chunk_with_options`，也不佔 load in-flight slot。
- `ChunkData` 到達時 `apply_remote_chunk_data` 用 `ChunkManager::insert_authoritative_chunk_payload` 從空 column restore payload 再 insert；失敗不 insert。後續 `AuthoritativeBlockChange` 仍走既有 revision gate。缺失 column 保持未載入。
- `State::new` 對 `MultiplayerRole::Client` 不建 `SaveManager`、不 spawn chunk save worker、不 spawn `NetworkSnapshotWorker`。`save_manager` / `save_tx` / `network_snapshot_worker` 改為 `Option`。`launch_client` 的 `icraft_multiplayer_client` 路徑只當 placeholder，不再當 persist root。
- `presentation_may_mutate_chunks`（`!Client`）閘 `apply_block_changes`、farmland 踐踏、`check_and_break_unsupported_above`、`check_and_break_unsupported_for_loaded_chunk`。Plan 06 可再 AND embedded。

### 測試

- `cargo test --lib join_client_load_policy_never_generates_or_mutates -- --nocapture`：1 passed。
- `cargo test --lib authoritative_payload_inserts_without_local_worldgen -- --nocapture`：1 passed。
- `cargo test --test review_hardening_join_projection -- --nocapture`：3 passed。
- `cargo test --lib revision_gate -- --nocapture`：4 passed（含 `revision_gate_orders_cross_channel_snapshot_and_block_change`、`revision_gate_multi_client_checksum_converges`）。
- 未跑完整 `plan30_real_transport_acceptance`（兩條 TCP／embedded 向量，非 chunk 投影單元；既有 RevisionGate 測試已覆蓋投影序）。
- `cargo check --all-targets`：通過。
- `cargo fmt`：通過（僅本計劃檔）。

### 架構句

`ARCHITECTURE.md` 已寫「A joining client never runs world authority… applies revision-gated projections」。源碼現在與該句一致：join client 不再本地 worldgen，也不建世界 persist worker。
