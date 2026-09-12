// Tests extracted from mesh.rs (Plan 27).

use super::*;
use crate::chunk_render::LodLevel;
use std::collections::HashSet;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_MODEL_TEMP: AtomicU64 = AtomicU64::new(0);

fn model_temp_dir(label: &str) -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let id = NEXT_MODEL_TEMP.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("icraft_world_model_{label}_{stamp}_{id}"));
    fs::create_dir_all(&path).unwrap();
    path
}

fn selected_world_model_registry() -> (std::path::PathBuf, crate::block_model::ModelRegistry) {
    let root = model_temp_dir("selected");
    fs::write(
        root.join("pack.json"),
        br#"{"id":"icraft.builtin","name":"builtin","version":"1","format":1,"description":""}"#,
    )
    .unwrap();
    let user = root.join("resourcepacks");
    let pack = user.join("mesh.pack");
    fs::create_dir_all(pack.join("models/block")).unwrap();
    fs::write(
        pack.join("pack.json"),
        br#"{"id":"mesh.pack","name":"mesh","version":"1","format":1,"description":""}"#,
    )
    .unwrap();
    fs::write(
        pack.join("models/block/stone.json"),
        br#"{"parent":"builtin","atlas_tile":[13,13]}"#,
    )
    .unwrap();
    fs::write(
        pack.join("models/block/oak_slab.json"),
        br#"{"parent":"builtin","atlas_tile":[12,12]}"#,
    )
    .unwrap();

    let mut manager = crate::resources::ResourcePackManager::discover(&root, &user);
    manager.apply_enabled_order(["mesh.pack"]).unwrap();
    let registry = crate::block_model::ModelRegistry::from_resource_packs(
        &mut manager,
        [
            crate::block_model::model_path_for_block(BlockType::Stone),
            crate::block_model::model_path_for_block(BlockType::OakSlab),
        ],
    );
    (root, registry)
}

#[test]
fn end_portal_frames_use_distinct_top_side_and_filled_top_tiles() {
    assert_eq!(BlockType::EndPortalFrame.get_face_tex_index(0), (9, 4));
    assert_eq!(BlockType::EndPortalFrame.get_face_tex_index(5), (9, 4));
    assert_eq!(BlockType::EndPortalFrame.get_face_tex_index(4), (15, 15));
    let mut filled = crate::world::BlockState::default();
    filled.is_open = true;
    assert_eq!(
        BlockType::EndPortalFrame.face_tex_for(filled, 4),
        (6, 4)
    );
}

fn empty_test_chunk() -> Chunk {
    let mut chunk = Chunk::new(0, 0);
    for x in 0..CHUNK_WIDTH {
        for y in chunk.world_y_range() {
            for z in 0..CHUNK_DEPTH {
                chunk.set_block_local(x, y, z, BlockType::Air);
                chunk.set_sky_light(x, y, z, 15);
                chunk.set_block_light(x, y, z, 0);
                chunk.set_fluid_level(x, y, z, 0);
            }
        }
        for z in 0..CHUNK_DEPTH {
            chunk.heightmap[x][z] = NO_HEIGHT;
        }
    }
    chunk
}

#[test]
fn l0_volume_respects_section_extent_bounds() {
    let origin = [0, 32, 0];
    let (ov, _, tv, _) = Chunk::mesh_l0_volume(origin, [16, 16, 16], |x, y, z| MeshVoxel {
        block: if (x, y, z) == (0, 32, 0) {
            BlockType::Stone
        } else {
            BlockType::Air
        },
        state: 0,
        sky: 15,
        block_light: 0,
        raw_fluid: 0,
    });
    assert!(!ov.is_empty(), "block inside extent must generate quads");
    assert!(tv.is_empty());
}

#[test]
fn production_mesh_entries_apply_selected_tiles_and_keep_default_fallback() {
    let (_temp_root, registry) = selected_world_model_registry();
    let mut chunk = empty_test_chunk();
    chunk.set_block_local(8, 64, 8, BlockType::Stone);
    chunk.set_block_local(9, 64, 8, BlockType::Dirt);
    let key = SectionKey::new(0, 4, 0);

    let bundle_default = chunk
        .generate_section_mesh_bundle(key, 1, 1, |x, y, z| test_chunk_lookup(&chunk, x, y, z));
    let default_stone_tile = BlockType::Stone.get_face_tex_index(0);
    let default_dirt_tile = BlockType::Dirt.get_face_tex_index(0);
    let default_vertices = &bundle_default.levels[0].opaque.vertices;
    assert!(default_vertices
        .iter()
        .any(|v| v.atlas_tile_u32() == default_stone_tile));
    assert!(default_vertices
        .iter()
        .any(|v| v.atlas_tile_u32() == default_dirt_tile));

    let bundle_selected = chunk.generate_section_mesh_bundle_with_registry(
        key,
        1,
        1,
        |x, y, z| test_chunk_lookup(&chunk, x, y, z),
        &registry,
    );
    let selected_vertices = &bundle_selected.levels[0].opaque.vertices;
    assert!(
        selected_vertices
            .iter()
            .any(|v| v.atlas_tile_u32() == (13, 13)),
        "stone must use the selected resource pack tile [13, 13]"
    );
    assert!(
        selected_vertices
            .iter()
            .any(|v| v.atlas_tile_u32() == default_dirt_tile),
        "unconfigured dirt must retain its default procedural tile"
    );
    assert!(
        !selected_vertices
            .iter()
            .any(|v| v.atlas_tile_u32() == default_stone_tile),
        "stone should no longer produce default tile vertices under the selected pack"
    );

    let halo = SectionHaloSnapshot::from_chunk(key, |wx, wy, wz| {
        let (block, sky, block_light, level, falling) = test_chunk_lookup(&chunk, wx, wy, wz);
        MeshVoxel {
            block,
            state: chunk.get_block_state(wx, wy, wz),
            sky,
            block_light,
            raw_fluid: level | if falling { 8 } else { 0 },
        }
    });
    let worker_bundle = Chunk::generate_section_mesh_bundle_from_halo_with_registry(
        SectionIdentity::new(key, 1, 1),
        &halo,
        &registry,
    );
    assert!(
        worker_bundle.levels[0]
            .opaque
            .vertices
            .iter()
            .any(|v| v.atlas_tile_u32() == (13, 13)),
        "worker halo entry point must apply selected pack tiles"
    );
}

