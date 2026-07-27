# Entity Persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement full persistence for world entities (mobs, bosses, dropped items) so entity state is preserved across world saves, re-entry, and dimension switches.

**Architecture:** Add `EntitySaveData` in `src/save.rs` to serialize persistent entity state with Bincode into `saves/<world>/entities.dat` (Overworld) and `saves/<world>/dimensions/{nether,end}/entities.dat` (Nether/End). Integrate `SaveManager` entity save/load methods into `State::new`, `State::save_world`/autosave, and dimension switching.

**Tech Stack:** Rust, Bincode, Serde, Glam Vec3.

## Global Constraints

- Preserve backward compatibility for existing world save directories without `entities.dat`.
- Filter out non-persisted transient entities (`RemotePlayer`, projectiles, particles).
- All unit tests must pass with `cargo test`.

---

### Task 1: `EntitySaveData` Data Structure and Conversion Methods

**Files:**
- Modify: `src/save.rs`
- Test: `src/save.rs` (inline test `test_entity_save_data_roundtrip`)

**Interfaces:**
- Produces: `pub struct EntitySaveData`, `impl EntitySaveData`, `impl From<&Entity> for EntitySaveData`

- [ ] **Step 1: Write failing unit test for `EntitySaveData`**

Add test to `src/save.rs`:
```rust
#[test]
fn test_entity_save_data_roundtrip() {
    use crate::entity::{Entity, EntityType};
    use glam::Vec3;

    let mut entity = Entity::new(42, EntityType::Pig, Vec3::new(10.5, 64.0, -15.2));
    entity.health = 7.5;
    entity.age = -120.0;
    entity.has_wool = true;

    let save_data = EntitySaveData::from(&entity);
    assert_eq!(save_data.entity_type, EntityType::Pig);
    assert_eq!(save_data.position, [10.5, 64.0, -15.2]);

    let restored = save_data.to_entity(100);
    assert_eq!(restored.id, 100);
    assert_eq!(restored.entity_type, EntityType::Pig);
    assert_eq!(restored.position, Vec3::new(10.5, 64.0, -15.2));
    assert_eq!(restored.health, 7.5);
    assert_eq!(restored.age, -120.0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_entity_save_data_roundtrip`
Expected: FAIL due to missing `EntitySaveData`.

- [ ] **Step 3: Implement `EntitySaveData` in `src/save.rs`**

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EntitySaveData {
    pub entity_type: crate::entity::EntityType,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub health: f32,
    pub max_health: f32,
    pub is_ignited: bool,
    pub burn_timer: f32,
    pub age: f32,
    pub breeding_timer: f32,
    pub breed_cooldown: f32,
    pub has_wool: bool,
    pub wool_color: [f32; 3],
    pub dropped_item: Option<crate::inventory::Item>,
    pub dropped_count: u32,
}

impl From<&crate::entity::Entity> for EntitySaveData {
    fn from(entity: &crate::entity::Entity) -> Self {
        Self {
            entity_type: entity.entity_type,
            position: [entity.position.x, entity.position.y, entity.position.z],
            velocity: [entity.velocity.x, entity.velocity.y, entity.velocity.z],
            yaw: entity.yaw,
            pitch: entity.pitch,
            health: entity.health,
            max_health: entity.max_health,
            is_ignited: entity.is_ignited,
            burn_timer: entity.burn_timer,
            age: entity.age,
            breeding_timer: entity.breeding_timer,
            breed_cooldown: entity.breed_cooldown,
            has_wool: entity.has_wool,
            wool_color: entity.wool_color,
            dropped_item: entity.dropped_item,
            dropped_count: entity.dropped_count,
        }
    }
}

impl EntitySaveData {
    pub fn to_entity(&self, id: u64) -> crate::entity::Entity {
        let pos = glam::Vec3::new(self.position[0], self.position[1], self.position[2]);
        let mut entity = crate::entity::Entity::new(id, self.entity_type, pos);
        entity.velocity = glam::Vec3::new(self.velocity[0], self.velocity[1], self.velocity[2]);
        entity.yaw = self.yaw;
        entity.pitch = self.pitch;
        entity.health = self.health;
        entity.max_health = self.max_health;
        entity.is_ignited = self.is_ignited;
        entity.burn_timer = self.burn_timer;
        entity.age = self.age;
        entity.breeding_timer = self.breeding_timer;
        entity.breed_cooldown = self.breed_cooldown;
        entity.has_wool = self.has_wool;
        entity.wool_color = self.wool_color;
        entity.dropped_item = self.dropped_item;
        entity.dropped_count = self.dropped_count;
        entity
    }

