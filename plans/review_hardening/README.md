# iCraft 架構審查後續 — 硬化路線

> 來源：2026-08-16 全專案五軸 review（對照 `ARCHITECTURE.md`）。
> 代碼基線：`tommy-dev`（審查當日 `ARCHITECTURE.md` 標為 `b3912c6`）。
> 這不是新的 Minecraft 功能缺口，而是**已宣稱契約與現役適配器不一致**的修復路線。

## 1. 怎麼用

一次只執行一份編號計劃。把 [prompt.md](prompt.md) 裡的 `{{PLAN_FILE}}` 換成該檔路徑。
完成並留下窄測試證據後，另開任務再做下一份。

依賴只在各計劃的「前置」寫明。沒寫前置的計劃可以與其他 P 同級項並行。

## 2. 優先級

| 級 | 意思 | 計劃 |
| --- | --- | --- |
| P0 | 專用伺服器對不信任網路開放前必須關掉 | 01–05 |
| P1 | 現役單人／LAN 會丟物品、地形或不同步 | 06–10 |
| P2 | 效能、結構、驗證契約；不阻塞 LAN 遊玩 | 11–15 |

## 3. 執行包索引

| # | 單獨執行文件 | 優先 | 狀態 | 前置 |
| --- | --- | --- | --- | --- |
| 01 | [關閉 BlockUse 任意改方塊](01_close_block_use_mutation.md) | P0 | 已完成 | 無 |
| 02 | [容器點擊改為 session 守恆交易](02_container_click_conservation.md) | P0 | 已完成 | 無 |
| 03 | [有界 bincode 解碼與對抗性 TCP](03_bounded_bincode_decode.md) | P0 | 已完成 | 無 |
| 04 | [握手身份與 online-mode](04_handshake_identity.md) | P0 | 已完成 | 無 |
| 05 | [Chunk restore 失敗即失敗](05_fail_closed_chunk_restore.md) | P0 | 待執行 | 無 |
| 06 | [Embedded 表現層停止改世界](06_embedded_presentation_no_mutation.md) | P1 | 待執行 | 01、02 |
| 07 | [Join client 只吃投影](07_join_client_projection_only.md) | P1 | 待執行 | 無 |
| 08 | [權威 session 生命週期](08_authority_session_lifecycle.md) | P1 | 待執行 | 無 |
| 09 | [Signed-Y 殘留收斂](09_signed_y_completion.md) | P1 | 待執行 | 無 |
| 10 | [礦脈、結構 cache、基岩與樹冠](10_worldgen_ore_structure_floor.md) | P1 | 待執行 | 無 |
| 11 | [Interest 驅動 chunk 駐留](11_interest_chunk_residency.md) | P2 | 待執行 | 無 |
| 12 | [網路入口背壓與可靠容器廣播](12_network_ingress_backpressure.md) | P2 | 待執行 | 02 |
| 13 | [持久化雙寫、inflate 與 symlink](13_persistence_integrity.md) | P2 | 待執行 | 04、05 |
| 14 | [桌面 hitch、surface 與 lib 樹](14_desktop_runtime_hygiene.md) | P2 | 待執行 | 無 |
| 15 | [測試契約硬化](15_verification_contract.md) | P2 | 待執行 | 01、02 至少已合併 |

## 4. 與其他路線的關係

- `plans/minecraft_foundation_gap/` 01–34 是玩法閉環。本路線修的是那些計劃**已宣稱完成、但現役適配器仍繞過**的洞。
- 不要把本路線的修補寫回 31/34 當「未完成」；那些 typed 路徑本身是對的。
- `ARCHITECTURE.md` 仍是權威描述。計劃與源碼衝突時以源碼為準，並在該計劃驗收裡更新架構句。

## 5. 明確不在本路線

- 新方塊／物品／生物、完整 vanilla parity。
- TLS／Mojang 帳戶／Realms。
- GPU／window／audio-device／DPI／Host+Join 實機畫面。
- 把 `state.rs` 一次拆完（14 只做已確認的耦合與 hitch）。