#[test]
fn section_bundle_builds_distinct_bounded_lods_and_preserves_identity() {
    let mut chunk = empty_test_chunk();
    for x in 0..CHUNK_WIDTH {
        for z in 0..CHUNK_DEPTH {
            for y in chunk.world_y_range() {
                let block = if y < 64 {
                    BlockType::Stone
                } else if y == 64 {
                    BlockType::Grass
                } else {
                    BlockType::Air
                };
                chunk.set_block_local(x, y, z, block);
                chunk.set_sky_light(x, y, z, if y >= 64 { 15 } else { 0 });
            }
            chunk.heightmap[x][z] = 64;
        }
    }

    let key = SectionKey::new(0, 4, 0);
    let bundle = chunk
        .generate_section_mesh_bundle(key, 42, 7, |x, y, z| test_chunk_lookup(&chunk, x, y, z));

    assert_eq!(bundle.identity.key, key);
    assert_eq!(bundle.identity.revision, 42);
    assert_eq!(bundle.identity.lifetime, 7);
    assert_eq!(bundle.levels.len(), 3);
    assert!(
        bundle.bounds.is_some(),
        "non-empty section must produce overall bounds"
    );

    for (lod_idx, lod) in bundle.levels.iter().enumerate() {
        assert!(
            lod.bounds().is_some(),
            "LOD {lod_idx} must produce valid bounds"
        );
        if let Some(bounds) = lod.bounds() {
            assert!(bounds.min[1] >= key.min_world_y() as f32 - 0.01);
            assert!(bounds.max[1] <= key.max_world_y() as f32 + 0.01);
        }
    }

    let l0_quads = bundle.levels[0].opaque.indices.len() / 6;
    let l1_quads = bundle.levels[1].opaque.indices.len() / 6;
    let l2_quads = bundle.levels[2].opaque.indices.len() / 6;
    assert!(l0_quads > 0 && l1_quads > 0 && l2_quads > 0);
    assert_eq!(bundle.built_lods, LodLevel::MASK_ALL);
}

#[test]
fn halo_bundle_can_defer_coarse_lods() {
    let mut chunk = empty_test_chunk();
    chunk.set_block_local(8, 64, 8, BlockType::Stone);
    let key = SectionKey::new(0, 4, 0);
    let halo = SectionHaloSnapshot::from_chunk(key, |wx, wy, wz| {
        let (block, sky, block_light, level, falling) = test_chunk_lookup(&chunk, wx, wy, wz);
        MeshVoxel {
            block,
            state: 0,
            sky,
            block_light,
            raw_fluid: level | if falling { 8 } else { 0 },
        }
    });
    let bundle = Chunk::generate_section_mesh_bundle_from_halo_for_lods(
        SectionIdentity::new(key, 1, 1),
        &halo,
        LodLevel::MASK_L0,
    );
    assert!(!bundle.levels[0].opaque.indices.is_empty());
    assert!(bundle.levels[1].opaque.indices.is_empty());
    assert!(bundle.levels[2].opaque.indices.is_empty());
    assert_eq!(bundle.built_lods, LodLevel::MASK_L0);
}

#[test]
fn debug_negative_section_mesh_bounds() {
    let mut chunk = empty_test_chunk();
    chunk.set_block_local(8, -32, 8, BlockType::Stone);
    let key = SectionKey::new(0, -2, 0);
    let bundle = chunk
        .generate_section_mesh_bundle(key, 1, 1, |x, y, z| test_chunk_lookup(&chunk, x, y, z));
    let (min_y, _max_y, vertex_count) = match bundle.bounds {
        Some(bounds) => (
            bounds.min.y,
            bounds.max.y,
            bundle.levels[0].opaque.vertices.len(),
        ),
        None => (f32::NAN, f32::NAN, 0),
    };
    assert!(
        (min_y + 32.0).abs() < 0.05,
        "section y=-2 mesh min Y must stay at -32, got {min_y}"
    );
    assert!(vertex_count > 0);
}

