//! The one runner for every child process RADE starts (R140.1, ADR-0052):
//! stdin is `/dev/null` (or the given input), the child gets a process
//! group of its own (a [`Spec::session`] child a session too: no
//! controlling tty, so an interactive shell cannot prompt on one), a time
//! limit kills the whole group, it counts as finished when it exits even
//! if a grandchild still holds its pipes, and output is capped.
//!
//! A group costs nothing (`posix_spawn`); a session needs a `pre_exec`
//! hook, which makes `std` fork the whole app instead: only for probes.

use std::io::{Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Local git commands (status, diff, commit, worktree …).
pub const GIT_LOCAL: Duration = Duration::from_secs(30);
/// Network git commands (fetch, pull, push).
pub const GIT_NETWORK: Duration = Duration::from_secs(120);
/// Shell and other probes.
pub const PROBE: Duration = Duration::from_secs(5);
/// Default cap on each of stdout and stderr.
pub const OUTPUT_CAP: usize = 256 << 20;
/// How long the pipes may stay open after the child has exited.
const GRACE: Duration = Duration::from_millis(250);

/// How a child runs.
pub struct Spec<'a> {
    /// Written to the child's stdin; `/dev/null` without.
    pub input: Option<&'a [u8]>,
    /// After this the process group is killed and the run is `TimedOut`.
    pub limit: Duration,
    /// Per stream. A stream over the cap is cut and the group killed.
    pub cap: usize,
    /// Also a session of its own, and every process of it killed at the
    /// limit (shell probes: an interactive shell's jobs get groups of
    /// their own).
    pub session: bool,
}

impl<'a> Spec<'a> {
    pub fn new(limit: Duration) -> Self {
        Spec {
            input: None,
            limit,
            cap: OUTPUT_CAP,
            session: false,
        }
    }
    pub fn session(mut self) -> Self {
        self.session = true;
        self
    }
    pub fn input(mut self, input: &'a [u8]) -> Self {
        self.input = Some(input);
        self
    }
    pub fn cap(mut self, cap: usize) -> Self {
        self.cap = cap;
        self
    }
}

/// A finished child.
pub struct Done {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// A stream went over the cap: the output is cut and the child killed.
    pub truncated: bool,
}

impl Done {
    pub fn into_output(self) -> Output {
        Output {
            status: self.status,
            stdout: self.stdout,
            stderr: self.stderr,
        }
    }
}

/// `true` for the error a run returns at its time limit.
pub fn is_timeout(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::TimedOut
}

/// Run `cmd` to completion with stdin `/dev/null`; see [`Spec`].
pub fn output(cmd: &mut Command, limit: Duration) -> std::io::Result<Output> {
    run(cmd, &Spec::new(limit)).map(Done::into_output)
}

/// [`output`] for a shell probe: a session of its own, all of it killed.
pub fn output_session(cmd: &mut Command, limit: Duration) -> std::io::Result<Output> {
    run(cmd, &Spec::new(limit).session()).map(Done::into_output)
}

/// Like [`output`] with `input` on stdin.
pub fn output_input(cmd: &mut Command, input: &[u8], limit: Duration) -> std::io::Result<Output> {
    run(cmd, &Spec::new(limit).input(input)).map(Done::into_output)
}

enum Ev {
    Exit(std::io::Result<ExitStatus>),
    /// A pipe reached end of file.
    Eof,
    /// A stream went over its cap.
    Over,
}

fn read_stream(
    mut r: impl Read + Send + 'static,
    cap: usize,
    ev: mpsc::Sender<Ev>,
) -> Arc<Mutex<Vec<u8>>> {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let b = buf.clone();
    std::thread::spawn(move || {
        let mut chunk = [0u8; 16 << 10];
        loop {
            match r.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut b = b.lock().unwrap();
                    let room = cap.saturating_sub(b.len());
                    b.extend_from_slice(&chunk[..n.min(room)]);
                    if n > room {
                        let _ = ev.send(Ev::Over);
                    }
                }
            }
        }
        let _ = ev.send(Ev::Eof);
    });
    buf
}

