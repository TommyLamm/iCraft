# Track 13：門／活板門方向與薄碰撞 Block-State

> 對應計畫：`plans/implementation/13_door_trapdoor_state.md`（Task 10 後續項 W1）
> 相依：與計畫 12 共用 `PROTOCOL_VERSION` 升 v5 與 `BlockChange`/`ChunkData`（見協調說明）。
> 狀態：⏳ 待實作
> 目標：門與活板門擁有正式 facing、half（門頂／底）、hinge（左／右）與開啟狀態，
> 正確薄板碰撞 AABB 與專用立體模型，取代目前通用 1×1×1 完整 cube。
> Commit：`feat(blocks): door and trapdoor facing, thin collision, and proper models`

## 協議版本協調（重要）
- 計畫 13 與 12 都把 `PROTOCOL_VERSION` 4->5，且都動 `BlockChange`/`ChunkData`。
- **建議順序**：先做 12 升 v5；13 再於同一 v5 為 `BlockChange`/`ChunkData` 附加 `state` 欄位。
- 若先做 13，則 13 負責升 v5，12 不再升版。**只升一次版**。本 track 假設 12 已升 v5；若 13 先行，將 13.9 改為「升 v5」並在 12.3 改為「沿用 v5」。

## 相關程式碼位置（已核對）
- `src/world.rs:254` `BlockType`；`:323-326` `OakDoor=67/OakDoorOpen=68/OakTrapdoor=69/OakTrapdoorOpen=70`。
- `src/world.rs:1044-1057` `BlockProperties`（`OakDoor` is_solid=true、`OakDoorOpen` is_solid=false；活板門同）。
- `src/world.rs:1337-1338` atlas tile：門 `(9,14)`、活板門 `(10,14)`，六面同一 tile。
- `src/world.rs:1580` `append_torch_mesh`（特殊 mesh 範例）；`:1664` `is_greedy_cube`（門走 greedy 完整 cube）；`:1692` `Chunk` struct（無 metadata）；`:2203` `Chunk::get_block`。
- `src/chunk_manager.rs:159` `get_block`；`:182` `can_place_block_with_support`；`:193` `set_block`。
- `src/physics.rs:56` `unit_block_aabb`；`:70` `block_placement_decision`（non-solid 一律允許，solid 用完整 cube）；`:300` `resolve_collisions`（只檢查 `is_solid`，用 `unit_block_aabb`）；`:44-53` `player_aabb`。
- `src/redstone.rs:49` `Direction::from_yaw`；`:99-102` `ComponentState.facing`；`:700-712` 門／活板門 toggle（只 swap `BlockType`，不設 state）；`:1021-1024` 門為 redstone component。
- `src/save.rs:221` `ChunkSaveData`（已有 `redstone_metadata` sidecar 範例可仿）；`:399` `deserialize_chunk_save_data` legacy fallback。
- `src/state.rs` handle_click 放置路徑只 `set_block(OakDoor)`，不設 facing／half／hinge。

## 子任務清單

### 階段一：Block-state 儲存

#### 13.1 定義 `BlockState` 結構體與 u8 編碼
- [ ] 檔案：`src/world.rs`
- 步驟：
  1. 定義編碼：`bits[1:0]=facing`(N=0,S=1,W=2,E=3)、`bit2=half`(0=bottom,1=top,門專用)、`bit3=hinge`(0=left,1=right,門專用)、`bit4=open`(0=closed,1=open,活板門專用)、`bits[7:5]=reserved`。
  2. 提供 `BlockState::encode()->u8` / `decode(u8)->Self` 及 default（facing=North, bottom, left, closed）。
  3. 提供 facing/half/hinge/open 各欄位 accessor。
- 驗證：編譯通過；default == 0。

#### 13.2 `BlockState` encode/decode 測試
- [ ] 檔案：`src/world.rs` 測試區
- 步驟：所有 facing×half×hinge×open 組合 roundtrip；default encode==0；保留位忽略。
- 驗證：`cargo test --release block_state` 全綠。

#### 13.3 `Chunk` 新增 `block_states` 欄位
- [ ] 檔案：`src/world.rs:1692`
- 步驟：新增 `pub block_states: Box<[[[u8; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>`（每格 1 byte，預設 0）。只有具方向方塊使用非零值。
- 驗證：欄位存在。

#### 13.4 `Chunk::new_with_seed` 初始化 `block_states`
- [ ] 檔案：`src/world.rs:1708`
- 步驟：配置為全零陣列（與 `blocks` 同配置方式）。
- 驗證：新 chunk `block_states` 全 0。

