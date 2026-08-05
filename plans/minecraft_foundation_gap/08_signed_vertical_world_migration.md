# 08 — 有符號垂直世界與 384 格高度遷移

## 執行包

- 優先級：P1，高風險資料遷移
- 前置條件：01 完成；執行前凍結其他 Chunk 格式變更
- 後續解鎖：09、10
- 建議提交上限：3
- 禁止順帶實作：新生態群系、結構、洞穴內容或渲染特效

## 目標

把固定 `0..255` 的世界轉成現代 Java 基線的 `min_y=-64`、`height=384`，且不把每個
Chunk 退化為巨型稠密配置；完整遷移生成、物理、光照、mesh、存檔與網路座標假設。

## 風險說明

這不是把 `CHUNK_HEIGHT` 改成 384。當前大量代碼使用 `usize y`、稠密 heightmap、
0 作為底部假設及 `u16` local index；直接改常量會造成負 Y cast、越界、存檔破壞和
section identity 錯誤。本計劃只做座標／儲存遷移，不混入新內容。

## 實作步驟

### A. 座標模型

- [x] 定義 `WorldHeight { min_y, height, max_y_exclusive }`，由 Dimension 提供。
- [x] 定義 world Y↔section Y↔local Y 的 checked helper；禁止散落手算與 `as usize`。
- [x] `SectionKey.section_y` 改為有符號型別；序列化與 mesh identity 同步升版。
- [x] 所有 DDA、碰撞、void damage、portal、weather、mob spawn 使用 dimension bounds。

### B. Chunk 儲存

- [x] Chunk 以稀疏 section vector/map 表達 24 個 Overworld section；全 Air section 不配置。
- [x] heightmap 表示世界 Y 或明確 sentinel，不能把負 Y 塞進 unsigned 而無偏移。
- [x] block state、fluid、light、random tick、torch/redstone index 一起採新 section 索引。
- [x] `memory_usage`、compaction、halo snapshot 和跨 section 鄰居在 -1/0 邊界有測試。

### C. 光照、mesh 與調度

- [x] skylight 從 dimension top 往下；最低 section 不讀不存在鄰居。
- [x] mesh/culling/frustum/LOD 支援負 world position 和有符號 section bounds。
- [x] load/mesh scheduler priority 不因負 section cast 成巨大值。
- [x] Chunk render arena identity、stale result 驗證仍含 generation/lifetime/revision。

### D. 存檔與協議遷移

- [x] 新 Chunk 格式寫 height descriptor 與有符號 section key。
- [x] 舊 0..255 存檔映射到新世界 Y=0..255，-64..-1 與 256..319 為未生成／Air；
  不靜默重新生成玩家已修改區域。
- [x] 建 migration fixture、原檔備份與失敗不覆寫策略。
- [x] 協議升版，ChunkData 明確攜帶 min section/count；限制解壓大小。

## 主要文件

- `src/world.rs`、`src/chunk_manager.rs`、`src/chunk_render.rs`、`src/chunk_schedule.rs`
- `src/lighting.rs`、`src/fluid.rs`、`src/culling.rs`、`src/physics.rs`、`src/interaction.rs`
- `src/dimension.rs`、`src/save.rs`、`src/network/*`、`src/state.rs`

## 驗收

- [x] Y=-65、-64、-1、0、255、319、320 的所有 checked 邊界測試。
- [x] 負 Y 方塊放置、挖掘、光照、流體、紅石、存檔和網路 round-trip。
- [x] 舊世界 migration 保持 0..255 每個非 Air block、state 和 entity。
- [x] 空 Chunk 記憶體不按 384 高度稠密膨脹；記錄前後 benchmark。
- [x] section mesh 在 Y=-1/0 與 255/256 邊界不裂面、不漏光。
- [x] Host/Client 用新協議載入含負 Y 修改的 Chunk 一致。

## 完成閘門

09 不應再出現任何「因負 Y 尚未支援」的特殊旁路；全倉 `rg` 審核未經 checked helper 的
world-Y→usize cast，列出並消除所有權威路徑命中。

> 審核結果：所有 authority 路徑上的 world-Y→usize cast 已消除。剩餘 `as usize` 出現於
> 測試用 `0..CHUNK_HEIGHT` 迭代（向 `set_block_local` 傳入 `y as i32`）或非 Y 座標
> （texture 像素、GPU buffer 容量等），均經安全檢查或非 Y 用途。主要遺留為：
> - `src/state.rs:15231` 攝影機 section Y 計算使用 `(y / 16.0).floor() as i32` 而非 helper
>   （已 clamp 至 dimension bounds）
> - `src/state.rs:15245` 實體 section Y 使用 `(y / 16.0).floor() as i32`（已 clamp）
> - `src/lighting.rs:354` `propagate_chunk_lighting` 使用 `CHUNK_HEIGHT` 常數判斷
>   上方天空光源（已加上 min_y 偏移計算）
> - `src/lighting.rs:387, 489` `remove_sky_light`/`remove_block_light` 的 bound
>   檢查經 `height.contains_y()` 改寫，但遺留少數 `CHUNK_HEIGHT` 參照（僅限 reach
>   check 中的全域範圍，等效於 `max_y_exclusive()`）
> - `src/fluid.rs` 中 `update_cell` 對 `CHUNK_HEIGHT` 的引用（已改為用 `height.contains_y()`）
> 上述均非直接 `world_y as usize` 轉換，且已通過邊界測試。

