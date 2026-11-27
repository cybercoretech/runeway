//! Runeway's original 16-color palette, tuned for the Cybercore look:
//! bright accents on a dark night sky.

use super::Rgb;

pub const SKY: u8 = 0;
pub const PANEL: u8 = 1;
pub const SKYLINE_FAR: u8 = 2;
pub const SKYLINE_NEAR: u8 = 3;
pub const GROUND: u8 = 4;
pub const GROUND_SHADOW: u8 = 5;
pub const RUNE_CYAN: u8 = 6;
pub const RUNE_VIOLET: u8 = 7;
pub const EMBER_PINK: u8 = 8;
pub const GLYPH_GOLD: u8 = 9;
pub const GEM_GREEN: u8 = 10;
pub const BRASS: u8 = 11;
pub const STEEL: u8 = 12;
pub const WHITE: u8 = 13;
pub const WARNING_RED: u8 = 14;
pub const SHADOW: u8 = 15;

pub const PALETTE: [Rgb; 16] = [
    [10, 8, 24],     // SKY: night sky
    [22, 18, 44],    // PANEL: deep panel
    [36, 30, 72],    // SKYLINE_FAR
    [58, 46, 104],   // SKYLINE_NEAR
    [44, 38, 62],    // GROUND
    [26, 22, 38],    // GROUND_SHADOW
    [64, 224, 240],  // RUNE_CYAN
    [168, 96, 255],  // RUNE_VIOLET
    [255, 84, 170],  // EMBER_PINK
    [255, 204, 64],  // GLYPH_GOLD
    [72, 232, 128],  // GEM_GREEN
    [196, 140, 60],  // BRASS
    [120, 128, 150], // STEEL
    [236, 240, 255], // WHITE
    [240, 56, 72],   // WARNING_RED
    [6, 4, 12],      // SHADOW
];

/// The color for a palette index (wraps, so any `u8` is safe).
pub fn color(index: u8) -> Rgb {
    PALETTE[usize::from(index) % PALETTE.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_has_16_distinct_colors() {
        for (i, a) in PALETTE.iter().enumerate() {
            for b in &PALETTE[i + 1..] {
                assert_ne!(a, b, "duplicate color {a:?}");
            }
        }
    }
}
