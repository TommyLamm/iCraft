# Plan12 — 活型別裡的 leftover 衛生

## 定位

01 隔離的是「零生產呼叫、看起來像線上 API」的函式。本計劃針對 **仍掛在活型別上、會讓人改錯入口** 的殘渣。Plan 03／09 之後這些已經不是第二條權威路徑，但還編在 `ServerWorld`／`AuthorityCore` 裡。

掃描當日證據（執行前必須再 grep，以源碼為準）：

| 符號 | 位置 | 問題 |
| --- | --- | --- |
| `ServerWorld.last_snapshot` | `src/server_world.rs:73`、`:1666` 寫入 | **唯寫**。活消費者只用 `AuthorityCore.last_snapshot` |
| `AuthorityCore::tick` 的 activate 舞 | `src/authority/mod.rs:439` 存 `active_before_tick`，每維度 `activate_dimension`，`:504` restore | Plan 09 已改成 `BTreeMap` + key；`with_world`／`world_mut` 已能按維度取世界。tick／checksum 仍繞 `active_dimension` |
| `ServerWorld::dispatch` | `src/server_world.rs:1463` | rustdoc 已寫「retained for unit tests」。活請求走 `AuthorityCore` 單一 match（03） |
| `ensure_villager`／`ensure_vehicle`／`place_bonus_chest` | `server_world.rs:1086+` | 生產 tick 不呼叫；測試 fixture |
| `commands/mod.rs:5` | 「The executor in `State` remains the authority gate」 | 過期。活入口是 `AuthorityCore::apply_command`（`authority/mod.rs:2307`） |

`activate_dimension` 本身仍是合法 API（session 切維度、save、metrics 的相容視圖）。本計劃禁止刪這個方法，只禁止 **tick／checksum 迴圈裡為了換槽而呼叫它**。

## 前置

無。09 已完成（world map 不再 swap）。可與 11、13 並行。不要跟 16 搶 session 欄位寫入。

## 精確 acceptance

- [ ] `ServerWorld` 不再有 `last_snapshot` 欄位，tick 不再寫它。`AuthorityCore::tick` 仍組 `AuthoritySnapshot` 並存 `AuthorityCore.last_snapshot`。
- [ ] `AuthorityCore::tick` 與 checksum 迴圈改走 `world`／`world_mut(dimension)`（或同等、已存在的按 key 存取）。迴圈結束後 `active_dimension` 仍等於進入 tick 前的值（給 State／runtime／save 的相容視圖）。
- [ ] 若某個 `activate_dimension` 呼叫仍有 **可見副作用**（例如切 `active` 才會跑的 hook），寫進證據並留下該呼叫。不得為了少一行而改 checksum 輸入順序。
- [ ] 維度迭代仍是 `BTreeMap` key、`Dimension as u8` 順序。禁止 `HashMap`。
- [ ] `ServerWorld::dispatch` 標 `#[cfg(test)]`（或搬進 `#[cfg(test)]` 子模組）。`server_world` 單元測仍編譯、期望值不變。
- [ ] `ensure_villager`／`ensure_vehicle`／`place_bonus_chest` 若 grep 只有測試呼叫：改 `#[cfg(test)]` 或 `pub(crate)` + rustdoc「測試 fixture」。`ensure_entity`（掉落／XP 用）留下。
- [ ] `src/commands/mod.rs` 檔頭改成：session 指令由 `AuthorityCore::apply_command` 執行；`State` chat 是 leftover／表現層；dedicated console 是另一個 admin 面，不走 `commands::parse`。不得把三套 parser 合成一套。
- [ ] 既有測試期望值不得改。

## 預計檔案與測試

- 修改：`src/server_world.rs`、`src/authority/mod.rs`、`src/commands/mod.rs`。
- 測試：
  - `cargo test --lib server_world::`
  - `cargo test --lib authority::`
  - `cargo test --test review_hardening_invariants -- --test-threads=1`
  - `cargo test --test runtime_topology_parity -- --test-threads=1`
  - `cargo test --test review_hardening_chunk_residency -- --test-threads=1`
  - `cargo check --bin icraft-server`

## 建議階段

1. grep `last_snapshot`、`activate_dimension`、`ServerWorld::dispatch`、三個 ensure／place helper。
2. 先刪唯寫欄位，跑 `server_world` 單元測。
3. tick／checksum 改按 key 取世界；確認 `active_dimension` 進出不變。跑 `authority::` 與 invariants（含 arrival-order checksum）。
4. `dispatch` 與測試 fixture `cfg`。
5. 修 `commands` rustdoc。

## 不在本計劃

- 合併 `SessionContract` 與 `PlayerSessionState`（16 只收斂寫入；不合併型別）。
- 把 console 折進 `commands::parse`。
- 刪 `apply_batch`／`tick_all_loaded_*`／`update_mobs`（仍服務 leftover `state`／`legacy_sim`）。
- 把 `SpawningSystem`／`Brain` 接到 tick。
- 拆 `authority/mod.rs` 成 tick.rs／dispatch.rs（README §7）。
