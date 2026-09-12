// Tests extracted from boss.rs (Plan 27).

use super::*;
use crate::world::Chunk;
use std::collections::HashMap;

#[test]
fn legacy_live_crystal_without_tower_gets_obsidian_support_events() {
    let mut chunks = WorldColumns::new(1);
    let mut chunk = Chunk::new(2, 0);
    let local_x = 42usize.rem_euclid(CHUNK_WIDTH);
    for y in 1..78 {
        chunk.set_block_local(local_x, y, 0, BlockType::Air);
    }
    chunk.set_block_local(local_x, 65, 0, BlockType::EndStone);
    chunks.chunks.insert((2, 0), chunk);

    let mut events = BossEvents::default();
    repair_legacy_end_crystal_towers(&chunks, &[Vec3::new(42.5, 78.0, 0.5)], &mut events);

    assert!(events.block_placements.iter().any(|placement| {
        placement.position == (42, 77, 0) && placement.block == BlockType::Obsidian
    }));
}

fn pattern_blocks(axis: (i32, i32)) -> HashMap<BlockPos, BlockType> {
    let mut blocks = HashMap::new();
    for offset in -1..=1 {
        blocks.insert(
            (axis.0 * offset, 12, axis.1 * offset),
            BlockType::WitherSkeletonSkull,
        );
        blocks.insert((axis.0 * offset, 11, axis.1 * offset), BlockType::SoulSand);
    }
    blocks.insert((0, 10, 0), BlockType::SoulSand);
    blocks
}

#[test]
fn detects_x_oriented_wither_pattern() {
    let blocks = pattern_blocks((1, 0));
    let found = detect_wither_pattern((1, 12, 0), |pos| {
        blocks.get(&pos).copied().unwrap_or(BlockType::Air)
    });
    assert_eq!(found.as_ref().map(Vec::len), Some(7));
    assert!(found.unwrap().contains(&(0, 10, 0)));
}

#[test]
fn detects_z_oriented_wither_pattern() {
    let blocks = pattern_blocks((0, 1));
    let found = detect_wither_pattern((0, 11, -1), |pos| {
        blocks.get(&pos).copied().unwrap_or(BlockType::Air)
    });
    assert_eq!(found.as_ref().map(Vec::len), Some(7));
}

#[test]
fn rejects_near_miss_wither_pattern() {
    let mut blocks = pattern_blocks((1, 0));
    blocks.remove(&(0, 10, 0));
    assert!(detect_wither_pattern((0, 12, 0), |pos| {
        blocks.get(&pos).copied().unwrap_or(BlockType::Air)
    })
    .is_none());
}

#[test]
fn boss_bar_progress_is_clamped() {
    let mut entities = EntityManager::new();
    entities.spawn(EntityType::Wither, Vec3::ZERO);
    entities.entities[0].health = entities.entities[0].max_health * 2.0;
    assert_eq!(active_boss_hud(&entities).unwrap().progress, 1.0);
    entities.entities[0].health = -10.0;
    assert_eq!(active_boss_hud(&entities).unwrap().progress, 0.0);
}

#[test]
fn dragon_changes_phase_and_crystal_heals_it() {
    let mut entities = EntityManager::new();
    entities.spawn(EntityType::EnderDragon, Vec3::new(0.0, 80.0, 0.0));
    entities.spawn(EntityType::EndCrystal, Vec3::new(8.0, 80.0, 0.0));
    entities.entities[0].health = 100.0;
    let chunks = WorldColumns::new(1);

    update_dimension_entities(
        Dimension::End,
        &mut entities,
        &chunks,
        &[(Vec3::ZERO, Vec3::ZERO)],
        0.2,
        GameMode::Survival,
    );

    let dragon = entities
        .get_entities_by_type(EntityType::EnderDragon)
        .next()
        .unwrap();
    assert_eq!(dragon.ai_phase, 1);
    assert!(dragon.health > 100.0);
}

#[test]
fn dragon_orientation_follows_flight_velocity() {
    let mut dragon =
        crate::entity::Entity::new(1, EntityType::EnderDragon, Vec3::new(0.0, 80.0, 0.0));
    let mut pending_spawns = Vec::new();
    let mut events = BossEvents::default();

    update_dragon(
        &mut dragon,
        &[],
        Vec3::new(0.0, 80.0, 0.0),
        0.0,
        GameMode::Survival,
        &mut pending_spawns,
        &mut events,
    );

    let direction = dragon.velocity.normalize();
    assert!((dragon.yaw - f32::atan2(direction.x, direction.z)).abs() < 1e-6);
    assert!((dragon.pitch + f32::asin(direction.y)).abs() < 1e-6);
}

