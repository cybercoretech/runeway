//! Prints every Runeway sprite at 4× with half-block pixels, for a visual
//! check: `cargo run --example sprite_sheet`. Needs a true-color terminal.

use runeway::world::{
    Rgb,
    halfblock::pack,
    palette::{SKY, color},
    pixels::Pixels,
    sprites::{
        Backdrop, Character, Item, Pose, Sprite, Tile, backdrop, critter, frames, item, tile,
    },
};

const SCALE: usize = 4;
const GAP: usize = 4;

fn main() {
    for character in Character::ALL {
        let row: Vec<&Sprite> = Pose::ALL
            .iter()
            .flat_map(|pose| frames(character, *pose))
            .collect();
        print_row(
            &format!("{character:?} (run frames, jump, stumble, sit, stand, cheer)"),
            &row,
        );
    }
    let tiles: Vec<&Sprite> = [
        Tile::Ground,
        Tile::RuneBrick,
        Tile::SigilBlock,
        Tile::Gate,
        Tile::Beacon,
    ]
    .into_iter()
    .map(tile)
    .collect();
    print_row(
        "Tiles: ground, rune brick, sigil block, rune gate, beacon tower",
        &tiles,
    );
    let mut items: Vec<&Sprite> = [Item::GlyphCoin, Item::DataGem, Item::Spark]
        .into_iter()
        .map(item)
        .collect();
    items.extend(critter());
    print_row("Items: glyph coin, data gem, spark; cyber-critter", &items);
    for b in [
        Backdrop::Skyline,
        Backdrop::ServerRacks,
        Backdrop::RunePillars,
    ] {
        print_row(&format!("Backdrop: {b:?}"), &[backdrop(b)]);
    }
}

fn print_row(title: &str, sprites: &[&Sprite]) {
    println!("{title}");
    let width: usize = sprites.iter().map(|s| s.width * SCALE + GAP).sum();
    let height = sprites.iter().map(|s| s.height * SCALE).max().unwrap_or(0);
    let mut px = Pixels::new(width, height, color(SKY));
    let mut x = 0;
    for sprite in sprites {
        for (i, &index) in sprite.pixels.iter().enumerate() {
            if index == 0 {
                continue;
            }
            let (sx, sy) = (i % sprite.width, i / sprite.width);
            px.fill_rect(
                (x + sx * SCALE) as i32,
                (sy * SCALE) as i32,
                SCALE as i32,
                SCALE as i32,
                color(index),
            );
        }
        x += sprite.width * SCALE + GAP;
    }
    for row in pack(&px) {
        let mut line = String::new();
        for cell in row {
            line.push_str(&format!("{}{}▀", fg(cell.top), bg(cell.bottom)));
        }
        println!("{line}\x1b[0m");
    }
    println!();
}

fn fg(c: Rgb) -> String {
    format!("\x1b[38;2;{};{};{}m", c[0], c[1], c[2])
}

fn bg(c: Rgb) -> String {
    format!("\x1b[48;2;{};{};{}m", c[0], c[1], c[2])
}
