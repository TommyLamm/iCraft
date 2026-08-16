# 16 — 多人權威收斂與無 GPU 獨立伺服器

## 執行包

- 優先級：P3
- 前置條件：02–15 已完成且各自的 wire/save 契約穩定
- 後續解鎖：17 與總驗收
- 建議提交上限：3
- 禁止順帶實作：Mojang 帳戶驗證、Realms、原版 Java protocol 相容

## 目標

把 listen-server 中仍由 `State`／GPU composition root 承擔的權威模擬抽成可無視窗運行的
server runtime，並為新增容器、睡眠、戰鬥、村民、載具、指令等建立一致的請求／ACK／
snapshot/revision 策略。

官方 [Dedicated Server 說明](https://help.minecraft.net/hc/en-us/articles/4408873961869-Minecraft-Dedicated-and-Featured-Servers-FAQ-)
確認 Java Edition 正式提供獨立伺服器；iCraft 不需相容其協議，但應具備同類部署能力。

## 現況問題

- 單一 binary 以 `State` 同時擁有 GPU、UI、世界模擬和 host bridge。
- host authority 原則已建立，但每個新系統若另造 packet，很容易出現不同 revision／拒絕語義。
- Login 只有 username 與 protocol version；沒有 per-world permissions、server config 或白名單。
- 伺服器無法在沒有 window/GPU/audio 的環境啟動。

## 實作步驟

### A. Server runtime 邊界

- [x] 新增 library target（`src/lib.rs`）和 `src/server_runtime.rs`；權威 tick/session/world-mutation 邊界不依賴 wgpu/winit。
- [ ] `State` 成為 client presentation + local input；單人模式也透過 in-process server runtime。
- [ ] 將 fixed tick、world mutation、entity AI、block entity、rules、commands 移到 server ownership。
- [x] GPU mesh/cache、camera、UI、particles/audio presentation 留在 client；dedicated binary 不建構這些資源。
- [x] 新增 `src/bin/icraft-server.rs`，支援 world、bind address、port、max players、view distance、simulation distance。

### B. 統一交易協議

- [x] 定義 request ID、server sequence、成功／拒絕原因、128-entry idempotency window。
- [x] 方塊、容器、item use、combat、sleep、trade、mount、command 使用 `GameplayRequest` envelope。
- [x] 每個 request 綁認證 player/session；驗證維度、距離、狀態、權限、revision 和 client sequence。
- [x] 重複 request 不重複執行；client revision gates 與 server sequence 不倒退。
- [x] packet/frame/decompressed collection/string 上限及每 client rate limit 均有明確上限。

### C. 每玩家與世界狀態

- [x] server 分開保存每玩家 inventory、health、effects、mode、spawn、advancements、position/dimension。
- [x] 登入載入本人資料，登出原子保存；同一身份採 reject duplicate-login policy。
- [x] Chunk/entity interest、container viewers 由每玩家 session 維護並按 view distance 計算。
- [x] world autosave/shutdown 不依賴 UI；SIGINT/console shutdown 做同步 flush。

### D. Server 管理

- [x] `server.properties` 或等價配置：motd、port、max players、difficulty、online-mode 占位說明、
  whitelist、view/simulation distance、pvp。
- [x] operator/whitelist 保存與 console commands；錯誤配置 fail-fast 且不覆寫世界。
- [x] server list ping 回版本、motd、玩家數；client menu 保存多個地址和最近連線結果。

### E. 測試與可觀測性

- [x] headless integration harness 啟動 server + 2–4 clients，不使用 GPU/window/audio（network/runtime tests）。
- [x] fault coverage：重複、亂序、斷線、慢 client、滿 queue、配置/保存失敗均有 headless 測試或明確錯誤路徑。
- [x] metrics/logging：tick time、queue depth、packets/bytes、loaded chunks、entities、save latency。

## 主要文件

- 新增：`src/lib.rs`、`src/server_runtime.rs`、`src/bin/icraft-server.rs`
- 大幅重構：`src/state.rs`、`src/network/*`、`src/save.rs`
- 修改：所有權威 gameplay modules 只接受 server-owned context/event，不依賴 renderer

## 驗收

- [ ] `icraft-server` 在無 GPU、無 audio、無 desktop session 環境以 `--once`/headless tests 啟動；已驗證短跑，30 分鐘 soak 尚未執行。
- [ ] 單人、listen-server、dedicated server 共用 protocol/gameplay request vectors；headless vectors 已通過，GPU Host+Join 實機場景尚未執行。
- [x] 兩 client 競爭同一 block/container request 時由單一 server sequence/response cache 決定權威結果。
- [x] 斷線重連保持玩家資料，duplicate login 被拒絕且不複製 inventory/entity。
- [x] 慢／惡意 client 不拖死主 tick；超限由 frame/queue/rate limits 隔離並回報原因。
- [x] shutdown/save failure 以 `io::Result`/binary non-zero 路徑返回，保留原檔供重試。

## 完成閘門

`cargo test` 的多人核心場景必須 headless；若測試仍需要建立 wgpu device/window 才能驗證
權威玩法，server/client 邊界尚未完成。

## Plan 16 verification note

- Headless narrow tests cover the envelope round trips, authenticated session
  binding, idempotency/out-of-order handling, two-session authoritative
  sequencing, properties/whitelist parsing, server-list ping, and dedicated
  `--once` startup.
- A 30-minute soak and GPU Host+Join scene were not run in this environment;
  they remain explicit manual acceptance risks rather than claimed results.

