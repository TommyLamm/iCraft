# Plan02 — 刪桌面 `State` 上無權威的玩法殼

## 定位

`ServerWorld::tick`（`server_world.rs` 1574–1691）只 tick redstone／hoppers／fluids／random ticks／furnaces／entities／boss。下列型別只在 `State` 與 `sim_harness` 建構，權威從不進入，投影也不會改它們：

| 殼 | 宣告／建構 | live 讀取 | lib 規模 |
| :--- | :--- | :--- | :--- |
| `MapManager` | `state.rs` 2380 / 3865 | **0**（`create_map`／`update_explored_pixels` 只在 `navigation.rs` 測試） | `navigation.rs` ~195 行 |
| `PoiManager` | `state.rs` 2543 / 4010 | **0**（`register_poi`／`claim_poi` 無 caller） | `village/poi.rs` ~390 行 |
| `RaidManager` | `state.rs` 2545 / 4012 | **0**（`start_raid`／`tick` 無 caller） | `village/raid.rs` ~190 行 |
| `MountManager` | `state.rs` 2378 / 4546–4554 | 只 `sim_harness` | `vehicle.rs` ~340 行 |
| `MinecartState::tick` | — | 只 `sim_harness.rs` 1098–1159 | `rail.rs` ~245 行 |
| `FishingManager`（presentation） | `state.rs` 2379 / 4563–4578 / 6632 | 與 `authority/fishing.rs` 平行；HUD 應只看投影 hook 實體 | `fishing.rs` 151 起 ~150 行 |
| `ContainerSessionManager` | `state.rs` 2485 / 3963；`network_event.rs` 64、101 只 `clear`／`close_by_player`，從不 `open` | 權威已有 `ServerWorld.container_viewers`（59, 725+）與 `InterestSet.open_containers`（`interest.rs` 70） | `container_sessions.rs` 整檔，且是 `lib.rs:35` 的 **server 契約** |
| `merchant_sessions` | `state.rs` 2544 / 4011 | 只 `close_sessions_for_villager`（8604）；`open_session` 只在 `sim_harness` | — |
| `last_gameplay_response` | `network_event.rs:12` 寫、`state.rs:4003` 初始化 | **從未讀取** | — |
| 死 timer：`water_tick_timer`／`lava_tick_timer`／`boss_maintenance_timer`／`wither_effect_timer`／`wither_damage_timer` | `state.rs` 2433–2434、2512、1313–1317、1386–1390、3908–3909、6286 | 只 init／reset／強制歸零；從不遞增 | — |
| `open_merchant_trade_window` 本地 `generate_offers_for_level` | `state.rs` 8514 | GPU 執行緒生成 server-owned 交易 | — |

## 前置

無（Plan 01 先落地會少一個 `sim_harness` 例外，但不是硬前置：可先把殼改 `cfg(test)` 再刪）。

## 精確 acceptance

- [x] 上表 `State` 欄位、建構、dimension-switch reset、disconnect 清理全部刪除。
- [x] `navigation.rs`、`village/poi.rs`、`village/raid.rs`、`vehicle.rs`、`rail.rs` 的 manager／tick 型別刪除或改 `#[cfg(test)]`；保留 `VillagerProfession`／`TradeOffer`／`EntityType::{Minecart,Boat}` wire。
- [x] `container_sessions` 從 `lib.rs` server 契約移除（刪檔或改 `pub(crate)` + `cfg(test)`）。
- [x] presentation `FishingManager` 刪除；cast／reel HUD 只讀投影 hook 實體與 session overlay。
- [x] 商人 UI 只顯示投影 offers；不再本地 `generate_offers_for_level`。
- [x] `cargo check --bin icraft-server` 通過且不再編 `container_sessions`／`poi`／`raid`／`vehicle`／`rail` manager。
- [x] `ARCHITECTURE.md` Code map 拿掉 `container_sessions.rs`，Gameplay 列調整。

## 預計檔案與測試

