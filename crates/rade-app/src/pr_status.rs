//! PR status of the Worktrees (PRD R146, R147.4, R148, ADR-0053): the poller
//! that reads each Worktree's pull request and checks with `gh`, the
//! transitions it turns into Activity rows, Notifications and Toasts, the
//! opt-in auto-send of a failing CI log (`[scm] ci_failures = "send"`), and
//! the `pr.*` / `debug.pr` socket methods.
//!
//! For the widgets (chips, buttons):
//! - [`get`] / [`status`]: the last [`PrStatus`] of a Worktree (by folder or
//!   by Source and Worktree id); `None` = no PR or not read yet (the model's
//!   `Worktree.pr` is the number recorded at the last read, R146.5).
//! - [`connect_changed`]: called on the UI thread when a status changed.
//! - [`refresh`]: read a Worktree now (the Changes card's Refresh PR Status).
//! - [`send_to_agent`]: the prompt of the failed jobs to the Worktree's Agent.
//! - [`unavailable`]: gh's message while a Workspace's reads are paused.
//!
//! Nothing here runs `gh` or `git` on the UI thread: the 5 s timer only asks
//! the core schedule what is due, and each read runs in the background, at
//! most one per Workspace (`Gate`). Times are the Schedules' clock
//! (`schedules::now()`, moved by the test clock) since the app started.

// The widgets that read `get` / `unavailable` / `connect_changed` and call
// `refresh` / `send_to_agent` are the next task's.
#![allow(dead_code)]

use crate::app::{app, try_app, App};
use crate::handlers::{on_main, params};
use gtk::glib;
use gtk4 as gtk;
use gtk4::prelude::*;
use rade_core::activity::{Event as Act, Kind};
use rade_core::ci_log::{self, Action, AgentState, AutoSend, Mode, Read};
use rade_core::git;
use rade_core::model::PrRef;
use rade_core::pr::{self, Candidate, Checks, Event, Gate, PrState, PrStatus, Tracker};
use rade_core::rpc::{Error, ErrorKind, Result};
use rade_core::{AgentStatus, State};
use serde::Deserialize;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};

/// How often the timer asks the schedule what is due.
const TICK_SECS: u32 = 5;
/// How long a Worktree with `pr_status = false` is left alone before the
/// config is looked at again.
const OFF_RECHECK: Duration = Duration::from_secs(30);
/// `pr.wait` reads this often.
const WAIT_EVERY: Duration = Duration::from_secs(20);

/// What the poller keeps per Worktree (key `<source>/<worktree>`).
#[derive(Default)]
struct Wt {
    status: Option<PrStatus>,
    last_read: Option<Duration>,
    /// A Push / Create PR / `HEAD` change / refresh asked for a read.
    trigger: bool,
    /// The last read found no upstream (and no recorded PR): ask git again
    /// after [`pr::IDLE_EVERY`] or on a trigger.
    no_upstream_at: Option<Duration>,
    /// `pr_status = false` here: don't look again before this.
    disabled_until: Option<Duration>,
    /// The Worktree's `HEAD` and reflog with their mtimes at the last read.
    heads: Vec<(PathBuf, Option<SystemTime>)>,
}

/// A Workspace's `gh` state.
#[derive(Default)]
struct Ws {
    gate: Gate,
    in_flight: bool,
    down_until: Option<Duration>,
}

#[derive(Default)]
struct Poller {
    wts: HashMap<String, Wt>,
    ws: HashMap<String, Ws>,
    tracker: Tracker,
    autos: HashMap<String, AutoSend>,
    /// `debug.pr activate`: the window counts as active or not.
    active: Option<bool>,
    listeners: Vec<Rc<dyn Fn()>>,
}

thread_local! {
    static P: RefCell<Poller> = RefCell::default();
}

/// Unix seconds when the poller was installed.
static START: OnceLock<i64> = OnceLock::new();

fn clock() -> Duration {
    let start = *START.get_or_init(crate::schedules::now);
    Duration::from_secs((crate::schedules::now() - start).max(0) as u64)
}

fn key(pid: &str, wid: &str) -> String {
    format!("{pid}/{wid}")
}

// ---------------------------------------------------------------------------
// For the widgets
// ---------------------------------------------------------------------------

