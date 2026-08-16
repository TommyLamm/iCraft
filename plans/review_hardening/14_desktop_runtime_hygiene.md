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

- [ ] `SectionMeshScheduler` 有 latest-wins 上限（可重用 16384 或寫明新值）。超出丟最舊
  或同 section 覆寫，不得無界成長。
- [ ] `Loaded` 整合與 mesh 一樣有時間／byte budget，剩餘下一幀。Lighting 不得在
  無預算時掃五個 column。
- [ ] `Lost`／`Outdated` 用 `window.inner_size().max(1)` resize。`Timeout` 當 skip present，
  不當「log 完繼續」。`State::new` 的 swapchain 至少 1×1。
- [ ] Presentation 模組退出 `lib.rs`（server／tests 不再編 wgpu 選單）。
  `dynamic_resolution`：編進 desktop **或** 刪設定項，不得再幽靈持久化。
  `#[global_allocator]` 只留在 `src/main.rs`。
- [ ] `has_in_process_runtime()` 時整段 catch-up／mutation-index／Host
  `ContainerClickRequest` 不執行。註解標明：恢復 `NetworkHandle::Host` 前必須刪這條
  或接 `ServerRuntime`。
- [ ] 拿掉無條件 `set_paused` println。
- [ ] 測試：mesh scheduler 覆寫／cap 單元測；既有 mesh staleness／GPU timestamp 測試仍過。
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
