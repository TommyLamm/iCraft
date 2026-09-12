// Tests extracted from redstone.rs (Plan 27).

use super::*;
use crate::world::Chunk;

const Y: i32 = 200;

fn manager() -> WorldColumns {
    let mut manager = WorldColumns::new(2);
    manager.insert_resident_chunk((0, 0), Chunk::new(0, 0));
    manager
}

fn place(
    system: &mut RedstoneSystem,
    manager: &mut WorldColumns,
    x: i32,
    block: BlockType,
    facing: Direction,
) {
    manager.set_block(x, Y, 0, block);
    system.on_block_changed(manager, (x, Y, 0), facing);
}

fn canonical_fixture() -> RedstoneSystem {
    let mut system = RedstoneSystem::new();
    system.tick = 37;
    system.sleeping = true;
    system.known_chunks.extend([(1, -2), (0, 0)]);

    let mut component = ComponentState::new(BlockType::Repeater, Direction::East);
    component.signal = RedstoneState {
        power: 11,
        charge: ChargeKind::Weak,
    };
    component.repeater_delay = 4;
    component.comparator_mode = ComparatorMode::Subtract;
    component.note = 19;
    component.last_powered = true;
    system.components.insert((3, Y, -4), component);
    system.index_component((3, Y, -4), BlockType::Repeater);
    system.known_load_generation = 0;

    system.scheduled.push(ScheduledTick {
        due: 41,
        pos: (3, Y, -4),
        kind: ScheduledKind::Repeater(true),
    });
    system.scheduled.push(ScheduledTick {
        due: 99,
        pos: (-2, Y, 5),
        kind: ScheduledKind::Explode,
    });
    system.dirty.extend([(3, Y, -4), (-2, Y, 5)]);
    system.previous_plate_occupants.insert((3, Y - 1, -4));
    system
}

#[test]
fn canonical_snapshot_is_order_independent_for_unordered_state() {
    let mut first = RedstoneSystem::new();
    let mut second = RedstoneSystem::new();
    let entries = [
        (
            (3, Y, -4),
            ComponentState::new(BlockType::Repeater, Direction::East),
        ),
        (
            (-2, Y, 5),
            ComponentState::new(BlockType::RedstoneWire, Direction::West),
        ),
    ];
    for &(pos, state) in &entries {
        first.components.insert(pos, state);
    }
    for &(pos, state) in entries.iter().rev() {
        second.components.insert(pos, state);
    }
    for chunk in [(1, -2), (0, 0)] {
        first.known_chunks.insert(chunk);
    }
    for chunk in [(0, 0), (1, -2)] {
        second.known_chunks.insert(chunk);
    }
    for pos in [(3, Y, -4), (-2, Y, 5)] {
        first.dirty.insert(pos);
    }
    for pos in [(-2, Y, 5), (3, Y, -4)] {
        second.dirty.insert(pos);
    }
    for pos in [(3, Y - 1, -4), (-2, Y - 1, 5)] {
        first.previous_plate_occupants.insert(pos);
    }
    for pos in [(-2, Y - 1, 5), (3, Y - 1, -4)] {
        second.previous_plate_occupants.insert(pos);
    }
    assert_eq!(first.canonical_snapshot(), second.canonical_snapshot());
    assert_eq!(first.canonical_checksum(), second.canonical_checksum());
}

#[test]
fn canonical_checksum_changes_for_each_redstone_state_domain() {
    let baseline = canonical_fixture();
    let expected = baseline.canonical_checksum();

    let mut changed = canonical_fixture();
    changed.tick += 1;
    assert_ne!(changed.canonical_checksum(), expected);

    let mut changed = canonical_fixture();
    changed.sleeping = false;
    assert_ne!(changed.canonical_checksum(), expected);

    let mut changed = canonical_fixture();
    changed.known_chunks.insert((9, 9));
    assert_ne!(changed.canonical_checksum(), expected);

    let mut changed = canonical_fixture();
    changed.dirty.insert((9, Y, 9));
    assert_ne!(changed.canonical_checksum(), expected);

    let mut changed = canonical_fixture();
    changed.previous_plate_occupants.insert((9, Y - 1, 9));
    assert_ne!(changed.canonical_checksum(), expected);

    fn assert_component_change(expected: u64, change: fn(&mut ComponentState)) {
        let mut changed = canonical_fixture();
        change(changed.components.get_mut(&(3, Y, -4)).unwrap());
        assert_ne!(changed.canonical_checksum(), expected);
    }
    assert_component_change(expected, |state| state.signal.power += 1);
    assert_component_change(expected, |state| state.signal.charge = ChargeKind::Strong);
    assert_component_change(expected, |state| state.facing = Direction::South);
    assert_component_change(expected, |state| state.repeater_delay = 2);
    assert_component_change(expected, |state| {
        state.comparator_mode = ComparatorMode::Compare
    });
    assert_component_change(expected, |state| state.note = 3);
    assert_component_change(expected, |state| state.last_powered = false);

    let mut changed = canonical_fixture();
    changed.scheduled[0].due += 1;
    assert_ne!(changed.canonical_checksum(), expected);

    let mut changed = canonical_fixture();
    changed.scheduled[0].kind = ScheduledKind::Repeater(false);
    assert_ne!(changed.canonical_checksum(), expected);
}

