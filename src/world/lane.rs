//! One agent's lane: a scrolling level, the runner, and the effects its
//! activity spawns. Pure: time is `tick()`, randomness is seeded by id.
//!
//! Heights (`y`) of the runner and effects are measured up from the ground,
//! so the simulation does not depend on the size it is drawn at.

use crate::model::{ActivityKind, LaneState, SessionStatus};

use super::{
    Rgb,
    mapping::state_for,
    palette::{
        BRASS, EMBER_PINK, GEM_GREEN, GLYPH_GOLD, GROUND, GROUND_SHADOW, RUNE_CYAN, RUNE_VIOLET,
        SHADOW, SKY, SKYLINE_NEAR, STEEL, WARNING_RED, WHITE, color,
    },
    pixels::Pixels,
    rng::Rng,
    runner::{Runner, RunnerState, scroll_speed},
    sprites::{Backdrop, Character, Item, Sprite, Tile, backdrop, critter, frames, item, tile},
};

/// Ticks without a new activity before a moving runner sits down (30 s).
pub const IDLE_AFTER_TICKS: u32 = 900;
/// Level chunk width in px.
const CHUNK: i64 = 64;
/// Rows of ground drawn under the runner.
const GROUND_ROWS: i32 = 4;
/// Height above the ground of hanging blocks (rune bricks, sigil blocks).
const HANG: i32 = 12;
/// How long a coin waits to be collected, and a gem lies on the ground.
const COIN_LIFE: u32 = 150;
const GEM_LIFE: u32 = 45;
const SPARK_LIFE: u32 = 18;
/// Ticks a brick takes to break once the jump starts (the jump's peak).
const BRICK_BREAKS_AT: u32 = 3;
/// How long a helper's companion follows the runner (10 s).
const COMPANION_TICKS: u32 = 300;
/// Body colors the cyber-runner is tinted with, picked by agent name.
const TINTS: [u8; 5] = [EMBER_PINK, GEM_GREEN, BRASS, RUNE_VIOLET, GLYPH_GOLD];

/// A short-lived thing in the level, in world coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    Coin,
    Brick,
    Gem,
    Spark,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effect {
    pub kind: EffectKind,
    /// World x of the left edge.
    pub x: i64,
    /// Height of the bottom edge above the ground.
    pub y: i32,
    pub vx: i32,
    pub vy: i32,
    pub age: u32,
}

/// The seeded level: decor is generated per 64-px chunk on demand, so only
/// what is on screen ever exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Level {
    rng: Rng,
    pub backdrop: Backdrop,
}

/// What one chunk holds.
struct Chunk {
    sigil: Option<i64>,
    critter: Option<i64>,
    stars: [(i64, i32); 3],
}

impl Level {
    fn new(id: &str) -> Level {
        let mut rng = Rng::for_id(id);
        let backdrop = match rng.below(3) {
            0 => Backdrop::Skyline,
            1 => Backdrop::ServerRacks,
            _ => Backdrop::RunePillars,
        };
        Level { rng, backdrop }
    }

