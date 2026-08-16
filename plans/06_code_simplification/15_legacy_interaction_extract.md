# Plan15 — leftover 互動從 `state.rs` 抽出

## 定位

Plan 10 做到檔案邊界：`gpu_terrain`／`interpolation`／`network_inbound`／`legacy_sim`／`frame`／`bootstrap`。
`state.rs` 仍約 19,700 行。10 自己的缺口寫明：leftover click／inventory／`handle_single_network_event`／
`EmbeddedRuntimeBridge` 沒搬。

現役選單啟動永遠是 `EmbeddedRuntimeBridge`（`State::new` 對 SP／Host 失敗就 panic）。
`legacy_handle_click` 只在 `PresentationTopology::LegacyOwner`。menu 到不了，但仍與活
`handle_click` 坐在同一個 3k 行區間，修點擊會掃到第二套世界。

掃描當日（執行前再確認行號）：

| 符號 | 約略位置 | 閘門 |
| --- | --- | --- |
| `handle_click` | `state.rs:14044` | 活：Join／Embedded |
| `legacy_handle_click` | `:14180` | `LegacyOwner` |
| `legacy_try_use_held_item` | `:15193` | leftover |
| `set_block_and_broadcast` | `:12210` | leftover 廣播 |
| `calculate_mining_time` | `:11721` | leftover；權威是 `authority/mining.rs` |
| 村莊／載具／熔爐／漏斗 leftover | `:10226–10970` | 已有 `is_legacy_owner()` 早退 |

本計劃沿用 10 的 `#[path]` 模式（要碰 `State` 私有欄位）。成功標準是檔案邊界，不是刪 leftover、也不是拆 300 個欄位。

## 前置

- 02 已合併：用 `PresentationTopology`／`is_legacy_owner()`，不要再發明布林對。
- 10 已合併：`legacy_sim.rs` 已存在；本計劃是它的互動對應物。
- **建議** 06 已合併：不要重寫活 `handle_click`。

## 精確 acceptance

- [ ] 新增 `src/presentation/legacy_interaction.rs`，以 `#[path]` 掛在 `state` 下（與 `legacy_sim` 相同）。
      至少搬走：`legacy_handle_click`、`legacy_try_use_held_item`、leftover 物品欄／容器開啟、
      leftover `break_block`／`set_block_and_broadcast`。活 `handle_click` 的 match 臂只留閘門呼叫。
- [ ] 新增 `src/presentation/legacy_systems.rs`（或同等），搬已閘在 `is_legacy_owner()` 的
      村莊／raid、載具／釣魚、熔爐／漏斗本體。`tick_simulation` 仍是短編排器。
- [ ] 閘門語意不變：有 embedded runtime 或 Join → 不進 leftover。`LegacyOwner` 行為 bit-identical。
- [ ] 不得刪 `legacy_tick_*`。不得把權威行為搬進這些檔。
- [ ] 不得重排 `tick_simulation` 裡 leftover 呼叫順序（10 已鎖：item-use → world systems →
      night skip → leaf → oxygen → owned world）。
- [ ] `State` 欄位留在原 struct。`EmbeddedRuntimeBridge` **不**在本計劃搬（另案）。
- [ ] `handle_single_network_event` **不**在本計劃搬（另案）。
- [ ] 手寫 UI／render pass 不碰。
- [ ] `cargo check --bin icraft`；desktop 既有 `state` 單元測與 embedded presentation 通過。

## 預計檔案與測試

- 新增：`src/presentation/legacy_interaction.rs`、`src/presentation/legacy_systems.rs`。
- 修改：`src/state.rs`（`#[path]` + 閘門）、必要時 `src/presentation/mod.rs` 註解
      （這兩個檔跟 `legacy_sim` 一樣是 `state` 的孩子，不是 `presentation` 的 `pub(crate) mod`）。
- 測試：
  - `cargo check --bin icraft`
  - `cargo check --bin icraft-server`（仍不得編 `menu.rs`）
  - `cargo test --bin icraft interpolation_midpoint_and_clamps gpu_timestamp_state_tests mesh_invalidation -- --test-threads=1`
  - `cargo test --bin icraft multiplayer_host_keeps_world_ticks singleplayer_pause -- --test-threads=1`
  - `cargo test --test review_hardening_embedded_presentation -- --test-threads=1`
  - `cargo test --lib presentation_inventory_policy::`

## 建議階段

1. 列出 leftover 互動函式與每個 `is_legacy_owner()` 早退點。先搬 **只被 leftover 呼叫** 的 helper。
2. 搬 `legacy_handle_click` 整段。每搬完 `cargo check --bin icraft`。
3. 搬 leftover systems。`tick_simulation` 只留呼叫。
4. 跑窄測試。證據裡列出未搬的 leftover（inbound apply、bridge、SaveQueue）。

## 不在本計劃

- 刪 `LegacyOwner`、`legacy_sim`、desktop `SaveQueue`。
- 抽 `EmbeddedRuntimeBridge`、`handle_single_network_event`、`update_chunks`。
- 把 leftover 挖礦時間改接 `authority::mining::mining_time_seconds`（數字相同才可另案；本計劃只搬家）。
- 拆 `menu.rs`。
- 重排 render pass。
