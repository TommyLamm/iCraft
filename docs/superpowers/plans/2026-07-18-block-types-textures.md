# Block Types & Textures Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand block types to 30+, upgrade texture atlas to 256x256 with procedural patterns, implement dual-mesh rendering passes for opaque/translucent blocks, and enrich world generation with bedrock, ores, and beaches.

**Architecture:** Split each Chunk's mesh into opaque/cutout vertices and translucent vertices. Render them using two separate WGPU rendering pipelines (opaque with depth writing, transparent with alpha blending and read-only depth). Upgrade the procedural texture generator. Enrichment of the terrain generation loop.

**Tech Stack:** Rust, WGPU, WGSL, glam, noise

---

### Task 1: Block Types and Properties definition

**Files:**
- Modify: `F:\Desktop\MC\src\world.rs:9-17` (Implement expanded `BlockType` enum and `properties` lookup table)

- [ ] **Step 1: Write the updated enum and properties table**
Create the `RenderType` enum, `BlockProperties` struct, and implement `properties(&self) -> BlockProperties` for `BlockType` in `src/world.rs`.
```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum BlockType {
    Air = 0,
    Grass = 1,
    Dirt = 2,
    Stone = 3,
    Sand = 4,
    Gravel = 5,
    OakLog = 6,
    OakPlanks = 7,
    OakLeaves = 8,
    Cobblestone = 9,
    Bedrock = 10,
    Water = 11,
    CoalOre = 12,
    IronOre = 13,
    GoldOre = 14,
    DiamondOre = 15,
    RedstoneOre = 16,
    Glass = 17,
    Brick = 18,
    StoneBrick = 19,
    Snow = 20,
    Ice = 21,
    Clay = 22,
    Sandstone = 23,
    Obsidian = 24,
    CraftingTable = 25,
    Furnace = 26,
    Chest = 27,
    TNT = 28,
    Bookshelf = 29,
    Torch = 30,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum RenderType {
    Opaque,
    Cutout,
    Translucent,
}

pub struct BlockProperties {
    pub name: &'static str,
    pub hardness: f32,
    pub render_type: RenderType,
    pub is_solid: bool,
    pub is_passable: bool,
    pub light_emission: u8,
}

impl BlockType {
    pub fn properties(self) -> BlockProperties {
        match self {
            BlockType::Air => BlockProperties {
                name: "Air",
                hardness: 0.0,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::Grass => BlockProperties {
                name: "Grass Block",
                hardness: 0.6,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Dirt => BlockProperties {
                name: "Dirt",
                hardness: 0.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Stone => BlockProperties {
                name: "Stone",
                hardness: 1.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Sand => BlockProperties {
                name: "Sand",
                hardness: 0.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Gravel => BlockProperties {
                name: "Gravel",
                hardness: 0.6,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::OakLog => BlockProperties {
                name: "Oak Log",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::OakPlanks => BlockProperties {
                name: "Oak Planks",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::OakLeaves => BlockProperties {
                name: "Oak Leaves",
                hardness: 0.2,
                render_type: RenderType::Cutout,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Cobblestone => BlockProperties {
                name: "Cobblestone",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Bedrock => BlockProperties {
                name: "Bedrock",
                hardness: -1.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Water => BlockProperties {
                name: "Water",
                hardness: 100.0,
                render_type: RenderType::Translucent,
                is_solid: false,
                is_passable: true,
                light_emission: 0,
            },
            BlockType::CoalOre => BlockProperties {
                name: "Coal Ore",
                hardness: 3.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::IronOre => BlockProperties {
                name: "Iron Ore",
                hardness: 3.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::GoldOre => BlockProperties {
                name: "Gold Ore",
                hardness: 3.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::DiamondOre => BlockProperties {
                name: "Diamond Ore",
                hardness: 3.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::RedstoneOre => BlockProperties {
                name: "Redstone Ore",
                hardness: 3.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Glass => BlockProperties {
                name: "Glass",
                hardness: 0.3,
                render_type: RenderType::Cutout,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Brick => BlockProperties {
                name: "Brick",
                hardness: 2.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::StoneBrick => BlockProperties {
                name: "Stone Brick",
                hardness: 1.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Snow => BlockProperties {
                name: "Snow",
                hardness: 0.1,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Ice => BlockProperties {
                name: "Ice",
                hardness: 0.5,
                render_type: RenderType::Translucent,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Clay => BlockProperties {
                name: "Clay",
                hardness: 0.6,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Sandstone => BlockProperties {
                name: "Sandstone",
                hardness: 0.8,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Obsidian => BlockProperties {
                name: "Obsidian",
                hardness: 50.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::CraftingTable => BlockProperties {
                name: "Crafting Table",
                hardness: 2.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Furnace => BlockProperties {
                name: "Furnace",
                hardness: 3.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Chest => BlockProperties {
                name: "Chest",
                hardness: 2.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::TNT => BlockProperties {
                name: "TNT",
                hardness: 0.0,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Bookshelf => BlockProperties {
                name: "Bookshelf",
                hardness: 1.5,
                render_type: RenderType::Opaque,
                is_solid: true,
                is_passable: false,
                light_emission: 0,
            },
            BlockType::Torch => BlockProperties {
                name: "Torch",
                hardness: 0.0,
                render_type: RenderType::Cutout,
                is_solid: false,
                is_passable: false,
                light_emission: 14,
            },
        }
    }
}
```