/// The last status of the Worktree at `path`.
pub fn get(path: &Path) -> Option<PrStatus> {
    let app = try_app()?;
    let k = {
        let m = app.model.borrow();
        let (s, w) = m.worktrees().find(|(_, w)| git::same(&w.path, path))?;
        key(&s.id, &w.id)
    };
    P.with(|p| p.borrow().wts.get(&k).and_then(|w| w.status.clone()))
}

/// The last status of a Worktree by ids.
pub fn status(pid: &str, wid: &str) -> Option<PrStatus> {
    P.with(|p| {
        p.borrow()
            .wts
            .get(&key(pid, wid))
            .and_then(|w| w.status.clone())
    })
}

/// gh's first message line while the Workspace's reads are paused.
pub fn unavailable(workspace: &str) -> Option<String> {
    let now = clock();
    P.with(|p| {
        p.borrow()
            .ws
            .get(workspace)
            .and_then(|w| w.gate.message(now).map(str::to_string))
    })
}

/// Called (UI thread) after a status changed; draw the chips again.
pub fn connect_changed(f: impl Fn() + 'static) {
    P.with(|p| p.borrow_mut().listeners.push(Rc::new(f)));
}

fn changed() {
    let listeners: Vec<Rc<dyn Fn()>> = P.with(|p| p.borrow().listeners.clone());
    for l in listeners {
        l();
    }
    if let Some(app) = try_app() {
        crate::sidebar::resync(&app);
        crate::home_view::refresh();
    }
}

/// Read the Worktree at `path` now (Push, Publish, Sync, Create PR, `HEAD`
/// change, Refresh PR Status).
pub fn now(path: &Path) {
    let Some(app) = try_app() else { return };
    let found = {
        let m = app.model.borrow();
        let f = m
            .worktrees()
            .find(|(_, w)| git::same(&w.path, path))
            .map(|(s, w)| key(&s.id, &w.id));
        f
    };
    let Some(k) = found else { return };
    P.with(|p| {
        let mut p = p.borrow_mut();
        let w = p.wts.entry(k).or_default();
        w.trigger = true;
        w.no_upstream_at = None;
        w.disabled_until = None;
    });
    tick();
}

/// [`now`] for a Worktree by ids.
pub fn refresh(pid: &str, wid: &str) {
    let path = try_app().and_then(|a| a.model.borrow().worktree(pid, wid).map(|w| w.path.clone()));
    if let Some(p) = path {
        now(&p);
    }
}

/// A PR was created from the Worktree at `path` (R146.5): record its number
/// and read it.
pub fn created(path: &Path, url: &str) {
    let Some(app) = try_app() else { return };
    let digits: String = url
        .trim()
        .trim_end_matches('/')
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if let Ok(number) = digits.parse::<u64>() {
        let ids = {
            let m = app.model.borrow();
            let f = m
                .worktrees()
                .find(|(_, w)| git::same(&w.path, path))
                .map(|(s, w)| (s.id.clone(), w.id.clone()));
            f
        };
        if let Some((pid, wid)) = ids {
            let pr = PrRef {
                number,
                url: url.trim().to_string(),
            };
            if app.update_coalesced(|m| m.set_worktree_pr(&pid, &wid, pr)) {
                changed();
            }
        }
    }
    now(path);
}

/// The chip of a Worktree (R146.3): its last read, else its recorded
/// number in `fg3` (R146.5); `None` = no PR.
pub fn chip(pid: &str, wid: &str) -> Option<pr::Chip> {
    let recorded = try_app().and_then(|a| {
        a.model
            .borrow()
            .worktree(pid, wid)
            .and_then(|w| w.pr.as_ref().map(|p| p.number))
    });
    pr::worktree_chip(status(pid, wid).as_ref(), recorded)
}

/// The PR's URL: the last read's, else the recorded one.
pub fn url(pid: &str, wid: &str) -> Option<String> {
    status(pid, wid)
        .map(|s| s.url)
        .or_else(|| {
            try_app().and_then(|a| {
                a.model
                    .borrow()
                    .worktree(pid, wid)
                    .and_then(|w| w.pr.as_ref().map(|p| p.url.clone()))
            })
        })
        .filter(|u| !u.is_empty())
}

