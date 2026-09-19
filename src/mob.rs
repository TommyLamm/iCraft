use crate::chunk_manager::WorldColumns;
use crate::entity::{EntityManager, EntityType};
use glam::Vec3;

/// Deterministic time-and-position-varying PRNG helper for ambient mob spawning.
pub fn ambient_spawn_rng(player_pos: Vec3, entity_count: usize, time: f32) -> impl FnMut() -> u32 {
    let time_bits = (time * 1000.0) as u32;
    let mut rng_seed = (player_pos.x.to_bits())
        .wrapping_mul(31)
        .wrapping_add(player_pos.z.to_bits())
        .wrapping_add(entity_count as u32)
        .wrapping_add(time_bits.wrapping_mul(2654435761));

    move || crate::rng::lcg32_short(&mut rng_seed)
}

/// Light / ground gate for [`try_ambient_spawn`].
pub enum AmbientSpawnRule {
    /// Hostile: any solid underfoot, two air cells, total light ≤ 7.
    Hostile { sky_light_level: u8 },
    /// Passive: grass underfoot, two air cells, caller already gated daylight.
    Passive,
}

/// Shared ambient spawn attempt: cap / RNG / angle / distance / height / spawn.
pub fn try_ambient_spawn(
    entity_manager: &mut EntityManager,
    chunk_manager: &WorldColumns,
    player_pos: Vec3,
    time: f32,
    cap: usize,
    current_count: usize,
    attempt_modulus: u32,
    dist_min: u32,
    dist_span: u32,
    table: &[EntityType],
    rule: AmbientSpawnRule,
) {
    if current_count >= cap || table.is_empty() || attempt_modulus == 0 || dist_span == 0 {
        return;
    }

    let mut next_rand = ambient_spawn_rng(player_pos, entity_manager.entities.len(), time);
    if next_rand() % attempt_modulus != 0 {
        return;
    }

    let angle = (next_rand() % 360) as f32 * std::f32::consts::PI / 180.0;
    let dist = (dist_min + next_rand() % dist_span) as f32;
    let spawn_x = (player_pos.x + angle.cos() * dist) as i32;
    let spawn_z = (player_pos.z + angle.sin() * dist) as i32;

    let Some(solid_y) = chunk_manager.highest_solid_y(spawn_x, spawn_z) else {
        return;
    };
    let spawn_y = solid_y + 1;
    let height = chunk_manager.dimension.height();
    if !height.contains_y(spawn_y) || !height.contains_y(spawn_y + 1) {
        return;
    }

    let block_feet = chunk_manager.get_block(spawn_x, spawn_y, spawn_z);
    let block_head = chunk_manager.get_block(spawn_x, spawn_y + 1, spawn_z);
    if block_feet != crate::world::BlockType::Air || block_head != crate::world::BlockType::Air {
        return;
    }

    let allowed = match rule {
        AmbientSpawnRule::Hostile { sky_light_level } => {
            let block_light = chunk_manager.get_block_light(spawn_x, spawn_y, spawn_z);
            let effective_sky = if sky_light_level > 10 {
                sky_light_level
            } else {
                4
            };
            effective_sky.max(block_light) <= 7
        }
        AmbientSpawnRule::Passive => {
            chunk_manager.get_block(spawn_x, solid_y, spawn_z) == crate::world::BlockType::Grass
        }
    };
    if !allowed {
        return;
    }

    let et = table[(next_rand() as usize) % table.len()];
    entity_manager.spawn(
        et,
        Vec3::new(spawn_x as f32 + 0.5, spawn_y as f32, spawn_z as f32 + 0.5),
    );
}

pub fn spawn_mobs(
    entity_manager: &mut EntityManager,
    chunk_manager: &WorldColumns,
    player_pos: Vec3,
    sky_light_level: u8,
    time: f32,
) {
    const HOSTILE_TABLE: [EntityType; 3] = [
        EntityType::Zombie,
        EntityType::Skeleton,
        EntityType::Creeper,
    ];
    try_ambient_spawn(
        entity_manager,
        chunk_manager,
        player_pos,
        time,
        20,
        entity_manager.count_hostile(),
        100,
        24,
        56,
        &HOSTILE_TABLE,
        AmbientSpawnRule::Hostile { sky_light_level },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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
