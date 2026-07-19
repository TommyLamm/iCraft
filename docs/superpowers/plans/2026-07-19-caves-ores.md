# Caves & Ores Generation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a 3D noise-based cave carving system and a two-pass deterministic ore vein clustering generator in Rust.

**Architecture:** 
1. **Pass 1 (Terrain & Caves)**: Generate basic bedrock and terrain. Query 3D Perlin noise for caves and carve `Air` out of `Stone` layers, using safety guards to prevent surface leakage and bedrock damage.
2. **Pass 2 (Ore Veins)**: Run a deterministic chunk-based random walk algorithm. Distribute Coal, Iron, Gold, Redstone, and Diamond ore clusters by replacing `Stone` blocks.

**Tech Stack:** Rust, `noise` crate (using `Perlin`).

## Global Constraints

- Cave 3D Perlin seed: 54321. Scaling: horizontal 0.05, vertical 0.08. Threshold: 0.08.
- Cavern 3D Perlin seed: 65432. Scaling: 0.01. Threshold: 0.20 when noise > 0.6.
- Cave safety limit: Y > 4 and Y < min(base_height - 6, 62).
- Ore distribution configurations:
  - Coal: Y 0~128, size 17, frequency 15
  - Iron: Y 0~64, size 9, frequency 10
  - Gold: Y 0~32, size 9, frequency 3
  - Redstone: Y 0~16, size 8, frequency 4
  - Diamond: Y 0~16, size 8, frequency 1
- Ores can only replace `BlockType::Stone` blocks above bedrock (Y > 4).
- Deterministic random numbers generated using the chunk's existing `rng_seed` coordinate system.

---

### Task 1: Cave Generation (3D Noise Carving)

