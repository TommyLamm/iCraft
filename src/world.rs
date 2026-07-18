use crate::state::Vertex;
use noise::{NoiseFn, Perlin};


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
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub blocks: [[[BlockType; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH],
}

impl Chunk {
    pub fn new(chunk_x: i32, chunk_z: i32) -> Self {
        let mut blocks = [[[BlockType::Air; CHUNK_DEPTH]; CHUNK_HEIGHT]; CHUNK_WIDTH];
        let perlin = Perlin::new(12345); // Seed: 12345

        for x in 0..CHUNK_WIDTH {
            for z in 0..CHUNK_DEPTH {
                // Calculate surface height using Perlin noise
                let world_x = chunk_x * (CHUNK_WIDTH as i32) + x as i32;
                let world_z = chunk_z * (CHUNK_DEPTH as i32) + z as i32;
                let noise_val = perlin.get([world_x as f64 * 0.08, world_z as f64 * 0.08]);
                // Map noise value (-1.0 to 1.0) to height (e.g. 55 to 75)
                let height = (64.0 + noise_val * 10.0) as usize;

                for y in 0..CHUNK_HEIGHT {
                    if y < height - 4 {
                        blocks[x][y][z] = BlockType::Stone;
                    } else if y < height {
                        blocks[x][y][z] = BlockType::Dirt;
                      } else if y == height {
                        blocks[x][y][z] = BlockType::Grass;
                    } else {
                        blocks[x][y][z] = BlockType::Air;
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

    pub fn get_block(&self, x: i32, y: i32, z: i32) -> BlockType {
        if x < 0 || x >= CHUNK_WIDTH as i32 || y < 0 || y >= CHUNK_HEIGHT as i32 || z < 0 || z >= CHUNK_DEPTH as i32 {
            return BlockType::Air; // 超出範圍視為空氣
        }
        self.blocks[x as usize][y as usize][z as usize]
    }

    // 生成用於渲染的頂點和索引
    pub fn generate_mesh<F>(&self, get_block_at: F) -> (Vec<Vertex>, Vec<u32>)
    where
        F: Fn(i32, i32, i32) -> BlockType,
    {
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

                    let world_x = self.chunk_x * CHUNK_WIDTH as i32 + x as i32;
                    let world_y = y as i32;
                    let world_z = self.chunk_z * CHUNK_DEPTH as i32 + z as i32;

                    for (face_idx, (normal, corner_data)) in faces.iter().enumerate() {
                        let nx = world_x + normal[0] as i32;
                        let ny = world_y + normal[1] as i32;
                        let nz = world_z + normal[2] as i32;

                        // Face Culling: 檢查相鄰區塊是否透明
                        let neighbor = get_block_at(nx, ny, nz);
                        if neighbor == BlockType::Air {
                            let start_idx = vertices.len() as u32;

                            // Texture atlas mapping:
                            // Col 0: Grass Top, Col 1: Grass Side, Col 2: Dirt, Col 3: Stone
                            let tex_idx = match block {
                                BlockType::Stone => 3,
                                BlockType::Dirt => 2,
                                BlockType::Grass => {
                                    if face_idx == 4 { // Up face
                                        0
                                    } else if face_idx == 5 { // Down face
                                        2
                                    } else { // Side faces
                                        1
                                    }
                                }
                                BlockType::Air => 0,
                            };

                            for (offset, uv) in corner_data.iter() {
                                let u = (uv[0] + tex_idx as f32) * 0.25;
                                let v = uv[1] * 0.25;
                                vertices.push(Vertex {
                                    position: [
                                        world_x as f32 + offset[0],
                                        world_y as f32 + offset[1],
                                        world_z as f32 + offset[2],
                                    ],
                                    tex_coords: [u, v],
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