/// The last read says the checks fail: Send CI Failure to Agent is offered.
pub fn failing(pid: &str, wid: &str) -> bool {
    status(pid, wid).is_some_and(|s| s.checks == Checks::Failing && s.state == PrState::Open)
}

/// Send the failed jobs' log tails to the Worktree's Agent (R147.3, no
/// preview yet): the prompt is built off the UI thread, then delivered with
/// `review::deliver` (the only Agent; none or several → a Toast saying so).
pub fn send_to_agent(pid: &str, wid: &str) {
    send_then(pid, wid, |r| {
        if let Err(msg) = r {
            toast(&format!(
                "CI failure not sent: {}",
                glib::markup_escape_text(&msg)
            ));
        }
    });
}

fn send_then(
    pid: &str,
    wid: &str,
    done: impl FnOnce(std::result::Result<String, String>) + 'static,
) {
    let Some(app) = try_app() else { return };
    let Some(s) = status(pid, wid).filter(|s| s.checks == Checks::Failing) else {
        return done(Err("the checks are not failing".into()));
    };
    let Some(job) = job_for(&app.model.borrow(), &app, pid, wid) else {
        return done(Err("no such Worktree".into()));
    };
    let (pid, wid) = (pid.to_string(), wid.to_string());
    crate::worktrees::in_background(
        move || ci_log::fetch_failures(&job.dir, GH, &s, &job.env).map(|f| (f, s.number)),
        move |r| {
            let Some(app) = try_app() else { return };
            let (f, number) = match r {
                Ok(x) => x,
                Err(e) => return done(Err(e.message)),
            };
            let Some(prompt) = f.prompt else {
                return done(Err("the checks are not failing".into()));
            };
            match crate::review::deliver(&app, &pid, &wid, None, None, None, None, &prompt) {
                Ok((pane, _)) => {
                    let name = {
                        let m = app.model.borrow();
                        m.pane(&pane)
                            .and_then(|p| p.name.clone())
                            .unwrap_or_else(|| pane.clone())
                    };
                    activity(
                        &app,
                        &pid,
                        &wid,
                        Kind::PrNote,
                        &format!("#{number} CI failure sent to {name}"),
                        Some(pane.clone()),
                    );
                    done(Ok(pane));
                }
                Err(e) => done(Err(e.message)),
            }
        },
    );
}

/// An Agent's status changed: a failing PR waiting for it to stop running
/// may be sent now (R147.4).
pub fn agent_changed(pane: &str) {
    let Some(app) = try_app() else { return };
    let Some((pid, wid)) = app.model.borrow().pane_worktree(pane) else {
        return;
    };
    let failing = P.with(|p| {
        p.borrow()
            .wts
            .get(&key(&pid, &wid))
            .and_then(|w| w.status.as_ref())
            .is_some_and(|s| s.checks == Checks::Failing)
    });
    if failing {
        auto_step(&app, &pid, &wid);
    }
}

// ---------------------------------------------------------------------------
// The schedule
// ---------------------------------------------------------------------------

const GH: &str = "gh";

/// One read to run.
#[derive(Clone)]
struct Job {
    key: String,
    ws: String,
    pid: String,
    wid: String,
    dir: PathBuf,
    recorded: bool,
    env: Vec<(String, String)>,
}

fn job_for(m: &State, _app: &App, pid: &str, wid: &str) -> Option<Job> {
    let s = m.source(pid)?;
    let w = s.worktree(wid)?;
    Some(Job {
        key: key(pid, wid),
        ws: s.workspace.clone(),
        pid: pid.to_string(),
        wid: wid.to_string(),
        dir: w.path.clone(),
        recorded: w.pr.is_some(),
        env: crate::app::workspace_env(m, pid),
    })
}

fn active_now(app: &App) -> bool {
    P.with(|p| p.borrow().active)
        .unwrap_or_else(|| app.window.is_active())
}

fn mtimes(heads: &[(PathBuf, Option<SystemTime>)]) -> Vec<Option<SystemTime>> {
    heads
        .iter()
        .map(|(p, _)| std::fs::metadata(p).and_then(|m| m.modified()).ok())
        .collect()
}

