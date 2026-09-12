# 21 — 多維度權威拓撲與 Plan18 既有缺口

## 定位

- 優先級：P3，Plan18 parked-world review 的既有契約補強
- 前置條件：Plan18 `AuthorityCore`、`ServerWorld`、session gameplay contract 已存在
- 本計劃不新增玩法內容；只把既有權威路徑從單一 active world 收斂為可同時處理多維度，並記錄後續缺口
- 建議提交上限：2（Phase A 多維度核心；後續 B/C 各自另行拆分）

## Phase A — 多維度 AuthorityCore（本批次）

- [x] `AuthorityCore` 保留 `pub world` 作 active-world 相容視圖，並以 dimension-keyed world map 保存所有非 active `ServerWorld`；world/chunk/entity/revision/time 不互相 alias。
- [x] session 註冊會確保其 `SessionContract.dimension` 對應 headless world；`submit_request` 依認證 session dimension 路由 validate、dispatch、ACK 與 per-dimension revision gate。
- [x] fixed tick 依穩定 dimension 順序遍歷所有 loaded worlds，只餵相同 dimension 的 session；合併 mutation、session update、checksum 後恢復 tick 前的 active-world compatibility view。
- [x] revision 語義固定為 `(WorldMutation.dimension, revision)` namespace；`AuthoritySnapshot.revision` 僅為跨維度最大值相容摘要，client stale gate 必須使用 `revision_for_dimension`。
- [x] runtime interest fanout 使用 mutation dimension 與 `(dimension, revision)` dedup key；entity/block/container delta 不跨維度洩漏。
- [x] headless 雙 session 測試覆蓋 Overworld/Nether 同時 block mutation、tick、session snapshot、各自 world time/revision、active view restoration 與跨 active view request routing。

### Phase A persistence/interest contract

每個 dimension 的 chunk、block entity、entity 與 mutation revision index 仍由既有 `SaveManager` 以 dimension key 保存；AuthorityCore 的 world map 不複製 renderer state。Phase A runtime 已遍歷所有已載入維度做 save/restore，並合併一次 dimension-scoped revision index；完整多維度 reconnect/atomic failure matrix 仍未作為本批次完成條件，需在後續 persistence slice 以逐維度測試補證。Interest update 必須同時帶 dimension 與 revision；相同 revision number 在不同維度不是同一 mutation。

## Phase B — 既有玩法 authority domains（後續，不在本批次）

- [ ] fishing rod cast/loot/timeout 由 authority domain 真正結算，State 不再只 reject。
- [ ] furnace XP claim 與 crafting/enchanting/brewing/anvil 交易由 authority envelope 原子提交，包含 rich ItemStack metadata、XP、消耗與 rollback。
- [ ] combat damage source、armor/shield durability、knockback、death drops/XP/keep-inventory 與 respawn lifecycle 進入 session/world delta。

## Phase C — listen State/runtime 與拓撲驗收（後續，不在本批次）

- [ ] listen Host/Singleplayer 使用同一 in-process runtime scheduling path，並投影 dimension-aware snapshot/interest fanout，不建立第二套 local simulation。
- [ ] dedicated、listen、singleplayer common vectors 與 fault/duplicate/out-of-order/stale matrix 在三拓撲一致。
- [ ] multi-client GPU、30 分鐘 soak、dimension transfer reconnect 與真 transport delivery 完成人工驗收；headless 測試不代替實機證據。

## 驗證閘門

本批次至少執行：

```text
cargo test --lib authority::tests
cargo test --lib server_world::tests
cargo test --lib server_runtime::tests
cargo check --lib --bins
cargo check --release --locked
rustfmt --edition 2021 --check <owned source files>
git diff --check
```

完整 release suite、GPU/多人實機、Phase B/C 與多維度 reconnect/failure matrix 仍由後續批次負責，不能在本文件勾選或宣稱已通過。

## 本批次驗證紀錄（2026-08-11）

除上述最低閘門外，本批次亦補跑拓撲與持久化回歸：

- `cargo test --lib authority::tests`：14 passed。
- `cargo test --lib server_world::tests`：5 passed；malformed combat action 依目前 request bounds 語義拒絕為 `InvalidState`。
- `cargo test --lib server_runtime::tests`：13 passed。
- `cargo test --test authority_persistence`：3 passed。
- `cargo test --test headless_server_authority`：1 passed。
- `cargo test --test runtime_topology_parity`：4 passed。
- `cargo check --lib --bins`、`cargo check --release --locked`、rustfmt check 與 `git diff --check`：通過。

持久化測試透過 AuthorityCore session gameplay seam 設定 health；headless 測試透過明確的 server-authorized teleport seam 建立跨維度 interest 場景，沒有放寬一般 pose 速度/順序驗證。GPU/window、30 分鐘 soak、真 transport 與 Phase B/C 仍未驗收。

`cargo test --release` 額外全套執行首次為 785 passed、1 failed、3 ignored；唯一失敗是既有 `network::server::tests::transport_metrics_count_exact_successful_tcp_frames` 在跨執行緒 `record_outbound` 時序下偶發讀到 0。未修改該 metrics 路徑；isolated release 重跑 7 次為 6 passed、1 failed（第 7 次重現同一 race）。這不是本計劃 gate，列為後續 network metrics/測試穩定化計劃候選，不在此批次宣稱 release 全套通過。
