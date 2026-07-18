# Minecraft wgpu 複製版實作計劃 (Implementation Plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 使用 Rust + wgpu + winit 從零開始實現一個具有地形生成、物理碰撞和方塊挖掘/放置功能的 Minecraft 複製版。

**Architecture:** 採用 ECS/模組化設計。以 winit 作為視窗主循環，wgpu 管理現代渲染管線與 GPU 狀態。地圖劃分為 $16 \times 256 \times 16$ 的 Chunk，只渲染暴露於空氣的面（Face Culling）。利用 AABB 進行玩家物理碰撞，以 DDA 射線檢測進行方塊的動態挖掘與放置。

**Tech Stack:** Rust 1.78.0, wgpu 0.19, winit 0.29, glam 0.25, noise 0.8, image 0.24, pollster 0.3

---

## 預計建立的檔案結構
*   `Cargo.toml`：管理專案依賴。
*   `src/main.rs`：程式入口點。
*   `src/app.rs`：winit 視窗事件分發與遊戲狀態。
*   `src/state.rs`：wgpu 渲染上下文、資源加載與渲染流程。
*   `src/camera.rs`：相機矩陣、投影矩陣與視角變更。
*   `src/world.rs`：Chunk 數據、方塊 ID 與 Face Culling 網格生成。
*   `src/physics.rs`：玩家 AABB、重力、阻力與碰撞修正。
*   `src/interaction.rs`：DDA 射線檢測算法與方塊破壞/放置邏輯。
*   `src/texture.rs`：紋理與圖集 (Texture Atlas) 加載。
*   `src/shader.wgsl`：頂點與片元著色器代碼。

---

### Task 1: 專案初始化與 Cargo 設定

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`

- [ ] **Step 1: 建立 Cargo.toml 檔案**

在 `f:/Desktop/MC/Cargo.toml` 寫入依賴配置：
```toml
[package]
name = "minecraft_clone"
version = "0.1.0"
edition = "2021"

[dependencies]
winit = "0.29"
wgpu = "0.19"
glam = "0.25"
noise = "0.8"
image = "0.24"
pollster = "0.3"
```

- [ ] **Step 2: 建立基礎入口點 src/main.rs**

在 `f:/Desktop/MC/src/main.rs` 寫入：
```rust
fn main() {
    println!("Minecraft Clone initialized!");
}
```

- [ ] **Step 3: 編譯並運行專案進行驗證**

Run: `cargo run`
Expected: 輸出 `Minecraft Clone initialized!` 且成功編譯。

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml src/main.rs
git commit -m "feat: initialize cargo project"
```

---

### Task 2: winit 視窗與遊戲主循環建立

**Files:**
- Create: `src/app.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: 實作 winit ApplicationHandler**

建立 `f:/Desktop/MC/src/app.rs` 寫入視窗管理結構：
```rust
use winit::{
    application::ApplicationHandler,
    event::*,
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

pub struct App {
    window: Option<Window>,
}

impl App {
    pub fn new() -> Self {
        Self { window: None }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let window = event_loop
                .create_window(Window::default_attributes().with_title("Minecraft wgpu Clone"))
                .unwrap();
            self.window = Some(window);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested
            | WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        logical_key: winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape),
                        ..
                    },
                ..
            } => {
                println!("Esc pressed, exiting...");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}
```

- [ ] **Step 2: 在 main.rs 中啟動 EventLoop**

修改 `f:/Desktop/MC/src/main.rs`：
```rust
mod app;

use app::App;
use winit::event_loop::EventLoop;

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new();
    event_loop.run_app(&mut app).unwrap();
}
```

- [ ] **Step 3: 運行驗證**

Run: `cargo run`
Expected: 彈出一個標題為 `Minecraft wgpu Clone` 的空白視窗，按下 `Esc` 鍵或點選關閉按鈕時，視窗順暢關閉且控制台輸出 `Esc pressed, exiting...`。

- [ ] **Step 4: Commit**

```bash
git add src/app.rs src/main.rs
git commit -m "feat: setup winit window and event loop"
```

---

### Task 3: wgpu 顯示設定與基礎渲染器初始化

**Files:**
- Create: `src/state.rs`
- Create: `src/shader.wgsl`
- Modify: `src/app.rs`

- [ ] **Step 1: 建立基礎著色器 shader.wgsl**

建立 `f:/Desktop/MC/src/shader.wgsl`：
```wgsl
struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
};

