# Plan19 — `save.rs` 子模組拆分

## 定位

`src/save.rs` 約 5,800 行，其中約 2,200 行是 `#[cfg(test)]`。同一個檔混著：

- 活 `SaveManager`（`ServerRuntime` 唯一寫者：level、region、player、`mutation_revisions.bin`）
- leftover `SaveQueue`／`spawn_save_worker`／`NetworkSnapshotWorker`（05：embedded 不再建構）
- 格式（`ChunkSaveData` v3、player envelope、遷移）
- region IO、atomic replace、fail-closed restore

05 已保證 SP／Host／Dedicated 不建 desktop 存檔工人。檔案看起來仍像兩套活系統。
本計劃用 **路徑名** 標出 leftover，不刪工人（`LegacyOwner` 與單元測還在建構）。

## 前置

05 已完成。可與 18、20 並行。不要跟 15 搶 leftover `State` 的 `SaveQueue` 欄位——本計劃只拆 `save` 模組，不改 `State` 誰持有 queue。

## 精確 acceptance

- [ ] `src/save.rs` 變成 `src/save/mod.rs`（或根 + 子檔），`SaveManager` 仍是
      `ServerRuntime` 呼叫的唯一門面。`use crate::save::{SaveManager, ChunkSaveData, PlayerData, …}` 零改。
- [ ] 至少拆出（名稱可微調，責任不可混）：
      - `format`：`ChunkSaveData`、`LevelData`、`PlayerData`、版本／遷移
      - `region`：region cache、compress、atomic write、fail-closed restore
      - `player`：identity、`players/<id>.dat`、`player.dat`
      - `index`：`MutationRevisionIndex`
      - `legacy_queue`：`SaveQueue`、`spawn_save_worker`、`NetworkSnapshotWorker`——**路徑必須帶 leftover／legacy**
- [ ] `peek_current_dimension` 仍是唯讀、不建 `SaveManager`。
- [ ] 不得改 region 格式、zlib take 上限、atomic replace、`.bin.bak` 語意、fail-closed 座標進入
      `failed_restore_chunks` 的契約。
- [ ] 不得讓 embedded／Join 突然建構 `SaveQueue`。05 的閘門（`bootstrap.rs`：
      `is_client || in_process_authority` → workers `None`）保持。
- [ ] `save` 模組內的 failpoint／restore 單元測跟著 format／region 走，不要丟到 `tests/`
      而失去 hooks。
- [ ] 既有測試期望值不變。

## 預計檔案與測試

- 新增：`src/save/mod.rs` 與上列子檔。
- 修改：`src/lib.rs`／`src/main.rs` 的 `mod save` 路徑（若改目錄）。
- 測試：
  - `cargo test --lib save::`
  - `cargo test --test authority_persistence -- --test-threads=1`
  - `cargo test --test review_hardening_chunk_restore -- --test-threads=1`
  - `cargo test --test review_hardening_session_lifecycle -- --test-threads=1`
  - `cargo check --bin icraft-server`
  - `cargo test --test review_hardening_embedded_presentation -- --test-threads=1`
    （確認 embedded 仍無 presentation `SaveManager`）

## 建議階段

1. 先搬純格式型別 + `pub use`。`cargo test --lib save::`。
2. 搬 region IO／restore。跑 chunk_restore。
3. 把 leftover 工人整段移到 `legacy_queue.rs`，根檔只 re-export。
4. 跑 persistence／session lifecycle。

## 不在本計劃

- 刪 `SaveQueue`／snapshot worker。
- 改 mutation revision 容量或 region 檔名。
- 把 snapshot worker 搬進 `network/`（可列後續；本計劃只隔離在 `save/legacy_*`）。
- 合併 `PlayerData` 與 `SessionGameplayState`（16 的籬笆）。
