# Trees & Biomes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a multi-biome world generator (Plains, Forest, Desert, Taiga, Swamp, Mountains, Ocean), 3 tree species (Oak, Birch, Spruce) with neighbor-projection, decorative plants (Tall Grass, Flowers, Cactus, Sugar Cane, Pumpkin, Melon), leaf decay simulation, and cactus contact damage.

**Architecture:** Biomes are calculated using Temperature, Moisture, and Ocean Perlin noises and interpolated smoothly over a 3x3 grid. Trees are generated deterministically based on neighbor-chunk seeds and projected onto the current chunk. Leaf decay runs via random ticks in the game loop, and cactus damage is checked via AABB intersections in the physics update.

**Tech Stack:** Rust, wgpu (WGSL Shaders), noise crate

---

### Task 1: Update BlockType and Item Enums

**Files:**
- Modify: [src/world.rs](file:///f:/Desktop/MC/src/world.rs)
- Modify: [src/inventory.rs](file:///f:/Desktop/MC/src/inventory.rs)

- [ ] **Step 1: Add new block types to `BlockType` enum in `src/world.rs`**
  Modify `BlockType` enum starting around line 11:
  ```rust
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
      Lava = 31,
      // Trees & Biomes Additions
      BirchLog = 32,
      BirchPlanks = 33,
      BirchLeaves = 34,
      SpruceLog = 35,
      SprucePlanks = 36,
      SpruceLeaves = 37,
      TallGrass = 38,
      Dandelion = 39,
      Poppy = 40,
      Cactus = 41,
      SugarCane = 42,
      Pumpkin = 43,
      Melon = 44,
  }
  ```

- [ ] **Step 2: Add corresponding items to `Item` enum in `src/inventory.rs`**
  Modify `Item` enum starting around line 25:
  ```rust
  pub enum Item {
      Air,
      Grass,
      Dirt,
      Stone,
      Sand,
      Gravel,
      OakLog,
      OakPlanks,
      OakLeaves,
      Cobblestone,
      Bedrock,
      Water,
      CoalOre,
      IronOre,
      GoldOre,
      DiamondOre,
      RedstoneOre,
      Glass,
      Brick,
      StoneBrick,
      Snow,
      Ice,
      Clay,
      Sandstone,
      Obsidian,
      CraftingTable,
      Furnace,
      Chest,
      TNT,
      Bookshelf,
      Torch,
      Lava,
      // Existing food & items
      Apple, Bread,
      RottenFlesh, Bone, Bow, Gunpowder,
      Wheat, Seeds, Carrot, Shears, Bucket, MilkBucket,
      RawPorkchop, CookedPorkchop, RawBeef, CookedBeef,
      RawMutton, CookedMutton, RawChicken, CookedChicken,
      Wool, Leather, Feather, Egg, RedDye, BlueDye, GreenDye,
      // Trees & Biomes Additions
      BirchLog,
      BirchPlanks,
      BirchLeaves,
      SpruceLog,
      SprucePlanks,
      SpruceLeaves,
      TallGrass,
      Dandelion,
      Poppy,
      Cactus,
      SugarCane,
      Pumpkin,
      Melon,
  }
  ```

- [ ] **Step 3: Run verification compilation**
  Run: `cargo check`
  Expected: FAIL (missing match patterns for the new enum values)

---

### Task 2: Implement Block & Item Properties and Recipes

**Files:**
- Modify: [src/world.rs](file:///f:/Desktop/MC/src/world.rs)
- Modify: [src/inventory.rs](file:///f:/Desktop/MC/src/inventory.rs)
- Modify: [src/crafting.rs](file:///f:/Desktop/MC/src/crafting.rs)

- [ ] **Step 1: Define properties and texture atlas indices in `src/world.rs`**
  Modify `BlockType::sound_material`, `BlockType::properties`, and `BlockType::get_face_tex_index`:
  ```rust
  // sound_material additions:
  BlockType::BirchLeaves | BlockType::SpruceLeaves | BlockType::TallGrass | BlockType::Dandelion | BlockType::Poppy | BlockType::SugarCane => Some(crate::audio::SoundMaterial::Grass),
  BlockType::BirchLog | BlockType::BirchPlanks | BlockType::SpruceLog | BlockType::SprucePlanks | BlockType::Pumpkin | BlockType::Melon => Some(crate::audio::SoundMaterial::Wood),
  BlockType::Cactus => Some(crate::audio::SoundMaterial::Gravel),

  // properties additions in BlockType::properties:
  BlockType::BirchLog => BlockProperties { name: "Birch Log", hardness: 2.0, render_type: RenderType::Opaque, is_solid: true, is_passable: false, light_emission: 0 },
  BlockType::BirchPlanks => BlockProperties { name: "Birch Planks", hardness: 2.0, render_type: RenderType::Opaque, is_solid: true, is_passable: false, light_emission: 0 },
  BlockType::BirchLeaves => BlockProperties { name: "Birch Leaves", hardness: 0.2, render_type: RenderType::Cutout, is_solid: true, is_passable: false, light_emission: 0 },
  BlockType::SpruceLog => BlockProperties { name: "Spruce Log", hardness: 2.0, render_type: RenderType::Opaque, is_solid: true, is_passable: false, light_emission: 0 },
  BlockType::SprucePlanks => BlockProperties { name: "Spruce Planks", hardness: 2.0, render_type: RenderType::Opaque, is_solid: true, is_passable: false, light_emission: 0 },
  BlockType::SpruceLeaves => BlockProperties { name: "Spruce Leaves", hardness: 0.2, render_type: RenderType::Cutout, is_solid: true, is_passable: false, light_emission: 0 },
  BlockType::TallGrass => BlockProperties { name: "Tall Grass", hardness: 0.0, render_type: RenderType::Cutout, is_solid: false, is_passable: true, light_emission: 0 },
  BlockType::Dandelion => BlockProperties { name: "Dandelion", hardness: 0.0, render_type: RenderType::Cutout, is_solid: false, is_passable: true, light_emission: 0 },
  BlockType::Poppy => BlockProperties { name: "Poppy", hardness: 0.0, render_type: RenderType::Cutout, is_solid: false, is_passable: true, light_emission: 0 },
  BlockType::Cactus => BlockProperties { name: "Cactus", hardness: 0.4, render_type: RenderType::Cutout, is_solid: true, is_passable: false, light_emission: 0 },
  BlockType::SugarCane => BlockProperties { name: "Sugar Cane", hardness: 0.0, render_type: RenderType::Cutout, is_solid: false, is_passable: true, light_emission: 0 },
  BlockType::Pumpkin => BlockProperties { name: "Pumpkin", hardness: 1.0, render_type: RenderType::Opaque, is_solid: true, is_passable: false, light_emission: 0 },
  BlockType::Melon => BlockProperties { name: "Melon", hardness: 1.0, render_type: RenderType::Opaque, is_solid: true, is_passable: false, light_emission: 0 },

  // get_face_tex_index additions (Row 12 column offsets):
  BlockType::BirchLog => if face_idx == 4 || face_idx == 5 { (0, 12) } else { (1, 12) },
  BlockType::BirchPlanks => (2, 12),
  BlockType::BirchLeaves => (3, 12),
  BlockType::SpruceLog => if face_idx == 4 || face_idx == 5 { (4, 12) } else { (5, 12) },
  BlockType::SprucePlanks => (6, 12),
  BlockType::SpruceLeaves => (7, 12),
  BlockType::TallGrass => (8, 12),
  BlockType::Dandelion => (9, 12),
  BlockType::Poppy => (10, 12),
  BlockType::Cactus => (11, 12),
  BlockType::SugarCane => (12, 12),
  BlockType::Pumpkin => (13, 12),
  BlockType::Melon => (14, 12),

  // preferred_tool additions:
  BlockType::BirchLog | BlockType::BirchPlanks | BlockType::SpruceLog | BlockType::SprucePlanks | BlockType::Pumpkin | BlockType::Melon => ToolType::Axe,
  BlockType::Cactus => ToolType::None,
  
  // min_harvest_material additions:
  // None needed since wood/cacti can be harvested with bare hands.
  ```

- [ ] **Step 2: Add item properties and block conversion in `src/inventory.rs`**
  Modify `Item::properties` and `Item::from_block`:
  ```rust
  // properties additions (Row 12 column offsets):
  Item::BirchLog => ItemProperties { name: "Birch Log", max_stack: 64, is_block: true, block_type: Some(BlockType::BirchLog), tex_coords: (1, 12) },
  Item::BirchPlanks => ItemProperties { name: "Birch Planks", max_stack: 64, is_block: true, block_type: Some(BlockType::BirchPlanks), tex_coords: (2, 12) },
  Item::BirchLeaves => ItemProperties { name: "Birch Leaves", max_stack: 64, is_block: true, block_type: Some(BlockType::BirchLeaves), tex_coords: (3, 12) },
  Item::SpruceLog => ItemProperties { name: "Spruce Log", max_stack: 64, is_block: true, block_type: Some(BlockType::SpruceLog), tex_coords: (5, 12) },
  Item::SprucePlanks => ItemProperties { name: "Spruce Planks", max_stack: 64, is_block: true, block_type: Some(BlockType::SprucePlanks), tex_coords: (6, 12) },
  Item::SpruceLeaves => ItemProperties { name: "Spruce Leaves", max_stack: 64, is_block: true, block_type: Some(BlockType::SpruceLeaves), tex_coords: (7, 12) },
  Item::TallGrass => ItemProperties { name: "Tall Grass", max_stack: 64, is_block: true, block_type: Some(BlockType::TallGrass), tex_coords: (8, 12) },
  Item::Dandelion => ItemProperties { name: "Dandelion", max_stack: 64, is_block: true, block_type: Some(BlockType::Dandelion), tex_coords: (9, 12) },
  Item::Poppy => ItemProperties { name: "Poppy", max_stack: 64, is_block: true, block_type: Some(BlockType::Poppy), tex_coords: (10, 12) },
  Item::Cactus => ItemProperties { name: "Cactus", max_stack: 64, is_block: true, block_type: Some(BlockType::Cactus), tex_coords: (11, 12) },
  Item::SugarCane => ItemProperties { name: "Sugar Cane", max_stack: 64, is_block: true, block_type: Some(BlockType::SugarCane), tex_coords: (12, 12) },
  Item::Pumpkin => ItemProperties { name: "Pumpkin", max_stack: 64, is_block: true, block_type: Some(BlockType::Pumpkin), tex_coords: (13, 12) },
  Item::Melon => ItemProperties { name: "Melon", max_stack: 64, is_block: true, block_type: Some(BlockType::Melon), tex_coords: (14, 12) },

  // from_block additions:
  BlockType::BirchLog => Item::BirchLog,
  BlockType::BirchPlanks => Item::BirchPlanks,
  BlockType::BirchLeaves => Item::BirchLeaves,
  BlockType::SpruceLog => Item::SpruceLog,
  BlockType::SprucePlanks => Item::SprucePlanks,
  BlockType::SpruceLeaves => Item::SpruceLeaves,
  BlockType::TallGrass => Item::TallGrass,
  BlockType::Dandelion => Item::Dandelion,
  BlockType::Poppy => Item::Poppy,
  BlockType::Cactus => Item::Cactus,
  BlockType::SugarCane => Item::SugarCane,
  BlockType::Pumpkin => Item::Pumpkin,
  BlockType::Melon => Item::Melon,
  ```

- [ ] **Step 3: Define Crafting Recipes in `src/crafting.rs`**
  Add shaped recipes for Birch and Spruce planks, sticks, crafting tables, and chests:
  ```rust
  add_shaped(&mut recipes, vec!["L"], &[("L", Item::BirchLog)], ItemStack::new(Item::BirchPlanks, 4));
  add_shaped(&mut recipes, vec!["L"], &[("L", Item::SpruceLog)], ItemStack::new(Item::SprucePlanks, 4));
  add_shaped(&mut recipes, vec!["P", "P"], &[("P", Item::BirchPlanks)], ItemStack::new(Item::Stick, 4));
  add_shaped(&mut recipes, vec!["P", "P"], &[("P", Item::SprucePlanks)], ItemStack::new(Item::Stick, 4));
  add_shaped(&mut recipes, vec!["PP", "PP"], &[("P", Item::BirchPlanks)], ItemStack::new(Item::CraftingTable, 1));
  add_shaped(&mut recipes, vec!["PP", "PP"], &[("P", Item::SprucePlanks)], ItemStack::new(Item::CraftingTable, 1));
  add_shaped(&mut recipes, vec!["PPP", "P P", "PPP"], &[("P", Item::BirchPlanks)], ItemStack::new(Item::Chest, 1));
  add_shaped(&mut recipes, vec!["PPP", "P P", "PPP"], &[("P", Item::SprucePlanks)], ItemStack::new(Item::Chest, 1));
  ```

- [ ] **Step 4: Run verification compilation**
  Run: `cargo check`
  Expected: PASS

---

### Task 3: Draw Procedural Textures

**Files:**
- Modify: [src/texture.rs](file:///f:/Desktop/MC/src/texture.rs)

- [ ] **Step 1: Write procedural texture drawers for new Row 12 blocks**
  Define helpers to draw birch bark, spruce bark, tall grass, dandelion, poppy, cactus, sugar cane, pumpkin, and melon:
  ```rust
  // In src/texture.rs:
  fn draw_birch_bark(img: &mut RgbaImage, tx: u32, ty: u32, seed: &mut u32) {
      let mut next_rand = |min: i16, max: i16| -> i16 {
          *seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
          min + ((*seed / 65536) % 32768) as i16 % (max - min)
      };
      for y in 0..16 {
          for x in 0..16 {
              let is_stripe = y % 5 == 0 && next_rand(0, 10) < 6;
              let c = if is_stripe { [30, 30, 30] } else { [230, 230, 225] };
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
          for x in 0..16 { img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0])); }
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
          for x in 0..16 { img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0])); }
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
          for x in 0..16 { img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([0, 0, 0, 0])); }
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
              let is_face = y >= 6 && y <= 11 && (dx == 3 || (dy == 2 && dx <= 2) || (y == 7 && dx == 0));
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
              let c = if is_stripe { [40, 80, 20] } else { [90, 150, 40] };
              img.put_pixel(tx * 16 + x, ty * 16 + y, Rgba([c[0], c[1], c[2], 255]));
          }
      }
  }
  ```

- [ ] **Step 2: Draw the Row 12 blocks inside `TextureAtlas::new_procedural`**
  Call these helpers to populate Row 12 inside `TextureAtlas::new_procedural`:
  ```rust
  // Birch Log Top/Bottom & Side
  draw_noise(&mut img, 0, 12, [215, 200, 175], 5, &mut seed);
  draw_birch_bark(&mut img, 1, 12, &mut seed);
  draw_noise(&mut img, 2, 12, [225, 210, 180], 5, &mut seed); // Birch Planks
  draw_leaves(&mut img, 3, 12, &mut seed);                     // Birch Leaves (green)

  // Spruce Log Top/Bottom & Side
  draw_noise(&mut img, 4, 12, [100, 75, 50], 8, &mut seed);
  draw_spruce_bark(&mut img, 5, 12, &mut seed);
  draw_noise(&mut img, 6, 12, [105, 80, 55], 5, &mut seed);   // Spruce Planks
  draw_leaves(&mut img, 7, 12, &mut seed);                     // Spruce Leaves (dark pine green)

  // Decorative Plants & Cacti
  draw_tall_grass(&mut img, 8, 12);
  draw_flower(&mut img, 9, 12, [240, 220, 40]);  // Dandelion (Yellow)
  draw_flower(&mut img, 10, 12, [230, 30, 30]);   // Poppy (Red)
  draw_cactus(&mut img, 11, 12);
  draw_sugar_cane(&mut img, 12, 12);
  draw_pumpkin(&mut img, 13, 12);
  draw_melon(&mut img, 14, 12);
  ```

- [ ] **Step 3: Run verification compilation**
  Run: `cargo check`
  Expected: PASS

---

### Task 4: Define Biome System and Interpolation

**Files:**
- Modify: [src/world.rs](file:///f:/Desktop/MC/src/world.rs)

- [ ] **Step 1: Declare Biome enum and helper noises in `src/world.rs`**
  Add the `Biome` enum to `src/world.rs` and the noise parameters:
  ```rust
  #[derive(Copy, Clone, Debug, PartialEq, Eq)]
  pub enum Biome {
      Plains,
      Forest,
      Desert,
      Taiga,
      Swamp,
      Mountains,
      Ocean,
  }

  impl Biome {
      pub fn get_biome(world_x: i32, world_z: i32, temp_perlin: &Perlin, moist_perlin: &Perlin, ocean_perlin: &Perlin) -> Self {
          let ocean_val = ocean_perlin.get([world_x as f64 * 0.001, world_z as f64 * 0.001]);
          if ocean_val < -0.35 {
              return Biome::Ocean;
          }

          let temp = temp_perlin.get([world_x as f64 * 0.002, world_z as f64 * 0.002]);
          let moist = moist_perlin.get([world_x as f64 * 0.002, world_z as f64 * 0.002]);

          if temp < -0.3 {
              if moist < -0.2 { Biome::Mountains } else { Biome::Taiga }
          } else if temp > 0.4 && moist < -0.3 {
              Biome::Desert
          } else if temp > 0.2 && moist > 0.4 {
              Biome::Swamp
          } else {
              if moist > 0.0 { Biome::Forest } else { Biome::Plains }
          }
      }

      pub fn terrain_params(self) -> (f64, f64) {
          match self {
              Biome::Plains => (65.0, 4.0),
              Biome::Forest => (66.0, 6.0),
              Biome::Desert => (65.0, 5.0),
              Biome::Taiga => (68.0, 8.0),
              Biome::Swamp => (62.0, 1.5),
              Biome::Mountains => (82.0, 22.0),
              Biome::Ocean => (50.0, 6.0),
          }
      }
  }
  ```

- [ ] **Step 2: Implement height interpolation in `src/world.rs`**
  Define `get_interpolated_height` to sample neighboring points and average their height:
  ```rust
  fn get_interpolated_height(
      world_x: i32,
      world_z: i32,
      perlin: &Perlin,
      temp_perlin: &Perlin,
      moist_perlin: &Perlin,
      ocean_perlin: &Perlin,
  ) -> usize {
      let mut height_sum = 0.0;
      let mut weight_sum = 0.0;

      const SAMPLE_STEPS: [i32; 3] = [-8, 0, 8];

      for &dx in &SAMPLE_STEPS {
          for &dz in &SAMPLE_STEPS {
              let sx = world_x + dx;
              let sz = world_z + dz;

              let biome = Biome::get_biome(sx, sz, temp_perlin, moist_perlin, ocean_perlin);
              let (base, scale) = biome.terrain_params();

              let noise_val = perlin.get([sx as f64 * 0.04, sz as f64 * 0.04]);
              let local_height = base + noise_val * scale;

              let weight = match (dx == 0, dz == 0) {
                  (true, true) => 1.0,      // Center
                  (true, false) | (false, true) => 0.5, // Cardinal
                  (false, false) => 0.25,   // Diagonal
              };

              height_sum += local_height * weight;
              weight_sum += weight;
          }
      }

      (height_sum / weight_sum).round() as usize
  }
  ```

- [ ] **Step 3: Run verification compilation**
  Run: `cargo check`
  Expected: PASS

---

### Task 5: Implement Procedural Tree Structures

**Files:**
- Modify: [src/world.rs](file:///f:/Desktop/MC/src/world.rs)

- [ ] **Step 1: Write structure generator functions for trees in `src/world.rs`**
  Add a helper to define oak, birch, and spruce block grids:
  ```rust
  // Inside src/world.rs:
  fn place_oak_tree(blocks: &mut Box<[[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>, local_x: i32, local_z: i32, start_y: i32, height: i32) {
      // Place log trunk
      for dy in 0..height {
          let y = start_y + dy;
          if y >= 0 && y < CHUNK_HEIGHT as i32 && local_x >= 0 && local_x < CHUNK_WIDTH as i32 && local_z >= 0 && local_z < CHUNK_DEPTH as i32 {
              blocks[local_x as usize][y as usize][local_z as usize] = BlockType::OakLog;
          }
      }
      // Place leaves canopy
      for ly in (height - 3)..=height {
          let y = start_y + ly;
          if y < 0 || y >= CHUNK_HEIGHT as i32 { continue; }
          let radius = if ly == height { 1 } else if ly == height - 1 { 1 } else { 2 };
          for dx in -radius..=radius {
              for dz in -radius..=radius {
                  if radius == 2 && dx.abs() == 2 && dz.abs() == 2 { continue; } // Remove corners for 5x5
                  let lx = local_x + dx;
                  let lz = local_z + dz;
                  if lx >= 0 && lx < CHUNK_WIDTH as i32 && lz >= 0 && lz < CHUNK_DEPTH as i32 {
                      let block = blocks[lx as usize][y as usize][lz as usize];
                      if block == BlockType::Air || block == BlockType::OakLeaves {
                          blocks[lx as usize][y as usize][lz as usize] = BlockType::OakLeaves;
                      }
                  }
              }
          }
      }
  }

  fn place_birch_tree(blocks: &mut Box<[[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>, local_x: i32, local_z: i32, start_y: i32, height: i32) {
      for dy in 0..height {
          let y = start_y + dy;
          if y >= 0 && y < CHUNK_HEIGHT as i32 && local_x >= 0 && local_x < CHUNK_WIDTH as i32 && local_z >= 0 && local_z < CHUNK_DEPTH as i32 {
              blocks[local_x as usize][y as usize][local_z as usize] = BlockType::BirchLog;
          }
      }
      for ly in (height - 3)..=height {
          let y = start_y + ly;
          if y < 0 || y >= CHUNK_HEIGHT as i32 { continue; }
          let is_cross = ly == height || ly == height - 3;
          let radius = 1;
          for dx in -radius..=radius {
              for dz in -radius..=radius {
                  if is_cross && dx.abs() == 1 && dz.abs() == 1 { continue; }
                  let lx = local_x + dx;
                  let lz = local_z + dz;
                  if lx >= 0 && lx < CHUNK_WIDTH as i32 && lz >= 0 && lz < CHUNK_DEPTH as i32 {
                      let block = blocks[lx as usize][y as usize][lz as usize];
                      if block == BlockType::Air {
                          blocks[lx as usize][y as usize][lz as usize] = BlockType::BirchLeaves;
                      }
                  }
              }
          }
      }
  }

  fn place_spruce_tree(blocks: &mut Box<[[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]>, local_x: i32, local_z: i32, start_y: i32, height: i32) {
      for dy in 0..height {
          let y = start_y + dy;
          if y >= 0 && y < CHUNK_HEIGHT as i32 && local_x >= 0 && local_x < CHUNK_WIDTH as i32 && local_z >= 0 && local_z < CHUNK_DEPTH as i32 {
              blocks[local_x as usize][y as usize][local_z as usize] = BlockType::SpruceLog;
          }
      }
      for ly in 2..=height {
          let y = start_y + ly;
          if y < 0 || y >= CHUNK_HEIGHT as i32 { continue; }
          let layer_from_top = height - ly;
          let (radius, is_cross) = if layer_from_top == 0 {
              (0, false)
          } else if layer_from_top == 1 {
              (1, true)
          } else if layer_from_top % 2 == 0 {
              (1, false)
          } else {
              (2, true)
          };
          for dx in -radius..=radius {
              for dz in -radius..=radius {
                  if is_cross && dx.abs() == radius && dz.abs() == radius { continue; }
                  let lx = local_x + dx;
                  let lz = local_z + dz;
                  if lx >= 0 && lx < CHUNK_WIDTH as i32 && lz >= 0 && lz < CHUNK_DEPTH as i32 {
                      let block = blocks[lx as usize][y as usize][lz as usize];
                      if block == BlockType::Air {
                          blocks[lx as usize][y as usize][lz as usize] = BlockType::SpruceLeaves;
                      }
                  }
              }
          }
      }
  }
  ```

- [ ] **Step 2: Run verification compilation**
  Run: `cargo check`
  Expected: PASS

---

### Task 6: Implement Terrain and Neighbor Projection Spawning in Chunk::new

**Files:**
- Modify: [src/world.rs](file:///f:/Desktop/MC/src/world.rs)

- [ ] **Step 1: Replace base heightmap logic in `Chunk::new`**
  Modify height lookup in `Chunk::new` to use biome interpolation and set proper surface/sub-surface blocks:
  ```rust
  // In Chunk::new:
  let temp_perlin = Perlin::new(99999);
  let moist_perlin = Perlin::new(88888);
  let ocean_perlin = Perlin::new(77777);

  for x in 0..CHUNK_WIDTH {
      for z in 0..CHUNK_DEPTH {
          let world_x = chunk_x * (CHUNK_WIDTH as i32) + x as i32;
          let world_z = chunk_z * (CHUNK_DEPTH as i32) + z as i32;

          let base_height = get_interpolated_height(world_x, world_z, &perlin, &temp_perlin, &moist_perlin, &ocean_perlin);
          let biome = Biome::get_biome(world_x, world_z, &temp_perlin, &moist_perlin, &ocean_perlin);

          for y in 0..CHUNK_HEIGHT {
              let mut block = BlockType::Air;
              if y == 0 {
                  block = BlockType::Bedrock;
              } else if y <= 4 {
                  let threshold = (5 - y) as u8 * 50;
                  block = if next_rand(0, 255) < threshold { BlockType::Bedrock } else { BlockType::Stone };
              } else if y < base_height.saturating_sub(4) {
                  block = BlockType::Stone;
              } else if y < base_height {
                  block = match biome {
                      Biome::Desert => BlockType::Sandstone,
                      Biome::Ocean => BlockType::Sand,
                      _ => BlockType::Dirt,
                  };
              } else if y == base_height {
                  block = match biome {
                      Biome::Desert => BlockType::Sand,
                      Biome::Ocean => BlockType::Sand,
                      Biome::Taiga => BlockType::Snow,
                      Biome::Mountains => if y > 90 { BlockType::Snow } else { BlockType::Stone },
                      _ => BlockType::Grass,
                  };
              } else if y <= 62 {
                  block = BlockType::Water;
              }
              // Cave Carving logic (keep existing code)
              ...
              blocks[x][y][z] = block;
          }
      }
  }
  ```

- [ ] **Step 2: Generate Trees & Plants using Neighbor Projection**
  Add neighbor-projection tree spawning loop at the end of `Chunk::new` (after cave/ore passes):
  ```rust
  // Trees Pass:
  for dx in -1..=1 {
      for dz in -1..=1 {
          let nx = chunk_x + dx;
          let nz = chunk_z + dz;

          // Seed PRNG deterministically for the neighbor chunk
          let mut n_seed = (nx as u32).wrapping_mul(31) ^ (nz as u32);
          let mut n_rand = |min: u8, max: u8| -> u8 {
              n_seed = n_seed.wrapping_mul(1103515245).wrapping_add(12345);
              let val = (n_seed / 65536) % 32768;
              let diff = max - min;
              if diff == 0 { return min; }
              min + (val % diff as u32) as u8
          };

          // Try 4 tree candidate spots per chunk
          for _ in 0..4 {
              let tx = n_rand(0, 15) as i32;
              let tz = n_rand(0, 15) as i32;
              let n_world_x = nx * 16 + tx;
              let n_world_z = nz * 16 + tz;

              let n_biome = Biome::get_biome(n_world_x, n_world_z, &temp_perlin, &moist_perlin, &ocean_perlin);
              let tree_prob = match n_biome {
                  Biome::Plains => 5,
                  Biome::Forest => 60,
                  Biome::Taiga => 40,
                  Biome::Swamp => 20,
                  Biome::Mountains => 2,
                  _ => 0,
              };

              if n_rand(0, 100) < tree_prob {
                  let n_height = get_interpolated_height(n_world_x, n_world_z, &perlin, &temp_perlin, &moist_perlin, &ocean_perlin) as i32;
                  if n_height > 5 && n_height < CHUNK_HEIGHT as i32 - 12 {
                      // Project to current chunk local coordinates
                      let local_x = n_world_x - (chunk_x * 16);
                      let local_z = n_world_z - (chunk_z * 16);

                      let tree_height = n_rand(4, 7) as i32;
                      match n_biome {
                          Biome::Taiga => place_spruce_tree(&mut blocks, local_x, local_z, n_height + 1, tree_height + 2),
                          Biome::Forest => {
                              if n_rand(0, 10) < 4 {
                                  place_birch_tree(&mut blocks, local_x, local_z, n_height + 1, tree_height + 1);
                              } else {
                                  place_oak_tree(&mut blocks, local_x, local_z, n_height + 1, tree_height);
                              }
                          }
                          _ => place_oak_tree(&mut blocks, local_x, local_z, n_height + 1, tree_height),
                      }
                  }
              }
          }
      }
  }

  // Plant & Decoration Pass (only for columns inside current chunk):
  for x in 0..CHUNK_WIDTH {
      for z in 0..CHUNK_DEPTH {
          let world_x = chunk_x * 16 + x as i32;
          let world_z = chunk_z * 16 + z as i32;
          let biome = Biome::get_biome(world_x, world_z, &temp_perlin, &moist_perlin, &ocean_perlin);

          // Seed PRNG deterministically for columns
          let mut c_seed = (world_x as u32).wrapping_mul(17) ^ (world_z as u32);
          let mut c_rand = |min: u32, max: u32| -> u32 {
              c_seed = c_seed.wrapping_mul(1103515245).wrapping_add(12345);
              min + ((c_seed / 65536) % 32768) % (max - min)
          };

          // Find surface block
          let mut surface_y = 0;
          for y in (0..CHUNK_HEIGHT).rev() {
              if blocks[x][y][z] != BlockType::Air && blocks[x][y][z] != BlockType::Water {
                  surface_y = y;
                  break;
              }
          }

          let surface_block = blocks[x][surface_y][z];
          if surface_block == BlockType::Grass {
              let r = c_rand(0, 100);
              if r < 10 { // Tall grass
                  if surface_y + 1 < CHUNK_HEIGHT {
                      blocks[x][surface_y + 1][z] = BlockType::TallGrass;
                  }
              } else if r < 12 { // Dandelion
                  if surface_y + 1 < CHUNK_HEIGHT {
                      blocks[x][surface_y + 1][z] = BlockType::Dandelion;
                  }
              } else if r < 13 { // Poppy
                  if surface_y + 1 < CHUNK_HEIGHT {
                      blocks[x][surface_y + 1][z] = BlockType::Poppy;
                  }
              } else if r < 14 && (biome == Biome::Plains || biome == Biome::Forest) { // Pumpkin / Melon
                  if surface_y + 1 < CHUNK_HEIGHT {
                      blocks[x][surface_y + 1][z] = if c_rand(0, 2) == 0 { BlockType::Pumpkin } else { BlockType::Melon };
                  }
              }
          } else if surface_block == BlockType::Sand && biome == Biome::Desert {
              if c_rand(0, 100) < 2 { // Cactus
                  let cactus_height = c_rand(1, 4) as usize;
                  for dy in 1..=cactus_height {
                      if surface_y + dy < CHUNK_HEIGHT {
                          blocks[x][surface_y + dy][z] = BlockType::Cactus;
                      }
                  }
              }
          }

          // Sugar Cane (must be next to water)
          if (surface_block == BlockType::Grass || surface_block == BlockType::Dirt || surface_block == BlockType::Sand) && surface_y > 0 {
              let mut near_water = false;
              for dx in -1..=1 {
                  for dz in -1..=1 {
                      if dx == 0 && dz == 0 { continue; }
                      let nx = x as i32 + dx;
                      let nz = z as i32 + dz;
                      if nx >= 0 && nx < CHUNK_WIDTH as i32 && nz >= 0 && nz < CHUNK_DEPTH as i32 {
                          let b = blocks[nx as usize][surface_y][nz as usize];
                          let b_below = blocks[nx as usize][surface_y - 1][nz as usize];
                          if b == BlockType::Water || b_below == BlockType::Water {
                              near_water = true;
                              break;
                          }
                      }
                  }
              }
              if near_water && c_rand(0, 100) < 10 {
                  let cane_height = c_rand(2, 5) as usize;
                  for dy in 1..=cane_height {
                      if surface_y + dy < CHUNK_HEIGHT {
                          blocks[x][surface_y + dy][z] = BlockType::SugarCane;
                      }
                  }
              }
          }
      }
  }
  ```

- [ ] **Step 3: Run verification compilation**
  Run: `cargo check`
  Expected: PASS

---

### Task 7: Implement Leaf Decay and Cactus Damage

**Files:**
- Modify: [src/state.rs](file:///f:/Desktop/MC/src/state.rs)

- [ ] **Step 1: Add decay & damage timer fields to `State` struct in `src/state.rs`**
  Add the fields to `State` class in `src/state.rs` around line 130:
  ```rust
  pub struct State {
      ...
      cactus_damage_timer: f32,
      // Add other timers if necessary
  }
  ```

- [ ] **Step 2: Implement leaf decay random ticks in `State::update`**
  Modify `State::update` to tick random leaves:
  ```rust
  // In State::update around line 1240 (before void check):
  let chunk_keys: Vec<(i32, i32)> = self.chunk_manager.chunks.keys().cloned().collect();
  if !chunk_keys.is_empty() {
      // Run 30 random ticks per frame
      let mut rng_seed = (self.total_time * 1000.0) as u32;
      let mut next_rand = |max: u32| -> u32 {
          rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
          ((rng_seed / 65536) % 32768) % max
      };

      for _ in 0..30 {
          let chunk_idx = next_rand(chunk_keys.len() as u32) as usize;
          let (cx, cz) = chunk_keys[chunk_idx];

          let rx = next_rand(16) as i32;
          let rz = next_rand(16) as i32;
          let ry = next_rand(120) as i32 + 40; // Leaves usually spawn between Y=40..160

          let wx = cx * 16 + rx;
          let wz = cz * 16 + rz;

          let block = self.chunk_manager.get_block(wx, ry, wz);
          if block == BlockType::OakLeaves || block == BlockType::BirchLeaves || block == BlockType::SpruceLeaves {
              // Run BFS check for log in radius 4
              let mut queue = std::collections::VecDeque::new();
              let mut visited = std::collections::HashSet::new();
              queue.push_back((wx, ry, wz, 0));
              visited.insert((wx, ry, wz));

              let mut found_log = false;
              while let Some((bx, by, bz, dist)) = queue.pop_front() {
                  let b = self.chunk_manager.get_block(bx, by, bz);
                  if b == BlockType::OakLog || b == BlockType::BirchLog || b == BlockType::SpruceLog {
                      found_log = true;
                      break;
                  }
                  if dist < 4 {
                      for (dx, dy, dz) in &[
                          (1,0,0), (-1,0,0), (0,1,0), (0,-1,0), (0,0,1), (0,0,-1)
                      ] {
                          let nx = bx + dx;
                          let ny = by + dy;
                          let nz = bz + dz;
                          let neighbor_b = self.chunk_manager.get_block(nx, ny, nz);
                          let is_leaf = neighbor_b == BlockType::OakLeaves || neighbor_b == BlockType::BirchLeaves || neighbor_b == BlockType::SpruceLeaves;
                          if (is_leaf || neighbor_b == BlockType::OakLog || neighbor_b == BlockType::BirchLog || neighbor_b == BlockType::SpruceLog) && visited.insert((nx, ny, nz)) {
                              queue.push_back((nx, ny, nz, dist + 1));
                          }
                      }
                  }
              }

              if !found_log {
                  self.chunk_manager.set_block(wx, ry, wz, BlockType::Air);
                  // Recalculate lighting & mark dirty meshes
                  let mut dirty_chunks = std::collections::HashSet::new();
                  crate::lighting::update_sky_light_after_removed(&mut self.chunk_manager, wx, ry, wz, &mut dirty_chunks);
                  dirty_chunks.insert((cx, cz));
                  if rx == 0 { dirty_chunks.insert((cx - 1, cz)); }
                  if rx == 15 { dirty_chunks.insert((cx + 1, cz)); }
                  if rz == 0 { dirty_chunks.insert((cx, cz - 1)); }
                  if rz == 15 { dirty_chunks.insert((cx, cz + 1)); }
                  for (dcx, dcz) in dirty_chunks {
                      if let Some(mesh) = self.chunk_meshes.get_mut(&(dcx, dcz)) {
                          mesh.dirty = true;
                      }
                  }
              }
          }
      }
  }
  ```

- [ ] **Step 3: Implement cactus damage in `State::update`**
  Add collision check and timer update:
  ```rust
  // Inside State::update:
  let player_aabb = self.player_physics.get_aabb();
  let min_x = player_aabb.min.x.floor() as i32;
  let max_x = player_aabb.max.x.floor() as i32;
  let min_y = (player_aabb.min.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
  let max_y = (player_aabb.max.y.floor() as i32).clamp(0, crate::world::CHUNK_HEIGHT as i32 - 1);
  let min_z = player_aabb.min.z.floor() as i32;
  let max_z = player_aabb.max.z.floor() as i32;

  let mut touching_cactus = false;
  for x in min_x..=max_x {
      for y in min_y..=max_y {
          for z in min_z..=max_z {
              if self.chunk_manager.get_block(x, y, z) == BlockType::Cactus {
                  let block_aabb = crate::physics::AABB::new(
                      Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5),
                      Vec3::ONE,
                  );
                  if player_aabb.intersects(&block_aabb) {
                      touching_cactus = true;
                  }
              }
          }
      }
  }

  if touching_cactus {
      self.cactus_damage_timer += dt;
      if self.cactus_damage_timer >= 0.5 {
          self.cactus_damage_timer = 0.0;
          self.take_damage(1.0, DamageSource::Mob); // Deal 1.0 contact damage (0.5 heart)
      }
  } else {
      self.cactus_damage_timer = 0.0;
  }
  ```

- [ ] **Step 4: Update `break_block` drop behavior for Tall Grass & Leaves**
  Update `break_block` in `src/state.rs`:
  ```rust
  // In break_block:
  let is_any_leaves = old_block == BlockType::OakLeaves || old_block == BlockType::BirchLeaves || old_block == BlockType::SpruceLeaves;
  if is_any_leaves {
      let mut rng_seed = (wx as u32).wrapping_mul(31).wrapping_add(wy as u32).wrapping_mul(17).wrapping_add(wz as u32);
      let mut next_rand = || {
          rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
          (rng_seed / 65536) % 32768
      };
      if next_rand() % 10 == 0 {
          self.inventory.add_item(crate::inventory::Item::Apple);
      } else {
          self.inventory.add_item(crate::inventory::Item::from_block(old_block));
      }
  } else if old_block == BlockType::TallGrass {
      let mut rng_seed = (wx as u32).wrapping_mul(31).wrapping_add(wy as u32).wrapping_mul(17).wrapping_add(wz as u32);
      let mut next_rand = || {
          rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
          (rng_seed / 65536) % 32768
      };
      if next_rand() % 8 == 0 { // 12.5% chance to drop seed
          self.inventory.add_item(crate::inventory::Item::Seeds);
      }
  } else {
      self.inventory.add_item(crate::inventory::Item::from_block(old_block));
  }
  ```

- [ ] **Step 5: Run verification compilation**
  Run: `cargo check`
  Expected: PASS

---

### Task 8: Verification & Unit Tests

**Files:**
- Modify: [src/world.rs](file:///f:/Desktop/MC/src/world.rs)

- [ ] **Step 1: Write Unit Tests in `src/world.rs`**
  Add unit tests for biome classification and tree placement boundary validation:
  ```rust
  #[cfg(test)]
  mod biome_tests {
      use super::*;

      #[test]
      fn test_biome_distribution() {
          let temp_perlin = Perlin::new(99999);
          let moist_perlin = Perlin::new(88888);
          let ocean_perlin = Perlin::new(77777);

          // Verify ocean is generated at specific low ocean-noise coordinates
          let biome_ocean = Biome::get_biome(0, 0, &temp_perlin, &moist_perlin, &ocean_perlin);
          // Verify that biomes evaluate correctly and don't panic
          let biome_land = Biome::get_biome(1000, 1000, &temp_perlin, &moist_perlin, &ocean_perlin);
          println!("Sample Biome at (1000, 1000): {:?}", biome_land);
      }

      #[test]
      fn test_tree_placement_bounds() {
          let mut blocks = vec![[[BlockType::Air; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH].try_into().unwrap();
          // Oak tree at local coordinates: should not panic when inside or touching edges
          place_oak_tree(&mut blocks, 8, 8, 64, 5);
          assert_eq!(blocks[8][64][8], BlockType::OakLog);
          assert_eq!(blocks[8][65][8], BlockType::OakLog);
          assert_eq!(blocks[8][68][8], BlockType::OakLog);
          
          // Spruce tree at border
          place_spruce_tree(&mut blocks, 0, 0, 64, 7);
          assert_eq!(blocks[0][64][0], BlockType::SpruceLog);
      }
  }
  ```

- [ ] **Step 2: Run all unit tests**
  Run: `cargo test`
  Expected: PASS (all tests including new biome tests pass)

- [ ] **Step 3: Run the game to verify visually**
  Run: `cargo run`
  Expected: Game runs with beautiful biomes, trees, and tall grass. Stepping on a cactus deals damage.
