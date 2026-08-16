# Plan02 — 表現層拓撲改成單一 enum

## 定位

拓撲現在用兩個重疊的布林表達：

- `State::is_authoritative()` = `!role.is_join_client()`。Listen Host 也是 true。
- `State::has_in_process_runtime()` = `embedded_runtime.is_some()`。

呼叫端必須自己發明第三個謂語：

```12388:12388:src/state.rs
        let authoritative = self.is_authoritative() && !has_in_process_runtime;
```

`src/presentation_inventory_policy.rs` 的四個 helper 吃同一對布林：

- `should_mutate_presentation_world(has_in_process_runtime, is_client)`
- `presentation_chunk_load_policy`
- `presentation_may_generate_chunks`
- `presentation_may_mutate_chunks`

後兩個在生產碼幾乎不用，只活在測試／`menu.rs` 測試。`is_client` 這個名字也誤導：它其實是 Join，不是「任何客戶端」。

`ARCHITECTURE.md` 已定義三種 runtime：Singleplayer（embedded）、Listen host（embedded + TCP）、Join client（無權威）。缺少的是表現層對這三種（外加 leftover「無 runtime 的世界擁有者」）的單一型別。

## 前置

無。06／10 會消費本計劃的 enum，但本計劃不得預先拆 `handle_click` 或搬 `tick_simulation`。

## 精確 acceptance

- [x] 新增（或擴）一個公開、可 Copy 的 enum，語意至少覆蓋：
  - `Embedded`：Singleplayer／Host，有 in-process `ServerRuntime`
  - `JoinClient`：只吃投影
  - `LegacyOwner`：無 runtime 且非 Join（leftover 世界擁有者；現役選單啟動不應走到）
- [x] `State` 用 `role` + `embedded_runtime.is_some()` **導出**這個 enum，不得再讓呼叫端自己做 `a && !b`。
- [x] `presentation_inventory_policy.rs` 的決策函式改吃該 enum（或由其衍生的窄函式）。玩家物品欄 + Embedded 的 writeback 例外必須原樣保留，既有 4 個 policy 單元測繼續鎖定。
- [x] `should_mutate_presentation_world` 的布林簽名若還留著，只能是薄 wrapper，並標 deprecated／「測試相容」。生產呼叫改走 enum。
- [x] `presentation_may_generate_chunks`／`presentation_may_mutate_chunks` 若與 `presentation_chunk_load_policy` 等價：刪生產呼叫，測試改測一個函式。
- [x] 不得改變任何 mutation／投影／writeback 行為。不得刪 leftover `LegacyOwner` 路徑。

## 預計檔案與測試

- 修改：`src/presentation_inventory_policy.rs`、`src/state.rs`（閘門呼叫點，不拆大函式）、必要時 `src/menu.rs` 測試。
- 測試：
  - `cargo test --lib presentation_inventory_policy::`
  - `cargo test --bin icraft presentation_inventory_policy`
  - `cargo test --test review_hardening_embedded_presentation -- --test-threads=1`
  - `cargo test --test review_hardening_join_projection -- --test-threads=1`
  - `cargo check --all-targets`

## 建議階段

1. 讀完 `presentation_inventory_policy.rs` 與它的單元測，把現有真值表抄進計劃證據（Embedded／Join／Legacy × 世界 mutate／inventory writeback／chunk load）。
2. 先加 enum + `from(role, has_runtime)` + 用既有測試鎖真值表（測試應仍然通過，只是改呼叫簽名）。
3. 把 `State` 的導出 helper 補上，替換 `tick_simulation`、inventory policy、chunk load 的布林對。一次只改閘門，不搬函式體。
4. 刪等價的 `presentation_may_*` 生產面。
5. 跑窄測試。

## 不在本計劃

- 重寫 `handle_click`／`handle_inventory_click`（06）。
- 把 `tick_simulation` 的 leftover body 搬到新檔（10）。
- 刪 `LegacyOwner` 或禁止無 runtime 建構 `State`。
- 改 `sync_authority_gameplay_from_local` 的允許欄位。

## 實作與證據

### 改了什麼

- `src/presentation_inventory_policy.rs`：新增公開 `Copy` enum `PresentationTopology { Embedded, JoinClient, LegacyOwner }`。
  - 建構：`from(role, has_in_process_runtime)`（Join 優先），以及測試相容 `from_bools(has_rt, is_client)`。
  - 決策收斂到 enum methods：`should_mutate_world`（僅 LegacyOwner）、`should_sync_inventory`（僅 Embedded）、`inventory_decision`、`should_writeback_after_inventory_click`、`chunk_load_policy`。
  - 真值表不變：

    | | Embedded | JoinClient | LegacyOwner |
    |---|---|---|---|
    | should_mutate_world | false | false | true |
    | should_sync inventory | true | false | false |
    | writeback PlayerInventory | true | false | false |
    | writeback other/None | false | false | false |
    | ContainerSlot | SendAuthorityOp | SendAuthorityOp | LocalMutate |
    | PlayerInventory | LocalMutate | Reject | LocalMutate |
    | Workstation/Pickup/Farmland/Unsupported | Reject | Reject | LocalMutate |

  - 舊布林 helper（`should_mutate_presentation_world` 等）改成薄 wrapper，標 deprecated／測試相容；`review_hardening_embedded_presentation` 仍用字面量呼叫。
  - `presentation_may_*` 改成 `presentation_chunk_load_policy == GenerateLocally` 的薄別名。
- `src/state.rs`：新增 `State::presentation_topology()`（`role` + `embedded_runtime.is_some()`）。
  - 第三謂語 `is_authoritative() && !has_in_process_runtime` 及其反向全部改成 `presentation_topology().is_legacy_owner()`。
  - inventory／writeback／chunk load／`apply_block_changes` 閘門改走 topology。Embedded submit、Join no-op、LegacyOwner 本地 mutate。
  - 刪 `State::presentation_may_mutate_chunks`。
  - 未改 `handle_click`／`handle_inventory_click` 函式體（只換閘門謂語）。未動 `sync_authority_gameplay_from_local` 允許欄位。未刪 `LegacyOwner`。
- `src/menu.rs` 與 `tests/review_hardening_join_projection.rs`：`may_*` 測試改測 `presentation_chunk_load_policy`。未從 menu 再 export `PresentationTopology`。

### 測試

| 指令 | 結果 |
|---|---|
| `cargo test --lib presentation_inventory_policy::` | 4 passed |
| `cargo test --bin icraft presentation_inventory_policy` | 4 passed |
| `cargo test --test review_hardening_embedded_presentation -- --test-threads=1` | 4 passed |
| `cargo test --test review_hardening_join_projection -- --test-threads=1` | 3 passed |
| `cargo check --all-targets` | ok（既有 dead_code 警告，無新錯誤） |

### 剩餘缺口

- `is_authoritative()`／`has_in_process_runtime()` 仍是 accessor；Join-only（`!is_authoritative()` 單獨）與 runtime-only persist early-return 依計劃保留。
- 布林 wrapper 與 `presentation_may_*` 仍公開，供 integration／舊呼叫；desktop bin 會對它們發 dead_code（與其他 leftover API 同類）。
- Plan 06 仍負責拆 `handle_click`／`handle_inventory_click` 本體；Plan 10 仍負責搬 `tick_simulation` leftover body。
