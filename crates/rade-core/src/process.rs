//! Signal a Pane's process (R2 `close` / `close --force`). Ghostty exposes no
//! pid, so a Pane's process is our direct child whose environment has
//! `RADE_PANE_ID=<pane>`; Ghostty `setsid()`s each child, so its pid is also
//! the id of the process group the Agent and its tools run in.

pub use libc::{SIGHUP, SIGKILL};

use crate::preset::Invocation;
use std::path::Path;

/// `close_range` flag (linux/close_range.h).
const CLOSE_RANGE_CLOEXEC: libc::c_uint = 1 << 2;

/// In a child between fork and exec (async-signal-safe): every fd from 3
/// up closes on exec, so the app's GPU and other fds that libraries open
/// without `O_CLOEXEC` don't leak into language servers or
/// probes (M28 acceptance, Low).
pub fn no_inherit() {
    // SAFETY: a plain syscall on fd numbers; failure leaves fds as they were.
    unsafe {
        libc::syscall(
            libc::SYS_close_range,
            3 as libc::c_uint,
            libc::c_uint::MAX,
            CLOSE_RANGE_CLOEXEC,
        );
    }
}

/// Run `f` with the soft `RLIMIT_MEMLOCK` at 0, then restore it (M30,
/// ADR-0031). Around `ghostty_init`: libxev probes io_uring there, the
/// probe fails, and every Ghostty event loop uses epoll. io_uring rings
/// are charged to the *user's* locked-memory budget (8 MB, shared by all
/// of the user's processes); two rings per Terminal used it up after
/// ~120 Terminals across all RADE instances, and new Terminals then
/// failed with `SystemResources`.
pub fn without_locked_memory<T>(f: impl FnOnce() -> T) -> T {
    let mut old = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: get/setrlimit on our own process; lowering the soft limit
    // and raising it back up to the unchanged hard limit is always allowed.
    let got = unsafe { libc::getrlimit(libc::RLIMIT_MEMLOCK, &mut old) } == 0;
    if got {
        let zero = libc::rlimit {
            rlim_cur: 0,
            rlim_max: old.rlim_max,
        };
        unsafe { libc::setrlimit(libc::RLIMIT_MEMLOCK, &zero) };
    }
    let r = f();
    if got {
        unsafe { libc::setrlimit(libc::RLIMIT_MEMLOCK, &old) };
    }
    r
}

/// In the app: mark every open fd from 3 up close-on-exec (before Ghostty
/// starts a Terminal's process, which inherits the rest). Returns how many
/// it changed.
pub fn cloexec_all() -> usize {
    let Ok(dir) = std::fs::read_dir("/proc/self/fd") else {
        return 0;
    };
    let fds: Vec<i32> = dir
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
        .filter(|&fd| fd >= 3)
        .collect();
    let mut n = 0;
    for fd in fds {
        // SAFETY: fcntl on a number that may be closed by now only fails.
        unsafe {
            let f = libc::fcntl(fd, libc::F_GETFD);
            if f >= 0
                && f & libc::FD_CLOEXEC == 0
                && libc::fcntl(fd, libc::F_SETFD, f | libc::FD_CLOEXEC) == 0
            {
                n += 1;
            }
        }
    }
    n
}

/// Ancestors checked when looking for the Agent above a hook (hook → `sh -c`
/// → Agent is 2; wrappers add a few).
const MAX_ANCESTORS: usize = 6;

fn nul_split(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect()
}

