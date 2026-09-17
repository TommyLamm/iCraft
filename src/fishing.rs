use serde::{Deserialize, Serialize};

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

