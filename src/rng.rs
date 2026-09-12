//! Shared hash and LCG helpers.
//!
//! Worldgen / structure mixers stay in their own modules — those constants are
//! world-identity and must not move here.

pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
pub const FNV_PRIME: u64 = 0x100000001b3;

/// Classic FNV-1a over a byte slice.
#[inline]
pub fn fnv1a(data: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET;
    fnv1a_write(&mut hash, data);
    hash
}

/// Fold bytes into an existing FNV-1a state.
#[inline]
pub fn fnv1a_write(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}

/// glibc-style LCG step; returns the updated 32-bit state.
#[inline]
pub fn lcg32_step(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    *state
}

/// Truncated LCG used by ambient mob spawn (`(state / 65536) % 32768`).
#[inline]
pub fn lcg32_short(state: &mut u32) -> u32 {
    lcg32_step(state);
    (*state / 65536) % 32768
}
