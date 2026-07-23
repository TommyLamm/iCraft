use image::{Rgba, RgbaImage};
use wgpu::{Device, Queue, Sampler, Texture, TextureView};

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
        if diff <= 0 {
            return min;
        }
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

fn draw_fire(img: &mut RgbaImage, tx: u32, ty: u32) {
    for y in 0..16 {
        for x in 0..16 {
            let centered = (x as i32 - 8).abs();
            let flame_width = 6 - y as i32 / 3 + ((x + y * 3) % 3) as i32;
            let inside = centered <= flame_width.max(1) && y >= ((x * 7 + 3) % 5);
            let color = if !inside {
                [0, 0, 0, 0]
            } else if y > 10 || centered <= 2 {
                [255, 235, 70, 235]
            } else if y > 5 {
                [255, 145, 25, 225]
            } else {
                [220, 45, 10, 210]
            };
            img.put_pixel(tx * 16 + x, ty * 16 + (15 - y), Rgba(color));
        }
    }
}

fn draw_brick(
    img: &mut RgbaImage,
    tx: u32,
    ty: u32,
    base: [u8; 3],
    mortar: [u8; 3],
    seed: &mut u32,
) {
    let mut next_rand = |min: i16, max: i16| -> i16 {
        *seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let val = (*seed / 65536) % 32768;
        let diff = max - min;
        if diff <= 0 {
            return min;
        }
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
            img.put_pixel(
                tx * 16 + x,
                ty * 16 + y,
                Rgba([color[0], color[1], color[2], 255]),
            );
        }
    }
}

fn draw_planks(img: &mut RgbaImage, tx: u32, ty: u32, base: [u8; 3], seed: &mut u32) {
    let mut next_rand = |min: i16, max: i16| -> i16 {
        *seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let val = (*seed / 65536) % 32768;
        let diff = max - min;
        if diff <= 0 {
            return min;
        }
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
            img.put_pixel(
                tx * 16 + x,
                ty * 16 + y,
                Rgba([color[0], color[1], color[2], 255]),
            );
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
        if diff <= 0 {
            return min;
        }
        min + (val as i16 % diff)
    };

    // Overlay ore spots
    for _ in 0..8 {
        let ox = next_rand(1, 14) as u32;
        let oy = next_rand(1, 14) as u32;
        img.put_pixel(
            tx * 16 + ox,
            ty * 16 + oy,
            Rgba([ore_color[0], ore_color[1], ore_color[2], 255]),
        );
        img.put_pixel(
            tx * 16 + ox + 1,
            ty * 16 + oy,
            Rgba([ore_color[0], ore_color[1], ore_color[2], 255]),
        );
    }
}

