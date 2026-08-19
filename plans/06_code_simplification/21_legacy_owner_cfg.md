# Plan21 — leftover 模擬改為 test／feature 才編譯

## 定位

01–20 之後，活啟動路徑已經是 embedded runtime。`State::new` 對
`MultiplayerRole::Singleplayer | Host` **一定** 設 `in_process_authority` 並
`panic` 在 `EmbeddedRuntimeBridge` 失敗時。Join 走 `NetworkClient`。選單啟動
進不了 `PresentationTopology::LegacyOwner`。

leftover 仍無條件編進 desktop binary，和活方法坐在同一個 `impl State`：

| 檔／符號 | 約略規模 | 活啟動 |
| --- | --- | --- |
| `src/presentation/legacy_sim.rs` | leftover tick 本體 | 不進入 |
| `src/presentation/legacy_systems.rs` | 村莊／raid／載具／熔爐／漏斗 | 早退 |
| `src/presentation/legacy_interaction.rs` | leftover click／mutate | 只在 `LegacyOwner` 臂 |
| `State::tick_simulation` | `legacy_tick_*` 一串 | `is_legacy_owner()` 才呼叫 |
| `State::handle_click` | `LegacyOwner => leftover_handle_click` | 選單到不了 |
| `State::apply_mutation_batch` | `world_mutation::apply_batch` | 無其他生產呼叫 |
| `bootstrap::load_launch_world_state` else 臂 | desktop `SaveQueue` | 只有 leftover 建構 |

掃描當日 `state.rs` 仍約 16,751 行，約 40 處 `is_legacy_owner()`。新同事修
`tick_simulation`／`handle_click` 仍會掃到第二套世界。

本計劃**不刪** leftover 檔，也不刪 `PresentationTopology::LegacyOwner`（政策測試
仍用它）。只讓預設 desktop 編譯不再含 leftover 方法本體。

## 前置

15 已完成（leftover 已是 `#[path]` 孩子）。不要跟 25／28 同時改 `state.rs`。

## 精確 acceptance

- [ ] `Cargo.toml` 增加 feature `legacy_owner`（不要放進 `default`，不要跟
      `harness` 綁在一起）。
- [ ] `src/state.rs` 對三個 leftover `#[path]` 模組加
      `#[cfg(any(test, feature = "legacy_owner"))]`：
      `legacy_sim`、`legacy_systems`、`legacy_interaction`。
- [ ] 所有 leftover 方法呼叫（至少 `legacy_tick_*`、`legacy_handle_click`、
      leftover `update_village_and_raid_systems`／`update_vehicles_and_fishing`／
      `update_furnaces`／`update_hopper_power_states`）同樣 cfg。
      預設 `tick_simulation` 在 Embedded／Join 下不得再出現這些識別名。
- [ ] `handle_click` 的 `LegacyOwner` 臂：有 cfg 時呼叫 leftover；沒有 cfg 時
      `debug_assert`／編譯期不可達，**不得**變成第三條活世界 mutate。
      執行前再 grep `legacy_handle_click`。
- [ ] `State::apply_mutation_batch` 若仍無生產呼叫：與 leftover 一起 cfg。
      `src/world_mutation.rs` 模組留下（自己的單元測仍要跑）。
- [ ] `bootstrap.rs` leftover `SaveQueue`／`SaveManager`／snapshot worker 建構
      （`is_client || in_process_authority` 的 else 臂）同樣 cfg。
      Embedded／Join 仍是 `None`，不得突然又建第二個存檔工人。
- [ ] `PresentationTopology::LegacyOwner` **留下**。`presentation_inventory_policy`
      的 `from(&Singleplayer, false)` 測試期望值不變。
- [ ] `cargo check --bin icraft`（不開 feature、非 `--tests`）的 rustc JSON
      **不得**出現 `legacy_sim.rs`／`legacy_systems.rs`／`legacy_interaction.rs`。
- [ ] `cargo test --bin icraft` 仍編 leftover（因為 `cfg(test)`）。不得為了過關
      改測試期望值。
- [ ] `ARCHITECTURE.md` leftover 句改成：選單啟動編不到 leftover 本體；
      leftover 只在 `cfg(test)` 或 feature `legacy_owner` 編譯。

## 預計檔案與測試

- 修改：`Cargo.toml`、`src/state.rs`、`src/presentation/bootstrap.rs`、
      必要時 leftover 三檔檔頭 rustdoc、`ARCHITECTURE.md`。
- 測試：
  - `cargo check --bin icraft`
  - `cargo check --bin icraft-server`
  - `cargo test --bin icraft interpolation_midpoint_and_clamps gpu_timestamp_state_tests mesh_invalidation -- --test-threads=1`
  - `cargo test --bin icraft multiplayer_host_keeps_world_ticks singleplayer_pause -- --test-threads=1`
  - `cargo test --lib presentation_inventory_policy::`
  - `cargo test --lib world_mutation::`
  - `cargo test --test review_hardening_embedded_presentation -- --test-threads=1`
  - rustc JSON 證據：`cargo check --bin icraft --message-format=json` 不含三個 leftover 檔

## 建議階段

1. 對 `legacy_tick_`、`legacy_handle_click`、`apply_mutation_batch`、`SaveQueue`
   再 grep 一次，列出每個呼叫的 cfg 需求。
2. 先 cfg 三個 `#[path]` 模組，修編譯錯誤（一定是漏包的呼叫）。
3. cfg bootstrap leftover 存檔工人。跑 embedded presentation。
4. 確認未開 feature 的 `cargo check --bin icraft` 不含 leftover 檔。更新架構句。

## 不在本計劃

- 刪 leftover 檔或 `LegacyOwner` variant。
- 抽 `EmbeddedRuntimeBridge`／`handle_single_network_event`（28）。
- 改 `is_authoritative()` 謂詞（25）。
- 把 `SpawningSystem`／`Brain` cfg（22）。
- 開 workspace crate、把 `rodio` 改 optional。