#[test]
fn full_health_dragon_periodically_charges_and_knocks_player_back() {
    let mut entities = EntityManager::new();
    let dragon_id = entities.spawn(EntityType::EnderDragon, Vec3::new(0.0, 80.0, 4.0));
    entities.get_by_id_mut(dragon_id).unwrap().ai_timer = 9.9;
    let chunks = WorldColumns::new(1);

    let events = update_dimension_entities(
        Dimension::End,
        &mut entities,
        &chunks,
        &[(Vec3::new(0.0, 80.0, 0.0), Vec3::ZERO)],
        0.2,
        GameMode::Survival,
    );

    assert_eq!(entities.get_by_id(dragon_id).unwrap().ai_phase, 1);
    let hit = events
        .player_damage
        .iter()
        .find(|hit| hit.kind == DamageKind::DragonCharge)
        .expect("charging dragon should collide with a nearby player");
    assert!(hit.knockback.length() > 10.0);
    assert!(hit.knockback.y > 0.0);
}

#[test]
fn end_dimension_spawns_enderman_on_end_stone() {
    let mut chunks = WorldColumns::new(1);
    let mut chunk = Chunk::new(0, 0);
    for x in 0..CHUNK_WIDTH {
        for z in 0..CHUNK_DEPTH {
            for y in chunk.world_y_range() {
                chunk.set_block_local(x, y, z, BlockType::Air);
            }
            chunk.set_block_local(x, 64, z, BlockType::EndStone);
        }
    }
    chunks.chunks.insert((0, 0), chunk);
    let mut entities = EntityManager::new();

    ensure_dimension_entities(
        Dimension::End,
        &mut entities,
        &chunks,
        Vec3::new(100.0, 65.0, 100.0),
        3.0,
    );

    let enderman = entities
        .get_entities_by_type(EntityType::Enderman)
        .next()
        .expect("End terrain should spawn an Enderman");
    assert_eq!(
        chunks.get_block(
            enderman.position.x.floor() as i32,
            enderman.position.y.floor() as i32 - 1,
            enderman.position.z.floor() as i32,
        ),
        BlockType::EndStone
    );
}

#[test]
fn periodic_legacy_repair_runs_on_start_and_interval_boundaries() {
    assert!(periodic_work_due(0.0, 0.05, 1.0));
    assert!(!periodic_work_due(0.20, 0.05, 1.0));
    assert!(periodic_work_due(0.99, 0.05, 1.0));
}

#[test]
fn enderman_head_gaze_is_detected_from_every_direction() {
    let head = Vec3::new(4.0, 70.0, -3.0);
    for offset in [
        Vec3::X * 10.0,
        -Vec3::X * 10.0,
        Vec3::Y * 10.0,
        -Vec3::Y * 10.0,
        Vec3::Z * 10.0,
        -Vec3::Z * 10.0,
    ] {
        let player_eye = head + offset;
        assert!(player_is_gazing_at_enderman_head(
            player_eye,
            head - player_eye,
            head
        ));
    }

    // The crosshair may sit a few blocks beside the exact head center at
    // this distance and still count as deliberately watching it.
    let player_eye = head - Vec3::Z * 10.0;
    let approximate_look = head + Vec3::X * 3.0 - player_eye;
    assert!(player_is_gazing_at_enderman_head(
        player_eye,
        approximate_look,
        head
    ));
}

#[test]
fn enderman_only_attacks_after_three_seconds_of_head_gaze() {
    let mut entities = EntityManager::new();
    let id = entities.spawn(EntityType::Enderman, Vec3::new(0.0, 0.0, 10.0));
    entities.get_by_id_mut(id).unwrap().enderman_gaze_timer = 2.7;
    let chunks = WorldColumns::new(1);
    let player = Vec3::ZERO;
    let look = (Vec3::new(0.0, 2.62, 10.0) - Vec3::Y * 1.62).normalize();

    update_dimension_entities(
        Dimension::End,
        &mut entities,
        &chunks,
        &[(player, look)],
        0.2,
        GameMode::Survival,
    );
    assert_eq!(entities.get_by_id(id).unwrap().ai_phase, 0);
    assert!(!entities.get_by_id(id).unwrap().target_player);

    let moved_head = entities.get_by_id(id).unwrap().position + Vec3::Y * 2.62;
    let look = (moved_head - Vec3::Y * 1.62).normalize();
    update_dimension_entities(
        Dimension::End,
        &mut entities,
        &chunks,
        &[(player, look)],
        0.2,
        GameMode::Survival,
    );
    assert_eq!(entities.get_by_id(id).unwrap().ai_phase, 1);
    assert!(entities.get_by_id(id).unwrap().target_player);
}

