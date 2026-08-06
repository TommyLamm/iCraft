use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Seat offset relative to vehicle position and orientation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SeatOffset {
    pub local_offset: [f32; 3],
}

impl SeatOffset {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self {
            local_offset: [x, y, z],
        }
    }

    pub fn offset_vec3(&self) -> Vec3 {
        Vec3::from_array(self.local_offset)
    }

    /// Calculate world position of passenger given vehicle position and yaw (in radians).
    pub fn world_position(&self, vehicle_pos: Vec3, vehicle_yaw: f32) -> Vec3 {
        let offset = self.offset_vec3();
        let cos_y = vehicle_yaw.cos();
        let sin_y = vehicle_yaw.sin();
        let rot_x = offset.x * cos_y - offset.z * sin_y;
        let rot_z = offset.x * sin_y + offset.z * cos_y;
        vehicle_pos + Vec3::new(rot_x, offset.y, rot_z)
    }
}

/// Mounting relationship errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountError {
    AlreadyMounted,
    VehicleFull,
    DistanceTooFar,
    WouldCreateCycle,
    InvalidVehicle,
    InvalidPassenger,
    DimensionMismatch,
}

/// Authoritative manager for vehicle-passenger relationships.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MountManager {
    /// Maps passenger_id -> vehicle_id
    passenger_to_vehicle: HashMap<u64, u64>,
    /// Maps vehicle_id -> ordered list of passenger_ids
    vehicle_to_passengers: HashMap<u64, Vec<u64>>,
    /// Maps vehicle_id -> capacity
    vehicle_capacities: HashMap<u64, usize>,
}

impl MountManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_capacity(&mut self, vehicle_id: u64, capacity: usize) {
        self.vehicle_capacities.insert(vehicle_id, capacity);
    }

    pub fn get_vehicle(&self, passenger_id: u64) -> Option<u64> {
        self.passenger_to_vehicle.get(&passenger_id).copied()
    }

    pub fn get_passengers(&self, vehicle_id: u64) -> &[u64] {
        self.vehicle_to_passengers
            .get(&vehicle_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Check if mounting passenger on vehicle would create a cycle.
    pub fn would_create_cycle(&self, vehicle_id: u64, passenger_id: u64) -> bool {
        if vehicle_id == passenger_id {
            return true;
        }
        let mut curr = vehicle_id;
        let mut visited = HashSet::new();
        visited.insert(curr);
        while let Some(&next_vehicle) = self.passenger_to_vehicle.get(&curr) {
            if next_vehicle == passenger_id {
                return true;
            }
            if !visited.insert(next_vehicle) {
                return true;
            }
            curr = next_vehicle;
        }
        false
    }

    /// Authoritative request to mount a passenger onto a vehicle.
    pub fn mount(
        &mut self,
        vehicle_id: u64,
        passenger_id: u64,
        max_capacity: usize,
    ) -> Result<usize, MountError> {
        if self.passenger_to_vehicle.contains_key(&passenger_id) {
            return Err(MountError::AlreadyMounted);
        }
        if self.would_create_cycle(vehicle_id, passenger_id) {
            return Err(MountError::WouldCreateCycle);
        }
        let passengers = self.vehicle_to_passengers.entry(vehicle_id).or_default();
        if passengers.len() >= max_capacity {
            return Err(MountError::VehicleFull);
        }
        let seat_index = passengers.len();
        passengers.push(passenger_id);
        self.passenger_to_vehicle.insert(passenger_id, vehicle_id);
        self.vehicle_capacities.insert(vehicle_id, max_capacity);
        Ok(seat_index)
    }

    /// Dismount a specific passenger from their vehicle.
    pub fn dismount(&mut self, passenger_id: u64) -> Option<u64> {
        let vehicle_id = self.passenger_to_vehicle.remove(&passenger_id)?;
        if let Some(passengers) = self.vehicle_to_passengers.get_mut(&vehicle_id) {
            passengers.retain(|&id| id != passenger_id);
            if passengers.is_empty() {
                self.vehicle_to_passengers.remove(&vehicle_id);
                self.vehicle_capacities.remove(&vehicle_id);
            }
        }
        Some(vehicle_id)
    }

    /// Dismount all passengers from a vehicle.
    pub fn dismount_all(&mut self, vehicle_id: u64) -> Vec<u64> {
        let passengers = self
            .vehicle_to_passengers
            .remove(&vehicle_id)
            .unwrap_or_default();
        self.vehicle_capacities.remove(&vehicle_id);
        for &p in &passengers {
            self.passenger_to_vehicle.remove(&p);
        }
        passengers
    }

    /// Calculate safe dismount position around vehicle position.
    pub fn find_dismount_position<F>(vehicle_pos: Vec3, is_solid_block: F) -> Vec3
    where
        F: Fn(i32, i32, i32) -> bool,
    {
        let offsets = [
            (1.0, 0.0, 0.0),
            (-1.0, 0.0, 0.0),
            (0.0, 0.0, 1.0),
            (0.0, 0.0, -1.0),
            (1.0, 1.0, 0.0),
            (-1.0, 1.0, 0.0),
            (0.0, 1.0, 1.0),
            (0.0, 1.0, -1.0),
        ];

        for (dx, dy, dz) in offsets {
            let candidate = vehicle_pos + Vec3::new(dx, dy, dz);
            let cx = candidate.x.floor() as i32;
            let cy = candidate.y.floor() as i32;
            let cz = candidate.z.floor() as i32;

            if !is_solid_block(cx, cy, cz) && !is_solid_block(cx, cy + 1, cz) {
                return candidate;
            }
        }
        vehicle_pos + Vec3::new(0.0, 1.0, 0.0)
    }
}

/// Boat physics & simulation state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoatState {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub paddle_left: bool,
    pub paddle_right: bool,
    pub is_in_water: bool,
    pub health: f32,
}

