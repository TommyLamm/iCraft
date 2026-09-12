// Tests extracted from server_runtime.rs (Plan 27).

use super::*;
use crate::network::protocol::Packet;
use crate::network::protocol::{BlockActionKind, RejectReason};
use crate::world::BlockType;

fn temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("icraft_plan16_{label}_{unique}"))
}

fn embedded_runtime(label: &str) -> (ServerRuntime, RuntimeInput) {
    let mut properties = ServerProperties::default();
    properties.bind = "127.0.0.1".into();
    properties.port = 25580;
    properties.view_distance = 2;
    properties.simulation_distance = 2;
    properties.world_dir = temp_dir(label);
    ServerRuntime::new_embedded(
        properties,
        EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(99, "local")),
    )
    .unwrap()
}

fn write_world_meta(
    world_dir: &Path,
    game_mode: GameMode,
    cheats_enabled: bool,
) -> io::Result<()> {
    fs::create_dir_all(world_dir)?;
    fs::write(
        world_dir.join("world.meta"),
        format!(
            "name:TEST\nseed:1\ngame_mode:{}\ndifficulty:NORMAL\nlast_played:0\nworld_type:DEFAULT\ngenerate_structures:true\nbonus_chest:false\ncheats_enabled:{cheats_enabled}\nhardcore:false\nversion:3\nneeds_upgrade:false\n",
            match game_mode {
                GameMode::Creative => "CREATIVE",
                GameMode::Survival => "SURVIVAL",
                GameMode::Adventure => "ADVENTURE",
                GameMode::Spectator => "SPECTATOR",
            }
        ),
    )
}

fn embedded_runtime_in(world_dir: PathBuf) -> (ServerRuntime, RuntimeInput) {
    let mut properties = ServerProperties::default();
    properties.bind = "127.0.0.1".into();
    properties.port = 25580;
    properties.view_distance = 2;
    properties.simulation_distance = 2;
    properties.world_dir = world_dir;
    ServerRuntime::new_embedded(
        properties,
        EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(99, "local")),
    )
    .unwrap()
}