**Files:**
- Modify: [src/world.rs](file:///f:/Desktop/MC/src/world.rs)
- Test: [src/world.rs](file:///f:/Desktop/MC/src/world.rs)

**Interfaces:**
- Consumes: Existing chunk generation loop in `Chunk::new`.
- Produces: Carved cave air blocks underground.

- [ ] **Step 1: Write the failing test**

  Add the following test in `tests` module at the end of [src/world.rs](file:///f:/Desktop/MC/src/world.rs):

  ```rust
  #[test]
  fn test_cave_generation() {
      let chunk = Chunk::new(0, 0);
      let mut air_underground = 0;
      let mut stone_underground = 0;
      for x in 0..CHUNK_WIDTH {
          for z in 0..CHUNK_DEPTH {
              for y in 5..50 {
                  let block = chunk.blocks[x][y][z];
                  if block == BlockType::Air {
                      air_underground += 1;
                  } else if block == BlockType::Stone {
                      stone_underground += 1;
                  }
              }
          }
      }
      assert!(air_underground > 0, "Caves should carve some air underground");
      assert!(stone_underground > 0, "Caves should leave some stone underground");
  }
  ```

- [ ] **Step 2: Run test to verify it fails**

  Run: `cargo test tests::test_cave_generation`
  Expected: FAIL (assertion fails or compilation error if blocks are fully filled with stone/ores and no underground air is generated).

- [ ] **Step 3: Write minimal implementation**

  Modify `Chunk::new` in [src/world.rs](file:///f:/Desktop/MC/src/world.rs):
  - Initialize the cave and cavern Perlin noise instances.
  - In the vertical terrain loop, if the block coordinates fall under cave safety conditions, check cave noise.
  - Temporarily generate pure `BlockType::Stone` below the dirt layer (removing old inline ore generation).

  Replace `Chunk::new` terrain loop (around lines 393-477) with:

  ```rust
      pub fn new(chunk_x: i32, chunk_z: i32) -> Self {
          let mut blocks: Box<[[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]> =
              vec![[[BlockType::Air; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH]
                  .try_into().unwrap();
          let perlin = Perlin::new(12345); // Seed: 12345
          let caves_perlin = Perlin::new(54321);
          let caverns_perlin = Perlin::new(65432);

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
                  let world_x = chunk_x * (CHUNK_WIDTH as i32) + x as i32;
                  let world_z = chunk_z * (CHUNK_DEPTH as i32) + z as i32;
                  let noise_val = perlin.get([world_x as f64 * 0.04, world_z as f64 * 0.04]);
                  let base_height = (64.0 + noise_val * 12.0) as usize;
                  
                  let is_beach = base_height <= 63;
                  for y in 0..CHUNK_HEIGHT {
                      let world_y = y as i32;
                      
                      // Bedrock Y=0-4
                      if y <= 4 {
                          if y == 0 {
                              blocks[x][y][z] = BlockType::Bedrock;
                          } else {
                              let threshold = (5 - y) as u8 * 50;
                              if next_rand(0, 255) < threshold {
                                  blocks[x][y][z] = BlockType::Bedrock;
                              } else {
                                  blocks[x][y][z] = BlockType::Stone;
                              }
                          }
                      }
                      // Underground Layer
                      else if y < base_height - 4 {
                          // Check cave carving
                          let mut is_cave = false;
                          if y < base_height.saturating_sub(6) && y < 62 {
                              let cave_val = caves_perlin.get([world_x as f64 * 0.05, world_y as f64 * 0.08, world_z as f64 * 0.05]);
                              let cavern_val = caverns_perlin.get([world_x as f64 * 0.01, world_y as f64 * 0.01, world_z as f64 * 0.01]);
                              let threshold = if cavern_val > 0.6 { 0.20 } else { 0.08 };
                              
                              if cave_val.abs() < threshold {
                                  is_cave = true;
                              }
                          }

                          if is_cave {
                              blocks[x][y][z] = BlockType::Air;
                          } else {
                              blocks[x][y][z] = BlockType::Stone; // Pure Stone (ores added in Pass 2)
                          }
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
  ```

- [ ] **Step 4: Run test to verify it passes**

  Run: `cargo test tests::test_cave_generation`
  Expected: PASS

- [ ] **Step 5: Commit**

  ```bash
  git add src/world.rs
  git commit -m "feat: implement 3D noise cave generation"
  ```

---

### Task 2: Ore Vein Clustering (Second-Pass Generation)

**Files:**
- Modify: [src/world.rs](file:///f:/Desktop/MC/src/world.rs)
- Test: [src/world.rs](file:///f:/Desktop/MC/src/world.rs)

**Interfaces:**
- Consumes: Base chunk with cave air blocks carved from Task 1.
- Produces: Clusters of ore blocks embedded in stone layers.

- [ ] **Step 1: Write the failing test**

  Add the following test in `tests` module at the end of [src/world.rs](file:///f:/Desktop/MC/src/world.rs):

  ```rust
  #[test]
  fn test_ore_clustering() {
      let chunk = Chunk::new(0, 0);
      let mut clustered = false;
      let mut coal_count = 0;
      for x in 0..CHUNK_WIDTH {
          for z in 0..CHUNK_DEPTH {
              for y in 0..CHUNK_HEIGHT {
                  if chunk.blocks[x][y][z] == BlockType::CoalOre {
                      coal_count += 1;
                      let neighbors = [
                          (x as i32 + 1, y as i32, z as i32),
                          (x as i32 - 1, y as i32, z as i32),
                          (x as i32, y as i32 + 1, z as i32),
                          (x as i32, y as i32 - 1, z as i32),
                          (x as i32, y as i32, z as i32 + 1),
                          (x as i32, y as i32, z as i32 - 1),
                      ];
                      for &(nx, ny, nz) in &neighbors {
                          if nx >= 0 && nx < CHUNK_WIDTH as i32
                              && nz >= 0 && nz < CHUNK_DEPTH as i32
                              && ny >= 0 && ny < CHUNK_HEIGHT as i32 {
                              if chunk.blocks[nx as usize][ny as usize][nz as usize] == BlockType::CoalOre {
                                  clustered = true;
                                  break;
                              }
                          }
                      }
                  }
              }
          }
      }
      assert!(coal_count > 0, "Coal should be generated in the chunk");
      assert!(clustered, "Coal ores should generate in clusters (veins)");
  }
  ```

- [ ] **Step 2: Run test to verify it fails**

  Run: `cargo test tests::test_ore_clustering`
  Expected: FAIL (no Coal Ore generated at all, since inline generator was removed in Task 1).

- [ ] **Step 3: Write minimal implementation**

  Modify `Chunk::new` in [src/world.rs](file:///f:/Desktop/MC/src/world.rs) to append the second pass after heightmap initialization:

  ```rust
          // --- Pass 2: Ore Vein Distribution ---
          struct OreConfig {
              block_type: BlockType,
              min_y: i32,
              max_y: i32,
              vein_size: usize,
              frequency: usize,
          }

          let ore_configs = [
              OreConfig {
                  block_type: BlockType::CoalOre,
                  min_y: 0,
                  max_y: 128,
                  vein_size: 17,
                  frequency: 15,
              },
              OreConfig {
                  block_type: BlockType::IronOre,
                  min_y: 0,
                  max_y: 64,
                  vein_size: 9,
                  frequency: 10,
              },
              OreConfig {
                  block_type: BlockType::GoldOre,
                  min_y: 0,
                  max_y: 32,
                  vein_size: 9,
                  frequency: 3,
              },
              OreConfig {
                  block_type: BlockType::RedstoneOre,
                  min_y: 0,
                  max_y: 16,
                  vein_size: 8,
                  frequency: 4,
              },
              OreConfig {
                  block_type: BlockType::DiamondOre,
                  min_y: 0,
                  max_y: 16,
                  vein_size: 8,
                  frequency: 1,
              },
          ];

          let mut next_rand_range = |min: i32, max: i32| -> i32 {
              if min >= max { return min; }
              let diff = (max - min) as u32;
              rng_seed = rng_seed.wrapping_mul(1103515245).wrapping_add(12345);
              let val = (rng_seed / 65536) % 32768;
              min + (val % diff) as i32
          };

          for config in &ore_configs {
              for _ in 0..config.frequency {
                  let start_x = next_rand_range(0, CHUNK_WIDTH as i32) as usize;
                  let start_z = next_rand_range(0, CHUNK_DEPTH as i32) as usize;
                  let start_y = next_rand_range(config.min_y, config.max_y + 1) as usize;

                  if start_y >= CHUNK_HEIGHT { continue; }

                  if blocks[start_x][start_y][start_z] == BlockType::Stone {
                      let mut queue = Vec::new();
                      queue.push((start_x, start_y, start_z));
                      blocks[start_x][start_y][start_z] = config.block_type;
                      
                      let mut placed = 1;
                      let mut head = 0;

                      while head < queue.len() && placed < config.vein_size {
                          let (cx, cy, cz) = queue[head];
                          head += 1;

                          // Randomly select one of the 6 neighbor directions
                          let dir = next_rand_range(0, 6);
                          let neighbors = [
                              (cx as i32 + 1, cy as i32, cz as i32),
                              (cx as i32 - 1, cy as i32, cz as i32),
                              (cx as i32, cy as i32 + 1, cz as i32),
                              (cx as i32, cy as i32 - 1, cz as i32),
                              (cx as i32, cy as i32, cz as i32 + 1),
                              (cx as i32, cy as i32, cz as i32 - 1),
                          ];

                          let (nx, ny, nz) = neighbors[dir as usize];
                          if nx >= 0 && nx < CHUNK_WIDTH as i32
                              && nz >= 0 && nz < CHUNK_DEPTH as i32
                              && ny > 4 && ny < CHUNK_HEIGHT as i32 {
                              
                              let ux = nx as usize;
                              let uy = ny as usize;
                              let uz = nz as usize;
                              
                              if blocks[ux][uy][uz] == BlockType::Stone {
                                  blocks[ux][uy][uz] = config.block_type;
                                  queue.push((ux, uy, uz));
                                  placed += 1;
                              }
                          }
                      }
                  }
              }
          }
  ```

  *(Put this Pass 2 block right before `let mut sky_light: Box<...>` calculation at line 479).*

- [ ] **Step 4: Run all tests to verify they pass**

  Run: `cargo test`
  Expected: PASS (all 18 tests, including `test_cave_generation` and `test_ore_clustering`, pass).

- [ ] **Step 5: Commit**

  ```bash
  git add src/world.rs
  git commit -m "feat: implement two-pass deterministic ore vein clustering"
  ```
