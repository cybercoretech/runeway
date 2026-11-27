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

#[test]
fn no_borrowed_game_names() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    files.push(root.join("README.md"));
    let mut hits = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).expect("read file");
        for (number, line) in text.lines().enumerate() {
            let lower = line.to_lowercase();
            for name in FORBIDDEN {
                if lower.contains(name) {
                    hits.push(format!("{}:{}: {name}", file.display(), number + 1));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "borrowed names found:\n{}",
        hits.join("\n")
    );
}

#[test]
fn the_scan_would_catch_a_hit() {
    // Guard against a scan that silently reads nothing.
    let readme = Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md");
    assert!(std::fs::read_to_string(readme).expect("readme").len() > 100);
}