#[test]
fn canonical_checksum_preserves_scheduled_queue_order() {
    let first = canonical_fixture();
    let mut second = canonical_fixture();
    second.scheduled.swap(0, 1);
    assert_ne!(first.canonical_checksum(), second.canonical_checksum());
}

#[test]
fn dust_propagates_and_loses_one_level_per_block() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    place(
        &mut system,
        &mut manager,
        0,
        BlockType::Lever,
        Direction::East,
    );
    place(
        &mut system,
        &mut manager,
        1,
        BlockType::RedstoneWire,
        Direction::East,
    );
    place(
        &mut system,
        &mut manager,
        2,
        BlockType::RedstoneWire,
        Direction::East,
    );
    place(
        &mut system,
        &mut manager,
        3,
        BlockType::RedstoneLamp,
        Direction::East,
    );

    system.interact(&mut manager, (0, Y, 0));
    system.tick(&mut manager, &[]);

    assert_eq!(system.power_at((1, Y, 0)), 15);
    assert_eq!(system.power_at((2, Y, 0)), 14);
    assert_eq!(manager.get_block(3, Y, 0), BlockType::RedstoneLamp);
    assert_eq!(crate::world::BlockState::decode(manager.get_block_state(3, Y, 0)).is_open, true);
}

#[test]
fn repeater_applies_configured_tick_delay_and_restores_full_power() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    place(
        &mut system,
        &mut manager,
        0,
        BlockType::Lever,
        Direction::East,
    );
    place(
        &mut system,
        &mut manager,
        1,
        BlockType::Repeater,
        Direction::East,
    );
    place(
        &mut system,
        &mut manager,
        2,
        BlockType::RedstoneLamp,
        Direction::East,
    );
    system.set_repeater_delay((1, Y, 0), 4);
    system.interact(&mut manager, (0, Y, 0));

    for _ in 0..4 {
        system.tick(&mut manager, &[]);
        assert_eq!(manager.get_block(2, Y, 0), BlockType::RedstoneLamp);
    }
    system.tick(&mut manager, &[]);
    assert_eq!(system.power_at((1, Y, 0)), 15);
    assert_eq!(manager.get_block(2, Y, 0), BlockType::RedstoneLamp);
    assert_eq!(crate::world::BlockState::decode(manager.get_block_state(2, Y, 0)).is_open, true);
}

#[test]
fn piston_pushes_one_movable_block() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    place(
        &mut system,
        &mut manager,
        0,
        BlockType::Lever,
        Direction::East,
    );
    place(
        &mut system,
        &mut manager,
        1,
        BlockType::RedstoneWire,
        Direction::East,
    );
    place(
        &mut system,
        &mut manager,
        2,
        BlockType::Piston,
        Direction::East,
    );
    manager.set_block(3, Y, 0, BlockType::Stone);
    manager.set_block(4, Y, 0, BlockType::Air);

    system.interact(&mut manager, (0, Y, 0));
    system.tick(&mut manager, &[]);

    assert_eq!(manager.get_block(2, Y, 0), BlockType::Piston);
    assert_eq!(crate::world::BlockState::decode(manager.get_block_state(2, Y, 0)).is_open, true);
    assert_eq!(manager.get_block(3, Y, 0), BlockType::Air);
    assert_eq!(manager.get_block(4, Y, 0), BlockType::Stone);
}

#[test]
fn door_and_trapdoor_redstone_toggle_preserves_facing_and_updates_open_bit() {
    use crate::world::{BlockState, ChestType};

    let mut system = RedstoneSystem::new();
    let mut manager = WorldColumns::new(2);
    manager.insert_resident_chunk((0, 0), Chunk::new(0, 0));

    let initial_state = BlockState {
        facing: Direction::West,
        is_top: false,
        is_right_hinge: true,
        is_open: false,
        chest_type: ChestType::Single,
    };

    place(
        &mut system,
        &mut manager,
        0,
        BlockType::Lever,
        Direction::South,
    );
    place(
        &mut system,
        &mut manager,
        1,
        BlockType::RedstoneWire,
        Direction::East,
    );

    manager.set_block(2, Y, 0, BlockType::OakDoor);
    manager.set_block_state(2, Y, 0, initial_state.encode());
    system.on_block_changed(&manager, (2, Y, 0), Direction::West);

    system.interact(&mut manager, (0, Y, 0));
    system.tick(&mut manager, &[]);

    assert_eq!(manager.get_block(2, Y, 0), BlockType::OakDoor);
    assert_eq!(crate::world::BlockState::decode(manager.get_block_state(2, Y, 0)).is_open, true);
    let toggled_raw = manager.get_block_state(2, Y, 0);
    let toggled_state = BlockState::decode(toggled_raw);
    assert_eq!(toggled_state.facing, Direction::West);
    assert!(toggled_state.is_right_hinge);
    assert!(toggled_state.is_open);
}

