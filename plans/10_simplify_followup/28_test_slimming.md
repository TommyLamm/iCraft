# Plan28 — 測試瘦身：拓撲三重跑、leftover fixture、sleep、roundtrip 表、存檔 fixture

## 定位

09 波 16 收 `request()`／`TestServer` 複本。其餘：

| 項目 | 證據 | 收益 |
| :--- | :--- | :--- |
| 拓撲三重跑 | `tests/runtime_topology_parity.rs`（946 行）`TopologyHarness` 76–121；`plan24_plan22_gameplay_vectors_match_all_runtime_topologies`（389）與 `plan28_dispenser_item_projection_matches_all_runtime_topologies`（756）各對三個 `(label, AuthorityTopology, TransportMode)` 跑同一 gameplay；Dedicated 用 `TransportMode::Disabled`（不是 socket）；`waterlogging_authority.rs` 160–162 同表 | 09 波 02 刪 `AuthorityTopology` 後 ~400–600 行 harness + 三倍迴圈消失 |
| `leftover_block_use` fixture | `BlockUse` 已刪，但 helper 仍以 leftover 命名並送「空手／錯手持 Place DiamondOre」期待 `InvalidState`：`runtime_topology_parity.rs` 38–57、837、884；`review_hardening_invariants.rs` 44、151；`headless_server_authority.rs` 262–305；`authority/mod.rs` 1691–1714 | ~80–120 行複製 Place 信封 |
| 多餘 sleep | 正本是 `tcp_harness.rs` `drive_until`／`wait_for_response`（273、298，5 ms step）。多餘：`plan32_progression_travel.rs` 489、507、572、613、662 在 `wait_for_response` **前** `sleep(50ms)`；`headless_server_authority.rs` `drive_pair_for` 88–100 睡到 duration 而非 predicate；`review_hardening_ingress.rs` 55；`network/client.rs` 1399；`network/server.rs` 測試 15 處 `sleep`（363–2367，含 300 ms） | 少 50–300 ms padding 與 flake |
| `protocol.rs` ~40 個相同 roundtrip | 1608–2709：`let p = Packet::…; assert_eq!(decode(encode(p)), p)` ×~25；`fn v()` 存在卻每個仍手寫 `protocol_version: v()` | ~400–600 行 → 一張 `samples()` 表；對抗性手組 frame **保留** |
| `save/tests.rs` fixture | `unique_test_dir`（752–758）有 16 個使用者，但 186、251、293、449、494、520、645、1392、1499 九處仍手寫 `SystemTime` + `temp_dir().join(format!(...))`；20 欄 `PlayerData { … }` 字面量 ×4（34、159、~1588）而 `PlayerData::from_state`（`format.rs` 654）存在；`LevelData { spawn_x: 8, … version: 2, ..Default }` 同樣複製 | ~150–250 行 |
| `network/server.rs` inline 套件 | 生產 ~290 行；測試 ~2,080 行，與 `plan30_real_transport_acceptance.rs`（933 行）的 join／idempotency 重疊 | 去重疊 ~200–400 行（TestServer 合併是 09 波 16） |
| `inventory/tests.rs` | 12 個 `Inventory::new()`／`new_creative()` 無共用 fixture；metadata stack 建構複製（35–45 vs 102–106） | ~30–50 行 |

## 前置

09 波 02（`AuthorityTopology` 刪）、09 波 16（`request()`／`TestServer` 合併）。Plan 27 若先搬測試模組，本計劃在新位置改。

## 精確 acceptance

- [ ] `TopologyHarness` 與三重迴圈刪除；每個 gameplay vector 只在 embedded `ServerRuntime` 跑一次；socket 覆蓋由 Plan30 一個 listen smoke 承擔；`waterlogging_authority.rs`／`difficulty_authority.rs` 拓撲斷言同步。
- [ ] `leftover_block_use*` helper 改名 `rejected_place`，集中到 `tests/common`；四份複本縮成一個 reject + no-mutation 測試。
- [ ] 上列 `sleep` 刪除或改 `drive_until` predicate；`drive_pair_for(Duration)` 改 predicate 版本。
- [ ] `protocol.rs` roundtrip 改表驅動；crafted-length／bounds／未知 enum 等命名測試保留；對抗性 frame 維持手組。
- [ ] `save/tests.rs`：`sample_player()`／`sample_level()` 一份；全部 world dir 走 `unique_test_dir`；crash-child env-var 路徑不動。
- [ ] `network/server.rs` 測試只留唯一 wire／capacity 用例；重複 Plan30 的 join／idempotency 刪。
- [ ] `inventory/tests.rs` 一個 `named_stack()` helper。
- [ ] `cargo test` 全綠；總測試時間對比記錄。

## 預計檔案與測試

- 改：`tests/runtime_topology_parity.rs`、`tests/waterlogging_authority.rs`、`tests/difficulty_authority.rs`、`tests/review_hardening_invariants.rs`、`tests/headless_server_authority.rs`、`tests/plan32_progression_travel.rs`、`tests/review_hardening_ingress.rs`、`tests/common/{mod,tcp_harness}.rs`、`src/authority/mod.rs`（測試）、`src/network/{protocol,server,client}.rs`（測試）、`src/save/tests.rs`、`src/inventory/tests.rs`
- 驗證：`cargo test`（全量）；`cargo test --test plan30_real_transport_acceptance`

## 建議階段

1. sleep 清除（最小、立即少 flake）。
2. `leftover_block_use` fixture。
3. 拓撲三重跑。
4. protocol roundtrip 表。
5. save／inventory fixture。
6. server.rs 去重疊。

## 不在本計劃

- 刪對抗性手組 frame（README §5）。
- `SimHarness`（Plan 01）。
