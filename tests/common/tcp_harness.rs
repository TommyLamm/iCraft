use icraft::network::client::{ClientToGame, GameToClient, NetworkClient};
use icraft::network::protocol::{GameplayRequest, GameplayResponse};
use icraft::server_runtime::ServerRuntime;
use std::collections::VecDeque;
use std::net::{SocketAddr, TcpListener};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

/// Loopback port reservation that keeps the OS listener until the server bind.
///
/// Dropping a `TcpListener` before the server binds is a TOCTOU hole: another
/// process can steal the ephemeral port. Hold this until the caller is ready
/// to bind, then `release()` immediately before that bind, or keep the
/// listener if the server can take over the already-bound socket.
pub struct HeldLoopback {
    listener: Option<TcpListener>,
    addr: SocketAddr,
}

impl HeldLoopback {
    pub fn bind() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("reserve loopback test port");
        let addr = listener.local_addr().expect("read loopback port");
        Self {
            listener: Some(listener),
            addr,
        }
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Drop the reservation so a server can bind this exact port.
    pub fn release(mut self) -> u16 {
        drop(self.listener.take());
        self.addr.port()
    }
}

pub const STEP_SLEEP: Duration = Duration::from_millis(5);
pub const EVENT_TIMEOUT: Duration = Duration::from_secs(30);

/// A small, deterministic wrapper around one real `NetworkClient` socket.
/// Keeping the queue/event logic here prevents each topology vector from
/// inventing its own transport wait semantics.
pub struct TcpClient {
    commands: Sender<GameToClient>,
    inbound: Receiver<ClientToGame>,
    events: VecDeque<ClientToGame>,
    connected: Option<(u64, u64, u8)>,
    thread: Option<thread::JoinHandle<()>>,
}

impl TcpClient {
    pub fn connect(address: &str, username: &str) -> Self {
        let (commands, command_rx) = mpsc::channel();
        let (event_tx, inbound) =
            mpsc::sync_channel(icraft::network::client::CLIENT_TO_GAME_QUEUE_CAPACITY);
        let client_thread = NetworkClient::spawn(
            address.to_string(),
            username.to_string(),
            command_rx,
            event_tx,
        );
        Self {
            commands,
            inbound,
            events: VecDeque::new(),
            connected: None,
            thread: Some(client_thread),
        }
    }

    pub fn send(&self, command: GameToClient) {
        self.commands
            .send(command)
            .expect("TCP test client command queue is open");
    }

    pub fn send_position(
        &self,
        sequence: u32,
        sender_time_millis: u64,
        position: [f32; 3],
        yaw: f32,
        pitch: f32,
    ) {
        self.send(GameToClient::SendPosition {
            sequence,
            sender_time_millis,
            x: position[0],
            y: position[1],
            z: position[2],
            yaw,
            pitch,
        });
    }

    pub fn send_request(&self, request: GameplayRequest) {
        self.send(GameToClient::GameplayRequest { request });
    }

    pub fn drain(&mut self) {
        while let Ok(event) = self.inbound.try_recv() {
            if let ClientToGame::Connected {
                player_id,
                seed,
                gamemode,
            } = &event
            {
                self.connected = Some((*player_id, *seed, *gamemode));
            }
            self.events.push_back(event);
        }
    }

    pub fn clear_events(&mut self) {
        self.drain();
        self.events.clear();
    }

    pub fn player_id(&self) -> Option<u64> {
        self.connected.map(|connected| connected.0)
    }

    pub fn events(&self) -> &VecDeque<ClientToGame> {
        &self.events
    }

    pub fn take_response(&mut self, request_id: u128) -> Option<GameplayResponse> {
        let index = self.events.iter().position(|event| {
            matches!(
                event,
                ClientToGame::GameplayResponse { response }
                    if response.request_id == request_id
            )
        })?;
        match self.events.remove(index)? {
            ClientToGame::GameplayResponse { response } => Some(response),
            _ => unreachable!("event index was selected as a gameplay response"),
        }
    }

    pub fn has_session_update(&self, player_id: u64) -> bool {
        self.events.iter().any(|event| {
            matches!(
                event,
                ClientToGame::PlayerSessionUpdate {
                    player_id: event_player,
                    ..
                } if *event_player == player_id
            )
        })
    }

    pub fn disconnect_and_join(&mut self) {
        if let Some(handle) = self.thread.take() {
            let _ = self.commands.send(GameToClient::Disconnect);
            handle.join().expect("TCP test client thread panicked");
        }
    }
}

impl Drop for TcpClient {
    fn drop(&mut self) {
        self.disconnect_and_join();
    }
}

/// Tick the authoritative runtime while draining every socket in a topology.
/// The callback receives immutable views only after the tick/drain boundary,
/// so callers cannot accidentally mutate authority outside the transport.
pub fn drive_until(
    runtime: &mut ServerRuntime,
    clients: &mut [&mut TcpClient],
    description: &str,
    mut ready: impl FnMut(&ServerRuntime, &[&TcpClient]) -> bool,
) {
    let deadline = Instant::now() + EVENT_TIMEOUT;
    loop {
        runtime.tick().expect("headless authority tick succeeds");
        for client in clients.iter_mut() {
            client.drain();
        }
        let views: Vec<&TcpClient> = clients.iter().map(|client| &**client).collect();
        if ready(runtime, &views) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {description}; players={}; metrics={:?}; client_events={:?}",
            runtime.players.len(),
            runtime.metrics(),
            views
                .iter()
                .map(|client| client.events())
                .collect::<Vec<_>>(),
        );
        thread::sleep(STEP_SLEEP);
    }
}

pub fn wait_for_response(
    runtime: &mut ServerRuntime,
    clients: &mut [&mut TcpClient],
    owner_index: usize,
    request_id: u128,
) -> GameplayResponse {
    let deadline = Instant::now() + EVENT_TIMEOUT;
    loop {
        runtime.tick().expect("headless authority tick succeeds");
        for client in clients.iter_mut() {
            client.drain();
        }
        if let Some(response) = clients[owner_index].take_response(request_id) {
            return response;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for gameplay response {request_id}; metrics={:?}; owner_events={:?}",
            runtime.metrics(),
            clients[owner_index].events(),
        );
        thread::sleep(STEP_SLEEP);
    }
}
