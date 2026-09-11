//! Raw-TCP adversarial frames for Plan03.
//!
//! These fixtures write crafted length headers and lying `Vec` lengths with
//! `std::net::TcpStream`. They must not go through `NetworkClient`.

mod common;

use common::tcp_harness::connect_loopback_std;
use icraft::network::protocol::{Packet, MAX_PACKET_SIZE, PROTOCOL_VERSION};
use icraft::network::server::{HostToServer, NetworkServer, ServerToHost};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const EVENT_TIMEOUT: Duration = Duration::from_secs(5);

struct RawServer {
    addr: String,
    host_tx: tokio::sync::mpsc::Sender<HostToServer>,
    event_rx: Receiver<ServerToHost>,
    handle: Option<JoinHandle<()>>,
}

impl RawServer {
    fn start() -> Self {
        let reserved = TcpListener::bind("127.0.0.1:0").expect("reserve loopback port");
        let addr = reserved
            .local_addr()
            .expect("read reserved addr")
            .to_string();
        drop(reserved);

        let (host_tx, host_rx) = tokio::sync::mpsc::channel(16);
        let (event_tx, event_rx) = mpsc::channel();
        let handle = NetworkServer::spawn(addr.clone(), 0xCAFE_BABE, 1, host_rx, event_tx);
        Self {
            addr,
            host_tx,
            event_rx,
            handle: Some(handle),
        }
    }

    fn connect_raw(&self) -> TcpStream {
        let stream = connect_loopback_std(&self.addr);
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set read timeout");
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .expect("set write timeout");
        stream
    }

    fn assert_no_join(&self, description: &str) {
        let deadline = Instant::now() + Duration::from_millis(200);
        while Instant::now() < deadline {
            match self.event_rx.try_recv() {
                Ok(ServerToHost::ClientJoined { id, username }) => {
                    panic!("{description} created a session: id={id} username={username}");
                }
                Ok(ServerToHost::Disconnected { reason })
                    if reason.contains("failed to bind") || reason.contains("failed to create") =>
                {
                    panic!("server failed during {description}: {reason}");
                }
                Ok(_) | Err(mpsc::TryRecvError::Empty) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    panic!("server thread died during {description}");
                }
            }
        }
    }

    fn wait_for_join(&self, username: &str) -> u64 {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        loop {
            match self.event_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(ServerToHost::ClientJoined {
                    id,
                    username: joined,
                }) if joined == username => return id,
                Ok(ServerToHost::Disconnected { reason })
                    if reason.contains("failed to bind") || reason.contains("failed to create") =>
                {
                    panic!("server failed while waiting for {username}: {reason}");
                }
                Ok(_) if Instant::now() < deadline => {}
                Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
                Ok(event) => panic!("unexpected event while waiting for {username}: {event:?}"),
                Err(error) => panic!("timed out waiting for {username} to join: {error}"),
            }
        }
    }

    fn stop(mut self) {
        let _ = self.host_tx.try_send(HostToServer::Stop);
        if let Some(handle) = self.handle.take() {
            handle.join().expect("network server thread panicked");
        }
    }
}

impl Drop for RawServer {
    fn drop(&mut self) {
        let _ = self.host_tx.try_send(HostToServer::Stop);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn write_frame(stream: &mut TcpStream, payload: &[u8]) {
    let len = u32::try_from(payload.len()).expect("payload fits u32");
    stream.write_all(&len.to_be_bytes()).expect("write length");
    stream.write_all(payload).expect("write payload");
    stream.flush().expect("flush frame");
}

fn read_packet(stream: &mut TcpStream) -> Packet {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).expect("read length header");
    let len = usize::try_from(u32::from_be_bytes(header)).expect("frame length");
    assert!(len <= MAX_PACKET_SIZE, "server emitted an oversized frame");
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).expect("read payload");
    Packet::decode(&body).expect("server frame decodes")
}

fn handshake(username: &str) -> Packet {
    Packet::Handshake {
        protocol_version: PROTOCOL_VERSION,
        username: username.into(),
    }
}

fn crafted_chunk_data_blocks_len(claimed: u64) -> Vec<u8> {
    let packet = Packet::ChunkData {
        protocol_version: PROTOCOL_VERSION,
        dimension: 0,
        cx: 0,
        cz: 0,
        revision: 0,
        min_section_y: 0,
        section_count: 0,
        blocks: Vec::new(),
        block_states: Vec::new(),
        fluid_levels: Vec::new(),
        block_entities: Vec::new(),
    };
    let mut bytes = packet.encode();
    let len_offset = bytes.len() - 32;
    bytes[len_offset..len_offset + 8].copy_from_slice(&claimed.to_le_bytes());
    bytes.truncate(len_offset + 8);
    bytes
}

fn login_with_raw(stream: &mut TcpStream, username: &str) {
    write_frame(stream, &handshake(username).encode());
    let reply = read_packet(stream);
    assert!(
        matches!(reply, Packet::LoginSuccess { protocol_version, .. } if protocol_version == PROTOCOL_VERSION),
        "expected LoginSuccess, got {reply:?}"
    );
}

#[test]
fn oversized_length_header_does_not_create_session_and_server_survives() {
    let server = RawServer::start();

    let mut attacker = server.connect_raw();
    attacker
        .write_all(&0x0020_0001u32.to_be_bytes())
        .expect("write oversized length header");
    attacker
        .write_all(&[0u8; 16])
        .expect("write short body after oversized header");
    attacker.flush().expect("flush oversized header");
    server.assert_no_join("oversized length header");

    let mut honest = server.connect_raw();
    login_with_raw(&mut honest, "steve");
    let _ = server.wait_for_join("steve");

    server.stop();
}

#[test]
fn crafted_vec_len_before_handshake_does_not_create_session_and_server_survives() {
    let server = RawServer::start();

    let mut attacker = server.connect_raw();
    let payload = crafted_chunk_data_blocks_len(1 << 40);
    assert!(
        payload.len() < 64,
        "adversarial frame must stay far below the 2 MiB cap"
    );
    write_frame(&mut attacker, &payload);
    server.assert_no_join("crafted ChunkData Vec length");

    let mut honest = server.connect_raw();
    login_with_raw(&mut honest, "alex");
    let _ = server.wait_for_join("alex");

    server.stop();
}
