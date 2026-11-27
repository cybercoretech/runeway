//! The original cast, tiles and items, drawn as text maps of palette keys.

use std::{collections::HashMap, sync::LazyLock};

use crate::model::Tool;

use super::palette::*;

/// A pixel map; each value is a palette index, 0 is transparent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sprite {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

impl Sprite {
    /// Build a sprite from text rows; each char is looked up in `key`, and
    /// `.` or an unknown char is transparent. Width is the longest row.
    pub fn from_rows(rows: &[&str], key: &[(char, u8)]) -> Sprite {
        let width = rows
            .iter()
            .map(|row| row.chars().count())
            .max()
            .unwrap_or(0);
        let mut pixels = Vec::with_capacity(width * rows.len());
        for row in rows {
            let mut count = 0;
            for c in row.chars() {
                let index = key.iter().find(|(k, _)| *k == c).map_or(0, |(_, i)| *i);
                pixels.push(index);
                count += 1;
            }
            pixels.resize(pixels.len() + (width - count), 0);
        }
        Sprite {
            width,
            height: rows.len(),
            pixels,
        }
    }

    /// A copy with every `from` pixel changed to `to` (tints the cyber-runner).
    pub fn recolor(&self, from: u8, to: u8) -> Sprite {
        let pixels = self
            .pixels
            .iter()
            .map(|&p| if p == from { to } else { p })
            .collect();
        Sprite {
            pixels,
            ..self.clone()
        }
    }
}

/// The palette key every map uses.
pub const KEY: &[(char, u8)] = &[
    ('n', PANEL),
    ('f', SKYLINE_FAR),
    ('m', SKYLINE_NEAR),
    ('d', GROUND),
    ('D', GROUND_SHADOW),
    ('c', RUNE_CYAN),
    ('v', RUNE_VIOLET),
    ('p', EMBER_PINK),
    ('g', GLYPH_GOLD),
    ('e', GEM_GREEN),
    ('b', BRASS),
    ('s', STEEL),
    ('w', WHITE),
    ('r', WARNING_RED),
    ('k', SHADOW),
];

/// One runner per agent family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Character {
    RuneRunner,
    BrassAutomaton,
    HoverDrone,
    TwinSpirit,
    CyberRunner,
}

impl Character {
    pub const ALL: [Character; 5] = [
        Character::RuneRunner,
        Character::BrassAutomaton,
        Character::HoverDrone,
        Character::TwinSpirit,
        Character::CyberRunner,
    ];

