//! Schedules (PRD R137–R138, ADR-0051): the 15 s check that fires due
//! Schedule Runs into Agent Panes, and the `schedule.*` socket methods. The
//! model, cron and validation are `rade_core::schedule`'s. Runs and every
//! method run on a socket / worker thread (resolving a link's checkout runs
//! git); the model is touched on the main loop through `on_main`.

use crate::app::app;
use crate::handlers::{on_main, params, plan_spawn, type_when_settled, Spawn};
use gtk::glib;
use gtk4 as gtk;
use rade_core::activity::Event;
use rade_core::rpc::{Error, Result};
use rade_core::schedule::{self, Ctx, Due, Outcome, Run, RunPane, Schedule, Spec};
use rade_core::State;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};

/// The check interval (R137.1).
const TICK_SECS: u32 = 15;

/// The test clock's offset (API.md "Test clock"), test mode only.
static OFFSET: AtomicI64 = AtomicI64::new(0);
static STARTED: AtomicBool = AtomicBool::new(false);
/// One check at a time (the timer's and `debug.schedule tick`).
static TICKING: Mutex<()> = Mutex::new(());
/// One run at a time (a tick's, `schedule.run`'s): the busy check and the
/// Pane a run makes can't interleave with another run's.
static FIRING: Mutex<()> = Mutex::new(());

fn test_mode() -> bool {
    crate::handlers::DEBUG_SOCKET
        .get()
        .copied()
        .unwrap_or(false)
}

/// The Schedules' clock: Unix seconds, moved by the test clock.
pub fn now() -> i64 {
    static ENV: OnceLock<i64> = OnceLock::new();
    let env = *ENV.get_or_init(|| {
        if !test_mode() {
            return 0;
        }
        std::env::var("RADE_TEST_CLOCK_OFFSET")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0)
    });
    let real = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    real + env + OFFSET.load(Ordering::Relaxed)
}

pub fn tz() -> &'static rade_core::schedule::jiff::tz::TimeZone {
    static TZ: OnceLock<rade_core::schedule::jiff::tz::TimeZone> = OnceLock::new();
    TZ.get_or_init(rade_core::schedule::jiff::tz::TimeZone::system)
}

/// Start the checks once Staged Session Restore's queue is empty (R137.1).
pub fn start() {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    tick_in_background();
    glib::timeout_add_seconds_local(TICK_SECS, || {
        tick_in_background();
        glib::ControlFlow::Continue
    });
}

fn tick_in_background() {
    std::thread::spawn(|| {
        let _ = tick();
    });
}

/// One check (R137): what is due, then each run, off the main loop.
fn tick() -> Result<Vec<(String, Run)>> {
    let Ok(_guard) = TICKING.try_lock() else {
        return Ok(Vec::new());
    };
    let now = now();
    let due = on_main(move || {
        let app = app();
        let mut m = app.model.borrow_mut();
        refresh_runs(&mut m);
        let mut due = Vec::new();
        for s in m.schedules.iter_mut() {
            if !s.enabled || s.done {
                continue;
            }
            let d = s.due(now, tz());
            // Saved with the next save, not every 15 s (R136.4).
            s.checked = now;
            if d != Due::Nothing {
                due.push((s.workspace.clone(), s.name.clone(), d));
            }
        }
        Ok(due)
    })?;
    let mut out = Vec::new();
    for (ws, name, d) in due {
        let run = match d {
            Due::Start { at: _, missed } => fire(&ws, &name, missed),
            Due::Missed { count, .. } => {
                let mut r = Run::new(0, now, Outcome::Missed, Some(missed_text(count)));
                r.missed = Some(count);
                record(&ws, &name, r)
            }
            Due::Nothing => continue,
        };
        if let Ok(r) = run {
            out.push((name, r));
        }
    }
    Ok(out)
}

fn missed_text(count: u32) -> String {
    if count == 1 {
        "1 run missed".into()
    } else {
        format!("{count} runs missed")
    }
}

