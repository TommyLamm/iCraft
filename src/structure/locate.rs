use super::placement::get_structure_candidate_in_region;
use super::types::StructureId;
use crate::dimension::Dimension;

pub fn locate_structure(
    id: StructureId,
    current_pos: (i32, i32, i32),
    world_seed: u32,
    dimension: Dimension,
) -> Option<(i32, i32, i32)> {
    let current_chunk_x = current_pos.0.div_euclid(16);
    let current_chunk_z = current_pos.2.div_euclid(16);

    let mut closest: Option<((i32, i32, i32), f64)> = None;
    let radius_regions = 20;

    for r in 0..=radius_regions {
        for rx in -r..=r {
            for rz in -r..=r {
                if (rx as i32).abs() != r && (rz as i32).abs() != r {
                    continue;
                }
                let reg_x = current_chunk_x.div_euclid(24) + rx;
                let reg_z = current_chunk_z.div_euclid(24) + rz;

                if let Some((chunk_x, chunk_z)) =
                    get_structure_candidate_in_region(id, dimension, world_seed, reg_x, reg_z)
                {
                    let origin_x = chunk_x * 16 + 2;
                    let origin_z = chunk_z * 16 + 2;
                    let origin_y = match id {
                        StructureId::Dungeon => 25,
                        StructureId::Mineshaft => 25,
                        StructureId::Village => 64,
                        StructureId::Stronghold => 22,
                        StructureId::NetherFortress => 55,
                        StructureId::EndCity => 64,
                    };

                    let dx = (origin_x - current_pos.0) as f64;
                    let dz = (origin_z - current_pos.2) as f64;
                    let dist_sq = dx * dx + dz * dz;

                    if closest.map_or(true, |(_, best_dist)| dist_sq < best_dist) {
                        closest = Some(((origin_x, origin_y, origin_z), dist_sq));
                    }
                }
            }
        }
        if closest.is_some() {
            break;
        }
    }

    closest.map(|(pos, _)| pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locate_stronghold_deterministic() {
        let seed = 1234567;
        let pos1 = locate_structure(
            StructureId::Stronghold,
            (0, 64, 0),
            seed,
            Dimension::Overworld,
        );
        let pos2 = locate_structure(
            StructureId::Stronghold,
            (0, 64, 0),
            seed,
            Dimension::Overworld,
        );

        assert!(pos1.is_some());
        assert_eq!(pos1, pos2);

        let (x, _y, z) = pos1.unwrap();
        // Ensure not fixed (2,2) chunk (which was x=34, z=34)
        assert!(x != 34 || z != 34);
    }
}
