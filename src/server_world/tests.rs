// Tests extracted from server_world.rs (Plan 27).

use super::entities::block_revision_fingerprint;
use super::*;
use crate::authority::{AuthorityConfig, AuthorityCore};
use crate::entity::EntityType;
use crate::network::protocol::{GameplayOutcome, GameplayRequest};

#[test]
fn block_mutation_changes_real_chunk_and_revision() {
    let mut world = ServerWorld::new_with_difficulty(
        7,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        WorldRules::default(),
        2, Difficulty::default());
    let old = world.get_block(8, 80, 8);
    let mutation = world
        .set_block(8, 80, 8, BlockType::Chest, 0)
        .unwrap()
        .unwrap();
    assert_ne!(old, BlockType::Chest);
    assert_eq!(world.get_block(8, 80, 8), BlockType::Chest);
    assert_eq!(world.get_block_entity(8, 80, 8).is_some(), true);
    assert_eq!(mutation.revision, 1);
}

#[test]
fn authoritative_dispenser_matrix_preserves_payload_and_revisions() {
    let mut world = ServerWorld::new_with_difficulty(
        17,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        WorldRules::default(),
        2, Difficulty::default());
    let source = (8, 80, 8);
    let front = (8, 80, 9);
    world
        .set_block(source.0, source.1, source.2, BlockType::Dispenser, 0)
        .unwrap();
    let mut stack = crate::inventory::ItemStack::new(crate::inventory::Item::Stone, 2);
    stack.custom_name.set("automation");
    stack.can_break = 0x55;
    stack.can_place_on = 0xaa;
    if let Some(entity) = world
        .chunks
        .get_block_entity_mut(source.0, source.1, source.2)
    {
        entity.set_stack(0, Some(stack));
    }
    let before_revision = world
        .get_block_entity(source.0, source.1, source.2)
        .unwrap()
        .revision();
    assert!(world.execute_redstone_dispense(
        RedstoneAction::Dispense {
            pos: source,
            facing: crate::redstone::Direction::South,
            dropper: false,
        },
        1 << 63,
    ));
    let dropped = world
        .entities
        .entities
        .iter()
        .find(|entity| entity.entity_type == EntityType::DroppedItem)
        .expect("ordinary dispenser payload should drop");
    let payload = dropped.dropped_stack.expect("metadata-bearing drop");
    assert_eq!(payload.item, crate::inventory::Item::Stone);
    assert_eq!(payload.count, 1);
    assert_eq!(payload.custom_name.as_str(), "automation");
    assert_eq!(payload.can_break, 0x55);
    assert_eq!(payload.can_place_on, 0xaa);
    assert!(
        world
            .get_block_entity(source.0, source.1, source.2)
            .unwrap()
            .revision()
            > before_revision
    );
    assert_eq!(world.get_block_entity(front.0, front.1, front.2), None);
}

#[test]
fn authoritative_dropper_insert_is_merge_first_and_fallback_is_one_item() {
    let mut world = ServerWorld::new_with_difficulty(
        17,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        WorldRules::default(),
        2, Difficulty::default());
    let source = (8, 80, 8);
    let target = (8, 80, 9);
    world
        .set_block(source.0, source.1, source.2, BlockType::Dropper, 0)
        .unwrap();
    world
        .set_block(target.0, target.1, target.2, BlockType::Chest, 0)
        .unwrap();
    let payload = crate::inventory::ItemStack::new(crate::inventory::Item::Arrow, 3);
    if let Some(entity) = world
        .chunks
        .get_block_entity_mut(source.0, source.1, source.2)
    {
        entity.set_stack(0, Some(payload));
    }
    let chest_stack = crate::inventory::ItemStack::new(crate::inventory::Item::Arrow, 2);
    if let Some(entity) = world
        .chunks
        .get_block_entity_mut(target.0, target.1, target.2)
    {
        entity.set_stack(0, Some(chest_stack));
    }
    assert!(!world.execute_redstone_dispense(
        RedstoneAction::Dispense {
            pos: source,
            facing: crate::redstone::Direction::South,
            dropper: true,
        },
        1 << 63,
    ));
    assert_eq!(world.entities.entities.len(), 0);
    let chest = world
        .get_block_entity(target.0, target.1, target.2)
        .unwrap();
    assert_eq!(chest.get_stack(0).unwrap().count, 3);
    assert_eq!(
        world
            .get_block_entity(source.0, source.1, source.2)
            .unwrap()
            .get_stack(0)
            .unwrap()
            .count,
        2
    );

    // A non-mergeable target still consumes one and creates one fallback
    // entity carrying the complete stack metadata.
    if let Some(entity) = world
        .chunks
        .get_block_entity_mut(target.0, target.1, target.2)
    {
        entity.set_stack(
            0,
            Some(crate::inventory::ItemStack::new(
                crate::inventory::Item::Dirt,
                64,
            )),
        );
        for slot in 1..entity.slot_count() {
            entity.set_stack(
                slot,
                Some(crate::inventory::ItemStack::new(
                    crate::inventory::Item::Dirt,
                    64,
                )),
            );
        }
    }
    assert!(world.execute_redstone_dispense(
        RedstoneAction::Dispense {
            pos: source,
            facing: crate::redstone::Direction::South,
            dropper: true,
        },
        (1 << 63) + 1,
    ));
    assert!(world
        .entities
        .entities
        .iter()
        .any(|entity| entity.entity_type == EntityType::DroppedItem));
}

