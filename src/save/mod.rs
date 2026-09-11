pub mod format;
pub mod index;
pub mod player;
pub mod region;

#[cfg(test)]
mod tests;

use crate::dimension::Dimension;
use crate::network::protocol::PlayerEffectWire;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

pub use crate::inventory::{CreativeDragOrigin, GameMode};
pub use format::{
    load_world_creation_options, ChunkSaveData, DedicatedPlayerFile, EntitySaveData, IdentityError,
    InventoryData, ItemStackData, LevelData, MutationRevisionIndexCapacityError,
    NetworkTerrainPayload, PlayerData, SaveError, SaveResult, CHUNK_SAVE_DATA_VERSION,
    DEDICATED_PLAYER_SAVE_VERSION, PLAYER_IDENTITY_MAX_LEN, PLAYER_SAVE_MAGIC, PLAYER_SAVE_VERSION,
    WORLD_META_FILE,
};
pub use index::{
    default_mutation_revision_index_capacity, DirtyChunkSet, MutationRevisionIndex, SaveState,
    MUTATION_REVISION_INDEX_CAPACITY,
};
pub use player::normalize_player_identity;
pub use region::{
    atomic_write, compress_bytes, decompress_bytes, decompress_bytes_limited, RegionData,
};

/// Read-only `dimension.dat` peek. Embedded presentation uses this so it can
/// start in the last persisted dimension without constructing a presentation
/// `SaveManager`.
pub fn peek_current_dimension(world_dir: &Path) -> Dimension {
    match fs::read(world_dir.join("dimension.dat"))
        .ok()
        .and_then(|bytes| bytes.first().copied())
    {
        Some(1) => Dimension::Nether,
        Some(2) => Dimension::End,
        _ => Dimension::Overworld,
    }
}

pub struct SaveManager {
    pub world_dir: PathBuf,
    region_cache: HashMap<(Dimension, i32, i32), RegionData>,
    /// Serialized on-disk length last observed for a cached region. Write
    /// hits the cache when this still matches `metadata.len()`, so an
    /// externally truncated/corrupt file still fail-closes.
    region_disk_len: HashMap<(Dimension, i32, i32), u64>,
    lru_order: VecDeque<(Dimension, i32, i32)>,
}

impl SaveManager {
    pub fn new<P: AsRef<Path>>(world_dir: P) -> Self {
        let world_dir = world_dir.as_ref().to_path_buf();
        let regions_dir = world_dir.join("regions");
        if !regions_dir.exists() {
            fs::create_dir_all(&regions_dir).unwrap();
        }
        for name in ["nether", "end"] {
            let path = world_dir.join("dimensions").join(name).join("regions");
            if !path.exists() {
                fs::create_dir_all(path).unwrap();
            }
        }
        Self {
            world_dir,
            region_cache: HashMap::new(),
            region_disk_len: HashMap::new(),
            lru_order: VecDeque::new(),
        }
    }

    fn touch_region(&mut self, key: (Dimension, i32, i32)) {
        if !self.region_cache.contains_key(&key) {
            if let Some(pos) = self
                .lru_order
                .iter()
                .position(|candidate| candidate == &key)
            {
                self.lru_order.remove(pos);
            }
            return;
        }
        if let Some(pos) = self.lru_order.iter().position(|k| k == &key) {
            self.lru_order.remove(pos);
        }
        self.lru_order.push_back(key);
    }

    fn evict_lru_regions(&mut self) {
        const MAX_ENTRIES: usize = 16;
        const MAX_BYTES: u64 = 64 * 1024 * 1024;

        while self.region_cache.len() > MAX_ENTRIES || self.region_cache_bytes() > MAX_BYTES {
            if let Some(lru_key) = self.lru_order.pop_front() {
                self.region_cache.remove(&lru_key);
                self.region_disk_len.remove(&lru_key);
            } else {
                break;
            }
        }
    }

    pub fn region_cache_bytes(&self) -> u64 {
        self.region_cache
            .values()
            .map(|region| region.chunks.values().map(|v| v.len() as u64).sum::<u64>())
            .sum()
    }

    fn region_dir(&self, dimension: Dimension) -> PathBuf {
        match dimension {
            Dimension::Overworld => self.world_dir.join("regions"),
            Dimension::Nether => self
                .world_dir
                .join("dimensions")
                .join("nether")
                .join("regions"),
            Dimension::End => self
                .world_dir
                .join("dimensions")
                .join("end")
                .join("regions"),
        }
    }

