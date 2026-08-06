#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DamageSource {
    Fall,
    Void,
    Hunger,
    Mob,
    Explosion,
    Drowning,
    Lightning,
}

impl DamageSource {
    pub const fn to_wire(self) -> u8 {
        match self {
            Self::Fall => 1,
            Self::Void => 2,
            Self::Hunger => 3,
            Self::Mob => 4,
            Self::Explosion => 5,
            Self::Drowning => 6,
            Self::Lightning => 7,
        }
    }

    pub const fn from_wire(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Fall),
            2 => Some(Self::Void),
            3 => Some(Self::Hunger),
            4 => Some(Self::Mob),
            5 => Some(Self::Explosion),
            6 => Some(Self::Drowning),
            7 => Some(Self::Lightning),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Hand {
    MainHand,
    OffHand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ItemUseAction {
    Eat,
    Drink,
    Block,
    Bow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandSlot {
    MainHand(usize),
    OffHand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsingItemState {
    pub hand: Hand,
    pub action: ItemUseAction,
    pub item: crate::inventory::Item,
    pub slot: HandSlot,
    pub ticks_held: u32,
    pub max_ticks: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveEatingState {
    pub item: crate::inventory::Item,
    pub slot: usize,
    pub ticks_remaining: u32,
    pub total_duration: u32,
}

pub fn calculate_damage_reduction(
    raw_damage: f32,
    source: DamageSource,
    armor_points: f32,
    toughness: f32,
    epf: u32,
) -> f32 {
    let (bypasses_armor, bypasses_enchantments) = match source {
        DamageSource::Void | DamageSource::Hunger | DamageSource::Drowning => (true, true),
        DamageSource::Fall => (true, false),
        _ => (false, false),
    };

    let damage_after_armor = if bypasses_armor || armor_points <= 0.0 {
        raw_damage
    } else {
        let defense =
            (armor_points - raw_damage / (2.0 + toughness / 4.0)).clamp(armor_points * 0.2, 20.0);
        raw_damage * (1.0 - defense / 25.0)
    };

    let final_damage = if bypasses_enchantments || epf == 0 {
        damage_after_armor
    } else {
        let epf_clamped = epf.min(20) as f32;
        damage_after_armor * (1.0 - epf_clamped / 25.0)
    };

    final_damage.max(0.0)
}

pub fn can_shield_block(
    player_facing_yaw: f32,
    player_pos: [f32; 3],
    damage_source_pos: Option<[f32; 3]>,
    source: DamageSource,
) -> bool {
    match source {
        DamageSource::Void | DamageSource::Hunger | DamageSource::Drowning | DamageSource::Fall => {
            return false
        }
        _ => {}
    }
    let Some(src_pos) = damage_source_pos else {
        return false;
    };

    let dx = src_pos[0] - player_pos[0];
    let dz = src_pos[2] - player_pos[2];
    if dx * dx + dz * dz < 1e-6 {
        return true;
    }

    let look_x = player_facing_yaw.cos();
    let look_z = player_facing_yaw.sin();

    let dist = (dx * dx + dz * dz).sqrt();
    let src_x = dx / dist;
    let src_z = dz / dist;

    let dot = look_x * src_x + look_z * src_z;
    dot > 0.0
}

pub fn calculate_attack_damage(
    base_damage: f32,
    charge_ticks: u32,
    cooldown_max_ticks: u32,
) -> (f32, f32, bool) {
    let charge_ratio = if cooldown_max_ticks == 0 {
        1.0
    } else {
        (charge_ticks as f32 / cooldown_max_ticks as f32).clamp(0.0, 1.0)
    };
    let damage_mult = 0.2 + 0.8 * charge_ratio * charge_ratio;
    let damage = base_damage * damage_mult;
    let knockback_mult = charge_ratio;
    let is_full_charge = charge_ratio >= 0.9;
    (damage, knockback_mult, is_full_charge)
}

pub fn calculate_bow_shot(held_ticks: u32) -> Option<(f32, f32, bool)> {
    if held_ticks < 3 {
        return None;
    }
    let t = (held_ticks as f32 / 20.0).clamp(0.0, 1.0);
    let speed = t * 3.0;
    let is_critical = t >= 1.0;
    let base_damage = 2.0 * speed;
    let damage = if is_critical {
        base_damage + 2.0
    } else {
        base_damage
    };
    Some((speed, damage, is_critical))
}

pub struct PlayerState {
    pub health: f32,     // 0 ~ 20 (10 hearts)
    pub max_health: f32, // 20
    pub hunger: f32,     // 0 ~ 20 (10 drumsticks)
    pub saturation: f32, // 0 ~ 20
    pub exhaustion: f32, // 0 ~ 4.0
    pub is_dead: bool,
    pub death_reason: Option<DamageSource>,
    pub invulnerable_time: f32,  // in seconds
    pub damaged_flash_time: f32, // in seconds (for screen red flash)
    pub regen_timer: f32,        // in seconds
    pub starve_timer: f32,       // in seconds
    pub oxygen: f32,             // 0.0 to 300.0
    pub drowning_timer: f32,     // in seconds
    pub experience: u32,
    pub experience_level: u32,
    pub spawn_point: Option<[i32; 3]>,
    pub spawn_dimension: Option<crate::dimension::Dimension>,
    pub is_sleeping: bool,
    pub sleep_timer: f32,
    pub bed_pos: Option<[i32; 3]>,
    pub unlocked_recipes: std::collections::HashSet<String>,
    pub eating_state: Option<ActiveEatingState>,
    pub using_item: Option<UsingItemState>,
    pub attack_cooldown_ticks: u32,
    pub attack_cooldown_max_ticks: u32,
    pub shield_disable_ticks: u32,
    pub bad_omen_level: u8,
    pub hero_of_the_village_timer: f32,
}

impl PlayerState {
    pub fn new() -> Self {
        let mut unlocked_recipes = std::collections::HashSet::new();
        unlocked_recipes.insert("crafting/oak_planks".to_string());
        unlocked_recipes.insert("crafting/stick_oak".to_string());
        unlocked_recipes.insert("crafting/crafting_table_oak".to_string());
        unlocked_recipes.insert("crafting/furnace".to_string());
        unlocked_recipes.insert("crafting/torch".to_string());
        unlocked_recipes.insert("smelting/iron_ingot".to_string());
        unlocked_recipes.insert("smelting/stone".to_string());
        unlocked_recipes.insert("smelting/cooked_porkchop".to_string());

        Self {
            health: 20.0,
            max_health: 20.0,
            hunger: 20.0,
            saturation: 5.0,
            exhaustion: 0.0,
            is_dead: false,
            death_reason: None,
            invulnerable_time: 0.0,
            damaged_flash_time: 0.0,
            regen_timer: 0.0,
            starve_timer: 0.0,
            oxygen: 300.0,
            drowning_timer: 0.0,
            experience: 0,
            experience_level: 0,
            spawn_point: None,
            spawn_dimension: None,
            is_sleeping: false,
            sleep_timer: 0.0,
            bed_pos: None,
            unlocked_recipes,
            eating_state: None,
            using_item: None,
            attack_cooldown_ticks: 5,
            attack_cooldown_max_ticks: 5,
            shield_disable_ticks: 0,
            bad_omen_level: 0,
            hero_of_the_village_timer: 0.0,
        }
    }

    pub fn reset_for_respawn(&mut self) {
        self.health = self.max_health;
        self.hunger = 20.0;
        self.saturation = 5.0;
        self.exhaustion = 0.0;
        self.is_dead = false;
        self.death_reason = None;
        self.invulnerable_time = 1.0;
        self.damaged_flash_time = 0.0;
        self.regen_timer = 0.0;
        self.starve_timer = 0.0;
        self.oxygen = 300.0;
        self.drowning_timer = 0.0;
        self.experience = 0;
        self.experience_level = 0;
        self.is_sleeping = false;
        self.sleep_timer = 0.0;
        self.bed_pos = None;
    }

    pub fn death_experience_drop(&self) -> u32 {
        (self.experience_level * 7).min(100)
    }

    pub fn take_damage(&mut self, amount: f32, source: DamageSource) -> bool {
        if self.is_dead || self.invulnerable_time > 0.0 {
            return false;
        }

        self.health = (self.health - amount).max(0.0);
        self.invulnerable_time = 0.5; // 0.5 seconds of invulnerability
        self.damaged_flash_time = 0.5; // Flash screen red for 0.5s

        if self.health <= 0.0 {
            self.is_dead = true;
            self.death_reason = Some(source);
            true
        } else {
            false
        }
    }

    pub fn add_exhaustion(&mut self, amount: f32) {
        if self.is_dead {
            return;
        }
        self.exhaustion += amount;
        while self.exhaustion >= 4.0 {
            self.exhaustion -= 4.0;
            if self.saturation > 0.0 {
                self.saturation = (self.saturation - 1.0).max(0.0);
            } else {
                self.hunger = (self.hunger - 1.0).max(0.0);
            }
        }
    }

    pub fn add_experience(&mut self, amount: u32) {
        self.experience = self.experience.saturating_add(amount);
        while self.experience >= self.experience_to_next_level() {
            self.experience -= self.experience_to_next_level();
            self.experience_level += 1;
        }
    }

    pub fn experience_to_next_level(&self) -> u32 {
        7 + self.experience_level * 2
    }

    pub fn spend_levels(&mut self, levels: u32) -> bool {
        if self.experience_level < levels {
            return false;
        }
        self.experience_level -= levels;
        true
    }

    pub fn update(&mut self, dt: f32, is_underwater: bool) -> Option<(f32, DamageSource)> {
        self.update_with_oxygen_rate(dt, is_underwater, 1.0)
    }

    pub fn update_with_oxygen_rate(
        &mut self,
        dt: f32,
        is_underwater: bool,
        oxygen_rate: f32,
    ) -> Option<(f32, DamageSource)> {
        if self.is_dead {
            return None;
        }

        // Tick down invulnerability and damage flash timers
        self.invulnerable_time = (self.invulnerable_time - dt).max(0.0);
        self.damaged_flash_time = (self.damaged_flash_time - dt).max(0.0);

        // Natural health regeneration
        if self.health < self.max_health && self.hunger >= 18.0 {
            self.regen_timer += dt;
            let regen_interval = if self.hunger >= 20.0 && self.saturation > 0.0 {
                0.5 // fast regeneration
            } else {
                4.0 // slow regeneration
            };
            if self.regen_timer >= regen_interval {
                self.regen_timer = 0.0;
                self.health = (self.health + 1.0).min(self.max_health);
                self.add_exhaustion(6.0);
            }
        } else {
            self.regen_timer = 0.0;
        }

        // Hunger starvation damage
        let mut starve_damage = None;
        if self.hunger <= 0.0 {
            self.starve_timer += dt;
            if self.starve_timer >= 4.0 {
                self.starve_timer = 0.0;
                // Normal difficulty behavior: starve down to 1.0 HP (0.5 heart)
                if self.health > 1.0 {
                    starve_damage = Some((1.0, DamageSource::Hunger));
                }
            }
        } else {
            self.starve_timer = 0.0;
        }

        // Oxygen & Drowning logic
        let mut drown_damage = None;
        if is_underwater {
            let prev_oxygen = self.oxygen;
            self.oxygen = (self.oxygen - dt * 20.0 * oxygen_rate).max(0.0);
            if self.oxygen == 0.0 {
                if prev_oxygen == 0.0 {
                    self.drowning_timer += dt;
                }
                if self.drowning_timer >= 1.0 {
                    self.drowning_timer = 0.0;
                    drown_damage = Some((2.0, DamageSource::Drowning));
                }
            }
        } else {
            self.oxygen = (self.oxygen + dt * 100.0).min(300.0);
            self.drowning_timer = 0.0;
        }

        drown_damage.or(starve_damage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player_take_damage() {
        let mut state = PlayerState::new();
        assert_eq!(state.health, 20.0);
        assert!(!state.is_dead);

        // Take damage
        let died = state.take_damage(5.0, DamageSource::Fall);
        assert!(!died);
        assert_eq!(state.health, 15.0);
        assert_eq!(state.invulnerable_time, 0.5);
        assert_eq!(state.damaged_flash_time, 0.5);

        // Damage during invulnerability frame should be ignored
        let died = state.take_damage(2.0, DamageSource::Void);
        assert!(!died);
        assert_eq!(state.health, 15.0);

        // Reset invulnerability
        state.invulnerable_time = 0.0;
        let died = state.take_damage(15.0, DamageSource::Void);
        assert!(died);
        assert_eq!(state.health, 0.0);
        assert_eq!(state.death_reason, Some(DamageSource::Void));
    }

    #[test]
    fn test_player_exhaustion_hunger() {
        let mut state = PlayerState::new();
        assert_eq!(state.hunger, 20.0);
        assert_eq!(state.saturation, 5.0);

        // Add exhaustion (4.0 exhaust = -1 saturation)
        state.add_exhaustion(4.0);
        assert_eq!(state.saturation, 4.0);
        assert_eq!(state.hunger, 20.0);

        // Saturation goes to 0
        state.add_exhaustion(16.0);
        assert_eq!(state.saturation, 0.0);
        assert_eq!(state.hunger, 20.0);

        // Hunger starts depleting once saturation is 0
        state.add_exhaustion(4.0);
        assert_eq!(state.saturation, 0.0);
        assert_eq!(state.hunger, 19.0);
    }

    #[test]
    fn test_player_natural_regen() {
        let mut state = PlayerState::new();
        state.health = 10.0;
        state.hunger = 20.0;
        state.saturation = 5.0;

        // At 20 hunger and >0 saturation, fast regen is 0.5 seconds
        let starve = state.update(0.5, false);
        assert!(starve.is_none());
        assert_eq!(state.health, 11.0);
        // Regen consumes 6.0 exhaustion
        assert_eq!(state.saturation, 4.0); // 6 exhaustion = 1 saturation consumed, 2 left over
        assert_eq!(state.exhaustion, 2.0);
    }

    #[test]
    fn test_player_starvation() {
        let mut state = PlayerState::new();
        state.hunger = 0.0;
        state.saturation = 0.0;
        state.health = 10.0;

        // Starve timer ticks up
        let starve = state.update(3.9, false);
        assert!(starve.is_none());

        // At 4.0 seconds, hunger starvation damage triggers
        let starve = state.update(0.1, false);
        assert_eq!(starve, Some((1.0, DamageSource::Hunger)));
    }

    #[test]
    fn test_player_drowning() {
        let mut state = PlayerState::new();
        assert_eq!(state.oxygen, 300.0);
        // Deplete oxygen underwater: 300.0 / 20.0 = 15.0 seconds
        for _ in 0..15 {
            let dmg = state.update(1.0, true);
            assert!(dmg.is_none());
        }
        assert_eq!(state.oxygen, 0.0);
        // Next second underwater should trigger drowning damage
        let damage = state.update(1.0, true);
        assert_eq!(damage, Some((2.0, DamageSource::Drowning)));
    }

    #[test]
    fn reset_for_respawn_restores_survival_resources_and_timers() {
        let mut state = PlayerState::new();
        state.health = 0.0;
        state.hunger = 0.0;
        state.saturation = 0.0;
        state.exhaustion = 3.5;
        state.is_dead = true;
        state.death_reason = Some(DamageSource::Drowning);
        state.invulnerable_time = 0.0;
        state.damaged_flash_time = 0.4;
        state.regen_timer = 3.0;
        state.starve_timer = 3.9;
        state.oxygen = 0.0;
        state.drowning_timer = 0.9;
        state.experience = 17;
        state.experience_level = 4;

        state.reset_for_respawn();

        assert_eq!(state.health, state.max_health);
        assert_eq!(state.hunger, 20.0);
        assert_eq!(state.saturation, 5.0);
        assert_eq!(state.exhaustion, 0.0);
        assert!(!state.is_dead);
        assert_eq!(state.death_reason, None);
        assert_eq!(state.invulnerable_time, 1.0);
        assert_eq!(state.damaged_flash_time, 0.0);
        assert_eq!(state.regen_timer, 0.0);
        assert_eq!(state.starve_timer, 0.0);
        assert_eq!(state.oxygen, 300.0);
        assert_eq!(state.drowning_timer, 0.0);
        assert_eq!(state.experience, 0);
        assert_eq!(state.experience_level, 0);
    }

    #[test]
    fn test_death_experience_drop_caps_at_100() {
        let mut state = PlayerState::new();
        state.experience_level = 5;
        assert_eq!(state.death_experience_drop(), 35);

        state.experience_level = 20;
        assert_eq!(state.death_experience_drop(), 100);
    }

    #[test]
    fn test_damage_reduction_calculation() {
        // Full diamond armor (20 armor, 8 toughness) vs 10 mob damage
        let dmg = calculate_damage_reduction(10.0, DamageSource::Mob, 20.0, 8.0, 0);
        assert!((dmg - 3.0).abs() < 1e-4);

        // Armor bypass (Void/Hunger)
        let void_dmg = calculate_damage_reduction(10.0, DamageSource::Void, 20.0, 8.0, 10);
        assert_eq!(void_dmg, 10.0);

        // Fall damage bypasses armor but affected by EPF (Feather Falling)
        let fall_dmg = calculate_damage_reduction(10.0, DamageSource::Fall, 20.0, 8.0, 10);
        assert!((fall_dmg - 6.0).abs() < 1e-4);
    }

    #[test]
    fn test_shield_blocking_direction() {
        // Player facing +X (yaw = 0.0)
        let pos = [0.0, 64.0, 0.0];
        // Source in front at +X
        assert!(can_shield_block(
            0.0,
            pos,
            Some([5.0, 64.0, 0.0]),
            DamageSource::Mob
        ));
        // Source behind at -X
        assert!(!can_shield_block(
            0.0,
            pos,
            Some([-5.0, 64.0, 0.0]),
            DamageSource::Mob
        ));

        // Void damage cannot be blocked
        assert!(!can_shield_block(
            0.0,
            pos,
            Some([5.0, 64.0, 0.0]),
            DamageSource::Void
        ));
    }

    #[test]
    fn test_attack_cooldown_scaling() {
        let (dmg_0, kb_0, full_0) = calculate_attack_damage(10.0, 0, 10);
        assert_eq!(dmg_0, 2.0); // 20%
        assert_eq!(kb_0, 0.0);
        assert!(!full_0);

        let (dmg_50, kb_50, full_50) = calculate_attack_damage(10.0, 5, 10);
        assert!((dmg_50 - 4.0).abs() < 1e-4); // 0.2 + 0.8*0.25 = 0.4 -> 4.0
        assert_eq!(kb_50, 0.5);
        assert!(!full_50);

        let (dmg_100, kb_100, full_100) = calculate_attack_damage(10.0, 10, 10);
        assert_eq!(dmg_100, 10.0);
        assert_eq!(kb_100, 1.0);
        assert!(full_100);
    }

    #[test]
    fn test_bow_shot_charging() {
        assert!(calculate_bow_shot(2).is_none());

        let (spd_half, dmg_half, crit_half) = calculate_bow_shot(10).unwrap();
        assert!((spd_half - 1.5).abs() < 1e-4);
        assert!((dmg_half - 3.0).abs() < 1e-4);
        assert!(!crit_half);

        let (spd_full, dmg_full, crit_full) = calculate_bow_shot(20).unwrap();
        assert_eq!(spd_full, 3.0);
        assert_eq!(dmg_full, 8.0);
        assert!(crit_full);
    }
}
