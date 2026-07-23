# 實作計畫 06：修復 Survival 攻擊怪物

## 目標

Survival 左鍵按下能先攻擊準星內最近怪物；沒有實體時仍維持長按挖掘，
不會變成單擊瞬間破壞方塊。

## 已確認根因

- App 只在 Creative press 時呼叫唯一含 melee 的 `State::handle_click(true)`。
- Survival held path 只 raycast block，從不 raycast entity。
- 不能直接移除 Creative guard，否則 Survival 會走 instant block break。
- mob death cleanup 以 `health >= 0` 保留，恰好 0 HP 的怪物可能不消失。

## 實作步驟

1. 從 `handle_click` 抽出 `try_melee_attack() -> bool`，封裝 target selection、
   傷害、無敵幀、擊退、火焰、loot/XP 與工具耐久。
2. 所有模式在 left press 先呼叫 melee；命中實體（即使仍在 invulnerability
   window）就消耗該次點擊，避免誤挖身後方塊。
3. Creative 未命中才走 instant break；Survival 未命中維持現有 held mining。
4. target filter 排除 RemotePlayer、DroppedItem、粒子與非攻擊投射物，只讓
   可戰鬥實體攔截。
5. 修正活體恰好 0 HP 的清理，同時不誤刪 max_health=0 的非活體 entity。
6. 保持目前 host-authoritative 範圍；若 joined client 沒有 authoritative mob
   replication，不做會產生分歧的 client-local 傷害。
7. 更新架構與進度文件。

## 驗證

- Survival press 命中怪物會傷害；miss 不會 instant-break block。
- Creative 命中優先於 block；miss 才破壞。
- 最近 target、距離、filter、invulnerability、knockback、loot/XP/durability。
- 恰好 0 HP 活體會移除，非活體 entity 保留原生命週期。
- `cargo fmt -- --check`、`cargo test --release`、`cargo check --release`。
- 人工空手/武器攻擊敵對與被動怪物。

## Commit

單一功能 commit：`fix(combat): enable survival melee attacks`

