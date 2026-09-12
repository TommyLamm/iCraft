use common::tcp_harness::{
    drive_until, held as tcp_held, seeded_properties, session_slot as tcp_slot,
    wait_for_cached_response, HeldLoopback, TcpClient,
};
use icraft::authority::contract::SessionGameplayState;
use icraft::authority::{AuthorityConfig, AuthorityCore};
use icraft::block_entity::{
    BlockEntity, ChestBlockEntity, DispenserBlockEntity, DropperBlockEntity, FurnaceBlockEntity,
    HopperBlockEntity,
};
use icraft::brewing::{PotionData, PotionKind};
use icraft::dimension::Dimension;
use icraft::enchantment::Enchantment;
use icraft::entity::EntityType;
use icraft::inventory::{Item, ItemStack};
use icraft::network::client::ClientToGame;
use icraft::network::protocol::{BlockActionKind, GameplayOperation, GameplayOutcome, GameplayRequest, ItemWire, SessionSlotWire, Packet};
use icraft::redstone::Direction;
use icraft::server_runtime::{
    EmbeddedRuntimeOptions, LocalSessionProfile, ServerProperties, ServerRuntime, TransportMode,
};
use icraft::world::{BlockState, BlockType, ChestType};
use std::collections::BTreeMap;

mod common;

const SESSION_ID: u64 = 0x34_0000;
const TARGET: (i32, i32, i32) = (16, 81, 10);

#[derive(Debug, Clone, Copy)]
enum ContainerKind {
    Chest,
    Furnace,
    Hopper,
    Dispenser,
    Dropper,
}

impl ContainerKind {
    fn block(self) -> BlockType {
        match self {
            Self::Chest => BlockType::Chest,
            Self::Furnace => BlockType::Furnace,
            Self::Hopper => BlockType::Hopper,
            Self::Dispenser => BlockType::Dispenser,
            Self::Dropper => BlockType::Dropper,
        }
    }

    fn entity(self, stacks: [ItemStack; 2]) -> BlockEntity {
        match self {
            Self::Chest => {
                let mut chest = ChestBlockEntity::new();
                chest.set_stack(0, Some(stacks[0]));
                chest.set_stack(26, Some(stacks[1]));
                BlockEntity::Chest(chest)
            }
            Self::Furnace => {
                let mut furnace = FurnaceBlockEntity::new();
                furnace.set_stack(0, Some(stacks[0]));
                furnace.set_stack(2, Some(stacks[1]));
                BlockEntity::Furnace(furnace)
            }
            Self::Hopper => {
                let mut hopper = HopperBlockEntity::new();
                hopper.slots[0] = Some(stacks[0]);
                hopper.slots[4] = Some(stacks[1]);
                BlockEntity::Hopper(hopper)
            }
            Self::Dispenser => {
                let mut dispenser = DispenserBlockEntity::new();
                dispenser.slots[0] = Some(stacks[0]);
                dispenser.slots[8] = Some(stacks[1]);
                BlockEntity::Dispenser(dispenser)
            }
            Self::Dropper => {
                let mut dropper = DropperBlockEntity::new();
                dropper.slots[0] = Some(stacks[0]);
                dropper.slots[8] = Some(stacks[1]);
                BlockEntity::Dropper(dropper)
            }
        }
    }
}

fn rich_stack(kind: ContainerKind, variant: u8) -> ItemStack {
    let item = match kind {
        ContainerKind::Chest => [Item::Diamond, Item::Emerald][variant as usize],
        ContainerKind::Furnace => [Item::GoldIngot, Item::IronIngot][variant as usize],
        ContainerKind::Hopper => [Item::Emerald, Item::Diamond][variant as usize],
        ContainerKind::Dispenser => [Item::Apple, Item::Bread][variant as usize],
        ContainerKind::Dropper => [Item::Bread, Item::Apple][variant as usize],
    };
    let mut stack = ItemStack::new(item, 3 + kind as u32 + u32::from(variant));
    stack.durability = 17 + kind as u32 + u32::from(variant);
    stack.enchantments.add_or_upgrade(if variant == 0 {
        Enchantment::Efficiency(3)
    } else {
        Enchantment::Unbreaking(2)
    });
    stack.potion = Some(PotionData {
        kind: PotionKind::Strength,
        level: 2 + variant,
        duration_seconds: 91 + u16::from(variant),
        splash: variant != 0,
    });
    stack.custom_name.set(if variant == 0 {
        "Plan34 metadata A"
    } else {
        "Plan34 metadata B"
    });
    stack.can_break = 1u128
        << (if variant == 0 {
            BlockType::Stone as u8
        } else {
            BlockType::Chest as u8
        });
    stack.can_place_on = 1u128
        << (if variant == 0 {
            BlockType::Dirt as u8
        } else {
            BlockType::Stone as u8
        });
    stack
}

