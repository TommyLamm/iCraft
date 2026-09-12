use super::*;

pub(super) fn ensure_end_encounters(
    entities: &mut EntityManager,
    chunks: &WorldColumns,
    player_pos: Vec3,
    time: f32,
) {
    let dragon_exists = entities
        .get_entities_by_type(EntityType::EnderDragon)
        .next()
        .is_some();
    // Dragon completion always places the egg at this canonical location.
    // Looking it up directly avoids scanning every section of every loaded
    // chunk on every encounter-maintenance pass.
    let dragon_completed = chunks.get_block(
        DRAGON_EGG_POSITION.0,
        DRAGON_EGG_POSITION.1,
        DRAGON_EGG_POSITION.2,
    ) == BlockType::DragonEgg;

    // The dragon egg is the persistent world marker that prevents a defeated
    // dragon from being recreated after its entity has been removed.
    if !dragon_exists && !dragon_completed {
        entities.spawn(EntityType::EnderDragon, Vec3::new(0.5, 92.0, 0.5));
        for (x, y, z) in crate::dimension::END_CRYSTAL_TOWERS {
            entities.spawn(
                EntityType::EndCrystal,
                Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5),
            );
        }
    }

    ensure_enderman(entities, chunks, player_pos, time);

    let shulker_count = entities
        .get_entities_by_type(EntityType::Shulker)
        .filter(|entity| entity.health > 0.0)
        .count();
    if shulker_count >= SHULKER_CAP {
        return;
    }

    // Purpur is an unambiguous loaded End City marker. Section palettes reject
    // ordinary End chunks without walking their 4096 voxels. Reservoir-sample
    // one city chunk, then stop at its first valid roof instead of materializing
    // every roof position in every loaded chunk.
    let mut seed = mix64(time.to_bits() as u64 ^ shulker_count as u64);
    let mut city_chunks_seen = 0u64;
    let mut selected_city_chunk = None;
    for (coords, chunk) in chunks.chunks.iter() {
        if !chunk
            .sections
            .iter()
            .flatten()
            .any(|section| section.contains_block(BlockType::Purpur))
        {
            continue;
        }
        city_chunks_seen += 1;
        if next_u64(&mut seed) % city_chunks_seen == 0 {
            selected_city_chunk = Some(coords);
        }
    }
    let Some((cx, cz)) = selected_city_chunk else {
        return;
    };
    let chunk = chunks.chunks.get(&(cx, cz)).expect("chunk present");
    let mut candidate = None;
    'roof: for (section_index, section) in chunk.sections.iter().enumerate().rev() {
        let Some(section) = section else {
            continue;
        };
        if !section.contains_block(BlockType::Purpur) {
            continue;
        }
        let section_y = chunk.min_section_y as i32 + section_index as i32;
        let min_y = section_y * SECTION_SIZE as i32;
        let max_y = min_y + SECTION_SIZE as i32;
        for wy in (min_y.max(1)..max_y).rev() {
            for lx in 0..CHUNK_WIDTH {
                for lz in 0..CHUNK_DEPTH {
                    if chunk.get_block_local(lx, wy, lz) == BlockType::Purpur
                        && chunk.get_block_local(lx, wy + 1, lz) == BlockType::Air
                    {
                        candidate = Some((
                            cx * CHUNK_WIDTH as i32 + lx as i32,
                            wy + 1,
                            cz * CHUNK_DEPTH as i32 + lz as i32,
                        ));
                        break 'roof;
                    }
                }
            }
        }
    }
    let Some((x, y, z)) = candidate else {
        return;
    };
    let pos = Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5);
    if !entities
        .query_radius_types(pos, 2.0, &[EntityType::Shulker])
        .any(|entity| entity.position.distance_squared(pos) < 4.0)
    {
        entities.spawn(EntityType::Shulker, pos);
    }
}

