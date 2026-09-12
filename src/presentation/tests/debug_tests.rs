// Tests extracted from state.rs::debug_tests (Plan 27).

use super::*;

use super::*;
use crate::dimension::Dimension;

fn embedded_test_world(name: &str) -> std::path::PathBuf {
    let unique = format!(
        "icraft-state-runtime-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    );
    std::env::temp_dir().join(unique)
}

#[test]
fn embedded_runtime_uses_world_player_profile_and_fifo_ack() {
    let world_dir = embedded_test_world("fifo");
    let role = MultiplayerRole::Singleplayer;
    let mut bridge =
        EmbeddedRuntimeBridge::new(&role, world_dir.clone(), 1234, Difficulty::Normal, 8, true)
            .expect("embedded runtime should construct");
    let session = bridge
        .runtime
        .players
        .get(&u64::MAX)
        .expect("local world-player session");
    assert_eq!(
        bridge
            .runtime
            .authority
            .session(u64::MAX)
            .expect("local authority session")
            .username,
        "local"
    );
    assert_eq!(
        session.storage,
        crate::server_runtime::LocalSessionStorage::WorldPlayer
    );

    let before = bridge.runtime.authority.world(Dimension::Overworld).get_block(8, 80, 8);
    bridge
        .queue_request(crate::network::protocol::GameplayRequest {
            request_id: 0,
            client_sequence: 0,
            session_id: 0,
            dimension: crate::dimension::Dimension::Overworld as u8,
            client_revision: 0,
            operation: crate::network::protocol::GameplayOperation::BlockAction {
                action: crate::network::protocol::BlockActionKind::Place,
                x: 8,
                y: 80,
                z: 8,
                face: [0, 1, 0],
                hand: 0,
                held: None,
                block: BlockType::Glass.to_wire(),
                look_milli: [0, 0, 1000],
            },
        })
        .expect("request should enter bounded FIFO");
    assert_eq!(bridge.runtime.authority.world(Dimension::Overworld).get_block(8, 80, 8), before);
    let output = bridge.tick().expect("fixed tick should run");
    assert!(!output
        .snapshot
        .mutations
        .iter()
        .any(|mutation| mutation.position == (8, 80, 8)
            && mutation.block == BlockType::Glass.to_wire()));
    assert_eq!(bridge.runtime.authority.world(Dimension::Overworld).get_block(8, 80, 8), before);
    assert!(output.presentation_events.iter().any(|event| {
        matches!(
            event.as_packet_event(),
            Some(crate::server_runtime::ProjectionEvent {
                dest: crate::server_runtime::ProjectionDest::Session(target),
                packet: crate::network::protocol::Packet::GameplayResponse { response, .. },
                ..
            }) if *target == u64::MAX
                && response.request_id == 1
                && matches!(
                    response.outcome,
                    crate::network::protocol::GameplayOutcome::Rejected {
                        reason: crate::network::protocol::RejectReason::InvalidState
                    }
                )
        )
    }));
    bridge.shutdown().expect("runtime save/shutdown");
    let _ = std::fs::remove_dir_all(world_dir);
}

#[test]
fn embedded_runtime_poses_use_monotonic_sender_time() {
    let world_dir = embedded_test_world("pose");
    let role = MultiplayerRole::Singleplayer;
    let mut bridge =
        EmbeddedRuntimeBridge::new(&role, world_dir.clone(), 1234, Difficulty::Normal, 8, true)
            .expect("embedded runtime should construct");
    let initial = bridge
        .runtime
        .authority
        .session(u64::MAX)
        .expect("local session")
        .position;
    bridge
        .queue_position(
            1,
            glam::Vec3::new(initial[0] + 1.0, initial[1], initial[2]),
            0.5,
            0.1,
        )
        .expect("first pose should enter bounded FIFO");
    bridge.tick().expect("first pose tick");
    let first = bridge
        .runtime
        .authority
        .session(u64::MAX)
        .expect("local session")
        .position;
    assert_eq!(first[0], initial[0] + 1.0);

    bridge
        .queue_position(
            2,
            glam::Vec3::new(first[0] + 1.0, first[1], first[2]),
            0.5,
            0.1,
        )
        .expect("second pose should enter bounded FIFO");
    bridge.tick().expect("second pose tick");
    let second = bridge
        .runtime
        .authority
        .session(u64::MAX)
        .expect("local session")
        .position;
    assert_eq!(second[0], first[0] + 1.0);

    bridge.shutdown().expect("runtime save/shutdown");
    let _ = std::fs::remove_dir_all(world_dir);
}

#[test]
fn embedded_inventory_writeback_copies_only_inventory_and_hotbar() {
    let world_dir = embedded_test_world("writeback");
    let role = MultiplayerRole::Singleplayer;
    let mut bridge =
        EmbeddedRuntimeBridge::new(&role, world_dir.clone(), 1234, Difficulty::Normal, 8, true)
            .expect("embedded runtime should construct");
    let session_id = bridge.session_id();
    let before = bridge
        .runtime
        .authority
        .session(session_id)
        .expect("session")
        .gameplay;
    assert!(before.health_milli > 0);

    let mut inventory = [None; crate::authority::contract::SESSION_INVENTORY_SLOTS];
    inventory[0] = Some(crate::authority::contract::SessionInventorySlot::from_wire(
        crate::network::protocol::ItemWire::from_stack(&ItemStack::new(Item::Dirt, 8)),
        0,
        0,
    ));
    assert!(bridge.sync_local_inventory(inventory, None, 3));

    let after = bridge
        .runtime
        .authority
        .session(session_id)
        .expect("session")
        .gameplay;
    assert_eq!(after.health_milli, before.health_milli);
    assert_eq!(after.hunger_milli, before.hunger_milli);
    assert_eq!(after.experience, before.experience);
    assert_eq!(after.experience_level, before.experience_level);
    assert_eq!(after.mounted_entity, before.mounted_entity);
    assert_eq!(after.selected_hotbar_slot, 3);
    assert!(after.inventory[0].is_some());

    bridge.shutdown().expect("runtime save/shutdown");
    let _ = std::fs::remove_dir_all(world_dir);
}

fn rects_overlap(a: InventoryUiRect, b: InventoryUiRect) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
}

