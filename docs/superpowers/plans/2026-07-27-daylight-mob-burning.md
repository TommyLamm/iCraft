# Daylight Mob Burning Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement Minecraft daylight mob burning mechanics for Zombies and Skeletons (sunlight exposure, water/rain extinguishing, fire aspect timer ignition, visual flame overlay, and burn death drops/audio).

**Architecture:** Update `is_under_sun` and `update_mobs` in `src/mob.rs` to validate weather, water blocks, and sunlight exposure, setting `fire_aspect_timer` for Zombie/Skeleton mobs. In `src/mob_renderer.rs`, render an emissive fire overlay box for entities on fire. In `src/mob.rs` and `src/state.rs`, handle burn damage death loot drops and death audio.

**Tech Stack:** Rust, wgpu, glam, bincode

---

### Task 1: Enhance Daylight Exposure and Extinguish Logic

**Files:**
- Modify: `src/mob.rs:172-191`
- Modify: `src/mob.rs:273-380`
- Modify: `src/state.rs:5740-5754`

- [ ] **Step 1: Write unit test for daylight exposure and extinguish logic**

Add unit test `test_daylight_exposure_and_water_extinguish` to `src/mob.rs`:

```rust
#[test]
fn test_daylight_exposure_and_water_extinguish() {
    let chunk_manager = ChunkManager::new();
    let zombie_pos = Vec3::new(8.0, 64.0, 8.0);
    // Exposed to sky (15), sky_light_level = 15, not raining, not in water
    assert!(is_under_sun(&chunk_manager, zombie_pos, 15, false));
    // Raining -> should not be exposed to sun
    assert!(!is_under_sun(&chunk_manager, zombie_pos, 15, true));
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test test_daylight_exposure_and_water_extinguish`
Expected: FAIL due to missing `is_raining` parameter in `is_under_sun`.

- [ ] **Step 3: Implement enhanced `is_under_sun` and extinguish logic in `mob.rs`**

Update `is_under_sun` in `src/mob.rs`:

```rust
fn is_under_sun(
    chunk_manager: &ChunkManager,
    pos: Vec3,
    sky_light_level: u8,
    is_raining: bool,
) -> bool {
    if is_raining || sky_light_level <= 10 {
        return false;
    }
    let mx = pos.x.floor() as i32;
    let my = pos.y.floor() as i32;
    let mz = pos.z.floor() as i32;

    // Check if foot or eye block is water
    let feet_block = chunk_manager.get_block(mx, my, mz);
    let head_block = chunk_manager.get_block(mx, my + 1, mz);
    if feet_block == crate::world::BlockType::Water
        || feet_block == crate::world::BlockType::FlowingWater
        || head_block == crate::world::BlockType::Water
        || head_block == crate::world::BlockType::FlowingWater
    {
        return false;
    }

    if chunk_manager.get_sky_light(mx, my, mz) < 12 {
        return false;
    }

    for y in (my + 1)..(crate::world::CHUNK_HEIGHT as i32) {
        if chunk_manager.get_block(mx, y, mz).properties().is_solid {
            return false;
        }
    }
    true
}
```

Update `update_mobs` parameter and callers to pass `is_raining` (derived from weather system in `state.rs`), and update the mob fire timer:

In `mob.rs` inside `update_mobs`:
```rust
let is_in_water = {
    let mx = entity.position.x.floor() as i32;
    let my = entity.position.y.floor() as i32;
    let mz = entity.position.z.floor() as i32;
    let block = chunk_manager.get_block(mx, my, mz);
    block == crate::world::BlockType::Water || block == crate::world::BlockType::FlowingWater
};

if is_in_water || is_raining {
    entity.fire_aspect_timer = 0.0;
    entity.burn_timer = 0.0;
    entity.burn_damage_timer = 0.0;
} else if (entity.entity_type == EntityType::Zombie || entity.entity_type == EntityType::Skeleton)
    && is_under_sun(chunk_manager, entity.position, sky_light_level, is_raining)
{
    entity.fire_aspect_timer = entity.fire_aspect_timer.max(8.0);
}
```

