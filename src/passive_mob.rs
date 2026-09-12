use crate::chunk_manager::WorldColumns;
use crate::entity::{EntityManager, EntityType};
use crate::mob::{try_ambient_spawn, AmbientSpawnRule};
use glam::Vec3;

pub fn spawn_passive_mobs(
    entity_manager: &mut EntityManager,
    chunk_manager: &WorldColumns,
    player_pos: Vec3,
    sky_light_level: u8,
    time: f32,
) {
    if sky_light_level < 10 {
        return;
    }

    let passive_count = entity_manager.count_passive();
    let attempt_modulus = if passive_count == 0 { 5 } else { 100 };
    let max_dist = (chunk_manager.simulation_distance.max(1) as u32 * 16)
        .saturating_sub(4)
        .clamp(12, 64);
    // Former range: (8 + next_rand() % max_dist.saturating_sub(7))
    let dist_span = max_dist.saturating_sub(7).max(1);

    const PASSIVE_TABLE: [EntityType; 4] = [
        EntityType::Pig,
        EntityType::Cow,
        EntityType::Sheep,
        EntityType::Chicken,
    ];
    try_ambient_spawn(
        entity_manager,
        chunk_manager,
        player_pos,
        time,
        15,
        passive_count,
        attempt_modulus,
        8,
        dist_span,
        &PASSIVE_TABLE,
        AmbientSpawnRule::Passive,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_loaded_spawn_region_establishes_passive_population() {
        let seed = 2_563_678_733;
        let mut chunks = crate::chunk_manager::WorldColumns::new_in_dimension(
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