/// Advances dimension mobs and bosses, returning every world/player side
/// effect for the caller to apply. Entities with non-positive health are
/// consumed exactly once because they are removed before this function returns.
pub(super) fn repair_legacy_end_crystal_towers(
    chunks: &WorldColumns,
    live_crystals: &[Vec3],
    events: &mut BossEvents,
) {
    for (center_x, crystal_y, center_z) in crate::dimension::END_CRYSTAL_TOWERS {
        let crystal_position = Vec3::new(
            center_x as f32 + 0.5,
            crystal_y as f32,
            center_z as f32 + 0.5,
        );
        if !live_crystals
            .iter()
            .any(|position| position.distance_squared(crystal_position) < 1.0)
        {
            continue;
        }

        let top_y = crystal_y - 1;
        let Some(((center_chunk_x, center_chunk_z), _)) =
            chunks.world_to_local(center_x, top_y, center_z)
        else {
            continue;
        };
        if !chunks
            .chunks
            .contains_key(&(center_chunk_x, center_chunk_z))
        {
            continue;
        }
        if (1..=top_y)
            .rev()
            .any(|y| chunks.get_block(center_x, y, center_z) == BlockType::Obsidian)
        {
            continue;
        }

        for dx in -2..=2 {
            for dz in -2..=2 {
                if dx * dx + dz * dz > 4 {
                    continue;
                }
                let x = center_x + dx;
                let z = center_z + dz;
                let Some(((chunk_x, chunk_z), _)) = chunks.world_to_local(x, top_y, z) else {
                    continue;
                };
                if !chunks.chunks.contains_key(&(chunk_x, chunk_z)) {
                    continue;
                }
                let ground_y = (1..top_y)
                    .rev()
                    .find(|y| chunks.get_block(x, *y, z) != BlockType::Air)
                    .unwrap_or(0);
                for y in (ground_y + 1)..=top_y {
                    if chunks.get_block(x, y, z) == BlockType::Air {
                        events.block_placements.push(BlockPlacementEvent {
                            position: (x, y, z),
                            block: BlockType::Obsidian,
                        });
                    }
                }
            }
        }
    }
}

pub(super) fn update_dragon(
    dragon: &mut crate::entity::Entity,
    crystals: &[Vec3],
    player_pos: Vec3,
    dt: f32,
    game_mode: GameMode,
    pending_spawns: &mut Vec<(EntityType, Vec3, Vec3, f32)>,
    events: &mut BossEvents,
) {
    let health_ratio = if dragon.max_health > 0.0 {
        dragon.health / dragon.max_health
    } else {
        0.0
    };
    let attack_cycle = dragon.ai_timer % 18.0;
    dragon.ai_phase = if game_mode == GameMode::Creative {
        0
    } else if (10.0..14.5).contains(&attack_cycle) {
        1 // periodic charge, including while the dragon is at full health
    } else if health_ratio > 0.60 {
        0 // high orbit
    } else if health_ratio > 0.30 {
        1 // dive
    } else {
        2 // low orbit and breath
    };

    if !crystals.is_empty() && dragon.health < dragon.max_health {
        dragon.health = (dragon.health + 1.0 * dt).min(dragon.max_health);
    }

    match dragon.ai_phase {
        0 => {
            let angle = dragon.ai_timer * 0.35 + dragon.id as f32 * 0.13;
            let target = Vec3::new(angle.cos() * 42.0, 86.0, angle.sin() * 42.0);
            dragon.velocity = (target - dragon.position).normalize_or_zero() * 12.0;
        }
        1 => {
            let target = player_pos + Vec3::new(0.0, 1.5, 0.0);
            dragon.velocity = (target - dragon.position).normalize_or_zero() * 18.0;
            if game_mode != GameMode::Creative
                && dragon.position.distance_squared(player_pos) < 5.5 * 5.5
                && dragon.action_cooldown <= 0.0
            {
                let impact = dragon.velocity.normalize_or_zero() * 14.0 + Vec3::Y * 4.0;
                events.player_damage.push(
                    PlayerDamageEvent::new(10.0, Some(dragon.id), DamageKind::DragonCharge)
                        .with_knockback(impact),
                );
                dragon.action_cooldown = 1.25;
            }
        }
        _ => {
            let angle = dragon.ai_timer * 0.5;
            let target = player_pos + Vec3::new(angle.cos() * 18.0, 12.0, angle.sin() * 18.0);
            dragon.velocity = (target - dragon.position).normalize_or_zero() * 10.0;
            if game_mode != GameMode::Creative && dragon.action_cooldown <= 0.0 {
                let origin = dragon.position + Vec3::new(0.0, 1.0, 0.0);
                let velocity = (player_pos + Vec3::Y - origin).normalize_or_zero() * 11.0;
                pending_spawns.push((EntityType::DragonBreath, origin, velocity, 6.0));
                dragon.action_cooldown = 2.0;
            }
        }
    }

    // The dragon is moved directly by its AI instead of the generic flying
    // physics path, so keep its model orientation in sync explicitly. This
    // gives the body and head the same forward and vertical flight direction.
    if dragon.velocity.length_squared() > 0.0001 {
        let direction = dragon.velocity.normalize_or_zero();
        dragon.yaw = f32::atan2(direction.x, direction.z);
        // The mob vertex transform rotates +Z toward -Y for positive pitch.
        // Negate the geometric angle so the dragon's +Z head direction
        // follows an upward velocity with a matching upward tilt.
        dragon.pitch = -f32::asin(direction.y.clamp(-1.0, 1.0));
    }
    dragon.position += dragon.velocity * dt;
}

