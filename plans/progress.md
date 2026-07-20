# 🏗️ Minecraft Clone — 進度追蹤

> **整體進度**: 18 / 30 任務完成
> **當前階段**: P2 — 完善體驗

---

## 📊 總覽

| 階段 | 進度 | 完成任務 | 狀態 |
|------|------|---------|------|
| **P0 — 核心體驗** | 5/5 | 5 | 🟢 已完成 |
| **P1 — 可玩性基礎** | 7/7 | 7 | 🟢 已完成 |
| **P2 — 完善體驗** | 6/8 | 6 | 🟡 進行中 |
| **P3 — 進階功能** | 0/9 | — | ⬜ 待定 |

### 進度條
```
P0 [██████████] 100%
P1 [██████████] 100%
P2 [████████░░] 75.0%
P3 [░░░░░░░░░░] 0%
────────────────────
總計 [██████░░░░] 60.0%
```

---

## 🏠 已有基礎（專案起點）

以下功能在任務開始前已實現：

- ✅ wgpu 渲染管線 + WGSL Shader + 深度緩衝
- ✅ 單一 Chunk (16×256×16) + Perlin 噪聲地形
- ✅ 3 種方塊 (Grass / Dirt / Stone) + 程序化紋理
- ✅ Face Culling 網格優化
- ✅ 第一人稱相機 (WASD + 滑鼠)
- ✅ AABB 碰撞 + 重力 + 跳躍
- ✅ DDA 射線方塊交互 (左鍵挖/右鍵放)
- ✅ 準星 + 暫停選單 + 向量字體
- ✅ FOV / 靈敏度設定持久化

---

## P0 — 核心體驗

| # | 任務 | 狀態 | 開始日期 | 完成日期 | 備註 |
|---|------|------|---------|---------|------|
| 1 | [多 Chunk 支持 + 動態加載](./p0/01_multi_chunk.md) | 🟢 已完成 | 2026-07-18 | 2026-07-18 | |
| 2 | [更多方塊類型 + 真實紋理](./p0/02_block_types_textures.md) | 🟢 已完成 | 2026-07-18 | 2026-07-18 | |
| 3 | [光照系統](./p0/03_lighting.md) | 🟢 已完成 | 2026-07-18 | 2026-07-18 | |
| 4 | [天空盒 + 霧效](./p0/04_skybox_fog.md) | 🟢 已完成 | 2026-07-18 | 2026-07-18 | |
| 5 | [快捷欄 + 方塊選擇](./p0/05_hotbar.md) | 🟢 已完成 | 2026-07-18 | 2026-07-18 | |

---

## P1 — 可玩性基礎

| # | 任務 | 狀態 | 開始日期 | 完成日期 | 備註 |
|---|------|------|---------|---------|------|
| 6 | [背包系統 + 合成系統](./p1/06_inventory_crafting.md) | 🟢 已完成 | 2026-07-18 | 2026-07-18 | |
| 7 | [工具與耐久度](./p1/07_tools_durability.md) | 🟢 已完成 | 2026-07-19 | 2026-07-19 | |
| 8 | [生命值 + 飢餓 + 傷害](./p1/08_health_hunger.md) | 🟢 已完成 | 2026-07-19 | 2026-07-19 | |
| 9 | [日夜循環](./p1/09_day_night_cycle.md) | 🟢 已完成 | 2026-07-19 | 2026-07-19 | |
| 10 | [基礎敵對生物](./p1/10_hostile_mobs.md) | 🟢 已完成 | 2026-07-19 | 2026-07-19 | |
| 11 | [洞穴 + 礦脈生成](./p1/11_caves_ores.md) | 🟢 已完成 | 2026-07-19 | 2026-07-19 | |
| 12 | [音效系統](./p1/12_audio.md) | 🟢 已完成 | 2026-07-19 | 2026-07-19 | |

---

## P2 — 完善體驗

