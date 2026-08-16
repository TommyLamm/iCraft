# Plan15 — 測試契約硬化

## 定位

- Headless 矩陣與 Plan30–34 真 TCP 是好的。問題是測試**批准後門**，且沒鎖住
  `ARCHITECTURE.md` 宣稱的不變式。
- `src/final_acceptance.rs` 的 `network_rows_keep_scenario_specific_blockers` 仍斷言
  Listen／Dedicated 因「缺 Plan31／32 ingress」而失敗。那些 plan 已有通過的 TCP 測試。
  此測試把「未完成」鎖死。
- `final_acceptance`／`sim_harness` 走 `ChunkManager` + 本地物品欄，不是
  `AuthorityCore`。它們會在 `cargo test` 跑，但不證明 20 Hz 權威。
- `tests/passive_mob_tests.rs` 是 `assert!(true)`。
- 到達順序 checksum、`InvalidDimension` 不變更、2 MiB frame、視野外不物質化、
  region cache-after-failure、join-client `State` 縫——審查時都不存在或只測到一半。
- 釣魚／熔爐／craft 在 Plan30、topology parity、authority_gameplay_domains 重複三遍；
  負向與守恆比較薄。

## 前置

- 01 與 02 至少已合併，否則本計劃會被迫寫「BlockUse 應成功」或跳過契約測試。
- 03、05、08、11 各自擁有自己的窄測試。本計劃**不重做**那些；只補跨計劃缺口與
  刪／改過時 acceptance。

## 精確 acceptance

- [ ] 刪除或改寫 `network_rows_keep_scenario_specific_blockers`：不得再要求
  Plan31／32 「缺失」。網路列改指向 Plan30–34（及本路線 01／02）的 TCP 向量。
- [ ] `final_acceptance`／`sim_harness` 文件與測試名改成 recipe／physics smoke，
  不再叫「acceptance」暗示權威閉環。
- [ ] 刪除 `tests/passive_mob_tests.rs` 佔位，或換成真實的高度範圍／已載入 chunk 生成斷言
  （若 09 已合併，用 signed-Y；否則至少 `assert` 能失敗）。
- [ ] 下列測試存在且會因回歸失敗（若已在 01／03／08／11 出現，本計劃只補缺、不複製）：
  1. `fixed_tick_checksum_is_independent_of_inbound_arrival_order`
  2. `invalid_dimension_envelope_is_rejected_without_world_or_inventory_mutation`
  3. `nether_mutations_do_not_invalidate_overworld_client_revision`
  4. `stale_block_place_does_not_consume_held_stack_or_create_drops`
  5. `failed_region_write_does_not_replace_in_memory_region_cache`（05／13 若已有則引用）
  6. `join_client_applies_revision_gated_projections_without_local_authority`
     （headless sink 即可，不建 wgpu `State`）
- [ ] `rejects_v4_handshake` 改名或刪重疊。`reserve_port()` TOCTOU：改為持有 listener
  或把已 bind socket 交給 server（至少 `tests/common/tcp_harness.rs`）。
- [ ] 本計劃結束時跑一次：
  `cargo test --test review_hardening_block_use_rejected --test plan31_authoritative_block_actions --test plan34_container_break_inventory_conservation`
  （名稱以實際為準）以及新檔。不要宣稱全 repo suite，除非你真的跑了。

## 預計檔案與測試

- 修改：`src/final_acceptance.rs`、`src/sim_harness.rs`（只改命名／註解／過時 assert）、
  `tests/passive_mob_tests.rs`、`tests/common/tcp_harness.rs`、
  `src/authority/mod.rs` 的弱 revision 測試、`src/network/server.rs` 測試名。
- 新增：缺的 invariant 測試放 `tests/review_hardening_invariants.rs` 或既有最接近的檔。

## 建議階段

1. 改／刪過時 final_acceptance blocker（避免繼續鎖「未完成」）。
2. 到達順序 checksum + InvalidDimension + 跨維 revision。
3. Join-client 投影 sink。
4. Harness port 與佔位測清理。
5. 在 README 把 15 標完成，並列出仍缺的 GPU／soak。

## 不在本計劃

- 實作 01–14 的生產修復。發現仍開著的後門 → 停下來列證據，不要在本計劃順便修
  BlockUse 或 restore。
- GPU／window／audio／DPI／R9 實機。