#[test]
fn authoritative_dispense_skips_unloaded_front_without_consumption() {
    let mut world = ServerWorld::new_with_difficulty(
        17,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        WorldRules::default(),
        2, Difficulty::default());
    let source = (15, 80, 8);
    world
        .set_block(source.0, source.1, source.2, BlockType::Dispenser, 0)
        .unwrap();
    if let Some(entity) = world
        .chunks
        .get_block_entity_mut(source.0, source.1, source.2)
    {
        entity.set_stack(
            0,
            Some(crate::inventory::ItemStack::new(
                crate::inventory::Item::Arrow,
                1,
            )),
        );
    }
    let action = RedstoneAction::Dispense {
        pos: source,
        facing: crate::redstone::Direction::East,
        dropper: false,
    };
    assert!(!world.execute_redstone_dispense(action, 1 << 63));
    assert_eq!(
        world
            .get_block_entity(source.0, source.1, source.2)
            .unwrap()
            .get_stack(0)
            .unwrap()
            .count,
        1
    );
    assert!(world.entities.entities.is_empty());
}

#[test]
fn dispenser_invalid_entity_id_is_atomic_and_bucket_consumes_one_with_rollback() {
    let mut world = ServerWorld::new_with_difficulty(
        17,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        WorldRules::default(),
        2, Difficulty::default());
    let source = (8, 80, 8);
    let front = (8, 80, 9);
    world
        .set_block(source.0, source.1, source.2, BlockType::Dispenser, 0)
        .unwrap();
    if let Some(entity) = world
        .chunks
        .get_block_entity_mut(source.0, source.1, source.2)
    {
        entity.set_stack(
            0,
            Some(crate::inventory::ItemStack::new(
                crate::inventory::Item::Arrow,
                1,
            )),
        );
    }
    assert!(!world.execute_redstone_dispense(
        RedstoneAction::Dispense {
            pos: source,
            facing: crate::redstone::Direction::South,
            dropper: false,
        },
        0,
    ));
    assert_eq!(
        world
            .get_block_entity(source.0, source.1, source.2)
            .unwrap()
            .get_stack(0)
            .unwrap()
            .count,
        1
    );
    assert!(world.entities.entities.is_empty());

    world
        .set_block(front.0, front.1, front.2, BlockType::Water, 0)
        .unwrap();
    let mut bucket = crate::inventory::ItemStack::new(crate::inventory::Item::Bucket, 2);
    bucket.custom_name.set("bucket-meta");
    bucket.can_break = 0x55;
    bucket.can_place_on = 0xaa;
    if let Some(entity) = world
        .chunks
        .get_block_entity_mut(source.0, source.1, source.2)
    {
        entity.set_stack(0, Some(bucket));
        for slot in 1..entity.slot_count() {
            entity.set_stack(slot, Some(bucket));
        }
    }
    assert!(!world.execute_redstone_dispense(
        RedstoneAction::Dispense {
            pos: source,
            facing: crate::redstone::Direction::South,
            dropper: false,
        },
        1 << 63,
    ));
    assert_eq!(world.get_block(front.0, front.1, front.2), BlockType::Water);
    let unchanged = world
        .get_block_entity(source.0, source.1, source.2)
        .unwrap()
        .get_stack(0)
        .unwrap();
    assert_eq!(unchanged.item, crate::inventory::Item::Bucket);
    assert_eq!(unchanged.count, 2);
    assert_eq!(unchanged.custom_name.as_str(), "bucket-meta");
    assert_eq!(unchanged.can_break, 0x55);
    assert_eq!(unchanged.can_place_on, 0xaa);

    // Free one slot: exactly one empty bucket remains and one filled
    // bucket is inserted with the original metadata.
    if let Some(entity) = world
        .chunks
        .get_block_entity_mut(source.0, source.1, source.2)
    {
        entity.set_stack(8, None);
    }
    assert!(!world.execute_redstone_dispense(
        RedstoneAction::Dispense {
            pos: source,
            facing: crate::redstone::Direction::South,
            dropper: false,
        },
        1 << 63,
    ));
    assert_eq!(world.get_block(front.0, front.1, front.2), BlockType::Air);
    let source_entity = world
        .get_block_entity(source.0, source.1, source.2)
        .unwrap();
    let remaining_empty_buckets: u32 = (0..source_entity.slot_count())
        .filter_map(|slot| source_entity.get_stack(slot))
        .filter(|stack| stack.item == crate::inventory::Item::Bucket)
        .map(|stack| stack.count)
        .sum();
    assert_eq!(remaining_empty_buckets, 15);
    let filled = (0..source_entity.slot_count())
        .find_map(|slot| {
            source_entity
                .get_stack(slot)
                .filter(|stack| stack.item == crate::inventory::Item::WaterBucket)
        })
        .expect("one filled bucket");
    assert_eq!(filled.count, 1);
    assert_eq!(filled.custom_name.as_str(), "bucket-meta");
    assert_eq!(filled.can_break, 0x55);
    assert_eq!(filled.can_place_on, 0xaa);
}

