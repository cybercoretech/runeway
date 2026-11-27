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
    /// Outside tmux: the session to attach to yourself.
    TmuxAttachHint(String),
    /// Focus the window owned by this process.
    FocusWindow(u32),
    /// The agent runs in the same window as Runeway (another tab): a window
    /// manager cannot switch tabs, so say where it is instead.
    SameWindow { title: String, tty: Option<String> },
    /// Why there is nowhere to go.
    NotFound(String),
}

/// `pts/N` for a `stat` tty number, or `None` when there is no terminal.
pub fn tty_name(tty_nr: u32) -> Option<String> {
    // Linux device numbers: major in bits 8-19, minor in bits 0-7 and 20-31.
    let major = (tty_nr >> 8) & 0xfff;
    let minor = (tty_nr & 0xff) | ((tty_nr >> 12) & 0xfff00);
    // Pseudo-terminal slaves use majors 136-143.
    (136..=143)
        .contains(&major)
        .then(|| format!("pts/{}", (major - 136) * 256 + minor))
}

/// Whether planning a jump to `session` needs the window list (asking the
/// window manager costs a call, so tmux jumps skip it).
pub fn needs_windows(session: &Session) -> bool {
    session.tmux_session.is_none()
}

/// The plan for `session`. `window_pids` are the processes that own a
/// window (empty when no window manager can be asked).
/// `own_pid` is Runeway's own process, to notice an agent in the same window.
pub fn plan(
    session: &Session,
    in_tmux: bool,
    proc_root: &Path,
    window_pids: &[u32],
    own_pid: u32,
) -> JumpPlan {
    if let Some(name) = &session.tmux_session {
        return if in_tmux {
            JumpPlan::TmuxSwitch(name.clone())
        } else {
            JumpPlan::TmuxAttachHint(name.clone())
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
    match window_of(proc_root, pid, window_pids) {
        // The agent's window is the one Runeway runs in: it is in another
        // tab, which no window manager can switch to.
        Some(window) if window_of(proc_root, own_pid, window_pids) == Some(window) => {
            JumpPlan::SameWindow {
                title: session.title.clone(),
                tty: tty(proc_root, pid).and_then(tty_name),
            }
        }
        Some(window) => JumpPlan::FocusWindow(window),
        None => not_found(session),
    }
}

/// The first of `pid` and its ancestors that owns a window.
fn window_of(proc_root: &Path, pid: u32, window_pids: &[u32]) -> Option<u32> {
    let mut current = pid;
    for _ in 0..MAX_DEPTH {
        if window_pids.contains(&current) {
            return Some(current);
        }
        match parent(proc_root, current) {
            Some(next) if next > 1 && next != current => current = next,
            _ => return None,
        }
    }
    None
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
            plan(&s, true, &proc.root, &[], 1),
            JumpPlan::TmuxSwitch("deck_a".into())
        );
    }

    #[test]
    fn tmux_session_outside_tmux_gives_attach_hint() {
        let mut s = session(Tool::Gemini, "/w/a");
        s.tmux_session = Some("deck_a".into());
        let proc = Proc::new();
        assert_eq!(
            plan(&s, false, &proc.root, &[], 1),
            JumpPlan::TmuxAttachHint("deck_a".into())
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
            plan(&s, false, &proc.root, &[500, 900], 1),
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
            plan(&s, false, &proc.root, &[900], 1),
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
            plan(&s, false, &proc.root, &[900], 1),
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
            plan(&s, false, &proc.root, &[900], 1),
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
            plan(&s, false, &proc.root, &[900], 1),
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
                plan(&s, false, &proc.root, &[900], 1),
                JumpPlan::FocusWindow(900),
                "{path}"
            );
        }
    }

    #[test]
    fn agent_in_another_tab_of_our_window_is_same_window() {
        let proc = Proc::new();
        proc.add(900, 1, "ghostty", None)
            .add_tty(910, 900, "zsh", None, 34820)
            .add_tty(920, 910, "claude", Some("/home/u"), 34820)
            .add_tty(940, 900, "zsh", None, 34832)
            .add_tty(950, 940, "runeway", Some("/home/u"), 34832);
        let mut s = session(Tool::Claude, "/home/u");
        s.pid = Some(920);
        assert_eq!(
            plan(&s, false, &proc.root, &[900], 950),
            JumpPlan::SameWindow {
                title: "app".into(),
                tty: Some("pts/4".into())
            }
        );
    }

    #[test]
    fn agent_in_its_own_window_still_focuses() {
        let proc = Proc::new();
        proc.add(900, 1, "ghostty", None)
            .add(920, 900, "claude", Some("/home/u"))
            .add(800, 1, "ghostty", None)
            .add(950, 800, "runeway", Some("/home/u"));
        let mut s = session(Tool::Claude, "/home/u");
        s.pid = Some(920);
        assert_eq!(
            plan(&s, false, &proc.root, &[800, 900], 950),
            JumpPlan::FocusWindow(900)
        );
    }

    #[test]
    fn tty_names_follow_linux_numbering() {
        assert_eq!(tty_name((136 << 8) | 4).as_deref(), Some("pts/4"));
        assert_eq!(tty_name((137 << 8) | 44).as_deref(), Some("pts/300"));
        assert_eq!(
            tty_name(44 | (136 << 8) | (256 << 12)).as_deref(),
            Some("pts/300"),
            "large minor numbers on one major"
        );
        assert_eq!(tty_name(0), None);
        assert_eq!(tty_name((4 << 8) | 1), None, "not a pseudo-terminal");
    }

    #[test]
    fn two_codex_in_one_folder_refuses_to_guess() {
        let proc = Proc::new();
        proc.add(900, 1, "ghostty", None)
            .add(920, 900, "codex", Some("/w/api"))
            .add(921, 900, "codex", Some("/w/api"));
        let s = session(Tool::Codex, "/w/api");
        match plan(&s, false, &proc.root, &[900], 1) {
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
        match plan(&s, false, &proc.root, &[900], 1) {
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
            plan(&s, false, &proc.root, &[900], 1),
            JumpPlan::NotFound(_)
        ));
        s.pid = Some(12345); // no such process
        assert!(matches!(
            plan(&s, false, &proc.root, &[900], 1),
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
            plan(&s, false, &proc.root, &[900], 1),
            JumpPlan::FocusWindow(900)
        );
    }
}
