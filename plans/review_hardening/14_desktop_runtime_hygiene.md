# Plan14 — 桌面 hitch、surface 與 lib 樹

## 定位

- `SectionMeshScheduler::enqueue` 無上限。`MAX_DIRTY_MESH_QUEUE = 16384` 只綁廢棄的
  chunk scheduler。視距 16 的 Overworld 約 26k section。Load 整合沒有 hitch budget：
  `process_terrain_worker_results` 對 mesh 有 4／2MiB／3ms，卻在同一幀整合所有
  `Loaded`（含主執行緒 lighting flood）。
- `SurfaceError::Lost` 用 `state.size` resize，選單用 `window.inner_size()`。
  `Outdated`／`Timeout` 只 println。`State::new` 不對 0×0 做 `max(1)`（選單有）。
  沒有 device-lost → 回選單。
- `lib.rs` re-export `menu`／`camera`／`chunk_render`／`texture`／`perf`，
  `icraft-server` 因此編進 wgpu 選單碼。`dynamic_resolution.rs` 不在任一 module tree，
  但 `GameSettings.dynamic_resolution` 仍持久化。`perf.rs` 的 `#[global_allocator]`
  同時進 desktop bin 與 library。
- Host 建構把 `NetworkHandle::None`，但 `process_join_catchups`、mutation-index、
  `ContainerClickRequest` 的 Host 臂仍編譯。`GameplayRequest` 已是 no-op。一旦
  `NetworkHandle::Host` 恢復就變成雙權威。
- `set_paused` 每次 Escape 印 `[Debug] set_paused called with:`。

## 前置

無。06／07 可能同時改 `process_terrain_worker_results`；本計劃只加 budget／queue cap，
不改「誰准寫方塊」。

## 精確 acceptance

- [x] `SectionMeshScheduler` 有 latest-wins 上限（可重用 16384 或寫明新值）。超出丟最舊
  或同 section 覆寫，不得無界成長。
- [x] `Loaded` 整合與 mesh 一樣有時間／byte budget，剩餘下一幀。Lighting 不得在
  無預算時掃五個 column。
- [x] `Lost`／`Outdated` 用 `window.inner_size().max(1)` resize。`Timeout` 當 skip present，
  不當「log 完繼續」。`State::new` 的 swapchain 至少 1×1。
- [x] Presentation 模組退出 `lib.rs`（server／tests 不再編 wgpu 選單）。
  `dynamic_resolution`：編進 desktop **或** 刪設定項，不得再幽靈持久化。
  `#[global_allocator]` 只留在 `src/main.rs`。
- [x] `has_in_process_runtime()` 時整段 catch-up／mutation-index／Host
  `ContainerClickRequest` 不執行。註解標明：恢復 `NetworkHandle::Host` 前必須刪這條
  或接 `ServerRuntime`。
- [x] 拿掉無條件 `set_paused` println。
- [x] 測試：mesh scheduler 覆寫／cap 單元測；既有 mesh staleness／GPU timestamp 測試仍過。
  `dynamic_resolution` 若編進 desktop，其測試要真的編譯。

## 預計檔案與測試

- 修改：`src/chunk_schedule.rs`、`src/state.rs`、`src/app.rs`、`src/lib.rs`、
  `src/main.rs`、`src/dynamic_resolution.rs`、`src/perf.rs`、`src/menu.rs`（若設定項刪除）。
- 測試：scheduler 單元測；`cargo check --bin icraft-server` 確認不再拉選單／wgpu
  （若拿掉 re-export 後 server 仍編譯）。

## 建議階段

1. Mesh queue cap。
2. Load 整合 budget。
3. Surface recovery。
4. `lib.rs` 收斂 + allocator + dynamic_resolution 去留。
5. 閘死 Host 雙路徑 + 刪 debug print。

## 不在本計劃

- 把 25k 行 `state.rs` 拆成模組（可在證據裡列建議切點，不要在本計劃做完）。
- 重開 offscreen dynamic resolution（需要 offscreen target + upscale + native UI）。

## 實作與證據

### 行為

- `SectionMeshScheduler::enqueue` 重用 `MAX_DIRTY_MESH_QUEUE = 16384`。同 `SectionKey` latest-wins 覆寫；超出時從 priority `BTreeSet` 丟掉最遠 pending（與廢棄 chunk scheduler 同一策略），queue 不得無界成長。
- `process_terrain_worker_results` 對 `Loaded` 加 `MAX_INTEGRATE_LOADS = 2`／`MAX_INTEGRATE_LOAD_BYTES = 2MiB`／共用 `MAX_INTEGRATE_TIME_MS = 3`。過期／restore 失敗不佔預算；通過後才從 `chunk_load_in_flight` 移除並做 lighting flood。沒預算就把 result 推回 queue 頭，下一幀再整合。
- `Lost`／`Outdated` 用 `window.inner_size()` 各軸 `max(1)` resize。`Timeout` 空臂 skip present。`State::new` swapchain `width/height.max(1)`。
- `lib.rs` 拿掉 `menu`／`camera`／`texture`。`Difficulty` 進 `game_rules`；`MultiplayerRole` 與 presentation load policy 進 `presentation_inventory_policy`；`load_world_creation_options` 進 `save`。`chunk_render`／`perf` 仍留在 library，因為 `world.rs` mesh 型別與 network queue stats 仍依賴它們。`dynamic_resolution` 編進 desktop（未重開 offscreen upscale）。`#[global_allocator]` 只在 `src/main.rs`。
- `has_in_process_runtime()` 時 `schedule_player_catchup`、`process_join_catchups`（含 mutation-index persist）、Host `ContainerClickRequest` 直接 return。註解寫明恢復 `NetworkHandle::Host` 必須刪這條或接 `ServerRuntime`。
- 刪掉 `set_paused` 的無條件 println。

### 測試

- `cargo test --lib chunk_schedule::`：7 passed（含 `section_scheduler_overwrite_keeps_latest_identity`、`section_scheduler_caps_pending_at_max_dirty_mesh_queue`）。
- `cargo test --lib presentation_inventory_policy::`：4 passed。
- `cargo test --bin icraft -- gpu_timestamp_state_tests mesh_invalidation mutation_scheduler section_mesh_result dynamic_resolution thread_alloc_count`：14 passed（含 7 個 `dynamic_resolution`、3 個 GPU timestamp、mesh staleness、desktop allocator）。
- `cargo check --bin icraft-server`：通過；警告裡沒有 `src/menu.rs`。
- `cargo check --bin icraft`：通過。
- `cargo fmt --all`：通過。

### 建議的 `state.rs` 切點（未做）

- `NetworkHandle`／`NetworkInbound` 與 inbound drain（約 3150–5600）。
- `State::new` GPU／swapchain／launch（約 6090–7900）。
- Host catch-up／mutation-index／container 殘留（約 8825–9160、10060）。
- `process_terrain_worker_results` 與 streaming schedule（約 11680–12190）。
- `render`／surface／GPU timestamp（約 20345–24500）。

### 留下的缺口

- `chunk_render` 仍經 `world.rs` 進 library，server 仍會編 wgpu mesh 型別；本計劃只保證不再編 wgpu **選單**。
- Dynamic resolution 控制器已編譯，但沒有 offscreen target／upscale，畫面仍是 native scale。
- Device-lost 回選單未做（acceptance 未要求）。
