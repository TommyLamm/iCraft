use crate::chunk_manager::{mark_block_mesh_dependencies, ChunkManager};
use crate::entity::{Entity, EntityManager, EntityType};
use crate::inventory::GameMode;
use crate::physics::PlayerPhysics;
use crate::player::PlayerState;
use glam::Vec3;

pub fn calculate_explosion_damage(center: Vec3, player_pos: Vec3) -> f32 {
    let dist = center.distance(player_pos);
    if dist >= 5.0 {
        0.0
    } else {
        (5.0 - dist) * 5.0
    }
}

pub fn explode(
    center: Vec3,
    radius: f32,
    chunk_manager: &mut ChunkManager,
    chunk_meshes: &mut std::collections::HashMap<(i32, i32), crate::state::ChunkMesh>,
    player_physics: &mut PlayerPhysics,
    player_state: &mut PlayerState,
    game_mode: GameMode,
    damage_multiplier: f32,
) {
    let cx = center.x.floor() as i32;
    let cy = center.y.floor() as i32;
    let cz = center.z.floor() as i32;
    let r_ceil = radius.ceil() as i32;

    let mut dirty_chunks = std::collections::HashSet::new();
    let mut blocks_removed = Vec::new();

    // 1. Break blocks in radius
    for x in (cx - r_ceil)..=(cx + r_ceil) {
        for y in (cy - r_ceil)..=(cy + r_ceil) {
            for z in (cz - r_ceil)..=(cz + r_ceil) {
                let dx = x as f32 + 0.5 - center.x;
                let dy = y as f32 + 0.5 - center.y;
                let dz = z as f32 + 0.5 - center.z;
                if dx * dx + dy * dy + dz * dz <= radius * radius {
                    let block = chunk_manager.get_block(x, y, z);
                    if block != crate::world::BlockType::Air
                        && block != crate::world::BlockType::Bedrock
                    {
                        chunk_manager.set_block(x, y, z, crate::world::BlockType::Air);
                        blocks_removed.push((x, y, z, block));
                    }
                }
            }
        }
    }

    // 2. Recalculate lighting for affected spots
    for (x, y, z, old_block) in blocks_removed {
        crate::lighting::update_sky_light_after_removed(chunk_manager, x, y, z, &mut dirty_chunks);
        crate::lighting::update_block_light_after_removed(
            chunk_manager,
            x,
            y,
            z,
            old_block.properties().light_emission,
            &mut dirty_chunks,
        );

        mark_block_mesh_dependencies(&mut dirty_chunks, x, z);
    }

    // Mark chunk meshes dirty
    for (chx, chz) in dirty_chunks {
        if let Some(mesh) = chunk_meshes.get_mut(&(chx, chz)) {
            mesh.dirty = true;
        }
    }

    // 3. Player damage and knockback
    if game_mode != GameMode::Creative {
        let dist = center.distance(player_physics.position);
        if dist < 5.0 {
            let dmg = calculate_explosion_damage(center, player_physics.position);
            if dmg > 0.0 {
                // Inflict damage using player's existing interface
                player_state.take_damage(
                    dmg * damage_multiplier,
                    crate::player::DamageSource::Explosion,
                );
                let dir = (player_physics.position - center).normalize_or_zero();
                player_physics.velocity += dir * 12.0 + Vec3::new(0.0, 5.0, 0.0);
            }
        }
    }
}

fn is_under_sun(chunk_manager: &ChunkManager, pos: Vec3, sky_light_level: u8) -> bool {
    if sky_light_level <= 10 {
        return false;
    }
    let mx = pos.x.floor() as i32;
    let my = pos.y.floor() as i32;
    let mz = pos.z.floor() as i32;

    if chunk_manager.get_sky_light(mx, my, mz) < 12 {
        return false;
    }

    // Check if there is any solid block above
    for y in (my + 1)..(crate::world::CHUNK_HEIGHT as i32) {
        if chunk_manager.get_block(mx, y, mz).properties().is_solid {
            return false;
        }
    }
    true
}

fn get_highest_solid_y(chunk_manager: &ChunkManager, x: i32, z: i32) -> Option<i32> {
    for y in (0..crate::world::CHUNK_HEIGHT as i32).rev() {
        if chunk_manager.get_block(x, y, z).properties().is_solid {
            return Some(y);
        }
    }
    None
}

