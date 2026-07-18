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
    pub padding: [f32; 2],
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
            padding: [0.0; 2],
        }
    }

    pub fn update_view_proj(&mut self, camera: &Camera, aspect: f32, render_distance: u32) {
        let view_proj = camera.build_view_projection_matrix(aspect);
        self.view_proj = view_proj.to_cols_array_2d();
        self.inv_view_proj = view_proj.inverse().to_cols_array_2d();
        self.camera_pos = [camera.position.x, camera.position.y, camera.position.z, 0.0];
        self.sky_color_top = [0.1, 0.25, 0.45, 1.0];
        self.sky_color_horizon = [0.53, 0.81, 0.92, 1.0];
        
        let raw_sun = Vec3::new(0.5, 0.8, 0.3).normalize();
        self.sun_dir = [raw_sun.x, raw_sun.y, raw_sun.z, 0.0];

        let fog_end = (render_distance as f32) * 16.0;
        self.fog_end = fog_end;
        self.fog_start = fog_end * 0.6;
        self.padding = [0.0; 2];
    }
}
