//! Code navigation in the app (PRD R83–R86, ADR-0029): the pool of
//! language servers (one per Worktree root and language, started on the
//! first navigation, at most `[code] max_servers`, stopped after
//! `[code] idle_minutes`, on Worktree removal and on quit), the `code.*`
//! socket methods the CLI and Agents use, and the Viewer's navigation:
//! a definition jumps (the old place goes on Back), everything else opens
//! in the results popover on the Pane that asked ([`crate::code_results`]).
//! Queries run off the GTK thread. M29: the
//! same keys on the Review Pane's diff lines ([`navigate_removed`] for a
//! removed line), a Worktree's Impact (`code.impact`, the Review Pane's
//! Impact section) and the Impact section of the Fan-out Judge prompt
//! ([`prompt_impact`]).

use crate::app::{try_app, workspace_env, App};
use crate::handlers::{on_main, params};
use rade_core::code::impact::{self, Impact, Inputs};
use rade_core::code::lsp::Client;
use rade_core::code::nav::{self, Answer, Query};
use rade_core::code::servers::{Lang, Pool};
use rade_core::code::{self, Ctx};
use rade_core::config::CodeConfig;
use rade_core::rpc::{Error, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// UI requests (R83: 10 s, then text matches).
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// An Impact for a prompt waits at most this long (R88.4): the Fan-out
/// Judge is never held up longer by a slow server.
pub const PROMPT_BUDGET: Duration = Duration::from_secs(20);

/// The pool; the first use starts the idle reaper.
pub fn pool() -> &'static Mutex<Pool<Client>> {
    static POOL: OnceLock<Mutex<Pool<Client>>> = OnceLock::new();
    POOL.get_or_init(|| {
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_secs(1));
            let idle = pool().lock().unwrap().reap(Instant::now());
            code::stop(idle, Duration::from_secs(2));
        });
        Mutex::new(Pool::default())
    })
}

/// Stop the servers of a removed Worktree.
pub fn stop_under(dir: &Path) {
    let gone = pool().lock().unwrap().take_under(Some(dir));
    code::stop(gone, Duration::from_secs(2));
}

/// On quit: every server, briefly (they also exit when their stdin closes).
pub fn stop_all() {
    let Ok(mut p) = pool().try_lock() else { return };
    let all = p.take_under(None);
    drop(p);
    let threads: Vec<_> = all
        .into_iter()
        .map(|s| std::thread::spawn(move || s.shutdown(Duration::from_millis(500))))
        .collect();
    for t in threads {
        let _ = t.join();
    }
}

/// Files outside every Worktree that navigation reached (the std library,
/// a Go module cache, typeshed) → the Worktree and server root they were
/// reached from: asked through that server, never one of their own (as
/// VS Code keeps them in the originating workspace; 0.7.1).
static ORIGINS: LazyLock<Mutex<HashMap<PathBuf, (PathBuf, PathBuf)>>> =
    LazyLock::new(Mutex::default);

/// Remember where the files of answer `a` to `ctx` outside its Worktree
/// came from.
fn remember_origins<'a>(ctx: &Ctx, paths: impl Iterator<Item = &'a Path>) {
    let Some(lang) = Lang::of(&ctx.path) else {
        return;
    };
    let root = ctx
        .root
        .clone()
        .unwrap_or_else(|| code::servers::root_for(&ctx.path, &ctx.worktree, lang));
    let mut o = ORIGINS.lock().unwrap();
    for p in paths.filter(|p| !p.starts_with(&ctx.worktree)) {
        o.entry(p.to_path_buf())
            .or_insert_with(|| (ctx.worktree.clone(), root.clone()));
    }
}

/// Every file an answer names.
fn answer_paths(a: &Answer) -> impl Iterator<Item = &Path> {
    fn calls<'a>(c: &'a [nav::Call], out: &mut Vec<&'a Path>) {
        for k in c {
            out.push(&k.path);
            out.extend(k.from_ranges.iter().map(|s| s.path.as_path()));
            calls(&k.children, out);
        }
    }
    let mut out: Vec<&Path> = a
        .items
        .iter()
        .chain(&a.others)
        .map(|i| i.path.as_path())
        .collect();
    calls(&a.calls, &mut out);
    out.into_iter()
}