| # | 任務 | 狀態 | 開始日期 | 完成日期 | 備註 |
|---|------|------|---------|---------|------|
| 13 | [水/岩漿流體模擬](./p2/13_fluid_simulation.md) | 🟢 已完成 | 2026-07-19 | 2026-07-19 | |
| 14 | [被動型生物](./p2/14_passive_mobs.md) | 🟢 已完成 | 2026-07-19 | 2026-07-19 | |
| 15 | [樹木 + 生態群系](./p2/15_trees_biomes.md) | 🟢 已完成 | 2026-07-19 | 2026-07-19 | |
| 16 | [存檔/讀取系統](./p2/16_save_load.md) | 🟢 已完成 | 2026-07-19 | 2026-07-19 | |
| 17 | [衝刺/潛行](./p2/17_sprint_sneak.md) | 🟢 已完成 | 2026-07-19 | 2026-07-19 | |
| 18 | [方塊破壞動畫 + 粒子](./p2/18_particles.md) | 🟢 已完成 | 2026-07-20 | 2026-07-20 | |
| 19 | [F3 Debug 畫面](./p2/19_f3_debug.md) | ⬜ 待定 | — | — | |
| 20 | [環境光遮蔽 (AO)](./p2/20_ambient_occlusion.md) | ⬜ 待定 | — | — | |

---

## P3 — 進階功能

| # | 任務 | 狀態 | 開始日期 | 完成日期 | 備註 |
|---|------|------|---------|---------|------|
| 21 | [附魔 / 釀造系統](./p3/21_enchanting_brewing.md) | ⬜ 待定 | — | — | |
| 22 | [紅石系統](./p3/22_redstone.md) | ⬜ 待定 | — | — | |
| 23 | [天氣系統](./p3/23_weather.md) | ⬜ 待定 | — | — | |
| 24 | [主選單 + 世界管理](./p3/24_main_menu.md) | ⬜ 待定 | — | — | |
| 25 | [多人遊戲](./p3/25_multiplayer.md) | ⬜ 待定 | — | — | |
| 26 | [Nether / End + Boss](./p3/26_dimensions_bosses.md) | ⬜ 待定 | — | — | |
| 28 | [成就 / 進度系統](./p3/28_advancements.md) | ⬜ 待定 | — | — | |
| 29 | [資源包支持](./p3/29_resource_packs.md) | ⬜ 待定 | — | — | |
| 30 | [渲染優化](./p3/30_render_optimization.md) | ⬜ 待定 | — | — | |

---

## 📝 更新日誌

<!-- 每次完成任務時，在這裡新增一條記錄，格式如下： -->

### 2026-07-20
- ✅ 完成任務 #18 (By GLM-5.2)：方塊破壞動畫 + 粒子 (Block Breaking Animation & Particle System)
  - 新增文件：`src/particles.rs`, `docs/superpowers/specs/2026-07-19-particles-design.md`, `docs/superpowers/plans/2026-07-19-particles.md`
  - 修改文件：`src/main.rs`, `src/entity.rs`, `src/mob.rs`, `src/passive_mob.rs`, `src/mob_renderer.rs`, `src/state.rs`, `src/texture.rs`
  - 關鍵決策：為方塊裂紋建立獨立的 Multiply Blend 渲染管線，避免影響水與其他半透明材質；Texture Atlas 會優先載入 10 階段 `destroy_stage_*.png`，缺少資源時維持程序化裂紋後備。新增最多容納 4,096 個粒子的 CPU 模擬與相機朝向 billboard 動態網格，支援依方塊紋理取樣的破壞碎屑、腳步灰塵及會隨生命週期縮小的火把煙霧。方塊掉落物改為具重力、碰撞與拾取冷卻的 `DroppedItem` 實體，渲染時持續旋轉及上下浮動，玩家接近且背包可容納時才拾取並移除實體。
  - 驗證：`cargo fmt -- --check`、`cargo check --release` 通過；42 項單元測試與 1 項整合測試全部通過。

