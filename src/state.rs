use crate::camera::{Camera, CameraUniform};
use crate::chunk_manager::{
    mark_block_mesh_dependencies, mark_section_mesh_dependencies, surrounding_chunk_coords,
    ChunkManager,
};
use crate::chunk_render::{
    select_lod_for_bounds, DrawCandidate, DrawLayer, Frustum, LodLevel, LodThresholds, MeshBounds,
    TerrainVertex,
}; // LodLevel / MeshBounds used by #[cfg(test)] helpers and frame.rs via super
use crate::chunk_schedule::DependencyReason;
use crate::game_rules::Difficulty;
use crate::interaction::{raycast, RaycastTargetPolicy};
use crate::inventory::{
    CreativeTab, GameMode, Inventory, Item, ItemStack, ToolType, CREATIVE_COLUMNS, CREATIVE_ROWS,
    CREATIVE_VISIBLE_SLOTS,
};
use crate::menu::{GameSettings, MenuRect, WorldLaunch};
use crate::physics::{player_aabb_at, BlockPlacementDecision, PlayerPhysics};
use crate::player::{DamageSource, PlayerState};
use crate::presentation::gpu_terrain::{
    chunk_mesh_is_registered_with_region, empty_region_rebuild_worthwhile,
    region_allocation_handle_is_live, should_decrement_region_active_chunks, UploadMetrics,
};
use crate::presentation::interpolation::{
    interpolate_snapshot, placement_decision_for_players, sequence_is_newer,
    validated_remote_position, PlayerSnapshot, RemotePlayerState, ReplicatedEntityState,
    SnapshotPushResult, ENTITY_INTERPOLATION_DELAY, PLAYER_CORRECTION_SNAP_DISTANCE,
    REMOTE_INTERPOLATION_DELAY,
};
use crate::presentation::network_inbound::{NetworkInbound, NetworkStaging, TrackedNetworkSender};
use crate::presentation_click::{
    collect_inventory_ui_hits, resolve_world_click, InventoryHit, InventoryHitProbe, WorldClickHit,
    WorldClickIntent,
};
use crate::presentation_inventory_policy::MultiplayerRole;
use crate::presentation_inventory_policy::{
    PresentationInventoryAction, PresentationInventoryTarget, PresentationTopology,
};
use crate::recipes::RecipeManager;
use crate::world::{
    Biome, BlockType, Chunk, SectionIdentity, SectionKey, CHUNK_DEPTH, CHUNK_WIDTH,
};
use glam::{Mat4, Vec2, Vec3};
use std::sync::Arc;
use std::time::{Duration, Instant};
use wgpu::util::DeviceExt;
use winit::window::Window;

pub use crate::authority::mining::calculate_block_break_rewards;
pub use crate::presentation::gpu_terrain::{
    ChunkMesh, GpuMeshLayer, GpuMeshLevel, GpuSectionMesh, RenderRegion,
};
pub use crate::presentation::network_inbound::NetworkHandle;

#[path = "presentation/embedded_runtime.rs"]
mod embedded_runtime;
#[path = "presentation/frame.rs"]
mod frame;
#[path = "presentation/network_event.rs"]
mod network_event;

use embedded_runtime::EmbeddedRuntimeBridge;

const UI_VERTEX_CAPACITY: usize = 4096;
const UI_LINE_VERTEX_CAPACITY: usize = 16384;
const DEBUG_STATS_INTERVAL: f32 = 0.5;
const RAIN_LOOP_ID: u64 = u64::MAX - 1;
const CHAT_HISTORY_CAPACITY: usize = 50;
const CHAT_VISIBLE_LINES: usize = 8;
const CHAT_INPUT_CAPACITY: usize = 256;

const CREATIVE_FLIGHT_DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(300);
const MELEE_REACH: f32 = 4.0;
const NETWORK_MAX_EVENTS_PER_PASS: usize = 256;
const NETWORK_MAX_BYTES_PER_PASS: usize = 1_048_576;
const NETWORK_MAX_TIME_PER_PASS: Duration = Duration::from_millis(2);
const GPU_TIMESTAMP_READBACK_SLOT_COUNT: usize = 2;
const GPU_TIMESTAMP_QUERY_COUNT: u32 = 14;
const GPU_TIMESTAMP_READBACK_BYTES: u64 = GPU_TIMESTAMP_QUERY_COUNT as u64 * 8;
const SECTION_STORAGE_COMPACTIONS_PER_FRAME: usize = 4;
const PAUSE_WEATHER_VOLUME_BOUNDS: [f32; 4] = [-0.3, 0.3, -0.46, -0.36];
const PAUSE_QUIT_BOUNDS: [f32; 4] = [-0.3, 0.3, -0.60, -0.50];
pub const SIM_TICK_TIME: f32 = 0.05;
pub const MAX_CATCHUP_TICKS: usize = 4;

fn should_advance_simulation(
    role: &MultiplayerRole,
    network_ready: bool,
    is_paused: bool,
    is_dead: bool,
) -> bool {
    network_ready
        && match role {
            MultiplayerRole::Host { .. } => true,
            MultiplayerRole::Singleplayer | MultiplayerRole::Client { .. } => {
                !is_paused && !is_dead
            }
        }
}

fn point_in_bounds(x: f32, y: f32, bounds: [f32; 4]) -> bool {
    x >= bounds[0] && x <= bounds[1] && y >= bounds[2] && y <= bounds[3]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PrimaryPressDecision {
    keep_held_mining: bool,
    instant_break: bool,
}

fn primary_press_decision(game_mode: GameMode, melee_consumed: bool) -> PrimaryPressDecision {
    if melee_consumed {
        PrimaryPressDecision {
            keep_held_mining: false,
            instant_break: false,
        }
    } else if game_mode == GameMode::Creative {
        PrimaryPressDecision {
            keep_held_mining: false,
            instant_break: true,
        }
    } else {
        PrimaryPressDecision {
            keep_held_mining: true,
            instant_break: false,
        }
    }
}

fn can_break_block(block: BlockType, game_mode: GameMode) -> bool {
    block != BlockType::Air
        && (crate::game_rules::GameModePolicy::for_mode(game_mode, true).can_break
            || game_mode == GameMode::Adventure)
        && (game_mode == GameMode::Creative || block.properties().hardness >= 0.0)
}

fn is_legal_melee_target(entity: &crate::entity::Entity) -> bool {
    entity.is_player_melee_target()
}

fn closest_melee_target(
    entity_manager: &crate::entity::EntityManager,
    origin: Vec3,
    direction: Vec3,
    reach: f32,
) -> Option<u64> {
    if direction.length_squared() <= f32::EPSILON {
        return None;
    }
    let direction = direction.normalize();
    const MELEE_TYPES: [crate::entity::EntityType; 20] = [
        crate::entity::EntityType::Zombie,
        crate::entity::EntityType::Skeleton,
        crate::entity::EntityType::Creeper,
        crate::entity::EntityType::Pig,
        crate::entity::EntityType::Cow,
        crate::entity::EntityType::Sheep,
        crate::entity::EntityType::Chicken,
        crate::entity::EntityType::Blaze,
        crate::entity::EntityType::Piglin,
        crate::entity::EntityType::Husk,
        crate::entity::EntityType::Shulker,
        crate::entity::EntityType::EnderDragon,
        crate::entity::EntityType::Wither,
        crate::entity::EntityType::EndCrystal,
        crate::entity::EntityType::Enderman,
        crate::entity::EntityType::Villager,
        crate::entity::EntityType::IronGolem,
        crate::entity::EntityType::Pillager,
        crate::entity::EntityType::Ravager,
        crate::entity::EntityType::RemotePlayer,
    ];
    entity_manager
        .query_radius_types(origin, reach, &MELEE_TYPES)
        .filter(|entity| is_legal_melee_target(entity))
        .filter_map(|entity| {
            crate::entity::ray_intersects_aabb(origin, direction, &entity.get_aabb())
                .filter(|distance| distance.is_finite() && *distance <= reach.max(0.0))
                .map(|distance| (entity.id, distance))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(id, _)| id)
}

/// Apply a network-visible block value to CPU world state and return every
/// chunk whose mesh/light data depends on it. Redstone and gameplay side
/// effects remain the caller's responsibility.
fn apply_synced_block_change(
    chunk_manager: &mut ChunkManager,
    x: i32,
    y: i32,
    z: i32,
    block: BlockType,
    state: u8,
) -> Option<std::collections::HashSet<(i32, i32)>> {
    let ((cx, cz), _) = chunk_manager.world_to_local(x, y, z)?;
    if !chunk_manager.chunks.contains_key(&(cx, cz)) {
        return None;
    }
    let previous = chunk_manager.get_block(x, y, z);
    let previous_state = chunk_manager.get_block_state(x, y, z);
    if previous == block && previous_state == state {
        return None;
    }

    chunk_manager.set_block(x, y, z, block);
    chunk_manager.set_block_state(x, y, z, state);
    let old_properties = previous.properties();
    let new_properties = block.properties();
    let mut dirty_chunks = std::collections::HashSet::new();
    if old_properties.is_opaque() != new_properties.is_opaque() {
        if new_properties.is_opaque() {
            crate::lighting::update_sky_light_after_placed(
                chunk_manager,
                x,
                y,
                z,
                &mut dirty_chunks,
            );
        } else {
            crate::lighting::update_sky_light_after_removed(
                chunk_manager,
                x,
                y,
                z,
                &mut dirty_chunks,
            );
        }
    }
    if old_properties.light_emission != new_properties.light_emission {
        crate::lighting::update_block_light_after_removed(
            chunk_manager,
            x,
            y,
            z,
            old_properties.light_emission,
            &mut dirty_chunks,
        );
        if new_properties.light_emission > 0 {
            crate::lighting::update_block_light_after_placed(
                chunk_manager,
                x,
                y,
                z,
                new_properties.light_emission,
                &mut dirty_chunks,
            );
        }
    }
    mark_block_mesh_dependencies(&mut dirty_chunks, x, z);
    Some(dirty_chunks)
}

#[cfg(test)]
mod remote_sync_tests {
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
            staging.stage(NetworkInbound::StatusUpdate("burst".into()));
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
            NetworkInbound::StatusUpdate("one".into()),
            NetworkInbound::StatusUpdate("two".into()),
            NetworkInbound::StatusUpdate("three".into()),
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
            if let NetworkInbound::StatusUpdate(message) = event {
                delivered.push(message);
            }
        }
        assert_eq!(delivered, ["one", "two", "three"]);
    }

    #[test]
    fn latest_wins_state_is_sequence_aware_per_key() {
        let mut staging = NetworkStaging::default();
        for (id, sequence, x) in [(7_u64, 2_u32, 2.0_f32), (7, 1, 1.0), (8, 4, 4.0)] {
            staging.stage(NetworkInbound::PlayerPosition {
                id,
                sequence,
                sender_time_millis: sequence as u64,
                x,
                y: 0.0,
                z: 0.0,
                yaw: 0.0,
                pitch: 0.0,
            });
        }
        for sequence in [9, 8, 10] {
            staging.stage(NetworkInbound::PlayerHealth {
                sequence,
                player_id: 3,
                health: sequence as f32,
                max_health: 20.0,
                hunger: 19.0,
                saturation: 4.0,
                oxygen: 20.0,
                is_dead: false,
                death_reason: 0,
            });
            staging.stage(NetworkInbound::PlayerEffect {
                sequence,
                player_id: 3,
                effects: Vec::new(),
            });
        }
        for ticks in [40, 30, 50] {
            staging.stage(NetworkInbound::TimeSync {
                ticks,
                weather: 0,
                weather_remaining_ticks: 0.0,
            });
        }
        for sequence in [4, 3, 5] {
            staging.stage(NetworkInbound::EntityState {
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
            });
        }

        assert_eq!(staging.latest_positions.len(), 2);
        assert!(matches!(
            staging.latest_positions.get(&7),
            Some(NetworkInbound::PlayerPosition { sequence: 2, .. })
        ));
        assert!(matches!(
            staging.latest_health.get(&3),
            Some(NetworkInbound::PlayerHealth { sequence: 10, .. })
        ));
        assert!(matches!(
            staging.latest_effects.get(&3),
            Some(NetworkInbound::PlayerEffect { sequence: 10, .. })
        ));
        assert!(matches!(
            staging.latest_entities.get(&(0, 99)),
            Some(NetworkInbound::EntityState { sequence: 5, .. })
        ));
        assert!(matches!(
            staging.latest_time_sync,
            Some(NetworkInbound::TimeSync { ticks: 50, .. })
        ));
    }

    #[test]
    fn network_event_and_byte_caps_are_explicit_and_measurable() {
        let event = NetworkInbound::StatusUpdate("bounded".into());
        assert!(event.estimated_bytes() > 0);
        assert!(NETWORK_MAX_EVENTS_PER_PASS <= 256);
        assert!(NETWORK_MAX_BYTES_PER_PASS >= event.estimated_bytes());
        assert!(NETWORK_MAX_TIME_PER_PASS > Duration::ZERO);
        let small = NetworkInbound::StatusUpdate("x".into()).estimated_bytes();
        let large = NetworkInbound::StatusUpdate("x".repeat(4096)).estimated_bytes();
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
            protocol_version: crate::network::protocol::PROTOCOL_VERSION,
            dimension: 0,
            revision: 5,
            x: 4,
            y: 10,
            z: 4,
            entity: Some(chest_stub.clone()),
        };

        // Out of order/stale packet at revision 3
        let req_stale = crate::network::protocol::Packet::BlockEntityDelta {
            protocol_version: crate::network::protocol::PROTOCOL_VERSION,
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
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        manager.chunks.insert((1, 0), Chunk::new(1, 0));
        manager.set_sky_light(15, 80, 8, 15);

        let dirty = apply_synced_block_change(&mut manager, 15, 80, 8, BlockType::Stone, 0)
            .expect("loaded block should change");

        assert_eq!(manager.get_block(15, 80, 8), BlockType::Stone);
        assert_eq!(manager.get_sky_light(15, 80, 8), 0);
        assert!(dirty.contains(&(0, 0)));
        assert!(dirty.contains(&(1, 0)));
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
            ("state.rs", include_str!("state.rs")),
            ("mob.rs", include_str!("mob.rs")),
            ("passive_mob.rs", include_str!("passive_mob.rs")),
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
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let validation_error = pollster::block_on(device.pop_error_scope());
        assert!(
            validation_error.is_none(),
            "terrain WGSL failed validation: {validation_error:?}"
        );
    }
}

const MAX_CHUNK_LOAD_JOBS: usize = 2;
const MAX_CHUNK_MESH_JOBS: usize = 4;

#[cfg(test)]
#[derive(Clone, Copy)]
struct MeshVoxel {
    block: BlockType,
    sky_light: u8,
    block_light: u8,
    fluid: u8,
}

#[cfg(test)]
struct MeshSnapshot {
    min_world_x: i32,
    min_world_y: i32,
    min_world_z: i32,
    y_count: usize,
    voxels: Vec<MeshVoxel>,
    default_sky_light: u8,
}

#[cfg(test)]
impl MeshSnapshot {
    const WIDTH: usize = CHUNK_WIDTH + 2;
    const DEPTH: usize = CHUNK_DEPTH + 2;

    fn capture(
        coord: (i32, i32),
        chunks: &std::collections::HashMap<(i32, i32), Chunk>,
        default_sky_light: u8,
    ) -> Option<Self> {
        let center = chunks.get(&coord)?;
        let min_world_x = coord.0 * CHUNK_WIDTH as i32 - 1;
        let min_world_y = center.min_world_y();
        let min_world_z = coord.1 * CHUNK_DEPTH as i32 - 1;
        let y_count = (center.max_world_y_exclusive() - min_world_y) as usize;
        let mut voxels = Vec::with_capacity(Self::WIDTH * y_count * Self::DEPTH);
        for x in 0..Self::WIDTH {
            let world_x = min_world_x + x as i32;
            let chunk_x = world_x.div_euclid(CHUNK_WIDTH as i32);
            let local_x = world_x.rem_euclid(CHUNK_WIDTH as i32) as usize;
            for y in 0..y_count {
                let world_y = min_world_y + y as i32;
                for z in 0..Self::DEPTH {
                    let world_z = min_world_z + z as i32;
                    let chunk_z = world_z.div_euclid(CHUNK_DEPTH as i32);
                    let local_z = world_z.rem_euclid(CHUNK_DEPTH as i32) as usize;
                    let voxel = chunks
                        .get(&(chunk_x, chunk_z))
                        .map(|neighbor| MeshVoxel {
                            block: neighbor.get_block_local(local_x, world_y, local_z),
                            sky_light: neighbor.get_sky_light(local_x, world_y, local_z),
                            block_light: neighbor.get_block_light(local_x, world_y, local_z),
                            fluid: neighbor.get_fluid_level(local_x, world_y, local_z),
                        })
                        .unwrap_or(MeshVoxel {
                            block: BlockType::Air,
                            sky_light: default_sky_light,
                            block_light: 0,
                            fluid: 0,
                        });
                    voxels.push(voxel);
                }
            }
        }
        Some(Self {
            min_world_x,
            min_world_y,
            min_world_z,
            y_count,
            voxels,
            default_sky_light,
        })
    }

    fn get(&self, world_x: i32, world_y: i32, world_z: i32) -> (BlockType, u8, u8, u8, bool) {
        if world_y < self.min_world_y {
            return (BlockType::Air, 0, 0, 0, false);
        }
        if world_y >= self.min_world_y + self.y_count as i32 {
            return (BlockType::Air, self.default_sky_light, 0, 0, false);
        }
        let x = world_x - self.min_world_x;
        let z = world_z - self.min_world_z;
        if x < 0 || x >= Self::WIDTH as i32 || z < 0 || z >= Self::DEPTH as i32 {
            return (BlockType::Air, self.default_sky_light, 0, 0, false);
        }
        let local_y = (world_y - self.min_world_y) as usize;
        let index = (x as usize * self.y_count + local_y) * Self::DEPTH + z as usize;
        let voxel = self.voxels[index];
        (
            voxel.block,
            voxel.sky_light,
            voxel.block_light,
            voxel.fluid & 0x07,
            voxel.fluid & 0x08 != 0,
        )
    }
}

struct ChunkLoadResult {
    coord: (i32, i32),
    dimension: crate::dimension::Dimension,
    generation: u64,
    lifetime: u64,
    chunk: Chunk,
    restore_failed: bool,
}

struct SectionMeshResult {
    generation: u64,
    bundle: crate::chunk_render::SectionMeshBundle,
}

fn chunk_load_result_is_current(
    expected_lifetime: Option<u64>,
    result_lifetime: u64,
    result_generation: u64,
    current_generation: u64,
    result_dimension: crate::dimension::Dimension,
    current_dimension: crate::dimension::Dimension,
) -> bool {
    expected_lifetime == Some(result_lifetime)
        && result_generation == current_generation
        && result_dimension == current_dimension
}

fn section_mesh_result_is_current(
    expected_job: Option<SectionIdentity>,
    result_identity: SectionIdentity,
    result_generation: u64,
    current_generation: u64,
    current_identity: Option<SectionIdentity>,
) -> bool {
    expected_job == Some(result_identity)
        && result_generation == current_generation
        && current_identity == Some(result_identity)
}

/// Container entity revisions use serial-number arithmetic so a wrapped host
/// revision remains newer than the last value while duplicate and stale
/// packets are rejected.  This is the same half-range rule used for network
/// sequence numbers elsewhere in the state machine.
fn container_revision_is_newer(current: u64, candidate: u64) -> bool {
    candidate != current && candidate.wrapping_sub(current) < (1_u64 << 63)
}

enum TerrainWorkerResult {
    Loaded(ChunkLoadResult),
    SectionMeshed(SectionMeshResult),
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub tex_coords: [f32; 2],
    pub light_level: f32,
    pub ao: f32,
}

impl Vertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: (std::mem::size_of::<[f32; 3]>() + std::mem::size_of::<[f32; 2]>())
                        as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32,
                },
                wgpu::VertexAttribute {
                    offset: (std::mem::size_of::<[f32; 3]>()
                        + std::mem::size_of::<[f32; 2]>()
                        + std::mem::size_of::<f32>())
                        as wgpu::BufferAddress,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        }
    }
}

impl State {
    /// Drop all terrain GPU/CPU runtime state as one lifecycle boundary.
    /// Incrementing the generation invalidates every in-flight worker result.
    fn teardown_terrain_runtime(&mut self, reason: &str) {
        self.terrain_generation = self.terrain_generation.wrapping_add(1);
        self.chunk_load_in_flight.clear();
        self.section_scheduler.clear();
        self.chunk_lifetimes.clear();
        self.chunk_meshes.clear();
        self.render_regions.clear();
        self.compaction_pending_region = None;
        self.section_storage_compaction_queue.clear();
        self.section_storage_compaction_queued.clear();
        self.scheduler.clear();
        self.pending_worker_results.clear();
        eprintln!("[Terrain] runtime teardown: {reason}");
    }

    fn schedule_terrain_compaction(&mut self) {
        if self.compaction_pending_region.is_some() {
            return;
        }
        self.compaction_pending_region = self
            .render_regions
            .iter()
            .filter(|(_, region)| region.empty_rebuild_worthwhile())
            .map(|(coord, _)| *coord)
            .next();
    }

    /// Consume at most one staged terrain compaction per frame. A live arena
    /// cannot be compacted without rebasing every mesh handle and synchronizing
    /// the GPU copies, so the runtime deliberately rebuilds only arenas with no
    /// live allocations. This shrinks previously-grown empty buffers while
    /// preserving the resident-chunk count; all other candidates fail safe.
    fn process_terrain_compaction(&mut self) {
        let Some(coord) = self.compaction_pending_region.take() else {
            return;
        };
        let Some(region) = self.render_regions.get(&coord) else {
            return;
        };
        if !region.empty_rebuild_worthwhile() {
            return;
        }
        let active_chunks = region.active_chunks;
        let mut rebuilt = RenderRegion::new(&self.device, &self.region_bind_group_layout, coord);
        rebuilt.active_chunks = active_chunks;
        self.render_regions.insert(coord, rebuilt);
    }

    fn process_section_storage_compaction(&mut self) {
        for _ in 0..SECTION_STORAGE_COMPACTIONS_PER_FRAME {
            let Some(key) = self.section_storage_compaction_queue.pop_front() else {
                break;
            };
            self.section_storage_compaction_queued.remove(&key);
            let Some(chunk) = self.chunk_manager.chunks.get_mut(&(key.cx, key.cz)) else {
                continue;
            };
            let Some(sec_idx) = chunk.section_index(key.section_y) else {
                continue;
            };
            if let Some(ref mut section) = chunk.sections[sec_idx] {
                section.compact_if_worthwhile();
            }
        }
    }

    /// Reset only renderer/presentation caches after an authority-owned
    /// dimension transfer (portal, respawn, or session projection). No local
    /// worldgen, lighting, save, or gameplay mutation is allowed here;
    /// subsequent ChunkData/Entity events repopulate the destination.
    fn switch_dimension(&mut self, target: crate::dimension::Dimension) {
        self.reset_presented_dimension(target);
    }

    /// Reset only renderer/presentation caches after an authority-owned
    /// dimension transfer. No local world generation, save, or gameplay
    /// mutation is allowed on this path; subsequent ChunkData/Entity events
    /// repopulate the new dimension through the network projection lane.
    fn reset_presented_dimension(&mut self, target: crate::dimension::Dimension) {
        if target == self.current_dimension {
            return;
        }
        self.inventory.dragged = None;
        self.inventory.creative_drag_origin = None;
        self.inventory.is_open = false;
        self.inventory.is_table_open = false;
        self.container_target = None;
        self.container_is_double = false;
        self.active_station = None;
        self.sync_cursor_mode();

        clear_remote_players(&mut self.remote_players, &mut self.entity_manager);
        self.clear_replicated_entities();
        self.presented_fishing_hook_entity = None;
        self.current_dimension = target;
        let render_distance = self.chunk_manager.render_distance;
        self.teardown_terrain_runtime("authority dimension projection");
        self.chunk_manager = ChunkManager::new_in_dimension(render_distance, target);
        self.entity_manager = crate::entity::EntityManager::new();
        self.particles = crate::particles::ParticleSystem::new();
        self.pending_chunk_payloads.clear();
        self.pending_block_changes.clear();
        self.client_chunk_revisions.clear();
        self.mining_target = None;
        self.mining_progress = 0.0;
        self.left_mouse_pressed = false;
        self.audio_manager.stop_looping_sound(RAIN_LOOP_ID);
    }

