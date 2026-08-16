use glam::Vec3;
use icraft::authority::contract::{AuthorityTopology, SessionGameplayState, SessionInventorySlot};
use icraft::authority::transactions::BREW_TICKS;
use icraft::block_entity::{BlockEntity, FurnaceBlockEntity};
use icraft::dimension::Dimension;
use icraft::entity::EntityType;
use icraft::inventory::{GameMode, Inventory};
use icraft::network::protocol::{
    GameplayOperation, GameplayOutcome, GameplayRequest, GameplayResponse, ItemWire, RejectReason,
    SlotRefWire,
};
use icraft::server_runtime::{
    EmbeddedRuntimeOptions, LocalSessionProfile, LocalSessionStorage, RuntimeInput,
    RuntimePresentationEvent, RuntimeTickOutput, ServerProperties, ServerRuntime, TransportMode,
};
use icraft::{
    player::PlayerState, save::LevelData, save::PlayerData, save::SaveManager, world::BlockType,
};
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_world(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("icraft_runtime_topology_{label}_{unique}"))
}

fn available_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve an ephemeral test port");
    listener.local_addr().unwrap().port()
}

fn properties(label: &str) -> ServerProperties {
    ServerProperties {
        bind: "127.0.0.1".into(),
        port: available_port(),
        world_dir: temp_world(label),
        ..ServerProperties::default()
    }
}

fn leftover_block_use(session_id: u64, client_revision: u64, request_id: u128) -> GameplayRequest {
    GameplayRequest {
        request_id,
        client_sequence: 1,
        session_id,
        dimension: Dimension::Overworld as u8,
        client_revision,
        operation: GameplayOperation::BlockUse {
            x: 8,
            y: 80,
            z: 8,
            block: BlockType::DiamondOre.to_wire(),
        },
    }
}

fn response_for(
    events: &[RuntimePresentationEvent],
    target: u64,
    request_id: u128,
) -> Option<&GameplayResponse> {
    events.iter().find_map(|event| match event {
        RuntimePresentationEvent::GameplayResponse {
            target: event_target,
            response,
        } if *event_target == target && response.request_id == request_id => Some(response),
        _ => None,
    })
}

const TOPOLOGY_SESSION_ID: u64 = u64::MAX - 20;
const TOPOLOGY_VICTIM_ID: u64 = u64::MAX - 21;

struct TopologyHarness {
    runtime: ServerRuntime,
    input: RuntimeInput,
    session_id: u64,
    next_request_id: u128,
    next_sequence: u64,
}

impl TopologyHarness {
    fn new(label: &str, topology: AuthorityTopology, transport: TransportMode) -> Self {
        let mut properties = properties(label);
        properties.pvp = true;
        // The network thread is part of the listen topology even though this
        // vector drives the local session through the same bounded runtime
        // input FIFO.  A reserved ephemeral port keeps parallel runs isolated.
        if transport == TransportMode::Listen {
            properties.port = available_port();
        }
        let (mut runtime, input) = ServerRuntime::new_embedded(
            properties,
            EmbeddedRuntimeOptions {
                topology,
                transport,
                local_session: Some(LocalSessionProfile::new(TOPOLOGY_SESSION_ID, "vector")),
            },
        )
        .unwrap();
        // Drain the login projection before the first request.  Subsequent
        // outputs are one fixed tick, one request/ACK/snapshot transaction.
        let _ = runtime.tick_with_output().unwrap();
        Self {
            runtime,
            input,
            session_id: TOPOLOGY_SESSION_ID,
            next_request_id: 0x24_000,
            next_sequence: 1,
        }
    }

    fn current_revision(&self) -> u64 {
        self.runtime
            .authority
            .revision_for_dimension(Dimension::Overworld)
    }

    fn request(&mut self, operation: GameplayOperation) -> (GameplayRequest, RuntimeTickOutput) {
        let request = GameplayRequest {
            request_id: self.next_request_id,
            client_sequence: self.next_sequence,
            session_id: self.session_id,
            dimension: Dimension::Overworld as u8,
            client_revision: self.current_revision(),
            operation,
        };
        self.next_request_id = self.next_request_id.saturating_add(1);
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.input
            .submit_request(self.session_id, request.clone())
            .unwrap();
        let output = self.runtime.tick_with_output().unwrap();
        (request, output)
    }

