use std::sync::mpsc::{Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::time::{self, Instant};

use super::protocol::{Action, Packet, PlayerId, PROTOCOL_VERSION};
use super::transport::Connection;

#[derive(Debug)]
pub enum ClientToGame {
    Connected {
        player_id: PlayerId,
        seed: u64,
        gamemode: u8,
    },
    Disconnected {
        reason: String,
    },
    PlayerJoin {
        id: PlayerId,
        username: String,
    },
    PlayerLeave {
        id: PlayerId,
    },
    PlayerPosition {
        id: PlayerId,
        x: f32,
        y: f32,
        z: f32,
        yaw: f32,
        pitch: f32,
    },
    PlayerAction {
        id: PlayerId,
        action: Action,
    },
    BlockChange {
        x: i32,
        y: i32,
        z: i32,
        block: u32,
    },
    ChunkData {
        cx: i32,
        cz: i32,
        blocks: Vec<u8>,
    },
    Chat {
        sender: String,
        message: String,
    },
}

#[derive(Debug)]
pub enum GameToClient {
    SendPosition {
        x: f32,
        y: f32,
        z: f32,
        yaw: f32,
        pitch: f32,
    },
    SendAction {
        action: Action,
    },
    RequestBlockChange {
        x: i32,
        y: i32,
        z: i32,
        block: u32,
    },
    SendChat {
        message: String,
    },
    Disconnect,
}

pub struct NetworkClient;

impl NetworkClient {
    pub fn spawn(
        server_addr: String,
        username: String,
        game_to_client: Receiver<GameToClient>,
        client_to_game: Sender<ClientToGame>,
    ) -> JoinHandle<()> {
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Runtime::new() {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = client_to_game.send(ClientToGame::Disconnected {
                        reason: format!("failed to create network runtime: {error}"),
                    });
                    return;
                }
            };
            runtime.block_on(run_client(
                server_addr,
                username,
                game_to_client,
                client_to_game,
            ));
        })
    }
}

