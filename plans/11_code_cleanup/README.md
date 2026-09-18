# iCraft 第六波精簡計劃：22 個獨立執行包

基線：`83e751d`。日期：2026-09-17。狀態：**深入探索與計劃拆分完成；功能實作尚未開始。**

## 使用方式

這一版將原本 10 個大工作包細分成 22 份文件。每份包含定位／引用證據、明確修改步驟、前置、既有測試、新增測試缺口、驗收及實作紀錄。直接打開下表的單份文件執行；[prompt.md](prompt.md) 可作執行提示。

沿用本次要求的積極刪碼取向：允許刪檔、收窄 API、改 feature 和直接重構正式資料流，不為「以後可能會用」留下兼容殼。驗證用於判斷是否完成；test-only 原型可以刪，真正產品行為的測試遷移到正式入口。

這次只改 `plans/11_code_cleanup/`。沒有修改遊戲 source、存檔、資產、設定或 ARCHITECTURE 的現行契約。

## 深入探索結果

三路子代理分別核對桌面／mesh、runtime／network、worldgen／gameplay；主代理核對資源解碼、音效 bytes 與 Cargo 依賴，並抽查關鍵控制流。均先讀架構並使用現有 CodeGraph，再以 source 和全倉引用核對，未建新索引。

主要新增機會：

- inbound metrics 為算長度重編碼已解碼 Packet，可直接利用實際 frame bytes。
- texture 先解碼驗證再解碼使用；sound 每次播放 clone 整份 Vec。
- worldgen completed backlog 與 in-flight 的生命週期不一致，可能重複 schedule，需確定性測試重現並收斂狀態。
- chunk 五個派生索引有重複扫描，可合併；redstone→redstone membership 分支需新增回歸驗證。
- Embedded／Join 欄位安裝邏輯重複，decode 失敗前推 revision、預設 Overworld mesh 範圍及 halo 更新需在共用 commit 邊界處理。
- menu 修改視距未同步到 runtime，是取消桌面 worldgen 的真實前置。
- 未接線村民交易生成與升級算法可獨立刪除；surface API 有未用參數及每 voxel 重複表項取樣。

詳細原計劃修正及未納入項目見 [audit.md](audit.md)。上述控制流推論／新回歸案例沒有冒充已執行測試結果。

## 執行包索引

| # | 獨立計劃 | 類型 | 前置 | 狀態 |
| --- | --- | --- | --- | --- |
| 01 | [刪除完整原型與測試專用玩法殼](01_delete_gameplay_shells.md) | P0／死碼 | 無 | 已完成 |
| 02 | [Session 單一定義與 authority API 清理](02_session_and_authority_api.md) | P0／去重 | 無 | 已完成 |
| 03 | [worldgen 舊密度／洞穴算法與無用狀態](03_worldgen_dead_density.md) | P0／死碼 | 無 | 已完成 |
| 04 | [字體、手部及小型渲染死路徑](04_render_dead_paths.md) | P0／死碼 | 無 | 已完成 |
| 05 | [桌面無 producer 狀態與 menu 殘留](05_desktop_state_and_menu.md) | P0／死狀態 | 無 | 已完成 |
| 06 | [network 死封套與傳送支線](06_network_dead_envelopes.md) | P0／死碼 | 建議02 | 已完成 |
| 07 | [Save worker 無 producer 封套與多餘 job id](07_save_worker_envelopes.md) | P0／死封套 | 02 | 已完成 |
| 08 | [Runtime worldgen 固定 token 與空 metrics](08_worldgen_fixed_tokens.md) | P0／固定狀態 | 無 | 已完成 |
| 09 | [收包按實際 frame bytes 計量](09_inbound_frame_metrics.md) | P1／活躍流程 | 建議06 | 已完成 |
| 10 | [Embedded 本地視距同步到 runtime](10_embedded_view_distance.md) | P1／契約前置 | 02 | 已完成 |
| 11 | [刪除桌面本地 worldgen 與載入排程](11_remove_desktop_worldgen.md) | P1／活躍流程 | 05、10 | 已完成 |
| 12 | [Section mesh invalidation 單一來源](12_section_invalidation.md) | P1／去重與行為修正 | 05、21 | 已完成 |
| 13 | [刪未接線村民交易生成與升級算法](13_unused_trade_generation.md) | P0／死算法 | 建議01 | 已完成 |
| 14 | [Surface API 簡化與每欄取樣一次](14_surface_api_and_sampling.md) | P1／活躍流程 | 建議03 | 待執行 |
| 15 | [資源解碼只做一次，移出 shared 的桌面 codec](15_resource_decode_boundary.md) | P1／解碼去重 | 無 | 待執行 |
| 16 | [音效快取共用 bytes，移除播放時整份複製](16_audio_shared_bytes.md) | P1／資料共用 | 15 | 待執行 |
| 17 | [desktop feature 與 dedicated 建置邊界](17_desktop_cargo_feature.md) | P1／建置邊界 | 15 | 待執行 |
| 18 | [清理只作版本限制的直接依賴](18_dependency_pins.md) | P2／依賴精簡 | 建議17 | 待執行 |
| 19 | [Chunk 派生索引單次重建與 membership 去重](19_chunk_derived_indexes.md) | P1／算法去重 | 建議03 | 待執行 |
| 20 | [Worldgen demand／generating／completed 排程收斂](20_worldgen_job_lifecycle.md) | P1／排程收斂 | 08；建議02 | 待執行 |
| 21 | [整欄投影共用 commit 邊界](21_column_projection_commit.md) | P1／提交去重 | 11 | 已完成 |
| 22 | [公開面、測試與文件總驗收](22_final_surface_tests_docs.md) | 收尾 | 全部選定工作包 | 待執行 |