- [ ] **Step 2: Commit**
```bash
git add src/world.rs
git commit -m "feat: expand BlockType enum and implement BlockProperties"
```

---

### Task 2: Update Collision and Interaction Systems

**Files:**
- Modify: `F:\Desktop\MC\src\physics.rs:92-94` (Use `properties().is_solid` for collision checks)
- Modify: `F:\Desktop\MC\src\interaction.rs:37-39` (Use `properties().is_passable` or similar logic for raycast target filtering)

- [ ] **Step 1: Update `src/physics.rs`**
Replace `if block != crate::world::BlockType::Air` with:
```rust
if block.properties().is_solid {
```

- [ ] **Step 2: Update `src/interaction.rs`**
In `raycast()`, target blocks that are non-Air and not fully passable:
```rust
let props = block.properties();
if block != BlockType::Air && !props.is_passable {
    return Some(RaycastResult {
        block_pos: Vec3::new(x as f32, y as f32, z as f32),
        normal: last_face,
    });
}
```

- [ ] **Step 3: Run unit tests to check formatting & consistency**
Run: `cargo test`
Expected: PASS

- [ ] **Step 4: Commit**
```bash
git add src/physics.rs src/interaction.rs
git commit -m "feat: adjust physics and raycast to use block properties"
```

---

### Task 3: Upgrade Texture Atlas (256x256 Atlas Generation)

**Files:**
- Modify: `F:\Desktop\MC\src\texture.rs` (Generate 256x256 image with procedural blocks)

- [ ] **Step 1: Write helper drawer functions in `src/texture.rs`**
Add clean procedural helper functions to draw various Minecraft styles:
```rust
// In src/texture.rs
use image::{RgbaImage, Rgba};

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
            let is_fire = (x >= 6 && x <= 8 && y >= 2 && y <= 4);
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
```

- [ ] **Step 2: Generate full 256x256 image in `TextureAtlas::new_procedural`**
Update `new_procedural` to build a 256x256 image and populate the grid cells:
```rust
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
                let is_tool = (x > 3 && x < 12 && y > 3 && y < 12);
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
                let c = if is_shelf { [90, 60, 30] } else { [160, 80, 60] }; // Books pattern
                img.put_pixel(3 * 16 + x, 2 * 16 + y, Rgba([c[0], c[1], c[2], 255]));
            }
        }
        draw_torch(&mut img, 4, 2);

        // Fill remaining spaces with zero transparent
        for ty in 2..16 {
            for tx in 0..16 {
                if ty == 2 && tx <= 4 { continue; }
                for y in 0..16 {
                    for x in 0..16 {
                        img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0]));
                    }
                }
            }
        }
```

- [ ] **Step 3: Commit**
```bash
git add src/texture.rs
git commit -m "feat: upgrade texture atlas generation to 256x256 with complex procedural rules"
```

---

### Task 4: Dual Mesh Generation & UV Lookups

**Files:**
- Modify: `F:\Desktop\MC\src\world.rs` (Upgraded `generate_mesh` returning opaque and transparent buffers)