#[test]
fn creative_layout_is_only_used_without_a_station_or_crafting_table() {
    assert_eq!(
        inventory_layout_kind(GameMode::Creative, false, false),
        InventoryLayoutKind::CreativeCatalog
    );
    assert_eq!(
        inventory_layout_kind(GameMode::Survival, false, false),
        InventoryLayoutKind::Standard
    );
    assert_eq!(
        inventory_layout_kind(GameMode::Creative, true, false),
        InventoryLayoutKind::Standard
    );
    assert_eq!(
        inventory_layout_kind(GameMode::Creative, false, true),
        InventoryLayoutKind::Standard
    );
}

#[test]
fn creative_tabs_catalog_scrollbar_and_hotbar_do_not_overlap() {
    for aspect in [4.0 / 3.0, 16.0 / 9.0, 21.0 / 9.0] {
        let catalog: Vec<_> = (0..CREATIVE_VISIBLE_SLOTS)
            .map(|index| creative_catalog_slot_rect(index, aspect))
            .collect();
        let hotbar: Vec<_> = (0..9)
            .map(|index| creative_hotbar_slot_rect(index, aspect))
            .collect();
        let tabs: Vec<_> = (0..CreativeTab::TABS.len())
            .map(creative_tab_rect)
            .collect();
        let scrollbar = creative_scroll_track_rect(aspect);

        for group in [&catalog, &hotbar, &tabs] {
            for (index, rect) in group.iter().enumerate() {
                assert!(rect.x0 >= -1.0 && rect.x1 <= 1.0);
                assert!(rect.y0 >= -1.0 && rect.y1 <= 1.0);
                for other in group.iter().skip(index + 1) {
                    assert!(!rects_overlap(*rect, *other), "{rect:?} {other:?}");
                }
            }
        }
        for catalog_rect in &catalog {
            assert!(!rects_overlap(*catalog_rect, scrollbar));
            assert!(hotbar
                .iter()
                .all(|hotbar_rect| !rects_overlap(*catalog_rect, *hotbar_rect)));
            assert!(tabs
                .iter()
                .all(|tab_rect| !rects_overlap(*catalog_rect, *tab_rect)));
        }
        assert!(hotbar
            .iter()
            .all(|hotbar_rect| !rects_overlap(*hotbar_rect, scrollbar)));
        assert!(tabs
            .iter()
            .all(|tab_rect| !rects_overlap(*tab_rect, scrollbar)));
    }
}