「建議」表示檔案重疊或能减少反覆修改，不是硬依賴；未標「建議」的是該包方案需要的實際前置。

## 建議執行順序

1. 先做直接精簡：01、02、03、04、05、06、07、08、13。
2. 地形／呈現主線：10 → 11 → 21 → 12。
3. 世界生成主線：03 → 14；03 → 19；08 → 20。
4. 資源／依賴主線：15 → 16，15 → 17 → 18。
5. 收包計量：06 → 09。
6. 最後 22 統一驗收。實作時按已完成狀態選包，不必照文件號碼執行。

可獨立研究不同主線，但同檔修改要串行整合：

- 01／13 都改 village trade。
- 02／07／10／20 都可能改 runtime root/session。
- 04／05／10／11／21／12 都可能改 State/presentation。
- 03／14／19 都可能改 world/chunk 或生成呼叫點。
- 15／16／17／18 共用 resources/audio/Cargo 邊界。

## 原 10 包對應

| 原工作包 | 拆分後 |
| --- | --- |
| 01 舊玩法／navigation | 01、13 |
| 02 session／preflight | 02 |
| 03 worldgen | 03、14；深入探索另加19 |
| 04 桌面／渲染 | 04、05；資源深入探索另加15、16 |
| 05 network | 06、09 |
| 06 worker／authority | 07、08、20；小型死 API 尾項歸22 |
| 07 桌面 worldgen | 10、11、21 |
| 08 mesh invalidation | 12 |
| 09 Cargo | 17、18；15為codec邊界前置 |
| 10 總驗收 | 22 |

## 掃描基線與驗收規則

上一輪已執行 `cargo check --all-targets --all-features --message-format short`：exit 0，24.31 秒，仍有 unused/dead_code/private_interfaces/unreachable_patterns 警告。本輪新增的是唯讀呼叫鏈探索、Cargo tree 檢查和文件拆分，沒有重新執行產品測試、效能 benchmark 或實作任何修復。

五個可整檔刪除原型合計 1,174 行；session 重複檔148行，原盤點合計1,322行。這只是已核對檔案大小，不是最終淨刪碼承諾；新增機會也不預報未量測的 FPS／tick／編譯速度百分比。

完成每包時記錄實際 diff、命令結果、尚存缺口並更新此索引。涉及 architecture/data contract 的修改同包更新 ARCHITECTURE。單純死碼不新增鏡像測試；正式流程改造只補本包明列的行為缺口。完整測試只在22集中跑一次，後續有新增變更或未解失敗再重跑。
