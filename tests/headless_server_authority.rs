use icraft::authority::interest::InterestKind;
use icraft::dimension::Dimension;
use icraft::inventory::{Item, ItemStack};
use icraft::network::client::{ClientToGame, GameToClient, NetworkClient};
use icraft::network::protocol::{
    ContainerAction, GameplayOperation, GameplayOutcome, GameplayRequest, GameplayResponse,
    ItemWire, RejectReason,
};
use icraft::server_runtime::{ServerProperties, ServerRuntime};
use icraft::world::BlockType;
use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const STEP_SLEEP: Duration = Duration::from_millis(5);
const EVENT_TIMEOUT: Duration = Duration::from_secs(5);
const CHEST_POSITION: (i32, i32, i32) = (8, 80, 8);

static NEXT_WORLD: AtomicU64 = AtomicU64::new(0);

struct TempWorld {
    path: PathBuf,
}

impl TempWorld {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before Unix epoch")
            .as_nanos();
        let suffix = NEXT_WORLD.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "icraft-headless-authority-{}-{nonce}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create isolated headless test world");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempWorld {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct HeadlessClient {
    commands: Sender<GameToClient>,
    inbound: Receiver<ClientToGame>,
    events: VecDeque<ClientToGame>,
    connected: Option<(u64, u64, u8)>,
    thread: Option<thread::JoinHandle<()>>,
}

impl HeadlessClient {
    fn connect(address: &str, username: &str) -> Self {
        let (commands, command_rx) = mpsc::channel();
        let (event_tx, inbound) = mpsc::channel();
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

    fn send(&self, command: GameToClient) {
        self.commands
            .send(command)
            .expect("headless network client command queue is open");
    }

    fn drain(&mut self) {
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

    fn clear_events(&mut self) {
        self.drain();
        self.events.clear();
    }

    fn player_id(&self) -> Option<u64> {
        self.connected.map(|connected| connected.0)
    }

    fn take_response(&mut self, request_id: u128) -> Option<GameplayResponse> {
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

    fn take_open_result(
        &mut self,
        position: (i32, i32, i32),
    ) -> Option<(Vec<Option<ItemWire>>, u64)> {
        let index = self.events.iter().position(|event| {
            matches!(
                event,
                ClientToGame::ContainerOpenResult { x, y, z, .. }
                    if (*x, *y, *z) == position
            )
        })?;
        match self.events.remove(index)? {
            ClientToGame::ContainerOpenResult {
                success,
                slots,
                revision,
                ..
            } => {
                assert!(success, "authority reported a failed container open");
                Some((slots, revision))
            }
            _ => unreachable!("event index was selected as a container-open result"),
        }
    }

    fn take_click_result(&mut self, slot: u16) -> Option<(Option<ItemWire>, Option<ItemWire>)> {
        let index = self.events.iter().position(|event| {
            matches!(
                event,
                ClientToGame::ContainerClickResult { slot_index, .. }
                    if *slot_index == slot
            )
        })?;
        match self.events.remove(index)? {
            ClientToGame::ContainerClickResult {
                success,
                slot,
                dragged,
                ..
            } => {
                assert!(success, "authority reported a failed container click");
                Some((slot, dragged))
            }
            _ => unreachable!("event index was selected as a container-click result"),
        }
    }

    fn has_private_container_event(&self) -> bool {
        self.events.iter().any(|event| {
            matches!(
                event,
                ClientToGame::ContainerOpenResult { .. }
                    | ClientToGame::ContainerClickResult { .. }
                    | ClientToGame::ContainerSlotUpdate { .. }
            )
        })
    }

    fn disconnect_and_join(&mut self) {
        if let Some(handle) = self.thread.take() {
            let _ = self.commands.send(GameToClient::Disconnect);
            handle
                .join()
                .expect("headless network client thread panicked");
        }
    }
}

impl Drop for HeadlessClient {
    fn drop(&mut self) {
        self.disconnect_and_join();
    }
}

fn reserve_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve loopback test port");
    listener.local_addr().expect("read reserved port").port()
}

fn properties(world_dir: &Path, port: u16) -> ServerProperties {
    ServerProperties {
        bind: "127.0.0.1".into(),
        port,
        max_players: 2,
        view_distance: 2,
        simulation_distance: 2,
        world_dir: world_dir.to_path_buf(),
        seed: 0xC0FF_EE11,
        ..ServerProperties::default()
    }
}

fn drive_pair_until(
    runtime: &mut ServerRuntime,
    first: &mut HeadlessClient,
    second: &mut HeadlessClient,
    description: &str,
    mut ready: impl FnMut(&ServerRuntime, &HeadlessClient, &HeadlessClient) -> bool,
) {
    let deadline = Instant::now() + EVENT_TIMEOUT;
    loop {
        runtime.tick().expect("headless authority tick succeeds");
        first.drain();
        second.drain();
        if ready(runtime, first, second) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {description}; first={:?}, second={:?}, players={}",
            first.connected,
            second.connected,
            runtime.players.len()
        );
        thread::sleep(STEP_SLEEP);
    }
}

fn drive_one_until(
    runtime: &mut ServerRuntime,
    client: &mut HeadlessClient,
    description: &str,
    mut ready: impl FnMut(&ServerRuntime, &HeadlessClient) -> bool,
) {
    let deadline = Instant::now() + EVENT_TIMEOUT;
    loop {
        runtime.tick().expect("headless authority tick succeeds");
        client.drain();
        if ready(runtime, client) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {description}; connected={:?}, players={}",
            client.connected,
            runtime.players.len()
        );
        thread::sleep(STEP_SLEEP);
    }
}

fn drive_pair_for(
    runtime: &mut ServerRuntime,
    first: &mut HeadlessClient,
    second: &mut HeadlessClient,
    duration: Duration,
) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        runtime.tick().expect("headless authority tick succeeds");
        first.drain();
        second.drain();
        thread::sleep(STEP_SLEEP);
    }
}

