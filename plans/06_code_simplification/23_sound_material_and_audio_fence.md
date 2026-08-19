# Plan23 — `SoundMaterial` 抽出，audio 移出 library

## 定位

`icraft-server` 連同一個 `icraft` lib。`src/world/block.rs` 第一行是
`use crate::audio::SoundMaterial`，因此 dedicated server 為了一個純資料 enum
去編譯 `audio.rs`（rodio、`OutputStream`）。

掃描當日 lib 裡碰到 `crate::audio` 的生產點：

| 檔 | 用法 | 解耦方式 |
| --- | --- | --- |
| `src/world/block.rs` | `BlockType::sound_material() -> Option<SoundMaterial>` | enum 搬走 |
| `src/mob.rs` `update_mobs` | `&mut AudioManager` 參數 | leftover／測試才呼叫；改成不拿 rodio 型別 |
| `src/state.rs`、leftover 三檔 | `play_sound` | 留在 desktop |

`chunk_render.rs` 已是無 wgpu 的 CPU mesh helper，對照這個切法：`SoundMaterial`
是方塊屬性，不是音訊裝置。

本計劃讓 `cargo check --bin icraft-server` 不再編譯 `src/audio.rs`。
**不**把 `rodio` 改成 Cargo optional（那要等 23／24 都落地，另開 31+）。

## 前置

無。可與 22、24 並行。

**建議** 21 已合併：`update_mobs` 的生產呼叫只剩 leftover（cfg 後只在 test／
`legacy_owner`）。若 21 未合併，仍可抽 `SoundMaterial`，但 `update_mobs` 簽名
必須有無 rodio 的呼叫端（leftover 繼續播）。

## 精確 acceptance

- [ ] `SoundMaterial` 不再定義在 `src/audio.rs`。放到 `src/world/block.rs` 或
      `src/world/sound.rs`，由 `world` 模組 `pub use`。`BlockType::sound_material`
      的對照表 bit-identical。
- [ ] `src/world/block.rs` **不得** `use crate::audio`。
- [ ] `update_mobs` 不再接受 `&mut crate::audio::AudioManager`。改為：
      - 回傳／callback 一小組已存在的 `SoundId`（若 `SoundId` 仍在 audio，改用
        不含 rodio 的事件 enum，desktop 再 map 到 `SoundId`），或
      - leftover 呼叫端在 `update_mobs` 之後自己 `play_sound`。
      單元測用 no-op sink。不得改 mob 傷害／爆炸／掉落數字。
- [ ] `src/lib.rs` 刪除 `pub mod audio`。`src/main.rs` 改宣告 `mod audio`
      （與 `presentation` 相同：desktop-only，不得加回 lib）。
      desktop 既有 `crate::audio` 路徑用 `main.rs` 的 `mod` 消化；不要再
      `pub use icraft::audio`。
- [ ] `cargo check --bin icraft-server` 與 `cargo check --lib` 的 rustc JSON
      **不得**出現 `src/audio.rs`。
- [ ] `Cargo.toml` 的 `rodio`／`wgpu`／`winit` **保持** 普通依賴。不得開
      `client` feature 或 workspace crate。
- [ ] 既有測試期望值不變。`BlockType::sound_material` 單元測（若有）對照表不變。

## 預計檔案與測試

- 新增或修改：`src/world/block.rs` 或 `src/world/sound.rs`、`src/audio.rs`、
      `src/mob.rs`、`src/lib.rs`、`src/main.rs`、leftover 呼叫端（若改 signature）。
- 測試：
  - `cargo check --bin icraft-server`
  - `cargo check --lib`
  - `cargo check --bin icraft`
  - `cargo test --lib world::`
  - `cargo test --lib mob::`
  - `cargo test --bin icraft interpolation_midpoint_and_clamps -- --test-threads=1`
  - rustc JSON：server／lib 不含 `audio.rs`；desktop 仍含

## 建議階段

1. 搬 `SoundMaterial`，`world/block.rs` 去掉 `crate::audio`。`cargo test --lib world::`。
2. 拿掉 `update_mobs` 的 `AudioManager` 參數。跑 `mob::`。
3. `audio` 改由 `main.rs` 宣告。確認 server JSON 不再列 `audio.rs`。

## 不在本計劃

- `rodio`／`wgpu` optional、`client` feature、workspace crates。
- 拆 `culling`（24）。
- leftover cfg（21）本體。
- 改腳步／破壞音量公式。