    fn update_portal_travel(&mut self, dt: f32) {
        self.portal_cooldown = (self.portal_cooldown - dt).max(0.0);
        if self.portal_cooldown > 0.0 {
            self.portal_contact_time = 0.0;
            return;
        }
        let pos = self.player_physics.position;
        let x = pos.x.floor() as i32;
        let y = pos.y.floor() as i32;
        let z = pos.z.floor() as i32;
        let feet = self.chunk_manager.get_block(x, y, z);
        let body = self.chunk_manager.get_block(x, y + 1, z);
        let portal = if matches!(
            feet,
            BlockType::NetherPortal | BlockType::EndPortal | BlockType::EndGateway
        ) {
            Some(((x, y, z), feet))
        } else if matches!(
            body,
            BlockType::NetherPortal | BlockType::EndPortal | BlockType::EndGateway
        ) {
            Some(((x, y + 1, z), body))
        } else {
            None
        };
        if let Some(((portal_x, portal_y, portal_z), portal_block)) = portal {
            if self.portal_contact_time == 0.0 {
                let _ = self.submit_local_authority_block_action(
                    crate::network::protocol::BlockActionKind::EnterPortal,
                    portal_x,
                    portal_y,
                    portal_z,
                    [0, 0, 0],
                    portal_block,
                );
                self.portal_contact_time = dt.max(f32::EPSILON);
            }
        } else {
            self.portal_contact_time = 0.0;
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct UiVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

impl UiVertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<UiVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TexturedUiVertex {
    pub position: [f32; 3],
    pub tex_coords: [f32; 2],
    pub color: [f32; 4],
}

impl TexturedUiVertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TexturedUiVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: (std::mem::size_of::<[f32; 3]>() + std::mem::size_of::<[f32; 2]>())
                        as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

#[derive(Default)]
pub struct KeyState {
    pub w: bool,
    pub a: bool,
    pub s: bool,
    pub d: bool,
    pub space: bool,
    pub ctrl: bool,
    pub shift: bool,
}

pub(crate) fn allows_camera_look(
    is_paused: bool,
    inventory_open: bool,
    advancements_open: bool,
    chat_open: bool,
    connection_lost: bool,
    is_dead: bool,
    has_focus: bool,
) -> bool {
    !is_paused
        && !inventory_open
        && !advancements_open
        && !chat_open
        && !connection_lost
        && !is_dead
        && has_focus
}

fn allows_continuous_mining(
    left_mouse_pressed: bool,
    game_mode: GameMode,
    gameplay_input_allowed: bool,
) -> bool {
    left_mouse_pressed
        && matches!(game_mode, GameMode::Survival | GameMode::Adventure)
        && gameplay_input_allowed
}

fn cursor_position_to_ndc(x: f64, y: f64, width: u32, height: u32) -> [f32; 2] {
    [
        (x as f32 / width.max(1) as f32) * 2.0 - 1.0,
        1.0 - (y as f32 / height.max(1) as f32) * 2.0,
    ]
}

#[cfg(test)]
mod camera_input_tests {
    use super::{allows_camera_look, allows_continuous_mining, cursor_position_to_ndc, GameMode};

    #[test]
    fn every_gameplay_blocker_disables_camera_look() {
        assert!(allows_camera_look(
            false, false, false, false, false, false, true
        ));

        assert!(!allows_camera_look(
            true, false, false, false, false, false, true
        ));
        assert!(!allows_camera_look(
            false, true, false, false, false, false, true
        ));
        assert!(!allows_camera_look(
            false, false, true, false, false, false, true
        ));
        assert!(!allows_camera_look(
            false, false, false, true, false, false, true
        ));
        assert!(!allows_camera_look(
            false, false, false, false, true, false, true
        ));
        assert!(!allows_camera_look(
            false, false, false, false, false, true, true
        ));
        assert!(!allows_camera_look(
            false, false, false, false, false, false, false
        ));
    }

    #[test]
    fn cursor_position_still_maps_to_ui_coordinates() {
        assert_eq!(cursor_position_to_ndc(0.0, 0.0, 1280, 720), [-1.0, 1.0]);
        assert_eq!(cursor_position_to_ndc(640.0, 360.0, 1280, 720), [0.0, 0.0]);
        assert_eq!(
            cursor_position_to_ndc(1280.0, 720.0, 1280, 720),
            [1.0, -1.0]
        );
    }

    #[test]
    fn continuous_mining_requires_unblocked_survival_input() {
        assert!(allows_continuous_mining(true, GameMode::Survival, true));
        assert!(!allows_continuous_mining(false, GameMode::Survival, true));
        assert!(!allows_continuous_mining(true, GameMode::Creative, true));
        assert!(!allows_continuous_mining(true, GameMode::Survival, false));
    }
}

#[derive(Debug, Default)]
struct DoubleTapTracker {
    last_tap: Option<Instant>,
}

impl DoubleTapTracker {
    fn register(&mut self, now: Instant, enabled: bool, repeat: bool) -> bool {
        if !enabled {
            self.reset();
            return false;
        }
        if repeat {
            return false;
        }

        let is_double_tap = self
            .last_tap
            .and_then(|last| now.checked_duration_since(last))
            .is_some_and(|elapsed| elapsed <= CREATIVE_FLIGHT_DOUBLE_TAP_WINDOW);
        if is_double_tap {
            self.reset();
        } else {
            self.last_tap = Some(now);
        }
        is_double_tap
    }

    fn reset(&mut self) {
        self.last_tap = None;
    }
}

fn should_exit_creative_flight(was_flying: bool, vertical_input: f32, on_ground: bool) -> bool {
    was_flying && vertical_input < 0.0 && on_ground
}

fn sprint_allowed(game_mode: GameMode, hunger: f32) -> bool {
    crate::game_rules::GameModePolicy::for_mode(game_mode, true).can_fly || hunger > 6.0
}

fn sprint_exhaustion_amount(
    game_mode: GameMode,
    is_sprinting: bool,
    is_moving: bool,
    dt: f32,
) -> f32 {
    if crate::game_rules::GameModePolicy::for_mode(game_mode, true).hunger_enabled
        && is_sprinting
        && is_moving
    {
        dt * 0.15
    } else {
        0.0
    }
}

#[cfg(test)]
mod creative_flight_input_tests {
    use super::*;

    #[test]
    fn double_tap_toggles_only_inside_the_window() {
        let start = Instant::now();
        let mut tracker = DoubleTapTracker::default();

        assert!(!tracker.register(start, true, false));
        assert!(tracker.register(start + Duration::from_millis(300), true, false));

        assert!(!tracker.register(start + Duration::from_secs(1), true, false));
        assert!(!tracker.register(start + Duration::from_millis(1301), true, false));
    }

    #[test]
    fn repeat_does_not_count_as_a_second_tap() {
        let start = Instant::now();
        let mut tracker = DoubleTapTracker::default();

        assert!(!tracker.register(start, true, false));
        assert!(!tracker.register(start + Duration::from_millis(50), true, true));
        assert!(tracker.register(start + Duration::from_millis(100), true, false));
    }

    #[test]
    fn disabled_or_reset_tracker_cannot_prearm_creative_flight() {
        let start = Instant::now();
        let mut tracker = DoubleTapTracker::default();

        assert!(!tracker.register(start, false, false));
        assert!(!tracker.register(start + Duration::from_millis(100), true, false));
        tracker.reset();
        assert!(!tracker.register(start + Duration::from_millis(200), true, false));
        assert!(tracker.register(start + Duration::from_millis(250), true, false));
    }

    #[test]
    fn successful_double_tap_starts_a_fresh_pair() {
        let start = Instant::now();
        let mut tracker = DoubleTapTracker::default();

        assert!(!tracker.register(start, true, false));
        assert!(tracker.register(start + Duration::from_millis(50), true, false));
        assert!(!tracker.register(start + Duration::from_millis(100), true, false));
        assert!(tracker.register(start + Duration::from_millis(150), true, false));
    }

    #[test]
    fn only_descending_onto_the_ground_exits_flight() {
        assert!(should_exit_creative_flight(true, -1.0, true));
        assert!(!should_exit_creative_flight(true, 0.0, true));
        assert!(!should_exit_creative_flight(true, 1.0, true));
        assert!(!should_exit_creative_flight(true, -1.0, false));
        assert!(!should_exit_creative_flight(false, -1.0, true));
    }
}

#[cfg(test)]
mod sprint_policy_tests {
    use super::*;

    #[test]
    fn creative_sprint_ignores_hunger_and_exhaustion() {
        assert!(sprint_allowed(GameMode::Creative, 0.0));
        assert_eq!(
            sprint_exhaustion_amount(GameMode::Creative, true, true, 10.0),
            0.0
        );
    }

    #[test]
    fn survival_sprint_keeps_hunger_and_exhaustion_rules() {
        assert!(!sprint_allowed(GameMode::Survival, 6.0));
        assert!(sprint_allowed(GameMode::Survival, 6.01));
        assert!(
            (sprint_exhaustion_amount(GameMode::Survival, true, true, 10.0) - 1.5).abs()
                < f32::EPSILON
        );
        assert_eq!(
            sprint_exhaustion_amount(GameMode::Survival, false, true, 10.0),
            0.0
        );
        assert_eq!(
            sprint_exhaustion_amount(GameMode::Survival, true, false, 10.0),
            0.0
        );
    }
}

#[cfg(test)]
mod authority_policy_tests {
    use super::*;
    use crate::server_runtime::projection::entity_state_wire;

    #[test]
    fn multiplayer_host_keeps_world_ticks_running_while_paused_or_dead() {
        let host = MultiplayerRole::Host { port: 25565 };
        assert!(should_advance_simulation(&host, true, true, false));
        assert!(should_advance_simulation(&host, true, false, true));
        assert!(should_advance_simulation(&host, true, true, true));
        assert!(!should_advance_simulation(&host, false, false, false));
    }

    #[test]
    fn singleplayer_pause_and_death_still_stop_world_ticks() {
        assert!(!should_advance_simulation(
            &MultiplayerRole::Singleplayer,
            true,
            true,
            false,
        ));
        assert!(!should_advance_simulation(
            &MultiplayerRole::Singleplayer,
            true,
            false,
            true,
        ));
        assert!(should_advance_simulation(
            &MultiplayerRole::Singleplayer,
            true,
            false,
            false,
        ));
    }

    #[test]
    fn replicated_entity_samples_interpolate_without_mutating_authority() {
        let mut replicated = ReplicatedEntityState::new(99);
        let state = |sequence, x| crate::network::protocol::EntityStateWire {
            entity_id: 7,
            entity_type: crate::entity::EntityType::Zombie.to_wire(),
            position: [x, 64.0, 0.0],
            velocity: [1.0, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            health: 20.0 - sequence as f32,
            animation_state: 0,
            item: None,
        };
        assert!(!replicated.push(state(1, 0.0), 1, 1.0));
        assert!(!replicated.push(state(2, 2.0), 2, 2.0));
        let sample = replicated.sample(1.5).unwrap();
        assert_eq!(sample.position, [1.0, 64.0, 0.0]);
        assert_eq!(sample.health, 18.5);
        assert_eq!(replicated.snapshots.back().unwrap().state.position[0], 2.0);
    }

    #[test]
    fn host_client_sixty_second_entity_checksum_converges_without_client_spawns() {
        let host_player = Vec3::new(0.0, 64.0, 0.0);
        let client_player = Vec3::new(96.0, 64.0, 96.0);
        assert!(host_player.distance(client_player) > 128.0);

        let mut host = crate::entity::Entity::new(
            7,
            crate::entity::EntityType::Zombie,
            Vec3::new(8.0, 64.0, 8.0),
        );
        host.velocity = Vec3::new(0.5, 0.0, -0.25);
        let mut client_entities = crate::entity::EntityManager::new();
        let local_id = client_entities.spawn(host.entity_type, host.position);
        let mut replica = ReplicatedEntityState::new(local_id);

        for tick in 1..=1_200u64 {
            host.position += host.velocity * SIM_TICK_TIME;
            host.yaw += 0.0025;
            let state = entity_state_wire(&host);
            assert!(!replica.push(state, tick, tick as f64 * f64::from(SIM_TICK_TIME)));
            let visual = replica
                .sample(tick as f64 * f64::from(SIM_TICK_TIME))
                .unwrap();
            apply_entity_wire_state(client_entities.get_by_id_mut(local_id).unwrap(), visual);
            assert_eq!(
                client_entities.entities.len(),
                1,
                "client spawned an authority-owned living entity at tick {tick}"
            );
        }

        let client = client_entities.get_by_id(local_id).unwrap();
        let checksum = |entity: &crate::entity::Entity| {
            entity.position.x.to_bits() as u64
                ^ (entity.position.y.to_bits() as u64).rotate_left(11)
                ^ (entity.position.z.to_bits() as u64).rotate_left(22)
                ^ (entity.health.to_bits() as u64).rotate_left(33)
        };
        assert_eq!(checksum(client), checksum(&host));
    }

    #[test]
    fn host_clamps_remote_pose_before_echoing_authoritative_correction() {
        let latest = PlayerSnapshot {
            position: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            time: 0.0,
            sequence: 1,
            sender_time_millis: 1_000,
        };
        let accepted = validated_remote_position(Some(&latest), Vec3::new(1.2, 0.0, 0.0), 1_050);
        assert_eq!(accepted, Vec3::new(1.2, 0.0, 0.0));

        let corrected = validated_remote_position(Some(&latest), Vec3::new(100.0, 0.0, 0.0), 1_050);
        assert!((corrected.x - 1.6).abs() < f32::EPSILON);
        assert_eq!(
            validated_remote_position(Some(&latest), Vec3::ONE, 999),
            Vec3::ZERO
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StationKind {
    Enchanting,
    Brewing,
    Anvil,
    Merchant,
}

/// Explicit lifecycle for asynchronous GPU timestamp readback.  Mapping is
/// only entered after a submission and a device poll; the range is read only
/// in Mapped and is consumed exactly once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuTimestampReadbackState {
    Unmapped,
    CopyEncoded,
    Mapping,
    Mapped,
    Consumed,
}

impl GpuTimestampReadbackState {
    pub fn map_requested(self) -> Self {
        (self == Self::CopyEncoded)
            .then_some(Self::Mapping)
            .unwrap_or(self)
    }
    pub fn map_completed(self, success: bool) -> Self {
        if self == Self::Mapping && success {
            Self::Mapped
        } else if self == Self::Mapping {
            Self::Unmapped
        } else {
            self
        }
    }
    pub fn consume(self) -> Self {
        if self == Self::Mapped {
            Self::Consumed
        } else {
            self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GpuTimestampReadbackStatus {
    state: GpuTimestampReadbackState,
    submission_tag: Option<u64>,
}

impl GpuTimestampReadbackStatus {
    fn unmapped() -> Self {
        Self {
            state: GpuTimestampReadbackState::Unmapped,
            submission_tag: None,
        }
    }

    fn reserve_copy(&mut self, submission_tag: u64) -> bool {
        if !matches!(
            self.state,
            GpuTimestampReadbackState::Unmapped | GpuTimestampReadbackState::Consumed
        ) {
            return false;
        }
        self.state = GpuTimestampReadbackState::CopyEncoded;
        self.submission_tag = Some(submission_tag);
        true
    }

    fn begin_mapping(&mut self, submission_tag: u64) -> bool {
        if self.state != GpuTimestampReadbackState::CopyEncoded
            || self.submission_tag != Some(submission_tag)
        {
            return false;
        }
        self.state = self.state.map_requested();
        true
    }

    fn map_completed(&mut self, submission_tag: u64, success: bool) {
        if self.state != GpuTimestampReadbackState::Mapping
            || self.submission_tag != Some(submission_tag)
        {
            return;
        }
        self.state = self.state.map_completed(success);
        if !success {
            self.submission_tag = None;
        }
    }

    fn consume(&mut self, submission_tag: u64) -> bool {
        if self.state != GpuTimestampReadbackState::Mapped
            || self.submission_tag != Some(submission_tag)
        {
            return false;
        }
        self.state = self.state.consume();
        self.submission_tag = None;
        true
    }
}

struct GpuTimestampReadbackSlot {
    buffer: wgpu::Buffer,
    status: std::sync::Arc<std::sync::Mutex<GpuTimestampReadbackStatus>>,
}

/// Capability gate used by the renderer and HUD. Pass-local timing is only
/// valid when both feature bits are available.
#[cfg(test)]
pub const fn gpu_timestamp_capability(timestamp_query: bool, inside_passes: bool) -> bool {
    timestamp_query && inside_passes
}

#[cfg(test)]
mod gpu_timestamp_state_tests {
    use super::GpuTimestampReadbackState as S;
    #[test]
    fn transitions_are_ordered_and_failure_is_recoverable() {
        assert_eq!(S::Unmapped.map_requested(), S::Unmapped);
        assert_eq!(S::CopyEncoded.map_requested(), S::Mapping);
        assert_eq!(S::Mapping.map_completed(true), S::Mapped);
        assert_eq!(S::Mapped.consume(), S::Consumed);
        assert_eq!(S::Mapping.map_completed(false), S::Unmapped);
        assert_eq!(S::Consumed.consume(), S::Consumed);
        assert_eq!(S::Mapped.map_requested(), S::Mapped);
    }

    #[test]
    fn two_submission_tagged_slots_cannot_be_reused_while_mapping_or_mapped() {
        let mut slots = [
            super::GpuTimestampReadbackStatus::unmapped(),
            super::GpuTimestampReadbackStatus::unmapped(),
        ];
        assert!(slots[0].reserve_copy(10));
        assert!(slots[0].begin_mapping(10));
        assert!(!slots[0].reserve_copy(11));
        assert!(slots[1].reserve_copy(11));
        assert!(slots[1].begin_mapping(11));

        slots[0].map_completed(10, true);
        assert!(!slots[0].reserve_copy(12));
        assert!(slots[0].consume(10));
        assert!(slots[0].reserve_copy(12));
        assert_eq!(slots[0].submission_tag, Some(12));
    }

    #[test]
    fn capability_requires_timestamp_query_and_inside_passes() {
        assert!(!super::gpu_timestamp_capability(false, false));
        assert!(!super::gpu_timestamp_capability(true, false));
        assert!(super::gpu_timestamp_capability(true, true));
    }
}

fn apply_entity_wire_state(
    entity: &mut crate::entity::Entity,
    state: crate::network::protocol::EntityStateWire,
) {
    entity.position = Vec3::from_array(state.position);
    entity.velocity = Vec3::from_array(state.velocity);
    entity.yaw = state.yaw;
    entity.pitch = state.pitch;
    entity.health = state.health;
    entity.on_ground = state.animation_state & 1 != 0;
    entity.target_player = state.animation_state & (1 << 1) != 0;
    entity.is_ignited = state.animation_state & (1 << 2) != 0;
    entity.fire_aspect_timer = if state.animation_state & (1 << 3) != 0 {
        entity.fire_aspect_timer.max(0.1)
    } else {
        0.0
    };
    if entity.entity_type == crate::entity::EntityType::DroppedItem {
        if let Some(stack) = state.item.and_then(|item| item.to_stack()) {
            entity.dropped_item = Some(stack.item);
            entity.dropped_count = stack.count;
            entity.dropped_stack = Some(stack);
        }
    } else if entity.entity_type == crate::entity::EntityType::SplashPotion {
        entity.potion = state
            .item
            .and_then(|item| item.to_stack())
            .and_then(|stack| stack.potion);
    }
}

fn is_replicated_entity_type(entity_type: crate::entity::EntityType) -> bool {
    entity_type.is_living()
        || entity_type.is_projectile()
        || entity_type == crate::entity::EntityType::DroppedItem
        || entity_type == crate::entity::EntityType::EndCrystal
}

fn effect_from_wire(
    effect: crate::network::protocol::PlayerEffectWire,
) -> Option<crate::brewing::PotionEffect> {
    use crate::brewing::PotionEffect;
    let duration = effect.remaining_seconds.max(0.0);
    match effect.kind {
        0 => Some(PotionEffect::Speed {
            level: effect.level,
            duration,
        }),
        1 => Some(PotionEffect::Strength {
            level: effect.level,
            duration,
        }),
        2 => Some(PotionEffect::Healing {
            level: effect.level,
        }),
        3 => Some(PotionEffect::Regeneration {
            level: effect.level,
            duration,
        }),
        4 => Some(PotionEffect::NightVision { duration }),
        5 => Some(PotionEffect::Invisibility { duration }),
        6 => Some(PotionEffect::FireResistance { duration }),
        7 => Some(PotionEffect::WaterBreathing { duration }),
        8 => Some(PotionEffect::Poison {
            level: effect.level,
            duration,
        }),
        9 => Some(PotionEffect::Slowness {
            level: effect.level,
            duration,
        }),
        _ => None,
    }
}

fn normalized_chat_message(input: &str) -> Option<String> {
    let message: String = input
        .trim()
        .chars()
        .filter(|ch| !ch.is_control())
        .take(CHAT_INPUT_CAPACITY)
        .collect();
    (!message.is_empty()).then_some(message)
}

fn push_chat_history(
    history: &mut std::collections::VecDeque<(String, String)>,
    sender: String,
    message: String,
) {
    if history.len() == CHAT_HISTORY_CAPACITY {
        history.pop_front();
    }
    history.push_back((sender, message));
}

fn clear_remote_players(
    remote_players: &mut std::collections::HashMap<
        crate::network::protocol::PlayerId,
        RemotePlayerState,
    >,
    entity_manager: &mut crate::entity::EntityManager,
) {
    remote_players.clear();
    entity_manager
        .entities
        .retain(|entity| entity.entity_type != crate::entity::EntityType::RemotePlayer);
}

fn project_name_tag(position: Vec3, view_proj: Mat4) -> Option<Vec2> {
    let clip = view_proj * position.extend(1.0);
    if clip.w <= f32::EPSILON {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    if !(0.0..=1.0).contains(&ndc.z) || ndc.y < -1.2 || ndc.y > 1.2 {
        return None;
    }
    Some(Vec2::new(ndc.x, ndc.y))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CameraPerspective {
    #[default]
    FirstPerson,
    ThirdPersonBack,
    ThirdPersonFront,
}

impl CameraPerspective {
    pub fn next(self) -> Self {
        match self {
            Self::FirstPerson => Self::ThirdPersonBack,
            Self::ThirdPersonBack => Self::ThirdPersonFront,
            Self::ThirdPersonFront => Self::FirstPerson,
        }
    }

    pub fn is_third_person(self) -> bool {
        self != Self::FirstPerson
    }
}

fn perspective_camera_transform(
    perspective: CameraPerspective,
    yaw: f32,
    pitch: f32,
) -> (Vec3, f32, f32) {
    let forward = Vec3::new(
        yaw.cos() * pitch.cos(),
        pitch.sin(),
        yaw.sin() * pitch.cos(),
    )
    .normalize_or_zero();
    match perspective {
        CameraPerspective::FirstPerson => (Vec3::ZERO, yaw, pitch),
        CameraPerspective::ThirdPersonBack => (-forward * 4.0, yaw, pitch),
        CameraPerspective::ThirdPersonFront => (forward * 4.0, yaw + std::f32::consts::PI, -pitch),
    }
}

#[cfg(test)]
mod camera_perspective_tests {
    use super::*;

    #[test]
    fn f5_cycles_like_minecraft() {
        let first = CameraPerspective::FirstPerson;
        let back = first.next();
        let front = back.next();
        assert_eq!(back, CameraPerspective::ThirdPersonBack);
        assert_eq!(front, CameraPerspective::ThirdPersonFront);
        assert_eq!(front.next(), CameraPerspective::FirstPerson);
    }

    #[test]
    fn front_camera_sits_ahead_and_looks_back_at_player() {
        let (offset, view_yaw, view_pitch) =
            perspective_camera_transform(CameraPerspective::ThirdPersonFront, 0.35, -0.2);
        let view_forward = Vec3::new(
            view_yaw.cos() * view_pitch.cos(),
            view_pitch.sin(),
            view_yaw.sin() * view_pitch.cos(),
        )
        .normalize();
        assert!((offset.length() - 4.0).abs() < 1e-5);
        assert!(offset.normalize().dot(view_forward) < -0.999);
    }
}

pub struct State {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,
    terrain_render_pipeline: wgpu::RenderPipeline,
    terrain_trans_pipeline: wgpu::RenderPipeline,
    region_bind_group_layout: wgpu::BindGroupLayout,
    crack_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    pub camera: Camera,
    camera_uniform: CameraUniform,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    depth_view: wgpu::TextureView,
    pub chunk_manager: ChunkManager,
    pub chunk_meshes: std::collections::HashMap<(i32, i32), ChunkMesh>,
    pub render_regions: std::collections::HashMap<(i32, i32), RenderRegion>,
    /// At most one low-priority compaction candidate is staged per frame.
    compaction_pending_region: Option<(i32, i32)>,
    section_storage_compaction_queue: std::collections::VecDeque<SectionKey>,
    section_storage_compaction_queued: std::collections::HashSet<SectionKey>,
    terrain_worker_tx: std::sync::mpsc::Sender<TerrainWorkerResult>,
    terrain_worker_rx: std::sync::mpsc::Receiver<TerrainWorkerResult>,
    pending_worker_results: std::collections::VecDeque<TerrainWorkerResult>,
    scheduler: crate::chunk_schedule::ChunkStreamingScheduler,
    section_scheduler: crate::chunk_schedule::SectionMeshScheduler,
    chunk_load_in_flight: std::collections::HashMap<(i32, i32), u64>,
    chunk_lifetimes: std::collections::HashMap<(i32, i32), u64>,
    next_chunk_lifetime: u64,
    terrain_generation: u64,
    los_world_revision: u64,
    submitted_terrain_triangles: u64,
    submitted_terrain_draw_calls: usize,
    visible_chunk_count: usize,
    pub player_physics: PlayerPhysics,
    pub prev_player_position: Vec3,
    pub sim_accumulator: f32,
    pub keys: KeyState,
    jump_taps: DoubleTapTracker,
    #[allow(dead_code)]
    texture_atlas: crate::texture::TextureAtlas,
    crosshair_buffer: wgpu::Buffer,
    pub is_paused: bool,
    mouse_ndc: [f32; 2],
    pub sensitivity: f32,
    ui_pipeline: wgpu::RenderPipeline,
    ui_line_pipeline: wgpu::RenderPipeline,
    ui_vertex_buffer: wgpu::Buffer,
    ui_line_vertex_buffer: wgpu::Buffer,
    ui_textured_pipeline: wgpu::RenderPipeline,
    ui_textured_vertex_buffer: wgpu::Buffer,
    num_ui_vertices: u32,
    num_ui_line_vertices: u32,
    num_ui_textured_vertices: u32,
    pub game_mode: GameMode,
    pub inventory: Inventory,
    pub recipe_manager: RecipeManager,
    pub left_mouse_pressed: bool,
    /// Visible primary-action swing timing, kept separate from the Survival
    /// mining latch so air swings, Creative hits, and melee still animate.
    hand_swing_started_at: f32,
    hand_swing_until: f32,
    pub mining_target: Option<glam::Vec3>,
    pub mining_progress: f32,
    /// Held-stack identity latched with the typed StartBreak ingress. This
    /// prevents a held-item change from re-starting the same target without a
    /// single balancing CancelBreak request.
    mining_held: Option<crate::network::protocol::SessionSlotWire>,
    /// Suppresses duplicate CancelBreak traffic while the authoritative
    /// projection is still in flight after release/target loss.
    mining_cancel_sent: bool,
    crack_vertex_buffer: wgpu::Buffer,
    crack_index_buffer: wgpu::Buffer,
    pub player_state: PlayerState,
    pub world_time: crate::camera::WorldTime,
    pub show_debug: bool,
    /// F5 cycles first person, third-person back, and third-person front.
    pub camera_perspective: CameraPerspective,
    pub entity_manager: crate::entity::EntityManager,
    /// Authority session overlay: local player's fishing hook entity id, if any.
    presented_fishing_hook_entity: Option<u64>,
    mob_instanced_pipeline: wgpu::RenderPipeline,
    particle_instanced_pipeline: wgpu::RenderPipeline,

    mob_cuboid_proto_vbuf: wgpu::Buffer,
    mob_cuboid_proto_ibuf: wgpu::Buffer,
    mob_quad_proto_vbuf: wgpu::Buffer,
    mob_quad_proto_ibuf: wgpu::Buffer,

    particle_proto_vbuf: wgpu::Buffer,
    particle_proto_ibuf: wgpu::Buffer,

    frame_ring_index: usize,
    mob_cuboid_instance_buffers: [wgpu::Buffer; 3],
    mob_quad_instance_buffers: [wgpu::Buffer; 3],
    particle_instance_buffers: [wgpu::Buffer; 3],
    frame_resource_pool: crate::gpu_frame_resources::FrameResourcePool<()>,
    gpu_completion_tx: std::sync::mpsc::Sender<u64>,
    gpu_completion_rx: std::sync::mpsc::Receiver<u64>,
    next_gpu_submission_id: u64,

    mob_cuboid_instances_scratch: Vec<crate::mob_renderer::MobInstance>,
    mob_quad_instances_scratch: Vec<crate::mob_renderer::MobInstance>,
    particle_instances_scratch: Vec<crate::particles::ParticleInstance>,
    mob_cuboid_num_instances: u32,
    mob_quad_num_instances: u32,
    mob_num_indices: u32,
    hand_pipeline: wgpu::RenderPipeline,
    hand_vertex_buffer: wgpu::Buffer,
    hand_index_buffer: wgpu::Buffer,
    hand_num_indices: u32,
    #[allow(dead_code)] // Owned for bind group lifetime; not read directly.
    hand_camera_buffer: wgpu::Buffer,
    hand_camera_bind_group: wgpu::BindGroup,
    pub particles: crate::particles::ParticleSystem,
    particle_num_indices: u32,
    torch_smoke_timer: f32,
    total_time: f32,
    pub audio_manager: crate::audio::AudioManager,
    /// Selected resource packs are retained by the presentation root so all
    /// runtime text can be rebuilt in place when the language changes. This
    /// never rebuilds chunks/world authority or alters simulation state.
    resource_pack_manager: crate::resources::ResourcePackManager,
    /// Immutable descriptors selected from the same ordered resource packs as
    /// the atlas.  Terrain workers clone this handle instead of consulting a
    /// process-global/default model table.
    model_registry: Arc<crate::block_model::ModelRegistry>,
    /// Presentation font selected from the same ordered resource packs.  The
    /// built-in line font remains the fallback when no bitmap override exists.
    font_source: crate::resources::FontSource,
    pub translation_catalog: crate::localization::TranslationCatalog,
    pub footstep_accumulator: f32,
    pub was_on_ground: bool,
    pub is_saving: bool,
    pub save_error: Option<String>,
    pub is_sprinting: bool,
    sprint_toggle_latched: bool,
    sneak_toggle_latched: bool,
    last_ctrl_pressed: bool,
    last_shift_pressed: bool,
    pub base_fov: f32,
    pub w_click_timer: f32,
    pub last_w_pressed: bool,
    debug_frame_time_accumulator: f32,
    debug_frame_samples: u32,
    debug_fps: f32,
    debug_frame_ms: f32,
    perf_recorder: crate::perf::PerfRecorder,
    perf_summaries: [crate::perf::ScopeSummary; crate::perf::SCOPE_COUNT],
    perf_counters: crate::perf::PerfCounters,
    /// Bounded machine-readable per-frame samples for replayable telemetry.
    pub frame_perf_samples: std::collections::VecDeque<crate::perf::FramePerfSample>,
    next_perf_frame_id: u64,
    gpu_upload_time_frame: Duration,
    lighting_time_frame: Duration,
    lighting_scopes_frame: crate::perf::LightingPerfSample,
    gpu_upload_scopes_frame: crate::perf::GpuUploadPerfSample,
    gpu_timestamp_query_set: Option<wgpu::QuerySet>,
    gpu_timestamp_resolve_buffer: Option<wgpu::Buffer>,
    gpu_timestamp_readback_slots: Vec<GpuTimestampReadbackSlot>,
    gpu_pass_timings_ns: [u64; 7],
    gpu_pass_timings_valid: bool,
    gpu_pass_timing_submission_tag: Option<u64>,
    gpu_timestamps_supported: bool,
    gpu_timestamps_inside_passes: bool,
    terrain_candidates_scratch: Vec<crate::chunk_render::DrawCandidate>,
    terrain_draw_plan_scratch: crate::chunk_render::DrawPlan,
    pub entity_los_manager: crate::culling::EntityLosManager,
    visible_sections_scratch: std::collections::HashSet<(i32, i8, i32)>,
    section_visibility_scratch: crate::culling::SectionVisibilityScratch,
    hand_vertices_scratch: Vec<Vertex>,
    hand_indices_scratch: Vec<u32>,
    last_hand_mesh_key: Option<crate::hand_renderer::HandMeshKey>,
    ui_vertices_scratch: Vec<UiVertex>,
    ui_line_vertices_scratch: Vec<UiVertex>,
    debug_str_scratch: String,
    pub active_station: Option<StationKind>,
    pub container_target: Option<(i32, i32, i32)>,
    pub container_is_double: bool,
    pub enchanting: crate::enchantment::EnchantingState,
    pub brewing: crate::brewing::BrewingStandState,
    pub anvil: crate::enchantment::AnvilState,
    pub potion_effects: crate::brewing::EffectManager,
    pub recipe_book_open: bool,
    pub recipe_book_search: String,
    pub weather: crate::weather::WeatherPresentation,
    pub settings: GameSettings,
    /// Presentation-only timer for the End/dragon completion flash.
    pub end_flash_time: f32,
    pub world_seed: u32,
    pub world_spawn: (i32, i32, i32),
    pub difficulty: Difficulty,
    /// One authoritative snapshot consumed by simulation and commands.
    pub world_rules: crate::game_rules::WorldRules,
    pub world_type: crate::game_rules::WorldType,
    pub generate_structures: bool,
    pub bonus_chest: bool,
    pub cheats_enabled: bool,
    pub current_dimension: crate::dimension::Dimension,
    portal_contact_time: f32,
    portal_cooldown: f32,
    pub advancement_manager: crate::advancements::AdvancementManager,
    pub advancement_gui: crate::advancements::AdvancementGui,
    pub role: MultiplayerRole,
    /// Singleplayer and listen-host presentation roots submit to this shared
    /// fixed-tick runtime. Dedicated mode never constructs `State` and
    /// therefore never allocates this GPU-side bridge.
    embedded_runtime: Option<EmbeddedRuntimeBridge>,
    pub network: NetworkHandle,
    network_staging: NetworkStaging,
    network_ready: bool,
    local_player_id: Option<crate::network::protocol::PlayerId>,
    remote_players:
        std::collections::HashMap<crate::network::protocol::PlayerId, RemotePlayerState>,
    /// Client-only visual copies of host-owned non-player entities.
    replicated_entities: std::collections::HashMap<u64, ReplicatedEntityState>,
    client_player_health_sequence: u64,
    client_player_effect_sequence: u64,
    /// Private session projection ordering is independent from the legacy
    /// health/effect lanes.  Sequence orders dimension transfers; revision
    /// orders snapshots within a dimension.
    client_session_projection: Option<(u8, u64, u64)>,
    pub network_status: Option<String>,
    pub chat_messages: std::collections::VecDeque<(String, String)>,
    pub chat_input: String,
    pub is_chat_open: bool,
    pub connection_lost: bool,
    network_position_timer: f32,
    network_pose_sequence: u32,
    pub active_merchant_villager_id: Option<u64>,
    pub active_merchant_offers: Vec<crate::village::trade::TradeOffer>,
    pub active_merchant_profession: crate::village::poi::VillagerProfession,
    pub active_merchant_level: crate::village::trade::VillagerLevel,
    pub active_merchant_xp: u32,
    network_time: f64,
    /// Client-only: chunk payloads that arrived from the host before the chunk
    /// was streamed in. Applied when `update_chunks` loads the coordinate.
    pending_chunk_payloads:
        std::collections::HashMap<(i32, i32), (u64, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>)>,
    /// Client-only coalesced mutations for chunks that are not streamed in yet.
    /// The latest authoritative value wins for each world-space block.
    pending_block_changes: std::collections::HashMap<
        (i32, i32),
        std::collections::HashMap<(i32, i32, i32), (u64, u32, u8, u8)>,
    >,
    client_chunk_revisions: std::collections::HashMap<(crate::dimension::Dimension, i32, i32), u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotType {
    Creative(Item),
    Hotbar(usize),
    Backpack(usize),
    Armor(usize),
    Offhand,
    CraftInput(usize),
    CraftOutput,
    EnchantInput,
    EnchantLapis,
    BrewBottle(usize),
    BrewIngredient,
    ContainerSlot(usize),
    AnvilLeft,
    AnvilRight,
    AnvilOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InventoryLayoutKind {
    CreativeCatalog,
    Standard,
}

type InventoryUiRect = MenuRect;

fn presentation_target_for_slot(slot: SlotType) -> PresentationInventoryTarget {
    match slot {
        SlotType::ContainerSlot(_) => PresentationInventoryTarget::ContainerSlot,
        SlotType::AnvilOutput | SlotType::EnchantInput | SlotType::EnchantLapis => {
            PresentationInventoryTarget::Workstation
        }
        _ => PresentationInventoryTarget::PlayerInventory,
    }
}

fn presentation_target_for_authority_hit(
    hit: InventoryHit<SlotType>,
) -> Option<PresentationInventoryTarget> {
    match hit {
        InventoryHit::Slot(slot) => Some(presentation_target_for_slot(slot)),
        InventoryHit::Enchant { .. }
        | InventoryHit::RecipeBook
        | InventoryHit::RecipeBookToggle => Some(PresentationInventoryTarget::Workstation),
        InventoryHit::Merchant { .. } | InventoryHit::CreativeTab { .. } | InventoryHit::Empty => {
            None
        }
    }
}

fn inventory_layout_kind(
    game_mode: GameMode,
    station_open: bool,
    crafting_table_open: bool,
) -> InventoryLayoutKind {
    if game_mode == GameMode::Creative && !station_open && !crafting_table_open {
        InventoryLayoutKind::CreativeCatalog
    } else {
        InventoryLayoutKind::Standard
    }
}

fn creative_slot_metrics(aspect: f32) -> (f32, f32, f32, f32) {
    let safe_aspect = aspect.max(0.1);
    let slot_w = 0.08_f32.min(0.15 / safe_aspect);
    let slot_h = slot_w * safe_aspect;
    let gap = 0.01;
    let grid_w = CREATIVE_COLUMNS as f32 * slot_w + (CREATIVE_COLUMNS - 1) as f32 * gap;
    let start_x = -grid_w / 2.0;
    (slot_w, slot_h, gap, start_x)
}

fn creative_catalog_slot_rect(index: usize, aspect: f32) -> InventoryUiRect {
    let (slot_w, slot_h, gap, start_x) = creative_slot_metrics(aspect);
    let row = index / CREATIVE_COLUMNS;
    let column = index % CREATIVE_COLUMNS;
    let x0 = start_x + column as f32 * (slot_w + gap);
    let y1 = 0.64 - row as f32 * (slot_h + gap);
    InventoryUiRect {
        x0,
        x1: x0 + slot_w,
        y0: y1 - slot_h,
        y1,
    }
}

fn creative_hotbar_slot_rect(index: usize, aspect: f32) -> InventoryUiRect {
    let (slot_w, slot_h, gap, start_x) = creative_slot_metrics(aspect);
    let x0 = start_x + index as f32 * (slot_w + gap);
    InventoryUiRect {
        x0,
        x1: x0 + slot_w,
        y0: -0.85,
        y1: -0.85 + slot_h,
    }
}

fn creative_tab_rect(index: usize) -> InventoryUiRect {
    let width = 0.125;
    let gap = 0.005;
    let start_x = -(CreativeTab::TABS.len() as f32 * width
        + (CreativeTab::TABS.len() - 1) as f32 * gap)
        / 2.0;
    let x0 = start_x + index as f32 * (width + gap);
    InventoryUiRect {
        x0,
        x1: x0 + width,
        y0: 0.78,
        y1: 0.88,
    }
}

fn creative_scroll_track_rect(aspect: f32) -> InventoryUiRect {
    let first = creative_catalog_slot_rect(0, aspect);
    let last = creative_catalog_slot_rect(CREATIVE_VISIBLE_SLOTS - 1, aspect);
    InventoryUiRect {
        x0: first.x0
            + CREATIVE_COLUMNS as f32
                * (creative_slot_metrics(aspect).0 + creative_slot_metrics(aspect).2)
            - creative_slot_metrics(aspect).2
            + 0.02,
        x1: first.x0
            + CREATIVE_COLUMNS as f32
                * (creative_slot_metrics(aspect).0 + creative_slot_metrics(aspect).2)
            - creative_slot_metrics(aspect).2
            + 0.045,
        y0: last.y0,
        y1: first.y1,
    }
}

impl State {
    fn sync_translation_catalog(&mut self) {
        if self.translation_catalog.language() != self.settings.language {
            self.translation_catalog =
                crate::localization::TranslationCatalog::from_resource_packs_mut(
                    &mut self.resource_pack_manager,
                    self.settings.language,
                );
        }
    }

    /// Switch locale while the current world, input state and selected pack
    /// order stay alive.  Rebuilding this bounded catalog is presentation-only.
    pub fn set_language(&mut self, language: crate::localization::Language) {
        self.settings.language = language;
        self.sync_translation_catalog();
        self.settings.save();
    }

    pub fn translate(&self, key: &str) -> String {
        self.translation_catalog.lookup(key)
    }

    pub fn localized_item_name(&self, item: crate::inventory::Item) -> String {
        self.translation_catalog.item_name(item)
    }

    pub fn localized_block_name(&self, block: crate::world::BlockType) -> String {
        self.translation_catalog.block_name(block)
    }

    pub fn localized_entity_name(&self, entity: crate::entity::EntityType) -> String {
        self.translation_catalog.entity_name(entity)
    }

    fn create_depth_texture(
        device: &wgpu::Device,
        config: &wgpu::SurfaceConfiguration,
    ) -> wgpu::TextureView {
        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Depth Texture"),
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        depth_texture.create_view(&wgpu::TextureViewDescriptor::default())
    }

    pub async fn new(window: Arc<Window>, launch: WorldLaunch, settings: GameSettings) -> Self {
        let role = launch.role.clone();
        let is_client = matches!(role, MultiplayerRole::Client { .. });
        let in_process_authority = matches!(
            &role,
            MultiplayerRole::Singleplayer | MultiplayerRole::Host { .. }
        );
        let crate::presentation::bootstrap::GpuContext {
            surface,
            device,
            queue,
            config,
            size,
            gpu_timestamps_supported,
            gpu_timestamps_inside_passes,
        } = crate::presentation::bootstrap::create_gpu_context(&window, &settings).await;

        let (gpu_timestamp_query_set, gpu_timestamp_resolve_buffer, gpu_timestamp_readback_slots) =
            if gpu_timestamps_inside_passes {
                let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("Timestamp Query Set"),
                    count: GPU_TIMESTAMP_QUERY_COUNT,
                    ty: wgpu::QueryType::Timestamp,
                });
                let resolve_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Timestamp Resolve Buffer"),
                    size: GPU_TIMESTAMP_READBACK_BYTES,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                });
                let readback_slots = (0..GPU_TIMESTAMP_READBACK_SLOT_COUNT)
                    .map(|slot_index| GpuTimestampReadbackSlot {
                        buffer: device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some(match slot_index {
                                0 => "Timestamp Readback Buffer 0",
                                _ => "Timestamp Readback Buffer 1",
                            }),
                            size: GPU_TIMESTAMP_READBACK_BYTES,
                            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                            mapped_at_creation: false,
                        }),
                        status: std::sync::Arc::new(std::sync::Mutex::new(
                            GpuTimestampReadbackStatus::unmapped(),
                        )),
                    })
                    .collect();
                (Some(query_set), Some(resolve_buffer), readback_slots)
            } else {
                (None, None, Vec::new())
            };

        // Setup Depth Buffer
        let depth_view = Self::create_depth_texture(&device, &config);

        let crate::presentation::bootstrap::LaunchWorldState {
            current_dimension,
            player_physics,
            game_mode,
            inventory,
            player_state,
            camera_yaw,
            camera_pitch,
            world_time,
            world_seed,
            world_spawn,
            world_rules,
            world_type,
            generate_structures,
            bonus_chest,
            cheats_enabled,
            advancement_progress,
        } = crate::presentation::bootstrap::load_launch_world_state(
            &launch,
            is_client,
            in_process_authority,
        );
        let keys = KeyState::default();

        let mut resource_pack_manager = crate::resources::ResourcePackManager::discover_default();
        if !settings.resource_packs.is_empty() {
            let _ = resource_pack_manager.apply_enabled_order(&settings.resource_packs);
        }
        let translation_catalog = crate::localization::TranslationCatalog::from_resource_packs_mut(
            &mut resource_pack_manager,
            settings.language,
        );
        let model_registry = Arc::new(crate::block_model::ModelRegistry::from_resource_packs(
            &mut resource_pack_manager,
            crate::block_model::all_model_paths(),
        ));
        let font_source = resource_pack_manager.resolve_font_source("font/ui.json");
        let mut audio_manager =
            crate::audio::AudioManager::new_with_resource_packs(&mut resource_pack_manager);
        audio_manager.set_subtitles_enabled(settings.accessibility.subtitles);
        audio_manager.set_volume(settings.effective_sound_volume());
        audio_manager.set_weather_volume(settings.weather_volume);

        let advancement_manager =
            crate::advancements::AdvancementManager::new(advancement_progress);
        let advancement_gui = crate::advancements::AdvancementGui::new();

        // Setup Camera
        let camera = Camera::new(
            player_physics.position + Vec3::new(0.0, 1.6, 0.0), // Spawn at player eye height
            camera_yaw,
            camera_pitch,
            settings.fov,
        );
        let base_fov = camera.fov;
        let show_debug = false;
        let mut camera_uniform = CameraUniform::new();
        camera_uniform.update_view_proj(
            &camera,
            config.width as f32 / config.height as f32,
            settings.render_distance as u32,
            current_dimension.height().height(),
            &world_time,
            0.0,
            false,
        );

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let texture_atlas = crate::texture::TextureAtlas::new_procedural_with_manager(
            &device,
            &queue,
            &mut resource_pack_manager,
        );

        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
                label: Some("camera_bind_group_layout"),
            });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &camera_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture_atlas.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&texture_atlas.sampler),
                },
            ],
            label: Some("camera_bind_group"),
        });

        let crate::presentation::bootstrap::PipelineLayouts {
            shader,
            region_bind_group_layout,
            render_pipeline_layout,
            terrain_pipeline_layout,
        } = crate::presentation::bootstrap::create_pipelines(&device, &camera_bind_group_layout);

        let terrain_render_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Terrain Render Pipeline"),
                layout: Some(&terrain_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_terrain",
                    buffers: &[crate::presentation::bootstrap::terrain_vertex_layout()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs_terrain",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Cw,
                    cull_mode: Some(wgpu::Face::Back),
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });

        // First-person hand pipeline: same shaders and bind layout as the
        // main world pipeline, but depth always passes so the hand stays on
        // top of world geometry.
        let hand_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("First Person Hand Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let terrain_trans_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Terrain Translucent Render Pipeline"),
                layout: Some(&terrain_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_terrain",
                    buffers: &[crate::presentation::bootstrap::terrain_vertex_layout()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs_terrain",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::SrcAlpha,
                                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                                operation: wgpu::BlendOperation::Add,
                            },
                            alpha: wgpu::BlendComponent::OVER,
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Cw,
                    // Translucent terrain is double-sided; there is no live cull mode.
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });

