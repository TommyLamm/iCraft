use wgpu::{Device, Queue, Texture, TextureView, Sampler};
use image::{RgbaImage, Rgba};

pub struct TextureAtlas {
    pub texture: Texture,
    pub view: TextureView,
    pub sampler: Sampler,
}

fn draw_noise(img: &mut RgbaImage, tx: u32, ty: u32, base: [u8; 3], noise: u8, seed: &mut u32) {
    let mut next_rand = |min: i16, max: i16| -> i16 {
        *seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let val = (*seed / 65536) % 32768;
        let diff = max - min;
        if diff <= 0 { return min; }
        min + (val as i16 % diff)
    };

    for y in 0..16 {
        for x in 0..16 {
            let offset = next_rand(-(noise as i16), noise as i16);
            let r = (base[0] as i16 + offset).clamp(0, 255) as u8;
            let g = (base[1] as i16 + offset).clamp(0, 255) as u8;
            let b = (base[2] as i16 + offset).clamp(0, 255) as u8;
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([r, g, b, 255]));
        }
    }
}

fn draw_brick(img: &mut RgbaImage, tx: u32, ty: u32, base: [u8; 3], mortar: [u8; 3], seed: &mut u32) {
    let mut next_rand = |min: i16, max: i16| -> i16 {
        *seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let val = (*seed / 65536) % 32768;
        let diff = max - min;
        if diff <= 0 { return min; }
        min + (val as i16 % diff)
    };

    for y in 0..16 {
        for x in 0..16 {
            // Simple brick pattern: 4 rows of brick, offset alternate rows
            let is_mortar = y % 4 == 0 || (x + (y / 4) * 4) % 8 == 0;
            let color = if is_mortar {
                mortar
            } else {
                let offset = next_rand(-10, 10);
                [
                    (base[0] as i16 + offset).clamp(0, 255) as u8,
                    (base[1] as i16 + offset).clamp(0, 255) as u8,
                    (base[2] as i16 + offset).clamp(0, 255) as u8,
                ]
            };
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([color[0], color[1], color[2], 255]));
        }
    }
}

fn draw_planks(img: &mut RgbaImage, tx: u32, ty: u32, base: [u8; 3], seed: &mut u32) {
    let mut next_rand = |min: i16, max: i16| -> i16 {
        *seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let val = (*seed / 65536) % 32768;
        let diff = max - min;
        if diff <= 0 { return min; }
        min + (val as i16 % diff)
    };

    for y in 0..16 {
        for x in 0..16 {
            let is_border = y % 4 == 0 || x == 0 || x == 15;
            let color = if is_border {
                [base[0] / 2, base[1] / 2, base[2] / 2]
            } else {
                let offset = next_rand(-15, 15);
                [
                    (base[0] as i16 + offset).clamp(0, 255) as u8,
                    (base[1] as i16 + offset).clamp(0, 255) as u8,
                    (base[2] as i16 + offset).clamp(0, 255) as u8,
                ]
            };
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([color[0], color[1], color[2], 255]));
        }
    }
}

fn draw_ore(img: &mut RgbaImage, tx: u32, ty: u32, ore_color: [u8; 3], seed: &mut u32) {
    // Start with stone noise
    draw_noise(img, tx, ty, [120, 120, 120], 15, seed);
    
    let mut next_rand = |min: i16, max: i16| -> i16 {
        *seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let val = (*seed / 65536) % 32768;
        let diff = max - min;
        if diff <= 0 { return min; }
        min + (val as i16 % diff)
    };

    // Overlay ore spots
    for _ in 0..8 {
        let ox = next_rand(1, 14) as u32;
        let oy = next_rand(1, 14) as u32;
        img.put_pixel(tx * 16 + ox, ty * 16 + oy, Rgba([ore_color[0], ore_color[1], ore_color[2], 255]));
        img.put_pixel(tx * 16 + ox + 1, ty * 16 + oy, Rgba([ore_color[0], ore_color[1], ore_color[2], 255]));
    }
}

