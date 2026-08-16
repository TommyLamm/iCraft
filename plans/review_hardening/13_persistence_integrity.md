# Plan13 — 持久化雙寫、inflate 與 symlink

## 定位

01–05／08 處理契約與生命週期。本計劃收剩餘持久化完整性：

1. Listen-host 對同一 `world_dir` 有兩個 `SaveManager`：`ServerRuntime` autosave 與
   `State` 的 `NetworkSnapshotWorker::PersistIndex` 都 `atomic_write`
   `mutation_revisions.bin`，無共用鎖。Presentation 的 region cache 也獨立，
   catch-up `load_chunk_in` 可能讀到舊 region。
2. `decompress_bytes` 是無界 `read_to_end`。Pack 驗證有 8 MiB／entry；存檔沒有。
3. `discover_worlds`／`launch_existing` 不走 `validated_world_path`。`is_dir()` follow
   junction 後 canonicalize，可把 `regions/` 寫到 `saves/` 外。Delete／copy 有驗證；
   專用伺服器拒 symlink root，桌面 play 不拒。
4. `settings.txt`／`controls.config` 用 `fs::write`，崩潰會截斷。
5. `.bin.bak` 只在第一次非原子 `fs::copy`，之後不更新，也不是 last-known-good。
6. MOTD 在 CLI 有 256-byte cap，`ServerProperties::load` 沒有。

## 前置

- 04：玩家檔路徑必須已是單射正規化 key，本計劃不要再發明第二套 sanitize。
- 05：restore 語義已是失敗即失敗；本計劃只加 inflate **上限**，不改「空 = 生成」
  （那應已被 05 刪除）。

## 精確 acceptance

- [x] 只有 `ServerRuntime` 的 `SaveManager` 寫 `mutation_revisions.bin`。
  `try_persist_index` 在 `has_in_process_runtime()` 時 no-op 或改讀 runtime。
  Presentation 在 runtime 權威期間不得快取獨立 region 當 catch-up 來源。
- [x] 所有 save zlib inflate 使用 `decoder.take(expected + 1)`，expected 來自
  `data_version` × section 數。超過 → Err（走 05 的 fail-closed）。
- [x] 每個 launch／upgrade／discover 路徑走 `validated_world_path`。symlink／junction
  世界根不得出現在選單，也不得被 play。
- [x] `settings.txt` 與 `controls.config` 改 `atomic_write`。
- [x] `ServerProperties::validate` 對 motd 施加與 CLI 相同的 256-byte 上限。
- [x] 測試：runtime 在場時 persist-index 不改檔；超大 zlib 回 Err 且不 OOM 測試行程
  （用 take 上限，不要真的解 2GB）；選單拒絕 symlink 世界（Windows junction 若測試
  環境允許就測，否則測「canonicalize 後逃出 saves/」）。

## 預計檔案與測試

- 修改：`src/state.rs`（`try_persist_index` 閘）、`src/save.rs`（inflate take、
  可選 bak 策略）、`src/menu.rs`（discover／launch）、`src/server_runtime.rs`
  （motd validate）。
- 測試：`src/save.rs`、`src/menu.rs` 單元測。

## 建議階段

1. 關掉雙寫 index。
2. Inflate take。
3. 選單路徑驗證。
4. Settings atomic + motd cap。
5. Bak 若動：成功 `atomic_write` 後才輪替一份 last-good；否則維持現況並在計劃
   「實作與證據」寫明不改 bak。

## 不在本計劃

- `save_player_and_level` 合成單一 envelope（可選，不阻塞）。
- 05 的 restore 順序（已完成才進本計劃）。

## 實作與證據

- Persist 雙寫：`State::process_join_catchups` 在 `has_in_process_runtime()` 時不再
  `try_persist_index`；`SaveManager::save_mutation_revision_index_unless_runtime`
  是同步 no-op。Listen-host catch-up 設 `allow_disk_fallback: false`，worker 不得
  用 presentation 獨立 region cache 當來源。
- Inflate：所有存檔 zlib 走 `decompress_bytes_limited` + `decoder.take(expected + 1)`。
  voxel stream 的 expected 由 `data_version` × section 數（v0 = 256-high）算出；
  sidecar 另有 8 MiB 上限。超長 → `Err`，沿用 05 fail-closed，未恢復「空 = 生成」。
- 選單：`discover_worlds` / `launch_existing` / `create_world` 一律
  `validated_world_path`。該 guard 用 `symlink_metadata` 拒絕 symlink 與 Windows
  junction（`FILE_ATTRIBUTE_REPARSE_POINT`），再 `canonicalize` 確認仍在 `saves/` 內。
- `GameSettings::save` 對 `settings.txt` / `controls.config` 改 `atomic_write`。
- `ServerProperties::validate` 對 motd 施加與 CLI 相同的 `1..=256` bytes（含 trim 空）。
- Bak：未改。`.bin.bak` 仍只在第一次非原子 `fs::copy`、之後不輪替 last-good。
- 測試：`persist_index_is_noop_when_in_process_runtime_owns_world`、
  `snapshot_worker_skips_region_cache_when_disk_fallback_disabled`、
  `oversized_zlib_inflate_is_rejected_without_unbounded_output`、
  `world_path_guard_rejects_escape_and_symlink_roots`、
  `motd_validate_applies_cli_256_byte_cap`。
- 玩家檔仍沿用 04 的 `normalize_player_identity` 單射 key，未另做 sanitize。