/// Copy each run Pane's Agent Status onto its Run; a closed Pane leaves
/// `pane` null and keeps the last status (R137.4).
fn refresh_runs(m: &mut State) {
    // A session a hook reported (Codex) is kept before its Pane is gone.
    fill_sessions(m);
    let status: Vec<(String, Option<Option<rade_core::AgentStatus>>)> = m
        .schedules
        .iter()
        .flat_map(|s| s.runs.iter())
        .flat_map(|r| r.pane.iter().chain(r.resumed.iter()))
        .map(|p| (p.clone(), m.pane(p).map(|x| x.status)))
        .collect();
    let get = |p: &str| status.iter().find(|x| x.0 == p).and_then(|x| x.1);
    for s in m.schedules.iter_mut() {
        for r in s.runs.iter_mut() {
            if let Some(p) = r.pane.clone() {
                match get(&p) {
                    Some(st) => r.status = st.or(r.status),
                    None => r.pane = None,
                }
            }
            if let Some(p) = r.resumed.clone() {
                if get(&p).is_none() {
                    r.resumed = None;
                }
            }
        }
    }
}

/// Number a Run, record it, save, and say it in Activity (R137.3–R137.4).
fn record(ws: &str, name: &str, mut run: Run) -> Result<Run> {
    let (ws, name) = (ws.to_string(), name.to_string());
    on_main(move || {
        let app = app();
        let r = app.update(|m| {
            let s = m
                .schedule_mut(&ws, &name)
                .ok_or_else(|| Error::not_found(format!("no schedule '{name}'")))?;
            if run.n == 0 {
                run.n = s.next_n();
            }
            s.record(run.clone());
            Ok::<_, Error>(run)
        })?;
        app.save_now();
        crate::activity_view::push(Event::schedule(&name, &r));
        if r.outcome == Outcome::Failed {
            let (ws2, name2) = (ws.clone(), name.clone());
            app.chrome.toast(
                &glib::markup_escape_text(&format!("Schedule {name} failed")),
                Some((
                    "Open",
                    Box::new(move || crate::schedules_view::open(Some((&ws2, &name2)))),
                )),
            );
        }
        crate::schedules_view::refresh();
        Ok(r)
    })
}

/// Where a run goes: `(source, cwd, project label)`. A link's checkout is
/// resolved now (R98; may create the Project checkout); a folder goes to
/// the Workspace's Scratch (R137.2).
fn target(s: &Schedule) -> Result<(String, PathBuf)> {
    if let Some(dir) = &s.dir {
        if !dir.is_dir() {
            return Err(Error::usage(format!(
                "--dir '{}' is not a folder",
                dir.display()
            )));
        }
        return Ok((rade_core::general_id(&s.workspace), dir.clone()));
    }
    let (ws, project, link) = (
        s.workspace.clone(),
        s.project.clone().unwrap_or_default(),
        s.link.clone().unwrap_or_default(),
    );
    let (pid, sid, sc) = on_main(move || {
        let app = app();
        let m = app.model.borrow();
        let p = m
            .find_project(Some(&ws), &project)
            .map_err(|_| Error::not_found(format!("no project '{project}' in workspace '{ws}'")))?;
        let sid = m.find_source(&link)?.id.clone();
        if !p.lists(&sid) {
            return Err(Error::not_found(format!(
                "project '{}' has no link '{sid}'",
                p.id
            )));
        }
        Ok((p.id.clone(), sid, crate::projects::scene(&m, &ws)))
    })?;
    let r = crate::projects::resolve(sc, None);
    let res = r
        .get(&pid, &sid)
        .cloned()
        .ok_or_else(|| Error::not_found(format!("project '{pid}' has no link '{sid}'")))?;
    if let Some(e) = res.error() {
        return Err(e);
    }
    for c in &res.commands {
        rade_core::checkout::apply(c)?;
    }
    let r = if res.create {
        let ws = s.workspace.clone();
        let sc = on_main(move || Ok(crate::projects::scene(&app().model.borrow(), &ws)))?;
        crate::projects::resolve(sc, None)
    } else {
        r
    };
    crate::projects::record(&r);
    Ok((sid, res.path.clone().unwrap_or_default()))
}

