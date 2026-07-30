//! Small, allocation-free CPU performance instrumentation.
//!
//! `PerfRecorder` is intentionally a value type. A caller can keep one in its
//! state and use [`PerfRecorder::record`] on hot paths. Samples are nanoseconds
//! and overwrite the oldest sample when the fixed-capacity history is full.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

pub struct AllocTracker;

pub static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static THREAD_ALLOC_COUNT: Cell<u64> = const { Cell::new(0) };
}

#[inline]
fn increment_thread_alloc_count() {
    THREAD_ALLOC_COUNT.with(|count| count.set(count.get().wrapping_add(1)));
}

unsafe impl GlobalAlloc for AllocTracker {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        increment_thread_alloc_count();
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        increment_thread_alloc_count();
        System.alloc_zeroed(layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        increment_thread_alloc_count();
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: AllocTracker = AllocTracker;

pub fn alloc_count() -> u64 {
    ALLOC_COUNT.load(Ordering::Relaxed)
}

pub fn thread_alloc_count() -> u64 {
    THREAD_ALLOC_COUNT.with(Cell::get)
}

pub const SCOPE_COUNT: usize = 17;
pub const DEFAULT_HISTORY_CAPACITY: usize = 256;

/// Fine-grained lighting work buckets used by the frame HUD and telemetry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum LightingSource {
    Load,
    Block,
    Fluid,
    Weather,
    Redstone,
}

impl LightingSource {
    pub const COUNT: usize = 5;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Load,
        Self::Block,
        Self::Fluid,
        Self::Weather,
        Self::Redstone,
    ];
    pub const fn name(self) -> &'static str {
        match self {
            Self::Load => "load",
            Self::Block => "block",
            Self::Fluid => "fluid",
            Self::Weather => "weather",
            Self::Redstone => "redstone",
        }
    }
}

/// GPU buffer write buckets used by the frame HUD and telemetry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum UploadSource {
    Camera,
    Ui,
    Crack,
    Particle,
    Entity,
    Terrain,
}

