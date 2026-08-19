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

- [ ] `src/lib.rs`：`ai`、`spawning` 改
      `#[cfg(any(test, feature = "harness"))] pub(crate) mod`（或純 `#[cfg(test)]`，
      證據寫明選哪個）。預設 `cargo check --lib` 與 `cargo check --bin icraft-server`
      **不得**編譯 `src/ai/`、`src/spawning.rs`。
- [ ] 執行前再 grep `Brain`、`SpawningSystem`、`BoundedPathfinder`、`crate::ai`、
      `crate::spawning`。出現新的生產呼叫就從本計劃拿掉該項，寫進證據「未 cfg 原因」。
- [ ] 不得改 `mob::spawn_mobs`、`passive_mob::spawn_passive_mobs`、
      `ServerWorld::tick_entities` 的數字或呼叫順序。
- [ ] `src/navigation.rs`（羅盤／地圖）留下，不得跟 `ai/navigation.rs` 合併或改名。
- [ ] `cargo test --lib spawning::` 與 `cargo test --lib ai::`（若測試跟著模組走）
      仍通過。期望值不變。
- [ ] rustdoc 保留「未接到權威 tick；線上入口是 …」。

## 預計檔案與測試

- 修改：`src/lib.rs`，必要時 `src/ai/mod.rs`／`src/spawning.rs` 檔頭。
- 測試：
  - `cargo check --bin icraft-server`
  - `cargo check --lib`
  - `cargo test --lib spawning::`
  - `cargo test --lib ai::`（若 `cfg(test)` 後仍看得到）
  - `cargo test --lib mob::`
  - rustc JSON：`cargo check --bin icraft-server --message-format=json` 不含
    `src/ai/`、`src/spawning.rs`

## 建議階段

1. 再 grep 一次上表符號。有生產呼叫就停，不要硬 cfg。
2. cfg 兩個 `mod`。`cargo check --bin icraft-server`。
3. 跑 `spawning`／`mob` 單元測。補 JSON 證據。

## 不在本計劃

- 把 `SpawningSystem` 接到 `ServerWorld`。
- 把 `update_mobs` 改成 `Brain`。
- leftover cfg（21）。
- `navigation.rs` 改名為 maps。