/// Fire one run now (R137.2–R137.4); `missed` > 0 = a catch-up run.
fn fire(ws: &str, name: &str, missed: u32) -> Result<Run> {
    let _one = FIRING.lock().unwrap_or_else(|e| e.into_inner());
    let started = now();
    let (ws2, name2) = (ws.to_string(), name.to_string());
    // The Run number is taken now: the Pane's name and the Run agree.
    let (s, n) = on_main(move || {
        let app = app();
        let mut m = app.model.borrow_mut();
        refresh_runs(&mut m);
        let s = m.schedule_mut(&ws2, &name2).ok_or_else(|| {
            Error::not_found(format!("no schedule '{name2}' in workspace '{ws2}'"))
        })?;
        let n = s.next_n();
        Ok((s.clone(), n))
    })?;
    let skip = |reason: &str| {
        record(
            ws,
            name,
            Run::new(n, started, Outcome::Skipped, Some(reason.into())),
        )
    };
    let fail = |e: Error| {
        record(
            ws,
            name,
            Run::new(n, started, Outcome::Failed, Some(e.message)),
        )
    };
    // Busy previous run, paused Source: before any git (R137.3).
    let pre = {
        let s = s.clone();
        on_main(move || {
            let app = app();
            let m = app.model.borrow();
            if s.busy_pane(|p| m.pane(p).map(|x| x.status)).is_some() {
                return Ok(Some("previous run still running"));
            }
            let source = match (&s.dir, &s.link) {
                (Some(_), _) => Some(rade_core::general_id(&s.workspace)),
                (None, Some(l)) => m.find_source(l).ok().map(|x| x.id.clone()),
                _ => None,
            };
            if source.is_some_and(|src| m.check_not_paused(&src).is_err()) {
                return Ok(Some("source paused"));
            }
            Ok(None)
        })?
    };
    if let Some(reason) = pre {
        return skip(reason);
    }
    let (source, cwd) = match target(&s) {
        Ok(t) => t,
        Err(e) => return fail(e),
    };
    let s2 = s.clone();
    let placed = on_main(move || {
        let app = app();
        let spawn = Spawn {
            source: Some(source.clone()),
            worktree: None,
            tab: None,
            split: None,
            agent: Some(s2.agent.clone()),
            cmd: None,
            name: None,
            prompt: Some(s2.prompt.clone()),
            account: s2.account.clone(),
            caller: None,
        };
        let at = (source.clone(), None);
        let plan = plan_spawn(&app, &spawn, Some(&at))?;
        let account = plan.account.as_ref().map(|a| a.name.clone());
        let (tab, pane) = app.update(|m| {
            m.spawn_scheduled(RunPane {
                source: source.clone(),
                cwd: cwd.clone(),
                command: plan.command.clone(),
                name: format!("{}-{n}", s2.name),
                agent: s2.agent.clone(),
                session_id: plan.session_id.clone(),
                account: plan.account.clone(),
                schedule: s2.name.clone(),
                status: Some(rade_core::AgentStatus::Running),
                now: started as u64,
            })
        })?;
        let _ = app.update(|m| m.set_task(&pane, &s2.prompt));
        if let Some(text) = plan.typed {
            type_when_settled(pane.clone(), text, false);
        }
        let _ = tab;
        Ok((pane, plan.session_id, account))
    });
    let (pane, session, account) = match placed {
        Ok(x) => x,
        Err(e) => return fail(e),
    };
    let mut run = Run::new(n, started, Outcome::Started, None);
    if missed > 0 {
        run.reason = Some(format!("catch-up for {}", missed_text(missed)));
        run.missed = Some(missed);
    }
    run.pane = Some(pane);
    run.session = session;
    run.account = account;
    run.status = Some(rade_core::AgentStatus::Running);
    let r = record(ws, name, run)?;
    close_old(ws, name);
    Ok(r)
}

/// Close finished run Panes past `keep_runs` (R137.4).
fn close_old(ws: &str, name: &str) {
    let (ws, name) = (ws.to_string(), name.to_string());
    let _ = on_main(move || {
        let app = app();
        let close = {
            let m = app.model.borrow();
            let Some(s) = m.schedule(&ws, &name) else {
                return Ok(());
            };
            s.panes_to_close(|p| m.pane(p).map(|x| x.status))
        };
        for p in close {
            let _ = app.close_pane(&p, false);
        }
        Ok(())
    });
}