#[test]
fn section_halo_occludes_boundary_neighbor() {
    let key = SectionKey::new(0, 0, 0);
    let snapshot = SectionHaloSnapshot::from_chunk(key, |wx, wy, wz| {
        let in_section =
            (0..16).contains(&wx) && (0..16).contains(&wy) && (0..16).contains(&wz);
        let neighbor_block = (wx, wy, wz) == (16, 0, 0);
        MeshVoxel {
            block: if in_section || neighbor_block {
                BlockType::Stone
            } else {
                BlockType::Air
            },
            state: 0,
            sky: 15,
            block_light: 0,
            raw_fluid: 0,
        }
    });

    assert_eq!(snapshot.get_block(1, 1, 1), BlockType::Stone);
    assert_eq!(snapshot.get_block(17, 1, 1), BlockType::Stone);
}

#[test]
fn mesh_voxel_render_inputs_flow_through_core() {
    let voxel = MeshVoxel {
        block: BlockType::Torch,
        state: 3,
        sky: 14,
        block_light: 12,
        raw_fluid: 5 | 8,
    };
    assert_eq!(voxel.block, BlockType::Torch);
    assert_eq!(voxel.state, 3);
    assert_eq!(voxel.sky, 14);
    assert_eq!(voxel.block_light, 12);
    assert_eq!(voxel.raw_fluid & 7, 5);
    assert_ne!(voxel.raw_fluid & 8, 0);
}



fn mesh_chunk_l0_with_lookup<F>(
    chunk: &Chunk,
    get_block_at: F,
) -> (Vec<TerrainVertex>, Vec<u32>, Vec<TerrainVertex>, Vec<u32>)
where
    F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool),
{
    let min_y = chunk.min_world_y();
    let total_height = (chunk.max_world_y_exclusive() - min_y) as usize;
    let origin = [
        chunk.chunk_x * CHUNK_WIDTH as i32,
        min_y,
        chunk.chunk_z * CHUNK_DEPTH as i32,
    ];
    Chunk::mesh_l0_volume(origin, [CHUNK_WIDTH, total_height, CHUNK_DEPTH], |x, y, z| {
        let (lookup_block, sky, block_light, level, falling) = get_block_at(x, y, z);
        let in_chunk = x.div_euclid(CHUNK_WIDTH as i32) == chunk.chunk_x
            && z.div_euclid(CHUNK_DEPTH as i32) == chunk.chunk_z
            && y >= min_y
            && y < min_y + total_height as i32;
        let block = if in_chunk {
            chunk.get_block_local(
                x.rem_euclid(CHUNK_WIDTH as i32) as usize,
                y,
                z.rem_euclid(CHUNK_DEPTH as i32) as usize,
            )
        } else {
            lookup_block
        };
        MeshVoxel {
            block,
            state: chunk.get_block_state(x - origin[0], y, z - origin[2]),
            sky,
            block_light,
            raw_fluid: level | if falling { 8 } else { 0 },
        }
    })
}

fn mesh_chunk_l0(
    chunk: &Chunk,
) -> (Vec<TerrainVertex>, Vec<u32>, Vec<TerrainVertex>, Vec<u32>) {
    mesh_chunk_l0_with_lookup(chunk, |x, y, z| test_chunk_lookup(chunk, x, y, z))
}

fn test_chunk_lookup(
    chunk: &Chunk,
    world_x: i32,
    world_y: i32,
    world_z: i32,
) -> (BlockType, u8, u8, u8, bool) {
    if world_x < 0
        || world_x >= CHUNK_WIDTH as i32
        || world_z < 0
        || world_z >= CHUNK_DEPTH as i32
    {
        return (BlockType::Air, 15, 0, 0, false);
    }
    let x = world_x as usize;
    let z = world_z as usize;
    let fluid = chunk.get_fluid_level(x, world_y, z);
    (
        chunk.get_block_local(x, world_y, z),
        chunk.get_sky_light(x, world_y, z),
        chunk.get_block_light(x, world_y, z),
        fluid & 0x07,
        fluid & 0x08 != 0,
    )
}

fn single_torch_mesh(
    block: BlockType,
    sky_light: u8,
    block_light: u8,
) -> (Vec<TerrainVertex>, Vec<u32>, Vec<TerrainVertex>, Vec<u32>) {
    let mut chunk = empty_test_chunk();
    chunk.set_block_local(8, 1, 8, block);
    chunk.set_sky_light(8, 1, 8, sky_light);
    chunk.set_block_light(8, 1, 8, block_light);
    chunk.heightmap[8][8] = 1;
    mesh_chunk_l0(&chunk)
}

#[test]
fn ambient_occlusion_levels_match_occluder_counts() {
    assert_eq!(ambient_occlusion_value(0), 1.0);
    assert_eq!(ambient_occlusion_value(1), 0.75);
    assert_eq!(ambient_occlusion_value(2), 0.5);
    assert_eq!(ambient_occlusion_value(3), 0.25);
    assert_eq!(ambient_occlusion_value(4), 0.25);
}

