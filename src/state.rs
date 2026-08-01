use crate::camera::{Camera, CameraUniform};
use crate::chunk_manager::{
    mark_block_mesh_dependencies, mark_section_mesh_dependencies, surrounding_chunk_coords,
    ChunkManager,
};
use crate::chunk_render::{
    select_lod_for_bounds, DrawCandidate, DrawLayer, Frustum, LodLevel, LodThresholds, MeshBounds,
    TerrainVertex,
};
use crate::chunk_schedule::DependencyReason;
use crate::crafting::RecipeManager;
use crate::interaction::{raycast, RaycastTargetPolicy};
use crate::inventory::{
    CreativeTab, GameMode, Inventory, Item, ItemStack, ToolType, CREATIVE_COLUMNS, CREATIVE_ROWS,
    CREATIVE_VISIBLE_SLOTS,
};
use crate::menu::{Difficulty, GameSettings, MultiplayerRole, WorldLaunch};
use crate::physics::{
    block_placement_decision, player_aabb_at, BlockPlacementDecision, PlayerPhysics, AABB,
    PLAYER_STANDING_HEIGHT,
};
use crate::player::{DamageSource, PlayerState};
use crate::world::{
    Biome, BlockType, Chunk, SectionIdentity, SectionKey, CHUNK_DEPTH, CHUNK_HEIGHT, CHUNK_WIDTH,
    SECTION_COUNT,
};
use glam::{Mat4, Vec2, Vec3};
use std::sync::Arc;
use std::time::{Duration, Instant};
use wgpu::util::DeviceExt;
use winit::window::Window;

const UI_VERTEX_CAPACITY: usize = 4096;
const UI_LINE_VERTEX_CAPACITY: usize = 16384;
const DEBUG_STATS_INTERVAL: f32 = 0.5;
const RAIN_LOOP_ID: u64 = u64::MAX - 1;
const CHAT_HISTORY_CAPACITY: usize = 50;
const CHAT_VISIBLE_LINES: usize = 8;
const CHAT_INPUT_CAPACITY: usize = 256;
const REMOTE_SNAPSHOT_CAPACITY: usize = 32;
const REMOTE_INTERPOLATION_DELAY: f64 = 0.1;
const ENTITY_SNAPSHOT_CAPACITY: usize = 8;
const ENTITY_INTERPOLATION_DELAY: f64 = 0.1;
const ENTITY_SNAP_DISTANCE: f32 = 6.0;
const PLAYER_CORRECTION_SNAP_DISTANCE: f32 = 4.0;
const REMOTE_MAX_EXTRAPOLATION: f64 = 0.1;
const REMOTE_MAX_EXTRAPOLATION_SPEED: f32 = 40.0;
const REMOTE_AUTHORITY_MAX_SPEED: f32 = 12.0;
const REMOTE_AUTHORITY_POSITION_TOLERANCE: f32 = 1.0;
const REMOTE_MAX_ANGULAR_SPEED: f32 = std::f32::consts::TAU * 2.0;
const REMOTE_TELEPORT_DISTANCE: f32 = 8.0;
const REMOTE_TELEPORT_GAP: f64 = 0.5;
const CREATIVE_FLIGHT_DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(300);
const MELEE_REACH: f32 = 4.0;
const BLOCK_REACH: f32 = 5.0;
const BLOCK_REACH_TOLERANCE: f32 = 1.5;
const MAX_CATCHUP_SUBMITS_PER_FRAME: usize = 2;
const CATCHUP_ACK_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_CATCHUP_RETRIES: u8 = 3;
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

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct UploadMetrics {
    elapsed_ns: u64,
    bytes: u64,
}

impl UploadMetrics {
    fn add(self, other: Self) -> Self {
        Self {
            elapsed_ns: self.elapsed_ns.saturating_add(other.elapsed_ns),
            bytes: self.bytes.saturating_add(other.bytes),
        }
    }
}

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

fn validated_remote_position(
    latest: Option<&PlayerSnapshot>,
    candidate: Vec3,
    sender_time_millis: u64,
) -> Vec3 {
    let Some(latest) = latest else {
        return candidate;
    };
    if sender_time_millis <= latest.sender_time_millis {
        return latest.position;
    }
    let elapsed =
        ((sender_time_millis - latest.sender_time_millis) as f32 / 1_000.0).clamp(0.0, 0.5);
    let max_distance = REMOTE_AUTHORITY_MAX_SPEED * elapsed + REMOTE_AUTHORITY_POSITION_TOLERANCE;
    let delta = candidate - latest.position;
    if delta.length_squared() <= max_distance * max_distance {
        candidate
    } else {
        latest.position + delta.normalize_or_zero() * max_distance
    }
}

fn point_in_bounds(x: f32, y: f32, bounds: [f32; 4]) -> bool {
    x >= bounds[0] && x <= bounds[1] && y >= bounds[2] && y <= bounds[3]
}

fn block_within_reach(player_pos: Vec3, block_pos: (i32, i32, i32)) -> bool {
    let block_center = Vec3::new(
        block_pos.0 as f32 + 0.5,
        block_pos.1 as f32 + 0.5,
        block_pos.2 as f32 + 0.5,
    );
    let limit = BLOCK_REACH + BLOCK_REACH_TOLERANCE;
    (player_pos - block_center).length() <= limit
}

fn validate_remote_block_request(
    remote_players: &std::collections::HashMap<
        crate::network::protocol::PlayerId,
        RemotePlayerState,
    >,
    requester: crate::network::protocol::PlayerId,
    block_pos: (i32, i32, i32),
) -> bool {
    let Some(remote) = remote_players.get(&requester) else {
        return false;
    };
    let Some(snapshot) = remote.snapshots.back() else {
        return false;
    };
    let player_center = snapshot.position + Vec3::new(0.0, PLAYER_STANDING_HEIGHT * 0.5, 0.0);
    block_within_reach(player_center, block_pos)
}

fn terrain_translucent_cull_mode() -> Option<wgpu::Face> {
    None
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
    const MELEE_TYPES: [crate::entity::EntityType; 14] = [
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MeleeImpact {
    Invulnerable,
    Damaged { killed: bool },
}

fn apply_melee_impact(
    entity: &mut crate::entity::Entity,
    direction: Vec3,
    damage: f32,
    knockback: f32,
    fire_level: u8,
) -> MeleeImpact {
    if entity.invulnerable_time > 0.0 {
        return MeleeImpact::Invulnerable;
    }

    entity.health -= damage;
    entity.invulnerable_time = 0.4;
    entity.velocity += direction.normalize_or_zero() * knockback + Vec3::new(0.0, 3.0, 0.0);
    if fire_level > 0 {
        entity.fire_aspect_timer = entity.fire_aspect_timer.max(fire_level as f32 * 4.0);
    }

    MeleeImpact::Damaged {
        killed: entity.health <= 0.0,
    }
}

#[derive(Debug, Clone, Copy)]
struct PlayerKill {
    entity_type: crate::entity::EntityType,
    position: Vec3,
    burning: bool,
    has_wool: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct PlayerKillRewards {
    items: Vec<Item>,
    experience: u32,
}

fn claim_standard_player_kill(entity: &mut crate::entity::Entity) -> Option<PlayerKill> {
    if entity.health > 0.0
        || entity.player_kill_rewarded
        || !entity.entity_type.uses_standard_player_kill_rewards()
    {
        return None;
    }

    entity.player_kill_rewarded = true;
    Some(PlayerKill {
        entity_type: entity.entity_type,
        position: entity.position,
        burning: entity.burn_timer > 0.0 || entity.fire_aspect_timer > 0.0,
        has_wool: entity.has_wool,
    })
}

fn apply_player_projectile_damage(
    entity: &mut crate::entity::Entity,
    damage: f32,
) -> Option<PlayerKill> {
    if !entity.is_player_projectile_target() || damage <= 0.0 {
        return None;
    }

    entity.health -= damage;
    claim_standard_player_kill(entity)
}

fn apply_player_splash_effect(
    entity: &mut crate::entity::Entity,
    potion: crate::brewing::PotionData,
) -> Option<PlayerKill> {
    if !entity.is_local_living_target() {
        return None;
    }

    match potion.kind {
        crate::brewing::PotionKind::Healing | crate::brewing::PotionKind::Regeneration => {
            entity.health = (entity.health + 4.0 * potion.level as f32).min(entity.max_health);
        }
        crate::brewing::PotionKind::Poison => {
            entity.health -= 2.0 * potion.level as f32;
        }
        crate::brewing::PotionKind::Slowness => entity.velocity *= 0.4,
        _ => {}
    }

    claim_standard_player_kill(entity)
}

fn standard_player_kill_rewards(kill: PlayerKill, looting: u8) -> PlayerKillRewards {
    let mut items = Vec::new();
    for _ in 0..=(looting / 2) {
        match kill.entity_type {
            crate::entity::EntityType::Zombie => items.push(Item::RottenFlesh),
            crate::entity::EntityType::Skeleton => {
                items.push(Item::Bone);
                items.push(Item::Arrow);
                let mut rng_seed = (kill.position.x as u32)
                    .wrapping_mul(31)
                    .wrapping_add(kill.position.z as u32);
                rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
                if ((rng_seed / 65536) % 32768) % 10 == 0 {
                    items.push(Item::Bow);
                }
            }
            crate::entity::EntityType::Creeper => items.push(Item::Gunpowder),
            crate::entity::EntityType::Pig => items.push(if kill.burning {
                Item::CookedPorkchop
            } else {
                Item::RawPorkchop
            }),
            crate::entity::EntityType::Cow => {
                items.push(Item::RawBeef);
                if (kill.position.x as u32).wrapping_mul(31) % 2 == 0 {
                    items.push(Item::Leather);
                }
            }
            crate::entity::EntityType::Sheep => {
                items.push(Item::RawMutton);
                if kill.has_wool {
                    items.push(Item::Wool);
                }
            }
            crate::entity::EntityType::Chicken => {
                items.push(Item::RawChicken);
                items.push(Item::Feather);
            }
            _ => {}
        }
    }

    let experience = match kill.entity_type {
        crate::entity::EntityType::Zombie
        | crate::entity::EntityType::Skeleton
        | crate::entity::EntityType::Creeper => 5,
        _ => 2,
    };
    PlayerKillRewards { items, experience }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeneratedItemDestination {
    Inventory,
    Dropped,
    IgnoredAir,
}

fn spawn_dropped_item_entity(
    entity_manager: &mut crate::entity::EntityManager,
    item: Item,
    position: Vec3,
    random_seed: u32,
) -> bool {
    if item == Item::Air {
        return false;
    }

    let id = entity_manager.spawn(crate::entity::EntityType::DroppedItem, position);
    let Some(entity) = entity_manager.entities.last_mut() else {
        return false;
    };
    entity.dropped_item = Some(item);

    let mut rng = random_seed.wrapping_add((id.wrapping_mul(2_654_435_761)) as u32);
    rng = rng.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    let vx = ((rng / 65_536) as f32 / 32_768.0 - 0.5) * 1.5;
    rng = rng.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    let vz = ((rng / 65_536) as f32 / 32_768.0 - 0.5) * 1.5;
    rng = rng.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    let vy = 2.0 + ((rng / 65_536) as f32 / 32_768.0);
    entity.velocity = Vec3::new(vx, vy, vz);
    entity.pickup_cooldown = 0.5;
    true
}

fn store_or_drop_generated_item(
    inventory: &mut Inventory,
    entity_manager: &mut crate::entity::EntityManager,
    item: Item,
    position: Vec3,
    random_seed: u32,
) -> GeneratedItemDestination {
    if item == Item::Air {
        return GeneratedItemDestination::IgnoredAir;
    }
    if inventory.add_item(item) {
        return GeneratedItemDestination::Inventory;
    }

    let spawned = spawn_dropped_item_entity(entity_manager, item, position, random_seed);
    debug_assert!(spawned);
    GeneratedItemDestination::Dropped
}

// Creating an entire render distance while handling a menu click blocks the
// window event loop and can allocate hundreds of chunk meshes at once.  Start
// with a safe area around the player; `update_chunks` streams the rest in over
// subsequent frames.
const INITIAL_WORLD_CHUNK_RADIUS: i32 = 1;

fn initial_chunk_radius(render_distance: i32) -> i32 {
    render_distance.clamp(0, INITIAL_WORLD_CHUNK_RADIUS)
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
    if old_properties.is_solid != new_properties.is_solid {
        if new_properties.is_solid {
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
            .section_mut(key.section_y as usize)
            .unwrap();
        section.connectivity = crate::culling::SectionConnectivityState::Valid(
            crate::culling::SectionConnectivity::NONE,
        );
        section.meshed_revision = section.revision;
        let mut scheduler = crate::chunk_schedule::SectionMeshScheduler::new();

        section.invalidate();
        let first_revision = section.revision;
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
                .section_mut(key.section_y as usize)
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

pub struct GpuMeshLayer {
    pub handle: Option<crate::chunk_render::RegionAllocationHandle>,
    pub bounds: Option<MeshBounds>,
    pub vertex_bytes: usize,
    pub index_bytes: usize,
}

impl GpuMeshLayer {
    pub fn empty() -> Self {
        Self {
            handle: None,
            bounds: None,
            vertex_bytes: 0,
            index_bytes: 0,
        }
    }

    pub fn num_indices(&self) -> u32 {
        self.handle.map_or(0, |h| h.num_indices)
    }
}

pub struct RenderRegion {
    pub region_coord: (i32, i32),
    pub region_instance_id: u64,
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub vertex_capacity: u32,
    pub index_capacity: u32,
    pub vertex_freelist: crate::chunk_render::FreeList,
    pub index_freelist: crate::chunk_render::FreeList,
    pub active_chunks: usize,
    pub region_uniform_buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
}

impl RenderRegion {
    pub const INITIAL_VERTEX_CAPACITY: u32 = 65_536;
    pub const INITIAL_INDEX_CAPACITY: u32 = 98_304;

    pub fn new(
        device: &wgpu::Device,
        region_bind_group_layout: &wgpu::BindGroupLayout,
        region_coord: (i32, i32),
    ) -> Self {
        static NEXT_REGION_INSTANCE_ID: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(1);
        let region_instance_id = NEXT_REGION_INSTANCE_ID
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .max(1);
        let vertex_bytes = (Self::INITIAL_VERTEX_CAPACITY as usize)
            * std::mem::size_of::<crate::chunk_render::TerrainVertex>();
        let index_bytes = (Self::INITIAL_INDEX_CAPACITY as usize) * std::mem::size_of::<u32>();

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Render Region Vertex Buffer"),
            size: vertex_bytes as u64,
            usage: wgpu::BufferUsages::VERTEX
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Render Region Index Buffer"),
            size: index_bytes as u64,
            usage: wgpu::BufferUsages::INDEX
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let reg_origin = [
            (region_coord.0
                * crate::chunk_render::REGION_SIZE_CHUNKS
                * crate::world::CHUNK_WIDTH as i32) as f32,
            0.0,
            (region_coord.1
                * crate::chunk_render::REGION_SIZE_CHUNKS
                * crate::world::CHUNK_DEPTH as i32) as f32,
            0.0,
        ];
        use wgpu::util::DeviceExt;
        let region_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Region Uniform Buffer"),
            contents: bytemuck::cast_slice(&reg_origin),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: region_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: region_uniform_buffer.as_entire_binding(),
            }],
            label: Some("Region Bind Group"),
        });

        Self {
            region_coord,
            region_instance_id,
            vertex_buffer,
            index_buffer,
            vertex_capacity: Self::INITIAL_VERTEX_CAPACITY,
            index_capacity: Self::INITIAL_INDEX_CAPACITY,
            vertex_freelist: crate::chunk_render::FreeList::new(Self::INITIAL_VERTEX_CAPACITY),
            index_freelist: crate::chunk_render::FreeList::new(Self::INITIAL_INDEX_CAPACITY),
            active_chunks: 0,
            region_uniform_buffer,
            bind_group,
        }
    }

    pub fn deallocate_handle(
        &mut self,
        handle: &crate::chunk_render::RegionAllocationHandle,
    ) -> Result<(), crate::chunk_render::FreeListError> {
        if !region_allocation_handle_is_live(
            self.region_instance_id,
            &self.vertex_freelist,
            &self.index_freelist,
            handle,
        ) {
            return Err(crate::chunk_render::FreeListError::UnknownAllocation);
        }
        self.vertex_freelist.deallocate_owned(handle.vertex_token)?;
        self.index_freelist.deallocate_owned(handle.index_token)?;
        Ok(())
    }

    fn handle_is_live(&self, handle: &crate::chunk_render::RegionAllocationHandle) -> bool {
        region_allocation_handle_is_live(
            self.region_instance_id,
            &self.vertex_freelist,
            &self.index_freelist,
            handle,
        )
    }

    fn empty_rebuild_worthwhile(&self) -> bool {
        empty_region_rebuild_worthwhile(
            self.vertex_freelist.used_units(),
            self.index_freelist.used_units(),
            self.vertex_capacity,
            self.index_capacity,
        )
    }

    pub fn committed_bytes(&self) -> usize {
        (self.vertex_capacity as usize) * std::mem::size_of::<crate::chunk_render::TerrainVertex>()
            + (self.index_capacity as usize) * std::mem::size_of::<u32>()
    }

    pub fn used_bytes(&self) -> usize {
        (self.vertex_freelist.used_units() as usize)
            * std::mem::size_of::<crate::chunk_render::TerrainVertex>()
            + (self.index_freelist.used_units() as usize) * std::mem::size_of::<u32>()
    }

    pub fn buffer_object_count(&self) -> usize {
        // vertex + index + region uniform; bind groups are not buffers.
        3
    }

    pub fn ensure_capacity(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        needed_vertices: u32,
        needed_indices: u32,
    ) -> Result<(), crate::chunk_render::FreeListError> {
        let mut grow_v = false;
        let mut new_v_cap = self.vertex_capacity;
        if self.vertex_freelist.largest_free_block() < needed_vertices {
            grow_v = true;
            new_v_cap = self
                .vertex_capacity
                .checked_add(needed_vertices)
                .and_then(|needed| {
                    self.vertex_capacity
                        .checked_mul(2)
                        .map(|doubled| needed.max(doubled))
                })
                .ok_or(crate::chunk_render::FreeListError::ArithmeticOverflow)?;
        }

        let mut grow_i = false;
        let mut new_i_cap = self.index_capacity;
        if self.index_freelist.largest_free_block() < needed_indices {
            grow_i = true;
            new_i_cap = self
                .index_capacity
                .checked_add(needed_indices)
                .and_then(|needed| {
                    self.index_capacity
                        .checked_mul(2)
                        .map(|doubled| needed.max(doubled))
                })
                .ok_or(crate::chunk_render::FreeListError::ArithmeticOverflow)?;
        }

        if grow_v {
            self.vertex_freelist.resize(new_v_cap)?;
            let vertex_bytes =
                (new_v_cap as usize) * std::mem::size_of::<crate::chunk_render::TerrainVertex>();
            let new_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Render Region Vertex Buffer (Resized)"),
                size: vertex_bytes as u64,
                usage: wgpu::BufferUsages::VERTEX
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });

            if self.vertex_freelist.used_units() > 0 {
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Resize Region Vertex Buffer Encoder"),
                });
                let copy_size = (self.vertex_capacity as usize
                    * std::mem::size_of::<crate::chunk_render::TerrainVertex>())
                    as u64;
                encoder.copy_buffer_to_buffer(
                    &self.vertex_buffer,
                    0,
                    &new_vertex_buffer,
                    0,
                    copy_size,
                );
                queue.submit(Some(encoder.finish()));
            }

            self.vertex_buffer = new_vertex_buffer;
            self.vertex_capacity = new_v_cap;
        }

        if grow_i {
            self.index_freelist.resize(new_i_cap)?;
            let index_bytes = (new_i_cap as usize) * std::mem::size_of::<u32>();
            let new_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Render Region Index Buffer (Resized)"),
                size: index_bytes as u64,
                usage: wgpu::BufferUsages::INDEX
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });

            if self.index_freelist.used_units() > 0 {
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Resize Region Index Buffer Encoder"),
                });
                let copy_size = (self.index_capacity as usize * std::mem::size_of::<u32>()) as u64;
                encoder.copy_buffer_to_buffer(
                    &self.index_buffer,
                    0,
                    &new_index_buffer,
                    0,
                    copy_size,
                );
                queue.submit(Some(encoder.finish()));
            }

            self.index_buffer = new_index_buffer;
            self.index_capacity = new_i_cap;
        }
        Ok(())
    }

    pub fn upload_mesh_layer(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        data: &crate::chunk_render::ChunkMeshData,
        owner: u64,
    ) -> (GpuMeshLayer, UploadMetrics) {
        if data.is_empty() {
            return (GpuMeshLayer::empty(), UploadMetrics::default());
        }

        let num_vertices = data.vertices.len() as u32;
        let num_indices = data.indices.len() as u32;

        self.ensure_capacity(device, queue, num_vertices, num_indices)
            .unwrap_or_else(|error| {
                panic!("render-region freelist capacity growth failed: {error:?}")
            });

        let vertex_token = self
            .vertex_freelist
            .allocate_owned(num_vertices, owner)
            .map_err(|e| format!("vertex allocation failed: {e:?}"))
            .expect("vertex freelist allocation failed");
        let index_token = self
            .index_freelist
            .allocate_owned(num_indices, owner)
            .map_err(|e| format!("index allocation failed: {e:?}"))
            .expect("index freelist allocation failed");
        let vertex_offset = vertex_token.offset;
        let index_offset = index_token.offset;

        let vertex_bytes = bytemuck::cast_slice(&data.vertices);
        let index_bytes = bytemuck::cast_slice(&data.indices);

        let v_byte_offset = (vertex_offset as usize
            * std::mem::size_of::<crate::chunk_render::TerrainVertex>())
            as u64;
        let i_byte_offset = (index_offset as usize * std::mem::size_of::<u32>()) as u64;

        let upload_started = Instant::now();
        queue.write_buffer(&self.vertex_buffer, v_byte_offset, vertex_bytes);
        queue.write_buffer(&self.index_buffer, i_byte_offset, index_bytes);
        let metrics = UploadMetrics {
            elapsed_ns: upload_started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
            bytes: (vertex_bytes.len() + index_bytes.len()) as u64,
        };

        (
            GpuMeshLayer {
                handle: Some(crate::chunk_render::RegionAllocationHandle {
                    region_instance_id: self.region_instance_id,
                    vertex_token,
                    index_token,
                    vertex_offset,
                    index_offset,
                    num_vertices,
                    num_indices,
                }),
                bounds: data.bounds,
                vertex_bytes: vertex_bytes.len(),
                index_bytes: index_bytes.len(),
            },
            metrics,
        )
    }
}

fn region_allocation_handle_is_live(
    region_instance_id: u64,
    vertex_freelist: &crate::chunk_render::FreeList,
    index_freelist: &crate::chunk_render::FreeList,
    handle: &crate::chunk_render::RegionAllocationHandle,
) -> bool {
    handle.region_instance_id == region_instance_id
        && vertex_freelist.validate_owned(handle.vertex_token).is_ok()
        && index_freelist.validate_owned(handle.index_token).is_ok()
}

fn should_decrement_region_active_chunks(
    mesh_has_resident_section: bool,
    mesh_has_allocation_handles: bool,
    mesh_has_matching_region_handle: bool,
) -> bool {
    mesh_has_resident_section && (!mesh_has_allocation_handles || mesh_has_matching_region_handle)
}

fn chunk_mesh_is_registered_with_region(mesh: &ChunkMesh, region: Option<&RenderRegion>) -> bool {
    if !mesh.has_resident_section() {
        return false;
    }
    let Some(region) = region else {
        return false;
    };
    let (has_handles, has_matching_handle) =
        mesh.allocation_handle_region_membership(region.region_instance_id);
    !has_handles || has_matching_handle
}

fn empty_region_rebuild_worthwhile(
    used_vertices: u32,
    used_indices: u32,
    vertex_capacity: u32,
    index_capacity: u32,
) -> bool {
    used_vertices == 0
        && used_indices == 0
        && (vertex_capacity > RenderRegion::INITIAL_VERTEX_CAPACITY
            || index_capacity > RenderRegion::INITIAL_INDEX_CAPACITY)
}

pub struct GpuMeshLevel {
    opaque: GpuMeshLayer,
    transparent: GpuMeshLayer,
    bounds: Option<MeshBounds>,
}

pub struct GpuSectionMesh {
    levels: Option<[GpuMeshLevel; 3]>,
    connectivity: crate::culling::SectionConnectivityState,
    revision: u64,
    meshed_revision: u64,
}

impl GpuSectionMesh {
    fn pending() -> Self {
        Self {
            levels: None,
            connectivity: crate::culling::SectionConnectivityState::Invalid,
            revision: 0,
            meshed_revision: u64::MAX,
        }
    }

    fn invalidate(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.connectivity = crate::culling::SectionConnectivityState::Invalid;
    }

    fn needs_rebuild(&self) -> bool {
        self.levels.is_none() || self.meshed_revision != self.revision
    }

    fn level(&self, lod: LodLevel) -> Option<&GpuMeshLevel> {
        self.levels.as_ref().map(|levels| &levels[lod as usize])
    }

    fn finest_bounds(&self) -> Option<MeshBounds> {
        self.level(LodLevel::L0).and_then(|level| level.bounds)
    }

    fn total_indices(&self) -> usize {
        self.levels
            .as_ref()
            .into_iter()
            .flatten()
            .map(|level| {
                level.opaque.num_indices() as usize + level.transparent.num_indices() as usize
            })
            .sum()
    }

    fn gpu_bytes(&self) -> usize {
        self.levels
            .as_ref()
            .into_iter()
            .flatten()
            .map(|level| {
                level.opaque.vertex_bytes
                    + level.opaque.index_bytes
                    + level.transparent.vertex_bytes
                    + level.transparent.index_bytes
            })
            .sum()
    }
}

pub struct ChunkMesh {
    sections: [GpuSectionMesh; SECTION_COUNT],
}

impl ChunkMesh {
    fn pending() -> Self {
        Self {
            sections: std::array::from_fn(|_| GpuSectionMesh::pending()),
        }
    }

    fn section(&self, section_y: usize) -> Option<&GpuSectionMesh> {
        self.sections.get(section_y)
    }

    fn section_mut(&mut self, section_y: usize) -> Option<&mut GpuSectionMesh> {
        self.sections.get_mut(section_y)
    }

    fn finest_bounds(&self) -> Option<MeshBounds> {
        self.sections
            .iter()
            .filter_map(GpuSectionMesh::finest_bounds)
            .reduce(|left, right| left.union(right))
    }

    fn total_indices(&self) -> usize {
        self.sections
            .iter()
            .map(GpuSectionMesh::total_indices)
            .sum()
    }

    fn gpu_bytes(&self) -> usize {
        self.sections.iter().map(GpuSectionMesh::gpu_bytes).sum()
    }

    fn has_resident_section(&self) -> bool {
        self.sections.iter().any(|section| section.levels.is_some())
    }

    fn allocation_handle_region_membership(&self, region_instance_id: u64) -> (bool, bool) {
        let mut has_handles = false;
        let mut has_matching_handle = false;
        for section in &self.sections {
            let Some(levels) = &section.levels else {
                continue;
            };
            for level in levels {
                for layer in [&level.opaque, &level.transparent] {
                    let Some(handle) = layer.handle else {
                        continue;
                    };
                    has_handles = true;
                    has_matching_handle |= handle.region_instance_id == region_instance_id;
                }
            }
        }
        (has_handles, has_matching_handle)
    }
}

#[derive(Clone, Copy)]
struct MeshVoxel {
    block: BlockType,
    sky_light: u8,
    block_light: u8,
    fluid: u8,
}

struct MeshSnapshot {
    min_world_x: i32,
    min_world_z: i32,
    voxels: Vec<MeshVoxel>,
    default_sky_light: u8,
}

impl MeshSnapshot {
    const WIDTH: usize = CHUNK_WIDTH + 2;
    const DEPTH: usize = CHUNK_DEPTH + 2;

    fn capture(
        coord: (i32, i32),
        chunks: &std::collections::HashMap<(i32, i32), Chunk>,
        default_sky_light: u8,
    ) -> Option<Self> {
        if !chunks.contains_key(&coord) {
            return None;
        }
        let min_world_x = coord.0 * CHUNK_WIDTH as i32 - 1;
        let min_world_z = coord.1 * CHUNK_DEPTH as i32 - 1;
        let mut voxels = Vec::with_capacity(Self::WIDTH * CHUNK_HEIGHT * Self::DEPTH);
        for x in 0..Self::WIDTH {
            let world_x = min_world_x + x as i32;
            let chunk_x = world_x.div_euclid(CHUNK_WIDTH as i32);
            let local_x = world_x.rem_euclid(CHUNK_WIDTH as i32) as usize;
            for y in 0..CHUNK_HEIGHT {
                for z in 0..Self::DEPTH {
                    let world_z = min_world_z + z as i32;
                    let chunk_z = world_z.div_euclid(CHUNK_DEPTH as i32);
                    let local_z = world_z.rem_euclid(CHUNK_DEPTH as i32) as usize;
                    let voxel = chunks
                        .get(&(chunk_x, chunk_z))
                        .map(|neighbor| MeshVoxel {
                            block: neighbor.get_block_local(local_x, y, local_z),
                            sky_light: neighbor.get_sky_light(local_x, y, local_z),
                            block_light: neighbor.get_block_light(local_x, y, local_z),
                            fluid: neighbor.get_fluid_level(local_x, y, local_z),
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
            min_world_z,
            voxels,
            default_sky_light,
        })
    }

    fn get(&self, world_x: i32, world_y: i32, world_z: i32) -> (BlockType, u8, u8, u8, bool) {
        if world_y < 0 {
            return (BlockType::Air, 0, 0, 0, false);
        }
        if world_y >= CHUNK_HEIGHT as i32 {
            return (BlockType::Air, self.default_sky_light, 0, 0, false);
        }
        let x = world_x - self.min_world_x;
        let z = world_z - self.min_world_z;
        if x < 0 || x >= Self::WIDTH as i32 || z < 0 || z >= Self::DEPTH as i32 {
            return (BlockType::Air, self.default_sky_light, 0, 0, false);
        }
        let index = (x as usize * CHUNK_HEIGHT + world_y as usize) * Self::DEPTH + z as usize;
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
    mutated: bool,
    redstone_metadata: Vec<crate::redstone::RedstoneComponentMetadata>,
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

impl TerrainVertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TerrainVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Uint16x4,
                },
                wgpu::VertexAttribute {
                    offset: 8,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Uint16x2,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Uint16x2,
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
            let Some(section) = self
                .chunk_manager
                .chunks
                .get_mut(&(key.cx, key.cz))
                .and_then(|chunk| chunk.sections.get_mut(key.section_y as usize))
            else {
                continue;
            };
            section.compact_if_worthwhile();
        }
    }
    fn apply_block_changes(&mut self, changes: &[((i32, i32, i32), BlockType)]) {
        let mut dirty_chunks = std::collections::HashSet::new();
        let mut broadcast: Vec<((i32, i32, i32), BlockType)> = Vec::new();
        for &((x, y, z), new_block) in changes {
            let old_block = self.chunk_manager.get_block(x, y, z);
            if old_block == new_block {
                continue;
            }
            if old_block != BlockType::Air {
                self.chunk_manager.set_block(x, y, z, BlockType::Air);
                crate::lighting::update_sky_light_after_removed(
                    &mut self.chunk_manager,
                    x,
                    y,
                    z,
                    &mut dirty_chunks,
                );
                crate::lighting::update_block_light_after_removed(
                    &mut self.chunk_manager,
                    x,
                    y,
                    z,
                    old_block.properties().light_emission,
                    &mut dirty_chunks,
                );
            }
            self.chunk_manager.set_block(x, y, z, new_block);
            crate::lighting::update_sky_light_after_placed(
                &mut self.chunk_manager,
                x,
                y,
                z,
                &mut dirty_chunks,
            );
            crate::lighting::update_block_light_after_placed(
                &mut self.chunk_manager,
                x,
                y,
                z,
                new_block.properties().light_emission,
                &mut dirty_chunks,
            );
            mark_block_mesh_dependencies(&mut dirty_chunks, x, z);
            self.redstone.on_block_changed(
                &self.chunk_manager,
                (x, y, z),
                crate::redstone::Direction::North,
            );
            self.check_and_break_unsupported_above(x, y, z, &mut dirty_chunks);
            broadcast.push(((x, y, z), new_block));
        }
        self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::Block);
        // Fan each authoritative batch mutation out to connected clients.
        for ((x, y, z), block) in broadcast {
            self.broadcast_block_change(x, y, z, block);
        }
    }

    pub fn check_and_break_unsupported_above(
        &mut self,
        wx: i32,
        wy: i32,
        wz: i32,
        dirty_chunks: &mut std::collections::HashSet<(i32, i32)>,
    ) {
        let mut broken_blocks = Vec::new();
        self.chunk_manager.check_and_break_unsupported_above(
            wx,
            wy,
            wz,
            dirty_chunks,
            |(x, y, z), block| {
                broken_blocks.push(((x, y, z), block));
            },
        );
        self.finish_unsupported_breaks(broken_blocks);
    }

    fn check_and_break_unsupported_for_loaded_chunk(
        &mut self,
        cx: i32,
        cz: i32,
        dirty_chunks: &mut std::collections::HashSet<(i32, i32)>,
    ) {
        let mut broken_blocks = Vec::new();
        self.chunk_manager
            .check_and_break_unsupported_for_loaded_chunk(
                cx,
                cz,
                dirty_chunks,
                |position, block| broken_blocks.push((position, block)),
            );
        self.finish_unsupported_breaks(broken_blocks);
    }

    fn finish_unsupported_breaks(&mut self, broken_blocks: Vec<((i32, i32, i32), BlockType)>) {
        for ((x, y, z), block) in broken_blocks {
            if self.game_mode == GameMode::Survival {
                let drop_item = match block {
                    BlockType::TallGrass => {
                        let rng = (x as u32)
                            .wrapping_mul(31)
                            .wrapping_add(y as u32 * 17)
                            .wrapping_add(z as u32);
                        if rng % 8 == 0 {
                            Some(crate::inventory::Item::Seeds)
                        } else {
                            None
                        }
                    }
                    BlockType::SnowLayer => None,
                    _ => Some(crate::inventory::Item::from_block(block)),
                };
                if let Some(item) = drop_item {
                    self.spawn_dropped_item(
                        item,
                        glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5),
                    );
                }
            }
            self.broadcast_block_change(x, y, z, BlockType::Air);
        }
    }

    fn safe_dimension_spawn_y(&mut self, x: i32, z: i32) -> f32 {
        let top = if self.current_dimension == crate::dimension::Dimension::Nether {
            120
        } else {
            180
        };
        for y in (2..=top).rev() {
            if self
                .chunk_manager
                .get_block(x, y - 1, z)
                .properties()
                .is_solid
                && self
                    .chunk_manager
                    .get_block(x, y, z)
                    .properties()
                    .is_passable
                && self
                    .chunk_manager
                    .get_block(x, y + 1, z)
                    .properties()
                    .is_passable
            {
                return y as f32;
            }
        }
        let floor = match self.current_dimension {
            crate::dimension::Dimension::Nether => BlockType::Netherrack,
            crate::dimension::Dimension::End => BlockType::EndStone,
            crate::dimension::Dimension::Overworld => BlockType::Stone,
        };
        self.apply_block_changes(&[
            ((x, 63, z), floor),
            ((x, 64, z), BlockType::Air),
            ((x, 65, z), BlockType::Air),
        ]);
        64.0
    }

    fn build_linked_nether_portal(&mut self, chunk_x: i32, chunk_z: i32, spawn_y: i32) -> Vec3 {
        let base_x = chunk_x * CHUNK_WIDTH as i32 + 6;
        let base_z = chunk_z * CHUNK_DEPTH as i32 + 8;
        let base_y = (spawn_y - 1).clamp(5, 116);
        let mut changes = Vec::new();
        for x in base_x..=base_x + 3 {
            changes.push(((x, base_y, base_z), BlockType::Obsidian));
            changes.push(((x, base_y + 4, base_z), BlockType::Obsidian));
        }
        for y in base_y + 1..=base_y + 3 {
            changes.push(((base_x, y, base_z), BlockType::Obsidian));
            changes.push(((base_x + 3, y, base_z), BlockType::Obsidian));
            changes.push(((base_x + 1, y, base_z), BlockType::NetherPortal));
            changes.push(((base_x + 2, y, base_z), BlockType::NetherPortal));
        }
        self.apply_block_changes(&changes);
        Vec3::new(
            base_x as f32 + 1.5,
            base_y as f32 + 1.0,
            base_z as f32 + 0.5,
        )
    }

    fn switch_dimension(&mut self, target: crate::dimension::Dimension) {
        if target == self.current_dimension {
            return;
        }
        self.player_physics.set_flying(false);
        self.jump_taps.reset();
        let source = self.current_dimension;
        let tracker = self.chunk_manager.dirty_chunks.clone();
        for ((cx, cz), revision) in tracker.dirty_revisions() {
            if let Some(chunk) = self.chunk_manager.chunks.get(&(cx, cz)) {
                let redstone_metadata =
                    self.redstone
                        .collect_chunk_metadata(&self.chunk_manager, cx, cz);
                let snapshot = crate::save::UncompressedChunkSnapshot::from_chunk_with_redstone(
                    source,
                    chunk,
                    redstone_metadata,
                )
                .with_mutation_revision(self.mutation_revisions.latest(source, cx, cz));
                if let Err(error) = self.enqueue_chunk_save(snapshot, tracker.clone(), revision) {
                    eprintln!("[Save] Could not queue dimension-switch chunk: {error}");
                }
            }
        }

        let mut destination =
            crate::dimension::transform_position(source, target, self.player_physics.position);
        if target == crate::dimension::Dimension::End {
            destination = Vec3::new(0.5, 80.0, 0.5);
        } else if source == crate::dimension::Dimension::End {
            destination = Vec3::new(8.5, 80.0, 8.5);
        }

        if let Err(error) = self.save_current_dimension_entities() {
            eprintln!("[Save] Could not save dimension entities: {error}");
        }
        self.current_dimension = target;
        let render_distance = self.chunk_manager.render_distance;
        self.teardown_terrain_runtime("dimension switch");
        self.chunk_manager = ChunkManager::new_in_dimension(render_distance, target);
        self.entity_manager = crate::entity::EntityManager::new();
        self.load_current_dimension_entities();
        self.particles = crate::particles::ParticleSystem::new();
        self.redstone = crate::redstone::RedstoneSystem::new();
        self.redstone_tick_timer = 0.0;
        self.pending_chunk_payloads.clear();
        self.pending_block_changes.clear();
        self.client_chunk_revisions.clear();
        self.mining_target = None;
        self.mining_progress = 0.0;
        self.left_mouse_pressed = false;
        self.water_tick_timer = 0.0;
        self.lava_tick_timer = 0.0;
        self.lava_damage_timer = 0.0;
        self.cactus_damage_timer = 0.0;
        self.audio_manager.stop_looping_sound(RAIN_LOOP_ID);

        let cx = (destination.x / CHUNK_WIDTH as f32).floor() as i32;
        let cz = (destination.z / CHUNK_DEPTH as f32).floor() as i32;
        let mut chunk = crate::dimension::generate_chunk(target, cx, cz, self.world_seed);
        let mut restored_redstone = Vec::new();
        let saved_chunk = self
            .save_manager
            .lock()
            .unwrap()
            .load_chunk_in(target, cx, cz);
        if let Some(saved) = saved_chunk {
            let generated_blocks = crate::save::ChunkSaveData::from_chunk(&chunk).blocks;
            if saved.blocks != generated_blocks {
                match self.mutation_revisions.ensure_at_least(target, cx, cz, 1) {
                    Ok(true) => {
                        self.mutation_revision_generation =
                            self.mutation_revision_generation.saturating_add(1);
                        self.mutation_index_dirty = true;
                    }
                    Ok(false) => {}
                    Err(error) => self.report_mutation_revision_error(
                        error,
                        "restoring a mutated destination chunk",
                    ),
                }
            }
            restored_redstone = saved.redstone_metadata();
            saved.restore_to_chunk(&mut chunk);
        }
        self.chunk_manager.chunks.insert((cx, cz), chunk);
        if !restored_redstone.is_empty() {
            self.redstone
                .restore_chunk_metadata(&self.chunk_manager, cx, cz, &restored_redstone);
        }
        let lifetime = self.next_chunk_lifetime();
        self.chunk_lifetimes.insert((cx, cz), lifetime);
        self.chunk_meshes.insert((cx, cz), ChunkMesh::pending());
        let mut dirty = std::collections::HashSet::new();
        crate::lighting::propagate_chunk_lighting(&mut self.chunk_manager, cx, cz, &mut dirty);

        let wx = destination.x.floor() as i32;
        let wz = destination.z.floor() as i32;
        destination.y = self.safe_dimension_spawn_y(wx, wz);
        if matches!(
            target,
            crate::dimension::Dimension::Overworld | crate::dimension::Dimension::Nether
        ) && matches!(
            source,
            crate::dimension::Dimension::Overworld | crate::dimension::Dimension::Nether
        ) {
            destination = self.build_linked_nether_portal(cx, cz, destination.y as i32);
        }
        self.player_physics.position = destination;
        self.prev_player_position = destination;
        self.player_physics.velocity = Vec3::ZERO;
        self.player_physics.on_ground = false;
        self.player_physics.highest_y = destination.y;
        self.camera.position = destination + Vec3::new(0.0, 1.6, 0.0);
        self.portal_contact_time = 0.0;
        self.portal_cooldown = 3.0;
        let _ = self
            .save_manager
            .lock()
            .unwrap()
            .save_current_dimension(target);
        println!("[Dimension] {} -> {}", source.name(), target.name());
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
        if feet == BlockType::EndPortal || body == BlockType::EndPortal {
            let target = if self.current_dimension == crate::dimension::Dimension::End {
                crate::dimension::Dimension::Overworld
            } else {
                crate::dimension::Dimension::End
            };
            self.switch_dimension(target);
            return;
        }
        if feet == BlockType::NetherPortal || body == BlockType::NetherPortal {
            self.portal_contact_time += dt;
            if self.portal_contact_time >= 1.0 {
                let target = if self.current_dimension == crate::dimension::Dimension::Nether {
                    crate::dimension::Dimension::Overworld
                } else {
                    crate::dimension::Dimension::Nether
                };
                self.switch_dimension(target);
            }
        } else {
            self.portal_contact_time = 0.0;
        }
    }

    fn apply_boss_events(&mut self, events: crate::boss::BossEvents) {
        let authoritative = self.is_authoritative();
        for hit in events.player_damage {
            self.take_damage(hit.amount, DamageSource::Mob);
        }
        for effect in events.apply_wither {
            self.wither_effect_timer = self.wither_effect_timer.max(effect.duration);
        }
        for explosion in events.explosions {
            if explosion.break_blocks && authoritative {
                let mut dirty_meshes = std::collections::HashSet::new();
                let removed = crate::mob::explode(
                    explosion.position,
                    explosion.radius,
                    &mut self.chunk_manager,
                    &mut dirty_meshes,
                    &mut self.player_physics,
                    &mut self.player_state,
                    true,
                    GameMode::Creative,
                    0.0,
                );
                self.invalidate_chunk_meshes(dirty_meshes, DependencyReason::Mob);
                for (x, y, z) in removed {
                    self.broadcast_block_change(x, y, z, BlockType::Air);
                }
            }
            self.audio_manager
                .play_sound(crate::audio::SoundId::Explosion);
        }
        for drop in events.drops {
            for _ in 0..drop.count {
                self.spawn_dropped_item(drop.item, drop.position);
            }
        }
        let changes: Vec<_> = events
            .block_placements
            .into_iter()
            .map(|placement| (placement.position, placement.block))
            .collect();
        if authoritative {
            self.apply_block_changes(&changes);
        }
        if events.dragon_completion.is_some() {
            self.player_state.add_experience(120);
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
    pub f: bool,
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
    left_mouse_pressed && game_mode == GameMode::Survival && gameplay_input_allowed
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
    game_mode == GameMode::Creative || hunger > 6.0
}

fn sprint_exhaustion_amount(
    game_mode: GameMode,
    is_sprinting: bool,
    is_moving: bool,
    dt: f32,
) -> f32 {
    if game_mode == GameMode::Survival && is_sprinting && is_moving {
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
}

pub enum NetworkHandle {
    None,
    Host {
        server_to_host: std::sync::mpsc::Receiver<crate::network::server::ServerToHost>,
        host_to_server: std::sync::mpsc::Sender<crate::network::server::HostToServer>,
        thread: Option<std::thread::JoinHandle<()>>,
    },
    Client {
        client_to_game: std::sync::mpsc::Receiver<crate::network::client::ClientToGame>,
        game_to_client: std::sync::mpsc::Sender<crate::network::client::GameToClient>,
        thread: Option<std::thread::JoinHandle<()>>,
    },
}

trait TrackedNetworkSender<T> {
    fn tracked_send(&self, value: T) -> Result<(), std::sync::mpsc::SendError<T>>;
}

impl<T> TrackedNetworkSender<T> for std::sync::mpsc::Sender<T> {
    fn tracked_send(&self, value: T) -> Result<(), std::sync::mpsc::SendError<T>> {
        crate::perf::tracked_send(
            self,
            value,
            std::mem::size_of::<T>() as u64,
            &crate::perf::queue_stats(crate::perf::QueueCategory::Outbound),
        )
    }
}

/// Explicit lifecycle for asynchronous GPU timestamp readback.  Mapping is
/// only entered after a submission and a device poll; the range is read only
/// in Mapped and is consumed exactly once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuTimestampReadbackState {
    Unsupported,
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
pub const fn gpu_timestamp_capability(
    timestamp_query: bool,
    inside_passes: bool,
) -> GpuTimestampReadbackState {
    if timestamp_query {
        if inside_passes {
            GpuTimestampReadbackState::Unmapped
        } else {
            GpuTimestampReadbackState::Unsupported
        }
    } else {
        GpuTimestampReadbackState::Unsupported
    }
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
        assert_eq!(S::Unsupported.map_requested(), S::Unsupported);
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
        assert_eq!(
            super::gpu_timestamp_capability(false, false),
            S::Unsupported
        );
        assert_eq!(super::gpu_timestamp_capability(true, false), S::Unsupported);
        assert_eq!(super::gpu_timestamp_capability(true, true), S::Unmapped);
    }
}

#[derive(Debug, Clone, Copy)]
struct PlayerSnapshot {
    position: Vec3,
    yaw: f32,
    pitch: f32,
    time: f64,
    sequence: u32,
    sender_time_millis: u64,
}

#[derive(Debug, Clone)]
struct RemotePlayerState {
    entity_id: u64,
    snapshots: std::collections::VecDeque<PlayerSnapshot>,
    username: String,
}

#[derive(Debug, Clone, Copy)]
struct EntitySnapshot {
    state: crate::network::protocol::EntityStateWire,
    time: f64,
    sequence: u64,
}

#[derive(Debug)]
struct ReplicatedEntityState {
    local_entity_id: u64,
    snapshots: std::collections::VecDeque<EntitySnapshot>,
}

impl ReplicatedEntityState {
    fn new(local_entity_id: u64) -> Self {
        Self {
            local_entity_id,
            snapshots: std::collections::VecDeque::with_capacity(ENTITY_SNAPSHOT_CAPACITY),
        }
    }

    fn push(
        &mut self,
        state: crate::network::protocol::EntityStateWire,
        sequence: u64,
        arrival_time: f64,
    ) -> bool {
        if !state.position.iter().all(|value| value.is_finite())
            || !state.velocity.iter().all(|value| value.is_finite())
            || !state.yaw.is_finite()
            || !state.pitch.is_finite()
            || !state.health.is_finite()
        {
            return false;
        }
        if self
            .snapshots
            .back()
            .is_some_and(|latest| sequence <= latest.sequence)
        {
            return false;
        }
        let position = Vec3::from_array(state.position);
        let should_snap = self.snapshots.back().is_some_and(|latest| {
            position.distance(Vec3::from_array(latest.state.position)) > ENTITY_SNAP_DISTANCE
        });
        if should_snap {
            self.snapshots.clear();
        } else if self.snapshots.len() == ENTITY_SNAPSHOT_CAPACITY {
            self.snapshots.pop_front();
        }
        self.snapshots.push_back(EntitySnapshot {
            state,
            time: arrival_time,
            sequence,
        });
        should_snap
    }

    fn sample(&self, target_time: f64) -> Option<crate::network::protocol::EntityStateWire> {
        let first = self.snapshots.front().copied()?;
        if self.snapshots.len() == 1 || target_time <= first.time {
            return Some(first.state);
        }
        for index in 1..self.snapshots.len() {
            let next = self.snapshots[index];
            if target_time <= next.time {
                let prev = self.snapshots[index - 1];
                let span = (next.time - prev.time).max(f64::EPSILON);
                let t = ((target_time - prev.time) / span).clamp(0.0, 1.0) as f32;
                let mut state = next.state;
                state.position = Vec3::from_array(prev.state.position)
                    .lerp(Vec3::from_array(next.state.position), t)
                    .to_array();
                state.velocity = Vec3::from_array(prev.state.velocity)
                    .lerp(Vec3::from_array(next.state.velocity), t)
                    .to_array();
                state.yaw = prev.state.yaw
                    + ((next.state.yaw - prev.state.yaw + std::f32::consts::PI)
                        .rem_euclid(std::f32::consts::TAU)
                        - std::f32::consts::PI)
                        * t;
                state.pitch = prev.state.pitch + (next.state.pitch - prev.state.pitch) * t;
                state.health = prev.state.health + (next.state.health - prev.state.health) * t;
                return Some(state);
            }
        }
        self.snapshots.back().map(|snapshot| snapshot.state)
    }
}

fn entity_animation_state(entity: &crate::entity::Entity) -> u8 {
    u8::from(entity.on_ground)
        | (u8::from(entity.target_player) << 1)
        | (u8::from(entity.is_ignited) << 2)
        | (u8::from(entity.fire_aspect_timer > 0.0) << 3)
}

fn entity_state_wire(entity: &crate::entity::Entity) -> crate::network::protocol::EntityStateWire {
    crate::network::protocol::EntityStateWire {
        entity_id: entity.id,
        entity_type: entity.entity_type.to_wire(),
        position: entity.position.to_array(),
        velocity: entity.velocity.to_array(),
        yaw: entity.yaw,
        pitch: entity.pitch,
        health: entity.health,
        animation_state: entity_animation_state(entity),
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
}

fn is_replicated_entity_type(entity_type: crate::entity::EntityType) -> bool {
    entity_type.is_living()
        || entity_type.is_projectile()
        || entity_type == crate::entity::EntityType::EndCrystal
}

fn effect_to_wire(
    effect: crate::brewing::PotionEffect,
) -> crate::network::protocol::PlayerEffectWire {
    use crate::brewing::PotionEffect;
    let (kind, level) = match effect {
        PotionEffect::Speed { level, .. } => (0, level),
        PotionEffect::Strength { level, .. } => (1, level),
        PotionEffect::Healing { level } => (2, level),
        PotionEffect::Regeneration { level, .. } => (3, level),
        PotionEffect::NightVision { .. } => (4, 1),
        PotionEffect::Invisibility { .. } => (5, 1),
        PotionEffect::FireResistance { .. } => (6, 1),
        PotionEffect::WaterBreathing { .. } => (7, 1),
        PotionEffect::Poison { level, .. } => (8, level),
        PotionEffect::Slowness { level, .. } => (9, level),
    };
    crate::network::protocol::PlayerEffectWire {
        kind,
        level,
        remaining_seconds: effect.remaining(),
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapshotPushResult {
    Accepted,
    Snapped,
    Rejected,
}

impl RemotePlayerState {
    fn new(entity_id: u64, username: String) -> Self {
        Self {
            entity_id,
            snapshots: std::collections::VecDeque::with_capacity(REMOTE_SNAPSHOT_CAPACITY),
            username,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_snapshot(
        &mut self,
        position: Vec3,
        yaw: f32,
        pitch: f32,
        sequence: u32,
        sender_time_millis: u64,
        arrival_time: f64,
    ) -> SnapshotPushResult {
        if !position.is_finite()
            || !yaw.is_finite()
            || !pitch.is_finite()
            || !arrival_time.is_finite()
        {
            return SnapshotPushResult::Rejected;
        }

        let Some(latest) = self.snapshots.back().copied() else {
            self.snapshots.push_back(PlayerSnapshot {
                position,
                yaw,
                pitch,
                time: arrival_time,
                sequence,
                sender_time_millis,
            });
            return SnapshotPushResult::Snapped;
        };

        if !sequence_is_newer(sequence, latest.sequence)
            || sender_time_millis <= latest.sender_time_millis
        {
            return SnapshotPushResult::Rejected;
        }

        let sender_delta = (sender_time_millis - latest.sender_time_millis) as f64 / 1000.0;
        let should_snap = sender_delta > REMOTE_TELEPORT_GAP
            || position.distance(latest.position) > REMOTE_TELEPORT_DISTANCE;
        let local_time = if should_snap {
            arrival_time
        } else {
            latest.time + sender_delta
        };

        if should_snap {
            self.snapshots.clear();
        } else if self.snapshots.len() == REMOTE_SNAPSHOT_CAPACITY {
            self.snapshots.pop_front();
        }
        self.snapshots.push_back(PlayerSnapshot {
            position,
            yaw,
            pitch,
            time: local_time,
            sequence,
            sender_time_millis,
        });

        if should_snap {
            SnapshotPushResult::Snapped
        } else {
            SnapshotPushResult::Accepted
        }
    }

    fn sample(&self, target_time: f64) -> Option<PlayerSnapshot> {
        sample_snapshot_buffer(&self.snapshots, target_time)
    }
}

fn placement_decision_for_players<'a>(
    block: BlockType,
    block_pos: (i32, i32, i32),
    local_player_aabb: AABB,
    remote_players: impl IntoIterator<Item = &'a RemotePlayerState>,
) -> BlockPlacementDecision {
    if !block.properties().is_solid {
        return BlockPlacementDecision::Allowed;
    }

    let mut player_aabbs = vec![local_player_aabb];
    for remote in remote_players {
        let Some(latest) = remote.snapshots.back() else {
            // Until the host has an authenticated pose for every connected
            // player, conservatively reject solid placement rather than risk
            // creating a block inside an unknown player.
            return BlockPlacementDecision::BlockedByPlayer;
        };
        player_aabbs.push(player_aabb_at(latest.position));
    }

    block_placement_decision(block, 0, block_pos, player_aabbs)
}

fn sequence_is_newer(candidate: u32, previous: u32) -> bool {
    let distance = candidate.wrapping_sub(previous);
    distance != 0 && distance < (1 << 31)
}

fn interpolate_snapshot(
    prev: PlayerSnapshot,
    latest: PlayerSnapshot,
    target_time: f64,
) -> PlayerSnapshot {
    let span = (latest.time - prev.time).max(f64::EPSILON);
    let t = ((target_time - prev.time) / span).clamp(0.0, 1.0) as f32;
    PlayerSnapshot {
        position: prev.position.lerp(latest.position, t),
        yaw: prev.yaw
            + ((latest.yaw - prev.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI)
                * t,
        pitch: prev.pitch + (latest.pitch - prev.pitch) * t,
        time: target_time,
        sequence: latest.sequence,
        sender_time_millis: latest.sender_time_millis,
    }
}

fn sample_snapshot_buffer(
    snapshots: &std::collections::VecDeque<PlayerSnapshot>,
    target_time: f64,
) -> Option<PlayerSnapshot> {
    let first = snapshots.front().copied()?;
    if snapshots.len() == 1 || target_time <= first.time {
        return Some(PlayerSnapshot {
            time: target_time,
            ..first
        });
    }

    for index in 1..snapshots.len() {
        let next = snapshots[index];
        if target_time <= next.time {
            return Some(interpolate_snapshot(
                snapshots[index - 1],
                next,
                target_time,
            ));
        }
    }

    let latest = snapshots.back().copied().unwrap();
    let previous = snapshots[snapshots.len() - 2];
    let span = latest.time - previous.time;
    if span <= f64::EPSILON {
        return Some(PlayerSnapshot {
            time: target_time,
            ..latest
        });
    }

    let extrapolation = (target_time - latest.time).clamp(0.0, REMOTE_MAX_EXTRAPOLATION);
    let mut velocity = (latest.position - previous.position) / span as f32;
    let speed = velocity.length();
    if speed > REMOTE_MAX_EXTRAPOLATION_SPEED {
        velocity *= REMOTE_MAX_EXTRAPOLATION_SPEED / speed;
    }
    let yaw_delta = (latest.yaw - previous.yaw + std::f32::consts::PI)
        .rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    let yaw_rate =
        (yaw_delta / span as f32).clamp(-REMOTE_MAX_ANGULAR_SPEED, REMOTE_MAX_ANGULAR_SPEED);
    let pitch_rate = ((latest.pitch - previous.pitch) / span as f32)
        .clamp(-REMOTE_MAX_ANGULAR_SPEED, REMOTE_MAX_ANGULAR_SPEED);

    Some(PlayerSnapshot {
        position: latest.position + velocity * extrapolation as f32,
        yaw: latest.yaw + yaw_rate * extrapolation as f32,
        pitch: (latest.pitch + pitch_rate * extrapolation as f32)
            .clamp(-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2),
        time: target_time,
        ..latest
    })
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatchupStatus {
    Pending,
    WorkerInFlight,
    ServerSubmission { since: Instant },
    AwaitingAck { since: Instant },
}

#[derive(Debug)]
struct PlayerCatchupEntry {
    key: crate::save::NetworkSnapshotKey,
    status: CatchupStatus,
    retries: u8,
}

enum NetworkInbound {
    Connected {
        player_id: crate::network::protocol::PlayerId,
        seed: u64,
        gamemode: u8,
    },
    Disconnected(String),
    PlayerJoin {
        id: crate::network::protocol::PlayerId,
        username: String,
    },
    PlayerLeave(crate::network::protocol::PlayerId),
    PlayerPosition {
        id: crate::network::protocol::PlayerId,
        sequence: u32,
        sender_time_millis: u64,
        x: f32,
        y: f32,
        z: f32,
        yaw: f32,
        pitch: f32,
    },
    PlayerAction {
        id: crate::network::protocol::PlayerId,
        action: crate::network::protocol::Action,
    },
    ClientBlockChange {
        id: crate::network::protocol::PlayerId,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        state: u8,
    },
    ClientBlockAction {
        id: crate::network::protocol::PlayerId,
        action: crate::network::protocol::Action,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        held_item: Option<crate::network::protocol::ItemWire>,
    },
    BlockActionResult {
        x: i32,
        y: i32,
        z: i32,
        success: bool,
        consumed_item: bool,
        drops: Vec<crate::network::protocol::ItemWire>,
    },
    AuthoritativeBlockChange {
        dimension: u8,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        state: u8,
    },
    ChunkData {
        dimension: u8,
        cx: i32,
        cz: i32,
        revision: u64,
        blocks: Vec<u8>,
        block_states: Vec<u8>,
    },
    EntitySpawn {
        dimension: u8,
        sequence: u64,
        state: crate::network::protocol::EntityStateWire,
    },
    EntityState {
        dimension: u8,
        sequence: u64,
        state: crate::network::protocol::EntityStateWire,
    },
    EntityDespawn {
        dimension: u8,
        sequence: u64,
        entity_id: u64,
    },
    PlayerHealth {
        sequence: u64,
        player_id: crate::network::protocol::PlayerId,
        health: f32,
        max_health: f32,
        hunger: f32,
        saturation: f32,
        oxygen: f32,
        is_dead: bool,
        death_reason: u8,
    },
    PlayerEffect {
        sequence: u64,
        player_id: crate::network::protocol::PlayerId,
        effects: Vec<crate::network::protocol::PlayerEffectWire>,
    },
    TimeSync {
        ticks: u64,
        weather: u8,
        weather_remaining_ticks: f32,
    },
    LightningStrike(crate::network::protocol::LightningStrike),
    ChatFromClient {
        id: crate::network::protocol::PlayerId,
        message: String,
    },
    CatchupAccepted {
        id: crate::network::protocol::PlayerId,
        dimension: u8,
        cx: i32,
        cz: i32,
        revision: u64,
    },
    CatchupBackpressured {
        id: crate::network::protocol::PlayerId,
        dimension: u8,
        cx: i32,
        cz: i32,
        revision: u64,
        mailbox_full_count: u64,
    },
    CatchupAck {
        id: crate::network::protocol::PlayerId,
        dimension: u8,
        cx: i32,
        cz: i32,
        revision: u64,
    },
    Chat {
        sender: String,
        message: String,
    },
    StatusUpdate(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NetworkDeliveryClass {
    Reliable,
    LatestPosition,
    LatestEntity,
    LatestHealth,
    LatestEffect,
    LatestTimeSync,
}

fn classify_network_event(event: &NetworkInbound) -> NetworkDeliveryClass {
    match event {
        NetworkInbound::PlayerPosition { .. } => NetworkDeliveryClass::LatestPosition,
        NetworkInbound::EntityState { .. } => NetworkDeliveryClass::LatestEntity,
        NetworkInbound::PlayerHealth { .. } => NetworkDeliveryClass::LatestHealth,
        NetworkInbound::PlayerEffect { .. } => NetworkDeliveryClass::LatestEffect,
        NetworkInbound::TimeSync { .. } => NetworkDeliveryClass::LatestTimeSync,
        _ => NetworkDeliveryClass::Reliable,
    }
}

impl NetworkInbound {
    fn estimated_bytes(&self) -> usize {
        let inline = std::mem::size_of_val(self);
        let heap = match self {
            Self::Disconnected(reason) | Self::StatusUpdate(reason) => reason.len(),
            Self::PlayerJoin { username, .. } => username.len(),
            Self::ChunkData {
                blocks,
                block_states,
                ..
            } => blocks.len().saturating_add(block_states.len()),
            Self::PlayerEffect { effects, .. } => {
                effects.len() * std::mem::size_of::<crate::network::protocol::PlayerEffectWire>()
            }
            Self::BlockActionResult { drops, .. } => {
                drops.len() * std::mem::size_of::<crate::network::protocol::ItemWire>()
            }
            Self::ChatFromClient { message, .. } => message.len(),
            Self::Chat { sender, message } => sender.len().saturating_add(message.len()),
            _ => 0,
        };
        inline.saturating_add(heap)
    }
}

#[derive(Default)]
struct NetworkStaging {
    reliable: std::collections::VecDeque<NetworkInbound>,
    latest_positions: std::collections::HashMap<crate::network::protocol::PlayerId, NetworkInbound>,
    latest_entities: std::collections::HashMap<(u8, u64), NetworkInbound>,
    latest_health: std::collections::HashMap<crate::network::protocol::PlayerId, NetworkInbound>,
    latest_effects: std::collections::HashMap<crate::network::protocol::PlayerId, NetworkInbound>,
    latest_time_sync: Option<NetworkInbound>,
}

impl NetworkStaging {
    fn stage(&mut self, event: NetworkInbound) {
        match classify_network_event(&event) {
            NetworkDeliveryClass::Reliable => self.reliable.push_back(event),
            NetworkDeliveryClass::LatestPosition => {
                let NetworkInbound::PlayerPosition { id, sequence, .. } = &event else {
                    unreachable!("position delivery class must contain a position event");
                };
                let replace = self.latest_positions.get(id).map_or(true, |previous| {
                    matches!(
                        previous,
                        NetworkInbound::PlayerPosition {
                            sequence: old_sequence,
                            ..
                        } if sequence_is_newer(*sequence, *old_sequence)
                    )
                });
                if replace {
                    self.latest_positions.insert(*id, event);
                }
            }
            NetworkDeliveryClass::LatestEntity => {
                let NetworkInbound::EntityState {
                    dimension,
                    sequence,
                    state,
                } = &event
                else {
                    unreachable!("entity delivery class must contain an entity-state event");
                };
                let key = (*dimension, state.entity_id);
                let replace = self.latest_entities.get(&key).map_or(true, |previous| {
                    matches!(
                        previous,
                        NetworkInbound::EntityState {
                            sequence: old_sequence,
                            ..
                        } if sequence > old_sequence
                    )
                });
                if replace {
                    self.latest_entities.insert(key, event);
                }
            }
            NetworkDeliveryClass::LatestHealth => {
                let NetworkInbound::PlayerHealth {
                    player_id,
                    sequence,
                    ..
                } = &event
                else {
                    unreachable!("health delivery class must contain a health event");
                };
                let replace = self.latest_health.get(player_id).map_or(true, |previous| {
                    matches!(
                        previous,
                        NetworkInbound::PlayerHealth {
                            sequence: old_sequence,
                            ..
                        } if sequence > old_sequence
                    )
                });
                if replace {
                    self.latest_health.insert(*player_id, event);
                }
            }
            NetworkDeliveryClass::LatestEffect => {
                let NetworkInbound::PlayerEffect {
                    player_id,
                    sequence,
                    ..
                } = &event
                else {
                    unreachable!("effect delivery class must contain an effect event");
                };
                let replace = self.latest_effects.get(player_id).map_or(true, |previous| {
                    matches!(
                        previous,
                        NetworkInbound::PlayerEffect {
                            sequence: old_sequence,
                            ..
                        } if sequence > old_sequence
                    )
                });
                if replace {
                    self.latest_effects.insert(*player_id, event);
                }
            }
            NetworkDeliveryClass::LatestTimeSync => {
                let NetworkInbound::TimeSync { ticks, .. } = &event else {
                    unreachable!("time-sync delivery class must contain a time-sync event");
                };
                let replace = self.latest_time_sync.as_ref().map_or(true, |previous| {
                    matches!(
                        previous,
                        NetworkInbound::TimeSync {
                            ticks: old_ticks,
                            ..
                        } if ticks > old_ticks
                    )
                });
                if replace {
                    self.latest_time_sync = Some(event);
                }
            }
        }
    }

    fn take_smallest_if_fits<K>(
        map: &mut std::collections::HashMap<K, NetworkInbound>,
        remaining_bytes: usize,
    ) -> Option<(NetworkInbound, usize)>
    where
        K: Copy + Ord + std::hash::Hash + Eq,
    {
        let key = map.keys().min().copied()?;
        let event_bytes = map.get(&key)?.estimated_bytes();
        if event_bytes > remaining_bytes {
            return None;
        }
        map.remove(&key).map(|event| (event, event_bytes))
    }

    /// Remove one event only when its full estimated footprint fits. Reliable
    /// events are considered first and never skipped, preserving strict FIFO.
    fn pop_next_if_fits(&mut self, remaining_bytes: usize) -> Option<(NetworkInbound, usize)> {
        if let Some(event) = self.reliable.front() {
            let event_bytes = event.estimated_bytes();
            if event_bytes > remaining_bytes {
                return None;
            }
            return self.reliable.pop_front().map(|event| (event, event_bytes));
        }

        if !self.latest_positions.is_empty() {
            return Self::take_smallest_if_fits(&mut self.latest_positions, remaining_bytes);
        }
        if !self.latest_entities.is_empty() {
            return Self::take_smallest_if_fits(&mut self.latest_entities, remaining_bytes);
        }
        if !self.latest_health.is_empty() {
            return Self::take_smallest_if_fits(&mut self.latest_health, remaining_bytes);
        }
        if !self.latest_effects.is_empty() {
            return Self::take_smallest_if_fits(&mut self.latest_effects, remaining_bytes);
        }
        let event_bytes = self.latest_time_sync.as_ref()?.estimated_bytes();
        if event_bytes > remaining_bytes {
            return None;
        }
        self.latest_time_sync
            .take()
            .map(|event| (event, event_bytes))
    }

    fn reliable_len(&self) -> usize {
        self.reliable.len()
    }

    fn latest_len(&self) -> usize {
        self.latest_positions.len()
            + self.latest_entities.len()
            + self.latest_health.len()
            + self.latest_effects.len()
            + usize::from(self.latest_time_sync.is_some())
    }

    fn len(&self) -> usize {
        self.reliable_len() + self.latest_len()
    }

    fn reliable_bytes(&self) -> u64 {
        self.reliable
            .iter()
            .map(|event| event.estimated_bytes() as u64)
            .sum()
    }

    fn latest_bytes(&self) -> u64 {
        self.latest_positions
            .values()
            .chain(self.latest_entities.values())
            .chain(self.latest_health.values())
            .chain(self.latest_effects.values())
            .chain(self.latest_time_sync.iter())
            .map(|event| event.estimated_bytes() as u64)
            .sum()
    }
}

impl NetworkHandle {
    fn drain_inbound(&self) -> Vec<NetworkInbound> {
        const MAX_EVENTS: usize = 256;
        let started = Instant::now();
        match self {
            NetworkHandle::None => Vec::new(),
            NetworkHandle::Host { server_to_host, .. } => {
                let mut raw = Vec::with_capacity(MAX_EVENTS);
                while raw.len() < MAX_EVENTS && started.elapsed() < Duration::from_millis(2) {
                    match crate::perf::tracked_try_recv(
                        server_to_host,
                        std::mem::size_of::<crate::network::server::ServerToHost>() as u64,
                        &crate::perf::queue_stats(crate::perf::QueueCategory::Inbound),
                    ) {
                        Ok(event) => raw.push(event),
                        Err(_) => break,
                    }
                }
                raw.into_iter()
                    .into_iter()
                    .map(|event| match event {
                        crate::network::server::ServerToHost::Disconnected { reason } => {
                            NetworkInbound::Disconnected(reason)
                        }
                        crate::network::server::ServerToHost::ClientJoined { id, username } => {
                            NetworkInbound::PlayerJoin { id, username }
                        }
                        crate::network::server::ServerToHost::ClientLeft { id } => {
                            NetworkInbound::PlayerLeave(id)
                        }
                        crate::network::server::ServerToHost::ClientPosition {
                            id,
                            sequence,
                            sender_time_millis,
                            x,
                            y,
                            z,
                            yaw,
                            pitch,
                        } => NetworkInbound::PlayerPosition {
                            id,
                            sequence,
                            sender_time_millis,
                            x,
                            y,
                            z,
                            yaw,
                            pitch,
                        },
                        crate::network::server::ServerToHost::ClientAction { id, action } => {
                            NetworkInbound::PlayerAction { id, action }
                        }
                        crate::network::server::ServerToHost::ClientBlockChange {
                            id,
                            x,
                            y,
                            z,
                            block,
                            state,
                        } => NetworkInbound::ClientBlockChange {
                            id,
                            x,
                            y,
                            z,
                            block,
                            state,
                        },
                        crate::network::server::ServerToHost::ClientBlockAction {
                            id,
                            action,
                            x,
                            y,
                            z,
                            block,
                            held_item,
                        } => NetworkInbound::ClientBlockAction {
                            id,
                            action,
                            x,
                            y,
                            z,
                            block,
                            held_item,
                        },
                        crate::network::server::ServerToHost::ChatFromClient { id, message } => {
                            NetworkInbound::ChatFromClient { id, message }
                        }
                        crate::network::server::ServerToHost::CatchupAccepted {
                            id,
                            dimension,
                            cx,
                            cz,
                            revision,
                        } => NetworkInbound::CatchupAccepted {
                            id,
                            dimension,
                            cx,
                            cz,
                            revision,
                        },
                        crate::network::server::ServerToHost::CatchupBackpressured {
                            id,
                            dimension,
                            cx,
                            cz,
                            revision,
                            mailbox_full_count,
                        } => NetworkInbound::CatchupBackpressured {
                            id,
                            dimension,
                            cx,
                            cz,
                            revision,
                            mailbox_full_count,
                        },
                        crate::network::server::ServerToHost::CatchupAck {
                            id,
                            dimension,
                            cx,
                            cz,
                            revision,
                        } => NetworkInbound::CatchupAck {
                            id,
                            dimension,
                            cx,
                            cz,
                            revision,
                        },
                    })
                    .collect()
            }
            NetworkHandle::Client { client_to_game, .. } => {
                let mut raw = Vec::with_capacity(MAX_EVENTS);
                while raw.len() < MAX_EVENTS && started.elapsed() < Duration::from_millis(2) {
                    match crate::perf::tracked_try_recv(
                        client_to_game,
                        std::mem::size_of::<crate::network::client::ClientToGame>() as u64,
                        &crate::perf::queue_stats(crate::perf::QueueCategory::Inbound),
                    ) {
                        Ok(event) => raw.push(event),
                        Err(_) => break,
                    }
                }
                raw.into_iter()
                    .map(|event| match event {
                        crate::network::client::ClientToGame::Connected {
                            player_id,
                            seed,
                            gamemode,
                        } => NetworkInbound::Connected {
                            player_id,
                            seed,
                            gamemode,
                        },
                        crate::network::client::ClientToGame::Disconnected { reason } => {
                            NetworkInbound::Disconnected(reason)
                        }
                        crate::network::client::ClientToGame::PlayerJoin { id, username } => {
                            NetworkInbound::PlayerJoin { id, username }
                        }
                        crate::network::client::ClientToGame::PlayerLeave { id } => {
                            NetworkInbound::PlayerLeave(id)
                        }
                        crate::network::client::ClientToGame::PlayerPosition {
                            id,
                            sequence,
                            sender_time_millis,
                            x,
                            y,
                            z,
                            yaw,
                            pitch,
                        } => NetworkInbound::PlayerPosition {
                            id,
                            sequence,
                            sender_time_millis,
                            x,
                            y,
                            z,
                            yaw,
                            pitch,
                        },
                        crate::network::client::ClientToGame::PlayerAction { id, action } => {
                            NetworkInbound::PlayerAction { id, action }
                        }
                        crate::network::client::ClientToGame::BlockChange {
                            dimension,
                            revision,
                            x,
                            y,
                            z,
                            block,
                            state,
                        } => NetworkInbound::AuthoritativeBlockChange {
                            dimension,
                            revision,
                            x,
                            y,
                            z,
                            block,
                            state,
                        },
                        crate::network::client::ClientToGame::BlockActionResult {
                            x,
                            y,
                            z,
                            success,
                            consumed_item,
                            drops,
                        } => NetworkInbound::BlockActionResult {
                            x,
                            y,
                            z,
                            success,
                            consumed_item,
                            drops,
                        },
                        crate::network::client::ClientToGame::ChunkData {
                            dimension,
                            cx,
                            cz,
                            revision,
                            blocks,
                            block_states,
                        } => NetworkInbound::ChunkData {
                            dimension,
                            cx,
                            cz,
                            revision,
                            blocks,
                            block_states,
                        },
                        crate::network::client::ClientToGame::EntitySpawn {
                            dimension,
                            sequence,
                            state,
                        } => NetworkInbound::EntitySpawn {
                            dimension,
                            sequence,
                            state,
                        },
                        crate::network::client::ClientToGame::EntityState {
                            dimension,
                            sequence,
                            state,
                        } => NetworkInbound::EntityState {
                            dimension,
                            sequence,
                            state,
                        },
                        crate::network::client::ClientToGame::EntityDespawn {
                            dimension,
                            sequence,
                            entity_id,
                        } => NetworkInbound::EntityDespawn {
                            dimension,
                            sequence,
                            entity_id,
                        },
                        crate::network::client::ClientToGame::PlayerHealth {
                            sequence,
                            player_id,
                            health,
                            max_health,
                            hunger,
                            saturation,
                            oxygen,
                            is_dead,
                            death_reason,
                        } => NetworkInbound::PlayerHealth {
                            sequence,
                            player_id,
                            health,
                            max_health,
                            hunger,
                            saturation,
                            oxygen,
                            is_dead,
                            death_reason,
                        },
                        crate::network::client::ClientToGame::PlayerEffect {
                            sequence,
                            player_id,
                            effects,
                        } => NetworkInbound::PlayerEffect {
                            sequence,
                            player_id,
                            effects,
                        },
                        crate::network::client::ClientToGame::TimeSync {
                            ticks,
                            weather,
                            weather_remaining_ticks,
                        } => NetworkInbound::TimeSync {
                            ticks,
                            weather,
                            weather_remaining_ticks,
                        },
                        crate::network::client::ClientToGame::LightningStrike(strike) => {
                            NetworkInbound::LightningStrike(strike)
                        }
                        crate::network::client::ClientToGame::Chat { sender, message } => {
                            NetworkInbound::Chat { sender, message }
                        }
                        crate::network::client::ClientToGame::StatusUpdate { message } => {
                            NetworkInbound::StatusUpdate(message)
                        }
                    })
                    .collect()
            }
        }
    }

    fn send_position(
        &self,
        sequence: u32,
        sender_time_millis: u64,
        position: Vec3,
        yaw: f32,
        pitch: f32,
    ) {
        match self {
            NetworkHandle::Host { host_to_server, .. } => {
                let _ = crate::perf::tracked_send(
                    host_to_server,
                    crate::network::server::HostToServer::BroadcastPlayerPosition {
                        id: 0,
                        sequence,
                        sender_time_millis,
                        x: position.x,
                        y: position.y,
                        z: position.z,
                        yaw,
                        pitch,
                    },
                    std::mem::size_of::<crate::network::server::HostToServer>() as u64,
                    &crate::perf::queue_stats(crate::perf::QueueCategory::Outbound),
                );
            }
            NetworkHandle::Client { game_to_client, .. } => {
                let _ = crate::perf::tracked_send(
                    game_to_client,
                    crate::network::client::GameToClient::SendPosition {
                        sequence,
                        sender_time_millis,
                        x: position.x,
                        y: position.y,
                        z: position.z,
                        yaw,
                        pitch,
                    },
                    std::mem::size_of::<crate::network::client::GameToClient>() as u64,
                    &crate::perf::queue_stats(crate::perf::QueueCategory::Outbound),
                );
            }
            NetworkHandle::None => {}
        }
    }

    fn broadcast_player_position(
        &self,
        id: crate::network::protocol::PlayerId,
        sequence: u32,
        sender_time_millis: u64,
        position: Vec3,
        yaw: f32,
        pitch: f32,
    ) {
        if let NetworkHandle::Host { host_to_server, .. } = self {
            let _ = crate::perf::tracked_send(
                host_to_server,
                crate::network::server::HostToServer::BroadcastPlayerPosition {
                    id,
                    sequence,
                    sender_time_millis,
                    x: position.x,
                    y: position.y,
                    z: position.z,
                    yaw,
                    pitch,
                },
                std::mem::size_of::<crate::network::server::HostToServer>() as u64,
                &crate::perf::queue_stats(crate::perf::QueueCategory::Outbound),
            );
        }
    }

    fn request_block_change(&self, x: i32, y: i32, z: i32, block: u32) {
        if let NetworkHandle::Client { game_to_client, .. } = self {
            let _ = crate::perf::tracked_send(
                game_to_client,
                crate::network::client::GameToClient::RequestBlockChange { x, y, z, block },
                std::mem::size_of::<crate::network::client::GameToClient>() as u64,
                &crate::perf::queue_stats(crate::perf::QueueCategory::Outbound),
            );
        }
    }

    fn request_block_action(
        &self,
        action: crate::network::protocol::Action,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        held_item: Option<crate::network::protocol::ItemWire>,
    ) {
        if let NetworkHandle::Client { game_to_client, .. } = self {
            let _ = crate::perf::tracked_send(
                game_to_client,
                crate::network::client::GameToClient::RequestBlockAction {
                    action,
                    x,
                    y,
                    z,
                    block,
                    held_item,
                },
                std::mem::size_of::<crate::network::client::GameToClient>() as u64,
                &crate::perf::queue_stats(crate::perf::QueueCategory::Outbound),
            );
        }
    }

    /// Host-only: fan a block mutation out to every connected client. The host
    /// applies the mutation locally through the canonical path and then calls
    /// this so peers render the same world state.
    fn broadcast_block_change(
        &self,
        dimension: crate::dimension::Dimension,
        revision: u64,
        x: i32,
        y: i32,
        z: i32,
        block: u32,
        state: u8,
    ) {
        if let NetworkHandle::Host { host_to_server, .. } = self {
            let _ = host_to_server.tracked_send(
                crate::network::server::HostToServer::BroadcastBlockChange {
                    dimension: dimension as u8,
                    revision,
                    x,
                    y,
                    z,
                    block,
                    state,
                },
            );
        }
    }

    fn broadcast_entity_spawn(
        &self,
        dimension: crate::dimension::Dimension,
        sequence: u64,
        state: crate::network::protocol::EntityStateWire,
    ) {
        if let NetworkHandle::Host { host_to_server, .. } = self {
            let _ = host_to_server.tracked_send(
                crate::network::server::HostToServer::BroadcastEntitySpawn {
                    dimension: dimension as u8,
                    sequence,
                    state,
                },
            );
        }
    }

    fn broadcast_entity_state(
        &self,
        dimension: crate::dimension::Dimension,
        sequence: u64,
        state: crate::network::protocol::EntityStateWire,
    ) {
        if let NetworkHandle::Host { host_to_server, .. } = self {
            let _ = host_to_server.tracked_send(
                crate::network::server::HostToServer::BroadcastEntityState {
                    dimension: dimension as u8,
                    sequence,
                    state,
                },
            );
        }
    }

    fn broadcast_entity_despawn(
        &self,
        dimension: crate::dimension::Dimension,
        sequence: u64,
        entity_id: u64,
    ) {
        if let NetworkHandle::Host { host_to_server, .. } = self {
            let _ = host_to_server.tracked_send(
                crate::network::server::HostToServer::BroadcastEntityDespawn {
                    dimension: dimension as u8,
                    sequence,
                    entity_id,
                },
            );
        }
    }

    fn broadcast_player_health(
        &self,
        sequence: u64,
        player_id: crate::network::protocol::PlayerId,
        state: &PlayerState,
    ) {
        if let NetworkHandle::Host { host_to_server, .. } = self {
            let _ = host_to_server.tracked_send(
                crate::network::server::HostToServer::BroadcastPlayerHealth {
                    sequence,
                    player_id,
                    health: state.health,
                    max_health: state.max_health,
                    hunger: state.hunger,
                    saturation: state.saturation,
                    oxygen: state.oxygen,
                    is_dead: state.is_dead,
                    death_reason: state.death_reason.map_or(0, DamageSource::to_wire),
                },
            );
        }
    }

    fn broadcast_player_effects(
        &self,
        sequence: u64,
        player_id: crate::network::protocol::PlayerId,
        effects: Vec<crate::network::protocol::PlayerEffectWire>,
    ) {
        if let NetworkHandle::Host { host_to_server, .. } = self {
            let _ = host_to_server.tracked_send(
                crate::network::server::HostToServer::BroadcastPlayerEffect {
                    sequence,
                    player_id,
                    effects,
                },
            );
        }
    }

    /// Host-only: push a full chunk payload to a specific joining client as
    /// part of mid-game join catch-up.
    fn send_chunk_to(
        &self,
        dimension: crate::dimension::Dimension,
        cx: i32,
        cz: i32,
        revision: u64,
        blocks: Vec<u8>,
        block_states: Vec<u8>,
        to: crate::network::protocol::PlayerId,
    ) {
        if let NetworkHandle::Host { host_to_server, .. } = self {
            let _ = host_to_server.tracked_send(crate::network::server::HostToServer::SendChunk {
                dimension: dimension as u8,
                cx,
                cz,
                revision,
                blocks,
                block_states,
                to,
            });
        }
    }

    fn disconnect_slow_catchup_client(
        &self,
        to: crate::network::protocol::PlayerId,
        reason: String,
    ) {
        if let NetworkHandle::Host { host_to_server, .. } = self {
            let _ = host_to_server
                .send(crate::network::server::HostToServer::DisconnectCatchupClient { to, reason });
        }
    }

    fn broadcast_time_sync(&self, ticks: u64, weather: u8, weather_remaining_ticks: f32) {
        if let NetworkHandle::Host { host_to_server, .. } = self {
            let _ = host_to_server.tracked_send(
                crate::network::server::HostToServer::BroadcastTimeSync {
                    ticks,
                    weather,
                    weather_remaining_ticks,
                },
            );
        }
    }

    fn send_time_sync_to(
        &self,
        ticks: u64,
        weather: u8,
        weather_remaining_ticks: f32,
        to: crate::network::protocol::PlayerId,
    ) {
        if let NetworkHandle::Host { host_to_server, .. } = self {
            let _ =
                host_to_server.tracked_send(crate::network::server::HostToServer::SendTimeSync {
                    ticks,
                    weather,
                    weather_remaining_ticks,
                    to,
                });
        }
    }

    fn broadcast_lightning_strike(&self, strike: crate::network::protocol::LightningStrike) {
        if let NetworkHandle::Host { host_to_server, .. } = self {
            let _ = host_to_server
                .send(crate::network::server::HostToServer::BroadcastLightningStrike { strike });
        }
    }

    fn send_action(&self, action: crate::network::protocol::Action) {
        match self {
            NetworkHandle::Host { host_to_server, .. } => {
                let _ = host_to_server.tracked_send(
                    crate::network::server::HostToServer::BroadcastPlayerAction { id: 0, action },
                );
            }
            NetworkHandle::Client { game_to_client, .. } => {
                let _ = game_to_client
                    .tracked_send(crate::network::client::GameToClient::SendAction { action });
            }
            NetworkHandle::None => {}
        }
    }

    fn send_chat(&self, sender: String, message: String) {
        match self {
            NetworkHandle::Host { host_to_server, .. } => {
                let _ = host_to_server.tracked_send(
                    crate::network::server::HostToServer::BroadcastChat { sender, message },
                );
            }
            NetworkHandle::Client { game_to_client, .. } => {
                let _ = game_to_client
                    .tracked_send(crate::network::client::GameToClient::SendChat { message });
            }
            NetworkHandle::None => {}
        }
    }

    fn notify_player_join(&self, id: crate::network::protocol::PlayerId, username: String) {
        if let NetworkHandle::Host { host_to_server, .. } = self {
            let _ = host_to_server.tracked_send(
                crate::network::server::HostToServer::NotifyPlayerJoin { id, username },
            );
        }
    }

    fn shutdown(&mut self) {
        let thread = match self {
            NetworkHandle::None => None,
            NetworkHandle::Host {
                host_to_server,
                thread,
                ..
            } => {
                let _ = host_to_server.tracked_send(crate::network::server::HostToServer::Stop);
                thread.take()
            }
            NetworkHandle::Client {
                game_to_client,
                thread,
                ..
            } => {
                let _ =
                    game_to_client.tracked_send(crate::network::client::GameToClient::Disconnect);
                thread.take()
            }
        };
        if let Some(thread) = thread {
            let _ = thread.join();
        }
        crate::perf::reset_network_queue_stats();
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
    render_pipeline: wgpu::RenderPipeline,
    trans_pipeline: wgpu::RenderPipeline,
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
    crosshair_pipeline: wgpu::RenderPipeline,
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
    pub mining_target: Option<glam::Vec3>,
    pub mining_progress: f32,
    crack_vertex_buffer: wgpu::Buffer,
    crack_index_buffer: wgpu::Buffer,
    pub player_state: PlayerState,
    pub void_damage_timer: f32,
    pub world_time: crate::camera::WorldTime,
    pub show_debug: bool,
    /// F5 toggles third-person camera. When true the local player model is
    /// rendered and the camera sits behind the player.
    pub third_person: bool,
    pub entity_manager: crate::entity::EntityManager,
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
    mob_vertex_buffer: wgpu::Buffer,
    mob_index_buffer: wgpu::Buffer,
    mob_num_indices: u32,
    hand_pipeline: wgpu::RenderPipeline,
    hand_vertex_buffer: wgpu::Buffer,
    hand_index_buffer: wgpu::Buffer,
    hand_num_indices: u32,
    #[allow(dead_code)] // Owned for bind group lifetime; not read directly.
    hand_camera_buffer: wgpu::Buffer,
    hand_camera_bind_group: wgpu::BindGroup,
    pub particles: crate::particles::ParticleSystem,
    particle_vertex_buffer: wgpu::Buffer,
    particle_index_buffer: wgpu::Buffer,
    particle_num_indices: u32,
    torch_smoke_timer: f32,
    total_time: f32,
    pub audio_manager: crate::audio::AudioManager,
    pub footstep_accumulator: f32,
    pub was_on_ground: bool,
    pub water_tick_timer: f32,
    pub lava_tick_timer: f32,
    pub lava_damage_timer: f32,
    pub cactus_damage_timer: f32,
    pub save_manager: std::sync::Arc<std::sync::Mutex<crate::save::SaveManager>>,
    pub save_tx: crate::save::SaveQueue,
    save_queue_stats: std::sync::Arc<crate::save::SaveQueueStats>,
    pub autosave_timer: f32,
    pub is_saving: bool,
    pub save_error: Option<String>,
    pub is_sprinting: bool,
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
    visible_sections_scratch: std::collections::HashSet<(i32, usize, i32)>,
    section_visibility_scratch: crate::culling::SectionVisibilityScratch,
    mob_vertices_scratch: Vec<Vertex>,
    mob_indices_scratch: Vec<u32>,
    particle_vertices_scratch: Vec<Vertex>,
    particle_indices_scratch: Vec<u32>,
    hand_vertices_scratch: Vec<Vertex>,
    hand_indices_scratch: Vec<u32>,
    last_hand_mesh_key: Option<crate::hand_renderer::HandMeshKey>,
    ui_vertices_scratch: Vec<UiVertex>,
    ui_line_vertices_scratch: Vec<UiVertex>,
    debug_str_scratch: String,
    pub active_station: Option<StationKind>,
    pub enchanting: crate::enchantment::EnchantingState,
    pub brewing: crate::brewing::BrewingStandState,
    pub anvil: crate::enchantment::AnvilState,
    pub potion_effects: crate::brewing::EffectManager,
    pub redstone: crate::redstone::RedstoneSystem,
    redstone_tick_timer: f32,
    pub weather: crate::weather::WeatherSystem,
    pub settings: GameSettings,
    pub world_seed: u32,
    pub difficulty: Difficulty,
    pub current_dimension: crate::dimension::Dimension,
    portal_contact_time: f32,
    portal_cooldown: f32,
    wither_effect_timer: f32,
    wither_damage_timer: f32,
    pub advancement_manager: crate::advancements::AdvancementManager,
    pub advancement_gui: crate::advancements::AdvancementGui,
    pub role: MultiplayerRole,
    pub network: NetworkHandle,
    network_staging: NetworkStaging,
    network_ready: bool,
    local_player_id: Option<crate::network::protocol::PlayerId>,
    remote_players:
        std::collections::HashMap<crate::network::protocol::PlayerId, RemotePlayerState>,
    /// Client-only visual copies of host-owned non-player entities.
    replicated_entities: std::collections::HashMap<u64, ReplicatedEntityState>,
    /// Host-only set used to emit reliable spawn/despawn lifecycle edges.
    replicated_entity_ids: std::collections::HashSet<u64>,
    entity_replication_sequence: u64,
    /// Host-owned survival state for joining players. Clients display only the
    /// replicated entry matching `local_player_id`.
    remote_player_health:
        std::collections::HashMap<crate::network::protocol::PlayerId, PlayerState>,
    remote_player_effects: std::collections::HashMap<
        crate::network::protocol::PlayerId,
        crate::brewing::EffectManager,
    >,
    client_player_health_sequence: u64,
    client_player_effect_sequence: u64,
    pub network_status: Option<String>,
    pub chat_messages: std::collections::VecDeque<(String, String)>,
    pub chat_input: String,
    pub is_chat_open: bool,
    pub connection_lost: bool,
    network_position_timer: f32,
    network_pose_sequence: u32,
    network_time_sync_timer: f32,
    network_time: f64,
    /// Client-only: chunk payloads that arrived from the host before the chunk
    /// was streamed in. Applied when `update_chunks` loads the coordinate.
    pending_chunk_payloads: std::collections::HashMap<(i32, i32), (u64, Vec<u8>, Vec<u8>)>,
    /// Client-only coalesced mutations for chunks that are not streamed in yet.
    /// The latest authoritative value wins for each world-space block.
    pending_block_changes: std::collections::HashMap<
        (i32, i32),
        std::collections::HashMap<(i32, i32, i32), (u64, u32, u8)>,
    >,
    client_chunk_revisions: std::collections::HashMap<(crate::dimension::Dimension, i32, i32), u64>,
    /// Host-only persistent latest revision per mutated chunk. Keeping only
    /// the latest value bounds history while retaining unloaded coordinates.
    mutation_revisions: crate::save::MutationRevisionIndex,
    mutation_revision_generation: u64,
    mutation_index_persist_in_flight: Option<u64>,
    mutation_index_dirty: bool,
    network_snapshot_worker: crate::save::NetworkSnapshotWorker,
    /// Host-only ACK-owned catch-up entries per joining client.
    pending_player_catchups:
        std::collections::HashMap<crate::network::protocol::PlayerId, Vec<PlayerCatchupEntry>>,
    catchup_round_robin_cursor: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotType {
    Creative(Item),
    Hotbar(usize),
    Backpack(usize),
    Armor(usize),
    CraftInput(usize),
    CraftOutput,
    EnchantInput,
    EnchantLapis,
    BrewBottle(usize),
    BrewIngredient,
    AnvilLeft,
    AnvilRight,
    AnvilOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InventoryLayoutKind {
    CreativeCatalog,
    Standard,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct InventoryUiRect {
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
}

impl InventoryUiRect {
    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
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
        let size = window.inner_size();
        // The NVIDIA Vulkan ICD crashes during the menu-to-world transition on
        // this Windows setup. `PRIMARY` still chooses Vulkan first, so force
        // DX12 here to match the menu and keep other platforms unchanged.
        let backends = if cfg!(target_os = "windows") {
            wgpu::Backends::DX12
        } else {
            wgpu::Backends::PRIMARY
        };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let adapter_features = adapter.features();
        let gpu_timestamps_supported = adapter_features.contains(wgpu::Features::TIMESTAMP_QUERY);
        let gpu_timestamps_inside_passes =
            adapter_features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES);
        let mut required_features = wgpu::Features::empty();
        if gpu_timestamps_supported {
            required_features |= wgpu::Features::TIMESTAMP_QUERY;
            if gpu_timestamps_inside_passes {
                required_features |= wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES;
            }
        }

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    required_features,
                    required_limits: wgpu::Limits::default(),
                    label: None,
                },
                None,
            )
            .await
            .unwrap();

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

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: if settings.vsync {
                wgpu::PresentMode::Fifo
            } else if surface_caps
                .present_modes
                .contains(&wgpu::PresentMode::Mailbox)
            {
                wgpu::PresentMode::Mailbox
            } else if surface_caps
                .present_modes
                .contains(&wgpu::PresentMode::Immediate)
            {
                wgpu::PresentMode::Immediate
            } else {
                wgpu::PresentMode::Fifo
            },
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // Setup Depth Buffer
        let depth_view = Self::create_depth_texture(&device, &config);

        // Initialize SaveManager
        let save_manager = std::sync::Arc::new(std::sync::Mutex::new(
            crate::save::SaveManager::new(&launch.world_dir),
        ));
        let current_dimension = if is_client {
            crate::dimension::Dimension::Overworld
        } else {
            save_manager.lock().unwrap().load_current_dimension()
        };

        // The save queue is bounded by unique chunk keys and coalesces newer
        // revisions before the worker sees them.
        let save_tx = crate::save::spawn_save_worker(
            std::sync::Arc::clone(&save_manager),
            crate::save::SAVE_QUEUE_CAPACITY,
        );
        let save_queue_stats = save_tx.stats();
        let network_snapshot_worker = crate::save::spawn_network_snapshot_worker(
            std::sync::Arc::clone(&save_manager),
            crate::save::NETWORK_SNAPSHOT_QUEUE_CAPACITY,
        );
        let mut mutation_revisions = if is_client {
            crate::save::MutationRevisionIndex::default()
        } else {
            save_manager.lock().unwrap().load_mutation_revision_index()
        };
        let mut mutation_index_dirty = false;
        let mut mutation_index_load_error = None;

        // Initialize physics and keyboard input
        let mut player_physics = PlayerPhysics::new(Vec3::new(8.0, 80.0, 8.0));
        let keys = KeyState::default();

        let mut audio_manager = crate::audio::AudioManager::new();
        audio_manager.set_volume(settings.effective_sound_volume());
        audio_manager.set_weather_volume(settings.weather_volume);

        // Load save data if exists
        let mut game_mode = launch.game_mode;
        let mut inventory = match launch.game_mode {
            GameMode::Creative => Inventory::new_creative(),
            GameMode::Survival => Inventory::new(),
        };
        let mut player_state = PlayerState::new();
        let mut camera_yaw = f32::to_radians(90.0);
        let mut camera_pitch = f32::to_radians(-20.0);
        let mut world_time = crate::camera::WorldTime::new();
        let mut world_seed = launch.seed;

        let mut advancement_progress = crate::advancements::AdvancementProgressData::default();
        let has_save = !is_client && {
            let mgr = save_manager.lock().unwrap();
            mgr.load_player_and_level().is_ok()
        };

        if has_save {
            let (level, player) = {
                let mgr = save_manager.lock().unwrap();
                mgr.load_player_and_level().unwrap()
            };
            world_seed = level.seed;
            world_time.ticks = level.time;
            player_physics.position = Vec3::from_slice(&player.position);
            player_physics.velocity = Vec3::from_slice(&player.velocity);
            camera_yaw = player.yaw;
            camera_pitch = player.pitch;
            player_state.health = player.health;
            player_state.hunger = player.hunger;
            player_state.saturation = player.saturation;
            player_state.exhaustion = player.exhaustion;
            player_state.oxygen = player.oxygen;
            player_state.experience = player.experience;
            player_state.experience_level = player.experience_level;
            game_mode = player.game_mode;
            inventory = player.inventory.to_inventory();
            advancement_progress = player.advancements;
        }

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
            &world_time,
            0.0,
            false,
        );

        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let texture_atlas = crate::texture::TextureAtlas::new_procedural(&device, &queue);

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

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shader.wgsl"
            ))),
        });

        let region_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: Some("region_bind_group_layout"),
            });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[&camera_bind_group_layout],
                push_constant_ranges: &[],
            });

        let terrain_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Terrain Pipeline Layout"),
                bind_group_layouts: &[&camera_bind_group_layout, &region_bind_group_layout],
                push_constant_ranges: &[],
            });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
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
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let terrain_render_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Terrain Render Pipeline"),
                layout: Some(&terrain_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_terrain",
                    buffers: &[TerrainVertex::desc()],
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

        let trans_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Translucent Render Pipeline"),
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
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
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

        let terrain_trans_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Terrain Translucent Render Pipeline"),
                layout: Some(&terrain_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_terrain",
                    buffers: &[TerrainVertex::desc()],
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
                    cull_mode: terrain_translucent_cull_mode(),
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

        // Initialize Crosshair Pipeline
        let crosshair_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Crosshair Pipeline Layout"),
                bind_group_layouts: &[],
                push_constant_ranges: &[],
            });

        let crosshair_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Crosshair Render Pipeline"),
            layout: Some(&crosshair_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_crosshair",
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_crosshair",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
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

        // Crosshair Vertices (Horizontal and Vertical Lines)
        let aspect = size.width as f32 / size.height as f32;
        let crosshair_size = 0.02;
        let crosshair_vertices = [
            Vertex {
                position: [-crosshair_size, 0.0, 0.0],
                tex_coords: [0.0, 0.0],
                light_level: 1.0,
                ao: 1.0,
            },
            Vertex {
                position: [crosshair_size, 0.0, 0.0],
                tex_coords: [0.0, 0.0],
                light_level: 1.0,
                ao: 1.0,
            },
            Vertex {
                position: [0.0, -crosshair_size * aspect, 0.0],
                tex_coords: [0.0, 0.0],
                light_level: 1.0,
                ao: 1.0,
            },
            Vertex {
                position: [0.0, crosshair_size * aspect, 0.0],
                tex_coords: [0.0, 0.0],
                light_level: 1.0,
                ao: 1.0,
            },
        ];

        let crosshair_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Crosshair Vertex Buffer"),
            contents: bytemuck::cast_slice(&crosshair_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Initialize ChunkManager and load spawn area chunks
        let render_distance = settings.render_distance;
        let mut chunk_manager = ChunkManager::new_in_dimension(render_distance, current_dimension);
        let mut chunk_meshes = std::collections::HashMap::new();
        let (terrain_worker_tx, terrain_worker_rx) = std::sync::mpsc::channel();
        let mut chunk_lifetimes = std::collections::HashMap::new();
        let mut next_chunk_lifetime = 1u64;

        // Load only the immediate spawn area synchronously.  Loading every
        // chunk in a large render distance here used to create all CPU/GPU
        // meshes in one window event (625 chunks at distance 12), freezing the
        // app and often causing the graphics driver to reset.  `update_chunks`
        // loads the remaining requested chunks one at a time after the first
        // frame is visible.
        let player_chunk_x = (player_physics.position.x / CHUNK_WIDTH as f32).floor() as i32;
        let player_chunk_z = (player_physics.position.z / CHUNK_DEPTH as f32).floor() as i32;
        let mut pending_redstone_metadata: Vec<(
            i32,
            i32,
            Vec<crate::redstone::RedstoneComponentMetadata>,
        )> = Vec::new();
        if !is_client {
            let initial_radius = initial_chunk_radius(render_distance);
            for cx in player_chunk_x - initial_radius..=player_chunk_x + initial_radius {
                for cz in player_chunk_z - initial_radius..=player_chunk_z + initial_radius {
                    let mut chunk =
                        crate::dimension::generate_chunk(current_dimension, cx, cz, world_seed);
                    let saved_chunk = {
                        let mut manager = save_manager.lock().unwrap();
                        manager.load_chunk_in(current_dimension, cx, cz)
                    };
                    if let Some(data) = saved_chunk {
                        let generated_blocks =
                            crate::save::ChunkSaveData::from_chunk(&chunk).blocks;
                        if data.blocks != generated_blocks {
                            match mutation_revisions.ensure_at_least(current_dimension, cx, cz, 1) {
                                Ok(changed) => mutation_index_dirty |= changed,
                                Err(error) => {
                                    let message = format!(
                                        "Mutation revision tracking capacity was exhausted while \
                                         restoring spawn chunk ({cx}, {cz}): {error}"
                                    );
                                    eprintln!("[Save] {message}");
                                    mutation_index_load_error.get_or_insert(message);
                                }
                            }
                        }
                        let metadata = data.redstone_metadata();
                        data.restore_to_chunk(&mut chunk);
                        if !metadata.is_empty() {
                            pending_redstone_metadata.push((cx, cz, metadata));
                        }
                    }
                    chunk_manager.chunks.insert((cx, cz), chunk);
                }
            }
        }

        // Propagate lighting for spawn chunks synchronously
        let mut spawn_dirty = std::collections::HashSet::new();
        let chunk_keys: Vec<(i32, i32)> = chunk_manager.chunks.keys().cloned().collect();
        for &(cx, cz) in &chunk_keys {
            crate::lighting::propagate_chunk_lighting(&mut chunk_manager, cx, cz, &mut spawn_dirty);
        }

        // Spawn-area meshes are also built by the background workers. The
        // first frame can present immediately instead of blocking on nine CPU
        // meshes and their three LODs.
        for &coord in &chunk_keys {
            chunk_meshes.insert(coord, ChunkMesh::pending());
            chunk_lifetimes.insert(coord, next_chunk_lifetime);
            next_chunk_lifetime = next_chunk_lifetime.wrapping_add(1).max(1);
        }

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

        let mob_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Mob Vertex Buffer"),
            size: (std::mem::size_of::<Vertex>() * 8192) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mob_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Mob Index Buffer"),
            size: (std::mem::size_of::<u32>() * 12288) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // First-person hand buffers. Only a few dozen vertices are ever needed,
        // so keep them small and preallocated.
        let hand_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Hand Vertex Buffer"),
            size: (std::mem::size_of::<Vertex>() * 1024) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let hand_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Hand Index Buffer"),
            size: (std::mem::size_of::<u32>() * 1536) as wgpu::BufferAddress,
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

        let particle_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Particle Vertex Buffer"),
            size: (std::mem::size_of::<Vertex>() * crate::particles::MAX_PARTICLES * 4)
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let particle_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Particle Index Buffer"),
            size: (std::mem::size_of::<u32>() * crate::particles::MAX_PARTICLES * 6)
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
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
        let weather = crate::weather::WeatherSystem::new(world_seed);
        let network = match &role {
            MultiplayerRole::Singleplayer => NetworkHandle::None,
            MultiplayerRole::Host { port } => {
                let (host_to_server, host_commands) = std::sync::mpsc::channel();
                let (server_events, server_to_host) = std::sync::mpsc::channel();
                let gamemode = match game_mode {
                    GameMode::Creative => 0,
                    GameMode::Survival => 1,
                };
                let thread = crate::network::server::NetworkServer::spawn(
                    format!("0.0.0.0:{port}"),
                    u64::from(world_seed),
                    gamemode,
                    host_commands,
                    server_events,
                );
                NetworkHandle::Host {
                    server_to_host,
                    host_to_server,
                    thread: Some(thread),
                }
            }
            MultiplayerRole::Client {
                server_addr,
                port,
                username,
            } => {
                let (game_to_client, game_commands) = std::sync::mpsc::channel();
                let (client_events, client_to_game) = std::sync::mpsc::channel();
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
            render_pipeline,
            trans_pipeline,
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
            crosshair_pipeline,
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
            mining_target: None,
            mining_progress: 0.0,
            crack_vertex_buffer,
            crack_index_buffer,
            player_state,
            void_damage_timer: 0.0,
            world_time,
            show_debug,
            third_person: false,
            entity_manager: crate::entity::EntityManager::new(),
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
            mob_vertex_buffer,
            mob_index_buffer,
            mob_num_indices: 0,
            hand_pipeline,
            hand_vertex_buffer,
            hand_index_buffer,
            hand_num_indices: 0,
            hand_camera_buffer,
            hand_camera_bind_group,
            particles,
            particle_vertex_buffer,
            particle_index_buffer,
            particle_num_indices: 0,
            torch_smoke_timer: 0.0,
            total_time: 0.0,
            audio_manager,
            footstep_accumulator: 0.0,
            was_on_ground: false,
            water_tick_timer: 0.0,
            lava_tick_timer: 0.0,
            lava_damage_timer: 0.0,
            cactus_damage_timer: 0.0,
            save_manager,
            save_tx,
            save_queue_stats,
            autosave_timer: 0.0,
            is_saving: false,
            save_error: mutation_index_load_error,
            is_sprinting: false,
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
            mob_vertices_scratch: Vec::with_capacity(1024),
            mob_indices_scratch: Vec::with_capacity(1536),
            particle_vertices_scratch: Vec::with_capacity(1024),
            particle_indices_scratch: Vec::with_capacity(1536),
            hand_vertices_scratch: Vec::with_capacity(256),
            hand_indices_scratch: Vec::with_capacity(384),
            last_hand_mesh_key: None,
            ui_vertices_scratch: Vec::with_capacity(2048),
            ui_line_vertices_scratch: Vec::with_capacity(4096),
            debug_str_scratch: String::with_capacity(128),
            active_station: None,
            enchanting: crate::enchantment::EnchantingState::default(),
            brewing: crate::brewing::BrewingStandState::default(),
            anvil: crate::enchantment::AnvilState::default(),
            potion_effects: crate::brewing::EffectManager::default(),
            redstone: crate::redstone::RedstoneSystem::new(),
            redstone_tick_timer: 0.0,
            weather,
            difficulty: launch.difficulty,
            world_seed,
            settings,
            current_dimension,
            portal_contact_time: 0.0,
            portal_cooldown: 0.0,
            wither_effect_timer: 0.0,
            wither_damage_timer: 0.0,
            advancement_manager,
            advancement_gui,
            role,
            network,
            network_staging: NetworkStaging::default(),
            network_ready: !is_client,
            local_player_id: None,
            remote_players: std::collections::HashMap::new(),
            replicated_entities: std::collections::HashMap::new(),
            replicated_entity_ids: std::collections::HashSet::new(),
            entity_replication_sequence: 0,
            remote_player_health: std::collections::HashMap::new(),
            remote_player_effects: std::collections::HashMap::new(),
            client_player_health_sequence: 0,
            client_player_effect_sequence: 0,
            network_status: is_client.then(|| "CONNECTING TO SERVER...".to_string()),
            chat_messages: std::collections::VecDeque::new(),
            chat_input: String::new(),
            is_chat_open: false,
            connection_lost: false,
            network_position_timer: 0.0,
            network_pose_sequence: 0,
            network_time_sync_timer: 0.0,
            network_time: 0.0,
            pending_chunk_payloads: std::collections::HashMap::new(),
            pending_block_changes: std::collections::HashMap::new(),
            client_chunk_revisions: std::collections::HashMap::new(),
            mutation_revisions,
            mutation_revision_generation: u64::from(mutation_index_dirty),
            mutation_index_persist_in_flight: None,
            mutation_index_dirty,
            network_snapshot_worker,
            pending_player_catchups: std::collections::HashMap::new(),
            catchup_round_robin_cursor: 0,
        };

        // Restore persisted redstone component metadata (facing/delay/comparator
        // mode/note) for spawn-area chunks that were loaded before the redstone
        // system existed. The first `RedstoneSystem::tick` will call
        // `sync_loaded_chunks`, which rebuilds default `ComponentState` entries
        // for every loaded component; applying the sidecar first ensures those
        // rebuilt entries pick up the saved facing/delay/mode/note rather than
        // the defaults. The runtime first tick then settles power against the
        // restored facings. Subsequent streaming loads go through
        // `schedule_chunk_load`, which restores metadata alongside the chunk.
        for (cx, cz, metadata) in pending_redstone_metadata {
            state
                .redstone
                .restore_chunk_metadata(&state.chunk_manager, cx, cz, &metadata);
        }

        let initial_mesh_coords: Vec<_> = state.chunk_meshes.keys().copied().collect();
        state.invalidate_chunk_meshes(initial_mesh_coords, DependencyReason::ChunkLoad);
        state.load_current_dimension_entities();

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

    pub fn is_authoritative(&self) -> bool {
        !matches!(self.role, MultiplayerRole::Client { .. })
    }

    fn can_place_block_at(&self, x: i32, y: i32, z: i32, block: BlockType) -> bool {
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

    fn broadcast_block_change(&mut self, x: i32, y: i32, z: i32, block: BlockType) {
        if !matches!(self.role, MultiplayerRole::Host { .. }) {
            return;
        }
        let cx = x.div_euclid(CHUNK_WIDTH as i32);
        let cz = z.div_euclid(CHUNK_DEPTH as i32);
        let revision = match self.mutation_revisions.bump(self.current_dimension, cx, cz) {
            Ok(revision) => revision,
            Err(error) => {
                self.report_mutation_revision_error(error, "broadcasting a block mutation");
                return;
            }
        };
        self.mutation_revision_generation = self.mutation_revision_generation.saturating_add(1);
        self.mutation_index_dirty = true;
        let state = self.chunk_manager.get_block_state(x, y, z);
        self.network.broadcast_block_change(
            self.current_dimension,
            revision,
            x,
            y,
            z,
            block.to_wire(),
            state,
        );
    }

    fn report_mutation_revision_error(
        &mut self,
        error: crate::save::MutationRevisionIndexCapacityError,
        operation: &str,
    ) {
        let message = format!(
            "Mutation revision tracking capacity was exhausted while {operation}: {error}. \
             Multiplayer mutation delivery has been stopped to avoid sending an untracked revision."
        );
        eprintln!("[Save] {message}");
        self.save_error.get_or_insert(message);
    }

    fn schedule_player_catchup(&mut self, player_id: crate::network::protocol::PlayerId) {
        let entries = self
            .mutation_revisions
            .entries_in(self.current_dimension)
            .map(|((cx, cz), revision)| PlayerCatchupEntry {
                key: crate::save::NetworkSnapshotKey {
                    player_id,
                    dimension: self.current_dimension,
                    cx,
                    cz,
                    revision,
                },
                status: CatchupStatus::Pending,
                retries: 0,
            })
            .collect();
        self.pending_player_catchups.insert(player_id, entries);
    }

    fn process_join_catchups(&mut self) {
        if !matches!(self.role, MultiplayerRole::Host { .. }) {
            return;
        }
        let started = Instant::now();
        let worker_results: Vec<_> = self
            .network_snapshot_worker
            .try_iter()
            .take(64)
            .take_while(|_| started.elapsed() < Duration::from_millis(1))
            .collect();
        for result in worker_results {
            match result {
                crate::save::NetworkSnapshotWorkerResult::Snapshot(payload) => {
                    let Some(entry) = self
                        .pending_player_catchups
                        .get_mut(&payload.key.player_id)
                        .and_then(|entries| {
                            entries.iter_mut().find(|entry| entry.key == payload.key)
                        })
                    else {
                        continue;
                    };
                    match payload.result {
                        Ok((blocks, block_states)) => {
                            self.network.send_chunk_to(
                                payload.key.dimension,
                                payload.key.cx,
                                payload.key.cz,
                                payload.key.revision,
                                blocks,
                                block_states,
                                payload.key.player_id,
                            );
                            entry.status = CatchupStatus::ServerSubmission {
                                since: Instant::now(),
                            };
                        }
                        Err(error) => {
                            eprintln!("[Network] Catch-up snapshot retry: {error}");
                            entry.status = CatchupStatus::Pending;
                        }
                    }
                }
                crate::save::NetworkSnapshotWorkerResult::IndexPersisted { generation, result } => {
                    if self.mutation_index_persist_in_flight == Some(generation) {
                        self.mutation_index_persist_in_flight = None;
                    }
                    match result {
                        Ok(()) if generation == self.mutation_revision_generation => {
                            self.mutation_index_dirty = false;
                        }
                        Ok(()) => {
                            self.mutation_index_dirty = true;
                        }
                        Err(error) => {
                            eprintln!("[Network] Mutation revision index persist failed: {error}");
                            self.mutation_index_dirty = true;
                        }
                    }
                }
            }
        }

        if self.mutation_index_dirty && self.mutation_index_persist_in_flight.is_none() {
            let generation = self.mutation_revision_generation;
            if self
                .network_snapshot_worker
                .try_persist_index(generation, self.mutation_revisions.clone())
                .is_ok()
            {
                self.mutation_index_persist_in_flight = Some(generation);
            }
        }

        let now = Instant::now();
        let mut disconnect = Vec::new();
        for (&player_id, entries) in &mut self.pending_player_catchups {
            for entry in entries.iter_mut() {
                let timed_out = match entry.status {
                    CatchupStatus::ServerSubmission { since }
                    | CatchupStatus::AwaitingAck { since } => {
                        now.duration_since(since) >= CATCHUP_ACK_TIMEOUT
                    }
                    CatchupStatus::Pending | CatchupStatus::WorkerInFlight => false,
                };
                if timed_out {
                    entry.retries = entry.retries.saturating_add(1);
                    entry.status = CatchupStatus::Pending;
                    if entry.retries > MAX_CATCHUP_RETRIES {
                        disconnect.push(player_id);
                        break;
                    }
                }
            }
        }
        disconnect.sort_unstable();
        disconnect.dedup();
        for player_id in disconnect {
            self.pending_player_catchups.remove(&player_id);
            self.network.disconnect_slow_catchup_client(
                player_id,
                format!(
                    "catch-up ACK timed out after {} retries",
                    MAX_CATCHUP_RETRIES
                ),
            );
        }

        let mut player_ids: Vec<_> = self.pending_player_catchups.keys().copied().collect();
        player_ids.sort_unstable();
        let mut submitted = 0usize;
        let mut visited_without_submit = 0usize;
        while submitted < MAX_CATCHUP_SUBMITS_PER_FRAME
            && !player_ids.is_empty()
            && visited_without_submit < player_ids.len()
        {
            let slot = self.catchup_round_robin_cursor % player_ids.len();
            self.catchup_round_robin_cursor =
                (self.catchup_round_robin_cursor + 1) % player_ids.len();
            let player_id = player_ids[slot];
            let player_pos = self
                .remote_players
                .get(&player_id)
                .and_then(|remote| remote.snapshots.back().map(|snapshot| snapshot.position))
                .unwrap_or(self.player_physics.position);
            let candidate = self
                .pending_player_catchups
                .get(&player_id)
                .and_then(|entries| {
                    entries
                        .iter()
                        .enumerate()
                        .filter(|(_, entry)| entry.status == CatchupStatus::Pending)
                        .min_by(|(_, left), (_, right)| {
                            let distance = |entry: &PlayerCatchupEntry| {
                                let center_x = (entry.key.cx * CHUNK_WIDTH as i32
                                    + CHUNK_WIDTH as i32 / 2)
                                    as f32;
                                let center_z = (entry.key.cz * CHUNK_DEPTH as i32
                                    + CHUNK_DEPTH as i32 / 2)
                                    as f32;
                                (center_x - player_pos.x).powi(2)
                                    + (center_z - player_pos.z).powi(2)
                            };
                            distance(left)
                                .partial_cmp(&distance(right))
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                        .map(|(index, entry)| (index, entry.key))
                });
            let Some((entry_index, key)) = candidate else {
                visited_without_submit += 1;
                continue;
            };
            let chunk = (key.dimension == self.current_dimension)
                .then(|| self.chunk_manager.chunks.get(&(key.cx, key.cz)).cloned())
                .flatten()
                .map(Arc::new);
            match self
                .network_snapshot_worker
                .try_submit(crate::save::NetworkSnapshotRequest { key, chunk })
            {
                Ok(()) => {
                    if let Some(entry) = self
                        .pending_player_catchups
                        .get_mut(&player_id)
                        .and_then(|entries| entries.get_mut(entry_index))
                    {
                        entry.status = CatchupStatus::WorkerInFlight;
                    }
                    submitted += 1;
                    visited_without_submit = 0;
                }
                Err(crate::save::NetworkSnapshotSubmitError::Full) => break,
                Err(crate::save::NetworkSnapshotSubmitError::Closed) => {
                    eprintln!("[Network] Catch-up snapshot worker stopped");
                    break;
                }
            }
        }

        self.pending_player_catchups
            .retain(|_, entries| !entries.is_empty());
        self.perf_counters.network_queue_depth = self
            .pending_player_catchups
            .values()
            .map(|entries| entries.len() as u64)
            .sum::<u64>()
            .saturating_add(self.network_staging.len() as u64);
    }

    fn weather_sync_fields(&self) -> (u8, f32) {
        let snapshot = self.weather.snapshot();
        (snapshot.current.wire_value(), snapshot.remaining_ticks)
    }

    fn broadcast_time_sync(&self) {
        let (weather, weather_remaining_ticks) = self.weather_sync_fields();
        self.network
            .broadcast_time_sync(self.world_time.ticks, weather, weather_remaining_ticks);
    }

    fn send_time_sync_to(&self, player_id: crate::network::protocol::PlayerId) {
        let (weather, weather_remaining_ticks) = self.weather_sync_fields();
        self.network.send_time_sync_to(
            self.world_time.ticks,
            weather,
            weather_remaining_ticks,
            player_id,
        );
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
        if self.is_authoritative()
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
        if self.is_authoritative()
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
        if self.is_authoritative() {
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

    fn handle_single_network_event(&mut self, event: NetworkInbound) {
        match event {
            NetworkInbound::StatusUpdate(msg) => {
                self.network_status = Some(msg);
            }
            NetworkInbound::Connected {
                player_id,
                seed,
                gamemode,
            } => {
                self.local_player_id = Some(player_id);
                self.world_seed = seed as u32;
                let game_mode = if gamemode == 0 {
                    GameMode::Creative
                } else {
                    GameMode::Survival
                };
                self.set_game_mode(game_mode);
                self.inventory = match self.game_mode {
                    GameMode::Creative => Inventory::new_creative(),
                    GameMode::Survival => Inventory::new(),
                };
                self.weather = crate::weather::WeatherSystem::new(self.world_seed);
                self.chunk_manager.chunks.clear();
                self.teardown_terrain_runtime("network connect/reset");
                self.pending_chunk_payloads.clear();
                self.pending_block_changes.clear();
                self.client_chunk_revisions.clear();
                self.clear_replicated_entities();
                self.client_player_health_sequence = 0;
                self.client_player_effect_sequence = 0;
                self.network_ready = true;
                self.network_status = None;
                self.connection_lost = false;
                push_chat_history(
                    &mut self.chat_messages,
                    "[Network]".into(),
                    format!("Connected to server as player #{player_id}"),
                );
            }
            NetworkInbound::Disconnected(reason) => {
                eprintln!("[State] Network disconnected: {reason}");
                self.teardown_terrain_runtime("network disconnect");
                self.network_ready = false;
                self.network_status = Some(format!("CONNECTION LOST: {reason}"));
                self.connection_lost = true;
                self.is_chat_open = false;
                self.chat_input.clear();
                clear_remote_players(&mut self.remote_players, &mut self.entity_manager);
                self.clear_replicated_entities();
                self.set_paused(true);
                push_chat_history(
                    &mut self.chat_messages,
                    "[Network]".into(),
                    format!("Disconnected: {reason}"),
                );
            }
            NetworkInbound::PlayerJoin { id, username } => {
                if self.local_player_id != Some(id) {
                    if let Some(remote) = self.remote_players.get_mut(&id) {
                        remote.username = username.clone();
                        if let Some(entity) = self.entity_manager.get_by_id_mut(remote.entity_id) {
                            entity.username = username.clone();
                        }
                    } else {
                        let entity_id = self.entity_manager.spawn(
                            crate::entity::EntityType::RemotePlayer,
                            self.player_physics.position,
                        );
                        if let Some(entity) = self.entity_manager.get_by_id_mut(entity_id) {
                            entity.player_id = id;
                            entity.username = username.clone();
                        }
                        self.remote_players
                            .insert(id, RemotePlayerState::new(entity_id, username.clone()));
                    }
                    push_chat_history(
                        &mut self.chat_messages,
                        "[Network]".into(),
                        format!("{username} joined the game"),
                    );
                }
                if matches!(self.role, MultiplayerRole::Host { .. }) {
                    self.remote_player_health
                        .entry(id)
                        .or_insert_with(PlayerState::new);
                    self.remote_player_effects.entry(id).or_default();
                    self.network.notify_player_join(id, username);
                    self.send_time_sync_to(id);
                    self.schedule_player_catchup(id);
                }
            }
            NetworkInbound::PlayerLeave(id) => {
                self.pending_player_catchups.remove(&id);
                self.remote_player_health.remove(&id);
                self.remote_player_effects.remove(&id);
                if let Some(remote) = self.remote_players.remove(&id) {
                    push_chat_history(
                        &mut self.chat_messages,
                        "[Network]".into(),
                        format!("{} left the game", remote.username),
                    );
                    self.entity_manager.remove_by_id(remote.entity_id);
                } else {
                    push_chat_history(
                        &mut self.chat_messages,
                        "[Network]".into(),
                        format!("Player #{id} left the game"),
                    );
                }
            }
            NetworkInbound::PlayerPosition {
                id,
                sequence,
                sender_time_millis,
                x,
                y,
                z,
                yaw,
                pitch,
            } => {
                if self.local_player_id == Some(id) {
                    let authoritative = Vec3::new(x, y, z);
                    if self.player_physics.position.distance(authoritative)
                        > PLAYER_CORRECTION_SNAP_DISTANCE
                    {
                        self.player_physics.position = authoritative;
                        self.player_physics.velocity = Vec3::ZERO;
                        self.prev_player_position = authoritative;
                        self.camera.yaw = yaw;
                        self.camera.pitch = pitch;
                        self.perf_counters.prediction_rollback =
                            self.perf_counters.prediction_rollback.saturating_add(1);
                    }
                    return;
                }
                let candidate = Vec3::new(x, y, z);
                let position = if matches!(self.role, MultiplayerRole::Host { .. }) {
                    validated_remote_position(
                        self.remote_players
                            .get(&id)
                            .and_then(|remote| remote.snapshots.back()),
                        candidate,
                        sender_time_millis,
                    )
                } else {
                    candidate
                };
                if !self.remote_players.contains_key(&id) {
                    let username = String::new();
                    let entity_id = self
                        .entity_manager
                        .spawn(crate::entity::EntityType::RemotePlayer, position);
                    if let Some(entity) = self.entity_manager.get_by_id_mut(entity_id) {
                        entity.player_id = id;
                    }
                    self.remote_players
                        .insert(id, RemotePlayerState::new(entity_id, username));
                }

                let mut canonical_snapshot = None;
                if let Some(remote) = self.remote_players.get_mut(&id) {
                    let arrival = self.network_time;
                    let result = remote.push_snapshot(
                        position,
                        yaw,
                        pitch,
                        sequence,
                        sender_time_millis,
                        arrival,
                    );

                    if let Some(entity) = self.entity_manager.get_by_id_mut(remote.entity_id) {
                        let (snap_pos, snap_yaw, snap_pitch) =
                            if result == SnapshotPushResult::Snapped {
                                (position, yaw, pitch)
                            } else if let Some(samp) =
                                remote.sample(arrival - REMOTE_INTERPOLATION_DELAY)
                            {
                                (samp.position, samp.yaw, samp.pitch)
                            } else {
                                (Vec3::new(x, y, z), yaw, pitch)
                            };
                        entity.position = snap_pos;
                        entity.yaw = snap_yaw;
                        entity.pitch = snap_pitch;
                    }
                    if result != SnapshotPushResult::Rejected {
                        canonical_snapshot = remote.snapshots.back().copied();
                    }
                }
                if matches!(self.role, MultiplayerRole::Host { .. }) {
                    if let Some(snapshot) = canonical_snapshot {
                        self.network.broadcast_player_position(
                            id,
                            snapshot.sequence,
                            snapshot.sender_time_millis,
                            snapshot.position,
                            snapshot.yaw,
                            snapshot.pitch,
                        );
                    }
                }
            }
            NetworkInbound::PlayerAction { id, action } => {
                if let Some(remote) = self.remote_players.get(&id) {
                    if let Some(entity) = self.entity_manager.get_by_id_mut(remote.entity_id) {
                        entity.action_cooldown = match action {
                            crate::network::protocol::Action::Place
                            | crate::network::protocol::Action::Break
                            | crate::network::protocol::Action::Use => 0.25,
                        };
                    }
                }
                if matches!(self.role, MultiplayerRole::Host { .. }) {
                    if let NetworkHandle::Host { host_to_server, .. } = &self.network {
                        let _ = host_to_server.tracked_send(
                            crate::network::server::HostToServer::BroadcastPlayerAction {
                                id,
                                action,
                            },
                        );
                    }
                }
            }
            NetworkInbound::ClientBlockChange {
                id,
                x,
                y,
                z,
                block,
                state,
            } => {
                self.set_block_and_broadcast(id, x, y, z, block, state);
            }
            NetworkInbound::ClientBlockAction {
                id,
                action,
                x,
                y,
                z,
                block,
                held_item,
            } => {
                self.handle_client_block_action(id, action, x, y, z, block, held_item);
            }
            NetworkInbound::BlockActionResult {
                x,
                y,
                z,
                success,
                consumed_item,
                drops,
            } => {
                if success {
                    if consumed_item {
                        self.inventory
                            .use_selected_item(self.game_mode == GameMode::Creative);
                    }
                    for drop_wire in drops {
                        if let Some(stack) = drop_wire.to_stack() {
                            if let Some(leftover) = self.inventory.add_stack(stack) {
                                let sound_pos =
                                    glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                                self.spawn_dropped_item(leftover.item, sound_pos);
                            }
                        }
                    }
                    let mined_block = self.chunk_manager.get_block(x, y, z);
                    self.trigger_advancement(crate::advancements::AdvancementTrigger::MineBlock(
                        mined_block,
                    ));
                    self.damage_selected_tool(
                        (x as u32) ^ (y as u32).rotate_left(11) ^ (z as u32).rotate_left(22),
                    );
                }
            }
            NetworkInbound::AuthoritativeBlockChange {
                dimension,
                revision,
                x,
                y,
                z,
                block,
                state,
            } => {
                self.apply_remote_block_change(dimension, revision, x, y, z, block, state);
            }
            NetworkInbound::ChunkData {
                dimension,
                cx,
                cz,
                revision,
                blocks,
                block_states,
            } => {
                self.apply_remote_chunk_data(dimension, cx, cz, revision, blocks, block_states);
            }
            NetworkInbound::EntitySpawn {
                dimension,
                sequence,
                state,
            }
            | NetworkInbound::EntityState {
                dimension,
                sequence,
                state,
            } => {
                self.apply_replicated_entity_state(dimension, sequence, state);
            }
            NetworkInbound::EntityDespawn {
                dimension,
                sequence,
                entity_id,
            } => {
                self.apply_replicated_entity_despawn(dimension, sequence, entity_id);
            }
            NetworkInbound::PlayerHealth {
                sequence,
                player_id,
                health,
                max_health,
                hunger,
                saturation,
                oxygen,
                is_dead,
                death_reason,
            } => {
                if self.local_player_id == Some(player_id)
                    && !self.is_authoritative()
                    && sequence > self.client_player_health_sequence
                {
                    self.client_player_health_sequence = sequence;
                    self.player_state.health = health.clamp(0.0, max_health.max(0.0));
                    self.player_state.max_health = max_health.max(0.0);
                    self.player_state.hunger = hunger.clamp(0.0, 20.0);
                    self.player_state.saturation = saturation.clamp(0.0, 20.0);
                    self.player_state.oxygen = oxygen.clamp(0.0, 300.0);
                    self.player_state.is_dead = is_dead;
                    self.player_state.death_reason = DamageSource::from_wire(death_reason);
                    if is_dead {
                        self.clear_movement_input();
                        self.sync_cursor_mode();
                    }
                }
            }
            NetworkInbound::PlayerEffect {
                sequence,
                player_id,
                effects,
            } => {
                if self.local_player_id == Some(player_id)
                    && !self.is_authoritative()
                    && sequence > self.client_player_effect_sequence
                {
                    self.client_player_effect_sequence = sequence;
                    self.potion_effects.active =
                        effects.into_iter().filter_map(effect_from_wire).collect();
                }
            }
            NetworkInbound::TimeSync {
                ticks,
                weather,
                weather_remaining_ticks,
            } => {
                if !self.is_authoritative() {
                    self.world_time.ticks = ticks;
                    self.world_time.tick_accumulator = 0.0;
                    if let Some(current) = crate::weather::Weather::from_wire(weather) {
                        self.weather
                            .apply_snapshot(crate::weather::WeatherSnapshot {
                                current,
                                remaining_ticks: weather_remaining_ticks,
                            });
                    }
                }
            }
            NetworkInbound::LightningStrike(strike) => {
                if !self.is_authoritative()
                    && self.current_dimension == crate::dimension::Dimension::Overworld
                {
                    self.apply_lightning_strike(strike);
                }
            }
            NetworkInbound::ChatFromClient { id, message } => {
                let sender = self
                    .remote_players
                    .get(&id)
                    .map(|remote| remote.username.clone())
                    .filter(|username| !username.is_empty())
                    .unwrap_or_else(|| format!("Player {id}"));
                let Some(message) = normalized_chat_message(&message) else {
                    return;
                };
                push_chat_history(&mut self.chat_messages, sender.clone(), message.clone());
                self.network.send_chat(sender, message);
            }
            NetworkInbound::Chat { sender, message } => {
                let Some(message) = normalized_chat_message(&message) else {
                    return;
                };
                push_chat_history(&mut self.chat_messages, sender, message);
            }
            NetworkInbound::CatchupAccepted {
                id,
                dimension,
                cx,
                cz,
                revision,
            } => {
                let key = crate::dimension::Dimension::from_wire(dimension).map(|dimension| {
                    crate::save::NetworkSnapshotKey {
                        player_id: id,
                        dimension,
                        cx,
                        cz,
                        revision,
                    }
                });
                if let Some(key) = key {
                    if let Some(entry) = self
                        .pending_player_catchups
                        .get_mut(&id)
                        .and_then(|entries| entries.iter_mut().find(|entry| entry.key == key))
                    {
                        entry.status = CatchupStatus::AwaitingAck {
                            since: Instant::now(),
                        };
                    }
                }
            }
            NetworkInbound::CatchupBackpressured {
                id,
                dimension,
                cx,
                cz,
                revision,
                mailbox_full_count,
            } => {
                self.perf_counters.network_catchup_mailbox_full = self
                    .perf_counters
                    .network_catchup_mailbox_full
                    .max(mailbox_full_count);
                let key = crate::dimension::Dimension::from_wire(dimension).map(|dimension| {
                    crate::save::NetworkSnapshotKey {
                        player_id: id,
                        dimension,
                        cx,
                        cz,
                        revision,
                    }
                });
                if let Some(key) = key {
                    if let Some(entry) = self
                        .pending_player_catchups
                        .get_mut(&id)
                        .and_then(|entries| entries.iter_mut().find(|entry| entry.key == key))
                    {
                        entry.retries = entry.retries.saturating_add(1);
                        entry.status = CatchupStatus::Pending;
                    }
                }
            }
            NetworkInbound::CatchupAck {
                id,
                dimension,
                cx,
                cz,
                revision,
            } => {
                if let Some(dimension) = crate::dimension::Dimension::from_wire(dimension) {
                    if let Some(entries) = self.pending_player_catchups.get_mut(&id) {
                        entries.retain(|entry| {
                            entry.key
                                != (crate::save::NetworkSnapshotKey {
                                    player_id: id,
                                    dimension,
                                    cx,
                                    cz,
                                    revision,
                                })
                        });
                    }
                    self.pending_player_catchups
                        .retain(|_, entries| !entries.is_empty());
                }
            }
        }
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

    fn update_network_time_sync(&mut self, dt: f32) {
        if !matches!(self.role, MultiplayerRole::Host { .. }) || !self.network_ready {
            return;
        }
        self.network_time_sync_timer += dt;
        if self.network_time_sync_timer >= 1.0 {
            self.network_time_sync_timer %= 1.0;
            self.broadcast_time_sync();
        }
    }

    fn broadcast_authoritative_replication(&mut self, dt: f32) {
        if !matches!(self.role, MultiplayerRole::Host { .. }) || !self.network_ready {
            return;
        }

        self.entity_replication_sequence = self.entity_replication_sequence.wrapping_add(1).max(1);
        let sequence = self.entity_replication_sequence;
        let states: Vec<_> = self
            .entity_manager
            .entities
            .iter()
            .filter(|entity| is_replicated_entity_type(entity.entity_type))
            .map(entity_state_wire)
            .collect();
        let current_ids: std::collections::HashSet<_> =
            states.iter().map(|state| state.entity_id).collect();

        let mut despawned: Vec<_> = self
            .replicated_entity_ids
            .difference(&current_ids)
            .copied()
            .collect();
        despawned.sort_unstable();
        for entity_id in despawned {
            self.network
                .broadcast_entity_despawn(self.current_dimension, sequence, entity_id);
        }

        for state in &states {
            if !self.replicated_entity_ids.contains(&state.entity_id) {
                self.network
                    .broadcast_entity_spawn(self.current_dimension, sequence, *state);
            }
            self.network
                .broadcast_entity_state(self.current_dimension, sequence, *state);
        }
        self.replicated_entity_ids = current_ids;

        self.network
            .broadcast_player_health(sequence, 0, &self.player_state);
        self.network.broadcast_player_effects(
            sequence,
            0,
            self.potion_effects
                .active
                .iter()
                .copied()
                .map(effect_to_wire)
                .collect(),
        );

        let remote_positions: Vec<_> = self
            .remote_players
            .iter()
            .filter_map(|(id, remote)| {
                remote
                    .snapshots
                    .back()
                    .map(|snapshot| (*id, snapshot.position))
            })
            .collect();
        for (player_id, position) in remote_positions {
            let state = self
                .remote_player_health
                .entry(player_id)
                .or_insert_with(PlayerState::new);
            if let Some((amount, source)) = state.update(dt, false) {
                state.take_damage(amount, source);
            }
            let effects = self.remote_player_effects.entry(player_id).or_default();
            let effect_health = effects.update(dt);
            if effect_health > 0.0 {
                state.health = (state.health + effect_health).min(state.max_health);
            } else if effect_health < 0.0 && state.health > 1.0 {
                state.take_damage((-effect_health).min(state.health - 1.0), DamageSource::Mob);
            }

            if self.game_mode != GameMode::Creative
                && self
                    .entity_manager
                    .query_radius(position, 2.2)
                    .any(|entity| entity.entity_type.is_hostile() && entity.health > 0.0)
            {
                state.take_damage(3.0, DamageSource::Mob);
            }

            self.network
                .broadcast_player_health(sequence, player_id, state);
            self.network.broadcast_player_effects(
                sequence,
                player_id,
                effects.active.iter().copied().map(effect_to_wire).collect(),
            );
        }
    }

    pub fn shutdown_network(&mut self) {
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
        let can_fly = self.game_mode == GameMode::Creative && !self.player_state.is_dead;
        if self.jump_taps.register(now, can_fly, repeat) {
            let flying = !self.player_physics.is_flying();
            self.player_physics.set_flying(flying);
        }
    }

    pub fn set_game_mode(&mut self, game_mode: GameMode) {
        self.jump_taps.reset();
        if game_mode != GameMode::Creative {
            self.player_physics.set_flying(false);
        }
        self.game_mode = game_mode;
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
        if self.is_authoritative() {
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

    fn enqueue_chunk_save(
        &self,
        snapshot: crate::save::UncompressedChunkSnapshot,
        tracker: crate::save::DirtyChunkSet,
        revision: u64,
    ) -> crate::save::SaveResult<()> {
        let cx = snapshot.chunk_x;
        let cz = snapshot.chunk_z;
        if !tracker.begin_save(cx, cz, revision) {
            return Ok(());
        }
        if let Err(error) = self.save_tx.send(crate::save::SaveCommand::SaveChunk {
            snapshot,
            revision,
            tracker: tracker.clone(),
        }) {
            tracker.acknowledge_failed(cx, cz, revision);
            return Err(error);
        }
        Ok(())
    }

    pub fn trigger_background_save(&self) -> crate::save::SaveResult<()> {
        if !self.is_authoritative() {
            return Ok(());
        }
        let world_dir = self.save_manager.lock().unwrap().world_dir.clone();
        crate::menu::update_world_metadata(
            &world_dir,
            self.world_seed,
            self.game_mode,
            self.difficulty,
        )
        .map_err(|error| crate::save::SaveError::Io {
            operation: "update world metadata",
            path: world_dir.join("world.meta"),
            message: error.to_string(),
        })?;
        let level = crate::save::LevelData {
            seed: self.world_seed,
            time: self.world_time.ticks,
        };
        let player = crate::save::PlayerData::from_state(
            self.player_physics.position,
            self.player_physics.persistent_velocity(),
            self.camera.yaw,
            self.camera.pitch,
            &self.player_state,
            self.game_mode,
            &self.inventory,
            self.advancement_manager.progress.clone(),
        );
        self.save_tx
            .send(crate::save::SaveCommand::SaveLevelAndPlayer(level, player))?;

        let tracker = self.chunk_manager.dirty_chunks.clone();
        for ((cx, cz), revision) in tracker.dirty_revisions() {
            if let Some(chunk) = self.chunk_manager.chunks.get(&(cx, cz)) {
                let redstone_metadata =
                    self.redstone
                        .collect_chunk_metadata(&self.chunk_manager, cx, cz);
                let snapshot = crate::save::UncompressedChunkSnapshot::from_chunk_with_redstone(
                    self.current_dimension,
                    chunk,
                    redstone_metadata,
                )
                .with_mutation_revision(self.mutation_revisions.latest(
                    self.current_dimension,
                    cx,
                    cz,
                ));
                self.enqueue_chunk_save(snapshot, tracker.clone(), revision)?;
            }
        }
        self.save_manager
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .save_current_dimension(self.current_dimension)
            .map_err(|error| crate::save::SaveError::Io {
                operation: "save current dimension",
                path: world_dir.join("dimension.dat"),
                message: error.to_string(),
            })?;
        self.save_manager
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .save_mutation_revision_index(&self.mutation_revisions)
            .map_err(|error| crate::save::SaveError::Io {
                operation: "save mutation revision index",
                path: world_dir.join("mutation_revisions.bin"),
                message: error.to_string(),
            })?;
        self.save_current_dimension_entities()?;
        Ok(())
    }

    pub fn save_synchronously(&self) -> crate::save::SaveResult<()> {
        if !self.is_authoritative() {
            return Ok(());
        }
        // Mark all currently loaded chunks dirty for complete save and quit flush
        for &coord in self.chunk_manager.chunks.keys() {
            self.chunk_manager.dirty_chunks.mark_dirty(coord.0, coord.1);
        }
        self.trigger_background_save()?;

        let (ack_tx, ack_rx) = std::sync::mpsc::channel();
        self.save_tx.send(crate::save::SaveCommand::Flush(ack_tx))?;
        ack_rx
            .recv()
            .map_err(|_| crate::save::SaveError::QueueClosed)??;
        println!("[Save] Synchronously saved world state.");
        Ok(())
    }

    pub fn save_current_dimension_entities(&self) -> crate::save::SaveResult<()> {
        let save_manager = match self.save_manager.lock() {
            Ok(mgr) => mgr,
            Err(error) => error.into_inner(),
        };
        let persistent_entities: Vec<crate::save::EntitySaveData> = self
            .entity_manager
            .entities
            .iter()
            .map(crate::save::EntitySaveData::from)
            .filter(|data| data.should_persist())
            .collect();

        let path = save_manager.entities_file_path(self.current_dimension);
        save_manager
            .save_entities_in(self.current_dimension, &persistent_entities)
            .map_err(|error| crate::save::SaveError::Io {
                operation: "save entities",
                path,
                message: error.to_string(),
            })
    }

    pub fn load_current_dimension_entities(&mut self) {
        let save_manager = match self.save_manager.lock() {
            Ok(mgr) => mgr,
            Err(_) => return,
        };
        let saved_entities = save_manager.load_entities_in(self.current_dimension);
        if saved_entities.is_empty() {
            return;
        }

        self.entity_manager.entities.clear();
        for data in &saved_entities {
            self.entity_manager.add_restored_entity(data);
        }
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

        if let Some(levels) = &existing_section.levels {
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

        let mut metrics = UploadMetrics::default();
        let levels = std::array::from_fn(|index| {
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
            GpuMeshLevel {
                opaque,
                transparent,
                bounds: data.bounds(),
            }
        });
        (levels, metrics)
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
            .section(key.section_y as usize)?
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
            .and_then(|mesh| mesh.section_mut(key.section_y as usize))
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
        for section_y in 0..SECTION_COUNT {
            invalidated |= self.invalidate_section_mesh(
                SectionKey::new(coord.0, section_y as u16, coord.1),
                reason,
            );
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
    fn invalidate_block_mesh_dependencies(
        &mut self,
        wx: i32,
        wy: i32,
        wz: i32,
        reason: DependencyReason,
    ) {
        let owner = SectionKey::new(
            wx.div_euclid(CHUNK_WIDTH as i32),
            (wy as usize / crate::world::SECTION_SIZE) as u16,
            wz.div_euclid(CHUNK_DEPTH as i32),
        );
        let mut dependencies = std::collections::HashSet::new();
        mark_section_mesh_dependencies(&mut dependencies, wx, wy, wz);
        for key in dependencies {
            let dependency_reason = if key == owner {
                reason
            } else {
                DependencyReason::Ao
            };
            self.invalidate_section_mesh(key, dependency_reason);
        }
    }

    fn process_terrain_worker_results(&mut self, player_chunk: (i32, i32)) {
        let integrate_started = Instant::now();
        let mut lighting_elapsed = Duration::ZERO;
        let mut gpu_upload_elapsed = Duration::ZERO;

        let mut integrated_meshes = 0;
        let mut integrated_bytes = 0u64;

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
                    if expected == Some(result.lifetime) {
                        self.chunk_load_in_flight.remove(&result.coord);
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
                        self.perf_counters.stale_results =
                            self.perf_counters.stale_results.saturating_add(1);
                        continue;
                    }

                    let (cx, cz) = result.coord;
                    if result.mutated {
                        match self.mutation_revisions.ensure_at_least(
                            self.current_dimension,
                            cx,
                            cz,
                            1,
                        ) {
                            Ok(true) => {
                                self.mutation_revision_generation =
                                    self.mutation_revision_generation.saturating_add(1);
                                self.mutation_index_dirty = true;
                            }
                            Ok(false) => {}
                            Err(error) => self.report_mutation_revision_error(
                                error,
                                "integrating a mutated streamed chunk",
                            ),
                        }
                    }
                    self.chunk_manager.chunks.insert(result.coord, result.chunk);
                    self.chunk_lifetimes.insert(result.coord, result.lifetime);
                    self.chunk_meshes.insert(result.coord, ChunkMesh::pending());
                    self.invalidate_chunk_mesh(result.coord, DependencyReason::ChunkLoad);

                    // Restore persisted redstone component metadata before any
                    // redstone tick runs, so freshly-rebuilt `ComponentState`
                    // entries pick up the saved facing/delay/mode/note instead
                    // of the runtime defaults. The next `RedstoneSystem::tick`
                    // settles power against the restored facings.
                    if !result.redstone_metadata.is_empty() {
                        self.redstone.restore_chunk_metadata(
                            &self.chunk_manager,
                            cx,
                            cz,
                            &result.redstone_metadata,
                        );
                    }

                    let mut pending_base_revision = 0;
                    if let Some((revision, blocks, block_states)) =
                        self.pending_chunk_payloads.remove(&result.coord)
                    {
                        pending_base_revision = revision;
                        if let Some(chunk) = self.chunk_manager.chunks.get_mut(&result.coord) {
                            Self::restore_chunk_payload(chunk, &blocks, &block_states);
                        }
                        self.invalidate_chunk_mesh(result.coord, DependencyReason::Network);
                    }
                    if let Some(changes) = self.pending_block_changes.remove(&result.coord) {
                        self.client_chunk_revisions.insert(
                            (self.current_dimension, result.coord.0, result.coord.1),
                            pending_base_revision,
                        );
                        let mut changes: Vec<_> = changes.into_iter().collect();
                        changes.sort_by_key(|(_, (revision, _, _))| *revision);
                        for ((x, y, z), (revision, block, state)) in changes {
                            self.apply_remote_block_change(
                                self.current_dimension as u8,
                                revision,
                                x,
                                y,
                                z,
                                block,
                                state,
                            );
                        }
                    }

                    let mut dirty = std::collections::HashSet::new();
                    self.check_and_break_unsupported_for_loaded_chunk(cx, cz, &mut dirty);
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
                    let Some(section) = mesh.section_mut(identity.key.section_y as usize) else {
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
        let lifetime = self.next_chunk_lifetime();
        self.chunk_load_in_flight.insert(coord, lifetime);
        let sender = self.terrain_worker_tx.clone();
        let generation = self.terrain_generation;
        let dimension = self.current_dimension;
        let world_seed = self.world_seed;
        let authoritative = self.is_authoritative();
        let save_manager = self.save_manager.clone();
        rayon::spawn(move || {
            let mut chunk =
                crate::dimension::generate_chunk(dimension, coord.0, coord.1, world_seed);
            let mut mutated = false;
            let mut redstone_metadata = Vec::new();
            if authoritative {
                if let Some(saved) = save_manager
                    .lock()
                    .unwrap()
                    .load_chunk_in(dimension, coord.0, coord.1)
                {
                    let generated_blocks = crate::save::ChunkSaveData::from_chunk(&chunk).blocks;
                    mutated = saved.blocks != generated_blocks;
                    redstone_metadata = saved.redstone_metadata();
                    saved.restore_to_chunk(&mut chunk);
                }
            }
            let _ = sender.send(TerrainWorkerResult::Loaded(ChunkLoadResult {
                coord,
                dimension,
                generation,
                lifetime,
                chunk,
                mutated,
                redstone_metadata,
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
            return false;
        }
        let Some(section) = self
            .chunk_meshes
            .get(&(key.cx, key.cz))
            .and_then(|mesh| mesh.section(key.section_y as usize))
        else {
            return false;
        };
        if !section.needs_rebuild() || self.current_section_identity(key) != Some(work.identity) {
            return false;
        }
        let snapshot = self.chunk_manager.capture_section_halo(key);
        self.section_scheduler.mark_in_flight(work);
        let sender = self.terrain_worker_tx.clone();
        let generation = self.terrain_generation;
        rayon::spawn(move || {
            let bundle = Chunk::generate_section_mesh_bundle_from_halo(work.identity, &snapshot);
            let _ = sender.send(TerrainWorkerResult::SectionMeshed(SectionMeshResult {
                generation,
                bundle,
            }));
        });
        true
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

            let hysteresis_r = (r as i32) + crate::chunk_schedule::UNLOAD_HYSTERESIS;
            let mut to_unload = Vec::new();
            for &(cx, cz) in self.chunk_manager.chunks.keys() {
                if (cx - px).abs() > hysteresis_r || (cz - pz).abs() > hysteresis_r {
                    to_unload.push((cx, cz));
                }
            }
            for &(cx, cz) in &to_unload {
                let tracker = self.chunk_manager.dirty_chunks.clone();
                let revision = tracker.dirty_revision(cx, cz);
                let redstone_metadata = revision.map(|_| {
                    self.redstone
                        .collect_chunk_metadata(&self.chunk_manager, cx, cz)
                });
                if let Some(chunk) = self.chunk_manager.chunks.remove(&(cx, cz)) {
                    if self.is_authoritative() {
                        if let (Some(revision), Some(redstone_metadata)) =
                            (revision, redstone_metadata)
                        {
                            let snapshot =
                                crate::save::UncompressedChunkSnapshot::from_chunk_with_redstone(
                                    self.current_dimension,
                                    &chunk,
                                    redstone_metadata,
                                )
                                .with_mutation_revision(
                                    self.mutation_revisions
                                        .latest(self.current_dimension, cx, cz),
                                );
                            if let Err(error) = self.enqueue_chunk_save(snapshot, tracker, revision)
                            {
                                eprintln!("[Save] Could not queue unloaded chunk: {error}");
                            }
                        }
                    }
                }
            }
            for &(cx, cz) in &to_unload {
                for neighbor in surrounding_chunk_coords(cx, cz) {
                    if self.chunk_manager.chunks.contains_key(&neighbor) {
                        self.invalidate_chunk_mesh(neighbor, DependencyReason::ChunkLoad);
                    }
                }
                self.chunk_lifetimes.remove(&(cx, cz));
                self.section_scheduler.remove_chunk(cx, cz);
                self.scheduler.remove_dirty(&(cx, cz));
            }
            let mut removed_mesh_keys = Vec::new();
            for &(cx, cz) in self.chunk_meshes.keys() {
                if (cx - px).abs() > hysteresis_r || (cz - pz).abs() > hysteresis_r {
                    removed_mesh_keys.push((cx, cz));
                }
            }
            for coord in removed_mesh_keys {
                if let Some(mesh) = self.chunk_meshes.remove(&coord) {
                    Self::free_chunk_mesh_allocations(&mut self.render_regions, coord, &mesh);
                }
            }
            self.chunk_load_in_flight.retain(|&(cx, cz), _| {
                (cx - px).abs() <= hysteresis_r && (cz - pz).abs() <= hysteresis_r
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
            self.scheduler.reprioritize_dirty((px, pz));
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
                let Some(section) = self
                    .chunk_meshes
                    .get(&(key.cx, key.cz))
                    .and_then(|mesh| mesh.section(key.section_y as usize))
                else {
                    continue;
                };
                if !section.needs_rebuild()
                    || self.current_section_identity(key) != Some(work.identity)
                {
                    continue;
                }
                if self.section_scheduler.is_in_flight(key) {
                    deferred.push(work);
                    continue;
                }
                if self.schedule_section_mesh(work) {
                    dispatched += 1;
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
        println!("[Debug] set_paused called with: {}", paused);
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
        let authoritative = self.is_authoritative();

        self.autosave_timer += dt;
        if self.is_authoritative() && self.autosave_timer >= 300.0 {
            self.autosave_timer = 0.0;
            if let Err(error) = self.trigger_background_save() {
                eprintln!("[Save] Could not enqueue autosave: {error}");
            }
        }

        self.water_tick_timer += dt;
        if self.is_authoritative() && self.water_tick_timer >= 0.25 {
            self.water_tick_timer = 0.0;
            let lighting_started = Instant::now();
            let (mut dirty, mutations) =
                crate::fluid::tick_fluids(&mut self.chunk_manager, false, 2048);
            for ((x, y, z), block) in mutations {
                self.broadcast_block_change(x, y, z, block);
                self.check_and_break_unsupported_above(x, y, z, &mut dirty);
            }
            self.invalidate_chunk_meshes(dirty, DependencyReason::Fluid);
            let lighting_elapsed = lighting_started.elapsed();
            self.lighting_time_frame += lighting_elapsed;
            self.lighting_scopes_frame.record(
                crate::perf::LightingSource::Fluid as usize,
                lighting_elapsed,
            );
        }

        self.lava_tick_timer += dt;
        if self.is_authoritative() && self.lava_tick_timer >= 1.5 {
            self.lava_tick_timer = 0.0;
            let lighting_started = Instant::now();
            let (mut dirty, mutations) =
                crate::fluid::tick_fluids(&mut self.chunk_manager, true, 512);
            for ((x, y, z), block) in mutations {
                self.broadcast_block_change(x, y, z, block);
                self.check_and_break_unsupported_above(x, y, z, &mut dirty);
            }
            self.invalidate_chunk_meshes(dirty, DependencyReason::Fluid);
            let lighting_elapsed = lighting_started.elapsed();
            self.lighting_time_frame += lighting_elapsed;
            self.lighting_scopes_frame.record(
                crate::perf::LightingSource::Fluid as usize,
                lighting_elapsed,
            );
        }

        if self.is_authoritative() {
            self.update_portal_travel(dt);
        }

        if self.is_authoritative() {
            self.redstone_tick_timer += dt;
        }
        let redstone_started = Instant::now();
        let mut redstone_steps = 0;
        while self.is_authoritative() && self.redstone_tick_timer >= 0.05 && redstone_steps < 4 {
            self.redstone_tick_timer -= 0.05;
            redstone_steps += 1;
            let mut occupants = Vec::with_capacity(self.entity_manager.entities.len() + 1);
            occupants.push((
                self.player_physics.position.x.floor() as i32,
                self.player_physics.position.y.floor() as i32,
                self.player_physics.position.z.floor() as i32,
            ));
            occupants.extend(self.entity_manager.entities.iter().map(|entity| {
                (
                    entity.position.x.floor() as i32,
                    entity.position.y.floor() as i32,
                    entity.position.z.floor() as i32,
                )
            }));
            let update = self.redstone.tick(&mut self.chunk_manager, &occupants);
            self.apply_redstone_update(update);
        }
        if redstone_steps == 4 {
            self.redstone_tick_timer = self.redstone_tick_timer.min(0.05);
        }
        let redstone_elapsed = redstone_started.elapsed();
        self.perf_recorder
            .record(crate::perf::ScopeId::Redstone, redstone_elapsed);
        self.lighting_time_frame += redstone_elapsed;
        self.lighting_scopes_frame.record(
            crate::perf::LightingSource::Redstone as usize,
            redstone_elapsed,
        );

        self.brewing.update(dt);
        let effect_health = self.potion_effects.update(dt);
        if self.is_authoritative() && effect_health > 0.0 {
            self.player_state.health =
                (self.player_state.health + effect_health).min(self.player_state.max_health);
        } else if self.is_authoritative() && effect_health < 0.0 && self.player_state.health > 1.0 {
            self.take_damage(
                (-effect_health).min(self.player_state.health - 1.0),
                DamageSource::Mob,
            );
        }
        if self.is_authoritative() && self.wither_effect_timer > 0.0 {
            self.wither_effect_timer = (self.wither_effect_timer - dt).max(0.0);
            self.wither_damage_timer += dt;
            if self.wither_damage_timer >= 1.0 {
                self.wither_damage_timer -= 1.0;
                self.take_damage(1.0, DamageSource::Mob);
            }
        } else {
            self.wither_damage_timer = 0.0;
        }

        let can_sprint = sprint_allowed(self.game_mode, self.player_state.hunger);

        // Double click W logic
        if self.keys.w && !self.last_w_pressed {
            if self.w_click_timer > 0.0 && can_sprint {
                self.is_sprinting = true;
            }
            self.w_click_timer = 0.3;
        }
        self.last_w_pressed = self.keys.w;

        // Ctrl key sprint check
        if self.keys.ctrl && self.keys.w && can_sprint {
            self.is_sprinting = true;
        }

        // Cancel sprinting conditions
        if !self.keys.w || self.keys.shift || !can_sprint {
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

        // Consume more hunger when sprinting
        let sprint_exhaustion = sprint_exhaustion_amount(
            self.game_mode,
            self.is_sprinting,
            self.keys.w || self.keys.a || self.keys.s || self.keys.d,
            dt,
        );
        if authoritative && sprint_exhaustion > 0.0 {
            self.player_state.add_exhaustion(sprint_exhaustion);
        }

        // Update game time
        let speed_multiplier = if self.keys.f { 60.0 } else { 1.0 };
        let elapsed_world_ticks = dt * 20.0 * speed_multiplier;
        self.world_time.tick_accumulator += elapsed_world_ticks;
        let new_ticks = self.world_time.tick_accumulator.floor() as u64;
        self.world_time.ticks += new_ticks;
        self.world_time.tick_accumulator -= new_ticks as f32;
        if self.current_dimension == crate::dimension::Dimension::Overworld {
            let weather_update = if self.is_authoritative() {
                self.weather.update_authoritative(elapsed_world_ticks, dt)
            } else {
                self.weather.update_client(elapsed_world_ticks, dt);
                crate::weather::WeatherUpdate::default()
            };
            if weather_update.changed {
                self.broadcast_time_sync();
            }
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
            movement.y = match (self.keys.space, self.keys.shift) {
                (true, false) => 1.0,
                (false, true) => -1.0,
                _ => 0.0,
            };
        } else if self.keys.space {
            movement.y = 1.0;
        }

        // Jump exhaustion check
        let jumped = !was_flying && self.keys.space && self.player_physics.on_ground;
        if authoritative && jumped && self.game_mode == GameMode::Survival {
            self.player_state.add_exhaustion(0.05);
        }
        if jumped {
            self.audio_manager.play_sound(crate::audio::SoundId::Jump);
        }

        let old_pos = self.player_physics.position;

        let physics_started = Instant::now();
        let fall_damage = self.player_physics.update(
            dt,
            &self.chunk_manager,
            movement,
            self.keys.shift && !was_flying,
            self.is_sprinting,
        );
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

        // Apply fall damage
        if self.game_mode == GameMode::Survival && fall_damage > 0.0 {
            self.take_damage(fall_damage, DamageSource::Fall);
        }

        // Movement exhaustion check
        let horizontal_dist = glam::Vec2::new(
            self.player_physics.position.x - old_pos.x,
            self.player_physics.position.z - old_pos.z,
        )
        .length();
        if authoritative && self.game_mode == GameMode::Survival {
            self.player_state.add_exhaustion(0.02 * horizontal_dist);
        }

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

        self.was_on_ground = self.player_physics.on_ground;

        // Dropped item collection
        {
            let player_pos = self.player_physics.position;
            let to_collect: Vec<u64> = self
                .entity_manager
                .query_radius_types(player_pos, 1.5, &[crate::entity::EntityType::DroppedItem])
                .filter(|entity| entity.pickup_cooldown <= 0.0 && entity.dropped_item.is_some())
                .map(|entity| entity.id)
                .collect();
            for id in to_collect {
                let Some((item, mut remaining)) = self
                    .entity_manager
                    .get_by_id(id)
                    .map(|entity| (entity.dropped_item, entity.dropped_count.max(1)))
                else {
                    continue;
                };
                if let Some(item) = item {
                    while remaining > 0 && self.inventory.add_item(item) {
                        remaining -= 1;
                    }
                    if remaining == 0 {
                        self.entity_manager.remove_by_id(id);
                    } else {
                        if let Some(index) = self.entity_manager.id_to_index.get(&id).copied() {
                            self.entity_manager.entities[index].dropped_count = remaining;
                        }
                    }
                }
            }
        }

        // Void damage check
        if self.player_physics.position.y < -64.0 {
            self.void_damage_timer += dt;
            if self.void_damage_timer >= 0.5 {
                self.void_damage_timer = 0.0;
                self.take_damage(2.0, DamageSource::Void);
            }
        } else {
            self.void_damage_timer = 0.0;
        }

        // Lava damage check
        let px = self.player_physics.position.x.floor() as i32;
        let py = self.player_physics.position.y.floor() as i32;
        let pz = self.player_physics.position.z.floor() as i32;
        let block_at_feet = self.chunk_manager.get_block(px, py, pz);
        let block_at_eyes = self.chunk_manager.get_block(
            px,
            (self.player_physics.position.y + 1.62).floor() as i32,
            pz,
        );
        let player_in_lava = block_at_feet == BlockType::Lava || block_at_eyes == BlockType::Lava;

        if player_in_lava && !self.potion_effects.has_fire_resistance() {
            self.lava_damage_timer += dt;
            if self.lava_damage_timer >= 0.5 {
                self.lava_damage_timer = 0.0;
                self.take_damage(4.0, DamageSource::Mob);
            }
        } else {
            self.lava_damage_timer = 0.0;
        }

        // Leaf Decay Random Ticks (30 random ticks per 20 Hz sim tick)
        let chunk_keys: Vec<(i32, i32)> = self.chunk_manager.chunks.keys().cloned().collect();
        if self.is_authoritative() && !chunk_keys.is_empty() {
            let mut rng_seed = (self.total_time * 1000.0) as u32;
            let mut next_rand = |max: u32| -> u32 {
                rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
                ((rng_seed / 65536) % 32768) % max
            };

            for _ in 0..30 {
                let chunk_idx = next_rand(chunk_keys.len() as u32) as usize;
                let (cx, cz) = chunk_keys[chunk_idx];

                let rx = next_rand(16) as i32;
                let rz = next_rand(16) as i32;
                let ry = next_rand(120) as i32 + 40;

                let wx = cx * 16 + rx;
                let wz = cz * 16 + rz;

                let block = self.chunk_manager.get_block(wx, ry, wz);
                if block == BlockType::OakLeaves
                    || block == BlockType::BirchLeaves
                    || block == BlockType::SpruceLeaves
                {
                    let mut queue = std::collections::VecDeque::new();
                    let mut visited = std::collections::HashSet::new();
                    queue.push_back((wx, ry, wz, 0));
                    visited.insert((wx, ry, wz));

                    let mut found_log = false;
                    while let Some((bx, by, bz, dist)) = queue.pop_front() {
                        let b = self.chunk_manager.get_block(bx, by, bz);
                        if b == BlockType::OakLog
                            || b == BlockType::BirchLog
                            || b == BlockType::SpruceLog
                        {
                            found_log = true;
                            break;
                        }
                        if dist < 4 {
                            for (dx, dy, dz) in &[
                                (1, 0, 0),
                                (-1, 0, 0),
                                (0, 1, 0),
                                (0, -1, 0),
                                (0, 0, 1),
                                (0, 0, -1),
                            ] {
                                let nx = bx + dx;
                                let ny = by + dy;
                                let nz = bz + dz;
                                let neighbor_b = self.chunk_manager.get_block(nx, ny, nz);
                                let is_leaf = neighbor_b == BlockType::OakLeaves
                                    || neighbor_b == BlockType::BirchLeaves
                                    || neighbor_b == BlockType::SpruceLeaves;
                                if (is_leaf
                                    || neighbor_b == BlockType::OakLog
                                    || neighbor_b == BlockType::BirchLog
                                    || neighbor_b == BlockType::SpruceLog)
                                    && visited.insert((nx, ny, nz))
                                {
                                    queue.push_back((nx, ny, nz, dist + 1));
                                }
                            }
                        }
                    }

                    if !found_log {
                        self.chunk_manager.set_block(wx, ry, wz, BlockType::Air);
                        let mut dirty_chunks = std::collections::HashSet::new();
                        crate::lighting::update_sky_light_after_removed(
                            &mut self.chunk_manager,
                            wx,
                            ry,
                            wz,
                            &mut dirty_chunks,
                        );
                        mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);
                        self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::Mob);
                        self.broadcast_block_change(wx, ry, wz, BlockType::Air);
                    }
                }
            }
        }

        // Cactus damage check
        let player_aabb = self.player_physics.get_aabb();
        let min_x = player_aabb.min.x.floor() as i32;
        let max_x = player_aabb.max.x.floor() as i32;
        let min_y =
            (player_aabb.min.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
        let max_y =
            (player_aabb.max.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
        let min_z = player_aabb.min.z.floor() as i32;
        let max_z = player_aabb.max.z.floor() as i32;

        let mut touching_cactus = false;
        for x in min_x..=max_x {
            for y in min_y..=max_y {
                for z in min_z..=max_z {
                    if self.chunk_manager.get_block(x, y, z) == BlockType::Cactus {
                        let block_aabb = AABB::new(
                            Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5),
                            Vec3::ONE,
                        );
                        if player_aabb.intersects(&block_aabb) {
                            touching_cactus = true;
                        }
                    }
                }
            }
        }

        if touching_cactus {
            self.cactus_damage_timer += dt;
            if self.cactus_damage_timer >= 0.5 {
                self.cactus_damage_timer = 0.0;
                self.take_damage(1.0, DamageSource::Mob);
            }
        } else {
            self.cactus_damage_timer = 0.0;
        }

        // Update player state timers & starvation
        let is_underwater = block_at_eyes == BlockType::Water;
        let respiration_level: u8 = self
            .inventory
            .armor
            .iter()
            .flatten()
            .map(|stack| {
                stack
                    .enchantments
                    .level_of(crate::enchantment::Enchantment::Respiration(1))
            })
            .sum();
        let water_breathing = self.potion_effects.has_water_breathing();
        let oxygen_rate = 1.0 / (1.0 + respiration_level as f32);
        if self.is_authoritative() {
            if let Some((dmg, src)) = self.player_state.update_with_oxygen_rate(
                dt,
                is_underwater && !water_breathing,
                oxygen_rate,
            ) {
                self.take_damage(dmg, src);
            }
        }

        self.total_time += dt;

        if authoritative {
            let hostile_mobs_started = Instant::now();
            if self.difficulty == Difficulty::Peaceful {
                self.entity_manager
                    .entities
                    .retain(|entity| !entity.entity_type.is_hostile());
            } else if self.current_dimension == crate::dimension::Dimension::Overworld {
                crate::mob::spawn_mobs(
                    &mut self.entity_manager,
                    &self.chunk_manager,
                    self.player_physics.position,
                    self.world_time.sky_light_level(),
                    self.total_time,
                );
            }

            if self.difficulty != Difficulty::Peaceful {
                crate::boss::ensure_dimension_entities(
                    self.current_dimension,
                    &mut self.entity_manager,
                    &self.chunk_manager,
                    self.player_physics.position,
                    self.total_time,
                );
                let boss_events = crate::boss::update_dimension_entities(
                    self.current_dimension,
                    &mut self.entity_manager,
                    &self.chunk_manager,
                    self.player_physics.position,
                    dt,
                    self.game_mode,
                );
                self.apply_boss_events(boss_events);
            }

            // Update mobs
            self.update_player_projectiles(dt);
            let is_raining = matches!(
                self.weather.current,
                crate::weather::Weather::Rain | crate::weather::Weather::Thunder
            );
            let mut mob_dirty_meshes = std::collections::HashSet::new();
            let exploded_blocks = crate::mob::update_mobs(
                &mut self.entity_manager,
                &mut self.chunk_manager,
                &mut mob_dirty_meshes,
                &mut self.player_physics,
                &mut self.player_state,
                self.game_mode,
                self.world_time.sky_light_level(),
                is_raining,
                dt,
                &mut self.audio_manager,
                right,
                self.potion_effects.has_invisibility(),
                crate::enchantment::protection_multiplier(&self.inventory.armor, false),
                authoritative,
            );
            self.invalidate_chunk_meshes(mob_dirty_meshes, DependencyReason::Mob);
            for (x, y, z) in exploded_blocks {
                self.broadcast_block_change(x, y, z, BlockType::Air);
            }
            self.perf_recorder.record(
                crate::perf::ScopeId::HostileMobs,
                hostile_mobs_started.elapsed(),
            );

            // Update passive mobs
            let passive_mobs_started = Instant::now();
            let mut passive_dirty_meshes = std::collections::HashSet::new();
            let grazed_blocks = crate::passive_mob::update_passive_mobs(
                &mut self.entity_manager,
                &mut self.chunk_manager,
                &mut passive_dirty_meshes,
                &self.player_physics,
                &mut self.inventory,
                self.game_mode,
                dt,
                self.total_time,
                authoritative,
            );
            self.invalidate_chunk_meshes(passive_dirty_meshes, DependencyReason::Mob);
            for (x, y, z) in grazed_blocks {
                self.broadcast_block_change(x, y, z, BlockType::Dirt);
            }

            // Spawn passive mobs (daytime spawn)
            if self.current_dimension == crate::dimension::Dimension::Overworld {
                crate::passive_mob::spawn_passive_mobs(
                    &mut self.entity_manager,
                    &self.chunk_manager,
                    self.player_physics.position,
                    self.world_time.sky_light_level(),
                    self.total_time,
                );
            }
            self.perf_recorder.record(
                crate::perf::ScopeId::PassiveMobs,
                passive_mobs_started.elapsed(),
            );
        }

        self.broadcast_authoritative_replication(dt);

        self.perf_recorder.record(
            crate::perf::ScopeId::WorldTick,
            world_tick_started.elapsed(),
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

        self.update_network_time_sync(dt);

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
        if self.third_person {
            let forward = Vec3::new(
                self.camera.yaw.cos() * self.camera.pitch.cos(),
                self.camera.pitch.sin(),
                self.camera.yaw.sin() * self.camera.pitch.cos(),
            )
            .normalize_or_zero();
            self.camera.position =
                interp_player_pos + Vec3::new(0.0, eye_height, 0.0) - forward * 4.0;
        } else {
            self.camera.position = interp_player_pos + Vec3::new(0.0, eye_height, 0.0);
        }
        let is_underwater = self.chunk_manager.get_block(
            self.camera.position.x.floor() as i32,
            self.camera.position.y.floor() as i32,
            self.camera.position.z.floor() as i32,
        ) == BlockType::Water;

        self.camera_uniform.update_view_proj(
            &self.camera,
            self.config.width as f32 / self.config.height as f32,
            self.chunk_manager.render_distance as u32,
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

                if block != BlockType::Air && block.properties().hardness >= 0.0 {
                    if self.mining_target != Some(target) {
                        self.mining_target = Some(target);
                        self.mining_progress = 0.0;
                    }
                    let mining_time = self.calculate_mining_time(block);
                    if mining_time <= 0.0 {
                        self.break_block(target);
                        self.mining_target = None;
                        self.mining_progress = 0.0;
                    } else {
                        self.mining_progress += dt / mining_time;
                        if self.mining_progress >= 1.0 {
                            let pos = target;
                            self.break_block(pos);
                            self.mining_target = None;
                            self.mining_progress = 0.0;
                        }
                    }
                } else {
                    self.mining_target = None;
                    self.mining_progress = 0.0;
                }
            } else {
                self.mining_target = None;
                self.mining_progress = 0.0;
            }
        } else {
            self.mining_target = None;
            self.mining_progress = 0.0;
        }

        self.perf_recorder
            .record(crate::perf::ScopeId::Lighting, self.lighting_time_frame);
    }

    pub fn update(&mut self, dt: f32) {
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
        self.process_join_catchups();
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

    fn update_weather_effects(&mut self, dt: f32, lightning_due: bool) {
        use crate::weather::Precipitation;

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
            if surface_y >= CHUNK_HEIGHT as i32 - 2 {
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

        let accumulation_steps = if self.is_authoritative() {
            self.weather.take_snow_accumulation_steps(dt)
        } else {
            0
        };
        for _ in 0..accumulation_steps * 6 {
            let wx = player_x + self.weather.authority_random_offset(24);
            let wz = player_z + self.weather.authority_random_offset(24);
            if self.weather.precipitation_at(wx, wz) != Precipitation::Snow {
                continue;
            }
            let Some(surface_y) = self.surface_height(wx, wz) else {
                continue;
            };
            let target_y = surface_y + 1;
            if target_y >= CHUNK_HEIGHT as i32
                || self.chunk_manager.get_block(wx, target_y, wz) != BlockType::Air
            {
                continue;
            }
            let support = self.chunk_manager.get_block(wx, surface_y, wz);
            if support.properties().is_solid
                && !matches!(support, BlockType::Water | BlockType::Lava | BlockType::Ice)
            {
                self.apply_weather_block_change(wx, target_y, wz, BlockType::SnowLayer);
            }
        }

        if lightning_due && self.is_authoritative() {
            self.strike_lightning();
        }
    }

    fn surface_height(&self, wx: i32, wz: i32) -> Option<i32> {
        let ((cx, cz), (bx, _, bz)) = self.chunk_manager.world_to_local(wx, 0, wz)?;
        self.chunk_manager
            .chunks
            .get(&(cx, cz))
            .map(|chunk| chunk.heightmap[bx][bz] as i32)
    }

    fn strike_lightning(&mut self) {
        use crate::entity::EntityType;

        if !self.is_authoritative() {
            return;
        }
        let player_pos = self.player_physics.position;
        let living_types = [
            EntityType::Zombie,
            EntityType::Skeleton,
            EntityType::Creeper,
            EntityType::Pig,
            EntityType::Cow,
            EntityType::Sheep,
            EntityType::Chicken,
        ];
        let living_target = self
            .entity_manager
            .query_radius_types(player_pos, 32.0, &living_types)
            .filter(|entity| entity.health > 0.0)
            .min_by(|a, b| {
                a.position
                    .distance_squared(player_pos)
                    .total_cmp(&b.position.distance_squared(player_pos))
            })
            .map(|entity| entity.position);

        let (strike_x, strike_z) = if let Some(target) = living_target {
            (target.x.floor() as i32, target.z.floor() as i32)
        } else {
            (
                player_pos.x.floor() as i32 + self.weather.authority_random_offset(30),
                player_pos.z.floor() as i32 + self.weather.authority_random_offset(30),
            )
        };
        let Some(surface_y) = self.surface_height(strike_x, strike_z) else {
            return;
        };
        let strike = crate::network::protocol::LightningStrike {
            x: strike_x,
            y: surface_y + 1,
            z: strike_z,
            visual_seed: self.weather.authority_random_seed(),
        };
        self.network.broadcast_lightning_strike(strike);
        self.apply_lightning_strike(strike);
    }

    fn apply_lightning_strike(&mut self, strike: crate::network::protocol::LightningStrike) {
        let player_pos = self.player_physics.position;
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

        if self.is_authoritative() {
            for entity in &mut self.entity_manager.entities {
                if entity.entity_type == crate::entity::EntityType::RemotePlayer {
                    continue;
                }
                let horizontal = glam::Vec2::new(
                    entity.position.x - strike_pos.x,
                    entity.position.z - strike_pos.z,
                )
                .length();
                if entity.health > 0.0 && horizontal <= 3.5 {
                    entity.health -= 10.0;
                    entity.fire_aspect_timer = entity.fire_aspect_timer.max(5.0);
                }
            }
            let player_horizontal =
                glam::Vec2::new(player_pos.x - strike_pos.x, player_pos.z - strike_pos.z).length();
            if player_horizontal <= 3.5 {
                self.take_damage(10.0, DamageSource::Lightning);
            }
        }

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

        let fire_y = strike.y;
        let support_y = fire_y - 1;
        let support = self.chunk_manager.get_block(strike.x, support_y, strike.z);
        if self.is_authoritative()
            && fire_y < CHUNK_HEIGHT as i32
            && support.properties().is_solid
            && !matches!(
                support,
                BlockType::Water | BlockType::Lava | BlockType::Ice | BlockType::Snow
            )
            && self.chunk_manager.get_block(strike.x, fire_y, strike.z) == BlockType::Air
        {
            self.apply_weather_block_change(strike.x, fire_y, strike.z, BlockType::Fire);
        }
    }

    fn apply_weather_block_change(&mut self, wx: i32, wy: i32, wz: i32, block: BlockType) {
        if !self.is_authoritative() {
            return;
        }
        let old = self.chunk_manager.get_block(wx, wy, wz);
        if old == block {
            return;
        }
        self.chunk_manager.set_block(wx, wy, wz, block);
        self.redstone.on_block_changed(
            &self.chunk_manager,
            (wx, wy, wz),
            crate::redstone::Direction::North,
        );

        let old_properties = old.properties();
        let new_properties = block.properties();
        let mut dirty_chunks = std::collections::HashSet::new();
        if old_properties.is_solid != new_properties.is_solid {
            if new_properties.is_solid {
                crate::lighting::update_sky_light_after_placed(
                    &mut self.chunk_manager,
                    wx,
                    wy,
                    wz,
                    &mut dirty_chunks,
                );
            } else {
                crate::lighting::update_sky_light_after_removed(
                    &mut self.chunk_manager,
                    wx,
                    wy,
                    wz,
                    &mut dirty_chunks,
                );
            }
        }
        if old_properties.light_emission != new_properties.light_emission {
            crate::lighting::update_block_light_after_removed(
                &mut self.chunk_manager,
                wx,
                wy,
                wz,
                old_properties.light_emission,
                &mut dirty_chunks,
            );
            if new_properties.light_emission > 0 {
                crate::lighting::update_block_light_after_placed(
                    &mut self.chunk_manager,
                    wx,
                    wy,
                    wz,
                    new_properties.light_emission,
                    &mut dirty_chunks,
                );
            }
        }
        mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);
        self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::Weather);
        self.invalidate_block_mesh_dependencies(wx, wy, wz, DependencyReason::Weather);
        // Fan weather-driven block placement out to connected clients.
        self.broadcast_block_change(wx, wy, wz, block);
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

    pub fn calculate_mining_time(&self, block: BlockType) -> f32 {
        let hardness = block.properties().hardness;
        if hardness < 0.0 {
            return f32::MAX; // Unbreakable (e.g. bedrock)
        }

        let held_stack = self.inventory.hotbar[self.inventory.selected];
        let held_item = held_stack.map(|s| s.item).unwrap_or(Item::Air);
        let preferred = block.preferred_tool();

        let mut speed_multiplier = 1.0;
        let mut matching_tool = false;

        if let Some(tool_prop) = held_item.tool_properties() {
            if tool_prop.tool_type == preferred && preferred != ToolType::None {
                speed_multiplier = tool_prop.mining_speed;
                matching_tool = true;
            }
        }

        let base_time = if matching_tool || preferred == ToolType::None {
            hardness * 1.5
        } else {
            hardness * 5.0
        };

        let enchantment_multiplier = held_stack
            .map(|stack| crate::enchantment::mining_speed_multiplier(&stack.enchantments))
            .unwrap_or(1.0);
        base_time / (speed_multiplier * enchantment_multiplier)
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

    fn apply_redstone_update(&mut self, update: crate::redstone::RedstoneUpdate) {
        let mut dirty_chunks = std::collections::HashSet::new();
        let mut broadcast: Vec<((i32, i32, i32), BlockType)> = Vec::new();
        for mutation in update.mutations {
            let (wx, wy, wz) = mutation.pos;
            let old_properties = mutation.old_block.properties();
            let new_properties = mutation.new_block.properties();

            if old_properties.is_solid != new_properties.is_solid {
                if new_properties.is_solid {
                    crate::lighting::update_sky_light_after_placed(
                        &mut self.chunk_manager,
                        wx,
                        wy,
                        wz,
                        &mut dirty_chunks,
                    );
                } else {
                    crate::lighting::update_sky_light_after_removed(
                        &mut self.chunk_manager,
                        wx,
                        wy,
                        wz,
                        &mut dirty_chunks,
                    );
                }
            }
            if old_properties.light_emission != new_properties.light_emission {
                crate::lighting::update_block_light_after_removed(
                    &mut self.chunk_manager,
                    wx,
                    wy,
                    wz,
                    old_properties.light_emission,
                    &mut dirty_chunks,
                );
                if new_properties.light_emission > 0 {
                    crate::lighting::update_block_light_after_placed(
                        &mut self.chunk_manager,
                        wx,
                        wy,
                        wz,
                        new_properties.light_emission,
                        &mut dirty_chunks,
                    );
                }
            }
            mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);
            broadcast.push(((wx, wy, wz), mutation.new_block));
        }

        self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::Redstone);

        // Fan the redstone-driven block mutations out to connected clients.
        for ((x, y, z), block) in broadcast {
            self.broadcast_block_change(x, y, z, block);
        }

        for action in update.actions {
            match action {
                crate::redstone::RedstoneAction::Explode { pos } => {
                    let center =
                        Vec3::new(pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5);
                    let mut dirty_meshes = std::collections::HashSet::new();
                    let removed = crate::mob::explode(
                        center,
                        4.0,
                        &mut self.chunk_manager,
                        &mut dirty_meshes,
                        &mut self.player_physics,
                        &mut self.player_state,
                        true,
                        self.game_mode,
                        1.0,
                    );
                    self.invalidate_chunk_meshes(dirty_meshes, DependencyReason::Redstone);
                    for (x, y, z) in removed {
                        self.broadcast_block_change(x, y, z, BlockType::Air);
                    }
                    self.audio_manager
                        .play_sound(crate::audio::SoundId::Explosion);
                }
                crate::redstone::RedstoneAction::Dispense {
                    pos,
                    facing,
                    dropper,
                } => {
                    let delta = facing.delta();
                    let spawn_pos = Vec3::new(
                        pos.0 as f32 + 0.5 + delta.0 as f32 * 0.7,
                        pos.1 as f32 + 0.5,
                        pos.2 as f32 + 0.5 + delta.2 as f32 * 0.7,
                    );
                    if dropper {
                        self.spawn_dropped_item(Item::Redstone, spawn_pos);
                    } else {
                        let id = self
                            .entity_manager
                            .spawn(crate::entity::EntityType::Arrow, spawn_pos);
                        if let Some(arrow) = self
                            .entity_manager
                            .entities
                            .iter_mut()
                            .find(|entity| entity.id == id)
                        {
                            arrow.velocity = Vec3::new(delta.0 as f32, 0.0, delta.2 as f32) * 18.0;
                            arrow.friendly_projectile = true;
                            arrow.projectile_damage = 4.0;
                        }
                        self.audio_manager
                            .play_sound(crate::audio::SoundId::ArrowShoot);
                    }
                }
                crate::redstone::RedstoneAction::PlayNote { pos, note } => {
                    let sound_pos =
                        Vec3::new(pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5);
                    let listener_right =
                        Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos())
                            .normalize_or_zero();
                    self.audio_manager.play_sound_3d(
                        crate::audio::SoundId::Note(note),
                        sound_pos,
                        self.camera.position,
                        listener_right,
                    );
                }
            }
        }

        if update.propagation_overflowed {
            eprintln!("[Redstone] propagation pass limit reached; continuing next tick");
        }
    }

    /// Host-side canonical block mutation that also fans the result out to every
    /// connected client. Used for client-initiated changes (relayed through the
    /// server) and any host-derived mutation that should be visible to peers.
    ///
    /// This performs the full sequence the architecture mandates: `set_block`,
    /// sky/block light update, mesh-dependency invalidation, and redstone
    /// component rescan. It deliberately does **not** spawn drops, play sounds,
    /// grant XP, or trigger advancements - those are local gameplay reactions
    /// tied to the *player's* action, not to a relayed remote request.
    pub fn set_block_and_broadcast(
        &mut self,
        requester: crate::network::protocol::PlayerId,
        x: i32,
        y: i32,
        z: i32,
        block_wire: u32,
        state: u8,
    ) {
        let lighting_started = Instant::now();
        let block = match BlockType::from_wire(block_wire) {
            Some(b) => b,
            None => return,
        };
        if !validate_remote_block_request(&self.remote_players, requester, (x, y, z))
            || !self.can_place_block_at(x, y, z, block)
        {
            return;
        }
        let Some(((cx, cz), _)) = self.chunk_manager.world_to_local(x, y, z) else {
            return;
        };
        if !self.chunk_manager.chunks.contains_key(&(cx, cz)) {
            return;
        }
        if !self
            .chunk_manager
            .can_place_block_with_support(block, x, y, z)
        {
            return;
        }
        let prev = self.chunk_manager.get_block(x, y, z);
        let prev_state = self.chunk_manager.get_block_state(x, y, z);
        if prev == block && prev_state == state {
            // Echo the authoritative value to correct a requesting client's
            // prediction, but do not mark an unchanged chunk as mutated.
            let cx = x.div_euclid(CHUNK_WIDTH as i32);
            let cz = z.div_euclid(CHUNK_DEPTH as i32);
            let revision = self
                .mutation_revisions
                .latest(self.current_dimension, cx, cz);
            self.network.broadcast_block_change(
                self.current_dimension,
                revision,
                x,
                y,
                z,
                block_wire,
                state,
            );
            return;
        }
        let Some(mut dirty_chunks) =
            apply_synced_block_change(&mut self.chunk_manager, x, y, z, block, state)
        else {
            return;
        };
        self.redstone.on_block_changed(
            &self.chunk_manager,
            (x, y, z),
            crate::redstone::Direction::North,
        );
        self.check_and_break_unsupported_above(x, y, z, &mut dirty_chunks);
        self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::Block);
        self.broadcast_block_change(x, y, z, block);
        let lighting_elapsed = lighting_started.elapsed();
        self.lighting_time_frame += lighting_elapsed;
        self.lighting_scopes_frame.record(
            crate::perf::LightingSource::Block as usize,
            lighting_elapsed,
        );
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
                .insert((x, y, z), (revision, block_wire, state));
            return;
        }
        let Some(dirty_chunks) =
            apply_synced_block_change(&mut self.chunk_manager, x, y, z, block, state)
        else {
            return;
        };
        self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::Network);
    }

    /// Client-side application of a full chunk payload sent by the host during
    /// mid-game join catch-up. The payload uses the same Zlib-compressed layout
    /// as `save.rs::ChunkSaveData`. If the chunk is not loaded yet, the payload
    /// is buffered and applied once `update_chunks` loads that coordinate.
    fn apply_remote_chunk_data(
        &mut self,
        dimension_wire: u8,
        cx: i32,
        cz: i32,
        revision: u64,
        blocks: Vec<u8>,
        block_states: Vec<u8>,
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
        if let Some(chunk) = self.chunk_manager.chunks.get_mut(&(cx, cz)) {
            Self::restore_chunk_payload(chunk, &blocks, &block_states);
            self.invalidate_chunk_mesh((cx, cz), DependencyReason::Network);
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
        } else {
            // Chunk not streamed in yet; buffer for deferred application.
            let should_replace = self
                .pending_chunk_payloads
                .get(&(cx, cz))
                .map_or(true, |(existing_revision, _, _)| {
                    revision >= *existing_revision
                });
            if should_replace {
                self.pending_chunk_payloads
                    .insert((cx, cz), (revision, blocks, block_states));
            }
        }
    }

    /// Decode a `ChunkSaveData`-style compressed payload into an existing
    /// chunk. Reused by both the save loader and the network catch-up path so
    /// the wire format stays identical to the on-disk format.
    fn restore_chunk_payload(chunk: &mut crate::world::Chunk, blocks: &[u8], block_states: &[u8]) {
        let save_data = crate::save::ChunkSaveData {
            chunk_x: chunk.chunk_x,
            chunk_z: chunk.chunk_z,
            blocks: blocks.to_vec(),
            sky_light: Vec::new(),
            block_light: Vec::new(),
            fluid_levels: Vec::new(),
            redstone_metadata: Vec::new(),
            block_states: block_states.to_vec(),
            mutation_revision: 0,
        };
        save_data.restore_to_chunk(chunk);
    }

    pub fn break_block(&mut self, pos: glam::Vec3) {
        let lighting_started = Instant::now();
        let wx = pos.x as i32;
        let wy = pos.y as i32;
        let wz = pos.z as i32;
        let old_block = self.chunk_manager.get_block(wx, wy, wz);
        if old_block == BlockType::Air {
            return;
        }
        if !self.is_authoritative() {
            self.network
                .request_block_change(wx, wy, wz, BlockType::Air as u32);
            return;
        }

        let old_state_raw = self.chunk_manager.get_block_state(wx, wy, wz);
        let old_state = crate::world::BlockState::decode(old_state_raw);

        self.chunk_manager.set_block(wx, wy, wz, BlockType::Air);
        self.redstone.on_block_changed(
            &self.chunk_manager,
            (wx, wy, wz),
            crate::redstone::Direction::North,
        );
        println!("[Debug] Block mined at ({}, {}, {})", wx, wy, wz);

        let sound_pos = glam::Vec3::new(wx as f32 + 0.5, wy as f32 + 0.5, wz as f32 + 0.5);
        let listener_right =
            glam::Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos()).normalize_or_zero();
        if let Some(mat) = old_block.sound_material() {
            self.audio_manager.play_sound_3d(
                crate::audio::SoundId::BlockBreak(mat),
                sound_pos,
                self.camera.position,
                listener_right,
            );
        }

        // Spawn block-break debris particles (15-25 small quads textured from
        // the broken block's atlas tile).
        {
            let mut rng = (wx as u32)
                .wrapping_mul(2654435761)
                .wrapping_add(wy as u32)
                .wrapping_mul(40503)
                .wrapping_add(wz as u32)
                .wrapping_add(self.total_time.to_bits());
            let count = 15 + (rng % 11) as usize;
            crate::particles::spawn_block_debris(
                &mut self.particles,
                sound_pos,
                old_block,
                count,
                &mut rng,
            );
        }

        let held_stack = self.inventory.hotbar[self.inventory.selected];
        let rewards = calculate_block_break_rewards(
            old_block,
            (wx, wy, wz),
            held_stack.as_ref(),
            self.game_mode,
        );

        for drop in rewards.drops {
            self.spawn_dropped_item(drop.item, sound_pos);
        }
        if rewards.xp > 0 {
            self.player_state.add_experience(rewards.xp);
        }
        if rewards.exhaustion > 0.0 {
            self.player_state.add_exhaustion(rewards.exhaustion);
        }
        if rewards.tool_damaged {
            self.damage_selected_tool(
                (wx as u32) ^ (wy as u32).rotate_left(11) ^ (wz as u32).rotate_left(22),
            );
        }

        // recalculate lighting and redraw chunk
        let mut dirty_chunks = std::collections::HashSet::new();
        crate::lighting::update_sky_light_after_removed(
            &mut self.chunk_manager,
            wx,
            wy,
            wz,
            &mut dirty_chunks,
        );
        crate::lighting::update_block_light_after_removed(
            &mut self.chunk_manager,
            wx,
            wy,
            wz,
            old_block.properties().light_emission,
            &mut dirty_chunks,
        );

        mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);

        if old_block == BlockType::OakDoor {
            let other_y = if old_state.is_top { wy - 1 } else { wy + 1 };
            if self.chunk_manager.get_block(wx, other_y, wz) == BlockType::OakDoor {
                self.chunk_manager
                    .set_block(wx, other_y, wz, BlockType::Air);
                crate::lighting::update_sky_light_after_removed(
                    &mut self.chunk_manager,
                    wx,
                    other_y,
                    wz,
                    &mut dirty_chunks,
                );
                mark_block_mesh_dependencies(&mut dirty_chunks, wx, other_y);
                self.broadcast_block_change(wx, other_y, wz, BlockType::Air);
            }
        }

        self.check_and_break_unsupported_above(wx, wy, wz, &mut dirty_chunks);

        self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::BreakPlace);

        // Fan the authoritative break out to connected clients.
        self.broadcast_block_change(wx, wy, wz, BlockType::Air);
        let lighting_elapsed = lighting_started.elapsed();
        self.lighting_time_frame += lighting_elapsed;
        self.lighting_scopes_frame.record(
            crate::perf::LightingSource::Block as usize,
            lighting_elapsed,
        );
    }

    pub fn handle_client_block_action(
        &mut self,
        requester_id: crate::network::protocol::PlayerId,
        action: crate::network::protocol::Action,
        x: i32,
        y: i32,
        z: i32,
        block_wire: u32,
        held_item_wire: Option<crate::network::protocol::ItemWire>,
    ) {
        if !validate_remote_block_request(&self.remote_players, requester_id, (x, y, z)) {
            self.send_block_action_result(requester_id, x, y, z, false, false, vec![]);
            return;
        }

        let Some(((cx, cz), _)) = self.chunk_manager.world_to_local(x, y, z) else {
            self.send_block_action_result(requester_id, x, y, z, false, false, vec![]);
            return;
        };
        if !self.chunk_manager.chunks.contains_key(&(cx, cz)) {
            self.send_block_action_result(requester_id, x, y, z, false, false, vec![]);
            return;
        }

        match action {
            crate::network::protocol::Action::Break => {
                let old_block = self.chunk_manager.get_block(x, y, z);
                if old_block == BlockType::Air || old_block == BlockType::Bedrock {
                    self.send_block_action_result(requester_id, x, y, z, false, false, vec![]);
                    return;
                }

                let mut dirty_chunks = std::collections::HashSet::new();
                self.chunk_manager.set_block(x, y, z, BlockType::Air);
                self.redstone.on_block_changed(
                    &self.chunk_manager,
                    (x, y, z),
                    crate::redstone::Direction::North,
                );
                crate::lighting::update_sky_light_after_removed(
                    &mut self.chunk_manager,
                    x,
                    y,
                    z,
                    &mut dirty_chunks,
                );
                crate::lighting::update_block_light_after_removed(
                    &mut self.chunk_manager,
                    x,
                    y,
                    z,
                    old_block.properties().light_emission,
                    &mut dirty_chunks,
                );
                mark_block_mesh_dependencies(&mut dirty_chunks, x, z);
                self.check_and_break_unsupported_above(x, y, z, &mut dirty_chunks);
                self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::BreakPlace);

                self.broadcast_block_change(x, y, z, BlockType::Air);

                let held_stack = held_item_wire.and_then(|w| w.to_stack());
                let rewards = calculate_block_break_rewards(
                    old_block,
                    (x, y, z),
                    held_stack.as_ref(),
                    self.game_mode,
                );

                let sound_pos = glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                for drop in &rewards.drops {
                    self.spawn_dropped_item(drop.item, sound_pos);
                }

                let drops_wire = rewards
                    .drops
                    .iter()
                    .map(crate::network::protocol::ItemWire::from_stack)
                    .collect();
                self.send_block_action_result(requester_id, x, y, z, true, false, drops_wire);
            }
            crate::network::protocol::Action::Place => {
                let block = BlockType::from_u8(block_wire as u8);
                if block == BlockType::Air {
                    self.send_block_action_result(requester_id, x, y, z, false, false, vec![]);
                    return;
                }

                if !self.can_place_block_at(x, y, z, block)
                    || !self
                        .chunk_manager
                        .can_place_block_with_support(block, x, y, z)
                {
                    self.send_block_action_result(requester_id, x, y, z, false, false, vec![]);
                    return;
                }

                let mut dirty_chunks = std::collections::HashSet::new();
                self.chunk_manager.set_block(x, y, z, block);
                self.redstone.on_block_changed(
                    &self.chunk_manager,
                    (x, y, z),
                    crate::redstone::Direction::North,
                );
                let properties = block.properties();
                if properties.is_solid {
                    crate::lighting::update_sky_light_after_placed(
                        &mut self.chunk_manager,
                        x,
                        y,
                        z,
                        &mut dirty_chunks,
                    );
                }
                if properties.light_emission > 0 {
                    crate::lighting::update_block_light_after_placed(
                        &mut self.chunk_manager,
                        x,
                        y,
                        z,
                        properties.light_emission,
                        &mut dirty_chunks,
                    );
                }
                mark_block_mesh_dependencies(&mut dirty_chunks, x, z);
                self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::BreakPlace);

                self.broadcast_block_change(x, y, z, block);

                let consumed = self.game_mode == GameMode::Survival;
                self.send_block_action_result(requester_id, x, y, z, true, consumed, vec![]);
            }
            _ => {
                self.send_block_action_result(requester_id, x, y, z, false, false, vec![]);
            }
        }
    }

    fn send_block_action_result(
        &self,
        to: crate::network::protocol::PlayerId,
        x: i32,
        y: i32,
        z: i32,
        success: bool,
        consumed_item: bool,
        drops: Vec<crate::network::protocol::ItemWire>,
    ) {
        if let NetworkHandle::Host { host_to_server, .. } = &self.network {
            let _ = host_to_server.tracked_send(
                crate::network::server::HostToServer::SendBlockActionResult {
                    to,
                    x,
                    y,
                    z,
                    success,
                    consumed_item,
                    drops,
                },
            );
        }
    }

    /// Spawn a `DroppedItem` entity in the world carrying the given `Item`.
    /// The item is launched with a small random upward velocity and given a
    /// brief pickup cooldown so it can't be instantly re-collected.
    pub fn spawn_dropped_item(&mut self, item: crate::inventory::Item, pos: glam::Vec3) {
        let _ = spawn_dropped_item_entity(
            &mut self.entity_manager,
            item,
            pos,
            self.total_time.to_bits(),
        );
    }

    fn store_or_drop_generated_item(&mut self, item: Item, position: Vec3) {
        let _ = store_or_drop_generated_item(
            &mut self.inventory,
            &mut self.entity_manager,
            item,
            position,
            self.total_time.to_bits(),
        );
    }

    fn settle_standard_player_kill(&mut self, kill: PlayerKill, looting: u8) {
        if self.game_mode != GameMode::Survival {
            return;
        }

        let rewards = standard_player_kill_rewards(kill, looting);
        for item in rewards.items {
            self.store_or_drop_generated_item(item, kill.position);
        }
        self.player_state.add_experience(rewards.experience);
    }

    /// Throw `count` of `item` out from the player's eye in the camera look
    /// direction as a single `DroppedItem` entity. The thrower cannot
    /// instantly re-collect it thanks to a longer pickup cooldown.
    fn throw_dropped_item(&mut self, item: Item, count: u32) {
        if item == Item::Air || count == 0 {
            return;
        }
        let dir = Vec3::new(
            self.camera.yaw.cos() * self.camera.pitch.cos(),
            self.camera.pitch.sin(),
            self.camera.yaw.sin() * self.camera.pitch.cos(),
        )
        .normalize_or_zero();
        let spawn_pos = self.player_physics.position + Vec3::new(0.0, 1.5, 0.0) + dir * 0.5;
        self.entity_manager
            .spawn(crate::entity::EntityType::DroppedItem, spawn_pos);
        if let Some(entity) = self.entity_manager.entities.last_mut() {
            entity.dropped_item = Some(item);
            entity.dropped_count = count;
            entity.velocity = dir * 4.0 + Vec3::new(0.0, 1.5, 0.0);
            entity.pickup_cooldown = 1.0;
        }
    }

    /// Q pressed in the world: throw the selected hotbar item. One item is
    /// thrown, or the whole stack when `whole_stack` (Shift) is held.
    pub fn drop_held_item(&mut self, whole_stack: bool) {
        let selected = self.inventory.selected;
        let Some(stack) = self.inventory.hotbar[selected] else {
            return;
        };
        let count = if whole_stack { stack.count } else { 1 };
        self.throw_dropped_item(stack.item, count);
        if stack.count > count {
            self.inventory.hotbar[selected] = Some(ItemStack {
                count: stack.count - count,
                ..stack
            });
        } else {
            self.inventory.hotbar[selected] = None;
        }
    }

    /// Q pressed while the inventory is open: throw the item under the mouse
    /// cursor (or the stack being dragged with the cursor). One item is
    /// thrown, or the whole stack when `whole_stack` (Shift) is held.
    pub fn drop_hovered_item(&mut self, whole_stack: bool) {
        // A stack dragged with the cursor takes precedence, matching the
        // vanilla behaviour of throwing what is held in the hand.
        if let Some(dragged) = self.inventory.dragged {
            let count = if whole_stack { dragged.count } else { 1 };
            self.throw_dropped_item(dragged.item, count);
            if dragged.count > count {
                self.inventory.dragged = Some(ItemStack {
                    count: dragged.count - count,
                    ..dragged
                });
            } else {
                self.inventory.dragged = None;
            }
            return;
        }

        let mouse_x = self.mouse_ndc[0];
        let mouse_y = self.mouse_ndc[1];
        let hovered_slot = self
            .get_inventory_slots()
            .into_iter()
            .find(|&(_, x0, x1, y0, y1)| {
                mouse_x >= x0 && mouse_x <= x1 && mouse_y >= y0 && mouse_y <= y1
            });
        let Some((slot_type, _, _, _, _)) = hovered_slot else {
            return;
        };
        match slot_type {
            // The Creative catalog is a virtual infinite supply, and output
            // slots are take-out-only: none of them can be thrown from.
            SlotType::Creative(_) | SlotType::CraftOutput | SlotType::AnvilOutput => return,
            _ => {}
        }
        let Some(stack) = self.get_item_at_slot(slot_type) else {
            return;
        };
        let count = if whole_stack { stack.count } else { 1 };
        self.throw_dropped_item(stack.item, count);
        if stack.count > count {
            self.set_item_at_slot(
                slot_type,
                Some(ItemStack {
                    count: stack.count - count,
                    ..stack
                }),
            );
        } else {
            self.set_item_at_slot(slot_type, None);
        }

        // Keep derived state consistent when throwing out of an input slot.
        if let SlotType::CraftInput(_) = slot_type {
            let grid_size = if self.inventory.is_table_open { 3 } else { 2 };
            self.inventory.craft_output = self
                .recipe_manager
                .match_recipe(&self.inventory.craft_input, grid_size);
        }
        self.refresh_workstations();
    }

    fn update_player_projectiles(&mut self, dt: f32) {
        let mut player_kills = Vec::new();
        let mut splashes = Vec::new();
        for projectile in &mut self.entity_manager.entities {
            if projectile.entity_type != crate::entity::EntityType::SplashPotion {
                continue;
            }
            projectile.update_physics(dt, &self.chunk_manager);
            projectile.life_time -= dt;
            let pos = projectile.position;
            let hit_block = self
                .chunk_manager
                .get_block(
                    pos.x.floor() as i32,
                    pos.y.floor() as i32,
                    pos.z.floor() as i32,
                )
                .properties()
                .is_solid;
            if hit_block || projectile.life_time <= 0.0 {
                if let Some(potion) = projectile.potion {
                    splashes.push((pos, potion));
                }
                projectile.health = -1.0;
            }
        }

        for (position, potion) in splashes {
            if position.distance(self.player_physics.position) <= 4.0 {
                let healing = self.potion_effects.apply(potion);
                self.player_state.health =
                    (self.player_state.health + healing).min(self.player_state.max_health);
            }
            let ids: Vec<u64> = self
                .entity_manager
                .query_radius(position, 4.0)
                .map(|e| e.id)
                .collect();
            for id in ids {
                let Some(entity) = self.entity_manager.get_by_id_mut(id) else {
                    continue;
                };
                if let Some(kill) = apply_player_splash_effect(entity, potion) {
                    player_kills.push(kill);
                }
            }
        }

        let mut hits = Vec::new();
        let projectile_ids: Vec<u64> = self
            .entity_manager
            .get_entities_by_type(crate::entity::EntityType::Arrow)
            .filter(|p| p.friendly_projectile)
            .map(|p| p.id)
            .collect();
        for projectile_id in projectile_ids {
            let Some(projectile) = self.entity_manager.get_by_id(projectile_id) else {
                continue;
            };
            let target_ids: Vec<u64> = self
                .entity_manager
                .query_radius(projectile.position, 2.0)
                .map(|t| t.id)
                .collect();
            for target_id in target_ids {
                let Some(target) = self.entity_manager.get_by_id(target_id) else {
                    continue;
                };
                if target.id != projectile.id
                    && target.is_player_projectile_target()
                    && projectile.get_aabb().intersects(&target.get_aabb())
                {
                    hits.push((projectile.id, target.id, projectile.projectile_damage));
                    break;
                }
            }
        }
        for (projectile_id, target_id, damage) in hits {
            if let Some(target) = self.entity_manager.get_by_id_mut(target_id) {
                if let Some(kill) = apply_player_projectile_damage(target, damage) {
                    player_kills.push(kill);
                }
            }
            if let Some(projectile) = self.entity_manager.get_by_id_mut(projectile_id) {
                projectile.health = -1.0;
            }
        }
        for kill in player_kills {
            self.settle_standard_player_kill(kill, 0);
        }
        self.entity_manager.retain(|entity| {
            entity.health >= 0.0
                || matches!(
                    entity.entity_type,
                    crate::entity::EntityType::Blaze
                        | crate::entity::EntityType::Piglin
                        | crate::entity::EntityType::Husk
                        | crate::entity::EntityType::Shulker
                        | crate::entity::EntityType::EnderDragon
                        | crate::entity::EntityType::Wither
                        | crate::entity::EntityType::EndCrystal
                        | crate::entity::EntityType::RemotePlayer
                )
        });
    }

    pub fn take_damage(&mut self, amount: f32, source: DamageSource) {
        if !self.is_authoritative() || self.game_mode == GameMode::Creative {
            return;
        }

        let can_damage = !self.player_state.is_dead && self.player_state.invulnerable_time <= 0.0;
        let reduced = amount
            * crate::enchantment::protection_multiplier(
                &self.inventory.armor,
                source == DamageSource::Fall,
            );
        let died = self.player_state.take_damage(reduced, source);

        if can_damage {
            if died {
                self.player_physics.set_flying(false);
                self.jump_taps.reset();
                self.audio_manager
                    .play_sound(crate::audio::SoundId::PlayerDeath);
                println!("[Debug] Player died due to: {:?}", source);
                self.inventory.clear();

                self.clear_movement_input();
                self.sync_cursor_mode();
            } else {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::PlayerHurt);
            }
        }
    }

    pub fn respawn(&mut self) {
        self.player_physics.set_flying(false);
        self.jump_taps.reset();
        if self.current_dimension != crate::dimension::Dimension::Overworld {
            self.switch_dimension(crate::dimension::Dimension::Overworld);
        }
        // Reset player physics position to spawn point: (8.0, 80.0, 8.0)
        self.player_physics.position = glam::Vec3::new(8.0, 80.0, 8.0);
        self.player_physics.velocity = glam::Vec3::ZERO;
        self.player_physics.on_ground = false;
        self.player_physics.highest_y = 80.0;

        self.player_state.reset_for_respawn();
        self.void_damage_timer = 0.0;

        self.sync_cursor_mode();

        println!("[Debug] Player respawned at spawn point");
    }

    pub fn handle_death_click(&mut self) {
        let mouse_x = self.mouse_ndc[0];
        let mouse_y = self.mouse_ndc[1];

        // Respawn button: bounds X: [-0.3, 0.3], Y: [-0.1, 0.0]
        if mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.1 && mouse_y <= 0.0 {
            self.respawn();
        }
    }

    pub fn handle_primary_press(&mut self) -> bool {
        let melee_consumed = self.is_authoritative() && self.try_melee_attack();
        let decision = primary_press_decision(self.game_mode, melee_consumed);
        if decision.instant_break {
            self.handle_click(true);
        }
        decision.keep_held_mining
    }

    fn try_melee_attack(&mut self) -> bool {
        if !self.is_authoritative() {
            return false;
        }

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

        let held_stack = self.inventory.hotbar[self.inventory.selected];
        let held_item = held_stack
            .map(|stack| stack.item)
            .unwrap_or(crate::inventory::Item::Air);
        let enchantments = held_stack
            .map(|stack| stack.enchantments)
            .unwrap_or_default();
        let damage = held_item
            .tool_properties()
            .map(|tool| tool.damage)
            .unwrap_or(1.0)
            + crate::enchantment::attack_damage_bonus(&enchantments)
            + self.potion_effects.strength_bonus();
        let knockback =
            8.0 + enchantments.level_of(crate::enchantment::Enchantment::Knockback(1)) as f32 * 3.0;
        let fire_level = enchantments.level_of(crate::enchantment::Enchantment::FireAspect(1));

        let (entity_type, remaining_health, killed, kill) = {
            let Some(entity) = self.entity_manager.get_by_id_mut(entity_id) else {
                return false;
            };
            let MeleeImpact::Damaged { killed } =
                apply_melee_impact(entity, direction, damage, knockback, fire_level)
            else {
                return true;
            };
            let kill = if killed {
                claim_standard_player_kill(entity)
            } else {
                None
            };
            (entity.entity_type, entity.health, killed, kill)
        };

        println!(
            "[Debug] Hit {:?}, health={:.1}",
            entity_type, remaining_health
        );

        if killed {
            println!("[Debug] Killed {:?}", entity_type);
            if let Some(kill) = kill {
                let looting = enchantments.level_of(crate::enchantment::Enchantment::Looting(1));
                self.settle_standard_player_kill(kill, looting);
            }
        }

        self.damage_selected_tool(entity_id as u32 ^ self.total_time.to_bits());
        true
    }

    pub fn handle_click(&mut self, is_left_click: bool) {
        if !self.is_authoritative() {
            let direction = Vec3::new(
                self.camera.yaw.cos() * self.camera.pitch.cos(),
                self.camera.pitch.sin(),
                self.camera.yaw.sin() * self.camera.pitch.cos(),
            )
            .normalize_or_zero();
            let target_policy = if is_left_click {
                RaycastTargetPolicy::Break
            } else {
                RaycastTargetPolicy::Place
            };
            if let Some(hit) = raycast(
                self.camera.position,
                direction,
                5.0,
                &self.chunk_manager,
                target_policy,
            ) {
                let held_wire = self.inventory.hotbar[self.inventory.selected]
                    .as_ref()
                    .map(crate::network::protocol::ItemWire::from_stack);
                if is_left_click {
                    self.network.request_block_action(
                        crate::network::protocol::Action::Break,
                        hit.block_pos.x as i32,
                        hit.block_pos.y as i32,
                        hit.block_pos.z as i32,
                        BlockType::Air as u32,
                        held_wire,
                    );
                    self.network
                        .send_action(crate::network::protocol::Action::Break);
                } else if let Some(block) = self.inventory.get_selected_block() {
                    let target = hit.block_pos + hit.normal;
                    let (x, y, z) = (target.x as i32, target.y as i32, target.z as i32);
                    if !self.can_place_block_at(x, y, z, block) {
                        return;
                    }
                    self.network.request_block_action(
                        crate::network::protocol::Action::Place,
                        x,
                        y,
                        z,
                        block as u32,
                        held_wire,
                    );
                    self.network
                        .send_action(crate::network::protocol::Action::Place);
                }
            }
            return;
        }

        if !is_left_click {
            let held_stack = self.inventory.hotbar[self.inventory.selected];
            let held_item = held_stack
                .map(|s| s.item)
                .unwrap_or(crate::inventory::Item::Air);
            if let Some(potion) = held_stack.and_then(|stack| stack.potion) {
                if potion.splash || held_item == Item::SplashPotion {
                    let dir = Vec3::new(
                        self.camera.yaw.cos() * self.camera.pitch.cos(),
                        self.camera.pitch.sin(),
                        self.camera.yaw.sin() * self.camera.pitch.cos(),
                    )
                    .normalize_or_zero();
                    let id = self.entity_manager.spawn(
                        crate::entity::EntityType::SplashPotion,
                        self.camera.position + dir * 0.5,
                    );
                    if let Some(projectile) = self.entity_manager.get_by_id_mut(id) {
                        projectile.velocity = dir * 12.0;
                        projectile.potion = Some(potion);
                        projectile.life_time = 3.0;
                    }
                } else {
                    let healing = self.potion_effects.apply(potion);
                    self.player_state.health =
                        (self.player_state.health + healing).min(self.player_state.max_health);
                }
                self.inventory
                    .use_selected_item(self.game_mode == GameMode::Creative);
                return;
            }
            if held_item == Item::MilkBucket {
                self.potion_effects.active.clear();
                if self.game_mode == GameMode::Survival {
                    self.inventory.replace_selected_item(Item::Bucket);
                }
                return;
            }
            if held_item == Item::Bow {
                let enchantments = held_stack
                    .map(|stack| stack.enchantments)
                    .unwrap_or_default();
                let infinity = enchantments.level_of(crate::enchantment::Enchantment::Infinity) > 0;
                if self.game_mode == GameMode::Creative
                    || infinity
                    || self.inventory.remove_one(Item::Arrow)
                {
                    let dir = Vec3::new(
                        self.camera.yaw.cos() * self.camera.pitch.cos(),
                        self.camera.pitch.sin(),
                        self.camera.yaw.sin() * self.camera.pitch.cos(),
                    )
                    .normalize_or_zero();
                    let id = self.entity_manager.spawn(
                        crate::entity::EntityType::Arrow,
                        self.camera.position + dir * 0.6,
                    );
                    if let Some(arrow) = self.entity_manager.get_by_id_mut(id) {
                        arrow.velocity = dir * 22.0;
                        arrow.friendly_projectile = true;
                        arrow.projectile_damage = 4.0
                            + enchantments.level_of(crate::enchantment::Enchantment::Power(1))
                                as f32
                                * 1.25;
                    }
                }
                return;
            }
            if held_item == crate::inventory::Item::Apple
                || held_item == crate::inventory::Item::Bread
            {
                if self.player_state.hunger < 20.0 || self.game_mode == GameMode::Creative {
                    let (heal_hunger, heal_saturation) = match held_item {
                        crate::inventory::Item::Apple => (4.0, 2.4),
                        crate::inventory::Item::Bread => (5.0, 6.0),
                        _ => (0.0, 0.0),
                    };
                    self.player_state.hunger = (self.player_state.hunger + heal_hunger).min(20.0);
                    self.player_state.saturation = (self.player_state.saturation + heal_saturation)
                        .min(self.player_state.hunger);

                    let is_creative = self.game_mode == GameMode::Creative;
                    self.inventory.use_selected_item(is_creative);

                    println!(
                        "[Debug] Ate {:?}, hunger={:.1}, saturation={:.1}",
                        held_item, self.player_state.hunger, self.player_state.saturation
                    );
                    return;
                }
            }
        }

        let dir = Vec3::new(
            self.camera.yaw.cos() * self.camera.pitch.cos(),
            self.camera.pitch.sin(),
            self.camera.yaw.sin() * self.camera.pitch.cos(),
        )
        .normalize_or_zero();

        if !is_left_click {
            let mut closest_entity: Option<(u64, f32)> = None;
            for entity in self.entity_manager.query_radius(self.camera.position, 4.0) {
                if entity.entity_type == crate::entity::EntityType::Arrow
                    || entity.entity_type == crate::entity::EntityType::HeartParticle
                {
                    continue;
                }
                let aabb = entity.get_aabb();
                if let Some(dist) =
                    crate::entity::ray_intersects_aabb(self.camera.position, dir, &aabb)
                {
                    if dist <= 4.0 {
                        if let Some((_, closest_dist)) = closest_entity {
                            if dist < closest_dist {
                                closest_entity = Some((entity.id, dist));
                            }
                        } else {
                            closest_entity = Some((entity.id, dist));
                        }
                    }
                }
            }

            if let Some((entity_id, _)) = closest_entity {
                if let Some(entity) = self.entity_manager.get_by_id_mut(entity_id) {
                    let held_stack = self.inventory.hotbar[self.inventory.selected].clone();
                    let held_item = held_stack
                        .map(|s| s.item)
                        .unwrap_or(crate::inventory::Item::Air);

                    match entity.entity_type {
                        crate::entity::EntityType::Pig => {
                            if held_item == crate::inventory::Item::Carrot
                                && entity.age >= 0.0
                                && entity.breeding_timer <= 0.0
                                && entity.breed_cooldown <= 0.0
                            {
                                entity.breeding_timer = 20.0;
                                self.inventory.remove_selected_item(1);
                                println!("[Debug] Pig entered love mode!");
                                return;
                            }
                        }
                        crate::entity::EntityType::Cow => {
                            if held_item == crate::inventory::Item::Wheat
                                && entity.age >= 0.0
                                && entity.breeding_timer <= 0.0
                                && entity.breed_cooldown <= 0.0
                            {
                                entity.breeding_timer = 20.0;
                                self.inventory.remove_selected_item(1);
                                println!("[Debug] Cow entered love mode!");
                                return;
                            }
                            if held_item == crate::inventory::Item::Bucket {
                                self.inventory
                                    .replace_selected_item(crate::inventory::Item::MilkBucket);
                                println!("[Debug] Milked a Cow!");
                                return;
                            }
                        }
                        crate::entity::EntityType::Sheep => {
                            if held_item == crate::inventory::Item::Wheat
                                && entity.age >= 0.0
                                && entity.breeding_timer <= 0.0
                                && entity.breed_cooldown <= 0.0
                            {
                                entity.breeding_timer = 20.0;
                                self.inventory.remove_selected_item(1);
                                println!("[Debug] Sheep entered love mode!");
                                return;
                            }
                            if held_item == crate::inventory::Item::Shears && entity.has_wool {
                                let wool_position = entity.position;
                                entity.has_wool = false;
                                self.store_or_drop_generated_item(
                                    crate::inventory::Item::Wool,
                                    wool_position,
                                );
                                println!("[Debug] Sheared a Sheep!");
                                if let Some(stack) =
                                    &mut self.inventory.hotbar[self.inventory.selected]
                                {
                                    if stack.durability > 1 {
                                        stack.durability -= 1;
                                    } else {
                                        self.inventory.hotbar[self.inventory.selected] = None;
                                    }
                                }
                                return;
                            }
                        }
                        crate::entity::EntityType::Chicken => {
                            if held_item == crate::inventory::Item::Seeds
                                && entity.age >= 0.0
                                && entity.breeding_timer <= 0.0
                                && entity.breed_cooldown <= 0.0
                            {
                                entity.breeding_timer = 20.0;
                                self.inventory.remove_selected_item(1);
                                println!("[Debug] Chicken entered love mode!");
                                return;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        let target_policy = if is_left_click {
            RaycastTargetPolicy::Break
        } else {
            RaycastTargetPolicy::Place
        };
        if let Some(hit) = raycast(
            self.camera.position,
            dir,
            5.0,
            &self.chunk_manager,
            target_policy,
        ) {
            let target = if is_left_click {
                hit.block_pos
            } else {
                let clicked_block = self.chunk_manager.get_block(
                    hit.block_pos.x as i32,
                    hit.block_pos.y as i32,
                    hit.block_pos.z as i32,
                );
                let held = self.inventory.hotbar[self.inventory.selected];
                let clicked_pos = (
                    hit.block_pos.x as i32,
                    hit.block_pos.y as i32,
                    hit.block_pos.z as i32,
                );
                let held_item = held.map(|stack| stack.item).unwrap_or(Item::Air);
                if clicked_block == BlockType::Obsidian && held_item == Item::FlintAndSteel {
                    if let Some(interior) =
                        crate::dimension::detect_nether_frame(clicked_pos, |x, y, z| {
                            self.chunk_manager.get_block(x, y, z)
                        })
                    {
                        let changes: Vec<_> = interior
                            .into_iter()
                            .map(|position| (position, BlockType::NetherPortal))
                            .collect();
                        self.apply_block_changes(&changes);
                        self.inventory
                            .use_selected_item(self.game_mode == GameMode::Creative);
                        return;
                    }
                }
                if clicked_block == BlockType::EndPortalFrame && held_item == Item::EyeOfEnder {
                    self.apply_block_changes(&[(clicked_pos, BlockType::EndPortalFrameFilled)]);
                    self.inventory
                        .use_selected_item(self.game_mode == GameMode::Creative);
                    if let Some(interior) =
                        crate::dimension::detect_completed_end_portal(clicked_pos, |x, y, z| {
                            self.chunk_manager.get_block(x, y, z)
                        })
                    {
                        let changes: Vec<_> = interior
                            .into_iter()
                            .map(|position| (position, BlockType::EndPortal))
                            .collect();
                        self.apply_block_changes(&changes);
                    }
                    return;
                }
                if matches!(clicked_block, BlockType::Obsidian | BlockType::Bedrock)
                    && held_item == Item::EndCrystal
                {
                    self.entity_manager.spawn(
                        crate::entity::EntityType::EndCrystal,
                        Vec3::new(
                            clicked_pos.0 as f32 + 0.5,
                            clicked_pos.1 as f32 + 1.0,
                            clicked_pos.2 as f32 + 0.5,
                        ),
                    );
                    self.inventory
                        .use_selected_item(self.game_mode == GameMode::Creative);
                    return;
                }
                if clicked_block == BlockType::EndCityChest {
                    self.spawn_dropped_item(Item::Elytra, hit.block_pos + Vec3::Y);
                    self.apply_block_changes(&[(clicked_pos, BlockType::Air)]);
                    return;
                }
                if clicked_block == BlockType::Water
                    && held.is_some_and(|stack| stack.item == Item::GlassBottle)
                {
                    let selected = self.inventory.selected;
                    let original_selected = self.inventory.hotbar[selected];
                    self.inventory
                        .use_selected_item(self.game_mode == GameMode::Creative);
                    let mut water_bottle = ItemStack::new(Item::Potion, 1);
                    water_bottle.potion = Some(crate::brewing::PotionData::water());
                    if self.inventory.add_stack(water_bottle).is_some() {
                        self.inventory.hotbar[selected] = original_selected;
                    }
                    return;
                }
                if clicked_block == BlockType::CraftingTable {
                    self.inventory.is_table_open = true;
                    self.inventory.craft_input = vec![None; 9];
                    self.open_inventory();
                    return;
                }
                if matches!(
                    clicked_block,
                    BlockType::EnchantingTable | BlockType::BrewingStand | BlockType::Anvil
                ) {
                    let kind = match clicked_block {
                        BlockType::EnchantingTable => StationKind::Enchanting,
                        BlockType::BrewingStand => StationKind::Brewing,
                        _ => StationKind::Anvil,
                    };
                    self.open_station(kind, hit.block_pos);
                    return;
                }
                if matches!(
                    clicked_block,
                    BlockType::OakDoor
                        | BlockType::OakDoorOpen
                        | BlockType::OakTrapdoor
                        | BlockType::OakTrapdoorOpen
                ) {
                    let pos = (
                        hit.block_pos.x as i32,
                        hit.block_pos.y as i32,
                        hit.block_pos.z as i32,
                    );
                    let (target_block, sound) = match clicked_block {
                        BlockType::OakDoor => {
                            (BlockType::OakDoorOpen, crate::audio::SoundId::UiClick)
                        }
                        BlockType::OakDoorOpen => {
                            (BlockType::OakDoor, crate::audio::SoundId::UiClick)
                        }
                        BlockType::OakTrapdoor => {
                            (BlockType::OakTrapdoorOpen, crate::audio::SoundId::UiClick)
                        }
                        BlockType::OakTrapdoorOpen => {
                            (BlockType::OakTrapdoor, crate::audio::SoundId::UiClick)
                        }
                        _ => unreachable!(),
                    };
                    let cur_raw = self.chunk_manager.get_block_state(pos.0, pos.1, pos.2);
                    let mut bstate = crate::world::BlockState::decode(cur_raw);
                    bstate.is_open = !bstate.is_open;
                    let new_state_raw = bstate.encode();

                    self.chunk_manager
                        .set_block(pos.0, pos.1, pos.2, target_block);
                    self.chunk_manager
                        .set_block_state(pos.0, pos.1, pos.2, new_state_raw);
                    self.broadcast_block_change(pos.0, pos.1, pos.2, target_block);

                    if matches!(clicked_block, BlockType::OakDoor | BlockType::OakDoorOpen) {
                        let other_y = if bstate.is_top { pos.1 - 1 } else { pos.1 + 1 };
                        let other_block = self.chunk_manager.get_block(pos.0, other_y, pos.2);
                        if matches!(other_block, BlockType::OakDoor | BlockType::OakDoorOpen) {
                            let other_raw =
                                self.chunk_manager.get_block_state(pos.0, other_y, pos.2);
                            let mut other_bstate = crate::world::BlockState::decode(other_raw);
                            other_bstate.is_open = bstate.is_open;
                            let other_target = if bstate.is_open {
                                BlockType::OakDoorOpen
                            } else {
                                BlockType::OakDoor
                            };
                            self.chunk_manager
                                .set_block(pos.0, other_y, pos.2, other_target);
                            self.chunk_manager.set_block_state(
                                pos.0,
                                other_y,
                                pos.2,
                                other_bstate.encode(),
                            );
                            self.broadcast_block_change(pos.0, other_y, pos.2, other_target);
                        }
                    }

                    let mut dirty_chunks = std::collections::HashSet::new();
                    mark_block_mesh_dependencies(&mut dirty_chunks, pos.0, pos.2);
                    self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::Redstone);
                    self.audio_manager.play_sound(sound);
                    return;
                }
                if matches!(
                    clicked_block,
                    BlockType::Lever
                        | BlockType::LeverOn
                        | BlockType::StoneButton
                        | BlockType::StoneButtonPressed
                        | BlockType::Repeater
                        | BlockType::RepeaterPowered
                        | BlockType::Comparator
                        | BlockType::ComparatorPowered
                        | BlockType::NoteBlock
                ) {
                    let pos = (
                        hit.block_pos.x as i32,
                        hit.block_pos.y as i32,
                        hit.block_pos.z as i32,
                    );
                    let update = self.redstone.interact(&mut self.chunk_manager, pos);
                    self.apply_redstone_update(update);
                    self.audio_manager
                        .play_sound(crate::audio::SoundId::UiClick);
                    return;
                }
                hit.block_pos + hit.normal
            };

            let wx = target.x as i32;
            let wy = target.y as i32;
            let wz = target.z as i32;

            let mut dirty_chunks = std::collections::HashSet::new();
            // Resulting block at (wx, wy, wz) after this click, used to fan the
            // authoritative mutation out to connected clients. `None` means the
            // click did not mutate the world (e.g. broke nothing).
            let mut result_block: Option<BlockType> = None;
            if is_left_click {
                let old_block = self.chunk_manager.get_block(wx, wy, wz);
                if old_block != BlockType::Air {
                    if old_block.properties().hardness < 0.0 {
                        return;
                    }
                    self.chunk_manager.set_block(wx, wy, wz, BlockType::Air);
                    self.network
                        .send_action(crate::network::protocol::Action::Break);
                    self.trigger_advancement(crate::advancements::AdvancementTrigger::MineBlock(
                        old_block,
                    ));
                    self.redstone.on_block_changed(
                        &self.chunk_manager,
                        (wx, wy, wz),
                        crate::redstone::Direction::North,
                    );

                    let sound_pos =
                        glam::Vec3::new(wx as f32 + 0.5, wy as f32 + 0.5, wz as f32 + 0.5);
                    let listener_right =
                        glam::Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos())
                            .normalize_or_zero();
                    if let Some(mat) = old_block.sound_material() {
                        self.audio_manager.play_sound_3d(
                            crate::audio::SoundId::BlockBreak(mat),
                            sound_pos,
                            self.camera.position,
                            listener_right,
                        );
                    }

                    if self.game_mode == GameMode::Survival {
                        self.store_or_drop_generated_item(
                            crate::inventory::Item::from_block(old_block),
                            sound_pos,
                        );

                        if old_block == BlockType::Grass {
                            let rng = (wx as u32).wrapping_mul(31).wrapping_add(wz as u32);
                            if rng % 20 == 0 {
                                let drop = match rng % 3 {
                                    0 => crate::inventory::Item::Seeds,
                                    1 => crate::inventory::Item::Wheat,
                                    _ => crate::inventory::Item::Carrot,
                                };
                                self.store_or_drop_generated_item(drop, sound_pos);
                            }
                        }
                    }

                    // Update lighting for removal
                    crate::lighting::update_sky_light_after_removed(
                        &mut self.chunk_manager,
                        wx,
                        wy,
                        wz,
                        &mut dirty_chunks,
                    );
                    crate::lighting::update_block_light_after_removed(
                        &mut self.chunk_manager,
                        wx,
                        wy,
                        wz,
                        old_block.properties().light_emission,
                        &mut dirty_chunks,
                    );
                    self.check_and_break_unsupported_above(wx, wy, wz, &mut dirty_chunks);
                    result_block = Some(BlockType::Air);
                }
            } else {
                if let Some(placed_block) = self.inventory.get_selected_block() {
                    if placed_block == BlockType::OakDoor {
                        if wy + 1 >= crate::world::CHUNK_HEIGHT as i32 {
                            return;
                        }
                        if !self.can_place_block_at(wx, wy, wz, BlockType::OakDoor)
                            || !self.can_place_block_at(wx, wy + 1, wz, BlockType::OakDoor)
                        {
                            return;
                        }
                        if !self.chunk_manager.can_place_block_with_support(
                            BlockType::OakDoor,
                            wx,
                            wy,
                            wz,
                        ) {
                            return;
                        }
                        let (bottom_state, top_state) =
                            crate::world::BlockState::for_door_placement(
                                &self.chunk_manager,
                                wx,
                                wy,
                                wz,
                                self.camera.yaw,
                            );

                        self.chunk_manager.set_block(wx, wy, wz, BlockType::OakDoor);
                        self.chunk_manager
                            .set_block_state(wx, wy, wz, bottom_state.encode());
                        self.chunk_manager
                            .set_block(wx, wy + 1, wz, BlockType::OakDoor);
                        self.chunk_manager
                            .set_block_state(wx, wy + 1, wz, top_state.encode());

                        self.network
                            .send_action(crate::network::protocol::Action::Place);
                        self.redstone.on_block_changed(
                            &self.chunk_manager,
                            (wx, wy, wz),
                            bottom_state.facing,
                        );

                        let sound_pos =
                            glam::Vec3::new(wx as f32 + 0.5, wy as f32 + 0.5, wz as f32 + 0.5);
                        let listener_right =
                            glam::Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos())
                                .normalize_or_zero();
                        if let Some(mat) = BlockType::OakDoor.sound_material() {
                            self.audio_manager.play_sound_3d(
                                crate::audio::SoundId::BlockPlace(mat),
                                sound_pos,
                                self.camera.position,
                                listener_right,
                            );
                        }

                        let is_creative = self.game_mode == GameMode::Creative;
                        self.inventory.use_selected_item(is_creative);

                        crate::lighting::update_sky_light_after_placed(
                            &mut self.chunk_manager,
                            wx,
                            wy,
                            wz,
                            &mut dirty_chunks,
                        );
                        crate::lighting::update_sky_light_after_placed(
                            &mut self.chunk_manager,
                            wx,
                            wy + 1,
                            wz,
                            &mut dirty_chunks,
                        );
                        mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);
                        mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz + 1);

                        self.broadcast_block_change(wx, wy, wz, BlockType::OakDoor);
                        self.broadcast_block_change(wx, wy + 1, wz, BlockType::OakDoor);
                        result_block = Some(BlockType::OakDoor);
                    } else if placed_block == BlockType::OakTrapdoor {
                        if !self.chunk_manager.can_place_block_with_support(
                            placed_block,
                            wx,
                            wy,
                            wz,
                        ) || !self.can_place_block_at(wx, wy, wz, placed_block)
                        {
                            return;
                        }
                        let state =
                            crate::world::BlockState::for_trapdoor_placement(self.camera.yaw);

                        self.chunk_manager
                            .set_block(wx, wy, wz, BlockType::OakTrapdoor);
                        self.chunk_manager
                            .set_block_state(wx, wy, wz, state.encode());

                        self.network
                            .send_action(crate::network::protocol::Action::Place);
                        self.redstone.on_block_changed(
                            &self.chunk_manager,
                            (wx, wy, wz),
                            state.facing,
                        );

                        let sound_pos =
                            glam::Vec3::new(wx as f32 + 0.5, wy as f32 + 0.5, wz as f32 + 0.5);
                        let listener_right =
                            glam::Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos())
                                .normalize_or_zero();
                        if let Some(mat) = BlockType::OakTrapdoor.sound_material() {
                            self.audio_manager.play_sound_3d(
                                crate::audio::SoundId::BlockPlace(mat),
                                sound_pos,
                                self.camera.position,
                                listener_right,
                            );
                        }

                        let is_creative = self.game_mode == GameMode::Creative;
                        self.inventory.use_selected_item(is_creative);

                        crate::lighting::update_sky_light_after_placed(
                            &mut self.chunk_manager,
                            wx,
                            wy,
                            wz,
                            &mut dirty_chunks,
                        );
                        mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);

                        self.broadcast_block_change(wx, wy, wz, BlockType::OakTrapdoor);
                        result_block = Some(BlockType::OakTrapdoor);
                    } else {
                        if !self.chunk_manager.can_place_block_with_support(
                            placed_block,
                            wx,
                            wy,
                            wz,
                        ) {
                            return;
                        }
                        if !self.can_place_block_at(wx, wy, wz, placed_block) {
                            return;
                        }

                        self.chunk_manager.set_block(wx, wy, wz, placed_block);
                        self.network
                            .send_action(crate::network::protocol::Action::Place);
                        self.redstone.on_block_changed(
                            &self.chunk_manager,
                            (wx, wy, wz),
                            crate::redstone::Direction::from_yaw(self.camera.yaw),
                        );

                        let sound_pos =
                            glam::Vec3::new(wx as f32 + 0.5, wy as f32 + 0.5, wz as f32 + 0.5);
                        let listener_right =
                            glam::Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos())
                                .normalize_or_zero();
                        if let Some(mat) = placed_block.sound_material() {
                            self.audio_manager.play_sound_3d(
                                crate::audio::SoundId::BlockPlace(mat),
                                sound_pos,
                                self.camera.position,
                                listener_right,
                            );
                        }

                        let is_creative = self.game_mode == GameMode::Creative;
                        self.inventory.use_selected_item(is_creative);

                        // Update lighting for placement
                        crate::lighting::update_sky_light_after_placed(
                            &mut self.chunk_manager,
                            wx,
                            wy,
                            wz,
                            &mut dirty_chunks,
                        );
                        crate::lighting::update_block_light_after_placed(
                            &mut self.chunk_manager,
                            wx,
                            wy,
                            wz,
                            placed_block.properties().light_emission,
                            &mut dirty_chunks,
                        );

                        self.check_and_break_unsupported_above(wx, wy, wz, &mut dirty_chunks);
                        result_block = Some(placed_block);
                    }

                    if matches!(
                        placed_block,
                        BlockType::SoulSand | BlockType::WitherSkeletonSkull
                    ) {
                        if let Some(pattern) =
                            crate::boss::detect_wither_pattern((wx, wy, wz), |position| {
                                self.chunk_manager
                                    .get_block(position.0, position.1, position.2)
                            })
                        {
                            let spawn_pos = pattern.iter().fold(Vec3::ZERO, |sum, &(x, y, z)| {
                                sum + Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5)
                            }) / pattern.len() as f32;
                            let removals: Vec<_> = pattern
                                .into_iter()
                                .map(|position| (position, BlockType::Air))
                                .collect();
                            self.apply_block_changes(&removals);
                            // The wither ritual consumes the placed block too;
                            // broadcast that final state before spawning.
                            self.broadcast_block_change(wx, wy, wz, BlockType::Air);
                            self.entity_manager
                                .spawn(crate::entity::EntityType::Wither, spawn_pos);
                            return;
                        }
                    }
                } else {
                    return; // No block selected to place
                }
            }

            mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);

            self.invalidate_chunk_meshes(dirty_chunks, DependencyReason::BreakPlace);

            // Fan the authoritative player-driven mutation out to clients.
            if let Some(block) = result_block {
                self.broadcast_block_change(wx, wy, wz, block);
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

        // 4. Crafting Grid & Output
        if self.active_station.is_none() && self.inventory.is_table_open {
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
            None => {}
        }

        slots
    }

    pub fn get_item_at_slot(&self, slot: SlotType) -> Option<ItemStack> {
        match slot {
            SlotType::Creative(item) => Some(ItemStack::new(item, 1)),
            SlotType::Hotbar(i) => self.inventory.hotbar[i],
            SlotType::Backpack(i) => self.inventory.main[i],
            SlotType::Armor(i) => self.inventory.armor[i],
            SlotType::CraftInput(i) => self.inventory.craft_input.get(i).copied().flatten(),
            SlotType::CraftOutput => self.inventory.craft_output,
            SlotType::EnchantInput => self.enchanting.input,
            SlotType::EnchantLapis => self.enchanting.lapis,
            SlotType::BrewBottle(i) => self.brewing.bottles[i],
            SlotType::BrewIngredient => self.brewing.ingredient,
            SlotType::AnvilLeft => self.anvil.left,
            SlotType::AnvilRight => self.anvil.right,
            SlotType::AnvilOutput => self.anvil.output,
        }
    }

    pub fn set_item_at_slot(&mut self, slot: SlotType, stack: Option<ItemStack>) {
        match slot {
            SlotType::Creative(item) => self.inventory.write_creative_slot(item, stack),
            SlotType::Hotbar(i) => self.inventory.hotbar[i] = stack,
            SlotType::Backpack(i) => self.inventory.main[i] = stack,
            SlotType::Armor(i) => self.inventory.armor[i] = stack,
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
        }
    }

    fn slot_accepts(&self, slot: SlotType, stack: ItemStack) -> bool {
        match slot {
            SlotType::Creative(_) => false,
            SlotType::EnchantInput => crate::enchantment::can_enchant(stack.item),
            SlotType::EnchantLapis => stack.item == Item::LapisLazuli,
            SlotType::BrewBottle(_) => stack.potion.is_some(),
            SlotType::AnvilOutput | SlotType::CraftOutput => false,
            _ => true,
        }
    }

    fn refresh_workstations(&mut self) {
        self.enchanting.refresh();
        self.anvil.refresh();
    }

    pub fn handle_inventory_click(&mut self, is_left: bool) {
        let mouse_x = self.mouse_ndc[0];
        let mouse_y = self.mouse_ndc[1];
        let creative_catalog = self.is_creative_catalog_open();
        if creative_catalog && is_left {
            for (index, tab) in CreativeTab::TABS.into_iter().enumerate() {
                if creative_tab_rect(index).contains(mouse_x, mouse_y) {
                    self.audio_manager
                        .play_sound(crate::audio::SoundId::UiClick);
                    self.inventory.select_creative_tab(tab);
                    return;
                }
            }
        }
        let slots = self.get_inventory_slots();

        if self.active_station == Some(StationKind::Enchanting) && is_left {
            for index in 0..3 {
                let y1 = 0.28 - index as f32 * 0.12;
                let y0 = y1 - 0.09;
                if mouse_x >= 0.02 && mouse_x <= 0.62 && mouse_y >= y0 && mouse_y <= y1 {
                    self.perform_enchantment(index);
                    return;
                }
            }
        }

        let clicked_slot = slots.into_iter().find(|&(_, x0, x1, y0, y1)| {
            mouse_x >= x0 && mouse_x <= x1 && mouse_y >= y0 && mouse_y <= y1
        });

        if let Some((slot_type, _, _, _, _)) = clicked_slot {
            self.audio_manager
                .play_sound(crate::audio::SoundId::UiClick);
            let slot_item = self.get_item_at_slot(slot_type);

            match slot_type {
                SlotType::Creative(item) => {
                    self.inventory.creative_supply(item, is_left);
                    return;
                }
                SlotType::Hotbar(index) if creative_catalog => {
                    self.inventory.click_creative_hotbar(index, is_left);
                    return;
                }
                _ => {}
            }

            if let Some(dragged) = self.inventory.dragged {
                if !self.slot_accepts(slot_type, dragged) {
                    return;
                }
            }

            match slot_type {
                SlotType::CraftOutput => {
                    if let Some(output) = slot_item {
                        self.trigger_advancement(
                            crate::advancements::AdvancementTrigger::CraftItem(output.item),
                        );
                        // Can only take from output slot
                        let max_stack = output.item.properties().max_stack;
                        if self.inventory.dragged.is_none() {
                            self.inventory.dragged = Some(output);
                            // Consume craft input ingredients
                            for slot in self.inventory.craft_input.iter_mut() {
                                if let Some(stack) = slot {
                                    if stack.count > 1 {
                                        stack.count -= 1;
                                    } else {
                                        *slot = None;
                                    }
                                }
                            }
                            let grid_size = if self.inventory.is_table_open { 3 } else { 2 };
                            self.inventory.craft_output = self
                                .recipe_manager
                                .match_recipe(&self.inventory.craft_input, grid_size);
                        } else if let Some(ref mut dragged) = self.inventory.dragged {
                            if dragged.can_merge_with(&output)
                                && dragged.count + output.count <= max_stack
                            {
                                dragged.count += output.count;
                                // Consume craft input ingredients
                                for slot in self.inventory.craft_input.iter_mut() {
                                    if let Some(stack) = slot {
                                        if stack.count > 1 {
                                            stack.count -= 1;
                                        } else {
                                            *slot = None;
                                        }
                                    }
                                }
                                let grid_size = if self.inventory.is_table_open { 3 } else { 2 };
                                self.inventory.craft_output = self
                                    .recipe_manager
                                    .match_recipe(&self.inventory.craft_input, grid_size);
                            }
                        }
                    }
                }
                SlotType::AnvilOutput => {
                    if let Some(output) = self.anvil.output {
                        let affordable = self.game_mode == GameMode::Creative
                            || self.player_state.experience_level >= self.anvil.cost as u32;
                        if affordable && self.inventory.dragged.is_none() {
                            if self.game_mode == GameMode::Survival {
                                self.player_state.spend_levels(self.anvil.cost as u32);
                            }
                            self.inventory.dragged = Some(output);
                            self.anvil.left = None;
                            self.anvil.right = None;
                            self.anvil.rename.clear();
                            self.anvil.refresh();
                        }
                    }
                }
                _ => {
                    // Normal slots (Backpack, Hotbar, Armor, CraftInput)
                    let max_stack = slot_item
                        .map(|s| s.item.properties().max_stack)
                        .unwrap_or(64);

                    if is_left {
                        // Left Click interaction
                        if let Some(dragged) = self.inventory.dragged {
                            if let Some(slot) = slot_item {
                                if slot.can_merge_with(&dragged) {
                                    // Stack them
                                    let space = max_stack.saturating_sub(slot.count);
                                    let transfer = space.min(dragged.count);
                                    let new_slot_count = slot.count + transfer;
                                    let new_drag_count = dragged.count - transfer;

                                    self.set_item_at_slot(
                                        slot_type,
                                        Some(ItemStack {
                                            count: new_slot_count,
                                            ..slot
                                        }),
                                    );
                                    if new_drag_count > 0 {
                                        self.inventory.dragged = Some(ItemStack {
                                            count: new_drag_count,
                                            ..dragged
                                        });
                                    } else {
                                        self.inventory.dragged = None;
                                    }
                                } else {
                                    // Swap slot and dragged
                                    self.set_item_at_slot(slot_type, Some(dragged));
                                    self.inventory.dragged = Some(slot);
                                }
                            } else {
                                // Put dragged in empty slot
                                self.set_item_at_slot(slot_type, Some(dragged));
                                self.inventory.dragged = None;
                            }
                        } else {
                            // Pickup entire slot
                            if let Some(slot) = slot_item {
                                self.inventory.dragged = Some(slot);
                                self.set_item_at_slot(slot_type, None);
                            }
                        }
                    } else {
                        // Right Click interaction
                        if let Some(dragged) = self.inventory.dragged {
                            if let Some(slot) = slot_item {
                                if slot.can_merge_with(&dragged) && slot.count < max_stack {
                                    // Drop 1
                                    self.set_item_at_slot(
                                        slot_type,
                                        Some(ItemStack {
                                            count: slot.count + 1,
                                            ..slot
                                        }),
                                    );
                                    if dragged.count > 1 {
                                        self.inventory.dragged = Some(ItemStack {
                                            count: dragged.count - 1,
                                            ..dragged
                                        });
                                    } else {
                                        self.inventory.dragged = None;
                                    }
                                } else if !slot.can_merge_with(&dragged) {
                                    // Swap (like left click swap)
                                    self.set_item_at_slot(slot_type, Some(dragged));
                                    self.inventory.dragged = Some(slot);
                                }
                            } else {
                                // Drop 1 in empty slot
                                self.set_item_at_slot(
                                    slot_type,
                                    Some(ItemStack {
                                        count: 1,
                                        ..dragged
                                    }),
                                );
                                if dragged.count > 1 {
                                    self.inventory.dragged = Some(ItemStack {
                                        count: dragged.count - 1,
                                        ..dragged
                                    });
                                } else {
                                    self.inventory.dragged = None;
                                }
                            }
                        } else {
                            // Split stack in slot
                            if let Some(slot) = slot_item {
                                let take = (slot.count + 1) / 2;
                                let keep = slot.count - take;
                                self.inventory.dragged = Some(ItemStack {
                                    count: take,
                                    ..slot
                                });
                                if keep > 0 {
                                    self.set_item_at_slot(
                                        slot_type,
                                        Some(ItemStack {
                                            count: keep,
                                            ..slot
                                        }),
                                    );
                                } else {
                                    self.set_item_at_slot(slot_type, None);
                                }
                            }
                        }
                    }

                    // If we clicked a craft input slot, recalculate craft output
                    if let SlotType::CraftInput(_) = slot_type {
                        let grid_size = if self.inventory.is_table_open { 3 } else { 2 };
                        self.inventory.craft_output = self
                            .recipe_manager
                            .match_recipe(&self.inventory.craft_input, grid_size);
                    }
                    self.refresh_workstations();
                }
            }
        } else if let Some(dragged) = self.inventory.dragged {
            let aspect = self.size.width as f32 / self.size.height as f32;
            if creative_catalog
                && is_left
                && creative_scroll_track_rect(aspect).contains(mouse_x, mouse_y)
            {
                return;
            }
            if is_left {
                self.throw_dropped_item(dragged.item, dragged.count);
                self.inventory.dragged = None;
                self.inventory.creative_drag_origin = None;
            } else {
                self.throw_dropped_item(dragged.item, 1);
                if dragged.count > 1 {
                    self.inventory.dragged = Some(ItemStack {
                        count: dragged.count - 1,
                        ..dragged
                    });
                } else {
                    self.inventory.dragged = None;
                    self.inventory.creative_drag_origin = None;
                }
            }
        }
    }

    fn perform_enchantment(&mut self, index: usize) {
        let Some(mut input) = self.enchanting.input else {
            return;
        };
        if !crate::enchantment::can_enchant(input.item) {
            return;
        }
        let option = self.enchanting.options[index];
        let lapis_available = self
            .enchanting
            .lapis
            .filter(|stack| stack.item == Item::LapisLazuli)
            .map(|stack| stack.count)
            .unwrap_or(0);
        let affordable = self.game_mode == GameMode::Creative
            || (lapis_available >= option.lapis_cost as u32
                && self.player_state.experience_level >= option.cost as u32);
        if !affordable {
            return;
        }
        input.enchantments.merge(&option.enchantments);
        self.enchanting.input = Some(input);
        self.trigger_advancement(crate::advancements::AdvancementTrigger::EnchantItem);
        if self.game_mode == GameMode::Survival {
            self.player_state.spend_levels(option.cost as u32);
            if let Some(lapis) = &mut self.enchanting.lapis {
                if lapis.count > option.lapis_cost as u32 {
                    lapis.count -= option.lapis_cost as u32;
                } else {
                    self.enchanting.lapis = None;
                }
            }
        }
        self.enchanting.seed = self.enchanting.seed.wrapping_add(0x9E37_79B9);
        self.enchanting.refresh();
    }

    pub fn open_inventory(&mut self) {
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

    fn open_station(&mut self, kind: StationKind, position: Vec3) {
        self.active_station = Some(kind);
        if kind == StationKind::Enchanting {
            let wx = position.x as i32;
            let wy = position.y as i32;
            let wz = position.z as i32;
            let mut shelves = 0;
            for dx in -2i32..=2i32 {
                for dz in -2i32..=2i32 {
                    if dx.abs() != 2 && dz.abs() != 2 {
                        continue;
                    }
                    for dy in 0..=1 {
                        if self.chunk_manager.get_block(wx + dx, wy + dy, wz + dz)
                            == BlockType::Bookshelf
                        {
                            shelves += 1;
                        }
                    }
                }
            }
            self.enchanting.bookshelves = shelves.min(15);
            self.enchanting.seed =
                self.world_time.ticks as u32 ^ wx as u32 ^ (wz as u32).rotate_left(16);
            self.enchanting.refresh();
        }
        self.open_inventory();
    }

    pub fn close_inventory(&mut self) -> bool {
        let mut returning_items: Vec<ItemStack> = self
            .inventory
            .craft_input
            .iter()
            .flatten()
            .copied()
            .collect();
        returning_items.extend(match self.active_station {
            Some(StationKind::Enchanting) => [self.enchanting.input, self.enchanting.lapis]
                .into_iter()
                .flatten()
                .collect(),
            Some(StationKind::Brewing) => self
                .brewing
                .bottles
                .iter()
                .copied()
                .chain(std::iter::once(self.brewing.ingredient))
                .flatten()
                .collect(),
            Some(StationKind::Anvil) => [self.anvil.left, self.anvil.right]
                .into_iter()
                .flatten()
                .collect(),
            None => Vec::new(),
        });

        for stack in returning_items {
            if let Some(remainder) = self.inventory.add_stack(stack) {
                self.throw_dropped_item(remainder.item, remainder.count);
            }
        }

        if self.inventory.creative_drag_origin
            == Some(crate::inventory::CreativeDragOrigin::Catalog)
        {
            self.inventory.dragged = None;
            self.inventory.creative_drag_origin = None;
        } else if let Some(dragged) = self.inventory.dragged {
            if let Some(remainder) = self.inventory.add_stack(dragged) {
                self.throw_dropped_item(remainder.item, remainder.count);
            }
            self.inventory.dragged = None;
            self.inventory.creative_drag_origin = None;
        }

        self.inventory.craft_input.fill(None);
        match self.active_station {
            Some(StationKind::Enchanting) => {
                self.enchanting.input = None;
                self.enchanting.lapis = None;
            }
            Some(StationKind::Brewing) => {
                self.brewing.bottles.fill(None);
                self.brewing.ingredient = None;
            }
            Some(StationKind::Anvil) => {
                self.anvil.left = None;
                self.anvil.right = None;
            }
            None => {}
        }

        self.inventory.is_open = false;
        self.inventory.is_table_open = false;
        self.inventory.craft_input = vec![None; 4];
        self.inventory.craft_output = None;
        self.active_station = None;
        self.anvil.rename.clear();

        self.sync_cursor_mode();
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
        let allocs_before = crate::perf::thread_alloc_count();
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut gpu_upload_elapsed = Duration::ZERO;

        let terrain_prepare_started = Instant::now();
        let view_projection = Mat4::from_cols_array_2d(&self.camera_uniform.view_proj);
        let frustum = Frustum::from_view_projection(view_projection);

        let cam_pos = self.camera.position;
        let render_blocks = self.chunk_manager.render_distance as f32 * CHUNK_WIDTH as f32;
        let render_distance_sq = render_blocks * render_blocks;
        let r_i32 = self.chunk_manager.render_distance as i32;

        let cam_sec_x = (cam_pos.x / 16.0).floor() as i32;
        let cam_sec_y_raw = (cam_pos.y / 16.0).floor() as i32;
        let cam_sec_z = (cam_pos.z / 16.0).floor() as i32;

        let fail_open_section_vis = cam_sec_y_raw < 0
            || cam_sec_y_raw >= 16
            || !self.chunk_meshes.contains_key(&(cam_sec_x, cam_sec_z));

        if !fail_open_section_vis {
            crate::culling::traverse_section_visibility_with_scratch(
                cam_sec_x,
                cam_sec_y_raw as usize,
                cam_sec_z,
                r_i32,
                &frustum,
                |x, sy, z| {
                    self.chunk_meshes
                        .get(&(x, z))
                        .and_then(|mesh| mesh.section(sy))
                        .map(|section| section.connectivity.fail_open())
                },
                &mut self.visible_sections_scratch,
                &mut self.section_visibility_scratch,
            );
        }

        self.perf_counters.save_queue_depth = self.save_queue_stats.depth();
        self.perf_counters.save_queue_bytes = self.save_queue_stats.queued_bytes();
        self.perf_counters.save_in_flight = self.save_queue_stats.in_flight();
        self.perf_counters.save_in_flight_bytes = self.save_queue_stats.in_flight_bytes();
        self.perf_counters.save_drop = self.save_queue_stats.dropped();
        if let Ok(mgr) = self.save_manager.try_lock() {
            self.perf_counters.loaded_region_cache_bytes = mgr.region_cache_bytes();
        }

        let lod_thresholds = LodThresholds::new(render_blocks * 0.5, render_blocks * 0.75);
        self.terrain_candidates_scratch.clear();
        let mut occluded_sections = 0u64;

        for (&coord, mesh) in &self.chunk_meshes {
            for (section_y, section) in mesh.sections.iter().enumerate() {
                let Some(bounds) = section.finest_bounds() else {
                    continue;
                };

                let distance_sq = bounds.center_distance_squared(cam_pos);
                if distance_sq > render_distance_sq || !frustum.intersects_aabb(&bounds) {
                    continue;
                }

                if !fail_open_section_vis
                    && !self
                        .visible_sections_scratch
                        .contains(&(coord.0, section_y, coord.1))
                {
                    occluded_sections += 1;
                    continue;
                }

                let lod = select_lod_for_bounds(cam_pos, bounds, lod_thresholds);
                let Some(level) = section.level(lod) else {
                    continue;
                };
                let key = SectionKey::new(coord.0, section_y as u16, coord.1);

                if let Some(bounds) = level.opaque.bounds {
                    self.terrain_candidates_scratch
                        .push(DrawCandidate::for_section(
                            key,
                            bounds,
                            level.opaque.num_indices(),
                            DrawLayer::Opaque,
                            lod,
                            distance_sq,
                        ));
                }
                if let Some(bounds) = level.transparent.bounds {
                    self.terrain_candidates_scratch
                        .push(DrawCandidate::for_section(
                            key,
                            bounds,
                            level.transparent.num_indices(),
                            DrawLayer::Transparent,
                            lod,
                            distance_sq,
                        ));
                }
            }
        }

        let terrain_candidate_count = self.terrain_candidates_scratch.len();
        self.terrain_draw_plan_scratch
            .build_into(self.terrain_candidates_scratch.iter().copied(), &frustum);
        let draw_plan = &self.terrain_draw_plan_scratch;
        self.submitted_terrain_triangles = draw_plan.submitted_triangle_count();
        self.submitted_terrain_draw_calls = draw_plan.draw_call_count();
        self.visible_chunk_count = draw_plan.visible_chunk_count();
        self.perf_counters.loaded_chunks = self.chunk_manager.chunks.len() as u64;
        self.perf_counters.visible_chunks = self.visible_chunk_count as u64;
        self.perf_counters.occluded_chunks = occluded_sections;
        self.perf_counters.terrain_candidates = terrain_candidate_count as u64;
        self.perf_counters.terrain_triangles = self.submitted_terrain_triangles;
        self.perf_counters.in_flight =
            (self.chunk_load_in_flight.len() + self.section_scheduler.in_flight.len()) as u64;
        let total_committed: usize = self
            .render_regions
            .values()
            .map(|r| r.committed_bytes())
            .sum();
        let total_used: usize = self.render_regions.values().map(|r| r.used_bytes()).sum();
        self.perf_counters.gpu_mesh_bytes = total_committed as u64;
        self.perf_counters.gpu_arena_used_bytes = total_used as u64;
        self.perf_counters.gpu_arena_wasted_bytes =
            total_committed.saturating_sub(total_used) as u64;
        self.perf_counters.gpu_arena_regions = self.render_regions.len() as u64;
        self.perf_counters.gpu_buffer_objects = self
            .render_regions
            .values()
            .map(|region| region.buffer_object_count() as u64)
            .sum();
        self.perf_recorder.record(
            crate::perf::ScopeId::RenderPrepareTerrain,
            terrain_prepare_started.elapsed(),
        );

        while let Ok(completed) = self.gpu_completion_rx.try_recv() {
            self.frame_resource_pool.complete(completed);
        }
        let frame_submission_id = self.next_gpu_submission_id;
        self.next_gpu_submission_id = self.next_gpu_submission_id.wrapping_add(1).max(1);
        self.frame_ring_index = loop {
            match self.frame_resource_pool.acquire(frame_submission_id) {
                Ok(lease) => break lease.slot_id,
                Err(crate::gpu_frame_resources::AcquireError::Exhausted { .. }) => {
                    // The bounded three-slot pool never overwrites GPU-owned
                    // instance data. Waiting is rare (only when CPU outruns all
                    // configured frames in flight) and completion callbacks
                    // reclaim the exact submission's slot.
                    self.device.poll(wgpu::Maintain::Wait);
                    while let Ok(completed) = self.gpu_completion_rx.try_recv() {
                        self.frame_resource_pool.complete(completed);
                    }
                }
            }
        };

        self.entity_los_manager.counters = crate::culling::CullingCounters::default();
        self.entity_los_manager
            .set_current_identity(crate::culling::LosIdentity {
                dimension: self.current_dimension,
                generation: self.terrain_generation,
                world_revision: self.los_world_revision,
            });
        // Poll entity LOS async results
        self.entity_los_manager.poll_results();

        // Compile mob instance data with culling hierarchy
        let entity_prepare_started = Instant::now();
        self.mob_cuboid_instances_scratch.clear();
        self.mob_quad_instances_scratch.clear();

        let mut entities_rendered = 0u64;
        let mut entities_frustum_culled = 0u64;
        let mut entities_occlusion_culled = 0u64;

        let cam_cell = (
            cam_pos.x.floor() as i32,
            cam_pos.y.floor() as i32,
            cam_pos.z.floor() as i32,
        );

        for entity in self
            .entity_manager
            .query_radius(cam_pos, render_distance_sq.sqrt())
        {
            // 1. Distance check
            let entity_render_dist_sq = render_distance_sq
                * (self.settings.entity_distance_scale * self.settings.entity_distance_scale);
            let dist_sq = entity.position.distance_squared(cam_pos);
            if dist_sq > entity_render_dist_sq {
                self.entity_los_manager.counters.distance += 1;
                continue;
            }

            // 2. Frustum check
            let aabb = entity.get_aabb();
            let bounds = crate::chunk_render::MeshBounds::new(aabb.min, aabb.max);
            if !frustum.intersects_aabb(&bounds) {
                entities_frustum_culled += 1;
                self.entity_los_manager.counters.frustum += 1;
                continue;
            }

            // 3. Section visibility check
            let sec_x = (entity.position.x / 16.0).floor() as i32;
            let sec_y = (entity.position.y / 16.0).floor() as i32;
            let sec_z = (entity.position.z / 16.0).floor() as i32;

            if !fail_open_section_vis {
                let valid_y = sec_y.clamp(0, 15) as usize;
                if !self
                    .visible_sections_scratch
                    .contains(&(sec_x, valid_y, sec_z))
                {
                    entities_occlusion_culled += 1;
                    self.entity_los_manager.counters.section += 1;
                    continue;
                }
            }

            // 4. Asynchronous Entity LOS check
            if !self.entity_los_manager.is_entity_visible(
                entity,
                cam_pos,
                cam_cell,
                &self.chunk_manager,
            ) {
                entities_occlusion_culled += 1;
                continue;
            }

            entities_rendered += 1;
            crate::mob_renderer::render_mobs(
                std::iter::once(entity),
                &self.chunk_manager,
                &mut self.mob_cuboid_instances_scratch,
                &mut self.mob_quad_instances_scratch,
                self.total_time,
            );
        }

        self.perf_counters.rendered_entities = entities_rendered;
        self.perf_counters.frustum_culled_entities = entities_frustum_culled;
        self.perf_counters.occlusion_culled_entities = entities_occlusion_culled;

        if self.third_person {
            crate::mob_renderer::render_local_player(
                self.player_physics.position,
                std::f32::consts::FRAC_PI_2 - self.camera.yaw,
                -self.camera.pitch,
                &self.chunk_manager,
                &mut self.mob_cuboid_instances_scratch,
                self.total_time,
                self.player_physics.velocity,
            );
        }

        self.mob_cuboid_num_instances = self.mob_cuboid_instances_scratch.len() as u32;
        self.mob_quad_num_instances = self.mob_quad_instances_scratch.len() as u32;

        if self.mob_cuboid_num_instances > 0 {
            let limit = (self.mob_cuboid_num_instances as usize).min(16384);
            self.mob_cuboid_num_instances = limit as u32;
            let upload_started = Instant::now();
            self.queue.write_buffer(
                &self.mob_cuboid_instance_buffers[self.frame_ring_index],
                0,
                bytemuck::cast_slice(&self.mob_cuboid_instances_scratch[..limit]),
            );
            let upload_elapsed = upload_started.elapsed();
            self.gpu_upload_time_frame += upload_elapsed;
            self.gpu_upload_scopes_frame
                .record(crate::perf::UploadSource::Entity as usize, upload_elapsed);
            self.perf_counters.upload_bytes_frame =
                self.perf_counters.upload_bytes_frame.saturating_add(
                    (limit * std::mem::size_of::<crate::mob_renderer::MobInstance>()) as u64,
                );
        }

        if self.mob_quad_num_instances > 0 {
            let limit = (self.mob_quad_num_instances as usize).min(4096);
            self.mob_quad_num_instances = limit as u32;
            let upload_started = Instant::now();
            self.queue.write_buffer(
                &self.mob_quad_instance_buffers[self.frame_ring_index],
                0,
                bytemuck::cast_slice(&self.mob_quad_instances_scratch[..limit]),
            );
            let upload_elapsed = upload_started.elapsed();
            self.gpu_upload_time_frame += upload_elapsed;
            self.gpu_upload_scopes_frame
                .record(crate::perf::UploadSource::Entity as usize, upload_elapsed);
            self.perf_counters.upload_bytes_frame =
                self.perf_counters.upload_bytes_frame.saturating_add(
                    (limit * std::mem::size_of::<crate::mob_renderer::MobInstance>()) as u64,
                );
        }
        let mut entity_prepare_elapsed = entity_prepare_started.elapsed();

        // Compile particle instance data
        let particle_prepare_started = Instant::now();
        self.particle_instances_scratch.clear();
        self.particle_num_indices = self
            .particles
            .compile_instances(&mut self.particle_instances_scratch);
        let particle_count = self.particle_instances_scratch.len();
        if particle_count > 0 {
            let upload_started = Instant::now();
            self.queue.write_buffer(
                &self.particle_instance_buffers[self.frame_ring_index],
                0,
                bytemuck::cast_slice(&self.particle_instances_scratch),
            );
            let upload_elapsed = upload_started.elapsed();
            self.gpu_upload_time_frame += upload_elapsed;
            self.gpu_upload_scopes_frame
                .record(crate::perf::UploadSource::Particle as usize, upload_elapsed);
            self.perf_counters.upload_bytes_frame =
                self.perf_counters.upload_bytes_frame.saturating_add(
                    (particle_count * std::mem::size_of::<crate::particles::ParticleInstance>())
                        as u64,
                );
        }
        let particle_prepare_elapsed = particle_prepare_started.elapsed();
        self.perf_recorder.record(
            crate::perf::ScopeId::RenderPrepareParticles,
            particle_prepare_elapsed,
        );

        // Compile first-person hand mesh in view space. Hidden in third-person.
        let hand_prepare_started = Instant::now();
        if !self.third_person {
            let speed_2d = Vec3::new(
                self.player_physics.velocity.x,
                0.0,
                self.player_physics.velocity.z,
            )
            .length();
            let walking = speed_2d > 0.1;
            let walk_swing = if walking {
                (self.total_time * 8.0).sin() * 0.6
            } else {
                0.0
            };
            let attack_swing = if self.left_mouse_pressed { 1.0 } else { 0.0 };
            let mesh_key = crate::hand_renderer::hand_mesh_key(&self.inventory);
            if crate::hand_renderer::should_rebuild_hand_mesh(self.last_hand_mesh_key, mesh_key) {
                self.last_hand_mesh_key = Some(mesh_key);
                crate::hand_renderer::build_first_person_hand_base_mesh(
                    mesh_key,
                    &mut self.hand_vertices_scratch,
                    &mut self.hand_indices_scratch,
                );
                let hand_indices_len = self.hand_indices_scratch.len();
                self.hand_num_indices = hand_indices_len as u32;
                if hand_indices_len > 0 {
                    let vert_limit = self.hand_vertices_scratch.len().min(1024);
                    let ind_limit = hand_indices_len.min(1536);
                    self.hand_num_indices = ind_limit as u32;
                    let upload_started = Instant::now();
                    self.queue.write_buffer(
                        &self.hand_vertex_buffer,
                        0,
                        bytemuck::cast_slice(&self.hand_vertices_scratch[..vert_limit]),
                    );
                    self.queue.write_buffer(
                        &self.hand_index_buffer,
                        0,
                        bytemuck::cast_slice(&self.hand_indices_scratch[..ind_limit]),
                    );
                    let hand_upload_elapsed = upload_started.elapsed();
                    gpu_upload_elapsed += hand_upload_elapsed;
                    self.gpu_upload_scopes_frame.record(
                        crate::perf::UploadSource::Entity as usize,
                        hand_upload_elapsed,
                    );
                    self.perf_counters.upload_bytes_frame =
                        self.perf_counters.upload_bytes_frame.saturating_add(
                            (vert_limit * std::mem::size_of::<Vertex>()
                                + ind_limit * std::mem::size_of::<u32>())
                                as u64,
                        );
                }
            }

            // Animation is a per-frame uniform transform over the cached base
            // mesh; walking and attacking never regenerate or upload vertices.
            let animation =
                crate::hand_renderer::HandAnimationUniform::from_swings(walk_swing, attack_swing);
            let aspect = self.size.width.max(1) as f32 / self.size.height.max(1) as f32;
            let hand_proj = Mat4::perspective_lh(f32::to_radians(70.0), aspect, 0.01, 10.0);
            let combined = hand_proj * animation.matrix();
            let mut hand_uniform = crate::camera::CameraUniform::new();
            hand_uniform.view_proj = combined.to_cols_array_2d();
            hand_uniform.inv_view_proj = combined.inverse().to_cols_array_2d();
            hand_uniform.camera_pos = [0.0, 0.0, 0.0, 0.0];
            let upload_started = Instant::now();
            self.queue.write_buffer(
                &self.hand_camera_buffer,
                0,
                bytemuck::bytes_of(&hand_uniform),
            );
            let elapsed = upload_started.elapsed();
            gpu_upload_elapsed += elapsed;
            self.gpu_upload_scopes_frame
                .record(crate::perf::UploadSource::Entity as usize, elapsed);
            self.perf_counters.upload_bytes_frame = self
                .perf_counters
                .upload_bytes_frame
                .saturating_add(std::mem::size_of::<crate::camera::CameraUniform>() as u64);
        }
        entity_prepare_elapsed += hand_prepare_started.elapsed();
        self.perf_recorder.record(
            crate::perf::ScopeId::RenderPrepareEntities,
            entity_prepare_elapsed,
        );

        let ui_prepare_started = Instant::now();
        let mut ui_vertices = std::mem::take(&mut self.ui_vertices_scratch);
        let mut ui_line_vertices = std::mem::take(&mut self.ui_line_vertices_scratch);
        ui_vertices.clear();
        ui_line_vertices.clear();
        if self.is_saving || self.save_error.is_some() {
            let bg_color = [0.1, 0.1, 0.1, 0.75];
            ui_vertices.push(UiVertex {
                position: [-1.0, 1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [-1.0, -1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [1.0, -1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [-1.0, 1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [1.0, -1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [1.0, 1.0, 0.0],
                color: bg_color,
            });

            if self.save_error.is_some() {
                let [mouse_x, mouse_y] = self.mouse_ndc;
                for (y0, y1) in [(0.02, 0.12), (-0.16, -0.06)] {
                    let hovered = (-0.3..=0.3).contains(&mouse_x) && (y0..=y1).contains(&mouse_y);
                    add_ui_quad(
                        &mut ui_vertices,
                        -0.3,
                        0.3,
                        y0,
                        y1,
                        if hovered {
                            [0.45, 0.18, 0.14, 1.0]
                        } else {
                            [0.22, 0.08, 0.07, 1.0]
                        },
                    );
                    add_ui_border(
                        &mut ui_line_vertices,
                        -0.3,
                        0.3,
                        y0,
                        y1,
                        [0.9, 0.55, 0.45, 1.0],
                    );
                }
            }

            let draw_centered_text =
                |s: &str,
                 y: f32,
                 char_w: f32,
                 char_h: f32,
                 spacing: f32,
                 color: [f32; 4],
                 vertices: &mut Vec<UiVertex>| {
                    let upper = s.to_uppercase();
                    let n = upper.len() as f32;
                    let width = n * char_w + (n - 1.0) * spacing;
                    let start_x = -width / 2.0;
                    add_string_lines(&upper, start_x, y, char_w, char_h, spacing, color, vertices);
                };

            if let Some(error) = &self.save_error {
                draw_centered_text(
                    "SAVE FAILED",
                    0.38,
                    0.03,
                    0.06,
                    0.012,
                    [1.0, 0.35, 0.28, 1.0],
                    &mut ui_line_vertices,
                );
                let reason: String = error.chars().take(56).collect();
                draw_centered_text(
                    &reason,
                    0.25,
                    0.015,
                    0.03,
                    0.006,
                    [1.0, 0.8, 0.7, 1.0],
                    &mut ui_line_vertices,
                );
                draw_centered_text(
                    "RETRY",
                    0.05,
                    0.025,
                    0.05,
                    0.01,
                    [1.0, 1.0, 1.0, 1.0],
                    &mut ui_line_vertices,
                );
                draw_centered_text(
                    "QUIT WITHOUT SAVING",
                    -0.13,
                    0.018,
                    0.036,
                    0.007,
                    [1.0, 1.0, 1.0, 1.0],
                    &mut ui_line_vertices,
                );
            } else {
                draw_centered_text(
                    "SAVING WORLD...",
                    0.0,
                    0.03,
                    0.06,
                    0.012,
                    [1.0, 1.0, 1.0, 1.0],
                    &mut ui_line_vertices,
                );
            }

            let ui_vert_len = ui_vertices.len().min(4096);
            let ui_line_vert_len = ui_line_vertices.len().min(4096);

            let upload_started = Instant::now();
            self.queue.write_buffer(
                &self.ui_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_vertices[..ui_vert_len]),
            );
            self.queue.write_buffer(
                &self.ui_line_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_line_vertices[..ui_line_vert_len]),
            );
            let upload_elapsed = upload_started.elapsed();
            self.gpu_upload_time_frame += upload_elapsed;
            self.gpu_upload_scopes_frame
                .record(crate::perf::UploadSource::Ui as usize, upload_elapsed);
            self.perf_counters.upload_bytes_frame =
                self.perf_counters.upload_bytes_frame.saturating_add(
                    ((ui_vert_len * std::mem::size_of::<UiVertex>())
                        + (ui_line_vert_len * std::mem::size_of::<UiVertex>()))
                        as u64,
                );

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
            self.num_ui_textured_vertices = 0;
        } else if self.connection_lost {
            let [mouse_x, mouse_y] = self.mouse_ndc;
            let button_hover = (-0.3..=0.3).contains(&mouse_x) && (-0.10..=0.00).contains(&mouse_y);

            add_ui_quad(
                &mut ui_vertices,
                -1.0,
                1.0,
                -1.0,
                1.0,
                [0.04, 0.02, 0.02, 0.82],
            );
            add_ui_quad(
                &mut ui_vertices,
                -0.3,
                0.3,
                -0.10,
                0.00,
                if button_hover {
                    [0.45, 0.18, 0.14, 1.0]
                } else {
                    [0.22, 0.08, 0.07, 1.0]
                },
            );
            add_ui_border(
                &mut ui_line_vertices,
                -0.3,
                0.3,
                -0.10,
                0.00,
                if button_hover {
                    [1.0, 1.0, 1.0, 1.0]
                } else {
                    [0.75, 0.35, 0.3, 1.0]
                },
            );

            let mut draw_centered =
                |text: &str, y: f32, char_w: f32, char_h: f32, spacing: f32, color: [f32; 4]| {
                    let text = text.to_uppercase();
                    let width = text.chars().count() as f32 * (char_w + spacing) - spacing;
                    add_string_lines(
                        &text,
                        -width / 2.0,
                        y,
                        char_w,
                        char_h,
                        spacing,
                        color,
                        &mut ui_line_vertices,
                    );
                };
            draw_centered(
                "CONNECTION LOST",
                0.26,
                0.030,
                0.060,
                0.010,
                [1.0, 0.35, 0.28, 1.0],
            );
            if let Some(status) = &self.network_status {
                let reason: String = status
                    .strip_prefix("CONNECTION LOST: ")
                    .unwrap_or(status)
                    .chars()
                    .take(64)
                    .collect();
                draw_centered(&reason, 0.12, 0.012, 0.024, 0.005, [0.92, 0.92, 0.92, 1.0]);
            }
            draw_centered(
                "RETURN TO MENU",
                -0.07,
                0.020,
                0.040,
                0.008,
                [1.0, 1.0, 1.0, 1.0],
            );

            let ui_vert_len = ui_vertices.len().min(UI_VERTEX_CAPACITY);
            let ui_line_vert_len = ui_line_vertices.len().min(UI_LINE_VERTEX_CAPACITY);
            let upload_started = Instant::now();
            self.queue.write_buffer(
                &self.ui_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_vertices[..ui_vert_len]),
            );
            self.queue.write_buffer(
                &self.ui_line_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_line_vertices[..ui_line_vert_len]),
            );
            let upload_elapsed = upload_started.elapsed();
            self.gpu_upload_time_frame += upload_elapsed;
            self.gpu_upload_scopes_frame
                .record(crate::perf::UploadSource::Ui as usize, upload_elapsed);
            self.perf_counters.upload_bytes_frame =
                self.perf_counters.upload_bytes_frame.saturating_add(
                    ((ui_vert_len + ui_line_vert_len) * std::mem::size_of::<UiVertex>()) as u64,
                );
            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
            self.num_ui_textured_vertices = 0;
        } else if self.player_state.is_dead {
            let mouse_x = self.mouse_ndc[0];
            let mouse_y = self.mouse_ndc[1];

            // Respawn button hover (X: [-0.3, 0.3], Y: [-0.1, 0.0])
            let respawn_hover =
                mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.1 && mouse_y <= 0.0;

            // Reddish overlay
            let bg_color = [0.4, 0.0, 0.0, 0.6];
            ui_vertices.push(UiVertex {
                position: [-1.0, 1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [-1.0, -1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [1.0, -1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [-1.0, 1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [1.0, -1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [1.0, 1.0, 0.0],
                color: bg_color,
            });

            // Button background
            let btn_bg = if respawn_hover {
                [0.4, 0.1, 0.1, 1.0]
            } else {
                [0.2, 0.0, 0.0, 1.0]
            };
            let btn_border = if respawn_hover {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [0.6, 0.2, 0.2, 1.0]
            };
            let btn_y_min = -0.10;
            let btn_y_max = 0.00;

            ui_vertices.push(UiVertex {
                position: [-0.3, btn_y_max, 0.0],
                color: btn_bg,
            });
            ui_vertices.push(UiVertex {
                position: [-0.3, btn_y_min, 0.0],
                color: btn_bg,
            });
            ui_vertices.push(UiVertex {
                position: [0.3, btn_y_min, 0.0],
                color: btn_bg,
            });
            ui_vertices.push(UiVertex {
                position: [-0.3, btn_y_max, 0.0],
                color: btn_bg,
            });
            ui_vertices.push(UiVertex {
                position: [0.3, btn_y_min, 0.0],
                color: btn_bg,
            });
            ui_vertices.push(UiVertex {
                position: [0.3, btn_y_max, 0.0],
                color: btn_bg,
            });

            // Button border
            ui_line_vertices.push(UiVertex {
                position: [-0.3, btn_y_max, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [0.3, btn_y_max, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [0.3, btn_y_max, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [0.3, btn_y_min, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [0.3, btn_y_min, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [-0.3, btn_y_min, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [-0.3, btn_y_min, 0.0],
                color: btn_border,
            });
            ui_line_vertices.push(UiVertex {
                position: [-0.3, btn_y_max, 0.0],
                color: btn_border,
            });

            let draw_centered_text =
                |s: &str,
                 y: f32,
                 char_w: f32,
                 char_h: f32,
                 spacing: f32,
                 color: [f32; 4],
                 vertices: &mut Vec<UiVertex>| {
                    let upper = s.to_uppercase();
                    let n = upper.len() as f32;
                    let width = n * char_w + (n - 1.0) * spacing;
                    let start_x = -width / 2.0;
                    add_string_lines(&upper, start_x, y, char_w, char_h, spacing, color, vertices);
                };

            draw_centered_text(
                "YOU DIED!",
                0.30,
                0.04,
                0.08,
                0.015,
                [1.0, 0.2, 0.2, 1.0],
                &mut ui_line_vertices,
            );

            let msg = match self.player_state.death_reason {
                Some(DamageSource::Fall) => "FELL FROM A HIGH PLACE",
                Some(DamageSource::Void) => "FELL INTO THE VOID",
                Some(DamageSource::Hunger) => "STARVED TO DEATH",
                Some(DamageSource::Mob) => "WAS SLAIN BY ZOMBIE/SKELETON",
                Some(DamageSource::Explosion) => "WAS BLOWN UP BY CREEPER",
                Some(DamageSource::Drowning) => "DROWNED",
                Some(DamageSource::Lightning) => "WAS STRUCK BY LIGHTNING",
                None => "DIED",
            };
            draw_centered_text(
                msg,
                0.15,
                0.015,
                0.03,
                0.006,
                [1.0, 1.0, 1.0, 1.0],
                &mut ui_line_vertices,
            );
            draw_centered_text(
                "RESPAWN",
                -0.06,
                0.02,
                0.04,
                0.008,
                [1.0, 1.0, 1.0, 1.0],
                &mut ui_line_vertices,
            );

            let ui_vert_len = ui_vertices.len().min(4096);
            let ui_line_vert_len = ui_line_vertices.len().min(4096);

            let upload_started = Instant::now();
            self.queue.write_buffer(
                &self.ui_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_vertices[..ui_vert_len]),
            );
            self.queue.write_buffer(
                &self.ui_line_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_line_vertices[..ui_line_vert_len]),
            );
            let upload_elapsed = upload_started.elapsed();
            self.gpu_upload_time_frame += upload_elapsed;
            self.gpu_upload_scopes_frame
                .record(crate::perf::UploadSource::Ui as usize, upload_elapsed);
            self.perf_counters.upload_bytes_frame =
                self.perf_counters.upload_bytes_frame.saturating_add(
                    ((ui_vert_len + ui_line_vert_len) * std::mem::size_of::<UiVertex>()) as u64,
                );

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
            self.num_ui_textured_vertices = 0;
        } else if self.is_paused {
            let mouse_x = self.mouse_ndc[0];
            let mouse_y = self.mouse_ndc[1];

            // Hover states
            let resume_hover =
                mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= 0.24 && mouse_y <= 0.34;
            let fov_hover = mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= 0.10 && mouse_y <= 0.20;
            let sens_hover =
                mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.04 && mouse_y <= 0.06;
            let rd_hover =
                mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.18 && mouse_y <= -0.08;
            let vol_hover =
                mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.32 && mouse_y <= -0.22;
            let weather_vol_hover = point_in_bounds(mouse_x, mouse_y, PAUSE_WEATHER_VOLUME_BOUNDS);
            let quit_hover = point_in_bounds(mouse_x, mouse_y, PAUSE_QUIT_BOUNDS);

            // 1. Dark overlay (screen covers from -1.0 to 1.0)
            let bg_color = [0.1, 0.1, 0.1, 0.7];
            ui_vertices.push(UiVertex {
                position: [-1.0, 1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [-1.0, -1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [1.0, -1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [-1.0, 1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [1.0, -1.0, 0.0],
                color: bg_color,
            });
            ui_vertices.push(UiVertex {
                position: [1.0, 1.0, 0.0],
                color: bg_color,
            });

            // Button drawing helper
            let draw_button = |hover: bool,
                               y_min: f32,
                               y_max: f32,
                               ui_verts: &mut Vec<UiVertex>,
                               ui_line_verts: &mut Vec<UiVertex>| {
                let bg = if hover {
                    [0.4, 0.4, 0.4, 1.0]
                } else {
                    [0.2, 0.2, 0.2, 1.0]
                };
                let border = if hover {
                    [1.0, 1.0, 1.0, 1.0]
                } else {
                    [0.6, 0.6, 0.6, 1.0]
                };

                // Background (two triangles)
                ui_verts.push(UiVertex {
                    position: [-0.3, y_max, 0.0],
                    color: bg,
                });
                ui_verts.push(UiVertex {
                    position: [-0.3, y_min, 0.0],
                    color: bg,
                });
                ui_verts.push(UiVertex {
                    position: [0.3, y_min, 0.0],
                    color: bg,
                });
                ui_verts.push(UiVertex {
                    position: [-0.3, y_max, 0.0],
                    color: bg,
                });
                ui_verts.push(UiVertex {
                    position: [0.3, y_min, 0.0],
                    color: bg,
                });
                ui_verts.push(UiVertex {
                    position: [0.3, y_max, 0.0],
                    color: bg,
                });

                // Border (line loop)
                ui_line_verts.push(UiVertex {
                    position: [-0.3, y_max, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [0.3, y_max, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [0.3, y_max, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [0.3, y_min, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [0.3, y_min, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [-0.3, y_min, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [-0.3, y_min, 0.0],
                    color: border,
                });
                ui_line_verts.push(UiVertex {
                    position: [-0.3, y_max, 0.0],
                    color: border,
                });
            };

            // Draw Button backgrounds and borders
            draw_button(
                resume_hover,
                0.24,
                0.34,
                &mut ui_vertices,
                &mut ui_line_vertices,
            );
            draw_button(
                fov_hover,
                0.10,
                0.20,
                &mut ui_vertices,
                &mut ui_line_vertices,
            );
            draw_button(
                sens_hover,
                -0.04,
                0.06,
                &mut ui_vertices,
                &mut ui_line_vertices,
            );
            draw_button(
                rd_hover,
                -0.18,
                -0.08,
                &mut ui_vertices,
                &mut ui_line_vertices,
            );
            draw_button(
                vol_hover,
                -0.32,
                -0.22,
                &mut ui_vertices,
                &mut ui_line_vertices,
            );
            draw_button(
                weather_vol_hover,
                PAUSE_WEATHER_VOLUME_BOUNDS[2],
                PAUSE_WEATHER_VOLUME_BOUNDS[3],
                &mut ui_vertices,
                &mut ui_line_vertices,
            );
            draw_button(
                quit_hover,
                PAUSE_QUIT_BOUNDS[2],
                PAUSE_QUIT_BOUNDS[3],
                &mut ui_vertices,
                &mut ui_line_vertices,
            );

            // Centered text drawing helper
            let draw_centered_text =
                |s: &str,
                 y: f32,
                 char_w: f32,
                 char_h: f32,
                 spacing: f32,
                 color: [f32; 4],
                 vertices: &mut Vec<UiVertex>| {
                    let upper = s.to_uppercase();
                    let n = upper.len() as f32;
                    let width = n * char_w + (n - 1.0) * spacing;
                    let start_x = -width / 2.0;
                    add_string_lines(&upper, start_x, y, char_w, char_h, spacing, color, vertices);
                };

            // Render Text Labels
            let text_color = [1.0, 1.0, 1.0, 1.0];
            // "GAME PAUSED"
            draw_centered_text(
                "GAME PAUSED",
                0.40,
                0.03,
                0.06,
                0.012,
                text_color,
                &mut ui_line_vertices,
            );
            if let Some(status) = &self.network_status {
                draw_centered_text(
                    status,
                    0.52,
                    0.014,
                    0.028,
                    0.006,
                    [1.0, 0.45, 0.35, 1.0],
                    &mut ui_line_vertices,
                );
            }
            // "RESUME"
            draw_centered_text(
                "RESUME",
                0.28,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // "FOV < value >"
            let fov_text = format!("FOV < {:.0} >", self.base_fov);
            draw_centered_text(
                &fov_text,
                0.14,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // "SENS < value >"
            let sens_val = (self.sensitivity / 0.002 * 100.0).round();
            let sens_text = format!("SENS < {:.0} >", sens_val);
            draw_centered_text(
                &sens_text,
                0.00,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // "RENDER DISTANCE < value >"
            let rd_text = format!("RENDER DISTANCE < {} >", self.chunk_manager.render_distance);
            draw_centered_text(
                &rd_text,
                -0.14,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // "MASTER VOLUME < value >"
            let vol_text = format!(
                "MASTER VOLUME < {:.0}% >",
                self.settings.master_volume * 100.0
            );
            draw_centered_text(
                &vol_text,
                -0.28,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // "WEATHER VOLUME < value >"
            let weather_vol_text = format!(
                "WEATHER VOLUME < {:.0}% >",
                self.settings.weather_volume * 100.0
            );
            draw_centered_text(
                &weather_vol_text,
                -0.42,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // "SAVE AND QUIT"
            draw_centered_text(
                "SAVE AND QUIT",
                -0.56,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // Cap the sizes to the preallocated buffers (4096 vertices)
            let ui_vert_len = ui_vertices.len().min(4096);
            let ui_line_vert_len = ui_line_vertices.len().min(4096);

            let upload_started = Instant::now();
            self.queue.write_buffer(
                &self.ui_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_vertices[..ui_vert_len]),
            );
            self.queue.write_buffer(
                &self.ui_line_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_line_vertices[..ui_line_vert_len]),
            );

            let upload_elapsed = upload_started.elapsed();
            self.gpu_upload_time_frame += upload_elapsed;
            self.gpu_upload_scopes_frame
                .record(crate::perf::UploadSource::Ui as usize, upload_elapsed);
            self.perf_counters.upload_bytes_frame =
                self.perf_counters.upload_bytes_frame.saturating_add(
                    ((ui_vert_len + ui_line_vert_len) * std::mem::size_of::<UiVertex>()) as u64,
                );

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
        } else {
            let mut ui_textured_vertices = Vec::new();

            let aspect = self.size.width as f32 / self.size.height as f32;
            let slot_w = 0.08;
            let slot_h = 0.08 * aspect;
            let gap = 0.01;
            let start_x = -0.40;

            let draw_durability_bar =
                |stack: &ItemStack,
                 x0: f32,
                 x1: f32,
                 y0: f32,
                 y1: f32,
                 _aspect: f32,
                 ui_vertices: &mut Vec<UiVertex>| {
                    if let Some(tool_prop) = stack.item.tool_properties() {
                        let max_dur = tool_prop.durability;
                        if stack.durability < max_dur {
                            let ratio = (stack.durability as f32 / max_dur as f32).clamp(0.0, 1.0);

                            // Define bar bounds relative to slot size
                            let slot_w = x1 - x0;
                            let slot_h = y1 - y0;

                            let bar_x0 = x0 + slot_w * 0.15;
                            let bar_x1 = x1 - slot_w * 0.15;
                            let bar_y0 = y0 + slot_h * 0.10;
                            let bar_y1 = y0 + slot_h * 0.16;

                            // 1. Black background bar
                            let bg_color = [0.0, 0.0, 0.0, 1.0];
                            ui_vertices.push(UiVertex {
                                position: [bar_x0, bar_y1, 0.0],
                                color: bg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [bar_x0, bar_y0, 0.0],
                                color: bg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [bar_x1, bar_y0, 0.0],
                                color: bg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [bar_x0, bar_y1, 0.0],
                                color: bg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [bar_x1, bar_y0, 0.0],
                                color: bg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [bar_x1, bar_y1, 0.0],
                                color: bg_color,
                            });

                            // 2. Colored foreground bar
                            let fg_x1 = bar_x0 + (bar_x1 - bar_x0) * ratio;
                            let (r, g) = if ratio > 0.5 {
                                ((1.0 - ratio) * 2.0, 1.0)
                            } else {
                                (1.0, ratio * 2.0)
                            };
                            let fg_color = [r, g, 0.0, 1.0];

                            ui_vertices.push(UiVertex {
                                position: [bar_x0, bar_y1, 0.0],
                                color: fg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [bar_x0, bar_y0, 0.0],
                                color: fg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [fg_x1, bar_y0, 0.0],
                                color: fg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [bar_x0, bar_y1, 0.0],
                                color: fg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [fg_x1, bar_y0, 0.0],
                                color: fg_color,
                            });
                            ui_vertices.push(UiVertex {
                                position: [fg_x1, bar_y1, 0.0],
                                color: fg_color,
                            });
                        }
                    }
                };

            if self.inventory.is_open {
                let creative_catalog = self.is_creative_catalog_open();
                // 1. Dark overlay (screen covers from -1.0 to 1.0)
                let bg_color = [0.08, 0.08, 0.08, 0.6];
                ui_vertices.push(UiVertex {
                    position: [-1.0, 1.0, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [-1.0, -1.0, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [1.0, -1.0, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [-1.0, 1.0, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [1.0, -1.0, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [1.0, 1.0, 0.0],
                    color: bg_color,
                });

                if creative_catalog {
                    add_ui_quad(
                        &mut ui_vertices,
                        -0.49,
                        0.51,
                        -0.92,
                        0.92,
                        [0.10, 0.10, 0.10, 0.96],
                    );
                    add_ui_border(
                        &mut ui_line_vertices,
                        -0.49,
                        0.51,
                        -0.92,
                        0.92,
                        [0.52, 0.52, 0.52, 1.0],
                    );

                    for (index, tab) in CreativeTab::TABS.into_iter().enumerate() {
                        let rect = creative_tab_rect(index);
                        let hovered = rect.contains(self.mouse_ndc[0], self.mouse_ndc[1]);
                        let selected = tab == self.inventory.creative_tab;
                        add_ui_quad(
                            &mut ui_vertices,
                            rect.x0,
                            rect.x1,
                            rect.y0,
                            rect.y1,
                            if selected {
                                [0.30, 0.42, 0.22, 1.0]
                            } else if hovered {
                                [0.34, 0.34, 0.34, 1.0]
                            } else {
                                [0.18, 0.18, 0.18, 1.0]
                            },
                        );
                        add_ui_border(
                            &mut ui_line_vertices,
                            rect.x0,
                            rect.x1,
                            rect.y0,
                            rect.y1,
                            if selected || hovered {
                                [0.95, 0.95, 0.95, 1.0]
                            } else {
                                [0.42, 0.42, 0.42, 1.0]
                            },
                        );
                        let label = tab.label();
                        let char_w = 0.005;
                        let spacing = 0.0015;
                        let label_w = label.chars().count() as f32 * (char_w + spacing) - spacing;
                        add_string_lines(
                            label,
                            (rect.x0 + rect.x1 - label_w) * 0.5,
                            rect.y0 + 0.035,
                            char_w,
                            0.020,
                            spacing,
                            [1.0, 1.0, 1.0, 1.0],
                            &mut ui_line_vertices,
                        );
                    }

                    let track = creative_scroll_track_rect(aspect);
                    add_ui_quad(
                        &mut ui_vertices,
                        track.x0,
                        track.x1,
                        track.y0,
                        track.y1,
                        [0.035, 0.035, 0.035, 1.0],
                    );
                    add_ui_border(
                        &mut ui_line_vertices,
                        track.x0,
                        track.x1,
                        track.y0,
                        track.y1,
                        [0.34, 0.34, 0.34, 1.0],
                    );
                    let max_scroll = self.inventory.creative_max_scroll();
                    let total_rows = max_scroll + CREATIVE_ROWS;
                    let track_height = track.y1 - track.y0;
                    let thumb_height = if max_scroll == 0 {
                        track_height
                    } else {
                        (track_height * CREATIVE_ROWS as f32 / total_rows as f32).max(0.06)
                    };
                    let progress = if max_scroll == 0 {
                        0.0
                    } else {
                        self.inventory.creative_scroll_row as f32 / max_scroll as f32
                    };
                    let thumb_y1 = track.y1 - progress * (track_height - thumb_height);
                    add_ui_quad(
                        &mut ui_vertices,
                        track.x0 + 0.004,
                        track.x1 - 0.004,
                        thumb_y1 - thumb_height,
                        thumb_y1,
                        [0.68, 0.68, 0.68, 1.0],
                    );
                }

                // 2. Draw slots
                let slots = self.get_inventory_slots();
                let mouse_x = self.mouse_ndc[0];
                let mouse_y = self.mouse_ndc[1];
                let mut hovered_slot = None;

                for &(slot_type, x0, x1, y0, y1) in &slots {
                    let is_hovered =
                        mouse_x >= x0 && mouse_x <= x1 && mouse_y >= y0 && mouse_y <= y1;
                    if is_hovered {
                        hovered_slot = Some((slot_type, x0, x1, y0, y1));
                    }

                    // Background Quad
                    let slot_bg_color = if is_hovered {
                        [0.35, 0.35, 0.35, 0.8]
                    } else {
                        [0.15, 0.15, 0.15, 0.8]
                    };
                    ui_vertices.push(UiVertex {
                        position: [x0, y1, 0.0],
                        color: slot_bg_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [x0, y0, 0.0],
                        color: slot_bg_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [x1, y0, 0.0],
                        color: slot_bg_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [x0, y1, 0.0],
                        color: slot_bg_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [x1, y0, 0.0],
                        color: slot_bg_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [x1, y1, 0.0],
                        color: slot_bg_color,
                    });

                    // Borders
                    let border_color = match slot_type {
                        SlotType::Hotbar(idx) if idx == self.inventory.selected => {
                            [1.0, 1.0, 1.0, 1.0]
                        }
                        _ => [0.3, 0.3, 0.3, 0.8],
                    };
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y1, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y1, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y1, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y1, 0.0],
                        color: border_color,
                    });

                    // Slot Item
                    if let Some(stack) = self.get_item_at_slot(slot_type) {
                        let (col, row) = stack.item.properties().tex_coords;
                        let u0 = col as f32 * 0.0625;
                        let u1 = (col + 1) as f32 * 0.0625;
                        let v0 = row as f32 * 0.0625;
                        let v1 = (row + 1) as f32 * 0.0625;

                        let margin_x = 0.015;
                        let margin_y = 0.015 * aspect;
                        let tx0 = x0 + margin_x;
                        let tx1 = x1 - margin_x;
                        let ty0 = y0 + margin_y;
                        let ty1 = y1 - margin_y;

                        let c = if stack.enchantments.is_empty() {
                            [1.0, 1.0, 1.0, 1.0]
                        } else {
                            let pulse = 0.72 + (self.total_time * 3.0).sin() * 0.18;
                            [0.82, pulse, 1.0, 1.0]
                        };
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx0, ty1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx0, ty0, 0.0],
                            tex_coords: [u0, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx1, ty0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx0, ty1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx1, ty0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx1, ty1, 0.0],
                            tex_coords: [u1, v0],
                            color: c,
                        });

                        if stack.count > 1 {
                            let count_str = format!("{}", stack.count);
                            let cw = 0.008;
                            let ch = 0.016;
                            let cs = 0.003;
                            let n_chars = count_str.len() as f32;
                            let count_w = n_chars * cw + (n_chars - 1.0) * cs;
                            let count_x = x1 - count_w - 0.008;
                            let count_y = y0 + 0.01 * aspect;
                            add_string_lines(
                                &count_str,
                                count_x,
                                count_y,
                                cw,
                                ch,
                                cs,
                                [1.0, 1.0, 1.0, 1.0],
                                &mut ui_line_vertices,
                            );
                        }

                        // Draw durability bar
                        draw_durability_bar(&stack, x0, x1, y0, y1, aspect, &mut ui_vertices);
                    }
                }

                // 3. Draw crafting arrow symbol
                if !creative_catalog && self.active_station.is_none() {
                    let arrow_y = if self.inventory.is_table_open {
                        -0.10 + 1.0 * (slot_h + gap) + slot_h / 2.0
                    } else {
                        -0.05 + 0.5 * (slot_h + gap) + slot_h / 2.0
                    };
                    let arrow_x = if self.inventory.is_table_open {
                        -0.05 + 3.0 * (slot_w + gap) + 0.015
                    } else {
                        0.05 + 2.0 * (slot_w + gap) + 0.015
                    };
                    let ac = [0.8, 0.8, 0.8, 1.0];
                    ui_line_vertices.push(UiVertex {
                        position: [arrow_x, arrow_y, 0.0],
                        color: ac,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [arrow_x + 0.03, arrow_y, 0.0],
                        color: ac,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [arrow_x + 0.03, arrow_y, 0.0],
                        color: ac,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [arrow_x + 0.02, arrow_y + 0.01 * aspect, 0.0],
                        color: ac,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [arrow_x + 0.03, arrow_y, 0.0],
                        color: ac,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [arrow_x + 0.02, arrow_y - 0.01 * aspect, 0.0],
                        color: ac,
                    });
                }

                // 4. Draw texts (Labels)
                if creative_catalog {
                    add_string_lines(
                        "CREATIVE INVENTORY",
                        -0.45,
                        0.70,
                        0.010,
                        0.020,
                        0.003,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );
                    add_string_lines(
                        "HOTBAR",
                        -0.45,
                        -0.67,
                        0.008,
                        0.016,
                        0.003,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );
                } else {
                    add_string_lines(
                        "INVENTORY",
                        -0.40,
                        -0.70 + 3.0 * (slot_h + gap) + 0.02,
                        0.008,
                        0.016,
                        0.003,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );
                    if self.active_station.is_none() {
                        let craft_lbl_x = if self.inventory.is_table_open {
                            -0.05
                        } else {
                            0.05
                        };
                        let craft_lbl_y = if self.inventory.is_table_open {
                            -0.10 + 3.0 * (slot_h + gap) + 0.02
                        } else {
                            -0.05 + 2.0 * (slot_h + gap) + 0.02
                        };
                        add_string_lines(
                            "CRAFTING",
                            craft_lbl_x,
                            craft_lbl_y,
                            0.008,
                            0.016,
                            0.003,
                            [1.0, 1.0, 1.0, 1.0],
                            &mut ui_line_vertices,
                        );
                    }
                }

                match self.active_station {
                    Some(StationKind::Enchanting) => {
                        add_string_lines(
                            "ENCHANTING",
                            -0.18,
                            0.37,
                            0.012,
                            0.024,
                            0.004,
                            [0.75, 0.45, 1.0, 1.0],
                            &mut ui_line_vertices,
                        );
                        let level_text = format!(
                            "LEVEL {}  BOOKSHELVES {}",
                            self.player_state.experience_level, self.enchanting.bookshelves
                        );
                        add_string_lines(
                            &level_text,
                            -0.18,
                            0.31,
                            0.008,
                            0.016,
                            0.003,
                            [0.5, 1.0, 0.5, 1.0],
                            &mut ui_line_vertices,
                        );
                        for (index, option) in self.enchanting.options.iter().enumerate() {
                            let y1 = 0.28 - index as f32 * 0.12;
                            let y0 = y1 - 0.09;
                            let hovered = mouse_x >= 0.02
                                && mouse_x <= 0.62
                                && mouse_y >= y0
                                && mouse_y <= y1;
                            add_ui_quad(
                                &mut ui_vertices,
                                0.02,
                                0.62,
                                y0,
                                y1,
                                if hovered {
                                    [0.30, 0.16, 0.42, 0.95]
                                } else {
                                    [0.14, 0.07, 0.20, 0.95]
                                },
                            );
                            let enchantment =
                                option.enchantments.entries.iter().flatten().next().copied();
                            let label = enchantment
                                .map(|e| {
                                    format!(
                                        "{} {}  COST {} + {} LAPIS",
                                        e.short_name(),
                                        e.level(),
                                        option.cost,
                                        option.lapis_cost
                                    )
                                })
                                .unwrap_or_else(|| "NO ENCHANTMENT".to_string());
                            add_string_lines(
                                &label,
                                0.04,
                                y0 + 0.032,
                                0.007,
                                0.014,
                                0.002,
                                [0.8, 0.65, 1.0, 1.0],
                                &mut ui_line_vertices,
                            );
                        }
                    }
                    Some(StationKind::Brewing) => {
                        add_string_lines(
                            "BREWING STAND",
                            -0.18,
                            0.37,
                            0.012,
                            0.024,
                            0.004,
                            [0.8, 0.6, 0.3, 1.0],
                            &mut ui_line_vertices,
                        );
                        let progress = (self.brewing.progress / 10.0).clamp(0.0, 1.0);
                        add_ui_quad(
                            &mut ui_vertices,
                            0.04,
                            0.54,
                            0.20,
                            0.24,
                            [0.05, 0.05, 0.05, 1.0],
                        );
                        add_ui_quad(
                            &mut ui_vertices,
                            0.04,
                            0.04 + 0.5 * progress,
                            0.20,
                            0.24,
                            [0.85, 0.45, 0.1, 1.0],
                        );
                        let status = if self.brewing.can_brew() {
                            format!("BREWING {:.0} PCT", progress * 100.0)
                        } else {
                            "ADD BOTTLES AND INGREDIENT".to_string()
                        };
                        add_string_lines(
                            &status,
                            0.04,
                            0.28,
                            0.008,
                            0.016,
                            0.003,
                            [1.0, 0.85, 0.55, 1.0],
                            &mut ui_line_vertices,
                        );
                    }
                    Some(StationKind::Anvil) => {
                        add_string_lines(
                            "ANVIL",
                            -0.20,
                            0.37,
                            0.012,
                            0.024,
                            0.004,
                            [0.8, 0.8, 0.8, 1.0],
                            &mut ui_line_vertices,
                        );
                        add_ui_quad(
                            &mut ui_vertices,
                            -0.20,
                            0.45,
                            0.25,
                            0.31,
                            [0.04, 0.04, 0.04, 0.95],
                        );
                        let rename = if self.anvil.rename.is_empty() {
                            "TYPE A NAME"
                        } else {
                            &self.anvil.rename
                        };
                        add_string_lines(
                            rename,
                            -0.18,
                            0.27,
                            0.009,
                            0.018,
                            0.003,
                            [1.0, 1.0, 1.0, 1.0],
                            &mut ui_line_vertices,
                        );
                        let cost = format!("COST {} LEVELS", self.anvil.cost);
                        add_string_lines(
                            &cost,
                            0.20,
                            0.05,
                            0.008,
                            0.016,
                            0.003,
                            [0.5, 1.0, 0.5, 1.0],
                            &mut ui_line_vertices,
                        );
                    }
                    None => {}
                }

                // 5. Draw dragged item at cursor position
                if let Some(dragged) = self.inventory.dragged {
                    let (cursor_slot_w, cursor_slot_h) = if creative_catalog {
                        let (width, height, _, _) = creative_slot_metrics(aspect);
                        (width, height)
                    } else {
                        (slot_w, slot_h)
                    };
                    let (col, row) = dragged.item.properties().tex_coords;
                    let u0 = col as f32 * 0.0625;
                    let u1 = (col + 1) as f32 * 0.0625;
                    let v0 = row as f32 * 0.0625;
                    let v1 = (row + 1) as f32 * 0.0625;

                    let dx0 = mouse_x - cursor_slot_w / 2.0 + 0.015;
                    let dx1 = mouse_x + cursor_slot_w / 2.0 - 0.015;
                    let dy0 = mouse_y - cursor_slot_h / 2.0 + 0.015 * aspect;
                    let dy1 = mouse_y + cursor_slot_h / 2.0 - 0.015 * aspect;

                    let c = if dragged.enchantments.is_empty() {
                        [1.0, 1.0, 1.0, 1.0]
                    } else {
                        [0.82, 0.65 + (self.total_time * 3.0).sin() * 0.18, 1.0, 1.0]
                    };
                    ui_textured_vertices.push(TexturedUiVertex {
                        position: [dx0, dy1, 0.0],
                        tex_coords: [u0, v0],
                        color: c,
                    });
                    ui_textured_vertices.push(TexturedUiVertex {
                        position: [dx0, dy0, 0.0],
                        tex_coords: [u0, v1],
                        color: c,
                    });
                    ui_textured_vertices.push(TexturedUiVertex {
                        position: [dx1, dy0, 0.0],
                        tex_coords: [u1, v1],
                        color: c,
                    });
                    ui_textured_vertices.push(TexturedUiVertex {
                        position: [dx0, dy1, 0.0],
                        tex_coords: [u0, v0],
                        color: c,
                    });
                    ui_textured_vertices.push(TexturedUiVertex {
                        position: [dx1, dy0, 0.0],
                        tex_coords: [u1, v1],
                        color: c,
                    });
                    ui_textured_vertices.push(TexturedUiVertex {
                        position: [dx1, dy1, 0.0],
                        tex_coords: [u1, v0],
                        color: c,
                    });

                    if dragged.count > 1 {
                        let count_str = format!("{}", dragged.count);
                        let cw = 0.008;
                        let ch = 0.016;
                        let cs = 0.003;
                        let n_chars = count_str.len() as f32;
                        let count_w = n_chars * cw + (n_chars - 1.0) * cs;
                        let count_x = mouse_x + cursor_slot_w / 2.0 - count_w - 0.008;
                        let count_y = mouse_y - cursor_slot_h / 2.0 + 0.01 * aspect;
                        add_string_lines(
                            &count_str,
                            count_x,
                            count_y,
                            cw,
                            ch,
                            cs,
                            [1.0, 1.0, 1.0, 1.0],
                            &mut ui_line_vertices,
                        );
                    }
                }

                // 6. Draw tooltip for hovered slot
                if self.inventory.dragged.is_none() {
                    if let Some((slot_type, _, _, _, _)) = hovered_slot {
                        if let Some(stack) = self.get_item_at_slot(slot_type) {
                            let name = if !stack.custom_name.is_empty() {
                                stack.custom_name.as_str().to_string()
                            } else if let Some(potion) = stack.potion {
                                potion.display_name().to_string()
                            } else {
                                stack.item.properties().name.to_string()
                            };
                            let tw = name.len() as f32 * 0.014 + 0.02;
                            let th = 0.035 * aspect;
                            let tx = mouse_x + 0.02;
                            let ty = mouse_y + 0.02;

                            let tt_bg = [0.05, 0.05, 0.1, 0.95];
                            ui_vertices.push(UiVertex {
                                position: [tx, ty + th, 0.0],
                                color: tt_bg,
                            });
                            ui_vertices.push(UiVertex {
                                position: [tx, ty, 0.0],
                                color: tt_bg,
                            });
                            ui_vertices.push(UiVertex {
                                position: [tx + tw, ty, 0.0],
                                color: tt_bg,
                            });
                            ui_vertices.push(UiVertex {
                                position: [tx, ty + th, 0.0],
                                color: tt_bg,
                            });
                            ui_vertices.push(UiVertex {
                                position: [tx + tw, ty, 0.0],
                                color: tt_bg,
                            });
                            ui_vertices.push(UiVertex {
                                position: [tx + tw, ty + th, 0.0],
                                color: tt_bg,
                            });

                            let tt_border = [0.3, 0.3, 0.7, 1.0];
                            ui_line_vertices.push(UiVertex {
                                position: [tx, ty + th, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx + tw, ty + th, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx + tw, ty + th, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx + tw, ty, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx + tw, ty, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx, ty, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx, ty, 0.0],
                                color: tt_border,
                            });
                            ui_line_vertices.push(UiVertex {
                                position: [tx, ty + th, 0.0],
                                color: tt_border,
                            });

                            add_string_lines(
                                &name,
                                tx + 0.01,
                                ty + 0.01 * aspect,
                                0.008,
                                0.016,
                                0.003,
                                [1.0, 1.0, 1.0, 1.0],
                                &mut ui_line_vertices,
                            );
                        }
                    }
                }
            } else {
                // Background Bar
                let bg_color = [0.05, 0.05, 0.05, 0.6];
                let bg_x0 = -0.415;
                let bg_x1 = 0.415;
                let bg_y0 = -0.96;
                let bg_y1 = -0.94 + slot_h;
                ui_vertices.push(UiVertex {
                    position: [bg_x0, bg_y1, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [bg_x0, bg_y0, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [bg_x1, bg_y0, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [bg_x0, bg_y1, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [bg_x1, bg_y0, 0.0],
                    color: bg_color,
                });
                ui_vertices.push(UiVertex {
                    position: [bg_x1, bg_y1, 0.0],
                    color: bg_color,
                });

                // Slots
                for i in 0..9 {
                    let x0 = start_x + i as f32 * (slot_w + gap);
                    let x1 = x0 + slot_w;
                    let y0 = -0.95;
                    let y1 = y0 + slot_h;

                    let border_color = if i == self.inventory.selected {
                        [1.0, 1.0, 1.0, 1.0] // White for active
                    } else {
                        [0.3, 0.3, 0.3, 0.8] // Gray for inactive
                    };

                    // Push lines to ui_line_vertices (forms border box)
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y1, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y1, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y1, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x1, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y0, 0.0],
                        color: border_color,
                    });
                    ui_line_vertices.push(UiVertex {
                        position: [x0, y1, 0.0],
                        color: border_color,
                    });

                    if let Some(stack) = &self.inventory.hotbar[i] {
                        let (col, row) = stack.item.properties().tex_coords;
                        let u0 = col as f32 * 0.0625;
                        let u1 = (col + 1) as f32 * 0.0625;
                        let v0 = row as f32 * 0.0625;
                        let v1 = (row + 1) as f32 * 0.0625;

                        let margin_x = 0.015;
                        let margin_y = 0.015 * aspect;
                        let tx0 = x0 + margin_x;
                        let tx1 = x1 - margin_x;
                        let ty0 = y0 + margin_y;
                        let ty1 = y1 - margin_y;

                        let c = if stack.enchantments.is_empty() {
                            [1.0, 1.0, 1.0, 1.0]
                        } else {
                            [0.82, 0.65 + (self.total_time * 3.0).sin() * 0.18, 1.0, 1.0]
                        };
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx0, ty1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx0, ty0, 0.0],
                            tex_coords: [u0, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx1, ty0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx0, ty1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx1, ty0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [tx1, ty1, 0.0],
                            tex_coords: [u1, v0],
                            color: c,
                        });

                        if stack.count > 1 {
                            let count_str = format!("{}", stack.count);
                            let cw = 0.008;
                            let ch = 0.016;
                            let cs = 0.003;
                            let n_chars = count_str.len() as f32;
                            let count_w = n_chars * cw + (n_chars - 1.0) * cs;
                            let count_x = x1 - count_w - 0.01;
                            let count_y = y0 + 0.012 * aspect;
                            add_string_lines(
                                &count_str,
                                count_x,
                                count_y,
                                cw,
                                ch,
                                cs,
                                [1.0, 1.0, 1.0, 1.0],
                                &mut ui_line_vertices,
                            );
                        }

                        // Draw durability bar
                        draw_durability_bar(stack, x0, x1, y0, y1, aspect, &mut ui_vertices);
                    }
                }

                if self.game_mode == GameMode::Survival {
                    // Draw Health HUD
                    let hud_w = 0.03;
                    let hud_h = 0.03 * aspect;
                    let hud_gap = 0.005;
                    let x_hearts_start = -0.38;
                    let y_hud = -0.76;

                    for i in 0..10 {
                        let h_val = self.player_state.health;
                        let (col, row) = if h_val >= 2.0 * (i + 1) as f32 {
                            (0, 8) // Full
                        } else if h_val >= 2.0 * i as f32 + 1.0 {
                            (1, 8) // Half
                        } else {
                            (2, 8) // Empty
                        };

                        let u0 = col as f32 * 0.0625;
                        let u1 = (col + 1) as f32 * 0.0625;
                        let v0 = row as f32 * 0.0625;
                        let v1 = (row + 1) as f32 * 0.0625;

                        let hx0 = x_hearts_start + i as f32 * (hud_w + hud_gap);
                        let hx1 = hx0 + hud_w;
                        let hy0 = y_hud;
                        let hy1 = hy0 + hud_h;

                        let c = [1.0, 1.0, 1.0, 1.0];
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx0, hy1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx0, hy0, 0.0],
                            tex_coords: [u0, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx1, hy0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx0, hy1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx1, hy0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx1, hy1, 0.0],
                            tex_coords: [u1, v0],
                            color: c,
                        });
                    }

                    // Draw Hunger HUD
                    let x_hunger_start = 0.38 - 10.0 * hud_w - 9.0 * hud_gap;
                    for i in 0..10 {
                        let hung_val = self.player_state.hunger;
                        let (col, row) = if hung_val >= 2.0 * (i + 1) as f32 {
                            (3, 8) // Full
                        } else if hung_val >= 2.0 * i as f32 + 1.0 {
                            (4, 8) // Half
                        } else {
                            (5, 8) // Empty
                        };

                        let u0 = col as f32 * 0.0625;
                        let u1 = (col + 1) as f32 * 0.0625;
                        let v0 = row as f32 * 0.0625;
                        let v1 = (row + 1) as f32 * 0.0625;

                        let hx0 = x_hunger_start + i as f32 * (hud_w + hud_gap);
                        let hx1 = hx0 + hud_w;
                        let hy0 = y_hud;
                        let hy1 = hy0 + hud_h;

                        let c = [1.0, 1.0, 1.0, 1.0];
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx0, hy1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx0, hy0, 0.0],
                            tex_coords: [u0, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx1, hy0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx0, hy1, 0.0],
                            tex_coords: [u0, v0],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx1, hy0, 0.0],
                            tex_coords: [u1, v1],
                            color: c,
                        });
                        ui_textured_vertices.push(TexturedUiVertex {
                            position: [hx1, hy1, 0.0],
                            tex_coords: [u1, v0],
                            color: c,
                        });
                    }

                    // Draw Oxygen HUD
                    if self.player_state.oxygen < 300.0 {
                        let oxygen = self.player_state.oxygen;
                        let bubble_count = (oxygen / 30.0).ceil() as i32;
                        let y_bubbles = y_hud + hud_h + 0.005;

                        for i in 0..bubble_count {
                            let col = 15;
                            let row = 3;
                            let u0 = col as f32 * 0.0625;
                            let u1 = (col + 1) as f32 * 0.0625;
                            let v0 = row as f32 * 0.0625;
                            let v1 = (row + 1) as f32 * 0.0625;

                            let slot_idx = 9 - i;
                            let hx0 = x_hunger_start + slot_idx as f32 * (hud_w + hud_gap);
                            let hx1 = hx0 + hud_w;
                            let hy0 = y_bubbles;
                            let hy1 = hy0 + hud_h;

                            let c = [1.0, 1.0, 1.0, 1.0];
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [hx0, hy1, 0.0],
                                tex_coords: [u0, v0],
                                color: c,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [hx0, hy0, 0.0],
                                tex_coords: [u0, v1],
                                color: c,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [hx1, hy0, 0.0],
                                tex_coords: [u1, v1],
                                color: c,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [hx0, hy1, 0.0],
                                tex_coords: [u0, v0],
                                color: c,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [hx1, hy0, 0.0],
                                tex_coords: [u1, v1],
                                color: c,
                            });
                            ui_textured_vertices.push(TexturedUiVertex {
                                position: [hx1, hy1, 0.0],
                                tex_coords: [u1, v0],
                                color: c,
                            });
                        }
                    }
                }

                // Selected Block/Item Text
                let selected_item = self.inventory.hotbar[self.inventory.selected]
                    .map(|s| s.item)
                    .unwrap_or(crate::inventory::Item::Air);
                let selected_text = format!("{:?}", selected_item).to_uppercase();
                let char_w = 0.010;
                let char_h = 0.020;
                let spacing = 0.004;
                let n = selected_text.len() as f32;
                let width = n * char_w + (n - 1.0) * spacing;
                let text_x = -width / 2.0;
                add_string_lines(
                    &selected_text,
                    text_x,
                    -0.78,
                    char_w,
                    char_h,
                    spacing,
                    [1.0, 1.0, 1.0, 1.0],
                    &mut ui_line_vertices,
                );

                // Game Mode Status Text
                let mode_text = match (self.game_mode, self.player_physics.is_flying()) {
                    (GameMode::Creative, true) => "CREATIVE MODE - FLYING",
                    (GameMode::Creative, false) => "CREATIVE MODE",
                    (GameMode::Survival, _) => "SURVIVAL MODE",
                };
                let mode_w = 0.009;
                let mode_h = 0.018;
                let mode_s = 0.003;
                let n_mode = mode_text.len() as f32;
                let width_mode = n_mode * mode_w + (n_mode - 1.0) * mode_s;
                let mode_x = -width_mode / 2.0;
                add_string_lines(
                    mode_text,
                    mode_x,
                    -0.71,
                    mode_w,
                    mode_h,
                    mode_s,
                    [1.0, 0.9, 0.4, 1.0],
                    &mut ui_line_vertices,
                );

                if self.game_mode == GameMode::Survival {
                    let xp_text = format!("LEVEL {}", self.player_state.experience_level);
                    let width = xp_text.len() as f32 * 0.009;
                    add_string_lines(
                        &xp_text,
                        -width / 2.0,
                        -0.66,
                        0.009,
                        0.018,
                        0.003,
                        [0.35, 1.0, 0.25, 1.0],
                        &mut ui_line_vertices,
                    );
                }

                for (index, effect) in self.potion_effects.active.iter().enumerate() {
                    let seconds = effect.remaining().ceil() as u32;
                    let text = format!("{} {}:{:02}", effect.name(), seconds / 60, seconds % 60);
                    add_string_lines(
                        &text,
                        0.54,
                        0.86 - index as f32 * 0.05,
                        0.007,
                        0.014,
                        0.002,
                        [0.75, 0.55, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );
                }

                // Damaged screen red flash overlay
                if self.player_state.damaged_flash_time > 0.0 {
                    let alpha = (self.player_state.damaged_flash_time / 0.5).min(1.0) * 0.25;
                    let flash_color = [1.0, 0.0, 0.0, alpha];
                    ui_vertices.push(UiVertex {
                        position: [-1.0, 1.0, 0.0],
                        color: flash_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [-1.0, -1.0, 0.0],
                        color: flash_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [1.0, -1.0, 0.0],
                        color: flash_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [-1.0, 1.0, 0.0],
                        color: flash_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [1.0, -1.0, 0.0],
                        color: flash_color,
                    });
                    ui_vertices.push(UiVertex {
                        position: [1.0, 1.0, 0.0],
                        color: flash_color,
                    });
                }

                let lightning_flash = self.weather.flash_intensity();
                if lightning_flash > 0.0 {
                    let flash_color = [1.0, 1.0, 1.0, lightning_flash * 0.82];
                    for position in [
                        [-1.0, 1.0, 0.0],
                        [-1.0, -1.0, 0.0],
                        [1.0, -1.0, 0.0],
                        [-1.0, 1.0, 0.0],
                        [1.0, -1.0, 0.0],
                        [1.0, 1.0, 0.0],
                    ] {
                        ui_vertices.push(UiVertex {
                            position,
                            color: flash_color,
                        });
                    }
                }

                // F3 Debug Screen
                if self.show_debug {
                    use std::fmt::Write;

                    let char_w = 0.007;
                    let char_h = 0.014;
                    let spacing = 0.002;
                    let start_x = -0.98;
                    let mut line_y = 0.95;
                    let line_gap = 0.025;

                    let mut render_line = |s: &str, color: [f32; 4], verts: &mut Vec<UiVertex>| {
                        add_string_lines(s, start_x, line_y, char_w, char_h, spacing, color, verts);
                        line_y -= line_gap;
                    };

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "FPS: {:.1} / FRAME: {:.2} MS",
                        self.debug_fps, self.debug_frame_ms
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    let pos = self.player_physics.position;
                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "XYZ: {:.3} / {:.3} / {:.3}",
                        pos.x, pos.y, pos.z
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "FACING: YAW {:.2} / PITCH {:.2}",
                        self.camera.yaw.to_degrees().rem_euclid(360.0),
                        self.camera.pitch.to_degrees()
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    let chunk_x = debug_chunk_coordinate(pos.x, CHUNK_WIDTH);
                    let chunk_z = debug_chunk_coordinate(pos.z, CHUNK_DEPTH);
                    self.debug_str_scratch.clear();
                    let _ = write!(self.debug_str_scratch, "CHUNK: {} / {}", chunk_x, chunk_z);
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    let biome = self
                        .weather
                        .biome_at(pos.x.floor() as i32, pos.z.floor() as i32);
                    self.debug_str_scratch.clear();
                    let _ = write!(self.debug_str_scratch, "BIOME: {}", biome_debug_name(biome));
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "WEATHER: {:?}",
                        self.weather.current
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "CHUNKS: {} VISIBLE / {} OCCLUDED / {} LOADED / {} DRAWS",
                        self.visible_chunk_count,
                        self.perf_counters.occluded_chunks,
                        self.chunk_manager.chunks.len(),
                        self.submitted_terrain_draw_calls
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "ENTITIES: {} ({} RENDERED, {} FRUSTUM, {} OCCLUSION) / PARTICLES: {}",
                        self.entity_manager.entities.len(),
                        self.perf_counters.rendered_entities,
                        self.perf_counters.frustum_culled_entities,
                        self.perf_counters.occlusion_culled_entities,
                        self.particles.particles.len()
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    let culling = self.entity_los_manager.counters;
                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "CULL: DIST {} / FRUST {} / SEC {} / LOS {} / FAIL-OPEN {} / STALE {} / TIMEOUT {} / OVERFLOW {}",
                        culling.distance,
                        culling.frustum,
                        culling.section,
                        culling.los,
                        culling.fail_open,
                        culling.stale,
                        culling.timeouts,
                        culling.overflow
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    let terrain_indices = self.submitted_terrain_triangles.saturating_mul(3);
                    let rendered_indices = terrain_indices
                        + u64::from(self.mob_num_indices)
                        + u64::from(self.particle_num_indices);
                    let rendered_triangles = rendered_indices / 3;
                    let rendered_vertices = rendered_indices * 2 / 3;
                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "RENDER: {} VERTICES / {} TRIANGLES / {} DRAWS",
                        rendered_vertices, rendered_triangles, self.perf_counters.draw_calls
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "FRAME ALLOCS: {}",
                        self.perf_counters.frame_allocations
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "MEMORY TRACKED: {:.1} MB",
                        self.estimated_debug_memory_bytes() as f64 / (1024.0 * 1024.0)
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "GPU MESH: {:.1} MB / {} BUFFERS / UPLOAD: {:.1} KB",
                        self.perf_counters.gpu_mesh_bytes as f64 / (1024.0 * 1024.0),
                        self.perf_counters.gpu_buffer_objects,
                        self.perf_counters.upload_bytes_frame as f64 / 1024.0
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "WORKERS: {} IN FLIGHT / {} STALE / {} CANCELLED",
                        self.perf_counters.in_flight,
                        self.perf_counters.stale_results,
                        self.perf_counters.cancelled
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "SAVE Q: {} ({:.2} MB) | IN FLIGHT: {} | COALESCE: {} | REGION: {:.2} MB | NET Q: {} | NET FULL: {}",
                        self.perf_counters.save_queue_depth,
                        self.perf_counters.save_queue_bytes as f64 / (1024.0 * 1024.0),
                        self.perf_counters.save_in_flight,
                        self.perf_counters.save_drop,
                        self.perf_counters.loaded_region_cache_bytes as f64 / (1024.0 * 1024.0),
                        self.perf_counters.network_queue_depth,
                        self.perf_counters.network_catchup_mailbox_full
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    if self.perf_counters.gpu_timestamps_supported
                        && self.perf_counters.gpu_timestamps_inside_passes
                        && self.gpu_pass_timings_valid
                    {
                        let _ = write!(
                            self.debug_str_scratch,
                            "GPU PASSES: SKY {:.2}MS | OPAQUE {:.2}MS | MOBS {:.2}MS | TRANS {:.2}MS | PART {:.2}MS | CRACK {:.2}MS | UI {:.2}MS",
                            self.perf_counters.gpu_sky_ns as f64 / 1_000_000.0,
                            self.perf_counters.gpu_opaque_ns as f64 / 1_000_000.0,
                            self.perf_counters.gpu_mobs_ns as f64 / 1_000_000.0,
                            self.perf_counters.gpu_translucent_ns as f64 / 1_000_000.0,
                            self.perf_counters.gpu_particles_ns as f64 / 1_000_000.0,
                            self.perf_counters.gpu_crack_ns as f64 / 1_000_000.0,
                            self.perf_counters.gpu_ui_ns as f64 / 1_000_000.0,
                        );
                    } else if self.perf_counters.gpu_timestamps_supported
                        && self.perf_counters.gpu_timestamps_inside_passes
                    {
                        let _ = write!(
                            self.debug_str_scratch,
                            "GPU PASSES: N/A (WAITING FOR FIRST VALID TIMESTAMP SAMPLE)"
                        );
                    } else if self.perf_counters.gpu_timestamps_supported {
                        let _ = write!(
                            self.debug_str_scratch,
                            "GPU PASSES: N/A (TIMESTAMP_QUERY_INSIDE_PASSES UNSUPPORTED)"
                        );
                    } else {
                        let _ = write!(
                            self.debug_str_scratch,
                            "GPU PASSES: TIMESTAMP QUERY NOT SUPPORTED"
                        );
                    }
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    let time_of_day = self.world_time.time_of_day_smooth();
                    let hour = ((time_of_day * 24.0 + 6.0) % 24.0).floor() as u32;
                    let minute = (((time_of_day * 24.0 + 6.0) % 1.0) * 60.0).floor() as u32;
                    let day = self.world_time.ticks / self.world_time.day_length;
                    self.debug_str_scratch.clear();
                    let _ = write!(
                        self.debug_str_scratch,
                        "TIME: {:02}:{:02} / DAY: {} / TICKS: {}",
                        hour, minute, day, self.world_time.ticks
                    );
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    match &self.role {
                        MultiplayerRole::Host { port } => {
                            let _ = write!(
                                self.debug_str_scratch,
                                "NET: HOST ON PORT {} | CLIENTS: {}",
                                port,
                                self.remote_players.len()
                            );
                        }
                        MultiplayerRole::Client {
                            server_addr, port, ..
                        } => {
                            let _ = write!(
                                self.debug_str_scratch,
                                "NET: CLIENT @ {}:{} | LOCAL ID: {} | PLAYERS: {}",
                                server_addr,
                                port,
                                self.local_player_id
                                    .map(|id| id.to_string())
                                    .unwrap_or_else(|| "?".to_string()),
                                self.remote_players.len() + 1
                            );
                        }
                        MultiplayerRole::Singleplayer => {
                            let _ = write!(self.debug_str_scratch, "NET: SINGLEPLAYER");
                        }
                    }
                    render_line(
                        &self.debug_str_scratch,
                        [1.0, 1.0, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    for summary in self.perf_summaries.iter() {
                        let name_label = if summary.name == "lighting" {
                            "LIGHTING (LOAD+MUTATION)"
                        } else {
                            summary.name
                        };
                        self.debug_str_scratch.clear();
                        let _ = write!(
                            self.debug_str_scratch,
                            "CPU {}: AVG {:.3} / P95 {:.3} / P99 {:.3} MS / N {}",
                            name_label,
                            summary.average() as f64 / 1_000_000.0,
                            summary.p95() as f64 / 1_000_000.0,
                            summary.p99() as f64 / 1_000_000.0,
                            summary.sample_count(),
                        );
                        render_line(
                            &self.debug_str_scratch,
                            [0.82, 0.94, 1.0, 1.0],
                            &mut ui_line_vertices,
                        );
                    }

                    // Queue telemetry is sampled once per frame and retained in the
                    // bounded 240-frame ring; show every queue family plus p95/p99.
                    if let Some(latest) = self.frame_perf_samples.back() {
                        let categories = latest.queues.categories.clone();
                        for category in crate::perf::QueueCategory::ALL {
                            let name = match category {
                                crate::perf::QueueCategory::Inbound => "IN",
                                crate::perf::QueueCategory::Outbound => "OUT",
                                crate::perf::QueueCategory::Reliable => "REL",
                                crate::perf::QueueCategory::CatchUp => "CATCH",
                                crate::perf::QueueCategory::SaveProducer => "SAVE-P",
                                crate::perf::QueueCategory::SaveWorker => "SAVE-W",
                            };
                            let sample = categories.get(&category).cloned().unwrap_or_default();
                            let p95 = crate::perf::frame_percentile(
                                &self.frame_perf_samples,
                                95,
                                |frame| {
                                    frame
                                        .queues
                                        .categories
                                        .get(&category)
                                        .map_or(0, |queue| queue.depth)
                                },
                            );
                            let p99 = crate::perf::frame_percentile(
                                &self.frame_perf_samples,
                                99,
                                |frame| {
                                    frame
                                        .queues
                                        .categories
                                        .get(&category)
                                        .map_or(0, |queue| queue.depth)
                                },
                            );
                            self.debug_str_scratch.clear();
                            let _ = write!(
                                self.debug_str_scratch,
                                "Q {} D:{} B:{} DROP:{} RETRY:{} CANCEL:{} AGE:{}ms P95:{} P99:{}",
                                name,
                                sample.depth,
                                sample.bytes,
                                sample.drops,
                                sample.retries,
                                sample.cancels,
                                sample.oldest_age_ms,
                                p95,
                                p99
                            );
                            render_line(
                                &self.debug_str_scratch,
                                [0.82, 0.94, 1.0, 1.0],
                                &mut ui_line_vertices,
                            );
                        }
                    }

                    self.debug_str_scratch.clear();
                    let _ = write!(self.debug_str_scratch, "LIGHT SRC:");
                    for source in crate::perf::LightingSource::ALL {
                        let ms = self.lighting_scopes_frame.get(source as usize).unwrap_or(0)
                            as f64
                            / 1_000_000.0;
                        let _ = write!(self.debug_str_scratch, " {} {:.3}ms", source.name(), ms);
                    }
                    render_line(
                        &self.debug_str_scratch,
                        [0.82, 0.94, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );

                    self.debug_str_scratch.clear();
                    let _ = write!(self.debug_str_scratch, "UPLOAD SRC:");
                    for source in crate::perf::UploadSource::ALL {
                        let ms = self
                            .gpu_upload_scopes_frame
                            .get(source as usize)
                            .unwrap_or(0) as f64
                            / 1_000_000.0;
                        let _ = write!(self.debug_str_scratch, " {} {:.3}ms", source.name(), ms);
                    }
                    render_line(
                        &self.debug_str_scratch,
                        [0.82, 0.94, 1.0, 1.0],
                        &mut ui_line_vertices,
                    );
                }
            }

            // Remote-player name tags use the same vector-line UI as the rest
            // of the HUD. Project the point above each avatar into NDC, then
            // keep the label readable at the horizontal screen edge.
            let view_proj = self.camera.build_view_projection_matrix(
                aspect,
                crate::camera::render_far_plane(self.chunk_manager.render_distance as u32),
            );
            for remote in self.remote_players.values() {
                if remote.username.trim().is_empty() {
                    continue;
                }
                let Some(entity) = self.entity_manager.get_by_id(remote.entity_id) else {
                    continue;
                };
                if entity.position.distance_squared(self.camera.position) > 96.0 * 96.0 {
                    continue;
                }
                let Some(projected) =
                    project_name_tag(entity.position + Vec3::new(0.0, 2.05, 0.0), view_proj)
                else {
                    continue;
                };
                let label: String = remote.username.to_uppercase().chars().take(24).collect();
                let char_w = 0.009;
                let char_h = 0.018;
                let spacing = 0.003;
                let width = label.chars().count() as f32 * (char_w + spacing) - spacing;
                let center_x = projected.x.clamp(-0.98 + width / 2.0, 0.98 - width / 2.0);
                let y = (projected.y + 0.025).clamp(-0.94, 0.94);
                add_ui_quad(
                    &mut ui_vertices,
                    center_x - width / 2.0 - 0.012,
                    center_x + width / 2.0 + 0.012,
                    y - 0.007,
                    y + char_h + 0.007,
                    [0.02, 0.02, 0.02, 0.68],
                );
                add_string_lines(
                    &label,
                    center_x - width / 2.0,
                    y,
                    char_w,
                    char_h,
                    spacing,
                    [1.0, 1.0, 1.0, 1.0],
                    &mut ui_line_vertices,
                );
            }

            // Chat history is deliberately a compact ring buffer. The newest
            // line sits closest to the input box at the lower-left.
            let visible_messages: Vec<_> = self
                .chat_messages
                .iter()
                .rev()
                .take(CHAT_VISIBLE_LINES)
                .collect();
            for (line_index, (sender, message)) in visible_messages.iter().enumerate() {
                let line: String = format!("<{sender}> {message}")
                    .to_uppercase()
                    .chars()
                    .take(96)
                    .collect();
                let y = -0.80 + line_index as f32 * 0.050;
                let char_w = 0.008;
                let char_h = 0.018;
                let spacing = 0.002;
                let width = line.chars().count() as f32 * (char_w + spacing) - spacing;
                let alpha = 1.0 - line_index as f32 * 0.07;
                add_ui_quad(
                    &mut ui_vertices,
                    -0.985,
                    (-0.955 + width).min(0.985),
                    y - 0.007,
                    y + char_h + 0.007,
                    [0.01, 0.01, 0.01, 0.52 * alpha],
                );
                add_string_lines(
                    &line,
                    -0.97,
                    y,
                    char_w,
                    char_h,
                    spacing,
                    [1.0, 1.0, 1.0, alpha],
                    &mut ui_line_vertices,
                );
            }

            if self.is_chat_open {
                add_ui_quad(
                    &mut ui_vertices,
                    -0.99,
                    0.99,
                    -0.97,
                    -0.875,
                    [0.01, 0.01, 0.01, 0.78],
                );
                add_ui_border(
                    &mut ui_line_vertices,
                    -0.99,
                    0.99,
                    -0.97,
                    -0.875,
                    [0.65, 0.65, 0.65, 0.9],
                );
                let mut visible_input: Vec<char> = self.chat_input.chars().rev().take(92).collect();
                visible_input.reverse();
                let mut input = String::from("> ");
                input.extend(visible_input);
                if (self.total_time * 2.0) as u32 % 2 == 0 {
                    input.push('_');
                }
                add_string_lines(
                    &input.to_uppercase(),
                    -0.97,
                    -0.935,
                    0.008,
                    0.024,
                    0.002,
                    [1.0, 1.0, 1.0, 1.0],
                    &mut ui_line_vertices,
                );
            }

            if let Some(boss) = crate::boss::active_boss_hud(&self.entity_manager) {
                let x0 = -0.42;
                let x1 = 0.42;
                let y0 = 0.82;
                let y1 = 0.875;
                add_ui_quad(&mut ui_vertices, x0, x1, y0, y1, [0.05, 0.01, 0.07, 0.92]);
                add_ui_quad(
                    &mut ui_vertices,
                    x0 + 0.008,
                    x0 + 0.008 + (x1 - x0 - 0.016) * boss.progress,
                    y0 + 0.009,
                    y1 - 0.009,
                    [0.55, 0.05, 0.65, 1.0],
                );
                let char_w = 0.010;
                let spacing = 0.003;
                let width = boss.title.len() as f32 * (char_w + spacing) - spacing;
                add_string_lines(
                    boss.title,
                    -width / 2.0,
                    0.895,
                    char_w,
                    0.02,
                    spacing,
                    [1.0, 1.0, 1.0, 1.0],
                    &mut ui_line_vertices,
                );
            }

            self.render_advancement_ui_and_toasts(
                &mut ui_vertices,
                &mut ui_line_vertices,
                &mut ui_textured_vertices,
            );

            // Write Buffers
            let ui_vert_len = ui_vertices.len().min(UI_VERTEX_CAPACITY);
            let ui_line_vert_len = ui_line_vertices.len().min(UI_LINE_VERTEX_CAPACITY);
            let ui_textured_vert_len = ui_textured_vertices.len().min(UI_VERTEX_CAPACITY);

            let upload_started = Instant::now();
            self.queue.write_buffer(
                &self.ui_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_vertices[..ui_vert_len]),
            );
            self.queue.write_buffer(
                &self.ui_line_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_line_vertices[..ui_line_vert_len]),
            );
            self.queue.write_buffer(
                &self.ui_textured_vertex_buffer,
                0,
                bytemuck::cast_slice(&ui_textured_vertices[..ui_textured_vert_len]),
            );
            let upload_elapsed = upload_started.elapsed();
            self.gpu_upload_time_frame += upload_elapsed;
            self.gpu_upload_scopes_frame
                .record(crate::perf::UploadSource::Ui as usize, upload_elapsed);
            self.perf_counters.upload_bytes_frame =
                self.perf_counters.upload_bytes_frame.saturating_add(
                    ((ui_vert_len + ui_line_vert_len + ui_textured_vert_len)
                        * std::mem::size_of::<UiVertex>()) as u64,
                );

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
            self.num_ui_textured_vertices = ui_textured_vert_len as u32;
        }

        self.ui_vertices_scratch = ui_vertices;
        self.ui_line_vertices_scratch = ui_line_vertices;

        self.perf_recorder.record(
            crate::perf::ScopeId::RenderPrepareUi,
            ui_prepare_started.elapsed(),
        );
        self.gpu_upload_time_frame += gpu_upload_elapsed;
        let mut total_draw_calls = 1 + self.submitted_terrain_draw_calls as u64;
        total_draw_calls += u64::from(self.mob_cuboid_num_instances > 0)
            + u64::from(self.mob_quad_num_instances > 0);
        total_draw_calls += u64::from(!self.particle_instances_scratch.is_empty());
        total_draw_calls += u64::from(self.mining_target.is_some() && self.mining_progress > 0.0);
        total_draw_calls +=
            u64::from(self.hand_num_indices > 0 && !self.third_person && !self.is_paused);
        if self.is_paused {
            total_draw_calls += 2;
        } else {
            total_draw_calls += u64::from(self.num_ui_vertices > 0);
            total_draw_calls += u64::from(self.num_ui_textured_vertices > 0);
            total_draw_calls += 1; // Crosshair.
            total_draw_calls += u64::from(self.num_ui_line_vertices > 0);
        }
        self.perf_counters.draw_calls = total_draw_calls;

        let render_encode_started = Instant::now();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        let mut crack_metrics: Option<(u64, u64)> = None;
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: self.camera_uniform.sky_color_horizon[0] as f64,
                            g: self.camera_uniform.sky_color_horizon[1] as f64,
                            b: self.camera_uniform.sky_color_horizon[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            // Viewport-only scaling renders the world into the surface's
            // top-left corner; without an offscreen target and upscale pass it
            // is not dynamic resolution. Keep world rendering native-sized
            // until that complete path exists.
            let effective_scale = 1.0_f32;
            if effective_scale < 0.999 {
                render_pass.set_viewport(
                    0.0,
                    0.0,
                    (self.size.width as f32 * effective_scale).max(1.0),
                    (self.size.height as f32 * effective_scale).max(1.0),
                    0.0,
                    1.0,
                );
            }

            // Draw Skybox first
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 0);
                }
            }
            render_pass.set_pipeline(&self.sky_pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            render_pass.draw(0..6, 0..1);
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 1);
                }
            }

            // Pass 1: Opaque & Cutout
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 2);
                }
            }
            render_pass.set_pipeline(&self.terrain_render_pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            let mut bound_region: Option<(i32, i32)> = None;
            for candidate in &draw_plan.opaque {
                let lod = candidate.lod;
                let Some(layer) = self
                    .chunk_meshes
                    .get(&candidate.chunk_coord)
                    .and_then(|mesh| {
                        candidate
                            .section_y
                            .and_then(|section_y| mesh.section(section_y as usize))
                    })
                    .and_then(|section| section.level(lod))
                    .map(|level| &level.opaque)
                else {
                    continue;
                };
                let Some(handle) = layer.handle else {
                    continue;
                };
                let region_coord = crate::chunk_render::chunk_to_region_coord(
                    candidate.chunk_coord.0,
                    candidate.chunk_coord.1,
                );
                let Some(region) = self.render_regions.get(&region_coord) else {
                    continue;
                };
                if !region.handle_is_live(&handle) {
                    continue;
                }
                if bound_region != Some(region_coord) {
                    render_pass.set_vertex_buffer(0, region.vertex_buffer.slice(..));
                    render_pass
                        .set_index_buffer(region.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.set_bind_group(1, &region.bind_group, &[]);
                    bound_region = Some(region_coord);
                }
                let Some(index_end) = handle.index_offset.checked_add(handle.num_indices) else {
                    continue;
                };
                let Ok(base_vertex) = i32::try_from(handle.vertex_offset) else {
                    continue;
                };
                render_pass.draw_indexed(handle.index_offset..index_end, base_vertex, 0..1);
            }
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 3);
                }
            }

            // Draw Mobs
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 4);
                }
            }
            if self.mob_cuboid_num_instances > 0 {
                render_pass.set_pipeline(&self.mob_instanced_pipeline);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.mob_cuboid_proto_vbuf.slice(..));
                render_pass.set_vertex_buffer(
                    1,
                    self.mob_cuboid_instance_buffers[self.frame_ring_index].slice(..),
                );
                render_pass.set_index_buffer(
                    self.mob_cuboid_proto_ibuf.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                render_pass.draw_indexed(0..36, 0, 0..self.mob_cuboid_num_instances);
            }
            if self.mob_quad_num_instances > 0 {
                render_pass.set_pipeline(&self.mob_instanced_pipeline);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.mob_quad_proto_vbuf.slice(..));
                render_pass.set_vertex_buffer(
                    1,
                    self.mob_quad_instance_buffers[self.frame_ring_index].slice(..),
                );
                render_pass.set_index_buffer(
                    self.mob_quad_proto_ibuf.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                render_pass.draw_indexed(0..12, 0, 0..self.mob_quad_num_instances);
            }
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 5);
                }
            }

            // Pass 2: Translucent (Water/Ice)
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 6);
                }
            }
            render_pass.set_pipeline(&self.terrain_trans_pipeline);
            let mut bound_region: Option<(i32, i32)> = None;
            for candidate in &draw_plan.transparent {
                let lod = candidate.lod;
                let Some(layer) = self
                    .chunk_meshes
                    .get(&candidate.chunk_coord)
                    .and_then(|mesh| {
                        candidate
                            .section_y
                            .and_then(|section_y| mesh.section(section_y as usize))
                    })
                    .and_then(|section| section.level(lod))
                    .map(|level| &level.transparent)
                else {
                    continue;
                };
                let Some(handle) = layer.handle else {
                    continue;
                };
                let region_coord = crate::chunk_render::chunk_to_region_coord(
                    candidate.chunk_coord.0,
                    candidate.chunk_coord.1,
                );
                let Some(region) = self.render_regions.get(&region_coord) else {
                    continue;
                };
                if !region.handle_is_live(&handle) {
                    continue;
                }
                if bound_region != Some(region_coord) {
                    render_pass.set_vertex_buffer(0, region.vertex_buffer.slice(..));
                    render_pass
                        .set_index_buffer(region.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.set_bind_group(1, &region.bind_group, &[]);
                    bound_region = Some(region_coord);
                }
                let Some(index_end) = handle.index_offset.checked_add(handle.num_indices) else {
                    continue;
                };
                let Ok(base_vertex) = i32::try_from(handle.vertex_offset) else {
                    continue;
                };
                render_pass.draw_indexed(handle.index_offset..index_end, base_vertex, 0..1);
            }
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 7);
                }
            }

            // Draw billboard particles using instanced particle pipeline.
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 8);
                }
            }
            if !self.particle_instances_scratch.is_empty() {
                let num_particles = self.particle_instances_scratch.len() as u32;
                render_pass.set_pipeline(&self.particle_instanced_pipeline);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.particle_proto_vbuf.slice(..));
                render_pass.set_vertex_buffer(
                    1,
                    self.particle_instance_buffers[self.frame_ring_index].slice(..),
                );
                render_pass.set_index_buffer(
                    self.particle_proto_ibuf.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                render_pass.draw_indexed(0..6, 0, 0..num_particles);
            }
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 9);
                }
            }

            // Draw Block cracking animation overlay (multiply blend)
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 10);
                }
            }
            if let Some(target) = self.mining_target {
                if self.mining_progress > 0.0 {
                    if let Some((_num_vertices, num_indices, upload_ns, upload_bytes)) =
                        self.update_crack_buffers(target, self.mining_progress)
                    {
                        crack_metrics = Some((upload_ns, upload_bytes));
                        render_pass.set_pipeline(&self.crack_pipeline);
                        render_pass.set_vertex_buffer(0, self.crack_vertex_buffer.slice(..));
                        render_pass.set_index_buffer(
                            self.crack_index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        render_pass.draw_indexed(0..num_indices, 0, 0..1);
                    }
                }
            }
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 11);
                }
            }

            // Draw first-person right hand and held item. Uses a dedicated
            // camera with a very near plane so the view-space model never
            // clips into world geometry. Hidden in third-person mode and when
            // the game is paused.
            if self.hand_num_indices > 0 && !self.third_person && !self.is_paused {
                render_pass.set_pipeline(&self.hand_pipeline);
                render_pass.set_bind_group(0, &self.hand_camera_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.hand_vertex_buffer.slice(..));
                render_pass
                    .set_index_buffer(self.hand_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..self.hand_num_indices, 0, 0..1);
            }

            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 12);
                }
            }
            if effective_scale < 0.999 {
                render_pass.set_viewport(
                    0.0,
                    0.0,
                    self.size.width as f32,
                    self.size.height as f32,
                    0.0,
                    1.0,
                );
            }
            if !self.is_paused {
                // 1. Draw Colored UI (hotbar/slot backgrounds)
                if self.num_ui_vertices > 0 {
                    render_pass.set_pipeline(&self.ui_pipeline);
                    render_pass.set_vertex_buffer(0, self.ui_vertex_buffer.slice(..));
                    render_pass.draw(0..self.num_ui_vertices, 0..1);
                }

                // 2. Draw Textured UI (block/item thumbnails on top of backgrounds)
                if self.num_ui_textured_vertices > 0 {
                    render_pass.set_pipeline(&self.ui_textured_pipeline);
                    render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                    render_pass.set_vertex_buffer(0, self.ui_textured_vertex_buffer.slice(..));
                    render_pass.draw(0..self.num_ui_textured_vertices, 0..1);
                }

                // 3. Draw Crosshair
                render_pass.set_pipeline(&self.crosshair_pipeline);
                render_pass.set_vertex_buffer(0, self.crosshair_buffer.slice(..));
                render_pass.draw(0..4, 0..1);

                // 4. Draw Line/Text UI (slot borders & texts)
                if self.num_ui_line_vertices > 0 {
                    render_pass.set_pipeline(&self.ui_line_pipeline);
                    render_pass.set_vertex_buffer(0, self.ui_line_vertex_buffer.slice(..));
                    render_pass.draw(0..self.num_ui_line_vertices, 0..1);
                }
            } else {
                // 3. Draw Pause Menu
                // Background overlay & buttons
                render_pass.set_pipeline(&self.ui_pipeline);
                render_pass.set_vertex_buffer(0, self.ui_vertex_buffer.slice(..));
                render_pass.draw(0..self.num_ui_vertices, 0..1);

                // Borders & Text
                render_pass.set_pipeline(&self.ui_line_pipeline);
                render_pass.set_vertex_buffer(0, self.ui_line_vertex_buffer.slice(..));
                render_pass.draw(0..self.num_ui_line_vertices, 0..1);
            }
            if self.gpu_timestamps_inside_passes {
                if let Some(qs) = &self.gpu_timestamp_query_set {
                    render_pass.write_timestamp(qs, 13);
                }
            }
        }

        if let Some((upload_ns, upload_bytes)) = crack_metrics {
            self.gpu_upload_time_frame += Duration::from_nanos(upload_ns);
            self.gpu_upload_scopes_frame
                .record_nanos(crate::perf::UploadSource::Crack as usize, upload_ns);
            self.perf_counters.upload_bytes_frame = self
                .perf_counters
                .upload_bytes_frame
                .saturating_add(upload_bytes);
        }
        self.perf_recorder
            .record(crate::perf::ScopeId::GpuUpload, self.gpu_upload_time_frame);

        self.poll_gpu_timestamp_readbacks();
        let mut timestamp_readback_slot = None;
        if let (Some(query_set), Some(resolve_buffer)) = (
            &self.gpu_timestamp_query_set,
            &self.gpu_timestamp_resolve_buffer,
        ) {
            for (slot_index, slot) in self.gpu_timestamp_readback_slots.iter().enumerate() {
                if !slot
                    .status
                    .lock()
                    .unwrap()
                    .reserve_copy(frame_submission_id)
                {
                    continue;
                }
                encoder.resolve_query_set(
                    query_set,
                    0..GPU_TIMESTAMP_QUERY_COUNT,
                    resolve_buffer,
                    0,
                );
                encoder.copy_buffer_to_buffer(
                    resolve_buffer,
                    0,
                    &slot.buffer,
                    0,
                    GPU_TIMESTAMP_READBACK_BYTES,
                );
                timestamp_readback_slot = Some(slot_index);
                break;
            }
        }
        self.perf_counters.gpu_sky_ns = self.gpu_pass_timings_ns[0];
        self.perf_counters.gpu_opaque_ns = self.gpu_pass_timings_ns[1];
        self.perf_counters.gpu_mobs_ns = self.gpu_pass_timings_ns[2];
        self.perf_counters.gpu_translucent_ns = self.gpu_pass_timings_ns[3];
        self.perf_counters.gpu_particles_ns = self.gpu_pass_timings_ns[4];
        self.perf_counters.gpu_crack_ns = self.gpu_pass_timings_ns[5];
        self.perf_counters.gpu_ui_ns = self.gpu_pass_timings_ns[6];
        self.perf_counters.gpu_timestamps_supported = self.gpu_timestamps_supported;
        self.perf_counters.gpu_timestamps_inside_passes = self.gpu_timestamps_inside_passes;

        let command_buffer = encoder.finish();
        self.queue.submit(std::iter::once(command_buffer));
        let completion_tx = self.gpu_completion_tx.clone();
        self.queue.on_submitted_work_done(move || {
            let _ = completion_tx.send(frame_submission_id);
        });
        if let Some(slot_index) = timestamp_readback_slot {
            let slot = &self.gpu_timestamp_readback_slots[slot_index];
            if slot
                .status
                .lock()
                .unwrap()
                .begin_mapping(frame_submission_id)
            {
                let status = std::sync::Arc::clone(&slot.status);
                slot.buffer
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        status
                            .lock()
                            .unwrap()
                            .map_completed(frame_submission_id, result.is_ok());
                    });
            }
        }
        self.perf_recorder.record(
            crate::perf::ScopeId::RenderEncode,
            render_encode_started.elapsed(),
        );
        let present_started = Instant::now();
        output.present();
        self.perf_recorder
            .record(crate::perf::ScopeId::Present, present_started.elapsed());
        let allocs_after = crate::perf::thread_alloc_count();
        self.perf_counters.frame_allocations = allocs_after.saturating_sub(allocs_before);
        self.record_frame_perf_sample();
        Ok(())
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
        let save = &self.save_queue_stats;
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
            crate::perf::QueueCategorySample {
                depth: save.depth().saturating_sub(save.in_flight()),
                bytes: save.queued_bytes(),
                oldest_age_ms: 0,
                drops: save.dropped(),
                retries: save.retries(),
                cancels: save.cancels(),
            },
        );
        categories.insert(
            crate::perf::QueueCategory::SaveWorker,
            crate::perf::QueueCategorySample {
                depth: save.in_flight(),
                bytes: save.in_flight_bytes(),
                oldest_age_ms: 0,
                drops: 0,
                retries: save.retries(),
                cancels: save.cancels(),
            },
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
                "ADVANCEMENT MADE!",
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

fn add_char_lines(
    c: char,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
    vertices: &mut Vec<UiVertex>,
) {
    let x0 = x;
    let x1 = x + w;
    let xm = x + w * 0.5;
    let y0 = y;
    let y1 = y + h;
    let ym = y + h * 0.5;

    let mut add_line = |x_start: f32, y_start: f32, x_end: f32, y_end: f32| {
        vertices.push(UiVertex {
            position: [x_start, y_start, 0.0],
            color,
        });
        vertices.push(UiVertex {
            position: [x_end, y_end, 0.0],
            color,
        });
    };

    match c.to_ascii_uppercase() {
        'R' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, ym);
            add_line(x1, ym, x0, ym);
            add_line(x0, ym, x1, y0);
        }
        'E' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x0, ym, x1, ym);
            add_line(x0, y0, x1, y0);
        }
        'S' => {
            add_line(x1, y1, x0, y1);
            add_line(x0, y1, x0, ym);
            add_line(x0, ym, x1, ym);
            add_line(x1, ym, x1, y0);
            add_line(x1, y0, x0, y0);
        }
        'U' => {
            add_line(x0, y1, x0, y0);
            add_line(x0, y0, x1, y0);
            add_line(x1, y0, x1, y1);
        }
        'M' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, xm, ym);
            add_line(xm, ym, x1, y1);
            add_line(x1, y1, x1, y0);
        }
        'G' => {
            add_line(x1, y1, x0, y1);
            add_line(x0, y1, x0, y0);
            add_line(x0, y0, x1, y0);
            add_line(x1, y0, x1, ym);
            add_line(x1, ym, xm, ym);
        }
        'A' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, y0);
            add_line(x0, ym, x1, ym);
        }
        'Q' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, y0);
            add_line(x1, y0, x0, y0);
            add_line(xm, ym, x1 + w * 0.2, y0 - h * 0.2);
        }
        'I' => {
            add_line(xm, y0, xm, y1);
            add_line(x0, y1, x1, y1);
            add_line(x0, y0, x1, y0);
        }
        'T' => {
            add_line(x0, y1, x1, y1);
            add_line(xm, y1, xm, y0);
        }
        'P' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, ym);
            add_line(x1, ym, x0, ym);
        }
        'O' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, y0);
            add_line(x1, y0, x0, y0);
        }
        'D' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, xm, y1);
            add_line(xm, y1, x1, ym);
            add_line(x1, ym, xm, y0);
            add_line(xm, y0, x0, y0);
        }
        'F' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x0, ym, x1, ym);
        }
        'V' => {
            add_line(x0, y1, xm, y0);
            add_line(xm, y0, x1, y1);
        }
        'N' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y0);
            add_line(x1, y0, x1, y1);
        }
        'Y' => {
            add_line(x0, y1, xm, ym);
            add_line(x1, y1, xm, ym);
            add_line(xm, ym, xm, y0);
        }
        'C' => {
            add_line(x1, y1, x0, y1);
            add_line(x0, y1, x0, y0);
            add_line(x0, y0, x1, y0);
        }
        'H' => {
            add_line(x0, y0, x0, y1);
            add_line(x1, y0, x1, y1);
            add_line(x0, ym, x1, ym);
        }
        'L' => {
            add_line(x0, y1, x0, y0);
            add_line(x0, y0, x1, y0);
        }
        'B' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, ym);
            add_line(x1, ym, x0, ym);
            add_line(x1, ym, x1, y0);
            add_line(x1, y0, x0, y0);
        }
        'K' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, ym, x1, y1);
            add_line(x0, ym, x1, y0);
        }
        'W' => {
            add_line(x0, y1, x0 + w * 0.2, y0);
            add_line(x0 + w * 0.2, y0, xm, ym);
            add_line(xm, ym, x0 + w * 0.8, y0);
            add_line(x0 + w * 0.8, y0, x1, y1);
        }
        'X' => {
            add_line(x0, y0, x1, y1);
            add_line(x0, y1, x1, y0);
        }
        'Z' => {
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x0, y0);
            add_line(x0, y0, x1, y0);
        }
        '<' => {
            add_line(x1, y1, x0, ym);
            add_line(x0, ym, x1, y0);
        }
        '>' => {
            add_line(x0, y1, x1, ym);
            add_line(x1, ym, x0, y0);
        }
        '-' => {
            add_line(x0, ym, x1, ym);
        }
        '_' => {
            add_line(x0, y0, x1, y0);
        }
        '+' => {
            add_line(x0, ym, x1, ym);
            add_line(xm, y0, xm, y1);
        }
        '/' => {
            add_line(x0, y0, x1, y1);
        }
        ':' => {
            add_line(xm - w * 0.05, y0 + h * 0.7, xm + w * 0.05, y0 + h * 0.7);
            add_line(xm - w * 0.05, y0 + h * 0.3, xm + w * 0.05, y0 + h * 0.3);
        }
        '0' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, y0);
            add_line(x1, y0, x0, y0);
        }
        '1' => {
            add_line(xm, y0, xm, y1);
            add_line(x0, y0, x1, y0);
            add_line(xm - w * 0.2, y1 - h * 0.2, xm, y1);
        }
        '2' => {
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, ym);
            add_line(x1, ym, x0, ym);
            add_line(x0, ym, x0, y0);
            add_line(x0, y0, x1, y0);
        }
        '3' => {
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, y0);
            add_line(x1, y0, x0, y0);
            add_line(x0, ym, x1, ym);
        }
        '4' => {
            add_line(x0, y1, x0, ym);
            add_line(x0, ym, x1, ym);
            add_line(x1, y1, x1, y0);
        }
        '5' => {
            add_line(x1, y1, x0, y1);
            add_line(x0, y1, x0, ym);
            add_line(x0, ym, x1, ym);
            add_line(x1, ym, x1, y0);
            add_line(x1, y0, x0, y0);
        }
        '6' => {
            add_line(x1, y1, x0, y1);
            add_line(x0, y1, x0, y0);
            add_line(x0, y0, x1, y0);
            add_line(x1, y0, x1, ym);
            add_line(x1, ym, x0, ym);
        }
        '7' => {
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, y0);
        }
        '8' => {
            add_line(x0, y0, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, y0);
            add_line(x1, y0, x0, y0);
            add_line(x0, ym, x1, ym);
        }
        '9' => {
            add_line(x0, ym, x1, ym);
            add_line(x0, ym, x0, y1);
            add_line(x0, y1, x1, y1);
            add_line(x1, y1, x1, y0);
            add_line(x1, y0, x0, y0);
        }
        '.' => {
            add_line(xm - w * 0.05, y0, xm + w * 0.05, y0);
        }
        ' ' => {}
        _ => {}
    }
}

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
    let mut current_x = start_x;
    for c in s.chars() {
        add_char_lines(
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

fn weather_tile_uv(column: u32, row: u32) -> [f32; 4] {
    let inset = 0.08;
    [
        (column as f32 + inset) / 16.0,
        (row as f32 + inset) / 16.0,
        (column as f32 + 1.0 - inset) / 16.0,
        (row as f32 + 1.0 - inset) / 16.0,
    ]
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlockBreakRewards {
    pub drops: Vec<ItemStack>,
    pub xp: u32,
    pub exhaustion: f32,
    pub tool_damaged: bool,
}

pub fn calculate_block_break_rewards(
    old_block: BlockType,
    pos: (i32, i32, i32),
    held_stack: Option<&ItemStack>,
    game_mode: GameMode,
) -> BlockBreakRewards {
    if game_mode != GameMode::Survival {
        return BlockBreakRewards {
            drops: Vec::new(),
            xp: 0,
            exhaustion: 0.0,
            tool_damaged: false,
        };
    }

    let (wx, wy, wz) = pos;
    let mut drops = Vec::new();

    let mut eligible_to_harvest = true;
    if let Some(min_material) = old_block.min_harvest_material() {
        let held_item = held_stack.map(|s| s.item).unwrap_or(Item::Air);
        if let Some(tool_prop) = held_item.tool_properties() {
            eligible_to_harvest = tool_prop.tool_type == old_block.preferred_tool()
                && tool_prop.material >= min_material;
        } else {
            eligible_to_harvest = false;
        }
    }

    if eligible_to_harvest {
        let held_enchantments = held_stack
            .map(|stack| stack.enchantments)
            .unwrap_or_default();
        let silk_touch = held_enchantments.level_of(crate::enchantment::Enchantment::SilkTouch) > 0;
        let fortune =
            held_enchantments.level_of(crate::enchantment::Enchantment::Fortune(1)) as u32;
        let is_any_leaves = old_block == BlockType::OakLeaves
            || old_block == BlockType::BirchLeaves
            || old_block == BlockType::SpruceLeaves;
        if silk_touch {
            drops.push(ItemStack::new(Item::from_block(old_block), 1));
        } else if is_any_leaves {
            let mut rng_seed = (wx as u32)
                .wrapping_mul(31)
                .wrapping_add(wy as u32)
                .wrapping_mul(17)
                .wrapping_add(wz as u32);
            let mut next_rand = || {
                rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
                (rng_seed / 65536) % 32768
            };
            if next_rand() % 10 == 0 {
                drops.push(ItemStack::new(Item::Apple, 1));
            } else {
                drops.push(ItemStack::new(Item::from_block(old_block), 1));
            }
        } else if old_block == BlockType::TallGrass {
            let mut rng_seed = (wx as u32)
                .wrapping_mul(31)
                .wrapping_add(wy as u32)
                .wrapping_mul(17)
                .wrapping_add(wz as u32);
            let mut next_rand = || {
                rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
                (rng_seed / 65536) % 32768
            };
            if next_rand() % 8 == 0 {
                drops.push(ItemStack::new(Item::Seeds, 1));
            }
        } else {
            let base_drop = match old_block {
                BlockType::CoalOre => Item::Coal,
                BlockType::DiamondOre => Item::Diamond,
                BlockType::RedstoneOre => Item::Redstone,
                _ => Item::from_block(old_block),
            };
            let fortune_eligible = matches!(
                old_block,
                BlockType::CoalOre | BlockType::DiamondOre | BlockType::RedstoneOre
            );
            let bonus = if fortune_eligible && fortune > 0 {
                ((wx as u32)
                    .wrapping_mul(31)
                    .wrapping_add(wy as u32 * 17)
                    .wrapping_add(wz as u32 * 13)
                    % (fortune + 1))
                    + fortune / 2
            } else {
                0
            };
            for _ in 0..(1 + bonus) {
                drops.push(ItemStack::new(base_drop, 1));
            }
        }
    }

    let mut xp = 0;
    if matches!(
        old_block,
        BlockType::CoalOre
            | BlockType::IronOre
            | BlockType::GoldOre
            | BlockType::DiamondOre
            | BlockType::RedstoneOre
    ) {
        xp = if old_block == BlockType::DiamondOre {
            5
        } else {
            2
        };
        if old_block == BlockType::RedstoneOre && ((wx ^ wy ^ wz) & 1) == 0 {
            drops.push(ItemStack::new(Item::LapisLazuli, 1));
        }
    }

    BlockBreakRewards {
        drops,
        xp,
        exhaustion: 0.005,
        tool_damaged: true,
    }
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
        Biome::Desert => "DESERT",
        Biome::Taiga => "TAIGA",
        Biome::Swamp => "SWAMP",
        Biome::Mountains => "MOUNTAINS",
        Biome::Ocean => "OCEAN",
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

    #[test]
    fn terrain_translucent_pipeline_is_double_sided() {
        assert_eq!(terrain_translucent_cull_mode(), None);
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
    fn friendly_arrow_damage_settles_lethal_rewards_exactly_once() {
        let mut zombie =
            crate::entity::Entity::new(1, crate::entity::EntityType::Zombie, Vec3::ZERO);
        zombie.health = 5.0;

        assert!(apply_player_projectile_damage(&mut zombie, 4.0).is_none());
        assert_eq!(zombie.health, 1.0);
        assert!(!zombie.player_kill_rewarded);

        let kill =
            apply_player_projectile_damage(&mut zombie, 1.0).expect("lethal arrow should settle");
        assert_eq!(zombie.health, 0.0);
        assert!(zombie.player_kill_rewarded);
        assert_eq!(
            standard_player_kill_rewards(kill, 0),
            PlayerKillRewards {
                items: vec![Item::RottenFlesh],
                experience: 5,
            }
        );

        assert!(apply_player_projectile_damage(&mut zombie, 4.0).is_none());
        assert!(claim_standard_player_kill(&mut zombie).is_none());
        assert_eq!(zombie.health, 0.0);
    }

    #[test]
    fn full_inventory_generates_exactly_one_world_drop_at_the_source() {
        let mut inventory = Inventory::new();
        inventory
            .hotbar
            .fill(Some(ItemStack::new(Item::DiamondSword, 1)));
        inventory
            .main
            .fill(Some(ItemStack::new(Item::DiamondPickaxe, 1)));
        let original_hotbar = inventory.hotbar;
        let original_main = inventory.main;
        let mut entities = crate::entity::EntityManager::new();
        let source = Vec3::new(8.5, 64.5, -2.5);

        assert_eq!(
            store_or_drop_generated_item(
                &mut inventory,
                &mut entities,
                Item::RottenFlesh,
                source,
                123,
            ),
            GeneratedItemDestination::Dropped
        );
        assert_eq!(inventory.hotbar, original_hotbar);
        assert_eq!(inventory.main, original_main);
        assert_eq!(entities.entities.len(), 1);
        let dropped = &entities.entities[0];
        assert_eq!(dropped.entity_type, crate::entity::EntityType::DroppedItem);
        assert_eq!(dropped.dropped_item, Some(Item::RottenFlesh));
        assert_eq!(dropped.position, source);
        assert_eq!(dropped.pickup_cooldown, 0.5);
    }

    #[test]
    fn generated_item_stored_in_inventory_does_not_duplicate_as_a_drop() {
        let mut inventory = Inventory::new();
        let mut entities = crate::entity::EntityManager::new();

        assert_eq!(
            store_or_drop_generated_item(
                &mut inventory,
                &mut entities,
                Item::Wool,
                Vec3::ZERO,
                456,
            ),
            GeneratedItemDestination::Inventory
        );
        assert_eq!(inventory.count_item(Item::Wool), 1);
        assert!(entities.entities.is_empty());
    }

    #[test]
    fn friendly_arrows_destroy_end_crystals_without_standard_mob_rewards() {
        let mut crystal =
            crate::entity::Entity::new(1, crate::entity::EntityType::EndCrystal, Vec3::ZERO);
        assert!(!crystal.is_local_living_target());
        assert!(crystal.is_player_projectile_target());
        assert!(is_legal_melee_target(&crystal));

        assert!(apply_player_projectile_damage(&mut crystal, 5.0).is_none());
        assert_eq!(crystal.health, 0.0);
        assert!(!crystal.player_kill_rewarded);
        assert!(!crystal.is_player_projectile_target());
    }

    #[test]
    fn splash_effects_ignore_nonliving_entities_but_affect_living_targets() {
        let poison = crate::brewing::PotionData {
            kind: crate::brewing::PotionKind::Poison,
            level: 1,
            duration_seconds: 30,
            splash: true,
        };
        let slowness = crate::brewing::PotionData {
            kind: crate::brewing::PotionKind::Slowness,
            level: 1,
            duration_seconds: 30,
            splash: true,
        };
        let mut nonliving = vec![
            crate::entity::Entity::new(1, crate::entity::EntityType::DroppedItem, Vec3::ZERO),
            crate::entity::Entity::new(2, crate::entity::EntityType::Arrow, Vec3::ZERO),
            crate::entity::Entity::new(3, crate::entity::EntityType::HeartParticle, Vec3::ZERO),
            crate::entity::Entity::new(4, crate::entity::EntityType::EndCrystal, Vec3::ZERO),
        ];
        for entity in &mut nonliving {
            entity.velocity = Vec3::new(2.0, 1.0, -3.0);
            let health = entity.health;
            let velocity = entity.velocity;
            assert!(apply_player_splash_effect(entity, poison).is_none());
            assert!(apply_player_splash_effect(entity, slowness).is_none());
            assert_eq!(entity.health, health);
            assert_eq!(entity.velocity, velocity);
        }

        let mut zombie =
            crate::entity::Entity::new(5, crate::entity::EntityType::Zombie, Vec3::ZERO);
        zombie.health = 10.0;
        zombie.velocity = Vec3::new(2.0, 1.0, -3.0);
        assert!(apply_player_splash_effect(&mut zombie, poison).is_none());
        assert_eq!(zombie.health, 8.0);
        assert!(apply_player_splash_effect(&mut zombie, slowness).is_none());
        assert_eq!(zombie.velocity, Vec3::new(0.8, 0.4, -1.2));
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
    }

    #[test]
    fn invulnerable_melee_target_consumes_impact_without_damage_or_knockback() {
        let mut zombie = crate::entity::Entity::new(
            1,
            crate::entity::EntityType::Zombie,
            Vec3::new(0.0, 0.0, 2.0),
        );
        zombie.invulnerable_time = 0.25;
        let initial_health = zombie.health;
        let initial_velocity = zombie.velocity;

        assert_eq!(
            apply_melee_impact(&mut zombie, Vec3::Z, 5.0, 8.0, 2),
            MeleeImpact::Invulnerable
        );
        assert_eq!(zombie.health, initial_health);
        assert_eq!(zombie.velocity, initial_velocity);
        assert_eq!(zombie.fire_aspect_timer, 0.0);
    }

    #[test]
    fn melee_impact_applies_damage_knockback_fire_and_reports_lethal_hits() {
        let mut zombie = crate::entity::Entity::new(
            1,
            crate::entity::EntityType::Zombie,
            Vec3::new(0.0, 0.0, 2.0),
        );
        zombie.health = 5.0;

        assert_eq!(
            apply_melee_impact(&mut zombie, Vec3::Z, 5.0, 8.0, 2),
            MeleeImpact::Damaged { killed: true }
        );
        assert_eq!(zombie.health, 0.0);
        assert_eq!(zombie.invulnerable_time, 0.4);
        assert_eq!(zombie.velocity, Vec3::new(0.0, 3.0, 8.0));
        assert_eq!(zombie.fire_aspect_timer, 8.0);
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
    fn initial_world_load_is_bounded_independently_of_render_distance() {
        assert_eq!(initial_chunk_radius(0), 0);
        assert_eq!(initial_chunk_radius(2), INITIAL_WORLD_CHUNK_RADIUS);
        assert_eq!(initial_chunk_radius(12), INITIAL_WORLD_CHUNK_RADIUS);
        assert_eq!(initial_chunk_radius(16), INITIAL_WORLD_CHUNK_RADIUS);
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
    fn host_inbound_block_request_preserves_authenticated_player_id() {
        let (inbound_tx, inbound_rx) = std::sync::mpsc::channel();
        let (outbound_tx, _outbound_rx) = std::sync::mpsc::channel();
        let handle = NetworkHandle::Host {
            server_to_host: inbound_rx,
            host_to_server: outbound_tx,
            thread: None,
        };
        inbound_tx
            .send(crate::network::server::ServerToHost::ClientBlockChange {
                id: 7,
                x: 3,
                y: 80,
                z: -4,
                block: BlockType::Stone.to_wire(),
                state: 0,
            })
            .unwrap();

        let events = handle.drain_inbound();
        assert!(matches!(
            events.as_slice(),
            [NetworkInbound::ClientBlockChange {
                id: 7,
                x: 3,
                y: 80,
                z: -4,
                block,
                state: 0,
            }] if *block == BlockType::Stone.to_wire()
        ));
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
            Biome::Mountains,
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
    fn block_at_exact_reach_distance_passes() {
        let block_center = Vec3::new(0.5, 0.5, 0.5);
        let player_pos = block_center + Vec3::new(BLOCK_REACH, 0.0, 0.0);
        assert!(block_within_reach(player_pos, (0, 0, 0)));
    }

    #[test]
    fn block_within_tolerance_passes() {
        let block_center = Vec3::new(0.5, 0.5, 0.5);
        let player_pos = block_center + Vec3::new(6.0, 0.0, 0.0);
        assert!(block_within_reach(player_pos, (0, 0, 0)));
    }

    #[test]
    fn block_at_tolerance_boundary_passes() {
        let block_center = Vec3::new(0.5, 0.5, 0.5);
        let limit = BLOCK_REACH + BLOCK_REACH_TOLERANCE;
        let player_pos = block_center + Vec3::new(limit, 0.0, 0.0);
        assert!(block_within_reach(player_pos, (0, 0, 0)));
    }

    #[test]
    fn block_just_beyond_tolerance_is_rejected() {
        let block_center = Vec3::new(0.5, 0.5, 0.5);
        let player_pos = block_center + Vec3::new(6.51, 0.0, 0.0);
        assert!(!block_within_reach(player_pos, (0, 0, 0)));
    }

    #[test]
    fn block_far_away_is_rejected() {
        let block_center = Vec3::new(0.5, 0.5, 0.5);
        let player_pos = block_center + Vec3::new(10.0, 0.0, 0.0);
        assert!(!block_within_reach(player_pos, (0, 0, 0)));
    }

    #[test]
    fn diagonal_neighbor_block_passes() {
        let player_pos = Vec3::new(0.5, 0.5, 0.5);
        assert!(block_within_reach(player_pos, (1, 1, 1)));
    }

    #[test]
    fn block_center_uses_half_offset() {
        let player_pos = Vec3::new(10.5, 0.5, 0.5);
        assert!(block_within_reach(player_pos, (5, 0, 0)));
    }

    #[test]
    fn same_position_block_passes() {
        let player_pos = Vec3::new(0.5, 0.5, 0.5);
        assert!(block_within_reach(player_pos, (0, 0, 0)));
    }

    #[test]
    fn negative_coordinates_use_block_center_offset() {
        let block_center = Vec3::new(-4.5, 0.5, -4.5);
        let player_pos = block_center + Vec3::new(0.0, 0.0, 6.0);
        assert!(block_within_reach(player_pos, (-5, 0, -5)));
    }

    #[test]
    fn validate_remote_block_request_close_snapshot_passes() {
        let mut remote_players = std::collections::HashMap::new();
        let mut remote = RemotePlayerState::new(1, "Alex".to_string());
        remote.snapshots.push_back(PlayerSnapshot {
            position: Vec3::new(0.0, 60.0, 0.0),
            yaw: 0.0,
            pitch: 0.0,
            time: 0.0,
            sequence: 1,
            sender_time_millis: 100,
        });
        remote_players.insert(7, remote);
        // Player center = (0.0, 60.9, 0.0); block center (0.5, 60.5, 2.5) -> distance approx 2.58 <= 6.5
        assert!(validate_remote_block_request(
            &remote_players,
            7,
            (0, 60, 2)
        ));
    }

    #[test]
    fn validate_remote_block_request_far_snapshot_rejected() {
        let mut remote_players = std::collections::HashMap::new();
        let mut remote = RemotePlayerState::new(1, "Alex".to_string());
        remote.snapshots.push_back(PlayerSnapshot {
            position: Vec3::new(0.0, 60.0, 0.0),
            yaw: 0.0,
            pitch: 0.0,
            time: 0.0,
            sequence: 1,
            sender_time_millis: 100,
        });
        remote_players.insert(7, remote);
        // Target block at (10, 60, 0) -> distance > 6.5
        assert!(!validate_remote_block_request(
            &remote_players,
            7,
            (10, 60, 0)
        ));
    }

    #[test]
    fn validate_remote_block_request_empty_snapshots_rejected() {
        let mut remote_players = std::collections::HashMap::new();
        let remote = RemotePlayerState::new(1, "Alex".to_string());
        remote_players.insert(7, remote);
        assert!(!validate_remote_block_request(
            &remote_players,
            7,
            (0, 60, 0)
        ));
    }

    #[test]
    fn validate_remote_block_request_unknown_requester_rejected() {
        let remote_players = std::collections::HashMap::new();
        assert!(!validate_remote_block_request(
            &remote_players,
            99,
            (0, 60, 0)
        ));
    }

    #[test]
    fn validate_remote_block_request_destroy_close_passes_and_far_rejected() {
        let mut remote_players = std::collections::HashMap::new();
        let mut remote = RemotePlayerState::new(1, "Alex".to_string());
        remote.snapshots.push_back(PlayerSnapshot {
            position: Vec3::new(0.0, 60.0, 0.0),
            yaw: 0.0,
            pitch: 0.0,
            time: 0.0,
            sequence: 1,
            sender_time_millis: 100,
        });
        remote_players.insert(7, remote);
        // Destroying Air block close by -> true
        assert!(validate_remote_block_request(
            &remote_players,
            7,
            (0, 60, 1)
        ));
        // Destroying Air block far away -> false
        assert!(!validate_remote_block_request(
            &remote_players,
            7,
            (0, 60, 20)
        ));
    }

    #[test]
    fn calculate_block_break_rewards_harvest_and_drops() {
        let pos = (10, 60, 10);

        // Stone with bare hand in Survival -> not eligible to harvest (no drops)
        let rewards =
            calculate_block_break_rewards(BlockType::Stone, pos, None, GameMode::Survival);
        assert!(rewards.drops.is_empty());
        assert_eq!(rewards.xp, 0);

        // Stone with Pickaxe -> eligible, drops Stone
        let pick = ItemStack::new(Item::StonePickaxe, 1);
        let rewards =
            calculate_block_break_rewards(BlockType::Stone, pos, Some(&pick), GameMode::Survival);
        assert_eq!(rewards.drops.len(), 1);
        assert_eq!(rewards.drops[0].item, Item::Stone);

        // DiamondOre with IronPickaxe -> drops Diamond + 5 XP
        let iron_pick = ItemStack::new(Item::IronPickaxe, 1);
        let rewards = calculate_block_break_rewards(
            BlockType::DiamondOre,
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
            pos,
            Some(&silk_pick),
            GameMode::Survival,
        );
        assert_eq!(rewards.drops[0].item, Item::DiamondOre);

        // Creative mode -> zero drops
        let rewards =
            calculate_block_break_rewards(BlockType::Stone, pos, Some(&pick), GameMode::Creative);
        assert!(rewards.drops.is_empty());
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
}
