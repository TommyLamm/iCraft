use super::*;
use crate::inventory::{ToolMaterial, ToolType};
use crate::redstone::Direction;
#[test]
fn block_type_wire_roundtrip_covers_all_variants() {
    // Live discriminants round-trip; reserved holes alias to their base.
    for raw in 0..=BlockType::Observer as u32 {
        let block = BlockType::from_wire(raw).expect("valid discriminant");
        if BlockType::migrate_saved(raw as u8, 0).0 as u32 == raw {
            assert_eq!(block.to_wire(), raw);
        } else {
            assert_eq!(block.to_wire(), block as u32);
            assert_ne!(block as u32, raw, "hole {raw} must alias away");
        }
    }
}

#[test]
fn block_type_from_wire_rejects_unknown_values() {
    assert_eq!(BlockType::from_wire(9999), None);
    assert_eq!(BlockType::from_wire(255), None);
}

#[test]
fn test_plant_support_requirements() {
    assert!(BlockType::Dandelion.can_stay_on(BlockType::Grass));
    assert!(BlockType::Dandelion.can_stay_on(BlockType::Dirt));
    assert!(!BlockType::Dandelion.can_stay_on(BlockType::Air));
    assert!(!BlockType::Dandelion.can_stay_on(BlockType::Stone));
    assert!(!BlockType::Dandelion.can_stay_on(BlockType::OakPlanks));

    assert!(BlockType::Poppy.can_stay_on(BlockType::Grass));
    assert!(!BlockType::Poppy.can_stay_on(BlockType::Sand));

    assert!(BlockType::TallGrass.can_stay_on(BlockType::Grass));
    assert!(!BlockType::TallGrass.can_stay_on(BlockType::Stone));

    assert!(BlockType::SugarCane.can_stay_on(BlockType::Sand));
    assert!(BlockType::SugarCane.can_stay_on(BlockType::SugarCane));
    assert!(!BlockType::SugarCane.can_stay_on(BlockType::Air));

    assert!(BlockType::Cactus.can_stay_on(BlockType::Sand));
    assert!(BlockType::Cactus.can_stay_on(BlockType::Cactus));
    assert!(!BlockType::Cactus.can_stay_on(BlockType::Dirt));
}

#[test]
fn contextual_plant_support_enforces_water_and_lateral_clearance() {
    let position = (8, 100, 8);
    let mut blocks = std::collections::HashMap::new();
    blocks.insert((8, 99, 8), BlockType::Sand);
    let lookup = |x, y, z| Some(*blocks.get(&(x, y, z)).unwrap_or(&BlockType::Air));

    assert_eq!(
        BlockType::SugarCane.support_status_at(position, lookup),
        BlockSupportStatus::Unsupported
    );

    blocks.insert((9, 99, 8), BlockType::Water);
    assert_eq!(
        BlockType::SugarCane.support_status_at(position, |x, y, z| {
            Some(*blocks.get(&(x, y, z)).unwrap_or(&BlockType::Air))
        }),
        BlockSupportStatus::Supported
    );

    blocks.insert((8, 99, 8), BlockType::SugarCane);
    blocks.remove(&(9, 99, 8));
    assert_eq!(
        BlockType::SugarCane.support_status_at(position, |x, y, z| {
            Some(*blocks.get(&(x, y, z)).unwrap_or(&BlockType::Air))
        }),
        BlockSupportStatus::Supported,
        "upper cane inherits support from the cane below"
    );

    blocks.insert((8, 99, 8), BlockType::Sand);
    assert_eq!(
        BlockType::Cactus.support_status_at(position, |x, y, z| {
            Some(*blocks.get(&(x, y, z)).unwrap_or(&BlockType::Air))
        }),
        BlockSupportStatus::Supported
    );
    blocks.insert((9, 100, 8), BlockType::Stone);
    assert_eq!(
        BlockType::Cactus.support_status_at(position, |x, y, z| {
            Some(*blocks.get(&(x, y, z)).unwrap_or(&BlockType::Air))
        }),
        BlockSupportStatus::Unsupported
    );
    blocks.insert((9, 100, 8), BlockType::Lava);
    assert_eq!(
        BlockType::Cactus.support_status_at(position, |x, y, z| {
            Some(*blocks.get(&(x, y, z)).unwrap_or(&BlockType::Air))
        }),
        BlockSupportStatus::Unsupported,
        "lava is a forbidden lateral cactus neighbor despite being non-solid"
    );
}