/// The Worktree a file is in (the longest matching path; for a file
/// navigation reached outside every Worktree, the one it was reached
/// from) → the query's inputs, and the user's `[code]` (pool size and
/// idle time).
pub fn context(app: &App, q: Query, path: &Path, line: u32, col: u32) -> Result<(Ctx, CodeConfig)> {
    let mut origin = None;
    let (sid, wid, wt, env) = {
        let m = app.model.borrow();
        // A Worktree holding the file wins; else the one it was reached from
        // (while that Worktree exists).
        let holding = m
            .worktrees()
            .filter(|(_, w)| path.starts_with(&w.path))
            .max_by_key(|(_, w)| w.path.as_os_str().len());
        let (s, w) = holding
            .or_else(|| {
                let o = ORIGINS.lock().unwrap().get(path).cloned()?;
                let hit = m.worktrees().find(|(_, w)| w.path == o.0);
                origin = hit.is_some().then_some(o);
                hit
            })
            .ok_or_else(|| {
                Error::not_found(format!(
                    "{} is not in a RADE Worktree (rade source add its repository)",
                    path.display()
                ))
            })?;
        (
            s.id.clone(),
            w.id.clone(),
            w.path.clone(),
            workspace_env(&m, &s.id),
        )
    };
    let cfg = app.config_for(&sid, Some(&wid));
    let name = app
        .model
        .borrow()
        .find_source(&sid)
        .map(|s| s.name.clone())
        .unwrap_or_default();
    Ok((
        Ctx {
            query: q,
            path: path.to_path_buf(),
            line: line.max(1),
            col: col.max(1),
            worktree: wt,
            enabled: cfg.code_enabled(&sid, &name),
            cfg: cfg.code,
            env,
            wait: Duration::ZERO,
            timeout: TIMEOUT,
            root: origin.map(|o| o.1),
            depth: 1,
        },
        app.config.borrow().code.clone(),
    ))
}

/// Impact inputs for the Worktree holding `path`: `base` given, else the
/// Worktree's recorded base (else its Source's main branch).
pub fn impact_inputs(
    app: &App,
    path: &Path,
    base: Option<String>,
    budget: Duration,
    wait: Duration,
) -> Result<(Inputs, CodeConfig)> {
    let (c, user) = context(app, Query::Def, path, 1, 1)?;
    let base = base.or_else(|| app.model.borrow().comparison_base(&c.worktree));
    Ok((
        Inputs {
            worktree: c.worktree,
            base,
            cfg: c.cfg,
            enabled: c.enabled,
            env: c.env,
            budget,
            wait,
        },
        user,
    ))
}

/// The Impact of the Worktree holding `path` (blocking: not on the GTK
/// thread), within `budget`, waiting up to `wait` for a loading server.
pub fn impact_of(
    path: PathBuf,
    base: Option<String>,
    budget: Duration,
    wait: Duration,
) -> Result<Impact> {
    let (inp, user) = on_main(move || {
        let app = try_app().ok_or_else(|| Error::internal("no app"))?;
        impact_inputs(&app, &path, base, budget, wait)
    })?;
    pool().lock().unwrap().configure(&user);
    impact::within(pool(), inp)
}

/// The Impact section for an Agent's prompt (R88.4), or "" when nothing
/// changed or it failed. Blocking, at most [`PROMPT_BUDGET`].
pub fn prompt_impact(worktree: PathBuf, base: Option<String>, title: &str) -> String {
    match impact_of(worktree.clone(), base, PROMPT_BUDGET, PROMPT_BUDGET) {
        Ok(i) => impact::prompt_section(&i, title),
        Err(e) => {
            eprintln!("rade: impact of {}: {}", worktree.display(), e.message);
            String::new()
        }
    }
}

/// Run a query (blocking: a socket or background thread).
pub fn run(ctx: &Ctx, user: &CodeConfig) -> Answer {
    pool().lock().unwrap().configure(user);
    let a = code::run(pool(), ctx);
    remember_origins(ctx, answer_paths(&a));
    a
}