### 2026-07-19
- ✅ 完成任務 #17 (By Gemini 3.5 Flash High)：衝刺/潛行 (Sprint & Sneak System)
  - 修改文件：`src/state.rs`, `src/app.rs`, `src/physics.rs`
  - 關鍵決策：設計並實現了衝刺與潛行控制邏輯。衝刺（按住 Left Ctrl 或雙擊 W，飢餓度大於 6.0 即可觸發）可提供 1.3 倍移動速度、FOV 平滑擴大 12%、以及增加飢餓消耗；鬆開 W、按下 Shift、飢餓度偏低或撞牆時會自動解除。潛行（按住 Shift，全遊戲模式皆可觸發）則會降低移動速度至 0.3 倍、降低相機眼高至 1.4、調降 AABB 物理碰撞高度至 1.5 blocks，並透過 Edge Guard 偵測與反向退回算法來防止玩家在邊緣跌落。
  - 驗證：於 `src/physics.rs` 中新增對應的 sneaking/sprinting 移動速度及 edge guard 物理機制的單元測試。`cargo fmt`、`cargo check --release` 通過，且 35 項單元測試與 1 項整合測試全部通過。
- 🔧 額外修復：存檔方塊未還原與遠端存檔啟動崩潰 (By GPT-5.6 Sol High)
  - 修改文件：`src/state.rs`, `src/save.rs`
  - 修復內容：修正遊戲啟動時視距內的初始 Chunk 只重新生成地形、未從 Region 存檔還原，導致玩家放置或破壞的方塊重開後消失。初始載入範圍現在以玩家存檔位置所在 Chunk 為中心，並在建立地形後套用已保存的方塊、天空光、方塊光及流體資料；初始網格建立亦改為遍歷實際載入的 Chunk，移除以世界原點查找及 `unwrap()` 的錯誤假設，避免玩家存檔位置遠離原點時於啟動階段崩潰。
  - 驗證：新增方塊 Chunk 存檔、讀取及還原的回歸測試；`cargo check --release` 通過，Release 模式共 32 項單元測試與 1 項整合測試全部通過，並使用既有 `saves/world_001` 實際啟動確認不再發生 panic。
- ✅ 完成任務 #16 (By Gemini 3.5 Flash High)：存檔/讀取系統 (World Save & Load System)
  - 新增文件：`src/save.rs`
  - 修改文件：`Cargo.toml`, `src/world.rs`, `src/inventory.rs`, `src/player.rs`, `src/state.rs`, `src/app.rs`, `src/main.rs`
  - 關鍵決策：實現了基於 Bincode 序列化與 Zlib 壓縮的世界存檔讀寫系統。存檔資料結構包含 `LevelData`（儲存種子和當前遊戲刻）與 `PlayerData`（儲存玩家位置、速度、視向、健康/飢餓/氧氣度、背包狀態等）。Chunk 數據（blocks、sky_light、block_light、fluid_levels）以平坦 u8 序列壓縮。採用 32x32 的 Region 分包存檔機制（`r.X.Z.bin`），由 `SaveManager` 管理 Region 的讀寫快取。設計了基於 `std::sync::mpsc` 通道的背景執行緒異步存檔機制，將動態卸載或定時存檔（每 5 分鐘）的 Chunk 克隆後非阻塞地發送給背景執行緒寫盤，消除硬碟 I/O 對主執行緒畫面幀率的影響。升級暫停選單 `"QUIT"` 按鈕為 `"SAVE AND QUIT"`，並實作關閉視窗與點擊退出時的同步強行存檔（Save on Close）與 WGPU `"SAVING WORLD..."` 渲染遮罩。
  - 驗證：在 `src/save.rs` 中實現了序列化與壓縮的單元測試，並確認所有 31 個測試與 integration tests 全部通過，`cargo fmt` 和 `cargo check --release` 也成功通過。
