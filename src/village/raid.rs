use crate::dimension::Dimension;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RaidStatus {
    Ongoing,
    Victory,
    Defeat,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RaidWave {
    pub wave_number: u8,
    pub pillager_count: usize,
    pub ravager_count: usize,
    pub captain_count: usize,
}

impl RaidWave {
    pub fn for_wave(wave: u8) -> Self {
        match wave {
            1 => Self {
                wave_number: 1,
                pillager_count: 3,
                ravager_count: 0,
                captain_count: 1,
            },
            2 => Self {
                wave_number: 2,
                pillager_count: 4,
                ravager_count: 1,
                captain_count: 1,
            },
            _ => Self {
                wave_number: 3,
                pillager_count: 6,
                ravager_count: 2,
                captain_count: 1,
            },
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActiveRaid {
    pub id: u64,
    pub village_id: u64,
    pub center: (i32, i32, i32),
    pub dimension: Dimension,
    pub current_wave: u8,
    pub max_waves: u8,
    pub status: RaidStatus,
    pub spawned_mob_ids: Vec<u64>,
    pub wave_timer: f32,
    pub bad_omen_level: u8,
}

impl ActiveRaid {
    pub fn new(
        id: u64,
        village_id: u64,
        center: (i32, i32, i32),
        dimension: Dimension,
        bad_omen_level: u8,
    ) -> Self {
        Self {
            id,
            village_id,
            center,
            dimension,
            current_wave: 1,
            max_waves: 3,
            status: RaidStatus::Ongoing,
            spawned_mob_ids: Vec::new(),
            wave_timer: 0.0,
            bad_omen_level: bad_omen_level.max(1),
        }
    }

    pub fn is_active(&self) -> bool {
        self.status == RaidStatus::Ongoing
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct RaidManager {
    pub active_raids: HashMap<u64, ActiveRaid>,
    pub next_raid_id: u64,
}

impl RaidManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn trigger_raid(
        &mut self,
        village_id: u64,
        center: (i32, i32, i32),
        dimension: Dimension,
        bad_omen_level: u8,
    ) -> u64 {
        // Prevent duplicate raid for same village
        for raid in self.active_raids.values() {
            if raid.village_id == village_id && raid.is_active() {
                return raid.id;
            }
        }

        self.next_raid_id += 1;
        let raid_id = self.next_raid_id;
        let raid = ActiveRaid::new(raid_id, village_id, center, dimension, bad_omen_level);
        self.active_raids.insert(raid_id, raid);
        raid_id
    }

    pub fn get_raid_mut(&mut self, raid_id: u64) -> Option<&mut ActiveRaid> {
        self.active_raids.get_mut(&raid_id)
    }

    pub fn get_raid_for_village(&self, village_id: u64) -> Option<&ActiveRaid> {
        self.active_raids
            .values()
            .find(|r| r.village_id == village_id)
    }

    pub fn on_mob_killed(&mut self, mob_id: u64) {
        for raid in self.active_raids.values_mut() {
            if raid.is_active() {
                raid.spawned_mob_ids.retain(|&id| id != mob_id);
                if raid.spawned_mob_ids.is_empty() && raid.wave_timer <= 0.0 {
                    if raid.current_wave >= raid.max_waves {
                        raid.status = RaidStatus::Victory;
                    } else {
                        raid.current_wave += 1;
                        raid.wave_timer = 5.0; // 5 sec pause before next wave
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_raid_trigger_and_wave_advancement() {
        let mut raid_mgr = RaidManager::new();
        let raid_id = raid_mgr.trigger_raid(1, (0, 64, 0), Dimension::Overworld, 1);
        assert_eq!(raid_id, 1);

        let raid = raid_mgr.get_raid_mut(raid_id).unwrap();
        assert_eq!(raid.current_wave, 1);
        assert_eq!(raid.status, RaidStatus::Ongoing);

        // Add 2 mobs
        raid.spawned_mob_ids = vec![100, 101];

        // Kill 1 mob
        raid_mgr.on_mob_killed(100);
        assert_eq!(raid_mgr.get_raid_for_village(1).unwrap().current_wave, 1);
        assert_eq!(
            raid_mgr.get_raid_for_village(1).unwrap().status,
            RaidStatus::Ongoing
        );

        // Kill 2nd mob -> advances wave
        raid_mgr.on_mob_killed(101);
        assert_eq!(raid_mgr.get_raid_for_village(1).unwrap().current_wave, 2);

        // Wave 2 mobs
        raid_mgr.get_raid_mut(raid_id).unwrap().spawned_mob_ids = vec![102];
        raid_mgr.get_raid_mut(raid_id).unwrap().wave_timer = 0.0;
        raid_mgr.on_mob_killed(102);
        assert_eq!(raid_mgr.get_raid_for_village(1).unwrap().current_wave, 3);

        // Wave 3 mobs
        raid_mgr.get_raid_mut(raid_id).unwrap().spawned_mob_ids = vec![103];
        raid_mgr.get_raid_mut(raid_id).unwrap().wave_timer = 0.0;
        raid_mgr.on_mob_killed(103);
        assert_eq!(
            raid_mgr.get_raid_for_village(1).unwrap().status,
            RaidStatus::Victory
        );
    }
}
