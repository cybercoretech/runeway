//! Carries out a jump plan: runs `tmux` or `hyprctl` (no shell, output
//! captured so nothing reaches the screen) and returns a one-line notice.

use std::{
    io::Read,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

/// The longest a jump waits for tmux or hyprctl.
const TIMEOUT: Duration = Duration::from_secs(2);
const DONE: &str = "jumped to the agent's window";

use runeway::jump::JumpPlan;
use serde_json::Value;

/// Processes that own a Hyprland window; empty outside Hyprland or when
/// `hyprctl` is missing.
pub fn window_pids() -> Vec<u32> {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        return Vec::new();
    }
    let Ok(output) = run_with_timeout(Command::new("hyprctl").args(["clients", "-j"]), TIMEOUT)
    else {
        return Vec::new();
    };
    serde_json::from_slice::<Value>(&output.stdout)
        .ok()
        .and_then(|clients| {
            clients.as_array().map(|clients| {
                clients
                    .iter()
                    .filter_map(|client| client.get("pid")?.as_u64())
                    .filter_map(|pid| u32::try_from(pid).ok())
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// Do what `plan` says; the notice to show.
pub fn run(plan: &JumpPlan) -> String {
    if let JumpPlan::TmuxSwitch(name) | JumpPlan::TmuxAttachHint(name) = plan
        && let Some(notice) = tmux_notice(name, tmux_session_alive(name))
    {
        return notice;
    }
    match plan {
        JumpPlan::TmuxSwitch(name) => command(
            "tmux",
            &tmux_switch_args(name),
            format!("switched to tmux session {name}"),
        ),
        JumpPlan::TmuxAttachHint(name) => {
            format!("not inside tmux · attach with: tmux attach -t {name}")
        }
        JumpPlan::SameWindow { title, tty } => {
            let place = tty.as_deref().unwrap_or("another terminal");
            format!(
                "{title} is in another tab of this window ({place}); tabs can't be switched from outside, use your tab keys"
            )
        }
        JumpPlan::FocusWindow(pid) => focus_window(*pid),
        JumpPlan::NotFound(why) => why.clone(),
    }
}

/// Hyprland 0.57+ takes Lua dispatches; older versions take the classic
/// `focuswindow` form, so that is tried when the Lua form is rejected.
fn focus_window(pid: u32) -> String {
    let lua = format!("hl.dsp.focus({{ window = \"pid:{pid}\" }})");
    let lua_result = hyprctl(&["dispatch", &lua]);
    let classic = match &lua_result {
        Err(err) if err.contains("error") || err.contains("Invalid") => {
            Some(hyprctl(&["dispatch", "focuswindow", &format!("pid:{pid}")]))
        }
        _ => None,
    };
    focus_message(lua_result, classic)
}

/// Run hyprctl; it reports problems on stdout and often exits 0, so any
/// output other than "ok" counts as a failure.
fn hyprctl(args: &[&str]) -> Result<(), String> {
    let output = run_with_timeout(Command::new("hyprctl").args(args), TIMEOUT)?;
    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim();
    if output.status.success() && (text.is_empty() || text == "ok") {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let first = text
            .lines()
            .next()
            .or_else(|| stderr.lines().next())
            .unwrap_or("failed");
        Err(first.trim().to_string())
    }
}

fn command(program: &str, args: &[String], done: String) -> String {
    match run_with_timeout(Command::new(program).args(args), TIMEOUT) {
        Ok(output) if output.status.success() => done,
        Ok(output) => {
            let err = String::from_utf8_lossy(&output.stderr);
            let first = err.lines().next().unwrap_or("").trim();
            format!("{program} failed: {first}")
        }
        Err(err) => format!("{program}: {err}"),
    }
}

/// The notice when the tmux session behind a jump is gone, else `None`.
fn tmux_notice(name: &str, alive: bool) -> Option<String> {
    (!alive)
        .then(|| format!("tmux session {name} isn't running; start the agent again (agent-deck)"))
}

/// True when tmux has a session named exactly `name`.
fn tmux_session_alive(name: &str) -> bool {
    run_with_timeout(
        Command::new("tmux").args(["has-session", "-t", &format!("={name}")]),
        TIMEOUT,
    )
    .is_ok_and(|output| output.status.success())
}

/// `tmux switch-client` to exactly `name` (`=` stops prefix matching).
fn tmux_switch_args(name: &str) -> Vec<String> {
    vec!["switch-client".into(), "-t".into(), format!("={name}")]
}

/// The notice for a focus attempt: the Lua dispatch's result, and the
/// classic form's when the Lua one was rejected.
fn focus_message(lua: Result<(), String>, classic: Option<Result<(), String>>) -> String {
    match (lua, classic) {
        (Ok(()), _) | (Err(_), Some(Ok(()))) => DONE.to_string(),
        // The Lua error says more on Hyprland 0.57+ than the classic one.
        (Err(lua_err), _) => format!("hyprctl: {lua_err}"),
    }
}

/// Run `command`, giving up after `limit` so a stuck tool cannot freeze
/// the screen.
fn run_with_timeout(command: &mut Command, limit: Duration) -> Result<Output, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("could not run: {err}"))?;
    // Read both pipes while waiting, so a large output cannot fill a pipe
    // and stall the child.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            bytes
        })
    };
    let stdout = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let stderr = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < limit => thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {} ms", limit.as_millis()));
            }
            Err(err) => return Err(err.to_string()),
        }
    };
    Ok(Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_window_says_which_tab() {
        let message = run(&JumpPlan::SameWindow {
            title: "glyphsmith".into(),
            tty: Some("pts/4".into()),
        });
        assert!(message.contains("glyphsmith"), "{message}");
        assert!(message.contains("another tab"), "{message}");
        assert!(message.contains("pts/4"), "{message}");
    }

    #[test]
    fn dead_tmux_session_is_reported() {
        let dead = tmux_notice("agentdeck_x", false).expect("dead session");
        assert!(
            dead.contains("agentdeck_x") && dead.contains("isn't running"),
            "{dead}"
        );
        assert_eq!(tmux_notice("agentdeck_x", true), None);
    }

    #[test]
    fn tmux_target_is_exact() {
        assert_eq!(
            tmux_switch_args("deck_a"),
            ["switch-client", "-t", "=deck_a"]
        );
    }

    #[test]
    fn both_focus_forms_failing_reports_the_lua_error() {
        let message = focus_message(
            Err("error: lua parse failed".into()),
            Some(Err("Invalid dispatcher".into())),
        );
        assert!(message.contains("lua parse failed"), "{message}");
        assert_eq!(focus_message(Ok(()), None), "jumped to the agent's window");
        assert_eq!(
            focus_message(Err("error: x".into()), Some(Ok(()))),
            "jumped to the agent's window"
        );
        assert!(focus_message(Err("window not found".into()), None).contains("window not found"));
    }

    #[test]
    fn run_with_timeout_reads_large_output() {
        let output = run_with_timeout(
            Command::new("head").args(["-c", "200000", "/dev/zero"]),
            Duration::from_secs(2),
        )
        .expect("large output");
        assert_eq!(output.stdout.len(), 200_000);
    }

    #[test]
    fn run_with_timeout_gives_up() {
        let start = std::time::Instant::now();
        let result = run_with_timeout(Command::new("sleep").arg("5"), Duration::from_millis(200));
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );
        assert!(result.expect_err("timed out").contains("timed out"));
        let quick = run_with_timeout(Command::new("true").arg(""), Duration::from_secs(2));
        assert!(quick.is_ok());
    }
}
