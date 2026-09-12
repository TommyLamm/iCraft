use super::placement::{
    get_structure_candidate_in_region, origin_y_for, END_CITY_BASE_Y, END_CITY_X, END_CITY_Z,
};
use super::types::StructureId;
use crate::dimension::Dimension;
use crate::world::{chunk_origin, chunk_xz};

pub fn locate_structure(
    id: StructureId,
    current_pos: (i32, i32, i32),
    world_seed: u32,
    dimension: Dimension,
) -> Option<(i32, i32, i32)> {
    let (current_chunk_x, current_chunk_z) = chunk_xz(current_pos.0, current_pos.2);

    let mut closest: Option<((i32, i32, i32), f64)> = None;
    let radius_regions = 20;

    let consider = |pos: (i32, i32, i32), closest: &mut Option<((i32, i32, i32), f64)>| {
        let dx = (pos.0 - current_pos.0) as f64;
        let dz = (pos.2 - current_pos.2) as f64;
        let dist_sq = dx * dx + dz * dz;
        if closest.map_or(true, |(_, best_dist)| dist_sq < best_dist) {
            *closest = Some((pos, dist_sq));
        }
    };

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
                    let origin_x = chunk_origin(chunk_x) + 2;
                    let origin_z = chunk_origin(chunk_z) + 2;
                    let origin_y = origin_y_for(id, world_seed, chunk_x, chunk_z);
                    consider((origin_x, origin_y, origin_z), &mut closest);
                }
            }
        }
        if closest.is_some() {
            break;
        }
    }

    // Always compete the pinned familiar city so locate matches manager placement.
    if id == StructureId::EndCity && dimension == Dimension::End {
        consider(
            (END_CITY_X, END_CITY_BASE_Y, END_CITY_Z),
            &mut closest,
        );
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

    #[test]
    fn locate_dungeon_y_matches_placement() {
        let seed = 1234567;
        let pos = locate_structure(StructureId::Dungeon, (0, 64, 0), seed, Dimension::Overworld)
            .expect("dungeon candidate");
        let (chunk_x, chunk_z) = chunk_xz(pos.0, pos.2);
        assert_eq!(
            pos.1,
            origin_y_for(StructureId::Dungeon, seed, chunk_x, chunk_z)
        );

        let manager = crate::structure::StructureManager::new();
        let region_x = chunk_x.div_euclid(24);
        let region_z = chunk_z.div_euclid(24);
        let starts = manager.get_or_generate_starts(Dimension::Overworld, seed, region_x, region_z);
        let dungeon = starts
            .iter()
            .find(|s| s.id == StructureId::Dungeon)
            .expect("placed dungeon");
        assert_eq!(dungeon.origin_x, pos.0);
        assert_eq!(dungeon.origin_y, pos.1);
        assert_eq!(dungeon.origin_z, pos.2);
    }

    #[test]
    fn village_origin_y_uses_surface_height() {
        let seed = 424242;
        let chunk_x = 2;
        let chunk_z = -3;
        let y = origin_y_for(StructureId::Village, seed, chunk_x, chunk_z);
        let ctx = crate::worldgen::WorldGenContext::new(seed);
        let surface = ctx.surface_height_at(chunk_origin(chunk_x) + 2, chunk_origin(chunk_z) + 2);
        let height = Dimension::Overworld.height();
        assert_eq!(
            y,
            surface.clamp(height.min_y(), height.max_y_exclusive() - 1)
        );
    }

    #[test]
    fn locate_end_city_matches_pinned_placement() {
        let seed = 7;
        let pos = locate_structure(
            StructureId::EndCity,
            (1000, 64, 0),
            seed,
            Dimension::End,
        )
        .expect("End City");
        assert_eq!(pos, (END_CITY_X, END_CITY_BASE_Y, END_CITY_Z));

        let manager = crate::structure::StructureManager::new();
        let starts = manager.get_or_generate_starts(Dimension::End, seed, 2, 0);
        let city = starts
            .iter()
            .find(|s| {
                s.id == StructureId::EndCity
                    && s.origin_x == END_CITY_X
                    && s.origin_z == END_CITY_Z
            })
            .expect("pinned city in manager");
        assert_eq!(city.origin_x, pos.0);
        assert_eq!(city.origin_y, pos.1);
        assert_eq!(city.origin_z, pos.2);
    }
}