/// A session id reported later by a hook (Presets without
/// `session_id_flag`): copy it onto the Run of that Pane.
fn fill_sessions(m: &mut State) {
    let found: Vec<(String, String)> = m
        .schedules
        .iter()
        .flat_map(|s| s.runs.iter())
        .filter(|r| r.session.is_none())
        .filter_map(|r| r.pane.clone())
        .filter_map(|p| Some((p.clone(), m.pane(&p)?.session_id.clone()?)))
        .collect();
    for s in m.schedules.iter_mut() {
        for r in s.runs.iter_mut() {
            if let Some((_, sid)) = found.iter().find(|(p, _)| r.pane.as_ref() == Some(p)) {
                r.session = Some(sid.clone());
            }
        }
    }
}

/// `schedule.resume` (R137.6).
fn resume(ws: &str, name: &str, n: Option<u32>) -> Result<Value> {
    let (ws, name) = (ws.to_string(), name.to_string());
    on_main(move || {
        let app = app();
        let s = {
            let mut m = app.model.borrow_mut();
            refresh_runs(&mut m);
            fill_sessions(&mut m);
            m.schedule(&ws, &name).cloned().ok_or_else(|| {
                Error::not_found(format!("no schedule '{name}' in workspace '{ws}'"))
            })?
        };
        let run = match n {
            Some(n) => s
                .run(n)
                .cloned()
                .ok_or_else(|| Error::not_found(format!("schedule '{name}' has no run {n}")))?,
            None => s
                .last_started()
                .cloned()
                .ok_or_else(|| Error::usage(format!("schedule '{name}' has no started run")))?,
        };
        let open = run
            .resumed
            .clone()
            .filter(|p| app.model.borrow().pane(p).is_some())
            .or_else(|| {
                run.pane
                    .clone()
                    .filter(|p| app.model.borrow().pane(p).is_some())
            });
        if let Some(p) = open {
            app.update(|m| {
                let loc = m.locate(&p).ok_or_else(|| Error::not_found("pane gone"))?;
                m.select_source(&loc.source)?;
                m.focus_pane(&p)
            })?;
            return Ok(json!({"pane": p, "resumed": false}));
        }
        let Some(session) = run.session.clone() else {
            return Err(Error::usage(format!(
                "run {} has no session to resume",
                run.n
            )));
        };
        // The Tab's Source: the Run's own Pane is gone, so the Schedule's
        // Tab, else its target now.
        let source = {
            let m = app.model.borrow();
            m.schedule_tabs(&ws, &name)
                .first()
                .map(|t| t.0.clone())
                .or_else(|| match (&s.dir, &s.link) {
                    (Some(_), _) => Some(rade_core::general_id(&ws)),
                    (None, Some(l)) => m.find_source(l).ok().map(|x| x.id.clone()),
                    _ => None,
                })
                .ok_or_else(|| Error::not_found(format!("schedule '{name}' has no target")))?
        };
        app.model.borrow().check_not_paused(&source)?;
        let cwd = {
            let m = app.model.borrow();
            match &s.dir {
                Some(d) => d.clone(),
                None => m
                    .schedule_tabs(&ws, &name)
                    .first()
                    .and_then(|(src, w, t)| {
                        let tab = m.worktree(src, w)?.tab(t)?;
                        tab.panes().iter().find_map(|p| match &p.kind {
                            rade_core::PaneKind::Terminal { cwd, .. } => Some(cwd.clone()),
                            _ => None,
                        })
                    })
                    .or_else(|| m.source(&source).map(|x| x.worktrees[0].path.clone()))
                    .unwrap_or_default(),
            }
        };
        let spawn = Spawn {
            source: Some(source.clone()),
            worktree: None,
            tab: None,
            split: None,
            agent: Some(s.agent.clone()),
            cmd: None,
            name: None,
            prompt: None,
            account: run.account.clone(),
            caller: None,
        };
        let at = (source.clone(), None);
        let mut plan = plan_spawn(&app, &spawn, Some(&at))?;
        // R137.6: the Run's own account; a Run that had none resumes with
        // none, not today's default (another config dir has no such session).
        if run.account.is_none() {
            plan.account = None;
        }
        let preset = app
            .config_for(&source, None)
            .presets()
            .map_err(Error::usage)?
            .get(&s.agent)
            .cloned()
            .ok_or_else(|| Error::usage(format!("unknown agent '{}'", s.agent)))?;
        let command = preset
            .resume_command(&session)
            .ok_or_else(|| Error::usage(format!("agent '{}' cannot resume a session", s.agent)))?;
        if let Some(a) = &plan.account {
            let home = rade_core::paths::home_dir(&rade_core::paths::process_env);
            rade_core::account::check_command(&s.agent, &command, a, &home)?;
        }
        let (_, pane) = app.update(|m| {
            m.spawn_scheduled(RunPane {
                source: source.clone(),
                cwd,
                command: Some(command),
                name: format!("{name}-{}-r", run.n),
                agent: s.agent.clone(),
                session_id: Some(session.clone()),
                account: plan.account.clone(),
                schedule: name.clone(),
                status: None,
                now: now() as u64,
            })
        })?;
        app.update(|m| {
            if let Some(r) = m.schedule_mut(&ws, &name).and_then(|x| x.run_mut(run.n)) {
                r.resumed = Some(pane.clone());
            }
        });
        app.save_now();
        crate::schedules_view::refresh();
        Ok(json!({"pane": pane, "resumed": true}))
    })
}

