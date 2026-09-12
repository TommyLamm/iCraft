// Tests extracted from client.rs (Plan 27).

use super::*;
use crate::network::server::{HostToServer, NetworkServer, ServerToHost};
use std::net::TcpListener as StdTcpListener;
use std::sync::{mpsc, Mutex, MutexGuard, OnceLock};

fn network_test_guard() -> MutexGuard<'static, ()> {
    static GUARD: OnceLock<Mutex<()>> = OnceLock::new();
    GUARD
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn wait_for_event(rx: &Receiver<ClientToGame>) -> Packet {
    loop {
        let event = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("client event timed out");
        match event {
            ClientToGame::StatusUpdate { .. } => {}
            ClientToGame::Packet(Packet::PlayerSessionUpdate { .. }) => {}
            ClientToGame::Packet(packet) => return packet,
        }
    }
}

fn unwrap_packet(event: ClientToGame) -> Packet {
    match event {
        ClientToGame::Packet(packet) => packet,
        other => panic!("expected Packet inbound, got {other:?}"),
    }
}

#[test]
fn fresh_gameplay_input_rebases_revision_but_explicit_stale_input_does_not() {
    let operation = crate::network::protocol::GameplayOperation::Fishing {
        action: 1,
        hand: 0,
        look_milli: [0, 0, 1_000],
    };
    let mut next_request_id = 10;
    let mut last_client_sequence = 4;
    let mut last_client_revision = 23;

    let fresh = prepare_gameplay_request(
        GameplayRequest {
            request_id: 77,
            client_sequence: 0,
            session_id: 0,
            dimension: 0,
            client_revision: 7,
            operation: operation.clone(),
        },
        99,
        &mut next_request_id,
        &mut last_client_sequence,
        &mut last_client_revision,
    );
    assert_eq!(fresh.request_id, 77);
    assert_eq!(fresh.client_sequence, 5);
    assert_eq!(fresh.client_revision, 23);
    assert_eq!(fresh.session_id, 99);
    assert_eq!(last_client_revision, 23);

    let stale = prepare_gameplay_request(
        GameplayRequest {
            request_id: 78,
            client_sequence: 6,
            session_id: 0,
            dimension: 0,
            client_revision: 2,
            operation,
        },
        99,
        &mut next_request_id,
        &mut last_client_sequence,
        &mut last_client_revision,
    );
    assert_eq!(stale.client_sequence, 6);
    assert_eq!(stale.client_revision, 2);
    assert_eq!(last_client_revision, 23);

    let newer_fresh = prepare_gameplay_request(
        GameplayRequest {
            request_id: 79,
            client_sequence: 0,
            session_id: 0,
            dimension: 0,
            client_revision: 31,
            operation: crate::network::protocol::GameplayOperation::ItemUse {
                item: crate::inventory::Item::Bread as u32,
                count: 1,
            },
        },
        99,
        &mut next_request_id,
        &mut last_client_sequence,
        &mut last_client_revision,
    );
    assert_eq!(newer_fresh.client_sequence, 7);
    assert_eq!(newer_fresh.client_revision, 31);
    assert_eq!(last_client_revision, 31);
}

#[test]
fn connects_and_receives_join_for_second_client() {
    let _guard = network_test_guard();
    let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
    let addr = reserved.local_addr().unwrap().to_string();
    drop(reserved);
    let (host_tx, host_rx) = tokio::sync::mpsc::channel(128);
    let (server_tx, server_rx) = mpsc::channel();
    let server = NetworkServer::spawn(addr.clone(), 0xCAFE_BABE, 1, host_rx, server_tx);

    let (game_tx_a, game_rx_a) = mpsc::channel();
    let (event_tx_a, event_rx_a) = mpsc::sync_channel(CLIENT_TO_GAME_QUEUE_CAPACITY);
    let client_a = NetworkClient::spawn(addr.clone(), "steve".into(), game_rx_a, event_tx_a);
    let first = wait_for_event(&event_rx_a);
    let first_id = match first {
        Packet::LoginSuccess {
            player_id,
            seed,
            gamemode,
            ..
        } => {
            assert_eq!(seed, 0xCAFE_BABE);
            assert_eq!(gamemode, 1);
            player_id
        }
        other => panic!("expected Connected, got {other:?}"),
    };
    let _ = server_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("first join missing");

    let (game_tx_b, game_rx_b) = mpsc::channel();
    let (event_tx_b, event_rx_b) = mpsc::sync_channel(CLIENT_TO_GAME_QUEUE_CAPACITY);
    let client_b = NetworkClient::spawn(addr, "alex".into(), game_rx_b, event_tx_b);
    let second_id = match wait_for_event(&event_rx_b) {
        Packet::LoginSuccess { player_id, .. } => player_id,
        other => panic!("expected second Connected, got {other:?}"),
    };
    let second_join = server_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("second join missing");
    let username = match second_join {
        ServerToHost::ClientJoined { id, username } => {
            assert_eq!(id, second_id);
            username
        }
        other => panic!("expected ClientJoined, got {other:?}"),
    };
    host_tx
        .try_send(HostToServer::project_broadcast(Packet::PlayerJoin {
                id: second_id,
                username: username,
        }))
        .unwrap();
    assert!(matches!(
        wait_for_event(&event_rx_a),
        Packet::PlayerJoin { id, username, .. } if id == second_id && username == "alex"
    ));

    game_tx_a.send(GameToClient::Disconnect).unwrap();
    game_tx_b.send(GameToClient::Disconnect).unwrap();
    let _ = client_a.join();
    let _ = client_b.join();
    host_tx.try_send(HostToServer::Stop).unwrap();
    let _ = server.join();
    assert_ne!(first_id, second_id);
}

