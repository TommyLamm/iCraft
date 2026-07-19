use glam::{Mat4, Vec3};

pub struct Camera {
    pub position: Vec3,
    pub yaw: f32,   // 弧度
    pub pitch: f32, // 弧度
    pub fov: f32,   // 角度
}

impl Camera {
    pub fn new(position: Vec3, yaw: f32, pitch: f32, fov: f32) -> Self {
        Self { position, yaw, pitch, fov }
    }

    pub fn build_view_projection_matrix(&self, aspect: f32) -> Mat4 {
        let target = self.position + Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        );
        let view = Mat4::look_at_lh(self.position, target, Vec3::Y);
        let proj = Mat4::perspective_lh(f32::to_radians(self.fov), aspect, 0.1, 100.0);
        proj * view
    }
}

// 用於 Uniform 上傳的對齊結構體
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub view_proj: [[f32; 4]; 4],
    pub inv_view_proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 4],
    pub sky_color_top: [f32; 4],
    pub sky_color_horizon: [f32; 4],
    pub sun_dir: [f32; 4],
    pub fog_start: f32,
    pub fog_end: f32,
    pub total_time: f32,
    pub is_underwater: f32,
}

impl CameraUniform {
    pub fn new() -> Self {
        Self {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            inv_view_proj: Mat4::IDENTITY.to_cols_array_2d(),
            camera_pos: [0.0, 0.0, 0.0, 0.0],
            sky_color_top: [0.1, 0.25, 0.45, 1.0],
            sky_color_horizon: [0.53, 0.81, 0.92, 1.0],
            sun_dir: [0.5, 0.8, 0.3, 0.0],
            fog_start: 0.0,
            fog_end: 100.0,
            total_time: 0.0,
            is_underwater: 0.0,
        }
    }

    pub fn update_view_proj(&mut self, camera: &Camera, aspect: f32, render_distance: u32, world_time: &WorldTime, total_time: f32, is_underwater: bool) {
        let view_proj = camera.build_view_projection_matrix(aspect);
        self.view_proj = view_proj.to_cols_array_2d();
        self.inv_view_proj = view_proj.inverse().to_cols_array_2d();
        self.camera_pos = [camera.position.x, camera.position.y, camera.position.z, 0.0];

        let t = world_time.time_of_day_smooth();
        let (sky_top, sky_horizon) = if t < 0.25 {
            let factor = t / 0.25;
            (
                lerp_color(SUNRISE_TOP, DAY_TOP, factor),
                lerp_color(SUNRISE_HORIZON, DAY_HORIZON, factor),
            )
        } else if t < 0.5 {
            let factor = (t - 0.25) / 0.25;
            (
                lerp_color(DAY_TOP, SUNSET_TOP, factor),
                lerp_color(DAY_HORIZON, SUNSET_HORIZON, factor),
            )
        } else if t < 0.75 {
            let factor = (t - 0.5) / 0.25;
            (
                lerp_color(SUNSET_TOP, NIGHT_TOP, factor),
                lerp_color(SUNSET_HORIZON, NIGHT_HORIZON, factor),
            )
        } else {
            let factor = (t - 0.75) / 0.25;
            (
                lerp_color(NIGHT_TOP, SUNRISE_TOP, factor),
                lerp_color(NIGHT_HORIZON, SUNRISE_HORIZON, factor),
            )
        };

        self.sky_color_top = sky_top;
        self.sky_color_horizon = sky_horizon;

        let sun_angle = world_time.sun_angle();
        let x = sun_angle.cos() * 0.95;
        let y = sun_angle.sin() * 0.95;
        let z = 0.3f32;
        let sun_dir_vec = Vec3::new(x, y, z).normalize();
        let sky_intensity = world_time.sky_light_level() as f32 / 15.0;
        self.sun_dir = [sun_dir_vec.x, sun_dir_vec.y, sun_dir_vec.z, sky_intensity];

        let fog_end = (render_distance as f32) * 16.0;
        self.fog_end = fog_end;
        self.fog_start = fog_end * 0.6;
        self.total_time = total_time;
        self.is_underwater = if is_underwater { 1.0 } else { 0.0 };
    }
}

pub struct WorldTime {
    pub ticks: u64,
    pub day_length: u64,
    pub tick_accumulator: f32,
}

impl WorldTime {
    pub fn new() -> Self {
        Self {
            ticks: 6000, // Start at noon
            day_length: 24000,
            tick_accumulator: 0.0,
        }
    }

    pub fn time_of_day_smooth(&self) -> f32 {
        let current_ticks = (self.ticks % self.day_length) as f32 + self.tick_accumulator;
        current_ticks / self.day_length as f32
    }

    pub fn sun_angle(&self) -> f32 {
        self.time_of_day_smooth() * 2.0 * std::f32::consts::PI
    }

    pub fn sky_light_level(&self) -> u8 {
        let angle = self.sun_angle();
        let sin_angle = angle.sin();
        if sin_angle >= 0.2 {
            15
        } else if sin_angle <= -0.2 {
            4
        } else {
            let t = (sin_angle + 0.2) / 0.4;
            (4.0 + t * 11.0).round() as u8
        }
    }
}

fn lerp_color(c1: [f32; 4], c2: [f32; 4], t: f32) -> [f32; 4] {
    [
        c1[0] + (c2[0] - c1[0]) * t,
        c1[1] + (c2[1] - c1[1]) * t,
        c1[2] + (c2[2] - c1[2]) * t,
        c1[3] + (c2[3] - c1[3]) * t,
    ]
}

const SUNRISE_TOP: [f32; 4] = [0.1, 0.15, 0.3, 1.0];
const SUNRISE_HORIZON: [f32; 4] = [0.9, 0.5, 0.2, 1.0];
const DAY_TOP: [f32; 4] = [0.1, 0.25, 0.45, 1.0];
const DAY_HORIZON: [f32; 4] = [0.53, 0.81, 0.92, 1.0];
const SUNSET_TOP: [f32; 4] = [0.05, 0.1, 0.25, 1.0];
const SUNSET_HORIZON: [f32; 4] = [0.9, 0.4, 0.15, 1.0];
const NIGHT_TOP: [f32; 4] = [0.01, 0.01, 0.03, 1.0];
const NIGHT_HORIZON: [f32; 4] = [0.02, 0.02, 0.05, 1.0];

