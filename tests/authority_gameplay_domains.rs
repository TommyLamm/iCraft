//! Plan22 headless authority vectors.
//!
//! These tests intentionally drive the transport-independent `AuthorityCore`
//! directly.  They exercise the same authenticated request, fixed-tick and
//! snapshot path used by the dedicated/listen compositions without involving
//! a renderer or a network client.

mod common;

use common::tcp_harness::session_slot;
use glam::Vec3;
use icraft::authority::contract::{AuthorityTopology, SessionContract, SessionGameplayState};
use icraft::authority::fishing::water_probe_position;
use icraft::authority::transactions::BREW_TICKS;
use icraft::authority::{AuthorityConfig, AuthorityCore};
use icraft::block_entity::{BlockEntity, FurnaceBlockEntity};
use icraft::dimension::Dimension;
use icraft::entity::EntityType;
use icraft::inventory::{Item, ItemStack};
use icraft::network::protocol::{
    GameplayOperation, GameplayOutcome, GameplayRequest, GameplayResponse, RejectReason,
    SlotRefWire,
};
use icraft::world::BlockType;

const SESSION_ID: u64 = 7;

fn new_core() -> AuthorityCore {
    let mut core = AuthorityCore::new(AuthorityConfig::default(), AuthorityTopology::Dedicated);
    core.register_session(SessionContract::new(
        SESSION_ID,
        "headless",
        Dimension::Overworld as u8,
        [8.0, 80.0, 8.0],
        true,
        true,
    ))
    .unwrap();
    core
}

fn source(state: &SessionGameplayState, index: u8, count: u16) -> SlotRefWire {
    SlotRefWire {
        index,
        count,
        expected: state.inventory[usize::from(index)]
            .expect("source slot is present")
            .into(),
    }
}

fn request(
    core: &AuthorityCore,
    request_id: u128,
    client_sequence: u64,
    operation: GameplayOperation,
) -> GameplayRequest {
    GameplayRequest {
        request_id,
        client_sequence,
        session_id: SESSION_ID,
        dimension: Dimension::Overworld as u8,
        client_revision: core.revision_for_dimension(Dimension::Overworld),
        operation,
    }
}

fn submit(
    core: &mut AuthorityCore,
    request_id: u128,
    client_sequence: u64,
    operation: GameplayOperation,
) -> GameplayResponse {
    let request = request(core, request_id, client_sequence, operation);
    core.submit_request(request)
}

fn accepted(response: &GameplayResponse) -> u64 {
    match response.outcome {
        GameplayOutcome::Accepted { revision } => revision,
        GameplayOutcome::Rejected { reason } => panic!("request rejected: {reason:?}"),
    }
}

fn rejected(response: &GameplayResponse, reason: RejectReason) {
    assert_eq!(response.outcome, GameplayOutcome::Rejected { reason });
}

fn put_block(core: &mut AuthorityCore, position: [i32; 3], block: BlockType) {
    core.world_mut_active()
        .set_block(position[0], position[1], position[2], block, 0)
        .unwrap();
}