impl UploadSource {
    pub const COUNT: usize = 6;
    pub const ALL: [Self; Self::COUNT] = [
        Self::Camera,
        Self::Ui,
        Self::Crack,
        Self::Particle,
        Self::Entity,
        Self::Terrain,
    ];
    pub const fn name(self) -> &'static str {
        match self {
            Self::Camera => "camera",
            Self::Ui => "ui",
            Self::Crack => "crack",
            Self::Particle => "particle",
            Self::Entity => "entity",
            Self::Terrain => "terrain",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScopeAccumulator<const N: usize> {
    nanos: [u64; N],
}

impl<const N: usize> Default for ScopeAccumulator<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> ScopeAccumulator<N> {
    pub const fn new() -> Self {
        Self { nanos: [0; N] }
    }
    #[inline]
    pub fn record_nanos(&mut self, index: usize, nanos: u64) {
        if index < N {
            self.nanos[index] = self.nanos[index].saturating_add(nanos);
        }
    }
    #[inline]
    pub fn record(&mut self, index: usize, elapsed: Duration) {
        self.record_nanos(index, elapsed.as_nanos().min(u64::MAX as u128) as u64);
    }
    pub const fn get(&self, index: usize) -> Option<u64> {
        if index < N {
            Some(self.nanos[index])
        } else {
            None
        }
    }
    pub fn reset(&mut self) {
        self.nanos = [0; N];
    }
    pub fn is_zero(&self) -> bool {
        self.nanos == [0; N]
    }
    pub const fn values(&self) -> [u64; N] {
        self.nanos
    }
    pub fn to_vec(&self) -> Vec<u64> {
        self.nanos.to_vec()
    }
}

pub type LightingPerfSample = ScopeAccumulator<{ LightingSource::COUNT }>;
pub type GpuUploadPerfSample = ScopeAccumulator<{ UploadSource::COUNT }>;
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
    pub save_queue_bytes: u64,
    pub save_in_flight: u64,
    pub save_in_flight_bytes: u64,
    pub save_drop: u64,
    pub network_queue_depth: u64,
    /// Reliable FIFO events waiting in the persistent inbound inbox.
    pub network_inbound_reliable_pending: u64,
    pub network_inbound_reliable_bytes: u64,
    /// Latest-wins entries waiting for their per-key application pass.
    pub network_inbound_latest_pending: u64,
    pub network_inbound_latest_bytes: u64,
    pub network_catchup_mailbox_full: u64,
    pub prediction_rollback: u64,
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
    /// Timestamp queries may be supported while pass-local writes are not;
    /// in that case per-pass values are intentionally unavailable (N/A).
    pub gpu_timestamps_inside_passes: bool,
}

/// Machine-readable, per-frame performance sample.  Optional fields remain
/// `None` when a backend cannot provide the corresponding measurement.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct FramePerfSample {
    pub frame_id: u64,
    pub cpu_scopes_ns: std::collections::BTreeMap<String, u64>,
    pub gpu_scopes_ns: Option<std::collections::BTreeMap<String, u64>>,
    pub allocations: Option<u64>,
    pub upload_bytes: u64,
    pub draw_calls: u64,
    pub buffer_bytes: u64,
    pub culling: Option<CullingPerfSample>,
    pub queues: QueuePerfSample,
    pub checksum: Option<u64>,
    /// Per-source CPU lighting timings; `None` means the backend did not expose them.
    pub lighting: Option<Vec<u64>>,
    /// Per-source queue write timings; `None` means upload timing is unavailable.
    pub gpu_uploads: Option<Vec<u64>>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct CullingPerfSample {
    pub terrain_candidates: u64,
    pub visible_chunks: u64,
    pub occluded_chunks: u64,
    pub rendered_entities: u64,
    pub frustum_culled_entities: u64,
    pub occlusion_culled_entities: u64,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct QueuePerfSample {
    #[serde(default)]
    pub categories: std::collections::BTreeMap<QueueCategory, QueueCategorySample>,
    pub inbound_pending: Option<u64>,
    pub inbound_pending_bytes: Option<u64>,
    pub inbound_reliable_pending: Option<u64>,
    pub inbound_reliable_bytes: Option<u64>,
    pub inbound_latest_pending: Option<u64>,
    pub inbound_latest_bytes: Option<u64>,
    pub outbound_pending: Option<u64>,
    pub outbound_bytes: Option<u64>,
    pub reliable_pending: Option<u64>,
    pub reliable_bytes: Option<u64>,
    pub catchup_pending: Option<u64>,
    pub catchup_bytes: Option<u64>,
    pub save_queued_bytes: Option<u64>,
    pub save_in_flight_bytes: Option<u64>,
    pub retries: Option<u64>,
    pub drops: Option<u64>,
    pub cancels: Option<u64>,
    /// Oldest item age at the end of the frame, in milliseconds.
    pub oldest_age_ms: Option<u64>,
}

/// Queue categories used by network and persistence telemetry.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum QueueCategory {
    Inbound,
    Outbound,
    Reliable,
    CatchUp,
    SaveProducer,
    SaveWorker,
}

impl QueueCategory {
    pub const ALL: [Self; 6] = [
        Self::Inbound,
        Self::Outbound,
        Self::Reliable,
        Self::CatchUp,
        Self::SaveProducer,
        Self::SaveWorker,
    ];
    const fn index(self) -> usize {
        match self {
            Self::Inbound => 0,
            Self::Outbound => 1,
            Self::Reliable => 2,
            Self::CatchUp => 3,
            Self::SaveProducer => 4,
            Self::SaveWorker => 5,
        }
    }
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct QueueCategorySample {
    pub depth: u64,
    pub bytes: u64,
    pub oldest_age_ms: u64,
    pub drops: u64,
    pub retries: u64,
    pub cancels: u64,
}

/// A lock-free, cross-thread queue accounting primitive.
///
/// Producers call `enqueue` after ownership is accepted by the queue and
/// consumers call `dequeue` only after removing an item.  Thus depth and bytes
/// describe real backlog rather than attempted sends.  `oldest_enqueue_*`
/// tracks the oldest outstanding item using a monotonic sequence; it is safe
/// for concurrent producers and consumers and intentionally allows a
/// conservative age during races.
#[derive(Debug)]
pub struct SharedQueueStats {
    depth: AtomicU64,
    bytes: AtomicU64,
    oldest_enqueue_ms: AtomicU64,
    oldest_sequence: AtomicU64,
    next_sequence: AtomicU64,
    drops: AtomicU64,
    retries: AtomicU64,
    cancels: AtomicU64,
}

impl Default for SharedQueueStats {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedQueueStats {
    pub const fn new() -> Self {
        Self {
            depth: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            oldest_enqueue_ms: AtomicU64::new(0),
            oldest_sequence: AtomicU64::new(0),
            next_sequence: AtomicU64::new(1),
            drops: AtomicU64::new(0),
            retries: AtomicU64::new(0),
            cancels: AtomicU64::new(0),
        }
    }

    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    /// Clear all process-global queue accounting at a network-world boundary.
    /// Channels are owned by the world, so retaining backlog from a previous
    /// world would make the next F3 sample misleading.
    pub fn reset(&self) {
        self.depth.store(0, Ordering::Release);
        self.bytes.store(0, Ordering::Release);
        self.oldest_enqueue_ms.store(0, Ordering::Release);
        self.oldest_sequence.store(0, Ordering::Release);
        self.next_sequence.store(1, Ordering::Release);
        self.drops.store(0, Ordering::Release);
        self.retries.store(0, Ordering::Release);
        self.cancels.store(0, Ordering::Release);
    }

