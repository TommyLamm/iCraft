# 19 — Chunk 派生索引單次重建與 membership 去重

狀態：已完成。基線：`83e751d`，2026-09-17。
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

- 改動細節：
  1. 合併重建：在 `src/world/chunk.rs` 中刪除原先 5 個獨立掃描函式（`build_torch_index_from_sections`、`build_redstone_index_from_sections`、`build_furnace_index_from_sections`、`build_hopper_index_from_sections`、`build_random_tick_index_from_sections`）以及 5 個獨立 rebuild 方法，收斂為單一 `Chunk::rebuild_derived_indexes(&mut self)`。單次遍歷 section，非 air 空 section 跳過，每次讀取 block 後直接分類填入 `torch_positions`、`redstone_positions`、`furnace_positions`、`hopper_positions`，並依 section 順序維持 `random_tick_sections` 的嚴格遞增排序。
  2. 呼叫端統一：更新 `src/dimension.rs`（flat/nether chunk 生成）與 `src/save/format.rs`（chunk restore），將原本連續呼叫 5 個 rebuild 的流程替換為單一 `rebuild_derived_indexes()`。
  3. Membership 分類收斂與 bug 修復：提取 `update_index_membership` 輔助函式，將 `set_block_local` 的四組位置索引更新收斂為 `old_member && !new_member`（刪除）、`new_member && !old_member`（新增）、`old_member && new_member`（同類保留）的分類變化邏輯。修復了原先 redstone component 互相置換（如 `RedstoneWire` → `Repeater`）時因 `if old_is_redstone { remove } if new_is_redstone && !old_is_redstone { push }` 導致索引遺失的 bug；同時確保同位置重複設定不產生重複項。
  4. 公開輔助方法：將 `Chunk::encode_torch_position` 標註為 `pub fn`，與 `decode_torch_position` 對稱，便於測試與參考掃描對齊。
  5. 架構文件同步：更新 `ARCHITECTURE.md`，詳述 chunk 派生索引的單次掃描重建與 `set_block_local` membership 分類變更契約。
- 實際命令與結果：
  - `cargo test torch_index_tracks_local_mutations_without_duplicates`: ok (1 passed)
  - `cargo test furnace_index_tracks_local_mutations_without_duplicates`: ok (1 passed)
  - `cargo test hopper_index_tracks_local_mutations_without_duplicates`: ok (1 passed)
  - `cargo test random_tick_index_tracks_section_eligibility`: ok (1 passed)
  - `cargo test save::tests::block_states_roundtrip_and_restore`: ok (1 passed)
  - `cargo test --lib world`: ok (170 passed, 0 failed)
  - `cargo test --lib save`: ok (59 passed, 0 failed)
  - `cargo check --all-targets`: ok (exit 0)
  - 新增回歸與邊界測試（均通過）：
    - `redstone_index_tracks_component_mutations_without_loss_or_duplicates`: 驗證 component→component 互換（Wire ↔ Repeater ↔ Comparator ↔ Lever ↔ Door ↔ TNT）索引不遺失、不重複，變回 non-component 正確清除。
    - `random_tick_index_handles_negative_sections_and_last_block_removal`: 驗證負 section Y（-4, -2, 0）排序、多 block 下移除部分保留資格、移除最後一個 block 退出索引，以及 rebuild 後一致性。
    - `bulk_restore_and_rebuild_derived_indexes_match_exhaustive_scan`: 驗證跨正負 section 及空 section 的 chunk，經連續放置與同位重複置換後，live chunk、`rebuild_derived_indexes` 及 `ChunkSaveData::restore_to_chunk` 的所有 5 個派生索引完全等於參考逐格掃描（exhaustive scan），順序與成員完全一致。
- 淨刪碼／保留原因：
  - 刪除 5 份獨立且重複的 section 掃描實作與 5 個零碎 rebuild 方法（刪除 134 行重複掃描邏輯及零散呼叫）。
  - 保留 `Vec<u32>` compact encoding 與 `Vec<i8>` 嚴格升序表示，未改為 `HashSet`，防止輸出遍歷次序漂移。
  - 增加 3 組嚴密測試覆蓋原先缺少測試保護的 component 轉換漏洞與負 section/bulk restore 行為。

