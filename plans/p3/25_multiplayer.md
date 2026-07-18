# 任務 25：多人遊戲 (基礎)

> **複雜度**: ⭐⭐⭐⭐⭐ (最高)  
> **涉及面**: 網路架構、TCP 協議、狀態同步、併發處理  
> **前置條件**: 所有單人功能完成

---

## 25.1 網路架構

### 新增文件
- **[NEW]** `src/network/mod.rs` — 網路模組
- **[NEW]** `src/network/server.rs` — 伺服器邏輯
- **[NEW]** `src/network/client.rs` — 客戶端邏輯
- **[NEW]** `src/network/protocol.rs` — 封包定義

### 實現細節
```rust
pub enum Packet {
    // 連線
    Handshake { username: String },
    LoginSuccess { player_id: u64 },
    // 玩家
    PlayerPosition { id: u64, pos: Vec3, yaw: f32, pitch: f32 },
    PlayerAction { action: Action },
    // 方塊
    BlockChange { pos: IVec3, block: BlockType },
    // 聊天
    ChatMessage { sender: String, message: String },
    // Chunk
    ChunkData { cx: i32, cz: i32, data: Vec<u8> },
}
```

### 子任務清單
- [ ] TCP 連線層 (使用 `tokio`)
- [ ] 封包序列化/反序列化 (使用 `bincode` 或自定義)
- [ ] 伺服器: 監聽連線, 管理玩家, 權威性世界狀態
- [ ] 客戶端: 連線伺服器, 接收世界數據, 發送玩家動作
- [ ] 玩家位置同步: 每 tick 廣播所有玩家位置
- [ ] 方塊變更同步: 方塊修改廣播給所有客戶端
- [ ] 聊天系統: T 鍵打開聊天框, 發送/接收訊息
- [ ] 渲染其他玩家: 簡單的方塊人模型
- [ ] 斷線處理: 超時/異常斷線的清理

---

## 驗證
- [ ] 兩個客戶端可連線同一伺服器
- [ ] 玩家互相可見且位置同步
- [ ] 一個玩家放置的方塊另一個能看到
- [ ] 聊天功能正常
