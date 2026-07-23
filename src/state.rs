use crate::camera::{Camera, CameraUniform};
use crate::chunk_manager::{mark_block_mesh_dependencies, surrounding_chunk_coords, ChunkManager};
use crate::chunk_render::{
    build_draw_plan, select_lod_for_bounds, DrawCandidate, DrawLayer, Frustum, LodLevel,
    LodThresholds, MeshBounds, TerrainVertex,
};
use crate::crafting::RecipeManager;
use crate::interaction::raycast;
use crate::inventory::{GameMode, Inventory, Item, ItemStack, ToolType};
use crate::menu::{Difficulty, GameSettings, MultiplayerRole, WorldLaunch};
use crate::physics::{
    block_placement_decision, player_aabb_at, BlockPlacementDecision, PlayerPhysics, AABB,
};
use crate::player::{DamageSource, PlayerState};
use crate::world::{Biome, BlockType, Chunk, CHUNK_DEPTH, CHUNK_HEIGHT, CHUNK_WIDTH};
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
const REMOTE_MAX_EXTRAPOLATION: f64 = 0.1;
const REMOTE_MAX_EXTRAPOLATION_SPEED: f32 = 40.0;
const REMOTE_MAX_ANGULAR_SPEED: f32 = std::f32::consts::TAU * 2.0;
const REMOTE_TELEPORT_DISTANCE: f32 = 8.0;
const REMOTE_TELEPORT_GAP: f64 = 0.5;
const CREATIVE_FLIGHT_DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(300);
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
) -> Option<std::collections::HashSet<(i32, i32)>> {
    let ((cx, cz), _) = chunk_manager.world_to_local(x, y, z)?;
    if !chunk_manager.chunks.contains_key(&(cx, cz)) {
        return None;
    }
    let previous = chunk_manager.get_block(x, y, z);
    if previous == block {
        return None;
    }

    chunk_manager.set_block(x, y, z, block);
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

        let dirty = apply_synced_block_change(&mut manager, 15, 80, 8, BlockType::Stone)
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

        assert!(chunk_mesh_result_is_current(
            Some((7, 11)),
            7,
            11,
            3,
            3,
            Some(7),
            Some(11),
        ));
        assert!(!chunk_mesh_result_is_current(
            Some((7, 10)),
            7,
            11,
            3,
            3,
            Some(7),
            Some(11),
        ));
        assert!(!chunk_mesh_result_is_current(
            Some((7, 11)),
            7,
            11,
            3,
            3,
            Some(7),
            Some(12),
        ));
        assert!(!chunk_mesh_result_is_current(
            Some((7, 11)),
            7,
            11,
            2,
            3,
            Some(7),
            Some(11),
        ));
    }

    #[test]
    fn mesh_snapshot_owns_the_neighbor_halo() {
        let mut chunks = std::collections::HashMap::new();
        let mut center = Chunk::new(0, 0);
        let mut east = Chunk::new(1, 0);
        center.blocks[15][10][8] = BlockType::Stone;
        east.blocks[0][10][8] = BlockType::Dirt;
        east.sky_light[0][10][8] = 9;
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
                required_limits: wgpu::Limits::downlevel_defaults(),
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
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    num_indices: u32,
    bounds: Option<MeshBounds>,
}

pub struct GpuMeshLevel {
    opaque: GpuMeshLayer,
    transparent: GpuMeshLayer,
    bounds: Option<MeshBounds>,
}

pub struct ChunkMesh {
    levels: Option<[GpuMeshLevel; 3]>,
    revision: u64,
    meshed_revision: u64,
}

impl ChunkMesh {
    fn pending() -> Self {
        Self {
            levels: None,
            revision: 0,
            meshed_revision: u64::MAX,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.revision = self.revision.wrapping_add(1);
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
            .map(|level| level.opaque.num_indices as usize + level.transparent.num_indices as usize)
            .sum()
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
    chunk: Chunk,
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
        let chunk = chunks.get(&coord)?.clone();
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
                            block: neighbor.blocks[local_x][y][local_z],
                            sky_light: neighbor.sky_light[local_x][y][local_z],
                            block_light: neighbor.block_light[local_x][y][local_z],
                            fluid: neighbor.fluid_levels[local_x][y][local_z],
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
            chunk,
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
}

struct ChunkMeshResult {
    coord: (i32, i32),
    generation: u64,
    lifetime: u64,
    revision: u64,
    bundle: crate::chunk_render::ChunkMeshBundle,
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

fn chunk_mesh_result_is_current(
    expected_job: Option<(u64, u64)>,
    result_lifetime: u64,
    result_revision: u64,
    result_generation: u64,
    current_generation: u64,
    current_lifetime: Option<u64>,
    current_revision: Option<u64>,
) -> bool {
    expected_job == Some((result_lifetime, result_revision))
        && result_generation == current_generation
        && current_lifetime == Some(result_lifetime)
        && current_revision == Some(result_revision)
}

enum TerrainWorkerResult {
    Loaded(ChunkLoadResult),
    Meshed(ChunkMeshResult),
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
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: (std::mem::size_of::<[f32; 3]>() + 2 * std::mem::size_of::<[f32; 2]>())
                        as wgpu::BufferAddress,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32,
                },
                wgpu::VertexAttribute {
                    offset: (std::mem::size_of::<[f32; 3]>()
                        + 2 * std::mem::size_of::<[f32; 2]>()
                        + std::mem::size_of::<f32>())
                        as wgpu::BufferAddress,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        }
    }
}

impl State {
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
        for coord in dirty_chunks {
            if let Some(mesh) = self.chunk_meshes.get_mut(&coord) {
                mesh.mark_dirty();
            }
        }
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
        let game_mode = self.game_mode;
        let mut drops = Vec::new();
        let mut broken_blocks = Vec::new();
        self.chunk_manager.check_and_break_unsupported_above(
            wx,
            wy,
            wz,
            dirty_chunks,
            |(x, y, z), block| {
                broken_blocks.push((x, y, z));
                if game_mode == GameMode::Survival {
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
                        drops.push((
                            item,
                            glam::Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5),
                        ));
                    }
                }
            },
        );
        for (item, pos) in drops {
            self.spawn_dropped_item(item, pos);
        }
        for (x, y, z) in broken_blocks {
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
        for chunk in self.chunk_manager.chunks.values() {
            let data = crate::save::ChunkSaveData::from_chunk(chunk);
            let _ = self.save_tx.send(crate::save::SaveCommand::SaveChunk {
                dimension: source,
                data,
            });
        }

        let mut destination =
            crate::dimension::transform_position(source, target, self.player_physics.position);
        if target == crate::dimension::Dimension::End {
            destination = Vec3::new(0.5, 80.0, 0.5);
        } else if source == crate::dimension::Dimension::End {
            destination = Vec3::new(8.5, 80.0, 8.5);
        }

        self.current_dimension = target;
        let render_distance = self.chunk_manager.render_distance;
        self.chunk_manager = ChunkManager::new_in_dimension(render_distance, target);
        self.terrain_generation = self.terrain_generation.wrapping_add(1);
        self.chunk_load_in_flight.clear();
        self.chunk_mesh_in_flight.clear();
        self.chunk_lifetimes.clear();
        self.chunk_meshes.clear();
        self.entity_manager = crate::entity::EntityManager::new();
        self.particles = crate::particles::ParticleSystem::new();
        self.redstone = crate::redstone::RedstoneSystem::new();
        self.redstone_tick_timer = 0.0;
        self.pending_chunk_payloads.clear();
        self.pending_block_changes.clear();
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
        if let Some(saved) = self
            .save_manager
            .lock()
            .unwrap()
            .load_chunk_in(target, cx, cz)
        {
            let generated_blocks = crate::save::ChunkSaveData::from_chunk(&chunk).blocks;
            if saved.blocks != generated_blocks {
                self.mutated_chunks.insert((target, cx, cz));
            }
            saved.restore_to_chunk(&mut chunk);
        }
        self.chunk_manager.chunks.insert((cx, cz), chunk);
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
                let removed = crate::mob::explode(
                    explosion.position,
                    explosion.radius,
                    &mut self.chunk_manager,
                    &mut self.chunk_meshes,
                    &mut self.player_physics,
                    &mut self.player_state,
                    true,
                    GameMode::Creative,
                    0.0,
                );
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

    block_placement_decision(block, block_pos, player_aabbs)
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
    },
    AuthoritativeBlockChange {
        x: i32,
        y: i32,
        z: i32,
        block: u32,
    },
    ChunkData {
        cx: i32,
        cz: i32,
        blocks: Vec<u8>,
    },
    TimeSync {
        ticks: u64,
        weather: u8,
    },
    ChatFromClient {
        id: crate::network::protocol::PlayerId,
        message: String,
    },
    Chat {
        sender: String,
        message: String,
    },
    StatusUpdate(String),
}