/// What is due now: at most one read per Workspace, none while one is in
/// flight or the Workspace is down (UI thread; no `gh`, no `git`).
fn collect() -> Vec<Job> {
    let Some(app) = try_app() else {
        return Vec::new();
    };
    let now = clock();
    let active = active_now(&app);
    let m = app.model.borrow();
    let shown = m.active_workspace().to_string();
    let mut by_ws: BTreeMap<String, Vec<(&str, &rade_core::Worktree)>> = BTreeMap::new();
    for s in m.sources.iter().filter(|s| !s.is_general()) {
        for w in s.worktrees.iter().skip(1) {
            by_ws
                .entry(s.workspace.clone())
                .or_default()
                .push((&s.id, w));
        }
    }
    let mut jobs = Vec::new();
    P.with(|p| {
        let mut p = p.borrow_mut();
        let p = &mut *p;
        // A Worktree's `HEAD` changed: read it again (R146.2).
        for list in by_ws.values() {
            for (pid, w) in list {
                if let Some(t) = p.wts.get_mut(&key(pid, &w.id)) {
                    let cur = mtimes(&t.heads);
                    if !t.heads.is_empty() && cur.iter().zip(&t.heads).any(|(c, (_, o))| c != o) {
                        t.trigger = true;
                        t.no_upstream_at = None;
                        for (h, c) in t.heads.iter_mut().zip(cur) {
                            h.1 = c;
                        }
                    }
                }
            }
        }
        for (ws, list) in &by_ws {
            if !p.ws.entry(ws.clone()).or_default().gate.can_start(now) {
                continue;
            }
            let mut skip: HashSet<String> = HashSet::new();
            loop {
                let items: Vec<(String, Candidate)> = list
                    .iter()
                    .map(|(pid, w)| (key(pid, &w.id), *pid, *w))
                    .filter(|(k, ..)| !skip.contains(k))
                    .filter(|(k, ..)| {
                        !p.wts
                            .get(k)
                            .and_then(|t| t.disabled_until)
                            .is_some_and(|t| now < t)
                    })
                    .map(|(k, _, w)| {
                        let t = p.wts.get(&k);
                        let eligible = w.pr.is_some()
                            || t.and_then(|t| t.no_upstream_at)
                                .is_none_or(|at| now >= at + pr::IDLE_EVERY);
                        let c = Candidate {
                            eligible,
                            shown: *ws == shown && active,
                            pending: t
                                .and_then(|t| t.status.as_ref())
                                .is_some_and(|s| s.checks == Checks::Pending),
                            last_read: t.and_then(|t| t.last_read),
                            trigger: t.is_some_and(|t| t.trigger),
                        };
                        (k, c)
                    })
                    .collect();
                let Some(k) = pr::next_due(&items, now) else {
                    break;
                };
                let Some((pid, w)) = list.iter().find(|(pid, w)| key(pid, &w.id) == k) else {
                    break;
                };
                if !w.path.exists() {
                    skip.insert(k);
                    continue;
                }
                if !app.config_for(pid, Some(&w.id)).scm.pr_status {
                    p.wts.entry(k.clone()).or_default().disabled_until = Some(now + OFF_RECHECK);
                    skip.insert(k);
                    continue;
                }
                let st = p.ws.entry(ws.clone()).or_default();
                if st.gate.start(now) {
                    st.in_flight = true;
                    if let Some(j) = job_for(&m, &app, pid, &w.id) {
                        jobs.push(j);
                    }
                }
                break;
            }
        }
    });
    jobs
}

/// Start what is due, each read in the background.
fn tick() {
    for job in collect() {
        start(job);
    }
}

fn start(job: Job) {
    crate::worktrees::in_background(
        move || {
            let out = run_job(&job);
            (job, out)
        },
        |(job, out)| {
            let _ = apply(&job, out);
            // The next Worktree of the Workspace may be due.
            tick();
        },
    );
}

// ---------------------------------------------------------------------------
// One read
// ---------------------------------------------------------------------------

enum Outcome {
    /// No branch, or no upstream and no recorded PR: not read.
    NotEligible,
    /// gh's first message line.
    Failed(String),
    Read {
        status: Option<PrStatus>,
        heads: Vec<PathBuf>,
    },
}

