//! Where `Enter` takes you: the agent's tmux session, or the window that
//! runs it. Pure: process information is read from a proc root passed in,
//! and the window list comes from the caller.

use std::path::Path;

use crate::model::{Session, Tool};

/// How many parents to walk before giving up (guards against loops).
const MAX_DEPTH: usize = 64;

/// What to do to reach an agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JumpPlan {
    /// `tmux switch-client -t <name>` (Runeway runs inside tmux).
    TmuxSwitch(String),
    /// Outside tmux: the command to attach yourself.
    TmuxAttachHint(String),
    /// Focus the window owned by this process.
    FocusWindow(u32),
    /// Why there is nowhere to go.
    NotFound(String),
}

/// Whether planning a jump to `session` needs the window list (asking the
/// window manager costs a call, so tmux jumps skip it).
pub fn needs_windows(session: &Session) -> bool {
    session.tmux_session.is_none()
}

/// The plan for `session`. `window_pids` are the processes that own a
/// window (empty when no window manager can be asked).
pub fn plan(session: &Session, in_tmux: bool, proc_root: &Path, window_pids: &[u32]) -> JumpPlan {
    if let Some(name) = &session.tmux_session {
        return if in_tmux {
            JumpPlan::TmuxSwitch(name.clone())
        } else {
            JumpPlan::TmuxAttachHint(format!("tmux attach -t {name}"))
        };
    }
    let pid = match session.pid {
        Some(pid) => pid,
        None => {
            let names: &[&str] = match session.tool {
                Tool::Codex => &["codex"],
                Tool::OpenCode => &["opencode", "opencode.exe"],
                _ => &[],
            };
            let found = processes_in(proc_root, names, &session.path);
            match found.as_slice() {
                [pid] => *pid,
                [] => return not_found(session),
                many => {
                    return JumpPlan::NotFound(format!(
                        "{} {} agents in {}; can't tell which",
                        many.len(),
                        session.tool.name(),
                        session.path.display()
                    ));
                }
            }
        }
    };
    let mut current = pid;
    for _ in 0..MAX_DEPTH {
        if window_pids.contains(&current) {
            return JumpPlan::FocusWindow(current);
        }
        match parent(proc_root, current) {
            Some(next) if next > 1 && next != current => current = next,
            _ => break,
        }
    }
    not_found(session)
}

fn not_found(session: &Session) -> JumpPlan {
    JumpPlan::NotFound(format!("no window found for {}", session.title))
}