@vertex
fn vs_main(model: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(model.position, 1.0);
    out.color = model.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
```

- [ ] **Step 2: 實作 State 結構管理 wgpu 上下文**

建立 `f:/Desktop/MC/src/state.rs`。本模組初始化 wgpu Adapter, Device, Queue, RenderPipeline，並處理清屏（背景設為天藍色）：
```rust
use winit::window::Window;

pub struct State {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,
    render_pipeline: wgpu::RenderPipeline,
}

impl State {
    pub async fn new(window: Window) -> Self {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // 為了將 window 生命週期傳遞給 surface，我們安全地將 window 進行靜態轉換
        let surface = unsafe {
            let surface = instance.create_surface(&window).unwrap();
            std::mem::transmute::<wgpu::Surface<'_>, wgpu::Surface<'static>>(surface)
        };

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    features: wgpu::Features::empty(),
                    limits: wgpu::Limits::default(),
                    label: None,
                },
                None,
            )
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!("shader.wgsl"))),
        });

        let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        Self {
            surface,
            device,
            queue,
            config,
            size,
            render_pipeline,
        }
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        {
            // 天藍色背景清屏
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.53,
                            g: 0.81,
                            b: 0.92,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }
}
```

- [ ] **Step 3: 在 App 結構中整合 State**

修改 `f:/Desktop/MC/src/app.rs` 引入並初始化 `State`：
```rust
use crate::state::State;
use winit::{
    application::ApplicationHandler,
    event::*,
    event_loop::ActiveEventLoop,
    window::{Window, WindowId},
};

pub struct App {
    state: Option<State>,
    window: Option<Window>,
}

impl App {
    pub fn new() -> Self {
        Self {
            state: None,
            window: None,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let window = event_loop
                .create_window(Window::default_attributes().with_title("Minecraft wgpu Clone"))
                .unwrap();
            let state = pollster::block_on(State::new(window));
            self.state = Some(state);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested
            | WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        logical_key: winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape),
                        ..
                    },
                ..
            } => {
                event_loop.exit();
            }
            WindowEvent::Resized(physical_size) => {
                if let Some(state) = &mut self.state {
                    state.resize(physical_size);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(state) = &mut self.state {
                    match state.render() {
                        Ok(_) => {}
                        Err(wgpu::SurfaceError::Lost) => state.resize(state.size),
                        Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                        Err(e) => eprintln!("{:?}", e),
                    }
                }
            }
            _ => {}
        }
    }
}
```

- [ ] **Step 4: 在 main.rs 聲明 state 模組**

修改 `f:/Desktop/MC/src/main.rs`：
```rust
mod app;
mod state;

use app::App;
use winit::event_loop::EventLoop;

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new();
    event_loop.run_app(&mut app).unwrap();
}
```

- [ ] **Step 5: 運行驗證**

Run: `cargo run`
Expected: 彈出視窗且背景呈現為天藍色 (Sky Blue)，調整視窗大小時天藍色平滑展開，關閉無異常。

- [ ] **Step 6: Commit**

```bash
git add src/state.rs src/shader.wgsl src/app.rs src/main.rs
git commit -m "feat: initialize wgpu renderer with skyblue clear color"
```

---

### Task 4: 建立相機 (Camera) 與矩陣變換上傳

**Files:**
- Create: `src/camera.rs`
- Modify: `src/state.rs`
- Modify: `src/shader.wgsl`

- [ ] **Step 1: 建立 Camera 模組與矩陣計算**

建立 `f:/Desktop/MC/src/camera.rs`。使用 `glam` 來計算 View 與 Projection 矩陣：
```rust
use glam::{Mat4, Vec3};

pub struct Camera {
    pub position: Vec3,
    pub yaw: f32,   // 弧度
    pub pitch: f32, // 弧度
}

impl Camera {
    pub fn new(position: Vec3, yaw: f32, pitch: f32) -> Self {
        Self { position, yaw, pitch }
    }

