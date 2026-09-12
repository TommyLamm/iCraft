// Tests extracted from state.rs::remote_sync_tests (Plan 27).

use super::*;

use super::*;

#[test]
fn interpolation_midpoint_and_clamps() {
    let prev = PlayerSnapshot {
        position: Vec3::ZERO,
        yaw: 3.0,
        pitch: 0.0,
        time: 1.0,
        sequence: 1,
        sender_time_millis: 1000,
    };
    let latest = PlayerSnapshot {
        position: Vec3::new(10.0, 2.0, -4.0),
        yaw: -3.0,
        pitch: 1.0,
        time: 1.05,
        sequence: 2,
        sender_time_millis: 1050,
    };
    let mid = interpolate_snapshot(prev, latest, 1.025);
    assert!((mid.position.x - 5.0).abs() < 1e-5);
    assert!((mid.position.y - 1.0).abs() < 1e-5);
    assert!((mid.position.z + 2.0).abs() < 1e-5);
    let before = interpolate_snapshot(prev, latest, 0.0);
    let after = interpolate_snapshot(prev, latest, 2.0);
    assert_eq!(before.position, prev.position);
    assert_eq!(after.position, latest.position);
    assert!(
        mid.yaw.abs() > 3.0,
        "yaw should interpolate across the short wrap-around arc"
    );
}

#[test]
fn sequence_order_handles_duplicates_old_packets_and_wraparound() {
    assert!(sequence_is_newer(2, 1));
    assert!(!sequence_is_newer(1, 1));
    assert!(!sequence_is_newer(1, 2));
    assert!(sequence_is_newer(0, u32::MAX));
    assert!(!sequence_is_newer(u32::MAX, 0));
}

#[test]
fn container_revision_order_rejects_duplicates_and_accepts_wraparound() {
    assert!(container_revision_is_newer(4, 5));
    assert!(!container_revision_is_newer(5, 5));
    assert!(!container_revision_is_newer(5, 4));
    assert!(container_revision_is_newer(u64::MAX, 0));
    assert!(!container_revision_is_newer(0, u64::MAX));
}

#[test]
fn network_burst_budget_leaves_persistent_backlog() {
    let mut staging = NetworkStaging::default();
    for _ in 0..(NETWORK_MAX_EVENTS_PER_PASS + 17) {
        staging.stage(NetworkInbound::StatusUpdate { message: "burst".into() });
    }
    for _ in 0..NETWORK_MAX_EVENTS_PER_PASS {
        assert!(staging.pop_next_if_fits(usize::MAX).is_some());
    }
    assert_eq!(staging.reliable_len(), 17);
}

#[test]
fn reliable_events_remain_strict_fifo_until_eventual_delivery() {
    let mut staging = NetworkStaging::default();
    for event in [
        NetworkInbound::StatusUpdate { message: "one".into() },
        NetworkInbound::StatusUpdate { message: "two".into() },
        NetworkInbound::StatusUpdate { message: "three".into() },
    ] {
        staging.stage(event);
    }
    let first_bytes = staging.reliable.front().unwrap().estimated_bytes();
    assert!(staging
        .pop_next_if_fits(first_bytes.saturating_sub(1))
        .is_none());
    assert_eq!(staging.reliable_len(), 3);

    let mut delivered = Vec::new();
    while let Some((event, _)) = staging.pop_next_if_fits(usize::MAX) {
        if let NetworkInbound::StatusUpdate { message } = event {
            delivered.push(message);
        }
    }
    assert_eq!(delivered, ["one", "two", "three"]);
}

