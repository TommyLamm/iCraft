# Plan06 — lib 圍籬：桌面模組移出、weather／advancements 收桌面

## 定位

`ARCHITECTURE.md` 明令 GPU／UI 不得進 `lib.rs`，但 `lib.rs` 55–62 仍以 `pub mod` 發佈下列只有桌面使用的模組，`icraft-server` 每次都編：

| 模組 | 規模 | 桌面 caller | server caller |
| :--- | :--- | :--- | :--- |
| `accessibility.rs` | ~514 行 | `audio.rs`、`menu.rs`、`state.rs`、`frame.rs` | **0** |
| `localization.rs` + `include_str!` `en_us.json`／`de_de.json` | ~793 行 + JSON | `menu.rs`、`state.rs` | **0** |
| `culling/visibility.rs`（`EntityLosManager` 252、section traversal 61） | ~700 行 + 一條 worker 執行緒型別 | `state.rs`、`frame.rs` | **0**（server 只用 `culling::is_los_blocked`／`is_section_occluder`，`server_world.rs` 674–675） |
| `chunk_render.rs` `TerrainVertex::desc` | 123–145 | `state.rs` 2953、3031 | **0**；檔頭 2–4 宣稱「不依賴 wgpu」卻回 `wgpu::VertexBufferLayout` |
| `advancements.rs` | ~980 行 | `State::trigger_advancement`（`state.rs` 5395）；唯一生產 trigger 是 `network_event.rs` 228 的 `MineBlock` | **0**；craft／kill／brew／enchant／trade／fish／dimension trigger 無權威 caller，join 端與 dedicated 不共享進度 |
| `weather.rs` | ~390 行 | `state.rs` 6360–6363 每 frame tick；含第二份 `ClimateSystem::new(seed)` 與 `authority_rng` | `ServerWorld::tick` 從不碰；live `TimeSync` 永遠送 `weather: 0`（`server_runtime/ingress.rs` 271–285） |

weather 是 GPU 執行緒上的第二個氣候／RNG 權威：join 端永遠與 server（Clear）不同步；雪／閃電世界突變不可能權威化。

## 前置

無。

## 精確 acceptance

- [ ] `accessibility`、`localization`、`advancements`、`weather` 改為 `main.rs` 的 `mod`（或 `src/presentation/` 子模組）；`lib.rs` 不再 `pub mod` 它們。
- [ ] `culling` 拆：`los.rs`＋`connectivity.rs`＋`is_section_occluder` 留 lib；`visibility.rs`（含 `EntityLosManager`、`SectionVisibilityScratch`）移桌面。
- [ ] `TerrainVertex::desc` 移到 `state.rs`／`bootstrap.rs` pipeline 旁；`chunk_render.rs` 不再 `use wgpu`。
- [ ] weather：刪 presentation 模擬與第二份 `ClimateSystem`；HUD／粒子／雨聲讀一個由 `TimeSync.weather` 驅動的 enum（目前永遠 Clear）。`GameRules.do_weather_cycle` 保留欄位。
- [ ] advancements：保留桌面 UI／toast／save 欄位；權威 trigger 若要保留只留 `MineBlock`，其餘 trigger enum 變體刪除。
- [ ] `cargo check --bin icraft-server` 不再編 wgpu vertex layout、lang JSON、LOS worker、advancement tree、weather。
- [ ] `ARCHITECTURE.md` 「Do not add to `lib.rs`」列表補這些模組；Code map 更新。

## 預計檔案與測試

- 改：`src/lib.rs`、`src/main.rs`、`src/culling/{mod,visibility}.rs`、`src/chunk_render.rs`、`src/state.rs`、`src/presentation/{bootstrap,frame,network_event}.rs`、`src/weather.rs`、`src/advancements.rs`、`src/accessibility.rs`、`src/localization.rs`、`ARCHITECTURE.md`
- 驗證：`cargo check --bin icraft-server`；`cargo check --all-targets`；`cargo test --bin icraft`（單元測試隨模組搬家）；`cargo test --lib culling::`

## 建議階段

1. 先搬 `accessibility`／`localization`（純 `mod` 路徑）。
2. 拆 `culling`，搬 `TerrainVertex::desc`。
3. weather 刪模擬。
4. advancements 桌面化。

## 不在本計劃

- 刪 `EntityLosManager` 本體（Plan 18）。
- `State` UI 佈局搬家（Plan 27）。
- 把 weather 權威化進 `ServerWorld`（另案；本波只刪桌面第二權威）。
