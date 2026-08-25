# Plan22 — 未接線 AI／刷怪原型改 `cfg(test)`

## 定位

Plan 01 只在 rustdoc 寫明「未接到權威 tick」。兩個原型仍是 `lib.rs` 的
`pub(crate) mod`，因此 `icraft-server` 與 `cargo check --lib` 都編譯它們：

| 符號 | 檔案 | 生產呼叫（掃描當日，執行前再 grep） |
| --- | --- | --- |
| `Brain`、`Goal`、`BoundedPathfinder` | `src/ai/` | 只有 `ai/mod.rs` re-export。沒有 `new_for_entity`／`tick` |
| `SpawningSystem` | `src/spawning.rs` | 只有本檔單元測。線上是 `mob::spawn_mobs`／`passive_mob::spawn_passive_mobs` |

`AuthorityBoundary` 已是 `#[cfg(test)]`（`src/authority/mod.rs`）。這兩個模組用
同一招。**不得**把它們接到 `ServerWorld::tick_entities`／`update_mobs`（距離／
上限／追擊規則不同，合併會改玩法）。

## 前置

無。可與 21、23、24 並行。不要跟 26 搶 `authority/mod.rs`（本計劃不改該檔，
只拿 `AuthorityBoundary` 當 cfg 範例）。

## 精確 acceptance

- [x] `src/lib.rs`：`ai`、`spawning` 改
      `#[cfg(any(test, feature = "harness"))] pub(crate) mod`（選用 `#[cfg(any(test, feature = "harness"))]`
      保持與 `sim_harness`、`final_acceptance`、`microbench` 一致）。預設 `cargo check --lib`
      與 `cargo check --bin icraft-server` **不再**編譯 `src/ai/`、`src/spawning.rs`。
- [x] 執行前再 grep `Brain`、`SpawningSystem`、`BoundedPathfinder`、`crate::ai`、
      `crate::spawning`，確認全無生產呼叫。
- [x] 不得改 `mob::spawn_mobs`、`passive_mob::spawn_passive_mobs`、
      `ServerWorld::tick_entities` 的數字或呼叫順序（未改動）。
- [x] `src/navigation.rs`（羅盤／地圖）留下，不得跟 `ai/navigation.rs` 合併或改名（保留原樣）。
- [x] `cargo test --lib spawning::` 與 `cargo test --lib ai::`（若測試跟著模組走）
      仍通過。期望值不變。
- [x] rustdoc 保留「未接到權威 tick；線上入口是 …」。

## 預計檔案與測試

- 修改：`src/lib.rs`，必要時 `src/ai/mod.rs`／`src/spawning.rs` 檔頭。
- 測試：
  - `cargo check --bin icraft-server`（通過，不再有 `ai` / `spawning` 警告）
  - `cargo check --lib`（通過）
  - `cargo test --lib spawning::`（通過，4 passed）
  - `cargo test --lib ai::`（通過，0 passed）
  - `cargo test --lib mob::`（通過，15 passed）
  - rustc JSON：`cargo check --bin icraft-server --message-format=json` 不含
    `src/ai/`、`src/spawning.rs`

## 建議階段

1. 再 grep 一次上表符號。有生產呼叫就停，不要硬 cfg。（已執行確認）
2. cfg 兩個 `mod`。`cargo check --bin icraft-server`。（已完成）
3. 跑 `spawning`／`mob` 單元測。補 JSON 證據。（已完成）

## 不在本計劃

- 把 `SpawningSystem` 接到 `ServerWorld`。
- 把 `update_mobs` 改成 `Brain`。
- leftover cfg（21）。
- `navigation.rs` 改名為 maps。

