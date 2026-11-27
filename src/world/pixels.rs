//! An RGB pixel buffer the lanes render into.

use super::{Rgb, palette, sprites::Sprite};

/// A width × height RGB buffer, row-major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixels {
    pub width: usize,
    pub height: usize,
    pub data: Vec<Rgb>,
    /// The fill color, used where a packed cell has no pixel of its own.
    pub background: Rgb,
}

impl Pixels {
    pub fn new(width: usize, height: usize, fill: Rgb) -> Pixels {
        Pixels {
            width,
            height,
            data: vec![fill; width * height],
            background: fill,
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        let x = usize::try_from(x).ok().filter(|x| *x < self.width)?;
        let y = usize::try_from(y).ok().filter(|y| *y < self.height)?;
        Some(y * self.width + x)
    }

    /// Set one pixel; anything outside the buffer is ignored.
    pub fn set(&mut self, x: i32, y: i32, c: Rgb) {
        if let Some(i) = self.index(x, y) {
            self.data[i] = c;
        }
    }

    /// The pixel at (x, y); the background outside the buffer.
    pub fn get(&self, x: i32, y: i32) -> Rgb {
        self.index(x, y).map_or(self.background, |i| self.data[i])
    }

    /// Fill a rectangle, clipped to the buffer.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb) {
        let clamp = |v: i32, max: usize| v.clamp(0, i32::try_from(max).unwrap_or(i32::MAX));
        let (x0, x1) = (clamp(x, self.width), clamp(x.saturating_add(w), self.width));
        let (y0, y1) = (
            clamp(y, self.height),
            clamp(y.saturating_add(h), self.height),
        );
        for py in y0..y1 {
            for px in x0..x1 {
                self.set(px, py, c);
            }
        }
    }

    /// Draw `sprite` with its top-left at (x, y), mirrored when `flip`.
    /// Palette index 0 is transparent.
    pub fn blit(&mut self, sprite: &Sprite, x: i32, y: i32, flip: bool) {
        for (row, line) in sprite.pixels.chunks(sprite.width.max(1)).enumerate() {
            for (col, &index) in line.iter().enumerate() {
                if index == 0 {
                    continue;
                }
                let col = if flip { sprite.width - 1 - col } else { col };
                let (Ok(dx), Ok(dy)) = (i32::try_from(col), i32::try_from(row)) else {
                    continue;
                };
                self.set(
                    x.saturating_add(dx),
                    y.saturating_add(dy),
                    palette::color(index),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::palette::{GLYPH_GOLD, RUNE_CYAN, SKY, color};

    const RED: Rgb = [255, 0, 0];
    const BLACK: Rgb = [0, 0, 0];

    #[test]
    fn set_ignores_out_of_bounds() {
        let mut px = Pixels::new(4, 3, BLACK);
        px.set(-1, 0, RED);
        px.set(4, 0, RED);
        px.set(0, 3, RED);
        px.set(0, -1, RED);
        assert!(px.data.iter().all(|c| *c == BLACK));
        px.set(3, 2, RED);
        assert_eq!(px.get(3, 2), RED);
        assert_eq!(px.get(99, 99), BLACK);
    }

    #[test]
    fn fill_rect_is_clipped() {
        let mut px = Pixels::new(4, 4, BLACK);
        px.fill_rect(2, 2, 10, 10, RED);
        assert_eq!(px.get(1, 1), BLACK);
        assert_eq!(px.get(3, 3), RED);
        px.fill_rect(-5, -5, 6, 6, RED);
        assert_eq!(px.get(0, 0), RED);
        assert_eq!(px.get(1, 0), BLACK);
    }

    #[test]
    fn blit_skips_transparent_and_flips() {
        // [cyan, transparent, gold]
        let sprite = Sprite {
            width: 3,
            height: 1,
            pixels: vec![RUNE_CYAN, 0, GLYPH_GOLD],
        };
        let mut px = Pixels::new(5, 1, color(SKY));
        px.blit(&sprite, 1, 0, false);
        assert_eq!(px.get(1, 0), color(RUNE_CYAN));
        assert_eq!(px.get(2, 0), color(SKY));
        assert_eq!(px.get(3, 0), color(GLYPH_GOLD));
        let mut flipped = Pixels::new(5, 1, color(SKY));
        flipped.blit(&sprite, 1, 0, true);
        assert_eq!(flipped.get(1, 0), color(GLYPH_GOLD));
        assert_eq!(flipped.get(3, 0), color(RUNE_CYAN));
        // Partly off-screen: no panic, visible part drawn.
        let mut edge = Pixels::new(2, 1, color(SKY));
        edge.blit(&sprite, -2, 0, false);
        assert_eq!(edge.get(0, 0), color(GLYPH_GOLD));
    }
}
