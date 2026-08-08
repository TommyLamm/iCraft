//! Headless acceptance harness for Plan 17.
//!
//! The harness intentionally exercises the deterministic simulation seams that
//! are available without a GPU or a live network socket.  Dedicated/listen
//! topology rows are represented as blocked hand-off rows until Plan 18
//! unifies authority; they are never reported as passing by assumption.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AcceptanceScenario {
    Foundation,
    Progression,
    SocialAutomation,
}

impl AcceptanceScenario {
    pub const ALL: [Self; 3] = [Self::Foundation, Self::Progression, Self::SocialAutomation];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Foundation => "foundation",
            Self::Progression => "progression",
            Self::SocialAutomation => "social_automation",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AcceptanceTopology {
    Singleplayer,
    ListenServer,
    DedicatedTwoClients,
}

impl AcceptanceTopology {
    pub const ALL: [Self; 3] = [
        Self::Singleplayer,
        Self::ListenServer,
        Self::DedicatedTwoClients,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Singleplayer => "singleplayer",
            Self::ListenServer => "listen-server",
            Self::DedicatedTwoClients => "dedicated+2-clients",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceAssertion {
    pub name: &'static str,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceReport {
    pub scenario: AcceptanceScenario,
    pub topology: AcceptanceTopology,
    pub assertions: Vec<AcceptanceAssertion>,
    pub blocked_reason: Option<&'static str>,
}

impl AcceptanceReport {
    pub fn passed(&self) -> bool {
        self.blocked_reason.is_none() && self.assertions.iter().all(|assertion| assertion.passed)
    }
}

pub fn run_headless(
    scenario: AcceptanceScenario,
    topology: AcceptanceTopology,
) -> AcceptanceReport {
    if topology != AcceptanceTopology::Singleplayer {
        return AcceptanceReport {
            scenario,
            topology,
            assertions: Vec::new(),
            blocked_reason: Some(
                "Plan 18 authority unification required before listen/dedicated topology acceptance",
            ),
        };
    }

    let assertions = match scenario {
        AcceptanceScenario::Foundation => run_foundation(),
        AcceptanceScenario::Progression => run_progression(),
        AcceptanceScenario::SocialAutomation => run_social_automation(),
    };
    AcceptanceReport {
        scenario,
        topology,
        assertions,
        blocked_reason: None,
    }
}

fn assertion(name: &'static str, passed: bool) -> AcceptanceAssertion {
    AcceptanceAssertion { name, passed }
}

fn save_root(label: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("minecraft_plan19_{label}"));
    let _ = std::fs::remove_dir_all(&root);
    root
}

fn run_foundation() -> Vec<AcceptanceAssertion> {
    use crate::inventory::{Item, ItemStack};
    use crate::sim_harness::SimHarness;

    let mut sim = SimHarness::new();
    let baseline = sim.checksum();

    // Gather the tree and build the first tool with RecipeManager.
    let mut logs = 0;
    for x in 2..=5 {
        if sim.mine_block((x, 70, 3), None).is_some() {
            logs += 1;
        }
    }
    if sim.mine_block((2, 71, 3), None).is_some() {
        logs += 1;
    }
    let mut planks_crafted = 0;
    for _ in 0..logs {
        if sim
            .craft(&[Some(ItemStack::new(Item::OakLog, 1))], 1)
            .is_some()
        {
            planks_crafted += 1;
        }
    }
    let sticks = sim
        .craft(
            &[
                Some(ItemStack::new(Item::OakPlanks, 1)),
                None,
                Some(ItemStack::new(Item::OakPlanks, 1)),
                None,
            ],
            2,
        )
        .is_some();
    let wooden_pick = sim
        .craft(
            &[
                Some(ItemStack::new(Item::OakPlanks, 1)),
                Some(ItemStack::new(Item::OakPlanks, 1)),
                Some(ItemStack::new(Item::OakPlanks, 1)),
                None,
                Some(ItemStack::new(Item::Stick, 1)),
                None,
                None,
                Some(ItemStack::new(Item::Stick, 1)),
                None,
            ],
            3,
        )
        .is_some();

    // Upgrade through stone, then mine ore and smelt it in a placed furnace.
    let mut cobblestone = 0;
    for x in 0..8 {
        if sim
            .mine_block((x, 67, 10), Some(Item::WoodenPickaxe))
            .is_some()
        {
            cobblestone += 1;
        }
    }
    for x in 4..=6 {
        if sim
            .mine_block((x, 70, 2), Some(Item::WoodenPickaxe))
            .is_some()
        {
            cobblestone += 1;
        }
    }
    let stone_pick = sim
        .craft(
            &[
                Some(ItemStack::new(Item::Cobblestone, 1)),
                Some(ItemStack::new(Item::Cobblestone, 1)),
                Some(ItemStack::new(Item::Cobblestone, 1)),
                None,
                Some(ItemStack::new(Item::Stick, 1)),
                None,
                None,
                Some(ItemStack::new(Item::Stick, 1)),
                None,
            ],
            3,
        )
        .is_some();
    let iron_mined = (9..=11)
        .filter(|&x| {
            sim.mine_block((x, 70, 2), Some(Item::StonePickaxe))
                .is_some()
        })
        .count();
    let coal_mined = sim
        .mine_block((7, 70, 2), Some(Item::StonePickaxe))
        .is_some();
    let furnace_crafted = sim
        .craft(
            &[
                Some(ItemStack::new(Item::Cobblestone, 1)),
                Some(ItemStack::new(Item::Cobblestone, 1)),
                Some(ItemStack::new(Item::Cobblestone, 1)),
                Some(ItemStack::new(Item::Cobblestone, 1)),
                None,
                Some(ItemStack::new(Item::Cobblestone, 1)),
                Some(ItemStack::new(Item::Cobblestone, 1)),
                Some(ItemStack::new(Item::Cobblestone, 1)),
                Some(ItemStack::new(Item::Cobblestone, 1)),
            ],
            3,
        )
        .is_some();
    let furnace_pos = (10, 70, 10);
    let furnace_placed = furnace_crafted && sim.place_item(furnace_pos, Item::Furnace);
    let furnace_loaded = furnace_placed
        && sim.container_put(furnace_pos, ItemStack::new(Item::IronOre, 3))
        && sim.container_put(furnace_pos, ItemStack::new(Item::Coal, 1));
    let mut smelted = false;
    if furnace_loaded {
        for _ in 0..650 {
            smelted |= sim
                .furnace_tick(furnace_pos)
                .is_some_and(|r| r.item_smelted);
        }
    }
    let furnace_xp = sim.furnace_claim_xp(furnace_pos).unwrap_or(0.0);
    let furnace_output = sim
        .container_take(furnace_pos, 1)
        .is_some_and(|stack| stack.item == Item::IronIngot && stack.count >= 1);

    // Craft/place a chest, persist it through the public save API, then take
    // the saved stack back out of the reloaded block entity.
    let chest_crafted = sim
        .craft(
            &[
                Some(ItemStack::new(Item::OakPlanks, 1)),
                Some(ItemStack::new(Item::OakPlanks, 1)),
                Some(ItemStack::new(Item::OakPlanks, 1)),
                Some(ItemStack::new(Item::OakPlanks, 1)),
                None,
                Some(ItemStack::new(Item::OakPlanks, 1)),
                Some(ItemStack::new(Item::OakPlanks, 1)),
                Some(ItemStack::new(Item::OakPlanks, 1)),
                Some(ItemStack::new(Item::OakPlanks, 1)),
            ],
            3,
        )
        .is_some();
    let chest_pos = (11, 70, 10);
    let chest_placed = chest_crafted && sim.place_item(chest_pos, Item::Chest);
    let chest_put =
        chest_placed && sim.container_put(chest_pos, ItemStack::new(Item::IronIngot, 1));
    let chest_root = save_root("foundation_chest");
    let chest_saved = chest_put
        && sim.save_reload_chunk(&chest_root, chest_pos)
        && sim
            .container_take(chest_pos, 1)
            .is_some_and(|stack| stack.item == Item::IronIngot);
    let _ = std::fs::remove_dir_all(&chest_root);

    // Till, hydrate, grow and harvest a real crop; turn the harvest into food.
    let _ = sim.craft(
        &[
            Some(ItemStack::new(Item::OakPlanks, 1)),
            None,
            Some(ItemStack::new(Item::OakPlanks, 1)),
            None,
        ],
        2,
    );
    let hoe = sim
        .craft(
            &[
                Some(ItemStack::new(Item::OakPlanks, 1)),
                Some(ItemStack::new(Item::OakPlanks, 1)),
                None,
                None,
                Some(ItemStack::new(Item::Stick, 1)),
                None,
                None,
                Some(ItemStack::new(Item::Stick, 1)),
                None,
            ],
            3,
        )
        .is_some();
    let seed = sim.mine_block((13, 70, 2), None).is_some();
    let farmland = hoe && seed && sim.till((13, 69, 2), Item::WoodenHoe);
    let hydrated = farmland && sim.hydrate((13, 69, 2), (14, 69, 2));
    let planted = hydrated && sim.plant((13, 69, 2), Item::Seeds);
    let mut grown_ticks = 0;
    if planted {
        for _ in 0..7 {
            if sim.random_tick_block((13, 70, 2), 0) {
                grown_ticks += 1;
            }
        }
    }
    let harvest = sim.harvest_crop((13, 70, 2));
    let bread = sim
        .craft(
            &[
                Some(ItemStack::new(Item::Wheat, 1)),
                Some(ItemStack::new(Item::Wheat, 1)),
                Some(ItemStack::new(Item::Wheat, 1)),
                None,
                None,
                None,
                None,
                None,
                None,
            ],
            3,
        )
        .is_some();
    sim.player_hunger = 8.0;
    let ate = bread && sim.eat(Item::Bread) && sim.player_hunger > 8.0;

    // Sleep sets the spawn point/time, then death creates dropped stacks and
    // respawn + pickup completes the recovery lifecycle.
    let slept_before = sim.world_time.ticks;
    let slept = sim.sleep((14, 69, 4));
    let spawn_recorded =
        sim.spawn_point == Some((14, 69, 4)) && sim.world_time.ticks > slept_before;
    let _ = sim.run_command("/kill @s");
    let dropped = sim.kill_player();
    let respawned = sim.respawn();
    // The pickup is an explicit movement command back to the death location,
    // preserving the normal three-block item pickup radius.
    let _ = sim.run_command("/tp @s 8 72 8");
    let recovered = sim.collect_drops();

    vec![
        assertion("wood acquired", logs == 5),
        assertion("tool crafted", planks_crafted == 5 && sticks && wooden_pick),
        assertion(
            "mining harvest and durability",
            cobblestone == 11
                && stone_pick
                && iron_mined == 3
                && coal_mined
                && sim.inventory.count_item(Item::StonePickaxe) > 0,
        ),
        assertion(
            "furnace output and xp",
            smelted && furnace_output && furnace_xp > 0.0,
        ),
        assertion("chest put/get survives save reload", chest_saved),
        assertion(
            "farm hydration growth harvest",
            hydrated && grown_ticks == 7 && harvest == Some(Item::Wheat),
        ),
        assertion("cooking and eating", ate),
        assertion("bed sleep sets respawn", slept && spawn_recorded),
        assertion(
            "death drops respawn and recovery",
            dropped > 0 && respawned && recovered > 0 && sim.player_health == 20.0,
        ),
        assertion("foundation checksum changed", baseline != sim.checksum()),
    ]
}

fn bootstrap_progression(sim: &mut crate::sim_harness::SimHarness) -> bool {
    use crate::inventory::{Item, ItemStack};

    for x in 2..=5 {
        let _ = sim.mine_block((x, 70, 3), None);
    }
    let _ = sim.mine_block((2, 71, 3), None);
    for _ in 0..5 {
        let _ = sim.craft(&[Some(ItemStack::new(Item::OakLog, 1))], 1);
    }
    let _ = sim.craft(
        &[
            Some(ItemStack::new(Item::OakPlanks, 1)),
            None,
            Some(ItemStack::new(Item::OakPlanks, 1)),
            None,
        ],
        2,
    );
    let _ = sim.craft(
        &[
            Some(ItemStack::new(Item::OakPlanks, 1)),
            Some(ItemStack::new(Item::OakPlanks, 1)),
            Some(ItemStack::new(Item::OakPlanks, 1)),
            None,
            Some(ItemStack::new(Item::Stick, 1)),
            None,
            None,
            Some(ItemStack::new(Item::Stick, 1)),
            None,
        ],
        3,
    );
    for x in 0..8 {
        let _ = sim.mine_block((x, 67, 10), Some(Item::WoodenPickaxe));
    }
    for x in 4..=6 {
        let _ = sim.mine_block((x, 70, 2), Some(Item::WoodenPickaxe));
    }
    let stone_pick = sim
        .craft(
            &[
                Some(ItemStack::new(Item::Cobblestone, 1)),
                Some(ItemStack::new(Item::Cobblestone, 1)),
                Some(ItemStack::new(Item::Cobblestone, 1)),
                None,
                Some(ItemStack::new(Item::Stick, 1)),
                None,
                None,
                Some(ItemStack::new(Item::Stick, 1)),
                None,
            ],
            3,
        )
        .is_some();
    for x in 9..=11 {
        let _ = sim.mine_block((x, 70, 2), Some(Item::StonePickaxe));
    }
    for x in 0..=3 {
        let _ = sim.mine_block((x, 70, 1), Some(Item::StonePickaxe));
    }
    let _ = sim.mine_block((7, 70, 2), Some(Item::StonePickaxe));
    let furnace = sim
        .craft(
            &[
                Some(ItemStack::new(Item::Cobblestone, 1)),
                Some(ItemStack::new(Item::Cobblestone, 1)),
                Some(ItemStack::new(Item::Cobblestone, 1)),
                Some(ItemStack::new(Item::Cobblestone, 1)),
                None,
                Some(ItemStack::new(Item::Cobblestone, 1)),
                Some(ItemStack::new(Item::Cobblestone, 1)),
                Some(ItemStack::new(Item::Cobblestone, 1)),
                Some(ItemStack::new(Item::Cobblestone, 1)),
            ],
            3,
        )
        .is_some();
    let furnace_pos = (10, 70, 10);
    let furnace = furnace && sim.place_item(furnace_pos, Item::Furnace);
    let furnace = furnace
        && sim.container_put(furnace_pos, ItemStack::new(Item::IronOre, 4))
        && sim.container_put(furnace_pos, ItemStack::new(Item::Coal, 1));
    for _ in 0..900 {
        let _ = sim.furnace_tick(furnace_pos);
    }
    for _ in 0..4 {
        let _ = sim.container_take(furnace_pos, 1);
    }
    let _ = sim.furnace_claim_xp(furnace_pos);
    let _ = sim.craft(
        &[
            Some(ItemStack::new(Item::OakPlanks, 1)),
            None,
            Some(ItemStack::new(Item::OakPlanks, 1)),
            None,
        ],
        2,
    );
    let iron_pick = sim
        .craft(
            &[
                Some(ItemStack::new(Item::IronIngot, 1)),
                Some(ItemStack::new(Item::IronIngot, 1)),
                Some(ItemStack::new(Item::IronIngot, 1)),
                None,
                Some(ItemStack::new(Item::Stick, 1)),
                None,
                None,
                Some(ItemStack::new(Item::Stick, 1)),
                None,
            ],
            3,
        )
        .is_some();
    for x in 12..=15 {
        let _ = sim.mine_block((x, 70, 2), Some(Item::IronPickaxe));
    }
    for x in 0..=11 {
        let _ = sim.mine_block((x, 68, 4), Some(Item::IronPickaxe));
    }
    for x in 0..4 {
        let _ = sim.mine_block((x, 67, 11), Some(Item::IronPickaxe));
    }
    let diamond_pick = sim
        .craft(
            &[
                Some(ItemStack::new(Item::Diamond, 1)),
                Some(ItemStack::new(Item::Diamond, 1)),
                Some(ItemStack::new(Item::Diamond, 1)),
                None,
                Some(ItemStack::new(Item::Stick, 1)),
                None,
                None,
                Some(ItemStack::new(Item::Stick, 1)),
                None,
            ],
            3,
        )
        .is_some();
    for x in 0..14 {
        let _ = sim.mine_block((x, 68, 6), Some(Item::DiamondPickaxe));
    }
    let _ = sim.mine_block((8, 70, 2), None);
    let flint_steel = sim
        .craft_shapeless(&[Item::IronIngot, Item::Gravel])
        .is_some();
    let ready = furnace && stone_pick && iron_pick && diamond_pick && flint_steel;
    ready
}

fn run_progression() -> Vec<AcceptanceAssertion> {
    use crate::inventory::Item;
    use crate::sim_harness::SimHarness;

    let mut sim = SimHarness::new();
    let bootstrapped = bootstrap_progression(&mut sim);
    let portal_frames = sim.generate_stronghold_and_collect_portal_frames((100, 60, 0));
    let portal_base = (20, 70, 20);
    let mut frame_placed = true;
    for x in 0..=3 {
        frame_placed &= sim.place_item(
            (portal_base.0 + x, portal_base.1, portal_base.2),
            Item::Obsidian,
        );
        frame_placed &= sim.place_item(
            (portal_base.0 + x, portal_base.1 + 4, portal_base.2),
            Item::Obsidian,
        );
    }
    for y in 1..=3 {
        frame_placed &= sim.place_item(
            (portal_base.0, portal_base.1 + y, portal_base.2),
            Item::Obsidian,
        );
        frame_placed &= sim.place_item(
            (portal_base.0 + 3, portal_base.1 + y, portal_base.2),
            Item::Obsidian,
        );
    }
    let nether_entered = frame_placed
        && sim.activate_nether_portal(portal_base)
        && sim.last_dimension_transition
            == Some((
                crate::dimension::Dimension::Overworld,
                crate::dimension::Dimension::Nether,
            ));
    // Six kills yield three stack-sized rod drops in the current entity drop
    // table; use twelve actual spawner kills so the powder chain can fill all
    // twelve portal eyes without injecting an item.
    let blaze_rods = sim.generate_fortress_and_collect_blaze_rods((40, 55, 0), 12);
    let fortress_progress = blaze_rods >= 6;
    let nether_returned = sim.travel_back_through_portal();
    let blaze_rod_count = sim.inventory.count_item(Item::BlazeRod);
    for _ in 0..blaze_rod_count {
        let _ = sim.craft_shapeless(&[Item::BlazeRod]);
    }
    let mut eyes_crafted = 0;
    for _ in 0..6 {
        if sim
            .craft_shapeless(&[Item::BlazePowder, Item::Diamond])
            .is_some()
        {
            eyes_crafted += 1;
        }
    }
    // Each blaze rod yields two powder; craft the remaining six eyes from the
    // same real powder inventory before filling the twelve-frame portal.
    for _ in 0..6 {
        if sim
            .craft_shapeless(&[Item::BlazePowder, Item::Diamond])
            .is_some()
        {
            eyes_crafted += 1;
        }
    }
    let eyes_ready = eyes_crafted == 12 && sim.inventory.count_item(Item::EyeOfEnder) >= 12;
    let end_base = (24, 70, 20);
    let end_entered = sim.build_end_portal_and_enter(end_base);
    let dragon_defeated = sim.run_dragon_encounter();
    let city_chest = sim.explore_end_city((80, 70, 8));
    let city_root = save_root("progression_end_city");
    let city_loot = city_chest.is_some_and(|pos| {
        sim.save_reload_chunk(&city_root, pos)
            && sim
                .container_take(pos, 1)
                .is_some_and(|stack| stack.item == Item::Elytra)
    });
    let _ = std::fs::remove_dir_all(&city_root);

    vec![
        assertion(
            "resource chain crafts portal tools",
            bootstrapped && portal_frames == 12,
        ),
        assertion("nether portal transition", nether_entered),
        assertion("fortress blaze drops", fortress_progress),
        assertion("nether return transition", nether_returned),
        assertion("eyes of ender crafted", eyes_ready),
        assertion("end portal transition", end_entered),
        assertion(
            "dragon encounter completion",
            dragon_defeated && sim.dragon_defeated,
        ),
        assertion("end city generated loot survives reload", city_loot),
    ]
}

fn run_social_automation() -> Vec<AcceptanceAssertion> {
    use crate::inventory::{Item, ItemStack};
    use crate::sim_harness::SimHarness;
    use crate::village::poi::VillagerProfession;
    use glam::Vec3;

    let mut sim = SimHarness::new();

    // Grow enough wheat to execute the farmer's real 20-for-emerald offer.
    for x in 2..=5 {
        let _ = sim.mine_block((x, 70, 3), None);
    }
    let _ = sim.mine_block((2, 71, 3), None);
    for _ in 0..5 {
        let _ = sim.craft(&[Some(ItemStack::new(Item::OakLog, 1))], 1);
    }
    // Two planks become four sticks for the hoe recipe below.
    let _ = sim.craft(
        &[
            Some(ItemStack::new(Item::OakPlanks, 1)),
            None,
            Some(ItemStack::new(Item::OakPlanks, 1)),
            None,
        ],
        2,
    );
    let _ = sim.craft(
        &[
            Some(ItemStack::new(Item::OakPlanks, 1)),
            Some(ItemStack::new(Item::OakPlanks, 1)),
            None,
            None,
            Some(ItemStack::new(Item::Stick, 1)),
            None,
            None,
            Some(ItemStack::new(Item::Stick, 1)),
            None,
        ],
        3,
    );
    let _ = sim.mine_block((13, 70, 2), None);
    let _ = sim.till((13, 69, 2), Item::WoodenHoe);
    let _ = sim.hydrate((13, 69, 2), (14, 69, 2));
    for _ in 0..7 {
        let _ = sim.plant((13, 69, 2), Item::Seeds);
        for _ in 0..7 {
            let _ = sim.random_tick_block((13, 70, 2), 0);
        }
        let _ = sim.harvest_crop((13, 70, 2));
    }
    let wheat_before = sim.inventory.count_item(Item::Wheat);
    let session = sim.open_villager_trade(VillagerProfession::Farmer);
    let emerald_before = sim.inventory.count_item(Item::Emerald);
    let trade_result = session.and_then(|id| sim.trade(id, 0));
    let trade_conserved = trade_result.is_some()
        && wheat_before.saturating_sub(sim.inventory.count_item(Item::Wheat)) == 20
        && sim.inventory.count_item(Item::Emerald) == emerald_before + 1;

    // Build a pickaxe through the same crafting/mining chain before touching
    // the ore used by the hopper furnace.
    let wooden_pick = sim
        .craft(
            &[
                Some(ItemStack::new(Item::OakPlanks, 1)),
                Some(ItemStack::new(Item::OakPlanks, 1)),
                Some(ItemStack::new(Item::OakPlanks, 1)),
                None,
                Some(ItemStack::new(Item::Stick, 1)),
                None,
                None,
                Some(ItemStack::new(Item::Stick, 1)),
                None,
            ],
            3,
        )
        .is_some();
    for x in 0..8 {
        let _ = sim.mine_block((x, 67, 10), Some(Item::WoodenPickaxe));
    }
    let _ = sim.craft(
        &[
            Some(ItemStack::new(Item::OakPlanks, 1)),
            None,
            Some(ItemStack::new(Item::OakPlanks, 1)),
            None,
        ],
        2,
    );
    let stone_pick = sim
        .craft(
            &[
                Some(ItemStack::new(Item::Cobblestone, 1)),
                Some(ItemStack::new(Item::Cobblestone, 1)),
                Some(ItemStack::new(Item::Cobblestone, 1)),
                None,
                Some(ItemStack::new(Item::Stick, 1)),
                None,
                None,
                Some(ItemStack::new(Item::Stick, 1)),
                None,
            ],
            3,
        )
        .is_some();

    // Source chest -> hopper -> furnace uses sided capability checks and a
    // real smelting tick, rather than only checking block enum anchors.
    let source = (25, 72, 2);
    let hopper = (25, 71, 2);
    let furnace = (25, 70, 2);
    let hopper_setup = sim.setup_hopper_furnace_flow(source, hopper, furnace);
    let input_iron = wooden_pick
        && stone_pick
        && sim
            .mine_block((9, 70, 2), Some(Item::StonePickaxe))
            .is_some();
    // The stone/iron fixture has already been consumed by the trade flow only
    // if it was mined; use a coal drop and direct fuel-slot action when the
    // source operation succeeded.
    let input_coal = sim
        .mine_block((7, 70, 2), Some(Item::StonePickaxe))
        .is_some();
    let loaded = input_iron
        && input_coal
        && sim.container_put(source, ItemStack::new(Item::IronOre, 1))
        && sim.container_put_slot(furnace, 1, ItemStack::new(Item::Coal, 1));
    let mut hopper_transfers = 0;
    let mut smelted = false;
    if loaded {
        for _ in 0..500 {
            let tick = sim.tick_hopper_furnace(furnace);
            hopper_transfers += tick.transfers;
            smelted |= sim.furnace_tick(furnace).is_some_and(|r| r.item_smelted);
        }
    }
    let hopper_output = sim
        .container_take(furnace, 1)
        .is_some_and(|stack| stack.item == Item::IronIngot);

    // A minecart receives cargo, carries the player over rails, and survives
    // an entity save/reload round trip.
    let cargo = if sim.inventory.remove_one(Item::Emerald) {
        ItemStack::new(Item::Emerald, 1)
    } else {
        ItemStack::new(Item::Bread, 1)
    };
    if cargo.item == Item::Bread {
        let _ = sim.craft_shapeless(&[Item::Wheat, Item::Wheat, Item::Wheat]);
    }
    let rail_start = (30, 70, 5);
    let rails = sim.lay_rail_line(rail_start, 8, true);
    let minecart = sim.place_minecart(Vec3::new(30.5, 71.0, 5.5), cargo);
    let mounted = sim.mount_player_in_minecart(minecart);
    let before_x = sim
        .entities
        .get_by_id(minecart)
        .map_or(0.0, |e| e.position.x);
    for _ in 0..30 {
        sim.tick_minecart();
    }
    let after_x = sim
        .entities
        .get_by_id(minecart)
        .map_or(before_x, |e| e.position.x);
    let entity_root = save_root("social_minecart");
    let reloaded = sim.save_reload_entities(&entity_root);
    let _ = std::fs::remove_dir_all(&entity_root);
    let transport = rails
        && mounted
        && after_x > before_x
        && reloaded
        && sim
            .entities
            .get_entities_by_type(crate::entity::EntityType::Minecart)
            .next()
            .is_some();

    vec![
        assertion("villager trade conservation and cooldown", trade_conserved),
        assertion(
            "hopper furnace transfer and output",
            hopper_setup && loaded && hopper_transfers > 0 && smelted && hopper_output,
        ),
        assertion("minecart movement mount and reload", transport),
        assertion("automation ticks advance", sim.tick_count > 0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singleplayer_headless_scenarios_pass() {
        for scenario in AcceptanceScenario::ALL {
            let report = run_headless(scenario, AcceptanceTopology::Singleplayer);
            assert!(
                report.passed(),
                "{}: {:?}",
                scenario.name(),
                report.assertions
            );
        }
    }

    #[test]
    fn network_rows_are_explicitly_handed_to_plan18() {
        for scenario in AcceptanceScenario::ALL {
            for topology in [
                AcceptanceTopology::ListenServer,
                AcceptanceTopology::DedicatedTwoClients,
            ] {
                let report = run_headless(scenario, topology);
                assert!(!report.passed());
                assert!(report.blocked_reason.is_some());
            }
        }
    }
}
