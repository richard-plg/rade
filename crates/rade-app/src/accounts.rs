//! `account.*` and `usage.*` socket methods (docs/API.md, PRD R108–R110).
//! Accounts are read from each Workspace's `workspace.toml` on every call;
//! the rules live in `rade_core::account`. Usage (R109): `usage.record`
//! (the statusline wrapper) and `usage.refresh` (log tails, Detailed limits)
//! write each login's Usage store; a 30 s poll and every Agent turn end
//! refresh too; `usage.list` / `usage.get` read the stores
//! (`rade_core::usage::report`). Limit events go to Activity (R110.5).

use crate::app::app;
use crate::handlers::{on_main, params};
use gtk4::glib;
use rade_core::account::{self, Account, AccountDecl, AgentKind};
use rade_core::rpc::{Error, Result};
use rade_core::usage::store::{self, Record};
use rade_core::usage::{detailed, limits, report, tail, Limit, UsageState};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime};

pub fn dispatch(method: &str, p: Value) -> Result<Value> {
    match method {
        "account.list" => list(params(p)?),
        "account.add" => add(params(p)?),
        "account.set_default" => set_default(params(p)?),
        "usage.list" => usage_list(params(p)?),
        "usage.get" => usage_get(params(p)?),
        "usage.record" => usage_record(params(p)?),
        "usage.refresh" => usage_refresh(params(p)?),
        _ => Err(Error::usage(format!("unknown method '{method}'"))),
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Scope {
    workspace: Option<String>,
    caller: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Add {
    workspace: Option<String>,
    caller: Option<String>,
    agent: String,
    name: String,
    config_dir: String,
    default: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Named {
    workspace: Option<String>,
    caller: Option<String>,
    /// `usage.get` names it `account`, `account.set_default` `name`.
    name: Option<String>,
    account: Option<String>,
    agent: Option<String>,
}

/// The Workspace an `account.*` call acts on: `workspace`, else the
/// caller's, else the shown one → (id, root).
fn workspace(q: Option<String>, caller: Option<String>) -> Result<(String, PathBuf)> {
    on_main(move || {
        let app = app();
        let m = app.model.borrow();
        let id = match &q {
            Some(q) => m.find_workspace(q)?.id.clone(),
            None => caller
                .as_deref()
                .and_then(|c| m.locate(c))
                .and_then(|l| m.source(&l.source))
                .map(|s| s.workspace.clone())
                .unwrap_or_else(|| m.active_workspace().to_string()),
        };
        let root = m
            .workspace(&id)
            .map(|w| w.root.clone())
            .ok_or_else(|| Error::not_found(format!("no workspace '{id}'")))?;
        Ok((id, root))
    })
}

fn home() -> PathBuf {
    rade_core::paths::home_dir(&rade_core::paths::process_env)
}

/// The Workspace's accounts, or its validation error.
fn accounts_of(id: &str, root: &std::path::Path) -> Result<Vec<Account>> {
    let file = root.join(".rade/workspace.toml");
    let f = rade_core::workspace::WorkspaceFile::read(root).map_err(Error::usage)?;
    account::accounts(id, &f.account, &file, &home())
}

fn agent_arg(a: Option<&str>) -> Result<Option<AgentKind>> {
    a.map(|s| {
        AgentKind::parse(s)
            .ok_or_else(|| Error::usage(format!("--agent must be claude or codex, not '{s}'")))
    })
    .transpose()
}

fn row(a: &Account) -> Value {
    json!({
        "agent": a.agent, "name": a.name, "config_dir": a.config_dir,
        "default": a.default, "detailed_limits": a.detailed_limits,
    })
}

fn list(s: Scope) -> Result<Value> {
    let (id, root) = workspace(s.workspace, s.caller)?;
    let (rows, errors) = match accounts_of(&id, &root) {
        Ok(l) => (l.iter().map(row).collect::<Vec<_>>(), vec![]),
        Err(e) => (vec![], vec![e.message]),
    };
    Ok(json!({"workspace": id, "accounts": rows, "errors": errors}))
}

fn add(a: Add) -> Result<Value> {
    let (id, root) = workspace(a.workspace, a.caller)?;
    let decl = AccountDecl {
        agent: a.agent,
        name: a.name,
        config_dir: a.config_dir,
        default: a.default,
        detailed_limits: false,
    };
    let file = account::add_account_file(&root, &decl)?;
    forget_lists();
    let agent = AgentKind::parse(&decl.agent);
    let added = accounts_of(&id, &root)
        .ok()
        .and_then(|l| {
            l.into_iter()
                .find(|x| Some(x.agent) == agent && x.name == decl.name.trim())
        })
        .map(|x| row(&x));
    Ok(json!({"workspace": id, "file": file, "account": added}))
}

fn set_default(n: Named) -> Result<Value> {
    let name = n
        .name
        .or(n.account)
        .ok_or_else(|| Error::usage("set_default needs a name"))?;
    let agent = agent_arg(n.agent.as_deref())?;
    let (id, root) = workspace(n.workspace, n.caller)?;
    let file = account::set_default_file(&root, agent, &name)?;
    let found = accounts_of(&id, &root)
        .ok()
        .and_then(|l| account::find(&l, &name, agent, &id).ok().cloned());
    // The explicit default replaces a forecast Toast's choice (R110.4).
    if let Some(kind) = found.as_ref().map(|a| a.agent).or(agent) {
        let ws = id.clone();
        on_main(move || {
            app().update(|m| m.clear_next_default(&ws, kind));
            Ok(())
        })?;
    }
    forget_lists();
    let set = found.as_ref().map(row);
    Ok(json!({"workspace": id, "file": file, "account": set}))
}

// ---- Usage (R109, R110) ------------------------------------------------------

/// Each store's last scan result (`format-changed` hides its numbers), its
/// run of log lines without a sample, when the clock was last checked for
/// passed resets, and a lock per store file, so a scan and a statusline
/// record never interleave.
#[derive(Default)]
struct Stores {
    last_scan: HashMap<PathBuf, UsageState>,
    tracks: HashMap<PathBuf, rade_core::usage::ScanTrack>,
    reset_checked: HashMap<PathBuf, i64>,
    locks: HashMap<PathBuf, Arc<Mutex<()>>>,
}

fn stores() -> std::sync::MutexGuard<'static, Stores> {
    static S: OnceLock<Mutex<Stores>> = OnceLock::new();
    S.get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn store_lock(path: &Path) -> Arc<Mutex<()>> {
    stores()
        .locks
        .entry(path.to_path_buf())
        .or_default()
        .clone()
}

fn last_scan(path: &Path) -> Option<UsageState> {
    stores().last_scan.get(path).copied()
}

/// The Detailed limits gate (one request per login per 5 minutes).
fn gate() -> std::sync::MutexGuard<'static, detailed::Gate> {
    static G: OnceLock<Mutex<detailed::Gate>> = OnceLock::new();
    G.get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn now_s() -> i64 {
    crate::panes::now() as i64
}

fn store_of(a: &Account) -> PathBuf {
    store::store_path(
        &store::usage_dir(&rade_core::paths::process_env),
        a.agent,
        &a.config_dir,
    )
}

/// Activity lines (R110.5, R109.5), pushed on the main loop.
fn push_events(lines: Vec<String>) {
    if lines.is_empty() {
        return;
    }
    let at = crate::panes::now();
    glib::MainContext::default().invoke(move || {
        for l in lines {
            crate::activity_view::push(rade_core::activity::Event::limit_line(&l, at));
        }
    });
}

/// Append a batch of limit samples (one source, one time) to a store: only
/// those worth a line against its latest ones. Returns (appended, events).
fn record_limits(path: &Path, account: &str, limits: Vec<Limit>) -> (Vec<Record>, Vec<String>) {
    let records = store::read(path);
    let (recs, events) = report::new_samples(&records, account, limits);
    if let Err(e) = store::append(path, &recs) {
        eprintln!("rade: usage store {}: {e}", path.display());
    }
    (recs, events)
}

/// Every Workspace: (id, root).
fn all_workspaces() -> Result<Vec<(String, PathBuf)>> {
    on_main(|| {
        let app = app();
        let m = app.model.borrow();
        Ok(m.workspaces
            .iter()
            .map(|w| (w.id.clone(), w.root.clone()))
            .collect())
    })
}

/// The declared account of a login (agent + canonical config dir), the
/// first Workspace's in registry order.
fn account_of_login(agent: AgentKind, dir: &Path) -> Option<Account> {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let want = canon(dir);
    all_workspaces()
        .ok()?
        .into_iter()
        .filter_map(|(id, root)| accounts_of(&id, &root).ok())
        .flatten()
        .find(|a| a.agent == agent && canon(&a.config_dir) == want)
}

#[derive(Deserialize)]
struct RecordParams {
    agent: String,
    config_dir: PathBuf,
    #[serde(default)]
    limits: Vec<Limit>,
}

/// `usage.record` (what `rade status --statusline` sends): one batch of
/// samples; only the ones worth a line are appended (`worth_recording`).
fn usage_record(p: RecordParams) -> Result<Value> {
    let agent = AgentKind::parse(&p.agent)
        .ok_or_else(|| Error::usage(format!("agent must be claude or codex, not '{}'", p.agent)))?;
    let path = store::store_path(
        &store::usage_dir(&rade_core::paths::process_env),
        agent,
        &p.config_dir,
    );
    let account = account_of_login(agent, &p.config_dir);
    let name = account
        .as_ref()
        .map(|a| a.name.clone())
        .unwrap_or_else(|| p.config_dir.display().to_string());
    let t = now_s();
    let limits: Vec<Limit> = p
        .limits
        .into_iter()
        .map(|mut l| {
            if l.t == 0 {
                l.t = t;
            }
            if l.source.is_empty() {
                l.source = "statusline".into();
            }
            l
        })
        .collect();
    let offered = limits.len();
    let lock = store_lock(&path);
    let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
    let (recs, events) = record_limits(&path, &name, limits);
    if offered > 0 {
        // A readable sample of this login: its logs' state no longer hides it.
        let mut st = stores();
        st.tracks.remove(&path);
        st.last_scan.insert(path.clone(), UsageState::Ok);
    }
    eprintln!(
        "rade: usage.record {} {}: {} of {offered} samples recorded",
        p.agent,
        name,
        recs.len()
    );
    if account.is_some() {
        push_events(events);
    }
    drop(_held);
    if !recs.is_empty() {
        usage_changed();
    }
    Ok(json!({"store": path, "samples": recs.len()}))
}

/// What a refresh needs of one account, read on the main loop: the
/// Workspace root (to turn Detailed limits off) and the checkouts whose
/// sessions count for a Project.
struct Target {
    account: Account,
    root: PathBuf,
    checkouts: Vec<(PathBuf, String)>,
}

/// The checkouts of Workspace `ws` → Project name: each Project's Source
/// main checkouts, its Worktrees, and (shown Workspace) every link's
/// checkout.
fn checkouts(m: &rade_core::State, ws: &str, root: &Path) -> Vec<(PathBuf, String)> {
    let projects = rade_core::project::list(root);
    let name_of = |id: &str| {
        projects
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| id.to_string())
    };
    let mut out = Vec::new();
    if m.active_workspace() == ws {
        for l in crate::navdata::links(m) {
            if let Some(p) = l.path {
                out.push((p, l.project_name.clone()));
            }
        }
    }
    for s in m.sources.iter().filter(|s| s.workspace == ws) {
        for w in &s.worktrees {
            if let Some(p) = &w.project {
                out.push((w.path.clone(), name_of(p)));
            }
        }
        if let Some(p) = projects.iter().find(|p| {
            p.sources
                .iter()
                .any(|x| x.source == s.id || x.source == s.name)
        }) {
            out.push((s.path.clone(), p.name.clone()));
        }
    }
    out
}

/// The refresh targets: `only` = one account (by agent + name) of one
/// Workspace, else every account of `ws`, else of every Workspace.
fn targets(ws: Option<String>, only: Option<(AgentKind, String)>) -> Vec<Target> {
    let Ok(wss) = all_workspaces() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (id, root) in wss {
        if ws.as_ref().is_some_and(|w| *w != id) {
            continue;
        }
        let Ok(list) = accounts_of(&id, &root) else {
            continue;
        };
        let list: Vec<Account> = list
            .into_iter()
            .filter(|a| {
                only.as_ref()
                    .is_none_or(|(k, n)| a.agent == *k && a.name == *n)
            })
            .collect();
        if list.is_empty() {
            continue;
        }
        let (id2, root2) = (id.clone(), root.clone());
        let cos = on_main(move || {
            let app = app();
            let m = app.model.borrow();
            Ok(checkouts(&m, &id2, &root2))
        })
        .unwrap_or_default();
        out.extend(list.into_iter().map(|account| Target {
            account,
            root: root.clone(),
            checkouts: cos.clone(),
        }));
    }
    out
}

/// One account's refresh: tail its logs (once per store per call), then
/// Detailed limits when on. Returns its `usage.refresh` row.
fn refresh_one(t: &Target, scanned: &mut Vec<PathBuf>) -> Value {
    let a = &t.account;
    let path = store_of(a);
    let lock = store_lock(&path);
    let now = now_s();
    let offset = rade_core::date::local_offset(now);
    let mut events = Vec::new();
    let (mut read_bytes, mut samples) = (0u64, 0usize);
    if !scanned.contains(&path) {
        scanned.push(path.clone());
        let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
        let book_file = store::book_path(&path);
        let mut book = tail::TailBook::load(&book_file);
        // Resets the clock passed since the last check (an idle account
        // gets no new sample to say so).
        let checked = stores().reset_checked.insert(path.clone(), now);
        if let Some(from) = checked {
            let samples: Vec<Limit> = store::read(&path)
                .into_iter()
                .filter_map(|r| match r {
                    Record::Limit { limit, .. } => Some(limit),
                    _ => None,
                })
                .collect();
            events.extend(limits::reset_events(&a.name, &samples, from, now));
        }
        // Logs untouched for the retention period are never read.
        let project_of = |cwd: &str| report::project_for(cwd, &t.checkouts);
        let scan = tail::scan_login(
            a.agent,
            &a.config_dir,
            &mut book,
            &project_of,
            offset,
            SystemTime::now(),
        );
        if let Err(e) = book.save(&book_file) {
            eprintln!("rade: usage tail {}: {e}", book_file.display());
        }
        read_bytes = scan.read_bytes;
        samples = scan.samples;
        {
            let mut st = stores();
            let track = st.tracks.entry(path.clone()).or_default();
            if let Some(state) = track.after(scan.lines, scan.samples) {
                st.last_scan.insert(path.clone(), state);
            }
        }
        let (_, ev) = record_limits(&path, &a.name, scan.limits);
        events.extend(ev);
        let tokens: Vec<Record> = scan
            .tokens
            .into_iter()
            .map(|row| Record::Tokens { t: now, row })
            .collect();
        if let Err(e) = store::append(&path, &tokens) {
            eprintln!("rade: usage store {}: {e}", path.display());
        }
    }
    if a.agent == AgentKind::Claude && a.detailed_limits {
        let test_mode = crate::handlers::DEBUG_SOCKET
            .get()
            .copied()
            .unwrap_or(false);
        let url = detailed::endpoint(&rade_core::paths::process_env, test_mode);
        let outcome = {
            let mut g = gate();
            detailed::refresh(&a.config_dir, &url, &mut g, now, &detailed::curl_fetch)
        };
        match outcome {
            detailed::Outcome::Limits(l) => {
                samples += l.len();
                let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
                let (_, ev) = record_limits(&path, &a.name, l);
                events.extend(ev);
            }
            detailed::Outcome::Disable(why) => {
                eprintln!(
                    "rade: Detailed limits of {}: {why}; turning them off",
                    a.name
                );
                if let Err(e) =
                    account::set_detailed_file(&t.root, AgentKind::Claude, &a.name, false)
                {
                    eprintln!("rade: workspace.toml: {}", e.message);
                }
                events.push(format!("account {}: Detailed limits off ({why})", a.name));
            }
            detailed::Outcome::Failed(e) => {
                eprintln!("rade: Detailed limits of {}: {e}", a.name)
            }
            detailed::Outcome::NotDue | detailed::Outcome::NoLogin => {}
        }
    }
    let records = {
        let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = store::prune(&path, now) {
            eprintln!("rade: usage store {}: {e}", path.display());
        }
        store::read(&path)
    };
    push_events(events);
    json!({
        "account": a.name, "agent": a.agent, "read_bytes": read_bytes, "samples": samples,
        "state": report::state_of(&records, last_scan(&path)).as_str(),
    })
}

fn refresh_all(targets: &[Target]) -> Vec<Value> {
    let mut scanned = Vec::new();
    let rows = targets
        .iter()
        .map(|t| refresh_one(t, &mut scanned))
        .collect();
    usage_changed();
    rows
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RefreshParams {
    workspace: Option<String>,
    caller: Option<String>,
    account: Option<String>,
    agent: Option<String>,
}

/// `usage.refresh`: tail now (one account, else the Workspace's).
fn usage_refresh(p: RefreshParams) -> Result<Value> {
    let agent = agent_arg(p.agent.as_deref())?;
    let (id, root) = workspace(p.workspace, p.caller)?;
    let only = match &p.account {
        Some(n) => {
            let l = accounts_of(&id, &root)?;
            let a = account::find(&l, n, agent, &id)?;
            Some((a.agent, a.name.clone()))
        }
        None => None,
    };
    let _one = refreshing();
    Ok(json!({"accounts": refresh_all(&targets(Some(id), only))}))
}

/// One refresh at a time (the poll, turn ends and `usage.refresh`).
fn refreshing() -> std::sync::MutexGuard<'static, ()> {
    static R: Mutex<()> = Mutex::new(());
    R.lock().unwrap_or_else(|e| e.into_inner())
}

/// Refresh in the background. While one runs, a new ask is not lost: the
/// running one does a full refresh once more when it ends (a turn end can
/// land just after its scan read that log).
fn refresh_in_background(ws: Option<String>, only: Option<(AgentKind, String)>) {
    static BUSY: AtomicBool = AtomicBool::new(false);
    static AGAIN: AtomicBool = AtomicBool::new(false);
    if BUSY.swap(true, Ordering::SeqCst) {
        AGAIN.store(true, Ordering::SeqCst);
        return;
    }
    std::thread::spawn(move || {
        let (mut ws, mut only) = (ws, only);
        loop {
            {
                let _one = refreshing();
                refresh_all(&targets(ws.take(), only.take()));
            }
            if !AGAIN.swap(false, Ordering::SeqCst) {
                break;
            }
        }
        BUSY.store(false, Ordering::SeqCst);
        // An ask between the last check and here starts its own run.
        if AGAIN.swap(false, Ordering::SeqCst) {
            refresh_in_background(None, None);
        }
    });
}

/// The 30 s poll of every declared account (R109.2).
pub fn install() {
    glib::timeout_add_seconds_local(30, || {
        refresh_in_background(None, None);
        glib::ControlFlow::Continue
    });
}

/// An Agent ended a turn (`done` / `waiting`): refresh its account now.
pub fn turn_ended(m: &rade_core::State, pane: &str) {
    if let Some(Ok(a)) = m.pane_account(pane, None, &home()) {
        refresh_in_background(Some(a.workspace.clone()), Some((a.agent, a.name.clone())));
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ListParams {
    workspace: Option<String>,
    caller: Option<String>,
    all: bool,
    period: Option<String>,
}

fn period(p: Option<&str>) -> Result<report::Period> {
    report::Period::parse(p).map_err(Error::usage)
}

fn tightest_json(l: &Limit) -> Value {
    json!({
        "limit_id": l.limit_id, "name": l.label(), "used_percent": l.used_percent,
        "resets_at": l.resets_at,
    })
}

/// `usage.list`: the accounts of the Workspace (or all), sorted by Tightest
/// limit (no data last), and Next resets.
fn usage_list(p: ListParams) -> Result<Value> {
    let per = period(p.period.as_deref())?;
    let wss: Vec<(String, PathBuf)> = if p.all {
        all_workspaces()?
    } else {
        vec![workspace(p.workspace, p.caller)?]
    };
    let mut accounts = Vec::new();
    for (id, root) in &wss {
        // One broken file fails the call only when it is the one asked.
        match accounts_of(id, root) {
            Ok(l) => accounts.extend(l),
            Err(e) if !p.all => return Err(e),
            Err(_) => {}
        }
    }
    let now = now_s();
    let offset = rade_core::date::local_offset(now);
    let mut rows = Vec::new();
    let mut resets_in = Vec::new();
    // (agent, workspace) of each `resets_in` entry: names repeat across
    // agents (R108.1).
    let mut owners = Vec::new();
    // Each store is read and summed on its own thread (20 accounts × 90
    // days within the Usage page's budget, R111-d).
    let figures: Vec<(
        report::Summary,
        Option<rade_core::usage::limits::AccountLimits>,
    )> = std::thread::scope(|sc| {
        let jobs: Vec<_> = accounts
            .iter()
            .map(|a| {
                sc.spawn(move || {
                    let path = store_of(a);
                    let records = view_records(&path);
                    let s = report::summary(&records, last_scan(&path), per, now, offset);
                    let l = (s.state == UsageState::Ok)
                        .then(|| report::account_limits(&a.name, &records, now));
                    (s, l)
                })
            })
            .collect();
        jobs.into_iter()
            .map(|j| j.join().expect("usage summary thread"))
            .collect()
    });
    for (a, (s, l)) in accounts.iter().zip(figures) {
        if let Some(l) = l {
            resets_in.push(l);
            owners.push((a.agent, a.workspace.clone()));
        }
        rows.push((
            s.tightest.as_ref().and_then(|t| t.used_percent),
            s.tightest.as_ref().and_then(|t| t.resets_at),
            json!({
                "agent": a.agent, "name": a.name, "workspace": a.workspace,
                "default": a.default, "plan": null,
                "tightest": s.tightest.as_ref().map(tightest_json),
                "tokens": s.tokens, "updated": s.updated, "state": s.state.as_str(),
            }),
        ));
    }
    // Highest first, ties → earlier reset, no data last (stable otherwise).
    rows.sort_by(|a, b| match (a.0, b.0) {
        (Some(x), Some(y)) => y
            .total_cmp(&x)
            .then(a.1.unwrap_or(i64::MAX).cmp(&b.1.unwrap_or(i64::MAX))),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    let resets: Vec<Value> = limits::next_resets(&resets_in, now, offset, 6)
        .into_iter()
        .map(|r| {
            let owner = resets_in
                .iter()
                .zip(&owners)
                .find(|(al, _)| {
                    al.account == r.account
                        && al.limits.iter().any(|(l, _)| {
                            l.limit_id == r.limit_id && l.resets_at == Some(r.resets_at)
                        })
                })
                .map(|(_, o)| o.clone());
            json!({
                "agent": owner.as_ref().map(|o| o.0),
                "workspace": owner.as_ref().map(|o| o.1.clone()),
                "account": r.account, "limit_id": r.limit_id, "name": r.name,
                "resets_at": r.resets_at, "used_percent": r.used_percent,
                "forecast": r.forecast, "when": r.when, "what": r.what, "note": r.note,
            })
        })
        .collect();
    Ok(json!({
        "accounts": rows.into_iter().map(|r| r.2).collect::<Vec<_>>(),
        "resets": resets,
    }))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct GetParams {
    workspace: Option<String>,
    caller: Option<String>,
    name: Option<String>,
    account: Option<String>,
    agent: Option<String>,
    period: Option<String>,
}

fn limit_json(l: &Limit) -> Value {
    let mut v = serde_json::to_value(l).unwrap_or_default();
    v["sampled_at"] = json!(l.t);
    v["label"] = json!(l.label());
    if let Some(o) = v.as_object_mut() {
        o.remove("t");
    }
    v
}

/// `usage.get`: one account's latest limits (also grouped as spec E.30) and
/// its tokens for the period.
fn usage_get(n: GetParams) -> Result<Value> {
    let name = n
        .account
        .or(n.name)
        .ok_or_else(|| Error::usage("usage.get needs an account"))?;
    let agent = agent_arg(n.agent.as_deref())?;
    let per = period(n.period.as_deref())?;
    let (id, root) = workspace(n.workspace, n.caller)?;
    let l = accounts_of(&id, &root)?;
    let a = account::find(&l, &name, agent, &id)?;
    let path = store_of(a);
    let records = view_records(&path);
    let now = now_s();
    let offset = rade_core::date::local_offset(now);
    let s = report::summary(&records, last_scan(&path), per, now, offset);
    let latest = if s.state == UsageState::Ok {
        limits::as_of(store::latest(&records), now)
    } else {
        Vec::new()
    };
    let groups: Vec<Value> = report::groups(&latest)
        .into_iter()
        .map(|(g, v)| json!({"group": g, "limits": v.iter().map(limit_json).collect::<Vec<_>>()}))
        .collect();
    Ok(json!({
        "account": a.name, "agent": a.agent, "workspace": id, "config_dir": a.config_dir,
        "store": path, "plan": null, "detailed_limits": a.detailed_limits,
        "limits": latest.iter().map(limit_json).collect::<Vec<_>>(),
        "groups": groups,
        "tightest": s.tightest.as_ref().map(tightest_json),
        "tokens": report::tokens(&records, per, now, offset),
        "updated": s.updated,
        "state": s.state.as_str(),
    }))
}

// ---- What the UI shows (R110.2–R110.4) ---------------------------------------

/// Every Workspace's accounts (with the in-memory defaults for later
/// Agents), read at most every 2 s: the Statusline and the titlebar ask on
/// every sync.
pub fn lists_cached(
    m: &rade_core::State,
) -> std::collections::BTreeMap<String, Result<Vec<Account>>> {
    type Cached = (
        std::time::Instant,
        (std::collections::BTreeMap<String, String>, u64),
        Vec<String>,
        std::collections::BTreeMap<String, Result<Vec<Account>>>,
    );
    thread_local! {
        static C: std::cell::RefCell<Option<Cached>> = const { std::cell::RefCell::new(None) };
    }
    let wss: Vec<String> = m.workspaces.iter().map(|w| w.id.clone()).collect();
    let key = (m.next_defaults.clone(), LISTS_EPOCH.load(Ordering::SeqCst));
    C.with(|c| {
        let mut c = c.borrow_mut();
        if let Some((at, k, ids, l)) = &*c {
            if at.elapsed() < Duration::from_secs(2) && *k == key && *ids == wss {
                return l.clone();
            }
        }
        let l = m.account_lists(&home());
        // New or changed accounts: read their stores off the main loop now,
        // so the + menu and the usage cell find them cached (R111-d).
        let changed = c.as_ref().is_none_or(|(_, _, _, old)| *old != l);
        if changed {
            let all: Vec<Account> = l.values().flatten().flatten().cloned().collect();
            std::thread::spawn(move || warm(&all));
        }
        *c = Some((std::time::Instant::now(), key, wss, l.clone()));
        l
    })
}

/// Fill [`snapshot`]'s cache for `accounts` (off the main loop).
fn warm(accounts: &[Account]) {
    for a in accounts {
        snapshot(a);
    }
}

/// Forget [`lists_cached`] (an account was added or a default changed).
pub fn forget_lists() {
    LISTS_EPOCH.fetch_add(1, Ordering::SeqCst);
}

static LISTS_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// An account's store: its state and latest limit per limit, cached by
/// the store file's size and time (read on the main loop at most once
/// per change).
pub fn snapshot(a: &Account) -> (UsageState, Vec<Limit>) {
    type Snap = ((u64, Option<SystemTime>), bool, Vec<Limit>);
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Snap>>> = OnceLock::new();
    let path = store_of(a);
    let meta = std::fs::metadata(&path).ok();
    let stamp = (
        meta.as_ref().map_or(0, |m| m.len()),
        meta.and_then(|m| m.modified().ok()),
    );
    let cache = || {
        CACHE
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    };
    let hit = match cache().get(&path) {
        Some((s, e, l)) if *s == stamp => Some((*e, l.clone())),
        _ => None,
    };
    // Read without the lock: a store being read off the main loop never
    // holds up the main loop's own lookups.
    let (empty, latest) = hit.unwrap_or_else(|| {
        let records = view_records(&path);
        let latest = store::latest(&records);
        cache().insert(path.clone(), (stamp, records.is_empty(), latest.clone()));
        (records.is_empty(), latest)
    });
    let state = match last_scan(&path) {
        Some(UsageState::FormatChanged) => UsageState::FormatChanged,
        _ if empty => UsageState::NoData,
        _ => UsageState::Ok,
    };
    let latest = if state == UsageState::Ok {
        limits::as_of(latest, now_s())
    } else {
        Vec::new()
    };
    (state, latest)
}

/// What the UI reads of a store ([`store::for_view`]: token rows, the
/// newest sample and the current window of each limit), cached by the
/// file's size and time and shared by every thread (R111-d: 20 accounts ×
/// 90 days are read once per change, normally off the main loop by
/// [`warm`]).
fn view_records(path: &Path) -> Arc<Vec<Record>> {
    type Stamp = (u64, Option<SystemTime>);
    type Views = HashMap<PathBuf, (Stamp, Arc<Vec<Record>>)>;
    static CACHE: OnceLock<Mutex<Views>> = OnceLock::new();
    let cache = || {
        CACHE
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    };
    let meta = std::fs::metadata(path).ok();
    let stamp = (
        meta.as_ref().map_or(0, |m| m.len()),
        meta.and_then(|m| m.modified().ok()),
    );
    if let Some((s, v)) = cache().get(path) {
        if *s == stamp {
            return v.clone();
        }
    }
    let v = Arc::new(store::for_view(&store::read(path), now_s()));
    cache().insert(path.to_path_buf(), (stamp, v.clone()));
    v
}

/// The `+` menu detail of an own account (F.8–F.10).
pub fn menu_detail(a: &Account) -> Option<(String, bool)> {
    let (state, latest) = snapshot(a);
    Some(rade_core::usage::view::menu_detail(state, &latest))
}

/// The focused Agent's account and its Statusline usage cell (R110.3):
/// `None` when the focused Pane has no account or the account no figures.
pub fn focused_cell(m: &rade_core::State) -> Option<rade_core::usage::view::UsageCell> {
    let pane = m.active_tab()?.focused.clone();
    let p = m.pane(&pane)?;
    let name = p.account.clone()?;
    let (source, _) = m.pane_worktree(&pane)?;
    let ws = m.source(&source)?.workspace.clone();
    let lists = lists_cached(m);
    let l = lists.get(&ws)?.as_ref().ok()?;
    let a = account::find(l, &name, p.account_agent, &ws).ok()?;
    let (_, latest) = snapshot(a);
    rade_core::usage::view::status_cell(a.agent, &a.name, &latest)
}

/// Whether any Workspace declares an Agent Account (the titlebar `gauge`
/// button shows then).
pub fn any_accounts(m: &rade_core::State) -> bool {
    lists_cached(m)
        .values()
        .any(|l| l.as_ref().is_ok_and(|l| !l.is_empty()))
}

/// Forecast Toasts already raised: once per limit per window (F.15–F.20).
fn forecasts_shown() -> std::sync::MutexGuard<'static, Vec<String>> {
    static S: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    S.get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// After new samples (off the main loop): the shown Workspace's forecast
/// Toast when one is due, then the UI refreshes what it shows.
fn usage_changed() {
    // Stores that changed are read here rather than on the main loop.
    if let Ok(wss) = all_workspaces() {
        let all: Vec<Account> = wss
            .iter()
            .filter_map(|(id, root)| accounts_of(id, root).ok())
            .flatten()
            .collect();
        warm(&all);
    }
    let toast = (|| {
        let (ws, root) = on_main(|| {
            let app = app();
            let m = app.model.borrow();
            let id = m.active_workspace().to_string();
            let root = m
                .workspace(&id)
                .map(|w| w.root.clone())
                .ok_or_else(|| Error::not_found("no workspace"))?;
            Ok((id, root))
        })
        .ok()?;
        let list = accounts_of(&ws, &root).ok()?;
        let now = now_s();
        let offset = rade_core::date::local_offset(now);
        let accounts: Vec<rade_core::usage::view::AccountUsage> = list
            .iter()
            .map(|a| {
                let path = store_of(a);
                let records = view_records(&path);
                let ok = report::state_of(&records, last_scan(&path)) == UsageState::Ok;
                rade_core::usage::view::AccountUsage {
                    agent: Some(a.agent),
                    name: a.name.clone(),
                    limits: if ok {
                        report::account_limits(&a.name, &records, now).limits
                    } else {
                        Vec::new()
                    },
                }
            })
            .collect();
        let mut shown = forecasts_shown();
        let t = rade_core::usage::view::forecast_toast(&accounts, &shown, now, offset)?;
        shown.push(t.key.clone());
        Some((ws, t))
    })();
    glib::MainContext::default().invoke(move || {
        let Some(app) = crate::app::try_app() else {
            return;
        };
        if let Some((ws, t)) = toast {
            app.chrome.forecast_toast(&ws, t);
        }
        app.chrome.sync(&app, &app.model.borrow());
        crate::usage_view::refresh();
    });
}

/// `debug.ui {"op": "forecast_check", "arg": "reset"?}` (test mode): run the
/// forecast check now (off the main loop); `reset` forgets the Toasts
/// already raised first (a golden scene shows its Toast again).
pub fn debug_forecast(reset: bool) {
    if reset {
        forecasts_shown().clear();
    }
    std::thread::spawn(usage_changed);
}