/// Parent pid from `<proc>/<pid>/stat` (the comm field may hold spaces
/// and parentheses, so fields are counted after the last `)`).
fn parent_of(proc: &Path, pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(proc.join(format!("{pid}/stat"))).ok()?;
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// The Agent a hook runs under (R11.2): walks up from `start` (the hook's
/// parent) through at most [`MAX_ANCESTORS`] processes to the first whose
/// argv[0] basename is `binary`, or, for interpreter-wrapped CLIs (`node
/// …/codex`, `bash …/fake-agent.sh`), whose first non-flag argument's is
/// (not after `-c`).
/// Returns its argv from that word on, and those `config_env` variables that
/// are set in its environment. `proc` is `/proc` (a fake tree in tests).
/// Never fails: unreadable processes end the walk with `None`.
pub fn find_invocation(
    proc: &Path,
    start: u32,
    binary: &str,
    config_env: &[String],
) -> Option<Invocation> {
    let mut pid = start;
    for _ in 0..MAX_ANCESTORS {
        if pid <= 1 {
            return None;
        }
        let dir = proc.join(pid.to_string());
        let argv = nul_split(&std::fs::read(dir.join("cmdline")).ok()?);
        if let Some(at) = program_at(&argv, binary) {
            let env = nul_split(&std::fs::read(dir.join("environ")).unwrap_or_default())
                .into_iter()
                .filter_map(|e| {
                    let (k, v) = e.split_once('=')?;
                    config_env
                        .iter()
                        .any(|c| c == k)
                        .then(|| (k.to_string(), v.to_string()))
                })
                .collect();
            return Some(Invocation {
                argv: argv[at..].to_vec(),
                env,
            });
        }
        pid = parent_of(proc, pid)?;
    }
    None
}

/// Where `binary` starts in `argv`: argv[0] by basename, or, for
/// interpreter-wrapped CLIs, the first non-flag argument (not after `-c`:
/// `sh -c "<script>"` runs a command string, not a file).
fn program_at(argv: &[String], binary: &str) -> Option<usize> {
    let base = |a: &str| a.rsplit('/').next() == Some(binary);
    if argv.first().is_some_and(|a| base(a)) {
        return Some(0);
    }
    argv.iter()
        .skip(1)
        .position(|a| !a.starts_with('-'))
        .map(|i| i + 1)
        .filter(|i| base(&argv[*i]) && !argv[1..*i].iter().any(|a| a == "-c"))
}

/// Ancestors checked for an Agent above the hook's own Agent.
const MAX_NESTING: usize = 64;

/// Programs that are a Pane's login shell when they are the outermost
/// match in the Pane: never an Agent there, even when a Preset's command is one of
/// them (`[agents.lead] command = "bash"` would otherwise make every Agent
/// started by hand in a `bash` Pane look nested).
const SHELLS: [&str; 6] = ["sh", "bash", "zsh", "fish", "dash", "ksh"];

/// Whether a hook at `start` runs under an Agent started by another Agent
/// (Claude Code running `codex exec` in its Bash tool): two Agents among its
/// ancestors, one of `binaries` each. A process of the same binary right
/// above another (an npm wrapper over the native CLI) is the same Agent.
/// The walk ends at the first process whose environment lacks one of
/// `pane` (`RADE_PANE_ID=…`, `RADE_SOCKET=…`): the app and whatever started
/// it (an Agent that launched RADE) are outside the Pane. The outermost
/// Agent is not one when it is a shell ([`SHELLS`]): that is the Pane's own
/// shell (maybe under a `sh -c` wrapper) an Agent was started from.
/// Unreadable processes end the walk.
pub fn nested_agent(proc: &Path, start: u32, binaries: &[&str], pane: &[String]) -> bool {
    let mut pid = start;
    let (mut prev, mut agents) = (None, 0);
    // Whether the outermost Agent counted so far is a shell.
    let mut outer_shell = false;
    for _ in 0..MAX_NESTING {
        if pid <= 1 {
            break;
        }
        let dir = proc.join(pid.to_string());
        let env = nul_split(&std::fs::read(dir.join("environ")).unwrap_or_default());
        if !pane.iter().all(|kv| env.contains(kv)) {
            break;
        }
        let Ok(cmdline) = std::fs::read(dir.join("cmdline")) else {
            break;
        };
        let argv = nul_split(&cmdline);
        let here = binaries
            .iter()
            .find(|b| program_at(&argv, b).is_some())
            .copied();
        if let Some(b) = here.filter(|_| here != prev) {
            agents += 1;
            outer_shell = SHELLS.contains(&b);
        }
        prev = here;
        let Some(parent) = parent_of(proc, pid) else {
            break;
        };
        pid = parent;
    }
    agents - usize::from(outer_shell) >= 2
}

/// The argv of the program in the foreground of shell `shell`'s terminal
/// (R115.4): its `tpgid`'s leader; `None` while the shell itself is in
/// the foreground, without a terminal, or when unreadable.
pub fn foreground_argv(proc: &Path, shell: u32) -> Option<Vec<String>> {
    let stat = std::fs::read_to_string(proc.join(format!("{shell}/stat"))).ok()?;
    let f: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    let (pgrp, tpgid): (i32, i32) = (f.get(2)?.parse().ok()?, f.get(5)?.parse().ok()?);
    if tpgid <= 0 || tpgid == pgrp {
        return None;
    }
    let argv = nul_split(&std::fs::read(proc.join(format!("{tpgid}/cmdline"))).ok()?);
    (!argv.is_empty()).then_some(argv)
}

/// Process groups of `pane` (usually one; none once it exited).
pub fn pane_groups(pane: &str) -> Vec<i32> {
    let want = format!("RADE_PANE_ID={pane}");
    let Ok(tasks) = std::fs::read_dir("/proc/self/task") else {
        return vec![];
    };
    let mut pids: Vec<i32> = tasks
        .flatten()
        .filter_map(|t| std::fs::read_to_string(t.path().join("children")).ok())
        .flat_map(|s| {
            s.split_whitespace()
                .filter_map(|p| p.parse().ok())
                .collect::<Vec<i32>>()
        })
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/environ"))
                .is_ok_and(|e| e.split(|b| *b == 0).any(|v| v == want.as_bytes()))
        })
        .collect();
    pids.dedup();
    pids
}