        let crack_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Crack Overlay Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::Dst,
                            dst_factor: wgpu::BlendFactor::Zero,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::Zero,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Sky Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_sky",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_sky",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        // Crosshair uses the shared UI line pipeline (vs_ui / fs_ui).
        let aspect = size.width as f32 / size.height as f32;
        let crosshair_size = 0.02;
        let crosshair_color = [1.0, 1.0, 1.0, 0.8];
        let crosshair_vertices = [
            UiVertex {
                position: [-crosshair_size, 0.0, 0.0],
                color: crosshair_color,
            },
            UiVertex {
                position: [crosshair_size, 0.0, 0.0],
                color: crosshair_color,
            },
            UiVertex {
                position: [0.0, -crosshair_size * aspect, 0.0],
                color: crosshair_color,
            },
            UiVertex {
                position: [0.0, crosshair_size * aspect, 0.0],
                color: crosshair_color,
            },
        ];

        let crosshair_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Crosshair Vertex Buffer"),
            contents: bytemuck::cast_slice(&crosshair_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Live launches are Join (`is_client`) or Embedded (`in_process_authority`).
        // Presentation chunk maps start empty; terrain arrives from ServerRuntime
        // projection or join `ChunkData`, then `update_chunks`.
        let render_distance = settings.render_distance;
        let chunk_manager = ChunkManager::new_in_dimension(render_distance, current_dimension);
        let chunk_meshes = std::collections::HashMap::new();
        let (terrain_worker_tx, terrain_worker_rx) = std::sync::mpsc::channel();
        let chunk_lifetimes = std::collections::HashMap::new();
        let next_chunk_lifetime = 1u64;

        // Initialize UI Pipelines
        let ui_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("UI Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let ui_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("UI Render Pipeline"),
            layout: Some(&ui_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_ui",
                buffers: &[UiVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_ui",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let ui_line_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("UI Line Render Pipeline"),
            layout: Some(&ui_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_ui",
                buffers: &[UiVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_ui",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let ui_textured_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("UI Textured Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_textured_ui",
                buffers: &[TexturedUiVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_textured_ui",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        // Initialize UI Buffers
        let ui_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("UI Vertex Buffer"),
            size: (std::mem::size_of::<UiVertex>() * UI_VERTEX_CAPACITY) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let ui_line_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("UI Line Vertex Buffer"),
            size: (std::mem::size_of::<UiVertex>() * UI_LINE_VERTEX_CAPACITY)
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let ui_textured_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("UI Textured Vertex Buffer"),
            size: (std::mem::size_of::<TexturedUiVertex>() * 4096) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let crack_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Crack Vertex Buffer"),
            size: (24 * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let crack_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Crack Index Buffer"),
            size: (36 * std::mem::size_of::<u32>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // First-person hand buffers. Minecraft-style extruded tool silhouettes
        // need a few hundred vertices, still well below these fixed limits.
        let hand_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Hand Vertex Buffer"),
            size: (std::mem::size_of::<Vertex>() * crate::hand_renderer::HAND_VERTEX_CAPACITY)
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let hand_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Hand Index Buffer"),
            size: (std::mem::size_of::<u32>() * crate::hand_renderer::HAND_INDEX_CAPACITY)
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Hand camera uses a very near plane so the view-space hand model is
        // never clipped by world geometry.
        let mut hand_camera_uniform = crate::camera::CameraUniform::new();
        let aspect = config.width as f32 / config.height as f32;
        let hand_proj = Mat4::perspective_lh(f32::to_radians(70.0), aspect, 0.01, 10.0);
        hand_camera_uniform.view_proj = hand_proj.to_cols_array_2d();
        hand_camera_uniform.inv_view_proj = hand_proj.inverse().to_cols_array_2d();
        hand_camera_uniform.camera_pos = [0.0, 0.0, 0.0, 0.0];

        let hand_camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Hand Camera Buffer"),
            contents: bytemuck::cast_slice(&[hand_camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let hand_camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &camera_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: hand_camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture_atlas.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&texture_atlas.sampler),
                },
            ],
            label: Some("hand_camera_bind_group"),
        });

        let (cuboid_proto_verts, cuboid_proto_inds) =
            crate::mob_renderer::build_unit_cuboid_prototype();
        let (quad_proto_verts, quad_proto_inds) = crate::mob_renderer::build_unit_quad_prototype();
        let (particle_proto_verts, particle_proto_inds) =
            crate::particles::build_particle_prototype();

        let mob_cuboid_proto_vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Mob Cuboid Prototype VBuf"),
            contents: bytemuck::cast_slice(&cuboid_proto_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let mob_cuboid_proto_ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Mob Cuboid Prototype IBuf"),
            contents: bytemuck::cast_slice(&cuboid_proto_inds),
            usage: wgpu::BufferUsages::INDEX,
        });

        let mob_quad_proto_vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Mob Quad Prototype VBuf"),
            contents: bytemuck::cast_slice(&quad_proto_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let mob_quad_proto_ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Mob Quad Prototype IBuf"),
            contents: bytemuck::cast_slice(&quad_proto_inds),
            usage: wgpu::BufferUsages::INDEX,
        });

        let particle_proto_vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Particle Prototype VBuf"),
            contents: bytemuck::cast_slice(&particle_proto_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let particle_proto_ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Particle Prototype IBuf"),
            contents: bytemuck::cast_slice(&particle_proto_inds),
            usage: wgpu::BufferUsages::INDEX,
        });

        let mob_cuboid_instance_buffers = std::array::from_fn(|i| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!("Mob Cuboid Instance Buffer {i}")),
                size: (std::mem::size_of::<crate::mob_renderer::MobInstance>() * 16384)
                    as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });

        let mob_quad_instance_buffers = std::array::from_fn(|i| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!("Mob Quad Instance Buffer {i}")),
                size: (std::mem::size_of::<crate::mob_renderer::MobInstance>() * 4096)
                    as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });

        let particle_instance_buffers = std::array::from_fn(|i| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!("Particle Instance Buffer {i}")),
                size: (std::mem::size_of::<crate::particles::ParticleInstance>()
                    * crate::particles::MAX_PARTICLES) as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });

        let mob_instanced_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Mob Instanced Render Pipeline"),
                layout: Some(&render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_instanced_mob",
                    buffers: &[
                        crate::mob_renderer::MobPrototypeVertex::layout(),
                        crate::mob_renderer::MobInstance::layout(),
                    ],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs_main",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    // The camera uses a left-handed view/projection, so the
                    // CCW-outward cuboid faces in world space appear clockwise
                    // on screen. FrontFace::Cw keeps the outside faces
                    // visible; Ccw culled every cuboid surface and made mobs
                    // look hollow/see-through.
                    front_face: wgpu::FrontFace::Cw,
                    cull_mode: Some(wgpu::Face::Back),
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: 1,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
            });

        let particle_instanced_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Particle Instanced Render Pipeline"),
                layout: Some(&render_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_instanced_particle",
                    buffers: &[
                        crate::particles::ParticlePrototypeVertex::layout(),
                        crate::particles::ParticleInstance::layout(),
                    ],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs_main",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    // Same left-handed-camera convention as the mob pipeline;
                    // the billboard quads are wound CCW in world space and
                    // must use Cw to face the camera.
                    front_face: wgpu::FrontFace::Cw,
                    cull_mode: Some(wgpu::Face::Back),
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: 1,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
            });

        let particles = crate::particles::ParticleSystem::new();
        let weather = crate::weather::WeatherPresentation::new(world_seed);
        let network = match &role {
            // Embedded Singleplayer and listen-host sessions use the
            // ServerRuntime transport.  Keeping NetworkHandle::None here is
            // intentional: a second GPU-owned NetworkServer would create a
            // second scheduling/authority path for host input.
            MultiplayerRole::Singleplayer | MultiplayerRole::Host { .. } => NetworkHandle::None,
            MultiplayerRole::Client {
                server_addr,
                port,
                username,
            } => {
                let (game_to_client, game_commands) = std::sync::mpsc::channel();
                let (client_events, client_to_game) = std::sync::mpsc::sync_channel(
                    crate::network::client::CLIENT_TO_GAME_QUEUE_CAPACITY,
                );
                let thread = crate::network::client::NetworkClient::spawn(
                    format!("{server_addr}:{port}"),
                    username.clone(),
                    game_commands,
                    client_events,
                );
                NetworkHandle::Client {
                    client_to_game,
                    game_to_client,
                    thread: Some(thread),
                }
            }
        };

        let (gpu_completion_tx, gpu_completion_rx) = std::sync::mpsc::channel();
        let embedded_runtime = if in_process_authority {
            Some(
                EmbeddedRuntimeBridge::new(
                    &role,
                    launch.world_dir.clone(),
                    world_seed,
                    launch.difficulty,
                    settings.render_distance.max(2) as u32,
                    world_rules.pvp,
                )
                .unwrap_or_else(|error| panic!("failed to start embedded server runtime: {error}")),
            )
        } else {
            None
        };
        let embedded_session_id = embedded_runtime
            .as_ref()
            .map(EmbeddedRuntimeBridge::session_id);
        let mut state = Self {
            window,
            surface,
            device,
            queue,
            config,
            size,
            terrain_render_pipeline,
            terrain_trans_pipeline,
            region_bind_group_layout,
            crack_pipeline,
            sky_pipeline,
            camera,
            camera_uniform,
            camera_buffer,
            camera_bind_group,
            depth_view,
            chunk_manager,
            chunk_meshes,
            render_regions: std::collections::HashMap::new(),
            compaction_pending_region: None,
            section_storage_compaction_queue: std::collections::VecDeque::new(),
            section_storage_compaction_queued: std::collections::HashSet::new(),
            terrain_worker_tx,
            terrain_worker_rx,
            pending_worker_results: std::collections::VecDeque::new(),
            scheduler: crate::chunk_schedule::ChunkStreamingScheduler::new(),
            section_scheduler: crate::chunk_schedule::SectionMeshScheduler::new(),
            chunk_load_in_flight: std::collections::HashMap::new(),
            chunk_lifetimes,
            next_chunk_lifetime,
            terrain_generation: 0,
            los_world_revision: 0,
            submitted_terrain_triangles: 0,
            submitted_terrain_draw_calls: 0,
            visible_chunk_count: 0,
            prev_player_position: player_physics.position,
            sim_accumulator: 0.0,
            player_physics,
            keys,
            jump_taps: DoubleTapTracker::default(),
            texture_atlas,
            crosshair_buffer,
            is_paused: false,
            mouse_ndc: [0.0, 0.0],
            sensitivity: settings.sensitivity,
            ui_pipeline,
            ui_line_pipeline,
            ui_vertex_buffer,
            ui_line_vertex_buffer,
            ui_textured_pipeline,
            ui_textured_vertex_buffer,
            num_ui_vertices: 0,
            num_ui_line_vertices: 0,
            num_ui_textured_vertices: 0,
            game_mode,
            inventory,
            recipe_manager: RecipeManager::new(),
            left_mouse_pressed: false,
            hand_swing_started_at: 0.0,
            hand_swing_until: 0.0,
            mining_target: None,
            mining_progress: 0.0,
            mining_held: None,
            mining_cancel_sent: false,
            crack_vertex_buffer,
            crack_index_buffer,
            player_state,
            world_time,
            show_debug,
            camera_perspective: CameraPerspective::FirstPerson,
            entity_manager: crate::entity::EntityManager::new(),
            presented_fishing_hook_entity: None,
            mob_instanced_pipeline,
            particle_instanced_pipeline,
            mob_cuboid_proto_vbuf,
            mob_cuboid_proto_ibuf,
            mob_quad_proto_vbuf,
            mob_quad_proto_ibuf,
            particle_proto_vbuf,
            particle_proto_ibuf,
            frame_ring_index: 0,
            mob_cuboid_instance_buffers,
            mob_quad_instance_buffers,
            particle_instance_buffers,
            frame_resource_pool: crate::gpu_frame_resources::FrameResourcePool::with_initial(
                3,
                [(), (), ()],
            ),
            gpu_completion_tx,
            gpu_completion_rx,
            next_gpu_submission_id: 1,
            mob_cuboid_instances_scratch: Vec::with_capacity(1024),
            mob_quad_instances_scratch: Vec::with_capacity(512),
            particle_instances_scratch: Vec::with_capacity(4096),
            mob_cuboid_num_instances: 0,
            mob_quad_num_instances: 0,
            mob_num_indices: 0,
            hand_pipeline,
            hand_vertex_buffer,
            hand_index_buffer,
            hand_num_indices: 0,
            hand_camera_buffer,
            hand_camera_bind_group,
            particles,
            particle_num_indices: 0,
            torch_smoke_timer: 0.0,
            total_time: 0.0,
            audio_manager,
            resource_pack_manager,
            model_registry,
            font_source,
            translation_catalog,
            footstep_accumulator: 0.0,
            was_on_ground: false,
            is_saving: false,
            save_error: None,
            is_sprinting: false,
            sprint_toggle_latched: false,
            sneak_toggle_latched: false,
            last_ctrl_pressed: false,
            last_shift_pressed: false,
            base_fov,
            w_click_timer: 0.0,
            last_w_pressed: false,
            debug_frame_time_accumulator: 0.0,
            debug_frame_samples: 0,
            debug_fps: 0.0,
            debug_frame_ms: 0.0,
            perf_recorder: crate::perf::PerfRecorder::new(),
            perf_summaries:
                crate::perf::PerfRecorder::<{ crate::perf::DEFAULT_HISTORY_CAPACITY }>::new()
                    .snapshot(),
            perf_counters: crate::perf::PerfCounters::default(),
            frame_perf_samples: std::collections::VecDeque::with_capacity(240),
            next_perf_frame_id: 0,
            gpu_upload_time_frame: Duration::ZERO,
            lighting_time_frame: Duration::ZERO,
            lighting_scopes_frame: crate::perf::LightingPerfSample::new(),
            gpu_upload_scopes_frame: crate::perf::GpuUploadPerfSample::new(),
            gpu_timestamp_query_set,
            gpu_timestamp_resolve_buffer,
            gpu_timestamp_readback_slots,
            gpu_pass_timings_ns: [0; 7],
            gpu_pass_timings_valid: false,
            gpu_pass_timing_submission_tag: None,
            gpu_timestamps_supported,
            gpu_timestamps_inside_passes,
            terrain_candidates_scratch: Vec::with_capacity(256),
            terrain_draw_plan_scratch: crate::chunk_render::DrawPlan::default(),
            entity_los_manager: crate::culling::EntityLosManager::new(),
            visible_sections_scratch: std::collections::HashSet::new(),
            section_visibility_scratch: crate::culling::SectionVisibilityScratch::with_capacity(
                4096, 4096,
            ),
            hand_vertices_scratch: Vec::with_capacity(256),
            hand_indices_scratch: Vec::with_capacity(384),
            last_hand_mesh_key: None,
            ui_vertices_scratch: Vec::with_capacity(2048),
            ui_line_vertices_scratch: Vec::with_capacity(4096),
            debug_str_scratch: String::with_capacity(128),
            active_station: None,
            container_target: None,
            container_is_double: false,
            enchanting: crate::enchantment::EnchantingState::default(),
            brewing: crate::brewing::BrewingStandState::default(),
            anvil: crate::enchantment::AnvilState::default(),
            potion_effects: crate::brewing::EffectManager::default(),
            recipe_book_open: false,
            recipe_book_search: String::new(),
            weather,
            difficulty: launch.difficulty,
            world_rules,
            world_type,
            generate_structures,
            bonus_chest,
            cheats_enabled,
            world_seed,
            world_spawn,
            settings,
            end_flash_time: 0.0,
            current_dimension,
            portal_contact_time: 0.0,
            portal_cooldown: 0.0,
            advancement_manager,
            advancement_gui,
            role,
            embedded_runtime,
            network,
            network_staging: NetworkStaging::default(),
            network_ready: !is_client,
            local_player_id: embedded_session_id,
            remote_players: std::collections::HashMap::new(),
            replicated_entities: std::collections::HashMap::new(),
            client_player_health_sequence: 0,
            client_player_effect_sequence: 0,
            client_session_projection: None,
            network_status: is_client.then(|| "CONNECTING TO SERVER...".to_string()),
            chat_messages: std::collections::VecDeque::new(),
            chat_input: String::new(),
            is_chat_open: false,
            connection_lost: false,
            network_position_timer: 0.0,
            network_pose_sequence: 0,
            active_merchant_villager_id: None,
            active_merchant_offers: Vec::new(),
            active_merchant_profession: crate::village::poi::VillagerProfession::Unemployed,
            active_merchant_level: crate::village::trade::VillagerLevel::Novice,
            active_merchant_xp: 0,
            network_time: 0.0,
            pending_chunk_payloads: std::collections::HashMap::new(),
            pending_block_changes: std::collections::HashMap::new(),
            client_chunk_revisions: std::collections::HashMap::new(),
        };

        // Apply the centralized mode policy to the freshly loaded player (in
        // particular Spectator noclip/flight) before the first simulation tick.
        let initial_mode = state.game_mode;
        state.set_game_mode(initial_mode);

        let initial_mesh_coords: Vec<_> = state.chunk_meshes.keys().copied().collect();
        state.invalidate_chunk_meshes(initial_mesh_coords, DependencyReason::ChunkLoad);

        state
    }

    fn sync_audio_settings(&mut self) {
        self.settings.clamp_audio_volumes();
        self.audio_manager
            .set_volume(self.settings.effective_sound_volume());
        self.audio_manager
            .set_weather_volume(self.settings.weather_volume);
    }

    pub fn save_settings(&mut self) {
        self.settings.fov = self.base_fov;
        self.settings.sensitivity = self.sensitivity;
        self.settings.render_distance = self.chunk_manager.render_distance;
        self.sync_audio_settings();
        self.settings.save();
    }

    /// Presentation topology derived from role + in-process runtime presence.
    pub fn presentation_topology(&self) -> PresentationTopology {
        PresentationTopology::from(&self.role, self.embedded_runtime.is_some())
    }

    /// True when this presentation root is backed by the shared headless
    /// runtime. Renderer-side simulation and persistence stay disabled while
    /// this is set; the runtime is the only authority owner.
    pub(crate) fn has_in_process_runtime(&self) -> bool {
        self.embedded_runtime.is_some()
    }

    /// Advance the in-process authority by one fixed 20 Hz tick.  Dedicated
    /// mode has no `State`, while a network client correctly returns `None`.
    pub fn tick_authority_boundary(
        &mut self,
    ) -> Option<crate::authority::contract::AuthoritySnapshot> {
        let sequence = self.network_pose_sequence;
        let position = self.player_physics.position;
        let yaw = self.camera.yaw;
        let pitch = self.camera.pitch;
        let runtime = self.embedded_runtime.as_mut()?;
        let session_id = runtime.session_id();
        let _ = runtime.queue_position(sequence, position, yaw, pitch);
        let output = match runtime.tick() {
            Ok(output) => output,
            Err(error) => {
                self.save_error
                    .get_or_insert_with(|| format!("embedded runtime tick failed: {error}"));
                return None;
            }
        };
        for event in output.presentation_events {
            self.project_runtime_presentation_event(event, session_id);
        }
        self.project_authority_mutations(&output.snapshot.mutations);
        self.project_authority_sessions(&output.snapshot.session_updates);
        self.world_time.ticks = output.snapshot.tick;
        Some(output.snapshot)
    }

    /// Convert the runtime's target-aware presentation lane into the existing
    /// renderer staging path.  The runtime remains the sole mutation owner;
    /// these handlers only update presentation caches after a fixed tick.
    fn project_runtime_presentation_event(
        &mut self,
        event: crate::server_runtime::ProjectionEvent,
        session_id: crate::network::protocol::PlayerId,
    ) {
        use crate::network::protocol::Packet;
        use crate::server_runtime::ProjectionDest;

        let ProjectionDest::Session(target) = event.dest else {
            return;
        };
        if target != session_id {
            return;
        }

        let inbound = match event.packet {
            Packet::GameplayResponse { response, .. } => {
                // Game mode lives on SessionContract rather than inside the
                // legacy SessionGameplayWire.  Refresh it on every accepted
                // embedded response so an accepted /gamemode command is
                // immediately visible to the presentation.
                if matches!(
                    response.outcome,
                    crate::network::protocol::GameplayOutcome::Accepted { .. }
                ) {
                    if let Some(mode) = self
                        .embedded_runtime
                        .as_ref()
                        .and_then(EmbeddedRuntimeBridge::session_game_mode)
                    {
                        self.set_game_mode(mode);
                    }
                }
                Some(NetworkInbound::GameplayResponse { response })
            }
            Packet::BlockChange {
                dimension,
                revision,
                x,
                y,
                z,
                block,
                state,
                raw_fluid,
                ..
            } => Some(NetworkInbound::AuthoritativeBlockChange {
                dimension,
                revision,
                x,
                y,
                z,
                block,
                state,
                raw_fluid,
            }),
            Packet::ChunkData {
                dimension,
                cx,
                cz,
                revision,
                min_section_y,
                section_count,
                blocks,
                block_states,
                fluid_levels,
                block_entities,
                ..
            } => Some(NetworkInbound::ChunkData {
                dimension,
                cx,
                cz,
                revision,
                min_section_y,
                section_count,
                blocks,
                block_states,
                fluid_levels,
                block_entities,
            }),
            Packet::BlockEntityDelta {
                dimension,
                revision,
                x,
                y,
                z,
                entity,
                ..
            } => Some(NetworkInbound::BlockEntityDelta {
                dimension,
                revision,
                x,
                y,
                z,
                entity,
            }),
            Packet::EntitySpawn {
                dimension,
                sequence,
                state,
                ..
            } => Some(NetworkInbound::EntitySpawn {
                dimension,
                sequence,
                state,
            }),
            Packet::EntityState {
                dimension,
                sequence,
                state,
                ..
            } => Some(NetworkInbound::EntityState {
                dimension,
                sequence,
                state,
            }),
            Packet::EntityDespawn {
                dimension,
                sequence,
                entity_id,
                ..
            } => Some(NetworkInbound::EntityDespawn {
                dimension,
                sequence,
                entity_id,
            }),
            Packet::PlayerSessionUpdate {
                sequence,
                player_id,
                dimension,
                state,
                ..
            } => Some(NetworkInbound::PlayerSessionUpdate {
                sequence,
                player_id,
                dimension,
                state,
            }),
            Packet::PlayerEffect {
                sequence,
                player_id,
                effects,
                ..
            } => Some(NetworkInbound::PlayerEffect {
                sequence,
                player_id,
                effects,
            }),
            Packet::PlayerPosition {
                id,
                sequence,
                sender_time_millis,
                x,
                y,
                z,
                yaw,
                pitch,
                ..
            } => Some(NetworkInbound::PlayerPosition {
                id,
                sequence,
                sender_time_millis,
                x,
                y,
                z,
                yaw,
                pitch,
            }),
            Packet::ContainerOpenResult {
                dimension,
                success,
                x,
                y,
                z,
                slots,
                revision,
                ..
            } => Some(NetworkInbound::ContainerOpenResult {
                dimension,
                success,
                x,
                y,
                z,
                slots,
                revision,
            }),
            Packet::ContainerClickResult {
                dimension,
                success,
                slot_index,
                slot,
                dragged,
                ..
            } => Some(NetworkInbound::ContainerClickResult {
                dimension,
                success,
                slot_index,
                slot,
                dragged,
            }),
            Packet::ContainerSlotUpdate {
                dimension,
                revision,
                x,
                y,
                z,
                slot_index,
                slot,
                ..
            } => Some(NetworkInbound::ContainerSlotUpdate {
                dimension,
                revision,
                x,
                y,
                z,
                slot_index,
                slot,
            }),
            Packet::ContainerClose {
                dimension,
                x,
                y,
                z,
                ..
            } => Some(NetworkInbound::ContainerClose {
                id: target,
                dimension,
                x,
                y,
                z,
            }),
            Packet::PlayerRespawnResult {
                position,
                dimension,
                ..
            } => Some(NetworkInbound::PlayerRespawnResult {
                position,
                dimension,
            }),
            Packet::DimensionTransfer {
                dimension,
                position,
                ..
            } => Some(NetworkInbound::DimensionTransfer {
                dimension,
                position,
            }),
            Packet::WorldRulesSync { rules, .. } => {
                Some(NetworkInbound::WorldRulesSync { rules })
            }
            Packet::TimeSync {
                ticks,
                weather,
                weather_remaining_ticks,
                ..
            } => Some(NetworkInbound::TimeSync {
                ticks,
                weather,
                weather_remaining_ticks,
            }),
            _ => None,
        };
        if let Some(inbound) = inbound {
            self.handle_single_network_event(inbound);
        }
    }

    fn session_slot_from_stack(
        stack: Option<crate::inventory::ItemStack>,
    ) -> Option<crate::authority::contract::SessionInventorySlot> {
        let stack = stack?;
        if stack.count == 0 || stack.count > u32::from(u16::MAX) {
            return None;
        }
        Some(crate::authority::contract::SessionInventorySlot::from_wire(
            crate::network::protocol::ItemWire::from_stack(&stack),
            stack.can_break,
            stack.can_place_on,
        ))
    }

    fn stack_from_session_slot(
        slot: crate::authority::contract::SessionInventorySlot,
    ) -> Option<crate::inventory::ItemStack> {
        slot.to_stack()
    }

    fn local_inventory_writeback(
        &self,
    ) -> (
        [Option<crate::authority::contract::SessionInventorySlot>;
            crate::authority::contract::SESSION_INVENTORY_SLOTS],
        Option<crate::authority::contract::SessionInventorySlot>,
        u8,
    ) {
        use crate::authority::contract::SESSION_INVENTORY_SLOTS;
        let mut inventory = [None; SESSION_INVENTORY_SLOTS];
        let mut index = 0;
        for stack in self
            .inventory
            .hotbar
            .iter()
            .chain(self.inventory.main.iter())
            .chain(self.inventory.armor.iter())
        {
            if index >= SESSION_INVENTORY_SLOTS - 1 {
                break;
            }
            inventory[index] = Self::session_slot_from_stack(*stack);
            index += 1;
        }
        inventory[index] = Self::session_slot_from_stack(self.inventory.offhand);
        (
            inventory,
            Self::session_slot_from_stack(self.inventory.dragged),
            self.inventory.selected.min(8) as u8,
        )
    }

    pub(crate) fn sync_authority_gameplay_from_local(&mut self) {
        let (inventory, cursor, selected_hotbar_slot) = self.local_inventory_writeback();
        let Some(runtime) = self.embedded_runtime.as_mut() else {
            return;
        };
        let _ = runtime.sync_local_inventory(inventory, cursor, selected_hotbar_slot);
    }

    fn project_authority_sessions(
        &mut self,
        updates: &[crate::authority::contract::SessionGameplayUpdate],
    ) {
        let Some(session_id) = self
            .embedded_runtime
            .as_ref()
            .map(EmbeddedRuntimeBridge::session_id)
        else {
            return;
        };
        let Some(update) = updates.iter().find(|update| update.player_id == session_id) else {
            return;
        };
        self.project_authority_session(update);
    }

    fn project_authority_session(
        &mut self,
        update: &crate::authority::contract::SessionGameplayUpdate,
    ) {
        let gameplay = update.state;
        if !self.accept_session_projection(update.dimension, gameplay.revision, gameplay.revision) {
            return;
        }
        self.project_gameplay_state(update.dimension, gameplay);
    }

    /// Shared one-way presentation projection for embedded and socket session
    /// updates. Ordering is checked before any renderer-owned cache is touched:
    /// sequence orders dimension transfers, while revision orders snapshots in
    /// one dimension. The authority remains the sole writer of gameplay state.
    fn accept_session_projection(&mut self, dimension: u8, sequence: u64, revision: u64) -> bool {
        if let Some((latest_dimension, latest_sequence, latest_revision)) =
            self.client_session_projection
        {
            if sequence <= latest_sequence {
                return false;
            }
            if latest_dimension == dimension && revision <= latest_revision {
                return false;
            }
        }
        self.client_session_projection = Some((dimension, sequence, revision));
        true
    }

    fn project_gameplay_state(
        &mut self,
        dimension: u8,
        gameplay: crate::authority::contract::SessionGameplayState,
    ) {
        self.player_state.health = gameplay.health_milli as f32 / 1000.0;
        self.player_state.max_health = gameplay.max_health_milli as f32 / 1000.0;
        self.player_state.hunger = gameplay.hunger_milli as f32 / 1000.0;
        self.player_state.saturation = gameplay.saturation_milli as f32 / 1000.0;
        self.player_state.is_dead = gameplay.is_dead;
        self.player_state.death_reason = gameplay.death_source.and_then(DamageSource::from_wire);
        self.player_state.invulnerable_time = gameplay.invulnerability_ticks as f32 / 20.0;
        self.player_state.experience = gameplay.experience;
        self.player_state.experience_level = gameplay.experience_level;
        self.player_state.attack_cooldown_ticks = u32::from(gameplay.attack_cooldown_ticks);
        self.player_state.attack_cooldown_max_ticks = self
            .player_state
            .attack_cooldown_max_ticks
            .max(self.player_state.attack_cooldown_ticks);
        self.player_state.shield_disable_ticks = u32::from(gameplay.shield_cooldown_ticks);
        self.enchanting.seed = gameplay.enchant_seed.min(u64::from(u32::MAX)) as u32;
        if gameplay.shield_active {
            self.player_state.using_item = Some(crate::player::UsingItemState {
                hand: crate::player::Hand::MainHand,
                action: crate::player::ItemUseAction::Block,
                item: Item::Shield,
                slot: crate::player::HandSlot::MainHand(self.inventory.selected),
                ticks_held: 0,
                max_ticks: None,
            });
        } else if self
            .player_state
            .using_item
            .as_ref()
            .is_some_and(|using| using.action == crate::player::ItemUseAction::Block)
        {
            self.player_state.using_item = None;
        }
        self.player_physics.velocity = Vec3::new(
            gameplay.velocity_milli[0] as f32 / 1000.0,
            gameplay.velocity_milli[1] as f32 / 1000.0,
            gameplay.velocity_milli[2] as f32 / 1000.0,
        );
        self.inventory.selected = usize::from(gameplay.selected_hotbar_slot.min(8));
        let mut index = 0;
        for slot in self
            .inventory
            .hotbar
            .iter_mut()
            .chain(self.inventory.main.iter_mut())
            .chain(self.inventory.armor.iter_mut())
        {
            *slot = gameplay.inventory[index].and_then(Self::stack_from_session_slot);
            index += 1;
        }
        self.inventory.offhand = gameplay.inventory[index].and_then(Self::stack_from_session_slot);
        if let Some(mining) = gameplay.mining {
            if !self.mining_cancel_sent {
                self.mining_target = Some(Vec3::new(
                    mining.target[0] as f32,
                    mining.target[1] as f32,
                    mining.target[2] as f32,
                ));
                self.mining_progress = f32::from(mining.progress_milli) / 1000.0;
                self.mining_held = mining.held;
            }
        } else {
            self.mining_target = None;
            self.mining_progress = 0.0;
            self.mining_held = None;
            self.mining_cancel_sent = false;
        }

        if let Some(dimension) = crate::dimension::Dimension::from_wire(dimension) {
            self.reset_presented_dimension(dimension);
        }

        self.presented_fishing_hook_entity = match gameplay.fishing_hook {
            Some(hook)
                if crate::fishing::FishingHookStage::from_wire(hook.stage).is_some() =>
            {
                Some(hook.entity_id)
            }
            _ => None,
        };

        if let Some(brew) = gameplay.brew {
            self.active_station = Some(StationKind::Brewing);
            self.container_target = Some((brew.station[0], brew.station[1], brew.station[2]));
            self.brewing.progress = (10.0 - brew.remaining_ticks as f32 / 20.0).clamp(0.0, 10.0);
            self.brewing.ingredient = Self::stack_from_session_slot(
                crate::authority::contract::SessionInventorySlot::from_wire(
                    brew.ingredient.expected.item,
                    brew.ingredient.expected.can_break,
                    brew.ingredient.expected.can_place_on,
                ),
            );
            self.brewing.bottles = brew.bottles.map(|source| {
                source.and_then(|source| {
                    Self::stack_from_session_slot(
                        crate::authority::contract::SessionInventorySlot::from_wire(
                            source.expected.item,
                            source.expected.can_break,
                            source.expected.can_place_on,
                        ),
                    )
                })
            });
        } else if self.active_station == Some(StationKind::Brewing) {
            self.active_station = None;
            self.container_target = None;
        }
        if gameplay.is_dead {
            self.clear_movement_input();
            self.sync_cursor_mode();
        }
    }

    /// Apply authority output to the renderer-owned cache.  This is a one-way
    /// projection: the GPU-side chunk manager is never consulted by the core
    /// and never performs an authoritative mutation for Singleplayer/Host.
    fn project_authority_mutations(
        &mut self,
        mutations: &[crate::authority::contract::WorldMutation],
    ) {
        let mut dirty_chunks = std::collections::HashSet::new();
        for mutation in mutations {
            if mutation.dimension != self.current_dimension as u8 {
                continue;
            }
            let Some(block) = BlockType::from_wire(mutation.block) else {
                continue;
            };
            let (x, y, z) = mutation.position;
            let previous_block = self.chunk_manager.get_block(x, y, z);
            let previous_state = self.chunk_manager.get_block_state(x, y, z);
            self.play_chest_state_edge(
                (x, y, z),
                previous_block,
                previous_state,
                block,
                mutation.state,
            );
            if let Some(dirty) =
                apply_synced_block_change(&mut self.chunk_manager, x, y, z, block, mutation.state)
            {
                dirty_chunks.extend(dirty);
            }
            // C1's bounded runtime output does not carry block-entity or
            // container payloads yet.  Keep those renderer caches untouched
            // until a typed runtime projection event exists; never query or
            // mutate a second local authority as a fallback.
        }
        if !dirty_chunks.is_empty() {
            self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::Block);
        }
    }

    fn play_chest_state_edge(
        &self,
        position: (i32, i32, i32),
        previous_block: BlockType,
        previous_state: u8,
        block: BlockType,
        state: u8,
    ) {
        if !matches!(previous_block, BlockType::Chest | BlockType::EndCityChest)
            || !matches!(block, BlockType::Chest | BlockType::EndCityChest)
        {
            return;
        }
        let previous = crate::world::BlockState::decode(previous_state);
        let next = crate::world::BlockState::decode(state);
        if previous.is_open == next.is_open {
            return;
        }
        // A double chest emits two block-state mutations. Pick the
        // lexicographically first half so one edge produces one sound.
        if let Some(partner) =
            crate::block_entity::double_chest_partner(&self.chunk_manager, position)
        {
            if position > partner {
                return;
            }
        }
        self.audio_manager.play_sound(if next.is_open {
            crate::audio::SoundId::ChestOpen
        } else {
            crate::audio::SoundId::ChestClose
        });
    }

    pub fn submit_authority_request(
        &mut self,
        request: crate::network::protocol::GameplayRequest,
    ) -> Option<crate::network::protocol::GameplayResponse> {
        if let Some(runtime) = self.embedded_runtime.as_mut() {
            if let Err(error) = runtime.queue_request(request) {
                self.save_error
                    .get_or_insert_with(|| format!("embedded runtime input rejected: {error}"));
            }
            // ACKs are deliberately observed only from `tick_with_output`; a
            // queued request has no synchronous response to project.
            return None;
        }
        if matches!(self.role, MultiplayerRole::Client { .. }) {
            self.network.request_gameplay(request);
        }
        None
    }

    /// Submit a local input envelope through the same authority path used by
    /// a listen/dedicated transport.  Presentation code must not mutate the
    /// renderer cache first and then ask the core for an ACK: a rejected or
    /// stale request is intentionally a no-op on the client.
    fn submit_local_authority_operation(
        &mut self,
        operation: crate::network::protocol::GameplayOperation,
    ) -> Option<crate::network::protocol::GameplayResponse> {
        self.submit_authority_request(crate::network::protocol::GameplayRequest {
            request_id: 0,
            client_sequence: 0,
            session_id: 0,
            dimension: self.current_dimension as u8,
            client_revision: self
                .embedded_runtime
                .as_ref()
                .map(|runtime| runtime.revision_for_dimension(self.current_dimension))
                .or_else(|| {
                    self.client_session_projection
                        .map(|(_, _, revision)| revision)
                })
                .unwrap_or_default(),
            operation,
        })
    }

    fn submit_local_authority_container_action(
        &mut self,
        position: (i32, i32, i32),
        action: crate::network::protocol::ContainerAction,
        slot: u16,
    ) -> bool {
        let operation = crate::network::protocol::GameplayOperation::Container {
            action: action.to_wire(),
            x: position.0,
            y: position.1,
            z: position.2,
            slot,
        };
        let Some(response) = self.submit_local_authority_operation(operation) else {
            return false;
        };
        if !matches!(
            response.outcome,
            crate::network::protocol::GameplayOutcome::Accepted { .. }
        ) {
            return false;
        }
        match action {
            crate::network::protocol::ContainerAction::Open => false,
            crate::network::protocol::ContainerAction::Close => {
                self.container_target = None;
                self.container_is_double = false;
                self.inventory.is_open = false;
                self.active_station = None;
                self.sync_cursor_mode();
                true
            }
        }
    }

    /// Cancel the live authority mining latch. `mining_cancel_sent` still
    /// lives in `submit_local_authority_block_action` so duplicate CancelBreak
    /// packets are suppressed.
    fn cancel_authority_break(&mut self) {
        if let Some(previous) = self.mining_target {
            let _ = self.submit_local_authority_block_action(
                crate::network::protocol::BlockActionKind::CancelBreak,
                previous.x as i32,
                previous.y as i32,
                previous.z as i32,
                [0, 0, 0],
                BlockType::Air,
            );
        }
        self.mining_target = None;
        self.mining_held = None;
        self.mining_progress = 0.0;
    }

    fn submit_local_authority_block_action(
        &mut self,
        action: crate::network::protocol::BlockActionKind,
        x: i32,
        y: i32,
        z: i32,
        face: [i8; 3],
        block: BlockType,
    ) -> Option<crate::network::protocol::GameplayResponse> {
        if matches!(
            action,
            crate::network::protocol::BlockActionKind::CancelBreak
        ) {
            if self.mining_cancel_sent {
                return None;
            }
            self.mining_cancel_sent = true;
        } else if matches!(
            action,
            crate::network::protocol::BlockActionKind::StartBreak
        ) {
            self.mining_cancel_sent = false;
        }
        let look = Vec3::new(
            self.camera.yaw.cos() * self.camera.pitch.cos(),
            self.camera.pitch.sin(),
            self.camera.yaw.sin() * self.camera.pitch.cos(),
        )
        .normalize_or_zero();
        let look_milli = [
            (look.x * 1_000.0).round() as i16,
            (look.y * 1_000.0).round() as i16,
            (look.z * 1_000.0).round() as i16,
        ];
        let hand = 0;
        let held = self.inventory.hotbar[self.inventory.selected].map(|stack| {
            let slot = crate::authority::contract::SessionInventorySlot::from_wire(
                crate::network::protocol::ItemWire::from_stack(&stack),
                stack.can_break,
                stack.can_place_on,
            );
            crate::network::protocol::SessionSlotWire::from(slot)
        });
        self.submit_authority_request(crate::network::protocol::GameplayRequest {
            request_id: 0,
            client_sequence: 0,
            session_id: 0,
            dimension: self.current_dimension as u8,
            client_revision: self
                .embedded_runtime
                .as_ref()
                .map(|runtime| runtime.revision_for_dimension(self.current_dimension))
                .or_else(|| {
                    self.client_session_projection
                        .map(|(_, _, revision)| revision)
                })
                .unwrap_or_default(),
            operation: crate::network::protocol::GameplayOperation::BlockAction {
                action,
                x,
                y,
                z,
                face,
                hand,
                held: if matches!(
                    action,
                    crate::network::protocol::BlockActionKind::CancelBreak
                ) {
                    None
                } else {
                    held
                },
                block: if matches!(
                    action,
                    crate::network::protocol::BlockActionKind::Place
                        | crate::network::protocol::BlockActionKind::IgnitePortal
                        | crate::network::protocol::BlockActionKind::InsertEnderEye
                        | crate::network::protocol::BlockActionKind::EnterPortal
                ) {
                    block.to_wire()
                } else {
                    BlockType::Air.to_wire()
                },
                look_milli,
            },
        })
    }

    fn selected_mining_held(&self) -> Option<crate::network::protocol::SessionSlotWire> {
        self.inventory.hotbar[self.inventory.selected].map(|stack| {
            let slot = crate::authority::contract::SessionInventorySlot::from_wire(
                crate::network::protocol::ItemWire::from_stack(&stack),
                stack.can_break,
                stack.can_place_on,
            );
            crate::network::protocol::SessionSlotWire::from(slot)
        })
    }

    /// Route the command domains already understood by `ServerWorld` through
    /// the in-process authority. Unsupported legacy command domains continue
    /// through the existing presentation adapter until the remaining Phase A
    /// cutover lands; they must never be reported as an authority ACK here.
    fn submit_local_authority_command(
        &mut self,
        command: &str,
    ) -> Option<crate::network::protocol::GameplayResponse> {
        self.submit_authority_request(crate::network::protocol::GameplayRequest {
            request_id: 0,
            client_sequence: 0,
            session_id: 0,
            dimension: self.current_dimension as u8,
            client_revision: self
                .embedded_runtime
                .as_ref()
                .map(|runtime| runtime.revision_for_dimension(self.current_dimension))
                .unwrap_or_default(),
            operation: crate::network::protocol::GameplayOperation::Command {
                command: command.to_string(),
            },
        })
    }

    fn can_place_block_at(&self, x: i32, y: i32, z: i32, block: BlockType) -> bool {
        let policy = self.game_mode_policy();
        if (!policy.can_place && self.game_mode != GameMode::Adventure)
            || !policy.can_place_stack(
                self.inventory.hotbar[self.inventory.selected].as_ref(),
                block,
            )
        {
            return false;
        }
        matches!(
            placement_decision_for_players(
                block,
                (x, y, z),
                self.player_physics.get_aabb(),
                self.remote_players.values(),
            ),
            BlockPlacementDecision::Allowed
        )
    }

    fn can_break_current_block(&self, block: BlockType) -> bool {
        can_break_block(block, self.game_mode)
            && self.game_mode_policy().can_break_stack(
                self.inventory.hotbar[self.inventory.selected].as_ref(),
                block,
            )
    }

    fn drain_network_events(&mut self) {
        // Transport draining is bounded by `NetworkHandle`; every event it
        // yields is classified immediately so an apply-budget boundary can
        // never demote a latest-wins event into the reliable FIFO.
        const MAX_EVENTS: usize = NETWORK_MAX_EVENTS_PER_PASS;
        const MAX_BYTES: usize = NETWORK_MAX_BYTES_PER_PASS;
        const MAX_TIME: Duration = NETWORK_MAX_TIME_PER_PASS;
        for event in self.network.drain_inbound() {
            self.network_staging.stage(event);
        }

        let apply_started = Instant::now();
        let mut applied = 0usize;
        let mut applied_bytes = 0usize;
        while applied < MAX_EVENTS
            && applied_bytes < MAX_BYTES
            && apply_started.elapsed() < MAX_TIME
        {
            let remaining_bytes = MAX_BYTES.saturating_sub(applied_bytes);
            let Some((event, event_bytes)) = self.network_staging.pop_next_if_fits(remaining_bytes)
            else {
                break;
            };
            applied_bytes = applied_bytes.saturating_add(event_bytes);
            applied += 1;
            self.handle_single_network_event(event);
        }
        self.perf_counters.network_inbound_reliable_pending =
            self.network_staging.reliable_len() as u64;
        self.perf_counters.network_inbound_reliable_bytes = self.network_staging.reliable_bytes();
        self.perf_counters.network_inbound_latest_pending =
            self.network_staging.latest_len() as u64;
        self.perf_counters.network_inbound_latest_bytes = self.network_staging.latest_bytes();
    }

    fn clear_replicated_entities(&mut self) {
        let local_ids: Vec<_> = self
            .replicated_entities
            .drain()
            .map(|(_, replicated)| replicated.local_entity_id)
            .collect();
        for local_id in local_ids {
            self.entity_manager.remove_by_id(local_id);
        }
    }

    fn apply_replicated_entity_state(
        &mut self,
        dimension_wire: u8,
        sequence: u64,
        state: crate::network::protocol::EntityStateWire,
    ) {
        if !self.presentation_topology().is_join_client()
            || crate::dimension::Dimension::from_wire(dimension_wire)
                != Some(self.current_dimension)
        {
            return;
        }
        let Some(entity_type) = crate::entity::EntityType::from_wire(state.entity_type) else {
            return;
        };
        if !is_replicated_entity_type(entity_type) {
            return;
        }

        let needs_spawn = self
            .replicated_entities
            .get(&state.entity_id)
            .and_then(|replicated| self.entity_manager.get_by_id(replicated.local_entity_id))
            .map_or(true, |entity| entity.entity_type != entity_type);
        if needs_spawn {
            if let Some(previous) = self.replicated_entities.remove(&state.entity_id) {
                self.entity_manager.remove_by_id(previous.local_entity_id);
            }
            let local_entity_id = self
                .entity_manager
                .spawn(entity_type, Vec3::from_array(state.position));
            self.replicated_entities
                .insert(state.entity_id, ReplicatedEntityState::new(local_entity_id));
        }

        let snapped = self
            .replicated_entities
            .get_mut(&state.entity_id)
            .is_some_and(|replicated| replicated.push(state, sequence, self.network_time));
        if snapped {
            self.perf_counters.prediction_rollback =
                self.perf_counters.prediction_rollback.saturating_add(1);
        }
        if let Some(local_id) = self
            .replicated_entities
            .get(&state.entity_id)
            .map(|replicated| replicated.local_entity_id)
        {
            if let Some(entity) = self.entity_manager.get_by_id_mut(local_id) {
                apply_entity_wire_state(entity, state);
            }
        }
    }

    fn apply_replicated_entity_despawn(
        &mut self,
        dimension_wire: u8,
        sequence: u64,
        entity_id: u64,
    ) {
        if !self.presentation_topology().is_join_client()
            || crate::dimension::Dimension::from_wire(dimension_wire)
                != Some(self.current_dimension)
        {
            return;
        }
        if self
            .replicated_entities
            .get(&entity_id)
            .and_then(|replicated| replicated.snapshots.back())
            .is_some_and(|latest| sequence <= latest.sequence)
        {
            return;
        }
        if let Some(replicated) = self.replicated_entities.remove(&entity_id) {
            self.entity_manager.remove_by_id(replicated.local_entity_id);
        }
    }

    fn update_replicated_entity_interpolation(&mut self) {
        if !self.presentation_topology().is_join_client() {
            return;
        }
        let target = self.network_time - ENTITY_INTERPOLATION_DELAY;
        let samples: Vec<_> = self
            .replicated_entities
            .values()
            .filter_map(|replicated| {
                replicated
                    .sample(target)
                    .map(|state| (replicated.local_entity_id, state))
            })
            .collect();
        let moved_ids: Vec<_> = samples.iter().map(|(local_id, _)| *local_id).collect();
        for (local_id, state) in samples {
            if let Some(entity) = self.entity_manager.get_by_id_mut(local_id) {
                apply_entity_wire_state(entity, state);
            }
        }
        self.entity_manager.sync_entity_positions(&moved_ids);
    }

    fn update_network_position(&mut self, dt: f32) {
        if !self.network_ready || matches!(&self.network, NetworkHandle::None) {
            return;
        }
        self.network_position_timer += dt;
        if self.network_position_timer < 0.05 {
            return;
        }
        self.network_position_timer %= 0.05;
        self.network_pose_sequence = self.network_pose_sequence.wrapping_add(1);
        let sender_time_millis = (self.network_time * 1000.0).round() as u64;
        self.network.send_position(
            self.network_pose_sequence,
            sender_time_millis,
            self.player_physics.position,
            self.camera.yaw,
            self.camera.pitch,
        );
    }

    pub fn shutdown_network(&mut self) {
        if let Some(runtime) = self.embedded_runtime.as_mut() {
            if let Err(error) = runtime.shutdown() {
                self.save_error
                    .get_or_insert_with(|| format!("embedded runtime shutdown failed: {error}"));
            }
        }
        self.network.shutdown();
    }

    pub fn clear_movement_input(&mut self) {
        self.keys = KeyState::default();
        self.jump_taps.reset();
        self.left_mouse_pressed = false;
        self.mining_target = None;
        self.mining_progress = 0.0;
    }

    pub fn camera_look_allowed(&self) -> bool {
        allows_camera_look(
            self.is_paused,
            self.inventory.is_open,
            self.advancement_gui.is_open,
            self.is_chat_open,
            self.connection_lost,
            self.player_state.is_dead,
            self.window.has_focus(),
        )
    }

    pub fn sync_cursor_mode(&self) {
        if self.camera_look_allowed() {
            let _ = self
                .window
                .set_cursor_grab(winit::window::CursorGrabMode::Locked)
                .or_else(|_| {
                    self.window
                        .set_cursor_grab(winit::window::CursorGrabMode::Confined)
                });
            self.window.set_cursor_visible(false);
        } else {
            let _ = self
                .window
                .set_cursor_grab(winit::window::CursorGrabMode::None);
            self.window.set_cursor_visible(true);
        }
    }

    pub fn handle_jump_pressed(&mut self, now: Instant, repeat: bool) {
        let can_fly = self.game_mode_policy().can_fly && !self.player_state.is_dead;
        if self.jump_taps.register(now, can_fly, repeat) {
            let flying = !self.player_physics.is_flying();
            self.player_physics.set_flying(flying);
        }
    }

    pub fn set_game_mode(&mut self, game_mode: GameMode) {
        self.jump_taps.reset();
        let policy = crate::game_rules::GameModePolicy::for_rules(game_mode, &self.world_rules);
        if !policy.can_fly {
            self.player_physics.set_flying(false);
        } else if game_mode == GameMode::Spectator {
            self.player_physics.set_flying(true);
        }
        self.player_physics.set_no_clip(policy.can_phase);
        self.game_mode = game_mode;
        if game_mode == GameMode::Spectator {
            self.inventory.is_open = false;
            self.active_station = None;
            self.container_target = None;
        }
    }

    pub fn game_mode_policy(&self) -> crate::game_rules::GameModePolicy {
        crate::game_rules::GameModePolicy::for_rules(self.game_mode, &self.world_rules)
    }

    pub fn set_world_rules(&mut self, rules: crate::game_rules::WorldRules) {
        self.world_rules = rules.normalized();
        self.player_physics
            .set_no_clip(self.game_mode_policy().can_phase);
    }

    pub fn open_chat(&mut self) {
        if self.connection_lost
            || self.is_paused
            || self.inventory.is_open
            || self.advancement_gui.is_open
            || self.player_state.is_dead
            || !self.network_ready
        {
            return;
        }
        self.chat_input.clear();
        self.is_chat_open = true;
        self.clear_movement_input();
        self.left_mouse_pressed = false;
        self.sync_cursor_mode();
    }

    pub fn close_chat(&mut self) {
        self.chat_input.clear();
        if !self.is_chat_open {
            return;
        }
        self.is_chat_open = false;
        self.sync_cursor_mode();
    }

    pub fn submit_chat(&mut self) {
        let message = normalized_chat_message(&self.chat_input);
        self.close_chat();
        let Some(message) = message else {
            return;
        };

        if message.starts_with('/') {
            if self.presentation_topology().is_join_client() {
                let status = self.translate("command.host_only");
                push_chat_history(&mut self.chat_messages, "System".to_string(), status);
            } else if !self.cheats_enabled && !matches!(self.role, MultiplayerRole::Host { .. }) {
                let status = self.translate("command.disabled");
                push_chat_history(&mut self.chat_messages, "System".to_string(), status);
            } else {
                self.execute_command_line(&message);
            }
            return;
        }

        let sender = match &self.role {
            MultiplayerRole::Client { username, .. } => username.clone(),
            MultiplayerRole::Host { .. } => "Host".to_string(),
            MultiplayerRole::Singleplayer => "Player".to_string(),
        };
        if !matches!(self.role, MultiplayerRole::Client { .. }) {
            push_chat_history(&mut self.chat_messages, sender.clone(), message.clone());
        }
        self.network.send_chat(sender, message);
    }

    /// Executes a bounded, typed command on the authoritative world. This is
    /// intentionally kept on `State` so every mutation passes the same host
    /// authority and persistence path as regular gameplay input.
    pub fn execute_command_line(&mut self, input: &str) {
        let command = match crate::commands::parse(input) {
            Ok(command) => command,
            Err(error) => {
                let command_label = self.translate("command.feedback");
                push_chat_history(
                    &mut self.chat_messages,
                    command_label,
                    format!("at {}: {}", error.position, error.message),
                );
                return;
            }
        };

        use crate::commands::Command;

        // Every mutating command is submitted to the headless core for
        // Singleplayer/listen-host.  Help and read-only gamerule queries are
        // presentation-only; unsupported mutating domains receive an explicit
        // core rejection rather than falling back to renderer state.
        if self.has_in_process_runtime()
            && !matches!(
                &command,
                Command::Help(_) | Command::GameRule { value: None, .. }
            )
        {
            let response = self.submit_local_authority_command(input);
            let feedback = match response.as_ref().map(|response| &response.outcome) {
                Some(crate::network::protocol::GameplayOutcome::Accepted { .. }) => {
                    if matches!(&command, Command::GameMode { .. }) {
                        if let Some(mode) = self
                            .embedded_runtime
                            .as_ref()
                            .and_then(EmbeddedRuntimeBridge::session_game_mode)
                        {
                            self.set_game_mode(mode);
                        }
                    }
                    if matches!(&command, Command::GameRule { .. }) {
                        self.translate("command.game_rule_updated_authority")
                    } else if matches!(&command, Command::Time(_)) {
                        let ticks = self.world_time.ticks.to_string();
                        self.translation_catalog
                            .format_lookup("command.time_now", &[("ticks", &ticks)])
                    } else {
                        let name = command.name();
                        self.translation_catalog
                            .format_lookup("command.accepted", &[("name", &name)])
                    }
                }
                Some(crate::network::protocol::GameplayOutcome::Rejected { reason }) => {
                    let reason = format!("{reason:?}");
                    self.translation_catalog
                        .format_lookup("command.rejected", &[("reason", &reason)])
                }
                None => self.translate("command.queued"),
            };
            let command_label = self.translate("command.feedback");
            push_chat_history(&mut self.chat_messages, command_label, feedback);
            return;
        }

        let feedback = match command {
            Command::Help(command) => Some(crate::commands::help_text(command.as_deref()).into()),
            _ => None,
        };

        if let Some(feedback) = feedback {
            let command_label = self.translate("command.feedback");
            push_chat_history(&mut self.chat_messages, command_label, feedback);
        }
    }

    pub fn handle_connection_lost_click(&mut self) -> bool {
        if !self.connection_lost {
            return false;
        }
        let [x, y] = self.mouse_ndc;
        if !(-0.3..=0.3).contains(&x) || !(-0.10..=0.00).contains(&y) {
            return false;
        }
        self.audio_manager
            .play_sound(crate::audio::SoundId::UiClick);
        self.shutdown_network();
        if !self.presentation_topology().is_join_client() {
            if let Err(error) = self.save_synchronously() {
                self.is_saving = false;
                self.save_error = Some(error.to_string());
                return false;
            }
        }
        true
    }

    pub fn handle_save_error_click(&mut self) -> bool {
        let Some(_) = self.save_error else {
            return false;
        };
        let [x, y] = self.mouse_ndc;
        if !(-0.3..=0.3).contains(&x) {
            return false;
        }

        if (0.02..=0.12).contains(&y) {
            self.audio_manager
                .play_sound(crate::audio::SoundId::UiClick);
            self.save_error = None;
            self.is_saving = true;
            let _ = self.render();
            match self.save_synchronously() {
                Ok(()) => true,
                Err(error) => {
                    self.is_saving = false;
                    self.save_error = Some(error.to_string());
                    false
                }
            }
        } else if (-0.16..=-0.06).contains(&y) {
            self.audio_manager
                .play_sound(crate::audio::SoundId::UiClick);
            self.save_error = None;
            self.is_saving = false;
            true
        } else {
            false
        }
    }

    pub fn save_synchronously(&mut self) -> crate::save::SaveResult<()> {
        if self.presentation_topology().is_join_client() {
            return Ok(());
        }
        if let Some(runtime) = self.embedded_runtime.as_mut() {
            let world_dir = runtime.runtime.properties.world_dir.clone();
            runtime
                .save_all()
                .map_err(|error| crate::save::SaveError::Io {
                    operation: "embedded runtime save",
                    path: world_dir,
                    message: error.to_string(),
                })?;
            return Ok(());
        }
        Ok(())
    }

    pub fn trigger_advancement(&mut self, trigger: crate::advancements::AdvancementTrigger) {
        let newly_completed = self.advancement_manager.check_trigger(&trigger);
        for id in newly_completed {
            if let Some(adv) = self.advancement_manager.tree.get(&id) {
                if adv.xp_reward > 0 {
                    self.player_state.add_experience(adv.xp_reward);
                }
                self.audio_manager
                    .play_sound(crate::audio::SoundId::UiClick);
            }
        }
    }

    pub fn open_advancements_ui(&mut self) {
        if self.inventory.is_open && !self.close_inventory() {
            return;
        }
        self.advancement_gui.open();
        self.clear_movement_input();
        self.sync_cursor_mode();
    }

    pub fn close_advancements_ui(&mut self) {
        self.advancement_gui.close();
        self.sync_cursor_mode();
    }

    pub fn handle_advancements_click(&mut self, pressed: bool) {
        if !self.advancement_gui.is_open {
            return;
        }
        let (screen_w, screen_h) = (self.config.width as f32, self.config.height as f32);
        let mouse_x = (self.mouse_ndc[0] + 1.0) * 0.5 * screen_w;
        let mouse_y = (1.0 - self.mouse_ndc[1]) * 0.5 * screen_h;

        let wy0 = screen_h * 0.1;
        let wy1 = screen_h * 0.9;
        let wx0 = screen_w * 0.1;
        let wx1 = screen_w * 0.9;

        if pressed {
            if mouse_y >= wy0 && mouse_y <= wy0 + 40.0 && mouse_x >= wx0 && mouse_x <= wx1 {
                let tab_w = (wx1 - wx0) / 5.0;
                let tab_idx = ((mouse_x - wx0) / tab_w).floor() as usize;
                let categories = [
                    crate::advancements::AdvancementCategory::Minecraft,
                    crate::advancements::AdvancementCategory::Nether,
                    crate::advancements::AdvancementCategory::TheEnd,
                    crate::advancements::AdvancementCategory::Adventure,
                    crate::advancements::AdvancementCategory::Husbandry,
                ];
                if tab_idx < categories.len() {
                    self.advancement_gui.selected_category = categories[tab_idx];
                }
            } else if mouse_x >= wx0 && mouse_x <= wx1 && mouse_y >= wy0 + 40.0 && mouse_y <= wy1 {
                self.advancement_gui.is_dragging = true;
                self.advancement_gui.drag_start_x = mouse_x - self.advancement_gui.scroll_x;
                self.advancement_gui.drag_start_y = mouse_y - self.advancement_gui.scroll_y;
            }
        } else {
            self.advancement_gui.is_dragging = false;
        }
    }

    fn free_chunk_mesh_allocations(
        render_regions: &mut std::collections::HashMap<(i32, i32), RenderRegion>,
        coord: (i32, i32),
        mesh: &ChunkMesh,
    ) {
        let r_coord = crate::chunk_render::chunk_to_region_coord(coord.0, coord.1);
        if let Some(region) = render_regions.get_mut(&r_coord) {
            let mesh_has_resident_section = mesh.has_resident_section();
            let (mesh_has_handles, mesh_has_matching_handle) =
                mesh.allocation_handle_region_membership(region.region_instance_id);
            for section in &mesh.sections {
                if let Some(levels) = &section.levels {
                    for level in levels {
                        if let Some(h) = &level.opaque.handle {
                            if let Err(error) = region.deallocate_handle(h) {
                                eprintln!("[RenderRegion] deallocate failed: {error:?}");
                            }
                        }
                        if let Some(h) = &level.transparent.handle {
                            if let Err(error) = region.deallocate_handle(h) {
                                eprintln!("[RenderRegion] deallocate failed: {error:?}");
                            }
                        }
                    }
                }
            }
            if should_decrement_region_active_chunks(
                mesh_has_resident_section,
                mesh_has_handles,
                mesh_has_matching_handle,
            ) {
                region.active_chunks = region.active_chunks.saturating_sub(1);
            }
            let arena_is_empty =
                region.vertex_freelist.used_units() == 0 && region.index_freelist.used_units() == 0;
            if region.active_chunks == 0 && arena_is_empty {
                render_regions.remove(&r_coord);
            }
        }
    }

    fn upload_section_mesh_bundle(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        region_bind_group_layout: &wgpu::BindGroupLayout,
        render_regions: &mut std::collections::HashMap<(i32, i32), RenderRegion>,
        existing_section: &mut GpuSectionMesh,
        bundle: &crate::chunk_render::SectionMeshBundle,
        terrain_generation: u64,
        register_resident_chunk: bool,
    ) -> ([GpuMeshLevel; 3], UploadMetrics) {
        let coord = (bundle.identity.key.cx, bundle.identity.key.cz);
        let r_coord = crate::chunk_render::chunk_to_region_coord(coord.0, coord.1);
        let region = render_regions
            .entry(r_coord)
            .or_insert_with(|| RenderRegion::new(device, region_bind_group_layout, r_coord));
        if register_resident_chunk {
            region.active_chunks += 1;
        }

        let full_rebuild = existing_section.needs_rebuild();
        let mask = bundle.built_lods;

        let mut levels = if full_rebuild {
            if let Some(old) = existing_section.levels.take() {
                for level in &old {
                    Self::deallocate_gpu_mesh_level(region, level);
                }
            }
            std::array::from_fn(|_| GpuMeshLevel::empty())
        } else {
            existing_section
                .levels
                .take()
                .unwrap_or_else(|| std::array::from_fn(|_| GpuMeshLevel::empty()))
        };

        let mut metrics = UploadMetrics::default();
        for index in 0..3 {
            if mask & (1 << index) == 0 {
                continue;
            }
            if !full_rebuild {
                Self::deallocate_gpu_mesh_level(region, &levels[index]);
            }
            let data = &bundle.levels[index];
            let owner_opaque = crate::chunk_render::allocation_owner(
                terrain_generation,
                bundle.identity.lifetime,
                bundle.identity.key.section_y,
                index as u8,
                0,
            );
            let owner_transparent = crate::chunk_render::allocation_owner(
                terrain_generation,
                bundle.identity.lifetime,
                bundle.identity.key.section_y,
                index as u8,
                1,
            );
            let (opaque, opaque_metrics) =
                region.upload_mesh_layer(device, queue, &data.opaque, owner_opaque);
            let (transparent, transparent_metrics) =
                region.upload_mesh_layer(device, queue, &data.transparent, owner_transparent);
            metrics = metrics.add(opaque_metrics).add(transparent_metrics);
            levels[index] = GpuMeshLevel {
                opaque,
                transparent,
                bounds: data.bounds(),
            };
        }
        existing_section.built_lods = if full_rebuild {
            mask
        } else {
            existing_section.built_lods | mask
        };
        (levels, metrics)
    }

    fn deallocate_gpu_mesh_level(region: &mut RenderRegion, level: &GpuMeshLevel) {
        if let Some(h) = &level.opaque.handle {
            if let Err(error) = region.deallocate_handle(h) {
                eprintln!("[RenderRegion] deallocate failed: {error:?}");
            }
        }
        if let Some(h) = &level.transparent.handle {
            if let Err(error) = region.deallocate_handle(h) {
                eprintln!("[RenderRegion] deallocate failed: {error:?}");
            }
        }
    }

    fn next_chunk_lifetime(&mut self) -> u64 {
        let lifetime = self.next_chunk_lifetime;
        self.next_chunk_lifetime = self.next_chunk_lifetime.wrapping_add(1).max(1);
        lifetime
    }

    fn current_section_identity(&self, key: SectionKey) -> Option<SectionIdentity> {
        let lifetime = self.chunk_lifetimes.get(&(key.cx, key.cz)).copied()?;
        let revision = self
            .chunk_meshes
            .get(&(key.cx, key.cz))?
            .section(key.section_y)?
            .revision;
        Some(SectionIdentity::new(key, revision, lifetime))
    }

    fn invalidate_section_mesh(&mut self, key: SectionKey, reason: DependencyReason) -> bool {
        self.chunk_manager
            .acknowledge_section_mesh_invalidation(&key);
        let Some(lifetime) = self.chunk_lifetimes.get(&(key.cx, key.cz)).copied() else {
            return false;
        };
        let Some(section) = self
            .chunk_meshes
            .get_mut(&(key.cx, key.cz))
            .and_then(|mesh| mesh.section_mut(key.section_y))
        else {
            return false;
        };
        section.invalidate();
        self.los_world_revision = self.los_world_revision.wrapping_add(1);
        let identity = SectionIdentity::new(key, section.revision, lifetime);
        let player_chunk = (
            (self.player_physics.position.x / CHUNK_WIDTH as f32).floor() as i32,
            (self.player_physics.position.z / CHUNK_DEPTH as f32).floor() as i32,
        );
        self.section_scheduler
            .enqueue(identity, reason, player_chunk);
        if self.section_storage_compaction_queued.insert(key) {
            self.section_storage_compaction_queue.push_back(key);
        }
        true
    }

    fn invalidate_chunk_mesh(&mut self, coord: (i32, i32), reason: DependencyReason) -> bool {
        self.chunk_manager.acknowledge_mesh_invalidation(&coord);
        let mut invalidated = false;
        let height = self.chunk_manager.dimension.height();
        for section_y in height.min_section_y()..height.max_section_y_exclusive() {
            invalidated |=
                self.invalidate_section_mesh(SectionKey::new(coord.0, section_y, coord.1), reason);
        }
        invalidated
    }

    fn invalidate_chunk_meshes(
        &mut self,
        coords: impl IntoIterator<Item = (i32, i32)>,
        reason: DependencyReason,
    ) {
        for coord in coords {
            self.invalidate_chunk_mesh(coord, reason);
        }
    }

    /// Applies the same one-voxel halo dependency used by `MeshSnapshot`.
    /// Cardinal and diagonal dependents are tagged as derived AO work.
    fn process_terrain_worker_results(&mut self, player_chunk: (i32, i32)) {
        let integrate_started = Instant::now();
        let mut lighting_elapsed = Duration::ZERO;
        let mut gpu_upload_elapsed = Duration::ZERO;

        let mut integrated_meshes = 0;
        let mut integrated_bytes = 0u64;
        let mut integrated_loads = 0;
        let mut integrated_load_bytes = 0u64;

        loop {
            let result = if let Some(res) = self.pending_worker_results.pop_front() {
                res
            } else if let Ok(res) = self.terrain_worker_rx.try_recv() {
                res
            } else {
                break;
            };

            if let TerrainWorkerResult::SectionMeshed(_) = &result {
                let elapsed = integrate_started.elapsed();
                if integrated_meshes >= crate::chunk_schedule::MAX_INTEGRATE_MESHES
                    || integrated_bytes >= crate::chunk_schedule::MAX_INTEGRATE_UPLOAD_BYTES
                    || elapsed
                        >= Duration::from_millis(crate::chunk_schedule::MAX_INTEGRATE_TIME_MS)
                {
                    self.pending_worker_results.push_front(result);
                    break;
                }
            }

            match result {
                TerrainWorkerResult::Loaded(result) => {
                    let expected = self.chunk_load_in_flight.get(&result.coord).copied();
                    if result.restore_failed {
                        if expected == Some(result.lifetime) {
                            self.chunk_load_in_flight.remove(&result.coord);
                        }
                        continue;
                    }
                    let r = self.chunk_manager.render_distance;
                    if !chunk_load_result_is_current(
                        expected,
                        result.lifetime,
                        result.generation,
                        self.terrain_generation,
                        result.dimension,
                        self.current_dimension,
                    ) || (result.coord.0 - player_chunk.0).abs() > r
                        || (result.coord.1 - player_chunk.1).abs() > r
                        || self.chunk_manager.chunks.contains_key(&result.coord)
                    {
                        if expected == Some(result.lifetime) {
                            self.chunk_load_in_flight.remove(&result.coord);
                        }
                        self.perf_counters.stale_results =
                            self.perf_counters.stale_results.saturating_add(1);
                        continue;
                    }
                    let elapsed = integrate_started.elapsed();
                    if integrated_loads >= crate::chunk_schedule::MAX_INTEGRATE_LOADS
                        || integrated_load_bytes >= crate::chunk_schedule::MAX_INTEGRATE_LOAD_BYTES
                        || elapsed
                            >= Duration::from_millis(crate::chunk_schedule::MAX_INTEGRATE_TIME_MS)
                    {
                        self.pending_worker_results
                            .push_front(TerrainWorkerResult::Loaded(result));
                        break;
                    }
                    if expected == Some(result.lifetime) {
                        self.chunk_load_in_flight.remove(&result.coord);
                    }

                    let (cx, cz) = result.coord;
                    let load_bytes = result.chunk.memory_usage() as u64;
                    self.chunk_manager.chunks.insert(result.coord, result.chunk);
                    self.chunk_lifetimes.insert(result.coord, result.lifetime);
                    self.chunk_meshes.insert(result.coord, ChunkMesh::pending());
                    self.invalidate_chunk_mesh(result.coord, DependencyReason::ChunkLoad);
                    integrated_loads += 1;
                    integrated_load_bytes = integrated_load_bytes.saturating_add(load_bytes);

                    let mut pending_base_revision = 0;
                    if let Some((revision, blocks, block_states, fluid_levels, block_entities)) =
                        self.pending_chunk_payloads.remove(&result.coord)
                    {
                        pending_base_revision = revision;
                        if let Some(chunk) = self.chunk_manager.chunks.get_mut(&result.coord) {
                            Self::restore_chunk_payload(
                                chunk,
                                &blocks,
                                &block_states,
                                &fluid_levels,
                                &block_entities,
                            );
                        }
                        self.invalidate_chunk_mesh(result.coord, DependencyReason::Network);
                    }
                    if let Some(changes) = self.pending_block_changes.remove(&result.coord) {
                        self.client_chunk_revisions.insert(
                            (self.current_dimension, result.coord.0, result.coord.1),
                            pending_base_revision,
                        );
                        let mut changes: Vec<_> = changes.into_iter().collect();
                        changes.sort_by_key(|(_, (revision, _, _, _))| *revision);
                        for ((x, y, z), (revision, block, state, raw_fluid)) in changes {
                            self.apply_remote_block_change(
                                self.current_dimension as u8,
                                revision,
                                x,
                                y,
                                z,
                                block,
                                state,
                                raw_fluid,
                            );
                        }
                    }

                    let mut dirty = std::collections::HashSet::new();
                    let lighting_started = Instant::now();
                    for (lighting_cx, lighting_cz) in [
                        (cx, cz),
                        (cx - 1, cz),
                        (cx + 1, cz),
                        (cx, cz - 1),
                        (cx, cz + 1),
                    ] {
                        if self
                            .chunk_manager
                            .chunks
                            .contains_key(&(lighting_cx, lighting_cz))
                        {
                            crate::lighting::propagate_chunk_lighting(
                                &mut self.chunk_manager,
                                lighting_cx,
                                lighting_cz,
                                &mut dirty,
                            );
                        }
                    }
                    let elapsed = lighting_started.elapsed();
                    lighting_elapsed += elapsed;
                    self.lighting_scopes_frame
                        .record(crate::perf::LightingSource::Load as usize, elapsed);
                    for neighbor in surrounding_chunk_coords(cx, cz) {
                        self.invalidate_chunk_mesh(neighbor, DependencyReason::ChunkLoad);
                    }
                    for coord in dirty {
                        self.invalidate_chunk_mesh(coord, DependencyReason::Light);
                    }
                }
                TerrainWorkerResult::SectionMeshed(result) => {
                    let identity = result.bundle.identity;
                    let expected = self.section_scheduler.in_flight.get(&identity.key).copied();
                    if expected == Some(identity) {
                        self.section_scheduler.complete(identity);
                    }
                    let current_identity = self.current_section_identity(identity.key);
                    if !section_mesh_result_is_current(
                        expected,
                        identity,
                        result.generation,
                        self.terrain_generation,
                        current_identity,
                    ) {
                        self.perf_counters.stale_results =
                            self.perf_counters.stale_results.saturating_add(1);
                        continue;
                    }
                    let coord = (identity.key.cx, identity.key.cz);
                    let region_coord = crate::chunk_render::chunk_to_region_coord(coord.0, coord.1);
                    let register_resident_chunk =
                        self.chunk_meshes.get(&coord).is_some_and(|mesh| {
                            !chunk_mesh_is_registered_with_region(
                                mesh,
                                self.render_regions.get(&region_coord),
                            )
                        });
                    let Some(mesh) = self.chunk_meshes.get_mut(&coord) else {
                        continue;
                    };
                    let Some(section) = mesh.section_mut(identity.key.section_y) else {
                        continue;
                    };
                    let (levels, upload_metrics) = Self::upload_section_mesh_bundle(
                        &self.device,
                        &self.queue,
                        &self.region_bind_group_layout,
                        &mut self.render_regions,
                        section,
                        &result.bundle,
                        self.terrain_generation,
                        register_resident_chunk,
                    );
                    section.levels = Some(levels);
                    section.connectivity =
                        crate::culling::SectionConnectivityState::Valid(result.bundle.connectivity);
                    let upload_elapsed = Duration::from_nanos(upload_metrics.elapsed_ns);
                    gpu_upload_elapsed += upload_elapsed;
                    self.gpu_upload_scopes_frame
                        .record(crate::perf::UploadSource::Terrain as usize, upload_elapsed);
                    self.perf_counters.upload_bytes_frame = self
                        .perf_counters
                        .upload_bytes_frame
                        .saturating_add(upload_metrics.bytes);
                    let gpu_bytes = section.gpu_bytes() as u64;
                    section.meshed_revision = identity.revision;
                    integrated_meshes += 1;
                    integrated_bytes += gpu_bytes;
                }
            }
        }
        self.perf_recorder.record(
            crate::perf::ScopeId::TerrainResultIntegrate,
            integrate_started.elapsed(),
        );
        self.lighting_time_frame += lighting_elapsed;
        self.gpu_upload_time_frame += gpu_upload_elapsed;
    }

    fn schedule_chunk_load(&mut self, coord: (i32, i32)) {
        if self.chunk_load_in_flight.contains_key(&coord)
            || self.chunk_manager.chunks.contains_key(&coord)
            || self.chunk_load_in_flight.len() >= MAX_CHUNK_LOAD_JOBS
        {
            return;
        }
        if crate::presentation_inventory_policy::schedule_presentation_chunk_load(
            self.presentation_topology().chunk_load_policy(),
            || (),
        )
        .is_none()
        {
            // Join client: only enqueue interest and wait for ChunkData.
            return;
        }
        let lifetime = self.next_chunk_lifetime();
        self.chunk_load_in_flight.insert(coord, lifetime);
        let sender = self.terrain_worker_tx.clone();
        let generation = self.terrain_generation;
        let dimension = self.current_dimension;
        let world_seed = self.world_seed;
        let world_type = self.world_type;
        let generate_structures = self.generate_structures;
        rayon::spawn(move || {
            let chunk = crate::dimension::generate_chunk_with_options(
                dimension,
                coord.0,
                coord.1,
                world_seed,
                crate::dimension::WorldGenerationOptions {
                    world_type,
                    generate_structures,
                },
            );
            let _ = sender.send(TerrainWorkerResult::Loaded(ChunkLoadResult {
                coord,
                dimension,
                generation,
                lifetime,
                chunk,
                restore_failed: false,
            }));
        });
    }

    fn schedule_section_mesh(&mut self, work: crate::chunk_schedule::DirtySectionWork) -> bool {
        let key = work.identity.key;
        if self.section_scheduler.is_in_flight(key)
            || self.section_scheduler.in_flight.len() >= MAX_CHUNK_MESH_JOBS
        {
            return false;
        }
        if !self.chunk_manager.chunks.contains_key(&(key.cx, key.cz)) {
            return true;
        }
        let Some(section) = self
            .chunk_meshes
            .get(&(key.cx, key.cz))
            .and_then(|mesh| mesh.section(key.section_y))
        else {
            return true;
        };
        if self.current_section_identity(key) != Some(work.identity) {
            return true;
        }
        let selected = self.section_selected_lod(key);
        if !section.needs_rebuild() && section.lod_is_built(selected) {
            return true;
        }
        let lod_mask = selected.mask();
        let snapshot = self.chunk_manager.capture_section_halo(key);
        self.section_scheduler.mark_in_flight(work);
        let sender = self.terrain_worker_tx.clone();
        let generation = self.terrain_generation;
        let model_registry = Arc::clone(&self.model_registry);
        rayon::spawn(move || {
            let bundle = Chunk::generate_section_mesh_bundle_from_halo_with_registry_for_lods(
                work.identity,
                &snapshot,
                &model_registry,
                lod_mask,
            );
            let _ = sender.send(TerrainWorkerResult::SectionMeshed(SectionMeshResult {
                generation,
                bundle,
            }));
        });
        true
    }

    fn section_selected_lod(&self, key: SectionKey) -> LodLevel {
        let render_blocks = self.chunk_manager.render_distance as f32 * CHUNK_WIDTH as f32;
        let thresholds = LodThresholds::new(render_blocks * 0.5, render_blocks * 0.75);
        let min = Vec3::new(
            (key.cx * CHUNK_WIDTH as i32) as f32,
            key.min_world_y() as f32,
            (key.cz * CHUNK_DEPTH as i32) as f32,
        );
        let max = Vec3::new(
            (key.cx * CHUNK_WIDTH as i32 + CHUNK_WIDTH as i32) as f32,
            key.max_world_y() as f32,
            (key.cz * CHUNK_DEPTH as i32 + CHUNK_DEPTH as i32) as f32,
        );
        select_lod_for_bounds(
            self.player_physics.position,
            MeshBounds::new(min, max),
            thresholds,
        )
    }

    pub fn update_chunks(&mut self) {
        if !self.network_ready {
            return;
        }
        let unreported_sections = self.chunk_manager.drain_section_mesh_invalidations();
        for key in unreported_sections {
            self.invalidate_section_mesh(key, DependencyReason::Block);
        }
        // Section invalidations are authoritative; drain the legacy chunk set
        // so it cannot trigger a redundant whole-column rebuild.
        self.chunk_manager.drain_mesh_invalidations();
        let player_pos = self.player_physics.position;
        let px = (player_pos.x / 16.0).floor() as i32;
        let pz = (player_pos.z / 16.0).floor() as i32;
        let r = self.chunk_manager.render_distance;
        self.process_terrain_worker_results((px, pz));
        self.process_terrain_compaction();
        // Only empty, previously-grown arenas are staged. Processing is
        // bounded to one region per frame and never rebases a live handle.
        self.schedule_terrain_compaction();

        let target_changed = self.scheduler.last_player_chunk != Some((px, pz))
            || self.scheduler.last_render_distance != r
            || self.scheduler.last_dimension != Some(self.current_dimension);

        if target_changed {
            if self.scheduler.last_render_distance != r || self.scheduler.spiral_offsets.is_empty()
            {
                self.scheduler.spiral_offsets = crate::chunk_schedule::precompute_spiral_offsets(r);
            }

            let mut to_unload = Vec::new();
            for &(cx, cz) in self.chunk_manager.chunks.keys() {
                if !crate::chunk_schedule::within_unload_hysteresis(cx, cz, px, pz, r) {
                    to_unload.push((cx, cz));
                }
            }
            for &(cx, cz) in &to_unload {
                let _ = self.chunk_manager.chunks.remove(&(cx, cz));
            }
            for &(cx, cz) in &to_unload {
                for neighbor in surrounding_chunk_coords(cx, cz) {
                    if self.chunk_manager.chunks.contains_key(&neighbor) {
                        self.invalidate_chunk_mesh(neighbor, DependencyReason::ChunkLoad);
                    }
                }
                self.chunk_lifetimes.remove(&(cx, cz));
                self.section_scheduler.remove_chunk(cx, cz);
            }
            let mut removed_mesh_keys = Vec::new();
            for &(cx, cz) in self.chunk_meshes.keys() {
                if !crate::chunk_schedule::within_unload_hysteresis(cx, cz, px, pz, r) {
                    removed_mesh_keys.push((cx, cz));
                }
            }
            for coord in removed_mesh_keys {
                if let Some(mesh) = self.chunk_meshes.remove(&coord) {
                    Self::free_chunk_mesh_allocations(&mut self.render_regions, coord, &mesh);
                }
            }
            self.chunk_load_in_flight.retain(|&(cx, cz), _| {
                crate::chunk_schedule::within_unload_hysteresis(cx, cz, px, pz, r)
            });

            // Rebuild pending_load_queue in spiral order
            self.scheduler.pending_load_queue.clear();
            for &(dx, dz) in &self.scheduler.spiral_offsets {
                let cx = px + dx;
                let cz = pz + dz;
                if !self.chunk_manager.chunks.contains_key(&(cx, cz))
                    && !self.chunk_load_in_flight.contains_key(&(cx, cz))
                {
                    self.scheduler.pending_load_queue.push_back((cx, cz));
                }
            }

            self.scheduler.last_player_chunk = Some((px, pz));
            self.scheduler.last_render_distance = r;
            self.scheduler.last_dimension = Some(self.current_dimension);
            self.section_scheduler.reprioritize((px, pz));
        }

        // 2. Dispatch chunk loads from precomputed spiral load queue
        let available_load_slots =
            MAX_CHUNK_LOAD_JOBS.saturating_sub(self.chunk_load_in_flight.len());
        let mut dispatched_loads = 0;
        while dispatched_loads < available_load_slots {
            if let Some(coord) = self.scheduler.pending_load_queue.pop_front() {
                if !self.chunk_manager.chunks.contains_key(&coord)
                    && !self.chunk_load_in_flight.contains_key(&coord)
                {
                    self.schedule_chunk_load(coord);
                    dispatched_loads += 1;
                }
            } else {
                break;
            }
        }

        // 3. Dispatch dirty meshes prioritized by distance to player
        let available_mesh_slots =
            MAX_CHUNK_MESH_JOBS.saturating_sub(self.section_scheduler.in_flight.len());
        if available_mesh_slots > 0 && self.section_scheduler.len() > 0 {
            let r_i32 = r as i32;
            let mut dispatched = 0;
            let mut deferred = Vec::with_capacity(self.section_scheduler.in_flight.len().max(1));
            while dispatched < available_mesh_slots {
                let Some(work) = self.section_scheduler.pop_nearest((px, pz), r_i32) else {
                    break;
                };
                let key = work.identity.key;
                if self.section_scheduler.is_in_flight(key) {
                    deferred.push(work);
                    continue;
                }
                let in_flight_before = self.section_scheduler.in_flight.len();
                if self.schedule_section_mesh(work) {
                    if self.section_scheduler.in_flight.len() > in_flight_before {
                        dispatched += 1;
                    }
                } else {
                    deferred.push(work);
                }
            }
            for work in deferred {
                self.section_scheduler.requeue(work, (px, pz));
            }
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.is_paused = paused;
        if paused {
            self.clear_movement_input();
        }
        self.sync_cursor_mode();
    }

    pub fn handle_mouse_move(&mut self, x: f64, y: f64) {
        self.mouse_ndc = cursor_position_to_ndc(x, y, self.size.width, self.size.height);
    }

    pub fn handle_menu_click(&mut self) -> bool {
        if self.is_paused {
            let [x, y] = self.mouse_ndc;

            // Resume Button bounds: X: [-0.3, 0.3], Y: [0.24, 0.34]
            if x >= -0.3 && x <= 0.3 && y >= 0.24 && y <= 0.34 {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::UiClick);
                self.set_paused(false);
            }
            // FOV Button bounds: X: [-0.3, 0.3], Y: [0.10, 0.20]
            else if x >= -0.3 && x <= 0.3 && y >= 0.10 && y <= 0.20 {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::UiClick);
                if x < 0.0 {
                    self.base_fov = (self.base_fov - 5.0).max(30.0);
                } else {
                    self.base_fov = (self.base_fov + 5.0).min(120.0);
                }
                self.camera.fov = self.base_fov;
                // Update camera projection buffer immediately for visual feedback in paused state
                let is_underwater = self.chunk_manager.get_block(
                    self.camera.position.x.floor() as i32,
                    self.camera.position.y.floor() as i32,
                    self.camera.position.z.floor() as i32,
                ) == BlockType::Water;
                self.camera_uniform.update_view_proj(
                    &self.camera,
                    self.config.width as f32 / self.config.height as f32,
                    self.chunk_manager.render_distance as u32,
                    self.chunk_manager.dimension.height().height(),
                    &self.world_time,
                    self.total_time,
                    is_underwater,
                );
                let upload_started = Instant::now();
                self.queue.write_buffer(
                    &self.camera_buffer,
                    0,
                    bytemuck::cast_slice(&[self.camera_uniform]),
                );
                let upload_elapsed = upload_started.elapsed();
                self.gpu_upload_time_frame += upload_elapsed;
                self.gpu_upload_scopes_frame
                    .record(crate::perf::UploadSource::Camera as usize, upload_elapsed);
                self.perf_counters.upload_bytes_frame = self
                    .perf_counters
                    .upload_bytes_frame
                    .saturating_add(std::mem::size_of::<CameraUniform>() as u64);
                self.save_settings();
            }
            // Sensitivity Button bounds: X: [-0.3, 0.3], Y: [-0.04, 0.06]
            else if x >= -0.3 && x <= 0.3 && y >= -0.04 && y <= 0.06 {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::UiClick);
                if x < 0.0 {
                    self.sensitivity = (self.sensitivity - 0.0002).max(0.0002);
                } else {
                    self.sensitivity = (self.sensitivity + 0.0002).min(0.0060);
                }
                self.save_settings();
            }
            // Render Distance Button bounds: X: [-0.3, 0.3], Y: [-0.18, -0.08]
            else if x >= -0.3 && x <= 0.3 && y >= -0.18 && y <= -0.08 {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::UiClick);
                if x < 0.0 {
                    self.chunk_manager.render_distance =
                        (self.chunk_manager.render_distance - 1).max(2);
                } else {
                    self.chunk_manager.render_distance =
                        (self.chunk_manager.render_distance + 1).min(16);
                }
                self.save_settings();
            }
            // Master Volume Button: X: [-0.3, 0.3], Y: [-0.32, -0.22]
            else if x >= -0.3 && x <= 0.3 && y >= -0.32 && y <= -0.22 {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::UiClick);
                let delta = if x < 0.0 { -0.1 } else { 0.1 };
                self.settings.master_volume = (self.settings.master_volume + delta).clamp(0.0, 1.0);
                self.save_settings();
            }
            // Weather Volume Button: X: [-0.3, 0.3], Y: [-0.46, -0.36]
            else if point_in_bounds(x, y, PAUSE_WEATHER_VOLUME_BOUNDS) {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::UiClick);
                let delta = if x < 0.0 { -0.1 } else { 0.1 };
                self.settings.weather_volume =
                    (self.settings.weather_volume + delta).clamp(0.0, 1.0);
                self.save_settings();
            }
            // Quit Button bounds: X: [-0.3, 0.3], Y: [-0.60, -0.50]
            else if point_in_bounds(x, y, PAUSE_QUIT_BOUNDS) {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::UiClick);
                self.is_saving = true;
                self.save_error = None;
                let _ = self.render();
                self.shutdown_network();
                return match self.save_synchronously() {
                    Ok(()) => true,
                    Err(error) => {
                        self.is_saving = false;
                        self.save_error = Some(error.to_string());
                        false
                    }
                };
            }
        }
        false
    }

    pub fn tick_simulation(&mut self, dt: f32) {
        self.prev_player_position = self.player_physics.position;
        let world_tick_started = Instant::now();
        // Singleplayer and listen-host worlds advance exclusively in the
        // headless AuthorityCore.
        let has_in_process_runtime = self.has_in_process_runtime();
        if has_in_process_runtime {
            let _ = self.tick_authority_boundary();
        }

        // Tick attack cooldown & shield disable ticks
        if self.player_state.attack_cooldown_ticks < self.player_state.attack_cooldown_max_ticks {
            self.player_state.attack_cooldown_ticks += 1;
        }
        if self.player_state.shield_disable_ticks > 0 {
            self.player_state.shield_disable_ticks -= 1;
        }

        self.player_state.using_item = None;

        self.update_portal_travel(dt);

        let can_sprint = sprint_allowed(self.game_mode, self.player_state.hunger);

        // Accessibility toggle controls are edge-triggered so holding a key
        // does not repeatedly flip state. They only alter local input state;
        // simulation timing, authority, and network snapshots are unchanged.
        if self.settings.accessibility.toggle_sprint {
            if self.keys.ctrl && !self.last_ctrl_pressed {
                self.sprint_toggle_latched = !self.sprint_toggle_latched;
            }
        } else {
            self.sprint_toggle_latched = false;
        }
        if self.settings.accessibility.toggle_sneak {
            if self.keys.shift && !self.last_shift_pressed {
                self.sneak_toggle_latched = !self.sneak_toggle_latched;
            }
        } else {
            self.sneak_toggle_latched = false;
        }
        self.last_ctrl_pressed = self.keys.ctrl;
        self.last_shift_pressed = self.keys.shift;
        let sneak_input = if self.settings.accessibility.toggle_sneak {
            self.sneak_toggle_latched
        } else {
            self.keys.shift
        };

        // Double click W logic
        if self.keys.w && !self.last_w_pressed {
            if self.w_click_timer > 0.0 && can_sprint {
                self.is_sprinting = true;
            }
            self.w_click_timer = 0.3;
        }
        self.last_w_pressed = self.keys.w;

        // Ctrl key sprint check
        if ((!self.settings.accessibility.toggle_sprint && self.keys.ctrl)
            || (self.settings.accessibility.toggle_sprint && self.sprint_toggle_latched))
            && self.keys.w
            && can_sprint
        {
            self.is_sprinting = true;
        }

        // Cancel sprinting conditions
        if !self.keys.w || sneak_input || !can_sprint {
            self.is_sprinting = false;
        }

        // Cancel if player collides with a wall but has movement inputs
        if self.is_sprinting
            && (self.player_physics.velocity.x.abs() < 0.01
                && self.player_physics.velocity.z.abs() < 0.01)
            && (self.keys.w || self.keys.a || self.keys.s || self.keys.d)
        {
            self.is_sprinting = false;
        }

        // Weather phase is host TimeSync only — no GPU-thread climate/RNG cycle.
        // `do_weather_cycle` remains a synced gamerule field for a future authority owner.
        if self.current_dimension == crate::dimension::Dimension::Overworld {
            self.weather.tick_presentation(dt);
        } else {
            self.audio_manager.stop_looping_sound(RAIN_LOOP_ID);
        }

        let mut move_dir = Vec3::ZERO;
        let yaw_cos = self.camera.yaw.cos();
        let yaw_sin = self.camera.yaw.sin();
        let forward = Vec3::new(yaw_cos, 0.0, yaw_sin).normalize_or_zero();
        let right = Vec3::new(-yaw_sin, 0.0, yaw_cos).normalize_or_zero();

        if self.keys.w {
            move_dir += forward;
        }
        if self.keys.s {
            move_dir -= forward;
        }
        if self.keys.a {
            move_dir += right;
        }
        if self.keys.d {
            move_dir -= right;
        }
        let mut movement = move_dir.normalize_or_zero() * self.potion_effects.speed_multiplier();
        let was_flying = self.player_physics.is_flying();
        if was_flying {
            movement.y = match (self.keys.space, sneak_input) {
                (true, false) => 1.0,
                (false, true) => -1.0,
                _ => 0.0,
            };
        } else if self.keys.space {
            movement.y = 1.0;
        }

        // Jump exhaustion check
        let jumped = !was_flying && self.keys.space && self.player_physics.on_ground;
        if jumped {
            self.audio_manager.play_sound(crate::audio::SoundId::Jump);
        }

        let old_pos = self.player_physics.position;

        let physics_started = Instant::now();
        let fall_damage = self.player_physics.update(
            dt,
            &self.chunk_manager,
            movement,
            sneak_input && !was_flying,
            self.is_sprinting,
        );
        let _ = fall_damage; // Authority owns fall damage; local physics still computes it.
        self.perf_recorder.record(
            crate::perf::ScopeId::PlayerPhysics,
            physics_started.elapsed(),
        );
        if should_exit_creative_flight(was_flying, movement.y, self.player_physics.on_ground) {
            self.player_physics.set_flying(false);
            self.jump_taps.reset();
        }
        let chunk_schedule_started = Instant::now();
        self.update_chunks();
        self.perf_recorder.record(
            crate::perf::ScopeId::ChunkSchedule,
            chunk_schedule_started.elapsed(),
        );

        // Landing sound
        let px = self.player_physics.position.x.floor() as i32;
        let py = (self.player_physics.position.y - 0.1).floor() as i32;
        let pz = self.player_physics.position.z.floor() as i32;
        let under_block = self.chunk_manager.get_block(px, py, pz);

        if self.player_physics.on_ground && !self.was_on_ground {
            if let Some(mat) = under_block.sound_material() {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::Land(mat));
            }
        }

        // Apply fall damage is authority-owned; presentation never submits
        // self-damage Combat (rejected) or mutates health locally.

        // Movement exhaustion check
        let horizontal_dist = glam::Vec2::new(
            self.player_physics.position.x - old_pos.x,
            self.player_physics.position.z - old_pos.z,
        )
        .length();
        // Footstep sound update
        if self.player_physics.on_ground {
            if horizontal_dist > 0.0001 {
                let vel_h = glam::Vec2::new(
                    self.player_physics.velocity.x,
                    self.player_physics.velocity.z,
                )
                .length();
                let step_interval = if vel_h > 5.0 { 1.5 } else { 2.0 };
                self.footstep_accumulator += horizontal_dist;
                if self.footstep_accumulator >= step_interval {
                    self.footstep_accumulator = 0.0;
                    if let Some(mat) = under_block.sound_material() {
                        self.audio_manager
                            .play_sound(crate::audio::SoundId::Footstep(mat));
                    }

                    if under_block != BlockType::Air {
                        let feet_pos = glam::Vec3::new(
                            self.player_physics.position.x,
                            (self.player_physics.position.y - 0.05).max(0.0),
                            self.player_physics.position.z,
                        );
                        let mut rng = self
                            .total_time
                            .to_bits()
                            .wrapping_add(self.player_physics.position.x.to_bits());
                        crate::particles::spawn_footstep_dust(
                            &mut self.particles,
                            feet_pos,
                            under_block,
                            &mut rng,
                        );
                    }
                }
            }
        } else {
            self.footstep_accumulator = 0.0;
        }

        if self.player_state.is_sleeping {
            self.player_state.sleep_timer += dt;
        }

        // Void / lava / cactus damage scans are authority-owned. Presentation
        // health comes only from session projection (PlayerHealth / gameplay).

        self.total_time += dt;
        self.end_flash_time = (self.end_flash_time - dt.max(0.0)).max(0.0);

        self.perf_recorder.record(
            crate::perf::ScopeId::WorldTick,
            world_tick_started.elapsed(),
        );
    }

    pub fn use_fishing_rod(&mut self) {
        let action = if self.presented_fishing_hook_entity.is_some() {
            1
        } else {
            0
        };
        let look = self.camera.forward();
        let look_milli = [
            (look.x * 1000.0)
                .round()
                .clamp(i16::MIN as f32, i16::MAX as f32) as i16,
            (look.y * 1000.0)
                .round()
                .clamp(i16::MIN as f32, i16::MAX as f32) as i16,
            (look.z * 1000.0)
                .round()
                .clamp(i16::MIN as f32, i16::MAX as f32) as i16,
        ];
        let _ = self.submit_local_authority_operation(
            crate::network::protocol::GameplayOperation::Fishing {
                action,
                hand: 0,
                look_milli,
            },
        );
    }

    pub fn update_frame(&mut self, dt: f32) {
        let target = self.network_time - REMOTE_INTERPOLATION_DELAY;
        for remote in self.remote_players.values() {
            let Some(snap) = remote.sample(target) else {
                continue;
            };
            if let Some(entity) = self
                .entity_manager
                .entities
                .iter_mut()
                .find(|e| e.id == remote.entity_id)
            {
                entity.velocity = if dt > f32::EPSILON {
                    (snap.position - entity.position) / dt
                } else {
                    Vec3::ZERO
                };
                entity.position = snap.position;
                entity.yaw = snap.yaw;
                entity.pitch = snap.pitch;
                entity.action_cooldown = (entity.action_cooldown - dt).max(0.0);
            }
        }
        self.update_replicated_entity_interpolation();
        self.update_network_position(dt);

        self.debug_frame_time_accumulator += dt;
        self.debug_frame_samples += 1;
        if self.debug_frame_time_accumulator >= DEBUG_STATS_INTERVAL {
            let average_frame_time =
                self.debug_frame_time_accumulator / self.debug_frame_samples as f32;
            self.debug_frame_ms = average_frame_time * 1000.0;
            self.debug_fps = if average_frame_time > f32::EPSILON {
                1.0 / average_frame_time
            } else {
                0.0
            };
            self.debug_frame_time_accumulator = 0.0;
            self.debug_frame_samples = 0;
            self.perf_summaries = self.perf_recorder.snapshot();
        }

        self.advancement_manager.update_toasts(dt);
        if self.advancement_gui.is_open && self.advancement_gui.is_dragging {
            let (screen_w, screen_h) = (self.config.width as f32, self.config.height as f32);
            let mouse_x = (self.mouse_ndc[0] + 1.0) * 0.5 * screen_w;
            let mouse_y = (1.0 - self.mouse_ndc[1]) * 0.5 * screen_h;
            self.advancement_gui.scroll_x = mouse_x - self.advancement_gui.drag_start_x;
            self.advancement_gui.scroll_y = mouse_y - self.advancement_gui.drag_start_y;
        }

        // Advance lightweight particle simulation every frame
        let particles_started = Instant::now();
        self.particles.update(dt);
        self.perf_recorder.record(
            crate::perf::ScopeId::ParticlesUpdate,
            particles_started.elapsed(),
        );

        if self.w_click_timer > 0.0 {
            self.w_click_timer -= dt;
        }

        // Interpolate FOV smoothly
        let target_fov = if self.is_sprinting {
            self.base_fov * 1.12
        } else {
            self.base_fov
        };
        self.camera.fov = self.camera.fov + (target_fov - self.camera.fov) * dt * 10.0;

        // Torch smoke presentation updates
        self.torch_smoke_timer += dt;
        if self.torch_smoke_timer >= 0.4 {
            self.torch_smoke_timer = 0.0;
            let mut rng = self.total_time.to_bits().wrapping_add(0x9E3779B9);
            for chunk in self.chunk_manager.chunks.values() {
                for &encoded in chunk.torch_positions() {
                    let (bx, by, bz) = Chunk::decode_torch_position(encoded);
                    if by % 2 != 0 {
                        continue;
                    }
                    let wx = chunk.chunk_x * CHUNK_WIDTH as i32 + bx as i32;
                    let wz = chunk.chunk_z * CHUNK_DEPTH as i32 + bz as i32;
                    let torch_pos =
                        glam::Vec3::new(wx as f32 + 0.5, by as f32 + 0.6, wz as f32 + 0.5);
                    crate::particles::spawn_torch_smoke(&mut self.particles, torch_pos, &mut rng);
                    rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
                }
            }
        }

        if self.current_dimension == crate::dimension::Dimension::Overworld {
            let lighting_started = Instant::now();
            self.update_weather_effects(dt, false);
            let lighting_elapsed = lighting_started.elapsed();
            self.lighting_time_frame += lighting_elapsed;
            self.lighting_scopes_frame.record(
                crate::perf::LightingSource::Weather as usize,
                lighting_elapsed,
            );
        }

        // Interpolate player position between previous and current simulation snapshot for smooth rendering
        let alpha = (self.sim_accumulator / SIM_TICK_TIME).clamp(0.0, 1.0);
        let interp_player_pos = self
            .prev_player_position
            .lerp(self.player_physics.position, alpha);

        let eye_height = if self.keys.shift { 1.4 } else { 1.6 };
        let player_eye = interp_player_pos + Vec3::new(0.0, eye_height, 0.0);
        let (camera_offset, view_yaw, view_pitch) = perspective_camera_transform(
            self.camera_perspective,
            self.camera.yaw,
            self.camera.pitch,
        );
        self.camera.position = player_eye + camera_offset;
        // Keep gameplay/raycast camera coordinates canonical, and apply bob
        // only to a render copy.  This makes camera bob observable while
        // preserving movement, mining and authority calculations exactly.
        let horizontal_speed = Vec3::new(
            self.player_physics.velocity.x,
            0.0,
            self.player_physics.velocity.z,
        )
        .length();
        let bob = crate::accessibility::camera_bob_offset(
            self.total_time,
            horizontal_speed,
            self.settings.accessibility.camera_bobbing
                && !self.camera_perspective.is_third_person(),
        );
        let camera_right = Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos());
        let presentation_camera = Camera::new(
            self.camera.position + camera_right * bob[0] + Vec3::Y * bob[1],
            view_yaw,
            view_pitch,
            self.camera.fov,
        );
        let is_underwater = self.chunk_manager.get_block(
            self.camera.position.x.floor() as i32,
            self.camera.position.y.floor() as i32,
            self.camera.position.z.floor() as i32,
        ) == BlockType::Water;

        self.camera_uniform.update_view_proj(
            &presentation_camera,
            self.config.width as f32 / self.config.height as f32,
            self.chunk_manager.render_distance as u32,
            self.chunk_manager.dimension.height().height(),
            &self.world_time,
            self.total_time,
            is_underwater,
        );
        self.camera_uniform.camera_pos[3] = self.current_dimension as u8 as f32;
        if self.current_dimension == crate::dimension::Dimension::Overworld {
            let weather_brightness = self.weather.sky_brightness();
            for channel in 0..3 {
                self.camera_uniform.sky_color_top[channel] *= weather_brightness;
                self.camera_uniform.sky_color_horizon[channel] *= weather_brightness;
            }
            self.camera_uniform.sun_dir[3] *= weather_brightness;
        } else if self.current_dimension == crate::dimension::Dimension::Nether {
            self.camera_uniform.sky_color_top = [0.16, 0.018, 0.012, 1.0];
            self.camera_uniform.sky_color_horizon = [0.36, 0.055, 0.025, 1.0];
            self.camera_uniform.sun_dir[3] = 0.55;
        } else {
            self.camera_uniform.sky_color_top = [0.003, 0.002, 0.009, 1.0];
            self.camera_uniform.sky_color_horizon = [0.025, 0.006, 0.04, 1.0];
            self.camera_uniform.sun_dir[3] = 0.35;
        }
        if self.potion_effects.has_night_vision() {
            self.camera_uniform.sun_dir[3] = 1.0;
        }
        let upload_started = Instant::now();
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::cast_slice(&[self.camera_uniform]),
        );
        let upload_elapsed = upload_started.elapsed();
        self.gpu_upload_time_frame += upload_elapsed;
        self.gpu_upload_scopes_frame
            .record(crate::perf::UploadSource::Camera as usize, upload_elapsed);
        self.perf_counters.upload_bytes_frame = self
            .perf_counters
            .upload_bytes_frame
            .saturating_add(std::mem::size_of::<CameraUniform>() as u64);

        // Continuous mining logic
        if allows_continuous_mining(
            self.left_mouse_pressed,
            self.game_mode,
            self.camera_look_allowed(),
        ) {
            let dir = Vec3::new(
                self.camera.yaw.cos() * self.camera.pitch.cos(),
                self.camera.pitch.sin(),
                self.camera.yaw.sin() * self.camera.pitch.cos(),
            )
            .normalize_or_zero();

            if let Some(hit) = raycast(
                self.camera.position,
                dir,
                5.0,
                &self.chunk_manager,
                RaycastTargetPolicy::Break,
            ) {
                let target = hit.block_pos;
                let block =
                    self.chunk_manager
                        .get_block(target.x as i32, target.y as i32, target.z as i32);

                if self.can_break_current_block(block) {
                    let held = self.selected_mining_held();
                    let target_changed = self.mining_target != Some(target);
                    let held_changed = self.mining_held != held;
                    if target_changed || held_changed {
                        self.cancel_authority_break();
                        self.mining_target = Some(target);
                        self.mining_progress = 0.0;
                        self.mining_held = held;
                        let _ = self.submit_local_authority_block_action(
                            crate::network::protocol::BlockActionKind::StartBreak,
                            target.x as i32,
                            target.y as i32,
                            target.z as i32,
                            [hit.normal.x as i8, hit.normal.y as i8, hit.normal.z as i8],
                            BlockType::Air,
                        );
                    }
                } else {
                    self.cancel_authority_break();
                }
            } else {
                self.cancel_authority_break();
            }
        } else {
            self.cancel_authority_break();
        }

        self.perf_recorder
            .record(crate::perf::ScopeId::Lighting, self.lighting_time_frame);
    }

    pub fn update(&mut self, dt: f32) {
        self.sync_translation_catalog();
        self.audio_manager
            .set_subtitles_enabled(self.settings.accessibility.subtitles);
        // Frame instrumentation starts before network ingestion and fixed ticks
        // so catch-up simulation and terrain integration remain in this frame's
        // aggregate and per-category samples.
        self.perf_counters.upload_bytes_frame = 0;
        self.gpu_upload_time_frame = Duration::ZERO;
        self.lighting_time_frame = Duration::ZERO;
        self.lighting_scopes_frame.reset();
        self.gpu_upload_scopes_frame.reset();

        self.network_time += f64::from(dt);
        let network_started = Instant::now();
        self.drain_network_events();
        self.perf_recorder.record(
            crate::perf::ScopeId::NetworkDrain,
            network_started.elapsed(),
        );

        let simulation_enabled = should_advance_simulation(
            &self.role,
            self.network_ready,
            self.is_paused,
            self.player_state.is_dead,
        );
        if simulation_enabled {
            self.sim_accumulator += dt;
            if self.sim_accumulator > SIM_TICK_TIME * MAX_CATCHUP_TICKS as f32 {
                self.sim_accumulator = SIM_TICK_TIME * MAX_CATCHUP_TICKS as f32;
            }

            let mut ticks_run = 0;
            while self.sim_accumulator >= SIM_TICK_TIME && ticks_run < MAX_CATCHUP_TICKS {
                self.tick_simulation(SIM_TICK_TIME);
                self.sim_accumulator -= SIM_TICK_TIME;
                ticks_run += 1;
            }
        } else {
            self.prev_player_position = self.player_physics.position;
        }

        self.update_frame(dt);
        self.process_section_storage_compaction();
    }

    fn update_weather_effects(&mut self, dt: f32, #[allow(unused_variables)] lightning_due: bool) {
        use crate::weather::Precipitation;

        let world_max_y = self.chunk_manager.dimension.height().max_y_exclusive();
        let player_x = self.player_physics.position.x.floor() as i32;
        let player_z = self.player_physics.position.z.floor() as i32;
        if self.weather.precipitation_at(player_x, player_z) == Precipitation::Rain {
            self.audio_manager.start_looping_sound(
                RAIN_LOOP_ID,
                crate::audio::SoundId::Rain,
                self.player_physics.position,
            );
        } else {
            self.audio_manager.stop_looping_sound(RAIN_LOOP_ID);
        }

        let spawn_count = self.weather.take_precipitation_spawn_count(dt);
        let rain_uv = weather_tile_uv(10, 0);
        let snow_uv = weather_tile_uv(3, 1);
        for _ in 0..spawn_count {
            let wx = player_x + self.weather.presentation_random_offset(14);
            let wz = player_z + self.weather.presentation_random_offset(14);
            let precipitation = self.weather.precipitation_at(wx, wz);
            if precipitation == Precipitation::None {
                continue;
            }
            let Some(surface_y) = self.surface_height(wx, wz) else {
                continue;
            };
            if surface_y >= world_max_y - 2 {
                continue;
            }

            // Start above both the camera and the highest block in this column.
            // Lifetime ends at that height, so precipitation never passes through
            // leaves, terrain, or a player-built roof.
            let spawn_y = (self.camera.position.y + 14.0).max(surface_y as f32 + 10.0);
            let stop_y = surface_y as f32 + 1.05;
            match precipitation {
                Precipitation::Rain => {
                    let speed = 26.0 + self.weather.presentation_random_unit() * 8.0;
                    let lifetime = ((spawn_y - stop_y) / speed).clamp(0.08, 2.5);
                    self.particles.spawn_stretched(
                        Vec3::new(wx as f32 + 0.5, spawn_y, wz as f32 + 0.5),
                        Vec3::new(0.0, -speed, 0.0),
                        0.075,
                        lifetime,
                        rain_uv,
                        0.0,
                        7.0,
                    );
                }
                Precipitation::Snow => {
                    let drift_x = (self.weather.presentation_random_unit() - 0.5) * 0.8;
                    let drift_z = (self.weather.presentation_random_unit() - 0.5) * 0.8;
                    let speed = 2.2 + self.weather.presentation_random_unit();
                    let lifetime = ((spawn_y - stop_y) / speed).clamp(0.2, 8.0);
                    self.particles.spawn(
                        Vec3::new(wx as f32 + 0.5, spawn_y, wz as f32 + 0.5),
                        Vec3::new(drift_x, -speed, drift_z),
                        0.16,
                        lifetime,
                        snow_uv,
                        0.0,
                    );
                }
                Precipitation::None => {}
            }
        }
    }

    fn surface_height(&self, wx: i32, wz: i32) -> Option<i32> {
        let ((cx, cz), (bx, _, bz)) = self.chunk_manager.world_to_local(wx, 0, wz)?;
        self.chunk_manager
            .chunks
            .get(&(cx, cz))
            .map(|chunk| chunk.heightmap[bx][bz] as i32)
    }

    fn apply_lightning_strike(&mut self, strike: crate::network::protocol::LightningStrike) {
        let strike_pos = Vec3::new(
            strike.x as f32 + 0.5,
            strike.y as f32,
            strike.z as f32 + 0.5,
        );

        self.weather.trigger_lightning_flash();
        let listener_right =
            Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos()).normalize_or_zero();
        self.audio_manager.play_sound_3d(
            crate::audio::SoundId::Thunder,
            strike_pos,
            self.camera.position,
            listener_right,
        );

        // A short chain of bright, vertically stretched billboards forms the
        // visible bolt and persists just long enough to accompany the flash.
        let bolt_uv = weather_tile_uv(3, 1);
        let mut visual_seed = strike.visual_seed;
        for segment in 0..12 {
            let jitter_x = (crate::weather::seeded_visual_unit(&mut visual_seed) - 0.5) * 0.55;
            let jitter_z = (crate::weather::seeded_visual_unit(&mut visual_seed) - 0.5) * 0.55;
            self.particles.spawn_stretched(
                strike_pos + Vec3::new(jitter_x, segment as f32 * 3.0 + 1.5, jitter_z),
                Vec3::ZERO,
                0.28,
                0.32,
                bolt_uv,
                0.0,
                12.0,
            );
        }
    }

    pub fn update_crack_buffers(
        &self,
        target_pos: Vec3,
        progress: f32,
    ) -> Option<(u32, u32, u64, u64)> {
        let stage = (progress * 10.0).floor().clamp(0.0, 9.0) as u32;
        let wx = target_pos.x;
        let wy = target_pos.y;
        let wz = target_pos.z;

        // Cube corner scale (slightly expanded to 1.002 to avoid z-fighting)
        let s = 1.002f32;
        let offset_min = 0.5 - 0.5 * s;
        let offset_max = 0.5 + 0.5 * s;

        let faces = [
            // South
            (
                [0.0, 0.0, 1.0],
                [
                    ([offset_min, offset_min, offset_max], [0.0, 1.0]),
                    ([offset_max, offset_min, offset_max], [1.0, 1.0]),
                    ([offset_max, offset_max, offset_max], [1.0, 0.0]),
                    ([offset_min, offset_max, offset_max], [0.0, 0.0]),
                ],
            ),
            // North
            (
                [0.0, 0.0, -1.0],
                [
                    ([offset_max, offset_min, offset_min], [0.0, 1.0]),
                    ([offset_min, offset_min, offset_min], [1.0, 1.0]),
                    ([offset_min, offset_max, offset_min], [1.0, 0.0]),
                    ([offset_max, offset_max, offset_min], [0.0, 0.0]),
                ],
            ),
            // West
            (
                [-1.0, 0.0, 0.0],
                [
                    ([offset_min, offset_min, offset_min], [0.0, 1.0]),
                    ([offset_min, offset_min, offset_max], [1.0, 1.0]),
                    ([offset_min, offset_max, offset_max], [1.0, 0.0]),
                    ([offset_min, offset_max, offset_min], [0.0, 0.0]),
                ],
            ),
            // East
            (
                [1.0, 0.0, 0.0],
                [
                    ([offset_max, offset_min, offset_max], [0.0, 1.0]),
                    ([offset_max, offset_min, offset_min], [1.0, 1.0]),
                    ([offset_max, offset_max, offset_min], [1.0, 0.0]),
                    ([offset_max, offset_max, offset_max], [0.0, 0.0]),
                ],
            ),
            // Up
            (
                [0.0, 1.0, 0.0],
                [
                    ([offset_min, offset_max, offset_max], [0.0, 1.0]),
                    ([offset_max, offset_max, offset_max], [1.0, 1.0]),
                    ([offset_max, offset_max, offset_min], [1.0, 0.0]),
                    ([offset_min, offset_max, offset_min], [0.0, 0.0]),
                ],
            ),
            // Down
            (
                [0.0, -1.0, 0.0],
                [
                    ([offset_min, offset_min, offset_min], [0.0, 1.0]),
                    ([offset_max, offset_min, offset_min], [1.0, 1.0]),
                    ([offset_max, offset_min, offset_max], [1.0, 0.0]),
                    ([offset_min, offset_min, offset_max], [0.0, 0.0]),
                ],
            ),
        ];

        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        let sky_light = self
            .chunk_manager
            .get_sky_light(wx as i32, wy as i32, wz as i32);
        let block_light = self
            .chunk_manager
            .get_block_light(wx as i32, wy as i32, wz as i32);

        for (face_idx, (_normal, corners)) in faces.iter().enumerate() {
            let start_idx = vertices.len() as u32;
            let multiplier_code = match face_idx {
                4 => 0.0, // Top
                5 => 2.0, // Bottom
                _ => 1.0, // Sides
            };
            let light_val =
                (sky_light as f32) + (block_light as f32) * 16.0 + multiplier_code * 256.0;

            for &(corner, uv) in corners {
                // UV points to Row 15, Col "stage"
                let u = (uv[0] + stage as f32) * 0.0625;
                let v = (uv[1] + 15.0) * 0.0625;
                vertices.push(Vertex {
                    position: [wx + corner[0], wy + corner[1], wz + corner[2]],
                    tex_coords: [u, v],
                    light_level: light_val,
                    ao: 1.0,
                });
            }

            indices.push(start_idx + 0);
            indices.push(start_idx + 1);
            indices.push(start_idx + 2);
            indices.push(start_idx + 0);
            indices.push(start_idx + 2);
            indices.push(start_idx + 3);
        }

        let upload_started = Instant::now();
        self.queue.write_buffer(
            &self.crack_vertex_buffer,
            0,
            bytemuck::cast_slice(&vertices),
        );
        self.queue
            .write_buffer(&self.crack_index_buffer, 0, bytemuck::cast_slice(&indices));

        Some((
            vertices.len() as u32,
            indices.len() as u32,
            upload_started.elapsed().as_nanos() as u64,
            (vertices.len() * std::mem::size_of::<Vertex>()
                + indices.len() * std::mem::size_of::<u32>()) as u64,
        ))
    }

    fn damage_selected_tool(&mut self, salt: u32) {
        if self.game_mode == GameMode::Creative {
            return;
        }
        let selected = self.inventory.selected;
        let should_damage = self.inventory.hotbar[selected]
            .filter(|stack| stack.item.tool_properties().is_some())
            .is_some_and(|stack| {
                crate::enchantment::should_consume_durability(&stack.enchantments, salt)
            });
        if !should_damage {
            return;
        }
        if let Some(stack) = &mut self.inventory.hotbar[selected] {
            if stack.durability > 1 {
                stack.durability -= 1;
            } else {
                println!("[Debug] Tool broke: {:?}", stack.item);
                self.inventory.hotbar[selected] = None;
            }
        }
    }

    /// Client-side application of an authoritative block change received from
    /// the host. Mirrors the mutation half of the canonical path: `set_block`,
    /// lighting, mesh invalidation. Redstone is intentionally **not** rescanned
    /// - the host runs the redstone simulation and broadcasts its actuator
    /// effects as further `BlockChange`s, so running it here would double-apply
    /// and could diverge.
    fn apply_remote_block_change(
        &mut self,
        dimension_wire: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        block_wire: u32,
        state: u8,
        raw_fluid: u8,
    ) {
        let Some(dimension) = crate::dimension::Dimension::from_wire(dimension_wire) else {
            return;
        };
        if dimension != self.current_dimension {
            return;
        }
        let block = match BlockType::from_wire(block_wire) {
            Some(b) => b,
            None => return,
        };
        let Some(((cx, cz), _)) = self.chunk_manager.world_to_local(x, y, z) else {
            return;
        };
        let revision_key = (dimension, cx, cz);
        if revision
            <= self
                .client_chunk_revisions
                .get(&revision_key)
                .copied()
                .unwrap_or(0)
        {
            return;
        }
        self.client_chunk_revisions.insert(revision_key, revision);
        if !self.chunk_manager.chunks.contains_key(&(cx, cz)) {
            self.pending_block_changes
                .entry((cx, cz))
                .or_default()
                .insert((x, y, z), (revision, block_wire, state, raw_fluid));
            return;
        }
        let previous_block = self.chunk_manager.get_block(x, y, z);
        let previous_state = self.chunk_manager.get_block_state(x, y, z);
        let previous_raw_fluid = self.chunk_manager.get_fluid_raw(x, y, z);
        self.play_chest_state_edge((x, y, z), previous_block, previous_state, block, state);
        let mut dirty_chunks =
            apply_synced_block_change(&mut self.chunk_manager, x, y, z, block, state)
                .unwrap_or_default();
        if previous_raw_fluid != raw_fluid {
            self.chunk_manager.set_fluid_raw(x, y, z, raw_fluid);
            crate::chunk_manager::mark_block_mesh_dependencies(&mut dirty_chunks, x, z);
        }
        if previous_block == block && previous_state == state && previous_raw_fluid == raw_fluid {
            return;
        }
        self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::Network);
    }

    /// Client-side application of an incremental block entity update from host.
    fn apply_remote_block_entity_delta(
        &mut self,
        dimension_wire: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        entity: Option<crate::block_entity::BlockEntity>,
    ) {
        let Some(dimension) = crate::dimension::Dimension::from_wire(dimension_wire) else {
            return;
        };
        if dimension != self.current_dimension {
            return;
        }
        let Some(((cx, cz), (bx, by, bz))) = self.chunk_manager.world_to_local(x, y, z) else {
            return;
        };
        let revision_key = (dimension, cx, cz);
        if revision
            <= self
                .client_chunk_revisions
                .get(&revision_key)
                .copied()
                .unwrap_or(0)
        {
            return;
        }
        self.client_chunk_revisions.insert(revision_key, revision);
        if let Some(chunk) = self.chunk_manager.chunks.get_mut(&(cx, cz)) {
            if let Some(ent) = entity {
                let _ = chunk.insert_block_entity(bx as u8, by as i16, bz as u8, ent);
            } else {
                chunk.remove_block_entity(bx as u8, by as i16, bz as u8);
            }
        }
    }

    /// Client-side application of a full chunk payload sent by the host during
    /// mid-game join catch-up. Live projection sends uncompressed terrain
    /// streams; restore also accepts the historical zlib `ChunkSaveData` layout.
    /// Missing columns are inserted from the payload only — join clients never
    /// generate a stand-in.
    fn apply_remote_chunk_data(
        &mut self,
        dimension_wire: u8,
        cx: i32,
        cz: i32,
        revision: u64,
        _min_section_y: i8,
        _section_count: u16,
        blocks: Vec<u8>,
        block_states: Vec<u8>,
        fluid_levels: Vec<u8>,
        block_entities: Vec<u8>,
    ) {
        let Some(dimension) = crate::dimension::Dimension::from_wire(dimension_wire) else {
            return;
        };
        if dimension != self.current_dimension {
            return;
        }
        let revision_key = (dimension, cx, cz);
        if revision
            < self
                .client_chunk_revisions
                .get(&revision_key)
                .copied()
                .unwrap_or(0)
        {
            return;
        }
        self.client_chunk_revisions.insert(revision_key, revision);
        let inserted_new = !self.chunk_manager.chunks.contains_key(&(cx, cz));
        if inserted_new {
            if self
                .chunk_manager
                .insert_authoritative_chunk_payload(
                    cx,
                    cz,
                    &blocks,
                    &block_states,
                    &fluid_levels,
                    &block_entities,
                )
                .is_err()
            {
                return;
            }
            let lifetime = self.next_chunk_lifetime();
            self.chunk_lifetimes.insert((cx, cz), lifetime);
            self.chunk_meshes.insert((cx, cz), ChunkMesh::pending());
        } else if let Some(chunk) = self.chunk_manager.chunks.get_mut(&(cx, cz)) {
            Self::restore_chunk_payload(
                chunk,
                &blocks,
                &block_states,
                &fluid_levels,
                &block_entities,
            );
        } else {
            return;
        }
        self.invalidate_chunk_mesh(
            (cx, cz),
            if inserted_new {
                DependencyReason::ChunkLoad
            } else {
                DependencyReason::Network
            },
        );
        if let Some(changes) = self.pending_block_changes.remove(&(cx, cz)) {
            let mut changes: Vec<_> = changes.into_iter().collect();
            changes.sort_by_key(|(_, (change_revision, _, _, _))| *change_revision);
            for ((x, y, z), (change_revision, block, state, raw_fluid)) in changes {
                self.apply_remote_block_change(
                    dimension_wire,
                    change_revision,
                    x,
                    y,
                    z,
                    block,
                    state,
                    raw_fluid,
                );
            }
        }
        // Re-seed boundary lighting so neighbors pick up the overwritten
        // column heights and light values.
        let mut dirty_chunks = std::collections::HashSet::new();
        for (lighting_cx, lighting_cz) in [
            (cx, cz),
            (cx - 1, cz),
            (cx + 1, cz),
            (cx, cz - 1),
            (cx, cz + 1),
        ] {
            if self
                .chunk_manager
                .chunks
                .contains_key(&(lighting_cx, lighting_cz))
            {
                crate::lighting::propagate_chunk_lighting(
                    &mut self.chunk_manager,
                    lighting_cx,
                    lighting_cz,
                    &mut dirty_chunks,
                );
                self.invalidate_chunk_mesh((lighting_cx, lighting_cz), DependencyReason::Light);
            }
        }
        self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::Light);
    }

    /// Decode a `ChunkSaveData`-style compressed payload into an existing
    /// chunk. Reused by both the save loader and the network catch-up path so
    /// the wire format stays identical to the on-disk format.
    fn restore_chunk_payload(
        chunk: &mut crate::world::Chunk,
        blocks: &[u8],
        block_states: &[u8],
        fluid_levels: &[u8],
        block_entities: &[u8],
    ) {
        let _ = crate::save::ChunkSaveData::restore_network_payload(
            chunk,
            blocks,
            block_states,
            fluid_levels,
            block_entities,
        );
    }

    pub fn respawn(&mut self) {
        if self.has_in_process_runtime() {
            let _ = self.submit_local_authority_command("/respawn");
            return;
        }
        if self.presentation_topology().is_join_client() {
            self.network.send_respawn_request();
            return;
        }
    }

    pub fn handle_death_click(&mut self) {
        let mouse_x = self.mouse_ndc[0];
        let mouse_y = self.mouse_ndc[1];

        // Respawn button: bounds X: [-0.3, 0.3], Y: [-0.1, 0.0]
        if mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.1 && mouse_y <= 0.0 {
            if matches!(&self.network, NetworkHandle::Client { .. }) {
                self.network.send_respawn_request();
            } else {
                self.respawn();
            }
        }
    }

    pub fn handle_primary_press(&mut self) -> bool {
        self.hand_swing_started_at = self.total_time;
        self.hand_swing_until = self.total_time + 0.25;
        let melee_consumed = self.submit_local_authority_combat();
        let decision = primary_press_decision(self.game_mode, melee_consumed);
        if decision.instant_break {
            self.handle_click(true);
        }
        decision.keep_held_mining
    }

    /// Combat is a typed authority operation even when the current core does
    /// not yet implement the entity mutation.  A rejected operation consumes
    /// the input and never falls through to renderer-side health mutation.
    fn submit_local_authority_combat(&mut self) -> bool {
        let direction = Vec3::new(
            self.camera.yaw.cos() * self.camera.pitch.cos(),
            self.camera.pitch.sin(),
            self.camera.yaw.sin() * self.camera.pitch.cos(),
        )
        .normalize_or_zero();
        let Some(entity_id) = closest_melee_target(
            &self.entity_manager,
            self.camera.position,
            direction,
            MELEE_REACH,
        ) else {
            return false;
        };
        let _ = self.submit_local_authority_operation(
            crate::network::protocol::GameplayOperation::Combat {
                target: entity_id,
                action: 0,
            },
        );
        true
    }

    pub fn handle_secondary_press(&mut self) {
        if self.is_paused
            || self.inventory.is_open
            || self.is_chat_open
            || self.player_state.is_dead
        {
            return;
        }

        let main_stack = self.inventory.hotbar[self.inventory.selected];
        let main_item = main_stack.map(|s| s.item).unwrap_or(Item::Air);
        let offhand_stack = self.inventory.offhand;
        let offhand_item = offhand_stack.map(|s| s.item).unwrap_or(Item::Air);

        if main_item == Item::FishingRod {
            self.use_fishing_rod();
            return;
        }

        if main_item != Item::Air && !main_item.properties().is_block {
            let _ = self.submit_local_authority_operation(
                crate::network::protocol::GameplayOperation::ItemUse {
                    item: main_item as u32,
                    count: 1,
                },
            );
            return;
        }

        if self.presentation_topology().is_join_client() && main_item == Item::Shield {
            let _ = self.submit_local_authority_operation(
                crate::network::protocol::GameplayOperation::UseState {
                    hand: 0,
                    active: true,
                },
            );
            return;
        }

        // Mainhand Shield
        if main_item == Item::Shield && self.player_state.shield_disable_ticks == 0 {
            self.player_state.using_item = Some(crate::player::UsingItemState {
                hand: crate::player::Hand::MainHand,
                action: crate::player::ItemUseAction::Block,
                item: Item::Shield,
                slot: crate::player::HandSlot::MainHand(self.inventory.selected),
                ticks_held: 0,
                max_ticks: None,
            });
            return;
        }
        // Mainhand Bow
        if main_item == Item::Bow {
            self.player_state.using_item = Some(crate::player::UsingItemState {
                hand: crate::player::Hand::MainHand,
                action: crate::player::ItemUseAction::Bow,
                item: Item::Bow,
                slot: crate::player::HandSlot::MainHand(self.inventory.selected),
                ticks_held: 0,
                max_ticks: None,
            });
            return;
        }
        // Mainhand Food / Drink
        if let Some(food_props) = main_item.food_properties() {
            if self.player_state.hunger < 20.0
                || food_props.always_edible
                || self.game_mode == GameMode::Creative
            {
                self.player_state.using_item = Some(crate::player::UsingItemState {
                    hand: crate::player::Hand::MainHand,
                    action: crate::player::ItemUseAction::Eat,
                    item: main_item,
                    slot: crate::player::HandSlot::MainHand(self.inventory.selected),
                    ticks_held: 0,
                    max_ticks: Some(food_props.use_duration_ticks),
                });
                return;
            }
        }
        if main_item == Item::MilkBucket || main_stack.and_then(|s| s.potion).is_some() {
            self.player_state.using_item = Some(crate::player::UsingItemState {
                hand: crate::player::Hand::MainHand,
                action: crate::player::ItemUseAction::Drink,
                item: main_item,
                slot: crate::player::HandSlot::MainHand(self.inventory.selected),
                ticks_held: 0,
                max_ticks: Some(32),
            });
            return;
        }

        let dir = Vec3::new(
            self.camera.yaw.cos() * self.camera.pitch.cos(),
            self.camera.pitch.sin(),
            self.camera.yaw.sin() * self.camera.pitch.cos(),
        );
        let villager_hit = self
            .entity_manager
            .query_radius_types(
                self.camera.position,
                4.0,
                &[crate::entity::EntityType::Villager],
            )
            .find(|e| {
                e.health > 0.0
                    && crate::entity::ray_intersects_aabb(self.camera.position, dir, &e.get_aabb())
                        .is_some()
            })
            .map(|e| e.id);

        if let Some(vid) = villager_hit {
            self.open_merchant_trade_window(vid);
            return;
        }

        // Try standard click (block interaction/placement) with mainhand
        self.handle_click(false);

        // If mainhand didn't start an item use action, check Offhand item
        if self.player_state.using_item.is_none() {
            if offhand_item != Item::Air && !offhand_item.properties().is_block {
                let _ = self.submit_local_authority_operation(
                    crate::network::protocol::GameplayOperation::ItemUse {
                        item: offhand_item as u32,
                        count: 1,
                    },
                );
                return;
            }
            if self.presentation_topology().is_join_client() && offhand_item == Item::Shield {
                let _ = self.submit_local_authority_operation(
                    crate::network::protocol::GameplayOperation::UseState {
                        hand: 1,
                        active: true,
                    },
                );
                return;
            }
            if offhand_item == Item::Shield && self.player_state.shield_disable_ticks == 0 {
                self.player_state.using_item = Some(crate::player::UsingItemState {
                    hand: crate::player::Hand::OffHand,
                    action: crate::player::ItemUseAction::Block,
                    item: Item::Shield,
                    slot: crate::player::HandSlot::OffHand,
                    ticks_held: 0,
                    max_ticks: None,
                });
                return;
            }
            if offhand_item == Item::Bow {
                self.player_state.using_item = Some(crate::player::UsingItemState {
                    hand: crate::player::Hand::OffHand,
                    action: crate::player::ItemUseAction::Bow,
                    item: Item::Bow,
                    slot: crate::player::HandSlot::OffHand,
                    ticks_held: 0,
                    max_ticks: None,
                });
                return;
            }
            if let Some(food_props) = offhand_item.food_properties() {
                if self.player_state.hunger < 20.0
                    || food_props.always_edible
                    || self.game_mode == GameMode::Creative
                {
                    self.player_state.using_item = Some(crate::player::UsingItemState {
                        hand: crate::player::Hand::OffHand,
                        action: crate::player::ItemUseAction::Eat,
                        item: offhand_item,
                        slot: crate::player::HandSlot::OffHand,
                        ticks_held: 0,
                        max_ticks: Some(food_props.use_duration_ticks),
                    });
                    return;
                }
            }
            if offhand_item == Item::MilkBucket || offhand_stack.and_then(|s| s.potion).is_some() {
                self.player_state.using_item = Some(crate::player::UsingItemState {
                    hand: crate::player::Hand::OffHand,
                    action: crate::player::ItemUseAction::Drink,
                    item: offhand_item,
                    slot: crate::player::HandSlot::OffHand,
                    ticks_held: 0,
                    max_ticks: Some(32),
                });
                return;
            }
        }
    }

    pub fn handle_secondary_release(&mut self) {
        if self.has_in_process_runtime() {
            self.player_state.using_item = None;
            return;
        }
        if self.presentation_topology().is_join_client() {
            let hand = if self
                .player_state
                .using_item
                .is_some_and(|using| matches!(using.hand, crate::player::Hand::OffHand))
            {
                1
            } else {
                0
            };
            let _ = self.submit_local_authority_operation(
                crate::network::protocol::GameplayOperation::UseState {
                    hand,
                    active: false,
                },
            );
            self.player_state.using_item = None;
            return;
        }
        self.player_state.using_item = None;
    }

    pub fn handle_click(&mut self, is_left_click: bool) {
        self.handle_live_world_click(is_left_click);
    }

    fn look_direction(&self) -> Vec3 {
        Vec3::new(
            self.camera.yaw.cos() * self.camera.pitch.cos(),
            self.camera.pitch.sin(),
            self.camera.yaw.sin() * self.camera.pitch.cos(),
        )
        .normalize_or_zero()
    }

    fn prepare_world_click(&self, is_left_click: bool) -> (WorldClickIntent, Option<Vec3>) {
        let direction = self.look_direction();
        let target_policy = if is_left_click {
            RaycastTargetPolicy::Break
        } else {
            RaycastTargetPolicy::Place
        };
        let Some(hit) = raycast(
            self.camera.position,
            direction,
            5.0,
            &self.chunk_manager,
            target_policy,
        ) else {
            return (WorldClickIntent::Miss, None);
        };
        let clicked = [
            hit.block_pos.x as i32,
            hit.block_pos.y as i32,
            hit.block_pos.z as i32,
        ];
        let place_pos = hit.block_pos + hit.normal;
        let place = [place_pos.x as i32, place_pos.y as i32, place_pos.z as i32];
        let clicked_block = self
            .chunk_manager
            .get_block(clicked[0], clicked[1], clicked[2]);
        let held_item = self.inventory.hotbar[self.inventory.selected]
            .map(|stack| stack.item)
            .unwrap_or(Item::Air);
        let selected_block = self.inventory.get_selected_block();
        let can_break = self.can_break_current_block(clicked_block);
        let can_place = selected_block
            .map(|block| self.can_place_block_at(place[0], place[1], place[2], block))
            .unwrap_or(true);
        let intent = resolve_world_click(
            self.presentation_topology(),
            is_left_click,
            Some(WorldClickHit {
                clicked,
                place,
                face: [hit.normal.x as i8, hit.normal.y as i8, hit.normal.z as i8],
                clicked_block,
            }),
            held_item,
            selected_block,
            can_break,
            can_place,
        );
        (intent, Some(hit.block_pos))
    }

    fn handle_live_world_click(&mut self, is_left_click: bool) {
        let (intent, hit_pos) = self.prepare_world_click(is_left_click);
        match intent {
            WorldClickIntent::Miss | WorldClickIntent::Rejected => {}
            WorldClickIntent::StartBreak { x, y, z, face } => {
                let _ = self.submit_local_authority_block_action(
                    crate::network::protocol::BlockActionKind::StartBreak,
                    x,
                    y,
                    z,
                    face,
                    BlockType::Air,
                );
                self.mining_target = hit_pos;
                self.mining_progress = 0.0;
                self.mining_held = self.selected_mining_held();
                if self.presentation_topology().is_embedded() {
                    self.network
                        .send_action(crate::network::protocol::Action::Break);
                }
            }
            WorldClickIntent::IgnitePortal { x, y, z, face } => {
                let _ = self.submit_local_authority_block_action(
                    crate::network::protocol::BlockActionKind::IgnitePortal,
                    x,
                    y,
                    z,
                    face,
                    BlockType::Fire,
                );
            }
            WorldClickIntent::InsertEnderEye { x, y, z, face } => {
                let _ = self.submit_local_authority_block_action(
                    crate::network::protocol::BlockActionKind::InsertEnderEye,
                    x,
                    y,
                    z,
                    face,
                    BlockType::EndPortalFrameFilled,
                );
            }
            WorldClickIntent::Sleep { x, y, z } => {
                let _ = self.submit_local_authority_operation(
                    crate::network::protocol::GameplayOperation::Sleep { x, y, z },
                );
            }
            WorldClickIntent::OpenContainer { x, y, z, .. } => {
                if self.presentation_topology().is_join_client() {
                    self.submit_join_container_open((x, y, z));
                } else {
                    let _ = self.submit_local_authority_operation(
                        crate::network::protocol::GameplayOperation::Container {
                            action: crate::network::protocol::ContainerAction::Open.to_wire(),
                            x,
                            y,
                            z,
                            slot: 0,
                        },
                    );
                }
            }
            WorldClickIntent::Place {
                x,
                y,
                z,
                face,
                block,
            } => {
                let _ = self.submit_local_authority_block_action(
                    crate::network::protocol::BlockActionKind::Place,
                    x,
                    y,
                    z,
                    face,
                    block,
                );
                if self.presentation_topology().is_embedded() && block != BlockType::Air {
                    self.network
                        .send_action(crate::network::protocol::Action::Place);
                }
            }
        }
    }

    pub fn is_creative_catalog_open(&self) -> bool {
        self.inventory.is_open
            && inventory_layout_kind(
                self.game_mode,
                self.active_station.is_some(),
                self.inventory.is_table_open,
            ) == InventoryLayoutKind::CreativeCatalog
    }

    pub fn get_inventory_slots(&self) -> Vec<(SlotType, f32, f32, f32, f32)> {
        let aspect = self.size.width as f32 / self.size.height as f32;
        if inventory_layout_kind(
            self.game_mode,
            self.active_station.is_some(),
            self.inventory.is_table_open,
        ) == InventoryLayoutKind::CreativeCatalog
        {
            let mut slots = Vec::with_capacity(CREATIVE_VISIBLE_SLOTS + 9);
            for (index, item) in self
                .inventory
                .creative_visible_items()
                .into_iter()
                .enumerate()
            {
                let rect = creative_catalog_slot_rect(index, aspect);
                slots.push((SlotType::Creative(item), rect.x0, rect.x1, rect.y0, rect.y1));
            }
            for index in 0..9 {
                let rect = creative_hotbar_slot_rect(index, aspect);
                slots.push((SlotType::Hotbar(index), rect.x0, rect.x1, rect.y0, rect.y1));
            }
            return slots;
        }

        let slot_w = 0.08;
        let slot_h = 0.08 * aspect;
        let gap = 0.01;
        let mut slots = Vec::new();

        // 1. Hotbar (0..9)
        for i in 0..9 {
            let x0 = -0.40 + i as f32 * (slot_w + gap);
            let y0 = -0.85;
            slots.push((SlotType::Hotbar(i), x0, x0 + slot_w, y0, y0 + slot_h));
        }

        // 2. Backpack (0..27)
        for r in 0..3 {
            for c in 0..9 {
                let i = r * 9 + c;
                let x0 = -0.40 + c as f32 * (slot_w + gap);
                let y0 = -0.70 + r as f32 * (slot_h + gap);
                slots.push((SlotType::Backpack(i), x0, x0 + slot_w, y0, y0 + slot_h));
            }
        }

        // 3. Armor (0..4)
        for i in 0..4 {
            let x0 = -0.40;
            let y0 = -0.15 + i as f32 * (slot_h + gap);
            slots.push((SlotType::Armor(i), x0, x0 + slot_w, y0, y0 + slot_h));
        }

        // 3b. Offhand
        let offhand_x0 = -0.40 + 1.2 * (slot_w + gap);
        let offhand_y0 = -0.15;
        slots.push((
            SlotType::Offhand,
            offhand_x0,
            offhand_x0 + slot_w,
            offhand_y0,
            offhand_y0 + slot_h,
        ));

        // 4. Container slots (if chest or furnace is open)
        if let Some(pos) = self.container_target {
            let block = self.chunk_manager.get_block(pos.0, pos.1, pos.2);
            if matches!(block, BlockType::Furnace | BlockType::FurnaceLit) {
                let in_x0 = -0.15;
                let in_y0 = 0.10;
                slots.push((
                    SlotType::ContainerSlot(0),
                    in_x0,
                    in_x0 + slot_w,
                    in_y0,
                    in_y0 + slot_h,
                ));

                let fuel_x0 = -0.15;
                let fuel_y0 = -0.10;
                slots.push((
                    SlotType::ContainerSlot(1),
                    fuel_x0,
                    fuel_x0 + slot_w,
                    fuel_y0,
                    fuel_y0 + slot_h,
                ));

                let out_x0 = 0.15;
                let out_y0 = 0.0;
                slots.push((
                    SlotType::ContainerSlot(2),
                    out_x0,
                    out_x0 + slot_w,
                    out_y0,
                    out_y0 + slot_h,
                ));
            } else {
                let container_slots = self.chunk_manager.container_slot_count(pos.0, pos.1, pos.2);
                let container_rows = container_slots.saturating_add(8) / 9;
                let x_start = -0.40;
                let y_start = -0.70 - (container_rows as f32) * (slot_h + gap) - gap;
                for r in 0..container_rows {
                    for c in 0..9 {
                        let i = r * 9 + c;
                        let x0 = x_start + c as f32 * (slot_w + gap);
                        let y0 = y_start + r as f32 * (slot_h + gap);
                        slots.push((SlotType::ContainerSlot(i), x0, x0 + slot_w, y0, y0 + slot_h));
                    }
                }
            }
        }

        // 5. Crafting Grid & Output
        if self.container_target.is_none()
            && self.active_station.is_none()
            && self.inventory.is_table_open
        {
            // 3x3 table
            let x_start = -0.05;
            for r in 0..3 {
                for c in 0..3 {
                    let i = r * 3 + c;
                    let x0 = x_start + c as f32 * (slot_w + gap);
                    let y0 = -0.10 + r as f32 * (slot_h + gap);
                    slots.push((SlotType::CraftInput(i), x0, x0 + slot_w, y0, y0 + slot_h));
                }
            }
            // Output
            let x0 = x_start + 3.0 * (slot_w + gap) + 0.06;
            let y0 = -0.10 + 1.0 * (slot_h + gap);
            slots.push((SlotType::CraftOutput, x0, x0 + slot_w, y0, y0 + slot_h));
        } else if self.active_station.is_none() {
            // 2x2 player craft
            let x_start = 0.05;
            for r in 0..2 {
                for c in 0..2 {
                    let i = r * 2 + c;
                    let x0 = x_start + c as f32 * (slot_w + gap);
                    let y0 = -0.05 + r as f32 * (slot_h + gap);
                    slots.push((SlotType::CraftInput(i), x0, x0 + slot_w, y0, y0 + slot_h));
                }
            }
            // Output
            let x0 = x_start + 2.0 * (slot_w + gap) + 0.06;
            let y0 = -0.05 + 0.5 * (slot_h + gap);
            slots.push((SlotType::CraftOutput, x0, x0 + slot_w, y0, y0 + slot_h));
        }

        match self.active_station {
            Some(StationKind::Enchanting) => {
                slots.push((
                    SlotType::EnchantInput,
                    -0.18,
                    -0.18 + slot_w,
                    0.12,
                    0.12 + slot_h,
                ));
                slots.push((
                    SlotType::EnchantLapis,
                    -0.18,
                    -0.18 + slot_w,
                    -0.02,
                    -0.02 + slot_h,
                ));
            }
            Some(StationKind::Brewing) => {
                for i in 0..3 {
                    let x0 = -0.18 + i as f32 * (slot_w + gap);
                    slots.push((
                        SlotType::BrewBottle(i),
                        x0,
                        x0 + slot_w,
                        -0.02,
                        -0.02 + slot_h,
                    ));
                }
                slots.push((
                    SlotType::BrewIngredient,
                    -0.09,
                    -0.09 + slot_w,
                    0.17,
                    0.17 + slot_h,
                ));
            }
            Some(StationKind::Anvil) => {
                slots.push((
                    SlotType::AnvilLeft,
                    -0.20,
                    -0.20 + slot_w,
                    0.10,
                    0.10 + slot_h,
                ));
                slots.push((
                    SlotType::AnvilRight,
                    -0.05,
                    -0.05 + slot_w,
                    0.10,
                    0.10 + slot_h,
                ));
                slots.push((
                    SlotType::AnvilOutput,
                    0.20,
                    0.20 + slot_w,
                    0.10,
                    0.10 + slot_h,
                ));
            }
            None | Some(StationKind::Merchant) => {}
        }

        slots
    }

    pub fn get_item_at_slot(&self, slot: SlotType) -> Option<ItemStack> {
        match slot {
            SlotType::Creative(item) => Some(ItemStack::new(item, 1)),
            SlotType::Hotbar(i) => self.inventory.hotbar[i],
            SlotType::Backpack(i) => self.inventory.main[i],
            SlotType::Armor(i) => self.inventory.armor[i],
            SlotType::Offhand => self.inventory.offhand,
            SlotType::CraftInput(i) => self.inventory.craft_input.get(i).copied().flatten(),
            SlotType::CraftOutput => self.inventory.craft_output,
            SlotType::EnchantInput => self.enchanting.input,
            SlotType::EnchantLapis => self.enchanting.lapis,
            SlotType::BrewBottle(i) => self.brewing.bottles[i],
            SlotType::BrewIngredient => self.brewing.ingredient,
            SlotType::AnvilLeft => self.anvil.left,
            SlotType::AnvilRight => self.anvil.right,
            SlotType::AnvilOutput => self.anvil.output,
            SlotType::ContainerSlot(i) => self.container_target.and_then(|pos| {
                self.chunk_manager
                    .container_slots(pos.0, pos.1, pos.2)
                    .and_then(|slots| slots.get(i).copied().flatten())
            }),
        }
    }

    pub fn set_item_at_slot(&mut self, slot: SlotType, stack: Option<ItemStack>) {
        if self.presentation_topology().is_join_client() {
            return;
        }
        // Presentation never mutates authority-owned container slots locally;
        // container clicks go through GameplayOperation::ContainerClick.
        if matches!(slot, SlotType::ContainerSlot(_)) {
            return;
        }
        match slot {
            SlotType::Creative(item) => self.inventory.write_creative_slot(item, stack),
            SlotType::Hotbar(i) => self.inventory.hotbar[i] = stack,
            SlotType::Backpack(i) => self.inventory.main[i] = stack,
            SlotType::Armor(i) => self.inventory.armor[i] = stack,
            SlotType::Offhand => self.inventory.offhand = stack,
            SlotType::CraftInput(i) => {
                if i < self.inventory.craft_input.len() {
                    self.inventory.craft_input[i] = stack;
                }
            }
            SlotType::CraftOutput => self.inventory.craft_output = stack,
            SlotType::EnchantInput => self.enchanting.input = stack,
            SlotType::EnchantLapis => self.enchanting.lapis = stack,
            SlotType::BrewBottle(i) => self.brewing.bottles[i] = stack,
            SlotType::BrewIngredient => self.brewing.ingredient = stack,
            SlotType::AnvilLeft => self.anvil.left = stack,
            SlotType::AnvilRight => self.anvil.right = stack,
            SlotType::AnvilOutput => {}
            SlotType::ContainerSlot(_) => {}
        }
    }

    pub fn handle_swap_offhand_pressed(&mut self) {
        let _ = (self.is_chat_open, self.is_paused, self.player_state.is_dead);
    }

    pub fn select_hotbar_slot(&mut self, slot: usize) {
        self.inventory.selected = slot.min(8);
        if self.presentation_topology().should_sync_inventory() {
            self.sync_authority_gameplay_from_local();
        }
    }

    fn refresh_workstations(&mut self) {
        self.enchanting.refresh();
        self.anvil.refresh();
    }

    fn resolve_inventory_hit(
        &self,
        is_left: bool,
    ) -> Option<(PresentationInventoryTarget, InventoryHit<SlotType>)> {
        let hit = self.probe_inventory_click(is_left).authority_hit();
        presentation_target_for_authority_hit(hit).map(|target| (target, hit))
    }

    fn presentation_inventory_click_target(&self) -> Option<PresentationInventoryTarget> {
        // Writeback has no mouse-button; overlay geometry matches the historical
        // always-on hit test (`collect_inventory_ui_hits` with `is_left = true`).
        self.resolve_inventory_hit(true).map(|(target, _)| target)
    }

    pub(crate) fn should_writeback_after_inventory_click(&self) -> bool {
        self.presentation_topology()
            .should_writeback_after_inventory_click(self.presentation_inventory_click_target())
    }

    fn probe_inventory_click(&self, is_left: bool) -> InventoryHitProbe<SlotType> {
        let mouse_x = self.mouse_ndc[0];
        let mouse_y = self.mouse_ndc[1];
        let ui = collect_inventory_ui_hits(
            mouse_x,
            mouse_y,
            is_left,
            self.active_station == Some(StationKind::Merchant),
            self.active_merchant_offers.len(),
            self.recipe_book_open,
            self.active_station == Some(StationKind::Enchanting),
        );
        let creative_tab = if self.is_creative_catalog_open() && is_left {
            (0..CreativeTab::TABS.len())
                .find(|&index| creative_tab_rect(index).contains(mouse_x, mouse_y))
        } else {
            None
        };
        let slot = self
            .get_inventory_slots()
            .into_iter()
            .find(|&(_, x0, x1, y0, y1)| {
                mouse_x >= x0 && mouse_x <= x1 && mouse_y >= y0 && mouse_y <= y1
            })
            .map(|(slot, _, _, _, _)| slot);
        InventoryHitProbe {
            merchant: ui.merchant,
            creative_tab,
            recipe_book_toggle: ui.recipe_book_toggle,
            recipe_book: ui.recipe_book,
            enchant: ui.enchant,
            slot,
        }
    }

    fn submit_inventory_container_click(&mut self, slot: usize, is_left: bool) {
        if let Some(position) = self.container_target {
            let _ = self.submit_local_authority_operation(
                crate::network::protocol::GameplayOperation::ContainerClick {
                    x: position.0,
                    y: position.1,
                    z: position.2,
                    slot: slot as u16,
                    is_left,
                    dragged: self
                        .inventory
                        .dragged
                        .as_ref()
                        .map(crate::network::protocol::ItemWire::from_stack),
                },
            );
        }
    }

    pub fn handle_inventory_click(&mut self, is_left: bool) {
        let probe = self.probe_inventory_click(is_left);
        let hit = probe.authority_hit();
        if let InventoryHit::Merchant { offer_index } = hit {
            // Merchant is not a `PresentationInventoryTarget`. Mapping it to
            // Workstation would `inventory_decision` → Reject, but the live
            // path submits `GameplayOperation::Trade` on both topologies.
            let _ = self.execute_active_merchant_trade(offer_index);
            return;
        }
        let Some(target) = presentation_target_for_authority_hit(hit) else {
            return;
        };
        match self.presentation_topology().inventory_decision(target) {
            PresentationInventoryAction::SendAuthorityOp => {
                if let InventoryHit::Slot(SlotType::ContainerSlot(slot)) = hit {
                    self.submit_inventory_container_click(slot, is_left);
                }
            }
            PresentationInventoryAction::LocalMutate => {
                // Embedded player-inventory writeback exception is applied
                // by `app` after this click via `sync_authority_gameplay_from_local`.
                // Local slot mutation lives in presentation inventory policy, not leftover sim.
                let _ = (probe, is_left);
            }
            PresentationInventoryAction::Reject => {}
        }
    }

    pub fn open_inventory(&mut self) {
        if !self.game_mode_policy().can_use_containers {
            return;
        }
        self.inventory.is_open = true;
        if self.advancement_gui.is_open {
            self.close_advancements_ui();
        }
        if self.is_creative_catalog_open() {
            self.inventory.clamp_creative_scroll();
        }
        self.clear_movement_input();
        self.sync_cursor_mode();
    }

    fn submit_join_container_open(&mut self, pos: (i32, i32, i32)) {
        if !self.game_mode_policy().can_use_containers {
            return;
        }
        let _ = self.submit_local_authority_operation(
            crate::network::protocol::GameplayOperation::Container {
                action: crate::network::protocol::ContainerAction::Open.to_wire(),
                x: pos.0,
                y: pos.1,
                z: pos.2,
                slot: 0,
            },
        );
    }

    pub fn open_merchant_trade_window(&mut self, villager_id: u64) {
        if !self.game_mode_policy().can_use_containers {
            return;
        }
        let villager_data =
            if let Some(index) = self.entity_manager.id_to_index.get(&villager_id).copied() {
                let entity = &self.entity_manager.entities[index];
                if entity.entity_type == crate::entity::EntityType::Villager
                    && entity.health > 0.0
                    && entity.age >= 0.0
                {
                    let profession = entity.profession;
                    let level = entity.villager_level;
                    let xp = entity.villager_xp;
                    // Offers are authority/entity projection only — never generate on GPU.
                    let offers = entity.offers.clone();
                    Some((profession, level, xp, offers))
                } else {
                    None
                }
            } else {
                None
            };

        if let Some((profession, level, xp, offers)) = villager_data {
            self.active_merchant_villager_id = Some(villager_id);
            self.active_merchant_offers = offers;
            self.active_merchant_profession = profession;
            self.active_merchant_level = level;
            self.active_merchant_xp = xp;
            self.active_station = Some(StationKind::Merchant);
            self.inventory.is_open = true;
            self.audio_manager
                .play_sound(crate::audio::SoundId::UiClick);
        }
    }

    pub fn execute_active_merchant_trade(&mut self, offer_index: usize) -> bool {
        if self.active_station != Some(StationKind::Merchant) {
            return false;
        }
        let Some(villager_id) = self.active_merchant_villager_id else {
            return false;
        };
        if offer_index >= self.active_merchant_offers.len() {
            return false;
        }
        let response = self.submit_local_authority_operation(
            crate::network::protocol::GameplayOperation::Trade {
                villager_id,
                offer_index: offer_index as u16,
            },
        );
        let accepted = matches!(
            response.map(|response| response.outcome),
            Some(crate::network::protocol::GameplayOutcome::Accepted { .. })
        );
        if accepted {
            if let Some(offer) = self.active_merchant_offers.get_mut(offer_index) {
                offer.uses = offer.uses.saturating_add(1);
            }
        }
        accepted
    }

    /// Tear down a container UI after an authoritative invalidation.
    ///
    /// Unlike [`close_inventory`], this path must not submit a close request,
    /// return a second copy of cursor/station items, or recurse into another
    /// authority route.  The authoritative session/block-entity projection is
    /// the only source of truth for any item conservation; this presentation
    /// cleanup merely drops transient client-side UI state.
    fn force_close_inventory(&mut self) {
        self.inventory.dragged = None;
        self.inventory.creative_drag_origin = None;
        self.inventory.craft_input.fill(None);
        self.inventory.craft_input = vec![None; 4];
        self.inventory.craft_output = None;
        self.enchanting.input = None;
        self.enchanting.lapis = None;
        self.brewing.bottles.fill(None);
        self.brewing.ingredient = None;
        self.anvil.left = None;
        self.anvil.right = None;
        self.anvil.output = None;
        self.anvil.rename.clear();
        self.active_merchant_villager_id = None;
        self.active_merchant_offers.clear();
        self.inventory.is_open = false;
        self.inventory.is_table_open = false;
        self.active_station = None;
        self.container_target = None;
        self.container_is_double = false;
        self.sync_cursor_mode();
    }

    pub fn close_inventory(&mut self) -> bool {
        if self.has_in_process_runtime() {
            let accepted = if let Some(pos) = self.container_target {
                self.submit_local_authority_container_action(
                    pos,
                    crate::network::protocol::ContainerAction::Close,
                    0,
                )
            } else {
                self.inventory.is_open = false;
                self.inventory.is_table_open = false;
                self.active_station = None;
                self.sync_cursor_mode();
                true
            };
            if accepted {
                self.inventory.dragged = None;
                self.inventory.creative_drag_origin = None;
                self.inventory.craft_input.fill(None);
                self.inventory.craft_output = None;
                self.enchanting.input = None;
                self.enchanting.lapis = None;
                self.brewing.bottles.fill(None);
                self.brewing.ingredient = None;
                self.anvil.left = None;
                self.anvil.right = None;
                self.anvil.output = None;
            }
            return accepted;
        }
        if self.presentation_topology().is_join_client() {
            if let Some(pos) = self.container_target {
                let _ = self.submit_local_authority_operation(
                    crate::network::protocol::GameplayOperation::Container {
                        action: crate::network::protocol::ContainerAction::Close.to_wire(),
                        x: pos.0,
                        y: pos.1,
                        z: pos.2,
                        slot: 0,
                    },
                );
            }
            self.inventory.dragged = None;
            self.inventory.creative_drag_origin = None;
            self.inventory.is_open = false;
            self.inventory.is_table_open = false;
            self.container_target = None;
            self.container_is_double = false;
            self.active_station = None;
            self.sync_cursor_mode();
            return true;
        }
        self.force_close_inventory();
        true
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
            // Recreate depth texture on resize
            self.depth_view = Self::create_depth_texture(&self.device, &self.config);
        }
    }

    fn estimated_debug_memory_bytes(&self) -> usize {
        let chunks_bytes: usize = self
            .chunk_manager
            .chunks
            .values()
            .map(Chunk::memory_usage)
            .sum();
        let mesh_bytes: usize = self.chunk_meshes.values().map(ChunkMesh::gpu_bytes).sum();

        let entities_bytes = self
            .entity_manager
            .entities
            .capacity()
            .saturating_mul(std::mem::size_of::<crate::entity::Entity>());
        let particles_bytes = self
            .particles
            .particles
            .capacity()
            .saturating_mul(std::mem::size_of::<crate::particles::Particle>());

        chunks_bytes
            .saturating_add(mesh_bytes)
            .saturating_add(entities_bytes)
            .saturating_add(particles_bytes)
    }

    fn poll_gpu_timestamp_readbacks(&mut self) {
        if self
            .gpu_timestamp_readback_slots
            .iter()
            .any(|slot| slot.status.lock().unwrap().state == GpuTimestampReadbackState::Mapping)
        {
            self.device.poll(wgpu::Maintain::Poll);
        }

        let mut newest_sample = None;
        for slot in &self.gpu_timestamp_readback_slots {
            let status = *slot.status.lock().unwrap();
            if status.state != GpuTimestampReadbackState::Mapped {
                continue;
            }
            let Some(submission_tag) = status.submission_tag else {
                continue;
            };

            let slice = slot.buffer.slice(..);
            let range = slice.get_mapped_range();
            if range.len() == GPU_TIMESTAMP_READBACK_BYTES as usize {
                let mut pass_timings_ns = [0; 7];
                let period = f64::from(self.queue.get_timestamp_period());
                for (pass_index, timing) in pass_timings_ns.iter_mut().enumerate() {
                    let start_offset = pass_index * 16;
                    let start = u64::from_ne_bytes(
                        range[start_offset..start_offset + 8].try_into().unwrap(),
                    );
                    let end = u64::from_ne_bytes(
                        range[start_offset + 8..start_offset + 16]
                            .try_into()
                            .unwrap(),
                    );
                    *timing = (end.saturating_sub(start) as f64 * period) as u64;
                }
                let newest_known_tag = newest_sample
                    .as_ref()
                    .map(|(tag, _)| *tag)
                    .or(self.gpu_pass_timing_submission_tag);
                if newest_known_tag.map_or(true, |current| submission_tag > current) {
                    newest_sample = Some((submission_tag, pass_timings_ns));
                }
            }
            drop(range);
            slot.buffer.unmap();
            let consumed = slot.status.lock().unwrap().consume(submission_tag);
            debug_assert!(consumed, "mapped timestamp slot must be consumed once");
        }

        if let Some((submission_tag, pass_timings_ns)) = newest_sample {
            self.gpu_pass_timings_ns = pass_timings_ns;
            self.gpu_pass_timings_valid = true;
            self.gpu_pass_timing_submission_tag = Some(submission_tag);
        }
    }

    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        self.sync_translation_catalog();
        let allocs_before = crate::perf::thread_alloc_count();
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut gpu_upload_elapsed = Duration::ZERO;

        self.prepare_terrain_draw_plan();

        // Frame-slot acquire / wait must stay between terrain prepare and
        // entity uploads. Do not reorder this with timestamp queries.
        while let Ok(completed) = self.gpu_completion_rx.try_recv() {
            self.frame_resource_pool.complete(completed);
        }
        let frame_submission_id = self.next_gpu_submission_id;
        self.next_gpu_submission_id = self.next_gpu_submission_id.wrapping_add(1).max(1);
        self.frame_ring_index = loop {
            match self.frame_resource_pool.acquire(frame_submission_id) {
                Ok(lease) => break lease.slot_id,
                Err(crate::gpu_frame_resources::AcquireError::Exhausted { .. }) => {
                    self.device.poll(wgpu::Maintain::Wait);
                    while let Ok(completed) = self.gpu_completion_rx.try_recv() {
                        self.frame_resource_pool.complete(completed);
                    }
                }
            }
        };

        self.prepare_entities(&mut gpu_upload_elapsed);
        self.prepare_hand(&mut gpu_upload_elapsed);
        self.build_hud(&mut gpu_upload_elapsed);
        self.encode_frame(output, view, frame_submission_id, allocs_before)
    }

    fn apply_ui_accessibility(
        &self,
        ui_vertices: &mut [UiVertex],
        ui_line_vertices: &mut [UiVertex],
        ui_textured_vertices: &mut [TexturedUiVertex],
    ) {
        let layout_scale =
            crate::accessibility::fit_ui_scale(self.settings.accessibility.ui_scale, 1.0);
        for vertex in ui_vertices.iter_mut() {
            vertex.position[0] *= layout_scale;
            vertex.position[1] *= layout_scale;
            if self.settings.accessibility.high_contrast {
                vertex.color = crate::accessibility::high_contrast_color(vertex.color);
            }
        }
        for vertex in ui_line_vertices.iter_mut() {
            vertex.position[0] *= layout_scale;
            vertex.position[1] *= layout_scale;
            if self.settings.accessibility.high_contrast {
                vertex.color = crate::accessibility::high_contrast_color(vertex.color);
            }
        }
        for vertex in ui_textured_vertices.iter_mut() {
            vertex.position[0] *= layout_scale;
            vertex.position[1] *= layout_scale;
            if self.settings.accessibility.high_contrast {
                vertex.color = crate::accessibility::high_contrast_color(vertex.color);
            }
        }
    }

    fn record_frame_perf_sample(&mut self) {
        let mut cpu_scopes = std::collections::BTreeMap::new();
        for summary in &self.perf_summaries {
            cpu_scopes.insert(summary.name.to_string(), summary.average_nanos);
        }
        let gpu_scopes = self.gpu_pass_timings_valid.then(|| {
            let names = [
                "sky",
                "opaque",
                "mobs",
                "translucent",
                "particles",
                "crack",
                "ui",
            ];
            names
                .into_iter()
                .zip(self.gpu_pass_timings_ns)
                .map(|(name, ns)| (name.to_string(), ns))
                .collect()
        });
        let reliable = self.perf_counters.network_inbound_reliable_pending;
        let latest = self.perf_counters.network_inbound_latest_pending;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        let mut categories: std::collections::BTreeMap<_, _> = crate::perf::QueueCategory::ALL
            .into_iter()
            .map(|category| {
                (
                    category,
                    crate::perf::queue_category_sample(category, now_ms),
                )
            })
            .collect();
        categories.insert(
            crate::perf::QueueCategory::SaveProducer,
            crate::perf::QueueCategorySample::default(),
        );
        categories.insert(
            crate::perf::QueueCategory::SaveWorker,
            crate::perf::QueueCategorySample::default(),
        );
        let inbound = categories
            .get(&crate::perf::QueueCategory::Inbound)
            .cloned()
            .unwrap_or_default();
        let outbound = categories
            .get(&crate::perf::QueueCategory::Outbound)
            .cloned()
            .unwrap_or_default();
        let reliable_queue = categories
            .get(&crate::perf::QueueCategory::Reliable)
            .cloned()
            .unwrap_or_default();
        let catchup_queue = categories
            .get(&crate::perf::QueueCategory::CatchUp)
            .cloned()
            .unwrap_or_default();
        let save_producer = categories
            .get(&crate::perf::QueueCategory::SaveProducer)
            .cloned()
            .unwrap_or_default();
        let save_worker = categories
            .get(&crate::perf::QueueCategory::SaveWorker)
            .cloned()
            .unwrap_or_default();
        let (retries, drops, cancels, oldest_age_ms) =
            categories
                .values()
                .fold((0_u64, 0_u64, 0_u64, 0_u64), |totals, sample| {
                    (
                        totals.0.saturating_add(sample.retries),
                        totals.1.saturating_add(sample.drops),
                        totals.2.saturating_add(sample.cancels),
                        totals.3.max(sample.oldest_age_ms),
                    )
                });
        let queues = crate::perf::QueuePerfSample {
            categories,
            inbound_pending: Some(inbound.depth),
            inbound_pending_bytes: Some(inbound.bytes),
            inbound_reliable_pending: Some(reliable),
            inbound_reliable_bytes: Some(self.perf_counters.network_inbound_reliable_bytes),
            inbound_latest_pending: Some(latest),
            inbound_latest_bytes: Some(self.perf_counters.network_inbound_latest_bytes),
            outbound_pending: Some(outbound.depth),
            outbound_bytes: Some(outbound.bytes),
            reliable_pending: Some(reliable_queue.depth),
            reliable_bytes: Some(reliable_queue.bytes),
            catchup_pending: Some(catchup_queue.depth),
            catchup_bytes: Some(catchup_queue.bytes),
            save_queued_bytes: Some(save_producer.bytes),
            save_in_flight_bytes: Some(save_worker.bytes),
            retries: Some(retries),
            drops: Some(drops),
            cancels: Some(cancels),
            oldest_age_ms: Some(oldest_age_ms),
        };
        let sample = crate::perf::FramePerfSample {
            frame_id: self.next_perf_frame_id,
            cpu_scopes_ns: cpu_scopes,
            gpu_scopes_ns: gpu_scopes,
            allocations: Some(self.perf_counters.frame_allocations),
            upload_bytes: self.perf_counters.upload_bytes_frame,
            draw_calls: self.perf_counters.draw_calls,
            buffer_bytes: self.perf_counters.gpu_mesh_bytes,
            culling: Some(crate::perf::CullingPerfSample {
                terrain_candidates: self.perf_counters.terrain_candidates,
                visible_chunks: self.perf_counters.visible_chunks,
                occluded_chunks: self.perf_counters.occluded_chunks,
                rendered_entities: self.perf_counters.rendered_entities,
                frustum_culled_entities: self.perf_counters.frustum_culled_entities,
                occlusion_culled_entities: self.perf_counters.occlusion_culled_entities,
            }),
            queues,
            checksum: None,
            lighting: Some(self.lighting_scopes_frame.values().to_vec()),
            gpu_uploads: Some(self.gpu_upload_scopes_frame.values().to_vec()),
        };
        self.next_perf_frame_id = self.next_perf_frame_id.wrapping_add(1);
        if self.frame_perf_samples.len() >= 240 {
            self.frame_perf_samples.pop_front();
        }
        self.frame_perf_samples.push_back(sample);
    }

    fn render_advancement_ui_and_toasts(
        &self,
        ui_vertices: &mut Vec<UiVertex>,
        ui_line_vertices: &mut Vec<UiVertex>,
        ui_textured_vertices: &mut Vec<TexturedUiVertex>,
    ) {
        let font_source = &self.font_source;
        let add_string_lines = |s: &str,
                                start_x: f32,
                                y: f32,
                                char_w: f32,
                                char_h: f32,
                                spacing: f32,
                                color: [f32; 4],
                                vertices: &mut Vec<UiVertex>| {
            add_string_lines_with_source(
                font_source,
                s,
                start_x,
                y,
                char_w,
                char_h,
                spacing,
                color,
                vertices,
            );
        };
        let (screen_w, screen_h) = (self.config.width as f32, self.config.height as f32);
        let aspect = screen_w / screen_h.max(1.0);

        // 1. Render Toast Notifications (top-right overlay)
        for toast in &self.advancement_manager.active_toasts {
            let slide = if toast.timer < 0.4 {
                (1.0 - (toast.timer / 0.4)) * 0.4
            } else if toast.timer > 2.6 {
                ((toast.timer - 2.6) / 0.4) * 0.4
            } else {
                0.0
            };

            let x0 = 0.55 + slide;
            let x1 = 0.95 + slide;
            let y0 = 0.72;
            let y1 = 0.92;

            add_ui_quad(ui_vertices, x0, x1, y0, y1, [0.08, 0.08, 0.12, 0.88]);

            let border_col = match toast.frame {
                crate::advancements::AdvancementFrameType::Challenge => [1.0, 0.85, 0.2, 1.0],
                crate::advancements::AdvancementFrameType::Goal => [0.4, 0.8, 1.0, 1.0],
                crate::advancements::AdvancementFrameType::Task => [0.9, 0.9, 0.9, 1.0],
            };
            add_ui_border(ui_line_vertices, x0, x1, y0, y1, border_col);

            let (col, row) = toast.icon_item.properties().tex_coords;
            let u0 = col as f32 * 0.0625;
            let u1 = (col + 1) as f32 * 0.0625;
            let v0 = row as f32 * 0.0625;
            let v1 = (row + 1) as f32 * 0.0625;

            let ix0 = x0 + 0.02;
            let ix1 = x0 + 0.08;
            let iy0 = y0 + 0.03 * aspect;
            let iy1 = y1 - 0.03 * aspect;

            ui_textured_vertices.push(TexturedUiVertex {
                position: [ix0, iy1, 0.0],
                tex_coords: [u0, v0],
                color: [1.0, 1.0, 1.0, 1.0],
            });
            ui_textured_vertices.push(TexturedUiVertex {
                position: [ix0, iy0, 0.0],
                tex_coords: [u0, v1],
                color: [1.0, 1.0, 1.0, 1.0],
            });
            ui_textured_vertices.push(TexturedUiVertex {
                position: [ix1, iy0, 0.0],
                tex_coords: [u1, v1],
                color: [1.0, 1.0, 1.0, 1.0],
            });
            ui_textured_vertices.push(TexturedUiVertex {
                position: [ix0, iy1, 0.0],
                tex_coords: [u0, v0],
                color: [1.0, 1.0, 1.0, 1.0],
            });
            ui_textured_vertices.push(TexturedUiVertex {
                position: [ix1, iy0, 0.0],
                tex_coords: [u1, v1],
                color: [1.0, 1.0, 1.0, 1.0],
            });
            ui_textured_vertices.push(TexturedUiVertex {
                position: [ix1, iy1, 0.0],
                tex_coords: [u1, v0],
                color: [1.0, 1.0, 1.0, 1.0],
            });

            add_string_lines(
                &self.translate("advancement.toast"),
                x0 + 0.09,
                y1 - 0.04 * aspect,
                0.007,
                0.014,
                0.002,
                border_col,
                ui_line_vertices,
            );
            add_string_lines(
                &toast.title.to_uppercase(),
                x0 + 0.09,
                y1 - 0.10 * aspect,
                0.008,
                0.016,
                0.002,
                [1.0, 1.0, 1.0, 1.0],
                ui_line_vertices,
            );
        }

        // 2. Render Advancements GUI screen when open
        if self.advancement_gui.is_open {
            add_ui_quad(ui_vertices, -1.0, 1.0, -1.0, 1.0, [0.0, 0.0, 0.0, 0.65]);

            let wx0 = -0.80;
            let wx1 = 0.80;
            let wy0 = -0.80;
            let wy1 = 0.80;

            add_ui_quad(ui_vertices, wx0, wx1, wy0, wy1, [0.12, 0.12, 0.15, 0.95]);
            add_ui_border(ui_line_vertices, wx0, wx1, wy0, wy1, [0.5, 0.5, 0.6, 1.0]);

            let tab_y0 = wy1 - 0.12;
            let tab_y1 = wy1;
            let tab_w = (wx1 - wx0) / 5.0;

            let categories = [
                (crate::advancements::AdvancementCategory::Minecraft, "STORY"),
                (crate::advancements::AdvancementCategory::Nether, "NETHER"),
                (crate::advancements::AdvancementCategory::TheEnd, "THE END"),
                (
                    crate::advancements::AdvancementCategory::Adventure,
                    "ADVENTURE",
                ),
                (
                    crate::advancements::AdvancementCategory::Husbandry,
                    "HUSBANDRY",
                ),
            ];

            for (i, (cat, name)) in categories.iter().enumerate() {
                let tx0 = wx0 + i as f32 * tab_w;
                let tx1 = tx0 + tab_w;
                let is_sel = *cat == self.advancement_gui.selected_category;
                let bg_col = if is_sel {
                    [0.25, 0.25, 0.32, 0.95]
                } else {
                    [0.16, 0.16, 0.20, 0.95]
                };
                let line_col = if is_sel {
                    [0.9, 0.8, 0.3, 1.0]
                } else {
                    [0.35, 0.35, 0.40, 1.0]
                };

                add_ui_quad(ui_vertices, tx0, tx1, tab_y0, tab_y1, bg_col);
                add_ui_border(ui_line_vertices, tx0, tx1, tab_y0, tab_y1, line_col);

                add_string_lines(
                    name,
                    tx0 + 0.015,
                    tab_y0 + 0.035,
                    0.007,
                    0.014,
                    0.002,
                    if is_sel {
                        [1.0, 0.9, 0.4, 1.0]
                    } else {
                        [0.7, 0.7, 0.7, 1.0]
                    },
                    ui_line_vertices,
                );
            }

            let view_x0 = wx0 + 0.02;
            let view_x1 = wx1 - 0.02;
            let view_y0 = wy0 + 0.02;
            let view_y1 = tab_y0 - 0.02;

            let center_x =
                (view_x0 + view_x1) * 0.5 + (self.advancement_gui.scroll_x / screen_w) * 2.0;
            let center_y =
                (view_y0 + view_y1) * 0.5 - (self.advancement_gui.scroll_y / screen_h) * 2.0;
            let zoom = self.advancement_gui.zoom;

            let advs = self
                .advancement_manager
                .tree
                .get_category_advancements(self.advancement_gui.selected_category);

            for adv in &advs {
                let nx = center_x + adv.x_pos * 0.15 * zoom;
                let ny = center_y + adv.y_pos * 0.15 * aspect * zoom;

                if let Some(parent_id) = adv.parent {
                    if let Some(parent_adv) = self.advancement_manager.tree.get(parent_id) {
                        let px = center_x + parent_adv.x_pos * 0.15 * zoom;
                        let py = center_y + parent_adv.y_pos * 0.15 * aspect * zoom;

                        let line_col = if self.advancement_manager.is_unlocked(adv.id) {
                            [0.9, 0.8, 0.3, 1.0]
                        } else {
                            [0.3, 0.3, 0.35, 1.0]
                        };

                        ui_line_vertices.push(UiVertex {
                            position: [px, py, 0.0],
                            color: line_col,
                        });
                        ui_line_vertices.push(UiVertex {
                            position: [nx, ny, 0.0],
                            color: line_col,
                        });
                    }
                }
            }

            let mouse_ndc_x = self.mouse_ndc[0];
            let mouse_ndc_y = self.mouse_ndc[1];
            let mut hovered = None;

            for adv in &advs {
                let nx = center_x + adv.x_pos * 0.15 * zoom;
                let ny = center_y + adv.y_pos * 0.15 * aspect * zoom;

                let nw = 0.04 * zoom;
                let nh = 0.04 * aspect * zoom;
                let bx0 = nx - nw;
                let bx1 = nx + nw;
                let by0 = ny - nh;
                let by1 = ny + nh;

                if mouse_ndc_x >= bx0
                    && mouse_ndc_x <= bx1
                    && mouse_ndc_y >= by0
                    && mouse_ndc_y <= by1
                {
                    hovered = Some(adv.id);
                }

                let is_unlocked = self.advancement_manager.is_unlocked(adv.id);
                let bg_col = if is_unlocked {
                    [0.18, 0.30, 0.18, 0.95]
                } else {
                    [0.10, 0.10, 0.12, 0.95]
                };
                let border_col = match adv.frame {
                    crate::advancements::AdvancementFrameType::Challenge => {
                        if is_unlocked {
                            [1.0, 0.85, 0.2, 1.0]
                        } else {
                            [0.5, 0.4, 0.1, 0.9]
                        }
                    }
                    crate::advancements::AdvancementFrameType::Goal => {
                        if is_unlocked {
                            [0.3, 0.75, 1.0, 1.0]
                        } else {
                            [0.15, 0.35, 0.5, 0.9]
                        }
                    }
                    crate::advancements::AdvancementFrameType::Task => {
                        if is_unlocked {
                            [0.9, 0.9, 0.9, 1.0]
                        } else {
                            [0.4, 0.4, 0.4, 0.9]
                        }
                    }
                };

                add_ui_quad(ui_vertices, bx0, bx1, by0, by1, bg_col);
                add_ui_border(ui_line_vertices, bx0, bx1, by0, by1, border_col);

                let (col, row) = adv.icon_item.properties().tex_coords;
                let u0 = col as f32 * 0.0625;
                let u1 = (col + 1) as f32 * 0.0625;
                let v0 = row as f32 * 0.0625;
                let v1 = (row + 1) as f32 * 0.0625;

                let icon_col = if is_unlocked {
                    [1.0, 1.0, 1.0, 1.0]
                } else {
                    [0.4, 0.4, 0.4, 0.6]
                };

                let ix0 = bx0 + 0.008 * zoom;
                let ix1 = bx1 - 0.008 * zoom;
                let iy0 = by0 + 0.008 * aspect * zoom;
                let iy1 = by1 - 0.008 * aspect * zoom;

                ui_textured_vertices.push(TexturedUiVertex {
                    position: [ix0, iy1, 0.0],
                    tex_coords: [u0, v0],
                    color: icon_col,
                });
                ui_textured_vertices.push(TexturedUiVertex {
                    position: [ix0, iy0, 0.0],
                    tex_coords: [u0, v1],
                    color: icon_col,
                });
                ui_textured_vertices.push(TexturedUiVertex {
                    position: [ix1, iy0, 0.0],
                    tex_coords: [u1, v1],
                    color: icon_col,
                });
                ui_textured_vertices.push(TexturedUiVertex {
                    position: [ix0, iy1, 0.0],
                    tex_coords: [u0, v0],
                    color: icon_col,
                });
                ui_textured_vertices.push(TexturedUiVertex {
                    position: [ix1, iy0, 0.0],
                    tex_coords: [u1, v1],
                    color: icon_col,
                });
                ui_textured_vertices.push(TexturedUiVertex {
                    position: [ix1, iy1, 0.0],
                    tex_coords: [u1, v0],
                    color: icon_col,
                });
            }

            if let Some(adv_id) = hovered {
                if let Some(adv) = self.advancement_manager.tree.get(adv_id) {
                    let tx0 = mouse_ndc_x + 0.02;
                    let tx1 = tx0 + 0.40;
                    let ty0 = mouse_ndc_y - 0.15;
                    let ty1 = mouse_ndc_y;

                    add_ui_quad(ui_vertices, tx0, tx1, ty0, ty1, [0.05, 0.05, 0.08, 0.95]);
                    add_ui_border(ui_line_vertices, tx0, tx1, ty0, ty1, [0.8, 0.8, 0.3, 1.0]);

                    add_string_lines(
                        &adv.title.to_uppercase(),
                        tx0 + 0.015,
                        ty1 - 0.04,
                        0.008,
                        0.016,
                        0.002,
                        [1.0, 1.0, 1.0, 1.0],
                        ui_line_vertices,
                    );

                    let status = if self.advancement_manager.is_unlocked(adv.id) {
                        "[COMPLETED]"
                    } else {
                        "[LOCKED]"
                    };
                    let status_col = if self.advancement_manager.is_unlocked(adv.id) {
                        [0.3, 1.0, 0.3, 1.0]
                    } else {
                        [0.8, 0.3, 0.3, 1.0]
                    };
                    add_string_lines(
                        status,
                        tx0 + 0.015,
                        ty1 - 0.08,
                        0.007,
                        0.014,
                        0.002,
                        status_col,
                        ui_line_vertices,
                    );
                }
            }
        }
    }
}

