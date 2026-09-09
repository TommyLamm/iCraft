use crate::chunk_manager::ChunkManager;
use crate::entity::{EntityManager, EntityType};
use glam::Vec3;

pub fn spawn_passive_mobs(
    entity_manager: &mut EntityManager,
    chunk_manager: &ChunkManager,
    player_pos: Vec3,
    sky_light_level: u8,
    time: f32,
) {
    // Limit total passive mobs to prevent lag.
    let passive_count = entity_manager.count_passive();
    if passive_count >= 15 {
        return;
    }

    let mut next_rand =
        crate::mob::ambient_spawn_rng(player_pos, entity_manager.entities.len(), time);

    // Establish the first visible population promptly, then fall back to the
    // lower ambient spawn rate.
    let attempt_modulus = if passive_count == 0 { 5 } else { 100 };
    if sky_light_level < 10 || next_rand() % attempt_modulus != 0 {
        return;
    }

    let angle = (next_rand() % 360) as f32 * std::f32::consts::PI / 180.0;
    // Stay inside the loaded view radius.  The former 24..79 block range was
    // usually outside a fresh world's authoritative chunks, so every spawn
    // attempt sampled Air and silently failed.
    let max_dist = (chunk_manager.render_distance.max(1) as u32 * 16)
        .saturating_sub(4)
        .clamp(12, 64);
    let dist = (8 + next_rand() % max_dist.saturating_sub(7)) as f32;
    let spawn_x = (player_pos.x + angle.cos() * dist) as i32;
    let spawn_z = (player_pos.z + angle.sin() * dist) as i32;

    let height = chunk_manager.dimension.height();
    if let Some(solid_y) = chunk_manager.highest_solid_y(spawn_x, spawn_z) {
        let spawn_y = solid_y + 1;
        if height.contains_y(spawn_y) && height.contains_y(spawn_y + 1) {
            let block_below = chunk_manager.get_block(spawn_x, solid_y, spawn_z);
            let block_feet = chunk_manager.get_block(spawn_x, spawn_y, spawn_z);
            let block_head = chunk_manager.get_block(spawn_x, spawn_y + 1, spawn_z);

            // Passive mobs spawn on Grass Blocks under daylight
            if block_below == crate::world::BlockType::Grass
                && block_feet == crate::world::BlockType::Air
                && block_head == crate::world::BlockType::Air
            {
                let r = next_rand() % 4;
                let et = match r {
                    0 => EntityType::Pig,
                    1 => EntityType::Cow,
                    2 => EntityType::Sheep,
                    _ => EntityType::Chicken,
                };
                entity_manager.spawn(
                    et,
                    Vec3::new(spawn_x as f32 + 0.5, spawn_y as f32, spawn_z as f32 + 0.5),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_loaded_spawn_region_establishes_passive_population() {
        let seed = 2_563_678_733;
        let mut chunks = crate::chunk_manager::ChunkManager::new_in_dimension(
            2,
            crate::dimension::Dimension::Overworld,
        );
        for cx in -2..=2 {
            for cz in -2..=2 {
                chunks.chunks.insert(
                    (cx, cz),
                    crate::dimension::generate_chunk(
                        crate::dimension::Dimension::Overworld,
                        cx,
                        cz,
                        seed,
                    ),
                );
            }
        }
        let mut entities = EntityManager::new();
        for tick in 0..400 {
            spawn_passive_mobs(
                &mut entities,
                &chunks,
                Vec3::new(8.0, 80.0, 8.0),
                15,
                tick as f32 / 20.0,
            );
            if entities.count_passive() > 0 {
                break;
            }
        }
        assert!(entities.count_passive() > 0);
    }

    #[test]
    fn partner_query_matches_independent_radius_oracle() {
        let mut entities = EntityManager::new();
        let center = Vec3::new(0.0, 64.0, 0.0);
        let near = entities.spawn(EntityType::Cow, Vec3::new(3.0, 64.0, 4.0));
        entities.spawn(EntityType::Cow, Vec3::new(40.0, 64.0, 0.0));
        entities.spawn(EntityType::Pig, Vec3::new(2.0, 64.0, 0.0));
        let indexed: std::collections::HashSet<u64> = entities
            .query_radius_types(center, 8.0, &[EntityType::Cow])
            .map(|entity| entity.id)
            .collect();
        let oracle: std::collections::HashSet<u64> = entities
            .entities
            .iter()
            .filter(|entity| {
                entity.entity_type == EntityType::Cow
                    && entity.position.distance_squared(center) <= 64.0
            })
            .map(|entity| entity.id)
            .collect();
        assert_eq!(indexed, oracle);
        assert!(indexed.contains(&near));
    }
}
