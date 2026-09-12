#[cfg(test)]
use glam::Vec3;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use crate::inventory::ItemStack;

/// Fishing simulation constants shared by tests and the authoritative fixed-tick
/// domain. Durability is remaining durability, which matches the existing
/// `ItemStack` convention used by tool damage.
pub const FISHING_FIXED_TICK_HZ: i32 = 20;
pub const FISHING_INITIAL_WAIT_TICKS: u32 = 100;
pub const FISHING_BITE_WINDOW_TICKS: u32 = 40;
pub const FISHING_REPEAT_WAIT_TICKS: u32 = 200;
pub const FISHING_ROD_MAX_DURABILITY: u16 = 64;
pub const FISHING_MAX_DISTANCE_MILLI: i32 = 32_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FishingHookStage {
    Flying,
    FloatingInWater,
    Nibbling,
    Reeled,
}

impl FishingHookStage {
    pub const fn to_wire(self) -> u8 {
        match self {
            Self::Flying => 0,
            Self::FloatingInWater => 1,
            Self::Nibbling => 2,
            Self::Reeled => 3,
        }
    }

    pub const fn from_wire(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Flying),
            1 => Some(Self::FloatingInWater),
            2 => Some(Self::Nibbling),
            3 => Some(Self::Reeled),
            _ => None,
        }
    }
}

/// Convert a bounded protocol look vector into the launch velocity used by
/// authority cast, but entirely with integer arithmetic. The fixed 32-step
/// square-root keeps runtime work bounded for attacker-controlled inputs.
pub fn authoritative_launch_velocity_milli(look_milli: [i16; 3]) -> Option<[i32; 3]> {
    let squared = look_milli.into_iter().try_fold(0u64, |total, component| {
        let component = i64::from(component);
        total.checked_add((component * component) as u64)
    })?;
    if !(250_000..=1_210_000).contains(&squared) {
        return None;
    }
    let length = integer_sqrt(squared).max(1) as i64;
    let mut velocity = [0i32; 3];
    for (index, component) in look_milli.into_iter().enumerate() {
        velocity[index] = i32::try_from(i64::from(component) * 14_000 / length).ok()?;
    }
    velocity[1] = velocity[1].checked_add(3_000)?;
    Some(velocity)
}

/// A stable random lane for authority decisions. Inputs are immutable for one
/// hook, so a transport retry produces exactly the same loot and durability
/// decisions without consulting wall-clock time or mutable RNG state.
pub fn deterministic_fishing_roll(
    world_seed: u64,
    player_id: u64,
    hook_entity_id: u64,
    lane: u64,
) -> u32 {
    // Keep the lane mixer (bite distribution depends on it); only the
    // SplitMix64 finalizer is shared with world_tick.
    let seed = world_seed
        ^ player_id.rotate_left(17)
        ^ hook_entity_id.rotate_left(37)
        ^ lane.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    crate::world_tick::deterministic_rng(seed, 0) as u32
}

fn integer_sqrt(value: u64) -> u32 {
    let mut remainder = value;
    let mut root = 0u64;
    let mut bit = 1u64 << 62;
    // Exactly 32 base-four digits cover every u64 value.
    for _ in 0..32 {
        let trial = root.saturating_add(bit);
        if remainder >= trial {
            remainder -= trial;
            root = (root >> 1).saturating_add(bit);
        } else {
            root >>= 1;
        }
        bit >>= 2;
    }
    root.min(u64::from(u32::MAX)) as u32
}

#[cfg(test)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FishingHook {
    pub entity_id: u64,
    pub owner_player_id: u64,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub stage: FishingHookStage,
    pub wait_ticks_remaining: u32,
    pub bite_ticks_remaining: u32,
}

#[cfg(test)]
impl FishingHook {
    pub fn new(entity_id: u64, owner_player_id: u64, spawn_pos: Vec3, launch_dir: Vec3) -> Self {
        let initial_vel = launch_dir.normalize_or_zero() * 14.0 + Vec3::new(0.0, 3.0, 0.0);
        Self {
            entity_id,
            owner_player_id,
            position: [spawn_pos.x, spawn_pos.y, spawn_pos.z],
            velocity: [initial_vel.x, initial_vel.y, initial_vel.z],
            stage: FishingHookStage::Flying,
            wait_ticks_remaining: FISHING_INITIAL_WAIT_TICKS,
            bite_ticks_remaining: 0,
        }
    }

    pub fn pos_vec3(&self) -> Vec3 {
        Vec3::from_array(self.position)
    }

    pub fn vel_vec3(&self) -> Vec3 {
        Vec3::from_array(self.velocity)
    }

    pub fn set_pos(&mut self, pos: Vec3) {
        self.position = [pos.x, pos.y, pos.z];
    }

    pub fn set_vel(&mut self, vel: Vec3) {
        self.velocity = [vel.x, vel.y, vel.z];
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq)]
pub enum FishingResult {
    Caught(ItemStack),
    Junk(ItemStack),
    Treasure(ItemStack),
    Missed,
}

