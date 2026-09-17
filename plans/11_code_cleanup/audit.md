# 深入探索紀錄與原計劃修正

日期：2026-09-17。基線：`83e751d`。本文件記錄 source 核對結果，不是實作完成報告。

## 探索分工與方法

三路子代理唯讀探索：presentation/mesh、authority/runtime/network、worldgen/gameplay。主代理探索資源包、audio、Cargo，並抽查索引 membership、runtime worker、projection commit 和 frame 計量原始碼。

使用既有 CodeGraph 後再以 source／全倉 rg 查 caller、producer 和測試；未重建索引。Cargo tree 用於確認直接及傳遞依賴。子代理沒有改代碼或執行編譯。

## 修正原總計劃

| 原敘述或可能誤讀 | 深入核對後的修正 | 文件 |
| --- | --- | --- |
| session_revision 無 caller 可直接刪 | runtime 產品測試使用；搬測試 fixture 並保留 assertions | 02 |
| 手部 helper test-only，可連測試刪 | 多個測試驗正式工具幾何／UV／動畫；遷到 base mesh/uniform API | 04 |
| NETHER_COLUMN_HEIGHT 無 caller | Nether roof 有效測試使用；改 height API 再刪常數 | 03 |
| 現有跨執行緒測試覆蓋 chunk byte identical | 只比 y=64 的16×16 block查詢；改真正生成才有該覆蓋 | 03 |
| 已有完整固定 seed states fingerprint | 現有 golden hash只含 blocks/heightmap；states 是新增覆蓋 | 03、14 |
| PlayerFile ack 負責清 player dirty | 真正 player save 目前同步，worker variant無producer；同步修文件／註解 | 02、07 |
| runtime generation/lifetime 是現有防過期機制 | token固定1、bump無caller；刪假狀態，需求生命周期另做 | 08、20 |
| 已有 worldgen stale-result／worker barrier race 測試 | 本次未找到這些直接測試；明列新增缺口 | 07、08、20 |
| 取消桌面生成只需刪 scheduler | pause options 視距需先同步，且unload/reprioritize錨點仍活躍 | 10、11 |
| 所有 pending payload／changes 都可刪 | pending_chunk_payloads 無insert；pending_block_changes 有Join producer | 05、11 |
| 刪 network helper 可順帶刪 Outbound variant | ingress仍有正式producer/consumer | 06 |
| 圖形dependencies改optional即可headless | shared resources仍引用image/rodio，先移codec到消費者 | 15、17 |
| 無直接import即可刪version pins | pins可能限制傳遞解析；逐項cargo tree/lock比較 | 18 |
| GPU欄位無read就是死狀態 | 需看資源持有職責；WorldColumns.load_generation亦仍活躍 | 04、05 |

## 新增活躍流程機會與證據強度

| 機會 | 證據 | 狀態 |
| --- | --- | --- |
| 收包重encode | session.rs record_inbound→packet_bytes→encode；ingress兩個正式caller | source已確認；09待實作 |
| texture重decode | resources.rs:368→texture.rs:496–502 | source已確認；15待實作 |
| 每次音效複製bytes | audio.rs sound_cache Vec、get_source clone | source已確認；16待實作 |
| completed worldgen重schedule | poll移除in_flight早於apply移除demand；32生成/16套用上限 | 控制流推論，20先建確定性重現 |
| redstone索引遺失 | chunk.rs old component移除、new && !old才新增 | source分支已確認，19新增回歸後修正 |
| revision先於decode更新 | state.rs:4707提前insert revision，replacement忽略Result | source已確認，21新增失敗回歸 |
| state-only光源更新 | apply_synced_block_change用靜態emission，未比較state-aware emission | source線索，12新增回歸驗證 |
| 全欄mesh更新過寬 | cell記section後caller又invalidate全欄 | source已確認；預期排程數不是實测job數 |

## 已觀察但未混入刪碼主線

SaveWorker::wait_barrier 在 timeout／disconnect 時仍回傳普通 ack Vec，caller可能無法分辨是否真正等到指定Barrier。本次只記錄為持久化正確性線索，沒有跑重現；若另行處理，應獨立修改回傳契約與故障測試，不偽裝成刪PlayerFile封套的收益。

取消歷史save migration、移除serde資料欄位或縮減資源格式都屬相容性／產品行為變更，不能僅因名字legacy或當前UI未讀而當死碼。本波已選擇大幅活躍重構，沒有把未核對用途的欄位直接加入刪除清單。