pub(super) fn ensure_enderman(
    entities: &mut EntityManager,
    chunks: &WorldColumns,
    player_pos: Vec3,
    time: f32,
) {
    let count = entities
        .get_entities_by_type(EntityType::Enderman)
        .filter(|entity| entity.health > 0.0)
        .count();
    if count >= ENDERMAN_CAP || chunks.chunks.is_empty() {
        return;
    }

    let loaded: Vec<(i32, i32)> = chunks.chunks.keys().collect();
    let mut seed = mix64(
        time.to_bits() as u64
            ^ (player_pos.x.floor() as i64 as u64).rotate_left(13)
            ^ (player_pos.z.floor() as i64 as u64).rotate_left(37)
            ^ count as u64,
    );
    for _ in 0..8 {
        let (cx, cz) = loaded[(next_u64(&mut seed) as usize) % loaded.len()];
        let wx = cx * CHUNK_WIDTH as i32 + (next_u64(&mut seed) % CHUNK_WIDTH as u64) as i32;
        let wz = cz * CHUNK_DEPTH as i32 + (next_u64(&mut seed) % CHUNK_DEPTH as u64) as i32;
        let Some(y) = open_surface_y(chunks, wx, wz) else {
            continue;
        };
        if chunks.get_block(wx, y - 1, wz) != BlockType::EndStone
            || chunks.get_block(wx, y + 2, wz) != BlockType::Air
        {
            continue;
        }
        let position = Vec3::new(wx as f32 + 0.5, y as f32, wz as f32 + 0.5);
        if position.distance_squared(player_pos) < crate::interaction::player_reach_squared()
            || entities
                .query_radius_types(position, 3.0, &[EntityType::Enderman])
                .next()
                .is_some()
        {
            continue;
        }
        entities.spawn(EntityType::Enderman, position);
        break;
    }
}

pub(super) fn complete_dragon(dragon_id: u64, death_position: Vec3, events: &mut BossEvents) {
    const EXIT_Y: i32 = 73;
    let portal_center = (0, EXIT_Y, 0);
    for x in -1..=1 {
        for z in -1..=1 {
            events.block_placements.push(BlockPlacementEvent {
                position: (x, EXIT_Y, z),
                block: BlockType::EndPortal,
            });
        }
    }
    let egg = (0, EXIT_Y + 5, 0);
    events.block_placements.push(BlockPlacementEvent {
        position: egg,
        block: BlockType::DragonEgg,
    });
    events.drops.push(DropEvent {
        position: death_position,
        item: Item::EndCrystal,
        count: 4,
    });
    events.dragon_completion = Some(DragonCompletionEvent {
        dragon_id,
        portal_center,
        dragon_egg_position: egg,
    });
}
