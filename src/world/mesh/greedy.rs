use super::*;

impl Chunk {
    // Generate opaque/cutout and translucent terrain meshes. Full cube faces
    // use conservative greedy merging: material/light must match and AO must
    // be uniform so removing internal vertices cannot change shading.
    pub fn mesh_l0_volume<F>(
        origin: [i32; 3],
        extent: [usize; 3],
        get_voxel: F,
    ) -> (Vec<TerrainVertex>, Vec<u32>, Vec<TerrainVertex>, Vec<u32>)
    where
        F: Fn(i32, i32, i32) -> MeshVoxel,
    {
        Self::mesh_l0_volume_with_registry(origin, extent, get_voxel, None)
    }

    pub fn mesh_l0_volume_with_registry<F>(
        origin: [i32; 3],
        extent: [usize; 3],
        get_voxel: F,
        registry: Option<&crate::block_model::ModelRegistry>,
    ) -> (Vec<TerrainVertex>, Vec<u32>, Vec<TerrainVertex>, Vec<u32>)
    where
        F: Fn(i32, i32, i32) -> MeshVoxel,
    {
        let get_block_at = |x: i32, y: i32, z: i32| {
            let v = get_voxel(x, y, z);
            (
                v.block,
                v.sky,
                v.block_light,
                v.raw_fluid & 7,
                v.raw_fluid & 8 != 0,
            )
        };
        let mut non_air = 0usize;
        for x in 0..extent[0] {
            for z in 0..extent[2] {
                for y in 0..extent[1] {
                    if get_voxel(
                        origin[0] + x as i32,
                        origin[1] + y as i32,
                        origin[2] + z as i32,
                    )
                    .block
                        != BlockType::Air
                    {
                        non_air += 1;
                    }
                }
            }
        }
        // Six cube faces × 4 verts / 6 indices is a hard upper bound for greedy
        // cubes; custom models may grow past it and Vec will reallocate.
        let vert_cap = non_air.saturating_mul(24);
        let idx_cap = non_air.saturating_mul(36);
        let mut opaque_vertices = Vec::with_capacity(vert_cap);
        let mut opaque_indices = Vec::with_capacity(idx_cap);
        let mut trans_vertices = Vec::with_capacity(vert_cap / 4);
        let mut trans_indices = Vec::with_capacity(idx_cap / 4);

        let region_coord = crate::chunk_render::chunk_to_region_coord(
            origin[0] / CHUNK_WIDTH as i32,
            origin[2] / CHUNK_DEPTH as i32,
        );

        // Non-cubic geometry and non-solid decorative blocks retain the exact
        // per-block path. They cannot be combined into rectangular cube faces.
        for x in 0..extent[0] {
            for z in 0..extent[2] {
                for y in 0..extent[1] {
                    let voxel = get_voxel(
                        origin[0] + x as i32,
                        origin[1] + y as i32,
                        origin[2] + z as i32,
                    );
                    let block = voxel.block;
                    if block == BlockType::Air || is_greedy_cube(block) {
                        continue;
                    }

                    let world_x = origin[0] + x as i32;
                    let world_y = origin[1] + y as i32;
                    let world_z = origin[2] + z as i32;

                    let custom_mesh = if let Some(registry) = registry {
                        let model_path = crate::block_model::model_path_for_block(block);
                        crate::block_model::append_custom_block_mesh_with_registry(
                            block,
                            voxel.state,
                            [world_x as f32, world_y as f32, world_z as f32],
                            voxel.sky,
                            voxel.block_light,
                            region_coord,
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            &mut trans_vertices,
                            &mut trans_indices,
                            &model_path,
                            registry,
                            |nx, ny, nz| get_block_at(nx, ny, nz).0,
                        )
                    } else {
                        crate::block_model::append_custom_block_mesh(
                            block,
                            voxel.state,
                            [world_x as f32, world_y as f32, world_z as f32],
                            voxel.sky,
                            voxel.block_light,
                            region_coord,
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            &mut trans_vertices,
                            &mut trans_indices,
                            |nx, ny, nz| get_block_at(nx, ny, nz).0,
                        )
                    };
                    if custom_mesh {
                        if block.is_waterloggable() && voxel.raw_fluid & FLUID_WATERLOGGED_BIT != 0
                        {
                            crate::block_model::append_waterlogged_slab_mesh(
                                block,
                                voxel.state,
                                [world_x as f32, world_y as f32, world_z as f32],
                                voxel.sky,
                                voxel.block_light,
                                region_coord,
                                &mut trans_vertices,
                                &mut trans_indices,
                                registry.map(|registry| {
                                    registry.atlas_tile_for_block(
                                        BlockType::Water,
                                        BlockType::Water.get_face_tex_index(0),
                                    )
                                }),
                            );
                        }
                        continue;
                    }

                    let torch_atlas_tile = match block {
                        BlockType::Torch => Some(registry.map_or(TORCH_ATLAS_TILE, |registry| {
                            registry.atlas_tile_for_block(block, TORCH_ATLAS_TILE)
                        })),
                        BlockType::RedstoneTorch => {
                            Some(registry.map_or(REDSTONE_TORCH_ATLAS_TILE, |registry| {
                                registry.atlas_tile_for_block(block, REDSTONE_TORCH_ATLAS_TILE)
                            }))
                        }
                        _ => None,
                    };
                    if let Some(atlas_tile) = torch_atlas_tile {
                        append_torch_mesh(
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            [world_x as f32, world_y as f32, world_z as f32],
                            voxel.sky,
                            voxel.block_light,
                            atlas_tile,
                            region_coord,
                        );
                        continue;
                    }

                    if matches!(block, BlockType::OakDoor) {
                        let state = BlockState::decode(voxel.state);
                        append_door_mesh(
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            [world_x as f32, world_y as f32, world_z as f32],
                            state,
                            voxel.sky,
                            voxel.block_light,
                            registry.map_or((9, 14), |registry| {
                                registry.atlas_tile_for_block(block, (9, 14))
                            }),
                            region_coord,
                        );
                        continue;
                    }

                    if matches!(block, BlockType::OakTrapdoor) {
                        let state = BlockState::decode(voxel.state);
                        append_trapdoor_mesh(
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            [world_x as f32, world_y as f32, world_z as f32],
                            state,
                            voxel.sky,
                            voxel.block_light,
                            registry.map_or((10, 14), |registry| {
                                registry.atlas_tile_for_block(block, (10, 14))
                            }),
                            region_coord,
                        );
                        continue;
                    }

                    if block == BlockType::Cactus {
                        append_cactus_mesh(
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            [world_x as f32, world_y as f32, world_z as f32],
                            voxel.sky,
                            voxel.block_light,
                            registry.map_or((11, 12), |registry| {
                                registry.atlas_tile_for_block(block, (11, 12))
                            }),
                            region_coord,
                        );
                        continue;
                    }

                    if matches!(block, BlockType::EndPortalFrame) {
                        append_end_portal_frame_mesh(
                            &mut opaque_vertices,
                            &mut opaque_indices,
                            [world_x as f32, world_y as f32, world_z as f32],
                            block,
                            BlockState::decode(voxel.state),
                            voxel.sky,
                            voxel.block_light,
                            region_coord,
                            registry,
                        );
                        continue;
                    }

                    if block == BlockType::EndPortal {
                        append_end_portal_surface(
                            &mut trans_vertices,
                            &mut trans_indices,
                            [world_x as f32, world_y as f32, world_z as f32],
                            region_coord,
                            registry,
                        );
                        continue;
                    }

                    if block.def().is_cross_model {
                        let sky_val = voxel.sky;
                        let block_val = voxel.block_light;
                        let light_val = sky_val as f32 + block_val as f32 * 16.0 + 1.0 * 256.0;

                        let fallback_tile = block.get_face_tex_index(0);
                        let atlas_tile = registry.map_or(fallback_tile, |registry| {
                            registry.atlas_tile_for_block(block, fallback_tile)
                        });

                        let wx = world_x as f32;
                        let wy = world_y as f32;
                        let wz = world_z as f32;

                        let min_off = 0.1464466;
                        let max_off = 0.8535534;

                        let plane1_p0 = [wx + min_off, wy, wz + min_off];
                        let plane1_p1 = [wx + max_off, wy, wz + max_off];
                        let plane1_p2 = [wx + max_off, wy + 1.0, wz + max_off];
                        let plane1_p3 = [wx + min_off, wy + 1.0, wz + min_off];

                        let plane2_p0 = [wx + max_off, wy, wz + min_off];
                        let plane2_p1 = [wx + min_off, wy, wz + max_off];
                        let plane2_p2 = [wx + min_off, wy + 1.0, wz + max_off];
                        let plane2_p3 = [wx + max_off, wy + 1.0, wz + min_off];

                        let planes = [
                            (plane1_p0, plane1_p1, plane1_p2, plane1_p3),
                            (plane2_p0, plane2_p1, plane2_p2, plane2_p3),
                        ];

                        for (p0, p1, p2, p3) in planes {
                            push_terrain_quad(
                                &mut opaque_vertices,
                                &mut opaque_indices,
                                [p0, p1, p2, p3],
                                [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
                                atlas_tile,
                                light_val,
                                [1.0; 4],
                                region_coord,
                            );
                            push_terrain_quad(
                                &mut opaque_vertices,
                                &mut opaque_indices,
                                [p1, p0, p3, p2],
                                [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
                                atlas_tile,
                                light_val,
                                [1.0; 4],
                                region_coord,
                            );
                        }

                        continue;
                    }

                    for (face_idx, (normal, corner_data)) in BLOCK_FACES.iter().enumerate() {
                        let nx = world_x + normal[0];
                        let ny = world_y + normal[1];
                        let nz = world_z + normal[2];

                        let (
                            neighbor,
                            neighbor_sky,
                            neighbor_block,
                            neighbor_level,
                            neighbor_falling,
                        ) = get_block_at(nx, ny, nz);
                        let is_fluid = block == BlockType::Water || block == BlockType::Lava;
                        let fl_raw = voxel.raw_fluid;
                        let level = fl_raw & 0x07;
                        let falling = (fl_raw & 0x08) != 0;

                        if face_should_render(
                            block,
                            face_idx,
                            level,
                            falling,
                            neighbor,
                            neighbor_level,
                            neighbor_falling,
                        ) {
                            let block_render_type = block.def().properties.render_type;
                            let is_translucent = block_render_type == RenderType::Translucent;

                            let (v_list, i_list) = if is_translucent {
                                (&mut trans_vertices, &mut trans_indices)
                            } else {
                                (&mut opaque_vertices, &mut opaque_indices)
                            };

                            let state = BlockState::decode(voxel.state);
                            let fallback_tile = block.face_tex_for(state, face_idx);
                            let atlas_tile = registry.map_or(fallback_tile, |registry| {
                                registry.atlas_tile_for_block(block, fallback_tile)
                            });

                            let multiplier_code = match face_idx {
                                4 => 0.0, // Top
                                5 => 2.0, // Bottom
                                _ => 1.0, // Sides
                            };
                            let light_val = if block == BlockType::Lava {
                                15.0 * 16.0 + 15.0 + multiplier_code * 256.0
                            } else {
                                (neighbor_sky as f32)
                                    + (neighbor_block as f32) * 16.0
                                    + multiplier_code * 256.0
                            };

                            let h = if is_fluid {
                                if falling {
                                    1.0
                                } else {
                                    (8 - level) as f32 / 8.0 * 0.9
                                }
                            } else if block == BlockType::SnowLayer {
                                0.125
                            } else {
                                1.0
                            };

                            let mut ao = [1.0; 4];
                            for (corner_idx, (offset, _)) in corner_data.iter().enumerate() {
                                ao[corner_idx] = ambient_occlusion_for_vertex(
                                    [world_x, world_y, world_z],
                                    *normal,
                                    *offset,
                                    &get_block_at,
                                );
                            }

                            let mut positions = [[0.0; 3]; 4];
                            let mut local_uvs = [[0.0; 2]; 4];
                            for (corner_idx, (offset, uv)) in corner_data.iter().enumerate() {
                                let mut vy = world_y as f32 + offset[1];
                                if (is_fluid || block == BlockType::SnowLayer) && offset[1] > 0.0 {
                                    vy = world_y as f32 + h;
                                }

                                positions[corner_idx] =
                                    [world_x as f32 + offset[0], vy, world_z as f32 + offset[2]];
                                local_uvs[corner_idx] = *uv;
                            }
                            push_terrain_quad(
                                v_list,
                                i_list,
                                positions,
                                local_uvs,
                                atlas_tile,
                                light_val,
                                ao,
                                region_coord,
                            );
                        }
                    }
                }
            }
        }

        // Full cube faces are processed one direction/slice at a time. Each
        // mask cell describes one visible face. Rectangles only grow across
        // identical material/light and uniform AO.
        let dimensions = extent;
        for (face_idx, (normal, corner_data)) in BLOCK_FACES.iter().enumerate() {
            let normal_axis = (0..3).find(|axis| normal[*axis] != 0).unwrap();
            let u_axis = (0..3)
                .find(|axis| corner_data[0].0[*axis] != corner_data[1].0[*axis])
                .unwrap();
            let v_axis = (0..3)
                .find(|axis| corner_data[0].0[*axis] != corner_data[3].0[*axis])
                .unwrap();
            let u_len = dimensions[u_axis];
            let v_len = dimensions[v_axis];

            for slice in 0..dimensions[normal_axis] {
                let mut mask = vec![None::<GreedyFace>; u_len * v_len];
                for v in 0..v_len {
                    for u in 0..u_len {
                        let mut local = [0usize; 3];
                        local[normal_axis] = slice;
                        local[u_axis] = u;
                        local[v_axis] = v;
                        let [x, y, z] = local;

                        let voxel = get_voxel(
                            origin[0] + x as i32,
                            origin[1] + y as i32,
                            origin[2] + z as i32,
                        );
                        let block = voxel.block;
                        if !is_greedy_cube(block) {
                            continue;
                        }

                        let world = [
                            origin[0] + x as i32,
                            origin[1] + y as i32,
                            origin[2] + z as i32,
                        ];
                        let nx = world[0] + normal[0];
                        let ny = world[1] + normal[1];
                        let nz = world[2] + normal[2];
                        let (
                            neighbor,
                            neighbor_sky,
                            neighbor_block,
                            neighbor_level,
                            neighbor_falling,
                        ) = get_block_at(nx, ny, nz);
                        if !face_should_render(
                            block,
                            face_idx,
                            0,
                            false,
                            neighbor,
                            neighbor_level,
                            neighbor_falling,
                        ) {
                            continue;
                        }

                        let multiplier_code = match face_idx {
                            4 => 0u16,
                            5 => 2u16,
                            _ => 1u16,
                        };
                        let light_level = neighbor_sky as u16
                            + neighbor_block as u16 * 16
                            + multiplier_code * 256;
                        let mut ao_levels = [0u8; 4];
                        for (corner_idx, (offset, _)) in corner_data.iter().enumerate() {
                            ao_levels[corner_idx] = ao_level(ambient_occlusion_for_vertex(
                                world,
                                *normal,
                                *offset,
                                &get_block_at,
                            ));
                        }

                        let fallback_tile = block.get_face_tex_index(face_idx);
                        let (tile_x, tile_y) = registry.map_or(fallback_tile, |registry| {
                            registry.atlas_tile_for_block(block, fallback_tile)
                        });
                        mask[v * u_len + u] = Some(GreedyFace {
                            block,
                            atlas_tile: (tile_x, tile_y),
                            light_level,
                            ao_levels,
                        });
                    }
                }

                for v in 0..v_len {
                    let mut u = 0;
                    while u < u_len {
                        let index = v * u_len + u;
                        let Some(face) = mask[index] else {
                            u += 1;
                            continue;
                        };

                        let mut width = 1;
                        if face
                            .ao_levels
                            .iter()
                            .all(|level| *level == face.ao_levels[0])
                        {
                            while u + width < u_len
                                && mask[v * u_len + u + width]
                                    .is_some_and(|other| face.can_merge_with(other))
                            {
                                width += 1;
                            }
                        }

                        let mut height = 1;
                        'grow_height: while v + height < v_len {
                            for offset in 0..width {
                                if !mask[(v + height) * u_len + u + offset]
                                    .is_some_and(|other| face.can_merge_with(other))
                                {
                                    break 'grow_height;
                                }
                            }
                            height += 1;
                        }

                        for row in 0..height {
                            for column in 0..width {
                                mask[(v + row) * u_len + u + column] = None;
                            }
                        }

                        let mut min_local = [0.0f32; 3];
                        min_local[normal_axis] = slice as f32;
                        min_local[u_axis] = u as f32;
                        min_local[v_axis] = v as f32;
                        let mut max_local =
                            [min_local[0] + 1.0, min_local[1] + 1.0, min_local[2] + 1.0];
                        max_local[u_axis] = min_local[u_axis] + width as f32;
                        max_local[v_axis] = min_local[v_axis] + height as f32;

                        let world_origin = [origin[0] as f32, origin[1] as f32, origin[2] as f32];
                        let mut positions = [[0.0f32; 3]; 4];
                        let mut local_uvs = [[0.0f32; 2]; 4];
                        for (corner_idx, (offset, uv)) in corner_data.iter().enumerate() {
                            for axis in 0..3 {
                                positions[corner_idx][axis] = world_origin[axis]
                                    + if offset[axis] == 0.0 {
                                        min_local[axis]
                                    } else {
                                        max_local[axis]
                                    };
                            }
                            local_uvs[corner_idx] = [uv[0] * width as f32, uv[1] * height as f32];
                        }

                        let (vertices, indices) =
                            if face.block.def().properties.render_type == RenderType::Translucent {
                                (&mut trans_vertices, &mut trans_indices)
                            } else {
                                (&mut opaque_vertices, &mut opaque_indices)
                            };
                        push_terrain_quad(
                            vertices,
                            indices,
                            positions,
                            local_uvs,
                            face.atlas_tile,
                            face.light_level as f32,
                            face.ao(),
                            region_coord,
                        );
                        u += width;
                    }
                }
            }
        }

        (
            opaque_vertices,
            opaque_indices,
            trans_vertices,
            trans_indices,
        )
    }

    /// Generates meshes for one section only. Blocks outside the requested
    /// 16-block Y interval are blanked in the meshing copy while the supplied
    /// lookup remains world-backed, preserving the one-cell halo semantics.
    pub fn generate_section_mesh_bundle<F>(
        &self,
        key: SectionKey,
        revision: u64,
        lifetime: u64,
        get_block_at: F,
    ) -> crate::chunk_render::SectionMeshBundle
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool) + Copy,
    {
        self.generate_section_mesh_bundle_inner(key, revision, lifetime, get_block_at, None)
    }

