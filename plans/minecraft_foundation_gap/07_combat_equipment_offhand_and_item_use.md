# 07 — 戰鬥冷卻、裝備、副手、盾牌與蓄力使用

## 執行包

- 優先級：P1
- 前置條件：01、06 完成；04 的死亡掉落接口可後接但不可破壞
- 後續解鎖：11、15、16
- 建議提交上限：3
- 禁止順帶實作：全部武器、Crossbow、Trident、Mace 或 PvP 排位規則

## 目標

把目前「左鍵即滿傷害、右鍵 Bow 即滿速箭」提升為一致的 item-use 與裝備系統，加入
副手、盾牌、攻擊冷卻、蓄力弓、完整基礎護甲計算和多人權威命中。

## 現況問題

- Inventory 有 hotbar/main/4 armor，沒有 offhand。
- Bow 右鍵立即消耗箭並生成固定速度 projectile，沒有 draw/cancel/critical。
- melee 有傷害、擊退、附魔與耐久，但沒有 Java 式 attack strength cooldown。
- 只有 Iron armor item；減傷主要由 enchantment multiplier 表達。
- joined client 的方塊請求權威化較完整，玩家 combat request/ACK 仍需同級設計。

## 實作步驟

### A. 通用 Item Use 狀態機

- [ ] 新增 `ItemUseAction`（Eat/Drink/Block/Bow）與 `UsingItemState`。
- [ ] press/start、hold/tick、release/finish、cancel 四階段分離；UI gate、切 slot、死亡、切維度取消。
- [ ] main hand 不能使用時按明確優先級嘗試 offhand；兩手同時不得雙重消耗。
- [ ] 保存不持久化正在使用狀態；網路同步 pose 和完成結果。

### B. 副手與裝備

- [ ] Inventory/Data/Wire/UI 增加 offhand slot，所有關閉／死亡／存檔守恆測試更新。
- [ ] 補木／金工具和至少 Leather/Gold/Diamond armor；Netherite 可列為後續內容。
- [ ] 資料化 armor points、toughness、knockback resistance、durability 和 equip slot。
- [ ] 傷害管線固定順序：難度／來源→armor/toughness→enchantment→absorption/effect→health。
- [ ] 裝備受耐久、破損、Unbreaking；Creative 不耗損。

### C. 攻擊與盾牌

- [ ] 加 attack cooldown meter；傷害、擊退、橫掃資格按充能比例計算。
- [ ] raycast 命中用 entity AABB/LOS/距離，host 以已認證玩家姿勢重驗。
- [ ] Shield 按住格擋前方可格擋來源；爆炸／投射物／近戰規則資料化。
- [ ] Axe 可暫時禁用 Shield；盾承受耐久並有音效／第一人稱姿勢。
- [ ] 友軍傷害首版由 world rule 控制，預設與現有行為兼容。

### D. 弓與投射物

- [ ] Bow draw 0..full，release 決定速度、傷害和散布；不足最小時間不發射。
- [ ] 箭優先從 offhand，再從 inventory 消耗；Infinity 保持至少一箭的明確規則。
- [ ] host 生成 projectile，client 只播放預測動畫；拒絕時回滾手勢而不複製箭。
- [ ] 箭命中方塊可回收／超時清理，命中 entity 的 damage owner 正確記錄。

## 主要文件

- `src/inventory.rs`、`src/player.rs`、`src/state.rs`、`src/entity.rs`
- `src/enchantment.rs`、`src/mob.rs`、`src/physics.rs`、`src/hand_renderer.rs`
- `src/save.rs`、`src/network/*`、`src/audio.rs`、`src/mob_renderer.rs`

## 驗收

- [ ] offhand 保存、同步、shift-click、死亡掉落與游標操作守恆。
- [ ] attack cooldown 0/50/100% 的傷害與擊退邊界測試。
- [ ] Shield 的前／後／側面、禁用、耐久破損、projectile owner 分支。
- [ ] Bow 快點、半蓄、滿蓄、取消、Infinity、無箭、切 slot。
- [ ] Host/Client 延遲與重複 combat request 不造成雙傷害或雙耗耐久。
- [ ] 既有 Boss、mob death reward、Fire Aspect、Looting、Power 測試不回歸。

## 完成閘門

戰鬥結果必須只由 host 結算；若 client 可以直接改 entity health、箭數或盾耐久，本計劃
不得標完成。

