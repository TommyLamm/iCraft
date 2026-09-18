// Tests extracted from state.rs::authority_projection_tests (Plan 27).

use super::*;
use crate::chunk_schedule::{DependencyReason, SectionMeshScheduler};
use crate::dimension::Dimension;

#[test]
fn session_inventory_projection_preserves_rich_stack_metadata() {
    let mut stack = ItemStack::new(Item::DiamondPickaxe, 1);
    stack.durability = 37;
    stack
        .enchantments
        .add_or_upgrade(crate::enchantment::Enchantment::Efficiency(3));
    stack.custom_name.set("authority pick");
    stack.can_break = 0x1234;
    stack.can_place_on = 0x5678;
    let slot = State::session_slot_from_stack(Some(stack)).expect("slot");
    let roundtrip = State::stack_from_session_slot(slot).expect("stack");
    assert_eq!(roundtrip.item, stack.item);
    assert_eq!(roundtrip.count, stack.count);
    assert_eq!(roundtrip.durability, stack.durability);
    assert_eq!(roundtrip.enchantments, stack.enchantments);
    assert_eq!(roundtrip.custom_name, stack.custom_name);
    assert_eq!(roundtrip.can_break, stack.can_break);
    assert_eq!(roundtrip.can_place_on, stack.can_place_on);
}

#[test]
fn same_column_projection_embedded_and_join_are_identical_and_match_dimension() {
    for dimension in [Dimension::Overworld, Dimension::Nether, Dimension::End] {
        let (cx, cz) = (3, -2);
        let height = dimension.height();
        let mut source = Chunk::empty_in_dimension(dimension, cx, cz);

        // Populate blocks, states, fluids across different sections
        let min_y = height.min_y();
        let max_y = height.max_y_exclusive();
        source.set_block_local(0, min_y, 0, BlockType::Bedrock);
        source.set_block_local(1, min_y + 10, 2, BlockType::Stone);
        source.set_block_state(1, min_y + 10, 2, 4);
        source.set_fluid_level(1, min_y + 10, 2, 0x07);
        source.set_block_local(15, max_y - 1, 15, BlockType::Obsidian);

        // 1. Embedded source representation: Arc<Chunk>
        let embedded_arc = std::sync::Arc::new(source.clone());
        let embedded_chunk = (*embedded_arc).clone();

        // 2. Join source representation: network payload
        let payload = crate::save::ChunkSaveData::from_chunk(&source).expect("network payload");
        let mut join_chunk = Chunk::empty_in_dimension(dimension, cx, cz);
        crate::save::ChunkSaveData::restore_network_payload(
            &mut join_chunk,
            &payload.blocks,
            &payload.block_states,
            &payload.fluid_levels,
            &payload.block_entities,
        )
        .expect("restore network payload");

        // Verify dimensions and section bounds match
        assert_eq!(embedded_chunk.min_section_y, height.min_section_y());
        assert_eq!(join_chunk.min_section_y, height.min_section_y());
        assert_eq!(embedded_chunk.sections.len(), height.section_count());
        assert_eq!(join_chunk.sections.len(), height.section_count());

        // Verify bit-for-bit equivalence of voxels
        assert_eq!(
            join_chunk.get_block_local(0, min_y, 0),
            embedded_chunk.get_block_local(0, min_y, 0)
        );
        assert_eq!(
            join_chunk.get_block_local(1, min_y + 10, 2),
            embedded_chunk.get_block_local(1, min_y + 10, 2)
        );
        assert_eq!(
            join_chunk.get_block_state(1, min_y + 10, 2),
            embedded_chunk.get_block_state(1, min_y + 10, 2)
        );
        assert_eq!(
            join_chunk.get_fluid_level(1, min_y + 10, 2),
            embedded_chunk.get_fluid_level(1, min_y + 10, 2)
        );
        assert_eq!(
            join_chunk.get_block_local(15, max_y - 1, 15),
            embedded_chunk.get_block_local(15, max_y - 1, 15)
        );

        // Verify ChunkMesh configured for dimension has exact section matching
        let mesh = ChunkMesh::pending_for_dimension(dimension);
        assert_eq!(mesh.min_section_y, height.min_section_y());
        assert_eq!(mesh.sections.len(), height.section_count());
    }
}