- [ ] **Step 1: Write UV lookup helper in `src/world.rs`**
Add mapping from `BlockType` and face index to UV coordinates in `src/world.rs`.
```rust
// In src/world.rs
impl BlockType {
    pub fn get_face_tex_index(self, face_idx: usize) -> (u32, u32) {
        // face_idx matches faces: South (0), North (1), West (2), East (3), Up (4), Down (5)
        match self {
            BlockType::Grass => {
                if face_idx == 4 { (0, 0) } // Up
                else if face_idx == 5 { (2, 0) } // Down (Dirt)
                else { (1, 0) } // Side
            }
            BlockType::Dirt => (2, 0),
            BlockType::Stone => (3, 0),
            BlockType::Sand => (4, 0),
            BlockType::Gravel => (5, 0),
            BlockType::OakPlanks => (6, 0),
            BlockType::OakLeaves => (7, 0),
            BlockType::Cobblestone => (8, 0),
            BlockType::Bedrock => (9, 0),
            BlockType::Water => (10, 0),
            BlockType::CoalOre => (11, 0),
            BlockType::IronOre => (12, 0),
            BlockType::GoldOre => (13, 0),
            BlockType::DiamondOre => (14, 0),
            BlockType::RedstoneOre => (15, 0),
            
            BlockType::Glass => (0, 1),
            BlockType::Brick => (1, 1),
            BlockType::StoneBrick => (2, 1),
            BlockType::Snow => {
                if face_idx == 4 { (3, 1) } // Up
                else if face_idx == 5 { (2, 0) } // Down (Dirt)
                else { (4, 1) } // Side
            }
            BlockType::Ice => (5, 1),
            BlockType::Clay => (6, 1),
            BlockType::Sandstone => {
                if face_idx == 4 || face_idx == 5 { (7, 1) }
                else { (8, 1) }
            }
            BlockType::Obsidian => (9, 1),
            BlockType::OakLog => {
                if face_idx == 4 || face_idx == 5 { (10, 1) }
                else { (11, 1) }
            }
            BlockType::CraftingTable => {
                if face_idx == 4 { (12, 1) }
                else if face_idx == 5 { (6, 0) } // Oak Planks
                else { (13, 1) }
            }
            BlockType::Furnace => {
                if face_idx == 0 { (14, 1) } // Front face
                else { (3, 0) } // Stone sides
            }
            BlockType::Chest => (15, 1),
            
            BlockType::TNT => {
                if face_idx == 4 { (0, 2) }
                else if face_idx == 5 { (1, 2) }
                else { (2, 2) }
            }
            BlockType::Bookshelf => {
                if face_idx == 4 || face_idx == 5 { (6, 0) } // Planks
                else { (3, 2) }
            }
            BlockType::Torch => (4, 2),
            BlockType::Air => (0, 0),
        }
    }
}
```

- [ ] **Step 2: Update `Chunk::generate_mesh` signature and output**
Update `generate_mesh` in `src/world.rs` to return `(Vec<Vertex>, Vec<u32>, Vec<Vertex>, Vec<u32>)` (Opaque, Transparent):
```rust
    pub fn generate_mesh<F>(&self, get_block_at: F) -> (Vec<Vertex>, Vec<u32>, Vec<Vertex>, Vec<u32>)
    where
        F: Fn(i32, i32, i32) -> BlockType,
    {
        let mut opaque_vertices = Vec::new();
        let mut opaque_indices = Vec::new();
        let mut trans_vertices = Vec::new();
        let mut trans_indices = Vec::new();

        // faces definition...
        let faces = [ ... ]; // Same array as before
```

