# Plan27：半磚 Waterlogging 權威閉環

## 範圍與前置條件

本計劃只處理既有 `OakSlab`／`CobblestoneSlab` 的 waterlogging。`BlockState`
的 bit 7 保持 reserved；waterlogging 使用 Chunk fluid raw byte 的 bit 7。
現有 fluid engine、ChunkSection raw fluid array、save v3、authority request
sequence/revision gate、mesh halo 與 translucent pass 是前置條件，不重做。

Eligible set 僅為：

- `BlockType::OakSlab`
- `BlockType::CobblestoneSlab`

Stairs、trapdoors、fences/gates/walls/panes/ladders/signs/doors/hoppers、lava、
cauldrons、dispensers、swimming/oxygen 及其他原版 waterloggable 行為不在本計劃。

## Canonical wire/save contract

Chunk fluid raw byte 的 layout 固定為：bits `0..=2` fluid level、bit 3 falling、
bits `4..=6` reserved、bit 7 `WATERLOGGED`。既有 level/falling API 保持遮罩
語義；新增 raw getter/setter 與 eligible-host `is_waterlogged`。置入時將 level/
falling canonicalize 為零並保留 reserved bits；清除 bit 7 不可殘留低位流體狀態。
Malformed/non-eligible bit 7 不得成為 fluid source。

Save format 不升版：`ChunkSaveData::data_version = 3` 已持久化 raw
`fluid_levels`，舊資料 bit 7 為零並照舊讀取。`BlockState` bit 7 regression test
固定其 reserved 語義。

跨線新增 `WorldMutation.raw_fluid`、`BlockChange.raw_fluid`、`ChunkData.fluid_levels`
及對應 embedded/socket projection。新增 typed `GameplayOperation::FluidUse` 後
protocol 由 v16 升至 v17；v16 clients 在 handshake 被拒絕，不以 state bit 7 或
隱藏 serde 欄位模擬相容。

## 實作階段

1. **Raw codec/save + fluid carrier**
   - world/chunk_manager constants and raw API。
   - `OakSlab`/`CobblestoneSlab` eligibility。
   - Waterlogged solid source helper；fixed tick 保留 slab、可向 Air 流，跨
     chunk queue 正常工作。
   - fluid mutation 即使 block type 不變也輸出 raw fluid，供 authority revision
     與後續 replication 使用。
2. **Authority bucket transaction**
   - `FluidUse { x, y, z, face, hand, source }` 使用 exact `SlotRefWire`。
   - 驗證 authenticated session、dimension、range、合法 face、selected hand/source。
   - WaterBucket 對 eligible slab 設 bit 7；合法鄰格 Air 放 Water source；Bucket
     取 Water source 或清除 waterlogged slab。WaterBucket↔Bucket、容量與世界變更
     必須 atomic；duplicate/stale/invalid request 不得二次消耗。
3. **v17 wire/runtime/client/state**
   - BlockChange/ChunkData/host/runtime/presentation/client/state 全鏈路傳 raw fluid。
   - apply order 為 block → state → raw fluid；revision/interest stale gate 不變。
   - initial chunk payload 使用已壓縮 save fluid bytes；空/舊 payload default 為零。
4. **Slab mesh/invariants**
   - waterlogged bottom/top slab 只產生對應互補半格的 translucent water surface，
     不繪製與 solid slab 重疊的內部面；使用既有 halo/atlas/translucent pass。
   - toggle invalidates owner、鄰接及跨 chunk/section mesh；collision、selection、
     occlusion shape 與 sky/block light 數值保持 host slab 語義。
5. **Headless verification/docs**
   - 新增 `tests/waterlogging_authority.rs`，並擴充 protocol/client/server、fluid、
     save、world/mesh、headless/topology tests。
   - 更新本文件、README #27、ARCHITECTURE ownership/protocol/save/authority flow。

## 完成閘門

- [x] raw bit7 codec、legacy v3 save round-trip、BlockState bit7 reserved。
- [x] eligible/ineligible gate；waterlogged slab source/tick/跨 chunk flow；raw
      mutation 在同 block level/falling 改變時仍有 revision event。
- [x] FluidUse exact source/selected hand/range/face validation；WaterBucket↔Bucket
      atomic；duplicate/stale/idempotent vectors。
- [x] v17 handshake old-client rejection；BlockChange/ChunkData raw round-trip；
      embedded/listen/dedicated projection 與 stale/latest-wins。
- [x] slab complement translucent mesh；owner/neighbor invalidation；collision/
      voxel/light numeric invariants。
- [x] `cargo fmt --all -- --check`、targeted tests、`cargo check --all-targets`、
      debug/release suites、`git diff --check`。

## 驗證紀錄

目前已完成的窄閘門：

- `fluid::tests` 6、`chunk_manager::tests` 15、`block_model::tests` 5、
  `server_world::tests` 11。
- `network::protocol` 28、`network::client` 17、`network::server` 35、
  `server_runtime` 14。
- `authority_gameplay_domains` 3、`authority_persistence` 3、
  `headless_server_authority` 1、`runtime_topology_parity` 5、
  `waterlogging_authority` 5。

上述測試覆蓋 raw byte 與 v3 save、跨 chunk fixed tick、FluidUse 原子背包／世界
變更與 stale/duplicate、v17 raw packet/舊 handshake reject，以及 client revision
gate 的最新 raw fluid 投影。最終 full-suite `cargo test` debug（lib 676 passed/3 ignored、
binary 807 passed/3 ignored、其餘 lanes 2/3/3/3/1/1/5/3）、release（同樣
676/807 與 lanes）均通過；review regression 後窄閘門
`waterlogging_authority` 5、`chunk_manager` 15、`block_model` 5 及
`cargo check --all-targets`、`cargo fmt --all -- --check`、`git diff --check`
亦通過。GPU/window/audio/DPI、
完整原版 waterlogging parity 與 30 分鐘 soak 仍是本計劃明確 non-goal。

GPU/window/audio/DPI/manual Host+Join visual、GPU soak、完整 vanilla waterlogging
parity 明確不在本計劃；不得以 headless 結果宣稱其完成。
