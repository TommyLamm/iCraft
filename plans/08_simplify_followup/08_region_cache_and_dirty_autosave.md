# Plan08 — Region cache、同 region 批次、dirty-only autosave

## 定位

投影每 tick 最多 16 欄仍走 `ChunkSaveData::from_chunk`（5 條 dense 陣列 + 5 次 zlib level 6）。`save_chunk_in` 每個 chunk：不讀 `region_cache`、`fs::read` 整個 region、插入一欄、serialize、atomic_write。Autosave 對**所有 loaded 欄**做上述流程。同一 region 1024 欄會被完整重寫 1024 次，全在 50 ms tick 上。

## 前置

無。不要與 07 混在同一 PR（協定 vs 存檔）。

## 精確 acceptance

- [x] `load_region_for_write` 使用既有 `region_cache`。
- [x] 同一 region 的 dirty 欄批次寫一次。
- [x] autosave 只寫 `dirty_chunks`（或等價髒集合），不是全部 resident 欄。
- [x] 投影不要為了送地形再走完整 `ChunkSaveData` 存檔格式（可用未壓縮 palette／既有 section snapshot）。
- [x] zlib 可改 `fast`（level 1）若測試與磁碟相容允許；須在證據寫明。
- [x] 現有 persist／corrupt restore 測試通過；region `.bin.bak` 語意不變。

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

## 實作與證據

起點：`plan/08-05-remove-debug-logs` @ `497ee6059d95cafddf54ac739d01c29a4d74e04a`。

- **region cache on write**：`load_region_for_write` 在 cache 命中且 on-disk `metadata.len()` 仍等於上次讀寫長度時直接 clone cache，不再 `fs::read` + deserialize。長度不符或檔案遭換成垃圾時仍走 fail-closed `RegionCorruption`，所以 `corrupt_existing_region_is_never_overwritten` 繼續成立。證據：`save::tests::load_region_for_write_reuses_cache_when_the_file_is_gone`（刪檔後寫 sibling 仍保留第一欄，證明寫入走 cache 而不是空 region）。
- **同 region 批次**：新增 `SaveManager::save_chunks_in`；每個 `(rx, rz)` 只 `atomic_write` 一次。`save_chunk_in` 是單欄包裝。autosave 與 eviction flush 都走這條。證據：`save::tests::save_chunks_in_writes_same_region_siblings_together`；既有 `same_region_batch_*` 與 `.bin.bak`（首次 create 不建 bak、首次 replacement 建一次）仍過。
- **dirty-only autosave**：`save_authority_state` 只序列化 `dirty_chunks.dirty_revisions()`，不再掃全部 resident 欄。未 mutate 的 generated 欄不落盤（seed 可重建）。證據：`server_runtime::tests::save_all_persists_only_dirty_resident_columns`。
- **投影脫離存檔格式**：`ChunkSaveData::network_terrain_payload` 只 flatten blocks / block_states / fluid_levels + 未壓縮 block-entity bincode，不做 sky/block light、redstone sidecar、zlib。`restore_network_payload` 以 voxel 長度辨識未壓縮，否則仍 inflate 舊 zlib。證據：`network_terrain_payload_is_uncompressed_and_restores`、`restore_network_payload_still_accepts_disk_zlib_layout`。
- **zlib level 1**：`compress_bytes` 改 `Compression::fast()`（flate2 level 1）。仍是 zlib wrapper，舊 level-6 磁碟可 inflate；新寫入與測試 roundtrip 相容。chunk v3 欄位順序未改。
- **測試**（worktree `C:\Users\Tommy\Desktop\iCraft-wt-08-08`）：
  - `cargo test --lib save::` → 48 passed, 2 ignored
  - `cargo test --lib save_all_persists_only_dirty_resident_columns` → ok
  - `cargo test --test authority_persistence` → 4 passed
  - `cargo test --test review_hardening_chunk_restore` → 5 passed
  - `cargo test --test review_hardening_chunk_residency` → 4 passed（含 dirty origin flush-before-evict）

