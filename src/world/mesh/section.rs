use super::*;

impl Chunk {

    pub(super) fn mesh_section_lod_from_halo(
        key: SectionKey,
        halo: &SectionHaloSnapshot,
        step: usize,
    ) -> ChunkLodMeshData {
        Self::mesh_section_lod_from_halo_with_registry(key, halo, step, None)
    }

    pub(super) fn mesh_section_lod_from_halo_with_registry(
        key: SectionKey,
        halo: &SectionHaloSnapshot,
        step: usize,
        registry: Option<&crate::block_model::ModelRegistry>,
    ) -> ChunkLodMeshData {
        debug_assert!(step > 1 && SECTION_SIZE % step == 0);
        let mut coarse = [MeshVoxel::default(); SECTION_VOLUME];

        for cell_y in (0..SECTION_SIZE).step_by(step) {
            for cell_z in (0..CHUNK_DEPTH).step_by(step) {
                for cell_x in (0..CHUNK_WIDTH).step_by(step) {
                    let mut representative = MeshVoxel::default();
                    'sample: for dy in 0..step {
                        for dz in 0..step {
                            for dx in 0..step {
                                let voxel =
                                    halo.get(cell_x + dx + 1, cell_y + dy + 1, cell_z + dz + 1);
                                if voxel.block != BlockType::Air {
                                    representative = voxel;
                                    break 'sample;
                                }
                            }
                        }
                    }
                    if representative.block == BlockType::Air {
                        continue;
                    }
                    for dy in 0..step {
                        for dz in 0..step {
                            for dx in 0..step {
                                let x = cell_x + dx;
                                let y = cell_y + dy;
                                let z = cell_z + dz;
                                coarse[(y * CHUNK_DEPTH + z) * CHUNK_WIDTH + x] = representative;
                            }
                        }
                    }
                }
            }
        }

        let origin = [
            key.cx * CHUNK_WIDTH as i32,
            key.min_world_y(),
            key.cz * CHUNK_DEPTH as i32,
        ];
        let voxel = |wx: i32, wy: i32, wz: i32| {
            let x = wx - origin[0];
            let y = wy - origin[1];
            let z = wz - origin[2];
            if (0..CHUNK_WIDTH as i32).contains(&x)
                && (0..SECTION_SIZE as i32).contains(&y)
                && (0..CHUNK_DEPTH as i32).contains(&z)
            {
                return coarse[(y as usize * CHUNK_DEPTH + z as usize) * CHUNK_WIDTH + x as usize];
            }
            let hx = x + 1;
            let hy = y + 1;
            let hz = z + 1;
            if (0..SectionHaloSnapshot::SIDE as i32).contains(&hx)
                && (0..SectionHaloSnapshot::SIDE as i32).contains(&hy)
                && (0..SectionHaloSnapshot::SIDE as i32).contains(&hz)
            {
                halo.get(hx as usize, hy as usize, hz as usize)
            } else {
                MeshVoxel::default()
            }
        };
        let (opaque, opaque_indices, transparent, transparent_indices) =
            Self::mesh_l0_volume_with_registry(
                origin,
                [CHUNK_WIDTH, SECTION_SIZE, CHUNK_DEPTH],
                voxel,
                registry,
            );
        ChunkLodMeshData::from_parts(
            opaque,
            opaque_indices,
            transparent,
            transparent_indices,
            crate::chunk_render::chunk_to_region_coord(key.cx, key.cz),
        )
    }
}
