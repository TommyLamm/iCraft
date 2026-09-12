# Plan10 — 單一 session 記錄、單一 response cache、單一 preflight

## 定位

### 雙 session 記錄鏡像欄位

join 同時建兩份（`server_runtime/ingress.rs` 175–197）：`SessionContract` 與 `PlayerSessionState` 都有 `id`／`username`／`position`／`dimension`／`game_mode`。雙寫住在 `session_sync.rs`（`write_pose` 14–47、`sync_dimension` 64–71、`sync_gameplay_projection` 74–81）；存檔又把 gameplay 拷到 `PlayerData`（`server_runtime.rs` 1577–1628，41 slot 來回）。`PlayerSessionState` 註解（758–760）自己承認 pose／dimension／sequence 屬於 contract。

**不合併兩個擁有者型別**（`AuthorityCore`／`ServerRuntime`）。收益在刪鏡像欄位：runtime 側只留 `InterestSet`、`Instant` pose clocks、save codec、`teleport_allowance`，掛在 contract **旁邊**，不是第二份 pose／name／dimension。

### 雙 response cache、雙 sequence watermark

- authority：`SessionContract.response_cache`（`contract.rs` 605–651）→ `dispatch.rs` 22–27／181／1335；runtime 再 peek 一次（`ingress.rs` 421–425）。
- transport：`GameplaySessionState.response_cache`（`network/session.rs` 606–655）→ `network/ingress.rs` 94–96；egress 再 re-cache（`egress.rs` `normalize_host_response` 21–33）。
- sequence：TCP `last_client_sequence`／`last_client_revision`（`network/ingress.rs` 105–123）＋ `SessionContract::validate_sequence`（`contract.rs` 657–662）。

每 session 兩份 128 深 `VecDeque<GameplayResponse>`，每請求兩次線性掃、兩次 clone。

### 三重 gate

bounds／sequence／revision 在 `route_gameplay_request`（`network/ingress.rs` 102–117）、`submit_request`（`dispatch.rs` 18–67）、`ServerWorld::validate_request`（1131–1157，含 8² reach、finite pose、operator）各檢一次；`handle_gameplay_request`（`ingress.rs` 415–456）再 clone request 兩次（426、456）。

### 每次 block action／combat clone 整個 `SessionContract`

`dispatch.rs` 299 `self.sessions.get(&session_id).cloned()`；combat 1029／1041 同。`SessionContract` 帶 128 筆 cache（`contract.rs` 584、605）。

### 域錯誤 enum 幾乎全部映射到一個 `RejectReason`

`TransactionError` 10 變體 → 永遠 `InvalidState`（`dispatch.rs` 1414–1416）；`FishingDomainError` ~11 變體 → `TooFar` 一次、其餘 `InvalidState`（1397–1411）；`CombatReject` 9 變體 → `TooFar`／`Duplicate`／其餘 `InvalidState`（1478–1490）。

## 前置

Plan 07（投影單一型別後 egress 沒有第二個 cache 合併點）；09 波 13（`game_mode` 已走 `session_sync`、`PLAYER_REACH` 已是常數）。

## 精確 acceptance

- [x] `PlayerSessionState` 刪 `id`／`username`／`position`／`dimension`／`game_mode` 鏡像；runtime overlay 以 `PlayerId` 索引 contract。
- [x] `session_sync.rs` 縮為 interest／clock／teleport；`write_pose`／`sync_dimension`／`sync_gameplay_projection` 不再雙寫。
- [x] transport 只留 `in_flight` + 過濾用 sequence watermark，**不存 `GameplayResponse`**；重複 request 轉交 authority cache；`normalize_host_response` cache 合併刪除。
- [x] 一個 `preflight(session, request, world) -> Result<(), RejectReason>`；TCP 只留 rate-limit＋`in_flight`。
- [x] `apply_block_action`／combat 改用小型 `Copy` view（pose／dimension／game_mode／gameplay／portal flags），不 clone contract。
- [x] 域函式直接回 `RejectReason`（或三變體 enum）；詳細錯誤只留 `#[cfg(test)]`／`debug_assert`。
- [x] idempotency 測試（Plan30／31、`gameplay_requests_are_idempotent_*`、`response_cache_is_bounded_*`）改對單一 cache 仍全綠。
- [x] `ARCHITECTURE.md` Ownership 段「Two session records stay separate」改寫為「一份 contract + runtime overlay」；Mutation path 的 gate 列表更新。

## 預計檔案與測試

- 改：`src/server_runtime.rs`、`src/server_runtime/{ingress,session_sync,projection}.rs`、`src/authority/{contract,dispatch,transactions,fishing,combat}.rs`、`src/network/{session,ingress,egress}.rs`、`src/server_world.rs`（`validate_request`）、`ARCHITECTURE.md`
- 驗證：`cargo test --lib authority:: network:: server_runtime::`；`tests/review_hardening_session_lifecycle.rs`；`tests/review_hardening_ingress.rs`；`tests/plan30_real_transport_acceptance.rs`；`tests/plan31_authoritative_block_actions.rs`

## 建議階段

1. `SessionContract` clone → `Copy` view（獨立、零契約變動）。
2. 域錯誤 enum 塌縮。
3. 單一 preflight。
4. transport cache 拆除，重跑 idempotency。
5. 刪 `PlayerSessionState` 鏡像欄位，改寫 `session_sync`。

## 不在本計劃

- 合併 `AuthorityCore` 與 `ServerRuntime` 型別（README §5）。
- `active_dimension`（Plan 11）。

## 實作與證據

### 改了什麼

- `SessionActionView`（`Copy`）：`apply_block_action`／combat 不再 `.cloned()` 含 128 筆 cache 的 `SessionContract`。
- `TransactionError`／`FishingDomainError`／`CombatReject` 刪除；域函式直接回 `RejectReason`（HookTooFar→TooFar、ReplayedEvent→Duplicate、其餘 InvalidState）。
- `preflight`／`preflight_session`：authority 單一 gate；TCP 只留 rate-limit、in_flight、sequence watermark、completed request-id 集合（不存 `GameplayResponse`）。
- `GameplaySessionState.response_cache` 與 `normalize_host_response` cache 合併刪除；重複 request_id 轉交 authority cache。
- `PlayerSessionState` 刪 `id`／`username`；live pose 只在 contract + `last_pose_position` clock；`session_sync` 不再把 pose 雙寫進 `PlayerData`（save／projection 經 `sync_gameplay_projection` 單向 overlay）。

### 測了什麼

- `cargo test --lib -- authority network server_runtime` — 201 ok
- `cargo test --test review_hardening_session_lifecycle` — 3 ok
- `cargo test --test review_hardening_ingress` — 2 ok
- `cargo test --test plan30_real_transport_acceptance` — 2 ok
- `cargo test --test plan31_authoritative_block_actions` — 3 ok
- `cargo check --all-targets`／`cargo check --bin icraft-server` — ok
- 死路徑證據：`rg response_cache src/network` → 無 matches（transport 不再存 `GameplayResponse`）

### 留下的缺口

- Transport 仍保留 completed request-id 集合（非 response body）以便重傳轉交 authority；這不是第二份 response cache。
- `validate_bounds` 失敗仍不消耗 sequence（與 console-only／malformed 語意一致）；revision／reach 等 preflight 失敗會消耗 sequence，與 TCP watermark 對齊。
- Plan 11（`active_dimension`）與合併 `AuthorityCore`／`ServerRuntime` 型別仍排除。