    pub fn build_view_projection_matrix(&self, aspect: f32) -> Mat4 {
        let target = self.position + Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        );
        let view = Mat4::look_at_lh(self.position, target, Vec3::Y);
        let proj = Mat4::perspective_lh(f32::to_radians(60.0), aspect, 0.1, 100.0);
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
```

- [ ] **Step 2: 修改著色器以支援相機矩陣與 3D 頂點**

修改 `f:/Desktop/MC/src/shader.wgsl`，加入 3D Uniform 的變換：
```wgsl
struct CameraUniform {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) tex_coords: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
};

@vertex
fn vs_main(model: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = camera.view_proj * vec4<f32>(model.position, 1.0);
    out.tex_coords = model.tex_coords;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // 當前暫用 UV 進行簡單著色以驗證 3D 投影效果
    return vec4<f32>(in.tex_coords, 0.5, 1.0);
}
```

- [ ] **Step 3: 修改 state.rs 初始化 Camera Uniform 與 Bind Group**

在 `f:/Desktop/MC/src/state.rs` 加入 `Camera` 和 Uniform 緩衝區建立，並加入 `bytemuck` 作為依賴以支援 Pod 轉換：
修改 `Cargo.toml` 新增 `bytemuck` 依賴：
```toml
bytemuck = { version = "1.16", features = ["derive"] }
```

修改 `f:/Desktop/MC/src/state.rs` 中 `State` 結構與 `new`、`render` 方法：
```rust
// 新增引入
use crate::camera::{Camera, CameraUniform};
use glam::Vec3;
use wgpu::util::DeviceExt;

// 於 State 結構體新增屬性：
pub struct State {
    // ... 原有屬性
    camera: Camera,
    camera_uniform: CameraUniform,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
}

// 在 State::new 內建立 Uniform 資源：
// 1. 初始化 Camera
let camera = Camera::new(Vec3::new(0.0, 5.0, -10.0), f32::to_radians(90.0), f32::to_radians(-20.0));
let mut camera_uniform = CameraUniform::new();
camera_uniform.update_view_proj(&camera, config.width as f32 / config.height as f32);

// 2. 建立 Buffer
let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
    label: Some("Camera Buffer"),
    contents: bytemuck::cast_slice(&[camera_uniform]),
    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
});

// 3. 建立 Bind Group Layout 和 Bind Group
let camera_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
    entries: &[wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::VERTEX,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }],
    label: Some("camera_bind_group_layout"),
});

let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
    layout: &camera_bind_group_layout,
    entries: &[wgpu::BindGroupEntry {
        binding: 0,
        resource: camera_buffer.as_entire_binding(),
    }],
    label: Some("camera_bind_group"),
});

// 4. 註冊 Pipeline Layout
let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
    label: Some("Render Pipeline Layout"),
    bind_group_layouts: &[&camera_bind_group_layout],
    push_constant_ranges: &[],
});

// 5. 修改渲染管線的頂點緩衝格式描述：
// 建立一個頂點結構體描述
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub tex_coords: [f32; 2],
}

impl Vertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

// 在 Pipeline Descriptor 中套用描述：
// vertex.buffers = &[Vertex::desc()],

// 6. 在 render 函數中設置 Bind Group:
// render_pass.set_pipeline(&self.render_pipeline);
// render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
```

- [ ] **Step 4: 在 main.rs 聲明 camera 模組**

修改 `f:/Desktop/MC/src/main.rs`：
```rust
mod app;
mod state;
mod camera;
```

- [ ] **Step 5: 編譯測試**

Run: `cargo check`
Expected: 順利通過編譯，無語法錯誤。

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml src/camera.rs src/state.rs src/shader.wgsl src/main.rs
git commit -m "feat: add Camera math and uniform buffer binding in wgpu"
```

---

### Task 5: 區塊 (Chunk) 數據結構與 Face Culling 網格生成

**Files:**
- Create: `src/world.rs`
- Modify: `src/state.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: 建立 Chunk 與網格生成邏輯**

建立 `f:/Desktop/MC/src/world.rs`。實現三維方塊陣列，以及 Face Culling 的網格產生演算法：
```rust
use crate::state::Vertex;

pub const CHUNK_WIDTH: usize = 16;
pub const CHUNK_HEIGHT: usize = 256;
pub const CHUNK_DEPTH: usize = 16;

#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u8)]
pub enum BlockType {
    Air = 0,
    Grass = 1,
    Dirt = 2,
    Stone = 3,
}

