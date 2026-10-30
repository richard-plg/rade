//! `rade schedule …` (PRD R138; docs/API.md). Everything but `next` asks
//! the app (Schedules live in its state and are fired by it, ADR-0051).

use clap::{Args, Subcommand};
use rade_core::schedule::{jiff::tz::TimeZone, Cron};
use serde_json::{json, Value};

/// The fields `add` and `edit` share.
#[derive(Args, Clone)]
pub struct Fields {
    /// Run in this Project's link checkout (a Project of the Schedule's Workspace).
    #[arg(long, conflicts_with = "dir")]
    project: Option<String>,
    /// The link (its Source) when the Project has several.
    #[arg(long)]
    link: Option<String>,
    /// Run in this folder, in Scratch (an absolute path).
    #[arg(long)]
    dir: Option<String>,
    /// Repeat: 5 fields `minute hour day-of-month month day-of-week`, or
    /// @hourly / @daily / @weekly / @monthly / @yearly, in local time.
    #[arg(long, conflicts_with = "at")]
    cron: Option<String>,
    /// Once: "YYYY-MM-DD HH:MM", local time.
    #[arg(long)]
    at: Option<String>,
    /// Agent Preset (default for a new Schedule: claude).
    #[arg(long)]
    agent: Option<String>,
    /// Agent Account (default: the Workspace's default when it runs).
    #[arg(long, conflicts_with = "default_account")]
    account: Option<String>,
    /// Back to the Workspace's default account (edit).
    #[arg(long)]
    default_account: bool,
    /// skip (default) | once: what a run missed while the app was closed does.
    #[arg(long)]
    catch_up: Option<String>,
    /// Run Panes kept open (1-20, default 3).
    #[arg(long)]
    keep_runs: Option<u8>,
    /// Read the prompt from a file (`-` = stdin).
    #[arg(long, value_name = "FILE", conflicts_with = "prompt")]
    prompt_file: Option<String>,
    /// The prompt.
    prompt: Option<String>,
}