#[test]
fn fishing_fixed_tick_reel_is_atomic_and_idempotent() {
    let mut core = new_core();
    let mut gameplay = core.session(SESSION_ID).unwrap().gameplay;
    let rod = ItemStack::new(Item::FishingRod, 1);
    gameplay.inventory[0] = Some(session_slot(rod));
    gameplay.selected_hotbar_slot = 0;
    assert!(core.set_session_gameplay(SESSION_ID, gameplay));

    // Invalid hand/look requests never create a hook or consume durability;
    // the authenticated envelope still advances the client sequence.
    let invalid = submit(
        &mut core,
        1,
        1,
        GameplayOperation::Fishing {
            action: 0,
            hand: 1,
            look_milli: [0, 0, 1_000],
        },
    );
    rejected(&invalid, RejectReason::InvalidState);
    assert!(core
        .session(SESSION_ID)
        .unwrap()
        .gameplay
        .fishing_hook
        .is_none());

    let cast = submit(
        &mut core,
        2,
        2,
        GameplayOperation::Fishing {
            action: 0,
            hand: 0,
            look_milli: [0, 0, 1_000],
        },
    );
    accepted(&cast);
    let cast_state = core.session(SESSION_ID).unwrap().gameplay;
    let hook_id = cast_state
        .fishing_hook
        .expect("cast creates hook")
        .entity_id;
    // Retransmitting the same request is served from the bounded response
    // cache and cannot allocate another hook.
    let duplicate = submit(
        &mut core,
        2,
        2,
        GameplayOperation::Fishing {
            action: 0,
            hand: 0,
            look_milli: [0, 0, 1_000],
        },
    );
    assert_eq!(duplicate, cast);
    assert_eq!(
        core.session(SESSION_ID)
            .unwrap()
            .gameplay
            .fishing_hook
            .unwrap()
            .entity_id,
        hook_id
    );

    // The world probe is authoritative.  Seed the exact block under the
    // predicted hook position before each fixed tick until it lands and bites.
    let mut nibbled = false;
    for _ in 0..(icraft::fishing::FISHING_INITIAL_WAIT_TICKS + 8) {
        let state = core.session(SESSION_ID).unwrap().gameplay;
        let probe = water_probe_position(&state).expect("active hook has a probe");
        let block = [
            probe[0].div_euclid(1_000),
            probe[1].div_euclid(1_000),
            probe[2].div_euclid(1_000),
        ];
        if core.world().get_block(block[0], block[1], block[2]) != BlockType::Water {
            put_block(&mut core, block, BlockType::Water);
        }
        core.tick();
        let stage = core
            .session(SESSION_ID)
            .unwrap()
            .gameplay
            .fishing_hook
            .map(|hook| hook.stage);
        if stage == Some(icraft::fishing::FishingHookStage::Nibbling.to_wire()) {
            nibbled = true;
            break;
        }
    }
    assert!(nibbled, "fixed ticks reach a deterministic bite");

    let before_reel = core.session(SESSION_ID).unwrap().gameplay;
    let before_rod = before_reel.inventory[0].unwrap().item.durability;
    let before_experience = before_reel.experience;
    let reel = submit(
        &mut core,
        3,
        3,
        GameplayOperation::Fishing {
            action: 1,
            hand: 0,
            look_milli: [0, 0, 1_000],
        },
    );
    accepted(&reel);
    let after_reel = core.session(SESSION_ID).unwrap().gameplay;
    assert!(after_reel.fishing_hook.is_none());
    assert!(after_reel.inventory[0].unwrap().item.durability < before_rod);
    assert!(after_reel.experience > before_experience);
    assert!(core.world_mut_active().entities.get_by_id(hook_id).is_none());

    // Duplicate reel cannot grant a second catch, and stale revisions are
    // rejected before the domain seam is entered.
    let duplicate_reel = submit(
        &mut core,
        3,
        3,
        GameplayOperation::Fishing {
            action: 1,
            hand: 0,
            look_milli: [0, 0, 1_000],
        },
    );
    assert_eq!(duplicate_reel, reel);
    assert_eq!(core.session(SESSION_ID).unwrap().gameplay, after_reel);

    let stale_request = GameplayRequest {
        request_id: 4,
        client_sequence: 4,
        session_id: SESSION_ID,
        dimension: Dimension::Overworld as u8,
        client_revision: 0,
        operation: GameplayOperation::Fishing {
            action: 2,
            hand: 0,
            look_milli: [0, 0, 1_000],
        },
    };
    rejected(
        &core.submit_request(stale_request),
        RejectReason::InvalidRevision,
    );
    assert_eq!(core.session(SESSION_ID).unwrap().gameplay, after_reel);
}

