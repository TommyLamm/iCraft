# Plan26 — `AuthorityCore` tick／dispatch／portals 拆檔

## 定位

`src/authority/mod.rs` 約 3,967 行。子模組已有 `combat`／`contract`／`fishing`／
`interest`／`mining`／`transactions`，但合成根仍塞三塊不相干的 `impl`：

| 約略行 | 內容 |
| --- | --- |
| 168–436 | 建構、world map、session register、`activate_dimension` |
| 437–924 | `tick`（每維度 session domain／mining／portal／`ServerWorld::tick`） |
| 925–1114 | `execute_portal_dimension_transfer` |
| 1115–2308 | `submit_request` 單一 dispatch + 各 operation 本體 |

公開方法必須留在 `AuthorityCore`（與 16 對 `ServerRuntime` 相同：子檔是
`impl AuthorityCore`，不是新的 facade 型別）。

這是機械搬移。checksum 順序、BTreeMap 維度迭代、`activate_dimension` 只當
request-routing helper，都已由 Plan 09／hardening 鎖定。

## 前置

03、09、12、16 已完成。可與 27、29、30 並行。不要跟 22 以外的 authority 檔搶
`mod.rs` 的 use 清單——22 不改這個檔。

## 精確 acceptance

- [ ] `src/authority/mod.rs` 不再同時定義 `tick` 全文、`submit_request` 全文、
      以及 portal transfer 全文。至少拆出（名稱可微調，責任不可混）：
      - `tick.rs`：`AuthorityCore::tick` 與它私有的 per-dimension helpers
      - `dispatch.rs`：`submit_request` 與 operation match 臂（臂可繼續呼叫
        既有 `combat`／`mining`／`transactions`／`fishing`）
      - `portals.rs`：`execute_portal_dimension_transfer` 及只被它用的 helper
- [ ] 公開 API 仍是 `AuthorityCore::{tick, submit_request, execute_portal_dimension_transfer,
      world, world_mut, with_world, register_session, …}`。不得新增第二個核心型別。
- [ ] `tick` 仍按 `BTreeMap` key 走每個 loaded dimension，結束時把
      `active_dimension` 設回 tick 開始的值。不得改成 `HashMap`。
- [ ] `submit_request` 仍先驗證再 mutate；`BlockUse` 仍 `Unsupported`。
      spectator 拒絕集合不得縮小。
- [ ] 不得從 wire 刪或重排 `GameplayOperation` variant。
- [ ] 既有測試期望值不變。

## 預計檔案與測試

- 新增：`src/authority/tick.rs`、`src/authority/dispatch.rs`、
      `src/authority/portals.rs`（名稱可微調）。
- 修改：`src/authority/mod.rs`（`mod` + 留下 world map／session API）。
- 測試：
  - `cargo test --lib authority::`
  - `cargo test --test review_hardening_invariants -- --test-threads=1`
  - `cargo test --test review_hardening_block_use_rejected -- --test-threads=1`
  - `cargo test --test plan31_authoritative_block_actions -- --test-threads=1`
  - `cargo test --test plan32_progression_travel -- --test-threads=1`
  - `cargo test --test runtime_topology_parity -- --test-threads=1`

## 建議階段

1. 先搬 `tick`（呼叫點少、順序敏感）。跑 invariants。
2. 搬 portal transfer。跑 Plan32。
3. 搬 `submit_request`。跑 block-use rejected 與 Plan31。

## 不在本計劃

- 合併兩個 session 型別。
- 把 `InterestSet` 搬進 core。
- 拆 `server_runtime.rs`（27）。
- 改固定 tick 內容或刷怪／AI。