#[test]
fn bucket_rejects_flowing_or_falling_source_without_consumption() {
    let mut world = ServerWorld::new_with_difficulty(
        17,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        WorldRules::default(),
        2, Difficulty::default());
    let source = (8, 80, 8);
    let front = (8, 80, 9);
    world
        .set_block(source.0, source.1, source.2, BlockType::Dispenser, 0)
        .unwrap();
    world
        .set_block(front.0, front.1, front.2, BlockType::Water, 0)
        .unwrap();
    if let Some(entity) = world
        .chunks
        .get_block_entity_mut(source.0, source.1, source.2)
    {
        entity.set_stack(
            0,
            Some(crate::inventory::ItemStack::new(
                crate::inventory::Item::Bucket,
                1,
            )),
        );
    }
    world.chunks.set_fluid_level(front.0, front.1, front.2, 1);
    assert!(!world.execute_redstone_dispense(
        RedstoneAction::Dispense {
            pos: source,
            facing: crate::redstone::Direction::South,
            dropper: false
        },
        1 << 63,
    ));
    assert_eq!(
        world
            .get_block_entity(source.0, source.1, source.2)
            .unwrap()
            .get_stack(0)
            .unwrap()
            .item,
        crate::inventory::Item::Bucket
    );
    world.chunks.set_fluid_level(front.0, front.1, front.2, 0);
    world
        .chunks
        .set_fluid_falling(front.0, front.1, front.2, true);
    assert!(!world.execute_redstone_dispense(
        RedstoneAction::Dispense {
            pos: source,
            facing: crate::redstone::Direction::South,
            dropper: false
        },
        1 << 63,
    ));
    assert_eq!(
        world
            .get_block_entity(source.0, source.1, source.2)
            .unwrap()
            .get_stack(0)
            .unwrap()
            .item,
        crate::inventory::Item::Bucket
    );
}

#[test]
fn chest_first_and_last_viewer_toggle_authoritative_state() {
    let mut world = ServerWorld::new_with_difficulty(
        7,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        WorldRules::default(),
        2, Difficulty::default());
    let position = (8, 80, 8);
    world
        .set_block(position.0, position.1, position.2, BlockType::Chest, 0)
        .unwrap();

    world
        .open_container(position.0, position.1, position.2, 0, 7)
        .unwrap();
    assert!(crate::world::BlockState::decode(world.get_block_state(8, 80, 8)).is_open);
    assert_eq!(
        world
            .container_viewers_at(position)
            .copied()
            .collect::<Vec<_>>(),
        vec![7]
    );

    world
        .open_container(position.0, position.1, position.2, 0, 8)
        .unwrap();
    world
        .close_container(position.0, position.1, position.2, 0, 7)
        .unwrap();
    assert!(crate::world::BlockState::decode(world.get_block_state(8, 80, 8)).is_open);
    world
        .close_container(position.0, position.1, position.2, 0, 8)
        .unwrap();
    assert!(!crate::world::BlockState::decode(world.get_block_state(8, 80, 8)).is_open);
}