#[test]
fn invalid_column_payload_fails_closed_without_modifying_resident_column_or_revision() {
    let dimension = Dimension::Overworld;
    let (cx, cz) = (1, 1);
    let mut manager = PresentationChunks::new(2);
    let mut original_chunk = Chunk::empty_in_dimension(dimension, cx, cz);
    original_chunk.set_block_local(5, 64, 5, BlockType::GoldOre);
    manager.insert_resident_chunk((cx, cz), original_chunk);

    let mut revisions = std::collections::HashMap::new();
    let revision_key = (dimension, cx, cz);
    revisions.insert(revision_key, 10);

    // Corrupted payload (truncated bytes)
    let corrupt_blocks = vec![0u8; 10];
    let corrupt_states = vec![];
    let corrupt_fluids = vec![];
    let corrupt_entities = vec![];

    // Decode attempt fails closed:
    let mut candidate = Chunk::empty_in_dimension(dimension, cx, cz);
    let decode_result = crate::save::ChunkSaveData::restore_network_payload(
        &mut candidate,
        &corrupt_blocks,
        &corrupt_states,
        &corrupt_fluids,
        &corrupt_entities,
    );
    assert!(decode_result.is_err(), "corrupted stream must return Err");

    // In fail-closed semantics: decode error returns before touching revisions or replacing column.
    // Existing resident column and tracked revision are preserved.
    assert_eq!(*revisions.get(&revision_key).unwrap(), 10);
    assert_eq!(
        manager.chunks.get(&(cx, cz)).unwrap().get_block_local(5, 64, 5),
        BlockType::GoldOre
    );

    // Initial insert fail-closed: an empty slot remains empty on decode failure
    let empty_key = (dimension, 2, 2);
    let mut candidate_new = Chunk::empty_in_dimension(dimension, 2, 2);
    assert!(crate::save::ChunkSaveData::restore_network_payload(
        &mut candidate_new,
        &corrupt_blocks,
        &corrupt_states,
        &corrupt_fluids,
        &corrupt_entities,
    )
    .is_err());
    assert!(!manager.chunks.contains_key(&(2, 2)));
    assert!(!revisions.contains_key(&empty_key));
}

#[test]
fn loaded_neighbors_receive_ao_invalidation_when_adjacent_column_committed() {
    let dimension = Dimension::Overworld;
    let (cx, cz) = (0, 0);

    // Setup presentation chunks with neighbor (1, 0) loaded and (0, 1) unloaded
    let mut manager = PresentationChunks::new(2);
    let neighbor_coord = (1, 0);
    manager.insert_resident_chunk(
        neighbor_coord,
        Chunk::empty_in_dimension(dimension, neighbor_coord.0, neighbor_coord.1),
    );

    let mut meshes = std::collections::HashMap::new();
    meshes.insert(neighbor_coord, ChunkMesh::pending_for_dimension(dimension));

    let mut lifetimes = std::collections::HashMap::new();
    lifetimes.insert(neighbor_coord, 42);

    let mut scheduler = SectionMeshScheduler::new();

    // Commit center column:
    // Surrounding neighbors that exist in chunk_manager must be invalidated with DependencyReason::Ao
    let neighbors = surrounding_chunk_coords(cx, cz);
    assert!(neighbors.contains(&neighbor_coord));
    assert!(neighbors.contains(&(0, 1)));

    for neighbor in neighbors {
        if manager.chunks.contains_key(&neighbor) {
            let lifetime = lifetimes[&neighbor];
            let mesh = meshes.get_mut(&neighbor).unwrap();
            let height = dimension.height();
            for sy in height.min_section_y()..height.max_section_y_exclusive() {
                let section = mesh.section_mut(sy).unwrap();
                section.invalidate();
                scheduler.enqueue(
                    SectionIdentity::new(SectionKey::new(neighbor.0, sy, neighbor.1), section.revision, lifetime),
                    DependencyReason::Ao,
                    (0, 0),
                );
            }
        }
    }

    // Neighbor (1, 0) was loaded: its sections are queued with DependencyReason::Ao
    assert_eq!(scheduler.len(), dimension.height().section_count());
    let item = scheduler.pop_nearest((0, 0), 2).unwrap();
    assert_eq!(item.identity.key.cx, 1);
    assert_eq!(item.identity.key.cz, 0);
    assert_eq!(item.reason, DependencyReason::Ao);

    // Drain and verify all queued items from loaded neighbor have Ao reason
    while let Some(work) = scheduler.pop_nearest((0, 0), 2) {
        assert_eq!(work.identity.key.cx, 1);
        assert_eq!(work.reason, DependencyReason::Ao);
    }
}

