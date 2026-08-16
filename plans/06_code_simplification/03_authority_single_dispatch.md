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

- [x] `submit_request` 通過驗證後只有**一棵** `match request.operation`。Session 專屬 op 在 core 處理；世界專屬 op（Open/Close、Sleep、Time、GameRule）呼叫 `ServerWorld` **helper**，helper 不再認識整份 `GameplayOperation`。
- [x] `GameplayOperation::BlockUse` 只在 `submit_request` **一處**早退為 `RejectReason::Unsupported`，仍消耗 sequence、寫入 128-entry 回應 cache。刪 spectator 清單裡多餘的 `BlockUse` 臂、`dispatch_session_gameplay`／`ServerWorld::dispatch` 的重複臂、`handle_gameplay_request` 的空 accept 註解臂。
- [x] **不得**從 wire enum 刪除 `BlockUse`。`handle_block_change` 仍可構造它以便 leftover 封包走到同一拒絕。
- [x] `ServerWorld::apply_combat` 不存在。戰鬥只走 `authority/combat.rs`。
- [x] `ServerWorld::dispatch` 若還留下「我們已在 session 處理過的 op → Unsupported」長清單，改成 `_ => Err(Unsupported)`，或刪掉這個 fallback 入口。
- [x] `WorldDispatchError` 刪除或不再出現在公開簽名；世界方法直接回 `Result<_, RejectReason>`。
- [x] `/respawn` 的 TCP `ClientRespawnRequest` 路徑保持非 op 可用。不得把非 op 的 TCP respawn 折進「所有 Command 都要 operator」而不留這條入口。
- [x] `rejected()` 對 Unauthorized 仍分配 dimension revision（現有測試／ACK 可能 key 在 `server_sequence`）。
- [x] `tests/review_hardening_block_use_rejected.rs`、Plan31、container click、fishing 測試期望值不變。

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

## 實作與證據

### 執行前 op 表（源碼現況，改碼前）

`submit_request` 通過 sequence／revision 後：

```text
dispatch_session_command
  .or_else(dispatch_session_gameplay)
  .unwrap_or_else(|| world.dispatch(...).map_err(|e| e.reason()))
```

`BlockUse` 在此鏈之前已 `reject_for_session(..., Unsupported, Some(client_sequence))`。

| `GameplayOperation` | Session command | Session gameplay | World `dispatch` | 實際活路徑 |
| --- | --- | --- | --- | --- |
| `BlockAction` | n/a | `apply_block_action` | `Unsupported` | session |
| `BlockUse` | n/a | `Unsupported`（死） | `Unsupported`（死） | **早退** `submit_request` ~1238；spectator denylist 也列了（不可達） |
| `Container` Open | n/a | `_ => None` | `dispatch_container` Open | world |
| `Container` Close | n/a | `_ => None` | `dispatch_container` Close | world |
| `Container` Click（wire） | n/a | `apply_container_click` | 不會到 | session |
| `Container` 未知 wire | n/a | `_ => None` | `from_wire` 失敗 → `InvalidState` | world |
| `ContainerClick` | n/a | `apply_container_click` | `dispatch_container` Click → `Unsupported`（死） | session |
| `ItemUse` | n/a | 進食／扣物品 | `Unsupported` | session |
| `Combat` | n/a | `apply_authoritative_combat` | `Unsupported` | session（`authority/combat.rs`） |
| `Sleep` | n/a | `_ => None` | 床檢查 + `sleeping_players` | world |
| `Trade` | n/a | `world.apply_trade` | `Unsupported` | session |
| `Mount` | n/a | `world.apply_mount` | `Unsupported` | session |
| `Command` `/respawn`（字串） | `respawn_session` | 不到 | 不到 | session；`validate_request` 仍要求 operator |
| `Command` parse 失敗 | `InvalidState` | 不到 | 不到 | session |
| `Command` GameMode / Teleport / Give | 真工作 | 不到 | `Unsupported`（死） | session |
| `Command` Time Set/Add | `_ => None` | `_ => None` | `self.time = …` | world |
| `Command` GameRule | `_ => None` | `_ => None` | `rules.set` / sleeping % | world |
| `Command` Help/Kill/Weather/… | `_ => None` | `_ => None` | 逐個 `Unsupported` | world leftover |
| `Fishing` | n/a | `apply_fishing` | `Unsupported` | session |
| `FluidUse` | n/a | `apply_fluid_use` | `Unsupported` | session |
| `FurnaceTakeOutput` / `Craft` / `Enchant` / `Brew` / `Anvil` / `UseState` | n/a | `apply_transaction_operation` | `Unsupported` | session |