    fn replay(&mut self, request: GameplayRequest) -> RuntimeTickOutput {
        self.input.submit_request(self.session_id, request).unwrap();
        self.runtime.tick_with_output().unwrap()
    }

    fn source(&self, index: u8, count: u16) -> SlotRefWire {
        let state = self
            .runtime
            .authority
            .session(self.session_id)
            .unwrap()
            .gameplay;
        SlotRefWire {
            index,
            count,
            expected: state.inventory[usize::from(index)].unwrap().into(),
        }
    }

    fn session_state(&self) -> SessionGameplayState {
        self.runtime
            .authority
            .session(self.session_id)
            .unwrap()
            .gameplay
    }

    fn shutdown(mut self) {
        let world_dir = self.runtime.properties.world_dir.clone();
        self.runtime.shutdown().unwrap();
        drop(self.input);
        drop(self.runtime);
        let _ = fs::remove_dir_all(world_dir);
    }
}

fn session_slot(stack: icraft::inventory::ItemStack) -> SessionInventorySlot {
    SessionInventorySlot::from_wire(
        ItemWire::from_stack(&stack),
        stack.can_break,
        stack.can_place_on,
    )
}

fn prepare_topology_fixture(harness: &mut TopologyHarness) {
    let furnace_position = [8, 80, 9];
    let brew_position = [8, 80, 10];
    let enchanting_position = [8, 80, 11];
    let anvil_position = [8, 80, 12];
    for (position, block) in [
        (furnace_position, BlockType::Furnace),
        (brew_position, BlockType::BrewingStand),
        (enchanting_position, BlockType::EnchantingTable),
        (anvil_position, BlockType::Anvil),
    ] {
        harness
            .runtime
            .authority
            .world
            .set_block(position[0], position[1], position[2], block, 0)
            .unwrap();
    }
    let mut furnace = FurnaceBlockEntity::new();
    furnace.slots[2] = Some(icraft::inventory::ItemStack::new(
        icraft::inventory::Item::IronIngot,
        2,
    ));
    furnace.accumulated_xp = 4.0;
    harness.runtime.authority.world.chunks.set_block_entity(
        furnace_position[0],
        furnace_position[1],
        furnace_position[2],
        Some(BlockEntity::Furnace(furnace)),
    );

    let mut gameplay = harness.session_state();
    gameplay.inventory[0] = Some(session_slot(icraft::inventory::ItemStack::new(
        icraft::inventory::Item::DiamondSword,
        1,
    )));
    gameplay.inventory[1] = Some(session_slot(icraft::inventory::ItemStack::new(
        icraft::inventory::Item::OakPlanks,
        2,
    )));
    gameplay.inventory[2] = Some(session_slot(icraft::inventory::ItemStack::new(
        icraft::inventory::Item::Potion,
        1,
    )));
    gameplay.inventory[3] = Some(session_slot(icraft::inventory::ItemStack::new(
        icraft::inventory::Item::IronPickaxe,
        1,
    )));
    gameplay.inventory[4] = Some(session_slot(icraft::inventory::ItemStack::new(
        icraft::inventory::Item::LapisLazuli,
        3,
    )));
    gameplay.inventory[5] = Some(session_slot(icraft::inventory::ItemStack::new(
        icraft::inventory::Item::NetherWart,
        1,
    )));
    gameplay.inventory[40] = Some(session_slot(icraft::inventory::ItemStack::new(
        icraft::inventory::Item::FishingRod,
        1,
    )));
    gameplay.selected_hotbar_slot = 0;
    gameplay.experience_level = 30;
    gameplay.enchant_seed = 42;
    gameplay.attack_cooldown_ticks = 5;
    assert!(harness
        .runtime
        .authority
        .set_session_gameplay(harness.session_id, gameplay));

    harness
        .runtime
        .login_session(TOPOLOGY_VICTIM_ID, "victim")
        .unwrap();
    let victim = harness
        .runtime
        .players
        .get_mut(&TOPOLOGY_VICTIM_ID)
        .unwrap();
    victim.data.position = [8.0, 80.0, 9.0];
    harness
        .runtime
        .authority
        .session_mut(TOPOLOGY_VICTIM_ID)
        .unwrap()
        .position = [8.0, 80.0, 9.0];
    let mut victim_gameplay = harness
        .runtime
        .authority
        .session(TOPOLOGY_VICTIM_ID)
        .unwrap()
        .gameplay;
    victim_gameplay.health_milli = 1_000;
    victim_gameplay.inventory[0] = Some(session_slot(icraft::inventory::ItemStack::new(
        icraft::inventory::Item::Diamond,
        1,
    )));
    assert!(harness
        .runtime
        .authority
        .set_session_gameplay(TOPOLOGY_VICTIM_ID, victim_gameplay));
}