/// Presentation fishing manager. Dead outside tests: authority owns hooks in
/// `SessionGameplayState` / `authority::fishing`; State only mirrors entity id.
#[cfg(test)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FishingManager {
    pub active_hooks: std::collections::HashMap<u64, FishingHook>,
    next_hook_entity_id: u64,
}

#[cfg(test)]
impl FishingManager {
    pub fn new() -> Self {
        Self {
            active_hooks: std::collections::HashMap::new(),
            next_hook_entity_id: 100_000,
        }
    }

    pub fn cast_hook(&mut self, player_id: u64, player_pos: Vec3, look_dir: Vec3) -> u64 {
        let hook_id = self.next_hook_entity_id;
        self.next_hook_entity_id += 1;
        let eye_pos = player_pos + Vec3::new(0.0, 1.62, 0.0);
        let hook = FishingHook::new(hook_id, player_id, eye_pos, look_dir);
        self.active_hooks.insert(player_id, hook);
        hook_id
    }

    pub fn get_hook(&self, player_id: u64) -> Option<&FishingHook> {
        self.active_hooks.get(&player_id)
    }

    pub fn reel_in<R>(&mut self, player_id: u64, mut rng_roll: R) -> Option<FishingResult>
    where
        R: FnMut() -> u32,
    {
        let hook = self.active_hooks.remove(&player_id)?;
        if hook.stage == FishingHookStage::Nibbling {
            let roll = rng_roll() % 100;
            if roll < 85 {
                // 85% Fish loot
                let fish_roll = rng_roll() % 4;
                let fish_item = match fish_roll {
                    0 => crate::inventory::Item::RawCod,
                    1 => crate::inventory::Item::RawSalmon,
                    2 => crate::inventory::Item::TropicalFish,
                    _ => crate::inventory::Item::Pufferfish,
                };
                Some(FishingResult::Caught(ItemStack::new(fish_item, 1)))
            } else if roll < 95 {
                // 10% Junk loot
                Some(FishingResult::Junk(ItemStack::new(
                    crate::inventory::Item::LilyPad,
                    1,
                )))
            } else {
                // 5% Treasure loot
                Some(FishingResult::Treasure(ItemStack::new(
                    crate::inventory::Item::Bow,
                    1,
                )))
            }
        } else {
            Some(FishingResult::Missed)
        }
    }

    pub fn tick<F, P>(
        &mut self,
        dt: f32,
        player_positions: &std::collections::HashMap<u64, Vec3>,
        is_water_at: F,
        mut splash_particle_cb: P,
    ) where
        F: Fn(i32, i32, i32) -> bool,
        P: FnMut(Vec3),
    {
        let mut to_remove = Vec::new();

        for (&player_id, hook) in self.active_hooks.iter_mut() {
            let mut pos = hook.pos_vec3();
            let mut vel = hook.vel_vec3();

            if let Some(&p_pos) = player_positions.get(&player_id) {
                if pos.distance(p_pos) > 32.0 {
                    to_remove.push(player_id);
                    continue;
                }
            }

            match hook.stage {
                FishingHookStage::Flying => {
                    vel.y -= 12.0 * dt;
                    pos += vel * dt;

                    let bx = pos.x.floor() as i32;
                    let by = pos.y.floor() as i32;
                    let bz = pos.z.floor() as i32;

                    if is_water_at(bx, by, bz) {
                        hook.stage = FishingHookStage::FloatingInWater;
                        vel = Vec3::ZERO;
                        pos.y = by as f32 + 0.8;
                    }
                }
                FishingHookStage::FloatingInWater => {
                    if hook.wait_ticks_remaining > 0 {
                        hook.wait_ticks_remaining -= 1;
                    } else {
                        hook.stage = FishingHookStage::Nibbling;
                        hook.bite_ticks_remaining = FISHING_BITE_WINDOW_TICKS;
                    }
                }
                FishingHookStage::Nibbling => {
                    splash_particle_cb(pos);
                    if hook.bite_ticks_remaining > 0 {
                        hook.bite_ticks_remaining -= 1;
                    } else {
                        hook.stage = FishingHookStage::FloatingInWater;
                        hook.wait_ticks_remaining = FISHING_REPEAT_WAIT_TICKS;
                    }
                }
                FishingHookStage::Reeled => {
                    to_remove.push(player_id);
                }
            }

            hook.set_pos(pos);
            hook.set_vel(vel);
        }

        for id in to_remove {
            self.active_hooks.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fishing_cast_and_reel() {
        let mut fm = FishingManager::new();
        let p_id = 1u64;
        let pos = Vec3::new(0.0, 64.0, 0.0);
        let look = Vec3::new(0.0, 0.0, 1.0);

        let hook_id = fm.cast_hook(p_id, pos, look);
        assert!(fm.get_hook(p_id).is_some());
        assert_eq!(fm.get_hook(p_id).unwrap().entity_id, hook_id);

        let res = fm.reel_in(p_id, || 10);
        assert_eq!(res, Some(FishingResult::Missed));
        assert!(fm.get_hook(p_id).is_none());
    }
}
