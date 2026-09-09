# Plan08 — Region cache、同 region 批次、dirty-only autosave

## 定位

投影每 tick 最多 16 欄仍走 `ChunkSaveData::from_chunk`（5 條 dense 陣列 + 5 次 zlib level 6）。`save_chunk_in` 每個 chunk：不讀 `region_cache`、`fs::read` 整個 region、插入一欄、serialize、atomic_write。Autosave 對**所有 loaded 欄**做上述流程。同一 region 1024 欄會被完整重寫 1024 次，全在 50 ms tick 上。

## 前置

無。不要與 07 混在同一 PR（協定 vs 存檔）。

## 精確 acceptance

- [ ] `load_region_for_write` 使用既有 `region_cache`。
- [ ] 同一 region 的 dirty 欄批次寫一次。
- [ ] autosave 只寫 `dirty_chunks`（或等價髒集合），不是全部 resident 欄。
- [ ] 投影不要為了送地形再走完整 `ChunkSaveData` 存檔格式（可用未壓縮 palette／既有 section snapshot）。
- [ ] zlib 可改 `fast`（level 1）若測試與磁碟相容允許；須在證據寫明。
- [ ] 現有 persist／corrupt restore 測試通過；region `.bin.bak` 語意不變。

## 預計檔案與測試

- `src/save/mod.rs`、`region.rs`、`format.rs`、`src/server_runtime.rs` `save_authority_state`、`projection.rs`
- 驗證：`cargo test --lib save::`；`tests/` persist／restore／residency

## 建議階段

1. cache hit 路徑。
2. dirty-only autosave。
3. 同 region 批次。
4. 投影脫離存檔格式。

## 不在本計劃

- 改 chunk v3 磁碟欄位順序。
- Hopper cooldown dirty（10）——本計劃只消費 dirty 集合，不改誰 mark_dirty。
