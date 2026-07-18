struct CameraUniform {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    sky_color_top: vec4<f32>,
    sky_color_horizon: vec4<f32>,
    sun_dir: vec4<f32>,
    fog_start: f32,
    fog_end: f32,
    padding: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

@group(0) @binding(1)
var t_diffuse: texture_2d<f32>;
@group(0) @binding(2)
var s_diffuse: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) tex_coords: vec2<f32>,
    @location(2) light_level: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    @location(1) light_level: f32,
    @location(2) world_pos: vec3<f32>,
};

@vertex
fn vs_main(model: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = camera.view_proj * vec4<f32>(model.position, 1.0);
    out.tex_coords = model.tex_coords;
    out.light_level = model.light_level;
    out.world_pos = model.position;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    if (color.a < 0.5) {
        discard;
    }

    // Unpack lighting
    let packed = in.light_level;
    let multiplier_code = floor(packed / 256.0);
    let rest = packed - multiplier_code * 256.0;
    let block_light = floor(rest / 16.0);
    let sky_light = rest - block_light * 16.0;

    var multiplier = 1.0;
    if (multiplier_code > 1.5) {
        multiplier = 0.5;
    } else if (multiplier_code > 0.5) {
        multiplier = 0.8;
    }

    // Dynamically scale sky light with global intensity
    let sky_intensity = camera.sun_dir.w;
    let adjusted_sky_light = sky_light * sky_intensity;
    let max_light = max(adjusted_sky_light, block_light);

    let ambient = 0.08;
    let final_light = max(max_light / 15.0, ambient) * multiplier;
    let fragment_color = color * final_light;

    let dist = length(in.world_pos - camera.camera_pos.xyz);
    let fog_factor = clamp((dist - camera.fog_start) / (camera.fog_end - camera.fog_start), 0.0, 1.0);

    return mix(fragment_color, camera.sky_color_horizon, fog_factor);
}

@vertex
fn vs_crosshair(model: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(model.position, 1.0);
    out.tex_coords = model.tex_coords;
    out.light_level = model.light_level;
    out.world_pos = model.position;
    return out;
}

@fragment
fn fs_crosshair(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(1.0, 1.0, 1.0, 0.8);
}

struct UiVertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct UiVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_ui(model: UiVertexInput) -> UiVertexOutput {
    var out: UiVertexOutput;
    out.clip_position = vec4<f32>(model.position, 1.0);
    out.color = model.color;
    return out;
}

@fragment
fn fs_ui(in: UiVertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}

struct SkyVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc_pos: vec2<f32>,
};

@vertex
fn vs_sky(@builtin(vertex_index) vertex_index: u32) -> SkyVertexOutput {
    var pos = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0)
    );
    var out: SkyVertexOutput;
    let p = pos[vertex_index];
    out.clip_position = vec4<f32>(p, 0.99999, 1.0);
    out.ndc_pos = p;
    return out;
}

fn hash3(p: vec3<f32>) -> f32 {
    let sin_val = sin(dot(p, vec3<f32>(127.1, 311.7, 74.7)));
    return fract(sin_val * 43758.5453123);
}

fn get_star(dir: vec3<f32>) -> f32 {
    if (dir.y <= 0.0) {
        return 0.0;
    }
    
    let grid_size = 120.0;
    let grid_pos = floor(dir * grid_size);
    
    let h1 = hash3(grid_pos);
    let h2 = hash3(grid_pos + vec3<f32>(1.0, 2.0, 3.0));
    let h3 = hash3(grid_pos + vec3<f32>(4.0, 5.0, 6.0));
    
    if (h1 < 0.992) {
        return 0.0;
    }
    
    let cell_center = (grid_pos + vec3<f32>(0.5, 0.5, 0.5)) / grid_size;
    let offset = (vec3<f32>(h1, h2, h3) - vec3<f32>(0.5)) * 0.4 / grid_size;
    let star_pos = normalize(cell_center + offset);
    
    let d = dot(dir, star_pos);
    let star_size = 0.9998;
    if (d > star_size) {
        let intensity = (d - star_size) / (1.0 - star_size);
        return intensity * h2; 
    }
    return 0.0;
}

@fragment
fn fs_sky(in: SkyVertexOutput) -> @location(0) vec4<f32> {
    let unprojected = camera.inv_view_proj * vec4<f32>(in.ndc_pos.x, in.ndc_pos.y, 1.0, 1.0);
    let world_pos = unprojected.xyz / unprojected.w;
    let view_dir = normalize(world_pos - camera.camera_pos.xyz);

    let h = max(view_dir.y, 0.0);
    var sky_color = mix(camera.sky_color_horizon, camera.sky_color_top, h);

    // Sun
    let sun_dot = dot(view_dir, normalize(camera.sun_dir.xyz));
    if (sun_dot > 0.995) {
        let sun_factor = smoothstep(0.995, 0.997, sun_dot);
        sky_color = mix(sky_color, vec4<f32>(1.0, 1.0, 1.0, 1.0), sun_factor);
    }

    // Moon
    let moon_dot = dot(view_dir, normalize(-camera.sun_dir.xyz));
    if (moon_dot > 0.997) {
        let moon_factor = smoothstep(0.997, 0.998, moon_dot);
        sky_color = mix(sky_color, vec4<f32>(0.9, 0.9, 0.95, 1.0), moon_factor);
    }

    // Stars (with celestial rotation around the Z axis)
    let sun_angle = atan2(camera.sun_dir.y, camera.sun_dir.x);
    let cos_a = cos(-sun_angle);
    let sin_a = sin(-sun_angle);
    let rotated_dir = vec3<f32>(
        view_dir.x * cos_a - view_dir.y * sin_a,
        view_dir.x * sin_a + view_dir.y * cos_a,
        view_dir.z
    );
    
    let star_intensity = smoothstep(0.1, -0.1, camera.sun_dir.y);
    let star_val = get_star(rotated_dir) * star_intensity;
    sky_color = sky_color + vec4<f32>(star_val, star_val, star_val, 0.0);

    return sky_color;
}

struct TexturedUiVertexInput {
    @location(0) position: vec3<f32>,
    @location(1) tex_coords: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct TexturedUiVertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_textured_ui(model: TexturedUiVertexInput) -> TexturedUiVertexOutput {
    var out: TexturedUiVertexOutput;
    out.clip_position = vec4<f32>(model.position, 1.0);
    out.tex_coords = model.tex_coords;
    out.color = model.color;
    return out;
}

@fragment
fn fs_textured_ui(in: TexturedUiVertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    return tex_color * in.color;
}


