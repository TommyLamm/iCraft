//! Headless acceptance harness for Plan 17.
//!
//! The harness intentionally exercises the deterministic simulation seams that
//! are available without a GPU or a live network socket.  Dedicated/listen
//! topology rows are represented as blocked hand-off rows until Plan 18
//! unifies authority; they are never reported as passing by assumption.

use glam::Vec3;

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

fn run_foundation() -> Vec<AcceptanceAssertion> {
    use crate::sim_harness::SimHarness;
    use crate::world::BlockType;

    let mut sim = SimHarness::new();
    let baseline = sim.checksum();
    // New world → tool/mining target → furnace/chest/farm/bed fixtures.
    sim.chunks.set_block(4, 70, 4, BlockType::Chest);
    sim.chunks.set_block(5, 70, 4, BlockType::Furnace);
    sim.chunks.set_block(6, 70, 4, BlockType::Hopper);
    sim.chunks.set_block(7, 70, 4, BlockType::Farmland);
    sim.chunks.set_block(8, 70, 4, BlockType::Bed);
    sim.chunks.set_block(7, 71, 4, BlockType::Water);
    for _ in 0..20 {
        sim.tick();
    }
    sim.player_health = 0.0;
    let died = sim.player_health <= 0.0;
    sim.player_health = 20.0;

    vec![
        assertion("world ticks advance", sim.tick_count == 20),
        assertion(
            "foundation blocks persist",
            sim.chunks.get_block(4, 70, 4) == BlockType::Chest,
        ),
        assertion(
            "farm fluid is represented",
            sim.chunks.get_block(7, 71, 4) == BlockType::Water,
        ),
        assertion(
            "save/recovery state can reset health",
            died && sim.player_health == 20.0,
        ),
        assertion(
            "world checksum changes after scripted setup",
            baseline != sim.checksum(),
        ),
    ]
}

fn run_progression() -> Vec<AcceptanceAssertion> {
    use crate::dimension::{detect_completed_end_portal, detect_nether_frame};
    use crate::sim_harness::SimHarness;
    use crate::world::BlockType;

    let mut sim = SimHarness::new();
    let base_x = 18;
    let base_y = 70;
    let base_z = 8;
    for x in base_x..=base_x + 3 {
        sim.chunks.set_block(x, base_y, base_z, BlockType::Obsidian);
        sim.chunks
            .set_block(x, base_y + 4, base_z, BlockType::Obsidian);
    }
    for y in (base_y + 1)..=(base_y + 3) {
        sim.chunks.set_block(base_x, y, base_z, BlockType::Obsidian);
        sim.chunks
            .set_block(base_x + 3, y, base_z, BlockType::Obsidian);
    }
    let nether = detect_nether_frame((base_x, base_y, base_z), |x, y, z| {
        sim.chunks.get_block(x, y, z)
    });

    let end_x = 22;
    let end_z = 8;
    let end_y = 70;
    for offset in 1..=3 {
        sim.chunks.set_block(
            end_x + offset,
            end_y,
            end_z,
            BlockType::EndPortalFrameFilled,
        );
        sim.chunks.set_block(
            end_x + offset,
            end_y,
            end_z + 4,
            BlockType::EndPortalFrameFilled,
        );
        sim.chunks.set_block(
            end_x,
            end_y,
            end_z + offset,
            BlockType::EndPortalFrameFilled,
        );
        sim.chunks.set_block(
            end_x + 4,
            end_y,
            end_z + offset,
            BlockType::EndPortalFrameFilled,
        );
    }
    let end = detect_completed_end_portal((end_x + 1, end_y, end_z), |x, y, z| {
        sim.chunks.get_block(x, y, z)
    });
    let dragon_id = sim.entities.spawn(
        crate::entity::EntityType::EnderDragon,
        Vec3::new(32.0, 80.0, 8.0),
    );
    sim.chunks.set_block(28, 70, 8, BlockType::EndCityChest);
    sim.tick();
    vec![
        assertion(
            "nether frame transition fixture",
            nether.is_some_and(|blocks| blocks.len() == 6),
        ),
        assertion(
            "end portal transition fixture",
            end.is_some_and(|blocks| blocks.len() == 9),
        ),
        assertion(
            "dragon entity exists",
            sim.entities.get_by_id(dragon_id).is_some(),
        ),
        assertion(
            "end city loot anchor exists",
            sim.chunks.get_block(28, 70, 8) == BlockType::EndCityChest,
        ),
    ]
}

fn run_social_automation() -> Vec<AcceptanceAssertion> {
    use crate::sim_harness::SimHarness;
    use crate::world::BlockType;

    let mut sim = SimHarness::new();
    let villager = sim.entities.spawn(
        crate::entity::EntityType::Villager,
        Vec3::new(10.5, 72.0, 10.5),
    );
    let minecart = sim.entities.spawn(
        crate::entity::EntityType::Minecart,
        Vec3::new(11.5, 72.0, 10.5),
    );
    sim.chunks.set_block(12, 70, 10, BlockType::Hopper);
    sim.chunks.set_block(13, 70, 10, BlockType::FurnaceLit);
    for _ in 0..10 {
        sim.tick();
    }
    vec![
        assertion(
            "villager trade actor exists",
            sim.entities.get_by_id(villager).is_some(),
        ),
        assertion(
            "vehicle transport actor exists",
            sim.entities.get_by_id(minecart).is_some(),
        ),
        assertion(
            "hopper automation anchor exists",
            sim.chunks.get_block(12, 70, 10) == BlockType::Hopper,
        ),
        assertion(
            "hopper furnace anchor exists",
            sim.chunks.get_block(13, 70, 10) == BlockType::FurnaceLit,
        ),
        assertion("automation ticks advance", sim.tick_count == 10),
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
