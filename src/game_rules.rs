//! Authoritative game-mode and world-rule policy.
//!
//! The runtime keeps one `WorldRules` snapshot on the host.  Systems consume
//! this typed value instead of inferring rules from menu settings or from a
//! collection of ad-hoc `GameMode` checks.  The wire/save representation is
//! intentionally small and uses serde defaults so worlds created before this
//! module was introduced retain the vanilla-like defaults.

use crate::inventory::{GameMode, ItemStack};
use crate::world::BlockType;
use serde::{Deserialize, Serialize};

/// The two generation presets currently exposed by world creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorldType {
    Default,
    Superflat,
}

impl Default for WorldType {
    fn default() -> Self {
        Self::Default
    }
}

impl WorldType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "DEFAULT",
            Self::Superflat => "SUPERFLAT",
        }
    }

    pub fn parse(value: &str) -> Self {
        if value.trim().eq_ignore_ascii_case("superflat") {
            Self::Superflat
        } else {
            Self::Default
        }
    }
}

/// Rules that affect simulation.  Keep this a plain value: a host can clone
/// it for a tick and clients can replace their display snapshot atomically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldRules {
    #[serde(default)]
    pub keep_inventory: bool,
    #[serde(default = "default_true")]
    pub mob_griefing: bool,
    #[serde(default = "default_true")]
    pub do_mob_spawning: bool,
    #[serde(default = "default_true")]
    pub do_daylight_cycle: bool,
    #[serde(default = "default_true")]
    pub do_weather_cycle: bool,
    #[serde(default = "default_true")]
    pub do_fire_tick: bool,
    #[serde(default = "default_true")]
    pub do_insomnia: bool,
    #[serde(default = "default_sleeping_percentage")]
    pub sleeping_percentage: u8,
    #[serde(default = "default_true")]
    pub pvp: bool,
    /// Hardcore is a world property rather than a fifth player mode.  It is
    /// persisted with the same snapshot so a death cannot silently downgrade
    /// the world to Survival on reload.
    #[serde(default)]
    pub hardcore: bool,
}

const fn default_true() -> bool {
    true
}

const fn default_sleeping_percentage() -> u8 {
    100
}

impl Default for WorldRules {
    fn default() -> Self {
        Self {
            keep_inventory: false,
            mob_griefing: true,
            do_mob_spawning: true,
            do_daylight_cycle: true,
            do_weather_cycle: true,
            do_fire_tick: true,
            do_insomnia: true,
            sleeping_percentage: 100,
            pvp: true,
            hardcore: false,
        }
    }
}

impl WorldRules {
    pub fn normalized(mut self) -> Self {
        self.sleeping_percentage = self.sleeping_percentage.clamp(1, 100);
        self
    }

    pub fn set(&mut self, name: &str, value: bool) -> Result<(), RuleError> {
        match name.trim().to_ascii_lowercase().as_str() {
            "keepinventory" | "keep_inventory" => self.keep_inventory = value,
            "mobgriefing" | "mob_griefing" => self.mob_griefing = value,
            "domobspawning" | "do_mob_spawning" => self.do_mob_spawning = value,
            "dodaylightcycle" | "do_daylight_cycle" => self.do_daylight_cycle = value,
            "doweathercycle" | "do_weather_cycle" => self.do_weather_cycle = value,
            "dofiretick" | "do_fire_tick" => self.do_fire_tick = value,
            "doinsomnia" | "do_insomnia" => self.do_insomnia = value,
            "pvp" => self.pvp = value,
            _ => return Err(RuleError::UnknownRule(name.to_string())),
        }
        Ok(())
    }

    pub fn set_sleeping_percentage(&mut self, percentage: u8) {
        self.sleeping_percentage = percentage.clamp(1, 100);
    }

    pub fn value(&self, name: &str) -> Option<String> {
        let value = match name.trim().to_ascii_lowercase().as_str() {
            "keepinventory" | "keep_inventory" => self.keep_inventory,
            "mobgriefing" | "mob_griefing" => self.mob_griefing,
            "domobspawning" | "do_mob_spawning" => self.do_mob_spawning,
            "dodaylightcycle" | "do_daylight_cycle" => self.do_daylight_cycle,
            "doweathercycle" | "do_weather_cycle" => self.do_weather_cycle,
            "dofiretick" | "do_fire_tick" => self.do_fire_tick,
            "doinsomnia" | "do_insomnia" => self.do_insomnia,
            "pvp" => self.pvp,
            "playerssleepingpercentage" | "sleepingpercentage" | "sleeping_percentage" => {
                return Some(self.sleeping_percentage.to_string())
            }
            _ => return None,
        };
        Some(value.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleError {
    UnknownRule(String),
}

impl std::fmt::Display for RuleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownRule(rule) => write!(f, "unknown gamerule `{rule}`"),
        }
    }
}