// ---- socket methods (API.md `rade schedule`) ----

#[derive(Deserialize)]
struct Named {
    workspace: Option<String>,
    name: String,
    caller: Option<String>,
    #[serde(default)]
    n: Option<u32>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    close_panes: bool,
}

#[derive(Deserialize)]
struct Scope {
    workspace: Option<String>,
    caller: Option<String>,
    #[serde(default)]
    all: bool,
    project: Option<String>,
    #[serde(default)]
    scratch: bool,
}

#[derive(Deserialize)]
struct Debug {
    op: String,
    #[serde(default)]
    offset: Option<i64>,
}

pub fn dispatch(method: &str, p: Value) -> Result<Value> {
    match method {
        "schedule.add" => {
            let caller = p.get("caller").and_then(Value::as_str).map(String::from);
            let spec: Spec = params(p)?;
            add(spec, caller)
        }
        "schedule.edit" => {
            let caller = p.get("caller").and_then(Value::as_str).map(String::from);
            let spec: Spec = params(p)?;
            edit(spec, caller)
        }
        "schedule.list" => list(params(p)?),
        "schedule.get" => get(params(p)?),
        "schedule.set_enabled" => set_enabled(params(p)?),
        "schedule.remove" => remove(params(p)?),
        "schedule.run" => {
            let n: Named = params(p)?;
            let ws = workspace_of(n.workspace, n.caller)?;
            exists(&ws, &n.name)?;
            let r = fire(&ws, &n.name, 0)?;
            Ok(serde_json::to_value(r).unwrap())
        }
        "schedule.runs" => runs(params(p)?),
        "schedule.resume" => {
            let n: Named = params(p)?;
            let ws = workspace_of(n.workspace, n.caller)?;
            resume(&ws, &n.name, n.n)
        }
        "debug.schedule" => {
            let d: Debug = params(p)?;
            match d.op.as_str() {
                "clock" => {
                    OFFSET.store(d.offset.unwrap_or(0), Ordering::Relaxed);
                    Ok(json!({"now": now()}))
                }
                "tick" => {
                    let runs = tick()?;
                    Ok(
                        json!({"now": now(), "runs": runs.into_iter().map(|(name, r)| {
                        let mut v = serde_json::to_value(r).unwrap();
                        v["schedule"] = json!(name);
                        v
                    }).collect::<Vec<_>>()}),
                    )
                }
                o => Err(Error::usage(format!("unknown debug.schedule op '{o}'"))),
            }
        }
        _ => Err(Error::usage(format!("unknown method {method}"))),
    }
}

fn workspace_of(q: Option<String>, caller: Option<String>) -> Result<String> {
    on_main(move || {
        let app = app();
        let m = app.model.borrow();
        crate::projects::target_workspace(&m, q.as_deref(), caller.as_deref())
    })
}

