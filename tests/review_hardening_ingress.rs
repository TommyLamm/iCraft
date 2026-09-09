//! Plan12: ingress backpressure, oversized chat rejection, and peer isolation
//! over real sockets. Container-slot reliability under viewer-queue pressure
//! is covered by the lib test
//! `broadcast_container_slot_is_reliable_or_evicts_slow_viewer`.

mod common;

use common::tcp_harness::{drive_until, temp_world, wait_for_response, HeldLoopback, TcpClient};
use icraft::dimension::Dimension;
use icraft::network::client::ClientToGame;
use icraft::network::protocol::{GameplayOperation, GameplayRequest, Packet, PROTOCOL_VERSION};
use icraft::server_runtime::{ServerProperties, ServerRuntime};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

const POSITION: [f32; 3] = [8.0, 80.0, 8.0];

fn properties(label: &str, port: u16) -> ServerProperties {
    ServerProperties {
        bind: "127.0.0.1".into(),
        port,
        max_players: 4,
        view_distance: 2,
        simulation_distance: 2,
        seed: 0x12_12_12_12,
        world_dir: temp_world(&format!("plan12-ingress-{label}")),
        ..ServerProperties::default()
    }
}

fn write_packet(stream: &mut TcpStream, packet: &Packet) {
    stream
        .write_all(&packet.encode_frame().expect("legal packet frame"))
        .expect("write packet frame");
}

fn read_packet(stream: &mut TcpStream) -> Packet {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).expect("read packet length");
    let len = u32::from_be_bytes(header) as usize;
    let mut payload = vec![0u8; len];
    stream
        .read_exact(&mut payload)
        .expect("read packet payload");
    Packet::decode(&payload).expect("decode packet")
}

fn handshake(address: &str, username: &str) -> (TcpStream, u64) {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut stream = loop {
        match TcpStream::connect(address) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => panic!("flooder could not connect to {address}: {error}"),
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write_packet(
        &mut stream,
        &Packet::Handshake {
            protocol_version: PROTOCOL_VERSION,
            username: username.into(),
        },
    );
    loop {
        match read_packet(&mut stream) {
            Packet::LoginSuccess { player_id, .. } => return (stream, player_id),
            Packet::Disconnect { reason, .. } => {
                panic!("flooder handshake rejected: {reason}")
            }
            _ => {}
        }
    }
}

fn request(
    runtime: &ServerRuntime,
    player_id: u64,
    request_id: u128,
    sequence: u64,
) -> GameplayRequest {
    GameplayRequest {
        request_id,
        client_sequence: sequence,
        session_id: player_id,
        dimension: runtime
            .authority
            .session(player_id)
            .map(|session| session.dimension)
            .unwrap_or(0),
        client_revision: runtime
            .authority
            .revision_for_dimension(Dimension::Overworld),
        operation: GameplayOperation::ItemUse { item: 1, count: 1 },
    }
}

#[test]
fn pose_and_oversized_chat_flood_does_not_block_peer_gameplay() {
    let reserved = HeldLoopback::bind();
    let properties = properties("flood", reserved.port());
    let address = format!("{}:{}", properties.bind, properties.port);
    let world_dir = properties.world_dir.clone();
    let _port = reserved.release();
    let mut runtime = ServerRuntime::new(properties).expect("construct dedicated Plan12 runtime");

    let (mut flooder, flooder_id) = handshake(&address, "flooder");
    let mut peer = TcpClient::connect(&address, "peer");
    {
        let mut refs: Vec<&mut TcpClient> = vec![&mut peer];
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan12 peer authenticated while flooder is connected",
            |runtime, views| {
                views[0].player_id().is_some() && runtime.players.contains_key(&flooder_id)
            },
        );
    }
    let peer_id = peer.player_id().expect("peer player id");
    if let Some(player) = runtime.players.get_mut(&peer_id) {
        player.data.position = POSITION;
    }
    if let Some(session) = runtime.authority.session_mut(peer_id) {
        session.position = POSITION;
    }

    for sequence in 1..=200 {
        write_packet(
            &mut flooder,
            &Packet::PlayerPosition {
                protocol_version: PROTOCOL_VERSION,
                id: flooder_id,
                sequence,
                sender_time_millis: u64::from(sequence),
                x: POSITION[0],
                y: POSITION[1],
                z: POSITION[2],
                yaw: 0.0,
                pitch: 0.0,
            },
        );
    }
    write_packet(
        &mut flooder,
        &Packet::ChatMessage {
            protocol_version: PROTOCOL_VERSION,
            sender: "flooder".into(),
            message: "x".repeat(257),
        },
    );

    let gameplay = request(&runtime, peer_id, 12, 1);
    peer.send_request(gameplay);
    let _response = {
        let mut refs: Vec<&mut TcpClient> = vec![&mut peer];
        wait_for_response(&mut runtime, &mut refs, 0, 12)
    };

    peer.drain();
    assert!(
        peer.events().iter().all(|event| !matches!(
            event,
            ClientToGame::Chat { message, .. } if message.chars().count() > 256
        )),
        "oversized chat must not be broadcast after being rejected at ingress"
    );
    assert!(
        runtime.players.contains_key(&peer_id),
        "peer must stay connected through the flooder's pose/chat flood"
    );

    runtime
        .shutdown()
        .expect("shutdown dedicated Plan12 runtime");
    let _ = std::fs::remove_dir_all(world_dir);
    drop(flooder);
}

#[test]
fn oversized_chat_from_join_client_is_not_relayed() {
    let reserved = HeldLoopback::bind();
    let properties = properties("chat", reserved.port());
    let address = format!("{}:{}", properties.bind, properties.port);
    let world_dir = properties.world_dir.clone();
    let _port = reserved.release();
    let mut runtime = ServerRuntime::new(properties).expect("construct dedicated Plan12 runtime");

    let mut flooder = TcpClient::connect(&address, "chatter");
    let mut peer = TcpClient::connect(&address, "listener");
    {
        let mut refs: Vec<&mut TcpClient> = vec![&mut flooder, &mut peer];
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan12 chat clients authenticated",
            |_runtime, views| views.iter().all(|client| client.player_id().is_some()),
        );
        for client in refs.iter_mut() {
            client.clear_events();
        }
    }

    flooder.send(icraft::network::client::GameToClient::SendChat {
        message: "y".repeat(300),
    });
    flooder.send(icraft::network::client::GameToClient::SendChat {
        message: "ok".into(),
    });
    {
        let mut refs: Vec<&mut TcpClient> = vec![&mut flooder, &mut peer];
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan12 legal chat relayed",
            |_runtime, views| {
                views.iter().any(|client| {
                    client.events().iter().any(|event| {
                        matches!(event, ClientToGame::Chat { message, .. } if message == "ok")
                    })
                })
            },
        );
    }
    assert!(
        flooder
            .events()
            .iter()
            .chain(peer.events().iter())
            .all(|event| !matches!(
                event,
                ClientToGame::Chat { message, .. } if message.chars().count() > 256
            )),
        "oversized chat must not enter the host queue or be relayed"
    );

    runtime
        .shutdown()
        .expect("shutdown dedicated Plan12 runtime");
    let _ = std::fs::remove_dir_all(world_dir);
}
