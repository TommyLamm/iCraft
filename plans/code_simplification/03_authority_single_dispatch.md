# Plan03 — 權威請求單一 dispatch

## 定位

`AuthorityCore::submit_request`（`src/authority/mod.rs` ~1281）在通過 sequence／revision 檢查後：

```text
dispatch_session_command
  .or_else(dispatch_session_gameplay)
  .unwrap_or_else(|| world.dispatch(...))
```

三棵 match 對「誰支援這個 op」說法不一致：

| Operation | Session handler | World fallback |
| --- | --- | --- |
| BlockAction / Click / Combat / Fishing / … | 真工作 | `Unsupported` |
| BlockUse | `Unsupported` | `Unsupported` |
| Container Open/Close | 沒接（`_ => None`） | 真工作 |
| Command GameMode/TP/Give | 真工作 | `Unsupported` |
| Command Time/GameRule | 沒接 | 真工作 |
| Command Help/Kill/Weather/… | 沒接 | 逐個 `Unsupported` |

`BlockUse` 另外還在 `submit_request` 前段、spectator denylist、`ServerRuntime::handle_gameplay_request` 的空 accept arm 再拒絕一次。架構已寫：它永遠是相容信封，永遠 `Unsupported`。

`ServerWorld::apply_combat`（1159–1181，固定 −1 HP）若 01 還沒刪，本計劃必須刪；不得留下第三條戰鬥路徑。

`WorldDispatchError`（`src/server_world.rs` ~38）只是 `RejectReason` 的包裝，每個呼叫點都 `.map_err(|e| e.reason())`。

## 前置

無。若 01 已刪 `apply_combat`，本計劃跳過該項並在證據註明。

09 會改 `core.world` 儲存，本計劃**不得**動 park／swap。

## 精確 acceptance

- [ ] `submit_request` 通過驗證後只有**一棵** `match request.operation`。Session 專屬 op 在 core 處理；世界專屬 op（Open/Close、Sleep、Time、GameRule）呼叫 `ServerWorld` **helper**，helper 不再認識整份 `GameplayOperation`。
- [ ] `GameplayOperation::BlockUse` 只在 `submit_request` **一處**早退為 `RejectReason::Unsupported`，仍消耗 sequence、寫入 128-entry 回應 cache。刪 spectator 清單裡多餘的 `BlockUse` 臂、`dispatch_session_gameplay`／`ServerWorld::dispatch` 的重複臂、`handle_gameplay_request` 的空 accept 註解臂。
- [ ] **不得**從 wire enum 刪除 `BlockUse`。`handle_block_change` 仍可構造它以便 leftover 封包走到同一拒絕。
- [ ] `ServerWorld::apply_combat` 不存在。戰鬥只走 `authority/combat.rs`。
- [ ] `ServerWorld::dispatch` 若還留下「我們已在 session 處理過的 op → Unsupported」長清單，改成 `_ => Err(Unsupported)`，或刪掉這個 fallback 入口。
- [ ] `WorldDispatchError` 刪除或不再出現在公開簽名；世界方法直接回 `Result<_, RejectReason>`。
- [ ] `/respawn` 的 TCP `ClientRespawnRequest` 路徑保持非 op 可用。不得把非 op 的 TCP respawn 折進「所有 Command 都要 operator」而不留這條入口。
- [ ] `rejected()` 對 Unauthorized 仍分配 dimension revision（現有測試／ACK 可能 key 在 `server_sequence`）。
- [ ] `tests/review_hardening_block_use_rejected.rs`、Plan31、container click、fishing 測試期望值不變。

## 預計檔案與測試

- 修改：`src/authority/mod.rs`、`src/server_world.rs`、`src/server_runtime.rs`（只刪空 BlockUse arm／改 error 型別）。
- 測試：
  - `cargo test --test review_hardening_block_use_rejected -- --test-threads=1`
  - `cargo test --test plan31_authoritative_block_actions -- --test-threads=1`
  - `cargo test --test review_hardening_container_click -- --test-threads=1`
  - `cargo test --test plan33_tcp_fishing_lifecycle -- --test-threads=1`
  - `cargo test --test review_hardening_session_lifecycle -- --test-threads=1`
  - `cargo test --lib authority::`
  - `cargo check --all-targets`

## 建議階段

1. 列出每個 `GameplayOperation` 變體目前由哪一層處理，寫進證據（執行前表格）。
2. 先寫／確認 BlockUse 負向測試仍然失敗在 `Unsupported` 而不是別的 reason。
3. 抽世界 helper（`open_container`、`set_time`、`set_gamerule`…），讓 session match 呼叫它們。
4. 刪重複 BlockUse 臂與 `apply_combat`。
5. 收 `WorldDispatchError`。
6. 跑窄測試。特別核對 Open/Close／Sleep／Time／GameRule 沒有從 session match 漏掉而掉進 `_ => Unsupported`。

## 不在本計劃

- `activate_dimension` 的搬移模型（09）。
- 刪 `PlayerSessionState` 影子 interest（09）。
- 統一 `commands::parse` 與 dedicated console 語言（可在證據列後續，不要在本計劃做完）。
- 合併 Place／Ignite／Eye 的 debit helper（可做為本計劃的**可選**小步，但不得改變 Creative／Survival／brew-lock／「先 debit 再 set_block」順序）。預設不做，留給後續。
- Protocol bump。