pub struct Chunk {
    pub blocks: [[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH],
}

impl Chunk {
    pub fn new() -> Self {
        let mut blocks = [[[BlockType::Air; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH];
        // 簡單填充地面
        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                for y in 0..64 {
                    blocks[x][y][z] = BlockType::Stone;
                }
                for y in 64..68 {
                    blocks[x][y][z] = BlockType::Dirt;
                }
                blocks[x][68][z] = BlockType::Grass;
            }
        }
        Self { blocks }
    }

    pub fn get_block(&self, x: i32, y: i32, z: i32) -> BlockType {
        if x < 0 || x >= CHUNK_WIDTH as i32 || y < 0 || y >= CHUNK_HEIGHT as i32 || z < 0 || z >= CHUNK_DEPTH as i32 {
            return BlockType::Air; // 超出範圍視為空氣
        }
        self.blocks[x as usize][y as usize][z as usize]
    }

    // 生成用於渲染的頂點和索引
    pub fn generate_mesh(&self) -> (Vec<Vertex>, Vec<u32>) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        // 方塊的 6 個面法線偏移量與面頂點定義
        // 順序：前、後、左、右、上、下
        let faces = [
            // 前面 (South) (0, 0, 1)
            ([0.0, 0.0, 1.0], [
                ([0.0, 0.0, 1.0], [0.0, 1.0]),
                ([1.0, 0.0, 1.0], [1.0, 1.0]),
                ([1.0, 1.0, 1.0], [1.0, 0.0]),
                ([0.0, 1.0, 1.0], [0.0, 0.0]),
            ]),
            // 後面 (North) (0, 0, -1)
            ([0.0, 0.0, -1.0], [
                ([1.0, 0.0, 0.0], [0.0, 1.0]),
                ([0.0, 0.0, 0.0], [1.0, 1.0]),
                ([0.0, 1.0, 0.0], [1.0, 0.0]),
                ([1.0, 1.0, 0.0], [0.0, 0.0]),
            ]),
            // 左面 (West) (-1, 0, 0)
            ([-1.0, 0.0, 0.0], [
                ([0.0, 0.0, 0.0], [0.0, 1.0]),
                ([0.0, 0.0, 1.0], [1.0, 1.0]),
                ([0.0, 1.0, 1.0], [1.0, 0.0]),
                ([0.0, 1.0, 0.0], [0.0, 0.0]),
            ]),
            // 右面 (East) (1, 0, 0)
            ([1.0, 0.0, 0.0], [
                ([1.0, 0.0, 1.0], [0.0, 1.0]),
                ([1.0, 0.0, 0.0], [1.0, 1.0]),
                ([1.0, 1.0, 0.0], [1.0, 0.0]),
                ([1.0, 1.0, 1.0], [0.0, 0.0]),
            ]),
            // 上面 (Up) (0, 1, 0)
            ([0.0, 1.0, 0.0], [
                ([0.0, 1.0, 1.0], [0.0, 1.0]),
                ([1.0, 1.0, 1.0], [1.0, 1.0]),
                ([1.0, 1.0, 0.0], [1.0, 0.0]),
                ([0.0, 1.0, 0.0], [0.0, 0.0]),
            ]),
            // 下面 (Down) (0, -1, 0)
            ([0.0, -1.0, 0.0], [
                ([0.0, 0.0, 0.0], [0.0, 1.0]),
                ([1.0, 0.0, 0.0], [1.0, 1.0]),
                ([1.0, 0.0, 1.0], [1.0, 0.0]),
                ([0.0, 0.0, 1.0], [0.0, 0.0]),
            ]),
        ];

        for x in 0..CHUNK_WIDTH {
            for y in 0..CHUNK_HEIGHT {
                for z in 0..CHUNK_DEPTH {
                    let block = self.blocks[x][y][z];
                    if block == BlockType::Air {
                        continue;
                    }

                    let px = x as f32;
                    let py = y as f32;
                    let pz = z as f32;

                    for (face_idx, (normal, corner_data)) in faces.iter().enumerate() {
                        let nx = x as i32 + normal[0] as i32;
                        let ny = y as i32 + normal[1] as i32;
                        let nz = z as i32 + normal[2] as i32;

                        // Face Culling: 檢查相鄰區塊是否透明
                        let neighbor = self.get_block(nx, ny, nz);
                        if neighbor == BlockType::Air {
                            let start_idx = vertices.len() as u32;

                            for (offset, uv) in corner_data.iter() {
                                vertices.push(Vertex {
                                    position: [px + offset[0], py + offset[1], pz + offset[2]],
                                    tex_coords: *uv,
                                });
                            }

                            indices.push(start_idx + 0);
                            indices.push(start_idx + 1);
                            indices.push(start_idx + 2);
                            indices.push(start_idx + 0);
                            indices.push(start_idx + 2);
                            indices.push(start_idx + 3);
                        }
                    }
                }
            }
        }

