//! Commands that run without (or must survive without) the app: Agent hook
//! mode of `rade status` and `rade setup` (PRD R16.3).

use crate::call_at;
use rade_core::paths::{self, process_env};
use rade_core::rpc::{Error, ErrorKind};
use rade_core::setup;
use serde_json::{json, Value};
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// `rade status … --hook`: runs from Agent hooks in every session, RADE or
/// not, so it never prints and always exits 0, fast. Outside a RADE
/// Terminal (no RADE_PANE_ID / RADE_SOCKET) it does nothing at all.
pub fn hook(
    state: &Option<String>,
    session_id: &Option<String>,
    agent: &Option<String>,
    payload: &Option<String>,
) -> Result<Value, Error> {
    let (Some(pane), Some(socket)) = (process_env("RADE_PANE_ID"), process_env("RADE_SOCKET"))
    else {
        return Ok(Value::Null);
    };
    let needs_payload = session_id.is_none() || state.as_deref() == Some("done");
    let text = payload
        .clone()
        .or_else(|| needs_payload.then(stdin_json).flatten());
    let session = match session_id {
        Some(id) => Some(id.clone()).filter(|s| !s.is_empty()),
        None => text.as_deref().and_then(session_from),
    };
    let state = hook_state(state, text.as_deref());
    // The report that names the Preset (session start) also records how the
    // Agent was really started, so Session Restore can re-run it (R11.2).
    let invocation = agent.as_deref().and_then(invocation);
    let params = json!({"pane": pane, "status": state, "session_id": session, "agent": agent,
        "invocation": invocation});
    let _ = call_at(
        &PathBuf::from(socket),
        "pane.set_status",
        params,
        Some(Duration::from_secs(2)),
    );
    Ok(Value::Null)
}

/// How long `rade status --statusline` waits for RADE (R109.1).
const STATUSLINE_BUDGET: Duration = Duration::from_secs(2);

/// `usage.record`'s params for a statusline JSON read at `t`: `None` when
/// there is nothing to record (no `rate_limits`, not JSON).
pub(crate) fn statusline_params(text: &str, config_dir: &Path, t: i64) -> Option<Value> {
    let limits = rade_core::usage::claude::parse_statusline(text, t).ok()?;
    (!limits.is_empty())
        .then(|| json!({"agent": "claude", "config_dir": config_dir, "limits": limits}))
}

/// `rade status --statusline [--then <cmd>]` (R109.1): record the
/// statusline's `rate_limits` (at most 2 s for RADE; unreachable → dropped),
/// run `<cmd>` with the same stdin and let its stdout through. Prints
/// nothing of its own, never fails.
pub fn statusline(then: &Option<String>) -> Result<Value, Error> {
    let start = std::time::Instant::now();
    let input = stdin_json().unwrap_or_default();
    let env: paths::Env = &process_env;
    let config_dir = process_env("CLAUDE_CONFIG_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| paths::home_dir(env).join(".claude"));
    let t = stamp().parse::<i64>().unwrap_or(0);
    let (tx, rx) = std::sync::mpsc::channel();
    let sending = statusline_params(&input, &config_dir, t).map(|p| {
        let socket = paths::socket_path(env);
        std::thread::spawn(move || {
            let _ = call_at(&socket, "usage.record", p, Some(STATUSLINE_BUDGET));
            let _ = tx.send(());
        })
    });
    if let Some(cmd) = then.as_deref().filter(|c| !c.trim().is_empty()) {
        if let Ok(mut child) = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let bytes = input.clone().into_bytes();
                std::thread::spawn(move || {
                    let _ = stdin.write_all(&bytes);
                });
            }
            let _ = child.wait();
        }
    }
    if sending.is_some() {
        let _ = rx.recv_timeout(STATUSLINE_BUDGET.saturating_sub(start.elapsed()));
    }
    Ok(Value::Null)
}

/// The Agent process above this hook, found by the Preset's program name.
fn invocation(agent: &str) -> Option<rade_core::preset::Invocation> {
    let env: paths::Env = &process_env;
    let presets = std::fs::read_to_string(paths::config_file(env))
        .ok()
        .and_then(|t| rade_core::config::Config::parse(&t).ok())
        .unwrap_or_default()
        .presets()
        .ok()?;
    let p = presets.get(agent)?;
    rade_core::process::find_invocation(
        std::path::Path::new("/proc"),
        std::os::unix::process::parent_id(),
        p.binary(),
        &p.config_env,
    )
}

/// Session id from a hook payload: `session_id` (Claude Code, Codex hooks),
/// `thread-id` (Codex `notify`), or `conversationId` (Antigravity).
fn session_from(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    ["session_id", "thread-id", "conversationId"]
        .iter()
        .find_map(|k| v[k].as_str().filter(|s| !s.is_empty()).map(String::from))
}

/// The status a hook reports: Claude Code fires `Stop` when the turn ends
/// even while its background tasks (shells, Agents, monitors) still run, and
/// wakes the session again when they finish, so `done` with a running or
/// pending `background_tasks` entry is still `running`.
fn hook_state(state: &Option<String>, payload: Option<&str>) -> Option<String> {
    let busy = || {
        let v: Value = serde_json::from_str(payload?).ok()?;
        Some(v["background_tasks"].as_array()?.iter().any(|t| {
            matches!(t["status"].as_str(), Some("running" | "pending"))
        }))
    };
    match state.as_deref() {
        Some("done") if busy() == Some(true) => Some("running".into()),
        _ => state.clone(),
    }
}