#[test]
fn latest_wins_state_is_sequence_aware_per_key() {
    use crate::network::protocol::{Packet, PROTOCOL_VERSION};
    let mut staging = NetworkStaging::default();
    for (id, sequence, x) in [(7_u64, 2_u32, 2.0_f32), (7, 1, 1.0), (8, 4, 4.0)] {
        staging.stage(NetworkInbound::Packet(Packet::PlayerPosition {
            id,
            sequence,
            sender_time_millis: sequence as u64,
            x,
            y: 0.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
        }));
    }
    for sequence in [9, 8, 10] {
        staging.stage(NetworkInbound::Packet(Packet::PlayerHealth {
            sequence,
            player_id: 3,
            health: sequence as f32,
            max_health: 20.0,
            hunger: 19.0,
            saturation: 4.0,
            oxygen: 20.0,
            is_dead: false,
            death_reason: 0,
        }));
        staging.stage(NetworkInbound::Packet(Packet::PlayerEffect {
            sequence,
            player_id: 3,
            effects: Vec::new(),
        }));
    }
    for ticks in [40, 30, 50] {
        staging.stage(NetworkInbound::Packet(Packet::TimeSync {
            ticks,
            weather: 0,
            weather_remaining_ticks: 0.0,
        }));
    }
    for sequence in [4, 3, 5] {
        staging.stage(NetworkInbound::Packet(Packet::EntityState {
            dimension: 0,
            sequence,
            state: crate::network::protocol::EntityStateWire {
                entity_id: 99,
                entity_type: crate::entity::EntityType::Zombie.to_wire(),
                position: [sequence as f32, 0.0, 0.0],
                velocity: [0.0; 3],
                yaw: 0.0,
                pitch: 0.0,
                health: 20.0,
                animation_state: 0,
                item: None,
            },
        }));
    }

    assert_eq!(staging.latest_positions.len(), 2);
    assert!(matches!(
        staging.latest_positions.get(&7),
        Some(NetworkInbound::Packet(Packet::PlayerPosition { sequence: 2, .. }))
    ));
    assert!(matches!(
        staging.latest_health.get(&3),
        Some(NetworkInbound::Packet(Packet::PlayerHealth { sequence: 10, .. }))
    ));
    assert!(matches!(
        staging.latest_effects.get(&3),
        Some(NetworkInbound::Packet(Packet::PlayerEffect { sequence: 10, .. }))
    ));
    assert!(matches!(
        staging.latest_entities.get(&(0, 99)),
        Some(NetworkInbound::Packet(Packet::EntityState { sequence: 5, .. }))
    ));
    assert!(matches!(
        staging.latest_time_sync,
        Some(NetworkInbound::Packet(Packet::TimeSync { ticks: 50, .. }))
    ));
}

#[test]
fn network_event_and_byte_caps_are_explicit_and_measurable() {
    let event = NetworkInbound::StatusUpdate { message: "bounded".into() };
    assert!(event.estimated_bytes() > 0);
    assert!(NETWORK_MAX_EVENTS_PER_PASS <= 256);
    assert!(NETWORK_MAX_BYTES_PER_PASS >= event.estimated_bytes());
    assert!(NETWORK_MAX_TIME_PER_PASS > Duration::ZERO);
    let small = NetworkInbound::StatusUpdate { message: "x".into() }.estimated_bytes();
    let large = NetworkInbound::StatusUpdate { message: "x".repeat(4096) }.estimated_bytes();
    assert!(large >= small + 4095);
}

#[test]
fn remote_block_entity_delta_applies_and_respects_monotonic_revisions() {
    use crate::block_entity::{BlockEntity, ChestBlockEntity};
    use crate::world::{BlockType, Chunk};

    let mut chunk = Chunk::new(0, 0);
    chunk.set_block_local(4, 10, 4, BlockType::Chest);

    let chest_stub = BlockEntity::Chest(ChestBlockEntity {
        inventory: crate::inventory::ContainerInventory::new(),
        custom_name: Some("Host Chest".to_string()),
        loot_table: None,
        loot_seed: None,
        revision: 0,
    });

    // Insert at revision 5
    let req1 = crate::network::protocol::Packet::BlockEntityDelta {
        dimension: 0,
        revision: 5,
        x: 4,
        y: 10,
        z: 4,
        entity: Some(chest_stub.clone()),
    };

    // Out of order/stale packet at revision 3
    let req_stale = crate::network::protocol::Packet::BlockEntityDelta {
        dimension: 0,
        revision: 3,
        x: 4,
        y: 10,
        z: 4,
        entity: None,
    };

    let mut client_chunk_revisions = std::collections::HashMap::new();

    // Apply revision 5
    if let crate::network::protocol::Packet::BlockEntityDelta {
        revision,
        x,
        y,
        z,
        entity,
        ..
    } = req1
    {
        let key = (crate::dimension::Dimension::Overworld, 0, 0);
        if revision > *client_chunk_revisions.get(&key).unwrap_or(&0) {
            client_chunk_revisions.insert(key, revision);
            if let Some(ent) = entity {
                chunk
                    .insert_block_entity(x as u8, y as i16, z as u8, ent)
                    .unwrap();
            }
        }
    }
    assert_eq!(chunk.get_block_entity(4, 10, 4), Some(&chest_stub));

    // Attempt revision 3 (should be ignored due to monotonic revision)
    if let crate::network::protocol::Packet::BlockEntityDelta {
        revision,
        x,
        y,
        z,
        entity,
        ..
    } = req_stale
    {
        let key = (crate::dimension::Dimension::Overworld, 0, 0);
        if revision > *client_chunk_revisions.get(&key).unwrap_or(&0) {
            client_chunk_revisions.insert(key, revision);
            if let Some(ent) = entity {
                chunk
                    .insert_block_entity(x as u8, y as i16, z as u8, ent)
                    .unwrap();
            } else {
                chunk.remove_block_entity(x as u8, y as i16, z as u8);
            }
        }
    }
    // Chest entity should still remain because revision 3 was rejected!
    assert_eq!(chunk.get_block_entity(4, 10, 4), Some(&chest_stub));
}