/// The next level under a call hierarchy node (blocking).
pub fn expand(
    ctx: &Ctx,
    user: &CodeConfig,
    item: &Value,
    incoming: bool,
) -> std::result::Result<Vec<nav::Call>, String> {
    pool().lock().unwrap().configure(user);
    let r = code::expand(pool(), ctx, item, incoming)?;
    remember_origins(
        ctx,
        r.iter().flat_map(|c| {
            std::iter::once(c.path.as_path()).chain(c.from_ranges.iter().map(|s| s.path.as_path()))
        }),
    );
    Ok(r)
}

#[derive(Deserialize)]
struct QueryParams {
    query: String,
    path: PathBuf,
    #[serde(default)]
    line: Option<u32>,
    #[serde(default)]
    col: Option<u32>,
    /// Seconds to wait for an indexing server.
    #[serde(default)]
    wait: Option<f64>,
    /// `workspace`: the text symbol names contain.
    #[serde(default)]
    text: Option<String>,
    /// `callers` / `callees`: levels of the call hierarchy (default 1).
    #[serde(default)]
    depth: Option<u32>,
}

#[derive(Deserialize)]
struct ImpactParams {
    /// A directory or file in the Worktree (the CLI's cwd).
    path: PathBuf,
    #[serde(default)]
    worktree: Option<String>,
    #[serde(default)]
    base: Option<String>,
    /// The time budget in seconds (default 60).
    #[serde(default)]
    wait: Option<f64>,
}

#[derive(Deserialize)]
struct PathParams {
    path: PathBuf,
}

/// `code.*` (on the socket thread) and `debug.code`.
pub fn dispatch(method: &str, p: Value) -> Result<Value> {
    match method {
        "code.query" => {
            let qp: QueryParams = params(p)?;
            let q = Query::parse(&qp.query).ok_or_else(|| {
                Error::usage(format!(
                    "unknown query '{}' (def, refs, callers, callees, impl, symbols, hover, workspace)",
                    qp.query
                ))
            })?;
            let ws = q == Query::Workspace;
            if !(qp.path.is_file() || ws && qp.path.is_dir()) {
                return Err(Error::not_found(format!("no file {}", qp.path.display())));
            }
            let path = qp.path.clone();
            let (line, col) = (qp.line.unwrap_or(1), qp.col.unwrap_or(1));
            let (mut ctx, user) = on_main(move || {
                let app = try_app().ok_or_else(|| Error::internal("no app"))?;
                context(&app, q, &path, line, col)
            })?;
            ctx.wait = Duration::from_secs_f64(qp.wait.unwrap_or(0.0).clamp(0.0, 120.0));
            ctx.depth = qp.depth.unwrap_or(1).clamp(1, 10);
            if ws {
                pool().lock().unwrap().configure(&user);
                let a = code::workspace(pool(), &ctx, qp.text.as_deref().unwrap_or(""));
                return Ok(serde_json::to_value(a).unwrap_or_default());
            }
            Ok(serde_json::to_value(run(&ctx, &user)).unwrap_or_default())
        }
        "code.impact" => {
            let ip: ImpactParams = params(p)?;
            let budget = Duration::from_secs_f64(ip.wait.unwrap_or(60.0).clamp(1.0, 300.0));
            let at = ip.path.clone();
            let name = ip.worktree.clone();
            // `worktree`: a Worktree's name or id, in the Source of `path`
            // first, else anywhere (unique); else the one holding `path`.
            let path = match name {
                None => at,
                Some(n) => on_main(move || {
                    let app = try_app().ok_or_else(|| Error::internal("no app"))?;
                    let m = app.model.borrow();
                    let here = m
                        .worktrees()
                        .filter(|(_, w)| at.starts_with(&w.path))
                        .max_by_key(|(_, w)| w.path.as_os_str().len())
                        .map(|(s, _)| s.id.clone());
                    let hits: Vec<_> = m
                        .worktrees()
                        .filter(|(_, w)| w.name == n || w.id == n)
                        .map(|(s, w)| (s.id.clone(), w.path.clone()))
                        .collect();
                    let mine: Vec<_> = hits
                        .iter()
                        .filter(|h| Some(&h.0) == here.as_ref())
                        .collect();
                    match (mine.as_slice(), hits.as_slice()) {
                        ([one], _) => Ok(one.1.clone()),
                        ([], [one]) => Ok(one.1.clone()),
                        ([], []) => Err(Error::not_found(format!("no Worktree '{n}'"))),
                        _ => Err(Error::usage(format!(
                            "'{n}' names Worktrees in several Sources: run it inside one"
                        ))),
                    }
                })?,
            };
            // An explicit base must exist (a recorded one that is gone falls
            // back to the main branch; a typo must not).
            if let Some(b) = &ip.base {
                let dir = if path.is_dir() {
                    path.clone()
                } else {
                    path.parent().map(Path::to_path_buf).unwrap_or_default()
                };
                if rade_core::git::run(
                    &dir,
                    &[
                        "rev-parse",
                        "--verify",
                        "--quiet",
                        &format!("{b}^{{commit}}"),
                    ],
                )
                .is_err()
                {
                    return Err(Error::usage(format!(
                        "unknown base '{b}' (a branch or commit)"
                    )));
                }
            }
            Ok(serde_json::to_value(impact_of(path, ip.base, budget, budget)?).unwrap_or_default())
        }
        "code.status" => {
            let now = Instant::now();
            let p = pool().lock().unwrap();
            let servers: Vec<Value> = p
                .list(now)
                .into_iter()
                .map(|((root, lang), c, idle)| {
                    json!({"root": root, "language": lang.id(), "pid": c.pid,
                           "command": c.argv, "idle_secs": idle.as_secs_f64().round(),
                           "busy": c.busy()})
                })
                .collect();
            Ok(json!({"servers": servers, "max_servers": p.max,
                      "idle_minutes": p.idle.as_secs_f64() / 60.0}))
        }
        "code.restart" => {
            let pp: PathParams = params(p)?;
            let path = pp.path.clone();
            let (ctx, _) = on_main(move || {
                let app = try_app().ok_or_else(|| Error::internal("no app"))?;
                context(&app, Query::Def, &path, 1, 1)
            })?;
            let lang = Lang::of(&pp.path)
                .ok_or_else(|| Error::usage("no language server for this file type"))?;
            let root = ctx
                .root
                .clone()
                .unwrap_or_else(|| code::servers::root_for(&pp.path, &ctx.worktree, lang));
            let gone = pool().lock().unwrap().restart(&(root.clone(), lang));
            let stopped = gone.is_some();
            code::stop(gone.into_iter().collect(), Duration::from_secs(2));
            Ok(json!({"root": root, "language": lang.id(), "stopped": stopped}))
        }
        "debug.code" => on_main(move || crate::code_results::debug(&p)),
        m => Err(Error::not_found(format!("unknown method {m}"))),
    }
}

