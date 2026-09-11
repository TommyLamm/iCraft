# Plan18 — frame 熱路徑：只走 visible set、上傳批次、字型快取、刪 entity LOS worker

## 定位

| 問題 | 證據 | 每 frame 成本 |
| :--- | :--- | :--- |
| 地形 draw plan 在 visibility BFS 後再掃全部 mesh | `frame.rs` 31–46 填 `visible_sections_scratch`，61–114 又 `for (&coord, mesh) in &self.chunk_meshes { for section … }` 重做 frustum + `contains`；`lod_fills = Vec::new()`（59）；`query_radius(cam, render_distance_sq.sqrt())`（204–206） | O(loaded sections) AABB，RD 16 ≈ 數千；一次 sqrt；一次 alloc |
| 第四層實體剔除：專用 LOS worker | 208–255 已做距離、frustum、section-graph 三層；然後 `EntityLosManager::is_entity_visible`（`culling/visibility.rs` 500）spawn `entity_los_worker`（269–278）、clone voxel snapshot（319+）、DDA 48 步（`los.rs`）、cache／TTL／hysteresis（226–229）；近距／投射物／boss／物品 **fail-open**（508–525） | ~700 行 + 一條執行緒 + per-entity HashMap／snapshot，常常最後什麼都沒剔 |
| ~17 次小 `write_buffer` + `Maintain::Wait` | `frame.rs` 295、314、339、407、412、449、598、603、709、714、854、859、1172、1177、3398、3403、3408；`gpu_terrain.rs` 334–335；`state.rs` 8785–8793 `device.poll(Maintain::Wait)`；timestamp slot `Arc<Mutex<>>`（8710–8721、3884–3896） | driver overhead × 17；GPU 落後時 Wait 卡整 frame |
| 18 段複製的 GPU timestamp 寫入 | `frame.rs` 3517–3814 區間 18 個 `if self.gpu_timestamps_inside_passes { write_timestamp }` | ~80–100 行 encode loop 樣板 |
| `FrameResourcePool<()>` | `gpu_frame_resources.rs` ~270 行管 3 槽 ring，pool 存 `()`（`state.rs` 2396、3878–3881），完成用 mpsc + `on_submitted_work_done` | 一個 `frame_ring_index % 3` + in-flight flag 就夠 |
| 字型每 frame 展開成 primitive；HUD 每 frame clone String | menu `draw_text_with_font`（4108–4139）每字 7×5 bit → 35 quad；HUD `add_char_lines_with_source`（`state.rs` 9388–9408）同樣 bitmap 展線段；無 glyph cache。`TranslationCatalog::lookup` clone `String`（`localization.rs` 364–369），`frame.rs` 27 個 `translate` call；chat `format!(...).to_uppercase()`（3188）；物品數量 `format!`（1468、2085、2315）；`get_inventory_slots`（`state.rs` 8074）每 frame 配整份 slot `Vec`（`frame.rs` 1355），creative 再 `creative_visible_items()` | HUD 開著時每 frame 數千 vert + 數十 alloc |
| 遠端玩家線性 `find`；火把煙掃全部欄；F3 記憶體掃全部欄 | `state.rs` 6660–6668 `entities.iter_mut().find` per remote（有 `id_to_index`）；6729–6747 每 0.4 s 走 **每個** loaded chunk 的 torch index；8684–8707 每 debug frame 對全部欄 `Chunk::memory_usage`（`frame.rs` 2868） | O(entities × remotes)；O(loaded 欄) @2.5 Hz；O(loaded 欄) @F3 |

## 前置

09 波 04（同函式內的 `effective_scale` 死分支先刪）。Plan 06 先落地則 `EntityLosManager` 已在桌面側。

## 精確 acceptance

- [ ] `prepare_terrain_draw_plan` 只迭代 `visible_sections_scratch`；`lod_fills` 改 scratch 欄位；`query_radius` 收半徑不收 sqrt。
- [ ] `EntityLosManager`、`entity_los_worker`、`state.rs` 2473–2475 欄位、F3 culling 計數（`frame.rs` 2814）刪除；保留 `culling::is_los_blocked` 給 server。
- [ ] 一個 staging ring + 一次 mapped upload；present 路徑不 `Maintain::Wait`（GPU 落後時跳槽）；timestamp readback 不在 hot lock。
- [ ] `fn stamp(&self, pass, query)` helper（或 pass descriptor 的 `timestamp_writes`）；18 段複製消失；query index 0..13 順序不變。
- [ ] `gpu_frame_resources.rs` 收成 3 槽 in-flight flag；`GpuTimestampReadbackState::Unsupported` 刪。
- [ ] 5×7 glyph 一次光柵到小 atlas；menu 與 HUD 都畫 textured quad；`lookup` 回 `&str`；HUD／chat／物品數量共用一個 scratch `String`（F3 已有 `debug_str_scratch` 2708）。
- [ ] `get_inventory_slots` 寫進 scratch buffer；slot 矩形像素不變。
- [ ] 遠端玩家用 `id_to_index`；火把煙只走鏡頭鄰域 section；F3 記憶體改 `DEBUG_STATS_INTERVAL` 取樣。
- [ ] 桌面手動驗證：HUD／物品欄／選單字型外觀不變；F3 pass 時間仍顯示。

## 預計檔案與測試

- 改：`src/presentation/{frame,gpu_terrain}.rs`、`src/state.rs`、`src/gpu_frame_resources.rs`、`src/culling/{mod,visibility}.rs`、`src/menu.rs`、`src/localization.rs`、`src/chunk_manager.rs`（torch 鄰域查詢）
- 驗證：`cargo test --bin icraft`（`gpu_frame_resources`、timestamp state、inventory hit、culling 測試改寫）；`cargo test --lib culling::`；桌面 F3 對比 frame 時間

## 建議階段

1. draw plan 只走 visible set；`lod_fills`／sqrt（小、可量）。
2. 刪 `EntityLosManager`。
3. timestamp helper + `FrameResourcePool` 收縮。
4. 字型 atlas + `&str` lookup + scratch String + slot scratch。
5. staging ring / 去 `Maintain::Wait`。
6. 遠端玩家索引、火把煙、F3 取樣。

## 不在本計劃

- menu 畫面 widget 表（Plan 23）。
- mob／hand cuboid 去重（Plan 22）。
