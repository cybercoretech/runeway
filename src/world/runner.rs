//! A lane's runner: its state, pose and simple physics. Time is ticks.

use super::sprites::Pose;

/// What the runner is doing, driven by the agent's activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerState {
    Running,
    Collecting,
    Smashing,
    MachineSection,
    Stumbling,
    Celebrating,
    AtGate,
    Idle,
    Finished,
    LostSignal,
}

/// Ticks a stumble lasts before the runner gets going again.
pub const STUMBLE_TICKS: u32 = 12;
/// Ticks a celebration lasts.
pub const CHEER_TICKS: u32 = 20;
/// Upward speed at the start of a jump, in px per tick.
pub const JUMP_SPEED: i32 = 3;

/// The runner. `x` is the offset from its usual spot (negative while
/// knocked back); `y` is the height of its feet above the ground.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runner {
    pub state: RunnerState,
    pub pose: Pose,
    pub frame: usize,
    pub x: i32,
    pub y: i32,
    pub vy: i32,
    pub state_ticks: u32,
}

impl Default for Runner {
    fn default() -> Self {
        Runner::new()
    }
}

impl Runner {
    pub fn new() -> Runner {
        Runner {
            state: RunnerState::Running,
            pose: Pose::Run,
            frame: 0,
            x: 0,
            y: 0,
            vy: 0,
            state_ticks: 0,
        }
    }

    /// Switch state; a jump starts here so a repeated smash jumps again.
    pub fn set_state(&mut self, state: RunnerState) {
        self.state = state;
        self.state_ticks = 0;
        if state == RunnerState::Smashing && self.y == 0 {
            self.vy = JUMP_SPEED;
        }
        self.pose = pose_for(state, self.y);
    }

    /// Advance one tick: physics, timed states, animation frame.
    pub fn tick(&mut self) {
        self.state_ticks = self.state_ticks.saturating_add(1);
        // Gravity: rise while vy > 0, then fall back to the ground.
        if self.y > 0 || self.vy > 0 {
            self.y = (self.y + self.vy).max(0);
            self.vy -= 1;
            if self.y == 0 {
                self.vy = 0;
            }
        }
        match self.state {
            RunnerState::Stumbling => {
                if self.state_ticks <= STUMBLE_TICKS / 2 {
                    self.x -= 1;
                }
                if self.state_ticks >= STUMBLE_TICKS {
                    self.set_state(RunnerState::Running);
                }
            }
            RunnerState::Smashing if self.y == 0 && self.vy == 0 => {
                self.set_state(RunnerState::Running);
            }
            RunnerState::Celebrating if self.state_ticks >= CHEER_TICKS => {
                self.set_state(RunnerState::Running);
            }
            _ => {}
        }
        // Walk back to the usual spot after a knock-back.
        if self.state != RunnerState::Stumbling && self.x < 0 {
            self.x += 1;
        }
        self.pose = pose_for(self.state, self.y);
        if scroll_speed(self.state) > 0 && self.state_ticks.is_multiple_of(4) {
            self.frame = self.frame.wrapping_add(1);
        }
    }
}

fn pose_for(state: RunnerState, y: i32) -> Pose {
    match state {
        RunnerState::Smashing if y > 0 => Pose::Jump,
        RunnerState::Running
        | RunnerState::Collecting
        | RunnerState::Smashing
        | RunnerState::MachineSection => Pose::Run,
        RunnerState::Stumbling => Pose::Stumble,
        RunnerState::Celebrating | RunnerState::Finished => Pose::Cheer,
        RunnerState::AtGate | RunnerState::LostSignal => Pose::Stand,
        RunnerState::Idle => Pose::Sit,
    }
}

/// How fast the level scrolls past, in px per tick.
pub fn scroll_speed(state: RunnerState) -> i32 {
    match state {
        RunnerState::Running
        | RunnerState::Collecting
        | RunnerState::Smashing
        | RunnerState::MachineSection
        | RunnerState::Celebrating => 1,
        RunnerState::Stumbling
        | RunnerState::AtGate
        | RunnerState::Idle
        | RunnerState::Finished
        | RunnerState::LostSignal => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stumble_returns_to_running_after_its_ticks() {
        let mut runner = Runner::new();
        runner.set_state(RunnerState::Stumbling);
        assert_eq!(runner.pose, Pose::Stumble);
        runner.tick();
        assert!(runner.x < 0, "knocked back");
        for _ in 1..STUMBLE_TICKS {
            assert_eq!(runner.state, RunnerState::Stumbling);
            runner.tick();
        }
        assert_eq!(runner.state, RunnerState::Running);
        assert_eq!(runner.pose, Pose::Run);
        for _ in 0..STUMBLE_TICKS {
            runner.tick();
        }
        assert_eq!(runner.x, 0, "back at its spot");
    }

    #[test]
    fn smash_jump_lands_back_on_the_ground() {
        let mut runner = Runner::new();
        runner.set_state(RunnerState::Smashing);
        let mut peak = 0;
        for _ in 0..30 {
            runner.tick();
            peak = peak.max(runner.y);
            assert!(runner.y >= 0, "never below the ground");
        }
        assert!(peak >= 4, "jumped (peak {peak})");
        assert_eq!(runner.y, 0);
        assert_eq!(runner.vy, 0);
        assert_eq!(runner.state, RunnerState::Running);
    }

    #[test]
    fn celebration_ends_after_its_ticks() {
        let mut runner = Runner::new();
        runner.set_state(RunnerState::Celebrating);
        assert_eq!(runner.pose, Pose::Cheer);
        for _ in 0..CHEER_TICKS {
            runner.tick();
        }
        assert_eq!(runner.state, RunnerState::Running);
    }

    #[test]
    fn gate_and_idle_do_not_scroll() {
        for state in [
            RunnerState::AtGate,
            RunnerState::Idle,
            RunnerState::Finished,
            RunnerState::LostSignal,
        ] {
            assert_eq!(scroll_speed(state), 0, "{state:?}");
        }
        assert!(scroll_speed(RunnerState::Running) > 0);
    }

    #[test]
    fn run_frames_advance_while_running() {
        let mut runner = Runner::new();
        let first = runner.frame;
        for _ in 0..8 {
            runner.tick();
        }
        assert_ne!(runner.frame, first);
    }

    #[test]
    fn resting_states_hold_their_pose() {
        let cases = [
            (RunnerState::AtGate, Pose::Stand),
            (RunnerState::Idle, Pose::Sit),
            (RunnerState::Finished, Pose::Cheer),
            (RunnerState::LostSignal, Pose::Stand),
        ];
        for (state, pose) in cases {
            let mut runner = Runner::new();
            runner.set_state(state);
            for _ in 0..50 {
                runner.tick();
            }
            assert_eq!((runner.state, runner.pose), (state, pose));
        }
    }
}
