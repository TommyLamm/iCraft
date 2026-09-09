//! GPU terrain arena types: region buffers, section meshes, upload/compaction helpers.
//! Presentation-only derived caches. Authority chunks stay in ChunkManager / ServerWorld.

use crate::chunk_render::{LodLevel, MeshBounds};
use std::time::Instant;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct UploadMetrics {
    pub(crate) elapsed_ns: u64,
    pub(crate) bytes: u64,
}

impl UploadMetrics {
    pub(crate) fn add(self, other: Self) -> Self {
        Self {
            elapsed_ns: self.elapsed_ns.saturating_add(other.elapsed_ns),
            bytes: self.bytes.saturating_add(other.bytes),
        }
    }
}

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
            crate::chunk_render::REGION_ORIGIN_Y,
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

    pub(crate) fn handle_is_live(
        &self,
        handle: &crate::chunk_render::RegionAllocationHandle,
    ) -> bool {
        region_allocation_handle_is_live(
            self.region_instance_id,
            &self.vertex_freelist,
            &self.index_freelist,
            handle,
        )
    }

    pub(crate) fn empty_rebuild_worthwhile(&self) -> bool {
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

pub(crate) fn region_allocation_handle_is_live(
    region_instance_id: u64,
    vertex_freelist: &crate::chunk_render::FreeList,
    index_freelist: &crate::chunk_render::FreeList,
    handle: &crate::chunk_render::RegionAllocationHandle,
) -> bool {
    handle.region_instance_id == region_instance_id
        && vertex_freelist.validate_owned(handle.vertex_token).is_ok()
        && index_freelist.validate_owned(handle.index_token).is_ok()
}

pub(crate) fn should_decrement_region_active_chunks(
    mesh_has_resident_section: bool,
    mesh_has_allocation_handles: bool,
    mesh_has_matching_region_handle: bool,
) -> bool {
    mesh_has_resident_section && (!mesh_has_allocation_handles || mesh_has_matching_region_handle)
}

pub(crate) fn chunk_mesh_is_registered_with_region(
    mesh: &ChunkMesh,
    region: Option<&RenderRegion>,
) -> bool {
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

pub(crate) fn empty_region_rebuild_worthwhile(
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
    pub(crate) opaque: GpuMeshLayer,
    pub(crate) transparent: GpuMeshLayer,
    pub(crate) bounds: Option<MeshBounds>,
}

pub struct GpuSectionMesh {
    pub(crate) levels: Option<[GpuMeshLevel; 3]>,
    pub(crate) connectivity: crate::culling::SectionConnectivityState,
    pub(crate) revision: u64,
    pub(crate) meshed_revision: u64,
}

impl GpuSectionMesh {
    pub(crate) fn pending() -> Self {
        Self {
            levels: None,
            connectivity: crate::culling::SectionConnectivityState::Invalid,
            revision: 0,
            meshed_revision: u64::MAX,
        }
    }

    pub(crate) fn invalidate(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.connectivity = crate::culling::SectionConnectivityState::Invalid;
    }

    pub(crate) fn needs_rebuild(&self) -> bool {
        self.levels.is_none() || self.meshed_revision != self.revision
    }

    pub(crate) fn level(&self, lod: LodLevel) -> Option<&GpuMeshLevel> {
        self.levels.as_ref().map(|levels| &levels[lod as usize])
    }

    pub(crate) fn finest_bounds(&self) -> Option<MeshBounds> {
        self.level(LodLevel::L0).and_then(|level| level.bounds)
    }

    pub(crate) fn total_indices(&self) -> usize {
        self.levels
            .as_ref()
            .into_iter()
            .flatten()
            .map(|level| {
                level.opaque.num_indices() as usize + level.transparent.num_indices() as usize
            })
            .sum()
    }

    pub(crate) fn gpu_bytes(&self) -> usize {
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
    pub min_section_y: i8,
    pub(crate) sections: Vec<GpuSectionMesh>,
}

impl ChunkMesh {
    pub(crate) fn pending() -> Self {
        Self::pending_for_dimension(crate::dimension::Dimension::Overworld)
    }

    pub(crate) fn pending_for_dimension(dimension: crate::dimension::Dimension) -> Self {
        let height = dimension.height();
        Self::pending_for_height(height.min_section_y(), height.section_count())
    }

    pub(crate) fn pending_for_height(min_section_y: i8, section_count: usize) -> Self {
        Self {
            min_section_y,
            sections: (0..section_count)
                .map(|_| GpuSectionMesh::pending())
                .collect(),
        }
    }

    pub(crate) fn section_index(&self, section_y: i8) -> Option<usize> {
        let idx = (section_y as i32) - (self.min_section_y as i32);
        if idx >= 0 && (idx as usize) < self.sections.len() {
            Some(idx as usize)
        } else {
            None
        }
    }

    pub(crate) fn section_y_at_index(&self, index: usize) -> i8 {
        self.min_section_y + index as i8
    }

    pub(crate) fn section(&self, section_y: i8) -> Option<&GpuSectionMesh> {
        let idx = self.section_index(section_y)?;
        self.sections.get(idx)
    }

    pub(crate) fn section_mut(&mut self, section_y: i8) -> Option<&mut GpuSectionMesh> {
        let idx = self.section_index(section_y)?;
        self.sections.get_mut(idx)
    }

    pub(crate) fn finest_bounds(&self) -> Option<MeshBounds> {
        self.sections
            .iter()
            .filter_map(GpuSectionMesh::finest_bounds)
            .reduce(|left, right| left.union(right))
    }

    pub(crate) fn total_indices(&self) -> usize {
        self.sections
            .iter()
            .map(GpuSectionMesh::total_indices)
            .sum()
    }

    pub(crate) fn gpu_bytes(&self) -> usize {
        self.sections.iter().map(GpuSectionMesh::gpu_bytes).sum()
    }

    pub(crate) fn has_resident_section(&self) -> bool {
        self.sections.iter().any(|section| section.levels.is_some())
    }

    pub(crate) fn allocation_handle_region_membership(
        &self,
        region_instance_id: u64,
    ) -> (bool, bool) {
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