        (vertices, indices)
    }
}
```

- [ ] **Step 2: 修改 state.rs 將 Chunk 網格載入並渲染**

修改 `f:/Desktop/MC/src/state.rs`，加入對 Depth Buffer (深度測試) 的配置，以及將生成的頂點載入 GPU 渲染：
首先在 `State` 結構體新增：
```rust
use crate::world::Chunk;

pub struct State {
    // ...
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    num_indices: u32,
    depth_texture: wgpu::TextureView,
}
```

在 `State::new` 內：
1. 建立 Depth Buffer 支援 3D 前後遮擋判定：
```rust
let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
    label: Some("Depth Texture"),
    size: wgpu::Extent3d {
        width: size.width,
        height: size.height,
        depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: wgpu::TextureFormat::Depth32Float,
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    view_formats: &[],
});
let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());
```
2. 將渲染管線中的 `depth_stencil` 設為：
```rust
depth_stencil: Some(wgpu::DepthStencilState {
    format: wgpu::TextureFormat::Depth32Float,
    depth_write_enabled: true,
    depth_compare: wgpu::CompareFunction::Less,
    stencil: wgpu::StencilState::default(),
    bias: wgpu::DepthBiasState::default(),
}),
```
3. 生成 Chunk 網格並填充緩衝區：
```rust
let chunk = Chunk::new();
let (vertices, indices) = chunk.generate_mesh();
let num_indices = indices.len() as u32;

let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
    label: Some("Vertex Buffer"),
    contents: bytemuck::cast_slice(&vertices),
    usage: wgpu::BufferUsages::VERTEX,
});

let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
    label: Some("Index Buffer"),
    contents: bytemuck::cast_slice(&indices),
    usage: wgpu::BufferUsages::INDEX,
});
```

在 `render` 中，將 `depth_stencil_attachment` 加到 `RenderPass` 並繪製：
```rust
let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
    label: Some("Render Pass"),
    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
        view: &view,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.53, g: 0.81, b: 0.92, a: 1.0 }),
            store: wgpu::StoreOp::Store,
        },
    })],
    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
        view: &self.depth_view,
        depth_ops: Some(wgpu::Operations {
            load: wgpu::LoadOp::Clear(1.0),
            store: wgpu::StoreOp::Store,
        }),
        stencil_ops: None,
    }),
    occlusion_query_set: None,
    timestamp_writes: None,
});

render_pass.set_pipeline(&self.render_pipeline);
render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
render_pass.draw_indexed(0..self.num_indices, 0, 0..1);
```

- [ ] **Step 3: 修改 main.rs 引入 world**

修改 `f:/Desktop/MC/src/main.rs`：
```rust
mod app;
mod state;
mod camera;
mod world;
```

- [ ] **Step 4: 編譯並運行測試**

Run: `cargo run`
Expected: 畫面上渲染出一個由彩色方塊組成的巨大立體平台，且有正確的前後遮擋與遠近投影。

- [ ] **Step 5: Commit**

```bash
git add src/world.rs src/state.rs src/main.rs
git commit -m "feat: render 3D Chunk platform with face culling and depth testing"
```

---

### Task 6: 物理系統與 AABB 碰撞偵測

**Files:**
- Create: `src/physics.rs`
- Modify: `src/app.rs`
- Modify: `src/state.rs`

- [ ] **Step 1: 實作 AABB 與碰撞修正**

建立 `f:/Desktop/MC/src/physics.rs`。定義玩家的 AABB 盒子與運動邏輯，以及針對 World 方塊的碰撞排除演算法：
```rust
use glam::Vec3;
use crate::world::Chunk;

pub struct AABB {
    pub min: Vec3,
    pub max: Vec3,
}

