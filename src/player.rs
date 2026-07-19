#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageSource {
    Fall,
    Void,
    Hunger,
    Mob,
    Explosion,
    Drowning,
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
}

impl PlayerState {
    pub fn new() -> Self {
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
        }
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

    pub fn update(&mut self, dt: f32, is_underwater: bool) -> Option<(f32, DamageSource)> {
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
            self.oxygen = (self.oxygen - dt * 20.0).max(0.0);
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
}