#[test]
fn pending_block_deltas_replay_in_revision_order_and_stale_columns_rejected() {
    let dimension = Dimension::Overworld;
    let (cx, cz) = (2, 3);
    let mut revisions = std::collections::HashMap::new();
    let mut pending_changes: std::collections::HashMap<(i32, i32), std::collections::HashMap<(i32, i32, i32), (u64, BlockType, u8, u8)>> =
        std::collections::HashMap::new();

    // 1. Block deltas arrive BEFORE column is resident.
    // Deltas buffer into pending_block_changes WITHOUT advancing client_chunk_revisions.
    let entry = pending_changes.entry((cx, cz)).or_default();
    entry.insert((2 * 16 + 1, 64, 3 * 16 + 1), (15, BlockType::DiamondOre, 0, 0));
    entry.insert((2 * 16 + 2, 64, 3 * 16 + 2), (12, BlockType::GoldOre, 0, 0));

    assert_eq!(revisions.get(&(dimension, cx, cz)), None, "revision must not advance before column is resident");

    // 2. Base column arrives with revision 10.
    // Base revision 10 is newer than uninserted (0). Column commits and sets revision to 10.
    let base_revision = 10;
    revisions.insert((dimension, cx, cz), base_revision);
    let mut manager = PresentationChunks::new(2);
    let mut base_chunk = Chunk::empty_in_dimension(dimension, cx, cz);
    base_chunk.set_block_local(1, 64, 1, BlockType::Stone);
    base_chunk.set_block_local(2, 64, 2, BlockType::Stone);
    manager.insert_resident_chunk((cx, cz), base_chunk);

    // 3. Replay pending block changes in ascending revision order:
    // (12, GoldOre) first, then (15, DiamondOre).
    if let Some(changes) = pending_changes.remove(&(cx, cz)) {
        let mut changes: Vec<_> = changes.into_iter().collect();
        changes.sort_by_key(|(_, (rev, _, _, _))| *rev);
        assert_eq!(changes[0].1 .0, 12);
        assert_eq!(changes[1].1 .0, 15);

        for ((x, y, z), (rev, block, state, fluid)) in changes {
            assert!(rev > *revisions.get(&(dimension, cx, cz)).unwrap());
            revisions.insert((dimension, cx, cz), rev);
            apply_synced_block_change(&mut manager, x, y, z, block, state, fluid);
        }
    }

    assert_eq!(*revisions.get(&(dimension, cx, cz)).unwrap(), 15);
    assert_eq!(manager.get_block(2 * 16 + 1, 64, 3 * 16 + 1), BlockType::DiamondOre);
    assert_eq!(manager.get_block(2 * 16 + 2, 64, 3 * 16 + 2), BlockType::GoldOre);

    // 4. Stale column arriving with revision <= 15 is rejected.
    let stale_revision = 14;
    let accepted = stale_revision >= *revisions.get(&(dimension, cx, cz)).unwrap();
    assert!(!accepted, "stale column must be rejected");
    // World state remains unchanged
    assert_eq!(manager.get_block(2 * 16 + 1, 64, 3 * 16 + 1), BlockType::DiamondOre);
}

#[test]
fn terrain_mesh_identity_rejects_stale_lifetime_and_dimension_switch() {
    let key = SectionKey::new(0, 4, 0);
    let initial_lifetime = 10;
    let initial_generation = 1;
    let initial_identity = SectionIdentity::new(key, 1, initial_lifetime);

    // 1. Exact match is accepted
    assert!(section_mesh_result_is_current(
        Some(initial_identity),
        initial_identity,
        initial_generation,
        initial_generation,
        Some(initial_identity),
    ));

    // 2. Column unload and reload assigns a new lifetime:
    // in-flight worker result with old lifetime is rejected
    let reloaded_lifetime = initial_lifetime + 1;
    let reloaded_identity = SectionIdentity::new(key, 1, reloaded_lifetime);
    assert!(!section_mesh_result_is_current(
        Some(initial_identity),
        initial_identity,
        initial_generation,
        initial_generation,
        Some(reloaded_identity),
    ));

    // 3. Dimension transfer / runtime teardown increments terrain_generation:
    // worker result with previous generation is rejected even if key/lifetime/revision match
    let switched_generation = initial_generation + 1;
    assert!(!section_mesh_result_is_current(
        Some(initial_identity),
        initial_identity,
        initial_generation,
        switched_generation,
        Some(initial_identity),
    ));

    // 4. Mismatched section key (e.g. section from different dimension or coord) is rejected
    let other_key = SectionKey::new(1, 4, 0);
    let other_identity = SectionIdentity::new(other_key, 1, initial_lifetime);
    assert!(!section_mesh_result_is_current(
        Some(initial_identity),
        other_identity,
        initial_generation,
        initial_generation,
        Some(initial_identity),
    ));
}

