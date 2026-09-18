# 16 — 音效快取共用 bytes，移除播放時整份複製

狀態：已完成。基線：`83e751d`，2026-09-17。
前置：15；音效舊 helper 與相關測試在本包一起遷移。

## 定位與證據

- `src/audio.rs:92`：`sound_cache: HashMap<SoundId, Vec<u8>>`。
- `src/audio.rs:495–500`：從 ResourcePackManager 的 Arc bytes 轉成 Vec。
- `src/audio.rs:561–563`：每次 `get_source` clone 整個 Vec，再交給 Decoder。
- `src/resources.rs:971`：validator 也先 clone bytes；由 15 移到消費者邊界後一起消除。
- 這是活躍音效路徑，不是死碼。

## 實作步驟

1. sound cache 改存 `Arc<[u8]>`；資源包載入沿用 Arc，程序化 WAV 只在合成完成時轉一次。
2. `get_source` 為每個播放建立自己的 `Cursor<Arc<[u8]>>` 和 decoder，僅複製 Arc handle；實作時以本倉 rodio 0.19 trait bounds 驗證型別（Cursor<Arc<[u8]>> 滿足 Read + Seek）。
3. one-shot、3D sound、looping sound 共用這個 decoder 建構入口，保留各自播放 cursor。
4. 移除 `load_or_synthesize_sound` 的舊磁碟 helper 與僅為其存在的 wrapper；把其「有效 bytes 不覆寫／壞 WAV 回退」測試遷至資源包正式載入路徑。
5. 不引入 PCM 常駐 cache；不改目前支援格式。

## 驗證

既有：`audio::tests::invalid_wav_uses_decodable_procedural_fallback`、`valid_wav_is_loaded_without_replacing_its_bytes`（遷移入口）、`test_wav_synthesis`、`chest_feedback_synthesis_is_deterministic_and_distinct`、`weather_category_gain_only_affects_rain_and_thunder`。

新增不需音效裝置的 decoder 測試：同一快取建立兩個 decoder，各自 seek/read 或播放取樣互不干擾；驗證 clone 來源使用同一 Arc 配置。實機檢查 one-shot／雨聲循環與音量切換。

## 驗收

`get_source` 不 clone 音訊 Vec；每次播放只建獨立 cursor／decoder。保留字幕與音量語意，無法使用音效裝置時仍可安靜降級。

## 實作紀錄

- 改動細節：
  - `src/resources.rs`：
    - `resolve_decoded` 的 decoder closure 參數型別由 `&[u8]` 改為 `&Arc<[u8]>`，並移除候選包搜尋時的多餘 clone，允許消費端借用 slice 或零拷貝持有 `Arc` handle。
    - 更新 `resolve_font_source` 與內部單元測試匹配 `&Arc<[u8]>`。
  - `src/texture.rs`：
    - 適配 `apply_resource_pack_tiles_with_decode` 中 `resolve_decoded` 的 closure 呼叫。
  - `src/audio.rs`：
    - `AudioManager.sound_cache` 改存 `HashMap<SoundId, Arc<[u8]>>`。
    - `AudioManager::new_with_resource_packs` 載入資源包時直接沿用 `Arc<[u8]>`，程序化 WAV 合成完成時僅轉一次 `Arc<[u8]>`。
    - `get_source` 為每次播放建立獨立的 `Cursor<Arc<[u8]>>` 與 `rodio::Decoder`，僅 clone 16-byte 的 Arc handle，徹底消除播放時的整份 Vec clone。
    - one-shot (`play_sound`)、3D sound (`play_sound_3d`)、looping sound (`start_looping_sound`) 均共用 `get_source`，各自保留獨立播放游標。
    - `sound_bytes_are_decodable` 改收 `&Arc<[u8]>`，消除測試解碼時的 `bytes.to_vec()`。
    - 刪除舊磁碟 helper `load_or_synthesize_sound` 以及僅為其存在的 `temp_sound_path`、`remove_temp_sound` 與頂層 `std::path::Path` 引用。
    - 將 `invalid_wav_uses_decodable_procedural_fallback` 與 `valid_wav_is_loaded_without_replacing_its_bytes` 遷至透過 `ResourcePackManager` 的正式載入路徑，並驗證快取與資源包資產共享同一 `Arc::as_ptr` 位址。
    - 新增無音訊硬體相依的 decoder 測試 `decoders_from_same_cache_entry_sample_independently_and_share_arc_buffer`，驗證同一快取建立之多個 `Cursor<Arc<[u8]>>` 及 `rodio::Decoder` 各自 seek/read/sample 互不干擾，並透過 `Arc::as_ptr` 與 `Arc::strong_count` 驗證共用同一 Arc 配置。
  - 文件更新：
    - `ARCHITECTURE.md`：記錄 `resolve_decoded` 的 `&Arc<[u8]>` 契約與 `AudioManager` 的 `Arc<[u8]>` 快取和 zero-copy decoder 架構。
    - `plans/11_code_cleanup/README.md`：更新工作包 16 狀態為「已完成」。
- 實際命令與結果：
  - `cargo check --all-targets`（通過，0 errors）
  - `cargo test audio::tests::invalid_wav_uses_decodable_procedural_fallback`（通過，1 passed）
  - `cargo test test_wav_synthesis`（通過，1 passed）
  - `cargo test chest_feedback_synthesis_is_deterministic_and_distinct`（通過，1 passed）
  - `cargo test weather_category_gain_only_affects_rain_and_thunder`（通過，1 passed）
  - `cargo test --bin icraft audio`（通過，8 passed, 0 failed）
  - `cargo test --lib resources`（通過，17 passed, 0 failed）
  - `cargo test --bin icraft texture`（通過，11 passed, 0 failed）
- 淨刪碼／保留原因：
  - 刪除舊 `load_or_synthesize_sound` 及測試臨時檔案 helper（~35 行死碼）。
  - 消除每次 `get_source` 播放時對完整音訊 Vec 的拷貝分配，改為 16-byte Arc handle 複製。
  - 消除資源包載入時對聲音 bytes 的二次分配，直接沿用 ResourcePackManager 內的 Arc 配置。
  - 保留字幕隊列、音量計算、空間化音效與無裝置時靜默降級語意。
