# 🏗️ iCraft — 進度追蹤

> **整體進度**: 28 / 30 任務完成
> **當前階段**: P3 — 進階功能

---

## 📊 總覽

| 階段 | 進度 | 完成任務 | 狀態 |
|------|------|---------|------|
| **P0 — 核心體驗** | 5/5 | 5 | 🟢 已完成 |
| **P1 — 可玩性基礎** | 7/7 | 7 | 🟢 已完成 |
| **P2 — 完善體驗** | 8/8 | 8 | 🟢 已完成 |
| **P3 — 進階功能** | 8/9 | 8 | 🟡 進行中 |

### 進度條
```
P0 [██████████] 100%
P1 [██████████] 100%
P2 [██████████] 100%
P3 [█████████░] 88.9%
────────────────────
總計 [█████████░] 93.3%
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
| 19 | [F3 Debug 畫面](./p2/19_f3_debug.md) | 🟢 已完成 | 2026-07-20 | 2026-07-20 | |
| 20 | [環境光遮蔽 (AO)](./p2/20_ambient_occlusion.md) | 🟢 已完成 | 2026-07-20 | 2026-07-20 | |

---

## P3 — 進階功能

| # | 任務 | 狀態 | 開始日期 | 完成日期 | 備註 |
|---|------|------|---------|---------|------|
| 21 | [附魔 / 釀造系統](./p3/21_enchanting_brewing.md) | 🟢 已完成 | 2026-07-20 | 2026-07-20 | |
| 22 | [紅石系統](./p3/22_redstone.md) | 🟢 已完成 | 2026-07-20 | 2026-07-20 | |
| 23 | [天氣系統](./p3/23_weather.md) | 🟢 已完成 | 2026-07-20 | 2026-07-20 | |
| 24 | [主選單 + 世界管理](./p3/24_main_menu.md) | 🟢 已完成 | 2026-07-20 | 2026-07-20 | |
| 25 | [多人遊戲](./p3/25_multiplayer.md) | 🟢 已完成 | 2026-07-22 | 2026-07-22 | 子任務 6/6 完成 |
| 26 | [Nether / End + Boss](./p3/26_dimensions_bosses.md) | 🟢 已完成 | 2026-07-21 | 2026-07-21 | |
| 28 | [成就 / 進度系統](./p3/28_advancements.md) | 🟢 已完成 | 2026-07-21 | 2026-07-21 | |
| 29 | [資源包支持](./p3/29_resource_packs.md) | ❌ 廢案(不再實現) | — | — | |
| 30 | [渲染優化](./p3/30_render_optimization.md) | 🟢 已完成 | 2026-07-23 | 2026-07-23 | 60+ FPS 留待實機量測 |

---

## 📝 更新日誌

<!-- 每次完成任務時，在這裡新增一條記錄，格式如下： -->

### 2026-07-24
- ✅ 新增可調 Weather 音量 (By rain-volume sub-agent, reviewed by Codex)
  - 修改文件：`src/audio.rs`, `src/menu.rs`, `src/state.rs`, `ARCHITECTURE.md`, `plans/implementation/07_weather_volume.md`, `plans/progress.md`, `track.md`
  - 關鍵決策：`GameSettings` 新增向後相容的 `weather_volume`，舊設定缺鍵使用較安靜的 0.4，載入／保存會 clamp 超界值並安全處理 NaN。AudioManager 把已合成的 `Master×Sound` base 再按類別套用 Weather，故 Rain/Thunder 為 `Master×Sound×Weather`，其他 SFX 不受影響；active loop 保存 SoundId，Master 或 Weather 改動時立即刷新正在播放的雨聲。主選單 Options 與 pause menu 都加入 Weather 控制，State 只從 `self.settings` 同步 mixer，不再由 mixer 反推 master。
  - 驗證：舊檔預設、超界/NaN、save/load roundtrip、Rain/Thunder 與普通 SFX gain、idle Sink active Rain loop 0→恢復、主選單行列及 pause Weather/Quit hit region 測試通過；`cargo fmt -- --check`、`cargo check --release`、`cargo test --release` 通過，共 226 項單元測試與 1 項整合測試。
  - 備註：雨天主／暫停選單調整、聽感及重啟持久化的實際視窗操作保留為人工驗收。
- ✅ 修復 Survival 怪物攻擊 (By combat sub-agent, reviewed by Codex)
  - 修改文件：`src/app.rs`, `src/state.rs`, `src/mob.rs`, `ARCHITECTURE.md`, `plans/implementation/06_survival_combat.md`, `plans/progress.md`, `track.md`
  - 關鍵決策：所有左鍵 press 統一先走 authoritative melee；只選 4 格內最近、仍存活且具生命值的合法 combat entity，因此 RemotePlayer、掉落物、粒子與非戰鬥投射物不會吞點擊。Survival miss 才保留 held-mining latch，命中或 invulnerability-window 攔截會消耗 press 並阻止挖到怪物身後方塊；Creative miss 才走瞬間破壞。傷害、擊退、Strength、Fire Aspect、Looting、掉落、XP 與工具耐久沿用既有路徑。一般活體恰好 0 HP 現在會清除，非活體與 boss-owned 實體仍由各自生命週期管理。joined client 因沒有權威 mob replication，不建立會分歧的本地傷害。
  - Review 修正：初版雖攔截 press，App 仍預先鎖住 `left_mouse_pressed=true`，下一幀可能挖身後方塊；改由 `handle_primary_press()` 回傳是否保留 held mining，並在所有 UI gate 前處理 Left release。
  - 驗證：Survival/Creative hit-miss-latch 決策、最近合法 target、死目標跳過、invulnerability、致死 damage/knockback/fire 及 0 HP living/nonliving cleanup 測試通過；`cargo fmt -- --check`、`cargo check --release`、`cargo test --release` 通過，共 219 項單元測試與 1 項整合測試。
  - 備註：空手／武器攻擊敵對與被動怪物的實際視窗操作保留為人工驗收。
- ✅ 修復火把模型 (By torch-model sub-agent, reviewed by Codex)：加入正確 3D 地面火把
  - 修改文件：`src/world.rs`, `ARCHITECTURE.md`, `plans/implementation/05_torch_model.md`, `plans/progress.md`, `track.md`
  - 關鍵決策：在非完整 cube 的逐方塊 mesh 路徑加入專用 `append_torch_mesh`，生成置中的 X/Z `7/16..9/16`、Y `0..10/16` 六面 cuboid。沿用既有 outward CW face order，side/top/bottom 分別取 atlas `(4,2)` 內 half-texel inset 子區域；所有頂點以來源格 sky/block light、AO 1.0 送入 shader，不加入一般立方體面陰影。Cutout、非 solid、14 級光源、地面支撐及支撐移除清光保持不變；因缺少可存檔／同步的通用 facing state，本次不虛構壁掛火把。
  - 驗證：精確 24 vertices／36 indices 與 2×2×10 bounds、六面 winding、三類 UV、AO/packed light、屬性、支撐移除與光照清理測試通過；`cargo fmt -- --check`、`cargo check --release`、`cargo test --release` 通過，共 214 項單元測試與 1 項整合測試。
  - 備註：各角度實際查看模型及透明邊緣的視窗操作保留為人工驗收。
- ✅ 修復方塊放置碰撞 (By placement sub-agent, reviewed by Codex)：禁止把 solid 方塊放進玩家
  - 修改文件：`src/physics.rs`, `src/state.rs`, `src/network/server.rs`, `ARCHITECTURE.md`, `plans/implementation/04_player_placement_collision.md`, `plans/progress.md`, `track.md`
  - 關鍵決策：抽出統一玩家／單位方塊 AABB 與純放置 policy，只拒絕三軸都有正體積重疊的 solid 方塊，因此面／邊／角接觸及 Torch 等 non-solid 方塊仍合法。本地與 joined client 都在放置副作用或送出 request 前預檢；Host 保留 server 驗證過的 session ID，以本地當前 AABB 及所有遠端玩家 `snapshots.back()` 的最新權威位置做最終裁決。Host request 與 client 收到的權威 block change 使用不同事件類型，client 不會以延遲 render pose 重驗 Host 結果。
  - 驗證：AABB 座標、重疊／接觸邊界、non-solid、最新權威快照、未知姿勢、Host/Client 事件分流及 server authenticated ID 端到端測試通過；`cargo fmt -- --check`、`cargo check --release`、`cargo test --release` 通過，共 210 項單元測試與 1 項整合測試。
  - 備註：單人腳下／頭部及 Host + Join 互相放置的實際視窗操作保留為人工驗收。
- ✅ 新增額外功能 (By Codex)：Minecraft 式 Creative 飛行
  - 修改文件：`src/app.rs`, `src/physics.rs`, `src/state.rs`, `ARCHITECTURE.md`, `plans/implementation/03_creative_flight.md`, `plans/progress.md`, `track.md`
  - 關鍵決策：以事件時間追蹤 300 ms、忽略 key repeat 的 Jump 雙擊；Creative 中雙擊切換 transient flight，WASD 維持相機 yaw 水平移動，Space／Shift 升降，同時按下不產生垂直速度，衝刺飛行為兩倍水平速度。飛行略過重力、流體阻力／浮力及摔落傷害，但沿用 X/Y/Z solid collision；下降碰地退出，撞天花板只停止上升。模式切 Survival、死亡、重生與切維度會安全退出並重設 fall-distance，暫停／背包／聊天／進度介面／失焦只清輸入與 pending tap，保留 hover。飛行狀態及速度不持久化，F3 會標示 `FLYING`。
  - 驗證：雙擊邊界、repeat、停用/reset、新雙擊配對、落地退出、hover、升降、牆／頂／地碰撞、水／熔岩、衝刺速度、持久化速度與非飛行重力／摔落傷害回歸測試通過；`cargo fmt -- --check`、`cargo check --release`、`cargo test --release` 通過，共 201 項單元測試與 1 項整合測試。
  - 備註：第一／第三人稱鏡頭、實際 Host + Join 位置同步與模式切換手感需在互動式遊戲視窗人工驗收。
- ✅ 修復任務 #25 後續問題 (By Codex)：低延遲多人遊戲的遠端玩家移動閃現
  - 修改文件：`src/state.rs`, `src/mob.rs`, `src/network/protocol.rs`, `src/network/transport.rs`, `src/network/client.rs`, `src/network/server.rs`, `ARCHITECTURE.md`, `plans/implementation/02_multiplayer_smoothing.md`, `plans/progress.md`, `track.md`
  - 關鍵決策：協議提升至 v3，pose 加入 wrapping sequence 與 sender timestamp；遠端玩家改用 32 筆有界快照，以 100 ms 延遲找真正包住 target 的兩點插值，批量到達仍保留 sender cadence，並拒絕非法、重複和亂序資料。短缺最新點時只做限速且最多 100 ms 的外推，長 gap 或大位移清空歷史並 snap。client 與 server 對尚未發送的 pose 採 latest-wins，但可靠 world/chat 資料仍逐筆傳送；TCP 啟用 `TCP_NODELAY` 並將 header/payload 合為一次 write。RemotePlayer 的採樣速度不再被 mob update 清除。
  - 驗證：20 Hz→144 Hz 單調平滑採樣、sender cadence、yaw wrap、非法／重複／亂序、外推上限、teleport、protocol roundtrip、server relay/latest-wins、舊協議 handshake 拒絕、transport no-delay 與 RemotePlayer velocity 專項測試通過；`cargo fmt -- --check`、`cargo check --release`、`cargo test --release` 通過，共 191 項單元測試與 1 項整合測試。
  - 備註：本環境未自動操作兩個實際遊戲視窗；Host + Join 的走動、衝刺、跳躍及急轉向視覺 smoke test 保留為發佈前人工驗收。

### 2026-07-23
- ✅ 完成任務 #30 (By Codex)：渲染優化
  - 新增文件：`src/chunk_render.rs`
  - 修改文件：`src/camera.rs`, `src/main.rs`, `src/mob.rs`, `src/passive_mob.rs`, `src/shader.wgsl`, `src/state.rs`, `src/world.rs`, `ARCHITECTURE.md`, `plans/p3/30_render_optimization.md`, `plans/progress.md`, `track.md`
  - 關鍵決策：新增獨立 `TerrainVertex` 與 tile-local UV terrain shader，使完整立方體能按材質、光照與 uniform AO 保守合併並重複 atlas tile；特殊模型、流體與薄雪保留精確路徑。Chunk 生成和三層 LOD mesh 由有界 Rayon jobs 執行，主線程只建立一格 halo snapshot、整合光照和上傳 GPU；dimension generation、chunk lifetime 與 mesh revision 防止卸載、切維度或修改後的過期結果覆蓋新資料。渲染按實際 bounds 做 wgpu 0..1 深度視錐剔除，不透明前到後、透明後到前排序，並依距離選 L0 完整 greedy、L1 surface、L2 4×4 coarse surface；surface skirts 同樣合併。相機 far plane 覆蓋方形視距角落，F3 改報實際 visible chunks、submitted draw calls 與 triangles。
  - 驗證：`cargo fmt -- --check`、`cargo check --release`、`cargo test --release`；182 項單元測試與 1 項整合測試全部通過。新增 terrain vertex layout、六平面視錐、near/far、剔除、透明/不透明排序、LOD 邊界/縮減、greedy 材質/光照/AO、UV 重複、halo snapshot、worker token 過期與 WGSL validation 測試。
  - 備註：`Render distance = 16` 的 60+ FPS 為硬件／場景相關人工驗收，本環境未自動操作世界進行可靠 FPS 量測；非同步與提交量的功能路徑已由測試覆蓋。
- ❌ 決定任務 #29 廢案 (By Tommy)


### 2026-07-22
- ✅ 完成任務 #25 (By Codex)：多人遊戲 - 子任務 6/6 聊天、遠端玩家渲染與斷線處理
  - 修改文件：`src/state.rs`, `src/app.rs`, `src/mob_renderer.rs`, `src/network/server.rs`, `src/network/client.rs`, `ARCHITECTURE.md`, `plans/progress.md`, `docs/superpowers/plans/2026-07-22-multiplayer-06-chat-rendering-disconnect.md`
  - 關鍵決策：`State` 維護 50 筆聊天 ring buffer、文字輸入與連線遺失狀態；`T`/`Enter`/`Esc` 透過既有 `winit` 路由開啟、送出與取消聊天，聊天期間清空移動鍵並抑制視角/互動。Server 不信任 client packet 內的 sender，而以已驗證 `PlayerId` 交由 Host roster 解析 username，再以可靠佇列廣播。`RemotePlayer` 使用共享 mob cuboid path 組成頭、身體、雙臂與雙腿，插值速度驅動步行擺動；名稱以 camera view-projection 投影到螢幕並水平 clamp。Client 斷線會停止網路 gameplay command、清除 remote entity、凍結世界並顯示可返回主選單的非破壞性 overlay，client 暫存世界不會寫回 host save。
  - 驗證：`cargo fmt --check`、`cargo check --release`、`cargo test` 全部通過；149 項單元測試與 1 項整合測試通過。新增聊天 sender 防偽/可靠雙 client relay、client bridge 往返、聊天 ring buffer/清洗、名稱投影、斷線 entity 清理、host bind failure 回報與六部件 avatar mesh 測試；既有 position/action、block sync、host-stop 與 thread join 測試共同覆蓋多人驗收資料路徑。
  - 備註：本環境未執行兩個實際遊戲視窗的人工視覺 smoke test；UI/網格、雙 client 資料流、斷線與清理均有自動測試覆蓋，仍建議發佈前以 Host + 2 Join 視窗確認視覺尺寸與操作手感。
- 🟡 進度任務 #25 (By GPT-5.6 Sol)：多人遊戲 - 子任務 5/6 世界（方塊）同步
  - 修改文件：`src/world.rs`, `src/fluid.rs`, `src/mob.rs`, `src/passive_mob.rs`, `src/state.rs`, `src/network/protocol.rs`, `src/network/server.rs`, `src/network/client.rs`, `ARCHITECTURE.md`, `docs/superpowers/plans/2026-07-22-multiplayer-05-world-sync.md`, `track.md`
  - 關鍵決策：以穩定 `BlockType` wire 值及共享 seed 實作 mutation-only 同步；Host 對玩家、流體、紅石、爆炸、天氣、葉片衰減、支撐方塊連鎖破壞與羊吃草結果統一可靠廣播，Client 僅套用 block storage、光照與跨 Chunk/AO 網格失效，不執行衍生世界模擬。未載入 Chunk 的即時 mutation 會按座標合併延後，join 時另傳送本 session 或存檔中已偏離決定性生成的已載入 Chunk 壓縮 payload。新增 `TimeSync` 每秒校正遊戲 tick 與可見天氣，協議版本提升至 2；方塊與定向 Chunk payload 使用 backpressure 可靠傳送，pose/action 仍維持 bounded best-effort。
  - 驗證：`cargo fmt --check`、`cargo check --release`、`cargo test` 通過；140 項單元測試與 1 項整合測試全部通過。新增 block wire roundtrip、遠端邊界 block 的光照/mesh dependency、權威/純視覺爆炸、可靠 `BlockChange`、定向 `ChunkData` 與 `TimeSync` 端到端測試。
  - 備註：兩個實際遊戲視窗中的方塊/流體/爆炸視覺與 join-mid-game GUI smoke test 仍需人工互動驗證；資料協議、server/client relay 與 CPU mutation path 已由自動測試覆蓋。
- 🔧 補完任務 #25 子任務 3/6 未完成驗證步驟 (By GLM-5.2)：Step 7 編譯冒煙與 Step 2 雙實例煙霧測試
  - 修改文件：`src/network/client.rs`, `docs/superpowers/plans/2026-07-22-multiplayer-03-client-bridge.md`, `plans/progress.md`
  - 關鍵決策：完成子任務 3 計畫中最後兩個未勾選 checkbox。Step 7 以 `cargo check --release` 通過（僅 2 項 Sub-tasks 4-6 預留變體的既有 dead-code 警告，無錯誤），並以 release binary 啟動單一與雙實例各持續 8 秒與 6 秒無 panic、無 stderr，覆蓋「binary 啟動且伺服器執行緒不崩潰」需求。Step 2 新增自動化整合測試 `host_stop_notifies_client_and_threads_join_without_hanging`，驗證主機停止伺服器後 client 收到 `ClientToGame::Disconnected` 且雙背景執行緒於 3 秒逾時內乾淨 join 無 panic，補足「quitting either side cleans up the background thread without hanging」需求；既有 `connects_and_receives_join_for_second_client` 已覆蓋 seed 傳播與 `PlayerJoin`。
  - 驗證：`cargo fmt --check` 通過；`cargo check --release` 通過；`cargo test --release` 共 134 項單元測試與 1 項整合測試全部通過（含新增 1 項 host-stop 清理測試）。
  - 備註：兩視窗 Host/Join 點擊進入世界的完整 GUI 流程仍需互動式 Windows UI 自動化，維持手動檢查；資料路徑（seed 傳播、join 通知、disconnect 清理、執行緒拆解）已由自動測試完整覆蓋。
- 🟡 進度任務 #25 (By GPT-5.6 Sol High)：多人遊戲 - 子任務 4/6 玩家狀態同步
  - 修改文件：`src/entity.rs`, `src/state.rs`, `src/mob.rs`, `src/mob_renderer.rs`, `src/network/server.rs`, `ARCHITECTURE.md`, `docs/superpowers/plans/2026-07-22-multiplayer-04-player-sync.md`
  - 關鍵決策：新增 `EntityType::RemotePlayer` 與網路玩家 metadata；`State` 以穩定 entity ID 綁定 remote player，而非會受 `Vec` 刪除影響的 index。主機中繼 client pose/action 並以 `PlayerId(0)` 廣播本機狀態，server 在 newcomer 登入時重播既有 roster。remote pose 以獨立網路時鐘保留兩個 snapshot，採 100 ms 延遲、端點 clamp 與最短 yaw 路徑插值，10 秒未更新後固定在最新姿勢；join/leave/disconnect 會建立或清理相應 entity，remote player 不進入 mob physics、AI、despawn 或本機 combat damage path。成功的本機 block break/place 會送出最小 `PlayerAction` cosmetic cue。
  - 驗證：`cargo fmt --check`、`cargo check --release`、`cargo test` 全部通過；完整測試為 133 項單元測試與 1 項整合測試，包含 remote-player AABB/default、插值中點與端點 clamp、雙 client position/action relay、newcomer roster replay 及斷線清理。
  - 備註：本子任務提供 placeholder cuboid 以驗證共享 render path；完整玩家模型、名稱顯示、chat 與 disconnect UI 仍由子任務 6 完成。實際多視窗視覺 smoke test 仍需人工執行，資料路徑與 pose 計算已由自動測試覆蓋。
- 🟡 進度任務 #25 (By GPT-5.6 Sol High)：多人遊戲 - 子任務 3/6 客戶端橋接與 State 整合
  - 新增文件：`src/network/client.rs`
  - 修改文件：`src/network/mod.rs`, `src/menu.rs`, `src/app.rs`, `src/state.rs`, `ARCHITECTURE.md`, `docs/superpowers/plans/2026-07-22-multiplayer-03-client-bridge.md`
  - 關鍵決策：新增背景執行緒 Tokio `NetworkClient`，將所有 wire packet 映射為 `ClientToGame` / `GameToClient` 同步通道事件，包含版本握手、登入、keepalive、斷線回報與乾淨關閉。主選單新增 Host/Join 面板與連線欄位，`MultiplayerRole` 隨 `WorldLaunch` 進入 `State`。`NetworkHandle` 統一封裝 host/client 通道與執行緒；`State::update` 每幀先 drain 網路事件並以 20 Hz 送出本機位置。Client 在收到伺服器 seed 前不生成 chunk，也不讀寫本機 chunk save；方塊破壞／放置改送請求，fluid 與 redstone 本機 mutation 被抑制。Host/Singleplayer 保持 authoritative。
  - 驗證：`cargo fmt --check`、`cargo check --release` 通過；`cargo test network::client` 1/1 通過；完整 `cargo test` 為 129 項單元測試與 1 項整合測試全部通過。release binary 可啟動並在主選單持續運行而無 panic。自動雙 client 測試另驗證 server seed、第二位玩家 `PlayerJoin` 與三個背景執行緒乾淨 join。
  - 備註：Windows UI automation native pipe 在此環境不可用，因此單人／Host 點擊流程及兩個實際視窗的 Host/Join smoke test 仍需手動執行；計畫中的這兩個 checkbox 保持未勾選。為滿足全域 fmt gate，一併套用了先前已記錄的 6 個遊戲模組純 rustfmt 修正。
- 🟡 進度任務 #25 (By GPT-5.6 Sol High)：多人遊戲 - 子任務 2/6 整合式伺服器核心
  - 新增文件：`src/network/server.rs`
  - 修改文件：`src/network/mod.rs`, `src/network/transport.rs`, `ARCHITECTURE.md`, `docs/superpowers/plans/2026-07-22-multiplayer-02-server-core.md`
  - 關鍵決策：新增 `NetworkServer`，在專用背景執行緒建立 Tokio runtime，透過 `ServerToHost` / `HostToServer` 同步通道與尚未接線的主遊戲執行緒隔離。伺服器負責 TCP 接受、版本握手、從 1 開始的單調 `PlayerId`、登入、會話註冊、已驗證身分的玩家事件轉發、主機命令廣播、64 封包有界客戶端佇列、5 秒 keepalive、15 秒無輸入逾時及離線清理。`Connection` 新增 crate 內部 owned read/write split，使每個客戶端的接收/逾時與發送/keepalive 由獨立任務處理，慢速 socket 寫入不會凍結該會話的接收逾時。`std::sync::mpsc::Receiver` 以 10 ms `try_recv` 定時器橋接，避免阻塞 Tokio worker 或令 runtime 關閉卡在 `spawn_blocking`。
  - 驗證：`cargo test network::server` 3/3 通過（登入、雙客戶端位置中繼、斷線清理）；`cargo check --release` 通過；完整 `cargo test` 共 128 項單元測試與 1 項整合測試全部通過。新增 network 文件通過 rustfmt，`git diff --check` 通過。
  - 備註：全域 `cargo fmt --check` 仍只報子任務 1 已記錄的 6 個既有遊戲模組格式漂移；本子任務遵守「no game module is modified」範圍，未改寫無關檔案。
- 🟡 進度任務 #25 (By GLM-5.2)：多人遊戲 - 子任務 1/6 網路協議與傳輸層
  - 新增文件：`src/network/mod.rs`, `src/network/protocol.rs`, `src/network/transport.rs`
  - 修改文件：`Cargo.toml`, `src/main.rs`
  - 關鍵決策：建立獨立、可測試且不依賴任何遊戲模組的網路基礎層。`protocol.rs` 以既有 `bincode` 序列化版本化的 `Packet` 列舉（Handshake、LoginSuccess、Disconnect、PlayerPosition、PlayerAction、PlayerJoin、PlayerLeave、BlockChange、ChunkData、ChatMessage、Keepalive 共 11 種變體，每個封包攜帶 `protocol_version: u32` 以便未來拒絕不相容客戶端）。`transport.rs` 的 `Connection` 包裝 `tokio::net::TcpStream`，以 4 位元組大端長度前綴框架搭配 2 MiB 上限防護惡意對端，`recv`/`send` 為非同步並重用讀取緩衝區。Tokio runtime 不在此層啟動，沿用 `save.rs` 的背景執行緒 + mpsc 模式以保持主 winit 執行緒完全同步。`BlockType` 暫以 `u32` 刻面傳輸，與遊戲列舉內部佈局解耦（轉換輔助函式留待子任務 5）。
  - 驗證：`cargo check --release` 通過（僅基金會層未使用的預期 dead-code 警告）；`cargo test` 共 125 項單元測試與 1 項整合測試全部通過（含新增 13 項 protocol 往返測試與 2 項 transport TCP 框架往返測試）。本環境修復了損壞遺失的 `rust-std` 與 `rustfmt` 元件。
  - 備註：`cargo fmt --check` 於既有遊戲模組（`boss.rs`、`mob.rs`、`state.rs`、`world.rs`、`chunk_manager.rs`、`mob_renderer.rs`）存在先前的格式漂移，非本子任務引入；依計畫「no game module is touched」原則未一併處理，新增的 `network` 檔案已通過 fmt。

### 2026-07-21
- ✅ 完成任務 #28 (By Gemini 3.5 Flash High): 成就 / 進度系統
  - 新增文件：`src/advancements.rs`
  - 修改文件：`src/main.rs`, `src/entity.rs`, `src/save.rs`, `src/state.rs`, `src/app.rs`, `ARCHITECTURE.md`
  - 關鍵決策：設計並實現完整 Minecraft 規範的成就／進度系統。包含 5 大類別（Minecraft, Nether, TheEnd, Adventure, Husbandry）共 50 個成就樹、Task/Goal/Challenge 框型與經驗獎勵。實作觸發引擎（ObtainItem, MineBlock, CraftItem, EnchantItem, KillMob, EnterDimension, LevelUp, ConsumeItem, BrewPotion）、Top-Right Toast 彈出動畫、互動式 GUI 樹狀圖（`L` 鍵開啟、5 分頁、滑鼠滾輪縮放、按住拖曳平移、Tooltip 懸停解鎖狀態）、經驗值獎勵發放，以及完整 JSON Bincode 存檔持久化與舊存檔向下相容。
  - 驗證：`cargo fmt`、`cargo test`（103 項單元測試與 1 項整合測試全部通過）、`cargo check --release` 通過。
- 🔧 額外修復：選取／建立世界時 NVIDIA Vulkan 驅動崩潰 (By GPT-5.6 Terra Extra High)
  - 修改文件：`src/app.rs`, `src/menu.rs`, `src/state.rs`, `ARCHITECTURE.md`
  - 修復內容：世界切換改為在 `WindowEvent` 中排程、於 `about_to_wait` 才執行，避免在原生滑鼠 callback 尚未返回時銷毀 menu 的 wgpu surface 並為同一視窗建立遊戲 surface。`State::new` 僅同步載入玩家周圍 3×3 Chunk，剩餘視距交由既有逐幀串流處理，避免視距 12 時一次建立 625 個網格。Windows 上 menu 與遊戲統一強制使用 DX12；Windows Error Reporting 已確認原始故障模組為 NVIDIA Vulkan ICD `nvoglv64.dll`（`0xc0000005` / `0xc000041d`），而 `PRIMARY` 仍會優先選擇 Vulkan。
  - 驗證：`cargo fmt --check`、`cargo test`（100 項單元測試與 1 項整合測試）、`cargo build --release` 通過；實際點擊 Play Selected 與 Create World 已確認可進入世界且不再崩潰。
- ✅ 完成任務 #26 (By GPT-5.6 Sol Extra High)：Nether / End + Boss
  - 新增文件：`src/dimension.rs`, `src/boss.rs`
  - 修改文件：`src/main.rs`, `src/state.rs`, `src/world.rs`, `src/chunk_manager.rs`, `src/save.rs`, `src/entity.rs`, `src/mob.rs`, `src/passive_mob.rs`, `src/mob_renderer.rs`, `src/inventory.rs`, `src/crafting.rs`, `src/texture.rs`, `src/shader.wgsl`, `ARCHITECTURE.md`
  - 關鍵決策：新增 Overworld/Nether/End 維度模型，Chunk 生成與 Region 存檔依維度分流，`dimension.dat` 保存目前維度。Nether 具基岩天花板、地獄岩洞穴、岩漿海、靈魂沙與螢光石；地獄門採 8:1 水平座標縮放並建立連結門。End 具末地石島、起始噴泉、要塞末地門、末地城箱與鞘翅掉落。新增 Blaze/Piglin/Husk/Shulker/EndCrystal/EnderDragon/Wither 與投射物，Boss 邏輯集中於 `boss.rs`，支援末影龍階段、龍息、水晶回血、擊敗後回程門與龍蛋，以及凋零召喚、Boss 條、凋零效果、骷髏頭、衝刺爆炸與地獄之星掉落。
  - 驗證：`cargo test --release` 通過；99 項單元測試與 1 項整合測試全部通過。

### 2026-07-20
- ✅ 完成任務 #24 (By GPT-5.6 Sol High)：主選單 + 世界管理
  - 新增文件：`src/menu.rs`
  - 修改文件：`Cargo.toml`, `src/main.rs`, `src/app.rs`, `src/state.rs`, `src/world.rs`, `src/save.rs`, `ARCHITECTURE.md`
  - 關鍵決策：將應用重構為 `Menu` / `Game` 雙運行時，啟動時只建立輕量主選單渲染器，選定世界後才初始化完整遊戲。新增 iCraft 像素 Logo、程序化緩慢旋轉方塊全景、世界列表、建立/刪除確認、多世界目錄與向下相容的 `world.meta`；新世界種子現在實際驅動地形、洞穴與群系噪聲。設定擴充為 FOV、視距、全螢幕、VSync、主/音樂/音效音量、難度、英/德語言及八項可重綁控制鍵，和平難度會移除並停止生成敵對生物。
  - 驗證：實際啟動 Release 視窗確認主選單可渲染及回應；`cargo test` 的 83 項單元測試與 1 項整合測試全部通過，`cargo check --release` 通過。
- ✅ 完成任務 #23 (By GPT-5.6 Sol High)：天氣系統
  - 新增文件：`src/weather.rs`, `assets/sounds/rain.wav`, `assets/sounds/thunder.wav`
  - 修改文件：`src/main.rs`, `src/state.rs`, `src/particles.rs`, `src/audio.rs`, `src/player.rs`, `src/world.rs`, `src/inventory.rs`, `src/texture.rs`, `assets/texture_atlas.png`, `ARCHITECTURE.md`
  - 關鍵決策：以獨立、確定性且可測試的狀態機依序切換晴天／雨天／雷暴，每段持續 0.5~1 個遊戲日。降水依 Perlin 生態群系分類：沙漠保持乾燥、針葉林與山地降雪、其餘群系降雨；粒子生命週期由每欄高度圖截斷，避免穿過地形、樹葉與屋頂。雨天／雷暴降低天空與天空光，雨聲使用真正無限循環音源。雷擊提供全屏閃光、發光電弧與 3D 雷聲，會傷害／點燃實體並生成發光火焰方塊；寒冷群系會逐步鋪設具 1/8 方塊高度的薄雪層。
  - 驗證：`cargo fmt -- --check`、`cargo check --release`、`cargo test --release` 通過；79 項單元測試與 1 項整合測試全部通過。`cargo run --release` 實際啟動冒煙測試無 panic，互動式雨雪、閃電與音效抽查保留於任務文件。
- ✅ 完成任務 #22 (By GPT-5.6 Sol High)：紅石系統
  - 新增文件：`src/redstone.rs`
  - 修改文件：`src/main.rs`, `src/world.rs`, `src/inventory.rs`, `src/crafting.rs`, `src/texture.rs`, `src/audio.rs`, `src/state.rs`, `assets/texture_atlas.png`, `ARCHITECTURE.md`
  - 關鍵決策：以 20 Hz 確定性 tick、座標索引元件狀態及最多 64 輪的有界穩定傳播實作 0~15 紅石功率、弱／強充能、跨 Chunk 電路與循環保護。中繼器、按鈕和 TNT 使用統一 tick 排程；比較器支援比較／相減。執行器以方塊 mutation/action 回傳，由 `State` 統一處理光照、網格、爆炸、投射物與音效，涵蓋活塞／黏性活塞、紅石燈、門／活板門、TNT、發射器／投擲器及 25 音高音階盒。所有元件均加入材質、Creative 背包及 Survival 合成鏈。
  - 驗證：`cargo fmt -- --check`、`cargo test --release`、`cargo check --release` 通過；72 項單元測試與 1 項整合測試全部通過。`cargo run --release` 實際啟動冒煙測試無 panic。
- ✅ 完成任務 #21 (By GPT-5.6 Sol High)：附魔 / 釀造系統
  - 新增文件：`src/enchantment.rs`, `src/brewing.rs`
  - 修改文件：`src/main.rs`, `src/app.rs`, `src/inventory.rs`, `src/crafting.rs`, `src/world.rs`, `src/texture.rs`, `src/player.rs`, `src/entity.rs`, `src/mob.rs`, `src/mob_renderer.rs`, `src/passive_mob.rs`, `src/save.rs`, `src/state.rs`, `assets/texture_atlas.png`, `ARCHITECTURE.md`
  - 關鍵決策：以固定容量附魔集合、藥水資料及固定長度名稱擴充可複製的 `ItemStack`，保留既有背包拖放架構並提供舊存檔升級。附魔台提供受最多 15 個書架影響的三個選項並消耗經驗／青金石；鐵砧支援修復、合併與鍵盤重命名。釀造台支援十種效果、延時／升級／噴濺修飾與 4 格範圍投射物，效果接入移動、戰鬥、AI、光照、火焰、氧氣及 HUD。效率、耐久、絲綢之觸、時運、鋒利、擊退、火焰附加、搶奪、保護、摔落保護、水下呼吸、力量與無限均已接入現有玩法。
  - 驗證：`cargo fmt -- --check`、`cargo test --release`、`cargo check --release` 通過；64 項單元測試與 1 項整合測試全部通過。`cargo run --release` 實際啟動冒煙測試無 panic。
- ✅ 完成任務 #20 (By GPT-5.6 Sol High)：環境光遮蔽 (Ambient Occlusion)
  - 修改文件：`src/world.rs`, `src/state.rs`, `src/shader.wgsl`, `src/chunk_manager.rs`, `src/fluid.rs`, `src/mob.rs`, `src/passive_mob.rs`, `src/mob_renderer.rs`, `src/particles.rs`, `ARCHITECTURE.md`
  - 關鍵決策：在 Chunk CPU 網格生成階段依每個面頂點外側的兩個側邊格及對角格計算四級 AO，只有 solid opaque 方塊遮擋；以獨立平滑插值頂點屬性在 Shader 中與既有 flat 打包光照相乘，並依對角 AO 總和選擇三角線。統一方塊網格依賴 helper，支援負座標、Chunk 邊角的對角失效，以及載入／卸載時八鄰居重建；非 Chunk 幾何固定使用 AO 1.0。
  - 驗證：`cargo fmt -- --check`、`cargo check --release`、`cargo test --release` 通過；56 項單元測試與 1 項整合測試全部通過。互動式遊戲視覺抽查項目保留於任務文件。
- ✅ 完成任務 #19 (By GPT-5.6 Sol Medium)：F3 Debug 畫面 (F3 Debug Overlay)
  - 修改文件：`src/state.rs`, `src/app.rs`
  - 關鍵決策：在既有 F3 HUD 上加入每 0.5 秒平滑更新的 FPS 與平均幀時間，並完整顯示玩家座標、yaw/pitch、Chunk、生態群系、已載入 Chunk、實體與粒子、渲染頂點/三角形、估算記憶體及遊戲時間。記憶體估算涵蓋 Chunk 固定資料、活動網格、實體與粒子配置；F3 輸入忽略鍵盤 repeat，避免長按時反覆切換。擴充向量字體與文字頂點緩衝，確保新增欄位可完整顯示。
  - 驗證：`cargo fmt -- --check`、`cargo test --release` 通過；45 項單元測試與 1 項整合測試全部通過。
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
