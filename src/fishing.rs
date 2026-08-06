use crate::inventory::ItemStack;
use glam::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FishingHookStage {
    Flying,
    FloatingInWater,
    Nibbling,
    Reeled,
}

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

impl FishingHook {
    pub fn new(entity_id: u64, owner_player_id: u64, spawn_pos: Vec3, launch_dir: Vec3) -> Self {
        let initial_vel = launch_dir.normalize_or_zero() * 14.0 + Vec3::new(0.0, 3.0, 0.0);
        Self {
            entity_id,
            owner_player_id,
            position: [spawn_pos.x, spawn_pos.y, spawn_pos.z],
            velocity: [initial_vel.x, initial_vel.y, initial_vel.z],
            stage: FishingHookStage::Flying,
            wait_ticks_remaining: 100, // ~5 seconds baseline wait
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

#[derive(Debug, Clone, PartialEq)]
pub enum FishingResult {
    Caught(ItemStack),
    Junk(ItemStack),
    Treasure(ItemStack),
    Missed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FishingManager {
    pub active_hooks: std::collections::HashMap<u64, FishingHook>,
    next_hook_entity_id: u64,
}

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
                        hook.bite_ticks_remaining = 40; // 2 seconds bite window
                    }
                }
                FishingHookStage::Nibbling => {
                    splash_particle_cb(pos);
                    if hook.bite_ticks_remaining > 0 {
                        hook.bite_ticks_remaining -= 1;
                    } else {
                        hook.stage = FishingHookStage::FloatingInWater;
                        hook.wait_ticks_remaining = 200;
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
