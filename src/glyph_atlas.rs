//! Shared 5×7 glyph atlas: rasterize once, draw as textured quads.
//!
//! Desktop-only helpers used by menu and in-game HUD. The atlas is a small
//! Rgba8 image (16 columns × 6 rows of 6×8 cells).

use crate::resources::FontSource;

pub const CELL_W: u32 = 6;
pub const CELL_H: u32 = 8;
pub const COLS: u32 = 16;
pub const ROWS: u32 = 6;
pub const ATLAS_W: u32 = COLS * CELL_W;
pub const ATLAS_H: u32 = ROWS * CELL_H;

const ATLAS_CHARS: &[char] = &[
    ' ', '!', '"', '#', '$', '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/',
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', ':', ';', '<', '=', '>', '?',
    '@', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O',
    'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', '[', '\\', ']', '^', '_',
    '`', '{', '|', '}', '~',
];

fn char_index(ch: char) -> Option<usize> {
    let upper = ch.to_ascii_uppercase();
    ATLAS_CHARS.iter().position(|&c| c == upper || c == ch)
}

pub fn glyph(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 14],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [31, 4, 4, 4, 4, 4, 31],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        ':' => [0, 4, 4, 0, 4, 4, 0],
        '.' => [0, 0, 0, 0, 0, 4, 4],
        ',' => [0, 0, 0, 0, 0, 4, 8],
        '!' => [4, 4, 4, 4, 4, 0, 4],
        '?' => [14, 17, 1, 2, 4, 0, 4],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '+' => [0, 4, 4, 31, 4, 4, 0],
        '=' => [0, 31, 0, 31, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 31],
        '<' => [2, 4, 8, 16, 8, 4, 2],
        '>' => [8, 4, 2, 1, 2, 4, 8],
        '/' => [1, 2, 2, 4, 8, 8, 16],
        '%' => [17, 2, 4, 8, 17, 0, 0],
        '(' => [2, 4, 8, 8, 8, 4, 2],
        ')' => [8, 4, 2, 2, 2, 4, 8],
        '[' => [14, 8, 8, 8, 8, 8, 14],
        ']' => [14, 2, 2, 2, 2, 2, 14],
        _ => [0; 7],
    }
}

pub fn build_rgba(font: &FontSource) -> Vec<u8> {
    let mut pixels = vec![0u8; (ATLAS_W * ATLAS_H * 4) as usize];
    for (index, &ch) in ATLAS_CHARS.iter().enumerate() {
        let col = (index as u32) % COLS;
        let row = (index as u32) / COLS;
        if row >= ROWS {
            break;
        }
        let rows = font.glyph_override(ch).unwrap_or_else(|| glyph(ch));
        let ox = col * CELL_W;
        let oy = row * CELL_H;
        for (gy, bits) in rows.into_iter().enumerate() {
            for gx in 0..5u32 {
                if bits & (1 << (4 - gx)) != 0 {
                    let px = ox + gx;
                    let py = oy + gy as u32;
                    let i = ((py * ATLAS_W + px) * 4) as usize;
                    pixels[i] = 255;
                    pixels[i + 1] = 255;
                    pixels[i + 2] = 255;
                    pixels[i + 3] = 255;
                }
            }
        }
    }
    pixels
}

pub fn uv_for(ch: char) -> [f32; 4] {
    let index = char_index(ch).unwrap_or(0) as u32;
    let col = index % COLS;
    let row = index / COLS;
    let u0 = (col * CELL_W) as f32 / ATLAS_W as f32;
    let v0 = (row * CELL_H) as f32 / ATLAS_H as f32;
    let u1 = (col * CELL_W + 5) as f32 / ATLAS_W as f32;
    let v1 = (row * CELL_H + 7) as f32 / ATLAS_H as f32;
    [u0, v0, u1, v1]
}

pub fn push_glyph_quad<V>(
    vertices: &mut Vec<V>,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    ch: char,
    color: [f32; 4],
    mut make: impl FnMut([f32; 3], [f32; 2], [f32; 4]) -> V,
) {
    let [u0, v0, u1, v1] = uv_for(ch);
    let corners = [
        ([x0, y1, 0.0], [u0, v0]),
        ([x0, y0, 0.0], [u0, v1]),
        ([x1, y0, 0.0], [u1, v1]),
        ([x0, y1, 0.0], [u0, v0]),
        ([x1, y0, 0.0], [u1, v1]),
        ([x1, y1, 0.0], [u1, v0]),
    ];
    for (pos, uv) in corners {
        vertices.push(make(pos, uv, color));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_covers_alnum_and_is_nonempty() {
        let pixels = build_rgba(&FontSource::BuiltIn);
        assert_eq!(pixels.len(), (ATLAS_W * ATLAS_H * 4) as usize);
        assert!(pixels.iter().any(|&b| b != 0));
        let [u0, v0, u1, v1] = uv_for('A');
        assert!(u1 > u0 && v1 > v0);
    }
}