- 改：`src/state.rs`、`src/presentation/network_event.rs`、`src/presentation/bootstrap.rs`、`src/lib.rs`、`src/navigation.rs`、`src/village/{poi,raid,mod}.rs`、`src/vehicle.rs`、`src/rail.rs`、`src/fishing.rs`、`src/container_sessions.rs`
- 驗證：`cargo check --all-targets`；`cargo test --bin icraft`；`cargo test --lib village:: vehicle:: rail:: fishing::`；`tests/plan33_tcp_fishing_lifecycle.rs`（走 authority，應不受影響）；`tests/review_hardening_container_click.rs`

## 建議階段

1. 先刪 `last_gameplay_response` 與五個死 timer（零風險 warm-up）。
2. 刪 `MapManager`／`PoiManager`／`RaidManager`／`MountManager` 欄位，再處理 lib 側型別。
3. 刪 `ContainerSessionManager`／`merchant_sessions`，把 `network_event.rs` 的 close 呼叫換成直接關 UI 狀態。
4. 刪 presentation `FishingManager` 與商人本地 offers。

## 不在本計劃

- Q-drop、環境傷害、`switch_dimension` worldgen、join 本地模擬（Plan 03）。
- presentation `RedstoneSystem` 空殼（09 波 05）。
- weather／advancements 桌面化（Plan 06）。

## 實作與證據

### 改了什麼

- **State**：刪除 `map_manager`／`poi_manager`／`raid_manager`／`mount_manager`／`fishing_manager`／`container_sessions`／`merchant_sessions`／`last_gameplay_response` 與五個死 timer；投影掛鉤改為 `presented_fishing_hook_entity: Option<u64>`（來自 session overlay `gameplay.fishing_hook`）；商人窗只 clone `entity.offers`，不再呼叫 `generate_offers_for_level`。
- **network_event**：`GameplayResponse` 不再寫入；disconnect／PlayerLeave 不再碰 presentation container sessions（直接 `force_close_inventory`／關 UI）。
- **lib 契約**：`container_sessions` 改 `#[cfg(test)] mod`（退出 server 契約）；`vehicle`／`rail`／`village::raid` 與 POI／Map／MerchantSession／Fishing managers 改 `cfg(test)`；保留 `VillagerProfession`／`TradeOffer`／`FishingHookStage` 與 authority fishing helpers。
- **ARCHITECTURE.md**：Code map 拿掉 `container_sessions.rs`；註明 presentation shells 為 test-only。

### 死路徑證據（刪前／後）

- 刪後 `rg`：State 上已無 `map_manager|poi_manager|raid_manager|mount_manager|fishing_manager|container_sessions|merchant_sessions|last_gameplay_response|water_tick_timer`。
- `create_map`／`register_poi`／`MountManager::`／`FishingManager::`／`ContainerSessionManager::`／`open_session` 僅剩各模組 `#[cfg(test)]` 測試（及 `cfg(test)` 型別本體）。
- `generate_offers_for_level` 僅剩 `village/trade.rs` 測試呼叫，State 無引用。

### 測了什麼

- `cargo check --all-targets`：通過
- `cargo check --bin icraft-server`：通過（managers 不進 production 編譯單元）
- `cargo test --bin icraft`：187 passed
- `cargo test --lib village::`：7 passed；`vehicle::` 3；`rail::` 2；`fishing::` 8；`container_sessions::` 5
- `cargo test --test plan33_tcp_fishing_lifecycle`：3 passed
- `cargo test --test review_hardening_container_click`：9 passed

### 留下的缺口

- `generate_offers_for_level` 仍在 lib（僅測試用）；權威尚未有 live villager offer 生成路徑時，投影 offers 可能為空，商人 UI 會開空表。
- `mounted_entity` session overlay 仍投影到 State，但不再寫入本地 `MountManager`（渲染若需坐姿，應之後直接讀 entity／overlay，不在本計劃）。
- Plan 03 的 leftover 本地突變（Q-drop、環境傷害、`switch_dimension` worldgen 等）未動。