#### 13.5 `ChunkManager` get/set_block_state accessor
- [ ] 檔案：`src/chunk_manager.rs`
- 步驟：`get_block_state(x,y,z)->u8`（chunk 未載入回 0）；`set_block_state(x,y,z,state:u8)`。座標轉換沿用 `get_block`/`set_block` 模式。
- 驗證：accessor 正確讀寫；跨 chunk 邊界不 panic。

### 階段二：存檔與網路同步

#### 13.6 `ChunkSaveData` 新增 `block_states` 欄位
- [ ] 檔案：`src/save.rs:221`
- 步驟：新增 `pub block_states: Vec<u8>`（Zlib 壓縮的平坦陣列，仿 `redstone_metadata` sidecar 模式）。
- 驗證：欄位存在。

#### 13.7 序列化／反序列化 + legacy fallback
- [ ] 檔案：`src/save.rs`
- 步驟：
  1. `from_chunk` 寫入 `block_states`（壓縮）。
  2. `block_states()` accessor 解壓回傳。
  3. `deserialize_chunk_save_data`(`:399`) legacy fallback：舊存檔無此欄位 -> 全零。
- 驗證：roundtrip；舊存檔 fallback 全零。

#### 13.8 存檔測試
- [ ] 檔案：`src/save.rs` 測試區（仿 `:1094` redstone sidecar 測試）
- 步驟：`block_states` roundtrip；舊版（無欄位）fallback 全零；非零 state（門 facing）保存後還原。
- 驗證：`cargo test --release save` 全綠。

#### 13.9 協議：`BlockChange` 新增 `state` 欄位（v5）
- [ ] 檔案：`src/network/protocol.rs:63`
- 步驟：
  1. 若 12 已升 v5：在 `BlockChange` 加 `state: u8`。
  2. 若 13 先行：升 `PROTOCOL_VERSION=5` 並加 `state`。
  3. 更新 `protocol_version()` 不需動（已有 arm）。
- 驗證：編譯通過。

#### 13.10 協議：`ChunkData` 附加 `block_states` 區段
- [ ] 檔案：`src/network/protocol.rs:70`
- 步驟：`ChunkData` payload 末尾附加壓縮 `block_states`（或在 `blocks` 後加獨立 `Vec<u8>` 欄位）；client 解時長度不足視為全零。
- 驗證：編譯通過。

#### 13.11 協議 roundtrip 測試
- [ ] 檔案：`src/network/protocol.rs` 測試區
- 步驟：`BlockChange` 帶 state roundtrip；`ChunkData` 帶 block_states roundtrip；舊長度不足 fallback 全零。
- 驗證：`cargo test --release` 全綠。

#### 13.12 Server relay 傳遞 `state`
- [ ] 檔案：`src/network/server.rs`
- 步驟：`ServerToHost::ClientBlockChange`/`ClientBlockAction` 與 `HostToServer::BroadcastBlockChange` 帶 `state`；server 轉發 `Packet::BlockChange` 帶 `state`；`SendChunk`/`Packet::ChunkData` 帶 block_states。
- 驗證：server 編譯；state 透傳。

#### 13.13 Client apply 傳遞 `state`
- [ ] 檔案：`src/network/client.rs`
- 步驟：`ClientToGame::BlockChange`/`ChunkData` 帶 state/block_states；收 `Packet::BlockChange`/`ChunkData` 映射時帶上。
- 驗證：client 編譯；映射正確。

#### 13.14 State apply 函式帶 `state`
- [ ] 檔案：`src/state.rs`
- 步驟：`apply_synced_block_change`(`:315`)、`apply_remote_block_change`(`:6384`)、`set_block_and_broadcast`(`:6324`)、`apply_remote_chunk_data`(`:6414`) 都帶 `state`；`ChunkManager::set_block` 同步更新對應 `block_states`。
- 驗證：state 正確寫入 chunk。

#### 13.15 同步測試
- [ ] 檔案：`src/network/*` + `src/state.rs` 測試區
- 步驟：host 廣播帶 state 的 BlockChange -> client 套用正確 state；ChunkData 帶 block_states -> client 還原；舊長度 fallback 全零。
- 驗證：`cargo test --release` 全綠。

### 階段三：放置邏輯

