//! agent-deck: the list of sessions, from `agent-deck ls --json`.

use std::{
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde::Deserialize;

use crate::{
    Error, Result,
    model::{Session, SessionStatus, Tool},
};

/// How long `agent-deck ls --json` may take before it is stopped.
const TIMEOUT: Duration = Duration::from_secs(5);

/// Anything that can list the current agent sessions.
pub trait SessionLister {
    fn list(&self) -> Result<Vec<Session>>;
}

/// Lists sessions by running `<program> ls --json` (no shell).
#[derive(Debug, Clone)]
pub struct AgentDeck {
    program: PathBuf,
}

impl AgentDeck {
    pub fn new(program: PathBuf) -> AgentDeck {
        AgentDeck { program }
    }
}

impl SessionLister for AgentDeck {
    fn list(&self) -> Result<Vec<Session>> {
        let fail = |what: String| Error::AgentDeck(format!("{} {what}", self.program.display()));
        let mut child = Command::new(&self.program)
            .args(["ls", "--json"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| fail(format!("could not start: {err}")))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| fail("gave no output".to_string()))?;
        // Read on another thread so a large listing cannot fill the pipe and
        // stall the child while we wait for it.
        let reader = thread::spawn(move || {
            let mut out = String::new();
            stdout.read_to_string(&mut out).map(|_| out)
        });
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() < TIMEOUT => thread::sleep(Duration::from_millis(50)),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(fail(format!("took longer than {}s", TIMEOUT.as_secs())));
                }
                Err(err) => return Err(fail(format!("could not be waited on: {err}"))),
            }
        };
        let output = reader
            .join()
            .map_err(|_| fail("output reader panicked".to_string()))?
            .map_err(|err| fail(format!("output unreadable: {err}")))?;
        if !status.success() {
            return Err(fail(format!("failed with {status} (exit)")));
        }
        parse_sessions(&output)
    }
}

#[derive(Debug, Deserialize)]
struct RawSession {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    path: PathBuf,
    #[serde(default)]
    tool: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    tmux_session: String,
    #[serde(default)]
    archived: bool,
}

/// Parse `agent-deck ls --json` output, leaving out archived sessions.
pub fn parse_sessions(json: &str) -> Result<Vec<Session>> {
    let raw: Vec<RawSession> = serde_json::from_str(json).map_err(|source| Error::Json {
        what: "agent-deck output",
        source,
    })?;
    Ok(raw
        .into_iter()
        .filter(|session| !session.archived)
        .map(|session| Session {
            tool: Tool::from_agent_deck(&session.tool),
            status: SessionStatus::from_agent_deck(&session.status),
            tmux_session: (!session.tmux_session.is_empty()).then_some(session.tmux_session),
            id: session.id,
            title: session.title,
            path: session.path,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{SessionStatus, Tool};
    use std::path::Path;

    fn fixture() -> String {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agentdeck/ls.json"),
        )
        .expect("fixture")
    }

    #[test]
    fn parses_fixture_sessions_and_skips_archived() {
        let sessions = parse_sessions(&fixture()).expect("parse");
        let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["a1", "b2", "c3"]);
        assert_eq!(sessions[0].tool, Tool::Claude);
        assert_eq!(sessions[0].status, SessionStatus::Running);
        assert_eq!(sessions[0].path, Path::new("/work/app"));
        assert_eq!(sessions[0].tmux_session.as_deref(), Some("agentdeck_app_1"));
        assert_eq!(sessions[1].tool, Tool::Codex);
        assert_eq!(sessions[1].status, SessionStatus::Waiting);
        assert_eq!(sessions[2].tool, Tool::Gemini);
        assert_eq!(sessions[2].tmux_session, None);
    }

    #[test]
    fn invalid_json_is_an_error() {
        let err = parse_sessions("{not json").expect_err("invalid");
        assert!(err.to_string().contains("agent-deck"));
    }

    #[test]
    fn missing_program_is_an_agentdeck_error_naming_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("no-such-agent-deck");
        let err = AgentDeck::new(program.clone()).list().expect_err("missing");
        let message = err.to_string();
        assert!(message.starts_with("agent-deck:"), "{message}");
        assert!(message.contains("no-such-agent-deck"), "{message}");
    }

    #[cfg(unix)]
    #[test]
    fn runs_program_and_parses_its_output() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("fake-agent-deck");
        let json = dir.path().join("ls.json");
        std::fs::write(&json, fixture()).expect("json");
        std::fs::write(
            &program,
            format!(
                "#!/bin/sh\n[ \"$1 $2\" = \"ls --json\" ] || exit 9\ncat '{}'\n",
                json.display()
            ),
        )
        .expect("script");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let sessions = AgentDeck::new(program).list().expect("list");
        assert_eq!(sessions.len(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn nonzero_exit_is_an_error() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let program = dir.path().join("broken");
        std::fs::write(&program, "#!/bin/sh\necho oops >&2\nexit 3\n").expect("script");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let err = AgentDeck::new(program).list().expect_err("fails");
        assert!(err.to_string().contains("exit"), "{err}");
    }
}
