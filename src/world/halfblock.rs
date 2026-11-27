//! Packs a pixel buffer into terminal cells: two pixels per cell with `▀`
//! (top = foreground, bottom = background), or one per cell in low-res mode.

use super::{Rgb, pixels::Pixels};

/// One terminal cell's two pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HalfCell {
    pub top: Rgb,
    pub bottom: Rgb,
}

/// Rows of cells, `ceil(height / 2)` of them. An odd last row gets the
/// buffer's background as its bottom pixel.
pub fn pack(px: &Pixels) -> Vec<Vec<HalfCell>> {
    rows(px)
        .map(|(top, bottom)| {
            (0..px.width)
                .map(|x| HalfCell {
                    top: px.data[top + x],
                    bottom: bottom.map_or(px.background, |row| px.data[row + x]),
                })
                .collect()
        })
        .collect()
}

/// One color per cell: the average of each vertical pixel pair.
pub fn pack_lowres(px: &Pixels) -> Vec<Vec<Rgb>> {
    rows(px)
        .map(|(top, bottom)| {
            (0..px.width)
                .map(|x| {
                    let a = px.data[top + x];
                    match bottom {
                        Some(row) => average(a, px.data[row + x]),
                        None => a,
                    }
                })
                .collect()
        })
        .collect()
}

/// Start offsets of each (top row, bottom row) pair; `None` for the bottom
/// of an odd last row.
fn rows(px: &Pixels) -> impl Iterator<Item = (usize, Option<usize>)> + '_ {
    (0..px.height).step_by(2).map(move |y| {
        let bottom = (y + 1 < px.height).then(|| (y + 1) * px.width);
        (y * px.width, bottom)
    })
}

fn average(a: Rgb, b: Rgb) -> Rgb {
    let mid = |i: usize| ((u16::from(a[i]) + u16::from(b[i])) / 2) as u8;
    [mid(0), mid(1), mid(2)]
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Rgb = [10, 20, 30];
    const B: Rgb = [30, 40, 50];
    const BG: Rgb = [1, 2, 3];

    #[test]
    fn pack_pairs_rows_into_cells() {
        let mut px = Pixels::new(2, 4, BG);
        px.set(0, 0, A);
        px.set(0, 1, B);
        px.set(1, 3, A);
        let cells = pack(&px);
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].len(), 2);
        assert_eq!(cells[0][0], HalfCell { top: A, bottom: B });
        assert_eq!(cells[1][1], HalfCell { top: BG, bottom: A });
    }

    #[test]
    fn pack_odd_height_has_no_stripe() {
        let mut px = Pixels::new(3, 5, BG);
        px.fill_rect(0, 0, 3, 5, A);
        let cells = pack(&px);
        assert_eq!(cells.len(), 3);
        for cell in &cells[2] {
            assert_eq!(*cell, HalfCell { top: A, bottom: BG });
        }
        for row in &cells[..2] {
            assert!(row.iter().all(|c| *c == HalfCell { top: A, bottom: A }));
        }
    }

    #[test]
    fn lowres_averages_pairs() {
        let mut px = Pixels::new(1, 3, BG);
        px.set(0, 0, A);
        px.set(0, 1, B);
        px.set(0, 2, B);
        let cells = pack_lowres(&px);
        assert_eq!(cells, vec![vec![[20, 30, 40]], vec![B]]);
    }

    #[test]
    fn empty_buffer_packs_to_nothing() {
        assert!(pack(&Pixels::new(0, 0, BG)).is_empty());
        assert!(pack_lowres(&Pixels::new(3, 0, BG)).is_empty());
    }
}