#[test]
fn receives_targeted_chunk_catchup_and_time_sync() {
    let _guard = network_test_guard();
    let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
    let addr = reserved.local_addr().unwrap().to_string();
    drop(reserved);
    let (host_tx, host_rx) = tokio::sync::mpsc::channel(128);
    let (server_tx, server_rx) = mpsc::channel();
    let server = NetworkServer::spawn(addr.clone(), 1234, 1, host_rx, server_tx);

    let (game_tx, game_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::sync_channel(CLIENT_TO_GAME_QUEUE_CAPACITY);
    let client = NetworkClient::spawn(addr, "catchup".into(), game_rx, event_tx);
    let player_id = match wait_for_event(&event_rx) {
        Packet::LoginSuccess { player_id, .. } => player_id,
        other => panic!("expected Connected, got {other:?}"),
    };
    let _ = server_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("join event missing");
    std::thread::sleep(Duration::from_millis(50));

    host_tx
        .try_send(HostToServer::project_broadcast(Packet::BlockChange {
                dimension: 0,
                revision: 100,
                x: 7,
                y: 80,
                z: -9,
                block: 3,
                state: 0,
                raw_fluid: 0,
        }))
        .unwrap();
    host_tx
        .try_send(HostToServer::project_session(
            player_id,
            Packet::ChunkData {
                dimension: 0,
                cx: 0,
                cz: -1,
                revision: 99,
                min_section_y: 0,
                section_count: 16,
                blocks: vec![1, 2, 3, 4],
                block_states: vec![0, 0, 0, 0],
                fluid_levels: vec![],
                block_entities: vec![],
            },
        ))
        .unwrap();
    host_tx
        .try_send(HostToServer::project_broadcast(Packet::TimeSync {
                ticks: 19_000,
                weather: 2,
                weather_remaining_ticks: 8_000.5,
        }))
        .unwrap();
    let strike = LightningStrike {
        x: -7,
        y: 90,
        z: 13,
        visual_seed: 77,
    };
    host_tx
        .try_send(HostToServer::project_broadcast(Packet::LightningStrike {
                strike: strike,
        }))
        .unwrap();

    let mut events = Vec::new();
    for _ in 0..10 {
        if let Ok(event) = event_rx.recv_timeout(Duration::from_secs(2)) {
            if matches!(
                event,
                ClientToGame::Packet(Packet::BlockChange { .. })
                    | ClientToGame::Packet(Packet::ChunkData { .. })
                    | ClientToGame::Packet(Packet::TimeSync { .. })
                    | ClientToGame::Packet(Packet::LightningStrike { .. })
            ) {
                events.push(event);
            }
        }
        if events.len() >= 4 {
            break;
        }
    }

    assert!(events.iter().any(|e| matches!(
        e,
        ClientToGame::Packet(Packet::BlockChange {
            x: 7,
            y: 80,
            z: -9,
            block: 3,
            state: 0,
            ..
        })
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        ClientToGame::Packet(Packet::ChunkData { cx: 0, cz: -1, blocks, block_states: _, .. }) if blocks == &vec![1, 2, 3, 4]
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        ClientToGame::Packet(Packet::TimeSync {
            ticks: 19_000,
            weather: 2,
            weather_remaining_ticks: 8_000.5,
            ..
        })
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        ClientToGame::Packet(Packet::LightningStrike { strike: received, .. }) if *received == strike
    )));

    game_tx.send(GameToClient::Disconnect).unwrap();
    client.join().unwrap();
    host_tx.try_send(HostToServer::Stop).unwrap();
    server.join().unwrap();
}

#[test]
fn tcp_capacity_one_retries_without_starving_second_client_and_converges() {
    let _guard = network_test_guard();
    fn checksum(chunk: &crate::world::Chunk) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for x in 0..16 {
            for y in chunk.world_y_range() {
                for z in 0..16 {
                    hash ^= chunk.get_block_local(x, y, z) as u8 as u64;
                    hash = hash.wrapping_mul(0x100_0000_01b3);
                }
            }
        }
        hash
    }

    fn receive_converged_chunk(
        rx: &Receiver<ClientToGame>,
        expected_payload: &crate::save::ChunkSaveData,
    ) -> crate::world::Chunk {
        let (blocks, block_states) = loop {
            match wait_for_event(rx) {
                Packet::ChunkData {
                    dimension: 0,
                    cx: 0,
                    cz: 0,
                    revision: 1,
                    blocks,
                    block_states,
                    block_entities: _,
                    ..
                } => break (blocks, block_states),
                Packet::PlayerJoin { .. } => {}
                other => panic!("expected persisted chunk snapshot, got {other:?}"),
            }
        };
        assert_eq!(blocks, expected_payload.blocks);
        assert_eq!(block_states, expected_payload.block_states);

        let mut chunk = crate::world::Chunk::new(0, 0);
        crate::save::ChunkSaveData {
            chunk_x: 0,
            chunk_z: 0,
            blocks,
            sky_light: Vec::new(),
            block_light: Vec::new(),
            fluid_levels: Vec::new(),
            redstone_metadata: Vec::new(),
            block_states,
            mutation_revision: 1,
            block_entities: Vec::new(),
            data_version: 0,
        }
        .restore_to_chunk(&mut chunk)
        .unwrap();

        match wait_for_event(rx) {
            Packet::BlockChange {
                dimension: 0,
                revision: 2,
                x: 2,
                y: 70,
                z: 2,
                block,
                ..
            } => chunk.set_block_local(
                2,
                70,
                2,
                crate::world::BlockType::from_wire(block).unwrap(),
            ),
            other => panic!("expected revision-2 block change, got {other:?}"),
        }
        chunk
    }

    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let world_dir = std::env::temp_dir().join(format!(
        "icraft_network_persisted_catchup_{}_{}",
        std::process::id(),
        unique
    ));
    let mut source_chunk = crate::world::Chunk::new(0, 0);
    source_chunk.set_block_local(1, 70, 1, crate::world::BlockType::Stone);
    let mut persisted = crate::save::ChunkSaveData::from_chunk(&source_chunk).unwrap();
    persisted.mutation_revision = 1;
    crate::save::SaveManager::new(&world_dir)
        .save_chunk(0, 0, persisted)
        .unwrap();
    let persisted = crate::save::SaveManager::new(&world_dir)
        .load_chunk(0, 0)
        .expect("persisted, currently-unloaded chunk must be reloadable");

    let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
    let addr = reserved.local_addr().unwrap().to_string();
    drop(reserved);
    let (host_tx, host_rx) = tokio::sync::mpsc::channel(128);
    let (server_tx, server_rx) = mpsc::channel();
    let server = NetworkServer::spawn_for_test(
        addr.clone(),
        1234,
        1,
        host_rx,
        server_tx,
        1,
        Duration::from_millis(200),
    );

    let (game_tx_a, game_rx_a) = mpsc::channel();
    let (event_tx_a, event_rx_a) = mpsc::sync_channel(CLIENT_TO_GAME_QUEUE_CAPACITY);
    let client_a = NetworkClient::spawn(addr.clone(), "slow".into(), game_rx_a, event_tx_a);
    let id_a = match wait_for_event(&event_rx_a) {
        Packet::LoginSuccess { player_id, .. } => player_id,
        other => panic!("expected first Connected, got {other:?}"),
    };
    assert!(matches!(
        server_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        ServerToHost::ClientJoined { id, .. } if id == id_a
    ));

    let (game_tx_b, game_rx_b) = mpsc::channel();
    let (event_tx_b, event_rx_b) = mpsc::sync_channel(CLIENT_TO_GAME_QUEUE_CAPACITY);
    let client_b = NetworkClient::spawn(addr, "fast".into(), game_rx_b, event_tx_b);
    let id_b = match wait_for_event(&event_rx_b) {
        Packet::LoginSuccess { player_id, .. } => player_id,
        other => panic!("expected second Connected, got {other:?}"),
    };
    assert!(matches!(
        server_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        ServerToHost::ClientJoined { id, .. } if id == id_b
    ));

    host_tx
        .try_send(HostToServer::project_broadcast(Packet::BlockChange {
                dimension: 0,
                revision: 2,
                x: 2,
                y: 70,
                z: 2,
                block: crate::world::BlockType::Dirt.to_wire(),
                state: 0,
                raw_fluid: 0,
        }))
        .unwrap();
    let snapshot = |to, cx, blocks, block_states| HostToServer::project_session(
            to,
            Packet::ChunkData {
                dimension: 0,
                cx: cx,
                cz: 0,
                revision: 1,
                min_section_y: 0,
                section_count: 16,
                blocks: blocks,
                block_states: block_states,
                fluid_levels: vec![],
                block_entities: vec![],
            },
        );
    // The host/state priority selector submits the near chunk first. The
    // transport must preserve it while reporting, rather than dropping,
    // the farther chunk when this client's capacity-one mailbox is full.
    host_tx
        .try_send(snapshot(
            id_a,
            0,
            persisted.blocks.clone(),
            persisted.block_states.clone(),
        ))
        .unwrap();
    host_tx
        .try_send(snapshot(id_a, 8, vec![8], vec![0]))
        .unwrap();
    host_tx
        .try_send(snapshot(
            id_b,
            0,
            persisted.blocks.clone(),
            persisted.block_states.clone(),
        ))
        .unwrap();

    let converged_a = receive_converged_chunk(&event_rx_a, &persisted);
    let converged_b = receive_converged_chunk(&event_rx_b, &persisted);
    let mut expected = source_chunk;
    expected.set_block_local(2, 70, 2, crate::world::BlockType::Dirt);
    assert_eq!(checksum(&converged_a), checksum(&expected));
    assert_eq!(checksum(&converged_b), checksum(&expected));

    host_tx
        .try_send(snapshot(id_a, 8, vec![8], vec![0]))
        .unwrap();
    assert!(matches!(
        wait_for_event(&event_rx_a),
        Packet::ChunkData {
            cx: 8,
            revision: 1,
            blocks,
            ..
        } if blocks == vec![8]
    ));

    game_tx_a.send(GameToClient::Disconnect).unwrap();
    game_tx_b.send(GameToClient::Disconnect).unwrap();
    client_a.join().unwrap();
    client_b.join().unwrap();
    host_tx.try_send(HostToServer::Stop).unwrap();
    server.join().unwrap();
    std::fs::remove_dir_all(world_dir).unwrap();
}

