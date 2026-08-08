# 18 — Plan16 核心權威遷移與獨立伺服器補齊

## 執行包

- 優先級：P3，Plan15–17 實作包完成後的缺口收斂
- 前置條件：Plan15、Plan16、Plan17 的目前 commit 與 wire/save 契約已固定；Plan16
  的 dedicated/runtime 基礎可重用，但不能把未完成的 authority migration 當作已完成。
- 後續解鎖：README 的多人權威完成狀態與整條路線最終驗收
- 建議提交上限：5（A core、B protocol、C persistence/interest、D management、E QA）
- 本計劃只補 Plan16 缺口；不順帶加入 Mojang 帳戶驗證、Realms、原版 Java protocol
  相容、Marketplace 或新的玩法內容。

## 目標與基線

`e905f13` 已提供 `src/lib.rs`、`src/server_runtime.rs`、`src/bin/icraft-server.rs`、
Gameplay request/response 型別、properties/ping 基礎、每玩家檔案與窄 headless tests。
這些是可保留的基線，不代表權威模擬已脫離 GPU composition root。

本計劃要讓 singleplayer、listen-server 與 dedicated server 共用同一個無 GPU 權威核心，
讓每一個 gameplay request 都有真正的 mutation、revision、拒絕與保存語義，並以同一組
headless vectors 驗證三種拓撲。Plan16 文件中 A 的兩個未勾項，以及 B–E 對整合程度的
過度宣稱，均在本計劃完成後才可關閉。

## 已知缺口（不可在本計劃開始前視為通過）

- `State::new` 仍建立 wgpu；Singleplayer 沒有 in-process `ServerRuntime`，Host 仍有
  自己的 State authority path。
- `ServerRuntime::tick` 只處理事件、時間、位置同步與保存，尚未擁有 world tick、entity
  AI、block entity、rules 或 commands。
- `GameplayOperation` 雖列出 block/container/item/combat/sleep/trade/mount/command，
  目前 runtime 只實作少量 bookkeeping；未實作的操作不可回 `Accepted` 假裝完成。
- current dimension、authoritative blocks/containers/entities 與實際 interest-based
  replication 尚未由 dedicated runtime 完整保存／路由。
- `ServerAddressBook` 尚未接到 Menu 的持久化與 server-list ping；login/max-player
  檢查需要 atomic reservation；runtime host inbound channel 需要明確有界。
- 現有 runtime 與 NetworkServer 測試分離，尚無同時啟動 authority runtime 與 2–4 clients
  的整合 harness；30 分鐘 soak 與 GPU Host+Join 仍是人工 QA。

## 實作步驟

### A. 無 GPU AuthorityCore 與三種拓撲

- [x] 新增 `ServerWorld`／`AuthorityCore`（可放在 `src/server_world.rs` 或
  `src/authority/`），只依賴 headless world、entity、block entity、rules、commands、
  persistence 與 protocol；不得依賴 wgpu、winit、audio、camera 或 UI。
- [x] 將 fixed tick、world mutation、entity AI、block entity/automation、world rules、
  command dispatch 的 ownership 移入 core；`ServerRuntime` 只負責 transport/session、
  tick scheduling、save/metrics 邊界。
- [x] 把 `State` 收斂成 presentation + local input。Singleplayer 透過 in-process
  runtime/channel，Host 透過同一個 runtime 加 listen transport；Dedicated binary 不
  建立 `State`。
- [x] 為 core 建立 deterministic tick API；同一 tick vector 在 local、listen、dedicated
  不得產生不同的 revision 或世界結果。

### B. 完整 Gameplay envelope 與 legacy migration

- [ ] 為 `BlockUse`、`Container`、`ItemUse`、`Combat`、`Sleep`、`Trade`、`Mount`、
  `Command` 逐一實作真正的 authority mutation、前置驗證、成功 revision、拒絕原因與
  response cache；任何未支援操作必須回 `Unsupported`／`InvalidState`，不可回空成功。
- [ ] 驗證 authenticated session、dimension、距離、狀態、權限、client sequence、
  client revision；保留具體 `RejectReason`（不可把所有 bounds error 摺成 `Malformed`）。
  server sequence、client revision gate 與 128-entry idempotency window 必須單調且可測。