    fn chunk(&self, index: i64) -> Chunk {
        let mut rng = self.rng.fork(index);
        let base = index * CHUNK;
        let sigil = (rng.below(3) == 0).then(|| base + rng.below(48) as i64);
        let critter = (rng.below(2) == 0).then(|| base + 8 + rng.below(16) as i64);
        let mut star = || (base + rng.below(CHUNK as u64) as i64, rng.below(8) as i32);
        let stars = [star(), star(), star()];
        Chunk {
            sigil,
            critter,
            stars,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LaneSim {
    pub id: String,
    pub character: Character,
    pub runner: Runner,
    pub scroll: i64,
    pub level: Level,
    pub effects: Vec<Effect>,
    pub last_activity_change: u64,
    /// The activity and status last applied, so a sync only reacts to change.
    last_activity: (ActivityKind, String),
    last_status: SessionStatus,
    /// Ticks since the last new activity.
    quiet: u32,
    ticks: u64,
    companion: u32,
    tint: Option<u8>,
}

impl LaneSim {
    pub fn new(lane: &LaneState) -> LaneSim {
        let character = Character::for_tool(&lane.session.tool);
        let tint = (character == Character::CyberRunner).then(|| {
            let pick = Rng::for_id(&lane.session.title).below(TINTS.len() as u64);
            TINTS[pick as usize]
        });
        let mut runner = Runner::new();
        runner.set_state(state_for(lane.activity, &lane.session.status));
        LaneSim {
            id: lane.session.id.clone(),
            character,
            runner,
            scroll: 0,
            level: Level::new(&lane.session.id),
            effects: Vec::new(),
            last_activity_change: lane.last_change,
            last_activity: (lane.activity, lane.detail.clone()),
            last_status: lane.session.status.clone(),
            quiet: 0,
            ticks: 0,
            companion: 0,
            tint,
        }
    }

    /// Apply what changed in `lane` since the last sync: a new activity
    /// switches the runner's state and spawns its effects; a status change
    /// alone only switches the state.
    pub fn sync(&mut self, lane: &LaneState) {
        // Anything new on the lane (tokens included) means the agent is busy.
        if lane.last_change != self.last_activity_change {
            self.quiet = 0;
        }
        self.last_activity_change = lane.last_change;
        let activity = (lane.activity, lane.detail.clone());
        let status = &lane.session.status;
        let state = state_for(lane.activity, status);
        if activity != self.last_activity {
            self.last_activity = activity;
            self.last_status = status.clone();
            self.quiet = 0;
            self.runner.set_state(state);
            self.spawn_for(lane.activity);
        } else if *status != self.last_status {
            self.last_status = status.clone();
            if state != self.runner.state {
                self.runner.set_state(state);
            }
        }
    }

    fn runner_world_x(&self) -> i64 {
        self.scroll + i64::from(self.runner.x)
    }

    fn spawn_for(&mut self, kind: ActivityKind) {
        let rx = self.runner_world_x();
        let effect = |kind, x, y, vx, vy| Effect {
            kind,
            x,
            y,
            vx,
            vy,
            age: 0,
        };
        match kind {
            ActivityKind::Reading | ActivityKind::Searching => {
                self.effects
                    .push(effect(EffectKind::Coin, rx + 28, 7, 0, 0));
            }
            ActivityKind::Editing => {
                self.effects
                    .push(effect(EffectKind::Brick, rx + 5, HANG, 0, 0));
            }
            ActivityKind::CommandPassed | ActivityKind::SubagentStarted => {
                let mut rng = Rng::for_id(&self.id).fork(self.ticks as i64);
                for _ in 0..5 {
                    let vx = rng.below(5) as i32 - 2;
                    let vy = 2 + rng.below(3) as i32;
                    self.effects
                        .push(effect(EffectKind::Spark, rx + 4, 10, vx, vy));
                }
                if kind == ActivityKind::SubagentStarted {
                    self.companion = COMPANION_TICKS;
                }
            }
            _ => {}
        }
    }

    /// Advance one tick (30 per second).
    pub fn tick(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
        self.quiet = self.quiet.saturating_add(1);
        // A command can run for minutes; its end arrives as an event.
        let moving = matches!(
            self.runner.state,
            RunnerState::Running | RunnerState::Collecting
        );
        if moving && self.quiet > IDLE_AFTER_TICKS {
            self.runner.set_state(RunnerState::Idle);
        }
        self.runner.tick();
        self.scroll += i64::from(scroll_speed(self.runner.state));
        self.companion = self.companion.saturating_sub(1);

        let rx = self.runner_world_x();
        for effect in &mut self.effects {
            effect.age += 1;
            match effect.kind {
                EffectKind::Brick if effect.age >= BRICK_BREAKS_AT => {
                    // The brick breaks; a gem pops out of it.
                    *effect = Effect {
                        kind: EffectKind::Gem,
                        x: effect.x + 1,
                        y: effect.y,
                        vx: 0,
                        vy: 2,
                        age: 0,
                    };
                }
                EffectKind::Gem => {
                    effect.y = (effect.y + effect.vy).max(0);
                    effect.vy = if effect.y == 0 { 0 } else { effect.vy - 1 };
                }
                EffectKind::Spark => {
                    effect.x += i64::from(effect.vx);
                    effect.y += effect.vy;
                    effect.vy -= 1;
                }
                _ => {}
            }
        }
        self.effects.retain(|effect| match effect.kind {
            // Collected once the runner reaches it.
            EffectKind::Coin => effect.age < COIN_LIFE && rx + 8 < effect.x,
            EffectKind::Brick => true,
            EffectKind::Gem => effect.age < GEM_LIFE,
            EffectKind::Spark => effect.age < SPARK_LIFE && effect.y > -4,
        });
    }

    /// True while something moves: the level scrolls, an effect plays, the
    /// runner is in the air or knocked back, or a companion follows. A lane
    /// that only sits or stands still is not animating.
    pub fn is_animating(&self) -> bool {
        scroll_speed(self.runner.state) > 0
            || !self.effects.is_empty()
            || self.runner.y > 0
            || self.runner.x != 0
            || self.runner.state == RunnerState::Stumbling
            || self.companion > 0
    }

    /// Where the runner stands on screen (its usual spot).
    fn home_x(width: usize) -> i32 {
        (width / 4).min(24) as i32
    }

    fn ground_top(px: &Pixels) -> i32 {
        px.height as i32 - GROUND_ROWS
    }

    /// Top-left of the rune gate on screen (drawn while waiting).
    pub fn gate_at(&self, px: &Pixels) -> (i32, i32) {
        let gate = tile(Tile::Gate);
        (
            Self::home_x(px.width) + 16,
            Self::ground_top(px) - gate.height as i32,
        )
    }

    /// Top-left of the beacon tower on screen (drawn once finished).
    pub fn beacon_at(&self, px: &Pixels) -> (i32, i32) {
        let beacon = tile(Tile::Beacon);
        (
            Self::home_x(px.width) + 18,
            Self::ground_top(px) - beacon.height as i32,
        )
    }

    /// Draw the lane into `px`, filling all of it.
    pub fn render(&self, px: &mut Pixels) {
        if px.width == 0 || px.height == 0 {
            return;
        }
        let width = px.width as i32;
        let ground = Self::ground_top(px);
        let home = Self::home_x(px.width);
        let to_screen = |x: i64, scroll: i64| (x - scroll) as i32;
        let visible_chunks = |scroll: i64| {
            let first = scroll.div_euclid(CHUNK) - 1;
            let last = (scroll + i64::from(width)).div_euclid(CHUNK) + 1;
            first..=last
        };

        px.fill_rect(0, 0, width, px.height as i32, color(SKY));

        // Far layer: stars at a quarter of the speed.
        let far = self.scroll / 4;
        for index in visible_chunks(far) {
            for (x, y) in self.level.chunk(index).stars {
                let twinkle = (self.ticks / 10 + x.rem_euclid(5) as u64).is_multiple_of(5);
                let c = if twinkle { WHITE } else { SKYLINE_NEAR };
                px.set(to_screen(x, far), y, color(c));
            }
        }

        // Middle layer: the backdrop strip at half speed.
        let strip = backdrop(self.level.backdrop);
        if strip.width > 0 {
            let mid = self.scroll / 2;
            let offset = mid.rem_euclid(strip.width as i64) as i32;
            let top = ground - strip.height as i32;
            let mut x = -offset;
            while x < width {
                px.blit(strip, x, top, false);
                x += strip.width as i32;
            }
        }

        if self.runner.state == RunnerState::MachineSection {
            self.draw_machine(px, home, ground);
        }

        // Near layer: level decor and ground at full speed.
        for index in visible_chunks(self.scroll) {
            let chunk = self.level.chunk(index);
            if let Some(x) = chunk.sigil {
                let block = tile(Tile::SigilBlock);
                let top = ground - HANG - block.height as i32;
                px.blit(block, to_screen(x, self.scroll), top, false);
            }
            if let Some(x) = chunk.critter {
                let frames = critter();
                if !frames.is_empty() {
                    let walk = (self.ticks / 2 % 32) as i64;
                    let pace = if walk < 16 { walk } else { 31 - walk };
                    let sprite = &frames[(self.ticks / 6) as usize % frames.len()];
                    let top = ground - sprite.height as i32;
                    px.blit(sprite, to_screen(x + pace, self.scroll), top, walk >= 16);
                }
            }
        }
        let ground_tile = tile(Tile::Ground);
        if ground_tile.width > 0 {
            let offset = self.scroll.rem_euclid(ground_tile.width as i64) as i32;
            let mut x = -offset;
            while x < width {
                px.blit(ground_tile, x, ground, false);
                x += ground_tile.width as i32;
            }
        }

        match self.runner.state {
            RunnerState::AtGate => {
                let (gx, gy) = self.gate_at(px);
                px.blit(tile(Tile::Gate), gx, gy, false);
            }
            RunnerState::Finished => {
                let (bx, by) = self.beacon_at(px);
                px.blit(tile(Tile::Beacon), bx, by, false);
                if (self.ticks / 8).is_multiple_of(2) {
                    px.fill_rect(bx + 3, by + 1, 2, 1, color(GLYPH_GOLD));
                }
            }
            _ => {}
        }

        for effect in &self.effects {
            let sprite = match effect.kind {
                EffectKind::Coin => item(Item::GlyphCoin),
                EffectKind::Brick => tile(Tile::RuneBrick),
                EffectKind::Gem => item(Item::DataGem),
                EffectKind::Spark => item(Item::Spark),
            };
            let top = ground - effect.y - sprite.height as i32;
            px.blit(sprite, to_screen(effect.x, self.scroll) + home, top, false);
        }

        let runner = self.runner_sprite();
        let runner_x = home + self.runner.x;
        let runner_top = runner.map_or(ground, |s| ground - self.runner.y - s.height as i32);
        if self.companion > 0 {
            let bob = ((self.ticks / 4) % 2) as i32;
            px.fill_rect(runner_x - 6, runner_top + 2 + bob, 4, 4, color(RUNE_CYAN));
            px.set(runner_x - 5, runner_top + 3 + bob, color(WHITE));
        }
        if let Some(sprite) = runner {
            self.blit_runner(px, sprite, runner_x, runner_top);
        }
        if self.runner.state == RunnerState::AtGate && (self.ticks / 8).is_multiple_of(2) {
            let glyph = item(Item::GlyphCoin);
            px.blit(
                glyph,
                runner_x + 3,
                runner_top - glyph.height as i32 - 1,
                false,
            );
        }

        // A failed command: a brief red flash around the lane.
        if self.runner.state == RunnerState::Stumbling && self.runner.state_ticks < 4 {
            let red = color(WARNING_RED);
            px.fill_rect(0, 0, width, 1, red);
            px.fill_rect(0, px.height as i32 - 1, width, 1, red);
            px.fill_rect(0, 0, 1, px.height as i32, red);
            px.fill_rect(width - 1, 0, 1, px.height as i32, red);
        }

        if self.runner.state == RunnerState::LostSignal {
            grey_out(px);
        }
    }

    fn runner_sprite(&self) -> Option<&'static Sprite> {
        let frames = frames(self.character, self.runner.pose);
        (!frames.is_empty()).then(|| &frames[self.runner.frame % frames.len()])
    }

    fn blit_runner(&self, px: &mut Pixels, sprite: &Sprite, x: i32, y: i32) {
        match self.tint {
            Some(tint) => px.blit(&sprite.recolor(STEEL, tint), x, y, false),
            None => px.blit(sprite, x, y, false),
        }
    }

    /// Gears turning and steam venting behind the runner.
    fn draw_machine(&self, px: &mut Pixels, home: i32, ground: i32) {
        let pipe_y = ground - 9;
        px.fill_rect(home - 8, pipe_y, 56, 2, color(STEEL));
        let turn = (self.ticks / 3).is_multiple_of(2);
        for (i, gx) in [home - 4, home + 18, home + 38].into_iter().enumerate() {
            let gy = ground - 7;
            px.fill_rect(gx, gy, 6, 6, color(BRASS));
            px.fill_rect(gx + 2, gy + 2, 2, 2, color(SHADOW));
            let (a, b) = if turn == (i % 2 == 0) {
                ((gx + 2, gy), (gx + 2, gy + 5))
            } else {
                ((gx, gy + 2), (gx + 5, gy + 2))
            };
            px.set(a.0, a.1, color(SHADOW));
            px.set(b.0, b.1, color(SHADOW));
        }
        for (i, vx) in [home + 10, home + 30].into_iter().enumerate() {
            let rise = ((self.ticks + i as u64 * 7) % 12) as i32;
            px.fill_rect(vx, pipe_y - 2 - rise, 2, 2, color(WHITE));
        }
    }
}

/// Lost signal: every pixel becomes one of four greys, with dark scanlines.
fn grey_out(px: &mut Pixels) {
    let levels: [Rgb; 4] = [
        color(SHADOW),
        color(GROUND_SHADOW),
        color(GROUND),
        color(STEEL),
    ];
    let width = px.width.max(1);
    for (i, c) in px.data.iter_mut().enumerate() {
        let lum = (u32::from(c[0]) * 3 + u32::from(c[1]) * 6 + u32::from(c[2])) / 10;
        let mut level: usize = match lum {
            0..16 => 0,
            16..50 => 1,
            50..110 => 2,
            _ => 3,
        };
        if (i / width).is_multiple_of(3) {
            level = level.saturating_sub(1);
        }
        *c = levels[level];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{Event, Session, Tool, World},
        world::{
            palette::{self, GLYPH_GOLD, GROUND, GROUND_SHADOW, SHADOW, SKY, STEEL, color},
            runner::RunnerState,
        },
    };
    use std::path::{Path, PathBuf};

    const W: usize = 160;
    const H: usize = 22;

    fn lane(
        id: &str,
        tool: Tool,
        status: SessionStatus,
        activity: Option<(ActivityKind, &str)>,
    ) -> LaneState {
        let mut world = World::default();
        world.apply(Event::Session(Session {
            id: id.into(),
            title: "app".into(),
            path: PathBuf::from("/w/app"),
            tool,
            status,
            tmux_session: None,
            transcript: None,
        }));
        if let Some((kind, detail)) = activity {
            world.apply(Event::Activity {
                id: id.into(),
                kind,
                detail: detail.into(),
            });
        }
        world.lanes.remove(0)
    }

    fn running(activity: ActivityKind, detail: &str) -> LaneState {
        lane(
            "a",
            Tool::Claude,
            SessionStatus::Running,
            Some((activity, detail)),
        )
    }

    fn render(sim: &LaneSim) -> Pixels {
        let mut px = Pixels::new(W, H, color(SKY));
        sim.render(&mut px);
        px
    }

    fn ticks(sim: &mut LaneSim, n: u32) {
        for _ in 0..n {
            sim.tick();
        }
    }

    /// One char per pixel: the palette index in hex, `?` for other colors.
    fn to_text(px: &Pixels) -> String {
        let mut out = String::new();
        for row in px.data.chunks(px.width) {
            for c in row {
                let index = palette::PALETTE.iter().position(|p| p == c);
                out.push(index.map_or('?', |i| char::from_digit(i as u32, 16).unwrap_or('?')));
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn same_id_same_level() {
        let lane_a = running(ActivityKind::Thinking, "");
        let mut one = LaneSim::new(&lane_a);
        let mut two = LaneSim::new(&lane_a);
        ticks(&mut one, 100);
        ticks(&mut two, 100);
        assert_eq!(render(&one), render(&two));
        let other = lane(
            "zz-other",
            Tool::Claude,
            SessionStatus::Running,
            Some((ActivityKind::Thinking, "")),
        );
        let mut three = LaneSim::new(&other);
        ticks(&mut three, 100);
        assert_ne!(
            render(&one),
            render(&three),
            "different agents, different levels"
        );
    }

    #[test]
    fn level_scrolls_while_running() {
        let mut sim = LaneSim::new(&running(ActivityKind::Thinking, ""));
        let before = render(&sim);
        ticks(&mut sim, 10);
        assert!(sim.scroll > 0);
        assert_ne!(before, render(&sim));
    }

    #[test]
    fn reading_spawns_coins() {
        let mut sim = LaneSim::new(&running(ActivityKind::Thinking, ""));
        sim.sync(&running(ActivityKind::Reading, "a.rs"));
        let coins = sim
            .effects
            .iter()
            .filter(|e| e.kind == EffectKind::Coin)
            .count();
        assert_eq!(coins, 1);
        let mut next = running(ActivityKind::Reading, "b.rs");
        next.last_change += 10;
        sim.sync(&next);
        let coins = sim
            .effects
            .iter()
            .filter(|e| e.kind == EffectKind::Coin)
            .count();
        assert_eq!(coins, 2, "one coin per file read");
        assert_eq!(sim.runner.state, RunnerState::Collecting);
    }

    #[test]
    fn same_activity_synced_twice_spawns_once() {
        let mut sim = LaneSim::new(&running(ActivityKind::Thinking, ""));
        let reading = running(ActivityKind::Reading, "a.rs");
        sim.sync(&reading);
        sim.sync(&reading);
        assert_eq!(sim.effects.len(), 1);
    }

    #[test]
    fn editing_spawns_brick_and_gem() {
        let mut sim = LaneSim::new(&running(ActivityKind::Thinking, ""));
        sim.sync(&running(ActivityKind::Editing, "lib.rs"));
        assert!(sim.effects.iter().any(|e| e.kind == EffectKind::Brick));
        assert_eq!(sim.runner.state, RunnerState::Smashing);
        ticks(&mut sim, 6);
        assert!(sim.effects.iter().any(|e| e.kind == EffectKind::Gem));
        assert!(
            !sim.effects.iter().any(|e| e.kind == EffectKind::Brick),
            "brick broken"
        );
    }

    #[test]
    fn passed_command_showers_sparks() {
        let mut sim = LaneSim::new(&running(ActivityKind::Thinking, ""));
        sim.sync(&running(ActivityKind::CommandPassed, "cargo"));
        assert!(
            sim.effects
                .iter()
                .filter(|e| e.kind == EffectKind::Spark)
                .count()
                >= 3
        );
        ticks(&mut sim, 200);
        assert!(sim.effects.is_empty(), "effects expire");
    }

    #[test]
    fn waiting_shows_gate_and_stops_scroll() {
        let mut sim = LaneSim::new(&running(ActivityKind::Thinking, ""));
        let mut waiting = lane("a", Tool::Claude, SessionStatus::Waiting, None);
        waiting.last_change += 1;
        sim.sync(&waiting);
        ticks(&mut sim, 30);
        let scroll = sim.scroll;
        ticks(&mut sim, 30);
        assert_eq!(sim.scroll, scroll);
        let px = render(&sim);
        let (gx, gy) = sim.gate_at(&px);
        assert_eq!(px.get(gx, gy), color(STEEL), "gate frame drawn");
    }

    #[test]
    fn finished_shows_beacon() {
        let mut sim = LaneSim::new(&running(ActivityKind::Thinking, ""));
        sim.sync(&running(ActivityKind::Finished, ""));
        ticks(&mut sim, 5);
        let px = render(&sim);
        let (bx, by) = sim.beacon_at(&px);
        assert_eq!(px.get(bx + 3, by), color(GLYPH_GOLD), "beacon tip drawn");
    }

    #[test]
    fn lost_signal_greys_lane() {
        let mut sim = LaneSim::new(&running(ActivityKind::Thinking, ""));
        let mut error = lane("a", Tool::Claude, SessionStatus::Error, None);
        error.last_change += 1;
        sim.sync(&error);
        let px = render(&sim);
        let greys = [
            color(SHADOW),
            color(GROUND_SHADOW),
            color(GROUND),
            color(STEEL),
        ];
        assert!(px.data.iter().all(|c| greys.contains(c)));
    }

    #[test]
    fn quiet_lane_sits_down() {
        let mut sim = LaneSim::new(&running(ActivityKind::Thinking, ""));
        ticks(&mut sim, IDLE_AFTER_TICKS + 1);
        assert_eq!(sim.runner.state, RunnerState::Idle);
        let mut active = running(ActivityKind::Editing, "x.rs");
        active.last_change += 5;
        sim.sync(&active);
        assert_eq!(sim.runner.state, RunnerState::Smashing);
    }

    #[test]
    fn new_usage_keeps_a_busy_runner_on_its_feet() {
        let mut sim = LaneSim::new(&running(ActivityKind::Thinking, ""));
        let mut lane = running(ActivityKind::Thinking, "");
        for step in 1..=4 {
            ticks(&mut sim, IDLE_AFTER_TICKS / 2);
            // Same activity, but the lane changed (tokens streamed in).
            lane.last_change += 1;
            sim.sync(&lane);
            assert_eq!(sim.runner.state, RunnerState::Running, "step {step}");
        }
    }

    #[test]
    fn a_long_command_stays_in_the_machine_section() {
        let mut sim = LaneSim::new(&running(ActivityKind::Thinking, ""));
        sim.sync(&running(ActivityKind::RunningCommand, "cargo"));
        ticks(&mut sim, IDLE_AFTER_TICKS * 4);
        assert_eq!(sim.runner.state, RunnerState::MachineSection);
    }

    #[test]
    fn short_or_tiny_buffers_do_not_panic() {
        let mut sim = LaneSim::new(&running(ActivityKind::Editing, "x"));
        ticks(&mut sim, 3);
        for (w, h) in [(0, 0), (1, 1), (5, 3), (160, 7)] {
            let mut px = Pixels::new(w, h, color(SKY));
            sim.render(&mut px);
        }
    }

    #[test]
    fn only_moving_lanes_are_animating() {
        let mut sim = LaneSim::new(&running(ActivityKind::Thinking, ""));
        assert!(sim.is_animating(), "running scrolls");
        let mut waiting = lane("a", Tool::Claude, SessionStatus::Waiting, None);
        waiting.last_change += 1;
        sim.sync(&waiting);
        ticks(&mut sim, 5);
        assert!(!sim.is_animating(), "standing at the gate");
        let mut edit = running(ActivityKind::Editing, "x.rs");
        edit.last_change += 2;
        sim.sync(&edit);
        assert!(sim.is_animating(), "jumping");
    }

    #[test]
    fn lane_snapshot_is_stable() {
        let mut sim = LaneSim::new(&running(ActivityKind::Thinking, ""));
        sim.sync(&running(ActivityKind::Editing, "main.rs"));
        ticks(&mut sim, 40);
        let text = to_text(&render(&sim));
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/lane_claude_tick40.txt");
        if std::env::var_os("RUNEWAY_UPDATE_SNAPSHOTS").is_some() {
            std::fs::create_dir_all(path.parent().expect("dir")).expect("mkdir");
            std::fs::write(&path, &text).expect("write snapshot");
        }
        let expected = std::fs::read_to_string(&path)
            .expect("snapshot missing: run with RUNEWAY_UPDATE_SNAPSHOTS=1");
        assert!(!text.contains('?'), "pixel outside the palette");
        assert_eq!(
            text, expected,
            "lane changed; rerun with RUNEWAY_UPDATE_SNAPSHOTS=1 if intended"
        );
    }
}
