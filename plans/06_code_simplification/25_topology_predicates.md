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

- [ ] 刪除 `State::is_authoritative`（或改成 `#[cfg(test)]` 薄包裝並在證據列出
      剩餘測試用法）。生產路徑不得再出現這個名字。
- [ ] 每個原呼叫點改成明確謂詞，分類寫進證據表（Join vs leftover）。
      預設：
      - 插值／replicated entity／client health／Join 盾牌：`is_join_client()`
      - leftover tick／leftover 傷害／leftover melee：維持 `is_legacy_owner()`
      - Embedded 仍要本地表現、但不擁有世界：不得誤標 leftover
- [ ] `tick_simulation` 現役路徑讀起來是：有 runtime → `tick_authority_boundary`；
      Join → 不跑 leftover；不得再出現 `let authoritative = is_authoritative()`。
- [ ] 不得改 `PresentationTopology::from` 的三值映射。
- [ ] 不得改 Join 投影、Embedded 物品欄 writeback、傷害／死亡閘。
- [ ] 既有測試期望值不變。`presentation_inventory_policy` 單元測不變。

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