Update face culling: do not cull faces next to transparent/cutout/translucent blocks.
```rust
                    for (face_idx, (normal, corner_data)) in faces.iter().enumerate() {
                        let nx = world_x + normal[0] as i32;
                        let ny = world_y + normal[1] as i32;
                        let nz = world_z + normal[2] as i32;

                        let neighbor = get_block_at(nx, ny, nz);
                        let neighbor_props = neighbor.properties();

                        // Cull if neighbor is Opaque AND it's not the same block type (to prevent internal water-water culling, or water adjacent to water culling)
                        let should_render = if neighbor == BlockType::Air {
                            true
                        } else if neighbor_props.render_type != RenderType::Opaque {
                            // If drawing water next to water, cull it to prevent inside mesh clutter
                            !(block == BlockType::Water && neighbor == BlockType::Water)
                        } else {
                            false
                        };

                        if should_render {
                            let block_render_type = block.properties().render_type;
                            let is_translucent = block_render_type == RenderType::Translucent;
                            
                            let (v_list, i_list) = if is_translucent {
                                (&mut trans_vertices, &mut trans_indices)
                            } else {
                                (&mut opaque_vertices, &mut opaque_indices)
                            };
                            
                            let start_idx = v_list.len() as u32;
                            let (tx_col, tx_row) = block.get_face_tex_index(face_idx);

                            for (offset, uv) in corner_data.iter() {
                                // 256x256 texture atlas: 16 columns and 16 rows.
                                // Each sub-texture occupies 1.0/16.0 = 0.0625.
                                let u = (uv[0] + tx_col as f32) * 0.0625;
                                let v = (uv[1] + tx_row as f32) * 0.0625;
                                v_list.push(Vertex {
                                    position: [
                                        world_x as f32 + offset[0],
                                        world_y as f32 + offset[1],
                                        world_z as f32 + offset[2],
                                    ],
                                    tex_coords: [u, v],
                                });
                            }

                            i_list.push(start_idx + 0);
                            i_list.push(start_idx + 1);
                            i_list.push(start_idx + 2);
                            i_list.push(start_idx + 0);
                            i_list.push(start_idx + 2);
                            i_list.push(start_idx + 3);
                        }
                    }
```

- [ ] **Step 3: Commit**
```bash
git add src/world.rs
git commit -m "feat: implement dual mesh classification and neighbor culling check in generate_mesh"
```

---

### Task 5: Upgrade ChunkMesh in State to Support Dual Buffers

**Files:**
- Modify: `F:\Desktop\MC\src\state.rs:11-16` (Update ChunkMesh structure)
- Modify: `F:\Desktop\MC\src\state.rs:385-420` (Build spawn chunk dual meshes)
- Modify: `F:\Desktop\MC\src\state.rs:617-660` (Rebuild dirty chunk meshes with dual buffers)

- [ ] **Step 1: Modify `ChunkMesh` structure in `src/state.rs`**
```rust
pub struct ChunkMesh {
    pub opaque_vertex_buffer: wgpu::Buffer,
    pub opaque_index_buffer: wgpu::Buffer,
    pub opaque_num_indices: u32,
    
    pub transparent_vertex_buffer: wgpu::Buffer,
    pub transparent_index_buffer: wgpu::Buffer,
    pub transparent_num_indices: u32,

    pub dirty: bool,
}
```

- [ ] **Step 2: Update spawn chunk mesh building in `State::new()`**
Change line 390 to call `generate_mesh` and create separate buffers:
```rust
                let (o_verts, o_inds, t_verts, t_inds) = chunk.generate_mesh(|wx, wy, wz| {
                    let cx_neighbor = wx.div_euclid(crate::world::CHUNK_WIDTH as i32);
                    let cz_neighbor = wz.div_euclid(crate::world::CHUNK_DEPTH as i32);
                    let bx_neighbor = wx.rem_euclid(crate::world::CHUNK_WIDTH as i32) as usize;
                    let bz_neighbor = wz.rem_euclid(crate::world::CHUNK_DEPTH as i32) as usize;
                    if wy < 0 || wy >= crate::world::CHUNK_HEIGHT as i32 {
                        return BlockType::Air;
                    }
                    if let Some(c) = chunks_ref.get(&(cx_neighbor, cz_neighbor)) {
                        c.blocks[bx_neighbor][wy as usize][bz_neighbor]
                    } else {
                        BlockType::Air
                    }
                });

                let opaque_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Chunk Opaque Vertex Buffer"),
                    contents: bytemuck::cast_slice(&o_verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                let opaque_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Chunk Opaque Index Buffer"),
                    contents: bytemuck::cast_slice(&o_inds),
                    usage: wgpu::BufferUsages::INDEX,
                });
                let transparent_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Chunk Translucent Vertex Buffer"),
                    contents: bytemuck::cast_slice(&t_verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                let transparent_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Chunk Translucent Index Buffer"),
                    contents: bytemuck::cast_slice(&t_inds),
                    usage: wgpu::BufferUsages::INDEX,
                });

                chunk_meshes.insert((cx, cz), ChunkMesh {
                    opaque_vertex_buffer,
                    opaque_index_buffer,
                    opaque_num_indices: o_inds.len() as u32,
                    transparent_vertex_buffer,
                    transparent_index_buffer,
                    transparent_num_indices: t_inds.len() as u32,
                    dirty: false,
                });
```