#[test]
fn workstation_transactions_cover_brew_ready_take_and_exact_sources() {
    let mut core = new_core();
    let furnace_position = [8, 80, 9];
    let brew_position = [8, 80, 10];
    let enchanting_position = [8, 80, 11];
    let anvil_position = [8, 80, 12];
    put_block(&mut core, furnace_position, BlockType::Furnace);
    put_block(&mut core, brew_position, BlockType::BrewingStand);
    put_block(&mut core, enchanting_position, BlockType::EnchantingTable);
    put_block(&mut core, anvil_position, BlockType::Anvil);

    let mut furnace = FurnaceBlockEntity::new();
    furnace.slots[2] = Some(ItemStack::new(Item::IronIngot, 2));
    furnace.accumulated_xp = 4.0;
    core.world_mut_active().chunks.set_block_entity(
        furnace_position[0],
        furnace_position[1],
        furnace_position[2],
        Some(BlockEntity::Furnace(furnace)),
    );

    let mut gameplay = core.session(SESSION_ID).unwrap().gameplay;
    gameplay.inventory[0] = Some(session_slot(ItemStack::new(Item::OakPlanks, 2)));
    gameplay.inventory[1] = Some(session_slot(ItemStack::new(Item::NetherWart, 1)));
    gameplay.inventory[2] = Some(session_slot(ItemStack::new(Item::Potion, 1)));
    gameplay.inventory[3] = Some(session_slot(ItemStack::new(Item::IronPickaxe, 1)));
    gameplay.inventory[4] = Some(session_slot(ItemStack::new(Item::LapisLazuli, 3)));
    gameplay.experience_level = 30;
    gameplay.enchant_seed = 42;
    assert!(core.set_session_gameplay(SESSION_ID, gameplay));

    // Furnace output and XP commit together, while a duplicate request is a
    // cache hit rather than a second extraction.
    let furnace_response = submit(
        &mut core,
        10,
        1,
        GameplayOperation::FurnaceTakeOutput {
            x: furnace_position[0],
            y: furnace_position[1],
            z: furnace_position[2],
            count: 1,
        },
    );
    accepted(&furnace_response);
    let furnace_state = core.session(SESSION_ID).unwrap().gameplay;
    assert_eq!(furnace_state.count_item(Item::IronIngot.to_u32()), 1);
    assert_eq!(furnace_state.experience_level, 30);
    assert_eq!(furnace_state.experience, 4);
    assert_eq!(
        core.world()
            .get_block_entity(
                furnace_position[0],
                furnace_position[1],
                furnace_position[2]
            )
            .unwrap()
            .get_stack(2)
            .unwrap()
            .count,
        1
    );
    assert_eq!(
        submit(
            &mut core,
            10,
            1,
            GameplayOperation::FurnaceTakeOutput {
                x: furnace_position[0],
                y: furnace_position[1],
                z: furnace_position[2],
                count: 1,
            },
        ),
        furnace_response
    );

    // A 2x2 recipe consumes the exact rich references and returns four sticks.
    let craft_state = core.session(SESSION_ID).unwrap().gameplay;
    let plank = source(&craft_state, 0, 1);
    let mut sources = [None; 9];
    sources[0] = Some(plank);
    sources[2] = Some(plank);
    let craft = submit(
        &mut core,
        11,
        2,
        GameplayOperation::Craft {
            grid: 2,
            sources,
            station: None,
        },
    );
    accepted(&craft);
    let after_craft = core.session(SESSION_ID).unwrap().gameplay;
    assert_eq!(after_craft.count_item(Item::OakPlanks.to_u32()), 0);
    assert_eq!(after_craft.count_item(Item::Stick.to_u32()), 4);

    // Enchant and anvil use workstation identity plus metadata-preserving
    // exact sources.  Keep the assertions focused on atomic publication.
    let enchant_state = core.session(SESSION_ID).unwrap().gameplay;
    let enchant = submit(
        &mut core,
        12,
        3,
        GameplayOperation::Enchant {
            x: enchanting_position[0],
            y: enchanting_position[1],
            z: enchanting_position[2],
            source: source(&enchant_state, 3, 1),
            option: 2,
        },
    );
    accepted(&enchant);
    let after_enchant = core.session(SESSION_ID).unwrap().gameplay;
    assert_eq!(after_enchant.count_item(Item::LapisLazuli.to_u32()), 0);
    assert!(after_enchant.inventory[3]
        .unwrap()
        .item
        .enchantments
        .iter()
        .any(|value| *value != 0));

    let anvil_state = core.session(SESSION_ID).unwrap().gameplay;
    let anvil = submit(
        &mut core,
        13,
        4,
        GameplayOperation::Anvil {
            x: anvil_position[0],
            y: anvil_position[1],
            z: anvil_position[2],
            left: source(&anvil_state, 3, 1),
            right: None,
            rename: "Plan22 Pick".into(),
        },
    );
    accepted(&anvil);
    let after_anvil = core.session(SESSION_ID).unwrap().gameplay;
    let custom_name = after_anvil.inventory[3].unwrap().item.custom_name;
    let name_end = custom_name
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(custom_name.len());
    assert_eq!(&custom_name[..name_end], b"Plan22 Pick");

    // Start reserves, but does not debit, ingredient/bottle stacks.  Exactly
    // 200 fixed ticks reach Ready; only explicit action=2 publishes output.
    let brew_state = core.session(SESSION_ID).unwrap().gameplay;
    let ingredient = source(&brew_state, 1, 1);
    let bottle = source(&brew_state, 2, 1);
    let start = submit(
        &mut core,
        14,
        5,
        GameplayOperation::Brew {
            action: 0,
            x: brew_position[0],
            y: brew_position[1],
            z: brew_position[2],
            ingredient: Some(ingredient),
            bottles: [Some(bottle), None, None],
        },
    );
    accepted(&start);
    let inventory_before_ticks = core.session(SESSION_ID).unwrap().gameplay.inventory;
    for _ in 0..BREW_TICKS {
        core.tick();
    }
    let ready = core.session(SESSION_ID).unwrap().gameplay;
    assert_eq!(ready.inventory, inventory_before_ticks);
    assert_eq!(ready.brew.unwrap().remaining_ticks, 0);
    let take = submit(
        &mut core,
        15,
        6,
        GameplayOperation::Brew {
            action: 2,
            x: brew_position[0],
            y: brew_position[1],
            z: brew_position[2],
            ingredient: None,
            bottles: [None, None, None],
        },
    );
    accepted(&take);
    let after_take = core.session(SESSION_ID).unwrap().gameplay;
    assert!(after_take.brew.is_none());
    assert_eq!(after_take.count_item(Item::NetherWart.to_u32()), 0);
    assert_eq!(
        after_take.inventory[2].unwrap().item.potion.unwrap().kind,
        icraft::brewing::PotionKind::Awkward as u8
    );
    assert_eq!(
        submit(
            &mut core,
            15,
            6,
            GameplayOperation::Brew {
                action: 2,
                x: brew_position[0],
                y: brew_position[1],
                z: brew_position[2],
                ingredient: None,
                bottles: [None, None, None],
            },
        ),
        take
    );

    // Disconnect cleanup clears only the uncommitted reservation; exact
    // inventory identities survive a reconnect and cannot be duplicated.
    let mut reconnect_state = core.session(SESSION_ID).unwrap().gameplay;
    reconnect_state.inventory[1] = Some(session_slot(ItemStack::new(Item::NetherWart, 1)));
    reconnect_state.inventory[2] = Some(session_slot(ItemStack::new(Item::Potion, 1)));
    assert!(core.set_session_gameplay(SESSION_ID, reconnect_state));
    let restart_sources = core.session(SESSION_ID).unwrap().gameplay;
    let restart = submit(
        &mut core,
        16,
        7,
        GameplayOperation::Brew {
            action: 0,
            x: brew_position[0],
            y: brew_position[1],
            z: brew_position[2],
            ingredient: Some(source(&restart_sources, 1, 1)),
            bottles: [Some(source(&restart_sources, 2, 1)), None, None],
        },
    );
    accepted(&restart);
    let reserved_inventory = core.session(SESSION_ID).unwrap().gameplay.inventory;
    let disconnected = core
        .remove_session(SESSION_ID)
        .expect("session disconnects");
    assert!(disconnected.gameplay.brew.is_none());
    assert_eq!(disconnected.gameplay.inventory, reserved_inventory);
    core.register_session(disconnected).unwrap();
    assert_eq!(
        core.session(SESSION_ID).unwrap().gameplay.inventory,
        reserved_inventory
    );

    // Unknown actions are rejected at the protocol boundary and cannot alter
    // the ready/taken inventory state.
    let unknown = GameplayRequest {
        request_id: 17,
        client_sequence: 8,
        session_id: SESSION_ID,
        dimension: Dimension::Overworld as u8,
        client_revision: core.revision_for_dimension(Dimension::Overworld),
        operation: GameplayOperation::Brew {
            action: 3,
            x: brew_position[0],
            y: brew_position[1],
            z: brew_position[2],
            ingredient: None,
            bottles: [None, None, None],
        },
    };
    rejected(&core.submit_request(unknown), RejectReason::InvalidState);
}