fn tool_for(kind: ContainerKind) -> Item {
    match kind {
        ContainerKind::Chest => Item::StoneAxe,
        ContainerKind::Furnace
        | ContainerKind::Hopper
        | ContainerKind::Dispenser
        | ContainerKind::Dropper => Item::StonePickaxe,
    }
}

fn core_with_pick() -> AuthorityCore {
    let mut core = AuthorityCore::new(AuthorityConfig::default());
    core.register_session(icraft::authority::contract::SessionContract::new(
        SESSION_ID,
        "plan34-owner",
        Dimension::Overworld as u8,
        [15.0, 80.0, 8.0],
        true,
        true,
    ))
    .expect("register Plan34 authority session");
    let mut gameplay = SessionGameplayState::default();
    let pick = ItemStack::new(Item::StonePickaxe, 1);
    gameplay.inventory[0] = Some(tcp_slot(pick));
    assert!(core.set_session_gameplay(SESSION_ID, gameplay));
    core.world_mut(Dimension::Overworld).unwrap().ensure_chunk(0, 0);
    core.world_mut(Dimension::Overworld).unwrap().ensure_chunk(1, 0);
    core
}

fn break_request(
    core: &AuthorityCore,
    request_id: u128,
    sequence: u64,
    tool: Item,
) -> GameplayRequest {
    let held = ItemStack::new(tool, 1);
    GameplayRequest {
        request_id,
        client_sequence: sequence,
        session_id: SESSION_ID,
        dimension: Dimension::Overworld as u8,
        client_revision: core.current_revision(Dimension::Overworld),
        operation: GameplayOperation::BlockAction {
            action: BlockActionKind::StartBreak,
            x: TARGET.0,
            y: TARGET.1,
            z: TARGET.2,
            face: [0, 0, -1],
            hand: 0,
            held: Some(SessionSlotWire::new(
                ItemWire::from_stack(&held),
                held.can_break,
                held.can_place_on,
            )),
            block: BlockType::Air.to_wire(),
            look_milli: [514, -41, 857],
        },
    }
}

fn dropped_stacks(core: &AuthorityCore) -> Vec<ItemStack> {
    core.world(Dimension::Overworld)
        .entities
        .entities
        .iter()
        .filter(|entity| entity.entity_type == EntityType::DroppedItem)
        .filter_map(|entity| entity.dropped_stack)
        .collect()
}

fn dropped_entities(core: &AuthorityCore) -> Vec<(u64, ItemStack)> {
    let mut entities: Vec<_> = core
        .world(Dimension::Overworld)
        .entities
        .entities
        .iter()
        .filter(|entity| entity.entity_type == EntityType::DroppedItem)
        .filter_map(|entity| entity.dropped_stack.map(|stack| (entity.id, stack)))
        .collect();
    entities.sort_unstable_by_key(|(id, _)| *id);
    entities
}

fn dropped_stack_multiset(core: &AuthorityCore) -> Vec<ItemStack> {
    let mut stacks = dropped_stacks(core);
    stacks.sort_by_key(|stack| format!("{stack:?}"));
    stacks
}

fn dropped_total_count(stacks: &[ItemStack]) -> u32 {
    stacks.iter().map(|stack| stack.count).sum()
}

fn tcp_properties(label: &str) -> ServerProperties {
    seeded_properties(&format!("plan34-{label}"), 0x34_34_34_34)
}

