// Tests extracted from state.rs::render_region_lifecycle_tests (Plan 27).

use super::*;

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