impl Default for BoatState {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            velocity: [0.0, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            paddle_left: false,
            paddle_right: false,
            is_in_water: false,
            health: 40.0,
        }
    }
}

impl BoatState {
    pub fn new(position: Vec3, yaw: f32) -> Self {
        Self {
            position: [position.x, position.y, position.z],
            yaw,
            ..Default::default()
        }
    }

    pub fn pos_vec3(&self) -> Vec3 {
        Vec3::from_array(self.position)
    }

    pub fn vel_vec3(&self) -> Vec3 {
        Vec3::from_array(self.velocity)
    }

    pub fn set_pos(&mut self, pos: Vec3) {
        self.position = [pos.x, pos.y, pos.z];
    }

    pub fn set_vel(&mut self, vel: Vec3) {
        self.velocity = [vel.x, vel.y, vel.z];
    }

    /// Seats: Seat 0 (Driver) = (-0.3, 0.25, 0.0), Seat 1 (Passenger) = (0.3, 0.25, 0.0)
    pub fn seat_offset(seat_index: usize) -> SeatOffset {
        match seat_index {
            0 => SeatOffset::new(-0.3, 0.25, 0.0),
            _ => SeatOffset::new(0.3, 0.25, 0.0),
        }
    }

    /// Advance boat physics per frame/tick.
    pub fn tick<F, G>(&mut self, dt: f32, is_water_at: F, is_solid_at: G)
    where
        F: Fn(i32, i32, i32) -> bool,
        G: Fn(i32, i32, i32) -> bool,
    {
        let mut pos = self.pos_vec3();
        let mut vel = self.vel_vec3();

        let bx = pos.x.floor() as i32;
        let by = pos.y.floor() as i32;
        let bz = pos.z.floor() as i32;

        self.is_in_water = is_water_at(bx, by, bz) || is_water_at(bx, by - 1, bz);

        let turn_speed = 2.5 * dt;
        let accel_speed = 8.0 * dt;

        if self.paddle_left && !self.paddle_right {
            self.yaw += turn_speed;
        } else if self.paddle_right && !self.paddle_left {
            self.yaw -= turn_speed;
        } else if self.paddle_left && self.paddle_right {
            let forward = Vec3::new(-self.yaw.sin(), 0.0, self.yaw.cos());
            vel += forward * accel_speed;
        }

        if self.is_in_water {
            let water_y = by as f32 + 0.9;
            let dy = water_y - pos.y;
            vel.y += dy * 5.0 * dt;
            vel.x *= 0.95;
            vel.z *= 0.95;
            vel.y *= 0.8;
        } else {
            vel.y -= 18.0 * dt;
            vel.x *= 0.6;
            vel.z *= 0.6;
        }

        let max_speed = if self.is_in_water { 8.0 } else { 2.0 };
        let horiz_speed = Vec3::new(vel.x, 0.0, vel.z).length();
        if horiz_speed > max_speed {
            let scale = max_speed / horiz_speed;
            vel.x *= scale;
            vel.z *= scale;
        }

        let new_pos = pos + vel * dt;
        let nx = new_pos.x.floor() as i32;
        let ny = new_pos.y.floor() as i32;
        let nz = new_pos.z.floor() as i32;

        if is_solid_at(nx, ny, nz) {
            vel.x = 0.0;
            vel.z = 0.0;
        } else {
            pos.x = new_pos.x;
            pos.z = new_pos.z;
        }

        if is_solid_at(pos.x.floor() as i32, ny, pos.z.floor() as i32) {
            vel.y = 0.0;
        } else {
            pos.y = new_pos.y;
        }

        self.set_pos(pos);
        self.set_vel(vel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_cycle_detection() {
        let mut mm = MountManager::new();
        assert!(mm.mount(100, 1, 2).is_ok());
        assert!(mm.mount(200, 100, 2).is_ok());
        assert!(mm.would_create_cycle(1, 200));
        assert_eq!(mm.mount(1, 200, 2), Err(MountError::WouldCreateCycle));
    }

    #[test]
    fn test_capacity_limit() {
        let mut mm = MountManager::new();
        assert!(mm.mount(100, 1, 1).is_ok());
        assert_eq!(mm.mount(100, 2, 1), Err(MountError::VehicleFull));
    }

    #[test]
    fn test_dismount_all() {
        let mut mm = MountManager::new();
        assert!(mm.mount(100, 1, 2).is_ok());
        assert!(mm.mount(100, 2, 2).is_ok());
        let passengers = mm.dismount_all(100);
        assert_eq!(passengers, vec![1, 2]);
        assert_eq!(mm.get_vehicle(1), None);
        assert_eq!(mm.get_vehicle(2), None);
    }
}