async fn run_client(
    server_addr: String,
    username: String,
    game_to_client: Receiver<GameToClient>,
    client_to_game: Sender<ClientToGame>,
) {
    let deadline = Instant::now() + Duration::from_secs(3);
    let stream = loop {
        match TcpStream::connect(&server_addr).await {
            Ok(stream) => break stream,
            Err(error) if Instant::now() < deadline => {
                time::sleep(Duration::from_millis(20)).await;
                let _ = error;
            }
            Err(error) => {
                let _ = client_to_game.send(ClientToGame::Disconnected {
                    reason: format!("connection failed: {error}"),
                });
                return;
            }
        }
    };

    let mut connection = Connection::new(stream);
    if let Err(error) = connection
        .send(&Packet::Handshake {
            protocol_version: PROTOCOL_VERSION,
            username: username.clone(),
        })
        .await
    {
        let _ = client_to_game.send(ClientToGame::Disconnected {
            reason: error.to_string(),
        });
        return;
    }

    let player_id = match time::timeout(Duration::from_secs(5), connection.recv()).await {
        Ok(Ok(Packet::LoginSuccess {
            protocol_version,
            player_id,
            seed,
            gamemode,
        })) if protocol_version == PROTOCOL_VERSION => {
            let _ = client_to_game.send(ClientToGame::Connected {
                player_id,
                seed,
                gamemode,
            });
            player_id
        }
        Ok(Ok(Packet::Disconnect { reason, .. })) => {
            let _ = client_to_game.send(ClientToGame::Disconnected { reason });
            return;
        }
        Ok(Ok(packet)) => {
            let _ = client_to_game.send(ClientToGame::Disconnected {
                reason: format!("unexpected handshake response: {packet:?}"),
            });
            return;
        }
        Ok(Err(error)) => {
            let _ = client_to_game.send(ClientToGame::Disconnected {
                reason: error.to_string(),
            });
            return;
        }
        Err(_) => {
            let _ = client_to_game.send(ClientToGame::Disconnected {
                reason: "login timed out".into(),
            });
            return;
        }
    };

    let (mut reader, mut writer) = connection.into_split();
    let mut tick = time::interval(Duration::from_millis(10));
    loop {
        tokio::select! {
            incoming = reader.recv() => {
                match incoming {
                    Ok(packet) if packet.protocol_version() != PROTOCOL_VERSION => {
                        let _ = client_to_game.send(ClientToGame::Disconnected { reason: "protocol version mismatch".into() });
                        break;
                    }
                    Ok(Packet::PlayerJoin { id, username, .. }) => { let _ = client_to_game.send(ClientToGame::PlayerJoin { id, username }); }
                    Ok(Packet::PlayerLeave { id, .. }) => { let _ = client_to_game.send(ClientToGame::PlayerLeave { id }); }
                    Ok(Packet::PlayerPosition { id, x, y, z, yaw, pitch, .. }) => { let _ = client_to_game.send(ClientToGame::PlayerPosition { id, x, y, z, yaw, pitch }); }
                    Ok(Packet::PlayerAction { id, action, .. }) => { let _ = client_to_game.send(ClientToGame::PlayerAction { id, action }); }
                    Ok(Packet::BlockChange { x, y, z, block, .. }) => { let _ = client_to_game.send(ClientToGame::BlockChange { x, y, z, block }); }
                    Ok(Packet::ChunkData { cx, cz, blocks, .. }) => { let _ = client_to_game.send(ClientToGame::ChunkData { cx, cz, blocks }); }
                    Ok(Packet::ChatMessage { sender, message, .. }) => { let _ = client_to_game.send(ClientToGame::Chat { sender, message }); }
                    Ok(Packet::Keepalive { .. }) => {
                        if writer.send(&Packet::Keepalive { protocol_version: PROTOCOL_VERSION }).await.is_err() {
                            let _ = client_to_game.send(ClientToGame::Disconnected { reason: "connection lost".into() });
                            break;
                        }
                    }
                    Ok(Packet::Disconnect { reason, .. }) => { let _ = client_to_game.send(ClientToGame::Disconnected { reason }); break; }
                    Ok(_) => {}
                    Err(_) => { let _ = client_to_game.send(ClientToGame::Disconnected { reason: "connection lost".into() }); break; }
                }
            }
            _ = tick.tick() => {
                loop {
                    match game_to_client.try_recv() {
                        Ok(GameToClient::SendPosition { x, y, z, yaw, pitch }) => {
                            if writer.send(&Packet::PlayerPosition { protocol_version: PROTOCOL_VERSION, id: player_id, x, y, z, yaw, pitch }).await.is_err() {
                                let _ = client_to_game.send(ClientToGame::Disconnected { reason: "connection lost".into() });
                                return;
                            }
                        }
                        Ok(GameToClient::SendAction { action }) => {
                            if writer.send(&Packet::PlayerAction { protocol_version: PROTOCOL_VERSION, id: player_id, action }).await.is_err() {
                                let _ = client_to_game.send(ClientToGame::Disconnected { reason: "connection lost".into() });
                                return;
                            }
                        }
                        Ok(GameToClient::RequestBlockChange { x, y, z, block }) => {
                            if writer.send(&Packet::BlockChange { protocol_version: PROTOCOL_VERSION, x, y, z, block }).await.is_err() {
                                let _ = client_to_game.send(ClientToGame::Disconnected { reason: "connection lost".into() });
                                return;
                            }
                        }
                        Ok(GameToClient::SendChat { message }) => {
                            if writer.send(&Packet::ChatMessage { protocol_version: PROTOCOL_VERSION, sender: username.clone(), message }).await.is_err() {
                                let _ = client_to_game.send(ClientToGame::Disconnected { reason: "connection lost".into() });
                                return;
                            }
                        }
                        Ok(GameToClient::Disconnect) => {
                            let _ = writer.send(&Packet::Disconnect { protocol_version: PROTOCOL_VERSION, reason: "client disconnect".into() }).await;
                            return;
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::server::{HostToServer, NetworkServer, ServerToHost};
    use std::net::TcpListener as StdTcpListener;
    use std::sync::mpsc;

    fn wait_for_event(rx: &Receiver<ClientToGame>) -> ClientToGame {
        rx.recv_timeout(Duration::from_secs(3))
            .expect("client event timed out")
    }

    #[test]
    fn connects_and_receives_join_for_second_client() {
        let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
        let addr = reserved.local_addr().unwrap().to_string();
        drop(reserved);
        let (host_tx, host_rx) = mpsc::channel();
        let (server_tx, server_rx) = mpsc::channel();
        let server = NetworkServer::spawn(addr.clone(), 0xCAFE_BABE, 1, host_rx, server_tx);

        let (game_tx_a, game_rx_a) = mpsc::channel();
        let (event_tx_a, event_rx_a) = mpsc::channel();
        let client_a = NetworkClient::spawn(addr.clone(), "steve".into(), game_rx_a, event_tx_a);
        let first = wait_for_event(&event_rx_a);
        let first_id = match first {
            ClientToGame::Connected {
                player_id,
                seed,
                gamemode,
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
        let (event_tx_b, event_rx_b) = mpsc::channel();
        let client_b = NetworkClient::spawn(addr, "alex".into(), game_rx_b, event_tx_b);
        let second_id = match wait_for_event(&event_rx_b) {
            ClientToGame::Connected { player_id, .. } => player_id,
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
            .send(HostToServer::NotifyPlayerJoin {
                id: second_id,
                username,
            })
            .unwrap();
        assert!(matches!(
            wait_for_event(&event_rx_a),
            ClientToGame::PlayerJoin { id, username } if id == second_id && username == "alex"
        ));

        game_tx_a.send(GameToClient::Disconnect).unwrap();
        game_tx_b.send(GameToClient::Disconnect).unwrap();
        let _ = client_a.join();
        let _ = client_b.join();
        host_tx.send(HostToServer::Stop).unwrap();
        let _ = server.join();
        assert_ne!(first_id, second_id);
    }

    /// Step 2 (Task 5) two-instance smoke test: when the host stops the server,
    /// the remaining client observes a `Disconnected` event and its background
    /// thread exits cleanly without hanging. This automates the "quitting either
    /// side cleans up the background thread without hanging" requirement that
    /// the two-window GUI scenario checks manually.
    #[test]
    fn host_stop_notifies_client_and_threads_join_without_hanging() {
        let reserved = StdTcpListener::bind("127.0.0.1:0").unwrap();
        let addr = reserved.local_addr().unwrap().to_string();
        drop(reserved);
        let (host_tx, host_rx) = mpsc::channel();
        let (server_tx, server_rx) = mpsc::channel();
        let server = NetworkServer::spawn(addr.clone(), 0xDEAD_BEEF, 0, host_rx, server_tx);

        let (_game_tx, game_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let client = NetworkClient::spawn(addr, "host_quit_witness".into(), game_rx, event_tx);

        match wait_for_event(&event_rx) {
            ClientToGame::Connected { seed, gamemode, .. } => {
                assert_eq!(seed, 0xDEAD_BEEF);
                assert_eq!(gamemode, 0);
            }
            other => panic!("expected Connected, got {other:?}"),
        }
        let _ = server_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("join event missing");

        // Host quits: stop the server. The client must be notified and exit.
        host_tx.send(HostToServer::Stop).unwrap();
        match event_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(ClientToGame::Disconnected { .. }) => {}
            Ok(other) => panic!("expected Disconnected, got {other:?}"),
            Err(_) => panic!("client did not observe disconnect after host stop"),
        }
        client
            .join()
            .expect("client thread panicked during host-stop shutdown");
        server
            .join()
            .expect("server thread panicked during shutdown");
    }
}
