# Plan27 — 巨檔機械切分

## 定位

純導航與編譯隔離；**零邏輯變更**。在 01–06 把死碼刪完後做，避免搬動即將刪除的程式。

### 權威／runtime

| 檔 | 規模 | 叢集（約略行號） |
| :--- | :--- | :--- |
| `server_world.rs` | ~3,254 行，測試從 2250 起 | 1–450 ctor／chunks／restore／save payload／eviction；454–790 container viewers／hook／drops／LOS；793–1570 `set_block`／place／fluid／validate／trade／mount／open-close／sleep；1574–1691 `tick`；1698–2248 dispense／furnaces／entities／checksum |
| `server_runtime.rs` | ~3,107 行，測試從 1664 起 | 1–750 `TransportMode`／queues／`RuntimePresentationEvent`／`ServerProperties`；761–930 `PlayerSessionState`；933–1530 construct／tick／save／console／eviction；1577–1648 inventory conversion |
| `authority/dispatch.rs` | ~1,492 行，無測試 | 15–198 `submit_request`；288–620 block action；622–857 fluid／fishing／container click；859–1009 workstation；1012–1205 combat；1212–1338 commands + reject |
| `authority/mod.rs` | ~450 行 API + **~1,450 行測試**（466–end） | 測試搬 `authority/tests.rs` |

### 網路

| 檔 | 規模 | 生產 vs 測試 |
| :--- | :--- | :--- |
| `network/server.rs` | 2,373 | 生產 1–290；**測試 291–2372（~2,080 行）** |
| `network/client.rs` | 2,744 | 生產 1–1210；測試 1212–2743（~1,530） |
| `network/protocol.rs` | 2,710 | 型別 1–1606（bounded decode 1–144；session wire 145–420；`GameplayOperation`／`Request` 421–870；`ItemWire`／`Action`／`Packet` 871–1606）；測試 1608–2709（~1,100） |

### 世界／玩法

| 檔 | 叢集 |
| :--- | :--- |
| `world/mesh.rs`（~3,000） | 1–68 `MeshVoxel`／`SectionHaloSnapshot`；69–703 faces／AO／特殊 emitter／`is_greedy_cube`；704–1290 greedy `mesh_l0_volume`；1411–1800 section-halo + LOD；~2040–end 測試（~960） |
| `world/block.rs`（~2,123） | 1–50 常數／`Biome`／`SoundMaterial`；155–281 enum；283–438 `BlockState`；440–725 wire／support／sound；726–1853 表（Plan 19 後縮）；1899–end 測試 |
| `redstone.rs`（~2,937） | 1–308 型別；310–678 snapshot／API；679–1135 tick／observers／settle；1137–1314 transitions／pistons；1316–1724 power helpers；~1725–end 測試（~1,200） |
| `inventory/catalog.rs`（~2,544） | Plan 21 後：`Item` enum + defs 表 |
| `boss.rs`（~1,494） | 1–132 型別；133–306 `ensure_*`；307–517 `update_dimension_entities`；518–912 dragon／enderman／wither；914–end HUD |

### 桌面

`state.rs`（~10,500 行）：`pub struct State` 2294–2563（~270 欄位）；`State::new` 2753–4055（**~1,300 行**）；authority 投影區 4067–5105（~1,000 行）；inventory／stations／UI layout 8074–8672（~600 行）+ `SlotType` 2566–2694；**inline 測試 271–2266 與 9441–10473（~1,800 行）**。`frame.rs`（~3,900）中 inventory／HUD encode ~1194–2600（~1,400 行）。

## 前置

01–06（死碼先刪）。建議 07／08／10 之後再切 `server_runtime.rs`／`client.rs`（enum 會大幅縮小）。

## 精確 acceptance

- [ ] 上表每個 `#[cfg(test)] mod tests` 超過 ~500 行者搬到同名 `*/tests.rs` 或 `tests/`；生產檔不再含巨型測試模組。
- [ ] `server_world.rs` → `server_world/{mod,columns,containers,mutation,tick,entities}.rs`；`server_runtime.rs` → 把 `ServerProperties`／`PlayerSessionState`／event 型別移到子檔；`dispatch.rs` → `dispatch/{mod,block_action,container,workstation,combat,command}.rs`。
- [ ] `protocol.rs` → `protocol/{decode,wire_types,gameplay,packet}.rs`；`client.rs`／`server.rs` 測試移出。
- [ ] `mesh.rs` → `mesh/{halo,faces,greedy,section}.rs`；`block.rs` → `block/{types,state,table}.rs`；`redstone.rs` → `redstone/{system,power,piston}.rs`；`boss.rs` → `boss/{dragon,wither,nether}.rs`。
- [ ] `state.rs`：`SlotType` + layout + slot get／set 移 `presentation/inventory_ui.rs`（無 wgpu）；`State::new` 的 GPU 半段移 `bootstrap.rs`；authority 投影區移 `presentation/authority_projection.rs`；inline 測試移 `presentation/tests/`。
- [ ] `pub use` 保持對外路徑不變（`use crate::server_world::ServerWorld` 等仍成立）。
- [ ] `git diff --stat` 顯示幾乎全是移動；`cargo test` 數量不變。

## 預計檔案與測試

- 改：上表全部；`src/lib.rs`／`main.rs` `mod` 路徑
- 驗證：`cargo check --all-targets`；`cargo test`（總數與切分前一致）；`cargo test --bin icraft-server`

## 建議階段

1. 測試模組搬出（最大、零風險）。
2. 網路三檔。
3. 權威三檔。
4. 世界／玩法。
5. `state.rs`（一次一個叢集）。

## 不在本計劃

- 任何行為或簽名變更。
