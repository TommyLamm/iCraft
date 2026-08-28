use super::*;
use crate::dimension::Dimension;
use crate::inventory::{CreativeDragOrigin, GameMode, Inventory, Item, ItemStack};
use crate::world::{BlockType, Chunk};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::format::{
    deserialize_chunk_save_data, destination_voxel_count, LegacyInventoryData, LegacyLevelData,
    LegacyRedstoneComponentMetadata, LegacyU8YRedstoneComponentMetadata, PreviousInventoryData,
    PreviousPlayerData,
};
use super::region::{ATOMIC_WRITE_FAILPOINT, COMPRESS_FAILPOINT};

#[test]
fn test_serialization_roundtrips() {
    let level = LevelData {
        seed: 12345,
        time: 6000,
        spawn_x: 8,
        spawn_y: 80,
        spawn_z: 8,
        spawn_dimension: Dimension::Overworld,
        spawn_yaw: 0.0,
        version: 2,
        ..LevelData::default()
    };
    let encoded_level = bincode::serialize(&level).unwrap();
    let decoded_level: LevelData = bincode::deserialize(&encoded_level).unwrap();
    assert_eq!(level.seed, decoded_level.seed);
    assert_eq!(level.time, decoded_level.time);

    let player = PlayerData {
        position: [1.0, 2.0, 3.0],
        velocity: [0.1, 0.2, 0.3],
        yaw: 1.5,
        pitch: 0.5,
        health: 20.0,
        hunger: 20.0,
        saturation: 5.0,
        exhaustion: 0.0,
        oxygen: 300.0,
        experience: 120,
        experience_level: 12,
        game_mode: GameMode::Survival,
        is_dead: false,
        spawn_point: None,
        spawn_dimension: None,
        inventory: InventoryData {
            hotbar: vec![Some(ItemStackData {
                item: Item::Stone,
                count: 64,
                durability: 0,
                enchantments: Default::default(),
                potion: None,
                custom_name: Default::default(),
                can_break: 0,
                can_place_on: 0,
            })],
            main: vec![None],
            armor: vec![None],
            offhand: None,
            selected: 0,
            dragged: None,
            creative_drag_origin: None,
        },
        advancements: Default::default(),
        unlocked_recipes: Default::default(),
        bad_omen_level: 0,
        hero_of_the_village_timer: 0.0,
    };
    let encoded_player = bincode::serialize(&player).unwrap();
    let decoded_player: PlayerData = bincode::deserialize(&encoded_player).unwrap();
    assert_eq!(player.position, decoded_player.position);
    assert_eq!(player.yaw, decoded_player.yaw);
    assert_eq!(player.health, decoded_player.health);
    assert_eq!(
        player.inventory.hotbar[0].as_ref().unwrap().item,
        Item::Stone
    );

    let mut original_blocks = vec![0u8; 16 * 256 * 16];
    original_blocks[0] = 1;
    original_blocks[100] = 3;
    let compressed_blocks = compress_bytes(&original_blocks).unwrap();
    let decompressed_blocks = decompress_bytes(&compressed_blocks).unwrap();
    assert_eq!(original_blocks, decompressed_blocks);
    println!(
        "Compressed size: {} bytes, Original: {} bytes",
        compressed_blocks.len(),
        original_blocks.len()
    );
}

#[test]
fn enchanted_potion_stack_metadata_roundtrips() {
    let mut stack = ItemStack::new(Item::Potion, 1);
    stack
        .enchantments
        .add_or_upgrade(crate::enchantment::Enchantment::Unbreaking(3));
    stack.potion = Some(crate::brewing::PotionData {
        kind: crate::brewing::PotionKind::Speed,
        level: 2,
        duration_seconds: 90,
        splash: true,
    });
    stack.custom_name.set("Swift Brew");
    let encoded = bincode::serialize(&ItemStackData::from(&stack)).unwrap();
    let decoded: ItemStackData = bincode::deserialize(&encoded).unwrap();
    let decoded = decoded.to_item_stack();
    assert_eq!(decoded.enchantments, stack.enchantments);
    assert_eq!(decoded.potion, stack.potion);
    assert_eq!(decoded.custom_name.as_str(), "Swift Brew");
}

#[test]
fn real_cursors_roundtrip_and_catalog_cursors_do_not_persist() {
    let mut stack = ItemStack::new(Item::Dirt, 17);
    stack.custom_name.set("Travel Stack");
    stack
        .enchantments
        .add_or_upgrade(crate::enchantment::Enchantment::Unbreaking(2));

    for origin in [None, Some(CreativeDragOrigin::Inventory)] {
        let mut inventory = Inventory::new();
        inventory.dragged = Some(stack);
        inventory.creative_drag_origin = origin;

        let restored = InventoryData::from(&inventory).to_inventory();
        assert_eq!(restored.dragged, Some(stack));
        assert_eq!(restored.creative_drag_origin, origin);
    }

    let mut catalog_inventory = Inventory::new();
    catalog_inventory.dragged = Some(stack);
    catalog_inventory.creative_drag_origin = Some(CreativeDragOrigin::Catalog);
    let saved = InventoryData::from(&catalog_inventory);
    assert!(saved.dragged.is_none());
    assert!(saved.creative_drag_origin.is_none());
    let restored = saved.to_inventory();
    assert!(restored.dragged.is_none());
    assert!(restored.creative_drag_origin.is_none());
}