- [ ] **Step 3: Update rate-limited rebuild in `State::update()`**
Similarly update the code inside `State::update()` around line 620 to build and insert the new `ChunkMesh`:
```rust
            let (o_verts, o_inds, t_verts, t_inds) = chunk.generate_mesh(|wx, wy, wz| {
                let cx_neighbor = wx.div_euclid(crate::world::CHUNK_WIDTH as i32);
                let cz_neighbor = wz.div_euclid(crate::world::CHUNK_DEPTH as i32);
                let bx_neighbor = wx.rem_euclid(crate::world::CHUNK_WIDTH as i32) as usize;
                let bz_neighbor = wz.rem_euclid(crate::world::CHUNK_DEPTH as i32) as usize;
                if wy < 0 || wy >= crate::world::CHUNK_HEIGHT as i32 {
                    return BlockType::Air;
                }
                if let Some(c) = chunks_ref.get(&(cx_neighbor, cz_neighbor)) {
                    c.blocks[bx_neighbor][wy as usize][bz_neighbor]
                } else {
                    BlockType::Air
                }
            });

            let opaque_vertex_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Chunk Opaque Vertex Buffer"),
                contents: bytemuck::cast_slice(&o_verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let opaque_index_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Chunk Opaque Index Buffer"),
                contents: bytemuck::cast_slice(&o_inds),
                usage: wgpu::BufferUsages::INDEX,
            });
            let transparent_vertex_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Chunk Translucent Vertex Buffer"),
                contents: bytemuck::cast_slice(&t_verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let transparent_index_buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Chunk Translucent Index Buffer"),
                contents: bytemuck::cast_slice(&t_inds),
                usage: wgpu::BufferUsages::INDEX,
            });

            self.chunk_meshes.insert((cx, cz), ChunkMesh {
                opaque_vertex_buffer,
                opaque_index_buffer,
                opaque_num_indices: o_inds.len() as u32,
                transparent_vertex_buffer,
                transparent_index_buffer,
                transparent_num_indices: t_inds.len() as u32,
                dirty: false,
            });
```

- [ ] **Step 4: Commit**
```bash
git add src/state.rs
git commit -m "feat: adjust ChunkMesh data structures and update mesh initialization"
```

---

### Task 6: Transparent Pipelines and Blending Shader

**Files:**
- Modify: `F:\Desktop\MC\src\shader.wgsl` (Implement alpha test / discard)
- Modify: `F:\Desktop\MC\src\state.rs` (Define `trans_pipeline` and render in two passes)

- [ ] **Step 1: Update `src/shader.wgsl` fragment shader**
Discard fragments where the alpha channel is less than 0.5:
```wgsl
// In src/shader.wgsl
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_color = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    if (tex_color.a < 0.5) {
        discard;
    }
    return tex_color;
}
```

- [ ] **Step 2: Add `trans_pipeline` field to `State`**
In `pub struct State`, add:
```rust
    trans_pipeline: wgpu::RenderPipeline,
```

- [ ] **Step 3: Initialize `trans_pipeline` in `State::new()`**
Create the transparent rendering pipeline in `State::new()` (right after `render_pipeline` initialization):
```rust
        let trans_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Translucent Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Cw,
                cull_mode: None, // Disable culling for transparent faces to look correct
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false, // Read only depth for water
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });
```

- [ ] **Step 4: Update render loop in `State::render()`**
Around line 950, split rendering of meshes into opaque pass and translucent pass:
```rust
            // Pass 1: Opaque & Cutout
            render_pass.set_pipeline(&self.render_pipeline);
            render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
            for mesh in self.chunk_meshes.values() {
                if mesh.opaque_num_indices > 0 {
                    render_pass.set_vertex_buffer(0, mesh.opaque_vertex_buffer.slice(..));
                    render_pass.set_index_buffer(mesh.opaque_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.draw_indexed(0..mesh.opaque_num_indices, 0, 0..1);
                }
            }

            // Pass 2: Translucent (Water/Ice)
            render_pass.set_pipeline(&self.trans_pipeline);
            for mesh in self.chunk_meshes.values() {
                if mesh.transparent_num_indices > 0 {
                    render_pass.set_vertex_buffer(0, mesh.transparent_vertex_buffer.slice(..));
                    render_pass.set_index_buffer(mesh.transparent_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    render_pass.draw_indexed(0..mesh.transparent_num_indices, 0, 0..1);
                }
            }
```

