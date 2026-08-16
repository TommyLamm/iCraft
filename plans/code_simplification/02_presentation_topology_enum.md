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

- [ ] 新增（或擴）一個公開、可 Copy 的 enum，語意至少覆蓋：
  - `Embedded`：Singleplayer／Host，有 in-process `ServerRuntime`
  - `JoinClient`：只吃投影
  - `LegacyOwner`：無 runtime 且非 Join（leftover 世界擁有者；現役選單啟動不應走到）
- [ ] `State` 用 `role` + `embedded_runtime.is_some()` **導出**這個 enum，不得再讓呼叫端自己做 `a && !b`。
- [ ] `presentation_inventory_policy.rs` 的決策函式改吃該 enum（或由其衍生的窄函式）。玩家物品欄 + Embedded 的 writeback 例外必須原樣保留，既有 4 個 policy 單元測繼續鎖定。
- [ ] `should_mutate_presentation_world` 的布林簽名若還留著，只能是薄 wrapper，並標 deprecated／「測試相容」。生產呼叫改走 enum。
- [ ] `presentation_may_generate_chunks`／`presentation_may_mutate_chunks` 若與 `presentation_chunk_load_policy` 等價：刪生產呼叫，測試改測一個函式。
- [ ] 不得改變任何 mutation／投影／writeback 行為。不得刪 leftover `LegacyOwner` 路徑。

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