impl std::error::Error for RuleError {}

/// Capability policy derived from the current mode and world rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameModePolicy {
    pub mode: GameMode,
    pub can_collide: bool,
    pub can_take_damage: bool,
    pub hunger_enabled: bool,
    pub can_fly: bool,
    pub can_phase: bool,
    pub can_break: bool,
    pub can_place: bool,
    pub can_pickup: bool,
    pub can_target_mobs: bool,
    pub can_use_containers: bool,
    pub can_attack_players: bool,
}

impl GameModePolicy {
    pub const fn for_mode(mode: GameMode, pvp: bool) -> Self {
        match mode {
            GameMode::Creative => Self {
                mode,
                can_collide: true,
                can_take_damage: false,
                hunger_enabled: false,
                can_fly: true,
                can_phase: false,
                can_break: true,
                can_place: true,
                can_pickup: true,
                can_target_mobs: true,
                can_use_containers: true,
                can_attack_players: pvp,
            },
            GameMode::Survival | GameMode::Adventure => Self {
                mode,
                can_collide: true,
                can_take_damage: true,
                hunger_enabled: true,
                can_fly: false,
                can_phase: false,
                can_break: matches!(mode, GameMode::Survival),
                can_place: matches!(mode, GameMode::Survival),
                can_pickup: true,
                can_target_mobs: true,
                can_use_containers: true,
                can_attack_players: pvp,
            },
            GameMode::Spectator => Self {
                mode,
                can_collide: false,
                can_take_damage: false,
                hunger_enabled: false,
                can_fly: true,
                can_phase: true,
                can_break: false,
                can_place: false,
                can_pickup: false,
                can_target_mobs: false,
                can_use_containers: false,
                can_attack_players: false,
            },
        }
    }

    pub const fn for_rules(mode: GameMode, rules: &WorldRules) -> Self {
        Self::for_mode(mode, rules.pvp)
    }

    pub fn can_break_stack(&self, stack: Option<&ItemStack>, block: BlockType) -> bool {
        if self.mode == GameMode::Adventure {
            return stack.is_some_and(|stack| stack.can_break_block(block));
        }
        if !self.can_break {
            return false;
        }
        true
    }

    pub fn can_place_stack(&self, stack: Option<&ItemStack>, block: BlockType) -> bool {
        if self.mode == GameMode::Adventure {
            return stack.is_some_and(|stack| stack.can_place_on_block(block));
        }
        if !self.can_place {
            return false;
        }
        true
    }
}

/// Options stored in `world.meta` while a world is created.  LevelData copies
/// these into the authoritative binary save on first successful save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldCreationOptions {
    pub world_type: WorldType,
    pub generate_structures: bool,
    pub bonus_chest: bool,
    pub cheats_enabled: bool,
    pub hardcore: bool,
}

impl Default for WorldCreationOptions {
    fn default() -> Self {
        Self {
            world_type: WorldType::Default,
            generate_structures: true,
            bonus_chest: false,
            cheats_enabled: false,
            hardcore: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_truth_table_covers_all_modes() {
        let rules = WorldRules::default();
        let survival = GameModePolicy::for_rules(GameMode::Survival, &rules);
        assert!(survival.can_collide && survival.can_break && survival.hunger_enabled);
        let creative = GameModePolicy::for_rules(GameMode::Creative, &rules);
        assert!(creative.can_fly && !creative.can_take_damage);
        let adventure = GameModePolicy::for_rules(GameMode::Adventure, &rules);
        assert!(!adventure.can_break && !adventure.can_place);
        let spectator = GameModePolicy::for_rules(GameMode::Spectator, &rules);
        assert!(spectator.can_phase && !spectator.can_use_containers && !spectator.can_pickup);
    }

    #[test]
    fn rule_updates_are_bounded_and_aliases_are_supported() {
        let mut rules = WorldRules::default();
        rules.set("doDaylightCycle", false).unwrap();
        rules.set_sleeping_percentage(255);
        assert!(!rules.do_daylight_cycle);
        assert_eq!(rules.sleeping_percentage, 100);
        assert_eq!(rules.value("do_daylight_cycle"), Some("false".to_string()));
    }

    #[test]
    fn adventure_item_predicates_require_explicit_tags() {
        let policy = GameModePolicy::for_mode(GameMode::Adventure, true);
        let mut stack = ItemStack::new(crate::inventory::Item::StonePickaxe, 1);
        assert!(!policy.can_break_stack(Some(&stack), BlockType::Stone));
        stack = stack.with_can_break(BlockType::Stone);
        assert!(policy.can_break_stack(Some(&stack), BlockType::Stone));
    }
}