impl NetworkHandle {
    fn drain_inbound(&self) -> Vec<NetworkInbound> {
        match self {
            NetworkHandle::None => Vec::new(),
            NetworkHandle::Host { server_to_host, .. } => server_to_host
                .try_iter()
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
                    } => NetworkInbound::ClientBlockChange { id, x, y, z, block },
                    crate::network::server::ServerToHost::ChatFromClient { id, message } => {
                        NetworkInbound::ChatFromClient { id, message }
                    }
                })
                .collect(),
            NetworkHandle::Client { client_to_game, .. } => client_to_game
                .try_iter()
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
                    crate::network::client::ClientToGame::BlockChange { x, y, z, block } => {
                        NetworkInbound::AuthoritativeBlockChange { x, y, z, block }
                    }
                    crate::network::client::ClientToGame::ChunkData { cx, cz, blocks } => {
                        NetworkInbound::ChunkData { cx, cz, blocks }
                    }
                    crate::network::client::ClientToGame::TimeSync { ticks, weather } => {
                        NetworkInbound::TimeSync { ticks, weather }
                    }
                    crate::network::client::ClientToGame::Chat { sender, message } => {
                        NetworkInbound::Chat { sender, message }
                    }
                    crate::network::client::ClientToGame::StatusUpdate { message } => {
                        NetworkInbound::StatusUpdate(message)
                    }
                })
                .collect(),
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
                let _ = host_to_server.send(
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
                );
            }
            NetworkHandle::Client { game_to_client, .. } => {
                let _ = game_to_client.send(crate::network::client::GameToClient::SendPosition {
                    sequence,
                    sender_time_millis,
                    x: position.x,
                    y: position.y,
                    z: position.z,
                    yaw,
                    pitch,
                });
            }
            NetworkHandle::None => {}
        }
    }

    fn request_block_change(&self, x: i32, y: i32, z: i32, block: u32) {
        if let NetworkHandle::Client { game_to_client, .. } = self {
            let _ = game_to_client.send(crate::network::client::GameToClient::RequestBlockChange {
                x,
                y,
                z,
                block,
            });
        }
    }

    /// Host-only: fan a block mutation out to every connected client. The host
    /// applies the mutation locally through the canonical path and then calls
    /// this so peers render the same world state.
    fn broadcast_block_change(&self, x: i32, y: i32, z: i32, block: u32) {
        if let NetworkHandle::Host { host_to_server, .. } = self {
            let _ =
                host_to_server.send(crate::network::server::HostToServer::BroadcastBlockChange {
                    x,
                    y,
                    z,
                    block,
                });
        }
    }

    /// Host-only: push a full chunk payload to a specific joining client as
    /// part of mid-game join catch-up.
    fn send_chunk_to(
        &self,
        cx: i32,
        cz: i32,
        blocks: Vec<u8>,
        to: crate::network::protocol::PlayerId,
    ) {
        if let NetworkHandle::Host { host_to_server, .. } = self {
            let _ = host_to_server.send(crate::network::server::HostToServer::SendChunk {
                cx,
                cz,
                blocks,
                to,
            });
        }
    }

    fn broadcast_time_sync(&self, ticks: u64, weather: u8) {
        if let NetworkHandle::Host { host_to_server, .. } = self {
            let _ = host_to_server
                .send(crate::network::server::HostToServer::BroadcastTimeSync { ticks, weather });
        }
    }

    fn send_action(&self, action: crate::network::protocol::Action) {
        match self {
            NetworkHandle::Host { host_to_server, .. } => {
                let _ = host_to_server.send(
                    crate::network::server::HostToServer::BroadcastPlayerAction { id: 0, action },
                );
            }
            NetworkHandle::Client { game_to_client, .. } => {
                let _ = game_to_client
                    .send(crate::network::client::GameToClient::SendAction { action });
            }
            NetworkHandle::None => {}
        }
    }

    fn send_chat(&self, sender: String, message: String) {
        match self {
            NetworkHandle::Host { host_to_server, .. } => {
                let _ = host_to_server
                    .send(crate::network::server::HostToServer::BroadcastChat { sender, message });
            }
            NetworkHandle::Client { game_to_client, .. } => {
                let _ =
                    game_to_client.send(crate::network::client::GameToClient::SendChat { message });
            }
            NetworkHandle::None => {}
        }
    }

    fn notify_player_join(&self, id: crate::network::protocol::PlayerId, username: String) {
        if let NetworkHandle::Host { host_to_server, .. } = self {
            let _ = host_to_server
                .send(crate::network::server::HostToServer::NotifyPlayerJoin { id, username });
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
                let _ = host_to_server.send(crate::network::server::HostToServer::Stop);
                thread.take()
            }
            NetworkHandle::Client {
                game_to_client,
                thread,
                ..
            } => {
                let _ = game_to_client.send(crate::network::client::GameToClient::Disconnect);
                thread.take()
            }
        };
        if let Some(thread) = thread {
            let _ = thread.join();
        }
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
    terrain_worker_tx: std::sync::mpsc::Sender<TerrainWorkerResult>,
    terrain_worker_rx: std::sync::mpsc::Receiver<TerrainWorkerResult>,
    chunk_load_in_flight: std::collections::HashMap<(i32, i32), u64>,
    chunk_mesh_in_flight: std::collections::HashMap<(i32, i32), (u64, u64)>,
    chunk_lifetimes: std::collections::HashMap<(i32, i32), u64>,
    next_chunk_lifetime: u64,
    terrain_generation: u64,
    submitted_terrain_triangles: u64,
    submitted_terrain_draw_calls: usize,
    visible_chunk_count: usize,
    pub player_physics: PlayerPhysics,
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
    mob_vertex_buffer: wgpu::Buffer,
    mob_index_buffer: wgpu::Buffer,
    mob_num_indices: u32,
    hand_pipeline: wgpu::RenderPipeline,
    hand_vertex_buffer: wgpu::Buffer,
    hand_index_buffer: wgpu::Buffer,
    hand_num_indices: u32,
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
    pub save_tx: std::sync::mpsc::Sender<crate::save::SaveCommand>,
    pub autosave_timer: f32,
    pub is_saving: bool,
    pub is_sprinting: bool,
    pub base_fov: f32,
    pub w_click_timer: f32,
    pub last_w_pressed: bool,
    debug_frame_time_accumulator: f32,
    debug_frame_samples: u32,
    debug_fps: f32,
    debug_frame_ms: f32,
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
    network_ready: bool,
    local_player_id: Option<crate::network::protocol::PlayerId>,
    remote_players:
        std::collections::HashMap<crate::network::protocol::PlayerId, RemotePlayerState>,
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
    pending_chunk_payloads: std::collections::HashMap<(i32, i32), Vec<u8>>,
    /// Client-only coalesced mutations for chunks that are not streamed in yet.
    /// The latest authoritative value wins for each world-space block.
    pending_block_changes:
        std::collections::HashMap<(i32, i32), std::collections::HashMap<(i32, i32, i32), u32>>,
    /// Host-only set of dimension-namespaced chunks that differ from their
    /// deterministic generated form and therefore need join-time catch-up.
    mutated_chunks: std::collections::HashSet<(crate::dimension::Dimension, i32, i32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotType {
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

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    label: None,
                },
                None,
            )
            .await
            .unwrap();

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
            present_mode: if settings.vsync
                || !surface_caps
                    .present_modes
                    .contains(&wgpu::PresentMode::Immediate)
            {
                wgpu::PresentMode::Fifo
            } else {
                wgpu::PresentMode::Immediate
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

        // Spawn background worker thread
        let (save_tx, save_rx) = std::sync::mpsc::channel::<crate::save::SaveCommand>();
        let save_manager_clone = std::sync::Arc::clone(&save_manager);
        std::thread::spawn(move || {
            while let Ok(cmd) = save_rx.recv() {
                match cmd {
                    crate::save::SaveCommand::SaveChunk { dimension, data } => {
                        let mut mgr = save_manager_clone.lock().unwrap();
                        let _ = mgr.save_chunk_in(dimension, data.chunk_x, data.chunk_z, data);
                    }
                    crate::save::SaveCommand::SaveLevelAndPlayer(level, player) => {
                        let mgr = save_manager_clone.lock().unwrap();
                        let _ = mgr.save_player_and_level(&level, &player);
                    }
                }
            }
        });

        // Initialize physics and keyboard input
        let mut player_physics = PlayerPhysics::new(Vec3::new(8.0, 80.0, 8.0));
        let keys = KeyState::default();

        let mut audio_manager = crate::audio::AudioManager::new();
        audio_manager.set_volume(settings.effective_sound_volume());

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

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[&camera_bind_group_layout],
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
                layout: Some(&render_pipeline_layout),
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
                layout: Some(&render_pipeline_layout),
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
        let mut mutated_chunks = std::collections::HashSet::new();

        // Load only the immediate spawn area synchronously.  Loading every
        // chunk in a large render distance here used to create all CPU/GPU
        // meshes in one window event (625 chunks at distance 12), freezing the
        // app and often causing the graphics driver to reset.  `update_chunks`
        // loads the remaining requested chunks one at a time after the first
        // frame is visible.
        let player_chunk_x = (player_physics.position.x / CHUNK_WIDTH as f32).floor() as i32;
        let player_chunk_z = (player_physics.position.z / CHUNK_DEPTH as f32).floor() as i32;
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
                            mutated_chunks.insert((current_dimension, cx, cz));
                        }
                        data.restore_to_chunk(&mut chunk);
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

        Self {
            window,
            surface,
            device,
            queue,
            config,
            size,
            terrain_render_pipeline,
            terrain_trans_pipeline,
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
            terrain_worker_tx,
            terrain_worker_rx,
            chunk_load_in_flight: std::collections::HashMap::new(),
            chunk_mesh_in_flight: std::collections::HashMap::new(),
            chunk_lifetimes,
            next_chunk_lifetime,
            terrain_generation: 0,
            submitted_terrain_triangles: 0,
            submitted_terrain_draw_calls: 0,
            visible_chunk_count: 0,
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
            autosave_timer: 0.0,
            is_saving: false,
            is_sprinting: false,
            base_fov,
            w_click_timer: 0.0,
            last_w_pressed: false,
            debug_frame_time_accumulator: 0.0,
            debug_frame_samples: 0,
            debug_fps: 0.0,
            debug_frame_ms: 0.0,
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
            network_ready: !is_client,
            local_player_id: None,
            remote_players: std::collections::HashMap::new(),
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
            mutated_chunks,
        }
    }

    pub fn save_settings(&self) {
        let mut settings = self.settings.clone();
        settings.fov = self.camera.fov;
        settings.sensitivity = self.sensitivity;
        settings.render_distance = self.chunk_manager.render_distance;
        settings.master_volume = if settings.sound_volume > 0.0 {
            (self.audio_manager.volume / settings.sound_volume).clamp(0.0, 1.0)
        } else {
            settings.master_volume
        };
        settings.save();
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
        self.mutated_chunks.insert((self.current_dimension, cx, cz));
        self.network
            .broadcast_block_change(x, y, z, block.to_wire());
    }

    fn send_mutated_chunks_to(&self, player_id: crate::network::protocol::PlayerId) {
        let payloads: Vec<_> = self
            .mutated_chunks
            .iter()
            .filter_map(|&(dimension, cx, cz)| {
                if dimension != self.current_dimension {
                    return None;
                }
                self.chunk_manager.chunks.get(&(cx, cz)).map(|chunk| {
                    let blocks = crate::save::ChunkSaveData::from_chunk(chunk).blocks;
                    (cx, cz, blocks)
                })
            })
            .collect();
        for (cx, cz, blocks) in payloads {
            self.network.send_chunk_to(cx, cz, blocks, player_id);
        }
    }

    fn weather_wire_value(&self) -> u8 {
        match self.weather.current {
            crate::weather::Weather::Clear => 0,
            crate::weather::Weather::Rain => 1,
            crate::weather::Weather::Thunder => 2,
        }
    }

    fn broadcast_time_sync(&self) {
        self.network
            .broadcast_time_sync(self.world_time.ticks, self.weather_wire_value());
    }

    fn drain_network_events(&mut self) {
        for event in self.network.drain_inbound() {
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
                    self.terrain_generation = self.terrain_generation.wrapping_add(1);
                    self.chunk_load_in_flight.clear();
                    self.chunk_mesh_in_flight.clear();
                    self.chunk_lifetimes.clear();
                    self.chunk_meshes.clear();
                    self.pending_chunk_payloads.clear();
                    self.pending_block_changes.clear();
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
                    self.network_ready = false;
                    self.network_status = Some(format!("CONNECTION LOST: {reason}"));
                    self.connection_lost = true;
                    self.is_chat_open = false;
                    self.chat_input.clear();
                    clear_remote_players(&mut self.remote_players, &mut self.entity_manager);
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
                            if let Some(entity) = self
                                .entity_manager
                                .entities
                                .iter_mut()
                                .find(|e| e.id == remote.entity_id)
                            {
                                entity.username = username.clone();
                            }
                        } else {
                            let entity_id = self.entity_manager.spawn(
                                crate::entity::EntityType::RemotePlayer,
                                self.player_physics.position,
                            );
                            if let Some(entity) = self
                                .entity_manager
                                .entities
                                .iter_mut()
                                .find(|e| e.id == entity_id)
                            {
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
                        self.network.notify_player_join(id, username);
                        self.send_mutated_chunks_to(id);
                        self.broadcast_time_sync();
                    }
                }
                NetworkInbound::PlayerLeave(id) => {
                    if let Some(remote) = self.remote_players.remove(&id) {
                        push_chat_history(
                            &mut self.chat_messages,
                            "[Network]".into(),
                            format!("{} left the game", remote.username),
                        );
                        self.entity_manager
                            .entities
                            .retain(|e| e.id != remote.entity_id);
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
                        continue;
                    }
                    if !self.remote_players.contains_key(&id) {
                        let username = String::new();
                        let entity_id = self
                            .entity_manager
                            .spawn(crate::entity::EntityType::RemotePlayer, Vec3::new(x, y, z));
                        if let Some(entity) = self
                            .entity_manager
                            .entities
                            .iter_mut()
                            .find(|e| e.id == entity_id)
                        {
                            entity.player_id = id;
                        }
                        let mut remote = RemotePlayerState::new(entity_id, username);
                        remote.push_snapshot(
                            Vec3::new(x, y, z),
                            yaw,
                            pitch,
                            sequence,
                            sender_time_millis,
                            self.network_time,
                        );
                        self.remote_players.insert(id, remote);
                    } else if let Some(remote) = self.remote_players.get_mut(&id) {
                        remote.push_snapshot(
                            Vec3::new(x, y, z),
                            yaw,
                            pitch,
                            sequence,
                            sender_time_millis,
                            self.network_time,
                        );
                    }
                    if matches!(self.role, MultiplayerRole::Host { .. }) {
                        if let NetworkHandle::Host { host_to_server, .. } = &self.network {
                            let _ = host_to_server.send(
                                crate::network::server::HostToServer::BroadcastPlayerPosition {
                                    id,
                                    sequence,
                                    sender_time_millis,
                                    x,
                                    y,
                                    z,
                                    yaw,
                                    pitch,
                                },
                            );
                        }
                    }
                }
                NetworkInbound::PlayerAction { id, action } => {
                    if let Some(remote) = self.remote_players.get(&id) {
                        if let Some(entity) = self
                            .entity_manager
                            .entities
                            .iter_mut()
                            .find(|e| e.id == remote.entity_id)
                        {
                            entity.action_cooldown = match action {
                                crate::network::protocol::Action::Place
                                | crate::network::protocol::Action::Break
                                | crate::network::protocol::Action::Use => 0.25,
                            };
                        }
                    }
                    if matches!(self.role, MultiplayerRole::Host { .. }) {
                        if let NetworkHandle::Host { host_to_server, .. } = &self.network {
                            let _ = host_to_server.send(
                                crate::network::server::HostToServer::BroadcastPlayerAction {
                                    id,
                                    action,
                                },
                            );
                        }
                    }
                }
                NetworkInbound::ClientBlockChange { id, x, y, z, block } => {
                    // The server supplied the authenticated session id. The
                    // host is the final authority for both player occupancy and
                    // the resulting world mutation.
                    self.set_block_and_broadcast(id, x, y, z, block);
                }
                NetworkInbound::AuthoritativeBlockChange { x, y, z, block } => {
                    // Clients always apply host authority without re-validating
                    // against their delayed render snapshots.
                    self.apply_remote_block_change(x, y, z, block);
                }
                NetworkInbound::ChunkData { cx, cz, blocks } => {
                    self.apply_remote_chunk_data(cx, cz, blocks);
                }
                NetworkInbound::TimeSync { ticks, weather } => {
                    if !self.is_authoritative() {
                        self.world_time.ticks = ticks;
                        self.world_time.tick_accumulator = 0.0;
                        self.weather.current = match weather {
                            1 => crate::weather::Weather::Rain,
                            2 => crate::weather::Weather::Thunder,
                            _ => crate::weather::Weather::Clear,
                        };
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
                        continue;
                    };
                    push_chat_history(&mut self.chat_messages, sender.clone(), message.clone());
                    self.network.send_chat(sender, message);
                }
                NetworkInbound::Chat { sender, message } => {
                    let Some(message) = normalized_chat_message(&message) else {
                        continue;
                    };
                    push_chat_history(&mut self.chat_messages, sender, message);
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

    pub fn shutdown_network(&mut self) {
        self.network.shutdown();
    }

    pub fn clear_movement_input(&mut self) {
        self.keys = KeyState::default();
        self.jump_taps.reset();
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
        let _ = self
            .window
            .set_cursor_grab(winit::window::CursorGrabMode::None);
        self.window.set_cursor_visible(true);
    }

    pub fn close_chat(&mut self) {
        self.chat_input.clear();
        if !self.is_chat_open {
            return;
        }
        self.is_chat_open = false;
        if !self.is_paused && !self.connection_lost && self.window.has_focus() {
            let _ = self
                .window
                .set_cursor_grab(winit::window::CursorGrabMode::Locked)
                .or_else(|_| {
                    self.window
                        .set_cursor_grab(winit::window::CursorGrabMode::Confined)
                });
            self.window.set_cursor_visible(false);
        }
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
            self.save_synchronously();
        }
        true
    }

    pub fn trigger_background_save(&self) {
        if !self.is_authoritative() {
            return;
        }
        let world_dir = self.save_manager.lock().unwrap().world_dir.clone();
        crate::menu::update_world_metadata(
            &world_dir,
            self.world_seed,
            self.game_mode,
            self.difficulty,
        );
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
        let _ = self
            .save_tx
            .send(crate::save::SaveCommand::SaveLevelAndPlayer(level, player));

        for chunk in self.chunk_manager.chunks.values() {
            let chunk_data = crate::save::ChunkSaveData::from_chunk(chunk);
            let _ = self.save_tx.send(crate::save::SaveCommand::SaveChunk {
                dimension: self.current_dimension,
                data: chunk_data,
            });
        }
        let _ = self
            .save_manager
            .lock()
            .unwrap()
            .save_current_dimension(self.current_dimension);
    }

    pub fn save_synchronously(&self) {
        if !self.is_authoritative() {
            return;
        }
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

        let mut mgr = self.save_manager.lock().unwrap();
        let _ = mgr.save_player_and_level(&level, &player);

        for chunk in self.chunk_manager.chunks.values() {
            let chunk_data = crate::save::ChunkSaveData::from_chunk(chunk);
            let _ = mgr.save_chunk_in(
                self.current_dimension,
                chunk.chunk_x,
                chunk.chunk_z,
                chunk_data,
            );
        }
        let _ = mgr.save_current_dimension(self.current_dimension);
        crate::menu::update_world_metadata(
            &mgr.world_dir,
            self.world_seed,
            self.game_mode,
            self.difficulty,
        );
        println!("[Save] Synchronously saved world state.");
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
        if self.inventory.is_open {
            self.close_inventory();
        }
        self.advancement_gui.open();
        self.clear_movement_input();
        let _ = self
            .window
            .set_cursor_grab(winit::window::CursorGrabMode::None);
        self.window.set_cursor_visible(true);
    }

    pub fn close_advancements_ui(&mut self) {
        self.advancement_gui.close();
        if !self.is_paused && !self.inventory.is_open {
            let _ = self
                .window
                .set_cursor_grab(winit::window::CursorGrabMode::Confined)
                .or_else(|_| {
                    self.window
                        .set_cursor_grab(winit::window::CursorGrabMode::Locked)
                });
            self.window.set_cursor_visible(false);
        }
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

    fn create_gpu_mesh_layer(
        device: &wgpu::Device,
        data: &crate::chunk_render::ChunkMeshData,
        label: &'static str,
    ) -> GpuMeshLayer {
        const EMPTY_BYTES: [u8; 4] = [0; 4];
        let vertex_bytes = bytemuck::cast_slice(&data.vertices);
        let index_bytes = bytemuck::cast_slice(&data.indices);
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: if vertex_bytes.is_empty() {
                &EMPTY_BYTES
            } else {
                vertex_bytes
            },
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: if index_bytes.is_empty() {
                &EMPTY_BYTES
            } else {
                index_bytes
            },
            usage: wgpu::BufferUsages::INDEX,
        });
        GpuMeshLayer {
            vertex_buffer,
            index_buffer,
            num_indices: data.indices.len() as u32,
            bounds: data.bounds,
        }
    }

    fn upload_mesh_bundle(
        device: &wgpu::Device,
        bundle: &crate::chunk_render::ChunkMeshBundle,
    ) -> [GpuMeshLevel; 3] {
        std::array::from_fn(|index| {
            let data = &bundle.levels[index];
            GpuMeshLevel {
                opaque: Self::create_gpu_mesh_layer(
                    device,
                    &data.opaque,
                    "Chunk Opaque Mesh Buffer",
                ),
                transparent: Self::create_gpu_mesh_layer(
                    device,
                    &data.transparent,
                    "Chunk Translucent Mesh Buffer",
                ),
                bounds: data.bounds(),
            }
        })
    }

    fn next_chunk_lifetime(&mut self) -> u64 {
        let lifetime = self.next_chunk_lifetime;
        self.next_chunk_lifetime = self.next_chunk_lifetime.wrapping_add(1).max(1);
        lifetime
    }

    fn process_terrain_worker_results(&mut self, player_chunk: (i32, i32)) {
        while let Ok(result) = self.terrain_worker_rx.try_recv() {
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
                        continue;
                    }

                    let (cx, cz) = result.coord;
                    if result.mutated {
                        self.mutated_chunks.insert((self.current_dimension, cx, cz));
                    }
                    self.chunk_manager.chunks.insert(result.coord, result.chunk);
                    self.chunk_lifetimes.insert(result.coord, result.lifetime);
                    self.chunk_meshes.insert(result.coord, ChunkMesh::pending());

                    if let Some(blocks) = self.pending_chunk_payloads.remove(&result.coord) {
                        if let Some(chunk) = self.chunk_manager.chunks.get_mut(&result.coord) {
                            Self::restore_chunk_payload(chunk, &blocks);
                        }
                        if let Some(mesh) = self.chunk_meshes.get_mut(&result.coord) {
                            mesh.mark_dirty();
                        }
                    }
                    if let Some(changes) = self.pending_block_changes.remove(&result.coord) {
                        for ((x, y, z), block) in changes {
                            self.apply_remote_block_change(x, y, z, block);
                        }
                    }

                    let mut dirty = std::collections::HashSet::new();
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
                    for neighbor in surrounding_chunk_coords(cx, cz) {
                        if let Some(mesh) = self.chunk_meshes.get_mut(&neighbor) {
                            mesh.mark_dirty();
                        }
                    }
                    for coord in dirty {
                        if let Some(mesh) = self.chunk_meshes.get_mut(&coord) {
                            mesh.mark_dirty();
                        }
                    }
                }
                TerrainWorkerResult::Meshed(result) => {
                    let expected = self.chunk_mesh_in_flight.get(&result.coord).copied();
                    if expected == Some((result.lifetime, result.revision)) {
                        self.chunk_mesh_in_flight.remove(&result.coord);
                    }
                    let current_lifetime = self.chunk_lifetimes.get(&result.coord).copied();
                    let current_revision = self
                        .chunk_meshes
                        .get(&result.coord)
                        .map(|mesh| mesh.revision);
                    if !chunk_mesh_result_is_current(
                        expected,
                        result.lifetime,
                        result.revision,
                        result.generation,
                        self.terrain_generation,
                        current_lifetime,
                        current_revision,
                    ) {
                        continue;
                    }
                    let Some(mesh) = self.chunk_meshes.get_mut(&result.coord) else {
                        continue;
                    };
                    mesh.levels = Some(Self::upload_mesh_bundle(&self.device, &result.bundle));
                    mesh.meshed_revision = result.revision;
                }
            }
        }
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
            if authoritative {
                if let Some(saved) = save_manager
                    .lock()
                    .unwrap()
                    .load_chunk_in(dimension, coord.0, coord.1)
                {
                    let generated_blocks = crate::save::ChunkSaveData::from_chunk(&chunk).blocks;
                    mutated = saved.blocks != generated_blocks;
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
            }));
        });
    }

    fn schedule_chunk_mesh(&mut self, coord: (i32, i32), default_sky_light: u8) {
        if self.chunk_mesh_in_flight.contains_key(&coord)
            || self.chunk_mesh_in_flight.len() >= MAX_CHUNK_MESH_JOBS
        {
            return;
        }
        let Some(mesh) = self.chunk_meshes.get(&coord) else {
            return;
        };
        let Some(lifetime) = self.chunk_lifetimes.get(&coord).copied() else {
            return;
        };
        let revision = mesh.revision;
        let Some(snapshot) =
            MeshSnapshot::capture(coord, &self.chunk_manager.chunks, default_sky_light)
        else {
            return;
        };
        self.chunk_mesh_in_flight
            .insert(coord, (lifetime, revision));
        let sender = self.terrain_worker_tx.clone();
        let generation = self.terrain_generation;
        rayon::spawn(move || {
            let bundle = snapshot
                .chunk
                .generate_mesh_bundle(|x, y, z| snapshot.get(x, y, z));
            let _ = sender.send(TerrainWorkerResult::Meshed(ChunkMeshResult {
                coord,
                generation,
                lifetime,
                revision,
                bundle,
            }));
        });
    }

    pub fn update_chunks(&mut self) {
        if !self.network_ready {
            return;
        }
        let player_pos = self.player_physics.position;
        let px = (player_pos.x / 16.0).floor() as i32;
        let pz = (player_pos.z / 16.0).floor() as i32;
        let r = self.chunk_manager.render_distance;
        self.process_terrain_worker_results((px, pz));

        // 1. Unload out-of-bounds chunks
        let mut to_unload = Vec::new();
        for &(cx, cz) in self.chunk_manager.chunks.keys() {
            if (cx - px).abs() > r || (cz - pz).abs() > r {
                to_unload.push((cx, cz));
            }
        }
        for &(cx, cz) in &to_unload {
            if let Some(chunk) = self.chunk_manager.chunks.remove(&(cx, cz)) {
                if self.is_authoritative() {
                    let chunk_data = crate::save::ChunkSaveData::from_chunk(&chunk);
                    let _ = self.save_tx.send(crate::save::SaveCommand::SaveChunk {
                        dimension: self.current_dimension,
                        data: chunk_data,
                    });
                }
            }
        }
        for &(cx, cz) in &to_unload {
            for neighbor in surrounding_chunk_coords(cx, cz) {
                if self.chunk_manager.chunks.contains_key(&neighbor) {
                    if let Some(mesh) = self.chunk_meshes.get_mut(&neighbor) {
                        mesh.mark_dirty();
                    }
                }
            }
            self.chunk_lifetimes.remove(&(cx, cz));
            self.chunk_mesh_in_flight.remove(&(cx, cz));
        }
        self.chunk_meshes
            .retain(|&(cx, cz), _| (cx - px).abs() <= r && (cz - pz).abs() <= r);
        self.chunk_load_in_flight
            .retain(|&(cx, cz), _| (cx - px).abs() <= r && (cz - pz).abs() <= r);

        // 2. Queue missing chunks
        let mut load_queue = Vec::new();
        for dx in -r..=r {
            for dz in -r..=r {
                let cx = px + dx;
                let cz = pz + dz;
                if !self.chunk_manager.chunks.contains_key(&(cx, cz))
                    && !self.chunk_load_in_flight.contains_key(&(cx, cz))
                {
                    load_queue.push((cx, cz));
                }
            }
        }

        load_queue.sort_by_key(|&(cx, cz)| {
            let dx = cx - px;
            let dz = cz - pz;
            dx * dx + dz * dz
        });

        // 3. Deterministic terrain generation and save restore run on Rayon.
        let available_load_slots =
            MAX_CHUNK_LOAD_JOBS.saturating_sub(self.chunk_load_in_flight.len());
        for coord in load_queue.into_iter().take(available_load_slots) {
            self.schedule_chunk_load(coord);
        }

        // 4. Snapshot and dispatch dirty meshes without waiting for workers.
        let mut to_rebuild = Vec::new();
        for (&(cx, cz), mesh) in &self.chunk_meshes {
            if mesh.needs_rebuild() && !self.chunk_mesh_in_flight.contains_key(&(cx, cz)) {
                let dx = cx - px;
                let dz = cz - pz;
                to_rebuild.push((cx, cz, dx * dx + dz * dz));
            }
        }

        // Sort by distance — rebuild closest chunks first
        to_rebuild.sort_by_key(|&(_, _, dist)| dist);

        let default_sky_light = if self.current_dimension.has_sky_light() {
            15
        } else {
            0
        };
        let available_mesh_slots =
            MAX_CHUNK_MESH_JOBS.saturating_sub(self.chunk_mesh_in_flight.len());
        for (cx, cz, _) in to_rebuild.into_iter().take(available_mesh_slots) {
            self.schedule_chunk_mesh((cx, cz), default_sky_light);
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.is_paused = paused;
        println!("[Debug] set_paused called with: {}", paused);
        if paused {
            let res = self
                .window
                .set_cursor_grab(winit::window::CursorGrabMode::None);
            println!("[Debug] Release grab result: {:?}", res);
            self.window.set_cursor_visible(true);
            self.clear_movement_input();
        } else {
            let res = self
                .window
                .set_cursor_grab(winit::window::CursorGrabMode::Locked)
                .or_else(|_| {
                    self.window
                        .set_cursor_grab(winit::window::CursorGrabMode::Confined)
                });
            println!("[Debug] Grab cursor result: {:?}", res);
            self.window.set_cursor_visible(false);
        }
    }

    pub fn handle_mouse_move(&mut self, x: f64, y: f64) {
        let ndc_x = (x as f32 / self.size.width as f32) * 2.0 - 1.0;
        let ndc_y = 1.0 - (y as f32 / self.size.height as f32) * 2.0;
        self.mouse_ndc = [ndc_x, ndc_y];
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
                    self.camera.fov = (self.camera.fov - 5.0).max(30.0);
                } else {
                    self.camera.fov = (self.camera.fov + 5.0).min(120.0);
                }
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
                self.queue.write_buffer(
                    &self.camera_buffer,
                    0,
                    bytemuck::cast_slice(&[self.camera_uniform]),
                );
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
            // Volume Button: X: [-0.3, 0.3], Y: [-0.32, -0.22]
            else if x >= -0.3 && x <= 0.3 && y >= -0.32 && y <= -0.22 {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::UiClick);
                let mut new_vol = self.audio_manager.volume;
                if x < 0.0 {
                    new_vol = (new_vol - 0.1).max(0.0);
                } else {
                    new_vol = (new_vol + 0.1).min(1.0);
                }
                self.audio_manager.set_volume(new_vol);
                self.save_settings();
            }
            // Quit Button bounds (Shifted): X: [-0.3, 0.3], Y: [-0.46, -0.36]
            else if x >= -0.3 && x <= 0.3 && y >= -0.46 && y <= -0.36 {
                self.audio_manager
                    .play_sound(crate::audio::SoundId::UiClick);
                self.is_saving = true;
                let _ = self.render();
                self.shutdown_network();
                self.save_synchronously();
                return true;
            }
        }
        false
    }

    pub fn update(&mut self, dt: f32) {
        self.network_time += f64::from(dt);
        self.drain_network_events();
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
        self.update_network_position(dt);
        if !self.network_ready {
            return;
        }

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
        }

        self.autosave_timer += dt;
        if self.is_authoritative() && self.autosave_timer >= 300.0 {
            self.autosave_timer = 0.0;
            self.trigger_background_save();
        }

        self.water_tick_timer += dt;
        if self.is_authoritative() && self.water_tick_timer >= 0.25 {
            self.water_tick_timer = 0.0;
            let (dirty, mutations) =
                crate::fluid::tick_fluids(&mut self.chunk_manager, false, 2048);
            for (cx, cz) in dirty {
                if let Some(mesh) = self.chunk_meshes.get_mut(&(cx, cz)) {
                    mesh.mark_dirty();
                }
            }
            // Fan fluid-driven block changes out to connected clients.
            for ((x, y, z), block) in mutations {
                self.broadcast_block_change(x, y, z, block);
            }
        }

        self.lava_tick_timer += dt;
        if self.is_authoritative() && self.lava_tick_timer >= 1.5 {
            self.lava_tick_timer = 0.0;
            let (dirty, mutations) = crate::fluid::tick_fluids(&mut self.chunk_manager, true, 512);
            for (cx, cz) in dirty {
                if let Some(mesh) = self.chunk_meshes.get_mut(&(cx, cz)) {
                    mesh.mark_dirty();
                }
            }
            for ((x, y, z), block) in mutations {
                self.broadcast_block_change(x, y, z, block);
            }
        }
        if self.player_state.is_dead {
            self.audio_manager.stop_looping_sound(RAIN_LOOP_ID);
            return;
        }
        if self.is_authoritative() {
            self.update_portal_travel(dt);
        }

        if self.is_authoritative() {
            self.redstone_tick_timer += dt;
        }
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

        self.brewing.update(dt);
        let effect_health = self.potion_effects.update(dt);
        if effect_health > 0.0 {
            self.player_state.health =
                (self.player_state.health + effect_health).min(self.player_state.max_health);
        } else if effect_health < 0.0 && self.player_state.health > 1.0 {
            self.take_damage(
                (-effect_health).min(self.player_state.health - 1.0),
                DamageSource::Mob,
            );
        }
        if self.wither_effect_timer > 0.0 {
            self.wither_effect_timer = (self.wither_effect_timer - dt).max(0.0);
            self.wither_damage_timer += dt;
            if self.wither_damage_timer >= 1.0 {
                self.wither_damage_timer -= 1.0;
                self.take_damage(1.0, DamageSource::Mob);
            }
        } else {
            self.wither_damage_timer = 0.0;
        }

        self.advancement_manager.update_toasts(dt);
        if self.advancement_gui.is_open && self.advancement_gui.is_dragging {
            let (screen_w, screen_h) = (self.config.width as f32, self.config.height as f32);
            let mouse_x = (self.mouse_ndc[0] + 1.0) * 0.5 * screen_w;
            let mouse_y = (1.0 - self.mouse_ndc[1]) * 0.5 * screen_h;
            self.advancement_gui.scroll_x = mouse_x - self.advancement_gui.drag_start_x;
            self.advancement_gui.scroll_y = mouse_y - self.advancement_gui.drag_start_y;
        }

        // Advance lightweight particle simulation every frame.
        self.particles.update(dt);

        // Double click W logic
        if self.keys.w && !self.last_w_pressed {
            if self.w_click_timer > 0.0 && self.player_state.hunger > 6.0 {
                self.is_sprinting = true;
            }
            self.w_click_timer = 0.3; // 0.3 seconds window
        }
        self.last_w_pressed = self.keys.w;
        if self.w_click_timer > 0.0 {
            self.w_click_timer -= dt;
        }

        // Ctrl key sprint check
        if self.keys.ctrl && self.keys.w && self.player_state.hunger > 6.0 {
            self.is_sprinting = true;
        }

        // Cancel sprinting conditions
        if !self.keys.w || self.keys.shift || self.player_state.hunger <= 6.0 {
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

        // Interpolate FOV smoothly
        let target_fov = if self.is_sprinting {
            self.base_fov * 1.12
        } else {
            self.base_fov
        };
        self.camera.fov = self.camera.fov + (target_fov - self.camera.fov) * dt * 10.0;

        // Consume more hunger when sprinting
        if self.is_sprinting && (self.keys.w || self.keys.a || self.keys.s || self.keys.d) {
            self.player_state.add_exhaustion(dt * 0.15);
        }

        // Update game time
        let elapsed_world_ticks = dt * 20.0;
        self.world_time.tick_accumulator += elapsed_world_ticks;
        let new_ticks = self.world_time.tick_accumulator.floor() as u64;
        self.world_time.ticks += new_ticks;
        self.world_time.tick_accumulator -= new_ticks as f32;
        if self.current_dimension == crate::dimension::Dimension::Overworld {
            let weather_update = self.weather.update(elapsed_world_ticks, dt);
            self.update_weather_effects(dt, weather_update.lightning_due);
        } else {
            self.audio_manager.stop_looping_sound(RAIN_LOOP_ID);
        }
        self.update_network_time_sync(dt);
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
        if jumped && self.game_mode == GameMode::Survival {
            self.player_state.add_exhaustion(0.05);
        }
        if jumped {
            self.audio_manager.play_sound(crate::audio::SoundId::Jump);
        }

        let old_pos = self.player_physics.position;

        let fall_damage = self.player_physics.update(
            dt,
            &self.chunk_manager,
            movement,
            self.keys.shift && !was_flying,
            self.is_sprinting,
        );
        if should_exit_creative_flight(was_flying, movement.y, self.player_physics.on_ground) {
            self.player_physics.set_flying(false);
            self.jump_taps.reset();
        }
        self.update_chunks();

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
        if self.game_mode == GameMode::Survival {
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

                    // Spawn footstep dust particles at the player's feet.
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

        // Torch smoke: periodically scan loaded chunks for torch blocks and
        // spawn a slowly rising smoke particle above each one.
        self.torch_smoke_timer += dt;
        if self.torch_smoke_timer >= 0.4 {
            self.torch_smoke_timer = 0.0;
            let mut rng = self.total_time.to_bits().wrapping_add(0x9E3779B9);
            let chunks: Vec<(i32, i32)> = self.chunk_manager.chunks.keys().copied().collect();
            for (cx, cz) in chunks {
                let chunk = match self.chunk_manager.chunks.get(&(cx, cz)) {
                    Some(c) => c,
                    None => continue,
                };
                // Scan a downsampled subset of columns for torches to keep the
                // cost bounded per frame.
                for bx in 0..16 {
                    for bz in 0..16 {
                        for by in (0..crate::world::CHUNK_HEIGHT).step_by(2) {
                            if chunk.blocks[bx][by][bz] == BlockType::Torch {
                                let wx = cx * crate::world::CHUNK_WIDTH as i32 + bx as i32;
                                let wy = by as i32;
                                let wz = cz * crate::world::CHUNK_DEPTH as i32 + bz as i32;
                                let torch_pos = glam::Vec3::new(
                                    wx as f32 + 0.5,
                                    wy as f32 + 0.6,
                                    wz as f32 + 0.5,
                                );
                                crate::particles::spawn_torch_smoke(
                                    &mut self.particles,
                                    torch_pos,
                                    &mut rng,
                                );
                                rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
                            }
                        }
                    }
                }
            }
        }

        // Dropped item collection: collect any DroppedItem entity within 1.5
        // meters of the player whose pickup cooldown has expired.
        {
            let player_pos = self.player_physics.position;
            let mut to_collect: Vec<usize> = Vec::new();
            for (i, entity) in self.entity_manager.entities.iter().enumerate() {
                if entity.entity_type != crate::entity::EntityType::DroppedItem {
                    continue;
                }
                if entity.pickup_cooldown > 0.0 {
                    continue;
                }
                if entity.dropped_item.is_none() {
                    continue;
                }
                let d = entity.position.distance(player_pos);
                if d < 1.5 {
                    to_collect.push(i);
                }
            }
            // Collect in reverse so indices stay valid as we remove.
            for &i in to_collect.iter().rev() {
                let item = self.entity_manager.entities[i].dropped_item;
                if let Some(item) = item {
                    let added = self.inventory.add_item(item);
                    if added {
                        self.entity_manager.entities.remove(i);
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
                self.take_damage(4.0, DamageSource::Mob); // Deal 4.0 damage (2 hearts) every 0.5s
            }
        } else {
            self.lava_damage_timer = 0.0;
        }

        // Leaf Decay Random Ticks
        let chunk_keys: Vec<(i32, i32)> = self.chunk_manager.chunks.keys().cloned().collect();
        if self.is_authoritative() && !chunk_keys.is_empty() {
            // Run 30 random ticks per frame
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
                let ry = next_rand(120) as i32 + 40; // Leaves usually spawn between Y=40..160

                let wx = cx * 16 + rx;
                let wz = cz * 16 + rz;

                let block = self.chunk_manager.get_block(wx, ry, wz);
                if block == BlockType::OakLeaves
                    || block == BlockType::BirchLeaves
                    || block == BlockType::SpruceLeaves
                {
                    // Run BFS check for log in radius 4
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
                        // Recalculate lighting & mark dirty meshes
                        let mut dirty_chunks = std::collections::HashSet::new();
                        crate::lighting::update_sky_light_after_removed(
                            &mut self.chunk_manager,
                            wx,
                            ry,
                            wz,
                            &mut dirty_chunks,
                        );
                        mark_block_mesh_dependencies(&mut dirty_chunks, wx, wz);
                        for (dcx, dcz) in dirty_chunks {
                            if let Some(mesh) = self.chunk_meshes.get_mut(&(dcx, dcz)) {
                                mesh.mark_dirty();
                            }
                        }
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
                self.take_damage(1.0, DamageSource::Mob); // Deal 1.0 contact damage (0.5 heart)
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
        if let Some((dmg, src)) = self.player_state.update_with_oxygen_rate(
            dt,
            is_underwater && !water_breathing,
            oxygen_rate,
        ) {
            self.take_damage(dmg, src);
        }

        self.total_time += dt;

        // Peaceful worlds keep passive creatures and dropped items, but remove
        // hostile actors immediately and do not schedule new hostile spawns.
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
        let authoritative = self.is_authoritative();
        let exploded_blocks = crate::mob::update_mobs(
            &mut self.entity_manager,
            &mut self.chunk_manager,
            &mut self.chunk_meshes,
            &mut self.player_physics,
            &mut self.player_state,
            self.game_mode,
            self.world_time.sky_light_level(),
            dt,
            &mut self.audio_manager,
            right,
            self.potion_effects.has_invisibility(),
            crate::enchantment::protection_multiplier(&self.inventory.armor, false),
            authoritative,
        );
        for (x, y, z) in exploded_blocks {
            self.broadcast_block_change(x, y, z, BlockType::Air);
        }

        // Update passive mobs
        let grazed_blocks = crate::passive_mob::update_passive_mobs(
            &mut self.entity_manager,
            &mut self.chunk_manager,
            &mut self.chunk_meshes,
            &self.player_physics,
            &mut self.inventory,
            self.game_mode,
            dt,
            self.total_time,
            authoritative,
        );
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

        // Sync camera position to player position at eye height. In third
        // person the camera pulls back behind the player so the model is visible.
        let eye_height = if self.keys.shift { 1.4 } else { 1.6 };
        if self.third_person {
            let forward = Vec3::new(
                self.camera.yaw.cos() * self.camera.pitch.cos(),
                self.camera.pitch.sin(),
                self.camera.yaw.sin() * self.camera.pitch.cos(),
            )
            .normalize_or_zero();
            self.camera.position =
                self.player_physics.position + Vec3::new(0.0, eye_height, 0.0) - forward * 4.0;
        } else {
            self.camera.position = self.player_physics.position + Vec3::new(0.0, eye_height, 0.0);
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
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            bytemuck::cast_slice(&[self.camera_uniform]),
        );

        // Continuous mining logic
        if self.left_mouse_pressed && self.game_mode == GameMode::Survival {
            let dir = Vec3::new(
                self.camera.yaw.cos() * self.camera.pitch.cos(),
                self.camera.pitch.sin(),
                self.camera.yaw.sin() * self.camera.pitch.cos(),
            )
            .normalize_or_zero();

            if let Some(hit) = raycast(self.camera.position, dir, 5.0, &self.chunk_manager, true) {
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
                        // Instant-break blocks such as tall grass and flowers.
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
        } else if !self.left_mouse_pressed {
            self.mining_target = None;
            self.mining_progress = 0.0;
        }
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
            let wx = player_x + self.weather.random_offset(14);
            let wz = player_z + self.weather.random_offset(14);
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
                    let speed = 26.0 + self.weather.random_unit() * 8.0;
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
                    let drift_x = (self.weather.random_unit() - 0.5) * 0.8;
                    let drift_z = (self.weather.random_unit() - 0.5) * 0.8;
                    let speed = 2.2 + self.weather.random_unit();
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
            let wx = player_x + self.weather.random_offset(24);
            let wz = player_z + self.weather.random_offset(24);
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

        if lightning_due {
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

        let player_pos = self.player_physics.position;
        let living_target = self
            .entity_manager
            .entities
            .iter()
            .filter(|entity| {
                entity.health > 0.0
                    && matches!(
                        entity.entity_type,
                        EntityType::Zombie
                            | EntityType::Skeleton
                            | EntityType::Creeper
                            | EntityType::Pig
                            | EntityType::Cow
                            | EntityType::Sheep
                            | EntityType::Chicken
                    )
                    && entity.position.distance_squared(player_pos) <= 32.0 * 32.0
            })
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
                player_pos.x.floor() as i32 + self.weather.random_offset(30),
                player_pos.z.floor() as i32 + self.weather.random_offset(30),
            )
        };
        let Some(surface_y) = self.surface_height(strike_x, strike_z) else {
            return;
        };
        let strike_pos = Vec3::new(
            strike_x as f32 + 0.5,
            surface_y as f32 + 1.0,
            strike_z as f32 + 0.5,
        );

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
        for segment in 0..12 {
            let jitter_x = (self.weather.random_unit() - 0.5) * 0.55;
            let jitter_z = (self.weather.random_unit() - 0.5) * 0.55;
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

        let fire_y = surface_y + 1;
        let support = self.chunk_manager.get_block(strike_x, surface_y, strike_z);
        if self.is_authoritative()
            && fire_y < CHUNK_HEIGHT as i32
            && support.properties().is_solid
            && !matches!(
                support,
                BlockType::Water | BlockType::Lava | BlockType::Ice | BlockType::Snow
            )
            && self.chunk_manager.get_block(strike_x, fire_y, strike_z) == BlockType::Air
        {
            self.apply_weather_block_change(strike_x, fire_y, strike_z, BlockType::Fire);
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
        for chunk_pos in dirty_chunks {
            if let Some(mesh) = self.chunk_meshes.get_mut(&chunk_pos) {
                mesh.mark_dirty();
            }
        }
        // Fan weather-driven block placement out to connected clients.
        self.broadcast_block_change(wx, wy, wz, block);
    }

    pub fn update_crack_buffers(&self, target_pos: Vec3, progress: f32) -> Option<(u32, u32)> {
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

        self.queue.write_buffer(
            &self.crack_vertex_buffer,
            0,
            bytemuck::cast_slice(&vertices),
        );
        self.queue
            .write_buffer(&self.crack_index_buffer, 0, bytemuck::cast_slice(&indices));

        Some((vertices.len() as u32, indices.len() as u32))
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

        for (dcx, dcz) in dirty_chunks {
            if let Some(mesh) = self.chunk_meshes.get_mut(&(dcx, dcz)) {
                mesh.mark_dirty();
            }
        }

        // Fan the redstone-driven block mutations out to connected clients.
        for ((x, y, z), block) in broadcast {
            self.broadcast_block_change(x, y, z, block);
        }

        for action in update.actions {
            match action {
                crate::redstone::RedstoneAction::Explode { pos } => {
                    let center =
                        Vec3::new(pos.0 as f32 + 0.5, pos.1 as f32 + 0.5, pos.2 as f32 + 0.5);
                    let removed = crate::mob::explode(
                        center,
                        4.0,
                        &mut self.chunk_manager,
                        &mut self.chunk_meshes,
                        &mut self.player_physics,
                        &mut self.player_state,
                        true,
                        self.game_mode,
                        1.0,
                    );
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
    ) {
        let block = match BlockType::from_wire(block_wire) {
            Some(b) => b,
            None => return,
        };
        if !self.remote_players.contains_key(&requester) || !self.can_place_block_at(x, y, z, block)
        {
            return;
        }
        let Some(((cx, cz), _)) = self.chunk_manager.world_to_local(x, y, z) else {
            return;
        };
        if !self.chunk_manager.chunks.contains_key(&(cx, cz)) {
            return;
        }
        let prev = self.chunk_manager.get_block(x, y, z);
        if prev == block {
            // Echo the authoritative value to correct a requesting client's
            // prediction, but do not mark an unchanged chunk as mutated.
            self.network.broadcast_block_change(x, y, z, block_wire);
            return;
        }
        let Some(mut dirty_chunks) =
            apply_synced_block_change(&mut self.chunk_manager, x, y, z, block)
        else {
            return;
        };
        self.redstone.on_block_changed(
            &self.chunk_manager,
            (x, y, z),
            crate::redstone::Direction::North,
        );
        self.check_and_break_unsupported_above(x, y, z, &mut dirty_chunks);
        for (dcx, dcz) in dirty_chunks {
            if let Some(mesh) = self.chunk_meshes.get_mut(&(dcx, dcz)) {
                mesh.mark_dirty();
            }
        }
        self.broadcast_block_change(x, y, z, block);
    }

    /// Client-side application of an authoritative block change received from
    /// the host. Mirrors the mutation half of the canonical path: `set_block`,
    /// lighting, mesh invalidation. Redstone is intentionally **not** rescanned
    /// - the host runs the redstone simulation and broadcasts its actuator
    /// effects as further `BlockChange`s, so running it here would double-apply
    /// and could diverge.
    fn apply_remote_block_change(&mut self, x: i32, y: i32, z: i32, block_wire: u32) {
        let block = match BlockType::from_wire(block_wire) {
            Some(b) => b,
            None => return,
        };
        let Some(((cx, cz), _)) = self.chunk_manager.world_to_local(x, y, z) else {
            return;
        };
        if !self.chunk_manager.chunks.contains_key(&(cx, cz)) {
            self.pending_block_changes
                .entry((cx, cz))
                .or_default()
                .insert((x, y, z), block_wire);
            return;
        }
        let Some(dirty_chunks) = apply_synced_block_change(&mut self.chunk_manager, x, y, z, block)
        else {
            return;
        };
        for (dcx, dcz) in dirty_chunks {
            if let Some(mesh) = self.chunk_meshes.get_mut(&(dcx, dcz)) {
                mesh.mark_dirty();
            }
        }
    }

    /// Client-side application of a full chunk payload sent by the host during
    /// mid-game join catch-up. The payload uses the same Zlib-compressed layout
    /// as `save.rs::ChunkSaveData`. If the chunk is not loaded yet, the payload
    /// is buffered and applied once `update_chunks` loads that coordinate.
    fn apply_remote_chunk_data(&mut self, cx: i32, cz: i32, blocks: Vec<u8>) {
        if let Some(chunk) = self.chunk_manager.chunks.get_mut(&(cx, cz)) {
            Self::restore_chunk_payload(chunk, &blocks);
            if let Some(mesh) = self.chunk_meshes.get_mut(&(cx, cz)) {
                mesh.mark_dirty();
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
                    if let Some(mesh) = self.chunk_meshes.get_mut(&(lighting_cx, lighting_cz)) {
                        mesh.mark_dirty();
                    }
                }
            }
            for coord in dirty_chunks {
                if let Some(mesh) = self.chunk_meshes.get_mut(&coord) {
                    mesh.mark_dirty();
                }
            }
        } else {
            // Chunk not streamed in yet; buffer for deferred application.
            self.pending_chunk_payloads.insert((cx, cz), blocks);
        }
    }

    /// Decode a `ChunkSaveData`-style compressed payload into an existing
    /// chunk. Reused by both the save loader and the network catch-up path so
    /// the wire format stays identical to the on-disk format.
    fn restore_chunk_payload(chunk: &mut crate::world::Chunk, blocks: &[u8]) {
        let decoded = match crate::save::decompress_bytes(blocks) {
            Ok(d) => d,
            Err(_) => return,
        };
        if decoded.len() == 16 * 256 * 16 {
            let mut idx = 0;
            for x in 0..16 {
                for y in 0..256 {
                    for z in 0..16 {
                        chunk.blocks[x][y][z] = BlockType::from_u8(decoded[idx]);
                        chunk.sky_light[x][y][z] = 0;
                        chunk.block_light[x][y][z] = 0;
                        chunk.fluid_levels[x][y][z] = 0;
                        idx += 1;
                    }
                }
            }
        }
        for x in 0..16 {
            for z in 0..16 {
                chunk.update_heightmap(x, z);
            }
        }
    }

    pub fn break_block(&mut self, pos: glam::Vec3) {
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

        // Survival drops check
        if self.game_mode == GameMode::Survival {
            let mut eligible_to_harvest = true;
            if let Some(min_material) = old_block.min_harvest_material() {
                let held_item = self.inventory.hotbar[self.inventory.selected]
                    .map(|s| s.item)
                    .unwrap_or(Item::Air);
                if let Some(tool_prop) = held_item.tool_properties() {
                    eligible_to_harvest = tool_prop.tool_type == old_block.preferred_tool()
                        && tool_prop.material >= min_material;
                } else {
                    eligible_to_harvest = false;
                }
            }

            if eligible_to_harvest {
                let held_enchantments = self.inventory.hotbar[self.inventory.selected]
                    .map(|stack| stack.enchantments)
                    .unwrap_or_default();
                let silk_touch =
                    held_enchantments.level_of(crate::enchantment::Enchantment::SilkTouch) > 0;
                let fortune =
                    held_enchantments.level_of(crate::enchantment::Enchantment::Fortune(1)) as u32;
                let is_any_leaves = old_block == BlockType::OakLeaves
                    || old_block == BlockType::BirchLeaves
                    || old_block == BlockType::SpruceLeaves;
                if silk_touch {
                    self.spawn_dropped_item(Item::from_block(old_block), sound_pos);
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
                        self.spawn_dropped_item(crate::inventory::Item::Apple, sound_pos);
                    } else {
                        self.spawn_dropped_item(
                            crate::inventory::Item::from_block(old_block),
                            sound_pos,
                        );
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
                        // 12.5% chance to drop seed
                        self.spawn_dropped_item(crate::inventory::Item::Seeds, sound_pos);
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
                        self.spawn_dropped_item(base_drop, sound_pos);
                    }
                }
            }

            if matches!(
                old_block,
                BlockType::CoalOre
                    | BlockType::IronOre
                    | BlockType::GoldOre
                    | BlockType::DiamondOre
                    | BlockType::RedstoneOre
            ) {
                let xp = if old_block == BlockType::DiamondOre {
                    5
                } else {
                    2
                };
                self.player_state.add_experience(xp);
                if old_block == BlockType::RedstoneOre && ((wx ^ wy ^ wz) & 1) == 0 {
                    self.spawn_dropped_item(Item::LapisLazuli, sound_pos);
                }
            }

            self.player_state.add_exhaustion(0.005);

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
        self.check_and_break_unsupported_above(wx, wy, wz, &mut dirty_chunks);

        for (dcx, dcz) in dirty_chunks {
            if let Some(mesh) = self.chunk_meshes.get_mut(&(dcx, dcz)) {
                mesh.mark_dirty();
            }
        }

        // Fan the authoritative break out to connected clients.
        self.broadcast_block_change(wx, wy, wz, BlockType::Air);
    }

    /// Spawn a `DroppedItem` entity in the world carrying the given `Item`.
    /// The item is launched with a small random upward velocity and given a
    /// brief pickup cooldown so it can't be instantly re-collected.
    pub fn spawn_dropped_item(&mut self, item: crate::inventory::Item, pos: glam::Vec3) {
        if item == Item::Air {
            return;
        }
        let id = self
            .entity_manager
            .spawn(crate::entity::EntityType::DroppedItem, pos);
        if let Some(entity) = self.entity_manager.entities.last_mut() {
            entity.dropped_item = Some(item);
            // Small random initial upward velocity plus a little horizontal
            // scatter so stacks don't overlap perfectly.
            let mut rng = self
                .total_time
                .to_bits()
                .wrapping_add((id.wrapping_mul(2654435761)) as u32);
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            let vx = ((rng / 65536) as f32 / 32768.0 - 0.5) * 1.5;
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            let vz = ((rng / 65536) as f32 / 32768.0 - 0.5) * 1.5;
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            let vy = 2.0 + ((rng / 65536) as f32 / 32768.0) * 1.0;
            entity.velocity = glam::Vec3::new(vx, vy, vz);
            entity.pickup_cooldown = 0.5;
        }
    }

    fn update_player_projectiles(&mut self, dt: f32) {
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
            for entity in &mut self.entity_manager.entities {
                if entity.entity_type == crate::entity::EntityType::RemotePlayer
                    || entity.position.distance(position) > 4.0
                {
                    continue;
                }
                match potion.kind {
                    crate::brewing::PotionKind::Healing
                    | crate::brewing::PotionKind::Regeneration => {
                        entity.health =
                            (entity.health + 4.0 * potion.level as f32).min(entity.max_health);
                    }
                    crate::brewing::PotionKind::Poison => {
                        entity.health -= 2.0 * potion.level as f32
                    }
                    crate::brewing::PotionKind::Slowness => entity.velocity *= 0.4,
                    _ => {}
                }
            }
        }

        let mut hits = Vec::new();
        for projectile in &self.entity_manager.entities {
            if projectile.entity_type != crate::entity::EntityType::Arrow
                || !projectile.friendly_projectile
            {
                continue;
            }
            for target in &self.entity_manager.entities {
                if target.id != projectile.id
                    && !matches!(
                        target.entity_type,
                        crate::entity::EntityType::Arrow
                            | crate::entity::EntityType::SplashPotion
                            | crate::entity::EntityType::DroppedItem
                            | crate::entity::EntityType::HeartParticle
                            | crate::entity::EntityType::RemotePlayer
                    )
                    && projectile.get_aabb().intersects(&target.get_aabb())
                {
                    hits.push((projectile.id, target.id, projectile.projectile_damage));
                    break;
                }
            }
        }
        for (projectile_id, target_id, damage) in hits {
            if let Some(target) = self
                .entity_manager
                .entities
                .iter_mut()
                .find(|entity| entity.id == target_id)
            {
                target.health -= damage;
            }
            if let Some(projectile) = self
                .entity_manager
                .entities
                .iter_mut()
                .find(|entity| entity.id == projectile_id)
            {
                projectile.health = -1.0;
            }
        }
        self.entity_manager.entities.retain(|entity| {
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
        if self.game_mode == GameMode::Creative {
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

                // Release cursor grab immediately on death so player can click Respawn
                let _ = self
                    .window
                    .set_cursor_grab(winit::window::CursorGrabMode::None);
                self.window.set_cursor_visible(true);
                self.clear_movement_input();
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

        // Reset player state
        self.player_state.health = self.player_state.max_health;
        self.player_state.hunger = 20.0;
        self.player_state.saturation = 5.0;
        self.player_state.exhaustion = 0.0;
        self.player_state.is_dead = false;
        self.player_state.death_reason = None;
        self.player_state.invulnerable_time = 1.0; // Give 1.0s invulnerability on respawn
        self.player_state.damaged_flash_time = 0.0;
        self.void_damage_timer = 0.0;

        // Grab cursor
        let _ = self
            .window
            .set_cursor_grab(winit::window::CursorGrabMode::Locked)
            .or_else(|_| {
                self.window
                    .set_cursor_grab(winit::window::CursorGrabMode::Confined)
            });
        self.window.set_cursor_visible(false);

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

    pub fn handle_click(&mut self, is_left_click: bool) {
        if !self.is_authoritative() {
            let direction = Vec3::new(
                self.camera.yaw.cos() * self.camera.pitch.cos(),
                self.camera.pitch.sin(),
                self.camera.yaw.sin() * self.camera.pitch.cos(),
            )
            .normalize_or_zero();
            if let Some(hit) = raycast(
                self.camera.position,
                direction,
                5.0,
                &self.chunk_manager,
                is_left_click,
            ) {
                if is_left_click {
                    self.network.request_block_change(
                        hit.block_pos.x as i32,
                        hit.block_pos.y as i32,
                        hit.block_pos.z as i32,
                        BlockType::Air as u32,
                    );
                    self.network
                        .send_action(crate::network::protocol::Action::Break);
                } else if let Some(block) = self.inventory.get_selected_block() {
                    let target = hit.block_pos + hit.normal;
                    let (x, y, z) = (target.x as i32, target.y as i32, target.z as i32);
                    if !self.can_place_block_at(x, y, z, block) {
                        return;
                    }
                    self.network.request_block_change(x, y, z, block as u32);
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
                    if let Some(projectile) = self
                        .entity_manager
                        .entities
                        .iter_mut()
                        .find(|entity| entity.id == id)
                    {
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
                    if let Some(arrow) = self
                        .entity_manager
                        .entities
                        .iter_mut()
                        .find(|entity| entity.id == id)
                    {
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

        // 1. Raycast against entities first for left-clicks
        if is_left_click {
            let mut closest_entity: Option<(u64, f32)> = None;
            for entity in &self.entity_manager.entities {
                if matches!(
                    entity.entity_type,
                    crate::entity::EntityType::Arrow | crate::entity::EntityType::RemotePlayer
                ) {
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
                if let Some(entity) = self
                    .entity_manager
                    .entities
                    .iter_mut()
                    .find(|e| e.id == entity_id)
                {
                    if entity.invulnerable_time <= 0.0 {
                        let held_stack = self.inventory.hotbar[self.inventory.selected];
                        let held_item = held_stack
                            .map(|s| s.item)
                            .unwrap_or(crate::inventory::Item::Air);
                        let enchantments = held_stack.map(|s| s.enchantments).unwrap_or_default();
                        let damage = held_item.tool_properties().map(|t| t.damage).unwrap_or(1.0)
                            + crate::enchantment::attack_damage_bonus(&enchantments)
                            + self.potion_effects.strength_bonus();
                        let knockback = 8.0
                            + enchantments.level_of(crate::enchantment::Enchantment::Knockback(1))
                                as f32
                                * 3.0;

                        entity.health -= damage;
                        entity.invulnerable_time = 0.4;
                        entity.velocity += dir * knockback + Vec3::new(0.0, 3.0, 0.0);
                        let fire_level =
                            enchantments.level_of(crate::enchantment::Enchantment::FireAspect(1));
                        if fire_level > 0 {
                            entity.fire_aspect_timer =
                                entity.fire_aspect_timer.max(fire_level as f32 * 4.0);
                        }

                        println!(
                            "[Debug] Hit {:?}, health={:.1}",
                            entity.entity_type, entity.health
                        );

                        if entity.health <= 0.0 {
                            println!("[Debug] Killed {:?}", entity.entity_type);
                            if self.game_mode == GameMode::Survival {
                                let looting = enchantments
                                    .level_of(crate::enchantment::Enchantment::Looting(1));
                                for _ in 0..=(looting / 2) {
                                    match entity.entity_type {
                                        crate::entity::EntityType::Zombie => {
                                            self.inventory
                                                .add_item(crate::inventory::Item::RottenFlesh);
                                        }
                                        crate::entity::EntityType::Skeleton => {
                                            self.inventory.add_item(crate::inventory::Item::Bone);
                                            self.inventory.add_item(crate::inventory::Item::Arrow);
                                            let mut rng_seed = (entity.position.x as u32)
                                                .wrapping_mul(31)
                                                .wrapping_add(entity.position.z as u32);
                                            let mut next_rand = || {
                                                rng_seed = rng_seed
                                                    .wrapping_mul(1103515245)
                                                    .wrapping_add(12345);
                                                (rng_seed / 65536) % 32768
                                            };
                                            if next_rand() % 10 == 0 {
                                                self.inventory
                                                    .add_item(crate::inventory::Item::Bow);
                                            }
                                        }
                                        crate::entity::EntityType::Creeper => {
                                            self.inventory
                                                .add_item(crate::inventory::Item::Gunpowder);
                                        }
                                        crate::entity::EntityType::Pig => {
                                            let is_on_fire = entity.burn_timer > 0.0
                                                || entity.fire_aspect_timer > 0.0;
                                            let drop = if is_on_fire {
                                                crate::inventory::Item::CookedPorkchop
                                            } else {
                                                crate::inventory::Item::RawPorkchop
                                            };
                                            self.inventory.add_item(drop);
                                        }
                                        crate::entity::EntityType::Cow => {
                                            self.inventory
                                                .add_item(crate::inventory::Item::RawBeef);
                                            let rng = (entity.position.x as u32).wrapping_mul(31);
                                            if rng % 2 == 0 {
                                                self.inventory
                                                    .add_item(crate::inventory::Item::Leather);
                                            }
                                        }
                                        crate::entity::EntityType::Sheep => {
                                            self.inventory
                                                .add_item(crate::inventory::Item::RawMutton);
                                            if entity.has_wool {
                                                self.inventory
                                                    .add_item(crate::inventory::Item::Wool);
                                            }
                                        }
                                        crate::entity::EntityType::Chicken => {
                                            self.inventory
                                                .add_item(crate::inventory::Item::RawChicken);
                                            self.inventory
                                                .add_item(crate::inventory::Item::Feather);
                                        }
                                        _ => {}
                                    }
                                }
                                self.player_state.add_experience(match entity.entity_type {
                                    crate::entity::EntityType::Zombie
                                    | crate::entity::EntityType::Skeleton
                                    | crate::entity::EntityType::Creeper => 5,
                                    _ => 2,
                                });
                            }
                        }

                        self.damage_selected_tool(entity_id as u32 ^ self.total_time.to_bits());

                        return;
                    }
                }
            }
        }

        if !is_left_click {
            let mut closest_entity: Option<(u64, f32)> = None;
            for entity in &self.entity_manager.entities {
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
                if let Some(entity) = self
                    .entity_manager
                    .entities
                    .iter_mut()
                    .find(|e| e.id == entity_id)
                {
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
                                entity.has_wool = false;
                                self.inventory.add_item(crate::inventory::Item::Wool);
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

        if let Some(hit) = raycast(
            self.camera.position,
            dir,
            5.0,
            &self.chunk_manager,
            is_left_click,
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
                    self.inventory
                        .use_selected_item(self.game_mode == GameMode::Creative);
                    let mut water_bottle = ItemStack::new(Item::Potion, 1);
                    water_bottle.potion = Some(crate::brewing::PotionData::water());
                    self.inventory.add_stack(water_bottle);
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
                        self.inventory
                            .add_item(crate::inventory::Item::from_block(old_block));

                        if old_block == BlockType::Grass {
                            let rng = (wx as u32).wrapping_mul(31).wrapping_add(wz as u32);
                            if rng % 20 == 0 {
                                let drop = match rng % 3 {
                                    0 => crate::inventory::Item::Seeds,
                                    1 => crate::inventory::Item::Wheat,
                                    _ => crate::inventory::Item::Carrot,
                                };
                                self.inventory.add_item(drop);
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
                    let below_block = self.chunk_manager.get_block(wx, wy - 1, wz);
                    if !placed_block.can_stay_on(below_block) {
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

            for (dcx, dcz) in dirty_chunks {
                if let Some(mesh) = self.chunk_meshes.get_mut(&(dcx, dcz)) {
                    mesh.mark_dirty();
                }
            }

            // Fan the authoritative player-driven mutation out to clients.
            if let Some(block) = result_block {
                self.broadcast_block_change(wx, wy, wz, block);
            }
        }
    }

    pub fn get_inventory_slots(&self) -> Vec<(SlotType, f32, f32, f32, f32)> {
        let aspect = self.size.width as f32 / self.size.height as f32;
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
                            if dragged.item == output.item
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
                                if slot.item == dragged.item {
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
                                if slot.item == dragged.item && slot.count < max_stack {
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
                                } else if slot.item != dragged.item {
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
        // Release cursor grab
        let _ = self
            .window
            .set_cursor_grab(winit::window::CursorGrabMode::None);
        self.window.set_cursor_visible(true);
        self.clear_movement_input();
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

    pub fn close_inventory(&mut self) {
        self.inventory.is_open = false;
        // Return craft input items
        let inputs: Vec<ItemStack> = self
            .inventory
            .craft_input
            .iter_mut()
            .filter_map(|slot| slot.take())
            .collect();
        for stack in inputs {
            for _ in 0..stack.count {
                self.inventory.add_item(stack.item);
            }
        }
        let station_items: Vec<ItemStack> = match self.active_station {
            Some(StationKind::Enchanting) => {
                [self.enchanting.input.take(), self.enchanting.lapis.take()]
                    .into_iter()
                    .flatten()
                    .collect()
            }
            Some(StationKind::Brewing) => self
                .brewing
                .bottles
                .iter_mut()
                .map(Option::take)
                .chain(std::iter::once(self.brewing.ingredient.take()))
                .flatten()
                .collect(),
            Some(StationKind::Anvil) => [self.anvil.left.take(), self.anvil.right.take()]
                .into_iter()
                .flatten()
                .collect(),
            None => Vec::new(),
        };
        for stack in station_items {
            self.inventory.add_stack(stack);
        }

        // Also return dragged item if any
        if let Some(dragged) = self.inventory.dragged.take() {
            self.inventory.add_stack(dragged);
        }

        self.inventory.is_table_open = false;
        self.inventory.craft_input = vec![None; 4];
        self.inventory.craft_output = None;
        self.active_station = None;
        self.anvil.rename.clear();

        // Re-lock cursor
        let _ = self
            .window
            .set_cursor_grab(winit::window::CursorGrabMode::Locked)
            .or_else(|_| {
                self.window
                    .set_cursor_grab(winit::window::CursorGrabMode::Confined)
            });
        self.window.set_cursor_visible(false);
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
        let chunk_volume = CHUNK_WIDTH * CHUNK_HEIGHT * CHUNK_DEPTH;
        let chunk_heap_bytes = chunk_volume
            * (std::mem::size_of::<BlockType>() + 3 * std::mem::size_of::<u8>())
            + CHUNK_WIDTH * CHUNK_DEPTH * std::mem::size_of::<u16>();
        let chunks_bytes = self
            .chunk_manager
            .chunks
            .len()
            .saturating_mul(std::mem::size_of::<Chunk>() + chunk_heap_bytes);

        let mesh_indices: usize = self
            .chunk_meshes
            .values()
            .map(ChunkMesh::total_indices)
            .sum();
        let mesh_vertices = mesh_indices.saturating_mul(2) / 3;
        let mesh_bytes = mesh_vertices
            .saturating_mul(std::mem::size_of::<TerrainVertex>())
            .saturating_add(mesh_indices.saturating_mul(std::mem::size_of::<u32>()));

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

    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let view_projection = Mat4::from_cols_array_2d(&self.camera_uniform.view_proj);
        let frustum = Frustum::from_view_projection(view_projection);
        let render_blocks = self.chunk_manager.render_distance as f32 * CHUNK_WIDTH as f32;
        let lod_thresholds = LodThresholds::new(render_blocks * 0.5, render_blocks * 0.75);
        let mut selected_lods = std::collections::HashMap::new();
        let mut candidates = Vec::new();
        for (&coord, mesh) in &self.chunk_meshes {
            let Some(bounds) = mesh.finest_bounds() else {
                continue;
            };
            let lod = select_lod_for_bounds(self.camera.position, bounds, lod_thresholds);
            let Some(level) = mesh.level(lod) else {
                continue;
            };
            selected_lods.insert(coord, lod);
            if let Some(bounds) = level.opaque.bounds {
                candidates.push(DrawCandidate::new(
                    coord,
                    bounds,
                    level.opaque.num_indices,
                    DrawLayer::Opaque,
                ));
            }
            if let Some(bounds) = level.transparent.bounds {
                candidates.push(DrawCandidate::new(
                    coord,
                    bounds,
                    level.transparent.num_indices,
                    DrawLayer::Transparent,
                ));
            }
        }
        let draw_plan = build_draw_plan(candidates, &frustum, self.camera.position);
        self.submitted_terrain_triangles = draw_plan.submitted_triangle_count();
        self.submitted_terrain_draw_calls = draw_plan.draw_call_count();
        self.visible_chunk_count = draw_plan.visible_chunk_count();

        // Compile mob meshes
        let mut mob_vertices = Vec::new();
        let mut mob_indices = Vec::new();
        crate::mob_renderer::render_mobs(
            &self.entity_manager,
            &self.chunk_manager,
            &mut mob_vertices,
            &mut mob_indices,
            self.total_time,
        );

        // In third-person mode also render the local player avatar.
        if self.third_person {
            crate::mob_renderer::render_local_player(
                self.player_physics.position,
                self.camera.yaw,
                self.camera.pitch,
                &self.chunk_manager,
                &mut mob_vertices,
                &mut mob_indices,
                self.total_time,
                self.player_physics.velocity,
            );
        }

        let mob_indices_len = mob_indices.len();
        self.mob_num_indices = mob_indices_len as u32;

        if mob_indices_len > 0 {
            let vert_limit = mob_vertices.len().min(8192);
            let ind_limit = mob_indices_len.min(12288);
            self.mob_num_indices = ind_limit as u32;
            self.queue.write_buffer(
                &self.mob_vertex_buffer,
                0,
                bytemuck::cast_slice(&mob_vertices[..vert_limit]),
            );
            self.queue.write_buffer(
                &self.mob_index_buffer,
                0,
                bytemuck::cast_slice(&mob_indices[..ind_limit]),
            );
        }

        // Compile billboard particle quads into the dynamic particle buffers.
        // Camera right/up vectors are derived from yaw/pitch so billboards face
        // the viewer.
        let cam_right =
            glam::Vec3::new(-self.camera.yaw.sin(), 0.0, self.camera.yaw.cos()).normalize_or_zero();
        let cam_up = glam::Vec3::new(
            -self.camera.yaw.cos() * self.camera.pitch.sin(),
            self.camera.pitch.cos(),
            -self.camera.yaw.sin() * self.camera.pitch.sin(),
        )
        .normalize_or_zero();
        self.particle_num_indices = self
            .particles
            .compile_mesh(
                &self.device,
                &self.queue,
                cam_right,
                cam_up,
                &self.particle_vertex_buffer,
                &self.particle_index_buffer,
            )
            .unwrap_or(0);

        // Compile first-person hand mesh in view space. Hidden in third-person.
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
            let (hand_vertices, hand_indices) = crate::hand_renderer::build_first_person_hand_mesh(
                &self.inventory,
                walk_swing,
                attack_swing,
            );
            let hand_indices_len = hand_indices.len();
            self.hand_num_indices = hand_indices_len as u32;
            if hand_indices_len > 0 {
                let vert_limit = hand_vertices.len().min(1024);
                let ind_limit = hand_indices_len.min(1536);
                self.hand_num_indices = ind_limit as u32;
                self.queue.write_buffer(
                    &self.hand_vertex_buffer,
                    0,
                    bytemuck::cast_slice(&hand_vertices[..vert_limit]),
                );
                self.queue.write_buffer(
                    &self.hand_index_buffer,
                    0,
                    bytemuck::cast_slice(&hand_indices[..ind_limit]),
                );
            }
        } else {
            self.hand_num_indices = 0;
        }

        if self.is_saving {
            let mut ui_vertices = Vec::new();
            let mut ui_line_vertices = Vec::new();

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
                "SAVING WORLD...",
                0.0,
                0.03,
                0.06,
                0.012,
                [1.0, 1.0, 1.0, 1.0],
                &mut ui_line_vertices,
            );

            let ui_vert_len = ui_vertices.len().min(4096);
            let ui_line_vert_len = ui_line_vertices.len().min(4096);

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

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
            self.num_ui_textured_vertices = 0;
        } else if self.connection_lost {
            let mut ui_vertices = Vec::new();
            let mut ui_line_vertices = Vec::new();
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
            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
            self.num_ui_textured_vertices = 0;
        } else if self.player_state.is_dead {
            let mut ui_vertices = Vec::new();
            let mut ui_line_vertices = Vec::new();

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

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
            self.num_ui_textured_vertices = 0;
        } else if self.is_paused {
            let mut ui_vertices = Vec::new();
            let mut ui_line_vertices = Vec::new();

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
            let quit_hover =
                mouse_x >= -0.3 && mouse_x <= 0.3 && mouse_y >= -0.46 && mouse_y <= -0.36;

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
                quit_hover,
                -0.46,
                -0.36,
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
            let fov_text = format!("FOV < {:.0} >", self.camera.fov);
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

            // "VOLUME < value >"
            let vol_text = format!("VOLUME < {:.0}% >", self.audio_manager.volume * 100.0);
            draw_centered_text(
                &vol_text,
                -0.28,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // "SAVE AND QUIT"
            draw_centered_text(
                "SAVE AND QUIT",
                -0.42,
                0.02,
                0.04,
                0.008,
                text_color,
                &mut ui_line_vertices,
            );

            // Cap the sizes to the preallocated buffers (4096 vertices)
            let ui_vert_len = ui_vertices.len().min(4096);
            let ui_line_vert_len = ui_line_vertices.len().min(4096);

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

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
        } else {
            let mut ui_vertices = Vec::new();
            let mut ui_line_vertices = Vec::new();
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
                if self.active_station.is_none() {
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
                    let (col, row) = dragged.item.properties().tex_coords;
                    let u0 = col as f32 * 0.0625;
                    let u1 = (col + 1) as f32 * 0.0625;
                    let v0 = row as f32 * 0.0625;
                    let v1 = (row + 1) as f32 * 0.0625;

                    let dx0 = mouse_x - slot_w / 2.0 + 0.015;
                    let dx1 = mouse_x + slot_w / 2.0 - 0.015;
                    let dy0 = mouse_y - slot_h / 2.0 + 0.015 * aspect;
                    let dy1 = mouse_y + slot_h / 2.0 - 0.015 * aspect;

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
                        let count_x = mouse_x + slot_w / 2.0 - count_w - 0.008;
                        let count_y = mouse_y - slot_h / 2.0 + 0.01 * aspect;
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
                    let frame_str = format!(
                        "FPS: {:.1} / FRAME: {:.2} MS",
                        self.debug_fps, self.debug_frame_ms
                    );

                    let time_of_day = self.world_time.time_of_day_smooth();
                    let hour = ((time_of_day * 24.0 + 6.0) % 24.0).floor() as u32;
                    let minute = (((time_of_day * 24.0 + 6.0) % 1.0) * 60.0).floor() as u32;
                    let day = self.world_time.ticks / self.world_time.day_length;
                    let time_str = format!(
                        "TIME: {:02}:{:02} / DAY: {} / TICKS: {}",
                        hour, minute, day, self.world_time.ticks
                    );

                    let pos = self.player_physics.position;
                    let pos_str = format!("XYZ: {:.3} / {:.3} / {:.3}", pos.x, pos.y, pos.z);
                    let facing_str = format!(
                        "FACING: YAW {:.2} / PITCH {:.2}",
                        self.camera.yaw.to_degrees().rem_euclid(360.0),
                        self.camera.pitch.to_degrees()
                    );
                    let chunk_x = debug_chunk_coordinate(pos.x, CHUNK_WIDTH);
                    let chunk_z = debug_chunk_coordinate(pos.z, CHUNK_DEPTH);
                    let chunk_str = format!("CHUNK: {} / {}", chunk_x, chunk_z);

                    let biome = self
                        .weather
                        .biome_at(pos.x.floor() as i32, pos.z.floor() as i32);
                    let biome_str = format!("BIOME: {}", biome_debug_name(biome));
                    let weather_str = format!("WEATHER: {:?}", self.weather.current).to_uppercase();
                    let chunks_str = format!(
                        "CHUNKS: {} VISIBLE / {} LOADED / {} DRAWS",
                        self.visible_chunk_count,
                        self.chunk_manager.chunks.len(),
                        self.submitted_terrain_draw_calls
                    );
                    let entities_str = format!(
                        "ENTITIES: {} / PARTICLES: {}",
                        self.entity_manager.entities.len(),
                        self.particles.particles.len()
                    );

                    let terrain_indices = self.submitted_terrain_triangles.saturating_mul(3);
                    let rendered_indices = terrain_indices
                        + u64::from(self.mob_num_indices)
                        + u64::from(self.particle_num_indices);
                    let rendered_triangles = rendered_indices / 3;
                    let rendered_vertices = rendered_indices * 2 / 3;
                    let render_str = format!(
                        "RENDER: {} VERTICES / {} TRIANGLES",
                        rendered_vertices, rendered_triangles
                    );
                    let memory_str = format!(
                        "MEMORY EST: {:.1} MB",
                        self.estimated_debug_memory_bytes() as f64 / (1024.0 * 1024.0)
                    );

                    let net_str = match &self.role {
                        MultiplayerRole::Host { port } => {
                            format!(
                                "NET: HOST ON PORT {} | CLIENTS: {}",
                                port,
                                self.remote_players.len()
                            )
                        }
                        MultiplayerRole::Client {
                            server_addr, port, ..
                        } => {
                            format!(
                                "NET: CLIENT @ {}:{} | LOCAL ID: {} | PLAYERS: {}",
                                server_addr,
                                port,
                                self.local_player_id
                                    .map(|id| id.to_string())
                                    .unwrap_or_else(|| "?".to_string()),
                                self.remote_players.len() + 1
                            )
                        }
                        MultiplayerRole::Singleplayer => "NET: SINGLEPLAYER".to_string(),
                    };

                    let char_w = 0.007;
                    let char_h = 0.014;
                    let spacing = 0.002;

                    let start_x = -0.98;
                    let start_y = 0.95;
                    let line_gap = 0.025;

                    let debug_lines = [
                        frame_str,
                        pos_str,
                        facing_str,
                        chunk_str,
                        biome_str,
                        weather_str,
                        chunks_str,
                        entities_str,
                        render_str,
                        memory_str,
                        time_str,
                        net_str,
                    ];
                    for (line_index, line) in debug_lines.iter().enumerate() {
                        add_string_lines(
                            line,
                            start_x,
                            start_y - line_gap * line_index as f32,
                            char_w,
                            char_h,
                            spacing,
                            [1.0, 1.0, 1.0, 1.0],
                            &mut ui_line_vertices,
                        );
                    }
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
                let Some(entity) = self
                    .entity_manager
                    .entities
                    .iter()
                    .find(|entity| entity.id == remote.entity_id)
                else {
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

            self.num_ui_vertices = ui_vert_len as u32;
            self.num_ui_line_vertices = ui_line_vert_len as u32;
            self.num_ui_textured_vertices = ui_textured_vert_len as u32;
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

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

            // Draw Skybox first
            render_pass.set_pipeline(&self.sky_pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            render_pass.draw(0..6, 0..1);

            // Pass 1: Opaque & Cutout
            render_pass.set_pipeline(&self.terrain_render_pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            for candidate in &draw_plan.opaque {
                let Some(lod) = selected_lods.get(&candidate.chunk_coord).copied() else {
                    continue;
                };
                let Some(layer) = self
                    .chunk_meshes
                    .get(&candidate.chunk_coord)
                    .and_then(|mesh| mesh.level(lod))
                    .map(|level| &level.opaque)
                else {
                    continue;
                };
                render_pass.set_vertex_buffer(0, layer.vertex_buffer.slice(..));
                render_pass
                    .set_index_buffer(layer.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..layer.num_indices, 0, 0..1);
            }

            // Draw Mobs
            if self.mob_num_indices > 0 {
                render_pass.set_pipeline(&self.render_pipeline);
                render_pass.set_vertex_buffer(0, self.mob_vertex_buffer.slice(..));
                render_pass
                    .set_index_buffer(self.mob_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..self.mob_num_indices, 0, 0..1);
            }

            // Pass 2: Translucent (Water/Ice)
            render_pass.set_pipeline(&self.terrain_trans_pipeline);
            for candidate in &draw_plan.transparent {
                let Some(lod) = selected_lods.get(&candidate.chunk_coord).copied() else {
                    continue;
                };
                let Some(layer) = self
                    .chunk_meshes
                    .get(&candidate.chunk_coord)
                    .and_then(|mesh| mesh.level(lod))
                    .map(|level| &level.transparent)
                else {
                    continue;
                };
                render_pass.set_vertex_buffer(0, layer.vertex_buffer.slice(..));
                render_pass
                    .set_index_buffer(layer.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..layer.num_indices, 0, 0..1);
            }

            // Draw billboard particles using the translucent (alpha-blend) pipeline.
            if self.particle_num_indices > 0 {
                render_pass.set_pipeline(&self.trans_pipeline);
                render_pass.set_vertex_buffer(0, self.particle_vertex_buffer.slice(..));
                render_pass.set_index_buffer(
                    self.particle_index_buffer.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                render_pass.draw_indexed(0..self.particle_num_indices, 0, 0..1);
            }

            // Draw Block cracking animation overlay (multiply blend)
            if let Some(target) = self.mining_target {
                if self.mining_progress > 0.0 {
                    if let Some((_num_vertices, num_indices)) =
                        self.update_crack_buffers(target, self.mining_progress)
                    {
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

            if !self.is_paused {
                // 1. Draw Textured UI (block thumbnails)
                if self.num_ui_textured_vertices > 0 {
                    render_pass.set_pipeline(&self.ui_textured_pipeline);
                    render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                    render_pass.set_vertex_buffer(0, self.ui_textured_vertex_buffer.slice(..));
                    render_pass.draw(0..self.num_ui_textured_vertices, 0..1);
                }

                // 2. Draw Colored UI (hotbar background)
                if self.num_ui_vertices > 0 {
                    render_pass.set_pipeline(&self.ui_pipeline);
                    render_pass.set_vertex_buffer(0, self.ui_vertex_buffer.slice(..));
                    render_pass.draw(0..self.num_ui_vertices, 0..1);
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
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
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

    match c {
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
        add_char_lines(c, current_x, y, char_w, char_h, color, vertices);
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
mod debug_tests {
    use super::*;

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
        for character in ['B', 'K', 'W', 'X', 'Z', '/', '_'] {
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
                block
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
                x: 3,
                y: 80,
                z: -4,
                block: BlockType::Stone.to_wire(),
            })
            .unwrap();

        assert!(matches!(
            handle.drain_inbound().as_slice(),
            [NetworkInbound::AuthoritativeBlockChange {
                x: 3,
                y: 80,
                z: -4,
                block
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
}