/// Run `cmd` under `spec`. A timeout is `Err(TimedOut)` after the whole
/// process group (and every process of the child's session) is dead.
pub fn run(cmd: &mut Command, spec: &Spec) -> std::io::Result<Done> {
    if spec.session {
        // SAFETY: setsid and close_range are async-signal-safe.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                crate::process::no_inherit();
                Ok(())
            });
        }
    } else {
        cmd.process_group(0);
    }
    let mut child = cmd
        .stdin(if spec.input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pid = child.id() as i32;
    if let (Some(input), Some(mut stdin)) = (spec.input, child.stdin.take()) {
        let input = input.to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let (tx, rx) = mpsc::channel();
    let out = read_stream(child.stdout.take().unwrap(), spec.cap, tx.clone());
    let err = read_stream(child.stderr.take().unwrap(), spec.cap, tx.clone());
    let exit_tx = tx;
    std::thread::spawn(move || {
        let _ = exit_tx.send(Ev::Exit(child.wait()));
    });
    let start = Instant::now();
    let (mut eofs, mut truncated, mut killed) = (0, false, false);
    let status = loop {
        let left = spec.limit.saturating_sub(start.elapsed());
        // After a kill for the cap the exit report is certain: just wait.
        let ev = if killed {
            rx.recv().map_err(|_| ())
        } else {
            rx.recv_timeout(left).map_err(|_| ())
        };
        match ev {
            Ok(Ev::Exit(st)) => break st?,
            Ok(Ev::Eof) => eofs += 1,
            Ok(Ev::Over) => {
                truncated = true;
                if !killed {
                    killed = true;
                    kill(pid, spec.session);
                }
            }
            Err(_) => {
                kill(pid, spec.session);
                // Reap, so no zombie outlives the call.
                while let Ok(ev) = rx.recv() {
                    if matches!(ev, Ev::Exit(_)) {
                        break;
                    }
                }
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("timed out after {} s", spec.limit.as_secs_f32().round()),
                ));
            }
        }
    };
    // Finished: a grandchild that still holds a pipe gets a short grace to
    // close it, then we take what has arrived.
    let until = Instant::now() + GRACE;
    while eofs < 2 {
        match rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
            Ok(Ev::Eof) => eofs += 1,
            Ok(Ev::Over) => truncated = true,
            Ok(Ev::Exit(_)) => {}
            Err(_) => break,
        }
    }
    let take = |b: &Arc<Mutex<Vec<u8>>>| std::mem::take(&mut *b.lock().unwrap());
    Ok(Done {
        status,
        stdout: take(&out),
        stderr: take(&err),
        truncated,
    })
}

fn kill(pid: i32, session: bool) {
    if session {
        kill_session(pid);
    } else {
        unsafe { libc::killpg(pid, libc::SIGKILL) };
    }
}

/// SIGKILL the process group `sid` and every process whose session id is
/// `sid` (from `/proc`): an interactive shell's jobs get groups of their own.
pub fn kill_session(sid: i32) {
    unsafe { libc::killpg(sid, libc::SIGKILL) };
    kill_sid(sid);
}

fn kill_sid(sid: i32) {
    for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        // `pid (comm) state ppid pgrp session …`: comm may hold spaces.
        let stat = std::fs::read_to_string(e.path().join("stat")).unwrap_or_default();
        let sess = stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().nth(3)?.parse::<i32>().ok());
        if sess == Some(sid) {
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
    }
}