#[test]
fn forced_last_viewer_closes_chest_once_and_other_viewer_keeps_it_open() {
    let mut world = ServerWorld::new_with_difficulty(
        7,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        WorldRules::default(),
        2, Difficulty::default());
    let position = (8, 80, 8);
    world
        .set_block(position.0, position.1, position.2, BlockType::Chest, 0)
        .unwrap();

    world
        .open_container(position.0, position.1, position.2, 0, 7)
        .unwrap();
    world
        .open_container(position.0, position.1, position.2, 0, 8)
        .unwrap();
    assert!(world.close_container_viewer_forced(7, position));
    assert!(crate::world::BlockState::decode(world.get_block_state(8, 80, 8)).is_open);
    assert!(world.take_pending_mutations().is_empty());

    assert!(world.close_container_viewer_forced(8, position));
    assert!(!crate::world::BlockState::decode(world.get_block_state(8, 80, 8)).is_open);
    let mutations = world.take_pending_mutations();
    assert_eq!(mutations.len(), 1);
    assert_eq!(mutations[0].position, position);
    assert!(!world.close_container_viewer_forced(8, position));
    assert!(world.take_pending_mutations().is_empty());
}

#[test]
fn chest_block_break_emits_dimension_scoped_closures() {
    let mut world = ServerWorld::new_with_difficulty(
        7,
        Dimension::Nether,
        WorldType::Superflat,
        false,
        WorldRules::default(),
        2, Difficulty::default());
    let position = (8, 80, 8);
    world
        .set_block(position.0, position.1, position.2, BlockType::Chest, 0)
        .unwrap();
    world
        .container_viewers
        .entry(position)
        .or_default()
        .insert(7);

    world
        .set_block(position.0, position.1, position.2, BlockType::Air, 0)
        .unwrap();
    assert_eq!(
        world.take_container_closures(),
        vec![ContainerClosure {
            player_id: 7,
            dimension: Dimension::Nether,
            position,
        }]
    );
    assert!(world.container_viewers_at(position).next().is_none());
}

#[test]
fn double_chest_open_publishes_partner_state_mutation() {
    let mut world = ServerWorld::new_with_difficulty(
        7,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        WorldRules::default(),
        2, Difficulty::default());
    let left = (8, 80, 8);
    let right = (9, 80, 8);
    let left_state = crate::world::BlockState {
        // The wire chest type is relative to the facing direction: a
        // North-facing Right half pairs with the block on its +X side.
        chest_type: crate::world::ChestType::Right,
        ..Default::default()
    }
    .encode();
    let right_state = crate::world::BlockState {
        chest_type: crate::world::ChestType::Left,
        ..Default::default()
    }
    .encode();
    world
        .set_block(left.0, left.1, left.2, BlockType::Chest, left_state)
        .unwrap();
    world
        .set_block(right.0, right.1, right.2, BlockType::Chest, right_state)
        .unwrap();

    let primary = world
        .open_container(left.0, left.1, left.2, 0, 7)
        .unwrap()
        .unwrap();
    let partner = world.take_pending_mutations();
    assert_eq!(primary.position, left);
    assert_eq!(partner.len(), 1);
    assert_eq!(partner[0].position, right);
    assert!(
        crate::world::BlockState::decode(world.get_block_state(left.0, left.1, left.2)).is_open
    );
    assert!(
        crate::world::BlockState::decode(world.get_block_state(right.0, right.1, right.2))
            .is_open
    );
}

#[test]
fn fixed_tick_checksum_is_deterministic() {
    let make = || {
        let mut world = ServerWorld::new_with_difficulty(
            7,
            Dimension::Overworld,
            WorldType::Superflat,
            false,
            WorldRules::default(),
            2, Difficulty::default());
        world
            .entities
            .spawn(EntityType::Zombie, Vec3::new(10.0, 80.0, 10.0));
        world.tick_players(&[(7, [8.0, 80.0, 8.0], 0.0, 0.0)])
    };
    assert_eq!(make(), make());
}

#[test]
fn checksum_distinguishes_raw_fluid_mutations() {
    let make = || {
        ServerWorld::new_with_difficulty(
            7,
            Dimension::Overworld,
            WorldType::Superflat,
            false,
            WorldRules::default(),
            2, Difficulty::default())
    };
    let mut plain = make();
    let mut waterlogged = make();
    let position = (8, 80, 8);
    let plain_mutation = plain
        .set_block(position.0, position.1, position.2, BlockType::OakSlab, 0)
        .unwrap()
        .unwrap();
    waterlogged
        .set_block(position.0, position.1, position.2, BlockType::OakSlab, 0)
        .unwrap();
    assert!(waterlogged
        .chunks
        .set_waterlogged(position.0, position.1, position.2, true));
    let mut waterlogged_mutation = plain_mutation;
    waterlogged_mutation.raw_fluid = waterlogged
        .chunks
        .get_fluid_raw(position.0, position.1, position.2);
    assert_eq!(plain_mutation.block, waterlogged_mutation.block);
    assert_ne!(plain_mutation.raw_fluid, waterlogged_mutation.raw_fluid);
    assert_ne!(
        plain.checksum(&[plain_mutation]),
        waterlogged.checksum(&[waterlogged_mutation])
    );
}