    pub fn for_tool(tool: &Tool) -> Character {
        match tool {
            Tool::Claude => Character::RuneRunner,
            Tool::Codex => Character::BrassAutomaton,
            Tool::OpenCode => Character::HoverDrone,
            Tool::Gemini => Character::TwinSpirit,
            Tool::Other(_) => Character::CyberRunner,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pose {
    Run,
    Jump,
    Stumble,
    Sit,
    Stand,
    Cheer,
}

impl Pose {
    pub const ALL: [Pose; 6] = [
        Pose::Run,
        Pose::Jump,
        Pose::Stumble,
        Pose::Sit,
        Pose::Stand,
        Pose::Cheer,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tile {
    Ground,
    RuneBrick,
    SigilBlock,
    Gate,
    Beacon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    GlyphCoin,
    DataGem,
    Spark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Backdrop {
    Skyline,
    ServerRacks,
    RunePillars,
}

type Map = &'static [&'static str];

// ---- Rune-runner (Claude): hooded, cyan visor slit, trailing cloak. ----

const RUNE_RUN_1: Map = &[
    "....vvvv....",
    "...vvvvvv...",
    "..vvkkkkvv..",
    "..vkkcccwv..",
    "..vvkkkkvv..",
    "...vvvvvv...",
    "..vvvvvvvvs.",
    ".vvvvvvvv...",
    "vv.vvvvvv...",
    "v...ss.ss...",
    "...ss...ss..",
    "..kk.....kk.",
];
const RUNE_RUN_2: Map = &[
    "....vvvv....",
    "...vvvvvv...",
    "..vvkkkkvv..",
    "..vkkcccwv..",
    "..vvkkkkvv..",
    "...vvvvvv...",
    "..vvvvvvvs..",
    "vvvvvvvvv...",
    ".v.vvvvvv...",
    "....sss.....",
    "....ss.s....",
    "....kk.kk...",
];
const RUNE_RUN_3: Map = &[
    "....vvvv....",
    "...vvvvvv...",
    "..vvkkkkvv..",
    "..vkkcccwv..",
    "..vvkkkkvv..",
    "...vvvvvv...",
    "..vvvvvvvvs.",
    ".vvvvvvvv...",
    "vv.vvvvvv...",
    "v...ss.ss...",
    "....s..s....",
    "...kk..kk...",
];
const RUNE_JUMP: Map = &[
    "....vvvv..s.",
    "...vvvvvvs..",
    "..vvkkkkvv..",
    "..vkkcccwv..",
    "..vvkkkkvv..",
    "...vvvvvv...",
    ".vvvvvvvv...",
    "vvvvvvvvv...",
    "vv.vvvvvv...",
    "...ss..ss...",
    "..kk....ss..",
    ".........kk.",
];
const RUNE_STUMBLE: Map = &[
    "......vvvv..",
    ".....vvvvvv.",
    "....vvkkkkvv",
    "....vkrrrkvv",
    "....vvkkkkvv",
    "...vvvvvvv..",
    "..svvvvvvv..",
    "....vvvvvv..",
    "....vvvvv...",
    "....ss.ss...",
    "...ss...ss..",
    "..kk.....kk.",
];
const RUNE_SIT: Map = &[
    "............",
    "............",
    "....vvvv....",
    "...vvvvvv...",
    "..vvkkkkvv..",
    "..vkkcccwv..",
    "..vvkkkkvv..",
    "...vvvvvv...",
    "..vvvvvvvv..",
    ".vvvvvvvvvs.",
    ".vvvsssss...",
    "......kk.kk.",
];
const RUNE_STAND: Map = &[
    "....vvvv....",
    "...vvvvvv...",
    "..vvkkkkvv..",
    "..vkkcccwv..",
    "..vvkkkkvv..",
    "...vvvvvv...",
    "..vvvvvvvv..",
    ".svvvvvvvvs.",
    "..vvvvvvvv..",
    "...vvvvvv...",
    "...ss..ss...",
    "...kk..kk...",
];
const RUNE_CHEER: Map = &[
    "s...vvvv...s",
    ".s.vvvvvv.s.",
    "..vvkkkkvv..",
    "..vkkcccwv..",
    "..vvkkkkvv..",
    "...vvvvvv...",
    "..vvvvvvvv..",
    "..vvvvvvvv..",
    "..vvvvvvvv..",
    "...vvvvvv...",
    "...ss..ss...",
    "...kk..kk...",
];

// ---- Brass automaton (Codex): round walker, chimney that puffs. ----

const BRASS_RUN_1: Map = &[
    ".........w..",
    "......ww....",
    ".....ss.....",
    "...bbsbbb...",
    "..bbbbbbbb..",
    ".bbbbbbccbb.",
    ".bbbbbbccbb.",
    ".bbbbbbbbbb.",
    ".bbkkkkkkbb.",
    "..bbbbbbbb..",
    "...s....s...",
    "..ss....ss..",
];
const BRASS_RUN_2: Map = &[
    "............",
    ".......w....",
    ".....sww....",
    "...bbsbbb...",
    "..bbbbbbbb..",
    ".bbbbbbccbb.",
    ".bbbbbbccbb.",
    ".bbbbbbbbbb.",
    ".bbkkkkkkbb.",
    "..bbbbbbbb..",
    "....s..s....",
    "....ss.ss...",
];
const BRASS_JUMP: Map = &[
    "......w.....",
    ".....ww.....",
    ".....ss.....",
    "...bbsbbb...",
    "..bbbbbbbb..",
    ".bbbbbbccbb.",
    ".bbbbbbccbb.",
    ".bbbbbbbbbb.",
    ".bbkkkkkkbb.",
    "..bbbbbbbb..",
    "..ss....ss..",
    "............",
];
const BRASS_STUMBLE: Map = &[
    "..w.........",
    "....ww......",
    ".....ss.....",
    "...bbsbbb...",
    "..bbbbbbbb..",
    ".bbbbbbrrbb.",
    ".bbbbbbrrbb.",
    ".bbbbbbbbbb.",
    ".bbkkkkkkbb.",
    "..bbbbbbbb..",
    "..s......s..",
    ".ss......ss.",
];
const BRASS_SIT: Map = &[
    "............",
    "............",
    "............",
    ".....ss.....",
    "...bbsbbb...",
    "..bbbbbbbb..",
    ".bbbbbbccbb.",
    ".bbbbbbbbbb.",
    ".bbbbbbbbbb.",
    ".bbkkkkkkbb.",
    "..bbbbbbbb..",
    "..ss....ss..",
];
const BRASS_STAND: Map = &[
    "............",
    "............",
    ".....ss.....",
    "...bbsbbb...",
    "..bbbbbbbb..",
    ".bbbbbbccbb.",
    ".bbbbbbccbb.",
    ".bbbbbbbbbb.",
    ".bbkkkkkkbb.",
    "..bbbbbbbb..",
    "...s....s...",
    "..ss....ss..",
];
const BRASS_CHEER: Map = &[
    "...g..w..g..",
    "......ww....",
    ".....ss.....",
    "s..bbsbbb..s",
    ".sbbbbbbbbs.",
    ".bbbbbbccbb.",
    ".bbbbbbccbb.",
    ".bbbbbbbbbb.",
    ".bbkkkkkkbb.",
    "..bbbbbbbb..",
    "...s....s...",
    "..ss....ss..",
];

// ---- Hover-drone (OpenCode): a lens with two side rotors. ----

const DRONE_RUN_1: Map = &[
    "www......www",
    ".s........s.",
    ".s..ssss..s.",
    ".ssssccssss.",
    "...scwccs...",
    "...sccccs...",
    "....ssss....",
    ".....pp.....",
    "......p.....",
];
const DRONE_RUN_2: Map = &[
    ".www....www.",
    ".s........s.",
    ".s..ssss..s.",
    ".ssssccssss.",
    "...scwccs...",
    "...sccccs...",
    "....ssss....",
    ".....pp.....",
    ".....p......",
];
const DRONE_JUMP: Map = &[
    "www......www",
    ".s........s.",
    ".s..ssss..s.",
    ".ssssccssss.",
    "...scwccs...",
    "...sccccs...",
    "....ssss....",
    ".....pp.....",
    ".....pp.....",
];
const DRONE_STUMBLE: Map = &[
    "..www.......",
    ".s.......www",
    ".s..ssss..s.",
    ".ssssrrssss.",
    "...srwrrs...",
    "...srrrrs...",
    "....ssss....",
    "............",
    "............",
];
const DRONE_SIT: Map = &[
    "............",
    "............",
    "............",
    "ww..ssss..ww",
    ".ssssccssss.",
    "...scwccs...",
    "...sccccs...",
    "....ssss....",
    "...s....s...",
];
const DRONE_STAND: Map = &[
    ".www....www.",
    ".s........s.",
    ".s..ssss..s.",
    ".ssssccssss.",
    "...scwccs...",
    "...sccccs...",
    "....ssss....",
    ".....pp.....",
    "............",
];
const DRONE_CHEER: Map = &[
    "www......www",
    ".s...gg...s.",
    ".s..ssss..s.",
    ".ssssggssss.",
    "...sgwggs...",
    "...sggggs...",
    "....ssss....",
    ".....pp.....",
    "......p.....",
];

// ---- Twin-spirit (Gemini): two linked orbs, violet and cyan. ----

const TWIN_RUN_1: Map = &[
    "............",
    ".vvv........",
    "vvwvv.......",
    "vvvvv...ccc.",
    "vvvvvg.ccwcc",
    ".vvv..gccccc",
    "........ccc.",
    "............",
    "............",
    "............",
    "............",
    "............",
];
const TWIN_RUN_2: Map = &[
    "............",
    "............",
    "............",
    ".vvv....ccc.",
    "vvwvv..ccwcc",
    "vvvvvggccccc",
    "vvvvv..ccccc",
    ".vvv....ccc.",
    "............",
    "............",
    "............",
    "............",
];
const TWIN_RUN_3: Map = &[
    "............",
    "............",
    "............",
    "........ccc.",
    ".vvv...ccwcc",
    "vvwvv.gccccc",
    "vvvvvg.ccccc",
    "vvvvv...ccc.",
    ".vvv........",
    "............",
    "............",
    "............",
];
const TWIN_JUMP: Map = &[
    ".vvv....ccc.",
    "vvwvv..ccwcc",
    "vvvvvggccccc",
    "vvvvv..ccccc",
    ".vvv....ccc.",
    "............",
    "............",
    "............",
    "............",
    "............",
    "............",
    "............",
];
const TWIN_STUMBLE: Map = &[
    "............",
    "............",
    "............",
    "............",
    "........ccc.",
    ".rrr...ccwcc",
    "rrwrr..ccccc",
    "rrrrr...ccc.",
    "rrrrr.......",
    ".rrr........",
    "............",
    "............",
];
const TWIN_SIT: Map = &[
    "............",
    "............",
    "............",
    "............",
    "............",
    "............",
    "............",
    ".vvv....ccc.",
    "vvwvv..ccwcc",
    "vvvvvggccccc",
    "vvvvv..ccccc",
    ".vvv....ccc.",
];
const TWIN_STAND: Map = &[
    "............",
    "............",
    "............",
    "............",
    ".vvv....ccc.",
    "vvwvv..ccwcc",
    "vvvvvggccccc",
    "vvvvv..ccccc",
    ".vvv....ccc.",
    "............",
    "............",
    "............",
];
const TWIN_CHEER: Map = &[
    "..g......g..",
    ".vvv....ccc.",
    "vvwvv..ccwcc",
    "vvvvvggccccc",
    "vvvvv..ccccc",
    ".vvv....ccc.",
    "............",
    ".....gg.....",
    "............",
    "............",
    "............",
    "............",
];

// ---- Cyber-runner (any other agent): a plain visored runner; the steel
// body is tinted per agent name when drawn. ----

const CYBER_RUN_1: Map = &[
    "....ssss....",
    "...ssssss...",
    "...sccccs...",
    "...ssssss...",
    "....ssss....",
    "...ssssss...",
    "..ssssssss..",
    ".s.ssssss.s.",
    "...ssssss...",
    "...kk..kk...",
    "..kk....kk..",
    ".kk......kk.",
];
const CYBER_RUN_2: Map = &[
    "....ssss....",
    "...ssssss...",
    "...sccccs...",
    "...ssssss...",
    "....ssss....",
    "...ssssss...",
    "..sssssss...",
    "...ssssss.s.",
    "...ssssss...",
    "....kkk.....",
    "....kk.k....",
    "....kk..k...",
];
const CYBER_JUMP: Map = &[
    "s...ssss....",
    ".s.ssssss...",
    "...sccccs...",
    "...ssssss...",
    "....ssss....",
    "...ssssss...",
    "..ssssssss..",
    "...ssssss.s.",
    "...ssssss...",
    "..kk...kk...",
    ".kk.....kk..",
    "............",
];
const CYBER_STUMBLE: Map = &[
    "......ssss..",
    ".....ssssss.",
    ".....srrrrs.",
    ".....ssssss.",
    "......ssss..",
    "....ssssss..",
    "..sssssss...",
    "....ssssss..",
    "....ssssss..",
    "....kk..kk..",
    "...kk....kk.",
    "..kk......kk",
];
const CYBER_SIT: Map = &[
    "............",
    "............",
    "............",
    "....ssss....",
    "...ssssss...",
    "...sccccs...",
    "...ssssss...",
    "....ssss....",
    "..ssssssss..",
    ".s.ssssss.s.",
    "...sssssskk.",
    "..........kk",
];
const CYBER_STAND: Map = &[
    "....ssss....",
    "...ssssss...",
    "...sccccs...",
    "...ssssss...",
    "....ssss....",
    "...ssssss...",
    "..ssssssss..",
    "..ssssssss..",
    "...ssssss...",
    "...kk..kk...",
    "...kk..kk...",
    "...kk..kk...",
];
const CYBER_CHEER: Map = &[
    "s...ssss...s",
    ".s.ssssss.s.",
    "..ssccccss..",
    "...ssssss...",
    "....ssss....",
    "...ssssss...",
    "...ssssss...",
    "...ssssss...",
    "...ssssss...",
    "...kk..kk...",
    "...kk..kk...",
    "...kk..kk...",
];

// ---- Tiles (8×8; the gate and beacon are taller). ----

const GROUND_TILE: Map = &[
    "mmmmmmmm", "dddddddd", "ddDddddD", "dddddDdd", "dDdddddd", "ddddDddd", "Ddddddd.", "DDDDDDDD",
];
const RUNE_BRICK: Map = &[
    "kkkkkkkk", "kvvvvvvk", "kvvcvvvk", "kvcccvvk", "kvvcvcvk", "kvvvcvvk", "kvvvvvvk", "kkkkkkkk",
];
const SIGIL_BLOCK: Map = &[
    "kkkkkkkk", "knnggnnk", "kngnngnk", "kgnnnngk", "kgnnnngk", "kngnngnk", "knnggnnk", "kkkkkkkk",
];
const GATE: Map = &[
    "ssssssss", "sc.c.c.s", "sc.c.c.s", "sc.c.c.s", "ssssssss", "sc.c.c.s", "sc.c.c.s", "sc.c.c.s",
    "ssssssss", "sc.c.c.s", "sc.c.c.s", "ssssssss",
];
const BEACON: Map = &[
    "...gg...", "..gwwg..", "...gg...", "...ss...", "..ssss..", "..scss..", "..ssss..", "...ss...",
    "..ssss..", "..sscs..", "..ssss..", "...ss...", "..ssss..", ".sscsss.", ".ssssss.", "ssssssss",
];

// ---- Items (6×6) and the critter. ----

const GLYPH_COIN: Map = &["..gg..", ".gwgg.", "ggwggg", "gggwgg", ".ggwg.", "..gg.."];
const DATA_GEM: Map = &["..ee..", ".ewee.", "ewweee", "eeeeee", ".eeee.", "..ee.."];
const SPARK: Map = &["...g..", ".g.w.g", "..www.", "gwwwwg", "..www.", ".g.w.g"];
const CRITTER_1: Map = &[".pppp.", "pkppkp", "pppppp", ".p..p.", "p....p"];
const CRITTER_2: Map = &[".pppp.", "pkppkp", "pppppp", ".p..p.", "..pp.."];

// ---- Parallax backdrops (32 px strips, tiled sideways). ----

const SKYLINE: Map = &[
    "................................",
    "......ff..............ff........",
    "..fff.ff....ffff......ff..fff...",
    "..fcf.ffff..fcff...ffffff.fcf...",
    "..fff.ffcf..ffff.f.fcfcff.fff.ff",
    "fffff.ffff.fffcf.fffffffffffffff",
    "fcfffmmmmfffffffmmmmfffcffmmmfff",
    "fffffmcmmfffcfffmcmmffffffmcmfff",
    "ffffmmmmmmffffffmmmmmfffffmmmmff",
    "mmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmm",
];
const SERVER_RACKS: Map = &[
    "................................",
    ".nnnnnn...nnnnnn...nnnnnn.......",
    ".nkkkkn...nkkkkn...nkkkkn..nnnnn",
    ".nkeckn...nkcekn...nkeekn..nkkkk",
    ".nkkkkn...nkkkkn...nkkkkn..nkeck",
    ".nkcekn...nkeckn...nkcckn..nkkkk",
    ".nkkkkn...nkkkkn...nkkkkn..nkcek",
    ".nkeekn...nkkekn...nkcekn..nkkkk",
    ".nkkkkn...nkkkkn...nkkkkn..nkkkk",
    "nnnnnnnnnnnnnnnnnnnnnnnnnnnnnnnn",
];
const RUNE_PILLARS: Map = &[
    "................................",
    "..mmm..........mmm..............",
    "..mvm..........mvm.........mmm..",
    "..mmm....mmm...mmm.........mvm..",
    "..mvm....mvm...mvm.........mmm..",
    "..mmm....mmm...mmm....mmm..mvm..",
    "..mvm....mvm...mvm....mvm..mmm..",
    "..mmm....mmm...mmm....mmm..mvm..",
    ".mmmmm..mmmmm.mmmmm..mmmmm.mmm..",
    "mmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmm",
];

/// Each character's frames per pose.
const CAST: &[(Character, Pose, &[Map])] = &[
    (
        Character::RuneRunner,
        Pose::Run,
        &[RUNE_RUN_1, RUNE_RUN_2, RUNE_RUN_3, RUNE_RUN_2],
    ),
    (Character::RuneRunner, Pose::Jump, &[RUNE_JUMP]),
    (Character::RuneRunner, Pose::Stumble, &[RUNE_STUMBLE]),
    (Character::RuneRunner, Pose::Sit, &[RUNE_SIT]),
    (Character::RuneRunner, Pose::Stand, &[RUNE_STAND]),
    (Character::RuneRunner, Pose::Cheer, &[RUNE_CHEER]),
    (
        Character::BrassAutomaton,
        Pose::Run,
        &[BRASS_RUN_1, BRASS_RUN_2],
    ),
    (Character::BrassAutomaton, Pose::Jump, &[BRASS_JUMP]),
    (Character::BrassAutomaton, Pose::Stumble, &[BRASS_STUMBLE]),
    (Character::BrassAutomaton, Pose::Sit, &[BRASS_SIT]),
    (Character::BrassAutomaton, Pose::Stand, &[BRASS_STAND]),
    (Character::BrassAutomaton, Pose::Cheer, &[BRASS_CHEER]),
    (
        Character::HoverDrone,
        Pose::Run,
        &[DRONE_RUN_1, DRONE_RUN_2],
    ),
    (Character::HoverDrone, Pose::Jump, &[DRONE_JUMP]),
    (Character::HoverDrone, Pose::Stumble, &[DRONE_STUMBLE]),
    (Character::HoverDrone, Pose::Sit, &[DRONE_SIT]),
    (Character::HoverDrone, Pose::Stand, &[DRONE_STAND]),
    (Character::HoverDrone, Pose::Cheer, &[DRONE_CHEER]),
    (
        Character::TwinSpirit,
        Pose::Run,
        &[TWIN_RUN_1, TWIN_RUN_2, TWIN_RUN_3, TWIN_RUN_2],
    ),
    (Character::TwinSpirit, Pose::Jump, &[TWIN_JUMP]),
    (Character::TwinSpirit, Pose::Stumble, &[TWIN_STUMBLE]),
    (Character::TwinSpirit, Pose::Sit, &[TWIN_SIT]),
    (Character::TwinSpirit, Pose::Stand, &[TWIN_STAND]),
    (Character::TwinSpirit, Pose::Cheer, &[TWIN_CHEER]),
    (
        Character::CyberRunner,
        Pose::Run,
        &[CYBER_RUN_1, CYBER_RUN_2],
    ),
    (Character::CyberRunner, Pose::Jump, &[CYBER_JUMP]),
    (Character::CyberRunner, Pose::Stumble, &[CYBER_STUMBLE]),
    (Character::CyberRunner, Pose::Sit, &[CYBER_SIT]),
    (Character::CyberRunner, Pose::Stand, &[CYBER_STAND]),
    (Character::CyberRunner, Pose::Cheer, &[CYBER_CHEER]),
];

/// Every text map, by name, so tests can check them all.
pub const MAPS: &[(&str, Map)] = &[
    ("rune_run_1", RUNE_RUN_1),
    ("rune_run_2", RUNE_RUN_2),
    ("rune_run_3", RUNE_RUN_3),
    ("rune_jump", RUNE_JUMP),
    ("rune_stumble", RUNE_STUMBLE),
    ("rune_sit", RUNE_SIT),
    ("rune_stand", RUNE_STAND),
    ("rune_cheer", RUNE_CHEER),
    ("brass_run_1", BRASS_RUN_1),
    ("brass_run_2", BRASS_RUN_2),
    ("brass_jump", BRASS_JUMP),
    ("brass_stumble", BRASS_STUMBLE),
    ("brass_sit", BRASS_SIT),
    ("brass_stand", BRASS_STAND),
    ("brass_cheer", BRASS_CHEER),
    ("drone_run_1", DRONE_RUN_1),
    ("drone_run_2", DRONE_RUN_2),
    ("drone_jump", DRONE_JUMP),
    ("drone_stumble", DRONE_STUMBLE),
    ("drone_sit", DRONE_SIT),
    ("drone_stand", DRONE_STAND),
    ("drone_cheer", DRONE_CHEER),
    ("twin_run_1", TWIN_RUN_1),
    ("twin_run_2", TWIN_RUN_2),
    ("twin_run_3", TWIN_RUN_3),
    ("twin_jump", TWIN_JUMP),
    ("twin_stumble", TWIN_STUMBLE),
    ("twin_sit", TWIN_SIT),
    ("twin_stand", TWIN_STAND),
    ("twin_cheer", TWIN_CHEER),
    ("cyber_run_1", CYBER_RUN_1),
    ("cyber_run_2", CYBER_RUN_2),
    ("cyber_jump", CYBER_JUMP),
    ("cyber_stumble", CYBER_STUMBLE),
    ("cyber_sit", CYBER_SIT),
    ("cyber_stand", CYBER_STAND),
    ("cyber_cheer", CYBER_CHEER),
    ("ground", GROUND_TILE),
    ("rune_brick", RUNE_BRICK),
    ("sigil_block", SIGIL_BLOCK),
    ("gate", GATE),
    ("beacon", BEACON),
    ("glyph_coin", GLYPH_COIN),
    ("data_gem", DATA_GEM),
    ("spark", SPARK),
    ("critter_1", CRITTER_1),
    ("critter_2", CRITTER_2),
    ("skyline", SKYLINE),
    ("server_racks", SERVER_RACKS),
    ("rune_pillars", RUNE_PILLARS),
];

fn build(map: Map) -> Sprite {
    Sprite::from_rows(map, KEY)
}

static FRAMES: LazyLock<HashMap<(Character, Pose), Vec<Sprite>>> = LazyLock::new(|| {
    CAST.iter()
        .map(|(character, pose, maps)| {
            ((*character, *pose), maps.iter().map(|m| build(m)).collect())
        })
        .collect()
});

static TILES: LazyLock<HashMap<Tile, Sprite>> = LazyLock::new(|| {
    [
        (Tile::Ground, GROUND_TILE),
        (Tile::RuneBrick, RUNE_BRICK),
        (Tile::SigilBlock, SIGIL_BLOCK),
        (Tile::Gate, GATE),
        (Tile::Beacon, BEACON),
    ]
    .into_iter()
    .map(|(tile, map)| (tile, build(map)))
    .collect()
});

static ITEMS: LazyLock<HashMap<Item, Sprite>> = LazyLock::new(|| {
    [
        (Item::GlyphCoin, GLYPH_COIN),
        (Item::DataGem, DATA_GEM),
        (Item::Spark, SPARK),
    ]
    .into_iter()
    .map(|(item, map)| (item, build(map)))
    .collect()
});

static BACKDROPS: LazyLock<HashMap<Backdrop, Sprite>> = LazyLock::new(|| {
    [
        (Backdrop::Skyline, SKYLINE),
        (Backdrop::ServerRacks, SERVER_RACKS),
        (Backdrop::RunePillars, RUNE_PILLARS),
    ]
    .into_iter()
    .map(|(backdrop, map)| (backdrop, build(map)))
    .collect()
});

static CRITTER: LazyLock<Vec<Sprite>> = LazyLock::new(|| vec![build(CRITTER_1), build(CRITTER_2)]);

static EMPTY: Sprite = Sprite {
    width: 0,
    height: 0,
    pixels: Vec::new(),
};

/// The frames of `character` in `pose` (empty only if a map is missing).
pub fn frames(character: Character, pose: Pose) -> &'static [Sprite] {
    FRAMES
        .get(&(character, pose))
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

pub fn tile(tile: Tile) -> &'static Sprite {
    TILES.get(&tile).unwrap_or(&EMPTY)
}

pub fn item(item: Item) -> &'static Sprite {
    ITEMS.get(&item).unwrap_or(&EMPTY)
}

pub fn backdrop(backdrop: Backdrop) -> &'static Sprite {
    BACKDROPS.get(&backdrop).unwrap_or(&EMPTY)
}

/// The cosmetic patroller's two walk frames.
pub fn critter() -> &'static [Sprite] {
    &CRITTER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_rows_maps_keys_and_transparency() {
        let sprite = Sprite::from_rows(&["c.", "gc"], &[('c', RUNE_CYAN), ('g', GLYPH_GOLD)]);
        assert_eq!((sprite.width, sprite.height), (2, 2));
        assert_eq!(sprite.pixels, vec![RUNE_CYAN, 0, GLYPH_GOLD, RUNE_CYAN]);
    }

    #[test]
    fn every_sprite_row_has_its_width() {
        assert!(!MAPS.is_empty());
        for (name, rows) in MAPS {
            let width = rows[0].chars().count();
            for (i, row) in rows.iter().enumerate() {
                assert_eq!(row.chars().count(), width, "{name} row {i}");
            }
        }
    }

    #[test]
    fn every_sprite_uses_known_palette_keys() {
        for (name, rows) in MAPS {
            for row in *rows {
                for c in row.chars() {
                    assert!(
                        c == '.' || KEY.iter().any(|(k, _)| *k == c),
                        "{name}: unknown key {c:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_character_has_all_poses() {
        for character in Character::ALL {
            for pose in Pose::ALL {
                let count = frames(character, pose).len();
                let ok = match pose {
                    Pose::Run => (2..=4).contains(&count),
                    _ => count == 1,
                };
                assert!(ok, "{character:?} {pose:?} has {count} frames");
            }
        }
    }

    #[test]
    fn sprites_have_their_sizes() {
        for character in Character::ALL {
            let height = if character == Character::HoverDrone {
                9
            } else {
                12
            };
            for pose in Pose::ALL {
                for sprite in frames(character, pose) {
                    assert_eq!((sprite.width, sprite.height), (12, height), "{character:?}");
                }
            }
        }
        for t in [Tile::Ground, Tile::RuneBrick, Tile::SigilBlock] {
            let s = tile(t);
            assert_eq!((s.width, s.height), (8, 8), "{t:?}");
        }
        for i in [Item::GlyphCoin, Item::DataGem, Item::Spark] {
            let s = item(i);
            assert_eq!((s.width, s.height), (6, 6), "{i:?}");
        }
        for b in [
            Backdrop::Skyline,
            Backdrop::ServerRacks,
            Backdrop::RunePillars,
        ] {
            assert_eq!(backdrop(b).width, 32, "{b:?}");
        }
        assert_eq!(critter().len(), 2);
    }

    #[test]
    fn for_tool_maps_each_agent() {
        assert_eq!(Character::for_tool(&Tool::Claude), Character::RuneRunner);
        assert_eq!(Character::for_tool(&Tool::Codex), Character::BrassAutomaton);
        assert_eq!(Character::for_tool(&Tool::OpenCode), Character::HoverDrone);
        assert_eq!(Character::for_tool(&Tool::Gemini), Character::TwinSpirit);
        assert_eq!(
            Character::for_tool(&Tool::Other("pi".into())),
            Character::CyberRunner
        );
    }
}
