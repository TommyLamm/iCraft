# 20 — Plan01–17 回歸硬化

## 定位

- 優先級：P3，Plan15–17 完成後的全套 regression follow-up
- 前置條件：Plan15–17 foundation 已提交；Plan18/19 的權威與真驗收批次可並行
- 建議提交上限：2（結構／存檔語義、world-rule consumers）
- 本計劃只修復全套 debug/release 測試與 review 已證明的既有契約缺口，不新增內容或相容格式。

## A. 結構 seed 的 debug/release 一致性

- [x] Dungeon 與其他結構的 loot seed 對負世界座標使用明確 wrapping／signed hashing；debug
  不得 overflow，release 也不得產生另一套結果。
- [x] 覆蓋負 chunk／region、邊界座標與相同輸入重跑，驗證不 panic 且 loot seed 決定性一致。
- [x] `dimension::tests::overworld_stronghold_contains_twelve_empty_frames` 與
  `structure::manager::tests::test_apply_structures_to_chunk` 在 debug/release 都通過。

## B. Plan15 世界 metadata 真實性

- [x] `world.meta` 缺失時，legacy metadata 從 `LevelData` 還原 seed、mode、Hardcore、版本與
  generation options；不得把已保存的 Hardcore 世界顯示成普通 Survival。
- [x] 無法從舊格式推導的欄位使用明確 migration default，不能覆蓋可取得的 runtime 規則。
- [x] 加入 legacy world-list／upgrade-backup round-trip 測試，包含 `LevelData.hardcore` 與
  `WorldRules.hardcore` 的相容來源。

## C. `mob_griefing` consumer 語義

- [x] `mob_griefing=false` 只禁止爆炸破壞、吃草等 mob 世界修改；不得凍結敵對／被動 AI、
  移動、戰鬥、掉落或計時。
- [x] Singleplayer/listen/dedicated 共用相同 rule snapshot；補測 AI 仍前進且 block mutation 為零。

## 驗收

- [x] `cargo fmt --all -- --check`、`cargo test --locked --no-fail-fast`、
  `cargo test --release --locked --no-fail-fast`、`cargo check --release --locked`、
  `git diff --check` 全通過。
- [x] Plan15、Plan10 對應 targeted tests 在 debug/release 均通過；沒有只在 `debug_assert!`
  中執行的 gameplay side effect。
