# 任務 10：基礎敵對生物

> **複雜度**: ⭐⭐⭐⭐⭐ (最高)  
> **涉及面**: 實體系統、AI、物理、模型渲染、生成規則  
> **前置條件**: 任務 8 (生命值) + 任務 9 (日夜循環)

---

## 10.1 生物框架

### 新增/修改文件
- **[NEW]** `src/entity.rs` — 實體基礎系統
- **[NEW]** `src/mob.rs` — 生物 AI 與行為
- **[NEW]** `src/mob_renderer.rs` — 生物模型渲染
- **[MODIFY]** `src/state.rs` — 實體管理集成

### 實現細節
```rust
pub struct Entity {
    pub id: u64,
    pub position: Vec3,
    pub velocity: Vec3,
    pub rotation: f32,
    pub entity_type: EntityType,
    pub health: f32,
    pub aabb: AABB,
}

pub enum EntityType {
    Zombie { target: Option<u64> },
    Skeleton { target: Option<u64>, shoot_cooldown: f32 },
    Creeper { target: Option<u64>, fuse_timer: f32, primed: bool },
}
```

### 子任務清單
- [x] 定義 `Entity` 基礎結構體
- [x] `EntityManager`: 管理所有活動實體
- [x] 實體物理：重力、AABB碰撞、地面偵測
- [x] 實體與玩家的碰撞/傷害

---

## 10.2 殭屍 (Zombie)
- [x] 簡單的長方體模型 (頭+身體+四肢)
- [x] AI：在玩家 16 格範圍內追蹤、攻擊
- [x] 夜晚或暗處生成
- [x] 接觸玩家造成傷害
- [x] 日光下燃燒
- [x] 被擊殺後掉落腐肉

---

## 10.3 骷髏弓手 (Skeleton)
- [x] 長方體模型 (略白色)
- [x] AI：保持距離，射箭
- [x] 弓箭投射物實體
- [x] 日光下燃燒
- [x] 掉落骨頭和弓

---

## 10.4 苦力怕 (Creeper)
- [x] 長方體模型 (綠色)
- [x] AI：靠近玩家後開始點燃計時
- [x] 發出 ssss 音效
- [x] 爆炸：破壞周圍方塊 + 傷害玩家
- [x] 掉落火藥

---

## 10.5 生成規則
- [x] 光照等級 ≤ 7 的方塊上方可生成敵對生物
- [x] 每個 Chunk 有生物數量上限
- [x] 距離玩家 24~128 格範圍內生成
- [x] 距離玩家 > 128 格的生物消失

---

## 驗證
- [x] 夜晚或洞穴中會生成殭屍、骷髏、苦力怕
- [x] 生物能追蹤/攻擊玩家
- [x] 玩家能攻擊並消滅生物
- [x] 苦力怕爆炸能破壞方塊
- [x] 日光下殭屍和骷髏燃燒