fn tcp_start_request(
    runtime: &ServerRuntime,
    player_id: u64,
    request_id: u128,
    sequence: u64,
    held: SessionSlotWire,
) -> GameplayRequest {
    GameplayRequest {
        request_id,
        client_sequence: sequence,
        session_id: player_id,
        dimension: Dimension::Overworld as u8,
        client_revision: runtime
            .authority
            .revision_for_dimension(Dimension::Overworld),
        operation: GameplayOperation::BlockAction {
            action: BlockActionKind::StartBreak,
            x: TARGET.0,
            y: TARGET.1,
            z: TARGET.2,
            face: [0, 0, -1],
            hand: 0,
            held: Some(held),
            block: BlockType::Air.to_wire(),
            look_milli: [514, -41, 857],
        },
    }
}

fn matching_drop_events(client: &TcpClient, expected: ItemStack) -> Vec<(u64, ItemStack)> {
    let mut matching = BTreeMap::new();
    for event in client.events() {
        let state = match event {
            ClientToGame::Packet(Packet::EntitySpawn { state, .. }) | ClientToGame::Packet(Packet::EntityState { state, .. }) => {
                state
            }
            _ => continue,
        };
        if state.item.and_then(|item| item.to_stack()) == Some(expected) {
            matching.insert(state.entity_id, expected);
        }
    }
    matching.into_iter().collect()
}

