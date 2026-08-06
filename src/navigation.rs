use crate::dimension::Dimension;
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const MAP_SIZE: usize = 128;

/// Calculate compass needle angle relative to player view yaw.
pub fn calculate_compass_angle(
    player_pos: Vec3,
    player_yaw: f32,
    spawn_pos: (i32, i32),
    dimension: Dimension,
    game_time: u64,
) -> f32 {
    if dimension != Dimension::Overworld {
        // Wobble randomly in Nether/End
        let t = game_time as f32 * 0.1;
        return t.sin() * std::f32::consts::PI;
    }

    let dx = spawn_pos.0 as f32 + 0.5 - player_pos.x;
    let dz = spawn_pos.1 as f32 + 0.5 - player_pos.z;

    // Angle to target in world coordinates
    let target_angle = dz.atan2(dx);
    // Relative angle subtracting player yaw
    let mut rel_angle = target_angle - player_yaw;

    // Normalize to [-PI, PI]
    while rel_angle > std::f32::consts::PI {
        rel_angle -= 2.0 * std::f32::consts::PI;
    }
    while rel_angle < -std::f32::consts::PI {
        rel_angle += 2.0 * std::f32::consts::PI;
    }

    rel_angle
}

/// Calculate clock hand rotation fraction (0.0 to 1.0).
pub fn calculate_clock_fraction(game_time: u64, dimension: Dimension) -> f32 {
    if dimension != Dimension::Overworld {
        // Spin wildly in Nether/End
        let t = (game_time as f32 * 0.05).fract();
        return t;
    }
    // Overworld time cycle is 24000 ticks
    let day_time = (game_time % 24000) as f32;
    day_time / 24000.0
}

/// Single persistent map data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapData {
    pub id: u32,
    pub dimension: Dimension,
    pub center_x: i32,
    pub center_z: i32,
    pub scale: u8,
    pub colors: Vec<u8>, // MAP_SIZE * MAP_SIZE
}

impl MapData {
    pub fn new(id: u32, dimension: Dimension, center_x: i32, center_z: i32, scale: u8) -> Self {
        Self {
            id,
            dimension,
            center_x,
            center_z,
            scale,
            colors: vec![0; MAP_SIZE * MAP_SIZE], // 0 = unexplored/translucent
        }
    }

    /// Update map pixels around player position within a budget per call.
    pub fn update_explored_pixels<F>(
        &mut self,
        player_pos: Vec3,
        dimension: Dimension,
        mut sample_color_at: F,
        max_pixels_update: usize,
    ) -> usize
    where
        F: FnMut(i32, i32) -> u8,
    {
        if self.dimension != dimension {
            return 0;
        }

        let scale_factor = 1 << self.scale;
        let half_map = (MAP_SIZE / 2) as i32 * scale_factor;

        let px = player_pos.x.floor() as i32;
        let pz = player_pos.z.floor() as i32;

        let mut updated = 0;

        // Radius around player to update
        let update_radius_blocks = 16 * scale_factor;

        for dz in -update_radius_blocks..=update_radius_blocks {
            for dx in -update_radius_blocks..=update_radius_blocks {
                if updated >= max_pixels_update {
                    return updated;
                }

                let world_x = px + dx;
                let world_z = pz + dz;

                // Map coordinates
                let map_x = (world_x - self.center_x + half_map) / scale_factor;
                let map_z = (world_z - self.center_z + half_map) / scale_factor;

                if map_x >= 0 && map_x < MAP_SIZE as i32 && map_z >= 0 && map_z < MAP_SIZE as i32 {
                    let idx = (map_z as usize) * MAP_SIZE + (map_x as usize);
                    let color = sample_color_at(world_x, world_z);
                    if self.colors[idx] != color {
                        self.colors[idx] = color;
                        updated += 1;
                    }
                }
            }
        }

        updated
    }
}

/// Persistent manager for map data collection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MapManager {
    maps: HashMap<u32, MapData>,
    next_map_id: u32,
}

impl MapManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_map(&mut self, dimension: Dimension, center_x: i32, center_z: i32) -> u32 {
        let id = self.next_map_id;
        self.next_map_id += 1;
        let map = MapData::new(id, dimension, center_x, center_z, 0);
        self.maps.insert(id, map);
        id
    }

    pub fn get_map(&self, id: u32) -> Option<&MapData> {
        self.maps.get(&id)
    }

    pub fn get_map_mut(&mut self, id: u32) -> Option<&mut MapData> {
        self.maps.get_mut(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compass_angle_calculation() {
        let spawn = (0, 0);
        // Player at (0, 10), looking North (yaw = 0.0) -> spawn is South (0, 0)
        let player_pos = Vec3::new(0.0, 64.0, 10.0);
        let angle = calculate_compass_angle(player_pos, 0.0, spawn, Dimension::Overworld, 0);
        assert!((angle - (-std::f32::consts::FRAC_PI_2)).abs() < 0.1);
    }

    #[test]
    fn test_clock_overworld_fraction() {
        let frac = calculate_clock_fraction(6000, Dimension::Overworld);
        assert!((frac - 0.25).abs() < 0.001);
    }

    #[test]
    fn test_map_creation_and_update() {
        let mut mm = MapManager::new();
        let map_id = mm.create_map(Dimension::Overworld, 0, 0);
        let map = mm.get_map_mut(map_id).unwrap();

        let updated = map.update_explored_pixels(
            Vec3::new(0.0, 64.0, 0.0),
            Dimension::Overworld,
            |_x, _z| 12, // Color ID
            2000,
        );

        assert!(updated > 0);
        assert_eq!(map.colors[64 * MAP_SIZE + 64], 12);
    }
}
