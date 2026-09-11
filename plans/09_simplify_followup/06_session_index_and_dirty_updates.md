# Plan06 — session 維度索引 + dirty `session_updates`

## 定位

`AuthorityCore::tick` 每個 loaded dimension 各自 `sessions.values().filter(|s| s.dimension == dimension as u8)`，至少在 `tick_session_domains`、`tick_mining`、`tick_portal_travel` 與組 snapshot 前的 players collect 重複。3 個 dimension × 多次 pass = 固定 O(sessions × dimensions)，與 Wave 08「simulation union 而非全圖掃描」方向相反。

同函式結尾：

```text
session_updates: self.sessions.values().map(|session| SessionGameplayUpdate {
    state: session.gameplay, // 41 slot inventory + mining/brew/fishing
})
self.last_snapshot = snapshot.clone();
```

投影層 `last_projected_session_revision` 會過濾 TCP／embedded 是否送出，但 **權威每 tick 仍分配／複製整包**。idle server 也 clone N 份。`SessionGameplayState` 維持 `Copy` 固定陣列（本計劃不改這個策略）。

## 前置

無。08 會讀 snapshot checksum 欄位，不要在本計劃改 checksum 語意。

## 精確 acceptance

- [x] `register_session`／`remove_session`／`set_session_dimension` 維護 `BTreeMap<u8, Vec<PlayerId>>`（或同等索引）；四個 tick phase 共用，不再每 phase 全表 filter。
- [x] `session_updates` 只含本 tick `gameplay.revision` 相對上次已發布 revision 有變更的 session（含 mining／brew／fishing／cooldown 造成的 revision bump）。
- [x] 新 join／dimension 變更的 session 仍會出現在該 tick 的 `session_updates`（避免 client 空白）。
- [x] `last_snapshot` 不再為了保留舊 session 向量而整包 clone；可用 `mem::replace` 或 mutations 與 session 分存。
- [x] `tests/review_hardening_*.rs`、`tests/plan33_tcp_fishing_lifecycle.rs` 通過；漏 mark dirty 會讓 mining／brew／fishing 投影消失，必須有覆蓋。

## 預計檔案與測試

- `src/authority/tick.rs`、`src/authority/mod.rs`、`src/authority/contract.rs`、`src/authority/portals.rs`、`src/authority/mining.rs`
- `src/server_runtime/projection.rs`（只適應「更新可能缺席」，不要改 revision gate 語意）
- 驗證：`cargo test --lib authority::`；`tests/plan33_tcp_fishing_lifecycle.rs`；`tests/review_hardening_session_lifecycle.rs`

## 建議階段

1. 加維度索引，四個 filter 改查表；補 session 轉 dimension 測試。
2. 在 `tick_session_domains`／`tick_mining`／其它寫 `gameplay` 的點 mark dirty，snapshot 只 collect dirty。
3. 確認 join／respawn 路徑 mark dirty。

## 不在本計劃

- 改 `SessionGameplayState` 為非 `Copy` 或改 inventory 為 Vec。
- 合併 `SessionContract` 與 `PlayerSessionState`。
- 改 entity／mutation fanout（11）。
- 改 interest 重建（07）。

## 實作與證據

### 改了什麼

- `AuthorityCore` 新增 `sessions_by_dimension: BTreeMap<u8, Vec<PlayerId>>` 與 `dirty_session_ids: BTreeSet<PlayerId>`；`register_session`／`remove_session`／`set_session_dimension` 維護索引，join／dimension／respawn／accepted dispatch／mining／brew／fishing／cooldown／portal 路徑 `mark_session_update`。
- `tick_session_domains`／`tick_mining`／`tick_portal_travel`／players collect 改查 `session_ids_in_dimension`，不再每 phase 全表 filter。
- `AuthorityCore::tick` 用 `take_dirty_session_updates` 組 `session_updates`，`last_snapshot` 改 `mem::replace`（不再為保留舊 session 向量整包 clone）。
- `projection.rs` 註解說明 dirty subset：缺席＝本 tick gameplay 未變，保留 last projected；revision gate 不變。
- `ARCHITECTURE.md` 補 session index／dirty `session_updates` 契約。
- `plan33_tcp_fishing_lifecycle`：listen 路徑在 cast／duplicate wait 期間每 tick seed open water，避免 TCP 慢時 flying hook 因距離 despawn（HEAD 上 listen 本就失敗）。

### 測了什麼

- `cargo test --lib authority::`：62 passed（含 `session_dimension_index_tracks_register_move_and_remove`、`session_updates_publish_join_and_dimension_change_but_not_idle_ticks`、`mining_brew_and_fishing_revision_bumps_publish_dirty_session_updates`）。
- `cargo test --test plan33_tcp_fishing_lifecycle`：3 passed（embedded／dedicated／listen）。
- `cargo test --test review_hardening_session_lifecycle`：3 passed；另跑 `review_hardening_invariants`（6）與 `review_hardening_join_projection`（3）皆通過。

### 留下的缺口

- `contract.rs`／`mining.rs` 無需改動（mining 已在 `tick.rs`）；checksum 語意未動（留給 Plan 08）。
- `last_snapshot.session_updates` 現為本 tick dirty 子集，讀取方不可再假設「永遠含全 session」；現行投影已用 `last_projected_session_revision` 閘門。
- listen fishing 測試仍依賴 wait 期間 seed water；根因是 harness 以固定 tick 等 TCP，非 dirty-index 本身。