#[test]
fn pressure_plate_opens_and_closes_an_adjacent_door() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    place(
        &mut system,
        &mut manager,
        0,
        BlockType::PressurePlate,
        Direction::East,
    );
    place(
        &mut system,
        &mut manager,
        1,
        BlockType::OakDoor,
        Direction::East,
    );

    system.tick(&mut manager, &[(0, Y + 1, 0)]);
    assert_eq!(manager.get_block(0, Y, 0), BlockType::PressurePlate);
    assert_eq!(crate::world::BlockState::decode(manager.get_block_state(0, Y, 0)).is_open, true);
    assert_eq!(manager.get_block(1, Y, 0), BlockType::OakDoor);
    assert_eq!(crate::world::BlockState::decode(manager.get_block_state(1, Y, 0)).is_open, true);

    system.tick(&mut manager, &[]);
    assert_eq!(manager.get_block(0, Y, 0), BlockType::PressurePlate);
    assert_eq!(manager.get_block(1, Y, 0), BlockType::OakDoor);
}

#[test]
fn sleeping_constant_plate_occupant_skips_pressure_plate_scan() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    place(
        &mut system,
        &mut manager,
        0,
        BlockType::PressurePlate,
        Direction::East,
    );

    let occupant = (0, Y + 1, 0);
    system.tick(&mut manager, &[occupant]);
    system.tick(&mut manager, &[occupant]);
    assert!(system.is_sleeping());
    let scans = system.pressure_plate_scans;

    let update = system.tick(&mut manager, &[occupant]);
    assert!(update.mutations.is_empty());
    assert_eq!(system.pressure_plate_scans, scans);
    assert!(system.is_sleeping());
}

#[test]
fn occupant_movement_wakes_sleeping_pressure_plate_processing() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    place(
        &mut system,
        &mut manager,
        0,
        BlockType::PressurePlate,
        Direction::East,
    );
    place(
        &mut system,
        &mut manager,
        1,
        BlockType::OakDoor,
        Direction::East,
    );

    let occupant = (0, Y + 1, 0);
    system.tick(&mut manager, &[occupant]);
    system.tick(&mut manager, &[occupant]);
    assert_eq!(manager.get_block(0, Y, 0), BlockType::PressurePlate);
    assert_eq!(crate::world::BlockState::decode(manager.get_block_state(0, Y, 0)).is_open, true);
    assert_eq!(manager.get_block(1, Y, 0), BlockType::OakDoor);
    assert_eq!(crate::world::BlockState::decode(manager.get_block_state(1, Y, 0)).is_open, true);
    assert!(system.is_sleeping());

    let scans = system.pressure_plate_scans;
    let update = system.tick(&mut manager, &[]);
    assert!(!update.mutations.is_empty());
    assert_eq!(manager.get_block(0, Y, 0), BlockType::PressurePlate);
    assert_eq!(manager.get_block(1, Y, 0), BlockType::OakDoor);
    assert!(system.pressure_plate_scans > scans);
}

#[test]
fn sleeping_skips_comparator_and_observer_refresh() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    manager.set_block(0, Y, 0, BlockType::Chest);
    manager.set_block_entity(
        0,
        Y,
        0,
        Some(crate::block_entity::BlockEntity::Chest(
            crate::block_entity::ChestBlockEntity::new(),
        )),
    );
    place(
        &mut system,
        &mut manager,
        1,
        BlockType::Comparator,
        Direction::East,
    );
    place(
        &mut system,
        &mut manager,
        2,
        BlockType::Observer,
        Direction::East,
    );

    system.tick(&mut manager, &[]);
    system.tick(&mut manager, &[]);
    assert!(system.is_sleeping());
    let container_scans = system.container_revision_scans;
    let observer_scans = system.observer_scans;

    let update = system.tick(&mut manager, &[]);
    assert!(update.mutations.is_empty());
    assert!(system.is_sleeping());
    assert_eq!(system.container_revision_scans, container_scans);
    assert_eq!(system.observer_scans, observer_scans);
}

#[test]
fn pressure_plate_output_matches_independent_occupancy_oracle() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    let plates = [(0, Y, 0), (2, Y, 0)];
    for &(x, y, z) in &plates {
        manager.set_block(x, y, z, BlockType::PressurePlate);
        system.on_block_changed(&manager, (x, y, z), Direction::North);
    }

    let occupants = [(0, Y + 1, 0), (0, Y + 1, 0), (9, Y + 1, 9)];
    system.tick(&mut manager, &occupants);

    // Small independent oracle: a plate is powered iff an occupant stands
    // exactly one block above its x/z coordinate.
    for &(x, y, z) in &plates {
        let occupied = occupants
            .iter()
            .any(|&(ox, oy, oz)| ox == x && oz == z && oy == y + 1);
        assert_eq!(manager.get_block(x, y, z), BlockType::PressurePlate);
        assert_eq!(
            crate::world::BlockState::decode(manager.get_block_state(x, y, z)).is_open,
            occupied
        );
    }
}