#[test]
fn enderman_gaze_timer_resets_when_player_looks_away() {
    let mut entities = EntityManager::new();
    let id = entities.spawn(EntityType::Enderman, Vec3::new(0.0, 0.0, 10.0));
    entities.get_by_id_mut(id).unwrap().enderman_gaze_timer = 2.9;
    let chunks = WorldColumns::new(1);

    update_dimension_entities(
        Dimension::End,
        &mut entities,
        &chunks,
        &[(Vec3::ZERO, -Vec3::Z)],
        0.2,
        GameMode::Survival,
    );

    let enderman = entities.get_by_id(id).unwrap();
    assert_eq!(enderman.ai_phase, 0);
    assert_eq!(enderman.enderman_gaze_timer, 0.0);
    assert!(!enderman.target_player);
}

#[test]
fn calm_enderman_wanders_without_targeting_player() {
    let mut entities = EntityManager::new();
    let id = entities.spawn(EntityType::Enderman, Vec3::new(0.0, 64.0, 10.0));
    let chunks = WorldColumns::new(1);

    update_dimension_entities(
        Dimension::End,
        &mut entities,
        &chunks,
        &[(Vec3::ZERO, -Vec3::Z)],
        0.1,
        GameMode::Survival,
    );

    let enderman = entities.get_by_id(id).unwrap();
    assert_eq!(enderman.ai_phase, 0);
    assert!(!enderman.target_player);
    assert!(Vec3::new(enderman.velocity.x, 0.0, enderman.velocity.z).length() > 1.0);
}

#[test]
fn provoked_enderman_chases_and_damages_player() {
    let mut entities = EntityManager::new();
    let id = entities.spawn(EntityType::Enderman, Vec3::new(0.0, 0.0, 2.0));
    entities.get_by_id_mut(id).unwrap().enderman_gaze_timer = 2.95;
    let chunks = WorldColumns::new(1);
    let player = Vec3::ZERO;
    let head = entities.get_by_id(id).unwrap().position + Vec3::Y * 2.62;
    let look = head - (player + Vec3::Y * 1.62);

    let events = update_dimension_entities(
        Dimension::End,
        &mut entities,
        &chunks,
        &[(player, look)],
        0.1,
        GameMode::Survival,
    );

    let enderman = entities.get_by_id(id).unwrap();
    assert_eq!(enderman.ai_phase, 1);
    assert!(enderman.target_player);
    assert!(Vec3::new(enderman.velocity.x, 0.0, enderman.velocity.z).length() > 3.0);
    assert!(events
        .player_damage
        .iter()
        .any(|damage| damage.source_entity == Some(id) && damage.amount == 7.0));
}

#[test]
fn enderman_enters_attack_mode_after_continuous_three_second_head_gaze() {
    let mut entities = EntityManager::new();
    let id = entities.spawn(EntityType::Enderman, Vec3::new(8.0, 64.0, 8.0));
    let mut chunks = WorldColumns::new(1);
    let mut chunk = Chunk::new(0, 0);
    for x in 0..CHUNK_WIDTH {
        for z in 0..CHUNK_DEPTH {
            chunk.set_block_local(x, 63, z, BlockType::EndStone);
        }
    }
    chunks.chunks.insert((0, 0), chunk);
    let player = Vec3::new(8.0, 64.0, 0.0);

    for _ in 0..12 {
        let head = entities.get_by_id(id).unwrap().position + Vec3::Y * 2.62;
        let look = (head - (player + Vec3::Y * 1.62)).normalize();
        update_dimension_entities(
            Dimension::End,
            &mut entities,
            &chunks,
            &[(player, look)],
            0.25,
            GameMode::Survival,
        );
    }

    let enderman = entities.get_by_id(id).unwrap();
    assert_eq!(enderman.ai_phase, 1);
    assert!(enderman.target_player);
}

