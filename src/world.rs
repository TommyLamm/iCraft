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

                    for (_face_idx, (normal, corner_data)) in faces.iter().enumerate() {
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
