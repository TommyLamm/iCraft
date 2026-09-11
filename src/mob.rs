use crate::chunk_manager::{mark_block_mesh_dependencies, ChunkManager};
use crate::entity::{EntityManager, EntityType};
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
    dirty_meshes: &mut std::collections::HashSet<(i32, i32)>,
    player_physics: &mut PlayerPhysics,
    player_state: &mut PlayerState,
    break_blocks: bool,
    game_mode: GameMode,
    damage_multiplier: f32,
) -> Vec<(i32, i32, i32)> {
    let cx = center.x.floor() as i32;
    let cy = center.y.floor() as i32;
    let cz = center.z.floor() as i32;
    let r_ceil = radius.ceil() as i32;

    let mut dirty_chunks = std::collections::HashSet::new();
    let mut blocks_removed = Vec::new();

    if break_blocks {
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

        // 2. Recalculate lighting for affected spots. Unsupported plants and
        // snow broken above the blast are part of the returned authoritative
        // mutation list too.
        let mut unsupported_removed = Vec::new();
        for &(x, y, z, old_block) in &blocks_removed {
            crate::lighting::update_sky_light_after_removed(
                chunk_manager,
                x,
                y,
                z,
                &mut dirty_chunks,
            );
            crate::lighting::update_block_light_after_removed(
                chunk_manager,
                x,
                y,
                z,
                old_block.properties().light_emission,
                &mut dirty_chunks,
            );

            mark_block_mesh_dependencies(&mut dirty_chunks, x, z);
            chunk_manager.check_and_break_unsupported_above(
                x,
                y,
                z,
                &mut dirty_chunks,
                |pos, block| unsupported_removed.push((pos.0, pos.1, pos.2, block)),
            );
        }
        blocks_removed.extend(unsupported_removed);

        dirty_meshes.extend(dirty_chunks);
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

    blocks_removed
        .into_iter()
        .map(|(x, y, z, _)| (x, y, z))
        .collect()
}

/// Deterministic time-and-position-varying PRNG helper for ambient mob spawning.
pub fn ambient_spawn_rng(player_pos: Vec3, entity_count: usize, time: f32) -> impl FnMut() -> u32 {
    let time_bits = (time * 1000.0) as u32;
    let mut rng_seed = (player_pos.x.to_bits())
        .wrapping_mul(31)
        .wrapping_add(player_pos.z.to_bits())
        .wrapping_add(entity_count as u32)
        .wrapping_add(time_bits.wrapping_mul(2654435761));

    move || {
        rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
        (rng_seed / 65536) % 32768
    }
}

pub fn spawn_mobs(
    entity_manager: &mut EntityManager,
    chunk_manager: &ChunkManager,
    player_pos: Vec3,
    sky_light_level: u8,
    time: f32,
) {
    // Limit total hostile mobs to prevent lag
    if entity_manager.count_hostile() >= 20 {
        return;
    }

    let mut next_rand = ambient_spawn_rng(player_pos, entity_manager.entities.len(), time);

    // ~1% chance per frame to attempt a spawn
    if next_rand() % 100 != 0 {
        return;
    }

    let angle = (next_rand() % 360) as f32 * std::f32::consts::PI / 180.0;
    let dist = (24 + (next_rand() % 56)) as f32;
    let spawn_x = (player_pos.x + angle.cos() * dist) as i32;
    let spawn_z = (player_pos.z + angle.sin() * dist) as i32;

    if let Some(solid_y) = chunk_manager.highest_solid_y(spawn_x, spawn_z) {
        let spawn_y = solid_y + 1;
        let height = chunk_manager.dimension.height();
        if spawn_y >= height.min_y && spawn_y < height.max_y_exclusive() - 1 {
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
                }
            }
        }
    }
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

    #[test]
    fn explosion_reports_authoritative_block_removals_and_can_be_visual_only() {
        let mut manager = ChunkManager::new(1);
        manager
            .chunks
            .insert((0, 0), crate::world::Chunk::new(0, 0));
        manager.set_block(2, 10, 2, crate::world::BlockType::Stone);
        let mut meshes = std::collections::HashSet::new();
        let mut physics = PlayerPhysics::new(Vec3::new(100.0, 100.0, 100.0));
        let mut player = PlayerState::new();
        let center = Vec3::new(2.5, 10.5, 2.5);

        let visual_only = explode(
            center,
            1.0,
            &mut manager,
            &mut meshes,
            &mut physics,
            &mut player,
            false,
            GameMode::Creative,
            0.0,
        );
        assert!(visual_only.is_empty());
        assert_eq!(manager.get_block(2, 10, 2), crate::world::BlockType::Stone);

        let authoritative = explode(
            center,
            1.0,
            &mut manager,
            &mut meshes,
            &mut physics,
            &mut player,
            true,
            GameMode::Creative,
            0.0,
        );
        assert!(authoritative.contains(&(2, 10, 2)));
        assert_eq!(manager.get_block(2, 10, 2), crate::world::BlockType::Air);
    }

    #[test]
    fn test_mob_yaw_faces_player() {
        let mob_pos = Vec3::new(0.0, 0.0, 0.0);
        let player_pos = Vec3::new(0.0, 0.0, 5.0); // Player is at +Z relative to mob
        let dir = player_pos - mob_pos;
        let yaw = f32::atan2(dir.x, dir.z);

        // Front face (+Z) in local coordinates transforms to (sin(yaw), 0, cos(yaw)) in world coordinates.
        let facing_dir = Vec3::new(yaw.sin(), 0.0, yaw.cos()).normalize_or_zero();
        let expected_dir = dir.normalize_or_zero();

        assert!((facing_dir.x - expected_dir.x).abs() < 1e-5);
        assert!((facing_dir.z - expected_dir.z).abs() < 1e-5);
    }
}
