//! Chunk-column coordinate helpers for world-space X/Z.
//!
//! Signed-Y section helpers live in [`super::section`]; Chebyshev unload rings
//! stay on interest policy. This module only covers block↔chunk XZ.

/// Convert a world-space block X or Z into a chunk-column coordinate.
#[inline]
pub const fn chunk_coord(block: i32) -> i32 {
    block.div_euclid(16)
}

/// Chunk column `(cx, cz)` for a world-space block position.
#[inline]
pub const fn chunk_xz(x: i32, z: i32) -> (i32, i32) {
    (chunk_coord(x), chunk_coord(z))
}

/// Local 0..15 coordinate inside a chunk column for one axis.
#[inline]
pub const fn local_coord(block: i32) -> i32 {
    block.rem_euclid(16)
}

/// Local `(lx, lz)` inside a chunk column for a world-space block position.
#[inline]
pub const fn local_xz(x: i32, z: i32) -> (i32, i32) {
    (local_coord(x), local_coord(z))
}

/// World-space origin (min corner) of chunk column `cx` on one axis.
#[inline]
pub const fn chunk_origin(cx: i32) -> i32 {
    cx * 16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_helpers_match_div_euclid_for_negatives() {
        assert_eq!(
            chunk_xz(-1, -17),
            ((-1_i32).div_euclid(16), (-17_i32).div_euclid(16))
        );
        assert_eq!(
            local_xz(-1, -17),
            ((-1_i32).rem_euclid(16), (-17_i32).rem_euclid(16))
        );
        assert_eq!(chunk_origin(-2), -32);
        assert_eq!(chunk_origin(3) + local_coord(50), 50);
    }
}
