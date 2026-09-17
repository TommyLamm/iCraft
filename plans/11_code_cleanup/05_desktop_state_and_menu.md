# 05 — 桌面無 producer 狀態與 menu 殘留

狀態：待執行。基線：`83e751d`，2026-09-17。
前置：無；可早於取消 worldgen。

## 定位與判定

- state.rs:1354 pending_chunk_payloads 全倉只有初始化、clear、remove，沒有 insert；:3240 的套用分支沒有 producer。
- chunk_manager/presentation.rs:17 load_generation 只 bump、無 reader。WorldColumns 同名 counter 被 redstone/runtime 使用，必須保留。
- State 的 hud_str_scratch／recipe_book_search／world_spawn／difficulty／bonus_chest 只有宣告／初始化。
- presentation_click.rs:172 同一 matches! 重複 Furnace。
- menu/settings.rs 的 load／from_file_contents 重複 sanitize 流程。

## 實作步驟

1. 刪 pending_chunk_payloads 及清空／remove／不可能的 Loaded payload restore；不刪 Join 仍使用的 pending_block_changes。
2. 刪僅 presentation 的 load_generation 欄位、accessor、bump wrapper；authority counter 不動。
3. 刪無用 State 欄位，連 bootstrap.rs:142/146/187/198/212/216 的無用資料計算／傳遞一起清；launch.difficulty 給 runtime 的用途保留。
4. 核對 State::new 建立空 chunk_meshes 後的首次 invalidation loop；確認中間 set_game_mode 無 column producer 後刪空操作。
5. 刪 menu 的 dead hit-test／options 常數／無 caller constructor。settings 載入與測試解析共用一次 sanitize 結束步驟。
6. 刪重複 Furnace match arm。world_seed／world_type／generate_structures 此時仍被桌面 worldgen 使用，由 11 再刪。

## 驗證與驗收

desktop cargo check、menu／bootstrap 現有測試；不為單純無 producer 欄位新增鏡像測試。確認 authority load_generation 的 redstone／eviction 測試不受影響。刪除的狀態不搬到新的未使用 struct。

## 實作紀錄

尚未執行；完成時記錄實際修改、驗證結果、刪碼量及文件更新。