#[test]
fn batched_pose_arrivals_keep_sender_cadence() {
    let mut remote = RemotePlayerState::new(1, "Alex".into());
    for (sequence, sender_time_millis, x) in [(1, 1_000, 0.0), (2, 1_050, 1.0), (3, 1_100, 2.0)]
    {
        assert_ne!(
            remote.push_snapshot(
                Vec3::new(x, 0.0, 0.0),
                0.0,
                0.0,
                sequence,
                sender_time_millis,
                2.0,
            ),
            SnapshotPushResult::Rejected
        );
    }

    let times: Vec<_> = remote
        .snapshots
        .iter()
        .map(|snapshot| snapshot.time)
        .collect();
    for (actual, expected) in times.iter().zip([2.0, 2.05, 2.1]) {
        assert!((actual - expected).abs() < 1e-9);
    }
    let midpoint = remote.sample(2.075).unwrap();
    assert!((midpoint.position.x - 1.5).abs() < 1e-5);
}

#[test]
fn buffered_twenty_hz_motion_samples_smoothly_at_high_frame_rate() {
    let mut remote = RemotePlayerState::new(1, "Alex".into());
    for index in 0..=10 {
        let sender_time_millis = 1_000 + index * 50;
        let arrival_jitter = match index % 4 {
            0 => 0.008,
            1 => 0.001,
            2 => 0.012,
            _ => 0.004,
        };
        remote.push_snapshot(
            Vec3::new(index as f32 * 0.25, 0.0, 0.0),
            0.0,
            0.0,
            index as u32 + 1,
            sender_time_millis,
            2.0 + index as f64 * 0.05 + arrival_jitter,
        );
    }

    let mut previous_x = f32::NEG_INFINITY;
    for frame in 0..=72 {
        let target = 2.008 + frame as f64 / 144.0;
        let sample = remote.sample(target).unwrap();
        assert!(
            sample.position.x + 1e-5 >= previous_x,
            "sampled motion moved backwards at frame {frame}"
        );
        assert!(
            sample.position.x - previous_x <= 0.06 || !previous_x.is_finite(),
            "sampled motion jumped at frame {frame}"
        );
        previous_x = sample.position.x;
    }
}

#[test]
fn snapshots_reject_invalid_duplicate_and_out_of_order_data() {
    let mut remote = RemotePlayerState::new(1, "Alex".into());
    assert_eq!(
        remote.push_snapshot(Vec3::ZERO, 0.0, 0.0, 10, 1_000, 1.0),
        SnapshotPushResult::Snapped
    );
    assert_eq!(
        remote.push_snapshot(Vec3::X, 0.0, 0.0, 10, 1_050, 1.05),
        SnapshotPushResult::Rejected
    );
    assert_eq!(
        remote.push_snapshot(Vec3::X, 0.0, 0.0, 9, 1_050, 1.05),
        SnapshotPushResult::Rejected
    );
    assert_eq!(
        remote.push_snapshot(Vec3::new(f32::NAN, 0.0, 0.0), 0.0, 0.0, 11, 1_050, 1.05,),
        SnapshotPushResult::Rejected
    );
    assert_eq!(remote.snapshots.len(), 1);
}

