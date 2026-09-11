# Plan12 — tick 執行緒卸載：worldgen worker、非同步存檔、dirty 玩家／實體

## 定位

20 Hz tick 執行緒目前做兩種與確定性核心無關、卻會撐爆 50 ms 預算的工作：

### 同步 worldgen + flatten

`drain_initial_chunk_projections`（`projection.rs` 1024–1116）對每個 session 缺的欄呼叫 `world.ensure_chunk` → `generate_chunk_with_options`（`dimension.rs` 201–227：climate／density／caves／ores／structures）**同步**，再 `ChunkSaveData::network_terrain_payload`（`format.rs` 821–853、1205–1221）flatten 16×384×16 = 98,304 voxel × 3 陣列 ≈ 288 KiB。上限 `MAX_INITIAL_CHUNK_PROJECTIONS_PER_TICK = 16`（`server_runtime.rs` 65）。走路時最壞數十 ms／tick、~4.6 MiB heap。這就是「tick over budget」的主來源。09 波 07 明確跳過 `ensure_chunk`。

### 同步 eviction／autosave I/O

每 tick `evict_uninteresting_chunks`（`server_runtime.rs` 1112、1434–1488）：dirty 欄 → `chunk_save_payload` flatten 含光照（~480 KiB）+ zlib（`format.rs` 1161–1202；`region.rs` 151–155）+ `save_chunks_in`（`save/mod.rs` 355–432：**整份 region cache `clone()`**、`fs::metadata`、bincode 整個 region、atomic write）。每 6,000 tick `save_all`（1133–1136、1288–1311）再 dump **全部實體**（1231–1255）與**每個玩家**（1491–1526，無 dirty flag），`atomic_write`（`region.rs` 47–104）對每個小 sidecar（`dimension.dat` 1 byte、`mutation_revisions.bin` 全表、每個 `players/*.dat`）都 `sync_all` + Windows `WRITE_THROUGH`。

探索中每 region 5–50+ ms；多 dirty region autosave 50–500 ms 卡頓；失敗還 `eprintln!`（1135、1486）。

## 前置

無。Wave 09 §5「不把 autosave／eviction 移出 tick 執行緒」在本波解除。

## 精確 acceptance

- [ ] **worldgen worker**：`ensure_chunk` 缺欄時只登記需求；生成在 Rayon／專用執行緒完成，結果帶 `(dimension, generation, lifetime)`，過期丟棄（與 client mesh 同模式）；`AuthorityCore::tick` 在 tick 起點以 **session-id／nearest-first 排序** 套用已完成的欄，確定性不變。
- [ ] tick 內不再呼叫 `generate_chunk_with_options`；`MAX_INITIAL_CHUNK_PROJECTIONS_PER_TICK` 改為「每 tick 套用上限」而非「每 tick 生成上限」。
- [ ] **save 執行緒**：tick 只產生 `SavePayload { key, bytes-to-be-compressed or already flattened }` 並放入有界佇列；zlib／region bincode／atomic write 在 save 執行緒；tick 在下一輪接受 ack 後才清 dirty。
- [ ] `load_region_for_write` 不再 `clone()` 整份 cache：`remove` → mutate → 重插；cache hit 不做 `fs::metadata`（改用寫入 generation stamp）。
- [ ] 玩家 per-session dirty bit（pose／inventory／dimension／gameplay 變才存）；實體存檔用 dirty／generation watermark，不再全 dump。
- [ ] sidecar 寫入合併成一個 fsync group；`sync_all` 每批一次。
- [ ] shutdown／failed-restore 語意不變：shutdown `save_all` 阻塞等佇列排空；`failed_restore_chunks` 仍常駐不覆寫。
- [ ] `eprintln!` 改 metrics counter。
- [ ] `ARCHITECTURE.md` Tick vs frame 與 Persistence 段改寫：worldgen／save 在 worker，tick 只排程與 ack。

## 預計檔案與測試

- 改：`src/server_runtime.rs`、`src/server_runtime/projection.rs`、`src/server_world.rs`（`ensure_chunk`）、`src/authority/tick.rs`、`src/save/{mod,region}.rs`、`src/dimension.rs`、`ARCHITECTURE.md`
- 驗證：`cargo test --lib save:: server_runtime::`；`tests/review_hardening_chunk_residency.rs`；`tests/review_hardening_chunk_restore.rs`；`tests/authority_persistence.rs`；`save_all_persists_only_dirty_resident_columns`；crash-child 測試（`ICRAFT_TEST_ATOMIC_CRASH_WORLD`）仍通過；headless soak 比對 `max_tick_time_us` 前後

## 建議階段

1. region cache 零 clone + 跳 metadata（獨立、無執行緒變動）。
2. 玩家／實體 dirty bit。
3. save 執行緒 + ack 協定。
4. worldgen worker + 排序套用。
5. ARCHITECTURE。

## 不在本計劃

- embedded `Arc<Chunk>` 零拷貝投影（Plan 15）。
- flatten SoA／paletted 存檔格式（Plan 16；README §6）。
- 平行 entity physics（Plan 14 最後階段）。
