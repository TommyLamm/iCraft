use glam::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RailShape {
    NorthSouth = 0,
    EastWest = 1,
    AscendingEast = 2,
    AscendingWest = 3,
    AscendingNorth = 4,
    AscendingSouth = 5,
    SouthEast = 6,
    SouthWest = 7,
    NorthWest = 8,
    NorthEast = 9,
}

impl RailShape {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::NorthSouth,
            1 => Self::EastWest,
            2 => Self::AscendingEast,
            3 => Self::AscendingWest,
            4 => Self::AscendingNorth,
            5 => Self::AscendingSouth,
            6 => Self::SouthEast,
            7 => Self::SouthWest,
            8 => Self::NorthWest,
            9 => Self::NorthEast,
            _ => Self::NorthSouth,
        }
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn direction_vector(self, current_vel: Vec3) -> Vec3 {
        match self {
            Self::NorthSouth => {
                if current_vel.z < 0.0 {
                    Vec3::new(0.0, 0.0, -1.0)
                } else {
                    Vec3::new(0.0, 0.0, 1.0)
                }
            }
            Self::EastWest => {
                if current_vel.x < 0.0 {
                    Vec3::new(-1.0, 0.0, 0.0)
                } else {
                    Vec3::new(1.0, 0.0, 0.0)
                }
            }
            Self::AscendingEast => {
                let dir = Vec3::new(1.0, 0.5, 0.0).normalize();
                if current_vel.x < 0.0 {
                    -dir
                } else {
                    dir
                }
            }
            Self::AscendingWest => {
                let dir = Vec3::new(-1.0, 0.5, 0.0).normalize();
                if current_vel.x > 0.0 {
                    -dir
                } else {
                    dir
                }
            }
            Self::AscendingNorth => {
                let dir = Vec3::new(0.0, 0.5, -1.0).normalize();
                if current_vel.z > 0.0 {
                    -dir
                } else {
                    dir
                }
            }
            Self::AscendingSouth => {
                let dir = Vec3::new(0.0, 0.5, 1.0).normalize();
                if current_vel.z < 0.0 {
                    -dir
                } else {
                    dir
                }
            }
            Self::SouthEast | Self::SouthWest | Self::NorthWest | Self::NorthEast => {
                let mut dir = Vec3::ZERO;
                if current_vel.x.abs() > current_vel.z.abs() {
                    dir.x = current_vel.x.signum();
                } else {
                    dir.z = current_vel.z.signum();
                }
                if dir == Vec3::ZERO {
                    dir = Vec3::new(1.0, 0.0, 0.0);
                }
                dir
            }
        }
    }
}

/// Minecart simulation state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecartState {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub is_on_rail: bool,
}

impl Default for MinecartState {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            velocity: [0.0, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            is_on_rail: false,
        }
    }
}

impl MinecartState {
    pub fn new(position: Vec3) -> Self {
        Self {
            position: [position.x, position.y, position.z],
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

    /// Advance minecart physics along rail network per tick.
    pub fn tick<F, G>(&mut self, dt: f32, get_rail_info: F, mut set_detector_powered: G)
    where
        F: Fn(i32, i32, i32) -> Option<(RailType, RailShape, bool)>,
        G: FnMut(i32, i32, i32, bool),
    {
        let mut pos = self.pos_vec3();
        let mut vel = self.vel_vec3();

        let bx = pos.x.floor() as i32;
        let by = pos.y.floor() as i32;
        let bz = pos.z.floor() as i32;

        let rail_opt = get_rail_info(bx, by, bz).or_else(|| get_rail_info(bx, by - 1, bz));

        if let Some((rail_type, shape, is_powered)) = rail_opt {
            self.is_on_rail = true;

            let current_speed = vel.length();
            let rail_dir = shape.direction_vector(vel);

            let mut speed = current_speed * 0.98;

            match rail_type {
                RailType::Normal => {}
                RailType::Powered => {
                    if is_powered {
                        speed = (speed + 12.0 * dt).min(16.0);
                    } else {
                        speed *= 0.5;
                    }
                }
                RailType::Detector => {
                    set_detector_powered(bx, by, bz, true);
                }
                RailType::Activator => {}
            }

            vel = rail_dir * speed;
            pos += vel * dt;

            let rail_y = if get_rail_info(bx, by, bz).is_some() {
                by as f32 + 0.1
            } else {
                (by - 1) as f32 + 0.1
            };
            pos.y = pos.y * 0.8 + rail_y * 0.2;
        } else {
            self.is_on_rail = false;
            vel.y -= 18.0 * dt;
            vel.x *= 0.8;
            vel.z *= 0.8;
            pos += vel * dt;
        }

        self.set_pos(pos);
        self.set_vel(vel);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RailType {
    Normal,
    Powered,
    Detector,
    Activator,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rail_shape_conversion() {
        assert_eq!(RailShape::from_u8(0), RailShape::NorthSouth);
        assert_eq!(RailShape::from_u8(1), RailShape::EastWest);
        assert_eq!(RailShape::from_u8(2), RailShape::AscendingEast);
        assert_eq!(RailShape::from_u8(9), RailShape::NorthEast);
    }

    #[test]
    fn test_minecart_powered_rail_boost() {
        let mut cart = MinecartState::new(Vec3::new(0.5, 1.1, 0.5));
        cart.set_vel(Vec3::new(1.0, 0.0, 0.0));

        let dt = 0.05;
        cart.tick(
            dt,
            |_x, _y, _z| Some((RailType::Powered, RailShape::EastWest, true)),
            |_x, _y, _z, _p| {},
        );

        assert!(cart.is_on_rail);
        assert!(cart.vel_vec3().x > 1.0);
    }
}