fn prepare_dispenser_fixture(harness: &mut TopologyHarness) -> ItemWire {
    let source = (8, 80, 8);
    let front = (9, 80, 8);
    let lever = (7, 80, 8);
    harness
        .runtime
        .authority
        .world
        .set_block(source.0, source.1, source.2, BlockType::Dispenser, 0)
        .unwrap();
    harness
        .runtime
        .authority
        .world
        .set_block(front.0, front.1, front.2, BlockType::Air, 0)
        .unwrap();
    harness
        .runtime
        .authority
        .world
        .set_block(lever.0, lever.1, lever.2, BlockType::LeverOn, 0)
        .unwrap();

    let mut stack = icraft::inventory::ItemStack::new(icraft::inventory::Item::Stone, 2)
        .with_can_break(BlockType::Dirt)
        .with_can_place_on(BlockType::Stone);
    stack.custom_name.set("topology-drop");
    if let Some(BlockEntity::Dispenser(dispenser)) = harness
        .runtime
        .authority
        .world
        .chunks
        .get_block_entity_mut(source.0, source.1, source.2)
    {
        let mut entity = icraft::block_entity::DispenserBlockEntity::new();
        entity.slots[0] = Some(stack);
        *dispenser = entity;
    } else {
        panic!("dispenser block entity fixture is missing");
    }
    harness.runtime.authority.world.redstone.on_block_changed(
        &harness.runtime.authority.world.chunks,
        lever,
        icraft::redstone::Direction::East,
    );
    harness.runtime.authority.world.redstone.on_block_changed(
        &harness.runtime.authority.world.chunks,
        source,
        icraft::redstone::Direction::East,
    );
    assert!(harness
        .runtime
        .teleport_session(harness.session_id, [8.0, 80.0, 8.0]));

    let mut expected = ItemWire::from_stack(&stack);
    expected.count = 1;
    expected
}

fn accepted_response(
    output: &RuntimeTickOutput,
    session_id: u64,
    request_id: u128,
) -> GameplayResponse {
    let response = response_for(&output.presentation_events, session_id, request_id)
        .unwrap_or_else(|| panic!("missing GameplayResponse for request {request_id}"));
    assert!(matches!(response.outcome, GameplayOutcome::Accepted { .. }));
    response.clone()
}

fn rejected_response(
    output: &RuntimeTickOutput,
    session_id: u64,
    request_id: u128,
    reason: RejectReason,
) {
    let response = response_for(&output.presentation_events, session_id, request_id)
        .unwrap_or_else(|| panic!("missing rejection for request {request_id}"));
    assert_eq!(
        response.outcome,
        GameplayOutcome::Rejected { reason },
        "unexpected response for request {request_id}"
    );
}

