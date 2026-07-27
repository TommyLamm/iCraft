# 實作計畫 13：門／活板門方向與薄碰撞 Block-State

> 來源：Task 10 後續項 W1。

## 狀態

⏳ 待實作

## 目標

門與活板門擁有正式的面向（facing）、半身（half，門頂／底）、鉸鏈
（hinge，左／右）與開啟狀態，正確的薄板碰撞 AABB 與專用立體模型，取代
目前的通用 1×1×1 完整 cube。

## 已確認根因

### 方塊儲存

- `Chunk.blocks` 只儲存 `BlockType`（u8 enum），**無** per-block metadata
  （`src/world.rs:1692`）。
- `OakDoor`/`OakDoorOpen`/`OakTrapdoor`/`OakTrapdoorOpen`（discriminant
  67-70）是四個獨立 enum 變體，只編碼 open/closed 二元狀態
  （`src/world.rs:323-326`）。
- **沒有** facing、half（門的上下半身）、hinge（左右鉸鏈）資料。

### 模型

- `is_greedy_cube`（`src/world.rs:1664`）：`OakDoor`（closed, is_solid=true）
  走 greedy meshing 生成完整 1×1×1 cube；`OakDoorOpen`（is_solid=false）
  走 per-block BLOCK_FACES 也是完整 cube。
- **沒有** `append_door_mesh` / `append_trapdoor_mesh` 特殊路徑。
- 所有六面使用同一個 atlas tile（門 `(9,14)`、活板門 `(10,14)`），無
  top/bottom/side 區分（`src/world.rs:1337`）。

### 碰撞

- `resolve_collisions`（`src/physics.rs:300`）只檢查 `is_solid`，使用
  `unit_block_aabb`（完整 1×1×1 cube）。
- `block_placement_decision`（`src/physics.rs:70`）：non-solid（OakDoorOpen）
  一律允許；solid（OakDoor）用完整 cube 做 player overlap 檢查。
- **沒有**薄板 AABB 支援。

### 紅石

- 門已是 redstone component（`src/redstone.rs:1021`），`ComponentState` 有
  `facing: Direction` 欄位（`src/redstone.rs:99`），placement 時由
  `Direction::from_yaw(camera.yaw)` 捕獲。
- facing 透過 `redstone_metadata` sidecar 持久化（G3 修復），且 open/closed
  切換時 `reconcile_mutations` 用 `or_insert_with` 保留既有 facing。
- **但** model 與碰撞完全不使用此 facing。

### 放置

- 玩家放置門只 `set_block(OakDoor)`（`src/state.rs` handle_click 放置路徑），
  不設 facing、不放上半身、不判斷鉸鏈。

## 實作步驟

### 階段一：Block-state 儲存

1. 在 `src/world.rs` 定義 `BlockState` 結構體與 u8 編碼：
   ```text
   bits [1:0] = facing (North=0, South=1, West=2, East=3)
   bit  2     = half  (0=bottom, 1=top)   -- 門專用
   bit  3     = hinge (0=left, 1=right)   -- 門專用
   bit  4     = open  (0=closed, 1=open)  -- 活板門專用
   bits [7:5] = reserved
   ```
   提供 `BlockState::encode/decode` 及 default（facing=North, bottom, left,
   closed）。
2. 在 `Chunk` 新增 `pub block_states: Box<[[[u8; 16]; 256]; 16]>`（每格 1
   byte，預設 0）。只有具方向的方塊（門、活板門，未來可擴充樓梯、柵欄等）
   會使用非零值。
3. 更新 `Chunk::new` 初始化 `block_states`。
4. `ChunkManager` 新增 `get_block_state` / `set_block_state` accessor。

### 階段二：存檔與網路同步

5. `src/save.rs`：`ChunkSaveData` 新增 `block_states: Vec<u8>`（Zlib 壓縮）。
   舊存檔（無此欄位）反序列化時 fallback 為全零陣列。更新
   `deserialize_chunk_save_data` 的 legacy fallback。
6. `src/network/protocol.rs`：`BlockChange` 新增 `state: u8` 欄位（協議升
   v5）。`ChunkData` payload 末尾附加 `block_states` 壓縮區段；client 解
   時若長度不足視為全零。