#[test]
fn primary_press_decision_controls_block_fallback_and_held_mining_latch() {
    assert_eq!(
        primary_press_decision(GameMode::Survival, true),
        PrimaryPressDecision {
            keep_held_mining: false,
            instant_break: false,
        }
    );
    assert_eq!(
        primary_press_decision(GameMode::Survival, false),
        PrimaryPressDecision {
            keep_held_mining: true,
            instant_break: false,
        }
    );
    assert_eq!(
        primary_press_decision(GameMode::Creative, true),
        PrimaryPressDecision {
            keep_held_mining: false,
            instant_break: false,
        }
    );
    assert_eq!(
        primary_press_decision(GameMode::Creative, false),
        PrimaryPressDecision {
            keep_held_mining: false,
            instant_break: true,
        }
    );
}

#[test]
fn creative_can_break_end_portal_while_survival_cannot() {
    assert!(can_break_block(BlockType::EndPortal, GameMode::Creative));
    assert!(!can_break_block(BlockType::EndPortal, GameMode::Survival));
    assert!(!can_break_block(BlockType::Air, GameMode::Creative));
}

#[test]
fn melee_targeting_filters_noncombat_entities_and_selects_the_nearest_living_target() {
    use crate::entity::{Entity, EntityType};

    let mut entity_manager = crate::entity::EntityManager::new();
    let entities = [
        Entity::new(1, EntityType::DroppedItem, Vec3::new(0.0, 0.0, 0.75)),
        Entity::new(2, EntityType::HeartParticle, Vec3::new(0.0, 0.0, 0.9)),
        Entity::new(3, EntityType::Arrow, Vec3::new(0.0, 0.0, 1.0)),
        Entity::new(4, EntityType::SplashPotion, Vec3::new(0.0, 0.0, 1.1)),
        Entity::new(5, EntityType::WitherSkull, Vec3::new(0.0, 0.0, 1.2)),
        Entity::new(6, EntityType::DragonBreath, Vec3::new(0.0, 0.0, 1.3)),
        Entity::new(7, EntityType::RemotePlayer, Vec3::new(0.0, 0.0, 1.4)),
        Entity::new(8, EntityType::Zombie, Vec3::new(0.0, 0.0, 3.0)),
        Entity::new(9, EntityType::Skeleton, Vec3::new(0.0, 0.0, 2.0)),
    ];
    for entity in entities {
        entity_manager.entities.push(entity);
    }
    entity_manager.rebuild_indexes();
    let invalid_types = [
        EntityType::DroppedItem,
        EntityType::HeartParticle,
        EntityType::Arrow,
        EntityType::SplashPotion,
        EntityType::WitherSkull,
        EntityType::DragonBreath,
        EntityType::RemotePlayer,
    ];
    for entity_type in invalid_types {
        let entity = entity_manager
            .entities
            .iter()
            .find(|entity| entity.entity_type == entity_type)
            .unwrap();
        assert!(!is_legal_melee_target(entity));
    }

    assert_eq!(
        closest_melee_target(
            &entity_manager,
            Vec3::new(0.0, 0.1, 0.0),
            Vec3::Z,
            MELEE_REACH
        ),
        Some(9)
    );

    entity_manager
        .entities
        .iter_mut()
        .find(|entity| entity.id == 9)
        .unwrap()
        .health = 0.0;
    assert_eq!(
        closest_melee_target(
            &entity_manager,
            Vec3::new(0.0, 0.1, 0.0),
            Vec3::Z,
            MELEE_REACH
        ),
        Some(8)
    );

    let mut endermen = crate::entity::EntityManager::new();
    endermen.entities.push(Entity::new(
        10,
        EntityType::Enderman,
        Vec3::new(0.0, 0.0, 2.0),
    ));
    endermen.rebuild_indexes();
    assert_eq!(
        closest_melee_target(&endermen, Vec3::new(0.0, 0.1, 0.0), Vec3::Z, MELEE_REACH),
        Some(10)
    );
}