/// A navigation from a Viewer (keys, Ctrl+click, the results popover's
/// Retry): the results popover shows the answer; a definition or
/// implementation with one result jumps there instead.
pub fn navigate(pane: &str, q: Query, path: &Path, line: u32, col: u32, wait: Duration) {
    let Some(app) = try_app() else { return };
    let (mut ctx, user) = match context(&app, q, path, line, col) {
        Ok(c) => c,
        Err(e) => {
            app.chrome
                .toast(&gtk4::glib::markup_escape_text(&e.message), None);
            return;
        }
    };
    ctx.wait = wait;
    if let Some(v) = crate::code_results::get() {
        v.running(q, pane, &ctx);
    }
    answer_in_background(pane, q, move || run(&ctx, &user));
}

/// A navigation from a removed diff line in Review Pane `pane` (R87.2):
/// `file` is the diff's file in the Worktree (it may be gone), `word` the
/// name, `base` the commit the diff is against.
pub fn navigate_removed(pane: &str, q: Query, file: &Path, word: &str, base: &str) {
    let Some(app) = try_app() else { return };
    let (ctx, user) = match context(&app, q, file, 1, 1) {
        Ok(c) => c,
        Err(e) => {
            app.chrome
                .toast(&gtk4::glib::markup_escape_text(&e.message), None);
            return;
        }
    };
    if let Some(v) = crate::code_results::get() {
        v.running(q, pane, &ctx);
    }
    let (word, base) = (word.to_string(), base.to_string());
    answer_in_background(pane, q, move || {
        pool().lock().unwrap().configure(&user);
        code::run_removed(pool(), &ctx, &word, &base)
    });
}

