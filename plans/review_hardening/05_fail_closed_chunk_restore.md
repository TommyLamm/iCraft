# Plan05 — Chunk restore 失敗即失敗

## 定位

- `ChunkSaveData::restore_to_chunk` 對 blocks／states／light／fluid 做
  `decompress_bytes(...).unwrap_or_default()`；`total_voxels == 0` 就 return。
- `ServerWorld::restore_saved_chunk` 先 `ensure_chunk`（世界生成），再 no-op restore，
  然後記下已存的 `mutation_revision`。下一次 `save_all` 把生成地形寫回磁碟。
- `from_chunk` 壓縮失敗同樣 `unwrap_or_default()`，會持久化永遠無法還原的空 payload。
- 既有測試覆蓋損壞的 **region 容器**（不覆寫），不覆蓋損壞的 **內層 zlib**。

## 前置

無。13 會在本計劃之後加 inflate 上限。

## 精確 acceptance

- [ ] 任一必要 stream（至少 `blocks`）inflate 失敗、為空、或長度對不上該
  `data_version` × 維度高度 × 16 × 16 → restore **錯誤**，不得當成「未修改的生成 chunk」。
- [ ] `restore_saved_chunk` 在 inflate 成功之前不得 `ensure_chunk`。失敗時
  `chunks` map 不出現該 column，或出現但標成不可 save 的 dirty-restore-failed
  （選一種並用測試釘死；推薦：不 insert）。
- [ ] `save_all`／`save_chunk` 不得把 restore 失敗的 column 寫回 region
  （避免生成地形覆蓋玩家建築）。
- [ ] `from_chunk` 壓縮失敗回 `Err`，不得寫空 `blocks`。
- [ ] 測試：
  1. 合法 region envelope + 空或截斷 zlib → `restore` Err，磁碟原檔不被後續 `save_all` 換成生成地形。
  2. 先存一個玩家改過的 chunk，破壞內層 zlib，重載後該格仍不是「全新生成」寫回。
  3. 既有「損壞 region 容器不覆寫」測試仍通過。

## 預計檔案與測試

- 修改：`src/save.rs`（`restore_to_chunk`、`decompress_bytes` 呼叫點、`from_chunk`）、
  `src/server_world.rs`（`restore_saved_chunk`）、必要時 `src/server_runtime.rs` 載入迴圈
  （單一 chunk 失敗應記 log 並跳過，還是中止載入：預設 **該 column 跳過且列入 failed set**，
  不要讓整個世界載入 abort，也不要靜默生成）。
- 測試：`src/save.rs` 的 `#[cfg(test)]` 擴 failpoint；必要時
  `tests/review_hardening_chunk_restore.rs`。

## 建議階段

1. 為每個 `data_version` 算出期望 voxel 數（含 Overworld 384 與 legacy 256）。
2. `decompress_bytes` 改回 `Result`，呼叫端 `?`。
3. 調整 `restore_saved_chunk` 順序：decode → 成功才 ensure／insert。
4. 加內層 zlib 測試。跑既有 save 原子／corrupt-region 測試。

## 不在本計劃

- 無界 inflate 的 byte cap（13）。本計劃先改語義：空／錯 ≠ 生成成功。
- 雙 `SaveManager` 寫 `mutation_revisions.bin`（13）。
- 桌面 latest-wins worker（embedded 路徑已關掉）。

## 實作與證據

### 行為

- `ChunkSaveData::restore_to_chunk` 改回 `io::Result`。必要 stream `blocks` 在 inflate 失敗、為空、或長度不是「目的地 `Dimension::height()` × 16 × 16」也不是 documented legacy 256-high（`UncompressedChunkSnapshot` / pre-signed-Y）時回 `Err`，不再 `unwrap_or_default()` 後當成未修改生成 chunk。
- 可選 stream（`block_states` / `sky_light` / `block_light` / `fluid_levels`）：缺席（空壓縮 payload）仍可選；出現但 inflate 失敗、inflate 為空、或長度對不上 `blocks` → `Err`。
- `ServerWorld::restore_saved_chunk` 先 decode 進 empty column，成功才 insert。失敗：記入 `failed_restore_chunks`、從 `chunks` 移除該格（含 `new()` 預生成的 spawn）、不寫 revision。
- `ensure_chunk` 對 failed set 內座標直接 return，避免之後生成再被 `save_all` 寫回。
- `ServerRuntime::restore_authority_state` 單一 column 失敗只 `eprintln` 並 skip，不 abort 整個世界載入。
- `save_authority_state` / `save_all` 略過 failed set 與 compress 失敗的 column。
- `from_chunk` / `from_chunk_with_redstone` 壓縮失敗回 `Err`，不寫空 `blocks`。測試用 `COMPRESS_FAILPOINT` 注入。

### 測試

- `cargo test --lib save -- --nocapture`：52 passed（含既有 `corrupt_existing_region_is_never_overwritten`、`test_corrupt_region_file_is_not_overwritten_on_save_failure`、`atomic_replace_*`，以及新加的 inner-zlib / from_chunk failpoint / restore-not-insert / save-does-not-overwrite 測試）。
- `cargo test --test review_hardening_chunk_restore -- --nocapture`：5 passed。
- `cargo check --all-targets`：通過。

### Failed set

`ServerWorld.failed_restore_chunks: BTreeSet<(i32, i32)>`。載入失敗 log + insert；`ensure_chunk` / `save_authority_state` / 投影路徑查詢此集合。失敗 column 不進入 `chunks` map。