fn folded_block_revision_checksum(world: &ServerWorld) -> u64 {
    world
        .block_revisions
        .values()
        .flat_map(|column| column.iter())
        .fold(0u64, |acc, (&position, &revision)| {
            acc ^ block_revision_fingerprint(position, revision)
        })
}

fn superflat_world(seed: u32) -> ServerWorld {
    ServerWorld::new_with_difficulty(
        seed,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        WorldRules::default(),
        2, Difficulty::default())
}

#[test]
fn running_revision_checksum_matches_sorted_map_fold() {
    let mut world = superflat_world(7);
    assert_eq!(world.block_revision_checksum, 0);
    world
        .set_block(8, 80, 8, BlockType::Stone, 0)
        .unwrap()
        .unwrap();
    world
        .set_block(4, 80, 4, BlockType::Dirt, 0)
        .unwrap()
        .unwrap();
    world
        .set_block(8, 80, 8, BlockType::OakPlanks, 0)
        .unwrap()
        .unwrap();
    assert_eq!(
        world.block_revision_checksum,
        folded_block_revision_checksum(&world)
    );
    world.remove_resident_chunk(0, 0);
    assert!(world.block_revisions.is_empty());
    assert_eq!(world.block_revision_checksum, 0);
    assert_eq!(
        world.block_revision_checksum,
        folded_block_revision_checksum(&world)
    );
}

#[test]
fn checksum_is_independent_of_revision_insert_order() {
    let mut first = superflat_world(7);
    let mut second = superflat_world(7);
    first.set_block_revision((1, 80, 1), 3);
    first.set_block_revision((8, 80, 8), 9);
    second.set_block_revision((8, 80, 8), 9);
    second.set_block_revision((1, 80, 1), 3);
    assert_eq!(
        first.block_revision_checksum,
        second.block_revision_checksum
    );
    assert_eq!(first.checksum(&[]), second.checksum(&[]));
}

#[test]
fn empty_ticks_keep_matching_checksums_for_identical_worlds() {
    let tick = || {
        let mut world = superflat_world(11);
        world
            .set_block(8, 80, 8, BlockType::Stone, 0)
            .unwrap()
            .unwrap();
        world
            .entities
            .spawn(EntityType::Zombie, Vec3::new(10.0, 80.0, 10.0));
        world.tick_players(&[(7, [8.0, 80.0, 8.0], 0.0, 0.0)]);
        world.tick_players(&[(7, [8.0, 80.0, 8.0], 0.0, 0.0)])
    };
    assert_eq!(tick(), tick());
}

#[test]
fn checksum_distinguishes_entity_type_at_same_pose() {
    let mut arrow = superflat_world(7);
    let mut dropped = superflat_world(7);
    let position = Vec3::new(10.0, 80.0, 10.0);
    arrow.entities.spawn(EntityType::Arrow, position);
    dropped.entities.spawn(EntityType::DroppedItem, position);
    assert_ne!(arrow.checksum(&[]), dropped.checksum(&[]));
}

#[test]
fn idle_entity_fingerprint_skips_sort_and_hash_rebuild() {
    let mut world = superflat_world(7);
    world
        .entities
        .spawn(EntityType::EndCrystal, Vec3::new(4.0, 80.0, 4.0));
    // No tick: pose / ai_phase stay put, so the second checksum must reuse
    // the sorted entity fingerprint instead of rebuilding it.
    let _ = world.checksum(&[]);
    let builds_after_first = world.entities.entity_fingerprint_builds();
    assert_eq!(builds_after_first, 1);
    let first = world.checksum(&[]);
    assert_eq!(world.entities.entity_fingerprint_builds(), builds_after_first);
    let second = world.checksum(&[]);
    assert_eq!(first, second);
    assert_eq!(world.entities.entity_fingerprint_builds(), builds_after_first);

    // Pose / ai_phase change must invalidate the cache.
    world.entities.mark_checksum_inputs_changed();
    let third = world.checksum(&[]);
    assert_eq!(world.entities.entity_fingerprint_builds(), builds_after_first + 1);
    assert_eq!(third, first);

    // Membership change must invalidate and change the value.
    world
        .entities
        .spawn(EntityType::Arrow, Vec3::new(5.0, 80.0, 5.0));
    let fourth = world.checksum(&[]);
    assert_eq!(world.entities.entity_fingerprint_builds(), builds_after_first + 2);
    assert_ne!(fourth, first);
}