fn exists(ws: &str, name: &str) -> Result<()> {
    let (ws, name) = (ws.to_string(), name.to_string());
    on_main(move || {
        let app = app();
        let m = app.model.borrow();
        m.schedule(&ws, &name)
            .map(|_| ())
            .ok_or_else(|| Error::not_found(format!("no schedule '{name}' in workspace '{ws}'")))
    })
}

/// The account a run would use now, for the Preset's agent (R136.1).
fn account_now(app: &crate::app::App, m: &State, s: &Schedule) -> Result<Option<String>> {
    use rade_core::account::{AgentKind, SpawnAsk};
    let kind = app
        .config
        .borrow()
        .presets()
        .ok()
        .and_then(|p| p.get(&s.agent).map(|x| x.hooks))
        .and_then(AgentKind::of_hooks);
    let home = rade_core::paths::home_dir(&rade_core::paths::process_env);
    let ask = SpawnAsk {
        workspace: &s.workspace,
        agent: kind,
        explicit: s.account.as_deref(),
        parent: None,
    };
    Ok(m.resolve_account(&ask, &home)?.map(|a| a.name))
}

fn object(app: &crate::app::App, m: &State, s: &Schedule) -> Value {
    let acc = account_now(app, m, s).ok().flatten();
    s.to_json(now(), tz(), acc.as_deref())
}

/// Validate `spec` against Workspace `ws` (R136.5) on the main loop.
fn with_ctx<R>(
    m: &State,
    app: &crate::app::App,
    ws: &str,
    skip: Option<&str>,
    f: impl FnOnce(&Ctx) -> Result<R>,
) -> Result<R> {
    use rade_core::account::{AgentKind, SpawnAsk};
    let root = m.find_workspace(ws)?.root.clone();
    let projects: Vec<(String, Vec<String>)> = rade_core::project::list(&root)
        .into_iter()
        .map(|p| {
            let links = p.sources.iter().map(|s| s.source.clone()).collect();
            (p.id, links)
        })
        .collect();
    let presets_map = app.config.borrow().presets().map_err(Error::usage)?;
    let presets: Vec<String> = presets_map.keys().cloned().collect();
    let names: Vec<String> = m
        .schedules
        .iter()
        .filter(|s| s.workspace == ws && Some(s.name.as_str()) != skip)
        .map(|s| s.name.clone())
        .collect();
    let home = rade_core::paths::home_dir(&rade_core::paths::process_env);
    let account = |agent: &str, name: &str| -> Result<()> {
        let kind = presets_map
            .get(agent)
            .and_then(|p| AgentKind::of_hooks(p.hooks));
        let ask = SpawnAsk {
            workspace: ws,
            agent: kind,
            explicit: Some(name),
            parent: None,
        };
        m.resolve_account(&ask, &home).map(|_| ())
    };
    let ctx = Ctx {
        workspace: ws,
        projects: &projects,
        presets: &presets,
        names: &names,
        now: now(),
        tz: tz(),
        account: &account,
    };
    f(&ctx)
}

/// A Project given by name → its id in Workspace `ws` (left as is when
/// unknown: validation says `NOT_FOUND`).
fn project_id(m: &State, ws: &str, spec: &mut Spec) {
    if let Some(q) = spec.project.clone() {
        if let Ok(p) = m.find_project(Some(ws), &q) {
            spec.project = Some(p.id);
        }
    }
    // Only the Workspace's own Sources: a name may repeat in another.
    if let Some(l) = spec.link.clone() {
        let own = m
            .sources
            .iter()
            .filter(|s| s.workspace == ws)
            .find(|s| s.id == l || s.name == l);
        if let Some(s) = own {
            spec.link = Some(s.id.clone());
        }
    }
}

fn add(mut spec: Spec, caller: Option<String>) -> Result<Value> {
    on_main(move || {
        let app = app();
        let s = {
            let m = app.model.borrow();
            let ws = crate::projects::target_workspace(
                &m,
                spec.workspace.as_deref(),
                caller.as_deref(),
            )?;
            project_id(&m, &ws, &mut spec);
            with_ctx(&m, &app, &ws, None, |ctx| schedule::create(&spec, ctx))?
        };
        app.update(|m| m.schedules.push(s.clone()));
        app.save_now();
        crate::schedules_view::refresh();
        let m = app.model.borrow();
        Ok(object(&app, &m, &s))
    })
}

