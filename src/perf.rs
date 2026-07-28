//! Small, allocation-free CPU performance instrumentation.
//!
//! `PerfRecorder` is intentionally a value type. A caller can keep one in its
//! state and use [`PerfRecorder::record`] on hot paths. Samples are nanoseconds
//! and overwrite the oldest sample when the fixed-capacity history is full.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub struct AllocTracker;

pub static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for AllocTracker {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        System.alloc_zeroed(layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: AllocTracker = AllocTracker;

pub fn alloc_count() -> u64 {
    ALLOC_COUNT.load(Ordering::Relaxed)
}

pub const SCOPE_COUNT: usize = 17;
pub const DEFAULT_HISTORY_CAPACITY: usize = 256;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum ScopeId {
    NetworkDrain,
    WorldTick,
    PlayerPhysics,
    ChunkSchedule,
    TerrainResultIntegrate,
    Lighting,
    Redstone,
    HostileMobs,
    PassiveMobs,
    ParticlesUpdate,
    RenderPrepareTerrain,
    RenderPrepareEntities,
    RenderPrepareParticles,
    RenderPrepareUi,
    GpuUpload,
    RenderEncode,
    Present,
}

impl ScopeId {
    pub const ALL: [Self; SCOPE_COUNT] = [
        Self::NetworkDrain,
        Self::WorldTick,
        Self::PlayerPhysics,
        Self::ChunkSchedule,
        Self::TerrainResultIntegrate,
        Self::Lighting,
        Self::Redstone,
        Self::HostileMobs,
        Self::PassiveMobs,
        Self::ParticlesUpdate,
        Self::RenderPrepareTerrain,
        Self::RenderPrepareEntities,
        Self::RenderPrepareParticles,
        Self::RenderPrepareUi,
        Self::GpuUpload,
        Self::RenderEncode,
        Self::Present,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::NetworkDrain => "network_drain",
            Self::WorldTick => "world_tick",
            Self::PlayerPhysics => "player_physics",
            Self::ChunkSchedule => "chunk_schedule",
            Self::TerrainResultIntegrate => "terrain_result_integrate",
            Self::Lighting => "lighting",
            Self::Redstone => "redstone",
            Self::HostileMobs => "hostile_mobs",
            Self::PassiveMobs => "passive_mobs",
            Self::ParticlesUpdate => "particles_update",
            Self::RenderPrepareTerrain => "render_prepare_terrain",
            Self::RenderPrepareEntities => "render_prepare_entities",
            Self::RenderPrepareParticles => "render_prepare_particles",
            Self::RenderPrepareUi => "render_prepare_ui",
            Self::GpuUpload => "gpu_upload",
            Self::RenderEncode => "render_encode",
            Self::Present => "present",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScopeSummary {
    pub name: &'static str,
    pub samples: u64,
    pub average_nanos: u64,
    pub p95_nanos: u64,
    pub p99_nanos: u64,
}

impl ScopeSummary {
    #[inline]
    pub const fn sample_count(self) -> u64 {
        self.samples
    }
    #[inline]
    pub const fn average(self) -> u64 {
        self.average_nanos
    }
    #[inline]
    pub const fn p95(self) -> u64 {
        self.p95_nanos
    }
    #[inline]
    pub const fn p99(self) -> u64 {
        self.p99_nanos
    }
}

#[derive(Clone, Copy)]
pub struct PerfRecorder<const CAPACITY: usize = DEFAULT_HISTORY_CAPACITY> {
    samples: [[u64; CAPACITY]; SCOPE_COUNT],
    next: [usize; SCOPE_COUNT],
    counts: [u64; SCOPE_COUNT],
}

impl<const CAPACITY: usize> Default for PerfRecorder<CAPACITY> {
    fn default() -> Self {
        Self {
            samples: [[0; CAPACITY]; SCOPE_COUNT],
            next: [0; SCOPE_COUNT],
            counts: [0; SCOPE_COUNT],
        }
    }
}

impl<const CAPACITY: usize> PerfRecorder<CAPACITY> {
    pub const fn new() -> Self {
        Self {
            samples: [[0; CAPACITY]; SCOPE_COUNT],
            next: [0; SCOPE_COUNT],
            counts: [0; SCOPE_COUNT],
        }
    }

    /// Records one sample without allocating. A zero-capacity recorder is a no-op.
    #[inline]
    pub fn record_nanos(&mut self, scope: ScopeId, nanos: u64) {
        if CAPACITY == 0 {
            return;
        }
        let i = scope as usize;
        self.samples[i][self.next[i]] = nanos;
        self.next[i] = (self.next[i] + 1) % CAPACITY;
        self.counts[i] = self.counts[i].saturating_add(1);
    }

    #[inline]
    pub fn record(&mut self, scope: ScopeId, elapsed: Duration) {
        self.record_nanos(scope, elapsed.as_nanos().min(u64::MAX as u128) as u64);
    }

    pub fn snapshot(&self) -> [ScopeSummary; SCOPE_COUNT] {
        let mut result = [ScopeSummary::default(); SCOPE_COUNT];
        let mut i = 0;
        while i < SCOPE_COUNT {
            result[i] = self.summary(ScopeId::ALL[i]);
            i += 1;
        }
        result
    }

    pub fn summary(&self, scope: ScopeId) -> ScopeSummary {
        let i = scope as usize;
        let count = self.counts[i].min(CAPACITY as u64) as usize;
        if count == 0 {
            return ScopeSummary {
                name: scope.name(),
                ..ScopeSummary::default()
            };
        }
        let mut values = [0u64; CAPACITY];
        let start = if self.counts[i] >= CAPACITY as u64 {
            self.next[i]
        } else {
            0
        };
        let mut n = 0;
        let mut total = 0u128;
        while n < count {
            let value = self.samples[i][(start + n) % CAPACITY];
            values[n] = value;
            total += value as u128;
            n += 1;
        }
        values[..count].sort_unstable();
        ScopeSummary {
            name: scope.name(),
            samples: count as u64,
            average_nanos: (total / count as u128).min(u64::MAX as u128) as u64,
            p95_nanos: percentile(&values[..count], 95),
            p99_nanos: percentile(&values[..count], 99),
        }
    }
}

fn percentile(values: &[u64], percent: usize) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let rank = ((values.len() * percent) + 99) / 100;
    values[rank.saturating_sub(1).min(values.len() - 1)]
}

/// Fixed-field counters suitable for a HUD/debug overlay. Values are caller-defined units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PerfCounters {
    pub cpu_mesh_bytes: u64,
    pub gpu_mesh_bytes: u64,
    pub gpu_arena_used_bytes: u64,
    pub gpu_arena_wasted_bytes: u64,
    pub gpu_arena_regions: u64,
    pub gpu_buffer_objects: u64,
    pub loaded_chunks: u64,
    pub visible_chunks: u64,
    pub occluded_chunks: u64,
    pub terrain_candidates: u64,
    pub terrain_triangles: u64,
    pub draw_calls: u64,
    pub rendered_entities: u64,
    pub frustum_culled_entities: u64,
    pub occlusion_culled_entities: u64,
    pub worker_queue: u64,
    pub in_flight: u64,
    pub cancelled: u64,
    pub stale_results: u64,
    pub upload_bytes_frame: u64,
    pub save_queue_depth: u64,
    pub network_queue_depth: u64,
    pub loaded_region_cache_bytes: u64,
    pub frame_allocations: u64,
    pub gpu_sky_ns: u64,
    pub gpu_opaque_ns: u64,
    pub gpu_mobs_ns: u64,
    pub gpu_translucent_ns: u64,
    pub gpu_particles_ns: u64,
    pub gpu_crack_ns: u64,
    pub gpu_ui_ns: u64,
    pub gpu_timestamps_supported: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_history() {
        let p = PerfRecorder::<4>::new().summary(ScopeId::Lighting);
        assert_eq!(p.samples, 0);
        assert_eq!(p.average_nanos, 0);
    }
    #[test]
    fn percentile_math() {
        let mut p = PerfRecorder::<8>::new();
        for n in [1, 2, 3, 4] {
            p.record_nanos(ScopeId::Lighting, n);
        }
        let s = p.summary(ScopeId::Lighting);
        assert_eq!(s.average_nanos, 2);
        assert_eq!(s.p95_nanos, 4);
        assert_eq!(s.p99_nanos, 4);
    }
    #[test]
    fn ring_wrapping_keeps_latest() {
        let mut p = PerfRecorder::<3>::new();
        for n in 1..=5 {
            p.record_nanos(ScopeId::Lighting, n);
        }
        let s = p.summary(ScopeId::Lighting);
        assert_eq!(s.samples, 3);
        assert_eq!(s.average_nanos, 4);
        assert_eq!(s.p95_nanos, 5);
    }
}