fn draw_leaves(img: &mut RgbaImage, tx: u32, ty: u32, seed: &mut u32) {
    let mut next_rand = |min: i16, max: i16| -> i16 {
        *seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let val = (*seed / 65536) % 32768;
        let diff = max - min;
        if diff <= 0 {
            return min;
        }
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
        if diff <= 0 {
            return min;
        }
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
        if diff <= 0 {
            return min;
        }
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
                    [
                        color[0].saturating_add(40),
                        color[1].saturating_add(40),
                        color[2].saturating_add(40),
                    ]
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
                img.put_pixel(
                    tx * 16 + x,
                    ty * 16 + y,
                    Rgba([blade_color[0], blade_color[1], blade_color[2], 255]),
                );
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
            let is_head =
                (y == 3 && x >= 2 && x <= 6) || (x == 3 && y >= 2 && y <= 6) || (x + y == 6);
            if is_head {
                img.put_pixel(
                    tx * 16 + x,
                    ty * 16 + y,
                    Rgba([head_color[0], head_color[1], head_color[2], 255]),
                );
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
                img.put_pixel(
                    tx * 16 + x,
                    ty * 16 + y,
                    Rgba([head_color[0], head_color[1], head_color[2], 255]),
                );
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
                img.put_pixel(
                    tx * 16 + x,
                    ty * 16 + y,
                    Rgba([head_color[0], head_color[1], head_color[2], 255]),
                );
            } else if is_handle {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([139, 90, 43, 255]));
            } else {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn draw_crack_pattern(img: &mut RgbaImage, tx: u32, ty: u32, stage: u32) {
    // Determine crack pattern density based on stage (0..10)
    // We draw random dark gray lines.
    let mut seed = 54321 + stage;
    let mut next_rand = |min: i32, max: i32| -> i32 {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let val = (seed / 65536) % 32768;
        let diff = max - min;
        if diff <= 0 {
            return min;
        }
        min + (val as i32 % diff)
    };

    // Background is transparent Rgba([0, 0, 0, 0])
    for y in 0..16 {
        for x in 0..16 {
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
        }
    }

    // Number of crack lines scales with stage
    let num_lines = (stage + 1) * 2;
    for _ in 0..num_lines {
        let mut cx = next_rand(0, 16) as i32;
        let mut cy = next_rand(0, 16) as i32;
        let length = next_rand(3, 8);
        for _ in 0..length {
            if cx >= 0 && cx < 16 && cy >= 0 && cy < 16 {
                img.put_pixel(
                    tx * 16 + cx as u32,
                    ty * 16 + cy as u32,
                    Rgba([20, 20, 20, 200]),
                ); // Dark grey crack line
            }
            cx += next_rand(-1, 2);
            cy += next_rand(-1, 2);
        }
    }
}

fn draw_heart(img: &mut RgbaImage, tx: u32, ty: u32, fill: f32) {
    let ox = tx * 16 + 3;
    let oy = ty * 16 + 4;

    // Clear tile first
    for y in 0..16 {
        for x in 0..16 {
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
        }
    }

    let border = Rgba([0, 0, 0, 255]);
    let red = Rgba([220, 20, 20, 255]);
    let dark_red = Rgba([140, 10, 10, 255]);
    let empty_fill = Rgba([60, 60, 60, 255]);
    let empty_dark = Rgba([40, 40, 40, 255]);
    let white = Rgba([255, 255, 255, 255]);

    let is_border = |x: i32, y: i32| -> bool {
        match y {
            0 => x == 1 || x == 2 || x == 6 || x == 7,
            1 => x == 0 || x == 3 || x == 5 || x == 8,
            2 => x == 0 || x == 8,
            3 => x == 0 || x == 8,
            4 => x == 1 || x == 7,
            5 => x == 2 || x == 6,
            6 => x == 3 || x == 5,
            7 => x == 4,
            _ => false,
        }
    };

    let is_inside = |x: i32, y: i32| -> bool {
        if is_border(x, y) {
            return false;
        }
        match y {
            0 => false,
            1 => x == 1 || x == 2 || x == 6 || x == 7,
            2..=3 => x > 0 && x < 8,
            4 => x > 1 && x < 7,
            5 => x > 2 && x < 6,
            6 => x == 4,
            _ => false,
        }
    };

    for y in 0..8 {
        for x in 0..9 {
            let px = ox + x as u32;
            let py = oy + y as u32;
            if is_border(x, y) {
                img.put_pixel(px, py, border);
            } else if is_inside(x, y) {
                let is_left = x < 4;
                let is_filled = if fill >= 1.0 {
                    true
                } else if fill >= 0.5 {
                    is_left
                } else {
                    false
                };

                let color = if is_filled {
                    if y == 1 && x == 1 {
                        white
                    } else if y >= 4 || x >= 6 {
                        dark_red
                    } else {
                        red
                    }
                } else {
                    if y >= 4 || x >= 6 {
                        empty_dark
                    } else {
                        empty_fill
                    }
                };
                img.put_pixel(px, py, color);
            }
        }
    }
}

fn draw_hunger(img: &mut RgbaImage, tx: u32, ty: u32, fill: f32) {
    let ox = tx * 16 + 3;
    let oy = ty * 16 + 3;

    for y in 0..16 {
        for x in 0..16 {
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
        }
    }

    let border = Rgba([0, 0, 0, 255]);
    let bone = Rgba([220, 220, 220, 255]);
    let bone_shadow = Rgba([150, 150, 150, 255]);
    let meat = Rgba([160, 100, 60, 255]);
    let meat_dark = Rgba([110, 60, 30, 255]);
    let empty_meat = Rgba([70, 70, 70, 255]);
    let empty_meat_dark = Rgba([50, 50, 50, 255]);

    let grid = [
        [0, 0, 0, 0, 0, 1, 1, 1, 0, 0],
        [0, 0, 0, 0, 1, 2, 2, 2, 1, 0],
        [0, 0, 0, 1, 2, 2, 2, 2, 2, 1],
        [0, 0, 1, 2, 2, 2, 2, 2, 2, 1],
        [0, 0, 1, 2, 2, 2, 2, 2, 1, 0],
        [0, 0, 0, 1, 2, 2, 2, 1, 0, 0],
        [0, 0, 0, 0, 1, 3, 3, 1, 0, 0],
        [0, 0, 0, 1, 3, 1, 1, 0, 0, 0],
        [0, 0, 1, 3, 1, 0, 0, 0, 0, 0],
        [0, 1, 1, 1, 0, 0, 0, 0, 0, 0],
    ];

    for y in 0..10 {
        for x in 0..10 {
            let val = grid[y][x];
            if val == 0 {
                continue;
            }
            let px = ox + x as u32;
            let py = oy + y as u32;
            if val == 1 {
                img.put_pixel(px, py, border);
            } else if val == 3 {
                let c = if x == 2 || y == 8 { bone_shadow } else { bone };
                img.put_pixel(px, py, c);
            } else if val == 2 {
                let is_filled = if fill >= 1.0 {
                    true
                } else if fill >= 0.5 {
                    (y as i32 - x as i32) >= -2
                } else {
                    false
                };

                let c = if is_filled {
                    if x == 7 || (y == 2 && x == 8) || (y == 3 && x == 8) {
                        meat_dark
                    } else {
                        meat
                    }
                } else {
                    if x == 7 || (y == 2 && x == 8) || (y == 3 && x == 8) {
                        empty_meat_dark
                    } else {
                        empty_meat
                    }
                };
                img.put_pixel(px, py, c);
            }
        }
    }
}

fn draw_bubble_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
    let ox = tx * 16;
    let oy = ty * 16;

    for y in 0..16 {
        for x in 0..16 {
            img.put_pixel(ox + x, oy + y, Rgba([0, 0, 0, 0]));
        }
    }

    let border = Rgba([0, 50, 150, 255]);
    let body = Rgba([100, 200, 255, 255]);
    let highlight = Rgba([255, 255, 255, 255]);

    let grid = [
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 1, 3, 2, 2, 2, 1, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 1, 3, 3, 2, 2, 2, 2, 1, 0, 0, 0, 0],
        [0, 0, 0, 0, 1, 2, 2, 2, 2, 2, 2, 1, 0, 0, 0, 0],
        [0, 0, 0, 0, 1, 2, 2, 2, 2, 2, 2, 1, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 1, 2, 2, 2, 2, 1, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    ];

    for y in 0..16 {
        for x in 0..16 {
            let val = grid[y][x];
            let px = ox + x as u32;
            let py = oy + y as u32;
            if val == 1 {
                img.put_pixel(px, py, border);
            } else if val == 2 {
                img.put_pixel(px, py, body);
            } else if val == 3 {
                img.put_pixel(px, py, highlight);
            }
        }
    }
}

fn draw_apple_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
    let ox = tx * 16;
    let oy = ty * 16;

    for y in 0..16 {
        for x in 0..16 {
            img.put_pixel(ox + x, oy + y, Rgba([0, 0, 0, 0]));
        }
    }

    let border = Rgba([0, 0, 0, 255]);
    let red = Rgba([220, 20, 20, 255]);
    let dark_red = Rgba([140, 10, 10, 255]);
    let highlight = Rgba([255, 100, 100, 255]);
    let stem = Rgba([100, 70, 40, 255]);
    let leaf = Rgba([60, 140, 40, 255]);

    let grid = [
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 4, 3, 3, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 1, 3, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0],
        [0, 0, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2, 1, 0, 0, 0],
        [0, 0, 1, 2, 5, 2, 2, 2, 2, 2, 2, 2, 2, 1, 0, 0],
        [0, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 0, 0],
        [0, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 0, 0],
        [0, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 0, 0],
        [0, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2, 6, 6, 1, 0, 0],
        [0, 0, 0, 1, 2, 2, 2, 2, 2, 2, 6, 6, 1, 0, 0, 0],
        [0, 0, 0, 0, 1, 1, 2, 2, 2, 2, 1, 1, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    ];

    for y in 0..16 {
        for x in 0..16 {
            let val = grid[y][x];
            if val == 0 {
                continue;
            }
            let c = match val {
                1 => border,
                2 => red,
                3 => stem,
                4 => leaf,
                5 => highlight,
                6 => dark_red,
                _ => border,
            };
            img.put_pixel(ox + x as u32, oy + y as u32, c);
        }
    }
}

fn draw_bread_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
    let ox = tx * 16;
    let oy = ty * 16;

    for y in 0..16 {
        for x in 0..16 {
            img.put_pixel(ox + x, oy + y, Rgba([0, 0, 0, 0]));
        }
    }

    let border = Rgba([0, 0, 0, 255]);
    let brown = Rgba([185, 120, 60, 255]);
    let light_brown = Rgba([225, 160, 90, 255]);
    let dark_brown = Rgba([120, 70, 30, 255]);

    let grid = [
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2, 2, 2, 1, 0, 0],
        [0, 0, 0, 0, 0, 0, 1, 1, 2, 3, 2, 2, 2, 2, 1, 0],
        [0, 0, 0, 0, 1, 1, 2, 2, 2, 2, 2, 3, 2, 2, 1, 0],
        [0, 0, 0, 1, 2, 4, 2, 2, 3, 2, 2, 2, 2, 1, 0, 0],
        [0, 0, 1, 2, 4, 4, 2, 2, 2, 2, 3, 2, 1, 0, 0, 0],
        [0, 0, 1, 2, 2, 4, 4, 2, 2, 2, 2, 1, 0, 0, 0, 0],
        [0, 1, 2, 2, 2, 2, 4, 4, 2, 2, 1, 0, 0, 0, 0, 0],
        [0, 1, 2, 2, 2, 2, 2, 4, 2, 1, 0, 0, 0, 0, 0, 0],
        [0, 0, 1, 2, 2, 2, 2, 2, 1, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    ];

    for y in 0..16 {
        for x in 0..16 {
            let val = grid[y][x];
            if val == 0 {
                continue;
            }
            let c = match val {
                1 => border,
                2 => brown,
                3 => dark_brown,
                4 => light_brown,
                _ => border,
            };
            img.put_pixel(ox + x as u32, oy + y as u32, c);
        }
    }
}

