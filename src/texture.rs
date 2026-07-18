use wgpu::{Device, Queue, Texture, TextureView, Sampler};
use image::{RgbaImage, Rgba};

pub struct TextureAtlas {
    pub texture: Texture,
    pub view: TextureView,
    pub sampler: Sampler,
}

impl TextureAtlas {
    pub fn new_procedural(device: &Device, queue: &Queue) -> Self {
        // Generate a 64x64 texture atlas
        // 4 sub-textures of 16x16 on the first row
        // Col 0: Grass Top, Col 1: Grass Side, Col 2: Dirt, Col 3: Stone
        let mut img = RgbaImage::new(64, 64);
        
        // Deterministic pseudo-random number generator
        let mut rng = 12345u32;
        let mut next_rand = |min: u8, max: u8| -> u8 {
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            let val = (rng / 65536) % 32768;
            let diff = max - min;
            if diff == 0 {
                return min;
            }
            min + (val % diff as u32) as u8
        };

        for y in 0..64 {
            for x in 0..64 {
                let col = x / 16;
                let row = y / 16;
                
                let pixel = if row == 0 {
                    match col {
                        0 => { // Grass Top: shades of green
                            let g = next_rand(120, 180);
                            let r = g / 2;
                            let b = g / 3;
                            Rgba([r, g, b, 255])
                        }
                        1 => { // Grass Side: grass hanging over dirt
                            let local_y = y % 16;
                            // Jagged grass edge
                            let grass_height = if (x % 4 == 0) || (x % 3 == 0) { 5 } else { 3 };
                            if local_y < grass_height {
                                let g = next_rand(120, 180);
                                let r = g / 2;
                                let b = g / 3;
                                Rgba([r, g, b, 255])
                            } else {
                                let r = next_rand(90, 130);
                                let g = (r as f32 * 0.6) as u8;
                                let b = (r as f32 * 0.4) as u8;
                                Rgba([r, g, b, 255])
                            }
                        }
                        2 => { // Dirt: shades of brown
                            let r = next_rand(90, 130);
                            let g = (r as f32 * 0.6) as u8;
                            let b = (r as f32 * 0.4) as u8;
                            Rgba([r, g, b, 255])
                        }
                        3 => { // Stone: shades of grey
                            let l = next_rand(100, 140);
                            Rgba([l, l, l, 255])
                        }
                        _ => Rgba([0, 0, 0, 255]),
                    }
                } else {
                    Rgba([0, 0, 0, 255]) // other rows empty
                };
                
                img.put_pixel(x, y, pixel);
            }
        }

        // Save to assets folder
        let _ = std::fs::create_dir_all("assets");
        let _ = img.save("assets/texture_atlas.png");

        let dimensions = img.dimensions();
        let size = wgpu::Extent3d {
            width: dimensions.0,
            height: dimensions.1,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Texture Atlas"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &img,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * dimensions.0),
                rows_per_image: Some(dimensions.1),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Self {
            texture,
            view,
            sampler,
        }
    }
}