fn add_ui_quad(vertices: &mut Vec<UiVertex>, x0: f32, x1: f32, y0: f32, y1: f32, color: [f32; 4]) {
    for position in [
        [x0, y1, 0.0],
        [x0, y0, 0.0],
        [x1, y0, 0.0],
        [x0, y1, 0.0],
        [x1, y0, 0.0],
        [x1, y1, 0.0],
    ] {
        vertices.push(UiVertex { position, color });
    }
}

fn add_ui_border(
    vertices: &mut Vec<UiVertex>,
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
    color: [f32; 4],
) {
    for (p1, p2) in [
        ([x0, y1, 0.0], [x1, y1, 0.0]),
        ([x1, y1, 0.0], [x1, y0, 0.0]),
        ([x1, y0, 0.0], [x0, y0, 0.0]),
        ([x0, y0, 0.0], [x0, y1, 0.0]),
    ] {
        vertices.push(UiVertex {
            position: p1,
            color,
        });
        vertices.push(UiVertex {
            position: p2,
            color,
        });
    }
}

fn add_char_lines_with_source(
    font_source: &crate::resources::FontSource,
    c: char,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
    vertices: &mut Vec<UiVertex>,
) {
    let character = c.to_ascii_uppercase();
    let rows = font_source
        .glyph_override(character)
        .unwrap_or_else(|| crate::menu::glyph(character));

    let cell_w = w / 5.0;
    let cell_h = h / 7.0;
    for (row, mask) in rows.into_iter().enumerate() {
        let center_y = y + h - (row as f32 + 0.5) * cell_h;
        for column in 0..5 {
            if mask & (1 << (4 - column)) != 0 {
                let cell_x = x + column as f32 * cell_w;
                vertices.push(UiVertex {
                    position: [cell_x, center_y, 0.0],
                    color,
                });
                vertices.push(UiVertex {
                    position: [cell_x + cell_w, center_y, 0.0],
                    color,
                });
            }
        }
    }
}

