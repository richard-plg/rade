//! Import the user's login-shell environment (PATH from ~/.zshrc etc.) when
//! RADE is started from the desktop, like VS Code / Zed do. Without it,
//! Agents installed in ~/.local/bin are "not found" in Agent Panes.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// Variables describing the probe shell itself, not the user's environment.
const SKIP: &[&str] = &["PWD", "OLDPWD", "SHLVL", "_", "PS1", "PS2"];

/// Parse `env -0` output, dropping shell-local and invalid entries.
pub fn parse_env0(bytes: &[u8]) -> Vec<(String, String)> {
    bytes
        .split(|b| *b == 0)
        .filter_map(|e| std::str::from_utf8(e).ok())
        .filter_map(|e| e.split_once('='))
        .filter(|(k, _)| !k.is_empty() && !SKIP.contains(k))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Run `$SHELL -l -i -c 'env -0'` (5 s timeout) and return its environment.
/// `None` on any failure: callers keep the inherited environment.
pub fn login_shell_env(shell: &str) -> Option<Vec<(String, String)>> {
    probe(shell, None, &[], &[])
}

/// The environment a login shell ends up with: started with `set` added
/// and `unset` removed, after `cd <dir>` when given (so a `cd` / `chpwd`
/// hook in an rc file runs, R26 check 11). 5 s timeout; `None` on any
/// failure.
pub fn probe(
    shell: &str,
    dir: Option<&Path>,
    set: &[(String, String)],
    unset: &[&str],
) -> Option<Vec<(String, String)>> {
    let mut cmd = Command::new(shell);
    match dir {
        Some(d) => cmd
            .args(["-l", "-i", "-c", "cd -- \"$1\" && env -0", "_"])
            .arg(d),
        None => cmd.args(["-l", "-i", "-c", "env -0"]),
    };
    for k in unset {
        cmd.env_remove(k);
    }
    cmd.envs(set.iter().map(|(k, v)| (k, v)));
    let out = bounded_output(&mut cmd, Duration::from_secs(5)).ok()?;
    let env = parse_env0(&out.stdout);
    (out.status.success() && !env.is_empty()).then_some(env)
}

/// Run a probe that must never hang or touch the terminal: stdin is
/// `/dev/null`, it runs in its own session (no controlling tty, so an
/// interactive shell can't grab the foreground or prompt), and after
/// `limit` every process of that session is killed (an interactive shell's
/// jobs get process groups of their own). A timeout is `TimedOut`.
pub fn bounded_output(cmd: &mut Command, limit: Duration) -> std::io::Result<std::process::Output> {
    use std::os::unix::process::CommandExt;
    // SAFETY: setsid is async-signal-safe.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            crate::process::no_inherit();
            Ok(())
        });
    }
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pid = child.id() as i32;
    let (tx, rx) = mpsc::channel();
    // Read on a thread so a large output cannot fill the pipe and stall.
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(limit) {
        Ok(out) => out,
        Err(_) => {
            kill_session(pid);
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("timed out after {} s", limit.as_secs_f32()),
            ))
        }
    }
}

/// SIGKILL every process whose session id is `sid` (from `/proc`).
pub fn kill_session(sid: i32) {
    unsafe { libc::killpg(sid, libc::SIGKILL) };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_env0_and_skips_shell_locals() {
        let raw = b"PATH=/home/u/.local/bin:/usr/bin\0PWD=/x\0SHLVL=2\0A=b=c\0junk\0\0";
        assert_eq!(
            parse_env0(raw),
            vec![
                ("PATH".into(), "/home/u/.local/bin:/usr/bin".into()),
                ("A".into(), "b=c".into())
            ]
        );
    }

    #[test]
    fn reads_a_real_shell() {
        let env = login_shell_env("/bin/sh").expect("sh env");
        assert!(env.iter().any(|(k, _)| k == "PATH"));
    }

    #[test]
    fn r26_probe_runs_in_a_dir_with_env_set_and_unset() {
        let dir = std::env::temp_dir();
        let env = probe(
            "/bin/sh",
            Some(&dir),
            &[("RADE_PROBE_X".into(), "1".into())],
            &["HOME_PROBE_NONE"],
        )
        .expect("sh env");
        assert!(env.contains(&("RADE_PROBE_X".into(), "1".into())));
        assert!(probe("/bin/sh", Some(Path::new("/no/such/dir")), &[], &[]).is_none());
    }

    #[test]
    fn bounded_output_kills_a_hung_probe_and_its_children() {
        let t = std::time::Instant::now();
        let mut c = Command::new("/bin/sh");
        c.args(["-c", "sleep 30 & read x; sleep 30"]);
        let err = bounded_output(&mut c, Duration::from_millis(300)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(t.elapsed() < Duration::from_secs(5));
        // An interactive shell puts each job in its own process group (job
        // control): those die too.
        let mut c = Command::new("/bin/sh");
        c.args(["-c", "set -m; sleep 31.4159 & sleep 31.4159; wait"]);
        assert!(bounded_output(&mut c, Duration::from_millis(300)).is_err());
        std::thread::sleep(Duration::from_millis(200));
        let left = std::fs::read_dir("/proc")
            .unwrap()
            .flatten()
            .filter_map(|e| std::fs::read(e.path().join("cmdline")).ok())
            .filter(|c| c.windows(7).any(|w| w == b"31.4159"))
            .count();
        assert_eq!(left, 0, "a job of the probe outlived it");
        let mut c = Command::new("/no/such/probe");
        let err = bounded_output(&mut c, Duration::from_secs(1)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        let mut c = Command::new("/bin/sh");
        c.args(["-c", "tty || echo none"]);
        let out = bounded_output(&mut c, Duration::from_secs(5)).unwrap();
        assert!(String::from_utf8_lossy(&out.stdout).contains("none"));
    }
}