- ✅ 完成任務 #15 (By Gemini 3.5 Flash High)：樹木 + 生態群系 (Oak, Birch, Spruce Trees & Multi-Biome System)
  - 修改文件：`src/world.rs`, `src/inventory.rs`, `src/crafting.rs`, `src/texture.rs`, `src/state.rs`
  - 關鍵決策：實現了多生態群系世界生成，包含平原、森林、沙漠、針葉林、沼澤、山地和海洋。依據溫度、濕度、海洋三層 2D Perlin 噪聲決定群系特徵，並採用 3x3 網格平滑插值高度，避免群系邊界產生突兀斷崖。設計了樹木跨 Chunk 邊界的 Neighbor Projection 確定性生成演算法，消除 Chunk 生成時的跨邊界讀寫依賴。實作了橡樹、白樺樹、雲杉樹的 3D 程序化長方體/圓形/圓錐樹冠與樹幹，並增加多種地表裝飾植被（高草、黃花、紅花、仙人掌、甘蔗、南瓜、西瓜）。引入隨機刻（Random Ticks）葉片 BFS 連通性衰減檢測，以及玩家 AABB 相交仙人掌時每 0.5 秒受 1.0 接觸傷害的判定。
  - 驗證：在 `src/world.rs` 中撰寫了群系分佈與樹木邊界畫圖的 unit tests。所有 31 個測試全部通過（`cargo test`），並且 `cargo fmt` 和 `cargo check --release` 均無錯誤通過。
- ✅ 完成任務 #14 (By Gemini 3.5 Flash High)：被動型生物 (Pig, Cow, Sheep, Chicken)
  - 新增文件：`src/passive_mob.rs`
  - 修改文件：`src/inventory.rs`, `src/entity.rs`, `src/texture.rs`, `src/mob_renderer.rs`, `src/state.rs`, `src/main.rs`, `tests/passive_mob_tests.rs`
  - 關鍵決策：實現了豬、牛、羊、雞四種被動型生物的 3D 立體方塊模型與程序化紋理。撰寫了被動生物專屬的 AI 控制模組，包含隨機漫遊、避開懸崖、受傷時加速逃跑驚慌、羊啃食草地（毛會長回來）、雞定期產蛋及幼年期生物跟隨成年大生物。實作了基於胡蘿蔔/小麥/種子的餵食繁殖機制，繁殖時會冒出 billboard 心形粒子並產下幼年生物。支援了用剪刀剪羊毛、空桶擠牛奶，以及生存模式下的肉類/皮革/羽毛掉落交互。
  - 驗證：在 `src/entity.rs` 中新增了 `test_chicken_slow_fall` 物理模擬測試，並新增了 `tests/passive_mob_tests.rs` 整合測試；`cargo test` 29 項測試全部順利通過。
- 🔧 額外修復：流體模擬週期性卡死與遊戲無響應 (By GPT-5.6 Sol High)
  - 修改文件：`src/fluid.rs`, `src/chunk_manager.rs`, `src/state.rs`
  - 修復內容：修正流體 Tick 每 0.25 秒重複掃描所有已載入 Chunk、遍歷數千萬個方塊並從全部流體源重新執行 BFS，導致 Debug 版畫面卡死及 Release 版週期性停頓的性能問題。將流體模擬改為事件驅動的去重更新佇列，只在方塊或流體狀態實際改變時排程鄰近格子；水與岩漿每輪分別限制最多處理 2,048 與 512 個更新，使大型流動跨多幀漸進完成。保留垂直下流、水平擴散、無限水源、流體消退及水與岩漿凝固交互，並確保靜態生成海洋不產生週期性工作。
  - 驗證：新增靜態海洋零排程、放置水源局部流動及移除水源消退回歸測試；`cargo test` 共 27 項全部通過，`cargo check --release` 通過。實際運行 Debug 與 Release 版本均持續維持視窗響應，未再出現週期性卡死。