#[test]
fn comparator_subtract_mode_uses_side_input() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    place(
        &mut system,
        &mut manager,
        0,
        BlockType::Lever,
        Direction::East,
    );
    {
        let mut st = crate::world::BlockState::default();
        st.is_open = true;
        manager.set_block_state(0, Y, 0, st.encode());
        system.on_block_changed(&manager, (0, Y, 0), Direction::East);
    }
    place(
        &mut system,
        &mut manager,
        1,
        BlockType::Comparator,
        Direction::East,
    );
    manager.set_block(1, Y, 1, BlockType::Lever);
    let mut __st = crate::world::BlockState::default();
    __st.is_open = true;
    manager.set_block_state(1, Y, 1, __st.encode());
    system.on_block_changed(&manager, (1, Y, 1), Direction::North);
    system.set_comparator_mode((1, Y, 0), ComparatorMode::Subtract);

    system.tick(&mut manager, &[]);
    assert_eq!(system.power_at((1, Y, 0)), 0);
    assert_eq!(manager.get_block(1, Y, 0), BlockType::Comparator);
}

#[test]
fn container_revision_wakes_sleeping_comparator() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    manager.set_block(0, Y, 0, BlockType::Chest);
    manager.set_block_entity(
        0,
        Y,
        0,
        Some(crate::block_entity::BlockEntity::Chest(
            crate::block_entity::ChestBlockEntity::new(),
        )),
    );
    place(
        &mut system,
        &mut manager,
        1,
        BlockType::Comparator,
        Direction::East,
    );

    system.tick(&mut manager, &[]);
    assert_eq!(system.power_at((1, Y, 0)), 0);
    assert!(system.is_sleeping());

    if let Some(entity) = manager.get_block_entity_mut(0, Y, 0) {
        entity.set_stack(
            0,
            Some(crate::inventory::ItemStack::new(
                crate::inventory::Item::Redstone,
                64,
            )),
        );
    }
    system.mark_container_changed(&manager, (0, Y, 0));
    assert!(!system.is_sleeping());
    system.tick(&mut manager, &[]);
    assert!(system.power_at((1, Y, 0)) > 0);
}

#[test]
fn dispenser_actions_are_rising_edge_only_and_carry_facing() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    place(
        &mut system,
        &mut manager,
        0,
        BlockType::Lever,
        Direction::East,
    );
    {
        let mut st = crate::world::BlockState::default();
        st.is_open = true;
        manager.set_block_state(0, Y, 0, st.encode());
        system.on_block_changed(&manager, (0, Y, 0), Direction::East);
    }
    place(
        &mut system,
        &mut manager,
        1,
        BlockType::Dispenser,
        Direction::South,
    );

    let first = system.tick(&mut manager, &[]);
    assert_eq!(
        first.actions,
        vec![RedstoneAction::Dispense {
            pos: (1, Y, 0),
            facing: Direction::South,
            dropper: false,
        }]
    );
    assert!(system.tick(&mut manager, &[]).actions.is_empty());

    {
        let mut st = crate::world::BlockState::default();
        st.is_open = false;
        manager.set_block_state(0, Y, 0, st.encode());
        system.on_block_changed(&manager, (0, Y, 0), Direction::East);
    }
    assert!(system.tick(&mut manager, &[]).actions.is_empty());
    {
        let mut st = crate::world::BlockState::default();
        st.is_open = true;
        manager.set_block_state(0, Y, 0, st.encode());
        system.on_block_changed(&manager, (0, Y, 0), Direction::East);
    }
    let second = system.tick(&mut manager, &[]);
    assert_eq!(second.actions.len(), 1);
}

#[test]
fn powered_dispenser_latch_roundtrips_without_phantom_edge() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    place(
        &mut system,
        &mut manager,
        0,
        BlockType::Lever,
        Direction::East,
    );
    {
        let mut st = crate::world::BlockState::default();
        st.is_open = true;
        manager.set_block_state(0, Y, 0, st.encode());
        system.on_block_changed(&manager, (0, Y, 0), Direction::East);
    }
    place(
        &mut system,
        &mut manager,
        1,
        BlockType::Dispenser,
        Direction::South,
    );
    let first = system.tick(&mut manager, &[]);
    assert_eq!(first.actions.len(), 1);
    let metadata = system.collect_chunk_metadata(&manager, 0, 0);
    assert_eq!(
        metadata
            .iter()
            .find(|entry| entry.local_x == 1)
            .map(|entry| entry.last_powered),
        Some(true)
    );

    let mut reloaded = RedstoneSystem::new();
    reloaded.restore_chunk_metadata(&manager, 0, 0, &metadata);
    assert!(reloaded.tick(&mut manager, &[]).actions.is_empty());
}