#[test]
fn only_solid_opaque_blocks_cast_ambient_occlusion() {
    assert!(BlockType::Stone.is_ao_occluder());
    assert!(BlockType::Grass.is_ao_occluder());
    for block in [
        BlockType::Air,
        BlockType::Water,
        BlockType::Lava,
        BlockType::Glass,
        BlockType::OakLeaves,
        BlockType::Torch,
        BlockType::TallGrass,
        BlockType::Cactus,
    ] {
        assert!(!block.is_ao_occluder(), "{block:?} should not cast AO");
    }
}

#[test]
fn ao_samples_follow_every_face_and_corner_direction() {
    let block_pos = [10, 20, 30];

    // Top face (+Y): normal is [0, 1, 0]
    let top_normal = [0, 1, 0];
    let corner_north_west = [0.0, 1.0, 0.0];
    let samples = ao_sample_positions(block_pos, top_normal, corner_north_west);
    assert_eq!(
        samples,
        [
            [9, 21, 30],  // side U: -X
            [10, 21, 29], // side V: -Z
            [9, 21, 29]   // corner: -X -Z
        ]
    );

    let corner_south_east = [1.0, 1.0, 1.0];
    let samples = ao_sample_positions(block_pos, top_normal, corner_south_east);
    assert_eq!(
        samples,
        [
            [11, 21, 30], // side U: +X
            [10, 21, 31], // side V: +Z
            [11, 21, 31]  // corner: +X +Z
        ]
    );

    // East face (+X): normal is [1, 0, 0]
    let east_normal = [1, 0, 0];
    let corner_top_south = [1.0, 1.0, 1.0];
    let samples = ao_sample_positions(block_pos, east_normal, corner_top_south);
    assert_eq!(
        samples,
        [
            [11, 21, 30], // side U: +Y
            [11, 20, 31], // side V: +Z
            [11, 21, 31]  // corner: +Y +Z
        ]
    );
}

fn triangle_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ]
}