pub fn spawn_mobs(
    entity_manager: &mut EntityManager,
    chunk_manager: &ChunkManager,
    player_pos: Vec3,
    sky_light_level: u8,
    time: f32,
) {
    if entity_manager.entities.len() >= 25 {
        return;
    }

    // Use time-varying seed so RNG produces different results each frame
    let time_bits = (time * 1000.0) as u32;
    let mut rng_seed = (player_pos.x.to_bits())
        .wrapping_mul(31)
        .wrapping_add(player_pos.z.to_bits())
        .wrapping_add(entity_manager.entities.len() as u32)
        .wrapping_add(time_bits.wrapping_mul(2654435761));

    let mut next_rand = || {
        rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
        (rng_seed / 65536) % 32768
    };

    // ~1% chance per frame to attempt a spawn
    if next_rand() % 100 != 0 {
        return;
    }

    let angle = (next_rand() % 360) as f32 * std::f32::consts::PI / 180.0;
    let dist = (24 + (next_rand() % 56)) as f32;
    let spawn_x = (player_pos.x + angle.cos() * dist) as i32;
    let spawn_z = (player_pos.z + angle.sin() * dist) as i32;

    if let Some(solid_y) = get_highest_solid_y(chunk_manager, spawn_x, spawn_z) {
        let spawn_y = solid_y + 1;
        if spawn_y > 0 && spawn_y < (crate::world::CHUNK_HEIGHT as i32 - 2) {
            if chunk_manager.get_block(spawn_x, spawn_y, spawn_z) == crate::world::BlockType::Air
                && chunk_manager.get_block(spawn_x, spawn_y + 1, spawn_z)
                    == crate::world::BlockType::Air
            {
                let block_light = chunk_manager.get_block_light(spawn_x, spawn_y, spawn_z);
                let effective_sky = if sky_light_level > 10 {
                    sky_light_level
                } else {
                    4
                };
                let total_light = effective_sky.max(block_light);

                if total_light <= 7 {
                    let r = next_rand() % 3;
                    let et = match r {
                        0 => EntityType::Zombie,
                        1 => EntityType::Skeleton,
                        _ => EntityType::Creeper,
                    };
                    entity_manager.spawn(
                        et,
                        Vec3::new(spawn_x as f32 + 0.5, spawn_y as f32, spawn_z as f32 + 0.5),
                    );
                    println!(
                        "[Debug] Spawned {:?} at ({}, {}, {})",
                        et, spawn_x, spawn_y, spawn_z
                    );
                }
            }
        }
    }
}