#[test]
fn observer_baselines_without_false_pulse_and_pulses_on_rising_change() {
    let mut manager = manager();
    let mut observer_entity = crate::block_entity::ObserverBlockEntity::new();
    observer_entity.facing = Direction::East;
    manager.set_block(0, Y, 0, BlockType::Observer);
    manager.set_block_entity(
        0,
        Y,
        0,
        Some(crate::block_entity::BlockEntity::Observer(observer_entity)),
    );
    manager.set_block(1, Y, 0, BlockType::Air);
    let mut system = RedstoneSystem::new();

    let initial = system.tick(&mut manager, &[]);
    assert!(initial.actions.is_empty());
    assert_eq!(system.power_at((0, Y, 0)), 0);

    manager.set_block(1, Y, 0, BlockType::Stone);
    system.on_block_changed(&manager, (1, Y, 0), Direction::East);
    system.tick(&mut manager, &[]);
    // The edge is delayed by one redstone tick and is therefore not
    // observable until the following tick.
    assert_eq!(system.power_at((0, Y, 0)), 0);
    system.tick(&mut manager, &[]);
    assert_eq!(system.power_at((0, Y, 0)), 15);
    system.tick(&mut manager, &[]);
    assert_eq!(system.power_at((0, Y, 0)), 15);
    system.tick(&mut manager, &[]);
    assert_eq!(system.power_at((0, Y, 0)), 0);

    // A fresh redstone runtime must use the persisted observer baseline,
    // not emit a phantom pulse merely because the chunk was reloaded.
    let mut reloaded = RedstoneSystem::new();
    let update = reloaded.tick(&mut manager, &[]);
    assert!(update.actions.is_empty());
    assert_eq!(reloaded.power_at((0, Y, 0)), 0);
}

#[test]
fn observer_skips_unloaded_front_until_streamed() {
    let mut manager = manager();
    manager.set_block(15, Y, 0, BlockType::Observer);
    manager.set_block_entity(
        15,
        Y,
        0,
        Some(crate::block_entity::BlockEntity::Observer(
            crate::block_entity::ObserverBlockEntity {
                facing: Direction::East,
                ..Default::default()
            },
        )),
    );
    let mut system = RedstoneSystem::new();
    system.tick(&mut manager, &[]);
    let observer = manager.get_block_entity(15, Y, 0).unwrap();
    assert!(matches!(
        observer,
        crate::block_entity::BlockEntity::Observer(o) if !o.baseline_initialized
    ));

    manager.insert_resident_chunk((1, 0), Chunk::new(1, 0));
    manager.set_block(16, Y, 0, BlockType::Stone);
    system.tick(&mut manager, &[]);
    let observer = manager.get_block_entity(15, Y, 0).unwrap();
    if let crate::block_entity::BlockEntity::Observer(observer) = observer {
        assert!(observer.baseline_initialized);
        assert_eq!(observer.pending_pulse, 0);
    } else {
        panic!("expected observer block entity");
    }
}

#[test]
fn scheduled_redstone_work_is_bounded_and_deterministic() {
    let mut system = RedstoneSystem::new();
    for index in 0..(MAX_SCHEDULED_REDSTONE_TICKS + 128) {
        system.schedule_tick(ScheduledTick {
            due: index as u64,
            pos: (index as i32, Y, 0),
            kind: ScheduledKind::ObserverPulseOn,
        });
    }
    assert_eq!(system.scheduled.len(), MAX_SCHEDULED_REDSTONE_TICKS);
    assert!(system.scheduled.windows(2).all(|window| (
        window[0].due,
        window[0].pos,
        scheduled_kind_key(window[0].kind)
    ) <= (
        window[1].due,
        window[1].pos,
        scheduled_kind_key(window[1].kind)
    )));
}

#[test]
fn direct_sources_strongly_charge_solid_blocks() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    place(
        &mut system,
        &mut manager,
        0,
        BlockType::Lever,
        Direction::East,
    );
    manager.set_block(1, Y, 0, BlockType::Stone);
    place(
        &mut system,
        &mut manager,
        2,
        BlockType::RedstoneWire,
        Direction::East,
    );
    system.interact(&mut manager, (0, Y, 0));
    system.tick(&mut manager, &[]);

    assert_eq!(
        system.block_state_at(&manager, (1, Y, 0)),
        RedstoneState {
            power: 15,
            charge: ChargeKind::Strong,
        }
    );
    assert_eq!(system.power_at((2, Y, 0)), 15);
}

#[test]
fn powered_tnt_keeps_its_fuse_after_the_block_is_removed() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    place(
        &mut system,
        &mut manager,
        0,
        BlockType::Lever,
        Direction::East,
    );
    place(
        &mut system,
        &mut manager,
        1,
        BlockType::TNT,
        Direction::East,
    );
    system.interact(&mut manager, (0, Y, 0));

    let first = system.tick(&mut manager, &[]);
    assert_eq!(manager.get_block(1, Y, 0), BlockType::Air);
    assert!(first.actions.is_empty());
    for _ in 0..79 {
        assert!(system.tick(&mut manager, &[]).actions.is_empty());
    }
    let fired = system.tick(&mut manager, &[]);
    assert_eq!(
        fired.actions,
        vec![RedstoneAction::Explode { pos: (1, Y, 0) }]
    );
}