#[test]
fn extrapolation_is_speed_limited_and_stops_after_one_hundred_ms() {
    let mut remote = RemotePlayerState::new(1, "Alex".into());
    remote.push_snapshot(Vec3::ZERO, 0.0, 0.0, 1, 1_000, 1.0);
    remote.push_snapshot(Vec3::new(2.5, 0.0, 0.0), 0.0, 0.0, 2, 1_050, 1.05);

    let at_limit = remote.sample(1.15).unwrap();
    let long_after = remote.sample(5.0).unwrap();
    assert!((at_limit.position.x - 6.5).abs() < 1e-4);
    assert_eq!(long_after.position, at_limit.position);
}

#[test]
fn teleport_or_long_gap_clears_history_and_snaps() {
    let mut remote = RemotePlayerState::new(1, "Alex".into());
    remote.push_snapshot(Vec3::ZERO, 0.0, 0.0, 1, 1_000, 1.0);
    assert_eq!(
        remote.push_snapshot(Vec3::new(20.0, 0.0, 0.0), 0.0, 0.0, 2, 1_050, 1.05),
        SnapshotPushResult::Snapped
    );
    assert_eq!(remote.snapshots.len(), 1);
    assert_eq!(remote.sample(0.0).unwrap().position.x, 20.0);

    assert_eq!(
        remote.push_snapshot(Vec3::new(21.0, 0.0, 0.0), 0.0, 0.0, 3, 2_000, 2.0),
        SnapshotPushResult::Snapped
    );
    assert_eq!(remote.snapshots.len(), 1);
}

#[test]
fn placement_uses_latest_authoritative_snapshot_before_side_effects() {
    let mut remote = RemotePlayerState::new(1, "Alex".into());
    remote.push_snapshot(Vec3::new(2.0, 0.0, 0.5), 0.0, 0.0, 1, 1_000, 1.0);
    remote.push_snapshot(Vec3::new(0.5, 0.0, 0.5), 0.0, 0.0, 2, 1_050, 1.05);

    // A delayed render sample is still outside the candidate block, while
    // the authoritative back of the snapshot queue is inside it.
    assert_eq!(
        remote.sample(1.0).unwrap().position,
        Vec3::new(2.0, 0.0, 0.5)
    );
    assert_eq!(
        remote.snapshots.back().unwrap().position,
        Vec3::new(0.5, 0.0, 0.5)
    );

    let decision = placement_decision_for_players(
        BlockType::Stone,
        (0, 0, 0),
        player_aabb_at(Vec3::new(10.0, 0.0, 10.0)),
        [&remote],
    );
    assert_eq!(decision, BlockPlacementDecision::BlockedByPlayer);

    // This mirrors the early-return guard used by both local placement and
    // the host request handler. A rejected decision must gate every effect.
    let mut effects = Vec::new();
    if decision == BlockPlacementDecision::Allowed {
        effects.extend([
            "world mutation",
            "action",
            "sound",
            "inventory",
            "broadcast",
        ]);
    }
    assert!(effects.is_empty());
}

#[test]
fn unknown_remote_pose_blocks_only_solid_placement() {
    let remote = RemotePlayerState::new(1, "Alex".into());
    let local = player_aabb_at(Vec3::new(10.0, 0.0, 10.0));

    assert_eq!(
        placement_decision_for_players(BlockType::Stone, (0, 0, 0), local, [&remote]),
        BlockPlacementDecision::BlockedByPlayer
    );
    assert_eq!(
        placement_decision_for_players(BlockType::Torch, (0, 0, 0), local, [&remote]),
        BlockPlacementDecision::Allowed
    );
}

#[test]
fn remote_block_change_updates_light_and_boundary_mesh_dependencies() {
    let mut manager = PresentationChunks::new(2);
    manager.chunks.insert((0, 0), Chunk::new(0, 0));
    manager.chunks.insert((1, 0), Chunk::new(1, 0));
    manager.set_sky_light(15, 80, 8, 15);

    let dirty = apply_synced_block_change(&mut manager, 15, 80, 8, BlockType::Stone, 0, 0)
        .expect("loaded block should change");

    assert_eq!(manager.get_block(15, 80, 8), BlockType::Stone);
    assert_eq!(manager.get_sky_light(15, 80, 8), 0);
    assert!(dirty.contains(&(0, 0)));
    assert!(dirty.contains(&(1, 0)));
    // PresentationChunks has no fluid queues — type-level proof that
    // projection apply cannot enqueue authority fluid neighbors.
}

