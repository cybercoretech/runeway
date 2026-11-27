//! Carries out a jump plan: runs `tmux` or `hyprctl` (no shell, output
//! captured so nothing reaches the screen) and returns a one-line notice.

use std::process::Command;

use runeway::jump::JumpPlan;
use serde_json::Value;

/// Processes that own a Hyprland window; empty outside Hyprland or when
/// `hyprctl` is missing.
pub fn window_pids() -> Vec<u32> {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
        return Vec::new();
    }
    let Ok(output) = Command::new("hyprctl").args(["clients", "-j"]).output() else {
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
    match plan {
        JumpPlan::TmuxSwitch(name) => command(
            "tmux",
            &["switch-client", "-t", name],
            format!("switched to tmux session {name}"),
        ),
        JumpPlan::TmuxAttachHint(hint) => format!("not inside tmux · attach with: {hint}"),
        JumpPlan::FocusWindow(pid) => focus_window(*pid),
        JumpPlan::NotFound(why) => why.clone(),
    }
}

/// Hyprland 0.57+ takes Lua dispatches; older versions take the classic
/// `focuswindow` form, so that is tried when the Lua form is rejected.
fn focus_window(pid: u32) -> String {
    const DONE: &str = "jumped to the agent's window";
    let lua = format!("hl.dsp.focus({{ window = \"pid:{pid}\" }})");
    match hyprctl(&["dispatch", &lua]) {
        Ok(()) => DONE.to_string(),
        Err(lua_err) if lua_err.contains("error") || lua_err.contains("Invalid") => {
            match hyprctl(&["dispatch", "focuswindow", &format!("pid:{pid}")]) {
                Ok(()) => DONE.to_string(),
                Err(err) => format!("hyprctl: {err}"),
            }
        }
        Err(err) => format!("hyprctl: {err}"),
    }
}

/// Run hyprctl; it reports problems on stdout and often exits 0, so any
/// output other than "ok" counts as a failure.
fn hyprctl(args: &[&str]) -> Result<(), String> {
    let output = Command::new("hyprctl")
        .args(args)
        .output()
        .map_err(|err| format!("could not run: {err}"))?;
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

fn command(program: &str, args: &[&str], done: String) -> String {
    match Command::new(program).args(args).output() {
        Ok(output) if output.status.success() => done,
        Ok(output) => {
            let err = String::from_utf8_lossy(&output.stderr);
            let first = err.lines().next().unwrap_or("").trim();
            format!("{program} failed: {first}")
        }
        Err(err) => format!("{program} could not run: {err}"),
    }
}