#[test]
fn collect_and_restore_preserves_repeater_delay_comparator_mode_and_note() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    // Repeater with a non-default delay (4) and a non-North facing.
    place(
        &mut system,
        &mut manager,
        1,
        BlockType::Repeater,
        Direction::East,
    );
    system.set_repeater_delay((1, Y, 0), 4);
    // Comparator in Subtract mode.
    place(
        &mut system,
        &mut manager,
        2,
        BlockType::Comparator,
        Direction::South,
    );
    system.set_comparator_mode((2, Y, 0), ComparatorMode::Subtract);
    // NoteBlock tuned to pitch 12.
    place(
        &mut system,
        &mut manager,
        3,
        BlockType::NoteBlock,
        Direction::West,
    );
    for _ in 0..12 {
        system.interact(&mut manager, (3, Y, 0));
    }

    let metadata = system.collect_chunk_metadata(&manager, 0, 0);
    assert_eq!(metadata.len(), 3);

    // Simulate the unload+reload path: drop the in-memory component state
    // and let `sync_loaded_chunks` rebuild default entries from the blocks.
    let mut reloaded = RedstoneSystem::new();
    reloaded.tick(&mut manager, &[]);
    // Defaults restored by `sync_loaded_chunks` must differ from the saved
    // values before we apply the sidecar.
    assert_eq!(reloaded.repeater_delay((1, Y, 0)), Some(1));
    assert_eq!(
        reloaded.comparator_mode((2, Y, 0)),
        Some(ComparatorMode::Compare)
    );

    reloaded.restore_chunk_metadata(&manager, 0, 0, &metadata);
    assert_eq!(reloaded.repeater_delay((1, Y, 0)), Some(4));
    assert_eq!(
        reloaded.comparator_mode((2, Y, 0)),
        Some(ComparatorMode::Subtract)
    );
    assert_eq!(
        reloaded.components.get(&(3, Y, 0)).map(|s| s.note),
        Some(12)
    );
    assert_eq!(
        reloaded.components.get(&(1, Y, 0)).map(|s| s.facing),
        Some(Direction::East)
    );
    assert_eq!(
        reloaded.components.get(&(2, Y, 0)).map(|s| s.facing),
        Some(Direction::South)
    );
    assert_eq!(
        reloaded.components.get(&(3, Y, 0)).map(|s| s.facing),
        Some(Direction::West)
    );
}

#[test]
fn collect_skips_components_with_default_metadata() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    // A freshly-placed Repeater with default delay 1 and North facing
    // carries no persistent metadata and must not appear in the sidecar.
    place(
        &mut system,
        &mut manager,
        1,
        BlockType::Repeater,
        Direction::North,
    );
    // A lever is a component but only carries the default state.
    place(
        &mut system,
        &mut manager,
        2,
        BlockType::Lever,
        Direction::North,
    );

    let metadata = system.collect_chunk_metadata(&manager, 0, 0);
    assert!(metadata.is_empty());
}

#[test]
fn collect_only_emits_components_inside_the_target_chunk() {
    let mut manager = WorldColumns::new(2);
    manager.insert_resident_chunk((0, 0), Chunk::new(0, 0));
    manager.insert_resident_chunk((1, 0), Chunk::new(1, 0));
    let mut system = RedstoneSystem::new();
    // First component inside chunk (0, 0).
    manager.set_block(1, Y, 0, BlockType::Repeater);
    system.on_block_changed(&mut manager, (1, Y, 0), Direction::East);
    system.set_repeater_delay((1, Y, 0), 3);
    // Second component inside chunk (1, 0) (x = 16..31).
    manager.set_block(17, Y, 0, BlockType::Repeater);
    system.on_block_changed(&mut manager, (17, Y, 0), Direction::East);
    system.set_repeater_delay((17, Y, 0), 2);

    let metadata_chunk_0 = system.collect_chunk_metadata(&manager, 0, 0);
    assert_eq!(metadata_chunk_0.len(), 1);
    assert_eq!(metadata_chunk_0[0].local_x, 1);
    assert_eq!(metadata_chunk_0[0].repeater_delay, 3);

    let metadata_chunk_1 = system.collect_chunk_metadata(&manager, 1, 0);
    assert_eq!(metadata_chunk_1.len(), 1);
    assert_eq!(metadata_chunk_1[0].local_x, 1);
    assert_eq!(metadata_chunk_1[0].repeater_delay, 2);
}

#[test]
fn sidecar_preserves_signed_world_y() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    manager.set_block(4, -20, 5, BlockType::Repeater);
    system.on_block_changed(&mut manager, (4, -20, 5), Direction::East);
    system.set_repeater_delay((4, -20, 5), 3);
    manager.set_block(4, 256, 5, BlockType::Repeater);
    system.on_block_changed(&mut manager, (4, 256, 5), Direction::West);
    system.set_repeater_delay((4, 256, 5), 2);

    let metadata = system.collect_chunk_metadata(&manager, 0, 0);
    let ys: Vec<i16> = metadata.iter().map(|entry| entry.local_y).collect();
    assert!(
        ys.contains(&-20),
        "negative world Y must persist, got {ys:?}"
    );
    assert!(ys.contains(&256), "Y>=256 must persist, got {ys:?}");

    let mut reloaded = RedstoneSystem::new();
    reloaded.tick(&mut manager, &[]);
    reloaded.restore_chunk_metadata(&manager, 0, 0, &metadata);
    assert_eq!(reloaded.repeater_delay((4, -20, 5)), Some(3));
    assert_eq!(reloaded.repeater_delay((4, 256, 5)), Some(2));
}