/// Each Pane's process (the app's child whose environment has its
/// `RADE_PANE_ID`), in one pass over the app's children (R115.4).
pub fn pane_pids() -> std::collections::HashMap<String, i32> {
    let Ok(tasks) = std::fs::read_dir("/proc/self/task") else {
        return Default::default();
    };
    tasks
        .flatten()
        .filter_map(|t| std::fs::read_to_string(t.path().join("children")).ok())
        .flat_map(|s| {
            s.split_whitespace()
                .filter_map(|p| p.parse().ok())
                .collect::<Vec<i32>>()
        })
        .filter_map(|pid| {
            let env = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
            let pane = env
                .split(|b| *b == 0)
                .find_map(|v| v.strip_prefix(b"RADE_PANE_ID="))?;
            Some((String::from_utf8_lossy(pane).into_owned(), pid))
        })
        .collect()
}

/// `killpg`; signal 0 probes whether the group still exists.
pub fn signal_group(pgid: i32, signal: i32) -> bool {
    unsafe { libc::killpg(pgid, signal) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;

    /// Fake `/proc` entry: pid, ppid, argv, environ.
    fn fake(root: &Path, pid: u32, ppid: u32, argv: &[&str], env: &[&str]) {
        let d = root.join(pid.to_string());
        std::fs::create_dir_all(&d).unwrap();
        let z = |v: &[&str]| v.iter().map(|x| format!("{x}\0")).collect::<String>();
        std::fs::write(d.join("stat"), format!("{pid} (a (b) c) S {ppid} 1 1")).unwrap();
        std::fs::write(d.join("cmdline"), z(argv)).unwrap();
        std::fs::write(d.join("environ"), z(env)).unwrap();
    }

    #[test]
    fn r115_d_foreground_argv_is_the_ttys_foreground_group_not_the_shell() {
        let root = std::env::temp_dir().join(format!("rade-fg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // stat: pid (comm) state ppid pgrp session tty_nr tpgid …
        let stat = |pid: u32, tpgid: i32| {
            let d = root.join(pid.to_string());
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(
                d.join("stat"),
                format!("{pid} (ba sh) S 1 {pid} {pid} 34817 {tpgid} 0 0"),
            )
            .unwrap();
        };
        stat(10, 10);
        assert_eq!(foreground_argv(&root, 10), None, "the shell itself");
        stat(10, 20);
        let d = root.join("20");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("cmdline"), "codex\0--yolo\0").unwrap();
        assert_eq!(
            foreground_argv(&root, 10),
            Some(vec!["codex".to_string(), "--yolo".to_string()])
        );
        stat(11, -1);
        assert_eq!(foreground_argv(&root, 11), None, "no terminal");
        assert_eq!(foreground_argv(&root, 99), None, "gone");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn r10_hooks_of_an_agent_started_by_another_agent_are_nested() {
        let root = std::env::temp_dir().join(format!("rade-nest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let agents = ["claude", "codex", "agy"];
        let pane = [
            "RADE_PANE_ID=p_1".to_string(),
            "RADE_SOCKET=/r.sock".to_string(),
        ];
        let e = ["RADE_PANE_ID=p_1", "RADE_SOCKET=/r.sock", "HOME=/h"];
        let hook = ["/bin/sh", "-c", "rade status done --hook"];
        // RADE itself was started by an Agent: outside the Pane, not counted.
        fake(&root, 5, 1, &["claude"], &[]);
        fake(&root, 10, 5, &["rade-app"], &[]);
        fake(&root, 20, 10, &["/usr/bin/zsh"], &e);
        fake(
            &root,
            30,
            20,
            &["claude", "--dangerously-skip-permissions"],
            &e,
        );
        fake(&root, 31, 30, &hook, &e);
        assert!(
            !nested_agent(&root, 31, &agents, &pane),
            "the Pane's own Agent"
        );
        // Claude's Bash tool runs `codex exec`, whose hook reports.
        fake(&root, 40, 30, &["/usr/bin/zsh", "-c", "codex exec -"], &e);
        fake(&root, 41, 40, &["codex", "exec", "--json", "-"], &e);
        fake(&root, 42, 41, &hook, &e);
        assert!(nested_agent(&root, 42, &agents, &pane));
        // A Claude inside a Claude, too.
        fake(&root, 50, 40, &["claude", "-p"], &e);
        fake(&root, 51, 50, &hook, &e);
        assert!(nested_agent(&root, 51, &agents, &pane));
        // An npm wrapper over the native CLI is one Agent.
        fake(&root, 60, 20, &["node", "/usr/bin/codex", "--yolo"], &e);
        fake(&root, 61, 60, &["/opt/codex/bin/codex", "--yolo"], &e);
        fake(&root, 62, 61, &hook, &e);
        assert!(!nested_agent(&root, 62, &agents, &pane));
        // Gone or unreadable: not nested.
        assert!(!nested_agent(&root, 999, &agents, &pane));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn r11_2_an_agent_started_by_hand_in_a_shell_pane_is_not_nested() {
        let root = std::env::temp_dir().join(format!("rade-hand-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // A Preset whose command is `bash` (m2's `lead`) next to a script Agent.
        let agents = ["fake-agent.sh", "bash", "codex"];
        let pane = [
            "RADE_PANE_ID=p_1".to_string(),
            "RADE_SOCKET=/r.sock".to_string(),
        ];
        let e = ["RADE_PANE_ID=p_1", "RADE_SOCKET=/r.sock"];
        let hook = ["rade", "status", "done", "--hook"];
        fake(&root, 5, 1, &["rade-app"], &[]);
        fake(&root, 8, 5, &["/bin/sh", "-c", "/bin/bash --posix"], &e);
        fake(&root, 10, 8, &["/bin/bash", "--posix"], &e);
        fake(&root, 20, 10, &["bash", "/t/fake-agent.sh", "--flag-x"], &e);
        fake(&root, 21, 20, &hook, &e);
        assert!(
            !nested_agent(&root, 21, &agents, &pane),
            "the Pane's shell is not an Agent"
        );
        // An Agent started by that Agent still is nested.
        fake(&root, 30, 20, &["codex", "exec"], &e);
        fake(&root, 31, 30, &hook, &e);
        assert!(nested_agent(&root, 31, &agents, &pane));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn r11_2_finds_the_agent_above_a_hook() {
        let root = std::env::temp_dir().join(format!("rade-proc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let env = ["HOME=/h", "CLAUDE_CONFIG_DIR=/h/.claude-work", "X=1"];
        fake(
            &root,
            100,
            50,
            &["claude", "--dangerously-skip-permissions"],
            &env,
        );
        fake(
            &root,
            200,
            100,
            &["/bin/sh", "-c", "rade status --hook"],
            &env,
        );
        let cfg = ["CLAUDE_CONFIG_DIR".to_string()];
        let inv = find_invocation(&root, 200, "claude", &cfg).unwrap();
        assert_eq!(inv.argv, ["claude", "--dangerously-skip-permissions"]);
        assert_eq!(inv.env.len(), 1);
        assert_eq!(inv.env["CLAUDE_CONFIG_DIR"], "/h/.claude-work");
        // Wrapped by an interpreter: argv from the script on.
        fake(
            &root,
            300,
            1,
            &["node", "--x", "/usr/bin/codex", "-m", "o3"],
            &[],
        );
        fake(&root, 301, 300, &["sh", "-c", "codex"], &[]);
        let inv = find_invocation(&root, 301, "codex", &[]).unwrap();
        assert_eq!(inv.argv, ["/usr/bin/codex", "-m", "o3"]);
        assert!(inv.env.is_empty());
        // Not an ancestor within reach, a missing pid, or init: None.
        assert!(find_invocation(&root, 200, "codex", &cfg).is_none());
        assert!(find_invocation(&root, 999, "claude", &cfg).is_none());
        let mut ppid = 100;
        for pid in 400..407 {
            fake(&root, pid, ppid, &["sh"], &[]);
            ppid = pid;
        }
        assert!(
            find_invocation(&root, 406, "claude", &cfg).is_none(),
            "max depth"
        );
        assert!(find_invocation(&root, 404, "claude", &cfg).is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn m30_no_locked_memory_only_inside_and_io_uring_refused_there() {
        let limit = || {
            let mut r = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            // SAFETY: getrlimit fills `r`.
            unsafe { libc::getrlimit(libc::RLIMIT_MEMLOCK, &mut r) };
            r
        };
        let ring = || {
            let mut params = [0u8; 120];
            // SAFETY: io_uring_setup with a zeroed params struct; the fd is closed.
            let fd = unsafe { libc::syscall(libc::SYS_io_uring_setup, 8u32, params.as_mut_ptr()) };
            if fd >= 0 {
                unsafe { libc::close(fd as i32) };
            }
            fd >= 0
        };
        let before = limit();
        let (inside, ok) = without_locked_memory(|| (limit(), ring()));
        assert_eq!(inside.rlim_cur, 0);
        assert_eq!(inside.rlim_max, before.rlim_max);
        // Root (CAP_IPC_LOCK, e.g. a CI container) is never charged.
        if unsafe { libc::geteuid() } != 0 {
            assert!(!ok, "io_uring_setup is refused while nothing may be locked");
        }
        let after = limit();
        assert_eq!(
            (after.rlim_cur, after.rlim_max),
            (before.rlim_cur, before.rlim_max)
        );
    }

    #[test]
    fn finds_a_panes_child_by_env_and_kills_its_group() {
        let pane = format!("p_test_{}", std::process::id());
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .env("RADE_PANE_ID", &pane)
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        let mut found = vec![];
        for _ in 0..50 {
            found = pane_groups(&pane); // env is visible once exec'd
            if !found.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(found, [pid]);
        assert!(pane_groups("p_nobody").is_empty());
        assert!(signal_group(pid, 0));
        assert!(signal_group(pid, SIGKILL));
        child.wait().unwrap();
        assert!(!signal_group(pid, 0));
    }
}
