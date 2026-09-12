use icraft::authority::contract::{SessionGameplayState, SessionInventorySlot};
use icraft::dimension::Dimension;
use icraft::inventory::ItemStack;
use icraft::network::client::{ClientToGame, GameToClient, NetworkClient};
use icraft::network::protocol::{
    GameplayOperation, GameplayRequest, GameplayResponse, ItemWire, SessionSlotWire, SlotRefWire,
};
use icraft::server_runtime::{ServerProperties, ServerRuntime};
use std::collections::VecDeque;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

/// Connect to a freshly spawned loopback server, retrying Windows self-connect.
///
/// On Windows, connecting before the server has bound can transiently
/// self-connect when the reserved server port is selected as the client's
/// ephemeral port. Reject those sockets and retry until a real peer appears.
pub fn connect_loopback_std(addr: &str) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match TcpStream::connect(addr) {
            Ok(stream) if stream.local_addr().ok() != stream.peer_addr().ok() => {
                stream.set_nodelay(true).ok();
                return stream;
            }
            Ok(_) | Err(_) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10));
            }
            Ok(_) => panic!("server did not start before the connection deadline"),
            Err(error) => panic!("server did not start: {error}"),
        }
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
        use icraft::network::protocol::Packet;
        while let Ok(event) = self.inbound.try_recv() {
            if let ClientToGame::Packet(Packet::LoginSuccess {
                player_id,
                seed,
                gamemode,
                ..
            }) = &event
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

    pub fn connected(&self) -> Option<(u64, u64, u8)> {
        self.connected
    }

    pub fn events(&self) -> &VecDeque<ClientToGame> {
        &self.events
    }

    pub fn take_open_result(
        &mut self,
        position: (i32, i32, i32),
    ) -> Option<(Vec<Option<ItemWire>>, u64)> {
        use icraft::network::protocol::Packet;
        let index = self.events.iter().position(|event| {
            matches!(
                event,
                ClientToGame::Packet(Packet::ContainerOpenResult { x, y, z, .. })
                    if (*x, *y, *z) == position
            )
        })?;
        match self.events.remove(index)? {
            ClientToGame::Packet(Packet::ContainerOpenResult {
                success,
                slots,
                revision,
                ..
            }) => {
                assert!(success, "authority reported a failed container open");
                Some((slots, revision))
            }
            _ => unreachable!("event index was selected as a container-open result"),
        }
    }

    pub fn take_click_result(&mut self, slot: u16) -> Option<(Option<ItemWire>, Option<ItemWire>)> {
        use icraft::network::protocol::Packet;
        let index = self.events.iter().position(|event| {
            matches!(
                event,
                ClientToGame::Packet(Packet::ContainerClickResult { slot_index, .. })
                    if *slot_index == slot
            )
        })?;
        match self.events.remove(index)? {
            ClientToGame::Packet(Packet::ContainerClickResult {
                success,
                slot,
                dragged,
                ..
            }) => {
                assert!(success, "authority reported a failed container click");
                Some((slot, dragged))
            }
            _ => unreachable!("event index was selected as a container-click result"),
        }
    }

    pub fn has_private_container_event(&self) -> bool {
        use icraft::network::protocol::Packet;
        self.events.iter().any(|event| {
            matches!(
                event,
                ClientToGame::Packet(Packet::ContainerOpenResult { .. })
                    | ClientToGame::Packet(Packet::ContainerClickResult { .. })
                    | ClientToGame::Packet(Packet::ContainerSlotUpdate { .. })
            )
        })
    }

    pub fn take_response(&mut self, request_id: u128) -> Option<GameplayResponse> {
        use icraft::network::protocol::Packet;
        let index = self.events.iter().position(|event| {
            matches!(
                event,
                ClientToGame::Packet(Packet::GameplayResponse { response, .. })
                    if response.request_id == request_id
            )
        })?;
        match self.events.remove(index)? {
            ClientToGame::Packet(Packet::GameplayResponse { response, .. }) => Some(response),
            _ => unreachable!("event index was selected as a gameplay response"),
        }
    }

    pub fn has_session_update(&self, player_id: u64) -> bool {
        use icraft::network::protocol::Packet;
        self.events.iter().any(|event| {
            matches!(
                event,
                ClientToGame::Packet(Packet::PlayerSessionUpdate {
                    player_id: event_player,
                    ..
                }) if *event_player == player_id
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

/// Isolated temp world directory. `prefix` is the stem after `icraft-`,
/// e.g. `plan30-listen` → `icraft-plan30-listen-{nanos}`.
pub fn temp_world(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("icraft-{prefix}-{nanos}"))
}

/// Shared loopback defaults for the majority TCP cluster.
///
/// Sets bind, `max_players=4`, `view_distance=2`, and `simulation_distance=2`.
/// Does not force a seed or port — callers must set those (and any view/sim/
/// player-cap overrides) themselves.
pub fn loopback_properties(
    world_dir: impl Into<PathBuf>,
    bind_addr: impl Into<String>,
) -> ServerProperties {
    ServerProperties {
        bind: bind_addr.into(),
        max_players: 4,
        view_distance: 2,
        simulation_distance: 2,
        world_dir: world_dir.into(),
        ..ServerProperties::default()
    }
}

/// Runtime-aware gameplay request using the live session dimension + revision.
pub fn gameplay_request(
    runtime: &ServerRuntime,
    player_id: u64,
    request_id: u128,
    sequence: u64,
    operation: GameplayOperation,
) -> GameplayRequest {
    let dimension = runtime
        .authority
        .session(player_id)
        .and_then(|session| Dimension::from_wire(session.dimension))
        .expect("request session dimension");
    GameplayRequest {
        request_id,
        client_sequence: sequence,
        session_id: player_id,
        dimension: dimension as u8,
        client_revision: runtime.authority.revision_for_dimension(dimension),
        operation,
    }
}

pub fn session_slot(stack: ItemStack) -> SessionInventorySlot {
    SessionInventorySlot::from_stack(&stack).expect("fixture stack within u16 count")
}

pub fn held(stack: &ItemStack) -> SessionSlotWire {
    SessionSlotWire::new(
        ItemWire::from_stack(stack),
        stack.can_break,
        stack.can_place_on,
    )
}

pub fn source(state: &SessionGameplayState, index: u8, count: u16) -> SlotRefWire {
    SlotRefWire {
        index,
        count,
        expected: state.inventory[usize::from(index)]
            .expect("fixture source slot")
            .into(),
    }
}

pub fn current_revision(runtime: &ServerRuntime, id: u64) -> u64 {
    let dimension = runtime
        .authority
        .session(id)
        .and_then(|session| Dimension::from_wire(session.dimension))
        .expect("fixture session dimension");
    runtime.authority.revision_for_dimension(dimension)
}

pub fn seeded_properties(prefix: &str, seed: u64) -> ServerProperties {
    let mut properties = loopback_properties(temp_world(prefix), "127.0.0.1");
    properties.seed = seed;
    properties
}

