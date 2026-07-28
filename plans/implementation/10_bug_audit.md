# 實作計畫 10：潛在 Bug 審計與修復

> 狀態：🟢 已全部完成（所有審計項目及後續項 G3、N3、G1、W1 均已修復並通過測試）。
> 原始審計依使用者要求，只在 1–9 全部完成後開始。

## 目標

對目前 `master` 做分區靜態／測試審計，列出可證明的潛在 bug，修復所有本輪
確認且範圍可控的問題，為每項修復加入能重現原錯誤的回歸測試。功能需求、
純風格問題、已知架構限制與無法證明的猜測不列為已修 bug。

## 審計方法

1. [x] 以 CodeGraph 先追每個分區的資料流與 blast radius。
2. [x] 多個 sub-agent 並行審計互不重疊的 render/world、network、
   input/UI/inventory、entity/combat、save/settings、dimension/weather 等範圍。
3. [x] 每個候選問題必須附：嚴重度、精確 source path、觸發條件、實際結果、
   正確結果、根因、最小修復面與回歸測試。
4. [x] 根代理去重並以 source 與可執行測試獨立驗證；無證據或其實是設計限制
   的項目記錄為不採納，不直接修改。
5. [x] 將確認問題分派回 sub-agent 實作；檔案 ownership 不重疊，sub-agent
   不 commit、不改文件。
6. [x] 根代理用 CodeGraph 審查每條修復與影響面，必要時退回同一 sub-agent
   修正。
7. [x] 更新本文件、`track.md`、`plans/progress.md`、`ARCHITECTURE.md`，
   以單一 Task 10 commit 收束。

## 審計結果

| ID | 問題 | 狀態 |
|---|---|---|
| R1 | LOD 用 AABB 中心距離，近距離高柱體可能被降級 | 已修 |
| R2 | 遠平面未覆蓋 256 高度與視距角落 | 已修 |
| R3 | 透明地形背面剔除導致水下表面不可見 | 已修 |
| C1 | 卸載後異步存檔與立即重載可能恢復舊 chunk | 已修 |
| C2 | 遠端 ChunkData 只 invalidates 正交鄰居，漏對角 halo/AO | 已修 |
| C3 | 流體變更未完整更新 sky/block light | 已修 |
| N1 | 多 player pose 共用 mailbox，玩家 ID 會互相覆蓋 | 已修 |
| N2 | join/leave 可靠佇列滿時 roster 可能遺失 | 已修 |
| N3 | host 對遠端方塊請求缺 reach 驗證 | 已修（Task 11） |
| I1 | `add_stack` 部分寫入後回傳失敗，呼叫端可能丟 remainder | 已修 |
| I2 | 相同 Item 但 metadata 不同仍可能 merge | 已修 |
| I3 | 真實拖曳游標在關閉時可能遺失 | 已修 |
| U1 | 開 UI 後 held mining 仍在背景繼續 | 已修 |
| U2 | Escape/L/E repeat 可能重複切換 UI | 已修 |
| U3 | Controls Done 後隱藏 rebind 狀態未清 | 已修 |
| E1 | 玩家箭擊殺普通怪物不給標準 loot/XP | 已修 |
| E2 | splash 效果可套到非活體並刪除 DroppedItem | 已修 |
| E3 | 玩家擊退來源使用 `entities[0]` 而非實際攻擊者 | 已修 |
| P1 | Creative 高速 sprint 在大 dt 下可穿牆 | 已修 |
| P2 | 重生未重置氧氣/回血/飢餓/溺水計時器 | 已修 |
| P3 | Creative sprint 仍受飢餓限制並消耗 exhaustion | 已修 |
| G1 | joined client 生存放置/破壞缺權威消耗/drop ACK | 已修（Task 12） |
| G2 | 破壞 raycast 可選中 Water/Lava 等環境 passable | 已修 |
| G3 | 紅石 facing/delay/comparator/note 卸載後丟失 | 已修 |
| W1 | 門/活板門仍是通用完整 cube，缺方向/薄碰撞 state | 已修（Task 13） |
| W2 | RedstoneTorch/Off 使用普通完整 cube | 已修 |
| W3 | Sugar cane/cactus 支撐規則缺水/側邊約束 | 已修 |
| A1 | 多人 weather 只同步 enum，client 自行演化與雷擊分歧 | 已修 |
| A2 | NaN/Inf FOV/sensitivity 可通過 clamp | 已修 |
| A3 | 可讀但無效 WAV 會壓掉程序化 fallback | 已修 |

## 後續項

本輪最後依使用者要求停止再派 sub-agent 並快速驗證收尾，因此不再擴大改動面。
`N3/G1` 需要 protocol/State 的權威方塊 action-result 流程；`W1` 需要正式
block-state/orientation 與碰撞資料，不應只用視覺假修。`G3` 已於 2026-07-25
完成：redstone component metadata（facing/repeater_delay/comparator_mode/note）
現在以 `ChunkSaveData.redstone_metadata` sidecar（Zlib + bincode）持久化，跨
chunk 卸載／重載、維度切換、背景存檔與同步存檔皆保留；舊存檔透過
`deserialize_chunk_save_data` 的 legacy fallback 讀回並回報空 sidecar。

三項後續項已各自立為獨立實作計畫並全部完成：

- **N3** → [計畫 11：Host 遠端方塊請求 Reach 驗證](./11_reach_validation.md) (已完成)
- **G1** → [計畫 12：Joined Client 權威方塊 Action-Result 流程](./track_12_block_action_result.md) (已完成)
- **W1** → [計畫 13：門／活板門方向與薄碰撞 Block-State](./13_door_trapdoor_state.md) (已完成)

## 驗證門檻

- 每項已修問題都有失敗前／成功後可表達的純回歸測試。
- `cargo fmt`
- `cargo test`
- `cargo check --release`
- `git diff --check`
- 任何需要 GPU、多視窗、音訊裝置或 OS cursor 的檢查明確保留為人工驗收。

## Commit

單一審計修復 commit：`fix: resolve audited gameplay regressions`