/// Blocking: git facts, then `gh pr list`. Any thread but the UI's.
fn run_job(job: &Job) -> Outcome {
    let one = |args: &[&str]| {
        git::run(&job.dir, args)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let Some(branch) = one(&["symbolic-ref", "--short", "-q", "HEAD"]) else {
        return Outcome::NotEligible;
    };
    let has_upstream =
        one(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]).is_some();
    if !has_upstream && !job.recorded {
        return Outcome::NotEligible;
    }
    let head = one(&["rev-parse", "HEAD"]).unwrap_or_default();
    let heads = one(&["rev-parse", "--absolute-git-dir"])
        .map(|d| {
            let d = PathBuf::from(d);
            vec![d.join("HEAD"), d.join("logs").join("HEAD")]
        })
        .unwrap_or_default();
    match pr::read(&job.dir, GH, &branch, &job.env) {
        Err(e) => Outcome::Failed(e.message),
        Ok(prs) => Outcome::Read {
            status: pr::choose(&prs)
                .map(|p| PrStatus::new(p, &head, Some(crate::schedules::now().max(0) as u64))),
            heads,
        },
    }
}

fn same(a: &Option<PrStatus>, b: &Option<PrStatus>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => {
            PrStatus {
                checked_at: None,
                ..x.clone()
            } == PrStatus {
                checked_at: None,
                ..y.clone()
            }
        }
        (None, None) => true,
        _ => false,
    }
}

/// Store a read (UI thread): the status, the model's `pr`, the transitions.
/// `Err` = gh's message.
fn apply(job: &Job, out: Outcome) -> std::result::Result<Option<PrStatus>, String> {
    let Some(app) = try_app() else {
        return Ok(None);
    };
    let now = clock();
    let mut result: std::result::Result<Option<PrStatus>, String> = Ok(None);
    let mut events: Vec<Event> = Vec::new();
    let mut down_row: Option<String> = None;
    let mut modified = false;
    P.with(|p| {
        let mut p = p.borrow_mut();
        let p = &mut *p;
        let ws = p.ws.entry(job.ws.clone()).or_default();
        ws.in_flight = false;
        let wt = p.wts.entry(job.key.clone()).or_default();
        wt.last_read = Some(now);
        wt.trigger = false;
        match out {
            Outcome::Failed(msg) => {
                if ws.gate.finish_err(now, &msg) {
                    down_row = Some(msg.clone());
                }
                ws.down_until = Some(now + pr::DOWN_FOR);
                result = Err(msg);
            }
            Outcome::NotEligible => {
                ws.gate.finish_ok();
                ws.down_until = None;
                wt.no_upstream_at = Some(now);
                modified = wt.status.take().is_some();
                events = p.tracker.feed(&job.key, None);
            }
            Outcome::Read { status, heads } => {
                ws.gate.finish_ok();
                ws.down_until = None;
                wt.no_upstream_at = None;
                if !heads.is_empty() {
                    wt.heads = heads
                        .into_iter()
                        .map(|h| {
                            let t = std::fs::metadata(&h).and_then(|m| m.modified()).ok();
                            (h, t)
                        })
                        .collect();
                }
                modified = !same(&wt.status, &status);
                wt.status = status.clone();
                events = p.tracker.feed(&job.key, status.as_ref());
                result = Ok(status);
            }
        }
    });
    if let Some(msg) = down_row {
        activity(
            &app,
            &job.pid,
            &job.wid,
            Kind::PrNote,
            &format!(
                "GitHub pull request status unavailable: {msg} (paused for {} min)",
                pr::DOWN_FOR.as_secs() / 60
            ),
            None,
        );
    }
    if let Ok(Some(s)) = &result {
        let r = PrRef {
            number: s.number,
            url: s.url.clone(),
        };
        let (pid, wid) = (job.pid.clone(), job.wid.clone());
        if app.update_coalesced(|m| m.set_worktree_pr(&pid, &wid, r)) {
            modified = true;
        }
    }
    handle_events(&app, job, events);
    if modified {
        changed();
    }
    auto_step(&app, &job.pid, &job.wid);
    result
}

// ---------------------------------------------------------------------------
// Transitions (R146.4)
// ---------------------------------------------------------------------------

/// The Pane a Notification about the Worktree goes to: its Agent Pane, else
/// its first Pane.
fn notify_pane(m: &State, pid: &str, wid: &str) -> Option<String> {
    m.agent_panes(pid, wid)
        .first()
        .map(|p| p.id.clone())
        .or_else(|| m.panes_in(pid, wid).first().map(|p| p.id.clone()))
}

