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
}

impl CameraUniform {
    pub fn new() -> Self {
        Self {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(),
        }
    }

    pub fn update_view_proj(&mut self, camera: &Camera, aspect: f32) {
        self.view_proj = camera.build_view_projection_matrix(aspect).to_cols_array_2d();
    }
}