非 op：`ServerToHost::ClientRespawnRequest` → `authority.respawn_session`（不經 `GameplayOperation::Command`，不走 operator gate）。

`ServerWorld::apply_combat`：**已不存在**（Plan 01）。戰鬥只走 `authority/combat.rs`。

`WorldDispatchError`：`set_block`、`commit_container_item_slots`、`dispatch` / `dispatch_container` / `dispatch_command` 的公開／內部回傳；呼叫端一律 `.map_err(|e| e.reason())`。

### 實作後

`submit_request` 通過驗證後只有一棵 exhaustive `match request.operation`。不再呼叫 `world.dispatch`。

| Op | 活路徑 |
| --- | --- |
| `BlockAction` | `apply_block_action` |
| `BlockUse` | **早退**（驗證前，消耗 sequence + 128-entry cache）。match 裡仍有 `Unsupported` 臂只為 exhaustiveness，活路徑走不到。 |
| `Container` Open/Close | `ServerWorld::open_container` / `close_container` |
| `Container` Click | `apply_container_click` |
| `Container` 未知 wire | `InvalidState` |
| `ContainerClick` | `apply_container_click` |
| `ItemUse` / `Trade` / `Mount` | `apply_item_use` / `apply_trade` / `apply_mount` |
| `Combat` | `apply_authoritative_combat`（`authority/combat.rs`） |
| `Sleep` | `ServerWorld::sleep_player` |
| `Command` `/respawn` | `respawn_session`（仍受 `validate_request` operator gate） |
| `Command` parse 失敗 | `InvalidState` |
| `Command` GameMode / Teleport / Give | session `apply_command` |
| `Command` Time Set/Add | `set_time` / `add_time` |
| `Command` GameRule | `set_gamerule` |
| `Command` Help/Kill/Weather/… | `Unsupported` |
| `Fishing` / `FluidUse` / 工作站交易 | 既有 session helpers |

`ServerWorld::dispatch` 留下給 unit tests：Open/Close/Sleep/Time/GameRule 轉呼叫 helper；session-owned 與 `BlockUse` 走 `_ => Unsupported`。Container Click 經 world 仍先 `ensure_container_slot` 再 `Unsupported`。

`WorldDispatchError` 已刪。`set_block` / `commit_container_item_slots` / helper / thin `dispatch` 直接回 `RejectReason`。

`apply_combat`：Plan 01 已刪，本計劃未重建。

`ClientRespawnRequest` 與 `rejected()` 未改。

Open/Close/Sleep/Time/GameRule 都在 session match 的具名臂，不能掉進 `_ => Unsupported`（session match 沒有 `_`）。

### 測試

```
cargo check --all-targets                                          ok
cargo test --lib authority::                                       56 passed
cargo test --lib server_world::                                    18 passed
cargo test --test review_hardening_block_use_rejected -- --test-threads=1   3 passed
cargo test --test plan31_authoritative_block_actions -- --test-threads=1    3 passed
cargo test --test review_hardening_container_click -- --test-threads=1      9 passed
cargo test --test plan33_tcp_fishing_lifecycle -- --test-threads=1          3 passed
cargo test --test review_hardening_session_lifecycle -- --test-threads=1    3 passed
```

未 commit。