    /// Generates one section and its coarse LODs using immutable model-pack
    /// descriptors. The halo remains captured exactly once before dispatch.
    pub fn generate_section_mesh_bundle_with_registry<F>(
        &self,
        key: SectionKey,
        revision: u64,
        lifetime: u64,
        get_block_at: F,
        registry: &crate::block_model::ModelRegistry,
    ) -> crate::chunk_render::SectionMeshBundle
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool) + Copy,
    {
        self.generate_section_mesh_bundle_inner(
            key,
            revision,
            lifetime,
            get_block_at,
            Some(registry),
        )
    }

    pub(super) fn generate_section_mesh_bundle_inner<F>(
        &self,
        key: SectionKey,
        revision: u64,
        lifetime: u64,
        get_block_at: F,
        registry: Option<&crate::block_model::ModelRegistry>,
    ) -> crate::chunk_render::SectionMeshBundle
    where
        F: Fn(i32, i32, i32) -> (BlockType, u8, u8, u8, bool) + Copy,
    {
        assert_eq!((self.chunk_x, self.chunk_z), (key.cx, key.cz));
        assert!(self.section_index(key.section_y).is_some());
        // Materialize the immutable 18^3 halo up front. The worker lookup
        // below consults this snapshot for all block-occlusion decisions,
        // ensuring boundary/AO results are independent of later mutations.
        let halo = SectionHaloSnapshot::from_chunk(key, |wx, wy, wz| {
            if wx.div_euclid(CHUNK_WIDTH as i32) == key.cx
                && wz.div_euclid(CHUNK_DEPTH as i32) == key.cz
                && self.section_index(world_y_to_section_y(wy)).is_some()
            {
                let x = wx.rem_euclid(CHUNK_WIDTH as i32) as usize;
                let z = wz.rem_euclid(CHUNK_DEPTH as i32) as usize;
                MeshVoxel {
                    block: self.get_block_local(x, wy, z),
                    state: self.get_block_state(
                        wx - key.cx * CHUNK_WIDTH as i32,
                        wy,
                        wz - key.cz * CHUNK_DEPTH as i32,
                    ),
                    sky: self.get_sky_light(x, wy, z),
                    block_light: self.get_block_light(x, wy, z),
                    raw_fluid: self.get_fluid_level(x, wy, z),
                }
            } else {
                let (block, sky, block_light, level, falling) = get_block_at(wx, wy, wz);
                MeshVoxel {
                    block,
                    sky,
                    block_light,
                    raw_fluid: level | if falling { 8 } else { 0 },
                    ..MeshVoxel::default()
                }
            }
        });
        Self::generate_section_mesh_bundle_from_halo_inner(
            SectionIdentity::new(key, revision, lifetime),
            &halo,
            registry,
            LodLevel::MASK_ALL,
        )
    }

    /// Builds a section mesh exclusively from the immutable 18^3 worker
    /// snapshot. This is the runtime entry point; no live Chunk/WorldColumns
    /// state is consulted after dispatch.
    pub fn generate_section_mesh_bundle_from_halo(
        identity: SectionIdentity,
        halo: &SectionHaloSnapshot,
    ) -> crate::chunk_render::SectionMeshBundle {
        Self::generate_section_mesh_bundle_from_halo_inner(identity, halo, None, LodLevel::MASK_ALL)
    }

    /// Same worker entry, but only generates the requested LOD bits.
    pub fn generate_section_mesh_bundle_from_halo_for_lods(
        identity: SectionIdentity,
        halo: &SectionHaloSnapshot,
        lod_mask: u8,
    ) -> crate::chunk_render::SectionMeshBundle {
        Self::generate_section_mesh_bundle_from_halo_inner(identity, halo, None, lod_mask)
    }

    /// Worker-safe section mesh entry point with an immutable model registry.
    pub fn generate_section_mesh_bundle_from_halo_with_registry(
        identity: SectionIdentity,
        halo: &SectionHaloSnapshot,
        registry: &crate::block_model::ModelRegistry,
    ) -> crate::chunk_render::SectionMeshBundle {
        Self::generate_section_mesh_bundle_from_halo_inner(
            identity,
            halo,
            Some(registry),
            LodLevel::MASK_ALL,
        )
    }

    /// Same as the registry worker entry, but only generates the requested LOD bits.
    pub fn generate_section_mesh_bundle_from_halo_with_registry_for_lods(
        identity: SectionIdentity,
        halo: &SectionHaloSnapshot,
        registry: &crate::block_model::ModelRegistry,
        lod_mask: u8,
    ) -> crate::chunk_render::SectionMeshBundle {
        Self::generate_section_mesh_bundle_from_halo_inner(identity, halo, Some(registry), lod_mask)
    }

    pub(super) fn generate_section_mesh_bundle_from_halo_inner(
        identity: SectionIdentity,
        halo: &SectionHaloSnapshot,
        registry: Option<&crate::block_model::ModelRegistry>,
        lod_mask: u8,
    ) -> crate::chunk_render::SectionMeshBundle {
        let key = identity.key;
        debug_assert_eq!(halo.key, key);
        let lod_mask = if lod_mask == 0 {
            LodLevel::MASK_L0
        } else {
            lod_mask & LodLevel::MASK_ALL
        };
        let l0 = if LodLevel::L0.is_in(lod_mask) {
            let section_voxel = |wx: i32, wy: i32, wz: i32| {
                let hx = wx - key.cx * CHUNK_WIDTH as i32 + 1;
                let hy = wy - key.min_world_y() + 1;
                let hz = wz - key.cz * CHUNK_DEPTH as i32 + 1;
                if (0..SectionHaloSnapshot::SIDE as i32).contains(&hx)
                    && (0..SectionHaloSnapshot::SIDE as i32).contains(&hy)
                    && (0..SectionHaloSnapshot::SIDE as i32).contains(&hz)
                {
                    return halo.get(hx as usize, hy as usize, hz as usize);
                }
                MeshVoxel::default()
            };
            let origin = [
                key.cx * CHUNK_WIDTH as i32,
                key.min_world_y(),
                key.cz * CHUNK_DEPTH as i32,
            ];
            let (o, oi, t, ti) = Self::mesh_l0_volume_with_registry(
                origin,
                [CHUNK_WIDTH, SECTION_SIZE, CHUNK_DEPTH],
                section_voxel,
                registry,
            );
            let region_coord = crate::chunk_render::chunk_to_region_coord(key.cx, key.cz);
            ChunkLodMeshData::from_parts(o, oi, t, ti, region_coord)
        } else {
            ChunkLodMeshData::default()
        };
        let l1 = if LodLevel::L1.is_in(lod_mask) {
            Self::mesh_section_lod_from_halo_with_registry(key, halo, 2, registry)
        } else {
            ChunkLodMeshData::default()
        };
        let l2 = if LodLevel::L2.is_in(lod_mask) {
            Self::mesh_section_lod_from_halo_with_registry(key, halo, 4, registry)
        } else {
            ChunkLodMeshData::default()
        };
        let levels = [l0, l1, l2];
        let bounds = levels
            .iter()
            .filter_map(ChunkLodMeshData::bounds)
            .reduce(|a, b| a.union(b));
        crate::chunk_render::SectionMeshBundle {
            identity,
            levels,
            bounds,
            connectivity: crate::culling::compute_section_connectivity_snapshot(halo),
            built_lods: lod_mask,
        }
    }
}
