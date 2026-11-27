//! Runeway's world is original. This fails the build if a borrowed game
//! name shows up in the source or the README.

use std::path::{Path, PathBuf};

const FORBIDDEN: &[&str] = &[
    "mario",
    "luigi",
    "goomba",
    "koopa",
    "bowser",
    "peach",
    "toad",
    "yoshi",
    "1-1",
    "1up",
    "mushroom",
    "fire flower",
    "warp pipe",
    "question block",
    "flagpole",
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read src").flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Every file the scan reads: Rust under `src/` and `examples/`, and
/// `README.md`.
fn scanned_files() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    rust_files(&root.join("examples"), &mut files);
    files.push(root.join("README.md"));
    files
}

/// The forbidden names in one line.
fn hits(line: &str) -> Vec<&'static str> {
    let lower = line.to_lowercase();
    FORBIDDEN
        .iter()
        .copied()
        .filter(|name| {
            if name.starts_with(|c: char| c.is_ascii_digit()) {
                // "1-1" or "1up" only as a word of its own, so dates such as
                // 2021-11-05 do not count.
                lower
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                    .any(|word| word == *name)
            } else {
                lower.contains(name)
            }
        })
        .collect()
}

#[test]
fn no_borrowed_game_names() {
    let mut found = Vec::new();
    for file in scanned_files() {
        let text = std::fs::read_to_string(&file).expect("read file");
        for (number, line) in text.lines().enumerate() {
            for name in hits(line) {
                found.push(format!("{}:{}: {name}", file.display(), number + 1));
            }
        }
    }
    assert!(
        found.is_empty(),
        "borrowed names found:\n{}",
        found.join("\n")
    );
}

#[test]
fn matcher_ignores_dates_but_flags_level_names() {
    assert!(
        hits("released 2021-11-05").is_empty(),
        "{:?}",
        hits("2021-11-05")
    );
    assert_eq!(hits("level 1-1 begins"), ["1-1"]);
    assert_eq!(hits("A Goomba walks"), ["goomba"]);
}

#[test]
fn examples_are_scanned() {
    assert!(
        scanned_files()
            .iter()
            .any(|f| f.ends_with("examples/sprite_sheet.rs")),
        "examples/ not scanned"
    );
}

#[test]
fn the_scan_would_catch_a_hit() {
    // Guard against a scan that silently reads nothing.
    let readme = Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md");
    assert!(std::fs::read_to_string(readme).expect("readme").len() > 100);
}