- [ ] **Step 4: Run test to verify pass**

Run: `cargo test test_daylight_exposure_and_water_extinguish`
Expected: PASS

- [ ] **Step 5: Commit changes**

```bash
git add src/mob.rs src/state.rs
git commit -m "feat: enhance daylight burning exposure and extinguish logic"
```

---

### Task 2: Add Visual Flame Overlay Rendering for Burning Mobs

**Files:**
- Modify: `src/mob_renderer.rs:600-750`

- [ ] **Step 1: Write unit test for entity fire overlay check**

Add unit test to `src/mob_renderer.rs`:

```rust
#[test]
fn test_burning_entity_fire_flag() {
    let mut entity = crate::entity::Entity::new(1, crate::entity::EntityType::Zombie, Vec3::ZERO);
    entity.fire_aspect_timer = 5.0;
    assert!(entity.fire_aspect_timer > 0.0 || entity.burn_timer > 0.0);
}
```

- [ ] **Step 2: Run test to verify pass**

Run: `cargo test test_burning_entity_fire_flag`
Expected: PASS

- [ ] **Step 3: Implement fire overlay cuboid rendering in `mob_renderer.rs`**

In `src/mob_renderer.rs`, inside `render_mobs` (or helper function `render_fire_overlay_for_entity`), when `entity.fire_aspect_timer > 0.0 || entity.burn_timer > 0.0` and entity is not `RemotePlayer`:

```rust
if entity.fire_aspect_timer > 0.0 || entity.burn_timer > 0.0 {
    let fire_size = entity.size + Vec3::splat(0.1);
    let fire_offset = Vec3::new(0.0, entity.size.y * 0.5, 0.0);
    let fire_tile = crate::world::BlockType::Fire.properties().texture_indices.top as u32;
    add_cuboid(
        vertices,
        indices,
        fire_size,
        fire_offset,
        entity.position,
        entity.yaw,
        0.0,
        [fire_tile; 6],
        0, // tile row from atlas
        1.0, // full emissive light level
    );
}
```

- [ ] **Step 4: Run build check to verify compilation**

Run: `cargo check --release`
Expected: PASS with 0 compilation errors.

- [ ] **Step 5: Commit changes**

```bash
git add src/mob_renderer.rs
git commit -m "feat: add visual fire overlay rendering for burning entities"
```

---

### Task 3: Add Burn Death Loot Drops and Audio Effects

**Files:**
- Modify: `src/mob.rs:330-380`
- Modify: `src/mob.rs:610-630`

- [ ] **Step 1: Write unit test for mob burn death loot generation**

Add unit test to `src/mob.rs`:

```rust
#[test]
fn test_mob_burn_death_drops() {
    let mut zombie = Entity::new(1, EntityType::Zombie, Vec3::ZERO);
    zombie.health = 0.0;
    zombie.burn_timer = 1.0;
    assert_eq!(zombie.entity_type, EntityType::Zombie);
}
```

- [ ] **Step 2: Run test to verify pass**

Run: `cargo test test_mob_burn_death_drops`
Expected: PASS

- [ ] **Step 3: Implement loot drops and audio on mob burn death**

In `src/mob.rs` during entity health check or burn tick:
When `entity.health <= 0.0` and `entity.is_living()` and not already dead:
- Check mob entity type:
  - If `Zombie`: spawn `Item::RottenFlesh` as dropped item.
  - If `Skeleton`: spawn `Item::Bone` and `Item::Arrow` as dropped items.
- Play positional audio `SoundId::PlayerDeath` or hurt sound at entity position.
- Spawn smoke/debris particles via `particles::spawn_block_debris`.

- [ ] **Step 4: Run all unit tests to verify system stability**

Run: `cargo test`
Expected: ALL PASS

- [ ] **Step 5: Commit changes**

```bash
git add src/mob.rs
git commit -m "feat: add loot drops and audio when mobs burn to death"
```