#[test]
fn creative_mode_enderman_wanders_without_attacking() {
    let mut entities = EntityManager::new();
    let id = entities.spawn(EntityType::Enderman, Vec3::new(0.0, 64.0, 10.0));
    let enderman = entities.get_by_id_mut(id).unwrap();
    enderman.ai_phase = 1;
    enderman.target_player = true;
    let chunks = WorldColumns::new(1);

    let events = update_dimension_entities(
        Dimension::End,
        &mut entities,
        &chunks,
        &[(Vec3::ZERO, Vec3::Z)],
        0.1,
        GameMode::Creative,
    );

    let enderman = entities.get_by_id(id).unwrap();
    assert_eq!(enderman.ai_phase, 0);
    assert!(!enderman.target_player);
    assert!(events.player_damage.is_empty());
    assert!(Vec3::new(enderman.velocity.x, 0.0, enderman.velocity.z).length() > 1.0);
}

#[test]
fn dead_enderman_drops_one_eye_of_ender() {
    let mut entities = EntityManager::new();
    let id = entities.spawn(EntityType::Enderman, Vec3::new(3.0, 64.0, 5.0));
    entities.get_by_id_mut(id).unwrap().health = 0.0;
    let mut events = BossEvents::default();

    collect_deaths(&mut entities, &mut events);

    assert!(entities.get_by_id(id).is_none());
    assert_eq!(events.drops.len(), 1);
    assert_eq!(events.drops[0].item, Item::EyeOfEnder);
    assert_eq!(events.drops[0].count, 1);
}

#[test]
fn wither_enters_low_health_charge_phase() {
    let mut entities = EntityManager::new();
    entities.spawn(EntityType::Wither, Vec3::new(0.0, 8.0, 0.0));
    entities.entities[0].health = 140.0;
    let chunks = WorldColumns::new(1);

    update_dimension_entities(
        Dimension::Overworld,
        &mut entities,
        &chunks,
        &[(Vec3::ZERO, Vec3::ZERO)],
        0.1,
        GameMode::Survival,
    );

    assert_eq!(entities.entities[0].ai_phase, 1);
}

#[test]
fn creative_mode_bosses_do_not_attack_player() {
    let mut entities = EntityManager::new();
    entities.spawn(EntityType::Blaze, Vec3::new(5.0, 0.0, 0.0));
    entities.spawn(EntityType::Piglin, Vec3::new(1.0, 0.0, 0.0));
    entities.spawn(EntityType::EnderDragon, Vec3::new(0.0, 80.0, 0.0));
    let chunks = WorldColumns::new(1);

    let events = update_dimension_entities(
        Dimension::Nether,
        &mut entities,
        &chunks,
        &[(Vec3::ZERO, Vec3::ZERO)],
        0.1,
        GameMode::Creative,
    );

    assert!(
        events.player_damage.is_empty(),
        "Bosses emitted player damage events in Creative mode!"
    );
    let dragon = entities
        .get_entities_by_type(EntityType::EnderDragon)
        .next()
        .unwrap();
    assert_eq!(dragon.ai_phase, 0, "Dragon left phase 0 in Creative mode!");
}

#[test]
fn nether_spawning_is_bounded_and_inside_loaded_chunks() {
    let mut chunks = WorldColumns::new(1);
    let mut chunk = Chunk::new(0, 0);
    for x in 0..CHUNK_WIDTH {
        for z in 0..CHUNK_DEPTH {
            for y in chunk.world_y_range() {
                chunk.set_block_local(x, y, z, BlockType::Air);
            }
            chunk.set_block_local(x, 40, z, BlockType::Netherrack);
        }
    }
    chunks.chunks.insert((0, 0), chunk);
    let mut entities = EntityManager::new();
    for tick in 0..128 {
        ensure_dimension_entities(
            Dimension::Nether,
            &mut entities,
            &chunks,
            Vec3::new(128.0, 41.0, 128.0),
            tick as f32,
        );
    }
    assert!(entities.entities.len() <= NETHER_MOB_CAP);
    assert!(!entities.entities.is_empty());
    assert!(entities.entities.iter().all(|entity| {
        entity.position.x >= 0.0
            && entity.position.x < CHUNK_WIDTH as f32
            && entity.position.z >= 0.0
            && entity.position.z < CHUNK_DEPTH as f32
    }));
}
