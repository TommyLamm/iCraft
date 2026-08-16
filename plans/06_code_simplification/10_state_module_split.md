# Plan10 — `state.rs` 模組拆分

## 定位

`src/state.rs` 約 25,643 行，兩個 `impl State`（2016、6094），`pub struct State` 從 ~5668 起約 300 個欄位。它同時擁有 GPU、串流、UI、TCP `NetworkHandle`、embedded runtime、以及整條 leftover 模擬。

最大的函式（掃描當日）：

| 函式 | 約略行 |
| --- | --- |
| `State::render` | 20502–24601（~4,100） |
| `State::new` | 6150–7855（~1,700） |
| `handle_single_network_event` | 9366–10556（~1,190） |
| `tick_simulation` | 12378–13351（~970） |

`tick_simulation` 在有 embedded runtime 時只應 `tick_authority_boundary`，但同一函式仍含吃喝、autosave、水／熔岩、紅石、漏斗、釀造、凋靈、天氣、農田、睡眠 skip、撿取、虛空傷害、inline 樹葉腐爛 BFS。大多數包在 `if authoritative`（= 無 runtime 的 leftover）。

`State::render` 已有 `add_ui_quad`，物品欄仍手寫 6-vertex。本計劃允許把重複 quad 換成既有 helper，但 **禁止重排 render pass**（sky → opaque → entities → translucent → particles → mining → hand → UI → crosshair → text）。

這是結構搬移，不是刪 leftover。現役選單啟動都有 embedded runtime；測試／文件仍承認無 boundary 的世界。

## 前置

- 02 已合併：拓撲 enum 已存在，搬移時用它當閘門，不要再發明布林對。
- **建議** 06 已合併：click／inventory 已抽過，10 就不必一邊拆檔一邊拆 click。若 06 未合併，本計劃仍可拆 `new`／`render`／`tick_simulation`，但不得重寫 `handle_click`。

## 精確 acceptance

- [x] `src/state.rs` 不再同時定義 GPU arena、網路 inbound staging、`EmbeddedRuntimeBridge`、以及 `State::render` 全文。至少拆出這些模組（名稱可微調，責任不可混）：
  - `src/presentation/gpu_terrain.rs`（或同等）：`RenderRegion`、`ChunkMesh`、`GpuSectionMesh`、upload／compaction
  - `src/presentation/network_inbound.rs`：`NetworkInbound`、`NetworkStaging`、`NetworkHandle` 的送出／drain
  - `src/presentation/interpolation.rs`：`ReplicatedEntityState`、`RemotePlayerState`
  - `src/presentation/legacy_sim.rs`：從 `tick_simulation` 剪出的 `if authoritative` 世界擁有者本體
  - `src/presentation/frame.rs` 或 `render.rs`：`State::render` 的 prepare／encode 分段
- [x] `State` 仍是 desktop 合成根，欄位可暫留在原 struct（一次把 300 欄位拆進子 struct 容易打輸借用檢查）。本計劃成功標準是 **檔案邊界**，不是 ECS。
- [x] `tick_simulation` 現役路徑清楚可讀：有 runtime → `tick_authority_boundary` + 表現層（鍵、衝刺 latch、腳步、`update_chunks`）。leftover 世界本體只存在 `legacy_tick_owned_world`（或 `legacy_sim` 模組）。
- [x] `State::new` 抽 `create_gpu_context`、`create_pipelines`、`load_launch_world_state`（或同等）。Windows DX12 強制（`~6158`）與「embedded 不載 `player.dat`／spawn halo」註解必須跟著 helper 走。
- [x] `State::render` 抽 `prepare_terrain_draw_plan`、`prepare_entities`、`prepare_hand`、`build_hud`、`encode_frame`。pass 順序與 timestamp query、frame-slot acquire／wait（~20672）不得重排。
- [x] 手寫 UI quad 在新 HUD／物品欄路徑改走既有 `add_ui_quad`／border helper。視覺矩形不變。
- [x] `handle_single_network_event` 的 no-op `GameplayRequest` 臂：若 Host 確定不再產生它，改 `debug_assert` 或刪並在證據列出呼叫點搜尋結果。不確定就留。
- [x] 不得刪 leftover 模擬。不得把權威行為搬進這些 presentation 模組。
- [x] `cargo check --bin icraft`、`cargo test --bin icraft` 裡現有 `state` 單元測（interpolation、mesh invalidation、GPU timestamp、pause 拓撲）通過。

## 預計檔案與測試

- 新增：`src/presentation/mod.rs` 及上面列出的子檔（desktop `main.rs` 要 `mod presentation`；`lib.rs` **不要** 為了 server 再 export GPU 選單）。
- 修改：`src/state.rs`、`src/main.rs`（模組樹）、必要時 `src/lib.rs`（只在測試需要共用純函式時 `pub use`，不得把 wgpu menu 拉回 server）。
- 測試：
  - `cargo check --bin icraft`
  - `cargo check --bin icraft-server`（不得突然編譯 `src/menu.rs`）
  - `cargo test --bin icraft interpolation_midpoint_and_clamps gpu_timestamp_state_tests mesh_invalidation -- --test-threads=1`
  - `cargo test --bin icraft multiplayer_host_keeps_world_ticks singleplayer_pause -- --test-threads=1`
  - `cargo test --test review_hardening_embedded_presentation -- --test-threads=1`
  - `cargo test --lib presentation_inventory_policy::`

## 建議階段