fn add_string_lines_with_source(
    font_source: &crate::resources::FontSource,
    s: &str,
    start_x: f32,
    y: f32,
    char_w: f32,
    char_h: f32,
    spacing: f32,
    color: [f32; 4],
    vertices: &mut Vec<UiVertex>,
) {
    let mut current_x = start_x;
    for c in s.chars() {
        add_char_lines_with_source(
            font_source,
            c.to_ascii_uppercase(),
            current_x,
            y,
            char_w,
            char_h,
            color,
            vertices,
        );
        current_x += char_w + spacing;
    }
}

/// Built-in compatibility helper used by unit tests and non-State callers.
/// State's render paths install their selected `FontSource` through the local
/// closures at the render boundary above.
#[cfg(test)]
fn add_char_lines(
    c: char,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
    vertices: &mut Vec<UiVertex>,
) {
    add_char_lines_with_source(
        &crate::resources::FontSource::BuiltIn,
        c,
        x,
        y,
        w,
        h,
        color,
        vertices,
    );
}

/// Built-in compatibility helper used by unit tests and non-State callers.
#[cfg(test)]
fn add_string_lines(
    s: &str,
    start_x: f32,
    y: f32,
    char_w: f32,
    char_h: f32,
    spacing: f32,
    color: [f32; 4],
    vertices: &mut Vec<UiVertex>,
) {
    add_string_lines_with_source(
        &crate::resources::FontSource::BuiltIn,
        s,
        start_x,
        y,
        char_w,
        char_h,
        spacing,
        color,
        vertices,
    );
}