fn draw_rotten_flesh_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
    let ox = tx * 16;
    let oy = ty * 16;
    for y in 0..16 {
        for x in 0..16 {
            img.put_pixel(ox + x, oy + y, Rgba([0, 0, 0, 0]));
        }
    }
    let border = Rgba([0, 0, 0, 255]);
    let flesh_brown = Rgba([120, 80, 50, 255]);
    let flesh_green = Rgba([90, 110, 50, 255]);
    let dark_green = Rgba([50, 70, 30, 255]);
    let grid = [
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 1, 2, 3, 2, 1, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 1, 2, 4, 3, 4, 2, 1, 0, 0, 0, 0, 0],
        [0, 0, 0, 1, 2, 3, 4, 3, 2, 3, 2, 1, 0, 0, 0, 0],
        [0, 0, 1, 2, 4, 3, 2, 4, 3, 4, 2, 1, 0, 0, 0, 0],
        [0, 0, 1, 3, 2, 4, 3, 2, 3, 2, 1, 0, 0, 0, 0, 0],
        [0, 0, 0, 1, 1, 3, 4, 3, 2, 1, 1, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    ];
    for y in 0..grid.len() {
        for x in 0..grid[y].len() {
            let val = grid[y][x];
            if val == 0 {
                continue;
            }
            let c = match val {
                1 => border,
                2 => flesh_brown,
                3 => flesh_green,
                4 => dark_green,
                _ => border,
            };
            img.put_pixel(ox + x as u32, oy + y as u32, c);
        }
    }
}

fn draw_bone_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
    let ox = tx * 16;
    let oy = ty * 16;
    for y in 0..16 {
        for x in 0..16 {
            img.put_pixel(ox + x, oy + y, Rgba([0, 0, 0, 0]));
        }
    }
    let border = Rgba([0, 0, 0, 255]);
    let bone_white = Rgba([230, 230, 220, 255]);
    let bone_gray = Rgba([180, 180, 175, 255]);
    let grid = [
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 2, 1, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 2, 1, 1, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 2, 1, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 2, 1, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 1, 2, 2, 1, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 1, 2, 2, 1, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 1, 2, 2, 1, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 1, 2, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 1, 2, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 1, 1, 2, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 1, 2, 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 1, 3, 3, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    ];
    for y in 0..grid.len() {
        for x in 0..grid[y].len() {
            let val = grid[y][x];
            if val == 0 {
                continue;
            }
            let c = match val {
                1 => border,
                2 => bone_white,
                3 => bone_gray,
                _ => border,
            };
            img.put_pixel(ox + x as u32, oy + y as u32, c);
        }
    }
}

fn draw_bow_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
    let ox = tx * 16;
    let oy = ty * 16;
    for y in 0..16 {
        for x in 0..16 {
            img.put_pixel(ox + x, oy + y, Rgba([0, 0, 0, 0]));
        }
    }
    let border = Rgba([0, 0, 0, 255]);
    let wood = Rgba([140, 90, 45, 255]);
    let string = Rgba([200, 200, 200, 255]);
    let grid = [
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2, 2, 1, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 2, 1, 3, 1, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 1, 2, 1, 0, 0, 3, 1, 0, 0],
        [0, 0, 0, 0, 0, 0, 1, 2, 1, 0, 0, 0, 3, 1, 0, 0],
        [0, 0, 0, 0, 0, 1, 2, 1, 0, 0, 0, 0, 3, 1, 0, 0],
        [0, 0, 0, 0, 1, 2, 1, 0, 0, 0, 0, 0, 3, 1, 0, 0],
        [0, 0, 0, 1, 2, 1, 0, 0, 0, 0, 0, 0, 3, 1, 0, 0],
        [0, 0, 1, 2, 1, 0, 0, 0, 0, 0, 0, 0, 3, 1, 0, 0],
        [0, 1, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 3, 1, 0, 0],
        [0, 1, 2, 1, 0, 0, 0, 0, 0, 0, 0, 1, 3, 1, 0, 0],
        [0, 1, 1, 2, 1, 0, 0, 0, 0, 0, 1, 2, 1, 0, 0, 0],
        [0, 0, 1, 2, 2, 1, 1, 1, 1, 1, 2, 1, 0, 0, 0, 0],
        [0, 0, 0, 1, 1, 2, 2, 2, 2, 2, 1, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    ];
    for y in 0..grid.len() {
        for x in 0..grid[y].len() {
            let val = grid[y][x];
            if val == 0 {
                continue;
            }
            let c = match val {
                1 => border,
                2 => wood,
                3 => string,
                _ => border,
            };
            img.put_pixel(ox + x as u32, oy + y as u32, c);
        }
    }
}

fn draw_gunpowder_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
    let ox = tx * 16;
    let oy = ty * 16;
    for y in 0..16 {
        for x in 0..16 {
            img.put_pixel(ox + x, oy + y, Rgba([0, 0, 0, 0]));
        }
    }
    let border = Rgba([0, 0, 0, 255]);
    let gray1 = Rgba([110, 110, 110, 255]);
    let gray2 = Rgba([80, 80, 80, 255]);
    let grid = [
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 1, 2, 3, 1, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 1, 2, 2, 3, 2, 1, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 1, 3, 2, 3, 2, 2, 3, 1, 0, 0, 0, 0],
        [0, 0, 0, 1, 2, 2, 3, 2, 2, 3, 2, 2, 1, 0, 0, 0],
        [0, 0, 1, 3, 2, 2, 2, 3, 2, 2, 3, 2, 3, 1, 0, 0],
        [0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    ];
    for y in 0..grid.len() {
        for x in 0..grid[y].len() {
            let val = grid[y][x];
            if val == 0 {
                continue;
            }
            let c = match val {
                1 => border,
                2 => gray1,
                3 => gray2,
                _ => border,
            };
            img.put_pixel(ox + x as u32, oy + y as u32, c);
        }
    }
}