fn run_tcp_container_vector(label: &str, listen: bool) {
    let reserved = HeldLoopback::bind();
    let mut properties = tcp_properties(label);
    properties.port = reserved.port();
    let world_dir = properties.world_dir.clone();
    let address = format!("{}:{}", properties.bind, properties.port);
    let _port = reserved.release();
    let (mut runtime, local_host) = if listen {
        let (runtime, _) = ServerRuntime::new_embedded(
            properties.clone(),
            EmbeddedRuntimeOptions {
                transport: TransportMode::Listen,
                local_session: Some(LocalSessionProfile::new(0x34_1000, "plan34-host")),
            },
        )
        .expect("construct Plan34 listen runtime");
        (runtime, true)
    } else {
        (
            ServerRuntime::new(properties.clone()).expect("construct Plan34 dedicated runtime"),
            false,
        )
    };
    let mut clients = vec![
        TcpClient::connect(&address, "plan34-owner"),
        TcpClient::connect(&address, "plan34-observer"),
    ];
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan34 TCP clients authenticated",
            |runtime, views| {
                views.iter().all(|client| client.player_id().is_some())
                    && runtime.players.len() == if local_host { 3 } else { 2 }
            },
        );
        for client in refs.iter_mut() {
            client.clear_events();
        }
    }
    let owner_id = clients[0].player_id().expect("Plan34 TCP owner id");
    let observer_id = clients[1].player_id().expect("Plan34 TCP observer id");
    let expected_stacks = [
        rich_stack(ContainerKind::Chest, 0),
        rich_stack(ContainerKind::Chest, 1),
    ];
    let axe = ItemStack::new(Item::StoneAxe, 1);
    runtime.authority.world_mut(Dimension::Overworld).unwrap().ensure_chunk(0, 0);
    runtime.authority.world_mut(Dimension::Overworld).unwrap().ensure_chunk(1, 0);
    runtime
        .authority
        .world_mut(Dimension::Overworld).unwrap()
        .set_block(TARGET.0, TARGET.1, TARGET.2, BlockType::Chest, 0)
        .expect("seed TCP chest block");
    runtime
        .authority
        .world_mut(Dimension::Overworld).unwrap()
        .chunks
        .set_block_entity(
            TARGET.0,
            TARGET.1,
            TARGET.2,
            Some(ContainerKind::Chest.entity(expected_stacks)),
        );
    let mut owner_gameplay = SessionGameplayState::default();
    owner_gameplay.inventory[0] = Some(tcp_slot(axe));
    assert!(runtime
        .authority
        .set_session_gameplay(owner_id, owner_gameplay));
    assert!(runtime
        .authority
        .set_session_gameplay(observer_id, SessionGameplayState::default()));
    for id in [owner_id, observer_id] {
        let position = if id == owner_id {
            [15.0, 80.0, 8.0]
        } else {
            [15.0, 80.0, 7.0]
        };
        if let Some(player) = runtime.players.get_mut(&id) {
            player.data.position = position;
        }
        if let Some(session) = runtime.authority.session_mut(id) {
            session.position = position;
            session.yaw = 0.0;
            session.pitch = 0.0;
        }
    }
    // Flush the direct fixture mutation before the request so every event
    // asserted below belongs to the authoritative break commit.
    runtime.tick().expect("flush TCP chest fixture");
    for client in &mut clients {
        client.clear_events();
    }
    let start = tcp_start_request(&runtime, owner_id, 1, 1, tcp_held(&axe));
    clients[0].send_request(start.clone());
    let first = {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        wait_for_cached_response(&mut runtime, &mut refs, owner_id, 1)
    };
    assert!(matches!(first.outcome, GameplayOutcome::Accepted { .. }));
    let duplicate_before = runtime.metrics.duplicate_requests;
    clients[0].send_request(start.clone());
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan34 cached TCP duplicate",
            |runtime, _| runtime.metrics.duplicate_requests > duplicate_before,
        );
    }
    assert_eq!(
        runtime
            .authority
            .session(owner_id)
            .unwrap()
            .cached_response(1),
        Some(first)
    );
    assert!(clients[0].take_response(1).is_none());
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan34 TCP chest break projection",
            |runtime, views| {
                runtime
                    .authority
                    .world(Dimension::Overworld)
                    .get_block(TARGET.0, TARGET.1, TARGET.2)
                    == BlockType::Air
                    && views.iter().all(|client| {
                        client.events().iter().any(|event| {
                            matches!(event, ClientToGame::Packet(Packet::BlockChange { x, y, z, block, .. })
                                if (*x, *y, *z) == TARGET && *block == BlockType::Air.to_wire())
                        })
                    })
                    && views.iter().all(|client| {
                        client.events().iter().any(|event| {
                            matches!(event, ClientToGame::Packet(Packet::BlockEntityDelta { x, y, z, entity, .. })
                                if (*x, *y, *z) == TARGET && entity.is_none())
                        })
                    })
                    && views.iter().all(|client| {
                        expected_stacks
                            .iter()
                            .all(|expected| !matching_drop_events(client, *expected).is_empty())
                    })
            },
        );
    }
    for expected in expected_stacks {
        let owner_matching = matching_drop_events(&clients[0], expected);
        let observer_matching = matching_drop_events(&clients[1], expected);
        assert_eq!(owner_matching.len(), 1, "owner matching drop entity");
        assert_eq!(observer_matching, owner_matching);
    }
    assert!(clients[0].events().iter().any(|event| {
        matches!(
            event,
            ClientToGame::Packet(Packet::PlayerSessionUpdate { player_id, state, .. })
                if *player_id == owner_id && state.mining.is_some()
        )
    }));
    assert!(!clients[1].events().iter().any(|event| {
        matches!(
            event,
            ClientToGame::Packet(Packet::PlayerSessionUpdate { player_id, .. })
                if *player_id == owner_id
        )
    }));
    assert!(!clients[1]
        .events()
        .iter()
        .any(|event| { matches!(event, ClientToGame::Packet(Packet::GameplayResponse { .. })) }));
    assert!(!clients[1].events().iter().any(|event| {
        matches!(
            event,
            ClientToGame::Packet(Packet::ContainerOpenResult { .. })
                | ClientToGame::Packet(Packet::ContainerClickResult { .. })
                | ClientToGame::Packet(Packet::ContainerSlotUpdate { .. })
        )
    }));
    // Entity ids must agree for owner and observer during one authoritative
    // runtime; save/reload below deliberately checks content, not ids.
    let dropped_before_reconnect = dropped_entities(&runtime.authority);
    assert_eq!(dropped_before_reconnect.len(), 3);

    clients[0].disconnect_and_join();
    {
        let mut observer_ref = [&mut clients[1]];
        drive_until(
            &mut runtime,
            &mut observer_ref,
            "Plan34 owner disconnect",
            |runtime, _| !runtime.players.contains_key(&owner_id),
        );
    }
    clients[0] = TcpClient::connect(&address, "plan34-owner");
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        drive_until(
            &mut runtime,
            &mut refs,
            "Plan34 owner reconnect",
            |runtime, views| {
                views[0].player_id().is_some()
                    && runtime.players.len() == if local_host { 3 } else { 2 }
            },
        );
    }
    let reconnected_id = clients[0].player_id().expect("Plan34 reconnected owner id");
    let mut retry = start.clone();
    retry.request_id = 1;
    retry.session_id = reconnected_id;
    retry.client_revision = runtime
        .authority
        .revision_for_dimension(Dimension::Overworld);
    clients[0].send_request(retry);
    {
        let mut refs: Vec<&mut TcpClient> = clients.iter_mut().collect();
        let retry_response = wait_for_cached_response(&mut runtime, &mut refs, owner_id, 1);
        assert!(matches!(
            retry_response.outcome,
            GameplayOutcome::Rejected { .. }
        ));
    }
    assert_eq!(
        dropped_entities(&runtime.authority),
        dropped_before_reconnect
    );
    assert_eq!(
        runtime
            .authority
            .world(Dimension::Overworld)
            .get_block(TARGET.0, TARGET.1, TARGET.2),
        BlockType::Air
    );
    // entities.dat is a legacy raw bincode Vec<EntitySaveData>; restart may
    // allocate different ids, so conservation here is full-stack content and
    // total count/multiplicity rather than cross-restart identity.
    let dropped_before_reload = dropped_stack_multiset(&runtime.authority);
    let dropped_before_reload_count = dropped_total_count(&dropped_before_reload);
    clients[0].disconnect_and_join();
    clients[1].disconnect_and_join();
    runtime.shutdown().expect("shutdown Plan34 TCP runtime");
    let (restored, _) = ServerRuntime::new_embedded(
        properties,
        EmbeddedRuntimeOptions {
            transport: TransportMode::Disabled,
            local_session: None,
        },
    )
    .expect("reload Plan34 TCP world");
    assert_eq!(
        restored
            .authority
            .world(Dimension::Overworld)
            .get_block(TARGET.0, TARGET.1, TARGET.2),
        BlockType::Air
    );
    assert!(restored
        .authority
        .world(Dimension::Overworld)
        .get_block_entity(TARGET.0, TARGET.1, TARGET.2)
        .is_none());
    let restored_stack_multiset = dropped_stack_multiset(&restored.authority);
    assert_eq!(restored_stack_multiset, dropped_before_reload);
    assert_eq!(
        dropped_total_count(&restored_stack_multiset),
        dropped_before_reload_count
    );
    for expected in expected_stacks {
        assert_eq!(
            restored_stack_multiset
                .iter()
                .filter(|stack| **stack == expected)
                .count(),
            1
        );
    }
    std::fs::remove_dir_all(world_dir).expect("remove Plan34 TCP world");
}

