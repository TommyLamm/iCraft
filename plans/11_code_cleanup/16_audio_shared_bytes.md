# 16 — 音效快取共用 bytes，移除播放時整份複製

狀態：待執行。基線：`83e751d`，2026-09-17。
前置：15；音效舊 helper 與相關測試在本包一起遷移。

## 定位與證據

- `src/audio.rs:92`：`sound_cache: HashMap<SoundId, Vec<u8>>`。
- `src/audio.rs:495–500`：從 ResourcePackManager 的 Arc bytes 轉成 Vec。
- `src/audio.rs:561–563`：每次 `get_source` clone 整個 Vec，再交給 Decoder。
- `src/resources.rs:971`：validator 也先 clone bytes；由 15 移到消費者邊界後一起消除。
- 這是活躍音效路徑，不是死碼。

## 實作步驟

1. sound cache 改存 `Arc<[u8]>`；資源包載入沿用 Arc，程序化 WAV 只在合成完成時轉一次。
2. `get_source` 為每個播放建立自己的 `Cursor<Arc<[u8]>>` 和 decoder，複製 Arc handle；實作時以本倉 rodio 0.19 trait bounds 驗證型別。
3. one-shot、3D sound、looping sound 共用這個 decoder 建構入口，保留各自播放 cursor。
4. 移除 `load_or_synthesize_sound` 的舊磁碟 helper 與僅為其存在的 wrapper；把其「有效 bytes 不覆寫／壞 WAV 回退」測試遷至資源包正式載入路徑。
5. 不引入 PCM 常駐 cache；那會改記憶體取捨，也不是刪除 bytes clone 所必需。

## 驗證

既有：`audio::tests::invalid_wav_uses_decodable_procedural_fallback`、`valid_wav_is_loaded_without_replacing_its_bytes`（遷移入口）、`test_wav_synthesis`、`chest_feedback_synthesis_is_deterministic_and_distinct`、`weather_category_gain_only_affects_rain_and_thunder`。

新增不需音效裝置的 decoder 測試：同一快取建立兩個 decoder，各自 seek/read 或播放取樣互不干擾；驗證 clone 來源使用同一 Arc 配置。實機檢查 one-shot／雨聲循環與音量切換。

## 驗收

`get_source` 不 clone 音訊 Vec；每次播放只建獨立 cursor／decoder。保留字幕與音量語意，無法使用音效裝置時仍可安靜降級。

## 實作紀錄

尚未執行；完成時填寫改動、實際命令／結果、淨刪碼和文件更新。