#[test]
fn empty_world_checksum_reuses_entity_fingerprint_across_idle_ticks() {
    let mut world = superflat_world(11);
    world.rules.do_mob_spawning = false;
    world.rules.do_daylight_cycle = false;
    let _ = world.checksum(&[]);
    let builds = world.entities.entity_fingerprint_builds();
    world.tick_players(&[]);
    world.tick_players(&[]);
    let after = world.checksum(&[]);
    assert_eq!(world.entities.entity_fingerprint_builds(), builds);
    let mut twin = superflat_world(11);
    twin.rules.do_mob_spawning = false;
    twin.rules.do_daylight_cycle = false;
    twin.tick_players(&[]);
    twin.tick_players(&[]);
    assert_eq!(after, twin.checksum(&[]));
}

#[test]
fn stationary_living_entity_skips_physics_and_reuses_checksum_fingerprint() {
    let mut world = superflat_world(13);
    world.rules.do_mob_spawning = false;
    world.rules.do_daylight_cycle = false;
    let id = world
        .entities
        .spawn(EntityType::Pig, Vec3::new(8.0, 80.0, 8.0));
    {
        let entity = world.entities.get_by_id_mut(id).unwrap();
        entity.on_ground = true;
        entity.velocity = Vec3::ZERO;
        entity.ai_phase = 7;
    }
    let phase = world.entities.get_by_id(id).unwrap().ai_phase;
    let pos = world.entities.get_by_id(id).unwrap().position;
    let _ = world.checksum(&[]);
    let builds = world.entities.entity_fingerprint_builds();
    world.tick_players(&[]);
    world.tick_players(&[]);
    let entity = world.entities.get_by_id(id).unwrap();
    assert_eq!(entity.ai_phase, phase);
    assert_eq!(entity.position, pos);
    assert_eq!(world.entities.entity_fingerprint_builds(), builds);
}

#[test]
fn sitting_living_entity_skips_physics_while_airborne_velocity_is_zero() {
    let mut world = superflat_world(13);
    world.rules.do_mob_spawning = false;
    world.rules.do_daylight_cycle = false;
    let id = world
        .entities
        .spawn(EntityType::Wolf, Vec3::new(8.0, 90.0, 8.0));
    {
        let entity = world.entities.get_by_id_mut(id).unwrap();
        entity.is_sitting = true;
        entity.on_ground = false;
        entity.velocity = Vec3::ZERO;
        entity.ai_phase = 3;
    }
    let phase = world.entities.get_by_id(id).unwrap().ai_phase;
    let pos = world.entities.get_by_id(id).unwrap().position;
    world.tick_players(&[]);
    let entity = world.entities.get_by_id(id).unwrap();
    assert_eq!(entity.ai_phase, phase);
    assert_eq!(entity.position, pos);
}

#[test]
fn hostile_out_of_chase_range_does_not_rewrite_velocity_each_tick() {
    let mut world = ServerWorld::new_with_difficulty(
        7,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        {
            let mut rules = WorldRules::default();
            rules.do_mob_spawning = false;
            rules.do_daylight_cycle = false;
            rules
        },
        2,
        Difficulty::Normal,
    );
    let id = world
        .entities
        .spawn(EntityType::Zombie, Vec3::new(8.0, 80.0, 8.0));
    {
        let entity = world.entities.get_by_id_mut(id).unwrap();
        entity.on_ground = true;
        entity.velocity = Vec3::ZERO;
        entity.target_player = false;
        entity.ai_phase = 4;
    }
    // Player is beyond HOSTILE_CHASE_RANGE (40); chase must not arm.
    world.tick_players(&[(7, [8.0, 80.0, 80.0], 0.0, 0.0)]);
    world.tick_players(&[(7, [8.0, 80.0, 80.0], 0.0, 0.0)]);
    let entity = world.entities.get_by_id(id).unwrap();
    assert_eq!(entity.velocity, Vec3::ZERO);
    assert!(!entity.target_player);
    assert_eq!(entity.ai_phase, 4);
}

#[test]
fn tick_entities_keeps_moved_entity_findable_via_query_radius() {
    let mut world = superflat_world(19);
    world.rules.do_mob_spawning = false;
    world.rules.do_daylight_cycle = false;
    for cx in -1..=1 {
        for cz in -1..=1 {
            world.ensure_chunk(cx, cz);
        }
    }
    let id = world
        .entities
        .spawn(EntityType::Zombie, Vec3::new(8.0, 80.0, 8.0));
    {
        let entity = world.entities.get_by_id_mut(id).unwrap();
        entity.on_ground = true;
        entity.velocity = Vec3::ZERO;
    }
    // In-range player east of the zombie so chase writes +X velocity and
    // the mover crosses into chunk (1, 0) under incremental sync.
    world.tick_players(&[(7, [24.0, 80.0, 8.0], 0.0, 0.0)]);
    let pos = world.entities.get_by_id(id).unwrap().position;
    assert!(
        pos.x > 8.0,
        "chasing hostile must move toward the player (got x={})",
        pos.x
    );
    assert!(
        world
            .entities
            .query_radius(pos, 1.0)
            .any(|entity| entity.id == id),
        "moved ids must stay in spatial buckets after incremental sync"
    );
}