fn edit(mut spec: Spec, caller: Option<String>) -> Result<Value> {
    on_main(move || {
        let app = app();
        let name = spec
            .name
            .clone()
            .ok_or_else(|| Error::usage("which schedule? (name)"))?;
        let (ws, new) = {
            let m = app.model.borrow();
            let ws = crate::projects::target_workspace(
                &m,
                spec.workspace.as_deref(),
                caller.as_deref(),
            )?;
            let old = m
                .schedule(&ws, &name)
                .ok_or_else(|| {
                    Error::not_found(format!("no schedule '{name}' in workspace '{ws}'"))
                })?
                .clone();
            project_id(&m, &ws, &mut spec);
            let new = with_ctx(&m, &app, &ws, Some(&name), |ctx| {
                schedule::edit(&old, &spec, ctx)
            })?;
            (ws, new)
        };
        app.update(|m| {
            if new.name != name {
                m.rename_schedule(&ws, &name, &new.name);
            }
            if let Some(s) = m.schedule_mut(&ws, &name) {
                *s = new.clone();
            }
        });
        app.save_now();
        crate::schedules_view::refresh();
        let m = app.model.borrow();
        Ok(object(&app, &m, &new))
    })
}

fn set_enabled(n: Named) -> Result<Value> {
    let on = n
        .enabled
        .ok_or_else(|| Error::usage("set_enabled needs \"enabled\""))?;
    on_main(move || {
        let app = app();
        let ws = crate::projects::target_workspace(
            &app.model.borrow(),
            n.workspace.as_deref(),
            n.caller.as_deref(),
        )?;
        let new = {
            let m = app.model.borrow();
            let old = m
                .schedule(&ws, &n.name)
                .ok_or_else(|| {
                    Error::not_found(format!("no schedule '{}' in workspace '{ws}'", n.name))
                })?
                .clone();
            if !on {
                let mut s = old;
                s.enabled = false;
                s
            } else {
                if old.done && old.at.is_some_and(|a| a <= now()) {
                    return Err(Error::usage(format!(
                        "schedule '{}' ran once and its time has passed: give it a new --at",
                        n.name
                    )));
                }
                let spec = Spec {
                    enabled: Some(true),
                    ..Default::default()
                };
                with_ctx(&m, &app, &ws, Some(&n.name), |ctx| {
                    schedule::edit(&old, &spec, ctx)
                })?
            }
        };
        app.update(|m| {
            if let Some(s) = m.schedule_mut(&ws, &n.name) {
                *s = new.clone();
            }
        });
        app.save_now();
        crate::schedules_view::refresh();
        let m = app.model.borrow();
        Ok(object(&app, &m, &new))
    })
}

fn remove(n: Named) -> Result<Value> {
    on_main(move || {
        let app = app();
        let ws = crate::projects::target_workspace(
            &app.model.borrow(),
            n.workspace.as_deref(),
            n.caller.as_deref(),
        )?;
        let panes: Vec<String> = {
            let m = app.model.borrow();
            m.schedule(&ws, &n.name).ok_or_else(|| {
                Error::not_found(format!("no schedule '{}' in workspace '{ws}'", n.name))
            })?;
            if n.close_panes {
                m.schedule_tabs(&ws, &n.name)
                    .iter()
                    .filter_map(|(s, w, t)| m.worktree(s, w)?.tab(t))
                    .flat_map(|t| t.pane_ids())
                    .collect()
            } else {
                Vec::new()
            }
        };
        app.update(|m| {
            m.schedules
                .retain(|s| !(s.workspace == ws && s.name == n.name));
            m.forget_schedule(&ws, &n.name);
        });
        let mut closed = Vec::new();
        for p in panes {
            if app.close_pane(&p, false).is_ok() {
                closed.push(p);
            }
        }
        app.save_now();
        crate::schedules_view::refresh();
        Ok(json!({"name": n.name, "closed": closed}))
    })
}

