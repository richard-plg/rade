//! What the Navigator and Project home show besides the model (PRD R104.4,
//! R100.1, R105): the shown Workspace's Projects in shown order, every
//! Source link resolved with its checkout's git facts ([`LinkView`]) and
//! the Worktrees' `(ahead, changed)` — read off the UI thread every 3 s
//! while the window shows, at once when a checkout's `HEAD` or a
//! `project.toml` changes and when the focus moves; Keep (R100.2) held in
//! memory and applied to every link list.

use crate::app::try_app;
use gtk::glib;
use gtk4 as gtk;
use gtk4::prelude::*;
use rade_core::checkout;
use rade_core::linkview::{KeepSet, LinkView};
use rade_core::project::{self, Project};
use rade_core::State;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

/// One read of the shown Workspace.
pub struct Snap {
    pub ws: String,
    /// In shown order (`Workspace.project_order`, R104.4).
    pub projects: Vec<Project>,
    /// Every link, before Keep.
    links: Vec<LinkView>,
    /// `(ahead of its base, changed files)` per linked Worktree folder.
    pub wt: HashMap<PathBuf, (usize, usize)>,
    /// The `HEAD` files of the links' checkouts and the Worktrees, with
    /// their mtimes when read: a change refreshes at once (R104-h).
    heads: Vec<(PathBuf, Option<SystemTime>)>,
    sig: project::Signature,
}

thread_local! {
    static SNAP: RefCell<Option<Rc<Snap>>> = const { RefCell::new(None) };
    static BUSY: Cell<bool> = const { Cell::new(false) };
    static AGAIN: Cell<bool> = const { Cell::new(false) };
    static LAST: Cell<Option<Instant>> = const { Cell::new(None) };
    static KEEP: RefCell<KeepSet> = RefCell::default();
    /// The focused (source, worktree) at the last sync.
    static FOCUS: RefCell<(String, String)> = RefCell::default();
}

/// The last read, when it is of the shown Workspace.
pub fn snap(ws: &State) -> Option<Rc<Snap>> {
    SNAP.with(|s| s.borrow().clone())
        .filter(|s| s.ws == ws.active_workspace())
}

/// The shown Workspace's links with Keep applied.
pub fn links(ws: &State) -> Vec<LinkView> {
    let Some(s) = snap(ws) else {
        return Vec::new();
    };
    let mut l = s.links.clone();
    KEEP.with(|k| k.borrow_mut().apply(&mut l));
    l
}

/// The shown Workspace's Projects in shown order: the last read, else the
/// files read now (before the first read lands).
pub fn projects(ws: &State) -> Vec<Project> {
    if let Some(s) = snap(ws) {
        return s.projects.clone();
    }
    let order = ws
        .workspace(ws.active_workspace())
        .map(|w| w.project_order.clone())
        .unwrap_or_default();
    rade_core::navigator::ordered(&crate::sidebar::projects(ws), &order)
}

/// A Worktree folder's `(ahead, changed files)`.
pub fn worktree_facts(ws: &State, path: &Path) -> (usize, usize) {
    snap(ws)
        .and_then(|s| s.wt.get(path).copied())
        .unwrap_or_default()
}

/// Keep (R100.2): hide this link's drift until its HEAD changes.
pub fn keep(project: &str, source: &str, head: &str) {
    KEEP.with(|k| k.borrow_mut().keep(project, source, head));
    shown_changed();
}

/// Re-render what reads this data.
fn shown_changed() {
    if let Some(app) = try_app() {
        crate::sidebar::resync(&app);
        crate::home_view::refresh();
        // Changes by Project and Search's scope read the links too.
        let m = app.model.borrow();
        if let Some(c) = crate::changes::get() {
            c.follow_project(&m);
        }
        if let Some(s) = crate::search_view::get() {
            s.follow_scope(&m);
        }
    }
}

/// The focus moved (called from every sync): refresh when it did.
pub fn focus_moved(ws: &State) {
    let (s, w) = ws.active();
    let now = (s.id.clone(), w.id.clone());
    let moved = FOCUS.with(|f| {
        let mut f = f.borrow_mut();
        let moved = *f != now;
        *f = now;
        moved
    });
    if moved {
        refresh();
    }
}