#[test]
fn terrain_worker_tokens_reject_stale_generation_lifetime_and_revision() {
    use crate::dimension::Dimension;

    assert!(chunk_load_result_is_current(
        Some(7),
        7,
        3,
        3,
        Dimension::Overworld,
        Dimension::Overworld,
    ));
    assert!(!chunk_load_result_is_current(
        Some(8),
        7,
        3,
        3,
        Dimension::Overworld,
        Dimension::Overworld,
    ));
    assert!(!chunk_load_result_is_current(
        Some(7),
        7,
        2,
        3,
        Dimension::Overworld,
        Dimension::Overworld,
    ));
    assert!(!chunk_load_result_is_current(
        Some(7),
        7,
        3,
        3,
        Dimension::Nether,
        Dimension::Overworld,
    ));

    let key = SectionKey::new(1, 2, 3);
    let current = SectionIdentity::new(key, 11, 7);
    assert!(section_mesh_result_is_current(
        Some(current),
        current,
        3,
        3,
        Some(current),
    ));
    assert!(!section_mesh_result_is_current(
        Some(SectionIdentity::new(key, 10, 7)),
        current,
        3,
        3,
        Some(current),
    ));
    assert!(!section_mesh_result_is_current(
        Some(current),
        current,
        3,
        3,
        Some(SectionIdentity::new(key, 12, 7)),
    ));
    assert!(!section_mesh_result_is_current(
        Some(current),
        current,
        2,
        3,
        Some(current),
    ));
}

#[test]
fn mesh_invalidation_queues_latest_revision_and_invalidates_connectivity() {
    let coord = (2, -3);
    let mut meshes = std::collections::HashMap::from([(coord, ChunkMesh::pending())]);
    let key = SectionKey::new(coord.0, 5, coord.1);
    let section = meshes
        .get_mut(&coord)
        .unwrap()
        .section_mut(key.section_y)
        .unwrap();
    section.invalidate();
    let first_revision = section.revision;
    let mut scheduler = crate::chunk_schedule::SectionMeshScheduler::new();
    scheduler.enqueue(
        SectionIdentity::new(key, first_revision, 7),
        DependencyReason::Block,
        (0, 0),
    );
    assert_eq!(
        section.connectivity,
        crate::culling::SectionConnectivityState::Invalid
    );
    section.invalidate();
    scheduler.enqueue(
        SectionIdentity::new(key, section.revision, 7),
        DependencyReason::Light,
        (0, 0),
    );
    assert_eq!(scheduler.len(), 1);
    let work = scheduler.pop_nearest((0, 0), 8).unwrap();
    assert_eq!(work.identity.revision, first_revision + 1);
    assert_eq!(work.reason, DependencyReason::Light);
    assert!(!section_mesh_result_is_current(
        Some(SectionIdentity::new(key, first_revision, 7)),
        SectionIdentity::new(key, first_revision, 7),
        1,
        1,
        Some(work.identity),
    ));
}

#[test]
fn mutation_scheduler_worker_chain_commits_only_the_latest_visible_revision() {
    let coord = (0, 0);
    let lifetime = 9;
    let generation = 4;
    let mut meshes = std::collections::HashMap::from([(coord, ChunkMesh::pending())]);
    let key = SectionKey::new(0, 4, 0);
    let mut scheduler = crate::chunk_schedule::SectionMeshScheduler::new();
    let section = meshes.get_mut(&coord).unwrap().section_mut(4).unwrap();
    section.invalidate();
    scheduler.enqueue(
        SectionIdentity::new(key, section.revision, lifetime),
        DependencyReason::BreakPlace,
        coord,
    );
    let stale_work = scheduler.pop_nearest(coord, 1).unwrap();
    scheduler.mark_in_flight(stale_work);

    section.invalidate();
    let current = SectionIdentity::new(key, section.revision, lifetime);
    scheduler.enqueue(current, DependencyReason::Fluid, coord);
    assert!(!section_mesh_result_is_current(
        Some(stale_work.identity),
        stale_work.identity,
        generation,
        generation,
        Some(current),
    ));

    scheduler.complete(stale_work.identity);
    let latest_work = scheduler.pop_nearest(coord, 1).unwrap();
    assert!(section_mesh_result_is_current(
        Some(latest_work.identity),
        latest_work.identity,
        generation,
        generation,
        Some(current),
    ));
    let section = meshes.get_mut(&coord).unwrap().section_mut(4).unwrap();
    section.connectivity = crate::culling::SectionConnectivityState::Valid(
        crate::culling::SectionConnectivity::FULL,
    );
    section.meshed_revision = latest_work.identity.revision;
    assert_eq!(section.meshed_revision, section.revision);
}