    pub fn should_persist(&self) -> bool {
        self.entity_type.is_living()
            || self.entity_type.is_persistent()
            || self.entity_type == crate::entity::EntityType::DroppedItem
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_entity_save_data_roundtrip`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/save.rs
git commit -m "feat: add EntitySaveData serialization struct and roundtrip test"
```

---

### Task 2: `SaveManager` Entity Save/Load Methods

**Files:**
- Modify: `src/save.rs`
- Test: `src/save.rs` (inline test `test_save_manager_entities_persistence`)

**Interfaces:**
- Consumes: `EntitySaveData`
- Produces: `SaveManager::entities_file_path`, `SaveManager::save_entities_in`, `SaveManager::load_entities_in`

- [ ] **Step 1: Write failing unit test for `SaveManager` entity save/load**

Add test to `src/save.rs`:
```rust
#[test]
fn test_save_manager_entities_persistence() {
    let temp_dir = std::env::temp_dir().join(format!("icraft_test_entities_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let manager = SaveManager::new(&temp_dir);

    let test_data = vec![EntitySaveData {
        entity_type: crate::entity::EntityType::Zombie,
        position: [1.0, 65.0, 2.0],
        velocity: [0.0, 0.0, 0.0],
        yaw: 0.0,
        pitch: 0.0,
        health: 20.0,
        max_health: 20.0,
        is_ignited: false,
        burn_timer: 0.0,
        age: 0.0,
        breeding_timer: 0.0,
        breed_cooldown: 0.0,
        has_wool: false,
        wool_color: [1.0, 1.0, 1.0],
        dropped_item: None,
        dropped_count: 0,
    }];

    manager.save_entities_in(crate::dimension::Dimension::Overworld, &test_data).unwrap();

    let loaded = manager.load_entities_in(crate::dimension::Dimension::Overworld);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].entity_type, crate::entity::EntityType::Zombie);
    assert_eq!(loaded[0].position, [1.0, 65.0, 2.0]);

    let _ = std::fs::remove_dir_all(&temp_dir);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_save_manager_entities_persistence`
Expected: FAIL due to missing `save_entities_in` / `load_entities_in`.

- [ ] **Step 3: Implement entity save/load methods in `SaveManager`**

In `src/save.rs`:
```rust
impl SaveManager {
    pub fn entities_file_path(&self, dimension: crate::dimension::Dimension) -> PathBuf {
        match dimension {
            crate::dimension::Dimension::Overworld => self.world_dir.join("entities.dat"),
            crate::dimension::Dimension::Nether => self
                .world_dir
                .join("dimensions")
                .join("nether")
                .join("entities.dat"),
            crate::dimension::Dimension::End => self
                .world_dir
                .join("dimensions")
                .join("end")
                .join("entities.dat"),
        }
    }

    pub fn save_entities_in(
        &self,
        dimension: crate::dimension::Dimension,
        entities: &[EntitySaveData],
    ) -> io::Result<()> {
        let path = self.entities_file_path(dimension);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = bincode::serialize(entities)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        fs::write(path, bytes)
    }

    pub fn load_entities_in(
        &self,
        dimension: crate::dimension::Dimension,
    ) -> Vec<EntitySaveData> {
        let path = self.entities_file_path(dimension);
        if !path.exists() {
            return Vec::new();
        }
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(_) => return Vec::new(),
        };
        bincode::deserialize(&bytes).unwrap_or_default()
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_save_manager_entities_persistence`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/save.rs
git commit -m "feat: implement save_entities_in and load_entities_in in SaveManager"
```

---

### Task 3: `State` Integration for World Load, Autosave, and Dimension Switching

**Files:**
- Modify: `src/state.rs:1280-1340`, `src/state.rs:3790-3840`, `src/state.rs:7200-7260`
- Test: Manual verification + `cargo check --release` & `cargo test`

**Interfaces:**
- Consumes: `SaveManager::save_entities_in`, `SaveManager::load_entities_in`, `EntitySaveData`

- [ ] **Step 1: Add entity saving helper in `State`**

In `src/state.rs`:
```rust
impl State {
    pub fn save_current_dimension_entities(&self) {
        let save_manager = match self.save_manager.lock() {
            Ok(mgr) => mgr,
            Err(_) => return,
        };
        let persistent_entities: Vec<crate::save::EntitySaveData> = self
            .entity_manager
            .entities
            .iter()
            .map(crate::save::EntitySaveData::from)
            .filter(|data| data.should_persist())
            .collect();

        let _ = save_manager.save_entities_in(self.current_dimension, &persistent_entities);
    }

    pub fn load_current_dimension_entities(&mut self) {
        let save_manager = match self.save_manager.lock() {
            Ok(mgr) => mgr,
            Err(_) => return,
        };
        let saved_entities = save_manager.load_entities_in(self.current_dimension);
        if saved_entities.is_empty() {
            return;
        }

        self.entity_manager.entities.clear();
        self.entity_manager.next_id = 1;
        for data in saved_entities {
            let entity = data.to_entity(self.entity_manager.next_id);
            self.entity_manager.next_id += 1;
            self.entity_manager.entities.push(entity);
        }
    }
}
```

- [ ] **Step 2: Connect `load_current_dimension_entities` in `State::new`**

In `State::new` (around line 3800, right after `entity_manager` and `save_manager` initialization):
```rust
let mut state = State { ... };
state.load_current_dimension_entities();
```

- [ ] **Step 3: Connect entity saving in `State` autosave / save paths and dimension transitions**

1. In `State` save methods (where `save_level` / `save_player` are called):
Call `self.save_current_dimension_entities();`.

2. In dimension transition logic (around line 1286 of `src/state.rs`):
Before switching:
`self.save_current_dimension_entities();`
After resetting `entity_manager` for the target dimension:
`self.load_current_dimension_entities();`

- [ ] **Step 4: Verify with `cargo test` and `cargo check --release`**

Run: `cargo test`
Expected: ALL PASS.

Run: `cargo check --release`
Expected: Clean compilation with zero warnings/errors.

- [ ] **Step 5: Commit**

```bash
git add src/state.rs
git commit -m "feat: integrate entity saving and loading into State lifecycle"
```
