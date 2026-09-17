# 19 — Chunk 派生索引單次重建與 membership 去重

狀態：待執行。基線：`83e751d`，2026-09-17。
前置：03 建議先完成；不依賴 desktop 改造。

## 定位與判定

world/chunk.rs:170/198/226/254 的 build_torch/redstone/furnace/hopper_index_from_sections 各做一份 section→ly→z→x 掃描，:282 再處理 random tick sections。dimension.rs:668–672、save/format.rs:1057–1061 正式流程連續呼叫五次 rebuild。

set_block_local 的 redstone membership 分支（:509–518）在 old/new 都是 component 時，先移除舊索引，卻因 new && !old 不再插回。這是讀碼發現，需先用回歸案例驗證；不能把修復混報為純刪碼。

## 實作步驟

1. 合併成一次 rebuild_derived_indexes，每次讀 block 後填 torch／redstone／furnace／hopper 向量，並建立 random-tick section 清單。
2. 保留空 section 跳過、position encoding、各向量既有遍歷順序及 section 排序；不要把 Vec 改 HashSet 造成輸出次序漂移。
3. 所有 generation／restore 呼叫改用唯一 rebuild；刪四份重複掃描。
4. 將 set_block_local membership 更新收斂為 old/new 分類變化：old && !new 刪、new && !old 加、同類保留。避免同位置重複。
5. 新增 component→另一 component 回歸案例，再修正已確認的索引遺失；同類 Torch 等案例一併核對。
6. 更新 ARCHITECTURE 索引建構描述。此包不改權威 tick 對索引的消費順序。

## 驗證與驗收

既有：torch_index_tracks_local_mutations_without_duplicates、furnace_index_tracks_local_mutations_without_duplicates、hopper_index_tracks_local_mutations_without_duplicates、random_tick_index_tracks_section_eligibility、save::tests::block_states_roundtrip_and_restore。

新增：bulk restore 各索引等於參考逐格 scan；component→component 不丟索引；負 section Y、移除最後一個 random-tick block、重複設定不產生重複位置。

重建只有一份掃描骨架；相同 chunk 的索引順序與成員有實際證據。若報效能，用固定 column 的重建量測，不能把五個函式合一直接聲稱快五倍。

## 實作紀錄

尚未執行；完成時記錄實際修改、驗證結果、刪碼量及文件更新。