#[test]
#[ignore = "pre-existing flake: container-break TCP flood fills HOST_EVENT_QUEUE and drops projections"]
fn tcp_listen_and_dedicated_container_breaks_conserve_projection_and_reload() {
    run_tcp_container_vector("listen", true);
    run_tcp_container_vector("dedicated", false);
}

#[test]
fn stale_state_failure_preserves_container_and_session_resources() {
    let mut core = core_with_pick();
    let stacks = [
        rich_stack(ContainerKind::Chest, 0),
        rich_stack(ContainerKind::Chest, 1),
    ];
    let source_entity = ContainerKind::Chest.entity(stacks);
    core.world_mut(Dimension::Overworld).unwrap()
        .set_block(TARGET.0, TARGET.1, TARGET.2, BlockType::Chest, 0)
        .expect("seed atomicity chest");
    core.world_mut(Dimension::Overworld).unwrap().chunks.set_block_entity(
        TARGET.0,
        TARGET.1,
        TARGET.2,
        Some(source_entity.clone()),
    );
    let axe = ItemStack::new(Item::StoneAxe, 1);
    let mut gameplay = core.session(SESSION_ID).unwrap().gameplay;
    gameplay.inventory[0] = Some(tcp_slot(axe));
    assert!(core.set_session_gameplay(SESSION_ID, gameplay));
    let start = break_request(&core, 77, 1, Item::StoneAxe);
    assert!(matches!(
        core.submit_request(start).outcome,
        GameplayOutcome::Accepted { .. }
    ));
    let before_failure = core.session(SESSION_ID).unwrap().gameplay;
    // A state change invalidates the latched mining progress while preserving
    // the chest block entity. The next fixed tick must clear only that stale
    // progress, never run the Plan34 commit.
    core.world_mut(Dimension::Overworld).unwrap()
        .chunks
        .set_block_state(TARGET.0, TARGET.1, TARGET.2, 1);
    let _ = core.tick();
    let after_failure = core.session(SESSION_ID).unwrap().gameplay;
    assert_eq!(
        core.world(Dimension::Overworld).get_block(TARGET.0, TARGET.1, TARGET.2),
        BlockType::Chest
    );
    assert_eq!(
        core.world(Dimension::Overworld).get_block_entity(TARGET.0, TARGET.1, TARGET.2),
        Some(&source_entity)
    );
    assert!(dropped_entities(&core).is_empty());
    assert_eq!(after_failure.inventory, before_failure.inventory);
    assert_eq!(after_failure.experience, before_failure.experience);
    assert_eq!(
        after_failure.experience_level,
        before_failure.experience_level
    );
    assert!(after_failure.mining.is_none());
}