#[test]
fn terrain_vertex_layout_exposes_ambient_occlusion() {
    let layout = Vertex::desc();
    assert_eq!(std::mem::size_of::<Vertex>(), 28);
    assert_eq!(layout.array_stride, 28);
    assert_eq!(layout.attributes.len(), 4);
    assert_eq!(layout.attributes[3].offset, 24);
    assert_eq!(layout.attributes[3].shader_location, 3);
    assert_eq!(layout.attributes[3].format, wgpu::VertexFormat::Float32);
}

#[test]
fn debug_chunk_coordinates_handle_negative_world_positions() {
    assert_eq!(debug_chunk_coordinate(0.0, CHUNK_WIDTH), 0);
    assert_eq!(debug_chunk_coordinate(15.999, CHUNK_WIDTH), 0);
    assert_eq!(debug_chunk_coordinate(16.0, CHUNK_WIDTH), 1);
    assert_eq!(debug_chunk_coordinate(-0.001, CHUNK_WIDTH), -1);
    assert_eq!(debug_chunk_coordinate(-16.0, CHUNK_WIDTH), -1);
    assert_eq!(debug_chunk_coordinate(-16.001, CHUNK_WIDTH), -2);
}

#[test]
fn debug_overlay_font_supports_every_required_character() {
    let mut vertices = Vec::new();
    for character in ['B', 'K', 'W', 'X', 'Z', 'b', 'k', 'w', 'x', 'z', '/', '_'] {
        let before = vertices.len();
        add_char_lines(character, 0.0, 0.0, 0.1, 0.2, [1.0; 4], &mut vertices);
        assert!(vertices.len() > before, "missing glyph for {character}");
    }
}

#[test]
fn state_text_helper_uses_bitmap_override_and_builtin_fallback() {
    let source = crate::resources::FontSource::Bitmap(
        [('A', [0b1_1111, 0, 0, 0, 0, 0, 0])].into_iter().collect(),
    );
    let mut overridden = Vec::new();
    add_string_lines_with_source(
        &source,
        "A",
        0.0,
        0.0,
        0.1,
        0.2,
        0.0,
        [1.0; 4],
        &mut overridden,
    );
    // Five lit cells are five line-list segments (ten vertices).
    assert_eq!(overridden.len(), 10);

    let mut fallback = Vec::new();
    add_string_lines("A", 0.0, 0.0, 0.1, 0.2, 0.0, [1.0; 4], &mut fallback);
    // Built-in A uses the shared 5x7 glyph table (18 lit cells -> 36 vertices).
    assert_eq!(fallback.len(), 36);
}

#[test]
fn chat_history_evicts_the_oldest_message() {
    let mut history = std::collections::VecDeque::new();
    for index in 0..=CHAT_HISTORY_CAPACITY {
        push_chat_history(
            &mut history,
            "Player".to_string(),
            format!("message {index}"),
        );
    }
    assert_eq!(history.len(), CHAT_HISTORY_CAPACITY);
    assert_eq!(history.front().unwrap().1, "message 1");
    assert_eq!(history.back().unwrap().1, "message 50");
}

#[test]
fn chat_messages_are_trimmed_sanitized_and_bounded() {
    assert_eq!(normalized_chat_message(" \n\t "), None);
    assert_eq!(
        normalized_chat_message("  hello\nworld  ").as_deref(),
        Some("helloworld")
    );
    let oversized = "x".repeat(CHAT_INPUT_CAPACITY + 10);
    assert_eq!(
        normalized_chat_message(&oversized).unwrap().chars().count(),
        CHAT_INPUT_CAPACITY
    );
}