#[derive(Subcommand)]
pub enum ScheduleCmd {
    /// Create a Schedule: start an Agent with a prompt on a cron expression
    /// or once at a time, in a Project link's checkout or a folder.
    Add {
        name: String,
        #[command(flatten)]
        f: Fields,
        /// Create it switched off.
        #[arg(long)]
        off: bool,
        #[arg(long)]
        workspace: Option<String>,
    },
    /// The Workspace's Schedules, by Project then Scratch.
    List {
        #[arg(long, conflicts_with = "all")]
        workspace: Option<String>,
        /// Every Workspace.
        #[arg(long)]
        all: bool,
        /// Only this Project's.
        #[arg(long, conflicts_with = "scratch")]
        project: Option<String>,
        /// Only those running in a folder (Scratch).
        #[arg(long)]
        scratch: bool,
    },
    /// One Schedule with its next 5 times and newest Runs.
    Show {
        name: String,
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Change a Schedule: only the given fields.
    Edit {
        name: String,
        #[command(flatten)]
        f: Fields,
        /// A new name (its Tab follows).
        #[arg(long)]
        rename: Option<String>,
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Switch a Schedule on.
    On {
        name: String,
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Switch a Schedule off (its Panes stay).
    Off {
        name: String,
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Delete a Schedule and its Runs (its Tab and Panes stay unless --close-panes).
    Remove {
        name: String,
        #[arg(long)]
        close_panes: bool,
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Run it now (on or off).
    Run {
        name: String,
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Its Runs, newest first.
    Runs {
        name: String,
        /// At most this many (default 20, ≤ 50).
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        workspace: Option<String>,
    },
    /// Resume a Run's Agent session (default: the newest started Run); an
    /// open Pane is focused instead.
    Resume {
        name: String,
        /// The Run number (`rade schedule runs`).
        n: Option<u32>,
        #[arg(long)]
        workspace: Option<String>,
    },
    /// The next local times of a cron expression (no app needed).
    Next {
        cron: String,
        #[arg(long, default_value_t = 5)]
        count: usize,
    },
}

fn prompt(f: &Fields) -> Result<Option<String>, String> {
    match &f.prompt_file {
        None => Ok(f.prompt.clone()),
        Some(p) if p == "-" => {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)
                .map_err(|e| format!("reading the prompt from stdin: {e}"))?;
            Ok(Some(s))
        }
        Some(p) => std::fs::read_to_string(p)
            .map(Some)
            .map_err(|e| format!("reading the prompt from {p}: {e}")),
    }
}

/// `--dir .` and friends → an absolute path (the app runs elsewhere).
fn abs_dir(d: &str) -> String {
    let p = std::path::Path::new(d);
    if p.is_absolute() {
        return d.to_string();
    }
    if let Some(rest) = d.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::Path::new(&home).join(rest).display().to_string();
        }
    }
    std::env::current_dir()
        .map(|c| c.join(p).display().to_string())
        .unwrap_or_else(|_| d.to_string())
}

fn fields(f: &Fields, mut p: Value) -> Result<Value, String> {
    let set = |p: &mut Value, k: &str, v: Option<Value>| {
        if let Some(v) = v {
            p[k] = v;
        }
    };
    set(&mut p, "project", f.project.clone().map(Value::from));
    set(&mut p, "link", f.link.clone().map(Value::from));
    set(
        &mut p,
        "dir",
        f.dir.as_deref().map(abs_dir).map(Value::from),
    );
    set(&mut p, "cron", f.cron.clone().map(Value::from));
    set(&mut p, "at", f.at.clone().map(Value::from));
    set(&mut p, "agent", f.agent.clone().map(Value::from));
    if f.default_account {
        p["account"] = Value::Null;
    } else {
        set(&mut p, "account", f.account.clone().map(Value::from));
    }
    set(&mut p, "catch_up", f.catch_up.clone().map(Value::from));
    set(&mut p, "keep_runs", f.keep_runs.map(Value::from));
    set(&mut p, "prompt", prompt(f)?.map(Value::from));
    Ok(p)
}

pub fn request(c: &ScheduleCmd) -> Result<(&'static str, Value), String> {
    Ok(match c {
        ScheduleCmd::Add {
            name,
            f,
            off,
            workspace,
        } => {
            let mut p = json!({"name": name, "workspace": workspace});
            if *off {
                p["enabled"] = json!(false);
            }
            ("schedule.add", fields(f, p)?)
        }
        ScheduleCmd::Edit {
            name,
            f,
            rename,
            workspace,
        } => {
            let mut p = json!({"name": name, "workspace": workspace});
            if let Some(r) = rename {
                p["rename"] = json!(r);
            }
            ("schedule.edit", fields(f, p)?)
        }
        ScheduleCmd::List {
            workspace,
            all,
            project,
            scratch,
        } => (
            "schedule.list",
            json!({"workspace": workspace, "all": all, "project": project, "scratch": scratch}),
        ),
        ScheduleCmd::Show { name, workspace } => (
            "schedule.get",
            json!({"name": name, "workspace": workspace}),
        ),
        ScheduleCmd::On { name, workspace } => (
            "schedule.set_enabled",
            json!({"name": name, "workspace": workspace, "enabled": true}),
        ),
        ScheduleCmd::Off { name, workspace } => (
            "schedule.set_enabled",
            json!({"name": name, "workspace": workspace, "enabled": false}),
        ),
        ScheduleCmd::Remove {
            name,
            close_panes,
            workspace,
        } => (
            "schedule.remove",
            json!({"name": name, "workspace": workspace, "close_panes": close_panes}),
        ),
        ScheduleCmd::Run { name, workspace } => (
            "schedule.run",
            json!({"name": name, "workspace": workspace}),
        ),
        ScheduleCmd::Runs {
            name,
            limit,
            workspace,
        } => (
            "schedule.runs",
            json!({"name": name, "workspace": workspace, "limit": limit}),
        ),
        ScheduleCmd::Resume { name, n, workspace } => (
            "schedule.resume",
            json!({"name": name, "workspace": workspace, "n": n}),
        ),
        ScheduleCmd::Next { .. } => unreachable!("local command"),
    })
}

/// `rade schedule next` (R138.1): local, no socket. Exit 2 on a bad
/// expression.
pub fn next(cron: &str, count: usize) -> Result<Value, rade_core::rpc::Error> {
    let c = Cron::parse(cron).map_err(rade_core::rpc::Error::usage)?;
    let tz = TimeZone::system();
    let mut t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let mut out = Vec::new();
    for _ in 0..count.clamp(1, 50) {
        match c.next_after(t, &tz) {
            Some(n) => {
                out.push(n);
                t = n;
            }
            None => break,
        }
    }
    let lines: Vec<String> = out.iter().map(|t| local(*t, &tz)).collect();
    Ok(json!({"times": out, "local": lines}))
}

/// `2026-11-02 09:00` local.
fn local(t: i64, tz: &TimeZone) -> String {
    use rade_core::schedule::jiff::Timestamp;
    match Timestamp::from_second(t) {
        Ok(ts) => {
            let z = ts.to_zoned(tz.clone());
            format!("{} {:02}:{:02}", z.date(), z.hour(), z.minute())
        }
        Err(_) => t.to_string(),
    }
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

fn target(v: &Value) -> String {
    match (v["project"].as_str(), v["link"].as_str(), v["dir"].as_str()) {
        (Some(p), Some(l), _) => format!("{p}/{l}"),
        (Some(p), None, _) => p.to_string(),
        (_, _, Some(d)) => format!("{d} (Scratch)"),
        _ => String::new(),
    }
}

fn when(t: &Value) -> String {
    let tz = TimeZone::system();
    t.as_i64()
        .map(|t| local(t, &tz))
        .unwrap_or_else(|| "—".into())
}

/// `on  nightly  api/api  Daily 02:00  claude (default)  next 2026-10-31 02:00`
fn line(v: &Value) -> String {
    let state = if v["done"].as_bool() == Some(true) {
        "done"
    } else if v["enabled"].as_bool() == Some(true) {
        "on"
    } else {
        "off"
    };
    let account = match v["account"].as_str() {
        Some(a) => a.to_string(),
        None => match v["account_now"].as_str() {
            Some(a) => format!("default: {a}"),
            None => "default".into(),
        },
    };
    let last = match v["last"]["outcome"].as_str() {
        Some(o) => format!("  last #{} {o}", v["last"]["n"]),
        None => String::new(),
    };
    format!(
        "{state}  {}  {}  {}  {} ({account})  next {}{last}",
        s(&v["name"]),
        target(v),
        s(&v["when_text"]),
        s(&v["agent"]),
        when(&v["next"]),
    )
}

/// `#12  2026-10-30 09:00  started  done  p_88  3f2a…`
fn run_line(r: &Value) -> String {
    let mut out = format!("#{}  {}  {}", r["n"], when(&r["at"]), s(&r["outcome"]));
    if let Some(st) = r["status"].as_str() {
        out.push_str(&format!("  {st}"));
    }
    if let Some(p) = r["pane"].as_str() {
        out.push_str(&format!("  {p}"));
    }
    if let Some(sid) = r["session"].as_str() {
        out.push_str(&format!("  {sid}"));
    }
    if let Some(reason) = r["reason"].as_str() {
        out.push_str(&format!("  ({reason})"));
    }
    out
}

pub fn print(c: &ScheduleCmd, r: &Value) {
    match c {
        ScheduleCmd::List { .. } => {
            for v in r["schedules"].as_array().into_iter().flatten() {
                println!("{}", line(v));
            }
        }
        ScheduleCmd::Show { .. } => {
            println!("{}", line(r));
            println!("prompt: {}", s(&r["prompt"]));
            for t in r["next_times"].as_array().into_iter().flatten() {
                println!("next: {}", when(t));
            }
            for run in r["runs"].as_array().into_iter().flatten() {
                println!("{}", run_line(run));
            }
        }
        ScheduleCmd::Add { .. }
        | ScheduleCmd::Edit { .. }
        | ScheduleCmd::On { .. }
        | ScheduleCmd::Off { .. } => println!("{}", line(r)),
        ScheduleCmd::Remove { .. } => {
            println!("removed {}", s(&r["name"]));
            for p in r["closed"].as_array().into_iter().flatten() {
                println!("closed {}", s(p));
            }
        }
        ScheduleCmd::Run { .. } => println!("{}", run_line(r)),
        ScheduleCmd::Runs { .. } => {
            for run in r["runs"].as_array().into_iter().flatten() {
                println!("{}", run_line(run));
            }
        }
        ScheduleCmd::Resume { .. } => {
            let how = if r["resumed"].as_bool() == Some(true) {
                "resumed in"
            } else {
                "focused"
            };
            println!("{how} {}", s(&r["pane"]));
        }
        ScheduleCmd::Next { .. } => {
            for l in r["local"].as_array().into_iter().flatten() {
                println!("{}", s(l));
            }
        }
    }
}
