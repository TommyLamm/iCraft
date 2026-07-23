# 實作計畫 02：多人玩家移動平滑

## 目標

在穩定低延遲網路與一般 jitter 下，遠端玩家連續移動，不再以 20 Hz
停住後閃現；短暫丟包時有限外推，真正傳送/瞬移時能立即收斂。

## 已確認根因

- 位置每 50 ms 發送一次，但 `RemotePlayerState` 只保存 `prev/latest`。
- render target 固定為 `now - 100 ms`。最新兩點只相隔 50 ms，所以 target
  永遠早於 `prev`，插值係數被 clamp 為 0；每來一包才跳到新的 `prev`。
- 同 frame 排空的包共用 `f32 network_time`，會失去各自時間間隔。
- TCP 未開 `TCP_NODELAY`，frame header/payload 分兩次寫入。
- mob update 會清掉 RemotePlayer 的插值速度，使步行動畫停住。

## 實作步驟

1. 升協議版本；`PlayerPosition` 增加單調 `sequence` 和 sender timestamp。
2. client、server relay、host broadcast 保留 sequence/timestamp，但 server
   仍覆寫不可信的 player id。
3. `RemotePlayerState` 改為有上限的 `VecDeque<PlayerSnapshot>`，丟棄重複、
   逆序及非有限資料。
4. 用單調時間建立 render target，從佇列尋找真正包住 target 的兩點；
   position/pitch 線性插值，yaw 走最短弧。
5. target 晚於最新點時只外推最多 100 ms，限制速度，之後 hold。
6. 過大距離或過長 gap 判定為 teleport，清 buffer 並 snap。
7. RemotePlayer 跳過 mob physics 且保留由 sampled position 算出的速度。
8. transport 開啟 `TCP_NODELAY`，將 header + payload 合併成一次 write。
9. position queue 採 latest-wins/coalescing；可靠 world/chat 資料仍保留
   backpressure 語義。
10. 更新協議、架構與進度文件。

## 驗證

- 20 Hz snapshots 在 60/144 Hz sample 下位置連續且單調。
- jitter、批量到達、丟包、外推上限、teleport、yaw wrap 回歸測試。
- protocol roundtrip 與 server relay 保留 sequence/timestamp。
- 舊協議在 handshake 明確拒絕。
- RemotePlayer velocity 經 mob update 後仍存在。
- `cargo fmt -- --check`、`cargo test --release`、`cargo check --release`。
- 人工 Host + Join 雙視窗低延遲走動、衝刺、跳躍、急轉向。

## Commit

單一功能 commit：`fix(network): smooth remote player movement`