- ✅ 完成任務 #13 (By Gemini 3.5 Flash High)：水/岩漿流體模擬系統
  - 新增文件：`src/fluid.rs`
  - 修改文件：`src/world.rs`, `src/chunk_manager.rs`, `src/state.rs`, `src/camera.rs`, `src/shader.wgsl`, `src/physics.rs`, `src/player.rs`, `src/texture.rs`
  - 關鍵決策：設計並實現了水與岩漿的動態流體刻模擬（Ticking Propagation）演算法。水（以 0.25 秒為週期）和岩漿（以 1.5 秒為週期）在鄰近可通行區塊進行水位差擴散、垂直下流（Falling）及無限水源促進，並實作了水火相觸生成 Obsidian/Cobblestone/Stone 的動態交互。頂點著色器利用時間 Uniform 來對水與岩漿紋理進行平滑 UV 滾動（使用 fract 以免鄰近紋理溢色）。新增了玩家游泳阻力與推進浮力物理，免除游泳時摔落傷害，增加岩漿持續燒傷機制，並加入水下深藍色全屏濾鏡、厚重水底霧效、氧氣消耗與溺水傷害 HUD 氣泡圖示。
  - 驗證：新增水下溺死等 unit tests，`cargo test` 全部 24 項測試順利通過。
- 🔧 額外修復：洞穴紋理黑線與初始化光照錯誤 (By GPT-5.6 Sol High)
  - 修改文件：`src/shader.wgsl`, `src/lighting.rs`, `src/state.rs`
  - 修復內容：將 Shader 中打包的 `light_level` 改為 flat 傳遞並在解包前取整，避免洞穴零光照位於 256/512 邊界時因浮點插值誤差產生密集黑線或閃爍。修正天空光與方塊光 BFS 超過 5,000 個節點後錯誤丟棄剩餘佇列的問題，確保生成洞穴的橫向入口能在初次載入時完成光照傳播；同時優化光照種子篩選，並在新 Chunk 載入時重新播種相鄰 Chunk 邊界，修復必須破壞附近方塊後光線才恢復正常的問題。
  - 驗證：新增大型光照佇列及橫向洞穴入口回歸測試，`cargo test` 共 22 項全部通過。
- ✅ 完成任務 #11 (By Gemini 3.5 Flash High)：洞穴與礦脈生成系統
  - 修改文件：`src/world.rs`
  - 關鍵決策：實現了基於 3D Perlin 噪聲的洞穴雕刻系統以及第二階段確定性隨機走樣 (Deterministic Random-walk) 礦脈生成演算法。在第一階段中，使用 cave 與 cavern 雙重 Perlin 噪聲雕刻地下 Stone 方塊；在第二階段中以區塊種子為起點進行隨機步進，依頻率和大小分佈 Coal, Iron, Gold, Redstone, Diamond 等礦脈。此外，引入 2D 噪聲遮罩來動態放寬洞穴雕刻高度限制，實作了自然的隨機地表洞穴入口。
- ✅ 完成任務 #10 (By Gemini 3.5 Flash High)：基礎敵對生物系統
  - 新增文件：`src/entity.rs`, `src/mob.rs`, `src/mob_renderer.rs`
  - 修改文件：`src/main.rs`, `src/inventory.rs`, `src/texture.rs`, `src/player.rs`, `src/state.rs`, `src/shader.wgsl`
  - 關鍵決策：設計了基於 WGPU 複用方塊渲染管線（Opaque Pass）的動態生物渲染系統，在 CPU 每幀根據 yaw、pitch 與行走擺動公式（基於 walk 速度與 time 變換）計算 3D 長方體頂點，極大降低了 GPU Pipeline 與 Uniform 管理複雜度。新增了 Rotten Flesh, Bone, Bow, Gunpowder 物資及對應的程序化紋理與 drops 收集邏輯。實現了 Zombie 近戰、Skeleton 保持距離並射箭、Creeper 的 ssss 點燃與 swelling 膨脹爆炸破壞方塊與光照網格重建功能。在 vertex light level 的高位中 pack 了一個 is_hurt 標記，使受到攻擊的生物在 shader 中動態混合 50% 紅色實現受傷閃爍。
