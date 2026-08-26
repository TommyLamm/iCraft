# iCraft 第二波代碼簡化與瘦身路線 (Ponytail Review Wave 2)

> 來源：2026-08-25 全專案深度 Ponytail Review 審查（對照 `ARCHITECTURE.md`）。
> 代碼基線：`tommy-dev`（在 Plans 01–06 全部落地後進行的第四次全庫深掃）。
> 核心目標：**行為 100% 不變**、**協議/存檔 100% 相容**、**測試期望值零破壞**的前提下，刪除已證實死碼、消除重複實現、收斂 YAGNI 抽象、替換手寫標準庫/原生功能，使專案淨減 **~8,000+ 行**代碼。

---

## 1. 怎麼用

一次只執行一份編號計劃。把 [prompt.md](prompt.md) 裡的 `{{PLAN_FILE}}` 換成該檔路徑。
完成並留下窄測試證據後，另開任務再做下一份。

依賴只在各計劃的「前置」寫明。沒寫前置的計劃可以與其他同級項並行：

- **P0 死碼刪除組**（01、02、03、04、05、06）：彼此檔案獨立，可完全並行執行。
- **P1 去重與收斂組**（07、08、09、10、11、12、13）：
  - 07、08 涉及 `src/state.rs`，建議在 01 完成後進行。
  - 09、10 建議在 05、06 完成後進行。
  - 11、12、13 彼此無衝突，可並行。
- **P2 結構收斂組**（14、15、16）：
  - 14 涉及 `src/state.rs` 較大規模的遺留模擬遷移，必須等 01、07、08 全部完成後再進行。
  - 15、16 可獨立並行。

---

## 2. 本路線聚焦解決的問題

在前六波重構完成後，權威架構已完全確立（`ServerRuntime` → `AuthorityCore` → `ServerWorld`），但倉庫中依然殘留著顯著的複雜度與過度工程：

1. **已證實死碼與遺留管線 (Dead Code & Pipelines)**：
   - `State` 內遺留未被任何 pass 綁定的 WGPU 管線（`render_pipeline`, `trans_pipeline`）與頂點/索引/scratch 緩衝區。
   - `src/ai/` 與 `src/spawning.rs` 等未接線的舊原型代碼。
   - `src/save/legacy_queue.rs` 946 行舊多線程存檔佇列。
   - 網路層 6 個從未由 Ingress 產生的 `ServerToHost` 死變體與下游對應的死 handler。
2. **跨模組重複實現 (Duplication & Reinventing Wheels)**：
   - 挖掘掉落計算在 `state.rs` 與 `authority/mining.rs` 兩處各寫了 ~190 行。
   - 向量字型在 `state.rs` 手寫 350 行線段表，而 `menu.rs` 已有完整的 5x7 點陣表。
   - 實體與玩家在 `entity.rs` 與 `physics.rs` 重複手寫軸向碰撞分離。
   - 7 個模組各自手寫 SplitMix64 / Hash 常數與演算法。
3. **過度設計與樣板堆疊 (Over-engineering & Boilerplate)**：
   - `projection.rs` 9 個投射方法重複相同的本地/網路分發樣板。
   - `resources.rs` 手寫了完整的 ZIP 解析器與複雜的拓撲排序。
   - `network/server.rs` 手寫 10ms 定時器輪詢同步通道。
4. **邊界混雜 (Boundary Violations)**：
   - `state.rs` 頂層依然殘留 1,600+ 行未標記 `cfg` 的戰鬥、指令與紅石遺留運算。

---

## 3. 優先級劃分

| 級別 | 意義 | 計劃編號 |
| :--- | :--- | :--- |
| **P0** | 刪除已證實死碼、未接線原型與死通道，零風險高回報 | 01 – 06 |
| **P1** | 去重、標準庫/原生替代、樣板提煉與會話解耦 | 07 – 13 |
| **P2** | 表現層遺留隔離、結構收斂與測試腳手架精簡 | 14 – 16 |

---

## 4. 執行包索引

| # | 單獨執行文件 | 優先級 | 預估淨減行數 | 前置依賴 | 狀態 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 01 | [State 廢棄 GPU 管線與頂點緩衝清理](01_state_dead_gpu_pipelines_and_buffers.md) | P0 | ~-360 行 | 無 | 已完成 |
| 02 | [刪除未接線的 AI 與生成系統原型](02_unwired_ai_and_spawning_prototypes.md) | P0 | ~-825 行 | 無 | 已完成 |
| 03 | [刪除 mob 與 passive_mob 遺留渲染器更新循環](03_legacy_mob_update_loops.md) | P0 | ~-481 行 | 無 | 已完成 |
| 04 | [刪除 save/legacy_queue 遺留存檔佇列](04_legacy_save_queue_cleanup.md) | P0 | ~-946 行 | 無 | 已完成 |
| 05 | [刪除網路 dead 通道變體與未使用的 Packet 變體](05_network_dead_channels_and_packet_variants.md) | P0 | ~-375 行 | 無 | 已完成 |
| 06 | [刪除權威、世界與會話中的死函式](06_authority_and_container_dead_methods.md) | P0 | ~-270 行 | 無 | 已完成 |
| 07 | [State 點擊與挖礦掉落去重](07_state_click_and_mining_rewards_dedup.md) | P1 | ~-250 行 | 01 | 待執行 |
| 08 | [向量字型與選單 5x7 字型整合](08_vector_font_and_menu_font_unification.md) | P1 | ~-300 行 | 01 | 待執行 |
| 09 | [ServerRuntime 投射與會話同步樣板提煉](09_server_runtime_projection_boilerplate.md) | P1 | ~-210 行 | 05, 06 | 待執行 |
| 10 | [container_sessions 與 ServerWorld 職責解耦](10_container_sessions_decoupling.md) | P1 | ~-160 行 | 06 | 待執行 |
| 11 | [物理與實體軸向碰撞去重及 PRNG 整合](11_physics_collision_dedup_and_math_helpers.md) | P1 | ~-180 行 | 無 | 待執行 |
| 12 | [資源包解析與本地化邏輯精簡](12_resource_pack_and_localization_slimming.md) | P1 | ~-500 行 | 無 | 待執行 |
| 13 | [網路伺服器非同步原生化與通道目標整合](13_network_server_async_and_target_channel.md) | P1 | ~-240 行 | 05 | 待執行 |
| 14 | [State 殘留權威模擬與指令處理收斂至 Legacy 模組](14_state_legacy_authority_quarantine.md) | P2 | ~-1,650 行 | 01, 07, 08 | 待執行 |
| 15 | [紅石排程與維度堆配置優化](15_redstone_and_dimension_alloc_slimming.md) | P2 | ~-150 行 | 無 | 待執行 |
| 16 | [測試腳手架與驗收測試收斂](16_test_harness_and_acceptance_slimming.md) | P2 | ~-950 行 | 02, 04 | 待執行 |

**全路線預期削減成果**：`net: -7,857 ~ -8,500 lines possible.`

---

## 5. 明確不在本路線

- **不修改任何網路 wire 格式與協議版本號**（保留 Protocol v19 簽名，僅刪除未分配或完全內部死變體）。
- **不修改任何存檔磁碟格式與欄位順序**（保持 v3 格式與 region 解碼完全相容）。
- **不變更權威模擬邏輯或遊戲玩法數值**（包括 20Hz tick 順序、挖掘時間、掉落機率、紅石更新時序、簽名 Signed-Y `-64..320` 空間）。
- **不拆分外部 workspace crates**（保持目前單一 package + lib/bin 結構）。
- **不重寫或引入新的第三方 GUI / ECS 框架**。
