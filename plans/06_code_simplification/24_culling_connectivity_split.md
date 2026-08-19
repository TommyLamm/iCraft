# Plan24 — culling 連通／LOS 與 frustum 拆檔

## 定位

`src/culling.rs` 約 1,242 行，三件不相關的事擠在一個 lib 模組，因此
`icraft-server` 為了 `ServerWorld` 的戰鬥 LOS 去編譯 GPU 可見性工人：

| 約略責任 | 符號 | 誰用 |
| --- | --- | --- |
| section 連通 | `SectionConnectivity`、`compute_section_connectivity*`、`is_section_occluder` | `world/mesh.rs`、`chunk_render`、`gpu_terrain` |
| 權威 LOS | `is_los_blocked` | `server_world.rs`（生產） |
| 表現層可見性 | `traverse_section_visibility_with_scratch`、`EntityLosManager`、`SectionVisibilityScratch` | `presentation/frame.rs`、`state.rs` |

`culling.rs` 本身沒有 `wgpu`，但 `EntityLosManager` 會開背景執行緒，desktop 才需要。
本計劃是 **move + `pub use`**，不改 fail-open、不改 LOS 步進、不把模組移出 lib。

拆完之後才談「可見性半邊改 desktop-only／`client` feature」（§7）。

## 前置

無。可與 22、23 並行。不要跟 10／28 搶 `frame.rs` 的 import（靠 `culling` 根
`pub use` 消化）。

## 精確 acceptance

- [ ] `src/culling.rs` 變成模組根（或 `src/culling/mod.rs`），並 `pub use` 舊路徑。
      現有 `crate::culling::SectionConnectivity`／`is_los_blocked`／
      `traverse_section_visibility_with_scratch`／`EntityLosManager` **零改**。
- [ ] 至少拆出（名稱可微調，責任不可混）：
      - connectivity：`SectionConnectivity`、`SectionConnectivityState`、
        `is_section_occluder`、`compute_section_connectivity*`
      - los：`is_los_blocked`（權威／mesh 可共用的純函式）
      - visibility：`SectionVisibilityScratch`、`traverse_section_visibility_with_scratch`、
        `EntityLosManager`、`LosIdentity`、`CullingCounters`
- [ ] 不得改 `SectionConnectivity::fail_open` 對 `Invalid => FULL`。
- [ ] 不得改 `is_los_blocked` 的步進／occluder 回呼契約。
- [ ] 不得改 traverse 的 fail-open 與 scratch 容量語意。
- [ ] 不得把 `culling` 從 `lib.rs` 拿掉，不得把 `wgpu` 改 optional。
- [ ] `cargo test --lib culling::` 期望值不變。

## 預計檔案與測試

- 新增：`src/culling/mod.rs`（或保留 `culling.rs` 當根 + 子檔）、
      connectivity／los／visibility 目的地。
- 修改：呼叫端只在 `pub use` 不夠時改 import。
- 測試：
  - `cargo test --lib culling::`
  - `cargo test --lib world::`
  - `cargo test --lib chunk_render::`
  - `cargo test --test plan31_authoritative_block_actions -- --test-threads=1`
  - `cargo check --bin icraft-server`
  - `cargo check --bin icraft`

## 建議階段

1. 先搬 `SectionConnectivity*` 與 `compute_section_connectivity*`，根檔 `pub use`。
2. 搬 `is_los_blocked`。跑 `culling::` 與 Plan31。
3. 搬 traverse／`EntityLosManager`。`cargo check --bin icraft`。

## 不在本計劃

- 把 visibility 半邊改 desktop-only 或 `client` feature。
- 刪 `EntityLosManager` 執行緒。
- 合併兩個 `ray_intersects_aabb`。
- workspace crates。