fn activity(app: &App, pid: &str, wid: &str, kind: Kind, line: &str, pane: Option<String>) {
    let project = {
        let m = app.model.borrow();
        m.worktree(pid, wid)
            .and_then(|w| crate::activity_view::project_of_path(&m, &w.path))
            .and_then(|x| x.1)
    };
    crate::activity_view::push(
        Act::pr_status(kind, line, crate::panes::now())
            .in_project(project)
            .on_pane(pane),
    );
}

fn toast(markup: &str) {
    if let Some(app) = try_app() {
        app.chrome.toast(markup, None);
    }
}

fn handle_events(app: &App, job: &Job, events: Vec<Event>) {
    if events.is_empty() {
        return;
    }
    let pane = notify_pane(&app.model.borrow(), &job.pid, &job.wid);
    for ev in events {
        let kind = match ev {
            Event::ChecksFailed { .. } => Kind::PrFailed,
            Event::ChecksPassed { .. } => Kind::PrPassed,
            Event::Merged { .. } => Kind::PrMerged,
            Event::ChangesRequested { .. } => Kind::PrReview,
        };
        activity(app, &job.pid, &job.wid, kind, &ev.activity(), pane.clone());
        // The Notification's Send to Agent action is the UI's
        // (`offers_send_to_agent`, [`send_to_agent`]).
        if let (Some((title, body)), Some(pane)) = (ev.notification(), &pane) {
            app.update_coalesced(|m| m.notify_own(pane, Some(title), &body, crate::panes::now()));
        }
        if let Some(t) = ev.toast() {
            let (pid, wid) = (job.pid.clone(), job.wid.clone());
            app.chrome.toast(
                &glib::markup_escape_text(&t),
                Some((
                    "Remove Worktree\u{2026}",
                    Box::new(move || crate::worktrees::remove_dialog(&pid, &wid)),
                )),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Auto-send (R147.4)
// ---------------------------------------------------------------------------

fn agent_state(m: &State, pid: &str, wid: &str) -> (usize, Option<AgentState>) {
    let agents = m.agent_panes(pid, wid);
    // Idle = waiting for the user, done, or cut off by a restart.
    let state = match agents.as_slice() {
        [one] => Some(match one.status {
            Some(AgentStatus::Done) => AgentState::Done,
            Some(AgentStatus::Waiting | AgentStatus::Interrupted) => AgentState::Waiting,
            _ => AgentState::Running,
        }),
        _ => None,
    };
    (agents.len(), state)
}

/// Feed `AutoSend` the Worktree's last read and its Agent's state.
fn auto_step(app: &App, pid: &str, wid: &str) {
    let k = key(pid, wid);
    let Some((number, head, checks)) = P.with(|p| {
        p.borrow()
            .wts
            .get(&k)
            .and_then(|w| w.status.as_ref())
            .map(|s| (s.number, s.head.clone(), s.checks))
    }) else {
        return;
    };
    let mode = Mode::parse(&app.config_for(pid, Some(wid)).scm.ci_failures);
    let (agents, state) = agent_state(&app.model.borrow(), pid, wid);
    let read = match checks {
        Checks::Failing => Read::Failing { head: &head },
        Checks::Passing => Read::Passing,
        _ => Read::Other,
    };
    let action = P.with(|p| {
        p.borrow_mut()
            .autos
            .entry(k)
            .or_default()
            .step(mode, read, agents, state)
    });
    match action {
        Action::Send => {
            let (p, w) = (pid.to_string(), wid.to_string());
            send_then(pid, wid, move |r| {
                if let Err(msg) = r {
                    activity_toast(&p, &w, number, &msg);
                }
            });
        }
        Action::AskStillFailing => {
            let text = ci_log::still_failing(number);
            // The borrow must end before `update_coalesced` borrows the model mutably.
            let pane = notify_pane(&app.model.borrow(), pid, wid);
            if let Some(pane) = pane {
                app.update_coalesced(|m| m.notify_own(&pane, None, &text, crate::panes::now()));
            }
            activity(
                app,
                pid,
                wid,
                Kind::PrNote,
                &format!("#{number} {text}"),
                None,
            );
        }
        Action::Wait | Action::Nothing => {}
    }
}

fn activity_toast(pid: &str, wid: &str, number: u64, msg: &str) {
    if let Some(app) = try_app() {
        activity(
            &app,
            pid,
            wid,
            Kind::PrNote,
            &format!("#{number} CI failure not sent: {msg}"),
            None,
        );
    }
}

// ---------------------------------------------------------------------------
// Socket methods (docs/API.md `rade pr`, `debug.pr`)
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
struct Req {
    source: Option<String>,
    worktree: Option<String>,
    caller: Option<String>,
    refresh: bool,
    timeout: Option<u64>,
}

/// (source, worktree) ids: `--source` / `--worktree`, else the caller's
/// Worktree, else the one on screen (like `review.*`).
fn target(m: &State, r: &Req) -> Result<(String, String)> {
    let caller = r.caller.as_deref().and_then(|c| m.locate(c));
    let source = match (&r.source, &caller) {
        (Some(p), _) => m.find_source(p)?.id.clone(),
        (None, Some(l)) => l.source.clone(),
        (None, None) => m.active().0.id.clone(),
    };
    match (&r.worktree, &caller) {
        (Some(w), _) => m.find_worktree(&source, Some(w)),
        (None, Some(_)) if r.source.is_none() => m
            .pane_worktree(r.caller.as_deref().unwrap())
            .ok_or_else(|| Error::not_found("caller gone")),
        _ => m.find_worktree(&source, None),
    }
}

struct Ctx {
    job: Job,
    name: String,
}

fn resolve(r: Req) -> Result<Ctx> {
    on_main(move || {
        let app = app();
        let m = app.model.borrow();
        let (pid, wid) = target(&m, &r)?;
        let job =
            job_for(&m, &app, &pid, &wid).ok_or_else(|| Error::not_found("no such worktree"))?;
        let name = m
            .worktree(&pid, &wid)
            .map(|w| w.name.clone())
            .unwrap_or_default();
        Ok(Ctx { job, name })
    })
}

/// Take the Workspace's one gh slot for a read the caller asked for (also
/// when the Workspace is down). `Ok(None)` = busy.
fn begin_forced(pid: &str, wid: &str) -> Result<Option<Job>> {
    let app = app();
    let now = clock();
    if !app.config_for(pid, Some(wid)).scm.pr_status {
        return Err(Error::usage(
            "pull request status is off ([scm] pr_status = false)",
        ));
    }
    let job = job_for(&app.model.borrow(), &app, pid, wid)
        .ok_or_else(|| Error::not_found("no such worktree"))?;
    Ok(P.with(|p| {
        let mut p = p.borrow_mut();
        let ws = p.ws.entry(job.ws.clone()).or_default();
        if ws.in_flight {
            return None;
        }
        if ws.gate.is_down(now) {
            ws.gate.finish_ok();
            ws.down_until = None;
        }
        ws.gate.start(now).then(|| {
            ws.in_flight = true;
            job
        })
    }))
}

/// A read now, on the socket thread (the app stays responsive): waits up to
/// 25 s for the Workspace's slot.
fn read_now(c: &Ctx) -> Result<Option<PrStatus>> {
    let deadline = Instant::now() + Duration::from_secs(25);
    let job = loop {
        let (pid, wid) = (c.job.pid.clone(), c.job.wid.clone());
        match on_main(move || begin_forced(&pid, &wid))? {
            Some(j) => break j,
            None if Instant::now() > deadline => {
                return Err(Error::usage("another pull request read is still running"))
            }
            None => std::thread::sleep(Duration::from_millis(200)),
        }
    };
    let out = run_job(&job);
    on_main(move || Ok(apply(&job, out))).and_then(|r| r.map_err(Error::usage))
}

fn answer(c: &Ctx, s: Option<PrStatus>) -> Value {
    let branch = git::branch(&c.job.dir).unwrap_or_default();
    let text = match &s {
        Some(s) => pr::cli_line(s),
        None => pr::cli_line_none(&branch),
    };
    json!({"worktree": c.name, "source": c.job.pid, "branch": branch, "pr": s, "text": text})
}

fn timeout_error(secs: u64, s: &PrStatus) -> Error {
    Error::new(
        ErrorKind::Timeout,
        format!("#{} checks still pending after {secs} s", s.number),
    )
}

pub fn dispatch(method: &str, p: Value) -> Result<Value> {
    match method {
        "pr.status" => {
            let r: Req = params(p)?;
            let refresh = r.refresh;
            let c = resolve(r)?;
            let s = if refresh {
                read_now(&c)?
            } else {
                let (pid, wid) = (c.job.pid.clone(), c.job.wid.clone());
                on_main(move || Ok(status(&pid, &wid)))?
            };
            Ok(answer(&c, s))
        }
        "pr.wait" => {
            let r: Req = params(p)?;
            let secs = r.timeout.unwrap_or(1800);
            let c = resolve(r)?;
            let deadline = Instant::now() + Duration::from_secs(secs);
            loop {
                let Some(s) = read_now(&c)? else {
                    let branch = git::branch(&c.job.dir).unwrap_or_default();
                    return Err(Error::not_found(pr::cli_line_none(&branch)));
                };
                if s.checks != Checks::Pending || s.state != PrState::Open {
                    return Ok(answer(&c, Some(s)));
                }
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(timeout_error(secs, &s));
                }
                std::thread::sleep(left.min(WAIT_EVERY));
                if Instant::now() >= deadline {
                    return Err(timeout_error(secs, &s));
                }
            }
        }
        "pr.failures" => {
            let r: Req = params(p)?;
            let c = resolve(r)?;
            let Some(s) = read_now(&c)? else {
                let branch = git::branch(&c.job.dir).unwrap_or_default();
                return Err(Error::not_found(pr::cli_line_none(&branch)));
            };
            let f = ci_log::fetch_failures(&c.job.dir, GH, &s, &c.job.env)?;
            let mut v = serde_json::to_value(&f).map_err(|e| Error::internal(e.to_string()))?;
            v["worktree"] = json!(c.name);
            Ok(v)
        }
        "debug.pr" => debug(&p),
        _ => Err(Error::usage(format!("unknown method '{method}'"))),
    }
}

fn debug(p: &Value) -> Result<Value> {
    match p.get("op").and_then(Value::as_str).unwrap_or("state") {
        // Run what is due now, reads and all, before answering.
        "tick" => {
            let mut read = Vec::new();
            for _ in 0..200 {
                let jobs = on_main(|| Ok(collect()))?;
                if jobs.is_empty() {
                    break;
                }
                for job in jobs {
                    let out = run_job(&job);
                    read.push(job.wid.clone());
                    on_main(move || {
                        let _ = apply(&job, out);
                        Ok(())
                    })?;
                }
            }
            Ok(json!({"read": read}))
        }
        "state" => on_main(|| Ok(state_json())),
        "activate" => {
            let on = p.get("on").and_then(Value::as_bool);
            on_main(move || {
                P.with(|p| p.borrow_mut().active = on);
                Ok(json!({"on": on}))
            })
        }
        op => Err(Error::usage(format!(
            "debug.pr op must be tick|state|activate, not '{op}'"
        ))),
    }
}

fn state_json() -> Value {
    let now = clock();
    let active = try_app().map(|a| active_now(&a));
    P.with(|p| {
        let p = p.borrow();
        let ws: serde_json::Map<String, Value> =
            p.ws.iter()
                .map(|(k, w)| {
                    (
                        k.clone(),
                        json!({
                            "in_flight": w.in_flight,
                            "down_until": w.down_until.filter(|t| now < *t).map(|t| t.as_secs()),
                            "message": w.gate.message(now),
                        }),
                    )
                })
                .collect();
        let wts: serde_json::Map<String, Value> = p
            .wts
            .iter()
            .map(|(k, w)| {
                (
                    k.clone(),
                    json!({
                        "status": w.status,
                        "last_read": w.last_read.map(|t| t.as_secs()),
                        "trigger": w.trigger,
                        "no_upstream": w.no_upstream_at.is_some(),
                    }),
                )
            })
            .collect();
        json!({"now": now.as_secs(), "active": active, "workspaces": ws, "worktrees": wts})
    })
}

/// The 5 s timer. The first tick comes after the window is up (the first
/// frame), so a restore is not slowed down by `gh`.
pub fn install() {
    let _ = clock();
    glib::timeout_add_seconds_local(TICK_SECS, || {
        if try_app().is_some_and(|a| a.window.is_mapped()) {
            tick();
        }
        glib::ControlFlow::Continue
    });
}
