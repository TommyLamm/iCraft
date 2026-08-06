use crate::dimension::Dimension;
use crate::world::BlockType;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum VillagerProfession {
    Unemployed,
    Farmer,
    Librarian,
    Armorer,
    Cleric,
}

impl VillagerProfession {
    pub const fn to_wire(self) -> u8 {
        match self {
            Self::Unemployed => 0,
            Self::Farmer => 1,
            Self::Librarian => 2,
            Self::Armorer => 3,
            Self::Cleric => 4,
        }
    }

    pub const fn from_wire(val: u8) -> Self {
        match val {
            1 => Self::Farmer,
            2 => Self::Librarian,
            3 => Self::Armorer,
            4 => Self::Cleric,
            _ => Self::Unemployed,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Unemployed => "Unemployed",
            Self::Farmer => "Farmer",
            Self::Librarian => "Librarian",
            Self::Armorer => "Armorer",
            Self::Cleric => "Cleric",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PoiType {
    Bed,
    JobSite(VillagerProfession),
    MeetingPoint,
}

impl PoiType {
    pub fn from_block_type(block: BlockType) -> Option<Self> {
        match block {
            BlockType::Bed => Some(Self::Bed),
            BlockType::BrewingStand => Some(Self::JobSite(VillagerProfession::Cleric)),
            BlockType::Furnace | BlockType::FurnaceLit | BlockType::Anvil => {
                Some(Self::JobSite(VillagerProfession::Armorer))
            }
            BlockType::Bookshelf => Some(Self::JobSite(VillagerProfession::Librarian)),
            BlockType::Farmland | BlockType::CraftingTable => {
                Some(Self::JobSite(VillagerProfession::Farmer))
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PoiEntry {
    pub pos: (i32, i32, i32),
    pub poi_type: PoiType,
    pub owner_entity_id: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Village {
    pub id: u64,
    pub center: (i32, i32, i32),
    pub dimension: Dimension,
    pub bed_count: usize,
    pub job_count: usize,
    pub villager_ids: Vec<u64>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct PoiManager {
    /// Maps (Dimension, (x, y, z)) -> PoiEntry
    pub pois: HashMap<(Dimension, (i32, i32, i32)), PoiEntry>,
    /// Maps (Dimension, (cx, cz)) -> list of POI positions in that chunk
    pub chunk_pois: HashMap<(Dimension, i32, i32), Vec<(i32, i32, i32)>>,
    /// Active villages
    pub villages: Vec<Village>,
    pub next_village_id: u64,
}

impl PoiManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_poi(&mut self, dimension: Dimension, pos: (i32, i32, i32), poi_type: PoiType) {
        let entry = PoiEntry {
            pos,
            poi_type,
            owner_entity_id: None,
        };
        self.pois.insert((dimension, pos), entry);

        let cx = pos.0 >> 4;
        let cz = pos.2 >> 4;
        let positions = self.chunk_pois.entry((dimension, cx, cz)).or_default();
        if !positions.contains(&pos) {
            positions.push(pos);
        }
    }

    pub fn remove_poi(&mut self, dimension: Dimension, pos: (i32, i32, i32)) -> Option<PoiEntry> {
        let removed = self.pois.remove(&(dimension, pos));
        if removed.is_some() {
            let cx = pos.0 >> 4;
            let cz = pos.2 >> 4;
            if let Some(list) = self.chunk_pois.get_mut(&(dimension, cx, cz)) {
                list.retain(|p| *p != pos);
            }
        }
        removed
    }

    pub fn claim_poi(
        &mut self,
        dimension: Dimension,
        poi_type: PoiType,
        entity_id: u64,
        search_pos: (i32, i32, i32),
        max_dist: f32,
    ) -> Option<(i32, i32, i32)> {
        let max_dist_sq = max_dist * max_dist;
        let mut best_pos = None;
        let mut min_dist_sq = f32::MAX;

        for (key, entry) in self.pois.iter() {
            if key.0 != dimension {
                continue;
            }
            if entry.poi_type != poi_type {
                continue;
            }
            if entry.owner_entity_id.is_some() && entry.owner_entity_id != Some(entity_id) {
                continue;
            }

            let dx = (entry.pos.0 - search_pos.0) as f32;
            let dy = (entry.pos.1 - search_pos.1) as f32;
            let dz = (entry.pos.2 - search_pos.2) as f32;
            let dist_sq = dx * dx + dy * dy + dz * dz;

            if dist_sq <= max_dist_sq && dist_sq < min_dist_sq {
                min_dist_sq = dist_sq;
                best_pos = Some(entry.pos);
            }
        }

        if let Some(pos) = best_pos {
            if let Some(entry) = self.pois.get_mut(&(dimension, pos)) {
                entry.owner_entity_id = Some(entity_id);
            }
        }

        best_pos
    }

    pub fn release_poi(&mut self, dimension: Dimension, pos: (i32, i32, i32), entity_id: u64) {
        if let Some(entry) = self.pois.get_mut(&(dimension, pos)) {
            if entry.owner_entity_id == Some(entity_id) {
                entry.owner_entity_id = None;
            }
        }
    }

    pub fn release_all_claims_for_entity(&mut self, entity_id: u64) {
        for entry in self.pois.values_mut() {
            if entry.owner_entity_id == Some(entity_id) {
                entry.owner_entity_id = None;
            }
        }
    }

    pub fn on_chunk_unload(&mut self, dimension: Dimension, cx: i32, cz: i32) {
        if let Some(positions) = self.chunk_pois.remove(&(dimension, cx, cz)) {
            for pos in positions {
                self.pois.remove(&(dimension, pos));
            }
        }
    }

    pub fn find_nearest_poi_of_type(
        &self,
        dimension: Dimension,
        search_pos: (i32, i32, i32),
        poi_type: PoiType,
        max_dist: f32,
    ) -> Option<(i32, i32, i32)> {
        let max_dist_sq = max_dist * max_dist;
        let mut best_pos = None;
        let mut min_dist_sq = f32::MAX;

        for (key, entry) in self.pois.iter() {
            if key.0 != dimension {
                continue;
            }
            if entry.poi_type != poi_type {
                continue;
            }

            let dx = (entry.pos.0 - search_pos.0) as f32;
            let dy = (entry.pos.1 - search_pos.1) as f32;
            let dz = (entry.pos.2 - search_pos.2) as f32;
            let dist_sq = dx * dx + dy * dy + dz * dz;

            if dist_sq <= max_dist_sq && dist_sq < min_dist_sq {
                min_dist_sq = dist_sq;
                best_pos = Some(entry.pos);
            }
        }

        best_pos
    }

    pub fn get_unclaimed_beds_in_radius(
        &self,
        dimension: Dimension,
        center: (i32, i32, i32),
        radius: f32,
    ) -> usize {
        let r_sq = radius * radius;
        self.pois
            .iter()
            .filter(|(key, entry)| {
                key.0 == dimension
                    && entry.poi_type == PoiType::Bed
                    && entry.owner_entity_id.is_none()
                    && {
                        let dx = (entry.pos.0 - center.0) as f32;
                        let dy = (entry.pos.1 - center.1) as f32;
                        let dz = (entry.pos.2 - center.2) as f32;
                        dx * dx + dy * dy + dz * dz <= r_sq
                    }
            })
            .count()
    }

    pub fn update_village_clusters(&mut self, dimension: Dimension) {
        self.villages.retain(|v| v.dimension != dimension);

        let mut beds: Vec<(i32, i32, i32)> = Vec::new();
        let mut jobs: Vec<(i32, i32, i32)> = Vec::new();

        for (key, entry) in self.pois.iter() {
            if key.0 == dimension {
                match entry.poi_type {
                    PoiType::Bed => beds.push(entry.pos),
                    PoiType::JobSite(_) | PoiType::MeetingPoint => jobs.push(entry.pos),
                }
            }
        }

        if beds.is_empty() {
            return;
        }

        let mut visited = HashSet::new();

        for &bed in &beds {
            if visited.contains(&bed) {
                continue;
            }

            let mut cluster = Vec::new();
            let mut queue = vec![bed];
            visited.insert(bed);

            while let Some(curr) = queue.pop() {
                cluster.push(curr);
                for &other in &beds {
                    if visited.contains(&other) {
                        continue;
                    }
                    let dx = (curr.0 - other.0).abs();
                    let dy = (curr.1 - other.1).abs();
                    let dz = (curr.2 - other.2).abs();

                    if dx <= 48 && dy <= 24 && dz <= 48 {
                        visited.insert(other);
                        queue.push(other);
                    }
                }
            }

            let sum_x: i64 = cluster.iter().map(|p| p.0 as i64).sum();
            let sum_y: i64 = cluster.iter().map(|p| p.1 as i64).sum();
            let sum_z: i64 = cluster.iter().map(|p| p.2 as i64).sum();
            let count = cluster.len() as i64;

            let center = (
                (sum_x / count) as i32,
                (sum_y / count) as i32,
                (sum_z / count) as i32,
            );

            let job_count = jobs
                .iter()
                .filter(|p| {
                    (p.0 - center.0).abs() <= 48
                        && (p.1 - center.1).abs() <= 24
                        && (p.2 - center.2).abs() <= 48
                })
                .count();

            self.next_village_id += 1;
            self.villages.push(Village {
                id: self.next_village_id,
                center,
                dimension,
                bed_count: cluster.len(),
                job_count,
                villager_ids: Vec::new(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_poi_registration_and_claim() {
        let mut poi_mgr = PoiManager::new();
        let dim = Dimension::Overworld;
        let pos = (10, 64, 10);

        poi_mgr.register_poi(dim, pos, PoiType::Bed);
        assert_eq!(poi_mgr.pois.len(), 1);

        let claimed = poi_mgr.claim_poi(dim, PoiType::Bed, 100, (10, 64, 10), 16.0);
        assert_eq!(claimed, Some(pos));
        assert_eq!(
            poi_mgr.pois.get(&(dim, pos)).unwrap().owner_entity_id,
            Some(100)
        );

        // Second villager cannot claim same bed
        let claimed2 = poi_mgr.claim_poi(dim, PoiType::Bed, 101, (10, 64, 10), 16.0);
        assert_eq!(claimed2, None);

        // Release POI
        poi_mgr.release_poi(dim, pos, 100);
        assert_eq!(poi_mgr.pois.get(&(dim, pos)).unwrap().owner_entity_id, None);
    }

    #[test]
    fn test_poi_chunk_unload() {
        let mut poi_mgr = PoiManager::new();
        let dim = Dimension::Overworld;
        poi_mgr.register_poi(dim, (5, 64, 5), PoiType::Bed);
        poi_mgr.register_poi(dim, (20, 64, 20), PoiType::Bed);

        assert_eq!(poi_mgr.pois.len(), 2);
        // Chunk (0, 0) contains (5, 64, 5)
        poi_mgr.on_chunk_unload(dim, 0, 0);

        assert_eq!(poi_mgr.pois.len(), 1);
        assert!(poi_mgr.pois.contains_key(&(dim, (20, 64, 20))));
    }

    #[test]
    fn test_village_clustering() {
        let mut poi_mgr = PoiManager::new();
        let dim = Dimension::Overworld;
        poi_mgr.register_poi(dim, (0, 64, 0), PoiType::Bed);
        poi_mgr.register_poi(dim, (10, 64, 0), PoiType::Bed);
        poi_mgr.register_poi(dim, (100, 64, 100), PoiType::Bed);

        poi_mgr.update_village_clusters(dim);

        assert_eq!(poi_mgr.villages.len(), 2);
    }
}