- [ ] 將舊的 `BlockChange`、`BlockActionRequest`、Sleep、Container open/click/close
  入口改成只做一次 envelope adapter，保留原座標、slot、dimension、revision；完成 client
  遷移後移除重複 authority path。不得把 envelope 在 State 端降級成 StatusUpdate。
- [ ] 讓 local/listen/dedicated 使用相同 request/ACK/snapshot vectors，並測試 duplicate、
  out-of-order、stale revision、亂序 response 與重送不重複執行。

### C. 玩家、世界保存與 interest routing

- [x] Dedicated player file 明確保存 current dimension（與 spawn dimension 分開），以及
  inventory、health、effects、mode、spawn、advancements、position；登入載入、登出與
  shutdown/重啟以 atomic save 還原全部欄位。`SaveManager` v2 保留 v1 migration，並以
  `tests/authority_persistence.rs` 覆蓋 dimension/effects/reconnect round-trip。
- [x] 將 authoritative chunks/blocks、block entities/containers、entities 與各自
  revisions 接到 `SaveManager`；保存失敗保留原檔並可 retry，重啟後不丟失或複製狀態。
  Chunk region writes stage the new region and commit the cache only after atomic replacement;
  checked region/entity restore and the mutation-revision index are headless-tested.
- [x] 每個 session 維護 chunk/entity interest、simulation distance 與 container viewers，
  並讓 outgoing chunk/entity/block-entity/slot updates 真的依 interest、dimension、
  viewers 路由；測試進入／離開 interest 的增量與不向無關玩家洩漏。The C-owned runtime
  exposes a bounded `RoutedInterestUpdate` ledger and `InterestSet`; a targeted wire-packet
  adapter remains with the B/D protocol ownership and is not claimed as complete here.
- [x] duplicate identity policy 必須在 authority reservation 與 network session 兩層一致，
  不得以 silent ignore 取代明確拒絕。Runtime and the existing network session gate reject
  case-insensitive duplicates explicitly; atomic concurrent reservation/max-player evidence
  remains a D follow-up.

### D. 管理面、登入原子性與有界 transport

- [ ] `server.properties` 的 difficulty、PvP、view/simulation distance、motd、whitelist、
  operators、world path/seed 均要實際套用；錯誤配置 fail-fast 且不建立／覆寫世界。
- [ ] 將 `ServerAddressBook` 接入 Menu：多個地址、最近 ping 結果、錯誤與版本/MOTD/玩家數
  持久化，並以 server-list ping request/response 更新 UI；加入 round-trip/menu tests。
- [ ] login 在送出 LoginSuccess 前以 atomic reservation 同時檢查 duplicate identity 與
  `max_players`；並發登入測試不得超 cap、不得產生兩份玩家狀態。
- [ ] host inbound queue、每 client outbound queue、frame/packet/collection/string limits
  均有實際上限；滿 queue 要 deterministic backpressure 或 `QueueFull` response，不可只靠
  無界 channel 與每 tick 處理上限。保留 per-client rate limit。

### E. Headless harness、fault injection、metrics 與人工 QA

- [ ] 新增 headless integration harness，啟動 `ServerRuntime`／dedicated server 與 2–4 個
  clients（不建立 wgpu、window、audio），執行 A–C 的共同 gameplay vectors；至少覆蓋
  login、block/container、player save/reconnect、interest 與 revision gates。
- [ ] 建立可重現 fault injection：duplicate/out-of-order/stale request、斷線重連、慢 client、
  滿 queue、rate/size limit、bind/config failure、save failure、並發 duplicate login/max
  players；每項都要有明確 reject、disconnect 或 retry assertion。
- [ ] metrics/logging 真實更新並可在測試讀取：tick time、inbound/outbound packets 與 bytes、
  queue depth、loaded chunks、entities、players、save latency、reject/duplicate counters。
- [ ] headless `--once` 與短跑先納入 CI；完成後再執行 30 分鐘 dedicated soak。GPU Host+Join、
  窗口／音效與跨比例人工 QA 另列證據，不以 headless 結果代替。

## 分階段與並行邊界

1. **Phase 0（串行）**：固定 AuthorityCore/session/save/revision contracts，列出 legacy
   packet adapter 與共同 vectors；沒有這份 contract 不開始平行修改。
2. **Phase A（阻塞）**：完成無 GPU core 與 State 的 local/Host 邊界。A 通過前不得宣稱
   singleplayer 或 listen-server 已共用 dedicated authority。