#### 13.16 門放置：facing + bottom/top + hinge
- [ ] 檔案：`src/state.rs` handle_click 放置路徑
- 步驟：
  1. 從 `camera.yaw` 算 `Direction::from_yaw`，設為 facing。
  2. 目標格放 bottom half（`OakDoor`，state half=0）；上方格放 top half（`OakDoor`，state half=1）。
  3. 上方格不可放置（已佔用或超出 256）-> 拒絕。
  4. 鉸鏈：預設 left；facing 左側有實心方塊而右側無 -> right（vanilla 規則）。
  5. 兩格 `set_block` + `set_block_state`。
- 驗證：門放置產生 bottom+top 兩格、facing/hinge 正確。

#### 13.17 活板門放置：facing + open=0
- [ ] 檔案：`src/state.rs` handle_click 放置路徑
- 步驟：facing 從 yaw 推導；open bit=0（關閉）；`set_block(OakTrapdoor)`+`set_block_state`。
- 驗證：活板門放置 facing 正確、關閉。

#### 13.18 放置測試
- [ ] 檔案：`src/state.rs` 測試區
- 步驟：門 bottom+top 兩格、各 yaw 方向 facing 正確、鉸鏈依鄰居決定（左實右空->right）、上方佔用拒絕、活板門 facing+closed。
- 驗證：`cargo test --release` 全綠。

### 階段四：模型

#### 13.19 `append_door_mesh`
- [ ] 檔案：`src/world.rs`
- 步驟：
  1. 讀 block_state 的 facing/half/open。
  2. 關閉：薄板（3/16 厚度）貼方塊格 facing 側邊緣。
  3. 開啟：薄板旋轉 90° 貼鉸鏈側。
  4. 上／下半身用不同 atlas UV（門上／下半紋理不同；若 atlas 只有一塊可均分或重複）。
  5. 所有頂點 AO 1.0，以來源格 sky/block light 打包（與 torch 同策略，參考 `append_torch_mesh:1580`）。
- 驗證：關閉／開啟薄板位置、UV、winding、light 正確。

#### 13.20 `append_trapdoor_mesh`
- [ ] 檔案：`src/world.rs`
- 步驟：關閉：薄板（3/16 厚度）平貼方塊格底部；開啟：薄板旋轉至垂直貼 facing 側。AO/light 同 13.19。
- 驗證：關閉（底部水平）／開啟（側邊垂直）正確。

#### 13.21 `is_greedy_cube` 排除門／活板門
- [ ] 檔案：`src/world.rs:1664`
- 步驟：在 `is_greedy_cube` 排除 `OakDoor|OakDoorOpen|OakTrapdoor|OakTrapdoorOpen`，使其走特殊 mesh 路徑。
- 驗證：門／活板門不再生成完整 cube。

#### 13.22 `generate_mesh` 分支 + MeshSnapshot block_state
- [ ] 檔案：`src/world.rs`
- 步驟：
  1. `generate_mesh` 在 torch/cross-model 之後、generic cube 之前加 `OakDoor|OakDoorOpen => append_door_mesh`、`OakTrapdoor|OakTrapdoorOpen => append_trapdoor_mesh` 分支。
  2. 擴充 `MeshSnapshot`/closure 回傳型別加入 `u8 block_state` 欄位，使 closure 能取得鄰居 block_state。
- 驗證：mesh 分支正確觸發；鄰居 state 可讀。

#### 13.23 模型測試
- [ ] 檔案：`src/world.rs` 測試區（仿 torch mesh 測試）
- 步驟：門關閉／開啟各 facing 與 half 的頂點位置、UV、winding、AO/light；活板門關閉／開啟；精確 vertex/index 數量。
- 驗證：`cargo test --release door_mesh` 全綠。

### 階段五：碰撞

#### 13.24 `block_aabb` 函式
- [ ] 檔案：`src/physics.rs`
- 步驟：`block_aabb(block, block_state, pos)->AABB`：
  1. 門關閉：薄板 AABB（facing 側，3/16 厚度）。
  2. 門開啟：薄板 AABB（鉸鏈側，3/16 厚度）。
  3. 活板門關閉：薄板 AABB（底部，3/16 厚度）。
  4. 活板門開啟：薄板 AABB（facing 側垂直，3/16 厚度）。
  5. 其他方塊：回退 `unit_block_aabb`。
- 驗證：各狀態 bounds 精確。

#### 13.25 `resolve_collisions` 改用 `block_aabb`
- [ ] 檔案：`src/physics.rs:300`
- 步驟：`:318`/`:373` 兩處 `unit_block_aabb((x,y,z))` 改呼叫 `block_aabb(block, state, (x,y,z))`，需從 chunk_manager 取 block_state。開啟狀態（`OakDoorOpen`/`OakTrapdoorOpen`）`is_solid=false` 自然跳過。
- 驗證：關閉門薄板碰撞；開啟無碰撞。