fn weather_tile_uv(column: u32, row: u32) -> [f32; 4] {
    let inset = 0.08;
    [
        (column as f32 + inset) / 16.0,
        (row as f32 + inset) / 16.0,
        (column as f32 + 1.0 - inset) / 16.0,
        (row as f32 + 1.0 - inset) / 16.0,
    ]
}

impl Drop for State {
    fn drop(&mut self) {
        self.shutdown_network();
        let _ = self
            .window
            .set_cursor_grab(winit::window::CursorGrabMode::None);
        self.window.set_cursor_visible(true);
    }
}

fn biome_debug_name(biome: Biome) -> &'static str {
    match biome {
        Biome::Plains => "PLAINS",
        Biome::Forest => "FOREST",
        Biome::BirchForest => "BIRCH_FOREST",
        Biome::Taiga => "TAIGA",
        Biome::SnowyPlains => "SNOWY_PLAINS",
        Biome::Desert => "DESERT",
        Biome::Savanna => "SAVANNA",
        Biome::Swamp => "SWAMP",
        Biome::Jungle => "JUNGLE",
        Biome::Badlands => "BADLANDS",
        Biome::Meadow => "MEADOW",
        Biome::WindsweptHills => "WINDSWEPT_HILLS",
        Biome::River => "RIVER",
        Biome::Beach => "BEACH",
        Biome::Ocean => "OCEAN",
        Biome::DeepOcean => "DEEP_OCEAN",
    }
}