#[test]
fn creative_world_meta_seeds_new_player_and_survives_reload() {
    let world_dir = temp_dir("creative_persist");
    write_world_meta(&world_dir, GameMode::Creative, false).unwrap();

    let (mut runtime, _input) = embedded_runtime_in(world_dir.clone());
    assert_eq!(
        runtime.authority.session(99).unwrap().game_mode,
        GameMode::Creative
    );
    assert!(!runtime.level.cheats_enabled);
    runtime.shutdown().unwrap();
    drop(runtime);

    let (mut restored, _input) = embedded_runtime_in(world_dir.clone());
    assert_eq!(
        restored.authority.session(99).unwrap().game_mode,
        GameMode::Creative
    );
    assert!(!restored.level.cheats_enabled);
    restored.shutdown().unwrap();
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn creative_world_recovers_player_dat_forced_to_survival() {
    let world_dir = temp_dir("creative_recover");
    write_world_meta(&world_dir, GameMode::Creative, false).unwrap();
    let manager = SaveManager::new(&world_dir);
    manager
        .save_player_and_level(
            &LevelData::default(),
            &default_player_data(GameMode::Survival),
        )
        .unwrap();

    let (mut runtime, _input) = embedded_runtime_in(world_dir.clone());
    assert_eq!(
        runtime.authority.session(99).unwrap().game_mode,
        GameMode::Creative
    );
    runtime.shutdown().unwrap();
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn world_meta_cheats_survive_first_level_save() {
    let world_dir = temp_dir("cheats_persist");
    write_world_meta(&world_dir, GameMode::Creative, true).unwrap();

    let (mut runtime, _input) = embedded_runtime_in(world_dir.clone());
    assert!(runtime.level.cheats_enabled);
    assert!(runtime.authority.session(99).unwrap().cheats_enabled);
    runtime.shutdown().unwrap();
    drop(runtime);

    let (mut restored, _input) = embedded_runtime_in(world_dir.clone());
    assert!(restored.level.cheats_enabled);
    assert!(restored.authority.session(99).unwrap().cheats_enabled);
    restored.shutdown().unwrap();
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn cheats_world_keeps_saved_survival_after_mode_change() {
    let world_dir = temp_dir("cheats_keep_survival");
    write_world_meta(&world_dir, GameMode::Creative, true).unwrap();
    let manager = SaveManager::new(&world_dir);
    let mut level = LevelData::default();
    level.cheats_enabled = true;
    manager
        .save_player_and_level(&level, &default_player_data(GameMode::Survival))
        .unwrap();

    let (mut runtime, _input) = embedded_runtime_in(world_dir.clone());
    assert_eq!(
        runtime.authority.session(99).unwrap().game_mode,
        GameMode::Survival
    );
    runtime.shutdown().unwrap();
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn invalid_properties_fail_before_world_creation() {
    let path = temp_dir("invalid").join("server.properties");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "port=0\n").unwrap();
    let error = ServerProperties::load(&path).unwrap_err();
    assert!(error.to_string().contains("port"));
    assert!(!path.parent().unwrap().join("world").exists());
    let _ = fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn runtime_pose_validation_rejects_regression_and_speed_but_allows_server_teleport() {
    let (mut runtime, _input) = embedded_runtime("pose_validation");
    let initial = runtime.authority.session(99).unwrap().position;
    runtime
        .handle_position(
            99,
            1,
            100,
            initial[0] + 1.0,
            initial[1],
            initial[2],
            0.5,
            0.1,
        )
        .unwrap();
    let accepted = runtime.authority.session(99).unwrap().position;

    runtime
        .handle_position(
            99,
            2,
            90,
            accepted[0] + 1.0,
            accepted[1],
            accepted[2],
            0.5,
            0.1,
        )
        .unwrap();
    assert_eq!(runtime.authority.session(99).unwrap().position, accepted);
    runtime
        .handle_position(99, 2, 150, 5_000.0, accepted[1], 5_000.0, 0.5, 0.1)
        .unwrap();
    assert_eq!(runtime.authority.session(99).unwrap().position, accepted);

    let teleport = [5_000.0, accepted[1], 5_000.0];
    runtime.players.get_mut(&99).unwrap().teleport_allowance = Some(teleport);
    runtime
        .handle_position(99, 2, 150, teleport[0], teleport[1], teleport[2], 0.5, 0.1)
        .unwrap();
    assert_eq!(runtime.authority.session(99).unwrap().position, teleport);

    let world_dir = runtime.world_dir.clone();
    runtime.shutdown().unwrap();
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn presentation_saturation_preserves_ack_and_session_and_stays_bounded() {
    let (mut runtime, _input) = embedded_runtime("presentation_saturation");
    runtime.presentation_events.clear();
    assert!(
        runtime.push_presentation_event(ProjectionEvent::session(
            99,
            Packet::GameplayResponse {
                response: GameplayResponse {
                    request_id: 700,
                    server_sequence: 1,
                    outcome: GameplayOutcome::Accepted { revision: 1 },
                },
            },
        ))
    );
    let mut session_state = SessionGameplayWire::default();
    session_state.revision = 77;
    assert!(
        runtime.push_presentation_event(ProjectionEvent::session(
            99,
            Packet::PlayerSessionUpdate {
                sequence: 1,
                player_id: 99,
                dimension: Dimension::Overworld as u8,
                state: session_state,
            },
        ))
    );

    for index in 0..(MAX_PRESENTATION_EVENTS_PER_TICK * 2) {
        assert!(
            runtime.push_presentation_event(ProjectionEvent::session(
                99,
                Packet::PlayerPosition {
                    id: 10_000 + index as u64,
                    sequence: index as u32 + 1,
                    sender_time_millis: index as u64 + 1,
                    x: index as f32,
                    y: 80.0,
                    z: 0.0,
                    yaw: 0.0,
                    pitch: 0.0,
                },
            ))
        );
    }
    assert_eq!(
        runtime.presentation_events.len(),
        MAX_PRESENTATION_EVENTS_PER_TICK
    );
    assert!(runtime.presentation_events.iter().any(|event| matches!(
        event,
        PresentationEvent::Packet(ProjectionEvent { packet: Packet::GameplayResponse { response, .. }, .. })
            if response.request_id == 700
    )));
    assert!(runtime.presentation_events.iter().any(|event| matches!(
        event,
        PresentationEvent::Packet(ProjectionEvent { packet: Packet::PlayerSessionUpdate { state, .. }, .. })
            if state.revision == 77
    )));
    assert!(runtime.network_metrics.snapshot().queue_full > 0);

    runtime.presentation_events.clear();
    for index in 0..(MAX_PRESENTATION_QUEUE_LEN + 8) {
        let accepted =
            runtime.push_presentation_event(ProjectionEvent::session(
                99,
                Packet::GameplayResponse {
                    response: GameplayResponse {
                        request_id: index as u128,
                        server_sequence: index as u64 + 1,
                        outcome: GameplayOutcome::Accepted {
                            revision: index as u64 + 1,
                        },
                    },
                },
            ));
        assert_eq!(accepted, index < MAX_PRESENTATION_QUEUE_LEN);
    }
    assert_eq!(
        runtime.presentation_events.len(),
        MAX_PRESENTATION_QUEUE_LEN
    );
    assert!(runtime.presentation_events.iter().any(|event| matches!(
        event,
        PresentationEvent::Packet(ProjectionEvent { packet: Packet::GameplayResponse { response, .. }, .. })
            if response.request_id == 0
    )));

    let world_dir = runtime.world_dir.clone();
    runtime.shutdown().unwrap();
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn embedded_chunk_projection_uses_arc_column_not_dense_stream() {
    let (mut runtime, _input) = embedded_runtime("arc_chunk_column");
    let output = runtime.tick_with_output().unwrap();
    let columns: Vec<_> = output
        .presentation_events
        .iter()
        .filter_map(|event| match event {
            PresentationEvent::ChunkColumn {
                to,
                cx,
                cz,
                revision,
                chunk,
                ..
            } => Some((*to, *cx, *cz, *revision, Arc::strong_count(chunk))),
            _ => None,
        })
        .collect();
    assert!(
        !columns.is_empty(),
        "embedded local session must receive ChunkColumn projections"
    );
    assert!(columns.iter().all(|(to, ..)| *to == 99));
    assert!(
        !output.presentation_events.iter().any(|event| {
            matches!(
                event.as_packet_event(),
                Some(ProjectionEvent {
                    dest: ProjectionDest::Session(99),
                    packet: Packet::ChunkData { .. },
                    ..
                })
            )
        }),
        "embedded must not dense-stream ChunkData to the local session"
    );
    // Arc fanout: projection owns one strong ref; authority kept its owned Chunk.
    assert!(columns.iter().all(|(.., strong)| *strong >= 1));

    let world_dir = runtime.world_dir.clone();
    runtime.shutdown().unwrap();
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn embedded_interest_fanout_is_private_dimension_safe_and_exactly_once() {
    let (mut runtime, _input) = embedded_runtime("interest_fanout");
    runtime.login_session(2, "remote").unwrap();
    let baseline = runtime.tick_with_output().unwrap();
    assert!(baseline.presentation_events.iter().any(|event| matches!(
        event,
        PresentationEvent::ChunkColumn { to: 99, .. }
            | PresentationEvent::Packet(ProjectionEvent {
                dest: ProjectionDest::Session(99),
                packet: Packet::ChunkData { .. },
                ..
            })
    )));
    runtime.drain_routed_updates();

    runtime.authority.with_world(Dimension::Overworld, |world| {
        world
            .set_block(8, 80, 8, crate::world::BlockType::Glass, 0)
            .unwrap();
    });
    let revision = runtime.session_revision(2).unwrap();
    let response = runtime
        .submit_request(
            2,
            GameplayRequest {
                request_id: 700,
                client_sequence: 1,
                session_id: 2,
                dimension: Dimension::Overworld as u8,
                client_revision: revision,
                operation: GameplayOperation::BlockAction {
                    action: BlockActionKind::Place,
                    x: 8,
                    y: 80,
                    z: 8,
                    face: [0, 1, 0],
                    hand: 0,
                    held: None,
                    block: crate::world::BlockType::DiamondOre.to_wire(),
                    look_milli: [0, 0, 1000],
                },
            },
        )
        .unwrap();
    assert!(matches!(
        response.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidState
        }
    ));
    let output = runtime.tick_with_output().unwrap();
    assert_eq!(
        output
            .presentation_events
            .iter()
            .filter(|event| matches!(
                event,
                PresentationEvent::Packet(ProjectionEvent {
                    dest: ProjectionDest::Session(99),
                    packet: Packet::BlockChange { x: 8, y: 80, z: 8, .. },
                    ..
                })
            ))
            .count(),
        0,
        "rejected BlockAction must not project a BlockChange"
    );
    assert_eq!(
        runtime
            .authority
            .world_ref(Dimension::Overworld)
            .map(|world| world.get_block(8, 80, 8)),
        Some(crate::world::BlockType::Glass)
    );

    assert!(runtime.set_session_dimension(2, Dimension::Nether));
    runtime.drain_routed_updates();
    runtime.authority.with_world(Dimension::Overworld, |world| {
        world
            .set_block(9, 80, 8, crate::world::BlockType::Stone, 0)
            .unwrap();
    });
    let revision = runtime.session_revision(99).unwrap();
    let response = runtime
        .submit_request(
            99,
            GameplayRequest {
                request_id: 701,
                client_sequence: 1,
                session_id: 99,
                dimension: Dimension::Overworld as u8,
                client_revision: revision,
                operation: GameplayOperation::BlockAction {
                    action: BlockActionKind::Place,
                    x: 9,
                    y: 80,
                    z: 8,
                    face: [0, 1, 0],
                    hand: 0,
                    held: None,
                    block: crate::world::BlockType::DiamondOre.to_wire(),
                    look_milli: [0, 0, 1000],
                },
            },
        )
        .unwrap();
    assert!(matches!(
        response.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidState
        }
    ));
    assert!(!runtime
        .drain_routed_updates()
        .iter()
        .any(|update| update.target == 2 && update.dimension == Dimension::Overworld));
    assert_eq!(
        runtime
            .authority
            .world_ref(Dimension::Overworld)
            .map(|world| world.get_block(9, 80, 8)),
        Some(crate::world::BlockType::Stone)
    );

    let chest = (8, 80, 8);
    runtime
        .players
        .get_mut(&99)
        .unwrap()
        .interest
        .open_containers
        .clear();
    assert!(runtime.set_session_dimension(2, Dimension::Overworld));
    runtime
        .players
        .get_mut(&2)
        .unwrap()
        .interest
        .open_containers
        .insert(chest);
    assert_eq!(
        runtime.queue_interest_update(
            Dimension::Overworld,
            runtime
                .authority
                .revision_for_dimension(Dimension::Overworld),
            InterestKind::Container(chest),
        ),
        vec![2]
    );

    let world_dir = runtime.world_dir.clone();
    runtime.shutdown().unwrap();
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn embedded_block_action_loads_an_interested_boundary_chunk_on_demand() {
    let (mut runtime, _input) = embedded_runtime("boundary_block_action");
    let seed = runtime.level.seed;
    let generated = crate::dimension::generate_chunk(Dimension::Overworld, 1, 0, seed);
    let local_z = 8usize;
    let mut target_y = i32::from(generated.heightmap[0][local_z]);
    while !generated
        .get_block_local(0, target_y, local_z)
        .properties()
        .is_solid
    {
        target_y -= 1;
    }
    let target = (16, target_y, local_z as i32);
    assert_ne!(
        generated.get_block_local(0, target_y, local_z),
        BlockType::Air
    );

    let player_position = [15.25, target_y as f32 + 1.0, local_z as f32 + 0.5];
    assert!(runtime.teleport_session(99, player_position));
    assert!(runtime
        .authority
        .world_ref(Dimension::Overworld)
        .is_some_and(|world| !world.chunks.chunks.contains_key(&(1, 0))));
    runtime.authority.session_mut(99).unwrap().game_mode = GameMode::Creative;
    let held = runtime
        .authority
        .session(99)
        .and_then(|session| session.gameplay.slot(session.gameplay.selected_hotbar_slot))
        .flatten()
        .map(Into::into);

    let eye = Vec3::from_array(player_position) + Vec3::new(0.0, 1.62, 0.0);
    let look = (Vec3::new(16.5, target_y as f32 + 0.5, local_z as f32 + 0.5) - eye).normalize();
    let look_milli = [
        (look.x * 1_000.0).round() as i16,
        (look.y * 1_000.0).round() as i16,
        (look.z * 1_000.0).round() as i16,
    ];
    let response = runtime
        .submit_request(
            99,
            GameplayRequest {
                request_id: 702,
                client_sequence: 1,
                session_id: 99,
                dimension: Dimension::Overworld as u8,
                client_revision: runtime.session_revision(99).unwrap(),
                operation: GameplayOperation::BlockAction {
                    action: crate::network::protocol::BlockActionKind::StartBreak,
                    x: target.0,
                    y: target.1,
                    z: target.2,
                    face: [-1, 0, 0],
                    hand: 0,
                    held,
                    block: BlockType::Air.to_wire(),
                    look_milli,
                },
            },
        )
        .unwrap();
    let loaded_world = runtime
        .authority
        .world_ref(Dimension::Overworld)
        .expect("overworld remains loaded");
    let loaded_block = loaded_world.get_block(target.0, target.1, target.2);
    let has_line_of_sight =
        loaded_world.has_block_line_of_sight(player_position, look_milli, target);
    assert!(
        matches!(response.outcome, GameplayOutcome::Accepted { .. }),
        "boundary action was rejected: {:?} (target={target:?}, block={loaded_block:?}, player={player_position:?}, look={look_milli:?}, los={has_line_of_sight})",
        response.outcome,
    );
    assert_eq!(
        runtime
            .authority
            .world_ref(Dimension::Overworld)
            .unwrap()
            .get_block(target.0, target.1, target.2),
        BlockType::Air
    );

    let world_dir = runtime.world_dir.clone();
    runtime.shutdown().unwrap();
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn opening_new_container_replaces_old_session_and_preserves_other_viewers() {
    fn request(
        id: u64,
        sequence: u64,
        revision: u64,
        position: (i32, i32, i32),
    ) -> GameplayRequest {
        GameplayRequest {
            request_id: sequence as u128,
            client_sequence: sequence,
            session_id: id,
            dimension: Dimension::Overworld as u8,
            client_revision: revision,
            operation: GameplayOperation::Container {
                action: ContainerAction::Open,
                x: position.0,
                y: position.1,
                z: position.2,
                slot: 0,
            },
        }
    }

    let prepare =
        |runtime: &mut ServerRuntime, first: (i32, i32, i32), second: (i32, i32, i32)| {
            runtime.authority.with_world(Dimension::Overworld, |world| {
                world
                    .set_block(first.0, first.1, first.2, BlockType::Chest, 0)
                    .unwrap();
                world
                    .set_block(second.0, second.1, second.2, BlockType::Chest, 0)
                    .unwrap();
            });
        };

    let first = (10, 80, 8);
    let second = (11, 80, 8);
    let (mut runtime, _input) = embedded_runtime("container_open_replace");
    prepare(&mut runtime, first, second);
    let revision = runtime.session_revision(99).unwrap();
    assert!(matches!(
        runtime
            .submit_request(99, request(99, 1, revision, first))
            .unwrap()
            .outcome,
        GameplayOutcome::Accepted { .. }
    ));
    let revision = runtime.session_revision(99).unwrap();
    assert!(matches!(
        runtime
            .submit_request(99, request(99, 2, revision, second))
            .unwrap()
            .outcome,
        GameplayOutcome::Accepted { .. }
    ));
    let world = runtime.authority.world_ref(Dimension::Overworld).unwrap();
    assert!(world.container_viewers_at(first).next().is_none());
    assert!(
        !crate::world::BlockState::decode(world.get_block_state(first.0, first.1, first.2))
            .is_open
    );
    assert_eq!(
        world
            .container_viewers_at(second)
            .copied()
            .collect::<Vec<_>>(),
        vec![99]
    );
    assert!(
        crate::world::BlockState::decode(world.get_block_state(second.0, second.1, second.2))
            .is_open
    );
    let world_dir = runtime.world_dir.clone();
    runtime.shutdown().unwrap();
    let _ = fs::remove_dir_all(world_dir);

    let (mut runtime, _input) = embedded_runtime("container_open_replace_observer");
    prepare(&mut runtime, first, second);
    runtime.login_session(2, "observer").unwrap();
    let revision = runtime.session_revision(99).unwrap();
    assert!(matches!(
        runtime
            .submit_request(99, request(99, 1, revision, first))
            .unwrap()
            .outcome,
        GameplayOutcome::Accepted { .. }
    ));
    let revision = runtime.session_revision(2).unwrap();
    assert!(matches!(
        runtime
            .submit_request(2, request(2, 1, revision, first))
            .unwrap()
            .outcome,
        GameplayOutcome::Accepted { .. }
    ));
    let revision = runtime.session_revision(99).unwrap();
    assert!(matches!(
        runtime
            .submit_request(99, request(99, 2, revision, second))
            .unwrap()
            .outcome,
        GameplayOutcome::Accepted { .. }
    ));
    let world = runtime.authority.world_ref(Dimension::Overworld).unwrap();
    assert_eq!(
        world
            .container_viewers_at(first)
            .copied()
            .collect::<Vec<_>>(),
        vec![2]
    );
    assert!(
        crate::world::BlockState::decode(world.get_block_state(first.0, first.1, first.2))
            .is_open
    );
    assert_eq!(
        world
            .container_viewers_at(second)
            .copied()
            .collect::<Vec<_>>(),
        vec![99]
    );
    assert!(
        crate::world::BlockState::decode(world.get_block_state(second.0, second.1, second.2))
            .is_open
    );
    let world_dir = runtime.world_dir.clone();
    runtime.shutdown().unwrap();
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn complete_session_health_death_inventory_and_xp_reach_local_projection_once() {
    let (mut runtime, _input) = embedded_runtime("session_projection");
    let _ = runtime.tick_with_output().unwrap();
    let mut item = ItemWire::empty();
    item.item = crate::inventory::Item::Diamond as u32;
    item.count = 3;
    let mut gameplay = runtime.authority.session(99).unwrap().gameplay;
    gameplay.health_milli = 0;
    gameplay.is_dead = true;
    gameplay.death_source = Some(6);
    gameplay.experience = 77;
    gameplay.experience_level = 4;
    gameplay.selected_hotbar_slot = 5;
    gameplay.inventory[0] = Some(SessionInventorySlot::from_wire(item, 1, 2));
    gameplay.revision = 1;
    assert!(runtime.authority.set_session_gameplay(99, gameplay));

    let output = runtime.tick_with_output().unwrap();
    let updates: Vec<_> = output
        .presentation_events
        .iter()
        .filter_map(|event| match event.as_packet_event() {
            Some(ProjectionEvent {
                packet: Packet::PlayerSessionUpdate { state, .. },
                ..
            }) if state.revision == 1 => Some(*state),
            _ => None,
        })
        .collect();
    assert_eq!(updates.len(), 1);
    assert!(updates[0].is_dead);
    assert_eq!(updates[0].health_milli, 0);
    assert_eq!(updates[0].experience, 77);
    assert_eq!(updates[0].hotbar[0].unwrap().item, item);
    let player_data = &runtime.players[&99].data;
    assert!(player_data.is_dead);
    assert_eq!(player_data.experience, 77);
    assert_eq!(player_data.experience_level, 4);
    assert_eq!(player_data.inventory.selected, 5);
    assert!(runtime
        .tick_with_output()
        .unwrap()
        .presentation_events
        .iter()
        .all(|event| !matches!(
            event.as_packet_event(),
            Some(ProjectionEvent {
                packet: Packet::PlayerSessionUpdate { state, .. },
                ..
            }) if state.revision == 1
        )));

    let world_dir = runtime.world_dir.clone();
    runtime.shutdown().unwrap();
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn properties_roundtrip_and_whitelist_are_deterministic() {
    let dir = temp_dir("properties");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("server.properties");
    let mut expected = ServerProperties::default();
    expected.port = 25570;
    expected.whitelist = ["Alex".to_ascii_lowercase(), "Steve".to_ascii_lowercase()]
        .into_iter()
        .collect();
    expected.write(&path).unwrap();
    let loaded = ServerProperties::load(&path).unwrap();
    assert_eq!(loaded.port, expected.port);
    assert_eq!(loaded.whitelist, expected.whitelist);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn request_deduplication_and_out_of_order_are_authoritative() {
    let mut properties = ServerProperties::default();
    properties.bind = "127.0.0.1".into();
    properties.port = 0;
    // `new` validates a real port, so exercise the protocol core without
    // opening a listener by constructing a temporary runtime through the
    // normal path and replacing the ephemeral bind port.
    properties.port = 25565;
    properties.world_dir = temp_dir("dedupe");
    let mut runtime = ServerRuntime::new(properties).unwrap();
    runtime
        .handle_join(1, "steve".into())
        .expect("join should be local and deterministic");
    let request = GameplayRequest {
        request_id: 17,
        client_sequence: 1,
        session_id: 1,
        dimension: 0,
        client_revision: 0,
        operation: GameplayOperation::ItemUse { item: crate::inventory::Item::Bread as u32, count: 1 },
    };
    let first = runtime.submit_request(1, request.clone()).unwrap();
    let duplicate = runtime.submit_request(1, request).unwrap();
    assert_eq!(first, duplicate);
    let stale = runtime
        .submit_request(
            1,
            GameplayRequest {
                request_id: 18,
                client_sequence: 1,
                session_id: 1,
                dimension: 0,
                client_revision: 0,
                operation: GameplayOperation::ItemUse { item: crate::inventory::Item::Bread as u32, count: 1 },
            },
        )
        .unwrap();
    assert!(matches!(
        stale.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::OutOfOrder
        }
    ));
    let _ = runtime.shutdown();
    let _ = fs::remove_dir_all(&runtime.world_dir);
}

#[test]
fn headless_two_sessions_share_one_authoritative_sequence() {
    let mut properties = ServerProperties::default();
    properties.bind = "127.0.0.1".into();
    properties.port = 25566;
    properties.world_dir = temp_dir("competition");
    let mut runtime = ServerRuntime::new(properties).unwrap();
    runtime.handle_join(1, "alex".into()).unwrap();
    runtime.handle_join(2, "steve".into()).unwrap();
    let before = runtime
        .authority
        .world(Dimension::Overworld)
        .get_block(8, 80, 8);
    let revision_before = runtime.authority.current_revision(Dimension::Overworld);
    let first = runtime
        .submit_request(
            1,
            GameplayRequest {
                request_id: 1,
                client_sequence: 1,
                session_id: 1,
                dimension: 0,
                client_revision: 0,
                operation: GameplayOperation::BlockAction {
                    action: BlockActionKind::Place,
                    x: 8,
                    y: 80,
                    z: 8,
                    face: [0, 1, 0],
                    hand: 0,
                    held: None,
                    block: 1,
                    look_milli: [0, 0, 1000],
                },
            },
        )
        .unwrap();
    let second = runtime
        .submit_request(
            2,
            GameplayRequest {
                request_id: 2,
                client_sequence: 1,
                session_id: 2,
                dimension: 0,
                client_revision: 0,
                operation: GameplayOperation::BlockAction {
                    action: BlockActionKind::Place,
                    x: 8,
                    y: 80,
                    z: 8,
                    face: [0, 1, 0],
                    hand: 0,
                    held: None,
                    block: 2,
                    look_milli: [0, 0, 1000],
                },
            },
        )
        .unwrap();
    assert!(matches!(
        first.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidState
        }
    ));
    assert!(matches!(
        second.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidState
        }
    ));
    assert_eq!(
        runtime.authority.current_revision(Dimension::Overworld),
        revision_before
    );
    assert_eq!(first.server_sequence, revision_before);
    assert_eq!(second.server_sequence, revision_before);
    assert_eq!(
        runtime
            .authority
            .world(Dimension::Overworld)
            .get_block(8, 80, 8),
        before
    );
    let _ = runtime.shutdown();
    let _ = fs::remove_dir_all(&runtime.world_dir);
}

#[test]
fn dimension_interest_and_session_transfer_are_isolated() {
    let mut properties = ServerProperties::default();
    properties.bind = "127.0.0.1".into();
    properties.port = 25567;
    properties.world_dir = temp_dir("dimension_interest");
    let mut runtime = ServerRuntime::new(properties).unwrap();
    runtime.handle_join(1, "alex".into()).unwrap();
    runtime.handle_join(2, "steve".into()).unwrap();

    runtime.authority.with_world(Dimension::Overworld, |world| {
        assert!(world.ensure_entity(
            101,
            crate::entity::EntityType::Cow,
            [8.0, 80.0, 8.0],
            10.0,
        ));
    });
    runtime.authority.with_world(Dimension::Nether, |world| {
        assert!(world.ensure_entity(
            202,
            crate::entity::EntityType::Piglin,
            [8.0, 80.0, 8.0],
            10.0,
        ));
    });
    assert!(runtime.set_session_dimension(2, Dimension::Nether));
    runtime.update_interest_for(1, Dimension::Overworld, [8.0, 80.0, 8.0]);
    runtime.update_interest_for(2, Dimension::Nether, [8.0, 80.0, 8.0]);
    assert!(runtime.players[&1].interest.entities.contains(&101));
    assert!(!runtime.players[&1].interest.entities.contains(&202));
    assert!(runtime.players[&2].interest.entities.contains(&202));
    assert!(!runtime.players[&2].interest.entities.contains(&101));

    let _ = runtime.shutdown();
    let _ = fs::remove_dir_all(&runtime.world_dir);
}

fn entity_lifecycle_counts(
    events: &[PresentationEvent],
    entity_id: u64,
) -> (usize, usize, usize) {
    let mut spawns = 0;
    let mut states = 0;
    let mut despawns = 0;
    for event in events {
        match event.as_packet_event() {
            Some(ProjectionEvent {
                packet: Packet::EntitySpawn { state, .. },
                ..
            }) if state.entity_id == entity_id => {
                spawns += 1;
            }
            Some(ProjectionEvent {
                packet: Packet::EntityState { state, .. },
                ..
            }) if state.entity_id == entity_id => {
                states += 1;
            }
            Some(ProjectionEvent {
                packet: Packet::EntityDespawn { entity_id: id, .. },
                ..
            }) if *id == entity_id => {
                despawns += 1;
            }
            _ => {}
        }
    }
    (spawns, states, despawns)
}

#[test]
fn entity_state_broadcasts_dirty_or_entered_only() {
    let (mut runtime, _input) = embedded_runtime("entity_dirty");
    let _ = runtime.tick_with_output().unwrap();
    let player_pos = runtime.players[&99].data.position;
    const ENTITY_ID: u64 = 101;
    runtime.authority.with_world(Dimension::Overworld, |world| {
        assert!(world.ensure_entity(
            ENTITY_ID,
            crate::entity::EntityType::EndCrystal,
            player_pos,
            5.0,
        ));
    });

    let entered = runtime.tick_with_output().unwrap();
    let (spawns, states, despawns) =
        entity_lifecycle_counts(&entered.presentation_events, ENTITY_ID);
    assert!(
        spawns + states >= 1,
        "entering the interest set must project a full entity payload"
    );
    assert_eq!(despawns, 0);
    assert!(runtime.players[&99]
        .interest
        .simulation_entities
        .contains(&ENTITY_ID));

    let quiet = runtime.tick_with_output().unwrap();
    let (spawns, states, _) = entity_lifecycle_counts(&quiet.presentation_events, ENTITY_ID);
    assert_eq!(spawns, 0);
    assert_eq!(
        states, 0,
        "stationary pose/health/anim must not re-encode every tick"
    );

    runtime.authority.with_world(Dimension::Overworld, |world| {
        let entity = world.entities.get_by_id_mut(ENTITY_ID).unwrap();
        entity.health = 4.0;
    });
    let dirty = runtime.tick_with_output().unwrap();
    let (_, states, _) = entity_lifecycle_counts(&dirty.presentation_events, ENTITY_ID);
    assert_eq!(states, 1);
    let quiet_after_dirty = runtime.tick_with_output().unwrap();
    let (_, states, _) =
        entity_lifecycle_counts(&quiet_after_dirty.presentation_events, ENTITY_ID);
    assert_eq!(states, 0);

    let far = [player_pos[0] + 10_000.0, player_pos[1], player_pos[2]];
    assert!(runtime.teleport_session(99, far));
    let left: Vec<_> = runtime.presentation_events.drain(..).collect();
    let (_, _, despawns) = entity_lifecycle_counts(&left, ENTITY_ID);
    assert!(despawns >= 1);
    assert!(!runtime.players[&99]
        .interest
        .simulation_entities
        .contains(&ENTITY_ID));

    assert!(runtime.teleport_session(99, player_pos));
    let reentered: Vec<_> = runtime.presentation_events.drain(..).collect();
    let (spawns, _, _) = entity_lifecycle_counts(&reentered, ENTITY_ID);
    assert!(
        spawns >= 1,
        "re-entering view distance must send a full EntitySpawn"
    );
    assert!(runtime.players[&99]
        .last_projected_entity_states
        .get(&ENTITY_ID)
        .is_none());
    let reentered_tick = runtime.tick_with_output().unwrap();
    let (spawns, states, _) =
        entity_lifecycle_counts(&reentered_tick.presentation_events, ENTITY_ID);
    assert_eq!(spawns, 0);
    assert_eq!(
        states, 1,
        "re-entering the simulation set must send a full EntityState once"
    );
    let quiet_reentered = runtime.tick_with_output().unwrap();
    let (_, states, _) =
        entity_lifecycle_counts(&quiet_reentered.presentation_events, ENTITY_ID);
    assert_eq!(states, 0);

    let world_dir = runtime.world_dir.clone();
    runtime.shutdown().unwrap();
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn respawn_updates_authority_dimension_and_position() {
    let mut properties = ServerProperties::default();
    properties.bind = "127.0.0.1".into();
    properties.port = 25571;
    properties.world_dir = temp_dir("respawn_dimension");
    let mut runtime = ServerRuntime::new(properties).unwrap();
    runtime.handle_join(1, "alex".into()).unwrap();
    assert!(runtime.set_session_dimension(1, Dimension::Nether));
    runtime.players.get_mut(&1).unwrap().data.is_dead = true;
    runtime.authority.session_mut(1).unwrap().gameplay.is_dead = true;
    runtime
        .authority
        .session_mut(1)
        .unwrap()
        .gameplay
        .health_milli = 0;
    runtime
        .handle_event(ServerToHost::ClientRespawnRequest { id: 1 })
        .unwrap();
    let player = runtime.players.get(&1).unwrap();
    assert_eq!(player.interest.dimension, runtime.level.spawn_dimension);
    let authority_session = runtime.authority.session(1).unwrap();
    assert_eq!(authority_session.dimension, player.interest.dimension as u8);
    assert_eq!(authority_session.position, player.data.position);
    assert!(!authority_session.gameplay.is_dead);
    assert_eq!(
        authority_session.gameplay.health_milli,
        authority_session.gameplay.max_health_milli
    );
    assert_eq!(authority_session.gameplay.hunger_milli, 20_000);

    let _ = runtime.shutdown();
    let _ = fs::remove_dir_all(&runtime.world_dir);
}

#[test]
fn authority_gameplay_round_trips_through_dedicated_player_save() {
    let mut properties = ServerProperties::default();
    properties.bind = "127.0.0.1".into();
    properties.port = 25572;
    properties.world_dir = temp_dir("gameplay_roundtrip");
    let world_dir = properties.world_dir.clone();
    let mut runtime = ServerRuntime::new(properties.clone()).unwrap();
    runtime.handle_join(1, "alex".into()).unwrap();
    assert!(runtime.set_session_dimension(1, Dimension::Nether));

    let mut wire = ItemWire::empty();
    wire.item = crate::inventory::Item::DiamondSword as u32;
    wire.count = 1;
    wire.durability = 37;
    // ItemWire stores kind/level in the protocol's packed representation.
    // Keep the fixture canonical and avoid the Silk Touch/Fortune
    // incompatibility enforced by EnchantmentSet::add_or_upgrade, so the
    // save round-trip can assert both wire and semantic metadata.
    wire.enchantments = [0x15, 0x23, 0x31, 0x55, 0x62, 0];
    wire.custom_name = [b'R'; 24];
    wire.can_break = 0x11;
    wire.can_place_on = 0x22;
    let mut gameplay = SessionGameplayState::default();
    gameplay.health_milli = 12_345;
    gameplay.hunger_milli = 8_765;
    gameplay.saturation_milli = 1_250;
    gameplay.inventory[0] = Some(SessionInventorySlot::from_wire(wire, 0x11, 0x22));
    let authority_session = runtime.authority.session_mut(1).unwrap();
    authority_session.game_mode = GameMode::Adventure;
    authority_session.gameplay = gameplay;
    runtime.authority.with_world(Dimension::Overworld, |world| {
        world
            .set_block(12, 80, 12, crate::world::BlockType::Glass, 0)
            .unwrap();
    });
    runtime.authority.with_world(Dimension::Nether, |world| {
        world
            .set_block(12, 80, 12, crate::world::BlockType::Obsidian, 0)
            .unwrap();
    });
    runtime.save_all().unwrap();
    runtime.shutdown().unwrap();

    let mut restored = ServerRuntime::new(properties).unwrap();
    restored.handle_join(2, "alex".into()).unwrap();
    let authority_session = restored.authority.session(2).unwrap();
    assert_eq!(authority_session.dimension, Dimension::Nether as u8);
    assert_eq!(authority_session.game_mode, GameMode::Adventure);
    assert_eq!(authority_session.gameplay.health_milli, 12_345);
    assert_eq!(authority_session.gameplay.hunger_milli, 8_765);
    assert_eq!(authority_session.gameplay.saturation_milli, 1_250);
    let saved_slot = authority_session.gameplay.inventory[0].unwrap();
    assert_eq!(saved_slot.item, wire);
    assert_eq!(saved_slot.can_break, 0x11);
    assert_eq!(saved_slot.can_place_on, 0x22);
    let roundtrip_stack = saved_slot.item.to_stack().unwrap();
    assert_eq!(
        roundtrip_stack
            .enchantments
            .level_of(crate::enchantment::Enchantment::Efficiency(1)),
        5
    );
    assert_eq!(
        roundtrip_stack
            .enchantments
            .level_of(crate::enchantment::Enchantment::Unbreaking(1)),
        3
    );
    assert_eq!(
        roundtrip_stack
            .enchantments
            .level_of(crate::enchantment::Enchantment::SilkTouch),
        1
    );
    assert_eq!(
        roundtrip_stack
            .enchantments
            .level_of(crate::enchantment::Enchantment::Sharpness(1)),
        5
    );
    assert_eq!(
        roundtrip_stack
            .enchantments
            .level_of(crate::enchantment::Enchantment::Knockback(1)),
        2
    );
    assert_eq!(
        restored
            .authority
            .world_ref(Dimension::Overworld)
            .unwrap()
            .get_block(12, 80, 12),
        crate::world::BlockType::Glass
    );
    assert_eq!(
        restored
            .authority
            .world_ref(Dimension::Nether)
            .unwrap()
            .get_block(12, 80, 12),
        crate::world::BlockType::Obsidian
    );

    let _ = restored.shutdown();
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn disconnect_reconnect_loads_atomic_player_state() {
    let mut properties = ServerProperties::default();
    properties.bind = "127.0.0.1".into();
    properties.port = 25568;
    properties.world_dir = temp_dir("reconnect");
    let mut runtime = ServerRuntime::new(properties).unwrap();
    runtime.handle_join(1, "alex".into()).unwrap();
    runtime.authority.session_mut(1).unwrap().position = [12.0, 70.0, -4.0];
    runtime.players.get_mut(&1).unwrap().last_pose_position = [12.0, 70.0, -4.0];
    runtime.players.get_mut(&1).unwrap().data.health = 7.5;
    // Gameplay is authority-owned after join; keep the fixture's health
    // mutation on the authoritative session rather than the projection.
    runtime
        .authority
        .session_mut(1)
        .unwrap()
        .gameplay
        .health_milli = 7_500;
    runtime.handle_leave(1).unwrap();
    runtime.handle_join(2, "alex".into()).unwrap();
    let restored = &runtime.players.get(&2).unwrap().data;
    assert_eq!(restored.position, [12.0, 70.0, -4.0]);
    assert_eq!(restored.health, 7.5);
    let _ = runtime.shutdown();
    let _ = fs::remove_dir_all(&runtime.world_dir);
}

#[test]
fn save_failure_is_reported_and_retry_keeps_original_path() {
    let mut properties = ServerProperties::default();
    properties.bind = "127.0.0.1".into();
    properties.port = 25569;
    properties.world_dir = temp_dir("save_failure");
    let mut runtime = ServerRuntime::new(properties).unwrap();
    let world_dir = runtime.world_dir.clone();
    let blocking_file = world_dir.with_extension("blocked");
    fs::write(&blocking_file, b"not a directory").unwrap();
    runtime.world_dir = blocking_file.clone();
    runtime.save_manager.world_dir = blocking_file.clone();
    let saves_before_failure = runtime.metrics.saves;
    assert!(runtime.save_all().is_err());
    assert_eq!(runtime.metrics.saves, saves_before_failure);
    runtime.world_dir = world_dir.clone();
    runtime.save_manager.world_dir = world_dir.clone();
    assert!(runtime.save_all().is_ok());
    assert_eq!(runtime.metrics.saves, saves_before_failure + 1);
    assert!(runtime.metrics.last_save_latency_ms >= 1);
    let _ = runtime.shutdown();
    let _ = fs::remove_dir_all(&world_dir);
    let _ = fs::remove_file(blocking_file);
}

#[test]
fn motd_validate_applies_cli_256_byte_cap() {
    let mut properties = ServerProperties::default();
    properties.motd = "x".repeat(256);
    properties.validate().unwrap();
    properties.motd = "x".repeat(257);
    let error = properties.validate().unwrap_err();
    assert!(error.to_string().contains("motd"));
    assert!(error.to_string().contains("1..=256"));
    properties.motd = "   ".into();
    assert!(properties
        .validate()
        .unwrap_err()
        .to_string()
        .contains("motd"));
}

#[test]
fn online_mode_true_fails_closed_at_validate_and_startup() {
    let mut properties = ServerProperties::default();
    properties.online_mode = true;
    let error = properties.validate().unwrap_err();
    let message = error.to_string();
    assert!(message.contains("online-mode"));
    assert!(
        message.contains("尚未實作驗證，拒絕當憑證開關"),
        "{message}"
    );
    assert!(message.contains("not implemented"), "{message}");

    let dir = temp_dir("online_mode");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("server.properties");
    fs::write(&path, "online-mode=true\n").unwrap();
    let loaded = ServerProperties::load(&path).unwrap_err();
    assert!(loaded.to_string().contains("online-mode"));
    properties.world_dir = dir.join("world");
    properties.bind = "127.0.0.1".into();
    assert!(ServerRuntime::new(properties).is_err());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn login_grants_operator_only_from_console_op_set() {
    let mut properties = ServerProperties::default();
    properties.bind = "127.0.0.1".into();
    properties.port = 25573;
    properties.world_dir = temp_dir("op_policy");
    let mut runtime = ServerRuntime::new(properties).unwrap();

    runtime.login_session(1, "Alice").unwrap();
    assert!(!runtime.authority.session(1).unwrap().operator);
    assert_eq!(runtime.authority.session(1).unwrap().username, "alice");
    runtime.logout_session(1).unwrap();

    runtime.execute_console_command("op Alice").unwrap();
    assert!(runtime.properties.operators.contains("alice"));
    assert!(runtime.execute_console_command("op foo.bar").is_err());
    assert!(runtime.execute_console_command("op CON").is_err());
    assert!(!runtime.properties.operators.contains("foo_bar"));

    runtime.login_session(2, "ALICE").unwrap();
    assert!(runtime.authority.session(2).unwrap().operator);
    runtime.login_session(3, "bob").unwrap();
    assert!(!runtime.authority.session(3).unwrap().operator);
    assert!(runtime.login_session(4, "foo.bar").is_err());
    assert!(runtime.login_session(5, "CON").is_err());

    let _ = runtime.shutdown();
    let _ = fs::remove_dir_all(&runtime.world_dir);
}

#[test]
fn autosave_error_does_not_skip_shutdown_flush() {
    let mut properties = ServerProperties::default();
    properties.bind = "127.0.0.1".into();
    properties.port = 25574;
    properties.world_dir = temp_dir("autosave_flush");
    let mut runtime = ServerRuntime::new(properties).unwrap();
    runtime.login_session(1, "alex").unwrap();
    let saves_before = runtime.metrics.saves;

    SAVE_ALL_FAILPOINT.with(|failpoint| failpoint.set(true));
    runtime.metrics.ticks = AUTOSAVE_INTERVAL_TICKS - 1;
    assert!(runtime.tick().is_ok());
    assert_eq!(runtime.metrics.saves, saves_before);
    SAVE_ALL_FAILPOINT.with(|failpoint| failpoint.set(false));

    runtime.request_shutdown();
    assert!(runtime.is_stopped());
    runtime.shutdown().unwrap();
    assert!(runtime.metrics.saves > saves_before);
    assert!(runtime.save_flushed);

    let world_dir = runtime.world_dir.clone();
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn leave_save_failure_still_releases_identity() {
    let mut properties = ServerProperties::default();
    properties.bind = "127.0.0.1".into();
    properties.port = 25575;
    properties.world_dir = temp_dir("leave_save_fail");
    let mut runtime = ServerRuntime::new(properties).unwrap();
    runtime.login_session(1, "alex").unwrap();

    SAVE_PLAYER_FAILPOINT.with(|failpoint| failpoint.set(true));
    runtime.logout_session(1).unwrap();
    SAVE_PLAYER_FAILPOINT.with(|failpoint| failpoint.set(false));

    assert!(runtime.authority.session(1).is_none());
    assert!(!runtime.players.contains_key(&1));
    runtime.login_session(2, "ALEX").unwrap();
    assert_eq!(runtime.authority.session(2).unwrap().username, "alex");

    let _ = runtime.shutdown();
    let _ = fs::remove_dir_all(&runtime.world_dir);
}

#[test]
fn interest_evict_drops_origin_after_long_walk_and_metrics_match() {
    let (mut runtime, _input) = embedded_runtime("residency_walk");
    runtime.run_for_ticks(8).unwrap();
    assert!(runtime
        .authority
        .world_mut(Dimension::Overworld)
        .unwrap()
        .chunks
        .chunks
        .contains_key(&(0, 0)));
    assert!(!runtime
        .authority
        .world_mut(Dimension::Overworld)
        .unwrap()
        .chunks
        .chunks
        .contains_key(&(8, 0)));
    assert!(runtime.teleport_session(99, [32.0 * 16.0 + 8.0, 80.0, 8.0]));
    runtime.tick().unwrap();
    assert!(!runtime
        .authority
        .world_mut(Dimension::Overworld)
        .unwrap()
        .chunks
        .chunks
        .contains_key(&(0, 0)));
    let loaded: usize = runtime
        .authority
        .dimensions()
        .filter_map(|dimension| runtime.authority.world_ref(dimension))
        .map(|world| world.chunks.chunks.len())
        .sum();
    assert_eq!(runtime.metrics.loaded_chunks, loaded);
    let _ = runtime.shutdown();
    let _ = fs::remove_dir_all(&runtime.world_dir);
}

#[test]
fn stationary_session_skips_chunk_interest_rebuild_across_ticks() {
    let (mut runtime, _input) = embedded_runtime("stationary_interest");
    runtime.run_for_ticks(4).unwrap();
    let rebuilds_before = runtime
        .players
        .get(&99)
        .map(|session| session.interest.chunk_rebuilds())
        .expect("local session");
    let chunks_before = runtime
        .players
        .get(&99)
        .map(|session| session.interest.chunks.clone())
        .expect("local session");
    let sim_before = runtime
        .players
        .get(&99)
        .map(|session| session.interest.simulation_chunks.clone())
        .expect("local session");

    runtime.tick().unwrap();
    runtime.tick().unwrap();

    let session = runtime.players.get(&99).expect("local session");
    assert_eq!(
        session.interest.chunk_rebuilds(),
        rebuilds_before,
        "stationary ticks must not rebuild chunk HashSets"
    );
    assert_eq!(session.interest.chunks, chunks_before);
    assert_eq!(session.interest.simulation_chunks, sim_before);

    let _ = runtime.shutdown();
    let _ = fs::remove_dir_all(&runtime.world_dir);
}

#[test]
fn chunk_interest_index_tracks_join_move_and_leave() {
    let (mut runtime, _input) = embedded_runtime("chunk_interest_index");
    runtime.run_for_ticks(2).unwrap();

    let origin = (0i32, 0i32);
    assert!(
        runtime
            .chunk_interest_index
            .get(&(Dimension::Overworld, origin))
            .is_some_and(|set| set.contains(&99)),
        "join must register the local session in the reverse index"
    );
    let indexed_before = runtime.chunk_interest_index.len();
    assert!(indexed_before > 0);

    assert!(runtime.teleport_session(99, [32.0 * 16.0 + 8.0, 80.0, 8.0]));
    runtime.tick().unwrap();
    assert!(
        runtime
            .chunk_interest_index
            .get(&(Dimension::Overworld, origin))
            .map_or(true, |set| !set.contains(&99)),
        "departed origin column must drop the session from the reverse index"
    );
    assert!(
        runtime
            .chunk_interest_index
            .get(&(Dimension::Overworld, (32, 0)))
            .is_some_and(|set| set.contains(&99)),
        "entered column after teleport must list the session"
    );

    runtime.logout_session(99).unwrap();
    assert!(
        runtime
            .chunk_interest_index
            .values()
            .all(|set| !set.contains(&99)),
        "leave must clear every reverse-index membership"
    );

    let _ = runtime.shutdown();
    let _ = fs::remove_dir_all(&runtime.world_dir);
}

#[test]
fn empty_dimension_keeps_only_capped_spawn_ring() {
    let mut properties = ServerProperties::default();
    properties.bind = "127.0.0.1".into();
    properties.port = 25581;
    properties.view_distance = 2;
    properties.simulation_distance = 2;
    properties.world_dir = temp_dir("spawn_ring");
    let (mut runtime, _input) = ServerRuntime::new_embedded(
        properties,
        EmbeddedRuntimeOptions {
            transport: TransportMode::Disabled,
            local_session: None,
        },
    )
    .unwrap();
    runtime.authority.with_world(Dimension::Overworld, |world| {
        world.materialize_chunk(0, 0);
        world.materialize_chunk(8, 0);
        world.materialize_chunk(-3, 2);
    });
    runtime.tick().unwrap();
    let world = runtime.authority.world_ref(Dimension::Overworld).unwrap();
    assert!(world.chunks.chunks.contains_key(&(0, 0)));
    assert!(!world.chunks.chunks.contains_key(&(8, 0)));
    assert!(world.chunks.chunks.len() <= crate::authority::interest::SPAWN_RESIDENCY_CAP);
    let keep = crate::authority::interest::capped_spawn_residency(
        runtime.level.spawn_x,
        runtime.level.spawn_z,
    );
    for key in world.chunks.chunks.keys() {
        assert!(keep.contains(&key));
    }
    let _ = runtime.shutdown();
    let _ = fs::remove_dir_all(&runtime.world_dir);
}

#[test]
fn save_all_persists_only_dirty_resident_columns() {
    let mut properties = ServerProperties::default();
    properties.bind = "127.0.0.1".into();
    properties.port = 25584;
    properties.world_dir = temp_dir("dirty_only_autosave");
    let mut runtime = ServerRuntime::new(properties).unwrap();
    runtime.authority.with_world(Dimension::Overworld, |world| {
        world.materialize_chunk(0, 0);
        world.materialize_chunk(1, 0);
        world
            .set_block(8, 80, 8, crate::world::BlockType::DiamondOre, 0)
            .unwrap();
    });
    runtime.save_all().unwrap();
    let world_dir = runtime.world_dir.clone();
    runtime.shutdown().unwrap();

    let mut manager = SaveManager::new(&world_dir);
    let saved = manager
        .load_chunk(0, 0)
        .expect("dirty mutated column must persist");
    let mut restored = crate::world::Chunk::empty_in_dimension(Dimension::Overworld, 0, 0);
    saved.restore_to_chunk(&mut restored).unwrap();
    assert_eq!(
        restored.get_block_local(8, 80, 8),
        crate::world::BlockType::DiamondOre
    );
    assert!(
        manager.load_chunk(1, 0).is_none(),
        "unmodified generated columns must not be rewritten on autosave"
    );
    let _ = fs::remove_dir_all(world_dir);
}
