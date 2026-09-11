# Plan11 — 刪 `active_dimension`／`world()`／`world_mut_active()`，dispatch 帶 `Dimension`

## 定位

`active_dimension` 被文件標為「compatibility key」（`authority/mod.rs` 80–82；`ARCHITECTURE.md` 88–89），但仍是假的「目前世界」指針：

- `submit_request` 先 `activate_dimension` 再 `world()`／`world_mut_active()`（`dispatch.rs` 38–39；全檔 ~40+ 呼叫）。
- `tick.rs` 19 存、96 在 portal `set_session_dimension` 後 restore。
- `current_revision()`（376–377）**只看 active world**——跨維度請求容易讀錯 revision。
- `dimensions()`（212–214）**每次配一個 `Vec`**，tick（20）、metrics（1119–1131，兩次）、eviction（1435）、projection（768）都呼叫。
- `tick.rs` 33–60 對同一維度 `world_mut` 查 4–5 次，各自 `expect`。

## 前置

無。

## 精確 acceptance

- [ ] dispatch／tick／portals／combat／fishing／mining helper 全部顯式帶 `Dimension`（或 `&mut ServerWorld`）參數；`world()`／`world_mut_active()`／`activate_dimension`／tick 尾的 restore 刪除。
- [ ] `active_dimension` 欄位刪除；存檔「current dimension」改從 session contract 取（`server_runtime.rs` 1232、1284）。
- [ ] `current_revision(dimension)` 顯式帶維度。
- [ ] `dimensions()` 改回 `impl Iterator<Item = Dimension>` 或 `worlds.keys()`，零配置。
- [ ] `tick.rs` 每維度只做一次 `world_mut` 查找。
- [ ] `ARCHITECTURE.md` 刪「`active_dimension` is a compatibility key」與「After the pass, `active_dimension` is restored」兩句。

## 預計檔案與測試

- 改：`src/authority/{mod,dispatch,tick,portals,combat,fishing,mining}.rs`、`src/server_runtime.rs`、`src/server_runtime/{ingress,projection}.rs`、`ARCHITECTURE.md`
- 驗證：`cargo test --lib authority::`（~100+ 測試呼叫 `world()`／`world_mut_active()` 要改）；`tests/plan32_progression_travel.rs`（portal）；`tests/authority_persistence.rs`

## 建議階段

1. 先把 `dimensions()` 改零配置（獨立小 commit）。
2. helper 簽名加 `Dimension`，用編譯錯誤當清單。
3. 刪欄位與 restore；改測試。

## 不在本計劃

- session 記錄合併（Plan 10）。
- `BTreeMap<Dimension, ServerWorld>` 換容器（不需要）。