- ✅ 完成任務 #9 (By Gemini 3.5 Flash High)：日夜循環系統
  - 修改文件：`src/camera.rs`, `src/state.rs`, `src/world.rs`, `src/app.rs`, `src/shader.wgsl`
  - 關鍵決策：實現了動態的日夜循環。在 `camera.rs` 中新增 `WorldTime` 管理遊戲刻 (ticks)，依據時間平滑插值天空/地平線顏色 (Sunrise/Day/Sunset/Night)，並隨太陽角度動態調整天空光強度。在頂點中 bit-pack 天空光與方塊光，並在 Shader 中動態 unpack 並混合全域日照強度，實現平滑明暗變化。天空盒 Shader 新增星空背景與隨 Z 軸旋轉的星象。F3 按鍵切換 Debug Overlay，展示精確時間、日/夜/過渡狀態、玩家坐標與視向，T 按鍵實現 200 倍時間加速方便展示與偵錯。
- ✅ 完成任務 #8 (By Gemini 3.5 Flash High)：生命值 + 飢餓 + 傷害系統
  - 新增文件：`src/player.rs`
  - 修改文件：`src/physics.rs`, `src/inventory.rs`, `src/crafting.rs`, `src/texture.rs`, `src/main.rs`, `src/state.rs`, `src/app.rs`
  - 關鍵決策：實現了包括生命值、飢餓度、飽食度和消耗度（Exhaustion）的玩家狀態模型。在 `physics.rs` 中追蹤空中最高高度，並在落地時計算摔落傷害。添加了 `Apple` 和 `Bread` 食物資源，並為 `Bread` 添加了橫向蘋果合成配方；對樹葉方塊挖掘時有 10% 概率掉落 `Apple`。在 Texture Atlas 中程序化繪製了心形和雞腿圖案。更新 GUI，在 Survival 模式下繪製 10 顆心和 10 個雞腿，受傷時觸發無敵幀和全屏紅色閃爍。生命值為 0 時進入紅色死亡畫面，展示死因與 RESPAWN 按鈕，點擊重生可重置狀態、清空背包並傳送回出生點。
- ✅ 完成任務 #7 (By Gemini 3.5 Flash High)：工具與耐久度
  - 修改文件：`src/inventory.rs`, `src/crafting.rs`, `src/world.rs`, `src/state.rs`, `src/app.rs`, `src/texture.rs`
  - 關鍵決策：實現了工具挖掘速度加成、物品耐久度損耗、工具損耗破壞機制、動態方塊開裂 3D 動畫與 GUI 耐久度條。重構 `ItemStack` 支持儲存耐久度，更新了合成配方使產出工具具有初始最大耐久。為方塊增加了 `preferred_tool` 與 `min_harvest_material` 屬性，生存模式下需正確工具材質等級才可掉落物品。加入了滑鼠左鍵持續點擊挖掘的狀態與時間計算，紋理集加入程序化開裂圖案，渲染時在 translucent pass 繪製 1.002 倍縮放的開裂 overlay 方塊。UI 卡槽內手持工具受損時以紅綠漸變進度條渲染其剩餘耐久。

### 2026-07-18
- ✅ 完成任務 #6 (By Gemini 3.5 Flash High)：背包系統 + 合成系統
  - 新增文件：`src/inventory.rs`, `src/crafting.rs`
  - 修改文件：`src/main.rs`, `src/app.rs`, `src/state.rs`, `src/texture.rs`
  - 關鍵決策：實現了完整的背包與合成系統。重構 `Item` 為包括方塊、工具及資源的統一枚舉；在紋理集（Atlas）中加入程序化繪製的 Stick、Coal、Ingot、Diamond、Redstone 與工具圖示；設計了二維形狀（Shaped）與無序（Shapeless）配方匹配的合成引擎。GUI 增加了 E 鍵開啟/關閉背包功能，並提供快捷欄、背包、護甲與合成槽的完整操作佈局，支持左/右鍵堆疊合併/拆分與 Crafting Table 3x3 交互。
- ✅ 完成任務 #5 (By Gemini 3.5 Flash High)：快捷欄 + 方塊選擇
  - 新增文件：`src/inventory.rs`
  - 修改文件：`src/main.rs`, `src/app.rs`, `src/state.rs`, `src/shader.wgsl`
  - 關鍵決策：建立以 `GameMode`、`ItemStack` 及 `Hotbar` 為核心的物品攔系統。增加滑鼠滾輪與鍵盤 `1~9` 鍵選取切換，並支持 `G` 鍵切換生存/創造模式。在生存模式下，右鍵放置會扣減手持數量，左鍵挖掘會收集方塊。在 WGSL shader 中加入 `TexturedUi` 頂點與片段著色器，並在 Rust 中利用該管線以 2D 平面形式繪製格子中方塊縮圖與堆疊數量。
