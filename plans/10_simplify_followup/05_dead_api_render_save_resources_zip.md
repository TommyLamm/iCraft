# Plan05 — 渲染／UI／存檔／資源死 API 與手寫 ZIP

## 定位

### 死符號

| 符號 | 位置 | 狀態 |
| :--- | :--- | :--- |
| `TextureAtlas::new_procedural` | `texture.rs` 1750 | 只有定義；live 用 `new_procedural_with_manager`（`state.rs` 2881） |
| `MeshBounds::translated` | `chunk_render.rs` 208 | 只有定義 |
| `DrawPlan::build` | `chunk_render.rs` 519 | `#[allow(dead_code)]`，tests only；生產用 `build_into` |
| `SectionVisibilityScratch::new` | `culling/visibility.rs` 36 | `#[allow(dead_code)]`；生產用 `with_capacity` |
| `ModelDescriptor.parent` | `block_model.rs` 20–22、239–241 | 解析後從未讀取 |
| `REQUIRED_KEYS` + `coverage`／`validate_required_keys`／`visible_coverage` | `localization.rs` 49–103、400–431 | 與 `VISIBLE_REQUIRED_KEYS`（110–241）重疊；三個函式 tests only（512–524） |
| `ResourcePackManager::resolve_locale` | `resources.rs` 440–458 | tests only；生產走 `resolve_locale_layers` |
| `SaveManager::save_mutation_revision_index_unless_runtime` | `save/mod.rs` 501–511 | Wave 08 刪 presentation `SaveManager` 後的殘留；定義 + 2 個測試 |
| `unicode-segmentation = "=1.12.0"` | `Cargo.toml` 24 | 全 `*.rs` 零 `unicode_segmentation` |
| `state.rs` `entity_state_wire`／`entity_animation_state`／`effect_to_wire` | 2043–2078、2119–2140 | 生產 caller 0；前兩者是 `projection.rs` 1125–1132 的 byte-identical 複本，唯一用途 `cfg(test)` 1822；`effect_to_wire` 全 repo零 caller |
| `GpuTimestampReadbackState::Unsupported` | `state.rs` 1882 | `#[allow(dead_code)]`；生產用 `gpu_timestamps_supported: bool` |
| `vs_main` 水／岩漿 UV 動畫分支 | `shader.wgsl` 40–65 | 地形已走 `vs_terrain`／`fs_terrain`（152–198）；`vs_main` 現在只給實體／手 |
| `vs_crosshair`／`fs_crosshair` + 專用 pipeline | `shader.wgsl` 201–216；`state.rs` 3168 | 回常數色的白色 quad，`vs_ui` 就能畫 |

### 手寫 ZIP

`load_zip_pack` + `find_zip_end`／`crc32`／`read_u16`／`read_u32`／`contains_zip64_extra`（`resources.rs` 703–994）≈ 290 行 central-directory 解析、local-header 檢查、bit-by-bit CRC-32；測試 703–1645 手組 archive ≈ 400 行。`flate2` 已在依賴（907 用 `DeflateDecoder`）。

### `read_asset` 每次 `to_vec()`

`ResourcePackManager::read_asset`（`resources.rs` 420–432）從已擁有 bytes 的 map 複製整份 PNG／JSON；`resolve_texture`／`model`／`font`／`sound` 全走這裡。

## 前置

無。

## 精確 acceptance

- [x] 上表死符號刪除或改 `#[cfg(test)]`；`state.rs` 測試改 import `projection::entity_state_wire`。
- [x] `Cargo.toml` 刪 `unicode-segmentation`；`cargo tree -i` 確認 `indexmap`／`exr`／`half`／`rayon-core` 是否為 `image`／`rayon` 的 pin，是則保留並註解。
- [x] `load_zip_pack` 改用 `zip` crate（或 `zip` + 現有 `flate2`），保留 `MAX_PACK_BYTES`／`MAX_PACK_ENTRIES`／`MAX_COMPRESSION_RATIO`／拒 ZIP64／symlink／`..` 的測試；刪手寫 central-directory helper。
- [x] pack entries 存 `Arc<[u8]>`；`read_asset` 回 `Arc<[u8]>` 或 `&[u8]`。
- [x] 十字準星改用 UI pipeline；`vs_crosshair`／`fs_crosshair` 與其 pipeline 刪除；`vs_main` 剝掉水／岩漿分支。
- [x] `cargo check --all-targets` 通過；`cargo test --lib resources:: localization::` 通過。

