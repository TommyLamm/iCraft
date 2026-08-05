use super::types::StructureId;
use crate::dimension::Dimension;

pub fn hash_structure(seed: u32, salt: u32, x: i32, z: i32) -> u64 {
    let mut state = (seed as u64) ^ ((salt as u64) << 32);
    state ^= (x as u64).wrapping_mul(0x9E37_79B9);
    state ^= (z as u64).wrapping_mul(0x85EB_CA6B);
    state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    state ^ (state >> 32)
}

pub struct StructureGridConfig {
    pub spacing: i32,
    pub separation: i32,
    pub salt: u32,
}

pub fn get_structure_grid(id: StructureId) -> StructureGridConfig {
    match id {
        StructureId::Dungeon => StructureGridConfig {
            spacing: 16,
            separation: 4,
            salt: 0x4455_4E47,
        },
        StructureId::Mineshaft => StructureGridConfig {
            spacing: 20,
            separation: 5,
            salt: 0x4D49_4E45,
        },
        StructureId::Village => StructureGridConfig {
            spacing: 24,
            separation: 8,
            salt: 0x5649_4C4C,
        },
        StructureId::Stronghold => StructureGridConfig {
            spacing: 32,
            separation: 10,
            salt: 0x5354_524F,
        },
        StructureId::NetherFortress => StructureGridConfig {
            spacing: 20,
            separation: 6,
            salt: 0x464F_5254,
        },
        StructureId::EndCity => StructureGridConfig {
            spacing: 16,
            separation: 4,
            salt: 0x454E_4443,
        },
    }
}

pub fn get_structure_candidate_in_region(
    id: StructureId,
    dimension: Dimension,
    world_seed: u32,
    region_x: i32,
    region_z: i32,
) -> Option<(i32, i32)> {
    let config = get_structure_grid(id);

    match (id, dimension) {
        (StructureId::Dungeon, Dimension::Overworld)
        | (StructureId::Mineshaft, Dimension::Overworld)
        | (StructureId::Village, Dimension::Overworld)
        | (StructureId::Stronghold, Dimension::Overworld) => {}
        (StructureId::NetherFortress, Dimension::Nether) => {}
        (StructureId::EndCity, Dimension::End) => {}
        _ => return None,
    }

    let h = hash_structure(world_seed, config.salt, region_x, region_z);
    let max_off = (config.spacing - config.separation).max(1);
    let off_x = (h % max_off as u64) as i32;
    let off_z = ((h >> 16) % max_off as u64) as i32;

    let chunk_x = region_x * config.spacing + off_x;
    let chunk_z = region_z * config.spacing + off_z;

    // EndCity only spawns outer islands (distance from origin >= 50 chunks = 800 blocks)
    if id == StructureId::EndCity {
        let dist = ((chunk_x as f64).powi(2) + (chunk_z as f64).powi(2)).sqrt();
        if dist < 45.0 {
            return None;
        }
    }

    // Stronghold distance check: not inside chunk_x == 0, chunk_z == 0
    if id == StructureId::Stronghold {
        let dist = ((chunk_x as f64).powi(2) + (chunk_z as f64).powi(2)).sqrt();
        if dist < 12.0 {
            return None;
        }
    }

    Some((chunk_x, chunk_z))
}