fn debug_chunk_coordinate(position: f32, chunk_size: usize) -> i32 {
    (position.floor() as i32).div_euclid(chunk_size as i32)
}

#[cfg(test)]
mod render_region_lifecycle_tests {
    use super::{
        empty_region_rebuild_worthwhile, region_allocation_handle_is_live,
        should_decrement_region_active_chunks, RenderRegion,
    };
    use crate::chunk_render::{FreeList, RegionAllocationHandle};

    #[test]
    fn active_chunk_count_changes_only_for_resident_mesh_in_current_region() {
        assert!(!should_decrement_region_active_chunks(false, false, false));
        assert!(should_decrement_region_active_chunks(true, false, false));
        assert!(should_decrement_region_active_chunks(true, true, true));
        assert!(!should_decrement_region_active_chunks(true, true, false));
    }

    #[test]
    fn stale_region_instance_and_stale_tokens_are_rejected() {
        let mut vertices = FreeList::new(16);
        let mut indices = FreeList::new(24);
        let vertex_token = vertices.allocate_owned(4, 7).unwrap();
        let index_token = indices.allocate_owned(6, 7).unwrap();
        let handle = RegionAllocationHandle {
            region_instance_id: 41,
            vertex_token,
            index_token,
            vertex_offset: vertex_token.offset,
            index_offset: index_token.offset,
            num_vertices: vertex_token.count,
            num_indices: index_token.count,
        };

        assert!(region_allocation_handle_is_live(
            41, &vertices, &indices, &handle
        ));
        assert!(!region_allocation_handle_is_live(
            42, &vertices, &indices, &handle
        ));
        vertices.deallocate_owned(vertex_token).unwrap();
        assert!(!region_allocation_handle_is_live(
            41, &vertices, &indices, &handle
        ));
    }

