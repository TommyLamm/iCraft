# Plan32 — authoritative progression travel and completion

## 定位

- 這是 Plan30 驗收矩陣留下的後續缺口；本計劃尚未開始。
- Plan30 只記錄現有 progression harness 的能力，不以 direct dimension mutation、
  `set_block`、dragon/entity fixture 或 End City loot 來宣稱真玩家網路 E2E。

## 精確 acceptance

- [ ] 以既有 typed gameplay/legacy compatibility seam 實作並驗證 portal activation、
  dimension transfer、權威 spawn/return 與 reconnect；request 必須經真 TCP ingress。
- [ ] 對可表達的 fortress/End progression 建立最小 bounded vector：權威生成或既有
  內容互動、dragon completion、End City loot/save/reload，每一步都有 owner/interest
  projection、revision 與 duplicate/stale 行為。
- [ ] Singleplayer、Listen TCP（local host + remote）與 Dedicated TCP（兩個 clients）
  共用 assertion；跨 dimension 的 session/private payload 不得洩漏給其他玩家。
- [ ] 缺少 canonical content 或 wire operation 時，先補最小 typed seam 並留下
  protocol/backward-compatibility 記錄；不得以 direct core mutation 冒充玩家操作。

## 預計檔案與測試

- `src/network/protocol.rs`、`src/authority/contract.rs`、`src/authority/mod.rs`、
  `src/server_runtime.rs`、必要的 `src/dimension.rs`。
- `tests/common/tcp_harness.rs`、`tests/plan32_progression_travel.rs`、
  `tests/runtime_topology_parity.rs` 的跨拓撲 assertion。

## 不在本計劃

- 新世界內容、完整 vanilla structure/dragon AI、renderer/GPU/window/audio/DPI、
  30 分鐘 soak，以及與 progression blocker 無關的 transport metrics 修正。

