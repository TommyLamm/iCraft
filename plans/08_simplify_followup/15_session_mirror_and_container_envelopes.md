# Plan15 — session 鏡像欄位與 Container 雙信封

## 定位

`SessionContract` 與 `PlayerSessionState` **必須分開**（interest／save codec 不能進確定性核心）。不成立的是重疊欄位：`id`／`username`／`dimension`／pose／`last_client_sequence` 寫了兩份，還有第三份在 `network/session.rs`。`session_sync.rs` 整檔在維持鏡像。`projection.rs` 仍有 `session.dimension = dimension` 旁路。

`GameplayOperation::Container` 的 Click 與 `ContainerClick` 是同一點擊的兩種 wire shape。Live 桌面 open／close 用 `Container`，click 用 `ContainerClick`；`ContainerAction::Click` 仍可走舊 `Container { action: 1 }`。

`last_client_sequence`：ingress 與 `dispatch.rs` 雙閘。

純 session 突變多處手寫 `let mut candidate = original; … session.gameplay = candidate`，已有 `SessionGameplayState::transact`。

## 前置

02（舊 inbound Container* 適配刪掉後，只留一種 click 信封才安全）。

## 精確 acceptance

- [ ] `PlayerSessionState` 去掉與權威重複的 pose／dimension／`last_client_sequence`；只留 interest、存檔、`Instant` 時鐘、teleport allowance。
- [ ] 生產路徑維度／pose 只經 `write_pose` / `sync_pose_from_authority` / `sync_dimension` / `sync_gameplay_projection`。`projection.rs` 旁路賦值刪除。
- [ ] 傳輸層只分配序號；權威是唯一「已接受序號」來源（或證據說明為何 ingress 閘必須留下）。
- [ ] `Container` 只留 Open／Close；Click 只走 `ContainerClick`。刪 `ContainerAction::Click` 與 leftover `action: 1` 適配。
- [ ] 不碰 world 的 session 突變改 `transact`；跨 world+session 的容器交易維持 clone 兩邊再 commit。
- [ ] 會話／容器測試通過。

## 預計檔案與測試

- `src/server_runtime.rs`、`session_sync.rs`、`projection.rs`、`src/authority/contract.rs`、`dispatch.rs`、`src/network/protocol.rs`、`ingress.rs`
- 驗證：session lifecycle；container conservation；review_hardening_session_lifecycle

## 建議階段

1. 刪 projection 旁路，全部走 sync helper。
2. 收 Container 信封。
3. 去掉 runtime 重疊欄位（編譯器當清單）。
4. `transact` 替換純 session clone-assign。

## 不在本計劃

- 合併 `SessionContract` 與 `PlayerSessionState`。
- `activate_dimension` 改成第二條 tick 路徑。
