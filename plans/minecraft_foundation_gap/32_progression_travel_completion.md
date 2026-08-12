# Plan32 — authoritative progression travel and completion

## 定位

- 這是 Plan30 驗收矩陣留下的後續缺口；本計劃已於 2026-08-12 完成。
- Plan30 只記錄現有 progression harness 的能力，不以 direct dimension mutation、
  `set_block`、dragon/entity fixture 或 End City loot 來宣稱真玩家網路 E2E。

## 精確 acceptance

- [x] 以既有 typed gameplay/legacy compatibility seam 實作並驗證 portal activation、
  dimension transfer、權威 spawn/return 與 reconnect；request 必須經真 TCP ingress。
- [x] 對可表達的 fortress/End progression 建立最小 bounded vector：權威生成或既有
  內容互動、dragon completion、End City loot/save/reload，每一步都有 owner/interest
  projection、revision 與 duplicate/stale 行為。
- [x] Singleplayer、Listen TCP（local host + remote）與 Dedicated TCP（兩個 clients）
  共用 assertion；跨 dimension 的 session/private payload 不得洩漏給其他玩家。
- [x] 缺少 canonical content 或 wire operation 時，先補最小 typed seam 並留下
  protocol/backward-compatibility 記錄；不得以 direct core mutation 冒充玩家操作。

## 完成範圍

- Protocol v19 新增 typed portal ignition、Ender Eye insertion、portal entry 與
  owner-private dimension transfer；切換 dimension 時 transport/client revision namespace
  一併重設，避免跨世界 revision 誤判。
- `AuthorityCore` 是 portal cooldown/contact、linked portal mutation、combat damage、
  dragon completion、operator progression command 與 transfer intent 的唯一權威來源。
- 世界生成尊重 persisted world type/structure policy；generated Nether fortress 與固定
  End City chest 首次開啟才 materialize loot，並配置 revision、projection 與 save payload。
- 真 TCP vectors 驗證 cached duplicate ACK、stale revision rejection、owner-only transfer、
  observer interest privacy，以及 disconnect/reconnect 後維持 Nether dimension。
- Dedicated TCP dragon vector 只透過 typed command/pose/combat ingress 殺死生成的 dragon，
  再驗證 3x3 exit fountain、dragon egg 與 End gateway 的權威 completion mutations。

## 驗證

- Debug：`cargo test --test plan32_progression_travel -- --test-threads=1`（5 passed）。
- Release：`cargo test --release --test plan32_progression_travel -- --test-threads=1`
  （5 passed）。
- `cargo check --all-targets`、`cargo check --release --locked`、
  `cargo fmt --all -- --check` 與 `git diff --check` 通過。
- 詳細證據：`artifacts/plan32_20260812_verification.md`。

## 實作檔案與測試

- `src/network/protocol.rs`、`src/authority/contract.rs`、`src/authority/mod.rs`、
  `src/server_runtime.rs`、`src/server_world.rs`、`src/dimension.rs`、`src/state.rs`。
- `tests/plan32_progression_travel.rs` 的五個跨拓撲/內容 assertion。

## 不在本計劃

- 新世界內容、完整 vanilla structure/dragon AI、renderer/GPU/window/audio/DPI、
  30 分鐘 soak，以及與 progression blocker 無關的 transport metrics 修正。