#[test]
fn contextual_plant_support_reports_unknown_for_missing_neighbor_chunks() {
    let position = (15, 100, 8);
    let lookup = |x, y, z| {
        if x >= 16 {
            None
        } else if (x, y, z) == (15, 99, 8) {
            Some(BlockType::Sand)
        } else {
            Some(BlockType::Air)
        }
    };

    assert_eq!(
        BlockType::SugarCane.support_status_at(position, lookup),
        BlockSupportStatus::Unknown
    );
    assert_eq!(
        BlockType::Cactus.support_status_at(position, lookup),
        BlockSupportStatus::Unknown
    );
}

#[test]
fn weather_blocks_have_expected_collision_and_light() {
    assert!(BlockType::SnowLayer.properties().is_passable);
    assert!(!BlockType::SnowLayer.properties().is_solid);
    assert_eq!(BlockType::Fire.properties().light_emission, 15);
    assert!(BlockType::Fire.properties().is_passable);
    assert_eq!(BlockType::from_u8(74), BlockType::Fire);
    assert_eq!(BlockType::from_u8(75), BlockType::SnowLayer);
    for id in 0..=BlockType::Observer as u8 {
        let block = BlockType::from_u8(id);
        let (migrated, _) = BlockType::migrate_saved(id, 0);
        assert_eq!(block, migrated);
        if block as u8 == id {
            assert_eq!(block as u8, id);
        }
    }
    assert_eq!(BlockType::from_u8(255), BlockType::Air);
}

#[test]
fn legacy_powered_open_holes_migrate_into_state_bits() {
    let cases = [
        (50u8, BlockType::RedstoneTorch),
        (52, BlockType::Repeater),
        (54, BlockType::Comparator),
        (56, BlockType::StoneButton),
        (58, BlockType::Lever),
        (60, BlockType::PressurePlate),
        (62, BlockType::Piston),
        (64, BlockType::StickyPiston),
        (66, BlockType::RedstoneLamp),
        (68, BlockType::OakDoor),
        (70, BlockType::OakTrapdoor),
        (82, BlockType::EndPortalFrame),
        (90, BlockType::Furnace),
    ];
    for (raw, base) in cases {
        let (block, state) = BlockType::migrate_saved(raw, 0);
        assert_eq!(block, base);
        assert_ne!(state & BLOCK_STATE_OPEN_BIT, 0);
        let decoded = BlockState::decode(state);
        assert!(decoded.is_open);
    }
    // Live torch id keeps clear bit (= lit).
    let (torch, state) = BlockType::migrate_saved(49, 0);
    assert_eq!(torch, BlockType::RedstoneTorch);
    assert_eq!(state & BLOCK_STATE_OPEN_BIT, 0);
    assert_eq!(
        BlockType::RedstoneTorch.light_emission_for(BlockState::default()),
        7
    );
    let mut off = BlockState::default();
    off.is_open = true;
    assert_eq!(BlockType::RedstoneTorch.light_emission_for(off), 0);
}

#[test]
fn test_block_harvest_properties() {
    assert_eq!(BlockType::Obsidian.preferred_tool(), ToolType::Pickaxe);
    assert_eq!(
        BlockType::Obsidian.min_harvest_material(),
        Some(ToolMaterial::Diamond)
    );
    assert_eq!(BlockType::OakPlanks.preferred_tool(), ToolType::Axe);
    assert_eq!(BlockType::OakPlanks.min_harvest_material(), None);
}

