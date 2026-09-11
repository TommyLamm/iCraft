# Plan24 — 跨模組 helper 合併：slot／鄰居／ray／座標／milli／RNG／parse_bool／spawn

## 定位

09 波 13 只收 `position_to_milli`／`PLAYER_REACH`。其餘重複家族：

| 家族 | 位置 | 備註 |
| :--- | :--- | :--- |
| `session_slot_from_stack` ×3 | `server_runtime.rs` 1559–1571（`Option<&ItemStack>`）；`state.rs` 4367–4379（owned，多 `count > u16::MAX` 檢查）+ `stack_from_session_slot` 4381–4385；`authority/dispatch.rs` 1347–1353（永遠 `Some`）；`tests/common/tcp_harness.rs` 354 | 同一 ItemWire + Adventure mask 映射；count cap 只在桌面路徑 |
| 六鄰居表 | `redstone.rs` `NEIGHBORS` 8–15、`Direction::delta` 85–94（無 `ALL`）；`lighting.rs` `LIGHT_DIRS` 5–12 **加** 32、104、168、238、388、490 六次內聯；`dimension.rs` 470–477 | `chunk_manager.rs` `OFFSETS` 206–214（7 含自身）與 `fluid.rs` `HORIZONTAL_DIRECTIONS` 是不同語意，保留 |
| 兩份 `ray_intersects_aabb` | `voxel_shape.rs` 140–178（`max_dist`、回 `(t, normal)`、平行軸安全）；`entity.rs` 1002–1046（slab，**除以 `dir.x/y/z`**，只回 `Option<f32>`）；caller `state.rs` 184、7802 | entity 版在軸對齊射線上不安全——真 bug |
| block↔chunk 座標無 helper | `div_euclid(16)` 在 `server_world.rs` **22 處** + interest／ingress／dimension／structure／presentation／tests；`>> 4` 在 `village/poi.rs` 111–123、`authority/portals.rs` 127–128；反向 `chunk_x * 16` 在 `structure/{placement,locate,manager,types}.rs`、`server_world.rs` 1966 | signed-Y 已集中（`section.rs` 10–18）；Chebyshev ring 已是 `within_unload_hysteresis`，**不要再造** |
| milli／quantize 其他 | `server_world.rs` `milli_to_vec3` 2242–2248；`server_runtime.rs` `scalar_to_milli`／`milli_to_scalar` 1548–1557；`dispatch.rs` `quantize_health` 1470–1476、`look_from_angles` 1455–1468 | NaN／clamp 政策各異 |
| hash／RNG | SplitMix 正本 `world_tick.rs` 45–59（`boss.rs` `mix64` 993 已包）；重複：`fishing.rs` 73–79；FNV-1a ×4：`redstone.rs` 1607–1611、`authority/tick.rs` 498–506、`network/client.rs` 1502–1508、2205（`sim_harness` 那份隨 Plan 01 消失）；LCG：`mob.rs` `ambient_spawn_rng` 120–132、`weather.rs` `next_random`、`texture.rs` painter closures | **不動** `worldgen::hash_coord`（82–94）與 `structure::hash_structure`（27–35） |
| `parse_bool` ×4 | `save/format.rs` `parse_meta_bool` 44–50；`menu.rs` 536；`server_runtime.rs` 710（`Result`，不 trim）；`icraft-server.rs` `parse_bool_flag` 264 | `"On"` vs `"on"` 三處不一致 |
| hostile／passive 生成同骨架 | `spawn_mobs`（`mob.rs` 134–189）與 `spawn_passive_mobs`（`passive_mob.rs` 5–66）：cap、`ambient_spawn_rng`、角度／距離、`highest_solid_y`、兩格空氣、`entity_manager.spawn` | 重力常數 32／8 在 `entity.rs` 577–581 與 619–623 兩份 |

## 前置

09 波 13（milli 模組已存在，本計劃往裡加）。

## 精確 acceptance

- [ ] `SessionInventorySlot::from_stack`／`to_stack` 在 contract 型別上一份；runtime／dispatch／State／tcp_harness 全改用；count cap 政策一致。
- [ ] `Direction::ALL` + `Direction::delta` 為六鄰居唯一來源；lighting 七處內聯刪；`dimension.rs` 470–477 刪。
- [ ] entity／state 改用 `voxel_shape::ray_intersects_aabb`（或薄 `Option<f32>` wrapper）；`entity.rs` 1002–1046 刪；新增軸對齊射線測試。
- [ ] `world::chunk_xz(x,z)`／`local_xz`／`chunk_origin(cx)` 三個 helper；`server_world.rs` 22 處 + POI／portals `>> 4` 全改。
- [ ] `milli_to_vec3`／`scalar_to_milli`／`milli_to_scalar`／`quantize_health` 併入 09 波 13 的 milli 模組，統一 NaN／clamp。
- [ ] `fnv1a` 一份、fishing 用 `next_splitmix64`；`mob`／`weather`／`texture` LCG 用同一 `rng.rs`（fishing bite 分佈測試若依賴常數則保留其種子混法）。
- [ ] `parse_bool_flag` 一份（建議 `game_rules` 或 `save/format`）。
- [ ] `try_ambient_spawn(table, light_rule, dist_range)` 一份；重力常數一份。
- [ ] 相關單元測試全綠；spawn 密度／物種比例測試不變。

## 預計檔案與測試

- 改：`src/authority/{contract,dispatch,tick,portals}.rs`、`src/server_runtime.rs`、`src/state.rs`、`tests/common/tcp_harness.rs`、`src/redstone.rs`、`src/lighting.rs`、`src/dimension.rs`、`src/voxel_shape.rs`、`src/entity.rs`、`src/world/mod.rs`、`src/server_world.rs`、`src/village/poi.rs`、`src/structure/*.rs`、`src/fishing.rs`、`src/network/client.rs`、`src/mob.rs`、`src/passive_mob.rs`、`src/weather.rs`、`src/save/format.rs`、`src/menu.rs`、`src/bin/icraft-server.rs`
- 驗證：`cargo test --lib`；`tests/review_hardening_session_lifecycle.rs`；`tests/review_hardening_container_click.rs`；`tests/passive_mob_tests.rs`；`tests/plan33_tcp_fishing_lifecycle.rs`

## 建議階段

1. 座標 helper（機械替換，最多檔案）。
2. 鄰居表、ray、milli。
3. slot mapper。
4. RNG／FNV／parse_bool。
5. spawn 合併。

## 不在本計劃

- `thiserror`（README §6）。
- worldgen／structure hash（README §5）。