    #[test]
    fn arena_rebuild_is_limited_to_empty_grown_regions() {
        assert!(empty_region_rebuild_worthwhile(
            0,
            0,
            RenderRegion::INITIAL_VERTEX_CAPACITY * 2,
            RenderRegion::INITIAL_INDEX_CAPACITY
        ));
        assert!(!empty_region_rebuild_worthwhile(
            1,
            0,
            RenderRegion::INITIAL_VERTEX_CAPACITY * 2,
            RenderRegion::INITIAL_INDEX_CAPACITY
        ));
        assert!(!empty_region_rebuild_worthwhile(
            0,
            0,
            RenderRegion::INITIAL_VERTEX_CAPACITY,
            RenderRegion::INITIAL_INDEX_CAPACITY
        ));
    }
}

#[cfg(test)]
mod debug_tests {
    use super::*;

    fn embedded_test_world(name: &str) -> std::path::PathBuf {
        let unique = format!(
            "icraft-state-runtime-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn embedded_runtime_uses_world_player_profile_and_fifo_ack() {
        let world_dir = embedded_test_world("fifo");
        let role = MultiplayerRole::Singleplayer;
        let mut bridge =
            EmbeddedRuntimeBridge::new(&role, world_dir.clone(), 1234, Difficulty::Normal, 8, true)
                .expect("embedded runtime should construct");
        let session = bridge
            .runtime
            .players
            .get(&u64::MAX)
            .expect("local world-player session");
        assert_eq!(session.username, "local");
        assert_eq!(
            session.storage,
            crate::server_runtime::LocalSessionStorage::WorldPlayer
        );

        let before = bridge.runtime.authority.world().get_block(8, 80, 8);
        bridge
            .queue_request(crate::network::protocol::GameplayRequest {
                request_id: 0,
                client_sequence: 0,
                session_id: 0,
                dimension: crate::dimension::Dimension::Overworld as u8,
                client_revision: 0,
                operation: crate::network::protocol::GameplayOperation::BlockAction {
                    action: crate::network::protocol::BlockActionKind::Place,
                    x: 8,
                    y: 80,
                    z: 8,
                    face: [0, 1, 0],
                    hand: 0,
                    held: None,
                    block: BlockType::Glass.to_wire(),
                    look_milli: [0, 0, 1000],
                },
            })
            .expect("request should enter bounded FIFO");
        assert_eq!(bridge.runtime.authority.world().get_block(8, 80, 8), before);
        let output = bridge.tick().expect("fixed tick should run");
        assert!(!output
            .snapshot
            .mutations
            .iter()
            .any(|mutation| mutation.position == (8, 80, 8)
                && mutation.block == BlockType::Glass.to_wire()));
        assert_eq!(bridge.runtime.authority.world().get_block(8, 80, 8), before);
        assert!(output.presentation_events.iter().any(|event| {
            matches!(
                event,
                crate::server_runtime::ProjectionEvent {
                    dest: crate::server_runtime::ProjectionDest::Session(target),
                    packet: crate::network::protocol::Packet::GameplayResponse { response, .. },
                    ..
                } if *target == u64::MAX
                    && response.request_id == 1
                    && matches!(
                        response.outcome,
                        crate::network::protocol::GameplayOutcome::Rejected {
                            reason: crate::network::protocol::RejectReason::InvalidState
                        }
                    )
            )
        }));
        bridge.shutdown().expect("runtime save/shutdown");
        let _ = std::fs::remove_dir_all(world_dir);
    }

    #[test]
    fn embedded_runtime_poses_use_monotonic_sender_time() {
        let world_dir = embedded_test_world("pose");
        let role = MultiplayerRole::Singleplayer;
        let mut bridge =
            EmbeddedRuntimeBridge::new(&role, world_dir.clone(), 1234, Difficulty::Normal, 8, true)
                .expect("embedded runtime should construct");
        let initial = bridge
            .runtime
            .players
            .get(&u64::MAX)
            .expect("local session")
            .data
            .position;
        bridge
            .queue_position(
                1,
                glam::Vec3::new(initial[0] + 1.0, initial[1], initial[2]),
                0.5,
                0.1,
            )
            .expect("first pose should enter bounded FIFO");
        bridge.tick().expect("first pose tick");
        let first = bridge
            .runtime
            .players
            .get(&u64::MAX)
            .expect("local session")
            .data
            .position;
        assert_eq!(first[0], initial[0] + 1.0);

        bridge
            .queue_position(
                2,
                glam::Vec3::new(first[0] + 1.0, first[1], first[2]),
                0.5,
                0.1,
            )
            .expect("second pose should enter bounded FIFO");
        bridge.tick().expect("second pose tick");
        let second = bridge
            .runtime
            .players
            .get(&u64::MAX)
            .expect("local session")
            .data
            .position;
        assert_eq!(second[0], first[0] + 1.0);

        bridge.shutdown().expect("runtime save/shutdown");
        let _ = std::fs::remove_dir_all(world_dir);
    }

    #[test]
    fn embedded_inventory_writeback_copies_only_inventory_and_hotbar() {
        let world_dir = embedded_test_world("writeback");
        let role = MultiplayerRole::Singleplayer;
        let mut bridge =
            EmbeddedRuntimeBridge::new(&role, world_dir.clone(), 1234, Difficulty::Normal, 8, true)
                .expect("embedded runtime should construct");
        let session_id = bridge.session_id();
        let before = bridge
            .runtime
            .authority
            .session(session_id)
            .expect("session")
            .gameplay;
        assert!(before.health_milli > 0);

        let mut inventory = [None; crate::authority::contract::SESSION_INVENTORY_SLOTS];
        inventory[0] = Some(crate::authority::contract::SessionInventorySlot::from_wire(
            crate::network::protocol::ItemWire::from_stack(&ItemStack::new(Item::Dirt, 8)),
            0,
            0,
        ));
        assert!(bridge.sync_local_inventory(inventory, None, 3));

        let after = bridge
            .runtime
            .authority
            .session(session_id)
            .expect("session")
            .gameplay;
        assert_eq!(after.health_milli, before.health_milli);
        assert_eq!(after.hunger_milli, before.hunger_milli);
        assert_eq!(after.experience, before.experience);
        assert_eq!(after.experience_level, before.experience_level);
        assert_eq!(after.mounted_entity, before.mounted_entity);
        assert_eq!(after.selected_hotbar_slot, 3);
        assert!(after.inventory[0].is_some());

        bridge.shutdown().expect("runtime save/shutdown");
        let _ = std::fs::remove_dir_all(world_dir);
    }

    fn rects_overlap(a: InventoryUiRect, b: InventoryUiRect) -> bool {
        a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
    }

    #[test]
    fn creative_layout_is_only_used_without_a_station_or_crafting_table() {
        assert_eq!(
            inventory_layout_kind(GameMode::Creative, false, false),
            InventoryLayoutKind::CreativeCatalog
        );
        assert_eq!(
            inventory_layout_kind(GameMode::Survival, false, false),
            InventoryLayoutKind::Standard
        );
        assert_eq!(
            inventory_layout_kind(GameMode::Creative, true, false),
            InventoryLayoutKind::Standard
        );
        assert_eq!(
            inventory_layout_kind(GameMode::Creative, false, true),
            InventoryLayoutKind::Standard
        );
    }

    #[test]
    fn creative_tabs_catalog_scrollbar_and_hotbar_do_not_overlap() {
        for aspect in [4.0 / 3.0, 16.0 / 9.0, 21.0 / 9.0] {
            let catalog: Vec<_> = (0..CREATIVE_VISIBLE_SLOTS)
                .map(|index| creative_catalog_slot_rect(index, aspect))
                .collect();
            let hotbar: Vec<_> = (0..9)
                .map(|index| creative_hotbar_slot_rect(index, aspect))
                .collect();
            let tabs: Vec<_> = (0..CreativeTab::TABS.len())
                .map(creative_tab_rect)
                .collect();
            let scrollbar = creative_scroll_track_rect(aspect);

            for group in [&catalog, &hotbar, &tabs] {
                for (index, rect) in group.iter().enumerate() {
                    assert!(rect.x0 >= -1.0 && rect.x1 <= 1.0);
                    assert!(rect.y0 >= -1.0 && rect.y1 <= 1.0);
                    for other in group.iter().skip(index + 1) {
                        assert!(!rects_overlap(*rect, *other), "{rect:?} {other:?}");
                    }
                }
            }
            for catalog_rect in &catalog {
                assert!(!rects_overlap(*catalog_rect, scrollbar));
                assert!(hotbar
                    .iter()
                    .all(|hotbar_rect| !rects_overlap(*catalog_rect, *hotbar_rect)));
                assert!(tabs
                    .iter()
                    .all(|tab_rect| !rects_overlap(*catalog_rect, *tab_rect)));
            }
            assert!(hotbar
                .iter()
                .all(|hotbar_rect| !rects_overlap(*hotbar_rect, scrollbar)));
            assert!(tabs
                .iter()
                .all(|tab_rect| !rects_overlap(*tab_rect, scrollbar)));
        }
    }

    #[test]
    fn primary_press_decision_controls_block_fallback_and_held_mining_latch() {
        assert_eq!(
            primary_press_decision(GameMode::Survival, true),
            PrimaryPressDecision {
                keep_held_mining: false,
                instant_break: false,
            }
        );
        assert_eq!(
            primary_press_decision(GameMode::Survival, false),
            PrimaryPressDecision {
                keep_held_mining: true,
                instant_break: false,
            }
        );
        assert_eq!(
            primary_press_decision(GameMode::Creative, true),
            PrimaryPressDecision {
                keep_held_mining: false,
                instant_break: false,
            }
        );
        assert_eq!(
            primary_press_decision(GameMode::Creative, false),
            PrimaryPressDecision {
                keep_held_mining: false,
                instant_break: true,
            }
        );
    }

    #[test]
    fn creative_can_break_end_portal_while_survival_cannot() {
        assert!(can_break_block(BlockType::EndPortal, GameMode::Creative));
        assert!(!can_break_block(BlockType::EndPortal, GameMode::Survival));
        assert!(!can_break_block(BlockType::Air, GameMode::Creative));
    }

    #[test]
    fn melee_targeting_filters_noncombat_entities_and_selects_the_nearest_living_target() {
        use crate::entity::{Entity, EntityType};

        let mut entity_manager = crate::entity::EntityManager::new();
        let entities = [
            Entity::new(1, EntityType::DroppedItem, Vec3::new(0.0, 0.0, 0.75)),
            Entity::new(2, EntityType::HeartParticle, Vec3::new(0.0, 0.0, 0.9)),
            Entity::new(3, EntityType::Arrow, Vec3::new(0.0, 0.0, 1.0)),
            Entity::new(4, EntityType::SplashPotion, Vec3::new(0.0, 0.0, 1.1)),
            Entity::new(5, EntityType::WitherSkull, Vec3::new(0.0, 0.0, 1.2)),
            Entity::new(6, EntityType::DragonBreath, Vec3::new(0.0, 0.0, 1.3)),
            Entity::new(7, EntityType::RemotePlayer, Vec3::new(0.0, 0.0, 1.4)),
            Entity::new(8, EntityType::Zombie, Vec3::new(0.0, 0.0, 3.0)),
            Entity::new(9, EntityType::Skeleton, Vec3::new(0.0, 0.0, 2.0)),
        ];
        for entity in entities {
            entity_manager.entities.push(entity);
        }
        entity_manager.rebuild_indexes();
        let invalid_types = [
            EntityType::DroppedItem,
            EntityType::HeartParticle,
            EntityType::Arrow,
            EntityType::SplashPotion,
            EntityType::WitherSkull,
            EntityType::DragonBreath,
            EntityType::RemotePlayer,
        ];
        for entity_type in invalid_types {
            let entity = entity_manager
                .entities
                .iter()
                .find(|entity| entity.entity_type == entity_type)
                .unwrap();
            assert!(!is_legal_melee_target(entity));
        }

        assert_eq!(
            closest_melee_target(
                &entity_manager,
                Vec3::new(0.0, 0.1, 0.0),
                Vec3::Z,
                MELEE_REACH
            ),
            Some(9)
        );

        entity_manager
            .entities
            .iter_mut()
            .find(|entity| entity.id == 9)
            .unwrap()
            .health = 0.0;
        assert_eq!(
            closest_melee_target(
                &entity_manager,
                Vec3::new(0.0, 0.1, 0.0),
                Vec3::Z,
                MELEE_REACH
            ),
            Some(8)
        );

        let mut endermen = crate::entity::EntityManager::new();
        endermen.entities.push(Entity::new(
            10,
            EntityType::Enderman,
            Vec3::new(0.0, 0.0, 2.0),
        ));
        endermen.rebuild_indexes();
        assert_eq!(
            closest_melee_target(&endermen, Vec3::new(0.0, 0.1, 0.0), Vec3::Z, MELEE_REACH),
            Some(10)
        );
    }

    #[test]
    fn terrain_vertex_layout_exposes_ambient_occlusion() {
        let layout = Vertex::desc();
        assert_eq!(std::mem::size_of::<Vertex>(), 28);
        assert_eq!(layout.array_stride, 28);
        assert_eq!(layout.attributes.len(), 4);
        assert_eq!(layout.attributes[3].offset, 24);
        assert_eq!(layout.attributes[3].shader_location, 3);
        assert_eq!(layout.attributes[3].format, wgpu::VertexFormat::Float32);
    }

    #[test]
    fn debug_chunk_coordinates_handle_negative_world_positions() {
        assert_eq!(debug_chunk_coordinate(0.0, CHUNK_WIDTH), 0);
        assert_eq!(debug_chunk_coordinate(15.999, CHUNK_WIDTH), 0);
        assert_eq!(debug_chunk_coordinate(16.0, CHUNK_WIDTH), 1);
        assert_eq!(debug_chunk_coordinate(-0.001, CHUNK_WIDTH), -1);
        assert_eq!(debug_chunk_coordinate(-16.0, CHUNK_WIDTH), -1);
        assert_eq!(debug_chunk_coordinate(-16.001, CHUNK_WIDTH), -2);
    }

    #[test]
    fn debug_overlay_font_supports_every_required_character() {
        let mut vertices = Vec::new();
        for character in ['B', 'K', 'W', 'X', 'Z', 'b', 'k', 'w', 'x', 'z', '/', '_'] {
            let before = vertices.len();
            add_char_lines(character, 0.0, 0.0, 0.1, 0.2, [1.0; 4], &mut vertices);
            assert!(vertices.len() > before, "missing glyph for {character}");
        }
    }

    #[test]
    fn state_text_helper_uses_bitmap_override_and_builtin_fallback() {
        let source = crate::resources::FontSource::Bitmap(
            [('A', [0b1_1111, 0, 0, 0, 0, 0, 0])].into_iter().collect(),
        );
        let mut overridden = Vec::new();
        add_string_lines_with_source(
            &source,
            "A",
            0.0,
            0.0,
            0.1,
            0.2,
            0.0,
            [1.0; 4],
            &mut overridden,
        );
        // Five lit cells are five line-list segments (ten vertices).
        assert_eq!(overridden.len(), 10);

        let mut fallback = Vec::new();
        add_string_lines("A", 0.0, 0.0, 0.1, 0.2, 0.0, [1.0; 4], &mut fallback);
        // Built-in A uses the shared 5x7 glyph table (18 lit cells -> 36 vertices).
        assert_eq!(fallback.len(), 36);
    }

    #[test]
    fn chat_history_evicts_the_oldest_message() {
        let mut history = std::collections::VecDeque::new();
        for index in 0..=CHAT_HISTORY_CAPACITY {
            push_chat_history(
                &mut history,
                "Player".to_string(),
                format!("message {index}"),
            );
        }
        assert_eq!(history.len(), CHAT_HISTORY_CAPACITY);
        assert_eq!(history.front().unwrap().1, "message 1");
        assert_eq!(history.back().unwrap().1, "message 50");
    }

    #[test]
    fn chat_messages_are_trimmed_sanitized_and_bounded() {
        assert_eq!(normalized_chat_message(" \n\t "), None);
        assert_eq!(
            normalized_chat_message("  hello\nworld  ").as_deref(),
            Some("helloworld")
        );
        let oversized = "x".repeat(CHAT_INPUT_CAPACITY + 10);
        assert_eq!(
            normalized_chat_message(&oversized).unwrap().chars().count(),
            CHAT_INPUT_CAPACITY
        );
    }

    #[test]
    fn name_tag_projection_rejects_invalid_clip_space() {
        assert_eq!(
            project_name_tag(Vec3::new(0.25, -0.5, 0.5), Mat4::IDENTITY),
            Some(Vec2::new(0.25, -0.5))
        );
        assert_eq!(project_name_tag(Vec3::ZERO, Mat4::ZERO), None);
        assert_eq!(
            project_name_tag(Vec3::new(0.0, 0.0, 2.0), Mat4::IDENTITY),
            None
        );
    }

    #[test]
    fn network_handle_preserves_client_chat_and_disconnect_payloads() {
        let (inbound_tx, inbound_rx) = std::sync::mpsc::channel();
        let (outbound_tx, _outbound_rx) = std::sync::mpsc::channel();
        let handle = NetworkHandle::Client {
            client_to_game: inbound_rx,
            game_to_client: outbound_tx,
            thread: None,
        };
        inbound_tx
            .send(crate::network::client::ClientToGame::Chat {
                sender: "Alex".to_string(),
                message: "hello".to_string(),
            })
            .unwrap();
        inbound_tx
            .send(crate::network::client::ClientToGame::Disconnected {
                reason: "server stopped".to_string(),
            })
            .unwrap();

        let events = handle.drain_inbound();
        assert!(matches!(
            &events[0],
            NetworkInbound::Chat { sender, message }
                if sender == "Alex" && message == "hello"
        ));
        assert!(matches!(
            &events[1],
            NetworkInbound::Disconnected(reason) if reason == "server stopped"
        ));
    }

    #[test]
    fn network_handle_none_drains_no_inbound_events() {
        let handle = NetworkHandle::None;
        assert!(handle.drain_inbound().is_empty());
    }

    #[test]
    fn client_block_change_is_classified_as_host_authority() {
        let (inbound_tx, inbound_rx) = std::sync::mpsc::channel();
        let (outbound_tx, _outbound_rx) = std::sync::mpsc::channel();
        let handle = NetworkHandle::Client {
            client_to_game: inbound_rx,
            game_to_client: outbound_tx,
            thread: None,
        };
        inbound_tx
            .send(crate::network::client::ClientToGame::BlockChange {
                dimension: 0,
                revision: 1,
                x: 3,
                y: 80,
                z: -4,
                block: BlockType::Stone.to_wire(),
                state: 0,
                raw_fluid: 0,
            })
            .unwrap();

        assert!(matches!(
            handle.drain_inbound().as_slice(),
            [NetworkInbound::AuthoritativeBlockChange {
                x: 3,
                y: 80,
                z: -4,
                block,
                state: 0,
                ..
            }] if *block == BlockType::Stone.to_wire()
        ));
    }

    #[test]
    fn disconnect_cleanup_removes_only_remote_player_entities() {
        let mut entities = crate::entity::EntityManager::new();
        let remote_id = entities.spawn(crate::entity::EntityType::RemotePlayer, Vec3::ZERO);
        let zombie_id = entities.spawn(crate::entity::EntityType::Zombie, Vec3::ZERO);
        let mut remote_players = std::collections::HashMap::new();
        remote_players.insert(7, RemotePlayerState::new(remote_id, "Alex".to_string()));

        clear_remote_players(&mut remote_players, &mut entities);

        assert!(remote_players.is_empty());
        assert!(!entities
            .entities
            .iter()
            .any(|entity| entity.id == remote_id));
        assert!(entities
            .entities
            .iter()
            .any(|entity| entity.id == zombie_id));
    }

    #[test]
    fn every_biome_has_a_debug_name() {
        let biomes = [
            Biome::Plains,
            Biome::Forest,
            Biome::Desert,
            Biome::Taiga,
            Biome::Swamp,
            Biome::WindsweptHills,
            Biome::Ocean,
        ];
        assert!(biomes
            .into_iter()
            .all(|biome| !biome_debug_name(biome).is_empty()));
    }

    #[test]
    fn pause_weather_volume_and_quit_hit_regions_do_not_overlap() {
        assert!(point_in_bounds(0.0, -0.41, PAUSE_WEATHER_VOLUME_BOUNDS));
        assert!(!point_in_bounds(0.0, -0.41, PAUSE_QUIT_BOUNDS));
        assert!(point_in_bounds(0.0, -0.55, PAUSE_QUIT_BOUNDS));
        assert!(!point_in_bounds(0.0, -0.55, PAUSE_WEATHER_VOLUME_BOUNDS));
        assert!(!point_in_bounds(0.31, -0.41, PAUSE_WEATHER_VOLUME_BOUNDS));
    }

    #[test]
    fn fov_adjustment_updates_base_fov_and_camera_fov() {
        let mut base_fov: f32 = 70.0;
        let mut camera_fov: f32;

        // Simulate pause menu FOV increase (+5)
        base_fov = (base_fov + 5.0).min(120.0);
        camera_fov = base_fov;
        assert_eq!(base_fov, 75.0);
        assert_eq!(camera_fov, 75.0);

        // Simulate frame FOV interpolation when not sprinting
        let target_fov = base_fov;
        let dt = 0.016;
        camera_fov = camera_fov + (target_fov - camera_fov) * dt * 10.0;
        assert_eq!(camera_fov, 75.0);

        // Simulate pause menu FOV decrease (-5)
        base_fov = (base_fov - 5.0).max(30.0);
        camera_fov = base_fov;
        assert_eq!(base_fov, 70.0);
        assert_eq!(camera_fov, 70.0);
    }

    #[test]
    fn test_flower_breaks_and_pops_when_ground_is_destroyed() {
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));
        manager.set_block(2, 10, 2, BlockType::Grass);
        manager.set_block(2, 11, 2, BlockType::Dandelion);

        let mut dirty = std::collections::HashSet::new();
        let mut drops = Vec::new();

        // Destroy the grass block
        manager.set_block(2, 10, 2, BlockType::Air);
        manager.check_and_break_unsupported_above(2, 10, 2, &mut dirty, |pos, block| {
            drops.push((pos, block));
        });

        // Ground is Air now, flower above must be destroyed
        assert_eq!(manager.get_block(2, 11, 2), BlockType::Air);
        assert_eq!(drops, vec![((2, 11, 2), BlockType::Dandelion)]);
    }

    #[test]
    fn door_and_trapdoor_placement_states_and_hinges() {
        let mut manager = ChunkManager::new(2);
        manager.chunks.insert((0, 0), Chunk::new(0, 0));

        // Test door facing from yaw (yaw=0.0 -> East, yaw=FRAC_PI_2 -> South, -FRAC_PI_2 -> North, PI -> West)
        let (bottom, top) = crate::world::BlockState::for_door_placement(
            &manager,
            5,
            64,
            5,
            std::f32::consts::FRAC_PI_2,
        );
        assert_eq!(bottom.facing, crate::redstone::Direction::South);
        assert!(!bottom.is_top);
        assert!(!bottom.is_open);
        assert!(top.is_top);
        assert_eq!(top.facing, crate::redstone::Direction::South);

        // Hinge logic: left solid, right empty -> right hinge
        // North facing: left = West (-1, 0) -> (4, 64, 5), right = East (+1, 0) -> (6, 64, 5)
        manager.set_block(4, 64, 5, BlockType::Stone); // left neighbor
        manager.set_block(6, 64, 5, BlockType::Air); // right neighbor
        let (bottom_hinge, _) = crate::world::BlockState::for_door_placement(
            &manager,
            5,
            64,
            5,
            -std::f32::consts::FRAC_PI_2,
        );
        assert_eq!(bottom_hinge.facing, crate::redstone::Direction::North);
        assert!(bottom_hinge.is_right_hinge);

        // Trapdoor state
        let trapdoor =
            crate::world::BlockState::for_trapdoor_placement(-std::f32::consts::FRAC_PI_2);
        assert_eq!(trapdoor.facing, crate::redstone::Direction::North);
        assert!(!trapdoor.is_open);
    }
}

#[cfg(test)]
mod reach_tests {
    use super::*;

    #[test]
    fn calculate_block_break_rewards_harvest_and_drops() {
        let pos = (10, 60, 10);

        // Stone with bare hand in Survival -> not eligible to harvest (no drops)
        let rewards =
            calculate_block_break_rewards(BlockType::Stone, 0, pos, None, GameMode::Survival);
        assert!(rewards.drops.is_empty());
        assert_eq!(rewards.xp, 0);

        // Stone with Pickaxe -> eligible, drops Stone
        let pick = ItemStack::new(Item::StonePickaxe, 1);
        let rewards = calculate_block_break_rewards(
            BlockType::Stone,
            0,
            pos,
            Some(&pick),
            GameMode::Survival,
        );
        assert_eq!(rewards.drops.len(), 1);
        assert_eq!(rewards.drops[0].item, Item::Stone);

        // DiamondOre with IronPickaxe -> drops Diamond + 5 XP
        let iron_pick = ItemStack::new(Item::IronPickaxe, 1);
        let rewards = calculate_block_break_rewards(
            BlockType::DiamondOre,
            0,
            pos,
            Some(&iron_pick),
            GameMode::Survival,
        );
        assert_eq!(rewards.drops[0].item, Item::Diamond);
        assert_eq!(rewards.xp, 5);

        // DiamondOre with SilkTouch -> drops DiamondOre block
        let mut silk_pick = ItemStack::new(Item::IronPickaxe, 1);
        silk_pick
            .enchantments
            .add_or_upgrade(crate::enchantment::Enchantment::SilkTouch);
        let rewards = calculate_block_break_rewards(
            BlockType::DiamondOre,
            0,
            pos,
            Some(&silk_pick),
            GameMode::Survival,
        );
        assert_eq!(rewards.drops[0].item, Item::DiamondOre);
        assert_eq!(rewards.xp, 0);

        // Creative mode -> zero drops
        let rewards = calculate_block_break_rewards(
            BlockType::Stone,
            0,
            pos,
            Some(&pick),
            GameMode::Creative,
        );
        assert!(rewards.drops.is_empty());
    }

    #[test]
    fn calculate_block_break_rewards_mature_and_immature_crops() {
        let pos = (10, 60, 10);

        // Mature Wheat (age 7) -> Wheat
        let mature_wheat =
            calculate_block_break_rewards(BlockType::WheatCrop, 7, pos, None, GameMode::Survival);
        assert_eq!(mature_wheat.drops.len(), 1);
        assert_eq!(mature_wheat.drops[0].item, Item::Wheat);

        // Immature Wheat (age 3) -> Seeds
        let immature_wheat =
            calculate_block_break_rewards(BlockType::WheatCrop, 3, pos, None, GameMode::Survival);
        assert_eq!(immature_wheat.drops.len(), 1);
        assert_eq!(immature_wheat.drops[0].item, Item::Seeds);

        // Immature Carrot (age 2) -> Carrot
        let immature_carrot =
            calculate_block_break_rewards(BlockType::CarrotCrop, 2, pos, None, GameMode::Survival);
        assert_eq!(immature_carrot.drops.len(), 1);
        assert_eq!(immature_carrot.drops[0].item, Item::Carrot);
    }

    #[test]
    fn inventory_click_outside_and_close_overflow_tests() {
        let mut inv = Inventory::new();
        inv.dragged = Some(ItemStack::new(Item::Dirt, 64));
        assert_eq!(inv.dragged.unwrap().count, 64);

        // Fill inventory completely
        for slot in inv.hotbar.iter_mut() {
            *slot = Some(ItemStack::new(Item::Stone, 64));
        }
        for slot in inv.main.iter_mut() {
            *slot = Some(ItemStack::new(Item::Stone, 64));
        }

        // add_stack with full inventory returns remainder
        let remainder = inv.add_stack(ItemStack::new(Item::Dirt, 64));
        assert_eq!(remainder, Some(ItemStack::new(Item::Dirt, 64)));
    }

    #[test]
    fn inventory_slot_hits_map_to_inventory_decision_targets() {
        assert_eq!(
            presentation_target_for_slot(SlotType::ContainerSlot(3)),
            PresentationInventoryTarget::ContainerSlot
        );
        assert_eq!(
            presentation_target_for_slot(SlotType::Hotbar(0)),
            PresentationInventoryTarget::PlayerInventory
        );
        assert_eq!(
            presentation_target_for_slot(SlotType::Backpack(4)),
            PresentationInventoryTarget::PlayerInventory
        );
        assert_eq!(
            presentation_target_for_slot(SlotType::AnvilOutput),
            PresentationInventoryTarget::Workstation
        );
        assert_eq!(
            presentation_target_for_slot(SlotType::EnchantInput),
            PresentationInventoryTarget::Workstation
        );
        assert_eq!(
            presentation_target_for_slot(SlotType::EnchantLapis),
            PresentationInventoryTarget::Workstation
        );
        assert_eq!(
            presentation_target_for_authority_hit(InventoryHit::Merchant { offer_index: 0 }),
            None
        );
        assert_eq!(
            presentation_target_for_authority_hit(InventoryHit::RecipeBook),
            Some(PresentationInventoryTarget::Workstation)
        );
        assert_eq!(
            presentation_target_for_authority_hit(InventoryHit::Empty),
            None
        );

        let embedded = PresentationTopology::Embedded;
        let join = PresentationTopology::JoinClient;
        assert_eq!(
            embedded.inventory_decision(PresentationInventoryTarget::ContainerSlot),
            PresentationInventoryAction::SendAuthorityOp
        );
        assert_eq!(
            join.inventory_decision(PresentationInventoryTarget::ContainerSlot),
            PresentationInventoryAction::SendAuthorityOp
        );
        assert_eq!(
            embedded.inventory_decision(PresentationInventoryTarget::PlayerInventory),
            PresentationInventoryAction::LocalMutate
        );
        assert_eq!(
            join.inventory_decision(PresentationInventoryTarget::PlayerInventory),
            PresentationInventoryAction::Reject
        );
        assert_eq!(
            embedded.inventory_decision(PresentationInventoryTarget::Workstation),
            PresentationInventoryAction::Reject
        );
        assert_eq!(
            join.inventory_decision(PresentationInventoryTarget::Workstation),
            PresentationInventoryAction::Reject
        );
    }
}

#[cfg(test)]
mod authority_projection_tests {
    use super::*;

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
}
