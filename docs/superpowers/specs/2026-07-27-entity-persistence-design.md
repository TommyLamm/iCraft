# Entity Persistence Design Document

## Goal

Persist world entities (passive mobs, hostile mobs, bosses, and dropped items) across save/load cycles and dimension switches so entities are not lost or randomly respawned every time a world is loaded.

## Architecture & Storage Format

Entities are stored per-dimension using Bincode serialization in a dedicated binary file:
- **Overworld**: `saves/<world>/entities.dat`
- **Nether**: `saves/<world>/dimensions/nether/entities.dat`
- **End**: `saves/<world>/dimensions/end/entities.dat`

### Entity Persistence Rules

- **Persisted Entities**:
  - Passive mobs (`Pig`, `Cow`, `Sheep`, `Chicken`)
  - Hostile mobs (`Zombie`, `Skeleton`, `Creeper`, `Blaze`, `Piglin`, `Husk`, `Shulker`)
  - Bosses & End structures (`EnderDragon`, `Wither`, `EndCrystal`)
  - Dropped items (`DroppedItem`)
- **Transient Entities (Not Saved)**:
  - Remote player avatars (`RemotePlayer`)
  - Combat projectiles (`Arrow`, `SplashPotion`, `WitherSkull`, `DragonBreath`)
  - Pure visual particles (`HeartParticle`)

## Data Structures

### `EntitySaveData` (in `src/save.rs`)

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EntitySaveData {
    pub entity_type: EntityType,
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
```

- `From<&Entity> for EntitySaveData`: Constructs `EntitySaveData` from a active `Entity`.
- `to_entity(&self, id: u64) -> Entity`: Restores an `Entity` with restored position, orientation, health, passive attributes, and dropped item details.

## Save & Load Lifecycle Integrations

### 1. `SaveManager` (in `src/save.rs`)
- `entities_file_path(&self, dimension: Dimension) -> PathBuf`
- `save_entities_in(&self, dimension: Dimension, entities: &[EntitySaveData]) -> io::Result<()>`
- `load_entities_in(&self, dimension: Dimension) -> Vec<EntitySaveData>`

### 2. Game Startup (`State::new` in `src/state.rs`)
- Loads saved entities for the starting dimension from `save_manager` into `self.entity_manager`.
- Retains restored entities and prevents immediate duplicate mob wipe.

### 3. World Save (`State` save paths / autosave)
- When autosaving or saving & quitting, collects persistent entities from `self.entity_manager.entities`, converts them to `Vec<EntitySaveData>`, and saves them via `save_manager.save_entities_in(current_dimension, ...)`.

### 4. Dimension Transition
- Before switching dimensions, saves active entities of the current dimension.
- After switching, loads saved entities for the destination dimension into `self.entity_manager`.

## Verification Plan

### Automated Tests (`cargo test`)
- Unit test for `EntitySaveData` serialization / deserialization round-trip.
- Unit test for `SaveManager::save_entities_in` and `load_entities_in` across Overworld, Nether, and End.
- Unit test verifying transient entities (`RemotePlayer`, `Arrow`) are filtered out during conversion.

### Manual Verification (`cargo run`)
- Launch a world, breed/spawn mobs or drop items on the ground.
- Save and Quit to Main Menu.
- Re-enter the world and verify mobs and dropped items remain in exact position, health, and state.