    pub fn enqueue(&self, bytes: u64, enqueue_ms: u64) {
        let sequence = self.next_sequence.fetch_add(1, Ordering::Relaxed);
        self.depth.fetch_add(1, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
        let current = self.oldest_sequence.load(Ordering::Acquire);
        if current == 0 {
            let _ = self.oldest_sequence.compare_exchange(
                0,
                sequence,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            self.oldest_enqueue_ms
                .compare_exchange(0, enqueue_ms, Ordering::AcqRel, Ordering::Acquire)
                .ok();
        }
    }

    pub fn dequeue(&self, bytes: u64) {
        decrement_saturating(&self.depth, 1);
        decrement_saturating(&self.bytes, bytes);
        if self.depth.load(Ordering::Acquire) == 0 {
            self.oldest_sequence.store(0, Ordering::Release);
            self.oldest_enqueue_ms.store(0, Ordering::Release);
        }
    }

    pub fn drop_item(&self) {
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
    pub fn retry(&self) {
        self.retries.fetch_add(1, Ordering::Relaxed);
    }
    pub fn cancel(&self) {
        self.cancels.fetch_add(1, Ordering::Relaxed);
    }
    pub fn depth(&self) -> u64 {
        self.depth.load(Ordering::Acquire)
    }
    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Acquire)
    }
    pub fn drops(&self) -> u64 {
        self.drops.load(Ordering::Relaxed)
    }
    pub fn retries(&self) -> u64 {
        self.retries.load(Ordering::Relaxed)
    }
    pub fn cancels(&self) -> u64 {
        self.cancels.load(Ordering::Relaxed)
    }
    pub fn oldest_age_ms(&self, now_ms: u64) -> u64 {
        let started = self.oldest_enqueue_ms.load(Ordering::Acquire);
        if started == 0 {
            0
        } else {
            now_ms.saturating_sub(started)
        }
    }
}

/// Process-wide bridge for channels whose ownership is split between the
/// synchronous game thread and Tokio workers. It avoids changing protocol
/// message layouts while still giving F3 a shared accounting source.
pub fn queue_stats(category: QueueCategory) -> Arc<SharedQueueStats> {
    static STATS: OnceLock<[Arc<SharedQueueStats>; 6]> = OnceLock::new();
    Arc::clone(
        &STATS.get_or_init(|| std::array::from_fn(|_| SharedQueueStats::shared()))
            [category.index()],
    )
}

pub fn reset_network_queue_stats() {
    for category in QueueCategory::ALL {
        queue_stats(category).reset();
    }
}

pub fn queue_category_sample(category: QueueCategory, now_ms: u64) -> QueueCategorySample {
    let stats = queue_stats(category);
    QueueCategorySample {
        depth: stats.depth(),
        bytes: stats.bytes(),
        oldest_age_ms: stats.oldest_age_ms(now_ms),
        drops: stats.drops(),
        retries: stats.retries(),
        cancels: stats.cancels(),
    }
}

/// Account a synchronous producer only after `send` accepts ownership.
pub fn tracked_send<T>(
    tx: &std::sync::mpsc::Sender<T>,
    value: T,
    bytes: u64,
    stats: &SharedQueueStats,
) -> Result<(), std::sync::mpsc::SendError<T>> {
    // std::sync::mpsc has no reservation API. Account before publishing so a
    // racing consumer can never dequeue an item that telemetry has not seen.
    // A closed channel returns ownership and rolls the provisional entry back.
    stats.enqueue(bytes, monotonic_millis());
    match tx.send(value) {
        Ok(()) => Ok(()),
        Err(error) => {
            stats.dequeue(bytes);
            stats.drop_item();
            Err(error)
        }
    }
}

/// Account a synchronous consumer after removing an item from the channel.
pub fn tracked_try_recv<T>(
    rx: &std::sync::mpsc::Receiver<T>,
    bytes: u64,
    stats: &SharedQueueStats,
) -> Result<T, std::sync::mpsc::TryRecvError> {
    let result = rx.try_recv();
    if result.is_ok() {
        stats.dequeue(bytes);
    }
    result
}

fn monotonic_millis() -> u64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(u64::MAX as u128) as u64
        })
}

fn decrement_saturating(counter: &AtomicU64, amount: u64) {
    let mut current = counter.load(Ordering::Acquire);
    loop {
        let next = current.saturating_sub(amount);
        match counter.compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

/// Percentile over a bounded frame ring; returns zero for an empty history.
pub fn frame_percentile<F>(
    samples: &std::collections::VecDeque<FramePerfSample>,
    percent: usize,
    value: F,
) -> u64
where
    F: Fn(&FramePerfSample) -> u64,
{
    let mut values: Vec<u64> = samples.iter().map(value).collect();
    if values.is_empty() {
        return 0;
    }
    values.sort_unstable();
    let rank = ((values.len() * percent) + 99) / 100;
    values[rank.saturating_sub(1).min(values.len() - 1)]
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
    fn thread_alloc_count_is_local_to_calling_thread() {
        let handle = std::thread::spawn(|| {
            let _allocation = Box::new([0u8; 64]);
        });
        let caller_after_spawn = thread_alloc_count();
        handle.join().unwrap();
        assert_eq!(thread_alloc_count(), caller_after_spawn);

        let before = thread_alloc_count();
        let _allocation = Box::new([0u8; 64]);
        assert!(thread_alloc_count() > before);
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

    #[test]
    fn scope_accumulator_records_all_categories_and_resets() {
        let mut lighting = LightingPerfSample::new();
        for source in LightingSource::ALL {
            lighting.record_nanos(source as usize, source as u64 + 1);
        }
        assert_eq!(lighting.values(), [1, 2, 3, 4, 5]);
        lighting.reset();
        assert!(lighting.is_zero());

        let mut uploads = GpuUploadPerfSample::new();
        for source in UploadSource::ALL {
            uploads.record_nanos(source as usize, 10);
        }
        assert_eq!(uploads.get(UploadSource::Terrain as usize), Some(10));
        assert_eq!(uploads.get(UploadSource::COUNT), None);
        uploads.reset();
        assert_eq!(uploads.get(UploadSource::Camera as usize), Some(0));
    }

    #[test]
    fn frame_percentile_uses_bounded_history() {
        let mut samples = std::collections::VecDeque::new();
        for n in 1..=100u64 {
            samples.push_back(FramePerfSample {
                frame_id: n,
                upload_bytes: n,
                ..Default::default()
            });
        }
        assert_eq!(frame_percentile(&samples, 95, |s| s.upload_bytes), 95);
        assert_eq!(frame_percentile(&samples, 99, |s| s.upload_bytes), 99);
    }

    #[test]
    fn shared_queue_stats_tracks_backlog_age_and_terminal_counters() {
        let stats = SharedQueueStats::new();
        stats.enqueue(12, 100);
        stats.enqueue(8, 120);
        assert_eq!(stats.depth(), 2);
        assert_eq!(stats.bytes(), 20);
        assert_eq!(stats.oldest_age_ms(150), 50);
        stats.retry();
        stats.drop_item();
        stats.cancel();
        assert_eq!(stats.retries(), 1);
        assert_eq!(stats.drops(), 1);
        assert_eq!(stats.cancels(), 1);
        stats.dequeue(12);
        assert_eq!(stats.depth(), 1);
        assert_eq!(stats.bytes(), 8);
        stats.dequeue(8);
        assert_eq!(stats.depth(), 0);
        assert_eq!(stats.oldest_age_ms(500), 0);
    }

    #[test]
    fn shared_queue_stats_reset_clears_world_lifecycle_state() {
        let stats = SharedQueueStats::new();
        stats.enqueue(32, 10);
        stats.drop_item();
        stats.retry();
        stats.cancel();
        stats.reset();
        assert_eq!(stats.depth(), 0);
        assert_eq!(stats.bytes(), 0);
        assert_eq!(stats.drops(), 0);
        assert_eq!(stats.retries(), 0);
        assert_eq!(stats.cancels(), 0);
        assert_eq!(stats.oldest_age_ms(100), 0);
    }

    #[test]
    fn category_bank_is_independent() {
        reset_network_queue_stats();
        queue_stats(QueueCategory::Inbound).enqueue(7, 10);
        queue_stats(QueueCategory::Reliable).drop_item();
        assert_eq!(queue_stats(QueueCategory::Inbound).depth(), 1);
        assert_eq!(queue_stats(QueueCategory::Inbound).drops(), 0);
        assert_eq!(queue_stats(QueueCategory::Reliable).depth(), 0);
        assert_eq!(queue_stats(QueueCategory::Reliable).drops(), 1);
        reset_network_queue_stats();
    }

    #[test]
    fn frame_percentile_uses_240_sample_ring_window() {
        let mut samples = std::collections::VecDeque::with_capacity(240);
        for n in 1..=240u64 {
            if samples.len() == 240 {
                samples.pop_front();
            }
            samples.push_back(FramePerfSample {
                frame_id: n,
                upload_bytes: n,
                ..Default::default()
            });
        }
        assert_eq!(samples.len(), 240);
        assert_eq!(frame_percentile(&samples, 95, |s| s.upload_bytes), 228);
        assert_eq!(frame_percentile(&samples, 99, |s| s.upload_bytes), 238);
    }
}