fn owner_session_update(
    output: &RuntimeTickOutput,
    session_id: u64,
) -> icraft::network::protocol::SessionGameplayWire {
    output
        .presentation_events
        .iter()
        .find_map(|event| match event {
            RuntimePresentationEvent::PlayerSessionUpdate {
                target,
                player_id,
                state,
                ..
            } if *target == session_id && *player_id == session_id => Some(*state),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing owner PlayerSessionUpdate for {session_id}"))
}

#[test]
fn plan24_plan22_gameplay_vectors_match_all_runtime_topologies() {
    for (label, topology, transport) in [
        (
            "vector_singleplayer",
            AuthorityTopology::Singleplayer,
            TransportMode::Disabled,
        ),
        (
            "vector_listen",
            AuthorityTopology::ListenServer,
            TransportMode::Listen,
        ),
        (
            "vector_dedicated",
            AuthorityTopology::Dedicated,
            TransportMode::Disabled,
        ),
    ] {
        let mut harness = TopologyHarness::new(label, topology, transport);
        prepare_topology_fixture(&mut harness);
        assert_eq!(harness.runtime.authority.topology, topology);

        // Fishing uses the offhand rod, so the combat sword remains selected.
        // The fixed-tick path owns hook creation and reel cleanup; a replay is
        // served by the ACK cache and cannot allocate a second hook.
        let (cast_request, cast_output) = harness.request(GameplayOperation::Fishing {
            action: 0,
            hand: 1,
            look_milli: [0, 0, 1_000],
        });
        let cast_response =
            accepted_response(&cast_output, harness.session_id, cast_request.request_id);
        let cast_state = owner_session_update(&cast_output, harness.session_id);
        assert!(cast_state.fishing_hook.is_some());
        let duplicate_cast = harness.replay(cast_request.clone());
        assert_eq!(
            response_for(
                &duplicate_cast.presentation_events,
                harness.session_id,
                cast_request.request_id
            ),
            Some(&cast_response)
        );
        let _ = harness.runtime.tick_with_output().unwrap();
        let (reel_request, reel_output) = harness.request(GameplayOperation::Fishing {
            action: 1,
            hand: 1,
            look_milli: [0, 0, 1_000],
        });
        accepted_response(&reel_output, harness.session_id, reel_request.request_id);
        assert!(harness.session_state().fishing_hook.is_none());

        let (furnace_request, furnace_output) =
            harness.request(GameplayOperation::FurnaceTakeOutput {
                x: 8,
                y: 80,
                z: 9,
                count: 1,
            });
        accepted_response(
            &furnace_output,
            harness.session_id,
            furnace_request.request_id,
        );
        assert!(furnace_output
            .presentation_events
            .iter()
            .any(|event| matches!(event, RuntimePresentationEvent::BlockEntityDelta { .. })));

        let plank = harness.source(1, 1);
        let mut craft_sources = [None; 9];
        craft_sources[0] = Some(plank);
        craft_sources[2] = Some(plank);
        let (craft_request, craft_output) = harness.request(GameplayOperation::Craft {
            grid: 2,
            sources: craft_sources,
            station: None,
        });
        accepted_response(&craft_output, harness.session_id, craft_request.request_id);
        let craft_projection = owner_session_update(&craft_output, harness.session_id);
        assert_eq!(
            craft_projection.hotbar[1].unwrap().item.item,
            icraft::inventory::Item::Stick.to_u32()
        );
        assert_eq!(craft_projection.hotbar[1].unwrap().item.count, 4);
        assert!(
            harness
                .session_state()
                .count_item(icraft::inventory::Item::Stick.to_u32())
                >= 4
        );

        let (enchant_request, enchant_output) = harness.request(GameplayOperation::Enchant {
            x: 8,
            y: 80,
            z: 11,
            source: harness.source(3, 1),
            option: 2,
        });
        accepted_response(
            &enchant_output,
            harness.session_id,
            enchant_request.request_id,
        );
        let enchant_projection = owner_session_update(&enchant_output, harness.session_id);
        assert!(enchant_projection.hotbar[3]
            .unwrap()
            .item
            .enchantments
            .iter()
            .any(|value| *value != 0));

        let (anvil_request, anvil_output) = harness.request(GameplayOperation::Anvil {
            x: 8,
            y: 80,
            z: 12,
            left: harness.source(3, 1),
            right: None,
            rename: "Plan24 Pick".into(),
        });
        accepted_response(&anvil_output, harness.session_id, anvil_request.request_id);
        let anvil_projection = owner_session_update(&anvil_output, harness.session_id);
        let custom_name = anvil_projection.hotbar[3].unwrap().item.custom_name;
        let name_end = custom_name
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(custom_name.len());
        assert_eq!(&custom_name[..name_end], b"Plan24 Pick");

        let brew_ingredient = harness.source(5, 1);
        let brew_bottle = harness.source(2, 1);
        let (brew_start, brew_output) = harness.request(GameplayOperation::Brew {
            action: 0,
            x: 8,
            y: 80,
            z: 10,
            ingredient: Some(brew_ingredient),
            bottles: [Some(brew_bottle), None, None],
        });
        accepted_response(&brew_output, harness.session_id, brew_start.request_id);
        let inventory_before_brew = harness.session_state().inventory;
        for _ in 0..BREW_TICKS {
            let _ = harness.runtime.tick_with_output().unwrap();
        }
        let ready = harness.session_state();
        assert_eq!(ready.inventory, inventory_before_brew);
        assert_eq!(ready.brew.unwrap().remaining_ticks, 0);
        let (brew_take, brew_take_output) = harness.request(GameplayOperation::Brew {
            action: 2,
            x: 8,
            y: 80,
            z: 10,
            ingredient: None,
            bottles: [None, None, None],
        });
        accepted_response(&brew_take_output, harness.session_id, brew_take.request_id);
        assert!(harness.session_state().brew.is_none());
        assert_eq!(
            harness.session_state().inventory[2]
                .unwrap()
                .item
                .potion
                .unwrap()
                .kind,
            icraft::brewing::PotionKind::Awkward as u8
        );
        let brew_duplicate = harness.replay(brew_take.clone());
        assert_eq!(
            response_for(
                &brew_duplicate.presentation_events,
                harness.session_id,
                brew_take.request_id
            ),
            response_for(
                &brew_take_output.presentation_events,
                harness.session_id,
                brew_take.request_id
            )
        );

        // The world/entity combat path is driven after the long brew tick, so
        // the fixed tick has naturally restored the attack cooldown.
        let target_entity = harness
            .runtime
            .authority
            .world
            .entities
            .spawn(EntityType::Zombie, Vec3::new(8.0, 80.0, 9.0));
        harness
            .runtime
            .authority
            .world
            .entities
            .get_by_id_mut(target_entity)
            .unwrap()
            .health = 1.0;
        let (entity_attack, entity_output) = harness.request(GameplayOperation::Combat {
            target: target_entity,
            action: 0,
        });
        accepted_response(&entity_output, harness.session_id, entity_attack.request_id);
        assert!(harness
            .runtime
            .authority
            .world
            .entities
            .get_by_id(target_entity)
            .is_none());
        let _ = harness.replay(entity_attack);
        for _ in 0..4 {
            let _ = harness.runtime.tick_with_output().unwrap();
        }

        let (player_attack, player_output) = harness.request(GameplayOperation::Combat {
            target: TOPOLOGY_VICTIM_ID,
            action: 0,
        });
        accepted_response(&player_output, harness.session_id, player_attack.request_id);
        let victim_dead = player_output
            .snapshot
            .session_updates
            .iter()
            .find(|update| update.player_id == TOPOLOGY_VICTIM_ID)
            .expect("victim death session update");
        assert!(victim_dead.state.is_dead);
        assert_eq!(victim_dead.state.health_milli, 0);
        assert!(harness
            .runtime
            .authority
            .session(TOPOLOGY_VICTIM_ID)
            .unwrap()
            .gameplay
            .inventory
            .iter()
            .all(Option::is_none));
        let player_duplicate = harness.replay(player_attack.clone());
        assert_eq!(
            response_for(
                &player_duplicate.presentation_events,
                harness.session_id,
                player_attack.request_id
            ),
            response_for(
                &player_output.presentation_events,
                harness.session_id,
                player_attack.request_id
            )
        );
        assert!(
            harness
                .runtime
                .authority
                .session(TOPOLOGY_VICTIM_ID)
                .unwrap()
                .gameplay
                .is_dead
        );

        let stale_request = GameplayRequest {
            request_id: harness.next_request_id,
            client_sequence: harness.next_sequence,
            session_id: harness.session_id,
            dimension: Dimension::Overworld as u8,
            client_revision: 0,
            operation: GameplayOperation::ItemUse {
                item: icraft::inventory::Item::Bread.to_u32(),
                count: 1,
            },
        };
        harness.next_request_id = harness.next_request_id.saturating_add(1);
        harness.next_sequence = harness.next_sequence.saturating_add(1);
        let stale_output = harness.replay(stale_request.clone());
        rejected_response(
            &stale_output,
            harness.session_id,
            stale_request.request_id,
            RejectReason::InvalidRevision,
        );
        let out_of_order = GameplayRequest {
            request_id: harness.next_request_id,
            client_sequence: 1,
            session_id: harness.session_id,
            dimension: Dimension::Overworld as u8,
            client_revision: harness.current_revision(),
            operation: GameplayOperation::ItemUse {
                item: icraft::inventory::Item::Bread.to_u32(),
                count: 1,
            },
        };
        harness.next_request_id = harness.next_request_id.saturating_add(1);
        let out_of_order_output = harness.replay(out_of_order.clone());
        rejected_response(
            &out_of_order_output,
            harness.session_id,
            out_of_order.request_id,
            RejectReason::OutOfOrder,
        );

        harness
            .input
            .try_send(
                icraft::network::server::ServerToHost::ClientRespawnRequest {
                    id: TOPOLOGY_VICTIM_ID,
                },
            )
            .unwrap();
        let respawn_output = harness.runtime.tick_with_output().unwrap();
        let victim_alive = harness
            .runtime
            .authority
            .session(TOPOLOGY_VICTIM_ID)
            .unwrap()
            .gameplay;
        assert!(!victim_alive.is_dead);
        assert_eq!(victim_alive.health_milli, victim_alive.max_health_milli);
        assert_eq!(victim_alive.velocity_milli, [0; 3]);
        assert!(respawn_output
            .snapshot
            .session_updates
            .iter()
            .any(|update| update.player_id == TOPOLOGY_VICTIM_ID && !update.state.is_dead));

        assert!(harness
            .runtime
            .set_session_dimension(harness.session_id, Dimension::Nether));
        let dimension_output = harness.runtime.tick_with_output().unwrap();
        assert!(dimension_output.presentation_events.iter().all(|event| {
            !matches!(
                event,
                RuntimePresentationEvent::PlayerSessionUpdate { target, dimension, .. }
                    if *target == harness.session_id && *dimension != Dimension::Nether as u8
            )
        }));

        // A named session must persist its authoritative dimension across a
        // disconnect/reconnect.  Reconnecting through the public runtime seam
        // also proves that stale projections from the previous session are not
        // retained in the new interest set.
        assert!(harness
            .runtime
            .set_session_dimension(TOPOLOGY_VICTIM_ID, Dimension::Nether));
        let _ = harness.runtime.tick_with_output().unwrap();
        harness.runtime.logout_session(TOPOLOGY_VICTIM_ID).unwrap();
        assert!(!harness.runtime.players.contains_key(&TOPOLOGY_VICTIM_ID));
        harness
            .runtime
            .login_session(TOPOLOGY_VICTIM_ID, "victim")
            .unwrap();
        let reconnect_output = harness.runtime.tick_with_output().unwrap();
        assert_eq!(
            harness.runtime.players[&TOPOLOGY_VICTIM_ID].dimension,
            Dimension::Nether
        );
        assert!(reconnect_output.presentation_events.iter().all(|event| {
            !matches!(
                event,
                RuntimePresentationEvent::PlayerSessionUpdate { target, player_id, .. }
                    if *target == harness.session_id && *player_id == TOPOLOGY_VICTIM_ID
            )
        }));
        harness.shutdown();
    }
}

#[test]
fn plan28_dispenser_item_projection_matches_all_runtime_topologies() {
    let mut baseline: Option<(u64, ItemWire)> = None;
    for (label, topology, transport) in [
        (
            "dispenser_singleplayer",
            AuthorityTopology::Singleplayer,
            TransportMode::Disabled,
        ),
        (
            "dispenser_listen",
            AuthorityTopology::ListenServer,
            TransportMode::Listen,
        ),
        (
            "dispenser_dedicated",
            AuthorityTopology::Dedicated,
            TransportMode::Disabled,
        ),
    ] {
        let mut harness = TopologyHarness::new(label, topology, transport);
        let expected = prepare_dispenser_fixture(&mut harness);
        let mut projection = None;
        for _ in 0..8 {
            let output = harness.runtime.tick_with_output().unwrap();
            projection = output.presentation_events.into_iter().find_map(|event| {
                let state = match event {
                    RuntimePresentationEvent::EntitySpawn { target, state, .. }
                    | RuntimePresentationEvent::EntityState { target, state, .. }
                        if target == harness.session_id
                            && state.entity_type == EntityType::DroppedItem.to_wire() =>
                    {
                        state
                    }
                    _ => return None,
                };
                state.item.map(|item| (state.entity_id, item))
            });
            if projection.is_some() {
                break;
            }
        }
        let projection = projection
            .unwrap_or_else(|| panic!("{topology:?} did not project dispenser output in {label}"));
        assert_eq!(projection.1, expected);
        if let Some(previous) = baseline {
            assert_eq!(projection, previous, "topology projection diverged");
        } else {
            baseline = Some(projection);
        }
        let source_count = match harness.runtime.authority.world.get_block_entity(8, 80, 8) {
            Some(BlockEntity::Dispenser(dispenser)) => {
                dispenser.slots[0].map_or(0, |stack| stack.count)
            }
            _ => 0,
        };
        assert_eq!(
            source_count, 1,
            "powered edge must consume exactly one item"
        );
        harness.shutdown();
    }
}

#[test]
fn disabled_singleplayer_drains_local_request_through_fixed_tick_fifo() {
    let properties = properties("singleplayer");
    let world_dir = properties.world_dir.clone();
    let local_id = u64::MAX - 1;
    let (mut runtime, input) = ServerRuntime::new_embedded(
        properties,
        EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(local_id, "local")),
    )
    .unwrap();

    assert_eq!(runtime.authority.topology, AuthorityTopology::Singleplayer);
    assert_eq!(runtime.transport_mode(), TransportMode::Disabled);
    let revision = runtime
        .authority
        .revision_for_dimension(Dimension::Overworld);
    let before = runtime.authority.world.get_block(8, 80, 8);
    input
        .submit_request(local_id, leftover_block_use(local_id, revision, 41))
        .unwrap();

    // Publication is queued: authority state cannot change before the fixed
    // tick consumes the same bounded FIFO used by listen transport events.
    assert_eq!(runtime.authority.world.get_block(8, 80, 8), before);
    let output = runtime.tick_with_output().unwrap();
    let response = response_for(&output.presentation_events, local_id, 41).unwrap();
    assert!(matches!(
        response.outcome,
        GameplayOutcome::Rejected {
            reason: RejectReason::Unsupported
        }
    ));
    assert!(!output
        .snapshot
        .mutations
        .iter()
        .any(|mutation| mutation.position == (8, 80, 8)));
    assert_eq!(runtime.authority.world.get_block(8, 80, 8), before);
    assert_eq!(runtime.metrics().queue_depth, 0);
    assert_eq!(runtime.metrics().queue_full, 0);
    assert_eq!(runtime.metrics().outbound_packets, 0);

    runtime.shutdown().unwrap();
    drop(runtime);
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn listen_runtime_routes_local_response_to_tick_output() {
    let properties = properties("listen");
    let world_dir = properties.world_dir.clone();
    let local_id = u64::MAX - 2;
    let (mut runtime, input) = ServerRuntime::new_embedded(
        properties,
        EmbeddedRuntimeOptions::listen(LocalSessionProfile::new(local_id, "host")),
    )
    .unwrap();

    assert_eq!(runtime.authority.topology, AuthorityTopology::ListenServer);
    assert_eq!(runtime.transport_mode(), TransportMode::Listen);
    let revision = runtime
        .authority
        .revision_for_dimension(Dimension::Overworld);
    let before = runtime.authority.world.get_block(8, 80, 8);
    input
        .submit_request(local_id, leftover_block_use(local_id, revision, 42))
        .unwrap();
    let output = runtime.tick_with_output().unwrap();
    assert!(response_for(&output.presentation_events, local_id, 42).is_some_and(|response| {
        matches!(
            response.outcome,
            GameplayOutcome::Rejected {
                reason: RejectReason::Unsupported
            }
        )
    }));
    assert!(!output
        .snapshot
        .mutations
        .iter()
        .any(|mutation| mutation.position == (8, 80, 8)));
    assert_eq!(runtime.authority.world.get_block(8, 80, 8), before);

    runtime.shutdown().unwrap();
    drop(runtime);
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn legacy_constructor_remains_dedicated_listen_runtime() {
    let properties = properties("dedicated");
    let world_dir = properties.world_dir.clone();
    let mut runtime = ServerRuntime::new(properties).unwrap();
    assert_eq!(runtime.authority.topology, AuthorityTopology::Dedicated);
    assert_eq!(runtime.transport_mode(), TransportMode::Listen);

    runtime.tick().unwrap();
    let output = runtime.tick_with_output().unwrap();
    assert_eq!(runtime.metrics().ticks, 2);
    assert_eq!(output.snapshot.tick, 2);
    assert!(output.presentation_events.is_empty());

    runtime.shutdown().unwrap();
    drop(runtime);
    let _ = fs::remove_dir_all(world_dir);
}

#[test]
fn world_player_storage_loads_and_rewrites_legacy_player_dat() {
    let properties = properties("world_player_restart");
    let world_dir = properties.world_dir.clone();
    let manager = SaveManager::new(&world_dir);
    let mut level = LevelData::default();
    level.seed = 0x51A9;
    let player = PlayerData::from_state(
        Vec3::new(13.0, 72.0, -9.0),
        Vec3::ZERO,
        0.25,
        -0.5,
        &PlayerState::new(),
        GameMode::Creative,
        &Inventory::new(),
        Default::default(),
    );
    manager.save_player_and_level(&level, &player).unwrap();
    manager.save_current_dimension(Dimension::Nether).unwrap();

    let local_id = u64::MAX - 3;
    let (mut runtime, _input) = ServerRuntime::new_embedded(
        properties.clone(),
        EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(local_id, "legacy")),
    )
    .unwrap();
    let session = &runtime.players[&local_id];
    assert_eq!(session.storage, LocalSessionStorage::WorldPlayer);
    assert_eq!(session.dimension, Dimension::Nether);
    assert_eq!(session.data.position, [13.0, 72.0, -9.0]);
    assert_eq!(
        runtime.authority.session(local_id).unwrap().game_mode,
        GameMode::Creative
    );

    assert!(runtime.set_session_dimension(local_id, Dimension::End));
    runtime
        .authority
        .session_mut(local_id)
        .unwrap()
        .gameplay
        .health_milli = 4_321;
    runtime.shutdown().unwrap();
    drop(runtime);

    let restart_id = u64::MAX - 4;
    let (mut restored, _input) = ServerRuntime::new_embedded(
        properties,
        EmbeddedRuntimeOptions::singleplayer(LocalSessionProfile::new(restart_id, "legacy")),
    )
    .unwrap();
    assert_eq!(restored.players[&restart_id].dimension, Dimension::End);
    assert_eq!(
        restored
            .authority
            .session(restart_id)
            .unwrap()
            .gameplay
            .health_milli,
        4_321
    );
    assert!(world_dir.join("player.dat").is_file());
    assert!(!world_dir.join("players").join("legacy.dat").exists());

    restored.shutdown().unwrap();
    drop(restored);
    let _ = fs::remove_dir_all(world_dir);
}
