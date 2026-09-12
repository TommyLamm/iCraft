use super::*;

pub(super) fn ensure_nether_mob(
    entities: &mut EntityManager,
    chunks: &WorldColumns,
    player_pos: Vec3,
    time: f32,
) {
    // Global cap maintenance: count only indexed hostile buckets, not a
    // candidate radius/target query.
    debug_assert!(crate::entity::is_global_entity_maintenance(
        EntityIterationKind::GlobalCleanup
    ));
    let nether_count = [EntityType::Blaze, EntityType::Piglin, EntityType::Husk]
        .into_iter()
        .map(|kind| {
            entities
                .get_entities_by_type(kind)
                .filter(|entity| entity.health > 0.0)
                .count()
        })
        .sum::<usize>();
    if nether_count >= NETHER_MOB_CAP || chunks.chunks.is_empty() {
        return;
    }

    let mut seed = mix64(
        time.to_bits() as u64
            ^ (player_pos.x.floor() as i64 as u64).rotate_left(17)
            ^ (player_pos.z.floor() as i64 as u64).rotate_left(39)
            ^ nether_count as u64,
    );
    let loaded: Vec<(i32, i32)> = chunks.chunks.keys().collect();
    let (cx, cz) = loaded[(next_u64(&mut seed) as usize) % loaded.len()];
    let wx = cx * CHUNK_WIDTH as i32 + (next_u64(&mut seed) % CHUNK_WIDTH as u64) as i32;
    let wz = cz * CHUNK_DEPTH as i32 + (next_u64(&mut seed) % CHUNK_DEPTH as u64) as i32;
    let Some(y) = open_surface_y(chunks, wx, wz) else {
        return;
    };

    // Do not materialize an enemy directly on top of the player.
    let pos = Vec3::new(wx as f32 + 0.5, y as f32, wz as f32 + 0.5);
    if pos.distance_squared(player_pos) < crate::interaction::player_reach_squared() {
        return;
    }
    let kind = match next_u64(&mut seed) % 6 {
        0 | 1 => EntityType::Blaze,
        2 | 3 | 4 => EntityType::Piglin,
        _ => EntityType::Husk,
    };
    entities.spawn(kind, pos);
}