fn draw_leaves(img: &mut RgbaImage, tx: u32, ty: u32, seed: &mut u32) {
    let mut next_rand = |min: i16, max: i16| -> i16 {
        *seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let val = (*seed / 65536) % 32768;
        let diff = max - min;
        if diff <= 0 { return min; }
        min + (val as i16 % diff)
    };

    for y in 0..16 {
        for x in 0..16 {
            let is_transparent = next_rand(0, 10) < 2;
            if is_transparent {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            } else {
                let g = next_rand(90, 160) as u8;
                let r = g / 3;
                let b = g / 4;
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([r, g, b, 255]));
            }
        }
    }
}

fn draw_glass(img: &mut RgbaImage, tx: u32, ty: u32) {
    for y in 0..16 {
        for x in 0..16 {
            let is_border = x == 0 || x == 15 || y == 0 || y == 15;
            if is_border {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([240, 240, 240, 255]));
            } else {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn draw_water(img: &mut RgbaImage, tx: u32, ty: u32, seed: &mut u32) {
    let mut next_rand = |min: i16, max: i16| -> i16 {
        *seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let val = (*seed / 65536) % 32768;
        let diff = max - min;
        if diff <= 0 { return min; }
        min + (val as i16 % diff)
    };

    for y in 0..16 {
        for x in 0..16 {
            let b = next_rand(200, 255) as u8;
            let g = (b as f32 * 0.4) as u8;
            let r = (b as f32 * 0.2) as u8;
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([r, g, b, 150]));
        }
    }
}

fn draw_ice(img: &mut RgbaImage, tx: u32, ty: u32, seed: &mut u32) {
    let mut next_rand = |min: i16, max: i16| -> i16 {
        *seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let val = (*seed / 65536) % 32768;
        let diff = max - min;
        if diff <= 0 { return min; }
        min + (val as i16 % diff)
    };

    for y in 0..16 {
        for x in 0..16 {
            let l = next_rand(210, 250) as u8;
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([l - 20, l - 10, l, 180]));
        }
    }
}

fn draw_torch(img: &mut RgbaImage, tx: u32, ty: u32) {
    for y in 0..16 {
        for x in 0..16 {
            // Draw a stick in the middle
            let is_stick = x == 7 && y >= 6 && y <= 13;
            let is_coal = x == 7 && y == 5;
            let is_fire = x >= 6 && x <= 8 && y >= 2 && y <= 4;
            if is_stick {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([150, 100, 50, 255]));
            } else if is_coal {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([30, 30, 30, 255]));
            } else if is_fire {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([255, 120, 0, 255]));
            } else {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn draw_stick_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
    for y in 0..16 {
        for x in 0..16 {
            let is_stick = x == y && x >= 3 && x <= 12;
            if is_stick {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([139, 90, 43, 255]));
            } else {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn draw_coal_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
    for y in 0..16 {
        for x in 0..16 {
            let dx = (x as i32 - 8).abs();
            let dy = (y as i32 - 8).abs();
            let is_coal = dx + dy <= 5 && dx <= 4 && dy <= 4;
            if is_coal {
                let r = if dx == 0 && dy == 0 { 60 } else { 30 };
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([r, r, r, 255]));
            } else {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn draw_ingot_icon(img: &mut RgbaImage, tx: u32, ty: u32, color: [u8; 3]) {
    for y in 0..16 {
        for x in 0..16 {
            let is_ingot = x >= 3 && x <= 12 && y >= 5 && y <= 10;
            if is_ingot {
                let is_highlight = x == 3 || y == 5;
                let c = if is_highlight {
                    [color[0].saturating_add(40), color[1].saturating_add(40), color[2].saturating_add(40)]
                } else {
                    color
                };
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([c[0], c[1], c[2], 255]));
            } else {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn draw_diamond_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
    for y in 0..16 {
        for x in 0..16 {
            let dx = (x as i32 - 8).abs();
            let dy = (y as i32 - 8).abs();
            let is_diamond = dx + dy <= 5 && dy >= 1;
            if is_diamond {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([90, 220, 240, 255]));
            } else {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn draw_redstone_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
    for y in 0..16 {
        for x in 0..16 {
            let is_center = (x as i32 - 8).abs() <= 2 && (y as i32 - 8).abs() <= 2;
            if is_center {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([200, 20, 20, 255]));
            } else {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn draw_sword_icon(img: &mut RgbaImage, tx: u32, ty: u32, blade_color: [u8; 3]) {
    for y in 0..16 {
        for x in 0..16 {
            let is_handle = x == 3 && y == 12;
            let is_guard = (x == 4 && y == 11) || (x == 3 && y == 11) || (x == 4 && y == 12);
            let is_blade = x + y == 15 && x >= 5 && x <= 12;
            if is_blade {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([blade_color[0], blade_color[1], blade_color[2], 255]));
            } else if is_guard {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([120, 90, 60, 255]));
            } else if is_handle {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([100, 70, 40, 255]));
            } else {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn draw_pickaxe_icon(img: &mut RgbaImage, tx: u32, ty: u32, head_color: [u8; 3]) {
    for y in 0..16 {
        for x in 0..16 {
            let is_handle = x == y && x >= 3 && x <= 12;
            let is_head = (y == 3 && x >= 2 && x <= 6) || (x == 3 && y >= 2 && y <= 6) || (x + y == 6);
            if is_head {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([head_color[0], head_color[1], head_color[2], 255]));
            } else if is_handle {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([139, 90, 43, 255]));
            } else {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn draw_axe_icon(img: &mut RgbaImage, tx: u32, ty: u32, head_color: [u8; 3]) {
    for y in 0..16 {
        for x in 0..16 {
            let is_handle = x == y && x >= 3 && x <= 12;
            let is_head = x >= 2 && x <= 4 && y >= 2 && y <= 4 && !(x == 4 && y == 4);
            if is_head {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([head_color[0], head_color[1], head_color[2], 255]));
            } else if is_handle {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([139, 90, 43, 255]));
            } else {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn draw_shovel_icon(img: &mut RgbaImage, tx: u32, ty: u32, head_color: [u8; 3]) {
    for y in 0..16 {
        for x in 0..16 {
            let is_handle = x == y && x >= 4 && x <= 12;
            let is_head = x >= 2 && x <= 3 && y >= 2 && y <= 3;
            if is_head {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([head_color[0], head_color[1], head_color[2], 255]));
            } else if is_handle {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([139, 90, 43, 255]));
            } else {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

impl TextureAtlas {
    pub fn new_procedural(device: &Device, queue: &Queue) -> Self {
        let mut img = RgbaImage::new(256, 256);
        let mut seed = 12345u32;
        
        // Define row & col mapping for block textures
        // Row 0
        draw_noise(&mut img, 0, 0, [100, 160, 60], 10, &mut seed); // Grass Top (greenish)
        // Grass Side (grass on top, dirt below)
        for y in 0..16 {
            for x in 0..16 {
                let grass_h = if (x % 3 == 0) || (x % 5 == 0) { 5 } else { 3 };
                if y < grass_h {
                    img.put_pixel(1 * 16 + x, 0 * 16 + y, Rgba([100, 160, 60, 255]));
                } else {
                    img.put_pixel(1 * 16 + x, 0 * 16 + y, Rgba([120, 80, 50, 255]));
                }
            }
        }
        draw_noise(&mut img, 2, 0, [120, 80, 50], 10, &mut seed);   // Dirt
        draw_noise(&mut img, 3, 0, [120, 120, 120], 15, &mut seed); // Stone
        draw_noise(&mut img, 4, 0, [210, 200, 160], 8, &mut seed);  // Sand
        draw_noise(&mut img, 5, 0, [130, 120, 120], 12, &mut seed); // Gravel
        draw_planks(&mut img, 6, 0, [180, 140, 90], &mut seed);     // Oak Planks
        draw_leaves(&mut img, 7, 0, &mut seed);                     // Oak Leaves
        draw_brick(&mut img, 8, 0, [120, 120, 120], [80, 80, 80], &mut seed); // Cobblestone
        draw_noise(&mut img, 9, 0, [60, 60, 60], 20, &mut seed);     // Bedrock
        draw_water(&mut img, 10, 0, &mut seed);                      // Water
        draw_ore(&mut img, 11, 0, [30, 30, 30], &mut seed);          // Coal Ore
        draw_ore(&mut img, 12, 0, [220, 160, 120], &mut seed);        // Iron Ore
        draw_ore(&mut img, 13, 0, [240, 220, 70], &mut seed);         // Gold Ore
        draw_ore(&mut img, 14, 0, [100, 220, 240], &mut seed);        // Diamond Ore
        draw_ore(&mut img, 15, 0, [240, 30, 30], &mut seed);          // Redstone Ore

        // Row 1
        draw_glass(&mut img, 0, 1);                                  // Glass
        draw_brick(&mut img, 1, 1, [150, 70, 50], [200, 200, 200], &mut seed); // Brick
        draw_brick(&mut img, 2, 1, [110, 110, 110], [70, 70, 70], &mut seed); // Stone Brick
        draw_noise(&mut img, 3, 1, [240, 240, 240], 5, &mut seed);   // Snow Block
        // Snow Side
        for y in 0..16 {
            for x in 0..16 {
                if y < 4 {
                    img.put_pixel(4 * 16 + x, 1 * 16 + y, Rgba([240, 240, 240, 255]));
                } else {
                    img.put_pixel(4 * 16 + x, 1 * 16 + y, Rgba([120, 80, 50, 255]));
                }
            }
        }
        draw_ice(&mut img, 5, 1, &mut seed);                         // Ice
        draw_noise(&mut img, 6, 1, [140, 130, 120], 10, &mut seed);  // Clay
        draw_noise(&mut img, 7, 1, [200, 180, 130], 5, &mut seed);   // Sandstone (top/bottom)
        // Sandstone side
        for y in 0..16 {
            for x in 0..16 {
                let is_stripe = y == 3 || y == 4 || y == 11 || y == 12;
                let c = if is_stripe { [180, 150, 100] } else { [200, 180, 130] };
                img.put_pixel(8 * 16 + x, 1 * 16 + y, Rgba([c[0], c[1], c[2], 255]));
            }
        }
        draw_noise(&mut img, 9, 1, [20, 15, 30], 8, &mut seed);      // Obsidian
        draw_noise(&mut img, 10, 1, [150, 110, 70], 12, &mut seed);   // Log Top
        // Log Side
        for y in 0..16 {
            for x in 0..16 {
                let is_bark = x % 4 == 0 || y % 6 == 0;
                let c = if is_bark { [80, 55, 35] } else { [110, 80, 50] };
                img.put_pixel(11 * 16 + x, 1 * 16 + y, Rgba([c[0], c[1], c[2], 255]));
            }
        }
        // Crafting Table Top
        for y in 0..16 {
            for x in 0..16 {
                let is_border = x == 0 || x == 15 || y == 0 || y == 15;
                let c = if is_border { [90, 60, 30] } else { [180, 140, 90] };
                img.put_pixel(12 * 16 + x, 1 * 16 + y, Rgba([c[0], c[1], c[2], 255]));
            }
        }
        // Crafting Table Side
        for y in 0..16 {
            for x in 0..16 {
                let is_tool = x > 3 && x < 12 && y > 3 && y < 12;
                let c = if is_tool { [120, 80, 40] } else { [180, 140, 90] };
                img.put_pixel(13 * 16 + x, 1 * 16 + y, Rgba([c[0], c[1], c[2], 255]));
            }
        }
        // Furnace Front
        for y in 0..16 {
            for x in 0..16 {
                let is_mouth = y >= 9 && y <= 13 && x >= 3 && x <= 12;
                let c = if is_mouth { [30, 30, 30] } else { [120, 120, 120] };
                img.put_pixel(14 * 16 + x, 1 * 16 + y, Rgba([c[0], c[1], c[2], 255]));
            }
        }
        // Chest
        for y in 0..16 {
            for x in 0..16 {
                let is_latch = x >= 7 && x <= 8 && y >= 6 && y <= 8;
                let is_border = x == 0 || x == 15 || y == 0 || y == 15 || y == 5;
                let c = if is_latch { [220, 220, 220] } else if is_border { [70, 45, 20] } else { [140, 90, 45] };
                img.put_pixel(15 * 16 + x, 1 * 16 + y, Rgba([c[0], c[1], c[2], 255]));
            }
        }

        // Row 2
        // TNT Top
        draw_noise(&mut img, 0, 2, [200, 50, 40], 10, &mut seed);
        // TNT Bottom
        draw_noise(&mut img, 1, 2, [130, 130, 130], 15, &mut seed);
        // TNT Side
        for y in 0..16 {
            for x in 0..16 {
                let is_white_band = y >= 6 && y <= 9;
                let c = if is_white_band { [255, 255, 255] } else { [200, 50, 40] };
                img.put_pixel(2 * 16 + x, 2 * 16 + y, Rgba([c[0], c[1], c[2], 255]));
            }
        }
        // Bookshelf Side
        for y in 0..16 {
            for x in 0..16 {
                let is_shelf = y == 0 || y == 5 || y == 10 || y == 15;
                let c = if is_shelf { [90, 60, 30] } else { [160, 80, 60] };
                img.put_pixel(3 * 16 + x, 2 * 16 + y, Rgba([c[0], c[1], c[2], 255]));
            }
        }
        draw_torch(&mut img, 4, 2);

        // Clear remaining slots in Row 2 (index 2)
        for tx in 5..16 {
            for y in 0..16 {
                for x in 0..16 {
                    img.put_pixel(tx * 16 + x, 2 * 16 + y, Rgba([0, 0, 0, 0]));
                }
            }
        }

        // Row 3: Resources
        draw_stick_icon(&mut img, 0, 3);
        draw_coal_icon(&mut img, 1, 3);
        draw_ingot_icon(&mut img, 2, 3, [200, 200, 200]); // Iron Ingot
        draw_ingot_icon(&mut img, 3, 3, [240, 220, 70]);  // Gold Ingot
        draw_diamond_icon(&mut img, 4, 3);
        draw_redstone_icon(&mut img, 5, 3);
        // Clear remaining slots in Row 3
        for tx in 6..16 {
            for y in 0..16 {
                for x in 0..16 {
                    img.put_pixel(tx * 16 + x, 3 * 16 + y, Rgba([0, 0, 0, 0]));
                }
            }
        }

        // Row 4: Swords
        draw_sword_icon(&mut img, 0, 4, [160, 160, 160]); // Stone Sword (gray)
        draw_sword_icon(&mut img, 1, 4, [220, 220, 220]); // Iron Sword (silver)
        draw_sword_icon(&mut img, 2, 4, [100, 220, 240]); // Diamond Sword (cyan)
        // Clear remaining slots in Row 4
        for tx in 3..16 {
            for y in 0..16 {
                for x in 0..16 {
                    img.put_pixel(tx * 16 + x, 4 * 16 + y, Rgba([0, 0, 0, 0]));
                }
            }
        }

        // Row 5: Pickaxes
        draw_pickaxe_icon(&mut img, 0, 5, [160, 160, 160]); // Stone
        draw_pickaxe_icon(&mut img, 1, 5, [220, 220, 220]); // Iron
        draw_pickaxe_icon(&mut img, 2, 5, [100, 220, 240]); // Diamond
        // Clear remaining slots in Row 5
        for tx in 3..16 {
            for y in 0..16 {
                for x in 0..16 {
                    img.put_pixel(tx * 16 + x, 5 * 16 + y, Rgba([0, 0, 0, 0]));
                }
            }
        }

        // Row 6: Axes
        draw_axe_icon(&mut img, 0, 6, [160, 160, 160]); // Stone
        draw_axe_icon(&mut img, 1, 6, [220, 220, 220]); // Iron
        draw_axe_icon(&mut img, 2, 6, [100, 220, 240]); // Diamond
        // Clear remaining slots in Row 6
        for tx in 3..16 {
            for y in 0..16 {
                for x in 0..16 {
                    img.put_pixel(tx * 16 + x, 6 * 16 + y, Rgba([0, 0, 0, 0]));
                }
            }
        }

        // Row 7: Shovels
        draw_shovel_icon(&mut img, 0, 7, [160, 160, 160]); // Stone
        draw_shovel_icon(&mut img, 1, 7, [220, 220, 220]); // Iron
        draw_shovel_icon(&mut img, 2, 7, [100, 220, 240]); // Diamond
        // Clear remaining slots in Row 7
        for tx in 3..16 {
            for y in 0..16 {
                for x in 0..16 {
                    img.put_pixel(tx * 16 + x, 7 * 16 + y, Rgba([0, 0, 0, 0]));
                }
            }
        }

        // Fill remaining rows (8 to 15) with transparent
        for ty in 8..16 {
            for tx in 0..16 {
                for y in 0..16 {
                    for x in 0..16 {
                        img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
                    }
                }
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