/// Read again now (one read at a time; one more when asked meanwhile).
pub fn refresh() {
    let Some(app) = try_app() else { return };
    if BUSY.with(|b| b.replace(true)) {
        AGAIN.with(|a| a.set(true));
        return;
    }
    LAST.with(|l| l.set(Some(Instant::now())));
    let (sc, wts) = {
        let m = app.model.borrow();
        let ws = m.active_workspace().to_string();
        let sc = crate::projects::scene(&m, &ws);
        let wts: Vec<(String, PathBuf, Option<String>)> = m
            .visible_sources()
            .filter(|s| !s.is_general())
            .flat_map(|s| {
                s.worktrees
                    .iter()
                    .skip(1)
                    .map(|w| (s.id.clone(), w.path.clone(), m.comparison_base(&w.path)))
                    .collect::<Vec<_>>()
            })
            .collect();
        (sc, wts)
    };
    crate::worktrees::in_background(
        move || read(sc, wts),
        |snap| {
            BUSY.with(|b| b.set(false));
            let Some(app) = try_app() else { return };
            let ids: Vec<String> = snap.projects.iter().map(|p| p.id.clone()).collect();
            let ws = snap.ws.clone();
            let changed = SNAP.with(|s| {
                let mut s = s.borrow_mut();
                let changed = s.as_ref().is_none_or(|o| {
                    o.ws != snap.ws
                        || o.links != snap.links
                        || o.wt != snap.wt
                        || o.projects != snap.projects
                });
                *s = Some(Rc::new(snap));
                changed
            });
            // Projects first seen keep their place from now on (R104.4).
            let noted = !app
                .model
                .borrow()
                .workspace(&ws)
                .is_some_and(|w| ids.iter().all(|i| w.project_order.contains(i)));
            if noted {
                app.update(|m| m.note_projects(&ws, &ids));
            } else if changed {
                shown_changed();
            }
            if AGAIN.with(|a| a.replace(false)) {
                refresh();
            }
        },
    );
}

/// Resolve and observe (blocking: git). Records first resolutions like
/// `project.show` does, so a recorded main checkout that leaves its base
/// reads as drift (R98.1, R100.1).
fn read(sc: crate::projects::Scene, wts: Vec<(String, PathBuf, Option<String>)>) -> Snap {
    let sig = project::signature(&sc.root);
    let ws = sc.ws.clone();
    let r = crate::projects::resolve(sc, None);
    crate::projects::record(&r);
    let names = r.scene.names();
    let canon = crate::projects::canon;
    let mut heads: Vec<PathBuf> = Vec::new();
    let links: Vec<LinkView> = r
        .resolved
        .iter()
        .map(|res| {
            let f = match (&res.path, r.repos.get(&res.source)) {
                (Some(p), Some(repo)) if res.refused.is_none() => {
                    if p.exists() {
                        heads.push(p.clone());
                    }
                    checkout::facts(repo, p, &res.base, &canon)
                }
                _ => checkout::Facts::default(),
            };
            LinkView::new(res, &f, &names)
        })
        .collect();
    let mut wt = HashMap::new();
    for (src, path, base) in wts {
        let Some(repo) = r.repos.get(&src) else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        heads.push(path.clone());
        let base = base
            .or_else(|| rade_core::git::default_branch(&path))
            .unwrap_or_else(|| "main".into());
        let f = checkout::facts(repo, &path, &base, &canon);
        wt.insert(path, (f.ahead.unwrap_or(0), f.dirty));
    }
    let heads = heads
        .into_iter()
        .filter_map(|p| {
            let dir = rade_core::git::run(&p, &["rev-parse", "--absolute-git-dir"]).ok()?;
            let head = PathBuf::from(dir.trim()).join("HEAD");
            let t = mtime(&head);
            Some((head, t))
        })
        .collect();
    Snap {
        ws,
        projects: rade_core::navigator::ordered(&r.scene.projects, &r.scene.order),
        links,
        wt,
        heads,
        sig,
    }
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Whether the last read is out of date: another Workspace shown, a
/// `project.toml` or a `HEAD` changed.
fn stale(ws: &State) -> bool {
    let Some(s) = SNAP.with(|s| s.borrow().clone()) else {
        return true;
    };
    if s.ws != ws.active_workspace() {
        return true;
    }
    if s.heads.iter().any(|(p, t)| mtime(p) != *t) {
        return true;
    }
    ws.workspace(ws.active_workspace())
        .is_some_and(|w| project::signature(&w.root) != s.sig)
}

/// The refresh clock: every 500 ms a cheap staleness check (stat calls),
/// every 3 s a full read, both only while the window shows.
pub fn install() {
    glib::timeout_add_local(Duration::from_millis(500), || {
        let Some(app) = try_app() else {
            return glib::ControlFlow::Continue;
        };
        if !app.window.is_visible() {
            return glib::ControlFlow::Continue;
        }
        let due = LAST.with(|l| {
            l.get()
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(3))
        });
        if due || stale(&app.model.borrow()) {
            refresh();
        }
        glib::ControlFlow::Continue
    });
}
