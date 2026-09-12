pub mod dungeon;
pub mod end_city;
pub mod helpers;
pub mod mineshaft;
pub mod nether_fortress;
pub mod stronghold;
pub mod village;

#[cfg(test)]
mod fingerprint_tests {
    use super::*;
    use crate::block_entity::BlockEntity;
    use crate::rng::{fnv1a_write, FNV_OFFSET};
    use crate::structure::types::{BlockPlacement, StructureStart};

    const SEED: u32 = 0xC0FF_EE;
    const OX: i32 = 100;
    const OY: i32 = 40;
    const OZ: i32 = -80;

    fn placement_fingerprint(start: &StructureStart) -> u64 {
        let mut items: Vec<&BlockPlacement> = start
            .pieces
            .iter()
            .flat_map(|piece| piece.blocks.iter())
            .collect();
        items.sort_by_key(|p| {
            (
                p.world_x,
                p.world_y,
                p.world_z,
                p.block_type as u16,
                p.block_state,
            )
        });
        let mut hash = FNV_OFFSET;
        for p in items {
            fnv1a_write(&mut hash, &p.world_x.to_le_bytes());
            fnv1a_write(&mut hash, &p.world_y.to_le_bytes());
            fnv1a_write(&mut hash, &p.world_z.to_le_bytes());
            fnv1a_write(&mut hash, &(p.block_type as u16).to_le_bytes());
            fnv1a_write(&mut hash, &[p.block_state]);
            match &p.block_entity {
                None => fnv1a_write(&mut hash, &[0]),
                Some(BlockEntity::Chest(c)) => {
                    fnv1a_write(&mut hash, &[1]);
                    fnv1a_write(&mut hash, &c.loot_seed.unwrap_or(0).to_le_bytes());
                    if let Some(table) = &c.loot_table {
                        fnv1a_write(&mut hash, table.as_bytes());
                    }
                    if let Some(name) = &c.custom_name {
                        fnv1a_write(&mut hash, name.as_bytes());
                    }
                }
                Some(BlockEntity::Spawner(s)) => {
                    fnv1a_write(&mut hash, &[2]);
                    fnv1a_write(&mut hash, &(s.entity_type as u16).to_le_bytes());
                    fnv1a_write(&mut hash, &s.spawn_delay.to_le_bytes());
                }
                Some(_) => fnv1a_write(&mut hash, &[255]),
            }
        }
        fnv1a_write(&mut hash, &start.origin_x.to_le_bytes());
        fnv1a_write(&mut hash, &start.origin_y.to_le_bytes());
        fnv1a_write(&mut hash, &start.origin_z.to_le_bytes());
        fnv1a_write(&mut hash, &start.bounding_box.min_x.to_le_bytes());
        fnv1a_write(&mut hash, &start.bounding_box.min_y.to_le_bytes());
        fnv1a_write(&mut hash, &start.bounding_box.min_z.to_le_bytes());
        fnv1a_write(&mut hash, &start.bounding_box.max_x.to_le_bytes());
        fnv1a_write(&mut hash, &start.bounding_box.max_y.to_le_bytes());
        fnv1a_write(&mut hash, &start.bounding_box.max_z.to_le_bytes());
        hash
    }

    /// Locked before helper refactor; sorted placement multiset must not change.
    #[test]
    fn structure_gen_placement_fingerprints_locked() {
        let cases = [
            (
                "dungeon",
                placement_fingerprint(&dungeon::generate_dungeon(OX, OY, OZ, SEED)),
            ),
            (
                "village",
                placement_fingerprint(&village::generate_village(OX, OY, OZ, SEED)),
            ),
            (
                "mineshaft",
                placement_fingerprint(&mineshaft::generate_mineshaft(OX, OY, OZ, SEED)),
            ),
            (
                "stronghold",
                placement_fingerprint(&stronghold::generate_stronghold(OX, OY, OZ, SEED)),
            ),
            (
                "fortress",
                placement_fingerprint(&nether_fortress::generate_nether_fortress(
                    OX, OY, OZ, SEED,
                )),
            ),
            (
                "end_city",
                placement_fingerprint(&end_city::generate_end_city(OX, OY, OZ, SEED)),
            ),
        ];
        const EXPECTED: [(&str, u64); 6] = [
            ("dungeon", 0x7542_a818_60b2_2202),
            ("village", 0x37b2_e526_c1cf_0138),
            ("mineshaft", 0x5105_3bc6_dded_5fc4),
            ("stronghold", 0xc4b3_cd46_74e8_438a),
            ("fortress", 0xbf3b_c639_4247_34a6),
            ("end_city", 0x4b7b_c868_e4f3_bd19),
        ];
        for ((name, hash), (exp_name, expected)) in cases.iter().zip(EXPECTED.iter()) {
            assert_eq!(name, exp_name);
            assert_eq!(*hash, *expected, "{name} placement fingerprint changed");
        }
    }
}