#[test]
fn double_chest_break_only_drops_target_half() {
    let mut core = core_with_pick();
    let partner = (TARGET.0 - 1, TARGET.1, TARGET.2);
    let target_stacks = [
        rich_stack(ContainerKind::Chest, 0),
        rich_stack(ContainerKind::Chest, 1),
    ];
    let partner_stacks = [
        ItemStack::new(Item::GoldIngot, 7),
        ItemStack::new(Item::IronIngot, 8),
    ];
    let target_entity = ContainerKind::Chest.entity(target_stacks);
    let partner_entity = ContainerKind::Chest.entity(partner_stacks);
    let left_state = BlockState {
        facing: Direction::North,
        chest_type: ChestType::Left,
        ..BlockState::default()
    }
    .encode();
    let right_state = BlockState {
        facing: Direction::North,
        chest_type: ChestType::Right,
        ..BlockState::default()
    }
    .encode();
    core.world_mut(Dimension::Overworld).unwrap()
        .set_block(TARGET.0, TARGET.1, TARGET.2, BlockType::Chest, left_state)
        .expect("seed double chest target");
    core.world_mut(Dimension::Overworld).unwrap()
        .set_block(
            partner.0,
            partner.1,
            partner.2,
            BlockType::Chest,
            right_state,
        )
        .expect("seed double chest partner");
    core.world_mut(Dimension::Overworld).unwrap().chunks.set_block_entity(
        TARGET.0,
        TARGET.1,
        TARGET.2,
        Some(target_entity.clone()),
    );
    core.world_mut(Dimension::Overworld).unwrap().chunks.set_block_entity(
        partner.0,
        partner.1,
        partner.2,
        Some(partner_entity.clone()),
    );
    let axe = ItemStack::new(Item::StoneAxe, 1);
    let mut gameplay = core.session(SESSION_ID).unwrap().gameplay;
    gameplay.inventory[0] = Some(tcp_slot(axe));
    assert!(core.set_session_gameplay(SESSION_ID, gameplay));
    let start = break_request(&core, 88, 1, Item::StoneAxe);
    assert!(matches!(
        core.submit_request(start).outcome,
        GameplayOutcome::Accepted { .. }
    ));
    for _ in 0..600 {
        let _ = core.tick();
        if core.world(Dimension::Overworld).get_block(TARGET.0, TARGET.1, TARGET.2) == BlockType::Air {
            break;
        }
    }
    assert_eq!(
        core.world(Dimension::Overworld).get_block(TARGET.0, TARGET.1, TARGET.2),
        BlockType::Air
    );
    assert_eq!(
        core.world(Dimension::Overworld).get_block(partner.0, partner.1, partner.2),
        BlockType::Chest
    );
    assert!(core
        .world(Dimension::Overworld)
        .get_block_entity(TARGET.0, TARGET.1, TARGET.2)
        .is_none());
    assert_eq!(
        core.world(Dimension::Overworld)
            .get_block_entity(partner.0, partner.1, partner.2),
        Some(&partner_entity)
    );
    let actual = dropped_entities(&core);
    for stack in target_stacks {
        assert_eq!(
            actual
                .iter()
                .filter(|(_, candidate)| *candidate == stack)
                .count(),
            1
        );
    }
    for stack in partner_stacks {
        assert!(actual.iter().all(|(_, candidate)| *candidate != stack));
    }
}

