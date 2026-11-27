//! Writes Runeway's real sprites and palette as JSON for the website, so
//! the page draws the same art the terminal does:
//! `cargo run --example export_sprites [OUT]` (default `site/assets/sprites.json`).
//!
//! Format: `palette` is 16 hex colours; each sprite has `w`, `h` and `px`,
//! one hex digit per pixel (its palette index; 0 is transparent), row by row.

use std::{fmt::Write as _, path::PathBuf};

use runeway::world::{
    palette::PALETTE,
    sprites::{KEY, MAPS, Sprite},
};

fn main() {
    let out = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("site/assets/sprites.json"), PathBuf::from);
    let mut json = String::from("{\n  \"palette\": [");
    let colours: Vec<String> = PALETTE
        .iter()
        .map(|[r, g, b]| format!("\"#{r:02x}{g:02x}{b:02x}\""))
        .collect();
    json.push_str(&colours.join(", "));
    json.push_str("],\n  \"sprites\": {\n");
    let entries: Vec<String> = MAPS
        .iter()
        .map(|(name, rows)| {
            let sprite = Sprite::from_rows(rows, KEY);
            let mut px = String::with_capacity(sprite.pixels.len());
            for index in &sprite.pixels {
                let _ = write!(px, "{index:x}");
            }
            format!(
                "    \"{name}\": {{ \"w\": {}, \"h\": {}, \"px\": \"{px}\" }}",
                sprite.width, sprite.height
            )
        })
        .collect();
    json.push_str(&entries.join(",\n"));
    json.push_str("\n  }\n}\n");
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).expect("output folder");
    }
    std::fs::write(&out, json).expect("write sprites");
    println!("wrote {} sprites to {}", MAPS.len(), out.display());
}
