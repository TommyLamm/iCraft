# Plan03 — 刪除 `mob` 與 `passive_mob` 遺留渲染器更新循環

## 定位

在早期架構中，生物實體狀態更新由桌面端渲染主循環驅動（`update_mobs` 與 `update_passive_mobs`）。
在權威化完成後，20Hz 固定刻度下的生物移動、AI、攻擊、擊退、受傷、掉落與繁殖均由 `ServerWorld::tick_entities` 統一進行（見 `ARCHITECTURE.md` §3）。

目前殘留在 `src/mob.rs` 與 `src/passive_mob.rs` 中的舊更新循環：
- `src/mob.rs:L293-490`（198 行）：`update_mobs`
- `src/passive_mob.rs:L41-323`（283 行）：`update_passive_mobs`

僅由 `legacy_sim.rs`（桌面遺留模擬）與模組內部的遺留單元測試調用。活體 Singleplayer、Listen Host、Join Client 及 Dedicated Server 均不走此路徑。

## 前置

無。可與 01、02、04–06 並行。

## 精確 acceptance

- [ ] 移除 `src/mob.rs` 中的 `update_mobs` 函式及依賴其運作的遺留單元測試（或遷移至權威測試）。
- [ ] 移除 `src/passive_mob.rs` 中的 `update_passive_mobs` 函式及相關遺留更新輔助。
- [ ] 若 `src/presentation/legacy_sim.rs` 調用了這兩個函式，替換為空實現或直接移除該調用。
- [ ] 保持 `src/mob.rs` 中的 `spawn_mobs`、`Mob` 定義與渲染屬性完整。
- [ ] 保持 `src/passive_mob.rs` 中的 `spawn_passive_mobs`、`PassiveMob` 定義與渲染屬性完整。
- [ ] `cargo check --all-targets` 通過。
- [ ] 權威端生物模擬與集成測試全數通過。

## 預計檔案與測試

- 修改：
  - `src/mob.rs`
  - `src/passive_mob.rs`
  - `src/presentation/legacy_sim.rs`（若有引用）
- 驗證測試：
  - `cargo test --lib mob::`
  - `cargo test --lib passive_mob::`
  - `cargo test --lib server_world::`
  - `cargo check --all-targets`

## 建議階段

1. 檢查 `update_mobs` 與 `update_passive_mobs` 的全庫調用點。
2. 刪除 `src/mob.rs` 中 `update_mobs` 及相關舊單元測試。
3. 刪除 `src/passive_mob.rs` 中 `update_passive_mobs` 及相關舊單元測試。
4. 清理 `legacy_sim.rs` 中的調用點。
5. 運行編譯與單元測試。

## 不在本計劃

- 修改 `ServerWorld::tick_entities` 的生物更新邏輯。
- 變更生物渲染插值（`mob_renderer.rs`）。
