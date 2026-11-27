//! The platformer world: pixels, palette, sprites and the lane simulation.
//! Pure and deterministic: no clocks, no randomness, no terminal code.

pub mod halfblock;
pub mod mapping;
pub mod palette;
pub mod pixels;
pub mod runner;
pub mod sprites;

/// One pixel's color.
pub type Rgb = [u8; 3];