fn list(s: Scope) -> Result<Value> {
    on_main(move || {
        let app = app();
        {
            refresh_runs(&mut app.model.borrow_mut());
        }
        let m = app.model.borrow();
        let ws = if s.all {
            None
        } else {
            Some(crate::projects::target_workspace(
                &m,
                s.workspace.as_deref(),
                s.caller.as_deref(),
            )?)
        };
        let project = match (&s.project, &ws) {
            (Some(q), Some(w)) => Some(m.find_project(Some(w), q)?.id),
            (Some(q), None) => Some(m.find_project(None, q)?.id),
            _ => None,
        };
        let rows: Vec<Value> = ordered(&m, ws.as_deref())
            .into_iter()
            .filter(|x| {
                project
                    .as_ref()
                    .is_none_or(|p| x.project.as_ref() == Some(p))
            })
            .filter(|x| !s.scratch || x.in_scratch())
            .map(|x| object(&app, &m, x))
            .collect();
        Ok(json!({"schedules": rows}))
    })
}

/// Schedules in Navigator order: by Workspace, then Project (the
/// Workspace's `project_order`, then by name), then Scratch, then name
/// (R138.2, R139.1).
pub fn ordered<'a>(m: &'a State, ws: Option<&str>) -> Vec<&'a Schedule> {
    let mut out: Vec<&Schedule> = m
        .schedules
        .iter()
        .filter(|s| ws.is_none_or(|w| s.workspace == w))
        .collect();
    let ws_pos = |w: &str| {
        m.workspaces
            .iter()
            .position(|x| x.id == w)
            .unwrap_or(usize::MAX)
    };
    let proj_pos = |s: &Schedule| -> (usize, String) {
        match &s.project {
            None => (usize::MAX, String::new()),
            Some(p) => {
                let order = m
                    .workspaces
                    .iter()
                    .find(|x| x.id == s.workspace)
                    .map(|x| x.project_order.clone())
                    .unwrap_or_default();
                (
                    order.iter().position(|x| x == p).unwrap_or(usize::MAX - 1),
                    p.clone(),
                )
            }
        }
    };
    out.sort_by(|a, b| {
        (ws_pos(&a.workspace), proj_pos(a), &a.name).cmp(&(
            ws_pos(&b.workspace),
            proj_pos(b),
            &b.name,
        ))
    });
    out
}

fn get(n: Named) -> Result<Value> {
    on_main(move || {
        let app = app();
        {
            let mut m = app.model.borrow_mut();
            refresh_runs(&mut m);
            fill_sessions(&mut m);
        }
        let m = app.model.borrow();
        let ws =
            crate::projects::target_workspace(&m, n.workspace.as_deref(), n.caller.as_deref())?;
        let s = m.schedule(&ws, &n.name).ok_or_else(|| {
            Error::not_found(format!("no schedule '{}' in workspace '{ws}'", n.name))
        })?;
        let mut v = object(&app, &m, s);
        let next = if s.enabled && !s.done {
            s.next_times(now().max(s.checked), 5, tz())
        } else {
            Vec::new()
        };
        v["next_times"] = json!(next);
        v["checked"] = json!(s.checked);
        v["tab"] = json!(m.schedule_tabs(&ws, &n.name).first().map(|t| t.2.clone()));
        let runs: Vec<&Run> = s.runs.iter().rev().take(10).collect();
        v["runs"] = serde_json::to_value(runs).unwrap();
        Ok(v)
    })
}

fn runs(n: Named) -> Result<Value> {
    on_main(move || {
        let app = app();
        {
            let mut m = app.model.borrow_mut();
            refresh_runs(&mut m);
            fill_sessions(&mut m);
        }
        let m = app.model.borrow();
        let ws =
            crate::projects::target_workspace(&m, n.workspace.as_deref(), n.caller.as_deref())?;
        let s = m.schedule(&ws, &n.name).ok_or_else(|| {
            Error::not_found(format!("no schedule '{}' in workspace '{ws}'", n.name))
        })?;
        let limit = n.limit.unwrap_or(20).clamp(1, schedule::MAX_RUNS);
        let runs: Vec<&Run> = s.runs.iter().rev().take(limit).collect();
        Ok(json!({"runs": runs}))
    })
}

/// Schedules on in Workspace `ws` (the Statusline cell, R139.1).
pub fn on_count(m: &State, ws: &str) -> usize {
    m.schedules
        .iter()
        .filter(|s| s.workspace == ws && s.enabled && !s.done)
        .count()
}