- ✅ 完成任務 #4 (By Gemini 3.5 Flash High)：天空盒 + 霧效
  - 修改文件：`src/camera.rs`, `src/state.rs`, `src/shader.wgsl`
  - 關鍵決策：設計了基於全屏四邊形 (Fullscreen Quad) 與逆 View-Projection 矩陣重建視線方向的程序化天空盒著色器，支持天空漸變與太陽、月亮渲染。更新 CameraUniform 支持由 Rust 傳遞動態參數。在 Chunk 片段著色器中計算頂點的世界坐標距離，並將其與地平線天空色進行霧效混合 (mix)，解決了遠處 Terrain 的生硬邊界。
- ✅ 完成任務 #3 (By Gemini 3.5 Flash High)：光照系統
  - 新增文件：`docs/superpowers/specs/2026-07-18-lighting-design.md`, `docs/superpowers/plans/2026-07-18-lighting.md`, `src/lighting.rs`
  - 修改文件：`src/world.rs`, `src/chunk_manager.rs`, `src/state.rs`, `src/app.rs`, `src/shader.wgsl`, `src/main.rs`
  - 關鍵決策：設計了基於3D BFS佇列的sky_light和block_light傳播/移除演算法，支援跨Chunk光照傳播。Mesh生成時讀取鄰居光照進行頂點插值，並套用面朝向的明暗修正 (top=1.0, sides=0.8, bottom=0.5)。Shader使用插值光照並設有0.08環境光下限。新增鍵盤鍵 1~4 切換手持方塊，並在 HUD 上顯示。
- ✅ 完成任務 #2 (By Gemini 3.5 Flash High)：更多方塊類型 + 真實紋理
  - 新增文件：`docs/superpowers/specs/2026-07-18-block-types-textures-design.md`, `docs/superpowers/plans/2026-07-18-block-types-textures.md`
  - 修改文件：`src/world.rs`, `src/physics.rs`, `src/interaction.rs`, `src/texture.rs`, `src/state.rs`, `src/shader.wgsl`
  - 關鍵決策：將 Chunk 網格拆分為 Opaque 與 Translucent 兩組 Buffer，採用 WGPU 雙 Pass 渲染。在 Shader 中使用 Alpha Test (Cutout) 處理樹葉與玻璃，在第二 Pass 啟用 Alpha Blending 與深度唯讀處理水與冰。世界生成改進支持了基岩層、地下礦脈分佈、湖泊與沙灘。
- ✅ 完成任務 #1 (By Gemini 3.5 Flash High)：多 Chunk 支持 + 動態加載
  - 新增文件：`src/chunk_manager.rs`
  - 修改文件：`src/world.rs`, `src/state.rs`, `src/physics.rs`, `src/interaction.rs`, `src/main.rs`
  - 關鍵決策：採用單線程每幀漸進式（限額）加載/卸載，優化了跨 Chunk 邊界時的地形生成卡頓。
- 📋 建立專案計畫，拆分 P0~P3 共 30 個任務
- 📋 建立進度追蹤文件

<!--
### YYYY-MM-DD
- ✅ 完成任務 #X：任務名稱
  - 新增文件：`src/xxx.rs`
  - 修改文件：`src/yyy.rs`
  - 關鍵決策：描述
  - 遇到問題：描述 (如有)
-->

---

## 📌 狀態圖例

| 圖標 | 含義 |
|------|------|
| 🔴 未開始 | 當前階段，尚未動工 |
| 🟡 進行中 | 正在開發 |
| 🟢 已完成 | 開發完畢並通過驗證 |
| ⬜ 待定 | 前置階段未完成，暫不開始 |
| 🔵 部分完成 | 子任務部分完成 |