impl AABB {
    pub fn new(center: Vec3, size: Vec3) -> Self {
        let half = size * 0.5;
        Self {
            min: center - half,
            max: center + half,
        }
    }

    pub fn intersects(&self, other: &AABB) -> bool {
        self.min.x < other.max.x
            && self.max.x > other.min.x
            && self.min.y < other.max.y
            && self.max.y > other.min.y
            && self.min.z < other.max.z
            && self.max.z > other.min.z
    }
}

pub struct PlayerPhysics {
    pub position: Vec3,
    pub velocity: Vec3,
    pub size: Vec3,
    pub on_ground: bool,
}

impl PlayerPhysics {
    pub fn new(position: Vec3) -> Self {
        Self {
            position,
            velocity: Vec3::ZERO,
            size: Vec3::new(0.6, 1.8, 0.6), // Minecraft 玩家寬高
            on_ground: false,
        }
    }

    pub fn get_aabb(&self) -> AABB {
        AABB::new(self.position + Vec3::new(0.0, self.size.y * 0.5, 0.0), self.size)
    }

    pub fn update(&mut self, dt: f32, chunk: &Chunk, movement_input: Vec3) {
        // 1. 套用玩家移動控制
        let speed = 8.0;
        self.velocity.x = movement_input.x * speed;
        self.velocity.z = movement_input.z * speed;

        // 2. 套用重力
        self.velocity.y -= 32.0 * dt;
        if self.velocity.y < -50.0 {
            self.velocity.y = -50.0; // 終端速度
        }

        // 3. 沿 X 軸位移並處理碰撞
        self.position.x += self.velocity.x * dt;
        self.resolve_collisions(chunk, 0);

        // 4. 沿 Z 軸位移並處理碰撞
        self.position.z += self.velocity.z * dt;
        self.resolve_collisions(chunk, 2);

        // 5. 沿 Y 軸位移並處理碰撞
        self.position.y += self.velocity.y * dt;
        self.on_ground = false;
        self.resolve_collisions(chunk, 1);
    }