- [ ] **Step 5: Run a compile check**
Run: `cargo check`
Expected: COMPILE SUCCESS

- [ ] **Step 6: Commit**
```bash
git add src/shader.wgsl src/state.rs
git commit -m "feat: add trans_pipeline with alpha blending and render transparent blocks in a separate pass"
```

---

### Task 7: Improve World Generation

**Files:**
- Modify: `F:\Desktop\MC\src\world.rs:24-56` (Update Chunk::new terrain loop)

- [ ] **Step 1: Refactor terrain generator loop in `Chunk::new`**
```rust
// In src/world.rs
    pub fn new(chunk_x: i32, chunk_z: i32) -> Self {
        let mut blocks = [[[BlockType::Air; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH];
        let perlin = Perlin::new(12345); // Seed: 12345

        // Simple custom PRNG for ore distribution and bedrock blending
        let mut rng_seed = (chunk_x as u32).wrapping_mul(31) ^ (chunk_z as u32);
        let mut next_rand = |min: u8, max: u8| -> u8 {
            rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
            let val = (rng_seed / 65536) % 32768;
            let diff = max - min;
            if diff == 0 { return min; }
            min + (val % diff as u32) as u8
        };

        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                // Calculate surface height using Perlin noise
                let world_x = chunk_x * (CHUNK_WIDTH as i32) + x as i32;
                let world_z = chunk_z * (CHUNK_DEPTH as i32) + z as i32;
                let noise_val = perlin.get([world_x as f64 * 0.04, world_z as f64 * 0.04]);
                // Map noise value (-1.0 to 1.0) to height (e.g. 55 to 75)
                let base_height = (64.0 + noise_val * 12.0) as usize;
                
                let is_beach = base_height <= 63;
                let height = if base_height < 62 { 62 } else { base_height }; // Sea level is at 62

                for y in 0..CHUNK_HEIGHT {
                    // Bedrock Y=0-4
                    if y <= 4 {
                        if y == 0 {
                            blocks[x][y][z] = BlockType::Bedrock;
                        } else {
                            // Blended bedrock
                            let threshold = (5 - y) as u8 * 50; // Chance of bedrock
                            if next_rand(0, 255) < threshold {
                                blocks[x][y][z] = BlockType::Bedrock;
                            } else {
                                blocks[x][y][z] = BlockType::Stone;
                            }
                        }
                    }
                    // Underground Stone Layer
                    else if y < base_height - 4 {
                        // Ore generation distribution
                        let block = if y < 16 && next_rand(0, 100) < 1 {
                            if next_rand(0, 2) == 0 { BlockType::DiamondOre } else { BlockType::RedstoneOre }
                        } else if y < 32 && next_rand(0, 100) < 2 {
                            BlockType::GoldOre
                        } else if y < 64 && next_rand(0, 100) < 3 {
                            BlockType::IronOre
                        } else if y < 128 && next_rand(0, 100) < 5 {
                            BlockType::CoalOre
                        } else {
                            BlockType::Stone
                        };
                        blocks[x][y][z] = block;
                    }
                    // Dirt/Sand layer
                    else if y < base_height {
                        if is_beach {
                            blocks[x][y][z] = BlockType::Sand;
                        } else {
                            blocks[x][y][z] = BlockType::Dirt;
                        }
                    }
                    // Surface block
                    else if y == base_height {
                        if is_beach {
                            blocks[x][y][z] = BlockType::Sand;
                        } else {
                            blocks[x][y][z] = BlockType::Grass;
                        }
                    }
                    // Water/Air layer above base height
                    else {
                        if y <= 62 {
                            blocks[x][y][z] = BlockType::Water;
                        } else {
                            blocks[x][y][z] = BlockType::Air;
                        }
                    }
                }
            }
        }
        Self {
            chunk_x,
            chunk_z,
            blocks,
        }
    }
```

- [ ] **Step 2: Run verification and compile**
Run: `cargo test` and `cargo check`
Expected: ALL PASS

- [ ] **Step 3: Commit**
```bash
git add src/world.rs
git commit -m "feat: improve world generation with bedrock, beach, sand, water levels, and ore distribution"
```