#[test]
fn tcp_cross_channel_revision_gate_and_reliable_control_are_fifo() {
    let _guard = network_test_guard();
    let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
    let addr = reserved.local_addr().unwrap().to_string();
    drop(reserved);
    let (host_tx, host_rx) = tokio::sync::mpsc::channel(128);
    let (server_tx, server_rx) = mpsc::channel();
    let server = NetworkServer::spawn(addr.clone(), 1234, 1, host_rx, server_tx);
    let (game_tx, game_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::sync_channel(CLIENT_TO_GAME_QUEUE_CAPACITY);
    let client = NetworkClient::spawn(addr, "ordering".into(), game_rx, event_tx);
    let player_id = match wait_for_event(&event_rx) {
        Packet::LoginSuccess { player_id, .. } => player_id,
        other => panic!("expected Connected, got {other:?}"),
    };
    assert!(matches!(
        server_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        ServerToHost::ClientJoined { id, .. } if id == player_id
    ));

    host_tx
        .try_send(HostToServer::project_broadcast(Packet::BlockChange {
                dimension: 0,
                revision: 2,
                x: 1,
                y: 70,
                z: 1,
                block: crate::world::BlockType::Dirt.to_wire(),
                state: 0,
                raw_fluid: 0,
        }))
        .unwrap();
    host_tx
        .try_send(HostToServer::project_broadcast(Packet::ChatMessage {
                sender: "host".into(),
                message: "first".into(),
        }))
        .unwrap();
    host_tx
        .try_send(HostToServer::project_broadcast(Packet::TimeSync {
                ticks: 42,
                weather: 1,
                weather_remaining_ticks: 99.0,
        }))
        .unwrap();
    host_tx
        .try_send(HostToServer::project_broadcast(Packet::ChatMessage {
                sender: "host".into(),
                message: "second".into(),
        }))
        .unwrap();
    host_tx
        .try_send(HostToServer::project_session(
            player_id,
            Packet::ChunkData {
                dimension: 0,
                cx: 0,
                cz: 0,
                revision: 1,
                min_section_y: 0,
                section_count: 16,
                blocks: vec![1],
                block_states: vec![0],
                fluid_levels: vec![],
                block_entities: vec![],
            },
        ))
        .unwrap();

    assert!(matches!(
        wait_for_event(&event_rx),
        Packet::ChatMessage { message, .. } if message == "first"
    ));
    assert!(matches!(
        wait_for_event(&event_rx),
        Packet::TimeSync { ticks: 42, .. }
    ));
    assert!(matches!(
        wait_for_event(&event_rx),
        Packet::ChatMessage { message, .. } if message == "second"
    ));
    assert!(matches!(
        wait_for_event(&event_rx),
        Packet::ChunkData { revision: 1, .. }
    ));
    assert!(matches!(
        wait_for_event(&event_rx),
        Packet::BlockChange { revision: 2, .. }
    ));

    game_tx.send(GameToClient::Disconnect).unwrap();
    client.join().unwrap();
    host_tx.try_send(HostToServer::Stop).unwrap();
    server.join().unwrap();
}

#[test]
fn authoritative_weather_packets_map_to_pure_client_events() {
    let sync = Packet::TimeSync {
        ticks: 12_345,
        weather: 1,
        weather_remaining_ticks: 6_789.5,
    };
    assert!(matches!(
        authoritative_weather_event(&sync),
        Some(ClientToGame::Packet(Packet::TimeSync {
            ticks: 12_345,
            weather: 1,
            weather_remaining_ticks: 6_789.5,
            ..
        }))
    ));

    let strike = LightningStrike {
        x: 3,
        y: 72,
        z: -9,
        visual_seed: 123,
    };
    assert!(matches!(
        authoritative_weather_event(&Packet::LightningStrike {
            strike,
        }),
        Some(ClientToGame::Packet(Packet::LightningStrike { strike: received, .. })) if received == strike
    ));
}

#[test]
fn sends_and_receives_chat() {
    let _guard = network_test_guard();
    let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
    let addr = reserved.local_addr().unwrap().to_string();
    drop(reserved);
    let (host_tx, host_rx) = tokio::sync::mpsc::channel(128);
    let (server_tx, server_rx) = mpsc::channel();
    let server = NetworkServer::spawn(addr.clone(), 0xCAFE_BABE, 1, host_rx, server_tx);

    let (game_tx, game_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::sync_channel(CLIENT_TO_GAME_QUEUE_CAPACITY);
    let client = NetworkClient::spawn(addr, "steve".into(), game_rx, event_tx);
    let player_id = match wait_for_event(&event_rx) {
        Packet::LoginSuccess { player_id, .. } => player_id,
        other => panic!("expected Connected, got {other:?}"),
    };
    assert!(matches!(
        server_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        ServerToHost::ClientJoined { id, username }
            if id == player_id && username == "steve"
    ));

    game_tx
        .send(GameToClient::SendChat {
            message: "hello".into(),
        })
        .unwrap();
    assert!(matches!(
        server_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        ServerToHost::ChatFromClient { id, message }
            if id == player_id && message == "hello"
    ));

    host_tx
        .try_send(HostToServer::project_broadcast(Packet::ChatMessage {
                sender: "steve".into(),
                message: "hello".into(),
        }))
        .unwrap();
    assert!(matches!(
        wait_for_event(&event_rx),
        Packet::ChatMessage { sender, message, .. }
            if sender == "steve" && message == "hello"
    ));

    game_tx.send(GameToClient::Disconnect).unwrap();
    client.join().unwrap();
    host_tx.try_send(HostToServer::Stop).unwrap();
    server.join().unwrap();
}

#[test]
fn live_client_inputs_are_typed_gameplay_requests() {
    let _guard = network_test_guard();
    let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
    let addr = reserved.local_addr().unwrap().to_string();
    drop(reserved);
    let (host_tx, host_rx) = tokio::sync::mpsc::channel(128);
    let (server_tx, server_rx) = mpsc::channel();
    let server = NetworkServer::spawn(addr.clone(), 1234, 1, host_rx, server_tx);
    let (game_tx, game_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::sync_channel(CLIENT_TO_GAME_QUEUE_CAPACITY);
    let client = NetworkClient::spawn(addr, "live-client".into(), game_rx, event_tx);
    let player_id = match wait_for_event(&event_rx) {
        Packet::LoginSuccess { player_id, .. } => player_id,
        other => panic!("expected Connected, got {other:?}"),
    };
    assert!(matches!(
        server_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        ServerToHost::ClientJoined { id, .. } if id == player_id
    ));

    let next_request = |server_rx: &Receiver<ServerToHost>| loop {
        match server_rx.recv_timeout(Duration::from_secs(3)).unwrap() {
            ServerToHost::GameplayRequest { request, .. } => break request,
            _ => {}
        }
    };

    game_tx
        .send(GameToClient::GameplayRequest {
            request: GameplayRequest {
                request_id: 0,
                client_sequence: 0,
                session_id: 0,
                dimension: 0,
                client_revision: 0,
                operation: crate::network::protocol::GameplayOperation::BlockAction {
                    action: crate::network::protocol::BlockActionKind::StartBreak,
                    x: 10,
                    y: 64,
                    z: 20,
                    face: [0, 0, 0],
                    hand: 0,
                    held: None,
                    block: 0,
                    look_milli: [0, 0, 1000],
                },
            },
        })
        .unwrap();
    let action = next_request(&server_rx);
    assert_eq!(action.session_id, player_id);
    assert_eq!(action.client_sequence, 1);
    assert!(matches!(
        action.operation,
        crate::network::protocol::GameplayOperation::BlockAction {
            action: crate::network::protocol::BlockActionKind::StartBreak,
            x: 10,
            y: 64,
            z: 20,
            block: 0,
            ..
        }
    ));

    game_tx
        .send(GameToClient::GameplayRequest {
            request: GameplayRequest {
                request_id: 0,
                client_sequence: 0,
                session_id: 0,
                dimension: 1,
                client_revision: 17,
                operation: crate::network::protocol::GameplayOperation::ItemUse {
                    item: crate::inventory::Item::Bread as u32,
                    count: 1,
                },
            },
        })
        .unwrap();
    let typed = next_request(&server_rx);
    assert_eq!(typed.session_id, player_id);
    assert_eq!(typed.client_sequence, 2);
    assert_eq!(typed.client_revision, 17);
    assert!(matches!(
        typed.operation,
        crate::network::protocol::GameplayOperation::ItemUse { item, count }
            if item == crate::inventory::Item::Bread as u32 && count == 1
    ));

    game_tx.send(GameToClient::Disconnect).unwrap();
    client.join().unwrap();
    host_tx.try_send(HostToServer::Stop).unwrap();
    server.join().unwrap();
}

/// Step 2 (Task 5) two-instance smoke test: when the host stops the server,
/// the remaining client observes a `Disconnected` event and its background
/// thread exits cleanly without hanging. This automates the "quitting either
/// side cleans up the background thread without hanging" requirement that
/// the two-window GUI scenario checks manually.
#[test]
fn host_stop_notifies_client_and_threads_join_without_hanging() {
    let _guard = network_test_guard();
    let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
    let addr = reserved.local_addr().unwrap().to_string();
    drop(reserved);
    let (host_tx, host_rx) = tokio::sync::mpsc::channel(128);
    let (server_tx, server_rx) = mpsc::channel();
    let server = NetworkServer::spawn(addr.clone(), 0xDEAD_BEEF, 0, host_rx, server_tx);

    let (_game_tx, game_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::sync_channel(CLIENT_TO_GAME_QUEUE_CAPACITY);
    let client = NetworkClient::spawn(addr, "quit_witness".into(), game_rx, event_tx);

    match wait_for_event(&event_rx) {
        Packet::LoginSuccess { seed, gamemode, .. } => {
            assert_eq!(seed, 0xDEAD_BEEF);
            assert_eq!(gamemode, 0);
        }
        other => panic!("expected Connected, got {other:?}"),
    }
    let _ = server_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("join event missing");

    // Host quits: stop the server. The client must be notified and exit.
    host_tx.try_send(HostToServer::Stop).unwrap();
    match event_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(ClientToGame::Packet(Packet::Disconnect { .. })) => {}
        Ok(other) => panic!("expected Disconnected, got {other:?}"),
        Err(_) => panic!("client did not observe disconnect after host stop"),
    }
    client
        .join()
        .expect("client thread did not exit after host shutdown");
    server
        .join()
        .expect("server thread did not exit after host shutdown");
}

#[test]
fn revision_gate_orders_cross_channel_snapshot_and_block_change() {
    let mut gate = RevisionGate::default();
    assert!(gate
        .accept_block_change(0, 2, 1, 70, 1, 4, 0, crate::world::FLUID_WATERLOGGED_BIT,)
        .is_empty());

    let events = gate.accept_snapshot(
        0,
        0,
        0,
        1,
        0,
        16,
        vec![1, 2],
        vec![0, 0],
        vec![crate::world::FLUID_WATERLOGGED_BIT],
        vec![],
    );
    assert_eq!(events.len(), 2);
    assert!(matches!(
        &events[0],
        Packet::ChunkData {
            dimension: 0,
            cx: 0,
            cz: 0,
            revision: 1,
            ..
        }
    ));
    assert!(matches!(
        &events[0],
        Packet::ChunkData {
            fluid_levels,
            ..
        } if fluid_levels == &vec![crate::world::FLUID_WATERLOGGED_BIT]
    ));
    assert!(matches!(
        &events[1],
        Packet::BlockChange {
            dimension: 0,
            revision: 2,
            x: 1,
            y: 70,
            z: 1,
            block: 4,
            raw_fluid: crate::world::FLUID_WATERLOGGED_BIT,
            ..
        }
    ));

    assert!(gate
        .accept_snapshot(0, 0, 0, 1, 0, 16, vec![9], vec![9], vec![], vec![])
        .is_empty());
    assert!(gate.accept_block_change(0, 1, 1, 70, 1, 9, 0, 0).is_empty());

    // Session-only revisions legitimately create gaps in the world's
    // dimension-wide clock. Once a chunk snapshot establishes the base,
    // a newer block mutation must not wait for nonexistent chunk deltas.
    let skipped = gate.accept_block_change(0, 7, 1, 70, 1, 8, 0, 0);
    assert!(matches!(
        skipped.as_slice(),
        [Packet::BlockChange {
            revision: 7,
            block: 8,
            ..
        }]
    ));
    assert!(gate.accept_block_change(0, 6, 1, 70, 1, 7, 0, 0).is_empty());

    let mut same_revision = RevisionGate::default();
    assert!(same_revision
        .accept_block_change(0, 5, 1, 70, 1, 4, 0, 0)
        .is_empty());
    assert_eq!(
        same_revision
            .accept_snapshot(0, 0, 0, 5, 0, 16, vec![1], vec![0], vec![], vec![])
            .len(),
        1
    );
    assert!(same_revision.buffered.is_empty());
}

#[test]
fn revision_gate_multi_client_checksum_converges() {
    fn checksum(chunk: &crate::world::Chunk) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for x in 0..16 {
            for y in chunk.world_y_range() {
                for z in 0..16 {
                    hash ^= chunk.get_block_local(x, y, z) as u8 as u64;
                    hash = hash.wrapping_mul(0x100_0000_01b3);
                }
            }
        }
        hash
    }

    let mut snapshot_chunk = crate::world::Chunk::new(0, 0);
    snapshot_chunk.set_block_local(1, 70, 1, crate::world::BlockType::Stone);
    let snapshot = crate::save::ChunkSaveData::from_chunk(&snapshot_chunk).unwrap();
    let mut host = snapshot_chunk.clone();
    host.set_block_local(2, 70, 2, crate::world::BlockType::Dirt);

    for _ in 0..2 {
        let mut gate = RevisionGate::default();
        let mut events = gate.accept_block_change(
            0,
            2,
            2,
            70,
            2,
            crate::world::BlockType::Dirt.to_wire(),
            0,
            0,
        );
        assert!(events.is_empty());
        events.extend(gate.accept_snapshot(
            0,
            0,
            0,
            1,
            -4,
            24,
            snapshot.blocks.clone(),
            snapshot.block_states.clone(),
            snapshot.fluid_levels.clone(),
            snapshot.block_entities.clone(),
        ));

        let mut client = crate::world::Chunk::new(0, 0);
        for event in events {
            match event {
                Packet::ChunkData {
                    blocks,
                    block_states,
                    ..
                } => {
                    crate::save::ChunkSaveData {
                        chunk_x: 0,
                        chunk_z: 0,
                        blocks,
                        sky_light: Vec::new(),
                        block_light: Vec::new(),
                        fluid_levels: Vec::new(),
                        redstone_metadata: Vec::new(),
                        block_states,
                        mutation_revision: 1,
                        block_entities: Vec::new(),
                        data_version: 0,
                    }
                    .restore_to_chunk(&mut client)
                    .unwrap();
                }
                Packet::BlockChange { x, y, z, block, .. } => {
                    client.set_block_local(
                        x.rem_euclid(16) as usize,
                        y,
                        z.rem_euclid(16) as usize,
                        crate::world::BlockType::from_wire(block).unwrap(),
                    );
                }
                _ => {}
            }
        }
        assert_eq!(checksum(&client), checksum(&host));
    }
}

#[test]
fn replication_gate_rejects_stale_entity_health_and_effect_state() {
    let mut gate = ReplicationGate::default();
    assert!(gate.accept_entity(0, 9, 1));
    assert!(!gate.accept_entity(0, 9, 1));
    assert!(!gate.accept_entity(0, 9, 0));
    assert!(gate.accept_entity(0, 9, 2));
    assert!(gate.accept_entity(0, 10, 1));
    assert!(gate.accept_entity(1, 9, 1));

    assert!(gate.accept_session(4, 0, 1, 0));
    assert!(!gate.accept_session(4, 0, 2, 0));
    assert!(gate.accept_session(4, 0, 3, 1));
    assert!(gate.accept_session(4, 1, 4, 0));

    assert!(gate.accept_health(4, 7));
    assert!(!gate.accept_health(4, 7));
    assert!(!gate.accept_health(4, 6));
    assert!(gate.accept_effect(4, 7));
    assert!(!gate.accept_effect(4, 7));
}

#[test]
fn gameplay_response_gate_drops_exact_cached_ack_and_rewrites() {
    let mut gate = GameplayResponseGate::default();
    let response_two = GameplayResponse {
        request_id: 2,
        server_sequence: 2,
        outcome: crate::network::protocol::GameplayOutcome::Rejected {
            reason: crate::network::protocol::RejectReason::Unsupported,
        },
    };
    assert!(gate.accept(&response_two));
    assert!(!gate.accept(&response_two));

    let response_one = GameplayResponse {
        request_id: 1,
        server_sequence: 1,
        outcome: crate::network::protocol::GameplayOutcome::Accepted { revision: 1 },
    };
    assert!(!gate.accept(&response_one));

    let duplicate = GameplayResponse {
        request_id: 2,
        server_sequence: 3,
        outcome: response_two.outcome.clone(),
    };
    assert!(!gate.accept(&duplicate));
    assert!(!gate.accept(&GameplayResponse {
        request_id: 3,
        server_sequence: 0,
        outcome: crate::network::protocol::GameplayOutcome::Rejected {
            reason: crate::network::protocol::RejectReason::QueueFull,
        },
    }));
}

#[test]
fn replication_gate_drops_stale_block_entity_and_container_deltas() {
    let mut gate = ReplicationGate::default();
    assert!(gate.accept_block_entity((1, 2, 64, 3), 9));
    assert!(!gate.accept_block_entity((1, 2, 64, 3), 8));
    assert!(!gate.accept_block_entity((1, 2, 64, 3), 9));
    assert!(gate.accept_block_entity((1, 2, 64, 3), 10));
    assert!(gate.accept_container_update((0, 8, 80, 8), 3));
    assert!(!gate.accept_container_update((0, 8, 80, 8), 2));
}

#[test]
fn targeted_container_close_bypasses_revision_gate_and_exact_matches_active_key() {
    let _guard = network_test_guard();
    let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
    let addr = reserved.local_addr().unwrap().to_string();
    drop(reserved);
    let (host_tx, host_rx) = tokio::sync::mpsc::channel(128);
    let (server_tx, server_rx) = mpsc::channel();
    let server = NetworkServer::spawn(addr.clone(), 1234, 1, host_rx, server_tx);

    let (game_tx, game_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::sync_channel(CLIENT_TO_GAME_QUEUE_CAPACITY);
    let client = NetworkClient::spawn(addr, "container-close".into(), game_rx, event_tx);
    let player_id = match wait_for_event(&event_rx) {
        Packet::LoginSuccess { player_id, .. } => player_id,
        other => panic!("expected Connected, got {other:?}"),
    };
    let _ = server_rx.recv_timeout(Duration::from_secs(3)).unwrap();

    let position = (4, 64, 4);
    host_tx
        .try_send(HostToServer::project_session(
            player_id,
            Packet::ContainerOpenResult {
                dimension: 0,
                success: true,
                x: position.0,
                y: position.1,
                z: position.2,
                slots: vec![],
                revision: 2,
            },
        ))
        .unwrap();
    assert!(matches!(
        wait_for_event(&event_rx),
        Packet::ContainerOpenResult {
            success: true,
            x,
            y,
            z,
            ..
        } if (x, y, z) == position
    ));

    // Advance the slot revision so a close with no revision field proves
    // it is not accidentally sent through the container delta gate.
    host_tx
        .try_send(HostToServer::project_session(
            player_id,
            Packet::ContainerSlotUpdate {
                dimension: 0,
                revision: 100,
                x: position.0,
                y: position.1,
                z: position.2,
                slot_index: 0,
                slot: None,
            },
        ))
        .unwrap();
    assert!(matches!(
        wait_for_event(&event_rx),
        Packet::ContainerSlotUpdate { revision: 100, .. }
    ));

    host_tx
        .try_send(HostToServer::project_session(
            player_id,
            Packet::ContainerClose {
                dimension: 0,
                x: position.0 + 1,
                y: position.1,
                z: position.2,
            },
        ))
        .unwrap();
    assert!(matches!(
        event_rx.recv_timeout(Duration::from_millis(250)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));

    host_tx
        .try_send(HostToServer::project_session(
            player_id,
            Packet::ContainerClose {
                dimension: 0,
                x: position.0,
                y: position.1,
                z: position.2,
            },
        ))
        .unwrap();
    assert!(matches!(
        wait_for_event(&event_rx),
        Packet::ContainerClose { dimension: 0, x, y, z, .. }
            if (x, y, z) == position
    ));

    game_tx.send(GameToClient::Disconnect).unwrap();
    client.join().unwrap();
    host_tx.try_send(HostToServer::Stop).unwrap();
    server.join().unwrap();
}

#[test]
fn host_client_entity_health_and_effect_replication_converges() {
    let _guard = network_test_guard();
    let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
    let addr = reserved.local_addr().unwrap().to_string();
    drop(reserved);
    let (host_tx, host_rx) = tokio::sync::mpsc::channel(128);
    let (server_tx, server_rx) = mpsc::channel();
    let server = NetworkServer::spawn(addr.clone(), 1234, 1, host_rx, server_tx);

    let (game_tx, game_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::sync_channel(CLIENT_TO_GAME_QUEUE_CAPACITY);
    let client = NetworkClient::spawn(addr, "replica".into(), game_rx, event_tx);
    let player_id = match wait_for_event(&event_rx) {
        Packet::LoginSuccess { player_id, .. } => player_id,
        other => panic!("expected Connected, got {other:?}"),
    };
    assert!(matches!(
        server_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        ServerToHost::ClientJoined { id, .. } if id == player_id
    ));

    let state = |x| EntityStateWire {
        entity_id: 77,
        entity_type: crate::entity::EntityType::Zombie.to_wire(),
        position: [x, 64.0, 0.0],
        velocity: [1.0, 0.0, 0.0],
        yaw: 0.5,
        pitch: 0.0,
        health: 18.0,
        animation_state: 1,
        item: None,
    };
    host_tx
        .try_send(HostToServer::project_broadcast(Packet::EntitySpawn {
                dimension: 0,
                sequence: 1,
                state: state(0.0),
        }))
        .unwrap();
    host_tx
        .try_send(HostToServer::project_broadcast(Packet::EntityState {
                dimension: 0,
                sequence: 2,
                state: state(2.0),
        }))
        .unwrap();
    host_tx
        .try_send(HostToServer::project_broadcast(Packet::EntityState {
                dimension: 0,
                sequence: 3,
                state: state(3.0),
        }))
        .unwrap();
    host_tx
        .try_send(HostToServer::project_broadcast(Packet::PlayerEffect {
                sequence: 3,
                player_id: player_id,
                effects: vec![PlayerEffectWire {
                kind: 0,
                level: 2,
                remaining_seconds: 30.0,
            }],
        }))
        .unwrap();

    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut saw_spawn = false;
    let mut latest_entity_x = None;
    let mut effects = None;
    while std::time::Instant::now() < deadline
        && (!saw_spawn || latest_entity_x != Some(3.0) || effects.is_none())
    {
        let Ok(event) = event_rx.recv_timeout(Duration::from_millis(250)) else {
            continue;
        };
        match event {
            ClientToGame::Packet(Packet::EntitySpawn { state, .. })
                if state.entity_id == 77 =>
            {
                saw_spawn = true;
            }
            ClientToGame::Packet(Packet::EntityState { state, .. })
                if state.entity_id == 77 =>
            {
                latest_entity_x = Some(state.position[0]);
            }
            ClientToGame::Packet(Packet::PlayerEffect {
                player_id: id,
                effects: value,
                ..
            }) if id == player_id => effects = Some(value),
            _ => {}
        }
    }
    assert!(saw_spawn);
    assert_eq!(latest_entity_x, Some(3.0));
    assert_eq!(effects.unwrap()[0].level, 2);

    host_tx
        .try_send(HostToServer::project_broadcast(Packet::EntityDespawn {
                dimension: 0,
                sequence: 4,
                entity_id: 77,
        }))
        .unwrap();
    assert!(matches!(
        wait_for_event(&event_rx),
        Packet::EntityDespawn {
            sequence: 4,
            entity_id: 77,
            ..
        }
    ));

    game_tx.send(GameToClient::Disconnect).unwrap();
    client.join().unwrap();
    host_tx.try_send(HostToServer::Stop).unwrap();
    server.join().unwrap();
}

#[test]
fn queue_accounting_covers_success_and_closed_paths() {
    let stats = crate::perf::SharedQueueStats::new();
    let (tx, rx) = mpsc::channel();
    crate::perf::tracked_send(
        &tx,
        ClientToGame::StatusUpdate {
            message: "ok".into(),
        },
        16,
        &stats,
    )
    .unwrap();
    assert_eq!(stats.depth(), 1);
    let _ = crate::perf::tracked_try_recv(&rx, 16, &stats).unwrap();
    assert_eq!(stats.depth(), 0);

    drop(rx);
    assert!(crate::perf::tracked_send(
        &tx,
        ClientToGame::StatusUpdate {
            message: "closed".into()
        },
        16,
        &stats,
    )
    .is_err());
    assert_eq!(stats.depth(), 0);
    assert_eq!(stats.bytes(), 0);
    assert_eq!(stats.drops(), 1);
}

#[test]
fn queue_accounting_closed_consumer_is_cancelled() {
    let stats = crate::perf::SharedQueueStats::new();
    let (tx, rx) = mpsc::channel::<GameToClient>();
    drop(tx);
    assert!(matches!(
        crate::perf::tracked_try_recv(&rx, 32, &stats),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    ));
    stats.cancel();
    assert_eq!(stats.cancels(), 1);
}

#[test]
fn client_event_sender_disconnects_on_sustained_overflow() {
    let (tx, rx) = mpsc::sync_channel(1);
    let sender = ClientEventSender::new(tx);
    sender
        .send(ClientToGame::StatusUpdate {
            message: "hold".into(),
        })
        .unwrap();
    let mut saw_full = false;
    for index in 0..(CLIENT_TO_GAME_OVERFLOW_LIMIT + 4) {
        let result = sender.send(ClientToGame::StatusUpdate {
            message: format!("overflow-{index}"),
        });
        if result == Err(ClientQueueError::Full) {
            saw_full = true;
        }
    }
    assert!(saw_full);
    assert!(sender.is_dead());
    drop(rx);
}

#[test]
fn non_local_player_session_update_is_not_enqueued() {
    let _guard = network_test_guard();
    let listener = StdTcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let server = std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let (stream, _) = listener.accept().await.unwrap();
            let mut connection = crate::network::transport::Connection::new(stream);
            let _ = connection.recv().await.unwrap();
            connection
                .send(&Packet::LoginSuccess {
                    protocol_version: PROTOCOL_VERSION,
                    player_id: 1,
                    seed: 1,
                    gamemode: 1,
                })
                .await
                .unwrap();
            let mut foreign = SessionGameplayWire::default();
            foreign.revision = 3;
            connection
                .send(&Packet::PlayerSessionUpdate {
                    sequence: 1,
                    player_id: 99,
                    dimension: 0,
                    state: foreign,
                })
                .await
                .unwrap();
            let mut local = SessionGameplayWire::default();
            local.revision = 4;
            connection
                .send(&Packet::PlayerSessionUpdate {
                    sequence: 2,
                    player_id: 1,
                    dimension: 0,
                    state: local,
                })
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
        });
    });

    let (game_tx, game_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::sync_channel(CLIENT_TO_GAME_QUEUE_CAPACITY);
    let client = NetworkClient::spawn(addr, "owner".into(), game_rx, event_tx);
    assert!(matches!(
        wait_for_event(&event_rx),
        Packet::LoginSuccess { player_id: 1, .. }
    ));
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    let mut local_update = None;
    while std::time::Instant::now() < deadline && local_update.is_none() {
        match event_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(ClientToGame::Packet(Packet::PlayerSessionUpdate { player_id: 99, .. })) => {
                panic!("foreign player_id session snapshot entered the join-client queue")
            }
            Ok(event @ ClientToGame::Packet(Packet::PlayerSessionUpdate { player_id: 1, .. })) => {
                local_update = Some(unwrap_packet(event));
            }
            Ok(ClientToGame::StatusUpdate { .. }) => {}
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("client event channel failed: {error}"),
        }
    }
    assert!(matches!(
        local_update,
        Some(Packet::PlayerSessionUpdate {
            player_id: 1,
            state,
            ..
        }) if state.revision == 4
    ));
    game_tx.send(GameToClient::Disconnect).unwrap();
    client.join().unwrap();
    server.join().unwrap();
}
