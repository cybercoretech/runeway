//! Which runner state each agent activity becomes (the spec's table).

use crate::model::{ActivityKind, SessionStatus};

use super::runner::RunnerState;

/// The runner state for an agent doing `kind` with agent-deck `status`.
pub fn state_for(kind: ActivityKind, status: &SessionStatus) -> RunnerState {
    match status {
        SessionStatus::Error | SessionStatus::Stopped => return RunnerState::LostSignal,
        SessionStatus::Waiting => return RunnerState::AtGate,
        _ => {}
    }
    match kind {
        ActivityKind::Thinking => RunnerState::Running,
        ActivityKind::Reading | ActivityKind::Searching => RunnerState::Collecting,
        ActivityKind::Editing => RunnerState::Smashing,
        ActivityKind::RunningCommand => RunnerState::MachineSection,
        ActivityKind::CommandFailed => RunnerState::Stumbling,
        ActivityKind::CommandPassed | ActivityKind::SubagentStarted => RunnerState::Celebrating,
        ActivityKind::WaitingForPermission => RunnerState::AtGate,
        ActivityKind::Idle => RunnerState::Idle,
        ActivityKind::Finished => RunnerState::Finished,
        ActivityKind::Errored => RunnerState::LostSignal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running(kind: ActivityKind) -> RunnerState {
        state_for(kind, &SessionStatus::Running)
    }

    #[test]
    fn thinking_runs() {
        assert_eq!(running(ActivityKind::Thinking), RunnerState::Running);
    }

    #[test]
    fn reading_and_searching_collect_coins() {
        assert_eq!(running(ActivityKind::Reading), RunnerState::Collecting);
        assert_eq!(running(ActivityKind::Searching), RunnerState::Collecting);
    }

    #[test]
    fn editing_smashes_a_brick() {
        assert_eq!(running(ActivityKind::Editing), RunnerState::Smashing);
    }

    #[test]
    fn running_a_command_enters_a_machine_section() {
        assert_eq!(
            running(ActivityKind::RunningCommand),
            RunnerState::MachineSection
        );
    }

    #[test]
    fn failed_command_stumbles() {
        assert_eq!(running(ActivityKind::CommandFailed), RunnerState::Stumbling);
    }

    #[test]
    fn passed_command_celebrates() {
        assert_eq!(
            running(ActivityKind::CommandPassed),
            RunnerState::Celebrating
        );
    }

    #[test]
    fn waiting_for_permission_stops_at_the_gate() {
        assert_eq!(
            running(ActivityKind::WaitingForPermission),
            RunnerState::AtGate
        );
        assert_eq!(
            state_for(ActivityKind::Editing, &SessionStatus::Waiting),
            RunnerState::AtGate
        );
    }

    #[test]
    fn subagent_started_celebrates() {
        assert_eq!(
            running(ActivityKind::SubagentStarted),
            RunnerState::Celebrating
        );
    }

    #[test]
    fn idle_sits() {
        assert_eq!(running(ActivityKind::Idle), RunnerState::Idle);
    }

    #[test]
    fn finished_reaches_the_beacon() {
        assert_eq!(running(ActivityKind::Finished), RunnerState::Finished);
    }

    #[test]
    fn errors_lose_signal() {
        assert_eq!(running(ActivityKind::Errored), RunnerState::LostSignal);
        assert_eq!(
            state_for(ActivityKind::Editing, &SessionStatus::Error),
            RunnerState::LostSignal
        );
        assert_eq!(
            state_for(ActivityKind::Thinking, &SessionStatus::Stopped),
            RunnerState::LostSignal
        );
    }
}