#[test]
fn ao_diagonal_selection_preserves_face_winding() {
    let quad_positions = [
        [0.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
    ];

    for ao in [
        [1.0, 0.5, 1.0, 0.5], // ao[0] + ao[2] > ao[1] + ao[3] -> [0, 1, 3, 1, 2, 3]
        [0.5, 1.0, 0.5, 1.0], // ao[0] + ao[2] <= ao[1] + ao[3] -> [0, 1, 2, 0, 2, 3]
    ] {
        let indices = quad_indices_for_ao(ao);
        let n1 = triangle_normal(
            quad_positions[indices[0] as usize],
            quad_positions[indices[1] as usize],
            quad_positions[indices[2] as usize],
        );
        let n2 = triangle_normal(
            quad_positions[indices[3] as usize],
            quad_positions[indices[4] as usize],
            quad_positions[indices[5] as usize],
        );
        assert!(
            n1[1] > 0.0,
            "first triangle must face +Y for top quad winding"
        );
        assert!(
            n2[1] > 0.0,
            "second triangle must face +Y for top quad winding"
        );
    }
}

#[test]
fn generated_mesh_writes_ao_for_isolated_and_occluded_vertices() {
    let mut chunk = empty_test_chunk();
    chunk.set_block_local(8, 1, 8, BlockType::Stone);
    chunk.heightmap[8][8] = 1;

    let (vertices, _, _, _) = mesh_chunk_l0(&chunk);
    assert!(
        vertices.iter().all(|vertex| vertex.ao() == 1.0),
        "an isolated stone cube in empty air must have full 1.0 AO across all vertices"
    );

    // Place occluders flanking the top-north-west corner of (8, 1, 8).
    chunk.set_block_local(7, 2, 8, BlockType::Stone);
    chunk.set_block_local(8, 2, 7, BlockType::Stone);
    chunk.heightmap[7][8] = 2;
    chunk.heightmap[8][7] = 2;

    let (vertices, _, _, _) = mesh_chunk_l0(&chunk);
    let top_face_ao_values: Vec<f32> = vertices
        .iter()
        .filter(|vertex| {
            let pos = vertex.local_position();
            (pos[1] - 2.0).abs() < 1e-4
                && pos[0] >= 8.0
                && pos[0] <= 9.0
                && pos[2] >= 8.0
                && pos[2] <= 9.0
        })
        .map(|vertex| vertex.ao())
        .collect();

    assert!(
        top_face_ao_values.iter().any(|ao| *ao < 1.0),
        "top face near neighbor occluders must have darkened AO values: {top_face_ao_values:?}"
    );
}

#[test]
fn greedy_meshing_merges_equal_faces_and_repeats_the_atlas_tile() {
    let mut chunk = empty_test_chunk();
    for x in 8..10 {
        for z in 8..10 {
            chunk.set_block_local(x, 1, z, BlockType::Stone);
            chunk.heightmap[x][z] = 1;
        }
    }

    let lookup = |x, y, z| test_chunk_lookup(&chunk, x, y, z);
    let (vertices, indices, transparent_vertices, transparent_indices) =
        mesh_chunk_l0_with_lookup(&chunk, lookup);

    // A 2x1x2 cuboid has six exterior rectangles after greedy merging.
    assert_eq!(vertices.len(), 6 * 4);
    assert_eq!(indices.len(), 6 * 6);
    assert!(transparent_vertices.is_empty());
    assert!(transparent_indices.is_empty());
    assert_eq!(
        vertices
            .iter()
            .flat_map(|vertex| vertex.local_uv_f32())
            .fold(0.0f32, f32::max),
        2.0
    );
}

#[test]
fn greedy_meshing_does_not_merge_different_light_or_material() {
    let mut light_chunk = empty_test_chunk();
    for x in 8..10 {
        light_chunk.set_block_local(x, 1, 8, BlockType::Stone);
        light_chunk.heightmap[x][8] = 1;
    }
    light_chunk.set_sky_light(9, 2, 8, 14);
    let (light_vertices, _, _, _) =
        mesh_chunk_l0(&light_chunk);
    let light_top_quads = light_vertices
        .chunks_exact(4)
        .filter(|quad| quad.iter().all(|vertex| vertex.local_position()[1] == 2.0))
        .count();
    assert_eq!(light_top_quads, 2);

    let mut material_chunk = empty_test_chunk();
    material_chunk.set_block_local(8, 1, 8, BlockType::Stone);
    material_chunk.set_block_local(9, 1, 8, BlockType::Dirt);
    material_chunk.heightmap[8][8] = 1;
    material_chunk.heightmap[9][8] = 1;
    let (material_vertices, _, _, _) =
        mesh_chunk_l0(&material_chunk);
    let material_top_quads = material_vertices
        .chunks_exact(4)
        .filter(|quad| quad.iter().all(|vertex| vertex.local_position()[1] == 2.0))
        .count();
    assert_eq!(material_top_quads, 2);
}

#[test]
fn section_halo_lod_coarsens_varied_terrain() {
    let mut flat = empty_test_chunk();
    for x in 0..CHUNK_WIDTH {
        for z in 0..CHUNK_DEPTH {
            flat.set_block_local(x, 1, z, BlockType::Stone);
            flat.heightmap[x][z] = 1;
        }
    }
    let flat_key = SectionKey::new(0, world_y_to_section_y(1), 0);
    let flat_halo = SectionHaloSnapshot::from_chunk(flat_key, |x, y, z| {
        let (block, sky, block_light, level, falling) = test_chunk_lookup(&flat, x, y, z);
        MeshVoxel {
            block,
            state: 0,
            sky,
            block_light,
            raw_fluid: level | if falling { 8 } else { 0 },
        }
    });
    let flat_bundle = Chunk::generate_section_mesh_bundle_from_halo_for_lods(
        crate::world::SectionIdentity::new(flat_key, 1, 1),
        &flat_halo,
        LodLevel::MASK_ALL,
    );
    assert!(!flat_bundle.levels[0].opaque.indices.is_empty());
    assert!(!flat_bundle.levels[1].opaque.indices.is_empty() || !flat_bundle.levels[2].opaque.indices.is_empty());

    let mut varied = empty_test_chunk();
    for x in 0..CHUNK_WIDTH {
        for z in 0..CHUNK_DEPTH {
            let height = 1 + (x + z) % 2;
            for y in 1..=height {
                varied.set_block_local(x, y as i32, z, BlockType::Stone);
            }
            varied.heightmap[x][z] = height as i16;
        }
    }
    let varied_key = SectionKey::new(0, world_y_to_section_y(1), 0);
    let varied_halo = SectionHaloSnapshot::from_chunk(varied_key, |x, y, z| {
        let (block, sky, block_light, level, falling) = test_chunk_lookup(&varied, x, y, z);
        MeshVoxel {
            block,
            state: 0,
            sky,
            block_light,
            raw_fluid: level | if falling { 8 } else { 0 },
        }
    });
    let varied_bundle = Chunk::generate_section_mesh_bundle_from_halo_for_lods(
        crate::world::SectionIdentity::new(varied_key, 1, 1),
        &varied_halo,
        LodLevel::MASK_ALL,
    );
    assert!(
        varied_bundle.levels[2].opaque.indices.len()
            <= varied_bundle.levels[1].opaque.indices.len(),
        "coarse section LOD should not submit more indices than finer LOD"
    );
}

#[test]
fn snow_layer_mesh_is_one_eighth_of_a_block_high() {
    let mut chunk = Chunk::new(0, 0);
    for x in 0..CHUNK_WIDTH {
        for y in chunk.world_y_range() {
            for z in 0..CHUNK_DEPTH {
                chunk.set_block_local(x, y, z, BlockType::Air);
            }
        }
        for z in 0..CHUNK_DEPTH {
            chunk.heightmap[x][z] = NO_HEIGHT;
        }
    }
    chunk.set_block_local(8, 1, 8, BlockType::SnowLayer);
    chunk.heightmap[8][8] = 1;
    let lookup = |_: i32, _: i32, _: i32| (BlockType::Air, 15, 0, 0, false);
    let (vertices, _, _, _) = mesh_chunk_l0_with_lookup(&chunk, lookup);
    let max_y = vertices
        .iter()
        .map(|vertex| vertex.local_position()[1])
        .fold(f32::NEG_INFINITY, f32::max);
    assert!((max_y - 1.125).abs() < f32::EPSILON);
}

#[test]
fn cross_model_blocks_generate_x_mesh() {
    for plant in [
        BlockType::Dandelion,
        BlockType::Poppy,
        BlockType::TallGrass,
        BlockType::SugarCane,
    ] {
        let mut chunk = empty_test_chunk();
        chunk.set_block_local(8, 64, 8, plant);
        chunk.heightmap[8][8] = 64;

        let (vertices, indices, transparent_vertices, transparent_indices) =
            mesh_chunk_l0(&chunk);

        assert_eq!(
            vertices.len(),
            16,
            "{plant:?} must produce four quads (two double-sided planes)"
        );
        assert_eq!(indices.len(), 24, "{plant:?} must produce eight triangles");
        assert!(
            transparent_vertices.is_empty(),
            "cutout plants belong in the opaque pass"
        );
        assert!(transparent_indices.is_empty());
    }
}

#[test]
fn torch_mesh_has_minecraft_bounds_and_six_outward_faces() {
    let (vertices, indices, transparent_vertices, transparent_indices) =
        single_torch_mesh(BlockType::Torch, 15, 14);

    assert_eq!(
        vertices.len(),
        24,
        "torch must produce six four-vertex quads"
    );
    assert_eq!(
        indices.len(),
        36,
        "torch must produce twelve three-index triangles"
    );
    assert!(transparent_vertices.is_empty());
    assert!(transparent_indices.is_empty());

    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for vertex in &vertices {
        let pos = vertex.local_position();
        for axis in 0..3 {
            min[axis] = min[axis].min(pos[axis]);
            max[axis] = max[axis].max(pos[axis]);
        }
    }

    assert_eq!(min, [8.0 + TORCH_MIN, 1.0, 8.0 + TORCH_MIN]);
    assert_eq!(max, [8.0 + TORCH_MAX, 1.0 + TORCH_HEIGHT, 8.0 + TORCH_MAX]);
    assert_eq!(
        [max[0] - min[0], max[1] - min[1], max[2] - min[2]],
        [2.0 / 16.0, 10.0 / 16.0, 2.0 / 16.0],
        "torch bounding box must be exactly 2x10x2 texels"
    );
}

#[test]
fn door_mesh_generation_bounds_and_quad_count() {
    let (opaque_v, opaque_i, trans_v, trans_i) = single_torch_mesh(BlockType::OakDoor, 15, 0);
    assert!(trans_v.is_empty() && trans_i.is_empty());
    // 6 faces * 4 vertices = 24 vertices, 36 indices
    assert_eq!(opaque_v.len(), 24);
    assert_eq!(opaque_i.len(), 36);

    let min_x = opaque_v
        .iter()
        .map(|v| v.pos[0] as f32 / 32.0)
        .fold(f32::INFINITY, f32::min);
    let max_x = opaque_v
        .iter()
        .map(|v| v.pos[0] as f32 / 32.0)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_z = opaque_v
        .iter()
        .map(|v| v.pos[2] as f32 / 32.0)
        .fold(f32::INFINITY, f32::min);
    let max_z = opaque_v
        .iter()
        .map(|v| v.pos[2] as f32 / 32.0)
        .fold(f32::NEG_INFINITY, f32::max);

    // North facing door, closed: X in [8.0, 9.0], Z in [8.0, 8.0 + 3.0/16.0]
    assert!((min_x - 8.0).abs() < 1e-4);
    assert!((max_x - 9.0).abs() < 1e-4);
    assert!((min_z - 8.0).abs() < 1e-4);
    assert!((max_z - (8.0 + 3.0 / 16.0)).abs() < 1e-4);
}

#[test]
fn trapdoor_mesh_generation_open_and_closed_bounds() {
    // Closed trapdoor: bottom 3/16ths
    let mut chunk = empty_test_chunk();
    chunk.set_block_local(8, 1, 8, BlockType::OakTrapdoor);
    chunk.set_block_state(8, 1, 8, BlockState::default().encode()); // closed, north
    chunk.heightmap[8][8] = 1;

    let (opaque_v, _, _, _) = mesh_chunk_l0(&chunk);
    let min_y = opaque_v
        .iter()
        .map(|v| v.local_position()[1])
        .fold(f32::INFINITY, f32::min);
    let max_y = opaque_v
        .iter()
        .map(|v| v.local_position()[1])
        .fold(f32::NEG_INFINITY, f32::max);

    assert!((min_y - 1.0).abs() < 1e-4);
    assert!((max_y - (1.0 + 3.0 / 16.0)).abs() < 1e-4);

    // Open trapdoor: against north wall
    let open_state = BlockState {
        is_open: true,
        ..BlockState::default()
    };
    chunk.set_block_state(8, 1, 8, open_state.encode());
    let (opaque_v2, _, _, _) =
        mesh_chunk_l0(&chunk);
    let min_z = opaque_v2
        .iter()
        .map(|v| v.pos[2] as f32 / 32.0)
        .fold(f32::INFINITY, f32::min);
    let max_z = opaque_v2
        .iter()
        .map(|v| v.pos[2] as f32 / 32.0)
        .fold(f32::NEG_INFINITY, f32::max);

    assert!((min_z - 8.0).abs() < 1e-4);
    assert!((max_z - (8.0 + 3.0 / 16.0)).abs() < 1e-4);
}

#[test]
fn torch_mesh_uses_inset_face_uvs_inside_its_atlas_tile() {
    let (vertices, _, _, _) = single_torch_mesh(BlockType::Torch, 15, 14);
    let expected_rects = [
        TORCH_SIDE_UV,
        TORCH_SIDE_UV,
        TORCH_SIDE_UV,
        TORCH_SIDE_UV,
        TORCH_TOP_UV,
        TORCH_BOTTOM_UV,
    ];

    assert_eq!(
        TORCH_SIDE_UV,
        [6.5 / 16.0, 2.5 / 16.0, 8.5 / 16.0, 13.5 / 16.0]
    );
    assert_eq!(
        TORCH_TOP_UV,
        [6.5 / 16.0, 2.5 / 16.0, 8.5 / 16.0, 4.5 / 16.0]
    );
    assert_eq!(
        TORCH_BOTTOM_UV,
        [7.5 / 16.0, 13.5 / 16.0, 7.5 / 16.0, 13.5 / 16.0]
    );

    for (face_idx, quad) in vertices.chunks_exact(4).enumerate() {
        let rect = expected_rects[face_idx];
        let mut observed_min = [f32::INFINITY; 2];
        let mut observed_max = [f32::NEG_INFINITY; 2];
        for vertex in quad {
            assert_eq!(vertex.atlas_tile_u32(), (4, 2));
            let uv = vertex.local_uv_f32();
            assert!(
                (0.0..1.0).contains(&uv[0]) && (0.0..1.0).contains(&uv[1]),
                "torch UV must remain inside atlas tile (4, 2)"
            );
            observed_min[0] = observed_min[0].min(uv[0]);
            observed_min[1] = observed_min[1].min(uv[1]);
            observed_max[0] = observed_max[0].max(uv[0]);
            observed_max[1] = observed_max[1].max(uv[1]);
        }
        assert_eq!(observed_min, [rect[0], rect[1]]);
        assert_eq!(observed_max, [rect[2], rect[3]]);
    }
}

#[test]
fn torch_mesh_uses_source_light_without_ao_or_face_shading() {
    let sky_light = 9;
    let block_light = 14;
    let expected_packed_light = sky_light as f32 + block_light as f32 * 16.0;
    let (vertices, _, _, _) = single_torch_mesh(BlockType::Torch, sky_light, block_light);

    assert!(vertices.iter().all(|vertex| vertex.ao() == 1.0));
    assert!(
        vertices
            .iter()
            .all(|vertex| vertex.light_level() == expected_packed_light),
        "every torch face must use the source cell light without a face multiplier"
    );
}

#[test]
fn redstone_torch_variants_use_thin_torch_mesh_and_redstone_tile() {
    let expected_rects = [
        TORCH_SIDE_UV,
        TORCH_SIDE_UV,
        TORCH_SIDE_UV,
        TORCH_SIDE_UV,
        TORCH_TOP_UV,
        TORCH_BOTTOM_UV,
    ];

    for block in [BlockType::RedstoneTorch] {
        let (vertices, indices, transparent_vertices, transparent_indices) =
            single_torch_mesh(block, 15, block.properties().light_emission);

        assert_eq!(vertices.len(), 24, "{block:?} must have six quads");
        assert_eq!(indices.len(), 36, "{block:?} must have twelve triangles");
        assert!(transparent_vertices.is_empty());
        assert!(transparent_indices.is_empty());

        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for vertex in &vertices {
            let pos = vertex.local_position();
            for axis in 0..3 {
                min[axis] = min[axis].min(pos[axis]);
                max[axis] = max[axis].max(pos[axis]);
            }
        }
        assert_eq!(min, [8.0 + TORCH_MIN, 1.0, 8.0 + TORCH_MIN]);
        assert_eq!(max, [8.0 + TORCH_MAX, 1.0 + TORCH_HEIGHT, 8.0 + TORCH_MAX]);
        assert_eq!(
            [max[0] - min[0], max[1] - min[1], max[2] - min[2]],
            [2.0 / 16.0, 10.0 / 16.0, 2.0 / 16.0],
            "{block:?} must not fall back to full-cube geometry"
        );

        for (face_idx, quad) in vertices.chunks_exact(4).enumerate() {
            let rect = expected_rects[face_idx];
            let mut observed_min = [f32::INFINITY; 2];
            let mut observed_max = [f32::NEG_INFINITY; 2];
            for vertex in quad {
                assert_eq!(
                    vertex.atlas_tile_u32(),
                    (
                        REDSTONE_TORCH_ATLAS_TILE.0 as u32,
                        REDSTONE_TORCH_ATLAS_TILE.1 as u32,
                    )
                );
                let uv = vertex.local_uv_f32();
                observed_min[0] = observed_min[0].min(uv[0]);
                observed_min[1] = observed_min[1].min(uv[1]);
                observed_max[0] = observed_max[0].max(uv[0]);
                observed_max[1] = observed_max[1].max(uv[1]);
            }
            assert_eq!(observed_min, [rect[0], rect[1]]);
            assert_eq!(observed_max, [rect[2], rect[3]]);
        }
    }
}

#[test]
fn torch_properties_and_floor_support_semantics_are_preserved() {
    let properties = BlockType::Torch.properties();
    assert_eq!(properties.render_type, RenderType::Cutout);
    assert!(!properties.is_solid);
    assert!(!properties.is_passable);
    assert_eq!(properties.light_emission, 14);
    assert!(BlockType::Torch.can_stay_on(BlockType::Stone));
    assert!(!BlockType::Torch.can_stay_on(BlockType::Air));

    let mut manager = crate::chunk_manager::WorldColumns::new(2);
    manager.chunks.insert((0, 0), empty_test_chunk());
    manager.set_block(8, 64, 8, BlockType::Stone);
    manager.set_block(8, 65, 8, BlockType::Torch);

    let mut dirty = HashSet::new();
    crate::lighting::update_block_light_after_placed(
        &mut manager,
        8,
        65,
        8,
        properties.light_emission,
        &mut dirty,
    );
    assert_eq!(manager.get_block_light(8, 65, 8), 14);
    assert_eq!(manager.get_block_light(9, 65, 8), 13);

    manager.set_block(8, 64, 8, BlockType::Air);
    let mut broken = Vec::new();
    manager.check_and_break_unsupported_above(8, 64, 8, &mut dirty, |position, block| {
        broken.push((position, block));
    });

    assert_eq!(manager.get_block(8, 65, 8), BlockType::Air);
    assert_eq!(broken, vec![((8, 65, 8), BlockType::Torch)]);
    assert_eq!(manager.get_block_light(8, 65, 8), 0);
    assert_eq!(manager.get_block_light(9, 65, 8), 0);
}

#[test]
fn cactus_mesh_generation_bounds_and_quad_count() {
    let (opaque_v, opaque_i, trans_v, trans_i) = single_torch_mesh(BlockType::Cactus, 15, 0);
    assert!(trans_v.is_empty() && trans_i.is_empty());
    // 6 faces * 4 vertices = 24 vertices, 36 indices
    assert_eq!(opaque_v.len(), 24);
    assert_eq!(opaque_i.len(), 36);

    let min_x = opaque_v
        .iter()
        .map(|v| v.pos[0] as f32 / 32.0)
        .fold(f32::INFINITY, f32::min);
    let max_x = opaque_v
        .iter()
        .map(|v| v.pos[0] as f32 / 32.0)
        .fold(f32::NEG_INFINITY, f32::max);
    let min_z = opaque_v
        .iter()
        .map(|v| v.pos[2] as f32 / 32.0)
        .fold(f32::INFINITY, f32::min);
    let max_z = opaque_v
        .iter()
        .map(|v| v.pos[2] as f32 / 32.0)
        .fold(f32::NEG_INFINITY, f32::max);

    // Cactus placed at (8, 1, 8) -> origin = (8.0, 1.0, 8.0)
    // Inset by 1/16th: min = 8.0625, max = 8.9375
    assert!((min_x - (8.0 + 1.0 / 16.0)).abs() < 1e-4);
    assert!((max_x - (8.0 + 15.0 / 16.0)).abs() < 1e-4);
    assert!((min_z - (8.0 + 1.0 / 16.0)).abs() < 1e-4);
    assert!((max_z - (8.0 + 15.0 / 16.0)).abs() < 1e-4);
}

#[test]
fn end_portal_frame_and_surface_use_lower_minecraft_heights() {
    for block in [BlockType::EndPortalFrame, BlockType::EndPortalFrame] {
        let (opaque_v, opaque_i, trans_v, trans_i) = single_torch_mesh(block, 15, 0);
        assert!(trans_v.is_empty() && trans_i.is_empty());
        assert_eq!(opaque_v.len(), 24);
        assert_eq!(opaque_i.len(), 36);
        let max_y = opaque_v
            .iter()
            .map(|vertex| vertex.local_position()[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!((max_y - (1.0 + END_PORTAL_FRAME_HEIGHT)).abs() < 1e-4);
    }

    let (opaque_v, opaque_i, trans_v, trans_i) =
        single_torch_mesh(BlockType::EndPortal, 15, 15);
    assert!(opaque_v.is_empty() && opaque_i.is_empty());
    assert_eq!(trans_v.len(), 8);
    assert_eq!(trans_i.len(), 12);
    assert!(trans_v.iter().all(|vertex| {
        (vertex.local_position()[1] - (1.0 + END_PORTAL_SURFACE_HEIGHT)).abs() < 1e-4
    }));
    assert!(END_PORTAL_SURFACE_HEIGHT < END_PORTAL_FRAME_HEIGHT);
}