#[test]
fn canonical_block_table_covers_every_variant() {
    assert_eq!(BLOCK_TABLE.len(), BLOCK_TYPE_COUNT);
    assert_eq!(BLOCK_TYPE_COUNT, BlockType::Observer as usize + 1);
    for id in 0..BLOCK_TYPE_COUNT as u8 {
        let raw: BlockType = unsafe { std::mem::transmute(id) };
        if raw.is_reserved_hole() {
            // Hole rows stay for discriminant density; gameplay indexes the base.
            let base = raw.canonicalize();
            assert_eq!(base.def() as *const _, &BLOCK_TABLE[base as usize] as *const _);
            continue;
        }
        let block = BlockType::from_u8(id);
        assert_eq!(block as usize, id as usize);
        let def = block.def();
        assert!(
            std::ptr::eq(def, &BLOCK_TABLE[id as usize]),
            "variant {block:?} must index its own row"
        );
    }
}

#[test]
fn block_static_property_snapshot_is_byte_identical() {
    // Locked dump of every static field for every live discriminant.
    let mut lines = Vec::with_capacity(BLOCK_TYPE_COUNT);
    for id in 0..BLOCK_TYPE_COUNT as u8 {
        let raw: BlockType = unsafe { std::mem::transmute(id) };
        if raw.is_reserved_hole() {
            continue;
        }
        let b = BlockType::from_u8(id);
        let d = b.def();
        let p = &d.properties;
        let faces = (0..6)
            .map(|f| {
                let (c, r) = d.face_tex[f];
                format!("{c},{r}")
            })
            .collect::<Vec<_>>()
            .join(";");
        lines.push(format!(
            "{id}|{b:?}|{name}|{hardness:.3}|{render:?}|{solid}|{pass}|{light}|{faces}|{sound:?}|{tool:?}|{harvest:?}|{cross}",
            name = p.name,
            hardness = p.hardness,
            render = p.render_type,
            solid = p.is_solid as u8,
            pass = p.is_passable as u8,
            light = p.light_emission,
            sound = d.sound,
            tool = d.preferred_tool,
            harvest = d.min_harvest,
            cross = d.is_cross_model as u8,
        ));
    }
    let snapshot = lines.join("\n");
    let expected = include_str!("../block_property_snapshot.txt")
        .replace("\r\n", "\n")
        .trim_end()
        .to_string();
    assert_eq!(
        snapshot, expected,
        "BlockDef table drifted from the locked snapshot"
    );
    for id in 0..BLOCK_TYPE_COUNT as u8 {
        let raw: BlockType = unsafe { std::mem::transmute(id) };
        if raw.is_reserved_hole() {
            continue;
        }
        let b = BlockType::from_u8(id);
        let d = b.def();
        assert_eq!(b.properties().name, d.properties.name);
        assert_eq!(b.properties().hardness, d.properties.hardness);
        assert_eq!(b.sound_material(), d.sound);
        assert_eq!(b.preferred_tool(), d.preferred_tool);
        assert_eq!(b.min_harvest_material(), d.min_harvest);
        assert_eq!(b.is_cross_model(), d.is_cross_model);
        for face in 0..6 {
            assert_eq!(b.get_face_tex_index(face), d.face_tex[face]);
        }
    }
}

#[test]
fn block_state_encoding_roundtrip() {
    assert_eq!(BlockState::default().encode(), 0);

    let directions = [
        Direction::North,
        Direction::South,
        Direction::West,
        Direction::East,
    ];
    for facing in directions {
        for is_top in [false, true] {
            for is_right_hinge in [false, true] {
                for is_open in [false, true] {
                    for chest_type in [ChestType::Single, ChestType::Left, ChestType::Right] {
                        let state = BlockState {
                            facing,
                            is_top,
                            is_right_hinge,
                            is_open,
                            chest_type,
                        };
                        let encoded = state.encode();
                        let decoded = BlockState::decode(encoded);
                        assert_eq!(decoded, state);
                    }
                }
            }
        }
    }
    // Verify reserved bit (bit 7) is ignored
    for chest_type in [ChestType::Single, ChestType::Left, ChestType::Right] {
        let state = BlockState {
            facing: Direction::North,
            is_top: false,
            is_right_hinge: false,
            is_open: false,
            chest_type,
        };
        let encoded = state.encode() | 0b1000_0000;
        let decoded = BlockState::decode(encoded);
        assert_eq!(decoded, state);
    }
}