/// Run `f` off the GTK thread; its answer fills the results popover, or a
/// definition with one result jumps there.
fn answer_in_background(pane: &str, q: Query, f: impl FnOnce() -> Answer + Send + 'static) {
    let pane = pane.to_string();
    crate::worktrees::in_background(f, move |a| {
        // Only while the Viewer still shows the file it asked from (a late
        // answer, e.g. after indexing, never yanks it elsewhere); from a
        // Review Pane, while it is open.
        let here = try_app().is_some_and(|app| {
            app.reviews.borrow().contains_key(&pane)
                || app
                    .viewers
                    .borrow()
                    .get(&pane)
                    .and_then(|v| v.path())
                    .as_deref()
                    == Some(a.path.as_path())
        });
        let jump = matches!(q, Query::Def | Query::Impl) && a.items.len() == 1 && here;
        if let Some(v) = crate::code_results::get() {
            v.show(a.clone(), !jump);
        }
        if jump {
            let it = &a.items[0];
            jump_to(&pane, &it.path, it.line, it.col);
        }
    });
}

/// Show `path:line:col` in Viewer `pane` (a jump: Back returns), else in
/// the active Worktree's Viewer.
pub fn jump_to(pane: &str, path: &Path, line: u32, col: u32) {
    let Some(app) = try_app() else { return };
    // Back returns to the caret, not to where the file was opened.
    let caret = app.viewers.borrow().get(pane).map(|v| {
        v.set_pending_col(line, col);
        (v.path(), v.caret().0)
    });
    let path = path.to_path_buf();
    let exists = app.model.borrow().pane(pane).is_some_and(|p| p.is_viewer());
    // From a Review Pane: its Tab's Viewer jumps (R87.3).
    if app.model.borrow().pane(pane).is_some_and(|p| p.is_review()) {
        let old = {
            let m = app.model.borrow();
            let tab = m.pane_loc(pane).map(|l| l.tab);
            app.viewers
                .borrow()
                .keys()
                .find(|v| tab.is_some() && m.pane_loc(v).map(|l| l.tab) == tab)
                .cloned()
        };
        // The caret lands on the name (an existing Viewer scrolls during the
        // update).
        let caret = |id: &str| {
            if let Some(v) = app.viewers.borrow().get(id) {
                v.set_pending_col(line, col);
            }
        };
        if let Some(id) = &old {
            caret(id);
        }
        let r = app.update(|m| {
            let id = m.viewer_jump_from(pane, &path, Some(line))?;
            m.focus_pane(&id)?;
            Ok::<_, Error>(id)
        });
        match r {
            // A new Viewer loaded its file during the update: put the caret now.
            Ok(id) if old.as_ref() != Some(&id) => {
                if let Some(v) = app.viewers.borrow().get(&id) {
                    v.place(line, col);
                }
            }
            Ok(_) => {}
            Err(e) => eprintln!("rade: code jump {}: {e}", path.display()),
        }
        return;
    }
    let r = if exists {
        app.update(|m| {
            if let Some((Some(at), l)) = &caret {
                m.viewer_navigate(pane, at, Some(*l))?;
            }
            m.viewer_jump(pane, &path, Some(line))
        })
    } else {
        let (s, w) = {
            let m = app.model.borrow();
            let (s, w) = m.active();
            (s.id.clone(), w.id.clone())
        };
        app.update(|m| {
            m.open_viewer_in(&s, &w, &path, Some(line), true)
                .map(|_| ())
        })
    };
    if let Err(e) = r {
        eprintln!("rade: code jump {}: {e}", path.display());
    }
}

/// Open `path` at `line:col` in the active Worktree's Viewer (a palette
/// symbol, R92), the caret on the name.
pub fn open_at(path: &Path, line: u32, col: u32) {
    let Some(app) = try_app() else { return };
    let (s, w) = {
        let m = app.model.borrow();
        let (s, w) = m.active();
        (s.id.clone(), w.id.clone())
    };
    let p = path.to_path_buf();
    if let Err(e) = app.update(|m| m.open_viewer_in(&s, &w, &p, Some(line), true).map(|_| ())) {
        eprintln!("rade: open {}: {e}", path.display());
        return;
    }
    let shown: Vec<_> = app
        .viewers
        .borrow()
        .values()
        .filter(|v| v.path().as_deref() == Some(path))
        .cloned()
        .collect();
    for v in shown {
        v.place(line, col);
    }
}