1. 先搬 **沒有 State 借用糾纏** 的型別：`NetworkInbound`／staging、interpolation、`GpuSectionMesh`／`ChunkMesh`。每搬一個檔 `cargo check --bin icraft`。
2. 剪 `tick_simulation` 的 `authoritative` 本體到 `legacy_sim.rs`，閘門留在 `State`。
3. 抽 `State::new` 的 GPU／pipeline helper，DX12 與 embedded 空白啟動註解一起走。
4. 抽 `render` 的 prepare／encode。先抽、再把物品欄 quad 換成 helper。
5. 跑 desktop 單元測與 embedded presentation。

## 不在本計劃

- 刪 `legacy_tick_owned_world`。
- 把 Host `NetworkHandle` 廣播遷出（05／08 之後另案）。
- 重開 dynamic resolution offscreen。
- 把 desktop 改成 `use icraft::*` 單一 crate 樹（架構刻意雙編譯）。
- 一次拆完 `menu.rs`（4.8k；選單 hit-rect 去重可列後續，不要綁在 10）。

## 實作與證據

檔案邊界（desktop `main.rs` 宣告 `mod presentation;`；`lib.rs` 沒有 `pub mod presentation`）：

| 檔案 | 責任 | 約略行 |
| --- | --- | --- |
| `src/presentation/interpolation.rs` | `PlayerSnapshot`、`RemotePlayerState`、`ReplicatedEntityState`、sample／clamp | 330 |
| `src/presentation/network_inbound.rs` | `NetworkInbound`、`NetworkStaging`、`NetworkHandle` drain／send | 1,503 |
| `src/presentation/gpu_terrain.rs` | `RenderRegion`、`ChunkMesh`、`GpuSectionMesh`、upload／compaction helpers | 493 |
| `src/presentation/legacy_sim.rs` | leftover 世界本體（`legacy_tick_item_use`／`legacy_tick_world_systems`／`legacy_tick_owned_environment`／`legacy_tick_owned_world`）。以 `#[path]` 掛在 `state` 下才能碰私有欄位 | 541 |
| `src/presentation/bootstrap.rs` | `create_gpu_context`（Windows DX12 註解）、`load_launch_world_state`（embedded 不載 player.dat／spawn halo 註解）、`create_pipelines`（shader＋layout） | 377 |
| `src/presentation/frame.rs` | `prepare_terrain_draw_plan`、`prepare_entities`、`prepare_hand`、`build_hud`、`encode_frame`。同樣以 `#[path]` 掛在 `state` 下 | 3,767 |
| `src/state.rs` | 仍是 desktop 合成根；`EmbeddedRuntimeBridge`、300 個欄位、`tick_simulation` 閘門、`State::new`／`State::render` 編排器 | 18,663（原先約 25,643） |

`tick_simulation` 現役路徑：有 runtime → `tick_authority_boundary`；表現層（鍵、衝刺 latch、腳步、`update_chunks`）留在 `State`。leftover 只從 `legacy_sim` 呼叫。

`State::render` 編排器保留 frame-slot acquire／wait 在 terrain prepare 與 entity upload 之間；timestamp query 仍在 `encode_frame`。pass 順序未改。HUD／物品欄 6-vertex 手寫 quad 改走 `add_ui_quad`。

`handle_single_network_event` 的 no-op `GameplayRequest` 臂**留下**。呼叫點搜尋：

- `src/server_runtime.rs`：`RuntimeInput::submit_request` 仍 `try_send(ServerToHost::GameplayRequest { ... })`；`ServerRuntime::handle_event` 仍吃這個 variant（權威路徑，不是 presentation）。
- `src/network/server.rs`：TCP `NetworkServer` 仍 `server_to_host.send(ServerToHost::GameplayRequest)`。
- `src/presentation/network_inbound.rs`：`NetworkHandle::Host` drain 仍 map 成 `NetworkInbound::GameplayRequest`。
- 現役 SP／Host 用 `NetworkHandle::None`，但 leftover Host handle 與 server 測試仍會產生它。不確定可刪，因此保留 no-op 臂。

`EmbeddedRuntimeBridge` 留在 `state.rs`：搬走會打輸對 `ServerRuntime` 的私有可見性／借用，且成功標準的模組清單沒有強制它獨立成檔。

測試（全部通過）：

```
cargo check --bin icraft
cargo check --bin icraft-server
cargo test --bin icraft -- --test-threads=1 interpolation_midpoint_and_clamps
cargo test --bin icraft -- --test-threads=1 gpu_timestamp_state_tests
cargo test --bin icraft -- --test-threads=1 mesh_invalidation
cargo test --bin icraft -- --test-threads=1 multiplayer_host_keeps_world_ticks
cargo test --bin icraft -- --test-threads=1 singleplayer_pause
cargo test --test review_hardening_embedded_presentation -- --test-threads=1
cargo test --lib presentation_inventory_policy::
```

`cargo check --bin icraft-server` 只編譯 library 警告（audio／chunk_manager／server_runtime 等），**沒有**編譯 `src/menu.rs` 或 `src/presentation/gpu_terrain.rs`。

留下的缺口：

- `state.rs` 仍約 18.7k 行（`handle_single_network_event`、`State::new` 其餘 GPU pipeline 物件、輸入／物品欄、leftover 互動）。
- 個別 wgpu pipeline 物件仍在 `State::new`（與 buffer 交錯）；`create_pipelines` 只抽出 shader＋layout。
- `EmbeddedRuntimeBridge` 未拆檔。
- leftover 夜跳／樹葉／氧氣已拆回 `legacy_tick_night_skip` → pickup／void／lava → `legacy_tick_leaf_decay` → cactus → `legacy_tick_oxygen` → `total_time` → `legacy_tick_owned_world`，與拆檔前順序一致。