pub fn update_mobs(
    entity_manager: &mut EntityManager,
    chunk_manager: &mut ChunkManager,
    chunk_meshes: &mut std::collections::HashMap<(i32, i32), crate::state::ChunkMesh>,
    player_physics: &mut PlayerPhysics,
    player_state: &mut PlayerState,
    game_mode: GameMode,
    sky_light_level: u8,
    dt: f32,
    audio_manager: &mut crate::audio::AudioManager,
    listener_right: Vec3,
    player_invisible: bool,
    damage_multiplier: f32,
) {
    let player_pos = player_physics.position;

    // Despawn out-of-bounds mobs
    entity_manager.entities.retain(|entity| {
        if entity.entity_type == EntityType::Arrow || entity.entity_type == EntityType::SplashPotion
        {
            true
        } else {
            entity.position.distance(player_pos) <= 128.0
        }
    });

    // We collect arrows to spawn, creeper explosions to trigger, and sound effects to print.
    let mut arrows_to_spawn = Vec::new();
    let mut explosions = Vec::new();
    let mut hit_player = false;
    let mut hit_player_amount = 0.0;

    for entity in &mut entity_manager.entities {
        // Invulnerable frame countdown
        if entity.invulnerable_time > 0.0 {
            entity.invulnerable_time = (entity.invulnerable_time - dt).max(0.0);
        }

        if entity.entity_type == EntityType::SplashPotion {
            continue;
        }

        // Apply physical update
        entity.update_physics(dt, chunk_manager);

        if entity.fire_aspect_timer > 0.0 {
            entity.fire_aspect_timer = (entity.fire_aspect_timer - dt).max(0.0);
            entity.burn_damage_timer += dt;
            if entity.burn_damage_timer >= 1.0 {
                entity.burn_damage_timer -= 1.0;
                entity.health -= 1.0;
            }
        } else {
            entity.burn_damage_timer = 0.0;
        }

        if entity.entity_type == EntityType::Arrow {
            // Check collision with solid blocks
            let ax = entity.position.x.floor() as i32;
            let ay = entity.position.y.floor() as i32;
            let az = entity.position.z.floor() as i32;
            if chunk_manager.get_block(ax, ay, az).properties().is_solid {
                // Stuck in wall, mark arrow dead
                entity.health = -1.0;
                continue;
            }

            // Check collision with player AABB
            let player_aabb = player_physics.get_aabb();
            if !entity.friendly_projectile && entity.get_aabb().intersects(&player_aabb) {
                hit_player = true;
                hit_player_amount = 4.0;
                entity.health = -1.0; // Destroy arrow
            }
            continue;
        }

        // Sunlight burning logic
        if (entity.entity_type == EntityType::Zombie || entity.entity_type == EntityType::Skeleton)
            && is_under_sun(chunk_manager, entity.position, sky_light_level)
        {
            entity.burn_timer += dt;
            if entity.burn_timer >= 1.0 {
                entity.burn_timer = 0.0;
                entity.health -= 1.0; // Burn damage
            }
        } else {
            entity.burn_timer = 0.0;
        }

        // Dropped items only need physics; skip all hostile AI.
        if entity.entity_type == EntityType::DroppedItem {
            continue;
        }

        // AI decision logic
        let is_hostile = entity.entity_type == EntityType::Zombie
            || entity.entity_type == EntityType::Skeleton
            || entity.entity_type == EntityType::Creeper;

        let dist = entity.position.distance(player_pos);
        if is_hostile && dist <= 16.0 && !player_invisible {
            entity.target_player = true;

            // Turn towards player
            let dir = player_pos - entity.position;
            entity.yaw = f32::atan2(-dir.x, -dir.z);

            let walk_dir = Vec3::new(dir.x, 0.0, dir.z).normalize_or_zero();

            match entity.entity_type {
                EntityType::Zombie => {
                    // Chase player
                    let speed = 2.5;
                    entity.velocity.x = walk_dir.x * speed;
                    entity.velocity.z = walk_dir.z * speed;

                    // Obstacle jump check
                    let next_x = entity.position.x + walk_dir.x * 0.4;
                    let next_z = entity.position.z + walk_dir.z * 0.4;
                    let bx = next_x.floor() as i32;
                    let bz = next_z.floor() as i32;
                    let by = entity.position.y.floor() as i32;
                    if entity.on_ground
                        && chunk_manager.get_block(bx, by, bz).properties().is_solid
                        && !chunk_manager
                            .get_block(bx, by + 2, bz)
                            .properties()
                            .is_solid
                    {
                        entity.velocity.y = 8.0;
                    }

                    // Melee attack
                    if dist <= 1.2 && entity.action_cooldown <= 0.0 {
                        hit_player = true;
                        hit_player_amount = 3.0;
                        entity.action_cooldown = 1.0; // Cooldown
                    }
                }
                EntityType::Skeleton => {
                    // Keep distance AI
                    let speed = 2.5;
                    if dist < 8.0 {
                        // Back away
                        entity.velocity.x = -walk_dir.x * speed;
                        entity.velocity.z = -walk_dir.z * speed;
                    } else if dist > 12.0 {
                        // Move closer
                        entity.velocity.x = walk_dir.x * speed;
                        entity.velocity.z = walk_dir.z * speed;
                    } else {
                        // Stop and shoot
                        entity.velocity.x = 0.0;
                        entity.velocity.z = 0.0;
                    }

                    // Obstacle jump check
                    if entity.velocity.length_squared() > 0.0 {
                        let walk_vel = entity.velocity.normalize_or_zero();
                        let next_x = entity.position.x + walk_vel.x * 0.4;
                        let next_z = entity.position.z + walk_vel.z * 0.4;
                        let bx = next_x.floor() as i32;
                        let bz = next_z.floor() as i32;
                        let by = entity.position.y.floor() as i32;
                        if entity.on_ground
                            && chunk_manager.get_block(bx, by, bz).properties().is_solid
                            && !chunk_manager
                                .get_block(bx, by + 2, bz)
                                .properties()
                                .is_solid
                        {
                            entity.velocity.y = 8.0;
                        }
                    }

                    // Shooting arrows
                    if entity.action_cooldown <= 0.0 {
                        // Shoot Arrow
                        let spawn_pos = entity.position + Vec3::new(0.0, 1.4, 0.0);
                        let mut shoot_dir =
                            (player_pos + Vec3::new(0.0, 1.0, 0.0) - spawn_pos).normalize_or_zero();
                        // Add slight gravity correction
                        shoot_dir.y += 0.08;
                        let arrow_vel = shoot_dir.normalize() * 18.0;

                        arrows_to_spawn.push((spawn_pos, arrow_vel));
                        entity.action_cooldown = 2.0; // Shooting cooldown

                        let listener_pos = player_physics.position + Vec3::new(0.0, 1.6, 0.0);
                        audio_manager.play_sound_3d(
                            crate::audio::SoundId::ArrowShoot,
                            spawn_pos,
                            listener_pos,
                            listener_right,
                        );
                    }
                }
                EntityType::Creeper => {
                    // Chase player
                    let speed = if entity.is_ignited { 0.0 } else { 3.0 };
                    entity.velocity.x = walk_dir.x * speed;
                    entity.velocity.z = walk_dir.z * speed;

                    // Obstacle jump check
                    if !entity.is_ignited {
                        let next_x = entity.position.x + walk_dir.x * 0.4;
                        let next_z = entity.position.z + walk_dir.z * 0.4;
                        let bx = next_x.floor() as i32;
                        let bz = next_z.floor() as i32;
                        let by = entity.position.y.floor() as i32;
                        if entity.on_ground
                            && chunk_manager.get_block(bx, by, bz).properties().is_solid
                            && !chunk_manager
                                .get_block(bx, by + 2, bz)
                                .properties()
                                .is_solid
                        {
                            entity.velocity.y = 8.0;
                        }
                    }

                    // Ignite countdown
                    if dist <= 2.0 {
                        if !entity.is_ignited {
                            println!("[Debug] Creeper: ssssssssss...");
                            entity.is_ignited = true;
                            entity.action_cooldown = 1.5; // Fuse duration

                            let listener_pos = player_physics.position + Vec3::new(0.0, 1.6, 0.0);
                            audio_manager.play_sound_3d(
                                crate::audio::SoundId::CreeperIgnition,
                                entity.position,
                                listener_pos,
                                listener_right,
                            );
                        }
                    } else if dist > 3.5 {
                        if entity.is_ignited {
                            println!("[Debug] Creeper: fuse defused.");
                            entity.is_ignited = false;
                            entity.action_cooldown = 0.0;
                        }
                    }

                    if entity.is_ignited {
                        entity.action_cooldown -= dt;
                        if entity.action_cooldown <= 0.0 {
                            // Trigger explosion!
                            explosions.push(entity.position);
                            entity.health = -1.0; // Destroy creeper
                        }
                    }
                }
                _ => {}
            }
        } else {
            entity.target_player = false;
        }

        // Tick down cooldowns
        if entity.action_cooldown > 0.0 && !entity.is_ignited {
            entity.action_cooldown = (entity.action_cooldown - dt).max(0.0);
        }
    }

    // Spawn created arrows
    for (pos, vel) in arrows_to_spawn {
        let mut arrow = Entity::new(0, EntityType::Arrow, pos);
        arrow.velocity = vel;
        entity_manager.spawn(EntityType::Arrow, pos);
        // Fix the newly spawned arrow's velocity in the manager
        if let Some(new_arrow) = entity_manager.entities.last_mut() {
            new_arrow.velocity = vel;
        }
    }

    // Trigger explosions
    for exp_pos in explosions {
        explode(
            exp_pos,
            3.0, // radius
            chunk_manager,
            chunk_meshes,
            player_physics,
            player_state,
            game_mode,
            damage_multiplier,
        );

        let listener_pos = player_physics.position + Vec3::new(0.0, 1.6, 0.0);
        audio_manager.play_sound_3d(
            crate::audio::SoundId::Explosion,
            exp_pos,
            listener_pos,
            listener_right,
        );
    }

    // Handle player taking damage
    if hit_player && game_mode != GameMode::Creative {
        // Player is hit, apply damage and small knockback
        let died = player_state.take_damage(
            hit_player_amount * damage_multiplier,
            crate::player::DamageSource::Mob,
        );
        if died {
            println!("[Debug] Player died from mob attack!");
            player_state.is_dead = true;
            player_state.death_reason = Some(crate::player::DamageSource::Mob);
        } else {
            // Apply knockback
            let flat_dir = (player_pos - entity_manager.entities[0].position).normalize_or_zero(); // general direction
            player_physics.velocity += flat_dir * 8.0 + Vec3::new(0.0, 3.0, 0.0);
        }
    }

    // Clean up dead entities (health < 0 or health == 0)
    entity_manager
        .entities
        .retain(|entity| entity.health >= 0.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_explosion_damage() {
        let center = Vec3::new(0.0, 0.0, 0.0);

        // Exact center: maximum damage
        let d1 = calculate_explosion_damage(center, center);
        assert_eq!(d1, 25.0);

        // Distance = 2.0: damage = (5.0 - 2.0) * 5.0 = 15.0
        let d2 = calculate_explosion_damage(center, Vec3::new(2.0, 0.0, 0.0));
        assert_eq!(d2, 15.0);

        // Distance = 5.5: 0 damage
        let d3 = calculate_explosion_damage(center, Vec3::new(5.5, 0.0, 0.0));
        assert_eq!(d3, 0.0);
    }
}
