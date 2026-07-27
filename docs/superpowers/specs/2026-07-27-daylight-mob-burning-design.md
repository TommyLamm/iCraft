# Daylight Mob Burning Design Specification

## Overview

Implement authentic daylight mob burning mechanics for Zombies and Skeletons, full extinguish rules (water/rain), visual fire overlay rendering around burning mobs, particle effects, and death loot/audio handling when mobs burn to death.

## Functional Requirements

### 1. Daylight Burning Logic (`src/mob.rs`)
- **Target Mobs**: `EntityType::Zombie` and `EntityType::Skeleton`.
- **Daylight Exposure Check (`is_under_sun`)**:
  - `sky_light_level > 10` (Daytime).
  - Sky light at mob position `chunk_manager.get_sky_light(...) >= 12`.
  - No solid block above mob position up to max chunk height `CHUNK_HEIGHT`.
  - Not raining: current weather is neither `Weather::Rain` nor `Weather::Thunder`.
  - Not in water: block at mob feet/waist is neither `BlockType::Water` nor `BlockType::FlowingWater`.
- **Ignition & Extinguish State**:
  - Exposed mobs gain `entity.fire_aspect_timer = entity.fire_aspect_timer.max(8.0)`.
  - Mobs in water or rain have `fire_aspect_timer = 0.0` and `burn_timer = 0.0`.
  - Burn damage ticks 1.0 damage per second while `fire_aspect_timer > 0.0` or under sun.

### 2. Entity Fire Overlay & Particles (`src/mob_renderer.rs`, `src/particles.rs`)
- Mobs with active fire (`fire_aspect_timer > 0.0 || burn_timer > 0.0`) render a slightly enlarged fire bounding box/cross quad using `BlockType::Fire` texture atlas tiles at full emission (`light_level = 1.0`).
- Periodically spawn ambient smoke/flame particles around burning entities.

### 3. Burn Death Handling (`src/mob.rs`, `src/state.rs`)
- When a mob's health drops to `<= 0.0` from burn damage:
  - Play death audio and spawn death smoke particles.
  - Drop mob loot items (`RottenFlesh` for Zombie; `Bone` and `Arrow` for Skeleton) at the mob's position.
  - Clean up the dead entity from `EntityManager`.

## Component Changes

- **`src/mob.rs`**:
  - Enhance `is_under_sun` to accept weather (rain/thunder) and water block checks.
  - Update `update_mobs` to set `fire_aspect_timer` when exposed to sun, extinguish in water/rain, and spawn loot drops + death audio on burn death.
- **`src/mob_renderer.rs`**:
  - Render an emissive fire cuboid around mobs that have `fire_aspect_timer > 0.0` or `burn_timer > 0.0`.
- **`src/state.rs`**:
  - Pass weather rain status into `update_mobs`.

## Verification Plan

- `cargo check --release` to verify compilation.
- `cargo test` to verify existing and new unit tests for daylight burning, water/rain extinguish, and loot drop on death.