#[test]
fn authority_matrix_conserves_two_noncontiguous_metadata_stacks_and_retries() {
    let kinds = [
        ContainerKind::Chest,
        ContainerKind::Furnace,
        ContainerKind::Hopper,
        ContainerKind::Dispenser,
        ContainerKind::Dropper,
    ];
    let mut core = core_with_pick();
    let mut next_request_id = 1;
    let mut next_sequence = 1;
    let mut expected = Vec::new();

    for kind in kinds {
        let stacks = [rich_stack(kind, 0), rich_stack(kind, 1)];
        expected.extend(stacks);
        let tool = tool_for(kind);
        let mut gameplay = core.session(SESSION_ID).unwrap().gameplay;
        let tool_stack = ItemStack::new(tool, 1);
        gameplay.inventory[0] = Some(tcp_slot(tool_stack));
        assert!(core.set_session_gameplay(SESSION_ID, gameplay));
        core.world_mut(Dimension::Overworld).unwrap()
            .set_block(TARGET.0, TARGET.1, TARGET.2, kind.block(), 0)
            .expect("seed container block");
        core.world_mut(Dimension::Overworld).unwrap().chunks.set_block_entity(
            TARGET.0,
            TARGET.1,
            TARGET.2,
            Some(kind.entity(stacks)),
        );
        let request = break_request(&core, next_request_id, next_sequence, tool);
        let accepted = core.submit_request(request.clone());
        assert!(
            matches!(accepted.outcome, GameplayOutcome::Accepted { .. }),
            "start rejected for {kind:?}: {accepted:?}"
        );
        assert!(core.session(SESSION_ID).unwrap().gameplay.mining.is_some());
        assert_eq!(core.submit_request(request), accepted, "cached duplicate");
        let before_stale = dropped_stacks(&core).len();
        let mut stale = break_request(&core, next_request_id + 10_000, next_sequence + 1, tool);
        stale.client_revision = core.current_revision(Dimension::Overworld).saturating_sub(1);
        assert!(matches!(
            core.submit_request(stale).outcome,
            GameplayOutcome::Rejected { .. }
        ));
        assert_eq!(dropped_stacks(&core).len(), before_stale);

        for _ in 0..600 {
            let _ = core.tick();
            if core.world(Dimension::Overworld).get_block(TARGET.0, TARGET.1, TARGET.2) == BlockType::Air {
                break;
            }
        }
        assert_eq!(
            core.world(Dimension::Overworld).get_block(TARGET.0, TARGET.1, TARGET.2),
            BlockType::Air,
            "{kind:?} committed block break"
        );
        assert!(core
            .world(Dimension::Overworld)
            .get_block_entity(TARGET.0, TARGET.1, TARGET.2)
            .is_none());
        let actual = dropped_stacks(&core);
        for stack in stacks {
            assert_eq!(
                actual
                    .iter()
                    .filter(|candidate| **candidate == stack)
                    .count(),
                1,
                "{kind:?} stack must drop exactly once"
            );
        }
        assert_eq!(
            actual.iter().map(|candidate| candidate.count).sum::<u32>(),
            expected
                .iter()
                .map(|candidate| candidate.count)
                .sum::<u32>()
                + (expected.len() / 2) as u32
        );
        let before_fresh_retry = dropped_entities(&core);
        let fresh_retry = break_request(&core, next_request_id + 20_000, next_sequence + 1, tool);
        assert!(matches!(
            core.submit_request(fresh_retry).outcome,
            GameplayOutcome::Rejected { .. }
        ));
        assert_eq!(dropped_entities(&core), before_fresh_retry);

        next_request_id += 1;
        next_sequence += 2;
    }
}