#### 13.26 `block_placement_decision` 改用 `block_aabb`
- [ ] 檔案：`src/physics.rs:70`
- 步驟：`:79` `unit_block_aabb(block_pos)` 改 `block_aabb(block, state, block_pos)`；non-solid 仍直接 Allowed。函式簽名加入 `state: u8` 參數。
- 驗證：放置決策使用薄板 AABB；呼叫端同步更新。

#### 13.27 碰撞測試
- [ ] 檔案：`src/physics.rs` 測試區
- 步驟：門關閉／開啟各 facing 的薄板 AABB bounds；活板門關閉／開啟；`resolve_collisions` 使用薄板 AABB；開啟狀態無碰撞；放置決策使用薄板 AABB。
- 驗證：`cargo test --release collision` 全綠。

### 階段六：紅石整合與收尾

#### 13.28 紅石 toggle 更新 open bit
- [ ] 檔案：`src/redstone.rs:700-712`
- 步驟：toggle 門／活板門時，除 swap `BlockType` 外，更新 block_state 的 open bit（關閉<->開啟），`set_block_record` 帶上 state。`reconcile_mutations` 用 `or_insert_with` 保留既有 facing（G3 已修復模式）。
- 驗證：紅石 toggle 正確翻轉 open bit 並保留 facing。

#### 13.29 右鍵互動開關門／活板門
- [ ] 檔案：`src/state.rs` handle_click redstone interact 分支
- 步驟：加入 `OakDoor`/`OakTrapdoor`，toggle open bit 並更新 mesh（重建該 chunk mesh）。
- 驗證：右鍵開關門／活板門。

#### 13.30 紅石＋互動測試
- [ ] 檔案：`src/redstone.rs` + `src/state.rs` 測試區
- 步驟：紅石 toggle 更新 open bit 並重建 mesh；右鍵 toggle；facing 保留；既有 redstone door toggle 測試不回歸（`redstone.rs:1229` 系列）。
- 驗證：`cargo test --release` 全綠。

#### 13.31 既有測試不回歸
- [ ] 步驟：確認 placement collision、redstone door toggle、save/load、multiplayer block sync 測試仍通過。
- 驗證：`cargo test --release` 全綠。

#### 13.32 格式／編譯／測試閘門
- [ ] 步驟：`cargo fmt --all -- --check`、`cargo check --release`、`cargo test --release`。
- 驗證：三者通過。

#### 13.33 更新文件
- [ ] 檔案：`ARCHITECTURE.md`、`track.md`、`plans/progress.md`、`plans/implementation/10_bug_audit.md`
- 步驟：方塊儲存、模型、碰撞、紅石段落說明 block-state 系統；W1 標記已修復。
- 驗證：文件與實作一致。

#### 13.34 人工驗收
- [ ] 步驟：互動式視窗各角度查看門／活板門模型；開關碰撞；紅石控制；存檔重載方向保持。
- 驗證：模型正確、碰撞合理、紅石可控、存檔保持 facing。

#### 13.35 Commit
- [ ] 步驟：單一 commit `feat(blocks): door and trapdoor facing, thin collision, and proper models`，只 stage 本任務檔案。
- 驗證：`git diff --check` 通過。

## 驗收條件（對應計畫驗證清單）
- [ ] `BlockState` encode/decode roundtrip（所有 facing/half/hinge/open 組合）。
- [ ] `ChunkSaveData` block_states 序列化／反序列化；舊存檔 fallback 全零。
- [ ] 協議 v5 `BlockChange` 帶 state；舊 client handshake 被拒。
- [ ] 門放置：bottom + top 兩格、facing 正確、鉸鏈依鄰居決定。
- [ ] `append_door_mesh`：關閉／開啟薄板正確位置、UV、winding、AO/light。
- [ ] `append_trapdoor_mesh`：關閉（底部水平）／開啟（側邊垂直）。
- [ ] `block_aabb`：門／活板門薄板 AABB 各狀態精確 bounds。
- [ ] `resolve_collisions` 使用薄板 AABB；開啟狀態無碰撞。
- [ ] 紅石 toggle 更新 open bit 並重建 mesh。
- [ ] 右鍵開關門／活板門。
- [ ] 既有 placement collision、redstone door toggle、save/load 不回歸。
- [ ] `cargo fmt --all -- --check`、`cargo test --release`、`cargo check --release`。
- [ ] 人工：各角度查看門／活板門模型；開關碰撞；紅石控制；存檔重載方向保持。