#[test]
fn name_tag_projection_rejects_invalid_clip_space() {
    assert_eq!(
        project_name_tag(Vec3::new(0.25, -0.5, 0.5), Mat4::IDENTITY),
        Some(Vec2::new(0.25, -0.5))
    );
    assert_eq!(project_name_tag(Vec3::ZERO, Mat4::ZERO), None);
    assert_eq!(
        project_name_tag(Vec3::new(0.0, 0.0, 2.0), Mat4::IDENTITY),
        None
    );
}

#[test]
fn network_handle_preserves_client_chat_and_disconnect_payloads() {
    use crate::network::protocol::{Packet, PROTOCOL_VERSION};
    let (inbound_tx, inbound_rx) = std::sync::mpsc::channel();
    let (outbound_tx, _outbound_rx) = std::sync::mpsc::channel();
    let handle = NetworkHandle::Client {
        client_to_game: inbound_rx,
        game_to_client: outbound_tx,
        thread: None,
    };
    inbound_tx
        .send(crate::network::client::ClientToGame::packet(Packet::ChatMessage {
            sender: "Alex".to_string(),
            message: "hello".to_string(),
        }))
        .unwrap();
    inbound_tx
        .send(crate::network::client::ClientToGame::disconnect("server stopped"))
        .unwrap();

    let events = handle.drain_inbound();
    assert!(matches!(
        &events[0],
        NetworkInbound::Packet(Packet::ChatMessage { sender, message, .. })
            if sender == "Alex" && message == "hello"
    ));
    assert!(matches!(
        &events[1],
        NetworkInbound::Packet(Packet::Disconnect { reason, .. }) if reason == "server stopped"
    ));
}

#[test]
fn network_handle_none_drains_no_inbound_events() {
    let handle = NetworkHandle::None;
    assert!(handle.drain_inbound().is_empty());
}

#[test]
fn client_block_change_is_classified_as_host_authority() {
    use crate::network::protocol::{Packet, PROTOCOL_VERSION};
    let (inbound_tx, inbound_rx) = std::sync::mpsc::channel();
    let (outbound_tx, _outbound_rx) = std::sync::mpsc::channel();
    let handle = NetworkHandle::Client {
        client_to_game: inbound_rx,
        game_to_client: outbound_tx,
        thread: None,
    };
    inbound_tx
        .send(crate::network::client::ClientToGame::packet(Packet::BlockChange {
            dimension: 0,
            revision: 1,
            x: 3,
            y: 80,
            z: -4,
            block: BlockType::Stone.to_wire(),
            state: 0,
            raw_fluid: 0,
        }))
        .unwrap();

    assert!(matches!(
        handle.drain_inbound().as_slice(),
        [NetworkInbound::Packet(Packet::BlockChange {
            x: 3,
            y: 80,
            z: -4,
            block,
            state: 0,
            ..
        })] if *block == BlockType::Stone.to_wire()
    ));
}

#[test]
fn disconnect_cleanup_removes_only_remote_player_entities() {
    let mut entities = crate::entity::EntityManager::new();
    let remote_id = entities.spawn(crate::entity::EntityType::RemotePlayer, Vec3::ZERO);
    let zombie_id = entities.spawn(crate::entity::EntityType::Zombie, Vec3::ZERO);
    let mut remote_players = std::collections::HashMap::new();
    remote_players.insert(7, RemotePlayerState::new(remote_id, "Alex".to_string()));

    clear_remote_players(&mut remote_players, &mut entities);

    assert!(remote_players.is_empty());
    assert!(!entities
        .entities
        .iter()
        .any(|entity| entity.id == remote_id));
    assert!(entities
        .entities
        .iter()
        .any(|entity| entity.id == zombie_id));
}

#[test]
fn every_biome_has_a_debug_name() {
    let biomes = [
        Biome::Plains,
        Biome::Forest,
        Biome::Desert,
        Biome::Taiga,
        Biome::Swamp,
        Biome::WindsweptHills,
        Biome::Ocean,
    ];
    assert!(biomes
        .into_iter()
        .all(|biome| !biome_debug_name(biome).is_empty()));
}