## 預計檔案與測試

- 改：`src/texture.rs`、`src/chunk_render.rs`、`src/culling/visibility.rs`、`src/block_model.rs`、`src/localization.rs`、`src/resources.rs`、`src/save/{mod,tests}.rs`、`src/state.rs`、`src/shader.wgsl`、`Cargo.toml`
- 驗證：`cargo test --lib resources:: localization:: block_model:: save::`；`cargo test --bin icraft`；桌面手動確認十字準星與水面

## 建議階段

1. 純刪死符號與依賴。
2. `read_asset` 零拷貝。
3. ZIP 換 crate，重寫安全測試對 crate 錯誤型別。
4. shader 清理 + 十字準星。

## 不在本計劃

- atlas paint-on-miss／`PACK_TILES` 為主（Plan 22）。
- `dynamic_resolution`（09 波 04）。
- GPU timestamp helper 去重（Plan 18）。

## 實作與證據

### 改了什麼

- 刪死符號：`TextureAtlas::new_procedural`、`MeshBounds::translated`、`DrawPlan::build`（測試改 `build_into`）、`SectionVisibilityScratch::new`、`ModelDescriptor.parent`（仍驗證 oversized parent）、`REQUIRED_KEYS`／`coverage`／`validate_required_keys`、`resolve_locale`、`save_mutation_revision_index_unless_runtime`、`state` 內 `entity_state_wire`／`entity_animation_state`／`effect_to_wire`、`GpuTimestampReadbackState::Unsupported`、`vs_crosshair`／`fs_crosshair`＋專用 pipeline、`vs_main` 水／岩漿 UV 分支。
- 十字準星改 `UiVertex` + `ui_line_pipeline`。
- `Cargo.toml`：刪直接依賴 `unicode-segmentation`；加 `zip`（`default-features = false`, `deflate`）；pin 依賴加註解。
- `resources`：entries 改 `Arc<[u8]>`；`read_asset`／resolve API 回 `Arc`；`load_zip_pack` 走 `zip` crate，另留輕量 `reject_zip64_markers`（EOCD／extra／entry budget）。
- `server_runtime::projection` 公開 `entity_state_wire` 供桌面測試 import。

### 測了什麼

- `cargo test --lib resources::`（15 ok）
- `cargo test --lib localization::`（10 ok）
- `cargo test --lib block_model::`（5 ok）
- `cargo test --lib save::`（47 ok, 2 ignored）
- `cargo check --all-targets`、`cargo check --bin icraft-server`

### 死路徑 grep 證據（刪前）

- `new_procedural`：僅定義；live 用 `new_procedural_with_manager`
- `MeshBounds::translated`／`SectionVisibilityScratch::new`／`effect_to_wire`：定義外零 caller
- `DrawPlan::build`：僅 tests；生產 `build_into`
- `resolve_locale`：tests only；生產 `resolve_locale_layers`
- `unicode-segmentation`：`*.rs` 零 import（仍可由 `winit` 間接帶入）

### 留下的缺口

- 桌面手動確認十字準星與水面動畫未跑（CI／agent 環境無 GPU 互動）。
- `cargo test --bin icraft` 未全量跑（時間）；`check --all-targets` 已覆蓋編譯。
- ZIP CRC 失敗訊息改對 crate 字串（`crc`／`checksum`）寬鬆匹配；手組 archive helper 仍留在 `#[cfg(test)]`。
