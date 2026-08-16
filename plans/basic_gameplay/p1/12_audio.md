# 任務 12：音效系統

> **複雜度**: ⭐⭐⭐  
> **涉及面**: 外部庫集成、音頻管理、事件觸發  
> **前置條件**: 無（可獨立開發，但需其他系統的事件觸發點）

---

## 12.1 音頻引擎

### 新增/修改文件
- **[NEW]** `src/audio.rs` — 音效管理器
- **[MODIFY]** `Cargo.toml` — 新增 `rodio` 或 `kira` 依賴
- **[NEW]** `assets/sounds/` — 音效文件目錄

### 實現細節
```rust
pub struct AudioManager {
    sink: rodio::Sink,
    sounds: HashMap<SoundId, Vec<u8>>,
}

pub enum SoundId {
    BlockBreak(BlockMaterial),
    BlockPlace(BlockMaterial),
    Footstep(BlockMaterial),
    PlayerHurt,
    PlayerDeath,
    // ...
}
```

### 子任務清單
- [ ] 集成 `rodio` 音頻庫
- [ ] `AudioManager`：預加載音效 + 播放控制
- [ ] 生成/錄製基礎音效 (或使用開源音效):
  - [ ] 方塊放置音效 (按材質分類: 石/木/沙/草)
  - [ ] 方塊破壞音效
  - [ ] 腳步聲 (按踩的方塊材質不同)
  - [ ] 跳躍/著地音效
  - [ ] 受傷/死亡音效
  - [ ] UI 點擊音效
- [ ] 觸發點整合：方塊交互、移動、受傷等事件觸發音效
- [ ] 音量設定：在暫停選單增加音量調節

---

## 12.2 3D 空間音效（可選，P1 內簡化處理）
- [ ] 根據聲源距離衰減音量
- [ ] 左右聲道平衡 (基於聲源相對方向)

---

## 驗證
- [ ] 挖掘方塊有音效
- [ ] 放置方塊有音效
- [ ] 走路有腳步聲
- [ ] 受傷有音效
- [ ] 音量可在設定中調節