/// `true` when the status is a death by `sig`.
pub fn killed_by(status: &ExitStatus, sig: i32) -> bool {
    status.signal() == Some(sig)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alive(marker: &str) -> usize {
        std::fs::read_dir("/proc")
            .unwrap()
            .flatten()
            .filter_map(|e| std::fs::read(e.path().join("cmdline")).ok())
            .filter(|c| c.windows(marker.len()).any(|w| w == marker.as_bytes()))
            .count()
    }

    #[test]
    fn r140_a_a_term_ignoring_child_with_a_pipe_holding_grandchild_is_killed() {
        let t = Instant::now();
        let mut c = Command::new("/bin/sh");
        c.args([
            "-c",
            "trap '' TERM; sleep 27.1828 & while :; do sleep 0.2; done",
        ]);
        let e = output(&mut c, Duration::from_secs(1)).unwrap_err();
        assert!(is_timeout(&e));
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(alive("27.1828"), 0, "the group outlived the limit");
    }

    #[test]
    fn r140_a_finished_on_exit_when_a_grandchild_holds_stdout() {
        let t = Instant::now();
        let mut c = Command::new("/bin/sh");
        c.args(["-c", "sleep 3 & echo hi"]);
        let o = output(&mut c, Duration::from_secs(20)).unwrap();
        assert!(o.status.success());
        assert_eq!(String::from_utf8_lossy(&o.stdout), "hi\n");
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    }

    #[test]
    fn r140_a_output_is_capped_and_the_child_killed() {
        let mut c = Command::new("/bin/sh");
        c.args(["-c", "yes | head -c 100000000"]);
        let d = run(&mut c, &Spec::new(Duration::from_secs(10)).cap(1000)).unwrap();
        assert!(d.truncated);
        assert_eq!(d.stdout.len(), 1000);
    }

    #[test]
    fn input_reaches_stdin_and_stdin_defaults_to_null() {
        let mut c = Command::new("cat");
        let o = output_input(&mut c, b"abc", Duration::from_secs(5)).unwrap();
        assert_eq!(o.stdout, b"abc");
        let mut c = Command::new("cat");
        let o = output(&mut c, Duration::from_secs(5)).unwrap();
        assert!(o.stdout.is_empty());
    }

    /// Every file that builds a `Command` or calls `.output()` outside the
    /// runner, with why that is fine (R140.1). A new one must be added here
    /// on purpose, or go through `proc`.
    const SPAWN_ALLOW: &[(&str, &str)] = &[
        ("rade-core/src/git.rs", "builds commands; run through proc"),
        (
            "rade-core/src/shell_env.rs",
            "shell probes: the command is run by proc",
        ),
        (
            "rade-core/src/scm.rs",
            "gh pr create goes through proc; the Draft runner has its own limit and cancel",
        ),
        (
            "rade-core/src/pr.rs",
            "gh pr list / run view reads go through proc (20 s / 30 s)",
        ),
        (
            "rade-core/src/update.rs",
            "curl with --max-time; install is long-running by design",
        ),
        (
            "rade-core/src/usage/detailed.rs",
            "curl with --max-time and the token on stdin",
        ),
        (
            "rade-core/src/code/lsp.rs",
            "the language server is long-running by design",
        ),
        (
            "rade-app/src/browser.rs",
            "the stand-in browser for links: detached",
        ),
        (
            "rade-app/src/project_dialogs.rs",
            "runs rade, which has its own call limit",
        ),
        (
            "rade-app/src/sysmon_view.rs",
            "nvidia-smi: built here, run by proc in 2 s",
        ),
        (
            "rade-app/src/update.rs",
            "download / pkexec install: long-running by design",
        ),
        (
            "rade-app/src/voice.rs",
            "voice agent, STT and recorder: long-running by design",
        ),
        ("rade-app/build.rs", "build script"),
        (
            "rade-cli/src/chromium.rs",
            "the Chromium daemon: long-running by design",
        ),
        ("rade-cli/src/workspace.rs", "interactive setup commands"),
        ("rade-cli/src/clone.rs", "interactive clone"),
        (
            "rade-cli/src/local.rs",
            "rade setup diff and hook, bounded there",
        ),
    ];

    fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                rust_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }

    #[test]
    fn r140_h_every_direct_spawn_is_in_the_allow_list() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut files = Vec::new();
        rust_files(&crates, &mut files);
        let mut bad = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for f in files {
            let rel = f
                .strip_prefix(&crates)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let name = f.file_name().unwrap().to_string_lossy().into_owned();
            if rel.contains("/tests/") || name == "tests.rs" || name.ends_with("_tests.rs") {
                continue;
            }
            let src = std::fs::read_to_string(&f).unwrap();
            // Unit tests sit at the end of a file, after `#[cfg(test)]`.
            let code = src.split("\n#[cfg(test)]").next().unwrap();
            let needle_a = ["Command", "::new("].concat();
            let needle_b = [".out", "put()"].concat();
            if !(code.contains(&needle_a) || code.contains(&needle_b)) {
                continue;
            }
            match SPAWN_ALLOW.iter().find(|(p, _)| *p == rel) {
                Some((p, _)) => {
                    seen.insert(*p);
                }
                None => bad.push(rel),
            }
        }
        assert!(
            bad.is_empty(),
            "a direct child process outside proc.rs (use proc::output, or allow-list it with a reason): {bad:?}"
        );
        for (p, _) in SPAWN_ALLOW {
            assert!(
                seen.contains(p),
                "{p} no longer spawns: drop it from the allow-list"
            );
        }
    }
}