#[test]
fn difficulty_controls_hostile_policy_without_binding_pvp_or_spawn_rule() {
    let mut rules = WorldRules::default();
    rules.pvp = true;
    rules.do_mob_spawning = false;
    let mut peaceful = ServerWorld::new_with_difficulty(
        7,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        rules,
        2,
        Difficulty::Peaceful,
    );
    assert!(!peaceful.allows_hostile_spawning());
    assert!(peaceful.rules.pvp);
    let peaceful_id = peaceful
        .entities
        .spawn(EntityType::Zombie, Vec3::new(10.0, 80.0, 10.0));
    peaceful.tick_players(&[(7, [8.0, 80.0, 8.0], 0.0, 0.0)]);
    assert!(peaceful.entities.get_by_id(peaceful_id).is_none());

    let mut easy_rules = rules;
    easy_rules.do_mob_spawning = false;
    let mut easy = ServerWorld::new_with_difficulty(
        7,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        easy_rules,
        2,
        Difficulty::Easy,
    );
    assert!(!easy.allows_hostile_spawning());
    let easy_id = easy
        .entities
        .spawn(EntityType::Zombie, Vec3::new(10.0, 80.0, 10.0));
    easy.tick_players(&[(7, [8.0, 80.0, 8.0], 0.0, 0.0)]);
    assert!(easy.entities.get_by_id(easy_id).is_some());
    assert!(easy
        .entities
        .get_by_id(easy_id)
        .is_some_and(|entity| entity.velocity.x < 0.0));

    let mut normal = ServerWorld::new_with_difficulty(
        7,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        WorldRules::default(),
        2,
        Difficulty::Normal,
    );
    let normal_id = normal
        .entities
        .spawn(EntityType::Zombie, Vec3::new(10.0, 80.0, 10.0));
    normal.tick_players(&[(7, [8.0, 80.0, 8.0], 0.0, 0.0)]);

    let mut hard = ServerWorld::new_with_difficulty(
        7,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        WorldRules::default(),
        2,
        Difficulty::Hard,
    );
    let hard_id = hard
        .entities
        .spawn(EntityType::Zombie, Vec3::new(10.0, 80.0, 10.0));
    hard.tick_players(&[(7, [8.0, 80.0, 8.0], 0.0, 0.0)]);
    let easy_speed = easy.entities.get_by_id(easy_id).unwrap().velocity.x.abs();
    let normal_speed = normal
        .entities
        .get_by_id(normal_id)
        .unwrap()
        .velocity
        .x
        .abs();
    let hard_speed = hard.entities.get_by_id(hard_id).unwrap().velocity.x.abs();
    assert!(easy_speed < normal_speed && normal_speed < hard_speed);
}

#[test]
fn malformed_combat_action_is_explicitly_rejected() {
    let mut core = AuthorityCore::new(AuthorityConfig::default());
    core.register_session(crate::authority::contract::SessionContract::new(
        7,
        "alex",
        0,
        [8.0, 80.0, 8.0],
        true,
        true,
    ))
    .unwrap();
    let request = GameplayRequest {
        request_id: 1,
        client_sequence: 1,
        session_id: 7,
        dimension: 0,
        client_revision: 0,
        operation: GameplayOperation::Combat {
            target: 42,
            action: 1,
        },
    };
    assert!(matches!(
        core.submit_request(request).outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidState
        }
    ));
}

#[test]
fn mount_requires_range_and_updates_authoritative_passengers() {
    let mut world = ServerWorld::new_with_difficulty(
        7,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        WorldRules::default(),
        2, Difficulty::default());
    world.entities.entities.push(crate::entity::Entity::new(
        11,
        EntityType::Boat,
        glam::Vec3::new(9.0, 80.0, 8.0),
    ));
    world.entities.rebuild_indexes();
    assert_eq!(world.apply_mount(7, 11, [8.0, 80.0, 8.0]), Ok(Some(11)));
    assert!(world
        .entities
        .get_by_id(11)
        .unwrap()
        .passengers
        .contains(&7));
    world.entities.entities.push(crate::entity::Entity::new(
        12,
        EntityType::Boat,
        glam::Vec3::new(100.0, 80.0, 8.0),
    ));
    world.entities.rebuild_indexes();
    assert_eq!(
        world.apply_mount(7, 12, [8.0, 80.0, 8.0]),
        Err(RejectReason::TooFar)
    );
    assert!(world
        .entities
        .get_by_id(11)
        .unwrap()
        .passengers
        .contains(&7));
}