7. `src/network/server.rs` / `client.rs`：relay 與 apply 時傳遞 `state`。
8. `src/state.rs`：`apply_synced_block_change` / `apply_remote_block_change`
   / `set_block_and_broadcast` 都帶上 `state`；`ChunkManager::set_block` 同
   步更新 `block_states`。

### 階段三：放置邏輯

9. `src/state.rs` handle_click 放置路徑：放置門時：
   - 從 `camera.yaw` 算 `Direction::from_yaw`，設為 facing。
   - 在目標格放 bottom half (`OakDoor`)，在上方格放 top half（同樣
     `OakDoor`，block_state 的 half bit = 1）。
   - 若上方格不可放置（已佔用或超出 256），拒絕。
   - 鉸鏈（hinge）依相鄰方塊決定：預設 left；若 facing 左側有實心方塊而右側
     沒有，改為 right（vanilla 規則）。
   - 設定兩格的 `block_state`。
10. 放置活板門時：facing 從 yaw 推導，open bit = 0（關閉）。
11. `can_place_block_with_support` 不變（門仍不要求支撐，懸空合法）。

### 階段四：模型

12. 在 `src/world.rs` 新增 `append_door_mesh`：
    - 讀取 block_state 的 facing/half/open。
    - 關閉：薄板（3/16 厚度）貼在方塊格的 facing 側邊緣。
    - 開啟：薄板旋轉 90° 貼在鉸鏈側。
    - 上半身與下半身使用不同 atlas UV（門上半／下半紋理不同；若 atlas 只有
      一塊門紋理，可上下重複或均分）。
    - 所有頂點 AO 1.0，以來源格 sky/block light 打包（與 torch 同策略）。
13. 新增 `append_trapdoor_mesh`：
    - 關閉：薄板（3/16 厚度）平貼方塊格底部。
    - 開啟：薄板旋轉至垂直，貼 facing 側。
14. 更新 `is_greedy_cube` 排除所有門／活板門變體，使其走特殊路徑。
15. `generate_mesh` 在 torch/cross-model 之後、generic cube 之前加入
    `OakDoor | OakDoorOpen => append_door_mesh`、
    `OakTrapdoor | OakTrapdoorOpen => append_trapdoor_mesh` 分支。
16. `generate_mesh` 的 closure 需能取得鄰居的 block_state。擴充
    `MeshSnapshot` / closure 回傳型別，加入 `u8 block_state` 欄位。

### 階段五：碰撞

17. `src/physics.rs` 新增 `block_aabb(block, block_state, pos) -> AABB`：
    - 門關閉：薄板 AABB（facing 側，3/16 厚度）。
    - 門開啟：薄板 AABB（鉸鏈側，3/16 厚度）。
    - 活板門關閉：薄板 AABB（底部，3/16 厚度）。
    - 活板門開啟：薄板 AABB（facing 側垂直，3/16 厚度）。
    - 其他方塊：回退 `unit_block_aabb`。
18. `resolve_collisions` 改用 `block_aabb` 取代 `unit_block_aabb`。
19. `block_placement_decision` 同樣改用 `block_aabb`。
20. 開啟狀態（`OakDoorOpen`/`OakTrapdoorOpen`）的 `is_solid` 改為
    `false`（已經是），碰撞自然跳過。

### 階段六：紅石整合與收尾

21. `src/redstone.rs` toggle 門／活板門時，更新 block_state 的 open bit，
    不只 swap BlockType。`set_block_record` 需帶上 state。
22. 右鍵互動開關門：在 `handle_click` 的 redstone interact 分支加入
    OakDoor/OakTrapdoor，toggle open bit 並更新 mesh。
23. 更新 `ARCHITECTURE.md`（方塊儲存、模型、碰撞、紅石段落）、`track.md`、
    `plans/progress.md`、`plans/implementation/10_bug_audit.md` 的 W1 狀態。

## 驗證

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
- [ ] `cargo fmt --all -- --check`、`cargo test --release`、
      `cargo check --release`。
- [ ] 人工：各角度查看門／活板門模型；開關碰撞；紅石控制；存檔重載方向
      保持（需互動式視窗）。

## Commit

單一功能 commit：`feat(blocks): door and trapdoor facing, thin collision, and proper models`
