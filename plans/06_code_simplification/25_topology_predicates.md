# Plan25 — `is_authoritative()` 收成拓撲謂詞

## 定位

`State` 同時用兩套重疊謂詞：

```rust
pub fn is_authoritative(&self) -> bool {
    !self.role.is_join_client()
}
pub fn presentation_topology(&self) -> PresentationTopology { /* Join / Embedded / LegacyOwner */ }
```

`is_authoritative()` 在 Embedded **與** LegacyOwner 都是 true。掃描當日
`state.rs` 約 27 處 `is_authoritative()`，另約 40 處 `is_legacy_owner()`。
新人會把「表現層不是 Join」讀成「這台機器是 `AuthorityCore`」。

每個呼叫點只屬於兩類之一：

| 實際意思 | 應改成 |
| --- | --- |
| Join 不得在本地結算 HP／插值／傷害 | `presentation_topology().is_join_client()`（或 `!…`） |
| leftover 世界擁有者才跑第二套模擬 | 已是 `is_legacy_owner()`；不要改回 `is_authoritative` |

**禁止** 把 `is_authoritative()` 全域替換成 `is_legacy_owner()`：Embedded 會失去
「不是 Join」的表現層分支，行為就變了。

## 前置

02 已完成。

**建議** 21 已合併：leftover 呼叫已 cfg，剩下的 `is_authoritative` 比較好分類。
未合併時仍可做，但 leftover 檔裡的呼叫也要一起改，且不得改閘門語意。

不要跟 21／28 同時改 `state.rs`。

## 精確 acceptance

- [x] 刪除 `State::is_authoritative`（或改成 `#[cfg(test)]` 薄包裝並在證據列出
      剩餘測試用法）。生產路徑不得再出現這個名字。
- [x] 每個原呼叫點改成明確謂詞，分類寫進證據表（Join vs leftover）。
      預設：
      - 插值／replicated entity／client health／Join 盾牌：`is_join_client()`
      - leftover tick／leftover 傷害／leftover melee：維持 `is_legacy_owner()`
      - Embedded 仍要本地表現、但不擁有世界：不得誤標 leftover
- [x] `tick_simulation` 現役路徑讀起來是：有 runtime → `tick_authority_boundary`；
      Join → 不跑 leftover；不得再出現 `let authoritative = is_authoritative()`。
- [x] 不得改 `PresentationTopology::from` 的三值映射。
- [x] 不得改 Join 投影、Embedded 物品欄 writeback、傷害／死亡閘。
- [x] 既有測試期望值不變。`presentation_inventory_policy` 單元測不變。

## 預計檔案與測試

- 修改：`src/state.rs`、`src/presentation/legacy_systems.rs`（及其他
      `is_authoritative()` 命中檔）、必要時 rustdoc。
- 測試：
  - `cargo test --lib presentation_inventory_policy::`
  - `cargo test --bin icraft multiplayer_host_keeps_world_ticks singleplayer_pause -- --test-threads=1`
  - `cargo test --test review_hardening_embedded_presentation -- --test-threads=1`
  - `cargo test --test review_hardening_join_projection -- --test-threads=1`
  - `rg "is_authoritative" src` 證據為空（或只剩測試別名）

## 建議階段

1. 列出每個 `is_authoritative()` 與相鄰拓撲閘，標 Join／leftover／Embedded。
2. 先改 `tick_simulation` 與 `handle_primary_press`。跑 pause／embedded。
3. 改 inbound 投影（health／entity interpolation）。跑 join projection。
4. 刪方法。grep 必須乾淨。

## 不在本計劃

- leftover cfg（21）。
- 抽 `EmbeddedRuntimeBridge`（28）。
- 合併 `SessionContract` 與 `PlayerSessionState`。
- 改 `MultiplayerRole` 形狀。

## 實作與證據

### 1. 呼叫點分類與替換表