3. **Phase B + D（可並行，依 Phase 0 contract）**：protocol/legacy adapter（B）與
   properties/address-book/login reservation/bounded queues（D）可由不同 owner 進行；
   兩者不得各自新增第二套 revision 或 session policy。
4. **Phase C（依賴 A，部分可與 B/D 並行）**：保存與 interest routing 只接 AuthorityCore
   API；不得把 renderer `ChunkManager` 直接搬進 dedicated binary。
5. **Phase E（依賴 A–C，D 的 queue/metrics 可先做單元測試）**：整合 harness、fault injection、
   soak 與人工 QA；最後才更新 README/ARCHITECTURE 的完成狀態。

檔案 ownership 應保持分離：A 負責 `src/server_world.rs`／`src/authority/*`、
`src/server_runtime.rs`、`src/state.rs`；B 負責 `src/network/protocol.rs`、`client.rs`、
`server.rs`；C 負責 `src/save.rs` 與 authority persistence；D 負責 `src/menu.rs`、
`src/bin/icraft-server.rs` 與 login/config；E 負責 `tests/headless_server_authority.rs`
及各模組測試。若需要同改既有檔案，先合併 contract commit 再分批修改，避免互相覆寫。

## 主要文件

- 建議新增：`src/server_world.rs` 或 `src/authority/*`、
  `tests/headless_server_authority.rs`、fault/metrics test helpers。
- 修改：`src/server_runtime.rs`、`src/state.rs`、`src/network/{protocol,client,server}.rs`、
  `src/save.rs`、`src/menu.rs`、`src/bin/icraft-server.rs`、必要的 `src/lib.rs` exports。
- 最後才更新：`plans/minecraft_foundation_gap/README.md`、`ARCHITECTURE.md` 與 Plan16
  verification note；本文件本身只描述缺口，不實作功能。

## 驗收

- [ ] `cargo test` 的 authority core、protocol、persistence、network tests 在無 GPU/window/audio
  下通過；所有共同 vectors 在 singleplayer、listen-server、dedicated 三種拓撲產生相同
  mutation/revision/response。
- [ ] `State` 不再是 singleplayer/Host 的 authority；可在沒有 wgpu/winit/audio 的環境以
  in-process runtime 驗證單人與 listen authority。
- [ ] 八個 GameplayOperation domain 都有真實 mutation 或明確 Unsupported reject；legacy
  packet 不會繞過 envelope，也不會丟掉座標、slot、dimension 或 revision。
- [ ] 玩家 current dimension、inventory/health/effects/mode/spawn/advancements/position、
  world blocks/containers/entities 在 disconnect、shutdown、重啟後完整還原且不複製。
- [ ] 2–4 client harness 驗證競爭、interest routing、container viewers、duplicate/replay、
  stale/out-of-order、慢 client、滿 queue、配置／保存失敗與 retry/error 路徑。
- [ ] server.properties、whitelist/operators、address book/ping、atomic max-player/login
  reservation 與 metrics 均有 round-trip/並發測試；inbound/outbound bytes 與 queue depth
  不可為未更新的 placeholder。
- [ ] `icraft-server --once`、短跑與 30 分鐘 soak 有 headless 證據；GPU Host+Join 及窗口／音效
  QA 有獨立人工紀錄，未通過者不能關閉本計劃完成閘門。

## 完成閘門

只有同時滿足以下條件，才可把 Plan16 的核心權威缺口標為完成：

1. AuthorityCore 可無 GPU 運行，且 State Singleplayer/Host、listen transport、dedicated
   binary 共用同一 tick/world/request path。
2. 八個 gameplay domains 均有 mutation/revision/reject/保存語義；legacy path 已成單一
   adapter 或移除，沒有 Accepted no-op、StatusUpdate 假 ACK 或不受 revision gate 的旁路。
3. 玩家與世界狀態可跨 disconnect、shutdown、重啟保存，interest/viewer routing 有端到端
   測試且沒有跨玩家洩漏或複製。
4. 管理與 transport 的配置、ping/address book、atomic login/max-player reservation、
   bounded queues、metrics/fault paths 均有自動測試。
5. 2–4 client headless harness、短跑與 fault matrix 通過；30 分鐘 soak 及 GPU Host+Join
   人工 QA 已有明確 pass/fail 證據。任一未完成時，README 必須保留「核心權威遷移缺口轉18」
   而不可標示 Plan16 完成。
