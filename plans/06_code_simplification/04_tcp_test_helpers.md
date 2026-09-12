# Plan04 — TCP 測試 helper 收斂

## 定位

`tests/common/tcp_harness.rs` 已有 `HeldLoopback`、`TcpClient`、`drive_until`，註解寫明要用它關 bind TOCTOU（Windows 上 `TcpListener::bind(port 0)` 再 drop 後立刻重綁會踩 ephemeral）。

多數套件仍各自複製：

- `reserve_port`：`TcpListener::bind(...).port()` 然後 drop（`tests/plan30_real_transport_acceptance.rs` ~38、`tests/headless_server_authority.rs` ~205、數個 `review_hardening_*`）
- `drive_until`／`drive_pair_until`／`wait_for_response`
- `properties`／`gameplay_request`／`session_slot`／`held` 種子

審查硬化 08 甚至用 raw port reserve，Plan 30+ 用 `TcpClient`。改握手／身份／port 時要同時改 8+ 份複本，契約被藏在複製裡。

`review_hardening_adversarial_frames.rs` 必須繼續走 raw `TcpStream`（檔頭已說明），不要改接 `NetworkClient`。

## 前置

無。不改生產碼行為。可與所有其他計劃並行。

## 精確 acceptance

- [x] `tests/common/` 至少提供（名稱可微調，但語意必須穩定）：
  - `HeldLoopback`（已存在，繼續當 listen bind 的唯一來源）
  - `temp_world(prefix: &str) -> PathBuf`
  - `loopback_properties(world_dir, bind_addr) -> ServerProperties`（view／sim distance 與現有 fixture 一致的預設）
  - `gameplay_request(runtime, player_id, request_id, sequence, operation) -> GameplayRequest`（填齊 revision／sequence 的現役預設）
  - `session_slot(stack) -> SessionInventorySlot`
- [x] 下列檔案的 listen bind 改用 `HeldLoopback`，刪本地 `reserve_port`：
  - `tests/plan30_real_transport_acceptance.rs`
  - `tests/plan31_authoritative_block_actions.rs`
  - `tests/plan32_progression_travel.rs`
  - `tests/plan33_tcp_fishing_lifecycle.rs`
  - `tests/plan34_container_break_inventory_conservation.rs`
  - `tests/review_hardening_block_use_rejected.rs`
  - `tests/review_hardening_session_lifecycle.rs`
  - `tests/headless_server_authority.rs`（`HeadlessClient` 的 event 型別可留；只抽 port／drive／properties）
- [x] `review_hardening_adversarial_frames.rs` **不**改走 `NetworkClient`／`TcpClient`。
- [x] 領域種子（`seed_nether_frame`、`reset_persistent_domains` 等）留在各測試檔。
- [x] 測試期望值不變。不得為了共用 helper 放寬 timeout 或改 assertion，除非只是把既有常數移到 common 並保持同一數字。

## 預計檔案與測試

- 修改：`tests/common/mod.rs`、`tests/common/tcp_harness.rs`，以及 acceptance 表中的測試檔。
- 測試（`--test-threads=1`）：
  - `cargo test --test plan30_real_transport_acceptance`
  - `cargo test --test plan31_authoritative_block_actions`
  - `cargo test --test plan32_progression_travel`
  - `cargo test --test plan33_tcp_fishing_lifecycle`
  - `cargo test --test plan34_container_break_inventory_conservation`
  - `cargo test --test review_hardening_block_use_rejected`
  - `cargo test --test review_hardening_session_lifecycle`
  - `cargo test --test headless_server_authority`
  - `cargo test --test review_hardening_adversarial_frames`

Windows 上這些測試對 port／timeout 敏感。一次跑一個檔，失敗先看是否又用了 drop-then-rebind。

## 建議階段

1. 讀 `tests/common/tcp_harness.rs` 與 `tests/plan30_real_transport_acceptance.rs` 的本地 helper，列出可搬與必須留在檔內的項目。
2. 把 `temp_world`／`loopback_properties`／`gameplay_request`／`session_slot` 搬進 common，Plan30 改用它們並確認仍過。
3. 其餘 Plan31–34 與 review_hardening 檔逐個替換 `reserve_port`。
4. `headless_server_authority` 只抽 port／drive，不強行改成 `TcpClient`。
5. 跑表列測試。

## 不在本計劃

- 改生產網路協定或 ingress 預算。
- 把 `sim_harness`／`final_acceptance` 收進 `ServerRuntime`。
- 合併 `HeadlessClient` 與 `TcpClient` 成一個型別。
- 改 adversarial 測試走高階 client。

## 實作與證據

2026-08-16。只動測試 helper；未改生產網路／協定／ingress，未 commit。

`tests/common/tcp_harness.rs` 新增：

- `temp_world(prefix)` → `icraft-{prefix}-{nanos}`
- `loopback_properties(world_dir, bind_addr)`：bind + `max_players=4` + `view=2` + `sim=2`；不寫死 seed／port
- `gameplay_request(runtime, player_id, request_id, sequence, operation)`：讀 live session dim + `revision_for_dimension`
- `session_slot(stack) -> SessionInventorySlot`（`ItemWire::from_stack`）

Listen 路徑改 `HeldLoopback::bind()`，`release()` 後立刻 `ServerRuntime` bind。Embedded-only（Plan30/31/33 singleplayer、Plan32 部分、session-lifecycle）不再 reserve port。

保留：Plan32／session-lifecycle 的 pid+底線 path、`view=4`／`sim=4`／`max_players=20`；headless `max_players=2`、`TempWorld`、`HeadlessClient` 與本地 `drive_*`／`request`；Plan33 `fresh_tcp_request`；Plan34 `tcp_start_request`；領域種子。`review_hardening_adversarial_frames.rs` 未改。

測試（各檔 `--test-threads=1`，Windows）：

| 檔案 | 結果 |
| --- | --- |
| `plan30_real_transport_acceptance` | pass（2） |
| `plan31_authoritative_block_actions` | pass（3） |
| `plan32_progression_travel` | pass（5） |
| `plan33_tcp_fishing_lifecycle` | pass（3） |
| `plan34_container_break_inventory_conservation` | pass（4） |
| `review_hardening_block_use_rejected` | pass（3） |
| `review_hardening_session_lifecycle` | pass（3） |
| `headless_server_authority` | pass（2） |
| `review_hardening_adversarial_frames` | pass（2） |