#[test]
fn boundary_and_diagonal_ao_dependencies_queue_once() {
    let coords = [(0, 0), (1, 0), (0, 1), (1, 1)];
    let mut meshes = coords
        .into_iter()
        .map(|coord| (coord, ChunkMesh::pending()))
        .collect::<std::collections::HashMap<_, _>>();
    let mut scheduler = crate::chunk_schedule::SectionMeshScheduler::new();
    let mut dependencies = std::collections::HashSet::new();
    mark_section_mesh_dependencies(&mut dependencies, 15, 15, 15);

    for key in dependencies {
        let reason = if key == SectionKey::new(0, 0, 0) {
            DependencyReason::BreakPlace
        } else {
            DependencyReason::Ao
        };
        let section = meshes
            .get_mut(&(key.cx, key.cz))
            .unwrap()
            .section_mut(key.section_y)
            .unwrap();
        section.invalidate();
        scheduler.enqueue(
            SectionIdentity::new(key, section.revision, 1),
            reason,
            (0, 0),
        );
    }

    assert_eq!(scheduler.len(), 8);
    let mut reasons = std::collections::HashMap::new();
    while let Some(work) = scheduler.pop_nearest((0, 0), 2) {
        reasons.insert(work.identity.key, work.reason);
    }
    assert_eq!(
        reasons[&SectionKey::new(0, 0, 0)],
        DependencyReason::BreakPlace
    );
    assert!(reasons
        .iter()
        .filter(|(key, _)| **key != SectionKey::new(0, 0, 0))
        .all(|(_, reason)| *reason == DependencyReason::Ao));
}

#[test]
fn runtime_mesh_mutations_cannot_bypass_the_invalidation_api() {
    let forbidden = concat!("mesh.", "mark_", "dirty()");
    for (path, source) in [
        ("state.rs", include_str!("../../state.rs")),
        ("mob.rs", include_str!("../../mob.rs")),
        ("passive_mob.rs", include_str!("../../passive_mob.rs")),
    ] {
        assert!(
            !source.contains(forbidden),
            "{path} bypasses invalidate_chunk_mesh"
        );
    }
}

#[test]
fn mesh_snapshot_owns_the_neighbor_halo() {
    let mut chunks = std::collections::HashMap::new();
    let mut center = Chunk::new(0, 0);
    let mut east = Chunk::new(1, 0);
    center.set_block_local(15, 10, 8, BlockType::Stone);
    east.set_block_local(0, 10, 8, BlockType::Dirt);
    east.set_sky_light(0, 10, 8, 9);
    chunks.insert((0, 0), center);
    chunks.insert((1, 0), east);

    let snapshot = MeshSnapshot::capture((0, 0), &chunks, 15).expect("center chunk exists");
    assert_eq!(snapshot.get(15, 10, 8).0, BlockType::Stone);
    assert_eq!(snapshot.get(16, 10, 8), (BlockType::Dirt, 9, 0, 0, false));
    assert_eq!(snapshot.get(-1, 10, 8), (BlockType::Air, 15, 0, 0, false));
}

#[test]
fn terrain_shader_module_passes_wgpu_validation() {
    let instance = wgpu::Instance::default();
    let Some(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
    else {
        // Headless CI images are allowed to have no graphics adapter.
        return;
    };
    let Ok((device, _queue)) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("Terrain shader validation device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
        },
        None,
    )) else {
        return;
    };

    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let _shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Terrain shader validation"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../shader.wgsl").into()),
    });
    let validation_error = pollster::block_on(device.pop_error_scope());
    assert!(
        validation_error.is_none(),
        "terrain WGSL failed validation: {validation_error:?}"
    );
}