#[test]
fn combat_death_respawn_and_entity_loot_are_authoritative() {
    let mut core = new_core();
    let mut attacker = core.session(SESSION_ID).unwrap().gameplay;
    attacker.attack_cooldown_ticks = 5;
    attacker.inventory[0] = Some(session_slot(ItemStack::new(Item::DiamondSword, 1)));
    assert!(core.set_session_gameplay(SESSION_ID, attacker));

    let target = core
        .world_mut_active()
        .entities
        .spawn(EntityType::Zombie, Vec3::new(8.0, 80.0, 9.0));
    let before = core.world_mut_active().entities.get_by_id(target).unwrap().health;
    let hit = submit(
        &mut core,
        30,
        1,
        GameplayOperation::Combat { target, action: 0 },
    );
    accepted(&hit);
    let entity = core.world_mut_active().entities.get_by_id(target).unwrap();
    assert!(entity.health < before);
    assert!(entity.velocity.length_squared() > 0.0);
    assert_eq!(
        submit(
            &mut core,
            30,
            1,
            GameplayOperation::Combat { target, action: 0 },
        ),
        hit
    );

    // A lethal entity hit removes the target and emits exactly one drop/xp
    // vector.  The cached duplicate cannot emit another pair.
    let lethal_target = core
        .world_mut_active()
        .entities
        .spawn(EntityType::Zombie, Vec3::new(8.0, 80.0, 9.0));
    core.world_mut_active()
        .entities
        .get_by_id_mut(lethal_target)
        .unwrap()
        .health = 1.0;
    core.session_mut(SESSION_ID)
        .unwrap()
        .gameplay
        .attack_cooldown_ticks = 5;
    let lethal = submit(
        &mut core,
        31,
        2,
        GameplayOperation::Combat {
            target: lethal_target,
            action: 0,
        },
    );
    accepted(&lethal);
    assert!(core.world_mut_active().entities.get_by_id(lethal_target).is_none());
    let drops_after_lethal = core
        .world()
        .entities
        .entities
        .iter()
        .filter(|entity| {
            matches!(
                entity.entity_type,
                EntityType::DroppedItem | EntityType::ExperienceOrb
            )
        })
        .count();
    assert!(drops_after_lethal >= 1);
    assert_eq!(
        submit(
            &mut core,
            31,
            2,
            GameplayOperation::Combat {
                target: lethal_target,
                action: 0,
            },
        ),
        lethal
    );
    assert_eq!(
        core.world()
            .entities
            .entities
            .iter()
            .filter(|entity| {
                matches!(
                    entity.entity_type,
                    EntityType::DroppedItem | EntityType::ExperienceOrb
                )
            })
            .count(),
        drops_after_lethal
    );

    // Player death clears inventory and XP atomically, while respawn resets
    // health/hunger/saturation/velocity and publishes a new session revision.
    let target_id = 8;
    core.register_session(SessionContract::new(
        target_id,
        "victim",
        Dimension::Overworld as u8,
        [8.0, 80.0, 9.0],
        false,
        false,
    ))
    .unwrap();
    let mut victim = core.session(target_id).unwrap().gameplay;
    victim.health_milli = 20_000;
    victim.inventory[0] = Some(session_slot(ItemStack::new(Item::Diamond, 2)));
    victim.inventory[40] = Some(session_slot(ItemStack::new(Item::Shield, 1)));
    victim.shield_active = true;
    victim.experience_level = 4;
    victim.velocity_milli = [100, 200, 300];
    assert!(core.set_session_gameplay(target_id, victim));
    core.session_mut(target_id).unwrap().yaw = 180.0;
    core.session_mut(SESSION_ID)
        .unwrap()
        .gameplay
        .attack_cooldown_ticks = 5;
    let before_victim_revision = core.session(target_id).unwrap().last_revision;
    let shield_hit = submit(
        &mut core,
        32,
        3,
        GameplayOperation::Combat {
            target: target_id,
            action: 0,
        },
    );
    accepted(&shield_hit);
    let shielded = core.session(target_id).unwrap().gameplay;
    assert_eq!(shielded.health_milli, 20_000);
    assert!(shielded.inventory[40].unwrap().item.durability < 336);

    core.session_mut(target_id).unwrap().gameplay.shield_active = false;
    core.session_mut(target_id).unwrap().gameplay.health_milli = 1_000;
    core.session_mut(SESSION_ID)
        .unwrap()
        .gameplay
        .attack_cooldown_ticks = 5;
    let player_hit = submit(
        &mut core,
        33,
        4,
        GameplayOperation::Combat {
            target: target_id,
            action: 0,
        },
    );
    accepted(&player_hit);
    let dead = core.session(target_id).unwrap().gameplay;
    assert!(dead.is_dead);
    assert_eq!(dead.health_milli, 0);
    assert!(dead.inventory.iter().all(Option::is_none));
    assert_eq!(dead.experience_level, 0);
    assert!(dead.death_source.is_some());

    let respawn = core.respawn_session(target_id);
    assert!(respawn);
    let alive = core.session(target_id).unwrap();
    assert!(!alive.gameplay.is_dead);
    assert!(alive.gameplay.death_source.is_none());
    assert_eq!(alive.gameplay.health_milli, alive.gameplay.max_health_milli);
    assert_eq!(alive.gameplay.hunger_milli, 20_000);
    assert_eq!(alive.gameplay.saturation_milli, 5_000);
    assert_eq!(alive.gameplay.velocity_milli, [0; 3]);
    assert!(alive.last_revision > before_victim_revision);

    let far_target = core
        .world_mut_active()
        .entities
        .spawn(EntityType::Zombie, Vec3::new(20.0, 80.0, 8.0));
    core.session_mut(SESSION_ID)
        .unwrap()
        .gameplay
        .attack_cooldown_ticks = 5;
    let far_before = core.world_mut_active().entities.get_by_id(far_target).unwrap().health;
    let far_response = submit(
        &mut core,
        35,
        5,
        GameplayOperation::Combat {
            target: far_target,
            action: 0,
        },
    );
    rejected(&far_response, RejectReason::TooFar);
    assert_eq!(
        core.world_mut_active().entities.get_by_id(far_target).unwrap().health,
        far_before
    );

    core.session_mut(target_id).unwrap().game_mode = icraft::inventory::GameMode::Creative;
    core.session_mut(SESSION_ID)
        .unwrap()
        .gameplay
        .attack_cooldown_ticks = 5;
    let creative_response = submit(
        &mut core,
        36,
        6,
        GameplayOperation::Combat {
            target: target_id,
            action: 0,
        },
    );
    rejected(&creative_response, RejectReason::PermissionDenied);
    core.session_mut(target_id).unwrap().game_mode = icraft::inventory::GameMode::Spectator;
    core.session_mut(SESSION_ID)
        .unwrap()
        .gameplay
        .attack_cooldown_ticks = 5;
    let spectator_response = submit(
        &mut core,
        37,
        7,
        GameplayOperation::Combat {
            target: target_id,
            action: 0,
        },
    );
    rejected(&spectator_response, RejectReason::PermissionDenied);
    core.session_mut(target_id).unwrap().game_mode = icraft::inventory::GameMode::Survival;
    assert!(core.set_session_dimension(target_id, Dimension::Nether));
    core.session_mut(SESSION_ID)
        .unwrap()
        .gameplay
        .attack_cooldown_ticks = 5;
    let wrong_dimension = submit(
        &mut core,
        38,
        8,
        GameplayOperation::Combat {
            target: target_id,
            action: 0,
        },
    );
    rejected(&wrong_dimension, RejectReason::PermissionDenied);
    assert!(core.set_session_dimension(target_id, Dimension::Overworld));

    // keepInventory retains exact rich slots and emits no death drops.
    let mut keep_core = new_core();
    let mut keep_rules = keep_core.world_mut_active().rules;
    keep_rules.keep_inventory = true;
    keep_core.set_rules(keep_rules);
    keep_core
        .register_session(SessionContract::new(
            target_id,
            "kept",
            Dimension::Overworld as u8,
            [8.0, 80.0, 9.0],
            false,
            false,
        ))
        .unwrap();
    let mut kept = keep_core.session(target_id).unwrap().gameplay;
    kept.health_milli = 1_000;
    kept.inventory[0] = Some(session_slot(ItemStack::new(Item::Diamond, 1)));
    assert!(keep_core.set_session_gameplay(target_id, kept));
    keep_core
        .session_mut(SESSION_ID)
        .unwrap()
        .gameplay
        .attack_cooldown_ticks = 5;
    let kept_hit = submit(
        &mut keep_core,
        1,
        1,
        GameplayOperation::Combat {
            target: target_id,
            action: 0,
        },
    );
    accepted(&kept_hit);
    let kept_dead = keep_core.session(target_id).unwrap().gameplay;
    assert!(kept_dead.is_dead);
    assert_eq!(kept_dead.count_item(Item::Diamond.to_u32()), 1);
    assert!(keep_core.world_mut_active().entities.entities.iter().all(|entity| {
        !matches!(
            entity.entity_type,
            EntityType::DroppedItem | EntityType::ExperienceOrb
        )
    }));

    // A stale revision cannot replay combat after the authoritative death and
    // respawn transition.
    let stale = GameplayRequest {
        request_id: 39,
        client_sequence: 9,
        session_id: SESSION_ID,
        dimension: Dimension::Overworld as u8,
        client_revision: 0,
        operation: GameplayOperation::Combat {
            target: lethal_target,
            action: 0,
        },
    };
    rejected(&core.submit_request(stale), RejectReason::InvalidRevision);
}