    pub fn entities_file_path(&self, dimension: Dimension) -> PathBuf {
        match dimension {
            Dimension::Overworld => self.world_dir.join("entities.dat"),
            Dimension::Nether => self
                .world_dir
                .join("dimensions")
                .join("nether")
                .join("entities.dat"),
            Dimension::End => self
                .world_dir
                .join("dimensions")
                .join("end")
                .join("entities.dat"),
        }
    }

    pub fn save_entities_in(
        &self,
        dimension: Dimension,
        entities: &[EntitySaveData],
    ) -> io::Result<()> {
        let path = self.entities_file_path(dimension);
        let bytes =
            bincode::serialize(entities).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        atomic_write(path, &bytes)
    }

    /// Checked entity load used by the authority runtime. The historical
    /// `load_entities_in` API intentionally degrades corrupt optional entity
    /// files to an empty list for renderer callers; dedicated authority must
    /// instead surface the error so the caller can retry without losing state.
    pub fn load_entities_in_checked(
        &self,
        dimension: Dimension,
    ) -> io::Result<Vec<EntitySaveData>> {
        let path = self.entities_file_path(dimension);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let bytes = fs::read(&path)?;
        bincode::deserialize(&bytes).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("entity save decode failed for {}: {error}", path.display()),
            )
        })
    }

    /// Return the path for a dedicated player identity.
    /// Handshake, whitelist, operators, and this file all use the same
    /// normalized key; mutating names such as `foo.bar` are rejected instead
    /// of being rewritten onto `foo_bar.dat`.
    pub fn dedicated_player_file_path(&self, username: &str) -> Result<PathBuf, IdentityError> {
        player::dedicated_player_file_path(&self.world_dir, username)
    }

    /// Atomically save a dedicated player payload. A failed replacement does
    /// not touch the previous file, so the caller can retry with the same
    /// snapshot after reporting the error to the session manager.
    pub fn save_dedicated_player(
        &self,
        username: &str,
        current_dimension: Dimension,
        data: &PlayerData,
        effects: &[PlayerEffectWire],
    ) -> io::Result<()> {
        player::save_dedicated_player(&self.world_dir, username, current_dimension, data, effects)
    }

    /// Load a dedicated player payload, migrating the version-1 runtime file
    /// (which did not store current dimension) to the explicit Overworld or
    /// saved spawn dimension default.
    pub fn load_dedicated_player(&self, username: &str) -> io::Result<Option<DedicatedPlayerFile>> {
        player::load_dedicated_player(&self.world_dir, username)
    }

    /// Enumerate all readable chunk payloads for a dimension. Region files are
    /// scanned in a deterministic order and bounded to avoid turning a
    /// malformed save directory into an unbounded allocation.
    pub fn load_saved_chunks_in(&self, dimension: Dimension) -> io::Result<Vec<ChunkSaveData>> {
        const MAX_REGION_FILES: usize = 65_536;
        const MAX_CHUNKS: usize = 1_000_000;
        let directory = self.region_dir(dimension);
        if !directory.exists() {
            return Ok(Vec::new());
        }
        let mut region_paths = Vec::new();
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !name.starts_with("r.") || !name.ends_with(".bin") {
                continue;
            }
            let pieces: Vec<_> = name[2..name.len() - 4].split('.').collect();
            if pieces.len() != 2 {
                continue;
            }
            let (Ok(rx), Ok(rz)) = (pieces[0].parse::<i32>(), pieces[1].parse::<i32>()) else {
                continue;
            };
            region_paths.push((rx, rz, path));
        }
        if region_paths.len() > MAX_REGION_FILES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "too many region files in authoritative save",
            ));
        }
        region_paths.sort_by_key(|(rx, rz, _)| (*rx, *rz));
        let mut chunks = Vec::new();
        for (rx, rz, path) in region_paths {
            let bytes = fs::read(&path)?;
            let region: RegionData = bincode::deserialize(&bytes).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("region decode failed for {}: {error}", path.display()),
                )
            })?;
            let mut entries: Vec<_> = region.chunks.into_iter().collect();
            entries.sort_by_key(|((lx, lz), _)| (*lx, *lz));
            for ((lx, lz), chunk_bytes) in entries {
                if chunks.len() >= MAX_CHUNKS {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "too many saved chunks in authoritative world",
                    ));
                }
                let mut data =
                    format::deserialize_chunk_save_data(&chunk_bytes).ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("chunk decode failed in {}", path.display()),
                        )
                    })?;
                data.chunk_x = rx.saturating_mul(32).saturating_add(i32::from(lx));
                data.chunk_z = rz.saturating_mul(32).saturating_add(i32::from(lz));
                chunks.push(data);
            }
        }
        chunks.sort_by_key(|data| (data.chunk_x, data.chunk_z));
        Ok(chunks)
    }

    pub fn load_entities_in(&self, dimension: Dimension) -> Vec<EntitySaveData> {
        let path = self.entities_file_path(dimension);
        if !path.exists() {
            return Vec::new();
        }
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(_) => return Vec::new(),
        };
        bincode::deserialize(&bytes).unwrap_or_default()
    }

    pub fn load_chunk(&mut self, cx: i32, cz: i32) -> Option<ChunkSaveData> {
        self.load_chunk_in(Dimension::Overworld, cx, cz)
    }

    pub fn load_chunk_in(
        &mut self,
        dimension: Dimension,
        cx: i32,
        cz: i32,
    ) -> Option<ChunkSaveData> {
        let rx = cx.div_euclid(32);
        let rz = cz.div_euclid(32);
        let lx = cx.rem_euclid(32) as u8;
        let lz = cz.rem_euclid(32) as u8;
        let region_file = self
            .region_dir(dimension)
            .join(format!("r.{}.{}.bin", rx, rz));

        if !self.region_cache.contains_key(&(dimension, rx, rz)) {
            if region_file.exists() {
                if let Ok(mut file) = File::open(&region_file) {
                    let mut bytes = Vec::new();
                    if file.read_to_end(&mut bytes).is_ok() {
                        if let Ok(region_data) = bincode::deserialize::<RegionData>(&bytes) {
                            self.region_cache.insert((dimension, rx, rz), region_data);
                            self.region_disk_len
                                .insert((dimension, rx, rz), bytes.len() as u64);
                        }
                    }
                }
            }
        }

        if self.region_cache.contains_key(&(dimension, rx, rz)) {
            self.touch_region((dimension, rx, rz));
        }
        self.evict_lru_regions();

        let region = self.region_cache.get(&(dimension, rx, rz))?;

        if let Some(chunk_bytes) = region.chunks.get(&(lx, lz)) {
            format::deserialize_chunk_save_data(chunk_bytes)
        } else {
            None
        }
    }

    pub fn save_chunk(&mut self, cx: i32, cz: i32, data: ChunkSaveData) -> SaveResult<()> {
        self.save_chunk_in(Dimension::Overworld, cx, cz, data)
    }

    pub fn save_chunk_in(
        &mut self,
        dimension: Dimension,
        cx: i32,
        cz: i32,
        data: ChunkSaveData,
    ) -> SaveResult<()> {
        self.save_chunks_in(dimension, std::iter::once((cx, cz, data)))
    }

    /// Insert many columns, writing each region file at most once.
    pub fn save_chunks_in(
        &mut self,
        dimension: Dimension,
        chunks: impl IntoIterator<Item = (i32, i32, ChunkSaveData)>,
    ) -> SaveResult<()> {
        let mut by_region: BTreeMap<(i32, i32), Vec<(i32, i32, ChunkSaveData)>> = BTreeMap::new();
        for (cx, cz, data) in chunks {
            let rx = cx.div_euclid(32);
            let rz = cz.div_euclid(32);
            by_region.entry((rx, rz)).or_default().push((cx, cz, data));
        }
        for ((rx, rz), entries) in by_region {
            self.save_region_chunks(dimension, rx, rz, entries)?;
        }
        Ok(())
    }

    fn save_region_chunks(
        &mut self,
        dimension: Dimension,
        rx: i32,
        rz: i32,
        entries: Vec<(i32, i32, ChunkSaveData)>,
    ) -> SaveResult<()> {
        let Some((error_cx, error_cz, _)) = entries.first() else {
            return Ok(());
        };
        let region_file = self
            .region_dir(dimension)
            .join(format!("r.{}.{}.bin", rx, rz));
        let mut region =
            self.load_region_for_write(dimension, rx, rz, &region_file, *error_cx, *error_cz)?;

        for (cx, cz, data) in entries {
            let lx = cx.rem_euclid(32) as u8;
            let lz = cz.rem_euclid(32) as u8;
            let serialized_chunk = bincode::serialize(&data)
                .map_err(|error| SaveError::Serialization(error.to_string()))?;
            region.chunks.insert((lx, lz), serialized_chunk);
        }

        let serialized_region = bincode::serialize(&region)
            .map_err(|error| SaveError::Serialization(error.to_string()))?;

        region::backup_region_file_if_needed(&region_file);
        atomic_write(&region_file, &serialized_region)
            .map_err(|error| SaveError::io("atomic region replacement", &region_file, error))?;
        // Do not poison the in-memory cache if replacement fails; callers can
        // retry the same revision without losing the old on-disk snapshot.
        self.region_cache.insert((dimension, rx, rz), region);
        self.region_disk_len
            .insert((dimension, rx, rz), serialized_region.len() as u64);
        self.touch_region((dimension, rx, rz));
        self.evict_lru_regions();
        Ok(())
    }

    fn load_region_for_write(
        &self,
        dimension: Dimension,
        rx: i32,
        rz: i32,
        region_file: &Path,
        chunk_x: i32,
        chunk_z: i32,
    ) -> SaveResult<RegionData> {
        let key = (dimension, rx, rz);
        if let Some(cached) = self.region_cache.get(&key) {
            if !region_file.exists() {
                return Ok(cached.clone());
            }
            if let Ok(meta) = fs::metadata(region_file) {
                if self.region_disk_len.get(&key).copied() == Some(meta.len()) {
                    return Ok(cached.clone());
                }
            }
        }
        self.load_region_from_disk(region_file, chunk_x, chunk_z)
    }

    fn load_region_from_disk(
        &self,
        region_file: &Path,
        chunk_x: i32,
        chunk_z: i32,
    ) -> SaveResult<RegionData> {
        if !region_file.exists() {
            return Ok(RegionData {
                chunks: HashMap::new(),
            });
        }

        let bytes = fs::read(region_file).map_err(|error| SaveError::RegionCorruption {
            path: region_file.to_path_buf(),
            chunk_x,
            chunk_z,
            message: format!("could not read existing region: {error}"),
        })?;
        bincode::deserialize(&bytes).map_err(|error| SaveError::RegionCorruption {
            path: region_file.to_path_buf(),
            chunk_x,
            chunk_z,
            message: format!("could not deserialize existing region: {error}"),
        })
    }

    pub fn salvage_readable_region(&self, source: &Path, destination: &Path) -> SaveResult<usize> {
        let bytes = fs::read(source)
            .map_err(|error| SaveError::io("read region for salvage", source, error))?;
        let region: RegionData =
            bincode::deserialize(&bytes).map_err(|error| SaveError::RegionCorruption {
                path: source.to_path_buf(),
                chunk_x: 0,
                chunk_z: 0,
                message: format!("region container is not readable: {error}"),
            })?;
        let readable_chunks: HashMap<_, _> = region
            .chunks
            .into_iter()
            .filter(|(_, bytes)| format::deserialize_chunk_save_data(bytes).is_some())
            .collect();
        let count = readable_chunks.len();
        let serialized = bincode::serialize(&RegionData {
            chunks: readable_chunks,
        })
        .map_err(|error| SaveError::Serialization(error.to_string()))?;
        atomic_write(destination, &serialized)
            .map_err(|error| SaveError::io("write salvaged region", destination, error))?;
        Ok(count)
    }

    pub fn save_current_dimension(&self, dimension: Dimension) -> io::Result<()> {
        atomic_write(self.world_dir.join("dimension.dat"), &[dimension as u8])
    }

    pub fn save_mutation_revision_index(&self, index: &MutationRevisionIndex) -> io::Result<()> {
        let bytes = bincode::serialize(index)
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
        atomic_write(self.world_dir.join("mutation_revisions.bin"), &bytes)
    }

    pub fn load_mutation_revision_index(&self) -> MutationRevisionIndex {
        fs::read(self.world_dir.join("mutation_revisions.bin"))
            .ok()
            .and_then(|bytes| bincode::deserialize(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn load_current_dimension(&self) -> Dimension {
        peek_current_dimension(&self.world_dir)
    }

    pub fn save_player_and_level(&self, level: &LevelData, player: &PlayerData) -> io::Result<()> {
        player::save_player_and_level(&self.world_dir, level, player)
    }

    pub fn save_level(&self, level: &LevelData) -> io::Result<()> {
        let serialized = bincode::serialize(level)
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
        atomic_write(self.world_dir.join("level.dat"), &serialized)
    }

    pub fn load_level(&self) -> io::Result<Option<LevelData>> {
        let path = self.world_dir.join("level.dat");
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&path)?;
        match bincode::deserialize::<LevelData>(&bytes) {
            Ok(level) => Ok(Some(level)),
            Err(current_error) => bincode::deserialize::<format::LegacyLevelData>(&bytes)
                .map(LevelData::from)
                .map(Some)
                .map_err(|legacy_error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "level decode failed (current: {current_error}; legacy: {legacy_error})"
                        ),
                    )
                }),
        }
    }

    pub fn load_player_and_level(&self) -> io::Result<(LevelData, PlayerData)> {
        player::load_player_and_level(&self.world_dir)
    }
}
