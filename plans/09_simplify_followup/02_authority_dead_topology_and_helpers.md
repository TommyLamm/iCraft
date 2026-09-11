# Plan02 — 刪 `AuthorityTopology` 與死 helper

## 定位

`AuthorityTopology::{Singleplayer, ListenServer, Dedicated}` 寫進 `AuthorityCore` 與 `EmbeddedRuntimeBridge`，但 **production 從不讀**。實際分流是 `TransportMode::{Disabled, Listen}` 與 `PresentationTopology`。`State::authority_topology()` 無 caller。30+ 測試建構子重複傳一個從不分支的值。

同檔可刪的死／測試分叉：

- `server_runtime.rs`：`within_reach()` 零 caller（reach 已在 `ServerWorld::validate_request` 用 `8²`）。
- `server_world.rs`：`#[cfg(test)] ServerWorld::dispatch` — 註解寫 live 走 `AuthorityCore::submit_request`，測試仍維護縮小版 Container／Sleep／Command match，與 `dispatch.rs` 分叉。
- `world_tick.rs`：`MutationCause` 與 `BlockMutationRequest::{cause, new_entity}` 皆 `#[allow(dead_code)]`。ARCHITECTURE 寫明 authority **不 branch on cause tag**。
- `authority/dispatch.rs`：`rejected()` 對 pre-mutation reject 呼叫 `revisions.allocate()`，污染 dimension revision／checksum。

## 前置

無。不要改 `Packet`。

## 精確 acceptance

- [ ] 刪 `AuthorityTopology` enum、`AuthorityCore.topology`、`EmbeddedRuntimeBridge.topology`、`State::authority_topology()`。
- [ ] 測試建構子不再傳 topology；若需區分 listen／embedded，用既有 `TransportMode`。
- [ ] 刪 `within_reach`。
- [ ] 刪 `ServerWorld::dispatch`；`server_world` 單元測試改 `AuthorityCore::submit_request` 或直接呼叫 domain helper。
- [ ] `BlockMutationRequest` 只留 `{ pos, new_block, new_state }`；刪 `MutationCause` 與所有建構 site 的死欄位。
- [ ] `rejected()` 不再 `allocate()`；reject 的 `server_sequence` 用 `revisions.current()` 或與 accept 分離的固定值。確認 client／embedded 不以 reject sequence 推進 revision baseline。
- [ ] `cargo test --lib authority::` 與 `cargo test --lib server_world::` 通過。

## 預計檔案與測試

- `src/authority/contract.rs`、`src/authority/mod.rs`、`src/authority/dispatch.rs`
- `src/presentation/embedded_runtime.rs`、`src/state.rs`
- `src/server_runtime.rs`、`src/server_world.rs`、`src/world_tick.rs`
- 驗證：`cargo test --lib authority::`；`cargo test --lib -- world_tick`；`tests/authority_gameplay_domains.rs`（若 `rejected` 語意變了）

## 建議階段

1. Grep `AuthorityTopology`／`topology:` 全部建構點，一次刪欄位。
2. 刪 `within_reach`、`MutationCause`。
3. 改 `rejected()` 後跑 plan30／authority 測試，確認 reject 不推進世界 revision。
4. 遷移 `ServerWorld::dispatch` 測試。

## 不在本計劃

- 合併 `MultiplayerRole` 與 `PresentationTopology`（15）。
- 改 `game_mode` 同步（13）。
- 改 Command／ItemUse 的 Unsupported 集合（12）。