#[test]
fn restore_ignores_entries_whose_block_is_no_longer_a_component() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    manager.set_block(1, Y, 0, BlockType::Repeater);
    system.on_block_changed(&mut manager, (1, Y, 0), Direction::East);
    let metadata = system.collect_chunk_metadata(&manager, 0, 0);
    assert_eq!(metadata.len(), 1);

    // Simulate the block being replaced with Stone before reload. The
    // stale sidecar entry must not resurrect a facing on a non-component.
    manager.set_block(1, Y, 0, BlockType::Stone);
    let mut reloaded = RedstoneSystem::new();
    reloaded.tick(&mut manager, &[]);
    reloaded.restore_chunk_metadata(&manager, 0, 0, &metadata);
    assert!(reloaded.components.get(&(1, Y, 0)).is_none());
}

#[test]
fn cross_chunk_redstone_line_propagation() {
    let mut manager = WorldColumns::new(2);
    manager.insert_resident_chunk((0, 0), Chunk::new(0, 0));
    manager.insert_resident_chunk((1, 0), Chunk::new(1, 0));
    let mut system = RedstoneSystem::new();

    // Place Lever at x=14 (chunk 0) and RedstoneWires across x=15 (chunk 0) to x=20 (chunk 1)
    manager.set_block(14, Y, 0, BlockType::Lever);
    system.on_block_changed(&manager, (14, Y, 0), Direction::North);

    for x in 15..=20 {
        manager.set_block(x, Y, 0, BlockType::RedstoneWire);
        system.on_block_changed(&manager, (x, Y, 0), Direction::North);
    }

    // Toggle Lever ON
    system.interact(&mut manager, (14, Y, 0));
    system.tick(&mut manager, &[]);

    assert_eq!(system.power_at((14, Y, 0)), 15);
    assert_eq!(system.power_at((15, Y, 0)), 15); // Wire adjacent to Lever gets full 15 power
    assert_eq!(system.power_at((16, Y, 0)), 14); // In chunk 1 (attenuated by 1)
    assert_eq!(system.power_at((17, Y, 0)), 13);
    assert_eq!(system.power_at((20, Y, 0)), 10);
}

#[test]
fn sleeping_mechanism_behavior() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();

    place(
        &mut system,
        &mut manager,
        0,
        BlockType::Lever,
        Direction::North,
    );
    place(
        &mut system,
        &mut manager,
        1,
        BlockType::RedstoneWire,
        Direction::North,
    );

    // Initial tick settles power and enters sleeping state
    system.tick(&mut manager, &[]);
    assert!(system.is_sleeping());

    // Subsequent tick when idle stays sleeping
    let component_scans = system.component_sync_scans;
    let idle_update = system.tick(&mut manager, &[]);
    assert!(idle_update.mutations.is_empty());
    assert_eq!(system.component_sync_scans, component_scans);
    assert!(system.is_sleeping());

    // Interaction wakes system up
    system.interact(&mut manager, (0, Y, 0));
    assert!(!system.is_sleeping());

    // Tick settles again and returns to sleeping
    system.tick(&mut manager, &[]);
    assert!(system.is_sleeping());
    assert_eq!(system.power_at((0, Y, 0)), 15);
    assert_eq!(system.power_at((1, Y, 0)), 15);
}

#[test]
fn sleeping_redstone_skips_loaded_chunk_key_probes() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();
    place(
        &mut system,
        &mut manager,
        0,
        BlockType::Lever,
        Direction::North,
    );
    system.tick(&mut manager, &[]);
    assert!(system.is_sleeping());
    let probes = system.loaded_chunk_key_probes;
    let generation = manager.load_generation();
    let known = system.known_load_generation;
    system.tick(&mut manager, &[]);
    assert!(system.is_sleeping());
    assert_eq!(system.loaded_chunk_key_probes, probes);
    assert_eq!(manager.load_generation(), generation);
    assert_eq!(system.known_load_generation, known);
    assert_eq!(
        known, generation,
        "sleep path must compare generation, not probe chunk keys"
    );
}