/// The parent process id: field 4 of `stat`, read after the last `)` so a
/// name with spaces or parentheses cannot shift the fields.
fn parent(proc_root: &Path, pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(proc_root.join(pid.to_string()).join("stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(1)?.parse().ok()
}

/// Field 7 of `stat`: the controlling terminal, 0 for background processes.
fn tty(proc_root: &Path, pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(proc_root.join(pid.to_string()).join("stat")).ok()?;
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(4)?.parse().ok()
}

/// True when one of `pid`'s ancestors is in `others`.
fn descends_from(proc_root: &Path, pid: u32, others: &[u32]) -> bool {
    let mut current = pid;
    for _ in 0..MAX_DEPTH {
        match parent(proc_root, current) {
            Some(next) if next > 1 && next != current => {
                if others.contains(&next) {
                    return true;
                }
                current = next;
            }
            _ => return false,
        }
    }
    false
}

/// Agents named one of `names` whose working directory is `cwd`: on a
/// terminal (background services are not agents), and counting a wrapper
/// and the process it starts once.
fn processes_in(proc_root: &Path, names: &[&str], cwd: &Path) -> Vec<u32> {
    let found = matching(proc_root, names, cwd);
    found
        .iter()
        .copied()
        .filter(|pid| tty(proc_root, *pid).is_some_and(|tty| tty != 0))
        .filter(|pid| !descends_from(proc_root, *pid, &found))
        .collect()
}

/// `path` with symlinks and trailing slashes resolved, when it exists.
fn resolved(path: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.components().collect())
}

/// Processes named one of `names` whose working directory is `cwd`.
fn matching(proc_root: &Path, names: &[&str], cwd: &Path) -> Vec<u32> {
    let cwd = resolved(cwd);
    if names.is_empty() {
        return Vec::new();
    }
    let mut pids: Vec<u32> = std::fs::read_dir(proc_root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            let dir = proc_root.join(pid.to_string());
            let comm = std::fs::read_to_string(dir.join("comm")).unwrap_or_default();
            names.contains(&comm.trim())
                && std::fs::read_link(dir.join("cwd")).is_ok_and(|c| resolved(&c) == cwd)
        })
        .collect();
    pids.sort_unstable();
    pids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{SessionStatus, Tool};
    use std::path::PathBuf;

    struct Proc {
        _dir: tempfile::TempDir,
        root: PathBuf,
    }

    impl Proc {
        fn new() -> Proc {
            let dir = tempfile::tempdir().expect("proc");
            let root = dir.path().to_path_buf();
            Proc { _dir: dir, root }
        }

        /// A process on a terminal, with its parent, name and (optionally)
        /// working directory.
        fn add(&self, pid: u32, ppid: u32, comm: &str, cwd: Option<&str>) -> &Proc {
            self.add_tty(pid, ppid, comm, cwd, 34816)
        }

        /// A background process: no controlling terminal.
        fn add_daemon(&self, pid: u32, ppid: u32, comm: &str, cwd: Option<&str>) -> &Proc {
            self.add_tty(pid, ppid, comm, cwd, 0)
        }

        fn add_tty(&self, pid: u32, ppid: u32, comm: &str, cwd: Option<&str>, tty: u32) -> &Proc {
            let dir = self.root.join(pid.to_string());
            std::fs::create_dir_all(&dir).expect("pid dir");
            std::fs::write(
                dir.join("stat"),
                format!("{pid} ({comm}) S {ppid} {pid} {pid} {tty} 0"),
            )
            .expect("stat");
            std::fs::write(dir.join("comm"), format!("{comm}\n")).expect("comm");
            if let Some(cwd) = cwd {
                std::os::unix::fs::symlink(cwd, dir.join("cwd")).expect("cwd");
            }
            self
        }
    }

    fn session(tool: Tool, path: &str) -> Session {
        Session {
            id: "x".into(),
            title: "app".into(),
            path: PathBuf::from(path),
            tool,
            status: SessionStatus::Running,
            tmux_session: None,
            transcript: None,
            pid: None,
        }
    }

    #[test]
    fn only_non_tmux_jumps_need_windows() {
        let mut s = session(Tool::Claude, "/w");
        assert!(needs_windows(&s));
        s.tmux_session = Some("deck".into());
        assert!(!needs_windows(&s));
    }

    #[test]
    fn tmux_session_inside_tmux_switches() {
        let mut s = session(Tool::Gemini, "/w/a");
        s.tmux_session = Some("deck_a".into());
        let proc = Proc::new();
        assert_eq!(
            plan(&s, true, &proc.root, &[]),
            JumpPlan::TmuxSwitch("deck_a".into())
        );
    }

    #[test]
    fn tmux_session_outside_tmux_gives_attach_hint() {
        let mut s = session(Tool::Gemini, "/w/a");
        s.tmux_session = Some("deck_a".into());
        let proc = Proc::new();
        assert_eq!(
            plan(&s, false, &proc.root, &[]),
            JumpPlan::TmuxAttachHint("tmux attach -t deck_a".into())
        );
    }

    #[test]
    fn claude_pid_walks_up_to_its_window() {
        let proc = Proc::new();
        proc.add(900, 1, "ghostty", None)
            .add(910, 900, "zsh", None)
            .add(920, 910, "claude", Some("/home/u"));
        let mut s = session(Tool::Claude, "/home/u");
        s.pid = Some(920);
        assert_eq!(
            plan(&s, false, &proc.root, &[500, 900]),
            JumpPlan::FocusWindow(900)
        );
    }

    #[test]
    fn codex_found_by_cwd_walks_up_to_its_window() {
        let proc = Proc::new();
        proc.add(900, 1, "ghostty", None)
            .add(920, 900, "codex", Some("/w/api"))
            .add(930, 900, "codex", Some("/w/other"))
            .add(940, 900, "claude", Some("/w/api"));
        let s = session(Tool::Codex, "/w/api");
        assert_eq!(
            plan(&s, false, &proc.root, &[900]),
            JumpPlan::FocusWindow(900)
        );
    }

    #[test]
    fn opencode_exe_counts_as_opencode() {
        let proc = Proc::new();
        proc.add(900, 1, "ghostty", None)
            .add(920, 900, "opencode.exe", Some("/w/site"));
        let s = session(Tool::OpenCode, "/w/site");
        assert_eq!(
            plan(&s, false, &proc.root, &[900]),
            JumpPlan::FocusWindow(900)
        );
    }

    #[test]
    fn daemon_without_a_terminal_is_not_an_agent() {
        let proc = Proc::new();
        proc.add(900, 1, "ghostty", None)
            .add(920, 900, "codex", Some("/w/api"))
            .add_daemon(930, 1, "codex", Some("/w/api"))
            .add_daemon(931, 1, "opencode.exe", Some("/w/api"));
        let s = session(Tool::Codex, "/w/api");
        assert_eq!(
            plan(&s, false, &proc.root, &[900]),
            JumpPlan::FocusWindow(900)
        );
    }

    #[test]
    fn wrapper_and_its_child_count_once() {
        let proc = Proc::new();
        proc.add(900, 1, "ghostty", None)
            .add(920, 900, "opencode", Some("/w/site"))
            .add(921, 920, "opencode.exe", Some("/w/site"));
        let s = session(Tool::OpenCode, "/w/site");
        assert_eq!(
            plan(&s, false, &proc.root, &[900]),
            JumpPlan::FocusWindow(900)
        );
    }

    #[test]
    fn folder_reached_through_a_symlink_or_trailing_slash_matches() {
        let dirs = tempfile::tempdir().expect("dirs");
        let real = dirs.path().join("real");
        std::fs::create_dir_all(&real).expect("real");
        let link = dirs.path().join("link");
        std::os::unix::fs::symlink(&real, &link).expect("link");
        let proc = Proc::new();
        proc.add(900, 1, "ghostty", None).add(
            920,
            900,
            "codex",
            Some(real.to_str().expect("utf8")),
        );
        for path in [link.display().to_string(), format!("{}/", real.display())] {
            let s = session(Tool::Codex, &path);
            assert_eq!(
                plan(&s, false, &proc.root, &[900]),
                JumpPlan::FocusWindow(900),
                "{path}"
            );
        }
    }

    #[test]
    fn two_codex_in_one_folder_refuses_to_guess() {
        let proc = Proc::new();
        proc.add(900, 1, "ghostty", None)
            .add(920, 900, "codex", Some("/w/api"))
            .add(921, 900, "codex", Some("/w/api"));
        let s = session(Tool::Codex, "/w/api");
        match plan(&s, false, &proc.root, &[900]) {
            JumpPlan::NotFound(why) => assert!(why.contains("2 codex agents"), "{why}"),
            other => panic!("guessed: {other:?}"),
        }
    }

    #[test]
    fn no_window_ancestor_is_not_found() {
        let proc = Proc::new();
        proc.add(920, 1, "claude", Some("/w"));
        let mut s = session(Tool::Claude, "/w");
        s.pid = Some(920);
        match plan(&s, false, &proc.root, &[900]) {
            JumpPlan::NotFound(why) => assert!(why.contains("no window"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parent_loop_or_missing_stat_does_not_hang() {
        let proc = Proc::new();
        proc.add(920, 920, "claude", Some("/w"));
        let mut s = session(Tool::Claude, "/w");
        s.pid = Some(920);
        assert!(matches!(
            plan(&s, false, &proc.root, &[900]),
            JumpPlan::NotFound(_)
        ));
        s.pid = Some(12345); // no such process
        assert!(matches!(
            plan(&s, false, &proc.root, &[900]),
            JumpPlan::NotFound(_)
        ));
    }

    #[test]
    fn comm_with_spaces_and_parens_parses() {
        let proc = Proc::new();
        proc.add(900, 1, "ghostty", None)
            .add(920, 900, "weird (name) x", Some("/w"));
        let mut s = session(Tool::Claude, "/w");
        s.pid = Some(920);
        assert_eq!(
            plan(&s, false, &proc.root, &[900]),
            JumpPlan::FocusWindow(900)
        );
    }
}
