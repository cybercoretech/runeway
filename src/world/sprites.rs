//! Sprites: small pixel maps of palette indices.

/// A pixel map; each value is a palette index, 0 is transparent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sprite {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}