    fn resolve_collisions(&mut self, chunk: &Chunk, axis: usize) {
        let player_aabb = self.get_aabb();

        // 檢測玩家周圍可能相交的方塊
        let min_x = (player_aabb.min.x.floor() as i32).max(0);
        let max_x = (player_aabb.max.x.floor() as i32).max(0);
        let min_y = (player_aabb.min.y.floor() as i32).max(0);
        let max_y = (player_aabb.max.y.floor() as i32).max(0);
        let min_z = (player_aabb.min.z.floor() as i32).max(0);
        let max_z = (player_aabb.max.z.floor() as i32).max(0);

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                for z in min_z..=max_z {
                    let block = chunk.get_block(x, y, z);
                    if block != crate::world::BlockType::Air {
                        let block_aabb = AABB::new(
                            Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5),
                            Vec3::ONE,
                        );

                        if self.get_aabb().intersects(&block_aabb) {
                            if axis == 0 { // X 軸
                                if self.velocity.x > 0.0 {
                                    self.position.x = block_aabb.min.x - self.size.x * 0.5;
                                } else {
                                    self.position.x = block_aabb.max.x + self.size.x * 0.5;
                                }
                                self.velocity.x = 0.0;
                            } else if axis == 2 { // Z 軸
                                if self.velocity.z > 0.0 {
                                    self.position.z = block_aabb.min.z - self.size.x * 0.5;
                                } else {
                                    self.position.z = block_aabb.max.z + self.size.x * 0.5;
                                }
                                self.velocity.z = 0.0;
                            } else if axis == 1 { // Y 軸
                                if self.velocity.y > 0.0 {
                                    self.position.y = block_aabb.min.y - self.size.y;
                                } else {
                                    self.position.y = block_aabb.max.y;
                                    self.on_ground = true;
                                }
                                self.velocity.y = 0.0;
                            }
                        }
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 2: 撰寫物理碰撞單元測試**

在 `src/physics.rs` 底部加上單元測試：
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::BlockType;

    #[test]
    fn test_aabb_intersection() {
        let box1 = AABB::new(Vec3::new(0.0, 0.0, 0.0), Vec3::ONE);
        let box2 = AABB::new(Vec3::new(0.8, 0.0, 0.0), Vec3::ONE);
        let box3 = AABB::new(Vec3::new(1.5, 0.0, 0.0), Vec3::ONE);

        assert!(box1.intersects(&box2));
        assert!(!box1.intersects(&box3));
    }
}
```

- [ ] **Step 3: 運行單元測試驗證**

Run: `cargo test`
Expected: 測試順利通過 (PASS)。

- [ ] **Step 4: 在 State 中整合物理位移與鍵盤輸入**

修改 `f:/Desktop/MC/src/state.rs` 使其在每一幀的 `update` 函數中執行物理模擬：
在 `State` 結構體新增：
```rust
use crate::physics::PlayerPhysics;
// 屬性：
pub player_physics: PlayerPhysics,
pub keyboard_input: Vec3, // 用於記錄當前幀移動增量 (WASD)
```
在 `State::new` 中初始化：
```rust
let player_physics = PlayerPhysics::new(Vec3::new(8.0, 80.0, 8.0)); // 起始站在 Chunk 地面上空
```
在 `State` 結構體實作 `update`：
```rust
pub fn update(&mut self, dt: f32) {
    self.player_physics.update(dt, &self.chunk, self.keyboard_input);
    // 連動更新相機位置
    self.camera.position = self.player_physics.position + Vec3::new(0.0, 1.6, 0.0); // 眼睛高度
    self.camera_uniform.update_view_proj(&self.camera, self.config.width as f32 / self.config.height as f32);
    self.queue.write_buffer(&self.camera_buffer, 0, bytemuck::cast_slice(&[self.camera_uniform]));
}
```

- [ ] **Step 5: 修改 app.rs 捕獲移動按鍵與每幀時鐘更新**

修改 `f:/Desktop/MC/src/app.rs` 引入 delta_time 計算並傳遞鍵盤按鍵狀態：
```rust
use std::time::Instant;
// 在 App 內新增屬性：
last_render_time: Instant,

// 在 resumed 時初始化 last_render_time = Instant::now()

// 在 window_event 中捕獲鍵盤移動按鍵：
// WindowEvent::KeyboardInput { event, .. }
// 根據按鍵，更新 state.keyboard_input 的向量值 (WASD & Space)
```

- [ ] **Step 6: 編譯運行測試**

Run: `cargo run`
Expected: 玩家受重力掉落，在 Y=69.0 (草地表面) 停止掉落，且可使用鍵盤 WASD 移動。

- [ ] **Step 7: Commit**

```bash
git add src/physics.rs src/state.rs src/app.rs src/main.rs
git commit -m "feat: add AABB physics, gravity collision, and keyboard movement"
```

---

### Task 7: 射線檢測與方塊挖掘/放置 (滑鼠互動)

**Files:**
- Create: `src/interaction.rs`
- Modify: `src/state.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: 實作 DDA 3D 射線演算法**

建立 `f:/Desktop/MC/src/interaction.rs`。實作在 3D Voxel 網格中前進的 DDA 算法：
```rust
use glam::Vec3;
use crate::world::{Chunk, BlockType};

pub struct RaycastResult {
    pub block_pos: Vec3, // 命中的方塊整數座標
    pub normal: Vec3,    // 命中的表面法線（用於放置新方塊）
}

pub fn raycast(origin: Vec3, direction: Vec3, max_dist: f32, chunk: &Chunk) -> Option<RaycastResult> {
    let mut x = origin.x.floor() as i32;
    let mut y = origin.y.floor() as i32;
    let mut z = origin.z.floor() as i32;

    let step_x = if direction.x > 0.0 { 1 } else { -1 };
    let step_y = if direction.y > 0.0 { 1 } else { -1 };
    let step_z = if direction.z > 0.0 { 1 } else { -1 };

    let t_delta_x = (1.0 / direction.x).abs();
    let t_delta_y = (1.0 / direction.y).abs();
    let t_delta_z = (1.0 / direction.z).abs();

    let mut t_max_x = if direction.x > 0.0 { (x as f32 + 1.0 - origin.x) * t_delta_x } else { (origin.x - x as f32) * t_delta_x };
    let mut t_max_y = if direction.y > 0.0 { (y as f32 + 1.0 - origin.y) * t_delta_y } else { (origin.y - y as f32) * t_delta_y };
    let mut t_max_z = if direction.z > 0.0 { (z as f32 + 1.0 - origin.z) * t_delta_z } else { (origin.z - z as f32) * t_delta_z };

    let mut t = 0.0;
    let mut last_face = Vec3::ZERO;

    while t < max_dist {
        let block = chunk.get_block(x, y, z);
        if block != BlockType::Air {
            return Some(RaycastResult {
                block_pos: Vec3::new(x as f32, y as f32, z as f32),
                normal: last_face,
            });
        }

        if t_max_x < t_max_y {
            if t_max_x < t_max_z {
                t = t_max_x;
                x += step_x;
                t_max_x += t_delta_x;
                last_face = Vec3::new(-step_x as f32, 0.0, 0.0);
            } else {
                t = t_max_z;
                z += step_z;
                t_max_z += t_delta_z;
                last_face = Vec3::new(0.0, 0.0, -step_z as f32);
            }
        } else {
            if t_max_y < t_max_z {
                t = t_max_y;
                y += step_y;
                t_max_y += t_delta_y;
                last_face = Vec3::new(0.0, -step_y as f32, 0.0);
            } else {
                t = t_max_z;
                z += step_z;
                t_max_z += t_delta_z;
                last_face = Vec3::new(0.0, 0.0, -step_z as f32);
            }
        }
    }

    None
}
```

- [ ] **Step 2: 在 State 中實作方塊變更後重新生成 GPU 網格**

修改 `f:/Desktop/MC/src/state.rs`，加入更新 Chunk 方塊與重建 Vertex/Index Buffer 的功能：
```rust
use crate::interaction::raycast;

impl State {
    pub fn handle_click(&mut self, is_left_click: bool) {
        let dir = Vec3::new(
            self.camera.yaw.cos() * self.camera.pitch.cos(),
            self.camera.pitch.sin(),
            self.camera.yaw.sin() * self.camera.pitch.cos(),
        );

        if let Some(hit) = raycast(self.camera.position, dir, 5.0, &self.chunk) {
            if is_left_click {
                // 挖掘：設為 Air
                let bx = hit.block_pos.x as usize;
                let by = hit.block_pos.y as usize;
                let bz = hit.block_pos.z as usize;
                self.chunk.blocks[bx][by][bz] = crate::world::BlockType::Air;
            } else {
                // 放置：在法線方向放置 Stone
                let target = hit.block_pos + hit.normal;
                let bx = target.x as usize;
                let by = target.y as usize;
                let bz = target.z as usize;
                if bx < crate::world::CHUNK_WIDTH && by < crate::world::CHUNK_HEIGHT && bz < crate::world::CHUNK_DEPTH {
                    self.chunk.blocks[bx][by][bz] = crate::world::BlockType::Stone;
                }
            }
            // 重新生成網格並更新 GPU 緩衝區
            let (vertices, indices) = self.chunk.generate_mesh();
            self.num_indices = indices.len() as u32;
            self.queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&vertices));
            self.queue.write_buffer(&self.index_buffer, 0, bytemuck::cast_slice(&indices));
        }
    }
}
```

- [ ] **Step 3: 修改 app.rs 捕獲滑鼠點擊與滑鼠旋轉相機**

修改 `f:/Desktop/MC/src/app.rs` 捕獲 `WindowEvent::MouseInput` 並調用 `state.handle_click()`。
同時導入滑鼠鎖定與 CursorDelta，使玩家滑鼠滑動能更新相機的 `yaw` 與 `pitch`。

- [ ] **Step 4: 編譯運行測試**

Run: `cargo run`
Expected: 鼠標鎖定於畫面，轉動滑鼠可平滑環顧四周。對著方塊點滑鼠左鍵可將其摧毀，點滑鼠右鍵可在其表面加蓋新方塊。

- [ ] **Step 5: Commit**

```bash
git add src/interaction.rs src/state.rs src/app.rs src/main.rs
git commit -m "feat: implement DDA raycasting, block mining and block placing"
```

---

## 執行方案選擇

此實作計劃已建立，請選擇您偏好的執行方式：

1.  **Subagent-Driven (推薦)** - 我會建立一系列獨立的子代理人 (Subagent) 逐步執行這 7 個 Task，並在每個 Task 結束時由您確認，迭代效率最高。
2.  **Inline Execution (行內執行)** - 我在目前對話中逐步執行，並設有檢查點供您審查代碼。