#[test]
fn pause_weather_volume_and_quit_hit_regions_do_not_overlap() {
    assert!(point_in_bounds(0.0, -0.41, PAUSE_WEATHER_VOLUME_BOUNDS));
    assert!(!point_in_bounds(0.0, -0.41, PAUSE_QUIT_BOUNDS));
    assert!(point_in_bounds(0.0, -0.55, PAUSE_QUIT_BOUNDS));
    assert!(!point_in_bounds(0.0, -0.55, PAUSE_WEATHER_VOLUME_BOUNDS));
    assert!(!point_in_bounds(0.31, -0.41, PAUSE_WEATHER_VOLUME_BOUNDS));
}

#[test]
fn fov_adjustment_updates_base_fov_and_camera_fov() {
    let mut base_fov: f32 = 70.0;
    let mut camera_fov: f32;

    // Simulate pause menu FOV increase (+5)
    base_fov = (base_fov + 5.0).min(120.0);
    camera_fov = base_fov;
    assert_eq!(base_fov, 75.0);
    assert_eq!(camera_fov, 75.0);

    // Simulate frame FOV interpolation when not sprinting
    let target_fov = base_fov;
    let dt = 0.016;
    camera_fov = camera_fov + (target_fov - camera_fov) * dt * 10.0;
    assert_eq!(camera_fov, 75.0);

    // Simulate pause menu FOV decrease (-5)
    base_fov = (base_fov - 5.0).max(30.0);
    camera_fov = base_fov;
    assert_eq!(base_fov, 70.0);
    assert_eq!(camera_fov, 70.0);
}

#[test]
fn test_flower_breaks_and_pops_when_ground_is_destroyed() {
    let mut manager = crate::chunk_manager::WorldColumns::new(2);
    manager.chunks.insert((0, 0), Chunk::new(0, 0));
    manager.set_block(2, 10, 2, BlockType::Grass);
    manager.set_block(2, 11, 2, BlockType::Dandelion);

    let mut dirty = std::collections::HashSet::new();
    let mut drops = Vec::new();

    // Destroy the grass block
    manager.set_block(2, 10, 2, BlockType::Air);
    manager.check_and_break_unsupported_above(2, 10, 2, &mut dirty, |pos, block| {
        drops.push((pos, block));
    });

    // Ground is Air now, flower above must be destroyed
    assert_eq!(manager.get_block(2, 11, 2), BlockType::Air);
    assert_eq!(drops, vec![((2, 11, 2), BlockType::Dandelion)]);
}

#[test]
fn door_and_trapdoor_placement_states_and_hinges() {
    let mut manager = crate::chunk_manager::WorldColumns::new(2);
    manager.chunks.insert((0, 0), Chunk::new(0, 0));

    // Test door facing from yaw (yaw=0.0 -> East, yaw=FRAC_PI_2 -> South, -FRAC_PI_2 -> North, PI -> West)
    let (bottom, top) = crate::world::BlockState::for_door_placement(
        &manager,
        5,
        64,
        5,
        std::f32::consts::FRAC_PI_2,
    );
    assert_eq!(bottom.facing, crate::redstone::Direction::South);
    assert!(!bottom.is_top);
    assert!(!bottom.is_open);
    assert!(top.is_top);
    assert_eq!(top.facing, crate::redstone::Direction::South);

    // Hinge logic: left solid, right empty -> right hinge
    // North facing: left = West (-1, 0) -> (4, 64, 5), right = East (+1, 0) -> (6, 64, 5)
    manager.set_block(4, 64, 5, BlockType::Stone); // left neighbor
    manager.set_block(6, 64, 5, BlockType::Air); // right neighbor
    let (bottom_hinge, _) = crate::world::BlockState::for_door_placement(
        &manager,
        5,
        64,
        5,
        -std::f32::consts::FRAC_PI_2,
    );
    assert_eq!(bottom_hinge.facing, crate::redstone::Direction::North);
    assert!(bottom_hinge.is_right_hinge);

    // Trapdoor state
    let trapdoor =
        crate::world::BlockState::for_trapdoor_placement(-std::f32::consts::FRAC_PI_2);
    assert_eq!(trapdoor.facing, crate::redstone::Direction::North);
    assert!(!trapdoor.is_open);
}