/// Hook stdin, if it is a pipe that closes within a second.
fn stdin_json() -> Option<String> {
    if std::io::stdin().is_terminal() {
        return None;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::stdin().take(1 << 20).read_to_string(&mut s);
        let _ = tx.send(s);
    });
    rx.recv_timeout(Duration::from_secs(1)).ok()
}

pub(crate) fn on_path(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
}

/// Unified diff via diffutils; falls back to the whole new file.
pub(crate) fn show_diff(c: &setup::Change) {
    let old = if c.old.is_some() {
        c.path.clone()
    } else {
        PathBuf::from("/dev/null")
    };
    let label = c.path.display().to_string();
    let child = Command::new("diff")
        .args(["-u", "--label", &label, "--label", &label])
        .arg(&old)
        .arg("-")
        .stdin(Stdio::piped())
        .spawn();
    match child {
        Ok(mut child) => {
            let _ = child.stdin.take().unwrap().write_all(c.new.as_bytes());
            let _ = child.wait();
        }
        Err(_) => println!("{}", c.new),
    }
}

/// Backup suffix: Unix seconds.
pub(crate) fn stamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}

pub fn setup(agents: &[String], dry_run: bool) -> Result<Value, Error> {
    let agents: Vec<String> = if agents.is_empty() {
        setup::AGENTS
            .iter()
            .filter(|a| on_path(a))
            .map(|a| a.to_string())
            .collect()
    } else {
        agents.to_vec()
    };
    if agents.is_empty() {
        return Err(Error::usage(
            "no Agent found on PATH; name one with --agent claude|codex|agy|opencode",
        ));
    }
    let env: paths::Env = &process_env;
    let changes = setup::plan(&agents, env).map_err(Error::usage)?;
    if changes.is_empty() {
        println!("rade setup: already installed for {}", agents.join(", "));
    }
    for c in &changes {
        let what = match (c.remove, c.old.is_some()) {
            (true, _) => "remove",
            (_, true) => "update",
            _ => "create",
        };
        println!("== {what} {}", c.path.display());
        if !c.remove {
            show_diff(c);
        }
    }
    if dry_run {
        if !changes.is_empty() {
            println!("\n(dry run: nothing written)");
        }
    } else if !changes.is_empty() {
        let backups = setup::apply(&changes, &stamp())
            .map_err(|e| Error::new(ErrorKind::Internal, format!("writing: {e}")))?;
        for b in backups {
            println!("backup: {}", b.display());
        }
        println!("rade setup: wrote {} file(s)", changes.len());
    }
    for n in setup::notes(&agents, env) {
        println!("note: {n}");
    }
    Ok(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statusline_json_becomes_one_usage_record() {
        let text = include_str!("../../rade-core/tests/fixtures/usage/statusline-2.1.309.json");
        let p = statusline_params(text, Path::new("/h/.claude-work"), 1_793_170_000).unwrap();
        assert_eq!(p["agent"], "claude");
        assert_eq!(p["config_dir"], "/h/.claude-work");
        let ids: Vec<&str> = p["limits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["limit_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["five_hour", "seven_day", "spend_limit"]);
        assert!(p["limits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|l| l["t"] == 1_793_170_000));
        // No rate_limits (API key), not JSON: nothing to send.
        assert!(statusline_params(r#"{"model":{}}"#, Path::new("/h"), 1).is_none());
        assert!(statusline_params("garbage", Path::new("/h"), 1).is_none());
    }

    #[test]
    fn session_id_from_hook_payloads() {
        assert_eq!(
            session_from(r#"{"session_id":"abc","cwd":"/"}"#).as_deref(),
            Some("abc")
        );
        assert_eq!(
            session_from(r#"{"type":"agent-turn-complete","thread-id":"t1"}"#).as_deref(),
            Some("t1")
        );
        assert_eq!(
            session_from(r#"{"conversationId":"c-9","modelName":"m"}"#).as_deref(),
            Some("c-9")
        );
        assert_eq!(session_from(r#"{"session_id":""}"#), None);
        assert_eq!(session_from("not json"), None);
    }

    #[test]
    fn stop_with_running_background_tasks_stays_running() {
        let done = Some("done".to_string());
        let bg = |s: &str| {
            format!(r#"{{"hook_event_name":"Stop","background_tasks":[{{"id":"b1","type":"shell","status":"{s}"}}]}}"#)
        };
        assert_eq!(hook_state(&done, Some(&bg("running"))).as_deref(), Some("running"));
        assert_eq!(hook_state(&done, Some(&bg("pending"))).as_deref(), Some("running"));
        assert_eq!(hook_state(&done, Some(&bg("completed"))).as_deref(), Some("done"));
        let none = r#"{"hook_event_name":"Stop","background_tasks":[]}"#;
        assert_eq!(hook_state(&done, Some(none)).as_deref(), Some("done"));
        assert_eq!(hook_state(&done, None).as_deref(), Some("done"));
        assert_eq!(hook_state(&done, Some("not json")).as_deref(), Some("done"));
        // Only `done` is held back; other reports pass through.
        let waiting = Some("waiting".to_string());
        assert_eq!(hook_state(&waiting, Some(&bg("running"))).as_deref(), Some("waiting"));
        assert_eq!(hook_state(&None, Some(&bg("running"))), None);
    }
}