fn draw_wheat_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
    for y in 0..16 {
        for x in 0..16 {
            let is_wheat = (x + y) % 3 == 0 && x >= 3 && x <= 12 && y >= 3 && y <= 12;
            if is_wheat {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([220, 180, 70, 255]));
            } else {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn draw_seeds_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
    for y in 0..16 {
        for x in 0..16 {
            let is_seed =
                (x as i32 - y as i32).abs() <= 1 && x >= 5 && x <= 10 && y >= 5 && y <= 10;
            if is_seed {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([140, 110, 60, 255]));
            } else {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn draw_carrot_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
    for y in 0..16 {
        for x in 0..16 {
            let is_leaf = y <= 4 && x >= 6 && x <= 9;
            let is_carrot = y >= 5 && (x as i32 - 8).abs() <= (10 - y as i32) / 2 + 1;
            if is_leaf {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([40, 180, 40, 255]));
            } else if is_carrot {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([255, 130, 0, 255]));
            } else {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn draw_shears_icon(img: &mut RgbaImage, tx: u32, ty: u32) {
    for y in 0..16 {
        for x in 0..16 {
            let is_blade = (x == y || x == y + 1) && x >= 4 && x <= 11;
            let is_handle = (x == 3 && y == 12) || (x == 12 && y == 3);
            if is_blade {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([180, 180, 180, 255]));
            } else if is_handle {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([80, 80, 80, 255]));
            } else {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn draw_bucket_icon(img: &mut RgbaImage, tx: u32, ty: u32, content_color: Option<[u8; 4]>) {
    for y in 0..16 {
        for x in 0..16 {
            let is_rim = y == 4 && x >= 3 && x <= 12;
            let is_body = y >= 5 && y <= 12 && (x as i32 - 8).abs() <= (y as i32 - 4) / 2 + 3;
            let is_metal =
                is_rim || is_body && ((x as i32 - 8).abs() == (y as i32 - 4) / 2 + 3 || y == 12);
            if is_metal {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([160, 160, 160, 255]));
            } else if is_body && content_color.is_some() {
                let c = content_color.unwrap();
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([c[0], c[1], c[2], c[3]]));
            } else {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn draw_meat_icon(img: &mut RgbaImage, tx: u32, ty: u32, is_cooked: bool, base_color: [u8; 3]) {
    for y in 0..16 {
        for x in 0..16 {
            let dx = (x as i32 - 8).abs();
            let dy = (y as i32 - 8).abs();
            let is_meat = dx + dy <= 5 && dx <= 4 && dy <= 4;
            if is_meat {
                let c = if is_cooked {
                    [
                        base_color[0].saturating_sub(40),
                        base_color[1].saturating_sub(20),
                        base_color[2].saturating_add(20),
                    ]
                } else {
                    base_color
                };
                let is_fat = (x + y) % 5 == 0;
                let col = if is_fat {
                    [240, 240, 240, 255]
                } else {
                    [c[0], c[1], c[2], 255]
                };
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba(col));
            } else {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn draw_birch_bark(img: &mut RgbaImage, tx: u32, ty: u32, seed: &mut u32) {
    let mut next_rand = |min: i16, max: i16| -> i16 {
        *seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let val = (*seed / 65536) % 32768;
        let diff = max - min;
        if diff <= 0 {
            return min;
        }
        min + (val as i16 % diff)
    };
    for y in 0..16 {
        for x in 0..16 {
            let is_stripe = y % 5 == 0 && next_rand(0, 10) < 6;
            let c = if is_stripe {
                [30, 30, 30]
            } else {
                [230, 230, 225]
            };
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([c[0], c[1], c[2], 255]));
        }
    }
}

fn draw_spruce_bark(img: &mut RgbaImage, tx: u32, ty: u32, seed: &mut u32) {
    draw_noise(img, tx, ty, [70, 50, 35], 10, seed);
    for y in 0..16 {
        for x in 0..16 {
            if (x % 3 == 0 || y % 4 == 0) && (x + y) % 2 == 0 {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([50, 35, 25, 255]));
            }
        }
    }
}

fn draw_flower(img: &mut RgbaImage, tx: u32, ty: u32, petal_color: [u8; 3]) {
    // Clear tile first
    for y in 0..16 {
        for x in 0..16 {
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
        }
    }
    for y in 2..14 {
        for x in 4..12 {
            let dx = (x as i32 - 8).abs();
            let dy = (y as i32 - 6).abs();
            let is_stem = x == 8 && y >= 7;
            let is_petal = dx + dy <= 3 && y < 8;
            let is_center = dx == 0 && dy == 0;
            let c = if is_center {
                [230, 180, 20, 255]
            } else if is_petal {
                [petal_color[0], petal_color[1], petal_color[2], 255]
            } else if is_stem {
                [80, 150, 40, 255]
            } else {
                continue;
            };
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba(c));
        }
    }
}

fn draw_tall_grass(img: &mut RgbaImage, tx: u32, ty: u32) {
    for y in 0..16 {
        for x in 0..16 {
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
        }
    }
    for y in 2..16 {
        for x in 2..14 {
            let is_blade = (x + y) % 3 == 0 && x >= 4 && x <= 11;
            if is_blade {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([60, 140, 40, 255]));
            }
        }
    }
}

fn draw_cactus(img: &mut RgbaImage, tx: u32, ty: u32) {
    for y in 0..16 {
        for x in 0..16 {
            let is_border = x == 0 || x == 15 || y == 0 || y == 15;
            let is_spine = (x * y) % 7 == 3;
            let c = if is_spine {
                [230, 230, 230, 255]
            } else if is_border {
                [40, 100, 30, 255]
            } else {
                [60, 140, 40, 255]
            };
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba(c));
        }
    }
}

fn draw_sugar_cane(img: &mut RgbaImage, tx: u32, ty: u32) {
    for y in 0..16 {
        for x in 0..16 {
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
        }
    }
    for y in 0..16 {
        for x in 3..13 {
            let is_stalk = x == 4 || x == 8 || x == 11;
            if is_stalk {
                img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([90, 180, 60, 255]));
            }
        }
    }
}

fn draw_pumpkin(img: &mut RgbaImage, tx: u32, ty: u32) {
    for y in 0..16 {
        for x in 0..16 {
            let dx = (x as i32 - 8).abs();
            let dy = (y as i32 - 8).abs();
            let is_face =
                y >= 6 && y <= 11 && (dx == 3 || (dy == 2 && dx <= 2) || (y == 7 && dx == 0));
            let c = if is_face {
                [30, 20, 10]
            } else {
                [220, 120, 30]
            };
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([c[0], c[1], c[2], 255]));
        }
    }
}

fn draw_melon(img: &mut RgbaImage, tx: u32, ty: u32) {
    for y in 0..16 {
        for x in 0..16 {
            let is_stripe = (x + y / 2) % 4 == 0;
            let c = if is_stripe {
                [40, 80, 20]
            } else {
                [90, 150, 40]
            };
            img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([c[0], c[1], c[2], 255]));
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
        draw_noise(&mut img, 2, 0, [120, 80, 50], 10, &mut seed); // Dirt
        draw_noise(&mut img, 3, 0, [120, 120, 120], 15, &mut seed); // Stone
        draw_noise(&mut img, 4, 0, [210, 200, 160], 8, &mut seed); // Sand
        draw_noise(&mut img, 5, 0, [130, 120, 120], 12, &mut seed); // Gravel
        draw_planks(&mut img, 6, 0, [180, 140, 90], &mut seed); // Oak Planks
        draw_leaves(&mut img, 7, 0, &mut seed); // Oak Leaves
        draw_brick(&mut img, 8, 0, [120, 120, 120], [80, 80, 80], &mut seed); // Cobblestone
        draw_noise(&mut img, 9, 0, [60, 60, 60], 20, &mut seed); // Bedrock
        draw_water(&mut img, 10, 0, &mut seed); // Water
        draw_ore(&mut img, 11, 0, [30, 30, 30], &mut seed); // Coal Ore
        draw_ore(&mut img, 12, 0, [220, 160, 120], &mut seed); // Iron Ore
        draw_ore(&mut img, 13, 0, [240, 220, 70], &mut seed); // Gold Ore
        draw_ore(&mut img, 14, 0, [100, 220, 240], &mut seed); // Diamond Ore
        draw_ore(&mut img, 15, 0, [240, 30, 30], &mut seed); // Redstone Ore

        // Row 1
        draw_glass(&mut img, 0, 1); // Glass
        draw_brick(&mut img, 1, 1, [150, 70, 50], [200, 200, 200], &mut seed); // Brick
        draw_brick(&mut img, 2, 1, [110, 110, 110], [70, 70, 70], &mut seed); // Stone Brick
        draw_noise(&mut img, 3, 1, [240, 240, 240], 5, &mut seed); // Snow Block
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
        draw_ice(&mut img, 5, 1, &mut seed); // Ice
        draw_noise(&mut img, 6, 1, [140, 130, 120], 10, &mut seed); // Clay
        draw_noise(&mut img, 7, 1, [200, 180, 130], 5, &mut seed); // Sandstone (top/bottom)
                                                                   // Sandstone side
        for y in 0..16 {
            for x in 0..16 {
                let is_stripe = y == 3 || y == 4 || y == 11 || y == 12;
                let c = if is_stripe {
                    [180, 150, 100]
                } else {
                    [200, 180, 130]
                };
                img.put_pixel(8 * 16 + x, 1 * 16 + y, Rgba([c[0], c[1], c[2], 255]));
            }
        }
        draw_noise(&mut img, 9, 1, [20, 15, 30], 8, &mut seed); // Obsidian
        draw_noise(&mut img, 10, 1, [150, 110, 70], 12, &mut seed); // Log Top
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
                let c = if is_border {
                    [90, 60, 30]
                } else {
                    [180, 140, 90]
                };
                img.put_pixel(12 * 16 + x, 1 * 16 + y, Rgba([c[0], c[1], c[2], 255]));
            }
        }
        // Crafting Table Side
        for y in 0..16 {
            for x in 0..16 {
                let is_tool = x > 3 && x < 12 && y > 3 && y < 12;
                let c = if is_tool {
                    [120, 80, 40]
                } else {
                    [180, 140, 90]
                };
                img.put_pixel(13 * 16 + x, 1 * 16 + y, Rgba([c[0], c[1], c[2], 255]));
            }
        }
        // Furnace Front
        for y in 0..16 {
            for x in 0..16 {
                let is_mouth = y >= 9 && y <= 13 && x >= 3 && x <= 12;
                let c = if is_mouth {
                    [30, 30, 30]
                } else {
                    [120, 120, 120]
                };
                img.put_pixel(14 * 16 + x, 1 * 16 + y, Rgba([c[0], c[1], c[2], 255]));
            }
        }
        // Chest
        for y in 0..16 {
            for x in 0..16 {
                let is_latch = x >= 7 && x <= 8 && y >= 6 && y <= 8;
                let is_border = x == 0 || x == 15 || y == 0 || y == 15 || y == 5;
                let c = if is_latch {
                    [220, 220, 220]
                } else if is_border {
                    [70, 45, 20]
                } else {
                    [140, 90, 45]
                };
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
                let c = if is_white_band {
                    [255, 255, 255]
                } else {
                    [200, 50, 40]
                };
                img.put_pixel(2 * 16 + x, 2 * 16 + y, Rgba([c[0], c[1], c[2], 255]));
            }
        }
        // Bookshelf Side
        for y in 0..16 {
            for x in 0..16 {
                let is_shelf = y == 0 || y == 5 || y == 10 || y == 15;
                let c = if is_shelf {
                    [90, 60, 30]
                } else {
                    [160, 80, 60]
                };
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
        draw_ingot_icon(&mut img, 3, 3, [240, 220, 70]); // Gold Ingot
        draw_diamond_icon(&mut img, 4, 3);
        draw_redstone_icon(&mut img, 5, 3);
        draw_apple_icon(&mut img, 6, 3);
        draw_bread_icon(&mut img, 7, 3);
        draw_rotten_flesh_icon(&mut img, 8, 3);
        draw_bone_icon(&mut img, 9, 3);
        draw_bow_icon(&mut img, 10, 3);
        draw_gunpowder_icon(&mut img, 11, 3);
        draw_wheat_icon(&mut img, 12, 3);
        draw_seeds_icon(&mut img, 13, 3);
        draw_carrot_icon(&mut img, 14, 3);
        draw_bubble_icon(&mut img, 15, 3);

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

        // Draw Heart icons on Row 8
        draw_heart(&mut img, 0, 8, 1.0); // Full Heart
        draw_heart(&mut img, 1, 8, 0.5); // Half Heart
        draw_heart(&mut img, 2, 8, 0.0); // Empty Heart

        // Draw Hunger icons on Row 8
        draw_hunger(&mut img, 3, 8, 1.0); // Full Hunger
        draw_hunger(&mut img, 4, 8, 0.5); // Half Hunger
        draw_hunger(&mut img, 5, 8, 0.0); // Empty Hunger

        // Row 15: Crack overlays (cols 0..10)
        // Prefer loading high-quality 10-stage destroy stage PNGs from disk; fall
        // back to the procedural crack generator when files are missing or fail
        // to load (e.g. self-contained single-binary mode).
        for stage in 0..10u32 {
            let path = format!("assets/textures/destroy_stages/destroy_stage_{}.png", stage);
            let loaded = std::path::Path::new(&path)
                .exists()
                .then(|| image::open(&path).ok())
                .flatten();
            if let Some(loaded_img) = loaded {
                let stage_img = loaded_img.to_rgba8();
                let sw = stage_img.width();
                let sh = stage_img.height();
                let tx = stage;
                let ty = 15u32;
                for y in 0..16 {
                    for x in 0..16 {
                        let sx = (x as u32 * sw) / 16;
                        let sy = (y as u32 * sh) / 16;
                        let sx = sx.min(sw - 1);
                        let sy = sy.min(sh - 1);
                        let pixel = stage_img.get_pixel(sx, sy);
                        img.put_pixel(tx * 16 + x, ty * 16 + y, *pixel);
                    }
                }
            } else {
                draw_crack_pattern(&mut img, stage, 15, stage);
            }
        }

        // Row 9: Mob Skins
        // Col 0: Zombie Face (front)
        {
            let ox = 0 * 16;
            let oy = 9 * 16;
            for y in 0..16 {
                for x in 0..16 {
                    // Dark green zombie skin with face features
                    let is_eye_left = (x >= 3 && x <= 5) && (y >= 4 && y <= 6);
                    let is_eye_right = (x >= 10 && x <= 12) && (y >= 4 && y <= 6);
                    let is_mouth = (x >= 4 && x <= 11) && (y >= 9 && y <= 11);
                    let is_nose = (x >= 7 && x <= 8) && (y >= 6 && y <= 8);
                    let c = if is_eye_left || is_eye_right {
                        Rgba([20, 20, 20, 255]) // Dark eyes
                    } else if is_mouth {
                        Rgba([40, 60, 30, 255]) // Dark mouth
                    } else if is_nose {
                        Rgba([50, 80, 40, 255]) // Nose shadow
                    } else {
                        // Green zombie skin with variation
                        let var = ((x as i16 * 7 + y as i16 * 13) % 20 - 10) as i16;
                        let r = (70i16 + var).clamp(0, 255) as u8;
                        let g = (120i16 + var).clamp(0, 255) as u8;
                        let b = (60i16 + var / 2).clamp(0, 255) as u8;
                        Rgba([r, g, b, 255])
                    };
                    img.put_pixel(ox + x, oy + y, c);
                }
            }
        }

        // Col 1: Zombie Head sides/top/bottom
        {
            let ox = 1 * 16;
            let oy = 9 * 16;
            for y in 0..16 {
                for x in 0..16 {
                    let var = ((x as i16 * 11 + y as i16 * 7) % 16 - 8) as i16;
                    let r = (65i16 + var).clamp(0, 255) as u8;
                    let g = (110i16 + var).clamp(0, 255) as u8;
                    let b = (55i16 + var / 2).clamp(0, 255) as u8;
                    img.put_pixel(ox + x, oy + y, Rgba([r, g, b, 255]));
                }
            }
        }

        // Col 2: Zombie Torso
        {
            let ox = 2 * 16;
            let oy = 9 * 16;
            for y in 0..16 {
                for x in 0..16 {
                    // Teal shirt
                    let var = ((x as i16 * 5 + y as i16 * 9) % 14 - 7) as i16;
                    let r = (50i16 + var).clamp(0, 255) as u8;
                    let g = (140i16 + var).clamp(0, 255) as u8;
                    let b = (140i16 + var).clamp(0, 255) as u8;
                    img.put_pixel(ox + x, oy + y, Rgba([r, g, b, 255]));
                }
            }
        }

        // Col 3: Zombie Arms/Legs (dark blue pants + green arms)
        {
            let ox = 3 * 16;
            let oy = 9 * 16;
            for y in 0..16 {
                for x in 0..16 {
                    let var = ((x as i16 * 3 + y as i16 * 11) % 12 - 6) as i16;
                    let r = (40i16 + var).clamp(0, 255) as u8;
                    let g = (40i16 + var).clamp(0, 255) as u8;
                    let b = (80i16 + var).clamp(0, 255) as u8;
                    img.put_pixel(ox + x, oy + y, Rgba([r, g, b, 255]));
                }
            }
        }

        // Col 4: Skeleton Face (front)
        {
            let ox = 4 * 16;
            let oy = 9 * 16;
            for y in 0..16 {
                for x in 0..16 {
                    let is_eye_left = (x >= 2 && x <= 5) && (y >= 4 && y <= 7);
                    let is_eye_right = (x >= 10 && x <= 13) && (y >= 4 && y <= 7);
                    let is_nose = (x >= 7 && x <= 8) && (y >= 7 && y <= 9);
                    let is_mouth = (y >= 11 && y <= 12) && (x >= 3 && x <= 12) && (x % 2 == 0);
                    let c = if is_eye_left || is_eye_right || is_nose {
                        Rgba([15, 15, 15, 255]) // Dark eye sockets
                    } else if is_mouth {
                        Rgba([30, 30, 30, 255]) // Teeth gaps
                    } else {
                        // Bone white with noise
                        let var = ((x as i16 * 7 + y as i16 * 3) % 10 - 5) as i16;
                        let v = (220i16 + var).clamp(0, 255) as u8;
                        Rgba([v, v, (v as i16 - 10).max(0) as u8, 255])
                    };
                    img.put_pixel(ox + x, oy + y, c);
                }
            }
        }

        // Col 5: Skeleton Body parts (bone white)
        {
            let ox = 5 * 16;
            let oy = 9 * 16;
            for y in 0..16 {
                for x in 0..16 {
                    let var = ((x as i16 * 13 + y as i16 * 7) % 12 - 6) as i16;
                    let v = (210i16 + var).clamp(0, 255) as u8;
                    img.put_pixel(
                        ox + x,
                        oy + y,
                        Rgba([v, v, (v as i16 - 15).max(0) as u8, 255]),
                    );
                }
            }
        }

        // Col 6: Creeper Face (front) - iconic sad face pattern
        {
            let ox = 6 * 16;
            let oy = 9 * 16;
            // Creeper face pixel art grid
            let face = [
                [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                [0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0],
                [0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0],
                [0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0],
                [0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            ];
            for y in 0..16u32 {
                for x in 0..16u32 {
                    let c = if face[y as usize][x as usize] == 1 {
                        Rgba([15, 15, 15, 255]) // Black face features
                    } else {
                        // Green mottled skin
                        let var = ((x as i16 * 5 + y as i16 * 11) % 18 - 9) as i16;
                        let r = (80i16 + var).clamp(0, 255) as u8;
                        let g = (160i16 + var).clamp(0, 255) as u8;
                        let b = (70i16 + var / 2).clamp(0, 255) as u8;
                        Rgba([r, g, b, 255])
                    };
                    img.put_pixel(ox + x, oy + y, c);
                }
            }
        }

        // Col 7: Creeper Body (mottled green)
        {
            let ox = 7 * 16;
            let oy = 9 * 16;
            for y in 0..16 {
                for x in 0..16 {
                    let var = ((x as i16 * 9 + y as i16 * 13) % 20 - 10) as i16;
                    let r = (75i16 + var).clamp(0, 255) as u8;
                    let g = (150i16 + var).clamp(0, 255) as u8;
                    let b = (65i16 + var / 2).clamp(0, 255) as u8;
                    img.put_pixel(ox + x, oy + y, Rgba([r, g, b, 255]));
                }
            }
        }

        // Col 8: Arrow
        {
            let ox = 8 * 16;
            let oy = 9 * 16;
            for y in 0..16 {
                for x in 0..16 {
                    // Arrow shaft (brown) with gray tip and red fletching
                    let is_tip = y <= 3 && x >= 6 && x <= 9;
                    let is_shaft = x >= 7 && x <= 8 && y >= 4 && y <= 12;
                    let is_fletch = y >= 13 && ((x >= 5 && x <= 6) || (x >= 9 && x <= 10));
                    let c = if is_tip {
                        Rgba([180, 180, 180, 255]) // Gray stone tip
                    } else if is_shaft {
                        Rgba([140, 100, 55, 255]) // Brown shaft
                    } else if is_fletch {
                        Rgba([200, 200, 210, 255]) // White feather
                    } else {
                        Rgba([140, 100, 55, 255]) // Default brown (entire face is small)
                    };
                    img.put_pixel(ox + x, oy + y, c);
                }
            }
        }

        // Col 9: Bow Wood (Row 9) - Solid wood texture for 3D bow model
        {
            let ox = 9 * 16;
            let oy = 9 * 16;
            for y in 0..16 {
                for x in 0..16 {
                    let var = ((x * 7 + y * 13) % 25) as u8;
                    let r = 130 + var;
                    let g = 80 + var / 2;
                    let b = 40 + var / 2;
                    img.put_pixel(ox + x, oy + y, Rgba([r, g, b, 255]));
                }
            }
        }

        // Col 10: Bow String (Row 9) - Solid white string texture for 3D bow string
        {
            let ox = 10 * 16;
            let oy = 9 * 16;
            for y in 0..16 {
                for x in 0..16 {
                    let var = ((x * 3 + y * 5) % 15) as u8;
                    let c = 230 + var;
                    img.put_pixel(ox + x, oy + y, Rgba([c, c, c + 5, 255]));
                }
            }
        }

        // Row 10: Passive Mob Skins
        // Col 0: Pig Face, Col 1: Pig Body (pink)
        {
            let ox0 = 0 * 16;
            let ox1 = 1 * 16;
            let oy = 10 * 16;
            for y in 0..16 {
                for x in 0..16 {
                    // Pig Face: black eyes and protruding snout
                    let is_eye = (y >= 6 && y <= 7) && (x == 3 || x == 12);
                    let is_snout = (y >= 9 && y <= 11) && (x >= 5 && x <= 10);
                    let is_nostril = is_snout && y == 10 && (x == 6 || x == 9);

                    let c0 = if is_nostril {
                        Rgba([180, 70, 110, 255])
                    } else if is_snout {
                        Rgba([255, 140, 175, 255])
                    } else if is_eye {
                        Rgba([10, 10, 10, 255])
                    } else {
                        let var = ((x * 3 + y * 7) % 15) as u8;
                        Rgba([255, 160 + var, 190 + var / 2, 255])
                    };
                    img.put_pixel(ox0 + x, oy + y, c0);

                    let var = ((x * 7 + y * 3) % 12) as u8;
                    img.put_pixel(ox1 + x, oy + y, Rgba([255, 150 + var, 180 + var / 2, 255]));
                }
            }
        }

        // Col 2: Cow Face, Col 3: Cow Body (black and white)
        {
            let ox2 = 2 * 16;
            let ox3 = 3 * 16;
            let oy = 10 * 16;
            for y in 0..16 {
                for x in 0..16 {
                    // Cow face spots and nose
                    let is_nose = y >= 9 && x >= 4 && x <= 11;
                    let is_eye = (y == 6 || y == 7) && (x == 3 || x == 12);
                    let is_spot = (x * y) % 7 < 3;

                    let c2 = if is_nose {
                        Rgba([230, 160, 160, 255])
                    } else if is_eye {
                        Rgba([10, 10, 10, 255])
                    } else if is_spot {
                        Rgba([40, 40, 40, 255]) // Black spot
                    } else {
                        Rgba([230, 230, 230, 255]) // White base
                    };
                    img.put_pixel(ox2 + x, oy + y, c2);

                    let c3 = if is_spot {
                        Rgba([45, 45, 45, 255])
                    } else {
                        Rgba([225, 225, 225, 255])
                    };
                    img.put_pixel(ox3 + x, oy + y, c3);
                }
            }
        }

        // Col 4: Sheep Head/Legs (skin), Col 5: Wool layer (rough white), Col 6: Sheared sheep skin (pinkish skin)
        {
            let ox4 = 4 * 16;
            let ox5 = 5 * 16;
            let ox6 = 6 * 16;
            let oy = 10 * 16;
            for y in 0..16 {
                for x in 0..16 {
                    // Sheep Head: tan skin
                    let is_eye = (y == 7) && (x == 4 || x == 11);
                    let c4 = if is_eye {
                        Rgba([20, 20, 20, 255])
                    } else {
                        Rgba([235, 215, 190, 255])
                    };
                    img.put_pixel(ox4 + x, oy + y, c4);

                    // Wool: textured white/grey
                    let var = ((x * 13 + y * 7) % 20) as u8;
                    img.put_pixel(
                        ox5 + x,
                        oy + y,
                        Rgba([235 + var, 235 + var, 235 + var, 255]),
                    );

                    // Sheared skin: light pink skin with some sheep features
                    img.put_pixel(ox6 + x, oy + y, Rgba([245, 210, 200, 255]));
                }
            }
        }

        // Col 7: Chicken Head/Beak, Col 8: Chicken Body (white/yellow legs/red wattles)
        {
            let ox7 = 7 * 16;
            let ox8 = 8 * 16;
            let oy = 10 * 16;
            for y in 0..16 {
                for x in 0..16 {
                    // Chicken head: eyes, orange beak
                    let is_eye = y == 5 && (x == 5 || x == 10);
                    let is_beak = y >= 8 && y <= 9 && x >= 6 && x <= 9;
                    let is_wattle = y >= 10 && y <= 11 && x >= 7 && x <= 8; // red neck wattle

                    let c7 = if is_beak {
                        Rgba([255, 160, 0, 255])
                    } else if is_wattle {
                        Rgba([230, 20, 20, 255])
                    } else if is_eye {
                        Rgba([15, 15, 15, 255])
                    } else {
                        Rgba([245, 245, 245, 255])
                    };
                    img.put_pixel(ox7 + x, oy + y, c7);

                    // Chicken body (fluffy white)
                    let var = ((x * 5 + y * 11) % 15) as u8;
                    img.put_pixel(
                        ox8 + x,
                        oy + y,
                        Rgba([240 + var, 240 + var, 240 + var, 255]),
                    );
                }
            }
        }

        // Col 9 (Row 10): Plain player skin (no face features). Used by the
        // first-person arm so it does not inherit the sheep head's eye pixels.
        {
            let ox = 9 * 16;
            let oy = 10 * 16;
            for y in 0..16 {
                for x in 0..16 {
                    let var = ((x * 5 + y * 9) % 8) as u8;
                    img.put_pixel(
                        ox + x,
                        oy + y,
                        Rgba([235 - var / 2, 215 - var / 2, 190 - var / 3, 255]),
                    );
                }
            }
        }

        // Row 11: Passive Mob items
        draw_shears_icon(&mut img, 0, 11);
        draw_bucket_icon(&mut img, 1, 11, None); // Empty bucket
        draw_bucket_icon(&mut img, 2, 11, Some([240, 240, 245, 255])); // Milk bucket (white liquid)

        draw_meat_icon(&mut img, 3, 11, false, [220, 100, 100]); // Raw Porkchop
        draw_meat_icon(&mut img, 4, 11, false, [200, 70, 70]); // Raw Beef
        draw_meat_icon(&mut img, 5, 11, false, [220, 120, 120]); // Raw Mutton
        draw_meat_icon(&mut img, 6, 11, false, [240, 200, 180]); // Raw Chicken

        draw_meat_icon(&mut img, 7, 11, true, [140, 60, 40]); // Cooked Porkchop
        draw_meat_icon(&mut img, 8, 11, true, [120, 50, 30]); // Cooked Beef
        draw_meat_icon(&mut img, 9, 11, true, [140, 70, 50]); // Cooked Mutton
        draw_meat_icon(&mut img, 10, 11, true, [180, 110, 70]); // Cooked Chicken

        // Leather icon (Col 11, Row 11)
        for y in 0..16 {
            for x in 0..16 {
                let is_leather = x >= 4 && x <= 11 && y >= 3 && y <= 12;
                if is_leather {
                    img.put_pixel(11 * 16 + x, 11 * 16 + y, Rgba([140, 80, 40, 255]));
                }
            }
        }
        // Feather (Col 12, Row 11)
        for y in 0..16 {
            for x in 0..16 {
                let is_feather = x + y >= 10 && x + y <= 20 && (x as i32 - y as i32).abs() <= 2;
                if is_feather {
                    img.put_pixel(12 * 16 + x, 11 * 16 + y, Rgba([240, 240, 245, 255]));
                }
            }
        }
        // Egg (Col 13, Row 11)
        for y in 0..16 {
            for x in 0..16 {
                let dx = (x as i32 - 8).abs();
                let dy = (y as i32 - 8).abs();
                if dx * dx + dy * dy <= 12 {
                    img.put_pixel(13 * 16 + x, 11 * 16 + y, Rgba([240, 235, 215, 255]));
                }
            }
        }

        // Draw Row 12 Blocks & Items
        // Birch Log Top/Bottom & Side
        draw_noise(&mut img, 0, 12, [215, 200, 175], 5, &mut seed);
        draw_birch_bark(&mut img, 1, 12, &mut seed);
        draw_noise(&mut img, 2, 12, [225, 210, 180], 5, &mut seed); // Birch Planks
        draw_leaves(&mut img, 3, 12, &mut seed); // Birch Leaves (green)

        // Spruce Log Top/Bottom & Side
        draw_noise(&mut img, 4, 12, [100, 75, 50], 8, &mut seed);
        draw_spruce_bark(&mut img, 5, 12, &mut seed);
        draw_noise(&mut img, 6, 12, [105, 80, 55], 5, &mut seed); // Spruce Planks
        draw_leaves(&mut img, 7, 12, &mut seed); // Spruce Leaves (dark pine green)

        // Decorative Plants & Cacti
        draw_tall_grass(&mut img, 8, 12);
        draw_flower(&mut img, 9, 12, [240, 220, 40]); // Dandelion (Yellow)
        draw_flower(&mut img, 10, 12, [230, 30, 30]); // Poppy (Red)
        draw_cactus(&mut img, 11, 12);
        draw_sugar_cane(&mut img, 12, 12);
        draw_pumpkin(&mut img, 13, 12);
        draw_melon(&mut img, 14, 12);
        draw_fire(&mut img, 15, 12);

        // Row 13-14: enchanting / brewing workstations and compact item icons.
        draw_noise(&mut img, 0, 13, [45, 20, 75], 12, &mut seed); // Enchanting table
        draw_noise(&mut img, 1, 13, [125, 80, 45], 12, &mut seed); // Brewing stand
        draw_noise(&mut img, 2, 13, [70, 72, 78], 8, &mut seed); // Anvil
        draw_noise(&mut img, 3, 13, [35, 55, 190], 25, &mut seed); // Lapis
        for col in 4..=7 {
            draw_noise(&mut img, col, 13, [190, 195, 200], 10, &mut seed);
        }
        draw_glass(&mut img, 8, 13);
        draw_noise(&mut img, 9, 13, [65, 80, 220], 16, &mut seed);
        draw_noise(&mut img, 10, 13, [135, 65, 210], 16, &mut seed);
        draw_noise(&mut img, 11, 13, [120, 25, 35], 15, &mut seed);
        draw_noise(&mut img, 12, 13, [235, 235, 225], 8, &mut seed);
        draw_noise(&mut img, 13, 13, [240, 165, 35], 18, &mut seed);
        draw_noise(&mut img, 14, 13, [245, 195, 40], 12, &mut seed);
        draw_noise(&mut img, 15, 13, [225, 225, 235], 8, &mut seed);
        draw_noise(&mut img, 0, 14, [235, 165, 30], 12, &mut seed);
        draw_noise(&mut img, 1, 14, [100, 40, 55], 12, &mut seed);
        draw_noise(&mut img, 2, 14, [210, 80, 35], 14, &mut seed);
        draw_noise(&mut img, 3, 14, [75, 135, 90], 16, &mut seed);
        draw_noise(&mut img, 4, 14, [110, 25, 30], 12, &mut seed);
        draw_noise(&mut img, 5, 14, [235, 210, 85], 18, &mut seed);
        draw_redstone_icon(&mut img, 6, 14);
        draw_noise(&mut img, 7, 14, [175, 175, 180], 8, &mut seed);

        // Redstone components. The compact atlas intentionally shares a tile
        // between each component's powered/unpowered block variants, except
        // for the lamp where emissive state needs an obvious visual cue.
        draw_noise(&mut img, 5, 2, [95, 30, 30], 8, &mut seed); // wire
        draw_noise(&mut img, 6, 2, [150, 45, 35], 12, &mut seed); // redstone torch
        draw_noise(&mut img, 7, 2, [175, 170, 165], 7, &mut seed); // repeater
        draw_noise(&mut img, 8, 2, [155, 150, 145], 7, &mut seed); // comparator
        draw_noise(&mut img, 9, 2, [110, 110, 110], 5, &mut seed); // button
        draw_noise(&mut img, 10, 2, [95, 75, 55], 8, &mut seed); // lever
        draw_noise(&mut img, 11, 2, [130, 130, 130], 5, &mut seed); // plate
        draw_noise(&mut img, 12, 2, [125, 115, 95], 9, &mut seed); // piston
        draw_noise(&mut img, 13, 2, [90, 135, 70], 9, &mut seed); // sticky piston
        draw_noise(&mut img, 14, 2, [95, 70, 40], 12, &mut seed); // lamp off
        draw_noise(&mut img, 15, 2, [235, 90, 25], 14, &mut seed); // lava
        draw_noise(&mut img, 8, 14, [245, 185, 65], 10, &mut seed); // lamp lit
        draw_planks(&mut img, 9, 14, [145, 95, 50], &mut seed); // door
        draw_planks(&mut img, 10, 14, [125, 80, 42], &mut seed); // trapdoor
        draw_noise(&mut img, 11, 14, [100, 105, 110], 8, &mut seed); // dispenser
        draw_noise(&mut img, 12, 14, [85, 90, 95], 8, &mut seed); // dropper
        draw_planks(&mut img, 13, 14, [115, 70, 45], &mut seed); // note block

        // Dimensions and boss encounters. Crack stages occupy row 15 columns
        // 0..=9; the remaining compact-atlas slots are reserved here.
        draw_noise(&mut img, 10, 15, [105, 32, 34], 24, &mut seed); // netherrack
        draw_noise(&mut img, 11, 15, [82, 64, 52], 16, &mut seed); // soul sand
        draw_noise(&mut img, 12, 15, [238, 188, 72], 28, &mut seed); // glowstone
        draw_noise(&mut img, 13, 15, [105, 25, 170], 30, &mut seed); // Nether portal
        draw_noise(&mut img, 14, 15, [214, 220, 145], 13, &mut seed); // End stone
        draw_noise(&mut img, 15, 15, [78, 108, 82], 16, &mut seed); // End frame
        draw_noise(&mut img, 9, 10, [48, 18, 24], 10, &mut seed); // Nether brick
        draw_noise(&mut img, 10, 10, [94, 65, 112], 16, &mut seed); // End City chest
        draw_noise(&mut img, 14, 10, [12, 8, 24], 20, &mut seed); // End portal
        draw_noise(&mut img, 15, 10, [168, 102, 190], 14, &mut seed); // purpur
        draw_noise(&mut img, 14, 11, [28, 12, 36], 13, &mut seed); // dragon egg
        draw_noise(&mut img, 15, 11, [38, 34, 38], 12, &mut seed); // wither skull
        draw_noise(&mut img, 11, 10, [125, 130, 130], 22, &mut seed); // flint and steel
        draw_noise(&mut img, 12, 10, [48, 185, 110], 25, &mut seed); // Eye of Ender
        draw_noise(&mut img, 13, 10, [175, 145, 190], 18, &mut seed); // elytra
        draw_noise(&mut img, 3, 4, [240, 245, 230], 14, &mut seed); // Nether Star
        draw_noise(&mut img, 4, 4, [220, 80, 220], 25, &mut seed); // End Crystal
        draw_noise(&mut img, 5, 4, [235, 165, 55], 18, &mut seed); // Blaze rod
        draw_noise(&mut img, 6, 4, [90, 150, 95], 18, &mut seed); // filled End frame
        draw_noise(&mut img, 14, 14, [118, 72, 150], 14, &mut seed); // Shulker shell

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