fn wait_for_response(
    runtime: &mut ServerRuntime,
    client: &mut HeadlessClient,
    observer: &mut HeadlessClient,
    request_id: u128,
) -> GameplayResponse {
    let deadline = Instant::now() + EVENT_TIMEOUT;
    loop {
        runtime.tick().expect("headless authority tick succeeds");
        client.drain();
        observer.drain();
        if let Some(response) = client.take_response(request_id) {
            return response;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for gameplay response {request_id}"
        );
        thread::sleep(STEP_SLEEP);
    }
}

fn request(
    request_id: u128,
    client_sequence: u64,
    client_revision: u64,
    operation: GameplayOperation,
) -> GameplayRequest {
    GameplayRequest {
        request_id,
        client_sequence,
        session_id: 0,
        dimension: Dimension::Overworld as u8,
        client_revision,
        operation,
    }
}

fn accepted_revision(response: &GameplayResponse) -> u64 {
    match response.outcome {
        GameplayOutcome::Accepted { revision } => revision,
        GameplayOutcome::Rejected { reason } => {
            panic!("request {} was rejected: {reason:?}", response.request_id)
        }
    }
}

#[test]
fn two_clients_share_headless_authority_with_revision_interest_and_reconnect() {
    let world = TempWorld::new();
    let port = reserve_port();
    let server_properties = properties(world.path(), port);
    let address = format!("127.0.0.1:{port}");
    let mut runtime =
        ServerRuntime::new(server_properties.clone()).expect("start headless authority runtime");
    let mut alice = HeadlessClient::connect(&address, "alice");
    let mut bob = HeadlessClient::connect(&address, "bob");

    drive_pair_until(
        &mut runtime,
        &mut alice,
        &mut bob,
        "two authenticated clients",
        |runtime, alice, bob| {
            alice.player_id().is_some() && bob.player_id().is_some() && runtime.players.len() == 2
        },
    );
    let alice_id = alice.player_id().expect("alice authenticated");
    let bob_id = bob.player_id().expect("bob authenticated");
    assert_ne!(alice_id, bob_id);
    assert_eq!(alice.connected.map(|value| value.1), Some(0xC0FF_EE11));
    assert_eq!(bob.connected.map(|value| value.1), Some(0xC0FF_EE11));

    alice.send(GameToClient::SendPosition {
        sequence: 1,
        sender_time_millis: 1,
        x: 8.0,
        y: 80.0,
        z: 8.0,
        yaw: 0.0,
        pitch: 0.0,
    });
    bob.send(GameToClient::SendPosition {
        sequence: 1,
        sender_time_millis: 1,
        x: 512.0,
        y: 80.0,
        z: 512.0,
        yaw: 0.0,
        pitch: 0.0,
    });
    drive_pair_until(
        &mut runtime,
        &mut alice,
        &mut bob,
        "dimension-aware interest positions",
        |runtime, _, _| {
            runtime
                .players
                .get(&alice_id)
                .is_some_and(|player| player.data.position == [8.0, 80.0, 8.0])
                && runtime
                    .players
                    .get(&bob_id)
                    .is_some_and(|player| player.data.position == [512.0, 80.0, 512.0])
        },
    );
    runtime.drain_routed_updates();
    alice.clear_events();
    bob.clear_events();

    const BLOCK_REQUEST: u128 = 0xA001;
    let base_revision = runtime.authority.current_revision();
    let block_request = request(
        BLOCK_REQUEST,
        1,
        base_revision,
        GameplayOperation::BlockUse {
            x: CHEST_POSITION.0,
            y: CHEST_POSITION.1,
            z: CHEST_POSITION.2,
            block: BlockType::Chest.to_wire(),
        },
    );
    alice.send(GameToClient::GameplayRequest {
        request: block_request.clone(),
    });
    let block_response = wait_for_response(&mut runtime, &mut alice, &mut bob, BLOCK_REQUEST);
    let block_revision = accepted_revision(&block_response);
    assert!(block_revision > base_revision);
    assert_eq!(
        runtime
            .authority
            .world
            .get_block(CHEST_POSITION.0, CHEST_POSITION.1, CHEST_POSITION.2),
        BlockType::Chest
    );

    let block_targets: BTreeSet<_> = runtime
        .drain_routed_updates()
        .into_iter()
        .filter_map(|update| {
            (update.dimension == Dimension::Overworld
                && update.kind == InterestKind::Block(CHEST_POSITION))
            .then_some(update.target)
        })
        .collect();
    assert_eq!(
        block_targets,
        BTreeSet::from([alice_id]),
        "the distant client must not enter the block mutation's interest route"
    );

    let accepted_before_replay = runtime.metrics.requests_accepted;
    let rejected_before_replay = runtime.metrics.requests_rejected;
    let duplicate_before_replay = runtime.metrics.duplicate_requests;
    alice.send(GameToClient::GameplayRequest {
        request: block_request,
    });
    drive_pair_for(
        &mut runtime,
        &mut alice,
        &mut bob,
        Duration::from_millis(250),
    );
    assert!(
        alice.take_response(BLOCK_REQUEST).is_none(),
        "the client response gate must suppress a replayed cached response"
    );
    assert_eq!(
        (
            runtime.metrics.requests_accepted,
            runtime.metrics.requests_rejected,
            runtime.metrics.duplicate_requests,
        ),
        (
            accepted_before_replay,
            rejected_before_replay,
            duplicate_before_replay,
        ),
        "the transport replay cache must prevent a second authority dispatch"
    );
    assert_eq!(
        runtime
            .authority
            .session(alice_id)
            .and_then(|session| session.cached_response(BLOCK_REQUEST)),
        Some(block_response),
        "the original authority response remains the sole cached execution"
    );

    const OUT_OF_ORDER_REQUEST: u128 = 0xA002;
    alice.send(GameToClient::GameplayRequest {
        request: request(
            OUT_OF_ORDER_REQUEST,
            1,
            block_revision,
            GameplayOperation::ItemUse { item: 1, count: 1 },
        ),
    });
    let out_of_order = wait_for_response(&mut runtime, &mut alice, &mut bob, OUT_OF_ORDER_REQUEST);
    assert_eq!(
        out_of_order.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::OutOfOrder
        }
    );

    const STALE_REQUEST: u128 = 0xA003;
    alice.send(GameToClient::GameplayRequest {
        request: request(
            STALE_REQUEST,
            2,
            0,
            GameplayOperation::ItemUse { item: 1, count: 1 },
        ),
    });
    let stale = wait_for_response(&mut runtime, &mut alice, &mut bob, STALE_REQUEST);
    assert_eq!(
        stale.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::InvalidRevision
        }
    );
    assert_eq!(runtime.metrics.requests_accepted, accepted_before_replay);

    bob.send(GameToClient::SendPosition {
        sequence: 2,
        sender_time_millis: 2,
        x: 10.0,
        y: 80.0,
        z: 8.0,
        yaw: 0.0,
        pitch: 0.0,
    });
    drive_pair_until(
        &mut runtime,
        &mut alice,
        &mut bob,
        "nearby non-viewer interest",
        |runtime, _, _| {
            runtime
                .players
                .get(&bob_id)
                .is_some_and(|player| player.data.position == [10.0, 80.0, 8.0])
        },
    );
    runtime.drain_routed_updates();
    alice.clear_events();
    bob.clear_events();

    const OPEN_REQUEST: u128 = 0xA004;
    alice.send(GameToClient::GameplayRequest {
        request: request(
            OPEN_REQUEST,
            3,
            block_revision,
            GameplayOperation::Container {
                action: ContainerAction::Open.to_wire(),
                x: CHEST_POSITION.0,
                y: CHEST_POSITION.1,
                z: CHEST_POSITION.2,
                slot: 0,
            },
        ),
    });
    let open_response = wait_for_response(&mut runtime, &mut alice, &mut bob, OPEN_REQUEST);
    let open_revision = accepted_revision(&open_response);
    drive_pair_until(
        &mut runtime,
        &mut alice,
        &mut bob,
        "container-open projection",
        |_, alice, _| {
            alice.events.iter().any(|event| {
                matches!(
                    event,
                    ClientToGame::ContainerOpenResult { x, y, z, .. }
                        if (*x, *y, *z) == CHEST_POSITION
                )
            })
        },
    );
    let (slots, projected_open_revision) = alice
        .take_open_result(CHEST_POSITION)
        .expect("alice receives the container contents");
    assert_eq!(slots.len(), 27);
    assert_eq!(projected_open_revision, open_revision);
    let open_updates = runtime.drain_routed_updates();
    let block_entity_targets: BTreeSet<_> = open_updates
        .iter()
        .filter_map(|update| {
            (update.kind == InterestKind::BlockEntity(CHEST_POSITION)).then_some(update.target)
        })
        .collect();
    let container_targets: BTreeSet<_> = open_updates
        .iter()
        .filter_map(|update| {
            (update.kind == InterestKind::Container(CHEST_POSITION)).then_some(update.target)
        })
        .collect();
    assert_eq!(block_entity_targets, BTreeSet::from([alice_id, bob_id]));
    assert_eq!(
        container_targets,
        BTreeSet::from([alice_id]),
        "container contents are routed only to authenticated viewers"
    );
    drive_pair_for(
        &mut runtime,
        &mut alice,
        &mut bob,
        Duration::from_millis(100),
    );
    assert!(
        !bob.has_private_container_event(),
        "a nearby non-viewer received private container state"
    );

    alice.clear_events();
    bob.clear_events();
    runtime.drain_routed_updates();
    const CLICK_REQUEST: u128 = 0xA005;
    let stone = ItemWire::from_stack(&ItemStack::new(Item::Stone, 2));
    alice.send(GameToClient::GameplayRequest {
        request: request(
            CLICK_REQUEST,
            4,
            open_revision,
            GameplayOperation::ContainerClick {
                x: CHEST_POSITION.0,
                y: CHEST_POSITION.1,
                z: CHEST_POSITION.2,
                slot: 0,
                is_left: true,
                dragged: Some(stone),
            },
        ),
    });
    let click_response = wait_for_response(&mut runtime, &mut alice, &mut bob, CLICK_REQUEST);
    let click_revision = accepted_revision(&click_response);
    drive_pair_until(
        &mut runtime,
        &mut alice,
        &mut bob,
        "container-click projection",
        |_, alice, _| {
            alice.events.iter().any(|event| {
                matches!(
                    event,
                    ClientToGame::ContainerClickResult { slot_index: 0, .. }
                )
            })
        },
    );
    let (slot, dragged) = alice
        .take_click_result(0)
        .expect("alice receives the authoritative clicked slot");
    assert_eq!(slot, Some(stone));
    assert_eq!(dragged, Some(stone));
    let click_container_targets: BTreeSet<_> = runtime
        .drain_routed_updates()
        .iter()
        .filter_map(|update| {
            (update.kind == InterestKind::Container(CHEST_POSITION)).then_some(update.target)
        })
        .collect();
    assert_eq!(click_container_targets, BTreeSet::from([alice_id]));
    drive_pair_for(
        &mut runtime,
        &mut alice,
        &mut bob,
        Duration::from_millis(100),
    );
    assert!(
        !bob.has_private_container_event(),
        "a non-viewer received a container slot mutation"
    );

    alice.disconnect_and_join();
    bob.disconnect_and_join();
    drive_pair_until(
        &mut runtime,
        &mut alice,
        &mut bob,
        "disconnect persistence",
        |runtime, _, _| runtime.players.is_empty(),
    );
    runtime.shutdown().expect("save and stop first runtime");
    drop(runtime);

    let mut restarted =
        ServerRuntime::new(server_properties).expect("restart authority from persisted world");
    assert_eq!(
        restarted
            .authority
            .world
            .get_block(CHEST_POSITION.0, CHEST_POSITION.1, CHEST_POSITION.2),
        BlockType::Chest
    );
    assert_eq!(
        restarted
            .authority
            .world
            .container_slot_wire(CHEST_POSITION, 0),
        Some(Some(stone)),
        "container mutation must survive server restart exactly once"
    );
    assert!(
        restarted
            .authority
            .world
            .container_viewers_at(CHEST_POSITION)
            .next()
            .is_none(),
        "ephemeral container viewers must not leak across restart"
    );
    assert!(restarted.authority.current_revision() >= click_revision);

    let mut reconnected = HeadlessClient::connect(&address, "alice");
    drive_one_until(
        &mut restarted,
        &mut reconnected,
        "saved player reconnect",
        |runtime, client| {
            client.player_id().is_some()
                && client
                    .player_id()
                    .and_then(|id| runtime.players.get(&id))
                    .is_some_and(|player| player.data.position == [8.0, 80.0, 8.0])
        },
    );
    let reconnected_id = reconnected.player_id().expect("alice reconnected");
    assert_eq!(
        restarted.players[&reconnected_id].dimension,
        Dimension::Overworld
    );
    reconnected.disconnect_and_join();
    drive_one_until(
        &mut restarted,
        &mut reconnected,
        "reconnected player logout",
        |runtime, _| runtime.players.is_empty(),
    );
    restarted.shutdown().expect("stop restarted runtime");
}