#[test]
fn versioned_player_codec_preserves_real_cursor_metadata_and_provenance() {
    let mut inventory = Inventory::new();
    let mut cursor = ItemStack::new(Item::Potion, 1);
    cursor.custom_name.set("Exit Safe");
    cursor.potion = Some(crate::brewing::PotionData {
        kind: crate::brewing::PotionKind::Strength,
        level: 2,
        duration_seconds: 90,
        splash: false,
    });
    inventory.dragged = Some(cursor);
    inventory.creative_drag_origin = Some(CreativeDragOrigin::Inventory);
    let player = PlayerData {
        position: [1.0, 2.0, 3.0],
        velocity: [0.0; 3],
        yaw: 0.5,
        pitch: -0.25,
        health: 20.0,
        hunger: 18.0,
        saturation: 4.0,
        exhaustion: 1.0,
        oxygen: 300.0,
        experience: 10,
        experience_level: 2,
        game_mode: GameMode::Creative,
        is_dead: false,
        inventory: InventoryData::from(&inventory),
        advancements: Default::default(),
        spawn_point: None,
        spawn_dimension: None,
        unlocked_recipes: Default::default(),
        bad_omen_level: 0,
        hero_of_the_village_timer: 0.0,
    };

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let world_dir = std::env::temp_dir().join(format!(
        "icraft_cursor_player_save_{}_{}",
        std::process::id(),
        unique
    ));
    let manager = SaveManager::new(&world_dir);
    manager
        .save_player_and_level(
            &LevelData {
                seed: 99,
                time: 1234,
                spawn_x: 8,
                spawn_y: 80,
                spawn_z: 8,
                spawn_dimension: Dimension::Overworld,
                spawn_yaw: 0.0,
                version: 2,
                ..LevelData::default()
            },
            &player,
        )
        .unwrap();
    let encoded = fs::read(world_dir.join("player.dat")).unwrap();
    assert!(encoded.starts_with(PLAYER_SAVE_MAGIC));

    let (_, decoded) = manager.load_player_and_level().unwrap();
    let restored = decoded.inventory.to_inventory();
    assert_eq!(restored.dragged, Some(cursor));
    assert_eq!(
        restored.creative_drag_origin,
        Some(CreativeDragOrigin::Inventory)
    );
    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn previous_bincode_player_fixture_migrates_without_a_cursor() {
    let previous = PreviousPlayerData {
        position: [4.0, 5.0, 6.0],
        velocity: [0.1, 0.2, 0.3],
        yaw: 1.0,
        pitch: 0.2,
        health: 17.0,
        hunger: 12.0,
        saturation: 3.0,
        exhaustion: 2.0,
        oxygen: 250.0,
        experience: 42,
        experience_level: 5,
        game_mode: GameMode::Survival,
        inventory: PreviousInventoryData {
            hotbar: vec![Some(ItemStackData::from(&ItemStack::new(Item::Stone, 32)))],
            main: vec![None],
            armor: vec![None],
            selected: 0,
        },
        advancements: Default::default(),
    };
    let legacy_fixture = bincode::serialize(&previous).unwrap();
    assert!(!legacy_fixture.starts_with(PLAYER_SAVE_MAGIC));

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let world_dir = std::env::temp_dir().join(format!(
        "icraft_previous_player_save_{}_{}",
        std::process::id(),
        unique
    ));
    fs::create_dir_all(&world_dir).unwrap();
    fs::write(
        world_dir.join("level.dat"),
        bincode::serialize(&LevelData {
            seed: 7,
            time: 9000,
            spawn_x: 8,
            spawn_y: 80,
            spawn_z: 8,
            spawn_dimension: Dimension::Overworld,
            spawn_yaw: 0.0,
            version: 2,
            ..LevelData::default()
        })
        .unwrap(),
    )
    .unwrap();
    fs::write(world_dir.join("player.dat"), legacy_fixture).unwrap();

    let manager = SaveManager::new(&world_dir);
    let (_, migrated) = manager.load_player_and_level().unwrap();
    assert_eq!(migrated.experience, 42);
    assert_eq!(
        migrated.inventory.hotbar[0].as_ref().unwrap().item,
        Item::Stone
    );
    assert!(migrated.inventory.dragged.is_none());
    assert!(migrated.inventory.creative_drag_origin.is_none());
    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn saved_chunk_restores_player_placed_blocks() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let world_dir = std::env::temp_dir().join(format!(
        "icraft_chunk_save_{}_{}",
        std::process::id(),
        unique
    ));

    let mut original = Chunk::new(0, 0);
    original.set_block_local(8, 100, 8, BlockType::Brick);

    let mut manager = SaveManager::new(&world_dir);
    manager
        .save_chunk(0, 0, ChunkSaveData::from_chunk(&original).unwrap())
        .unwrap();

    let saved = manager.load_chunk(0, 0).expect("saved chunk should load");
    let mut restored = Chunk::new(0, 0);
    saved.restore_to_chunk(&mut restored).unwrap();

    assert_eq!(restored.get_block_local(8, 100, 8), BlockType::Brick);

    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn automation_block_entities_roundtrip() {
    let mut chunk = Chunk::new(0, 0);
    chunk.set_block_local(2, 64, 2, BlockType::Hopper);
    let mut hopper = crate::block_entity::HopperBlockEntity::new();
    hopper.facing = crate::redstone::Direction::West;
    hopper.transfer_cooldown = 6;
    hopper.is_powered = true;
    hopper.revision = 9;
    hopper.slots[0] = Some(crate::inventory::ItemStack::new(
        crate::inventory::Item::Diamond,
        3,
    ));
    chunk
        .insert_block_entity(
            2,
            64,
            2,
            crate::block_entity::BlockEntity::Hopper(hopper.clone()),
        )
        .unwrap();

    chunk.set_block_local(3, 64, 2, BlockType::Observer);
    let observer = crate::block_entity::ObserverBlockEntity {
        facing: crate::redstone::Direction::South,
        pending_pulse: 1,
        baseline_initialized: true,
        observed_block: BlockType::Stone,
        observed_state: 4,
        observed_entity_revision: 12,
        observed_entity_present: true,
        revision: 5,
    };
    chunk
        .insert_block_entity(
            3,
            64,
            2,
            crate::block_entity::BlockEntity::Observer(observer.clone()),
        )
        .unwrap();

    chunk.set_block_local(4, 64, 2, BlockType::Dispenser);
    let mut dispenser = crate::block_entity::DispenserBlockEntity::new();
    dispenser.slots[4] = Some(crate::inventory::ItemStack::new(
        crate::inventory::Item::Arrow,
        2,
    ));
    dispenser.revision = 7;
    chunk
        .insert_block_entity(
            4,
            64,
            2,
            crate::block_entity::BlockEntity::Dispenser(dispenser.clone()),
        )
        .unwrap();

    chunk.set_block_local(5, 64, 2, BlockType::Dropper);
    let mut dropper = crate::block_entity::DropperBlockEntity::new();
    dropper.slots[1] = Some(crate::inventory::ItemStack::new(
        crate::inventory::Item::Diamond,
        3,
    ));
    dropper.revision = 8;
    chunk
        .insert_block_entity(
            5,
            64,
            2,
            crate::block_entity::BlockEntity::Dropper(dropper.clone()),
        )
        .unwrap();

    chunk.set_block_local(6, 64, 2, BlockType::Furnace);
    let mut furnace = crate::block_entity::FurnaceBlockEntity::new();
    furnace.slots[0] = Some(crate::inventory::ItemStack::new(
        crate::inventory::Item::IronOre,
        1,
    ));
    furnace.slots[1] = Some(crate::inventory::ItemStack::new(
        crate::inventory::Item::Coal,
        1,
    ));
    furnace.burn_time = 42;
    furnace.cook_progress = 17;
    furnace.revision = 11;
    chunk
        .insert_block_entity(
            6,
            64,
            2,
            crate::block_entity::BlockEntity::Furnace(furnace.clone()),
        )
        .unwrap();

    let saved = ChunkSaveData::from_chunk(&chunk).unwrap();
    assert_eq!(saved.data_version, CHUNK_SAVE_DATA_VERSION);
    assert!(!saved.block_entities.is_empty());
    let mut restored = Chunk::new(0, 0);
    saved.restore_to_chunk(&mut restored).unwrap();
    assert_eq!(
        restored.get_block_entity(2, 64, 2),
        Some(&crate::block_entity::BlockEntity::Hopper(hopper.clone()))
    );
    assert_eq!(
        restored.get_block_entity(3, 64, 2),
        Some(&crate::block_entity::BlockEntity::Observer(
            observer.clone()
        ))
    );
    assert_eq!(
        restored.get_block_entity(4, 64, 2),
        Some(&crate::block_entity::BlockEntity::Dispenser(
            dispenser.clone()
        ))
    );
    assert_eq!(
        restored.get_block_entity(5, 64, 2),
        Some(&crate::block_entity::BlockEntity::Dropper(dropper.clone()))
    );
    assert_eq!(
        restored.get_block_entity(6, 64, 2),
        Some(&crate::block_entity::BlockEntity::Furnace(furnace.clone()))
    );
}

#[test]
fn dimension_chunk_namespaces_are_independent() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let world_dir = std::env::temp_dir().join(format!(
        "icraft_dimension_save_{}_{}",
        std::process::id(),
        unique
    ));
    let mut manager = SaveManager::new(&world_dir);
    let cases = [
        (crate::dimension::Dimension::Overworld, BlockType::Brick),
        (crate::dimension::Dimension::Nether, BlockType::Netherrack),
        (crate::dimension::Dimension::End, BlockType::EndStone),
    ];

    for (dimension, marker) in cases {
        let mut chunk = Chunk::new(4, -3);
        chunk.set_block_local(7, 90, 11, marker);
        manager
            .save_chunk_in(dimension, 4, -3, ChunkSaveData::from_chunk(&chunk).unwrap())
            .unwrap();
    }

    drop(manager);
    let mut manager = SaveManager::new(&world_dir);
    for (dimension, marker) in cases {
        let saved = manager
            .load_chunk_in(dimension, 4, -3)
            .expect("dimension chunk should load");
        let mut restored = Chunk::new(4, -3);
        saved.restore_to_chunk(&mut restored).unwrap();
        assert_eq!(restored.get_block_local(7, 90, 11), marker);
    }

    assert!(world_dir.join("regions/r.0.-1.bin").exists());
    assert!(world_dir
        .join("dimensions/nether/regions/r.0.-1.bin")
        .exists());
    assert!(world_dir.join("dimensions/end/regions/r.0.-1.bin").exists());
    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn current_dimension_sidecar_roundtrips_and_defaults_to_overworld() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let world_dir = std::env::temp_dir().join(format!(
        "icraft_dimension_state_{}_{}",
        std::process::id(),
        unique
    ));
    let manager = SaveManager::new(&world_dir);
    assert_eq!(
        manager.load_current_dimension(),
        crate::dimension::Dimension::Overworld
    );
    manager
        .save_current_dimension(crate::dimension::Dimension::End)
        .unwrap();
    assert_eq!(
        manager.load_current_dimension(),
        crate::dimension::Dimension::End
    );
    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn redstone_metadata_sidecar_roundtrips_through_save_and_load() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let world_dir = std::env::temp_dir().join(format!(
        "icraft_redstone_sidecar_{}_{}",
        std::process::id(),
        unique
    ));

    let chunk = Chunk::new(-2, 5);
    let metadata = vec![crate::redstone::RedstoneComponentMetadata {
        local_x: 3,
        local_y: 100,
        local_z: 7,
        facing: crate::redstone::Direction::East,
        repeater_delay: 4,
        comparator_mode: crate::redstone::ComparatorMode::Subtract,
        note: 12,
        last_powered: true,
    }];
    let mut manager = SaveManager::new(&world_dir);
    manager
        .save_chunk_in(
            crate::dimension::Dimension::Overworld,
            -2,
            5,
            ChunkSaveData::from_chunk_with_redstone(&chunk, &metadata).unwrap(),
        )
        .unwrap();

    let saved = manager
        .load_chunk_in(crate::dimension::Dimension::Overworld, -2, 5)
        .expect("redstone sidecar chunk should load");
    assert_eq!(saved.redstone_metadata(), metadata);

    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn legacy_redstone_sidecar_preserves_fields_and_defaults_latch() {
    let legacy = vec![LegacyRedstoneComponentMetadata {
        local_x: 6,
        local_y: 91,
        local_z: 4,
        facing: crate::redstone::Direction::South,
        repeater_delay: 3,
        comparator_mode: crate::redstone::ComparatorMode::Subtract,
        note: 9,
    }];
    let mut saved = ChunkSaveData::from_chunk(&Chunk::new(0, 0)).unwrap();
    saved.redstone_metadata = compress_bytes(&bincode::serialize(&legacy).unwrap()).unwrap();

    assert_eq!(
        saved.redstone_metadata(),
        vec![crate::redstone::RedstoneComponentMetadata {
            local_x: 6,
            local_y: 91,
            local_z: 4,
            facing: crate::redstone::Direction::South,
            repeater_delay: 3,
            comparator_mode: crate::redstone::ComparatorMode::Subtract,
            note: 9,
            last_powered: false,
        }]
    );
}

#[test]
fn legacy_u8_redstone_y_236_stays_236_not_negative_twenty() {
    let legacy = vec![LegacyU8YRedstoneComponentMetadata {
        local_x: 2,
        local_y: 236,
        local_z: 3,
        facing: crate::redstone::Direction::North,
        repeater_delay: 1,
        comparator_mode: crate::redstone::ComparatorMode::Compare,
        note: 0,
        last_powered: true,
    }];
    let mut saved = ChunkSaveData::from_chunk(&Chunk::new(0, 0)).unwrap();
    saved.redstone_metadata = compress_bytes(&bincode::serialize(&legacy).unwrap()).unwrap();

    let decoded = saved.redstone_metadata();
    assert_eq!(decoded.len(), 1);
    assert_eq!(
        decoded[0].local_y, 236,
        "old u8 236 must stay world Y 236, not wrap to -20"
    );
    assert!(decoded[0].last_powered);
}

#[test]
fn signed_redstone_y_roundtrips_negative_world_y() {
    let metadata = vec![crate::redstone::RedstoneComponentMetadata {
        local_x: 4,
        local_y: -20,
        local_z: 5,
        facing: crate::redstone::Direction::East,
        repeater_delay: 2,
        comparator_mode: crate::redstone::ComparatorMode::Compare,
        note: 0,
        last_powered: false,
    }];
    let saved = ChunkSaveData::from_chunk_with_redstone(&Chunk::empty(0, 0), &metadata).unwrap();
    assert_eq!(saved.redstone_metadata(), metadata);
}

#[test]
fn chunk_saved_without_redstone_sidecar_loads_as_empty_metadata() {
    // Older saves written before the redstone metadata sidecar existed
    // must deserialize cleanly and report an empty metadata vector. Build a
    // `ChunkSaveData` without the sidecar by serializing the historical
    // shape directly, then confirm `redstone_metadata()` degrades
    // gracefully.
    #[derive(serde::Serialize)]
    struct LegacyChunkSaveData {
        chunk_x: i32,
        chunk_z: i32,
        blocks: Vec<u8>,
        sky_light: Vec<u8>,
        block_light: Vec<u8>,
        fluid_levels: Vec<u8>,
    }

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let world_dir = std::env::temp_dir().join(format!(
        "icraft_redstone_legacy_{}_{}",
        std::process::id(),
        unique
    ));

    let chunk = Chunk::new(0, 0);
    let mut manager = SaveManager::new(&world_dir);
    let current = ChunkSaveData::from_chunk(&chunk).unwrap();
    let legacy = LegacyChunkSaveData {
        chunk_x: chunk.chunk_x,
        chunk_z: chunk.chunk_z,
        blocks: current.blocks,
        sky_light: current.sky_light,
        block_light: current.block_light,
        fluid_levels: current.fluid_levels,
    };
    let region = crate::save::RegionData {
        chunks: [((0u8, 0u8), bincode::serialize(&legacy).unwrap())]
            .into_iter()
            .collect(),
    };
    let region_bytes = bincode::serialize(&region).unwrap();
    std::fs::create_dir_all(world_dir.join("regions")).unwrap();
    std::fs::write(world_dir.join("regions/r.0.0.bin"), region_bytes).unwrap();

    let saved = manager
        .load_chunk_in(crate::dimension::Dimension::Overworld, 0, 0)
        .expect("legacy chunk should load");
    assert!(saved.redstone_metadata().is_empty());
    assert!(saved.block_states().is_empty());

    let mut restored = Chunk::new(0, 0);
    saved.restore_to_chunk(&mut restored).unwrap();
    assert_eq!(restored.get_block_state(0, 64, 0), 0);

    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn block_states_roundtrip_and_restore() {
    let mut chunk = Chunk::new(1, 1);
    chunk.set_block_local(5, 64, 5, BlockType::OakDoor);
    chunk.set_block_local(7, 65, 7, BlockType::Torch);
    chunk.set_block_state(5, 64, 5, 0b0000_1101); // facing East, top, right hinge

    let save_data = ChunkSaveData::from_chunk(&chunk).unwrap();
    assert!(!save_data.block_states().is_empty());

    let mut restored = Chunk::new(1, 1);
    save_data.restore_to_chunk(&mut restored).unwrap();
    assert_eq!(restored.get_block_state(5, 64, 5), 0b0000_1101);
    assert_eq!(restored.get_block(5, 64, 5), BlockType::OakDoor);
    assert!(restored
        .torch_positions()
        .iter()
        .any(|&position| Chunk::decode_torch_position(position) == (7, 65, 7)));
}

#[test]
fn test_entity_save_data_roundtrip() {
    use crate::entity::{Entity, EntityType};
    use glam::Vec3;

    let mut entity = Entity::new(42, EntityType::Pig, Vec3::new(10.5, 64.0, -15.2));
    entity.health = 7.5;
    entity.age = -120.0;
    entity.has_wool = true;

    let save_data = EntitySaveData::from(&entity);
    assert_eq!(save_data.entity_type, EntityType::Pig);
    assert_eq!(save_data.position, [10.5, 64.0, -15.2]);

    let restored = save_data.to_entity(100);
    assert_eq!(restored.id, 100);
    assert_eq!(restored.entity_type, EntityType::Pig);
    assert_eq!(restored.position, Vec3::new(10.5, 64.0, -15.2));
    assert_eq!(restored.health, 7.5);
    assert_eq!(restored.age, -120.0);
}

#[test]
fn test_save_manager_entities_persistence() {
    let temp_dir =
        std::env::temp_dir().join(format!("icraft_test_entities_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    let manager = SaveManager::new(&temp_dir);

    let test_entity = crate::entity::Entity::new(
        1,
        crate::entity::EntityType::Zombie,
        glam::Vec3::new(1.0, 65.0, 2.0),
    );
    let test_data = vec![EntitySaveData::from(&test_entity)];

    manager
        .save_entities_in(crate::dimension::Dimension::Overworld, &test_data)
        .unwrap();

    let loaded = manager.load_entities_in(crate::dimension::Dimension::Overworld);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].entity_type, crate::entity::EntityType::Zombie);
    assert_eq!(loaded[0].position, [1.0, 65.0, 2.0]);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

fn unique_test_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("icraft_{label}_{}_{}", std::process::id(), unique))
}

#[test]
fn stale_ack_cannot_clear_a_newer_dirty_revision() {
    let tracker = DirtyChunkSet::new();
    let first = tracker.mark_dirty(2, -4);
    assert!(tracker.begin_save(2, -4, first));
    let second = tracker.mark_dirty(2, -4);

    tracker.acknowledge_persisted(2, -4, first);
    assert_eq!(tracker.state(2, -4), Some(SaveState::Dirty(second)));

    assert!(tracker.begin_save(2, -4, second));
    tracker.acknowledge_persisted(2, -4, second);
    assert_eq!(tracker.state(2, -4), Some(SaveState::Persisted(second)));
}

#[test]
fn corrupt_existing_region_is_never_overwritten() {
    let world_dir = unique_test_dir("region_corruption");
    let mut manager = SaveManager::new(&world_dir);
    let first = Chunk::new(0, 0);
    manager
        .save_chunk(0, 0, ChunkSaveData::from_chunk(&first).unwrap())
        .unwrap();

    let region_path = world_dir.join("regions/r.0.0.bin");
    let corrupt_bytes = b"not a bincode region".to_vec();
    fs::write(&region_path, &corrupt_bytes).unwrap();

    let second = Chunk::new(1, 0);
    let error = manager
        .save_chunk(1, 0, ChunkSaveData::from_chunk(&second).unwrap())
        .unwrap_err();
    assert!(matches!(error, SaveError::RegionCorruption { .. }));
    assert_eq!(fs::read(&region_path).unwrap(), corrupt_bytes);

    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn atomic_replace_faults_leave_a_complete_old_or_new_file() {
    let world_dir = unique_test_dir("atomic_replace");
    fs::create_dir_all(&world_dir).unwrap();
    let path = world_dir.join("level.dat");
    atomic_write(&path, b"old complete value").unwrap();

    ATOMIC_WRITE_FAILPOINT.with(|failpoint| failpoint.set(1));
    assert!(atomic_write(&path, b"new complete value").is_err());
    assert_eq!(fs::read(&path).unwrap(), b"old complete value");

    ATOMIC_WRITE_FAILPOINT.with(|failpoint| failpoint.set(2));
    assert!(atomic_write(&path, b"new complete value").is_err());
    assert_eq!(fs::read(&path).unwrap(), b"new complete value");
    ATOMIC_WRITE_FAILPOINT.with(|failpoint| failpoint.set(0));

    assert!(fs::read_dir(&world_dir).unwrap().all(|entry| !entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".tmp")));
    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn atomic_replace_survives_process_crash_before_and_after_replace() {
    let world_dir = unique_test_dir("atomic_process_crash");
    fs::create_dir_all(&world_dir).unwrap();
    let path = world_dir.join("level.dat");
    atomic_write(&path, b"old complete value").unwrap();
    let test_binary = std::env::current_exe().unwrap();

    let before = std::process::Command::new(&test_binary)
        .args([
            "--ignored",
            "--exact",
            "save::tests::atomic_replace_crash_child",
        ])
        .env("ICRAFT_TEST_ATOMIC_CRASH_STAGE", "before_replace")
        .env("ICRAFT_TEST_ATOMIC_CRASH_PATH", &path)
        .output()
        .unwrap();
    assert!(!before.status.success());
    assert_eq!(fs::read(&path).unwrap(), b"old complete value");

    let after = std::process::Command::new(&test_binary)
        .args([
            "--ignored",
            "--exact",
            "save::tests::atomic_replace_crash_child",
        ])
        .env("ICRAFT_TEST_ATOMIC_CRASH_STAGE", "after_replace")
        .env("ICRAFT_TEST_ATOMIC_CRASH_PATH", &path)
        .output()
        .unwrap();
    assert!(!after.status.success());
    assert_eq!(fs::read(&path).unwrap(), b"new complete value");

    fs::remove_dir_all(world_dir).unwrap();
}

fn same_region_chunk_data(cx: i32, cz: i32, marker: BlockType) -> ChunkSaveData {
    let mut chunk = Chunk::new(cx, cz);
    chunk.set_block_local(0, 64, 0, marker);
    ChunkSaveData::from_chunk(&chunk).unwrap()
}

fn assert_saved_marker(manager: &mut SaveManager, cx: i32, cz: i32, marker: BlockType) {
    let saved = manager
        .load_chunk_in(crate::dimension::Dimension::Overworld, cx, cz)
        .expect("same-region chunk should load after restart");
    let mut restored = Chunk::new(cx, cz);
    saved.restore_to_chunk(&mut restored).unwrap();
    assert_eq!(restored.get_block_local(0, 64, 0), marker);
}

fn save_same_region_new_snapshots(world_dir: &Path) {
    let mut manager = SaveManager::new(world_dir);
    manager
        .save_chunk_in(
            crate::dimension::Dimension::Overworld,
            0,
            0,
            same_region_chunk_data(0, 0, BlockType::Obsidian),
        )
        .unwrap();
    manager
        .save_chunk_in(
            crate::dimension::Dimension::Overworld,
            1,
            0,
            same_region_chunk_data(1, 0, BlockType::StoneBrick),
        )
        .unwrap();
}

#[test]
fn same_region_batch_faults_replace_atomically_and_preserve_sibling_on_restart() {
    let world_dir = unique_test_dir("same_region_batch_faults");
    let mut manager = SaveManager::new(&world_dir);
    manager
        .save_chunk_in(
            crate::dimension::Dimension::Overworld,
            0,
            0,
            same_region_chunk_data(0, 0, BlockType::Brick),
        )
        .unwrap();
    manager
        .save_chunk_in(
            crate::dimension::Dimension::Overworld,
            1,
            0,
            same_region_chunk_data(1, 0, BlockType::Cobblestone),
        )
        .unwrap();
    drop(manager);

    let mut manager = SaveManager::new(&world_dir);
    ATOMIC_WRITE_FAILPOINT.with(|failpoint| failpoint.set(1));
    assert!(matches!(
        manager.save_chunk_in(
            crate::dimension::Dimension::Overworld,
            0,
            0,
            same_region_chunk_data(0, 0, BlockType::Obsidian),
        ),
        Err(SaveError::Io { .. })
    ));
    ATOMIC_WRITE_FAILPOINT.with(|failpoint| failpoint.set(0));
    drop(manager);

    let mut restarted = SaveManager::new(&world_dir);
    assert_saved_marker(&mut restarted, 0, 0, BlockType::Brick);
    assert_saved_marker(&mut restarted, 1, 0, BlockType::Cobblestone);
    drop(restarted);

    let mut manager = SaveManager::new(&world_dir);
    ATOMIC_WRITE_FAILPOINT.with(|failpoint| failpoint.set(2));
    assert!(matches!(
        manager.save_chunk_in(
            crate::dimension::Dimension::Overworld,
            0,
            0,
            same_region_chunk_data(0, 0, BlockType::Obsidian),
        ),
        Err(SaveError::Io { .. })
    ));
    ATOMIC_WRITE_FAILPOINT.with(|failpoint| failpoint.set(0));
    drop(manager);

    let mut restarted = SaveManager::new(&world_dir);
    assert_saved_marker(&mut restarted, 0, 0, BlockType::Obsidian);
    assert_saved_marker(&mut restarted, 1, 0, BlockType::Cobblestone);
    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn failed_region_write_does_not_replace_in_memory_region_cache() {
    let world_dir = unique_test_dir("region_cache_write_failure");
    let mut manager = SaveManager::new(&world_dir);
    manager
        .save_chunk_in(
            crate::dimension::Dimension::Overworld,
            0,
            0,
            same_region_chunk_data(0, 0, BlockType::Brick),
        )
        .unwrap();
    manager
        .save_chunk_in(
            crate::dimension::Dimension::Overworld,
            1,
            0,
            same_region_chunk_data(1, 0, BlockType::Cobblestone),
        )
        .unwrap();
    assert_saved_marker(&mut manager, 0, 0, BlockType::Brick);
    let cached_before = manager
        .region_cache
        .get(&(crate::dimension::Dimension::Overworld, 0, 0))
        .expect("successful write must populate the region cache")
        .chunks
        .clone();

    ATOMIC_WRITE_FAILPOINT.with(|failpoint| failpoint.set(1));
    assert!(matches!(
        manager.save_chunk_in(
            crate::dimension::Dimension::Overworld,
            0,
            0,
            same_region_chunk_data(0, 0, BlockType::Obsidian),
        ),
        Err(SaveError::Io { .. })
    ));
    ATOMIC_WRITE_FAILPOINT.with(|failpoint| failpoint.set(0));

    let cached_after = manager
        .region_cache
        .get(&(crate::dimension::Dimension::Overworld, 0, 0))
        .expect("failed write must leave the previous cache entry")
        .chunks
        .clone();
    assert_eq!(
        cached_after, cached_before,
        "a failed region replacement must not publish the in-memory region"
    );
    assert_saved_marker(&mut manager, 0, 0, BlockType::Brick);
    assert_saved_marker(&mut manager, 1, 0, BlockType::Cobblestone);
    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn same_region_batch_survives_process_crash_before_and_after_replace() {
    let world_dir = unique_test_dir("same_region_batch_crash");
    let mut manager = SaveManager::new(&world_dir);
    manager
        .save_chunk_in(
            crate::dimension::Dimension::Overworld,
            0,
            0,
            same_region_chunk_data(0, 0, BlockType::Brick),
        )
        .unwrap();
    manager
        .save_chunk_in(
            crate::dimension::Dimension::Overworld,
            1,
            0,
            same_region_chunk_data(1, 0, BlockType::Cobblestone),
        )
        .unwrap();
    drop(manager);

    let test_binary = std::env::current_exe().unwrap();
    let before = std::process::Command::new(&test_binary)
        .args([
            "--ignored",
            "--exact",
            "save::tests::same_region_batch_crash_child",
        ])
        .env("ICRAFT_TEST_ATOMIC_CRASH_STAGE", "before_replace")
        .env("ICRAFT_TEST_ATOMIC_CRASH_WORLD", &world_dir)
        .output()
        .unwrap();
    assert!(!before.status.success());

    let mut restarted = SaveManager::new(&world_dir);
    assert_saved_marker(&mut restarted, 0, 0, BlockType::Brick);
    assert_saved_marker(&mut restarted, 1, 0, BlockType::Cobblestone);
    drop(restarted);

    let after = std::process::Command::new(&test_binary)
        .args([
            "--ignored",
            "--exact",
            "save::tests::same_region_batch_crash_child",
        ])
        .env("ICRAFT_TEST_ATOMIC_CRASH_STAGE", "after_replace")
        .env("ICRAFT_TEST_ATOMIC_CRASH_WORLD", &world_dir)
        .output()
        .unwrap();
    assert!(!after.status.success());

    let mut restarted = SaveManager::new(&world_dir);
    assert_saved_marker(&mut restarted, 0, 0, BlockType::Obsidian);
    assert_saved_marker(&mut restarted, 1, 0, BlockType::Cobblestone);
    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
#[ignore = "helper subprocess for atomic_replace_survives_process_crash_before_and_after_replace"]
fn atomic_replace_crash_child() {
    let path = std::env::var_os("ICRAFT_TEST_ATOMIC_CRASH_PATH")
        .map(PathBuf::from)
        .expect("missing crash-test path");
    atomic_write(path, b"new complete value").unwrap();
    panic!("atomic crash failpoint did not abort");
}

#[test]
#[ignore = "helper subprocess for same_region_batch_survives_process_crash_before_and_after_replace"]
fn same_region_batch_crash_child() {
    let world_dir = std::env::var_os("ICRAFT_TEST_ATOMIC_CRASH_WORLD")
        .map(PathBuf::from)
        .expect("missing crash-test world path");
    save_same_region_new_snapshots(&world_dir);
    panic!("atomic crash failpoint did not abort");
}

#[test]
fn missing_region_load_does_not_create_phantom_lru_keys() {
    let world_dir = unique_test_dir("region_lru");
    let mut manager = SaveManager::new(&world_dir);
    assert!(manager.load_chunk(1024, 1024).is_none());
    assert!(manager.lru_order.is_empty());
    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn salvage_copies_only_readable_chunks_without_mutating_source() {
    let world_dir = unique_test_dir("region_salvage");
    let manager = SaveManager::new(&world_dir);
    let source = world_dir.join("corrupt-region.bin");
    let destination = world_dir.join("salvaged-region.bin");
    let valid = bincode::serialize(&ChunkSaveData::from_chunk(&Chunk::new(0, 0)).unwrap()).unwrap();
    let region = RegionData {
        chunks: [((0, 0), valid), ((1, 0), b"broken chunk".to_vec())]
            .into_iter()
            .collect(),
    };
    let source_bytes = bincode::serialize(&region).unwrap();
    fs::write(&source, &source_bytes).unwrap();

    assert_eq!(
        manager
            .salvage_readable_region(&source, &destination)
            .unwrap(),
        1
    );
    assert_eq!(fs::read(&source).unwrap(), source_bytes);
    let salvaged: RegionData = bincode::deserialize(&fs::read(destination).unwrap()).unwrap();
    assert_eq!(salvaged.chunks.len(), 1);
    assert!(salvaged.chunks.contains_key(&(0, 0)));
    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn mutation_index_survives_reload() {
    let world_dir = unique_test_dir("network_revision_index");
    let manager = SaveManager::new(&world_dir);
    let mut index = MutationRevisionIndex::default();
    assert_eq!(
        index
            .bump(crate::dimension::Dimension::Overworld, 7, -4)
            .unwrap(),
        1
    );
    assert_eq!(
        index
            .bump(crate::dimension::Dimension::Overworld, 7, -4)
            .unwrap(),
        2
    );
    manager.save_mutation_revision_index(&index).unwrap();
    drop(manager);

    let manager = SaveManager::new(&world_dir);
    let reloaded = manager.load_mutation_revision_index();
    assert_eq!(
        reloaded.latest(crate::dimension::Dimension::Overworld, 7, -4),
        2
    );
    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn mutation_revision_index_refuses_new_coordinate_at_capacity() {
    let dimension = crate::dimension::Dimension::Overworld;
    let mut index = MutationRevisionIndex::with_capacity_limit(2);

    assert_eq!(index.bump(dimension, 1, 1).unwrap(), 1);
    assert!(index.ensure_at_least(dimension, 2, 2, 7).unwrap());
    assert_eq!(index.bump(dimension, 1, 1).unwrap(), 2);
    assert!(!index.ensure_at_least(dimension, 2, 2, 3).unwrap());

    assert_eq!(
        index.bump(dimension, 3, 3),
        Err(MutationRevisionIndexCapacityError { capacity: 2 })
    );
    assert_eq!(
        index.ensure_at_least(dimension, 4, 4, 9),
        Err(MutationRevisionIndexCapacityError { capacity: 2 })
    );
    assert_eq!(index.len(), 2);
    assert_eq!(index.latest(dimension, 1, 1), 2);
    assert_eq!(index.latest(dimension, 2, 2), 7);
    assert_eq!(index.latest(dimension, 3, 3), 0);
}

#[test]
fn mutation_revision_index_reclaim_is_revision_safe_and_frees_capacity() {
    let dimension = crate::dimension::Dimension::Nether;
    let mut index = MutationRevisionIndex::with_capacity_limit(1);

    assert!(index.ensure_at_least(dimension, -5, 8, 12).unwrap());
    assert!(!index.reclaim_through(dimension, -5, 8, 11));
    assert_eq!(index.latest(dimension, -5, 8), 12);
    assert!(index.bump(dimension, 9, 9).is_err());

    assert!(index.reclaim_through(dimension, -5, 8, 12));
    assert!(index.is_empty());
    assert_eq!(index.bump(dimension, 9, 9).unwrap(), 1);
    assert_eq!(index.remove(dimension, 9, 9), Some(1));
    assert!(index.is_empty());
}

#[test]
fn mutation_revision_index_roundtrip_preserves_highest_revision() {
    let dimension = crate::dimension::Dimension::End;
    let mut index = MutationRevisionIndex::default();

    assert!(index.ensure_at_least(dimension, 6, -3, 41).unwrap());
    assert!(!index.ensure_at_least(dimension, 6, -3, 17).unwrap());
    let encoded = bincode::serialize(&index).unwrap();
    let reloaded: MutationRevisionIndex = bincode::deserialize(&encoded).unwrap();

    assert_eq!(reloaded.latest(dimension, 6, -3), 41);
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded.capacity(), MUTATION_REVISION_INDEX_CAPACITY);
}

#[test]
fn block_entity_save_and_restore_roundtrip() {
    use crate::block_entity::{BlockEntity, ChestBlockEntity};

    let mut chunk = Chunk::new(0, 0);
    chunk.set_block_local(1, 2, 3, BlockType::Chest);
    let chest_stub = BlockEntity::Chest(ChestBlockEntity {
        inventory: crate::inventory::ContainerInventory::new(),
        custom_name: Some("Secret Stash".to_string()),
        loot_table: None,
        loot_seed: None,
        revision: 0,
    });
    chunk
        .insert_block_entity(1, 2, 3, chest_stub.clone())
        .unwrap();

    // Roundtrip via ChunkSaveData
    let save_data = ChunkSaveData::from_chunk(&chunk).unwrap();
    let mut restored = Chunk::new(0, 0);
    save_data.restore_to_chunk(&mut restored).unwrap();

    assert_eq!(restored.get_block_local(1, 2, 3), BlockType::Chest);
    assert_eq!(restored.get_block_entity(1, 2, 3), Some(&chest_stub));

    // Roundtrip via bincode serialization of ChunkSaveData
    let bytes = bincode::serialize(&save_data).unwrap();
    let loaded_save_data = deserialize_chunk_save_data(&bytes).unwrap();
    let mut reloaded = Chunk::new(0, 0);
    loaded_save_data.restore_to_chunk(&mut reloaded).unwrap();

    assert_eq!(reloaded.get_block_entity(1, 2, 3), Some(&chest_stub));
}

#[test]
fn test_plan04_save_roundtrip_and_migration() {
    // 1. LevelData roundtrip
    let level = LevelData {
        seed: 12345,
        time: 6000,
        spawn_x: 100,
        spawn_y: 65,
        spawn_z: -200,
        spawn_dimension: crate::dimension::Dimension::Overworld,
        spawn_yaw: 90.0,
        version: 2,
        ..LevelData::default()
    };
    let bytes = bincode::serialize(&level).unwrap();
    let restored_level: LevelData = bincode::deserialize(&bytes).unwrap();
    assert_eq!(restored_level.spawn_x, 100);
    assert_eq!(restored_level.spawn_y, 65);
    assert_eq!(restored_level.spawn_z, -200);

    // A pre-Plan-15 level payload must load with explicit rule/creation
    // defaults instead of failing at the newly appended bincode fields.
    let legacy = LegacyLevelData {
        seed: 77,
        time: 123,
        spawn_x: 4,
        spawn_y: 80,
        spawn_z: -9,
        spawn_dimension: crate::dimension::Dimension::Overworld,
        spawn_yaw: 0.0,
        version: 2,
    };
    let legacy_bytes = bincode::serialize(&legacy).unwrap();
    let migrated = bincode::deserialize::<LevelData>(&legacy_bytes)
        .or_else(|_| bincode::deserialize::<LegacyLevelData>(&legacy_bytes).map(Into::into))
        .unwrap();
    assert_eq!(migrated.seed, 77);
    assert!(migrated.rules == crate::game_rules::WorldRules::default());
    assert!(migrated.generate_structures);

    // 2. EntitySaveData dropped_stack migration
    let mut stack = crate::inventory::ItemStack::new(crate::inventory::Item::DiamondSword, 1);
    stack.durability = 100;
    stack.custom_name.set("automation-drop");
    stack.can_break = 0x55;
    stack.can_place_on = 0xaa;

    let mut entity = crate::entity::Entity::new(
        1,
        crate::entity::EntityType::DroppedItem,
        glam::Vec3::new(10.0, 64.0, 10.0),
    );
    entity.dropped_stack = Some(stack.clone());
    entity.dropped_item = Some(crate::inventory::Item::DiamondSword);
    entity.dropped_count = 1;

    let save_entity = EntitySaveData::from(&entity);
    let restored_entity = save_entity.to_entity(1);
    let restored_stack = restored_entity.dropped_stack.unwrap();
    assert_eq!(restored_stack.item, crate::inventory::Item::DiamondSword);
    assert_eq!(restored_stack.durability, 100);
    assert_eq!(restored_stack.custom_name.as_str(), "automation-drop");
    assert_eq!(restored_stack.can_break, 0x55);
    assert_eq!(restored_stack.can_place_on, 0xaa);

    // Pet and entity save data test (Plan 11)
    let mut wolf = crate::entity::Entity::new(
        2,
        crate::entity::EntityType::Wolf,
        glam::Vec3::new(5.0, 64.0, 5.0),
    );
    wolf.is_tamed = true;
    wolf.is_sitting = true;
    wolf.owner_id = Some(42);
    wolf.collar_color = [0.0, 0.0, 1.0]; // Blue collar

    let wolf_save = EntitySaveData::from(&wolf);
    let wolf_bytes = bincode::serialize(&wolf_save).unwrap();
    let restored_wolf_save: EntitySaveData = bincode::deserialize(&wolf_bytes).unwrap();
    let restored_wolf = restored_wolf_save.to_entity(2);

    assert!(restored_wolf.is_tamed);
    assert!(restored_wolf.is_sitting);
    assert_eq!(restored_wolf.owner_id, Some(42));
    assert_eq!(restored_wolf.collar_color, [0.0, 0.0, 1.0]);

    // 3. PlayerData spawn_point
    let mut player_state = crate::player::PlayerState::new();
    player_state.spawn_point = Some([12, 64, -15]);
    player_state.spawn_dimension = Some(crate::dimension::Dimension::Overworld);

    let inv = crate::inventory::Inventory::new();
    let adv = crate::advancements::AdvancementProgressData::default();
    let player_data = PlayerData::from_state(
        glam::Vec3::ZERO,
        glam::Vec3::ZERO,
        0.0,
        0.0,
        &player_state,
        crate::save::GameMode::Survival,
        &inv,
        adv,
    );

    let bytes = bincode::serialize(&player_data).unwrap();
    let restored_player: PlayerData = bincode::deserialize(&bytes).unwrap();
    assert_eq!(restored_player.spawn_point, Some([12, 64, -15]));
    assert_eq!(
        restored_player.spawn_dimension,
        Some(crate::dimension::Dimension::Overworld)
    );
}

#[test]
fn offhand_save_roundtrip_and_legacy_migration() {
    let mut inv = crate::inventory::Inventory::new();
    let shield = crate::inventory::ItemStack::new(crate::inventory::Item::Shield, 1);
    inv.offhand = Some(shield);

    let data = InventoryData::from(&inv);
    assert_eq!(
        data.offhand.as_ref().unwrap().item,
        crate::inventory::Item::Shield
    );

    let restored = data.to_inventory();
    assert_eq!(
        restored.offhand.unwrap().item,
        crate::inventory::Item::Shield
    );

    // Test LegacyInventoryData conversion backward compatibility (where legacy data has no offhand field)
    let legacy = LegacyInventoryData {
        hotbar: vec![],
        main: vec![],
        armor: vec![],
        selected: 0,
    };
    let migrated = InventoryData::from(legacy);
    assert!(migrated.offhand.is_none());
}

#[test]
fn migration_fixture_legacy_0_to_255_preserves_data_and_creates_backup() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let world_dir = std::env::temp_dir().join(format!(
        "icraft_migration_test_{}_{}",
        std::process::id(),
        unique
    ));

    // 1. Create a legacy 256-height format ChunkSaveData fixture (data_version = 0)
    // representing a historical save at chunk (0, 0).
    let total_voxels_256 = 16 * 256 * 16;
    let mut legacy_blocks = vec![0u8; total_voxels_256];
    let mut legacy_states = vec![0u8; total_voxels_256];
    let legacy_sky = vec![15u8; total_voxels_256];
    let legacy_block_light = vec![0u8; total_voxels_256];
    let legacy_fluid = vec![0u8; total_voxels_256];

    // Place custom blocks at y = 10, y = 100, y = 255
    // Index formula in legacy flat array: (x * 256 + y) * 16 + z
    let idx_y10 = (8 * 256 + 10) * 16 + 8;
    let idx_y100 = (4 * 256 + 100) * 16 + 4;
    let idx_y255 = (2 * 256 + 255) * 16 + 2;

    legacy_blocks[idx_y10] = BlockType::DiamondOre as u8;
    legacy_blocks[idx_y100] = BlockType::Obsidian as u8;
    legacy_blocks[idx_y255] = BlockType::GoldOre as u8;
    legacy_states[idx_y100] = 0b00000001; // custom block state bit

    let legacy_save_data = ChunkSaveData {
        chunk_x: 0,
        chunk_z: 0,
        blocks: compress_bytes(&legacy_blocks).unwrap(),
        sky_light: compress_bytes(&legacy_sky).unwrap(),
        block_light: compress_bytes(&legacy_block_light).unwrap(),
        fluid_levels: compress_bytes(&legacy_fluid).unwrap(),
        redstone_metadata: Vec::new(),
        block_states: compress_bytes(&legacy_states).unwrap(),
        mutation_revision: 5,
        block_entities: Vec::new(),
        data_version: 0,
    };

    // 2. Write the legacy save data into region r.0.0.bin manually using SaveManager
    let mut manager = SaveManager::new(&world_dir);
    let region_path = world_dir.join("regions").join("r.0.0.bin");
    let backup_path = world_dir.join("regions").join("r.0.0.bin.bak");

    // Write initial legacy region file
    let legacy_bytes = bincode::serialize(&legacy_save_data).unwrap();
    let mut initial_region = RegionData {
        chunks: std::collections::HashMap::new(),
    };
    initial_region.chunks.insert((0, 0), legacy_bytes);
    let serialized_initial = bincode::serialize(&initial_region).unwrap();
    atomic_write(&region_path, &serialized_initial).unwrap();

    assert!(region_path.exists());
    assert!(!backup_path.exists());

    // 3. Load the chunk via SaveManager in modern Overworld (min_y = -64, height = 384)
    let loaded = manager.load_chunk(0, 0).expect("legacy chunk should load");
    assert_eq!(loaded.data_version, 0);

    let mut modern_chunk = Chunk::empty(0, 0);
    loaded.restore_to_chunk(&mut modern_chunk).unwrap();

    // Verify Y=0..255 block mapping and states
    assert_eq!(
        modern_chunk.get_block_local(8, 10, 8),
        BlockType::DiamondOre
    );
    assert_eq!(modern_chunk.get_block_local(4, 100, 4), BlockType::Obsidian);
    assert_eq!(modern_chunk.get_block_state(4, 100, 4), 0b00000001);
    assert_eq!(modern_chunk.get_block_local(2, 255, 2), BlockType::GoldOre);

    // Verify sections Y < 0 (-64..-1) and Y >= 256 (256..319) remain unconfigured/empty Air
    for wy in -64..0 {
        assert_eq!(modern_chunk.get_block_local(8, wy, 8), BlockType::Air);
    }
    for wy in 256..384 {
        assert_eq!(modern_chunk.get_block_local(8, wy, 8), BlockType::Air);
    }

    // 4. Modify and re-save chunk to trigger region update and original file backup
    modern_chunk.set_block_local(8, -10, 8, BlockType::Bedrock);
    let updated_save_data = ChunkSaveData::from_chunk(&modern_chunk).unwrap();
    assert_eq!(updated_save_data.data_version, CHUNK_SAVE_DATA_VERSION);

    manager.save_chunk(0, 0, updated_save_data).unwrap();

    // Verify original file backup r.0.0.bin.bak was created during migration/save
    assert!(
        backup_path.exists(),
        "original region file backup should exist after migration save"
    );

    let backup_bytes = fs::read(&backup_path).unwrap();
    let backup_region: RegionData = bincode::deserialize(&backup_bytes).unwrap();
    assert!(backup_region.chunks.contains_key(&(0, 0)));

    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn test_corrupt_region_file_is_not_overwritten_on_save_failure() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let world_dir = std::env::temp_dir().join(format!(
        "icraft_corrupt_save_test_{}_{}",
        std::process::id(),
        unique
    ));

    let mut manager = SaveManager::new(&world_dir);
    let region_path = world_dir.join("regions").join("r.0.0.bin");
    fs::create_dir_all(region_path.parent().unwrap()).unwrap();

    // Write corrupt bytes to region file
    let corrupt_content = b"INVALID_BINCODE_REGION_CORRUPT_BYTES_123456789";
    fs::write(&region_path, corrupt_content).unwrap();

    // Attempt to save a chunk into the corrupt region
    let chunk = Chunk::new(0, 0);
    let save_data = ChunkSaveData::from_chunk(&chunk).unwrap();
    let result = manager.save_chunk(0, 0, save_data);

    // Verify save_chunk fails with RegionCorruption error
    assert!(result.is_err());
    if let Err(SaveError::RegionCorruption { path, .. }) = result {
        assert_eq!(path, region_path);
    } else {
        panic!("expected SaveError::RegionCorruption");
    }

    // Verify the corrupt file on disk was NOT overwritten or wiped
    let on_disk_bytes = fs::read(&region_path).unwrap();
    assert_eq!(
        on_disk_bytes, corrupt_content,
        "corrupt region file must be preserved on disk without overwrite"
    );

    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn normalize_player_identity_table() {
    let cases: &[(&str, Result<&str, IdentityError>)] = &[
        ("alice", Ok("alice")),
        ("Alice", Ok("alice")),
        ("ALICE", Ok("alice")),
        ("foo_bar", Ok("foo_bar")),
        ("foo-bar", Ok("foo-bar")),
        ("a", Ok("a")),
        ("abcdefghijklmnop", Ok("abcdefghijklmnop")),
        ("1234567890ab-_", Ok("1234567890ab-_")),
        ("", Err(IdentityError::Empty)),
        ("abcdefghijklmnopq", Err(IdentityError::TooLong)),
        ("foo.bar", Err(IdentityError::InvalidCharset)),
        ("Alice/../Alice", Err(IdentityError::InvalidCharset)),
        ("player!", Err(IdentityError::InvalidCharset)),
        (" ", Err(IdentityError::InvalidCharset)),
        ("你好", Err(IdentityError::InvalidCharset)),
        ("CON", Err(IdentityError::ReservedStem)),
        ("con", Err(IdentityError::ReservedStem)),
        ("prn", Err(IdentityError::ReservedStem)),
        ("aux", Err(IdentityError::ReservedStem)),
        ("nul", Err(IdentityError::ReservedStem)),
        ("com1", Err(IdentityError::ReservedStem)),
        ("COM9", Err(IdentityError::ReservedStem)),
        ("lpt1", Err(IdentityError::ReservedStem)),
        ("LPT9", Err(IdentityError::ReservedStem)),
        ("con.txt", Err(IdentityError::ReservedStem)),
        ("NUL.dat", Err(IdentityError::ReservedStem)),
    ];
    for (raw, expected) in cases {
        let actual = normalize_player_identity(raw);
        match expected {
            Ok(identity) => {
                assert_eq!(actual.as_deref(), Ok(*identity), "{raw:?}");
                assert_eq!(
                    actual.as_deref().unwrap(),
                    raw.to_ascii_lowercase(),
                    "accepted names must already be the lowercase identity: {raw:?}"
                );
            }
            Err(error) => {
                assert_eq!(actual, Err(*error), "{raw:?}");
            }
        }
    }
}

#[test]
fn dedicated_player_files_use_normalized_identity_and_reject_colliding_names() {
    let world_dir = unique_test_dir("player_identity");
    let manager = SaveManager::new(&world_dir);
    let data = PlayerData {
        position: [0.0; 3],
        velocity: [0.0; 3],
        yaw: 0.0,
        pitch: 0.0,
        health: 20.0,
        hunger: 20.0,
        saturation: 5.0,
        exhaustion: 0.0,
        oxygen: 300.0,
        experience: 0,
        experience_level: 0,
        game_mode: GameMode::Survival,
        is_dead: false,
        spawn_point: None,
        spawn_dimension: None,
        inventory: InventoryData {
            hotbar: Vec::new(),
            main: Vec::new(),
            armor: Vec::new(),
            offhand: None,
            selected: 0,
            dragged: None,
            creative_drag_origin: None,
        },
        advancements: Default::default(),
        unlocked_recipes: Default::default(),
        bad_omen_level: 0,
        hero_of_the_village_timer: 0.0,
    };
    manager
        .save_dedicated_player("Alice", Dimension::Overworld, &data, &[])
        .unwrap();
    assert_eq!(
        manager.dedicated_player_file_path("ALICE").unwrap(),
        world_dir.join("players").join("alice.dat")
    );
    assert!(manager.load_dedicated_player("alice").unwrap().is_some());
    assert!(manager.dedicated_player_file_path("foo.bar").is_err());
    assert!(manager
        .dedicated_player_file_path("Alice/../Alice")
        .is_err());
    assert!(manager.dedicated_player_file_path("CON").is_err());
    assert!(manager
        .save_dedicated_player("foo.bar", Dimension::Overworld, &data, &[])
        .is_err());
    assert!(manager.load_dedicated_player("foo.bar").is_err());
    assert!(!world_dir.join("players").join("foo_bar.dat").exists());
    assert!(world_dir.join("players").join("alice.dat").exists());
    fs::remove_dir_all(world_dir).unwrap();
}

fn region_chunk_payload(path: &Path, lx: u8, lz: u8) -> Vec<u8> {
    let region: RegionData = bincode::deserialize(&fs::read(path).unwrap()).unwrap();
    region.chunks.get(&(lx, lz)).cloned().unwrap()
}

fn overwrite_region_chunk_payload(path: &Path, lx: u8, lz: u8, payload: Vec<u8>) {
    let mut region: RegionData = bincode::deserialize(&fs::read(path).unwrap()).unwrap();
    region.chunks.insert((lx, lz), payload);
    fs::write(path, bincode::serialize(&region).unwrap()).unwrap();
}

fn with_corrupt_inner_blocks(payload: &[u8], mutate: impl FnOnce(&mut ChunkSaveData)) -> Vec<u8> {
    let mut data = deserialize_chunk_save_data(payload).unwrap();
    mutate(&mut data);
    bincode::serialize(&data).unwrap()
}

#[test]
fn empty_or_truncated_inner_zlib_restore_is_error() {
    let mut chunk = Chunk::empty(0, 0);
    chunk.set_block_local(8, 10, 8, BlockType::DiamondOre);
    let valid = ChunkSaveData::from_chunk(&chunk).unwrap();

    let mut empty_blocks = valid.clone();
    empty_blocks.blocks.clear();
    assert!(empty_blocks
        .restore_to_chunk(&mut Chunk::empty(0, 0))
        .is_err());

    let mut truncated = valid.clone();
    truncated.blocks.truncate(truncated.blocks.len().min(4));
    assert!(truncated.restore_to_chunk(&mut Chunk::empty(0, 0)).is_err());

    let mut wrong_len = valid.clone();
    wrong_len.blocks = compress_bytes(&[1, 2, 3, 4]).unwrap();
    assert!(wrong_len.restore_to_chunk(&mut Chunk::empty(0, 0)).is_err());

    let mut corrupt_states = valid;
    corrupt_states.block_states = vec![1, 2, 3, 4];
    assert!(corrupt_states
        .restore_to_chunk(&mut Chunk::empty(0, 0))
        .is_err());
}

#[test]
fn from_chunk_compression_failure_returns_err_not_empty_blocks() {
    COMPRESS_FAILPOINT.with(|failpoint| failpoint.set(true));
    let result = ChunkSaveData::from_chunk(&Chunk::empty(0, 0));
    COMPRESS_FAILPOINT.with(|failpoint| failpoint.set(false));
    let error = result.expect_err("compression failure must not succeed");
    assert!(error.to_string().contains("injected compress failure"));
}

#[test]
fn restore_saved_chunk_does_not_insert_corrupt_inner_zlib() {
    let mut world = crate::server_world::ServerWorld::new(
        7,
        crate::dimension::Dimension::Overworld,
        crate::game_rules::WorldType::Superflat,
        false,
        crate::game_rules::WorldRules::default(),
        2,
    );
    assert!(world.chunks.chunks.contains_key(&(0, 0)));

    let mut data = ChunkSaveData::from_chunk(&Chunk::empty(0, 0)).unwrap();
    data.chunk_x = 0;
    data.chunk_z = 0;
    data.blocks.clear();
    assert!(world.restore_saved_chunk(&data).is_err());
    assert!(!world.chunks.chunks.contains_key(&(0, 0)));
    assert!(world.failed_restore_chunks().contains(&(0, 0)));

    world.ensure_chunk(0, 0);
    assert!(
        !world.chunks.chunks.contains_key(&(0, 0)),
        "failed restore must not generate the column"
    );
}

#[test]
fn legal_region_with_empty_inner_zlib_is_not_replaced_by_generated_terrain() {
    let world_dir = unique_test_dir("inner_zlib_empty");
    let mut chunk = Chunk::empty(0, 0);
    chunk.set_block_local(4, 70, 4, BlockType::DiamondOre);
    let mut manager = SaveManager::new(&world_dir);
    manager
        .save_chunk(0, 0, ChunkSaveData::from_chunk(&chunk).unwrap())
        .unwrap();

    let region_path = world_dir.join("regions/r.0.0.bin");
    let original_payload = region_chunk_payload(&region_path, 0, 0);
    let corrupt_payload = with_corrupt_inner_blocks(&original_payload, |data| {
        data.blocks.clear();
    });
    overwrite_region_chunk_payload(&region_path, 0, 0, corrupt_payload.clone());

    let loaded = SaveManager::new(&world_dir)
        .load_chunk(0, 0)
        .expect("envelope still readable");
    assert!(loaded.restore_to_chunk(&mut Chunk::empty(0, 0)).is_err());

    let mut world = crate::server_world::ServerWorld::new(
        99,
        crate::dimension::Dimension::Overworld,
        crate::game_rules::WorldType::Default,
        false,
        crate::game_rules::WorldRules::default(),
        2,
    );
    assert!(world.restore_saved_chunk(&loaded).is_err());
    assert!(!world.chunks.chunks.contains_key(&(0, 0)));

    let mut manager = SaveManager::new(&world_dir);
    for (&(cx, cz), column) in &world.chunks.chunks {
        if world.failed_restore_chunks().contains(&(cx, cz)) {
            continue;
        }
        manager
            .save_chunk(cx, cz, ChunkSaveData::from_chunk(column).unwrap())
            .unwrap();
    }

    assert_eq!(
        region_chunk_payload(&region_path, 0, 0),
        corrupt_payload,
        "save must not replace a failed restore with generated terrain"
    );
    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn persist_index_is_noop_when_in_process_runtime_owns_world() {
    let world_dir = unique_test_dir("persist_index_runtime");
    let manager = SaveManager::new(&world_dir);
    let path = world_dir.join("mutation_revisions.bin");
    atomic_write(&path, b"runtime-owned").unwrap();

    let mut index = MutationRevisionIndex::default();
    assert_eq!(
        index
            .bump(crate::dimension::Dimension::Overworld, 1, 1)
            .unwrap(),
        1
    );

    let wrote = manager
        .save_mutation_revision_index_unless_runtime(&index, true)
        .unwrap();
    assert!(!wrote);
    assert_eq!(fs::read(&path).unwrap(), b"runtime-owned");

    let wrote = manager
        .save_mutation_revision_index_unless_runtime(&index, false)
        .unwrap();
    assert!(wrote);
    assert_ne!(fs::read(&path).unwrap(), b"runtime-owned");
    fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn oversized_zlib_inflate_is_rejected_without_unbounded_output() {
    let expected = 64;
    let bomb = vec![0u8; expected + 256];
    let compressed = compress_bytes(&bomb).unwrap();
    let error = decompress_bytes_limited(&compressed, expected).expect_err("take must cap");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("exceeds"));

    let mut chunk = Chunk::empty(0, 0);
    let dest = destination_voxel_count(&chunk);
    let mut data = ChunkSaveData::from_chunk(&chunk).unwrap();
    data.blocks = compress_bytes(&vec![0u8; dest + 64]).unwrap();
    assert!(data.restore_to_chunk(&mut chunk).is_err());
}

#[test]
fn player_modified_chunk_with_corrupt_inner_zlib_is_not_written_as_generated() {
    let world_dir = unique_test_dir("inner_zlib_player");
    let mut chunk = Chunk::new(0, 0);
    chunk.set_block_local(8, 80, 8, BlockType::GoldOre);
    let mut manager = SaveManager::new(&world_dir);
    manager
        .save_chunk(0, 0, ChunkSaveData::from_chunk(&chunk).unwrap())
        .unwrap();

    let region_path = world_dir.join("regions/r.0.0.bin");
    let original_payload = region_chunk_payload(&region_path, 0, 0);
    let corrupt_payload = with_corrupt_inner_blocks(&original_payload, |data| {
        data.blocks.truncate(3);
    });
    overwrite_region_chunk_payload(&region_path, 0, 0, corrupt_payload.clone());

    let mut world = crate::server_world::ServerWorld::new(
        12345,
        crate::dimension::Dimension::Overworld,
        crate::game_rules::WorldType::Default,
        true,
        crate::game_rules::WorldRules::default(),
        2,
    );
    let loaded = SaveManager::new(&world_dir)
        .load_chunk(0, 0)
        .expect("region envelope remains readable");
    assert!(world.restore_saved_chunk(&loaded).is_err());
    world.ensure_chunk(0, 0);
    assert!(!world.chunks.chunks.contains_key(&(0, 0)));

    let mut manager = SaveManager::new(&world_dir);
    for (&(cx, cz), column) in &world.chunks.chunks {
        if world.failed_restore_chunks().contains(&(cx, cz)) {
            continue;
        }
        manager
            .save_chunk(cx, cz, ChunkSaveData::from_chunk(column).unwrap())
            .unwrap();
    }

    assert_eq!(region_chunk_payload(&region_path, 0, 0), corrupt_payload);
    let still_corrupt = SaveManager::new(&world_dir).load_chunk(0, 0).unwrap();
    assert!(still_corrupt
        .restore_to_chunk(&mut Chunk::empty(0, 0))
        .is_err());
    fs::remove_dir_all(world_dir).unwrap();
}
