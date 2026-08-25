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

- [x] `SoundMaterial` 不再定義在 `src/audio.rs`。放到 `src/world/block.rs`，由 `world`
      模組 `pub use`。`BlockType::sound_material` 的對照表 bit-identical。
- [x] `src/world/block.rs` **不得** `use crate::audio`（已完全移除）。
- [x] `update_mobs` 不再接受 `&mut crate::audio::AudioManager`。改為：
      - 定義 `MobSoundEvent`，透過 callback `on_sound: impl FnMut(MobSoundEvent, Vec3)`
        通知音效事件，由 desktop (`legacy_sim.rs`) 映射至 `SoundId` 並呼叫 `play_sound_3d`。
      - 單元測試傳入 no-op sink `|_, _| {}`。未改動任何 mob 傷害／爆炸／掉落數字。
- [x] `src/lib.rs` 刪除 `pub mod audio`。`src/main.rs` 改宣告 `mod audio`
      （desktop-only）。desktop 既有 `crate::audio` 路徑由 `main.rs` 消化。
- [x] `cargo check --bin icraft-server` 與 `cargo check --lib` 的 rustc JSON
      **不再**出現 `src/audio.rs`。
- [x] `Cargo.toml` 的 `rodio`／`wgpu`／`winit` **保持** 普通依賴。未修改 Cargo features。
- [x] 既有測試期望值不變。`BlockType::sound_material` 對照表不變。

## 預計檔案與測試

- 修改：`src/world/block.rs`、`src/audio.rs`、`src/mob.rs`、`src/lib.rs`、`src/main.rs`、`src/presentation/legacy_sim.rs`、`ARCHITECTURE.md`。
- 測試：
  - `cargo check --bin icraft-server`（通過，不再編譯 `src/audio.rs`）
  - `cargo check --lib`（通過）
  - `cargo check --bin icraft`（通過）
  - `cargo test --lib world::`（通過，73 passed）
  - `cargo test --lib mob::`（通過，15 passed）
  - `cargo test --bin icraft interpolation_midpoint_and_clamps -- --test-threads=1`（通過，1 passed）
  - rustc JSON：server／lib 不含 `audio.rs`；desktop 仍含（已驗證）

## 建議階段

1. 搬 `SoundMaterial`，`world/block.rs` 去掉 `crate::audio`。`cargo test --lib world::`。（已完成）
2. 拿掉 `update_mobs` 的 `AudioManager` 參數。跑 `mob::`。（已完成）
3. `audio` 改由 `main.rs` 宣告。確認 server JSON 不再列 `audio.rs`。（已完成）

## 不在本計劃

- `rodio`／`wgpu` optional、`client` feature、workspace crates。
- 拆 `culling`（24）。
- leftover cfg（21）本體。
- 改腳步／破壞音量公式。