| 檔案與行號（原始） | 呼叫點上下文 | 語意類別 | 替換後謂詞 |
| --- | --- | --- | --- |
| `src/presentation/legacy_systems.rs:21` | `update_village_and_raid_systems` (Join 早退) | Join 閘門 | `self.presentation_topology().is_join_client()` |
| `src/presentation/legacy_systems.rs:347` | `update_vehicles_and_fishing` (船隻模擬) | leftover 模擬 | `self.presentation_topology().is_legacy_owner()` |
| `src/state.rs:1763` | `switch_dimension` (Join 不自推維度) | Join 閘門 | `self.presentation_topology().is_join_client()` |
| `src/state.rs:2055` | `apply_boss_events` (凋靈/終界龍 leftover 傷害/爆炸/方塊) | leftover 模擬 | `self.presentation_topology().is_legacy_owner()` |
| `src/state.rs:5219` | `pub fn is_authoritative(&self)` 方法定義 | 冗餘別名 | **徹底刪除** |
| `src/state.rs:6640` | `apply_replicated_entity_state` | Join 專用投影 | `!self.presentation_topology().is_join_client()` |
| `src/state.rs:6694` | `apply_replicated_entity_despawn` | Join 專用投影 | `!self.presentation_topology().is_join_client()` |
| `src/state.rs:6714` | `update_replicated_entity_interpolation` | Join 專用插值 | `!self.presentation_topology().is_join_client()` |
| `src/state.rs:7127` | `NetworkInbound::PlayerHealth` | Join 封包接收 | `self.presentation_topology().is_join_client()` |
| `src/state.rs:7150` | `NetworkInbound::PlayerEffect` | Join 封包接收 | `self.presentation_topology().is_join_client()` |
| `src/state.rs:7183` | `NetworkInbound::TimeSync` | Join 封包接收 | `self.presentation_topology().is_join_client()` |
| `src/state.rs:7196` | `NetworkInbound::WorldRulesSync` | Join 封包接收 | `self.presentation_topology().is_join_client()` |
| `src/state.rs:7201` | `NetworkInbound::LightningStrike` | Join 封包接收 | `self.presentation_topology().is_join_client()` |
| `src/state.rs:8183` | `submit_chat` (Join 不得本機下指令) | Join 限制 | `self.presentation_topology().is_join_client()` |
| `src/state.rs:8576` | `handle_pause_menu_click` (Join 退出不本機存檔) | Join 限制 | `!self.presentation_topology().is_join_client()` |
| `src/state.rs:8647` | `trigger_background_save` (僅 legacy owner 存檔) | leftover 存檔 | `!self.presentation_topology().is_legacy_owner()` |
| `src/state.rs:8752` | `save_synchronously` (Join 不本機存檔) | Join 限制 | `self.presentation_topology().is_join_client()` |
| `src/state.rs:9369` | `schedule_chunk_load` (legacy SaveManager 載入方塊) | leftover 存檔 | `self.presentation_topology().is_legacy_owner()` |
| `src/state.rs:9757` | `tick_simulation` (變數重新命名) | leftover 模擬 | `let is_legacy_owner = self.presentation_topology().is_legacy_owner();` |
| `src/state.rs:10777` | `update_weather` (積雪方塊變更) | leftover 模擬 | `self.presentation_topology().is_legacy_owner()` |
| `src/state.rs:10805` | `update_weather` (閃電打擊) | leftover 模擬 | `self.presentation_topology().is_legacy_owner()` |
| `src/state.rs:10821` | `strike_lightning` (閃電實體與方塊著火) | leftover 模擬 | `!self.presentation_topology().is_legacy_owner()` |
| `src/state.rs:12465` | `take_damage_with_attacker` (legacy 玩家傷害結算) | leftover 模擬 | `!self.presentation_topology().is_legacy_owner()` |
| `src/state.rs:12625` | `respawn` (Join 走網路請求) | Join 轉發 | `self.presentation_topology().is_join_client()` |
| `src/state.rs:12740` | `handle_primary_press` (legacy melee 攻擊) | leftover 模擬 | `self.presentation_topology().is_legacy_owner()` |
| `src/state.rs:12923` | `handle_secondary_click` (主手持盾 UseState 轉發) | Join 轉發 | `self.presentation_topology().is_join_client()` |
| `src/state.rs:13027` | `handle_secondary_click` (副手持盾 UseState 轉發) | Join 轉發 | `self.presentation_topology().is_join_client()` |
| `src/state.rs:13093` | `handle_secondary_release` (盾牌釋放 UseState 轉發) | Join 轉發 | `self.presentation_topology().is_join_client()` |
| `src/state.rs:13700` | `set_item_at_slot` (Join 不得本機修改物品欄) | Join 限制 | `self.presentation_topology().is_join_client()` |
| `src/state.rs:14647` | `open_chest` (Join 走網路請求) | Join 轉發 | `self.presentation_topology().is_join_client()` |
| `src/state.rs:15037` | `close_inventory` (Join 走網路請求) | Join 轉發 | `self.presentation_topology().is_join_client()` |

### 2. 測試與驗證證據

- `cargo test --lib presentation_inventory_policy::`：4/4 通過。
- `cargo test --bin icraft multiplayer_host_keeps_world_ticks -- --test-threads=1`：1/1 通過。
- `cargo test --bin icraft singleplayer_pause -- --test-threads=1`：1/1 通過。
- `cargo test --test review_hardening_embedded_presentation -- --test-threads=1`：4/4 通過。
- `cargo test --test review_hardening_join_projection -- --test-threads=1`：3/3 通過。
- `rg "is_authoritative" src` 搜尋結果為 0，生產與測試代碼均無殘留。

### 3. 留下的缺口

- leftover 模擬系統與型別在預設 build 下的條件編譯隔離（`#[cfg(any(test, feature = "legacy_owner"))]`）留待 Plan 21 處理。
- `EmbeddedRuntimeBridge` 與 inbound 模組自 `state.rs` 抽出留待 Plan 28 處理。

