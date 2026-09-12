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

- [x] `src/authority/mod.rs` 不再同時定義 `tick` 全文、`submit_request` 全文、
      以及 portal transfer 全文。至少拆出（名稱可微調，責任不可混）：
      - `tick.rs`：`AuthorityCore::tick` 與它私有的 per-dimension helpers
      - `dispatch.rs`：`submit_request` 與 operation match 臂（臂可繼續呼叫
        既有 `combat`／`mining`／`transactions`／`fishing`）
      - `portals.rs`：`execute_portal_dimension_transfer` 及只被它用的 helper
- [x] 公開 API 仍是 `AuthorityCore::{tick, submit_request, execute_portal_dimension_transfer,
      world, world_mut, with_world, register_session, …}`。不得新增第二個核心型別。
- [x] `tick` 仍按 `BTreeMap` key 走每個 loaded dimension，結束時把
      `active_dimension` 設回 tick 開始的值。不得改成 `HashMap`。
- [x] `submit_request` 仍先驗證再 mutate；`BlockUse` 仍 `Unsupported`。
      spectator 拒絕集合不得縮小。
- [x] 不得從 wire 刪或重排 `GameplayOperation` variant。
- [x] 既有測試期望值不變。

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

## 實作與證據

### 1. 模組拆分與責任劃分

- `src/authority/tick.rs`：
  - `AuthorityCore::tick`：依 `BTreeMap` 鍵迭代所有 loaded dimensions，維護每維度 session domain、mining、portal travel 與 `ServerWorld::tick`，並於結束時還原 `active_dimension`。
  - `tick_session_domains`：冷卻、盾牌、釀造、釣魚計時推進。
  - `tick_mining`、`clear_mining_progress`、`commit_mining_break`：挖掘進度推進與方塊破壞結算。
  - `aggregate_dimension_checksums`：跨維度校驗和聚合。
- `src/authority/portals.rs`：
  - `tick_portal_travel`：傳送門接觸時間累加、冷卻判定與傳送門觸發。
  - `execute_portal_dimension_transfer`：維度傳送意圖生成、session 維度切換與位移。
- `src/authority/dispatch.rs`：
  - `AuthorityCore::submit_request`：請求驗證、序列號與 revision 檢查、spectator 權限檢查、`BlockUse` 拒絕、單一 operation dispatch 與結果/快取處理。
  - operation match 分流與實作：`apply_item_use`、`apply_trade`、`apply_mount`、`apply_block_action`、`apply_fluid_use`、`apply_fishing`、`apply_container_click`、`apply_authoritative_combat`、`apply_transaction_operation`、`apply_command`。
  - 輔助函式：`rejected`、`reject_for_session`、`stack_from_slot`、`held_slot_index`、`preserves_brew_locks` 等。
- `src/authority/mod.rs`：
  - 保留核心結構體 `AuthorityCore`、`AuthorityConfig`、`DimensionTransferIntent`、測試輔助 `AuthorityBoundary`。
  - 保留世界地圖、維度管理、Session 註冊與查詢、規則更新、容器關閉事件等生命週期方法。

### 2. 測試與驗證證據

- `cargo test --lib authority::`：56/56 通過。
- `cargo test --test review_hardening_invariants -- --test-threads=1`：6/6 通過。
- `cargo test --test review_hardening_block_use_rejected -- --test-threads=1`：3/3 通過。
- `cargo test --test plan31_authoritative_block_actions -- --test-threads=1`：3/3 通過。
- `cargo test --test plan32_progression_travel -- --test-threads=1`：5/5 通過。
- `cargo test --test runtime_topology_parity -- --test-threads=1`：6/6 通過。

### 3. 留下的缺口

- `ServerRuntime` 投影與 ingress 拆檔留待 Plan 27 處理。
- `EmbeddedRuntimeBridge` 與 inbound 抽檔留待 Plan 28 處理。