fn reference_full_settle(
    components: &mut HashMap<BlockPos, ComponentState>,
    manager: &WorldColumns,
) -> bool {
    // Deliberately small reference model.  Keep this independent from the
    // production evaluator: it only models the fixture primitives used by
    // the differential tests (sources, wires, repeaters and consumers).
    fn reference_output(
        manager: &WorldColumns,
        snapshot: &HashMap<BlockPos, ComponentState>,
        pos: BlockPos,
        block: BlockType,
        _state: ComponentState,
    ) -> u8 {
        let open = crate::world::BlockState::decode(manager.get_block_state(pos.0, pos.1, pos.2))
            .is_open;
        let own_source = match block {
            BlockType::RedstoneTorch => !open,
            BlockType::Lever | BlockType::StoneButton | BlockType::PressurePlate => open,
            BlockType::Repeater => open,
            _ => false,
        };
        if own_source {
            return 15;
        }
        if matches!(
            block,
            BlockType::Lever | BlockType::StoneButton | BlockType::PressurePlate
        ) {
            return 0;
        }
        // Unpowered repeater emits nothing until its scheduled tick sets the open bit.
        if block == BlockType::Repeater {
            return 0;
        }

        let mut best = 0;
        for offset in NEIGHBORS {
            let neighbor = add(pos, offset);
            let Some(neighbor_state) = snapshot.get(&neighbor).copied() else {
                continue;
            };
            let neighbor_block = get_block(manager, neighbor);
            let neighbor_open = crate::world::BlockState::decode(
                manager.get_block_state(neighbor.0, neighbor.1, neighbor.2),
            )
            .is_open;
            let mut emitted = neighbor_state.signal.power;
            if matches!(neighbor_block, BlockType::Repeater | BlockType::Comparator) {
                if !neighbor_open
                    || add(neighbor, neighbor_state.facing.delta()) != pos
                {
                    emitted = 0;
                }
            }
            if neighbor_block == BlockType::RedstoneWire
                && matches!(block, BlockType::RedstoneWire)
            {
                emitted = emitted.saturating_sub(1);
            }
            best = best.max(emitted);
        }

        best
    }

    for _ in 0..MAX_PROPAGATION_PASSES {
        let snapshot = components.clone();
        let mut changed = false;
        for (&pos, state) in components.iter_mut() {
            let block = get_block(manager, pos);
            let new_power = reference_output(manager, &snapshot, pos, block, *state);
            let new_charge = if new_power == 0 {
                ChargeKind::Unpowered
            } else if matches!(
                block,
                BlockType::RedstoneTorch
                    | BlockType::Lever
                    | BlockType::StoneButton
                    | BlockType::PressurePlate
                    | BlockType::Repeater
            ) {
                ChargeKind::Strong
            } else {
                ChargeKind::Weak
            };
            if state.signal.power != new_power || state.signal.charge != new_charge {
                state.signal.power = new_power;
                state.signal.charge = new_charge;
                changed = true;
            }
        }
        if !changed {
            return true;
        }
    }
    false
}

#[test]
fn differential_dirty_worklist_vs_full_settle_parity() {
    let mut manager = WorldColumns::new(2);
    manager.insert_resident_chunk((0, 0), Chunk::new(0, 0));
    manager.insert_resident_chunk((1, 0), Chunk::new(1, 0));
    let mut system = RedstoneSystem::new();

    // Build circuit spanning multiple chunks:
    // Lever at 0, wires 1..10, repeater at 11, wires 12..20
    manager.set_block(0, Y, 0, BlockType::Lever);
    system.on_block_changed(&manager, (0, Y, 0), Direction::East);

    for x in 1..=10 {
        manager.set_block(x, Y, 0, BlockType::RedstoneWire);
        system.on_block_changed(&manager, (x, Y, 0), Direction::North);
    }
    manager.set_block(11, Y, 0, BlockType::Repeater);
    system.on_block_changed(&manager, (11, Y, 0), Direction::East);

    for x in 12..=20 {
        manager.set_block(x, Y, 0, BlockType::RedstoneWire);
        system.on_block_changed(&manager, (x, Y, 0), Direction::North);
    }
    manager.set_block(21, Y, 0, BlockType::OakDoor);
    system.on_block_changed(&manager, (21, Y, 0), Direction::East);

    // Action 1: Toggle lever ON and tick. Both evaluators start from the
    // same pre-transition component fixture.
    let mut ref_components = system.components.clone();
    system.interact(&mut manager, (0, Y, 0));
    system.tick(&mut manager, &[]);

    // Check parity against full settle reference
    reference_full_settle(&mut ref_components, &manager);
    for (pos, state) in &system.components {
        let ref_state = ref_components.get(pos).unwrap();
        assert_eq!(
            state.signal.power, ref_state.signal.power,
            "Mismatch at pos {:?}",
            pos
        );
        assert_eq!(
            state.signal.charge, ref_state.signal.charge,
            "Mismatch at pos {:?}",
            pos
        );
    }

    // Action 2: Advance ticks for repeater propagation. After each production
    // tick the settled component map must be a fixed point of the reference
    // full-settle evaluator (same power rules, including BlockState.open).
    for _ in 0..5 {
        system.tick(&mut manager, &[]);
        let mut ref_comp = system.components.clone();
        reference_full_settle(&mut ref_comp, &manager);
        for (pos, state) in &system.components {
            let ref_state = ref_comp.get(pos).unwrap();
            assert_eq!(
                state.signal.power, ref_state.signal.power,
                "Mismatch post-repeater at pos {:?}",
                pos
            );
            assert_eq!(
                state.signal.charge, ref_state.signal.charge,
                "Mismatch post-repeater at pos {:?}",
                pos
            );
        }
    }
}

#[test]
fn loop_budget_parity_test() {
    let mut manager = manager();
    let mut system = RedstoneSystem::new();

    // Create a feedback loop of wires
    let positions = [(0, Y, 0), (1, Y, 0), (1, Y, 1), (0, Y, 1)];
    for &pos in &positions {
        manager.set_block(pos.0, pos.1, pos.2, BlockType::RedstoneWire);
        system.on_block_changed(&manager, pos, Direction::North);
    }

    system.tick(&mut manager, &[]);
    assert!(!system.tick(&mut manager, &[]).propagation_overflowed);
}