#[test]
fn trade_second_cost_failure_rolls_back_first_cost() {
    let mut world = ServerWorld::new_with_difficulty(
        7,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        WorldRules::default(),
        2, Difficulty::default());
    {
        let mut entity = crate::entity::Entity::new(
            21,
            EntityType::Villager,
            glam::Vec3::new(9.0, 80.0, 8.0),
        );
        entity.profession = crate::village::poi::VillagerProfession::Farmer;
        entity.villager_level = crate::village::trade::VillagerLevel::Novice;
        entity.offers = vec![crate::village::trade::TradeOffer::new(
            crate::inventory::ItemStack::new(crate::inventory::Item::Wheat, 2),
            Some(crate::inventory::ItemStack::new(
                crate::inventory::Item::Carrot,
                1
            )),
            crate::inventory::ItemStack::new(crate::inventory::Item::Emerald, 1),
            4,
            1,
        )];
        world.entities.entities.push(entity);
        world.entities.rebuild_indexes();
    }
    let mut gameplay = SessionGameplayState::default();
    let mut wheat = crate::network::protocol::ItemWire::empty();
    wheat.item = crate::inventory::Item::Wheat as u32;
    wheat.count = 2;
    gameplay.inventory[0] = Some(SessionInventorySlot::from_wire(wheat, 0, 0));
    let before = gameplay;
    assert_eq!(
        world.apply_trade(&mut gameplay, 21, 0, [8.0, 80.0, 8.0]),
        Err(RejectReason::InvalidState)
    );
    assert_eq!(gameplay, before);
    assert_eq!(world.entities.get_by_id(21).unwrap().offers[0].uses, 0);
}

#[test]
fn tick_automation_walks_simulation_columns_not_residency() {
    let mut world = ServerWorld::new_with_difficulty(
        7,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        WorldRules::default(),
        2, Difficulty::default());
    world.set_block(8, 80, 8, BlockType::Hopper, 0).unwrap();
    world.set_block(128, 80, 8, BlockType::Hopper, 0).unwrap();
    if let Some(BlockEntity::Hopper(hopper)) = world.chunks.get_block_entity_mut(8, 80, 8) {
        hopper.transfer_cooldown = 6;
    }
    if let Some(BlockEntity::Hopper(hopper)) = world.chunks.get_block_entity_mut(128, 80, 8) {
        hopper.transfer_cooldown = 6;
    }
    world.tick_players(&[(7, [8.0, 80.0, 8.0], 0.0, 0.0)]);
    let near = match world.get_block_entity(8, 80, 8) {
        Some(BlockEntity::Hopper(hopper)) => hopper.transfer_cooldown,
        _ => panic!("near hopper"),
    };
    let far = match world.get_block_entity(128, 80, 8) {
        Some(BlockEntity::Hopper(hopper)) => hopper.transfer_cooldown,
        _ => panic!("far hopper"),
    };
    assert_eq!(near, 5);
    assert_eq!(far, 6);
    assert!(world.chunks.chunks.contains_key(&(8, 0)));
}

#[test]
fn evict_flushes_dirty_then_removes_unkept_columns() {
    let mut world = ServerWorld::new_with_difficulty(
        7,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        WorldRules::default(),
        2, Difficulty::default());
    world
        .set_block(128, 80, 8, BlockType::DiamondOre, 0)
        .unwrap();
    assert!(world.chunks.chunks.contains_key(&(8, 0)));
    let mut flushed = Vec::new();
    let keep = BTreeSet::from([(0, 0)]);
    world.evict_unkept_chunks(&keep, |cx, cz, data| {
        flushed.push((cx, cz, data.mutation_revision));
        Ok(())
    });
    assert!(!world.chunks.chunks.contains_key(&(8, 0)));
    assert!(world.chunks.chunks.contains_key(&(0, 0)));
    assert_eq!(flushed.len(), 1);
    assert_eq!(flushed[0].0, 8);
    assert_eq!(flushed[0].1, 0);
}

#[test]
fn do_fire_tick_false_filters_fire_random_ticks() {
    let mut rules = WorldRules::default();
    rules.do_fire_tick = false;
    let mut world = ServerWorld::new_with_difficulty(
        11,
        Dimension::Overworld,
        WorldType::Superflat,
        false,
        rules,
        2, Difficulty::default());
    world.set_block(4, 65, 4, BlockType::Fire, 0).unwrap();
    let players = [(1u64, [4.0_f32, 65.0, 4.0], 0.0_f32, 0.0_f32)];
    for _ in 0..400 {
        let _ = world.tick_players(&players);
    }
    assert_eq!(world.get_block(4, 65, 4), BlockType::Fire);
}
