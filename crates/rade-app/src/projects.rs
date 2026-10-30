//! Projects and layered config in the app (PRD R28, R29, R31; ADR-0017):
//! the `project.*` and `config.*` socket methods. Files are read fresh on
//! every call; git runs off the GTK thread.

use crate::app::app;
use crate::handlers::{on_main, params, ListParams};
use adw::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::checkout::{self, LinkCheckout};
use rade_core::layers;
use rade_core::project::{self, Project};
use rade_core::rpc::{Error, Result};
use rade_core::{drift, git, paths, State};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Called on a socket thread.
pub fn dispatch(method: &str, p: Value) -> Result<Value> {
    match method {
        "project.list" => on_main(move || list(params(p)?)),
        "project.show" => show(params(p)?),
        "project.add" => {
            let (v, ws) = on_main(move || add(params(p)?))?;
            if v["existing"] == false {
                // The links' first checkouts are chosen now (R98.1).
                let sc = on_main(move || Ok(scene(&app().model.borrow(), &ws)))?;
                record(&resolve(sc, None));
            }
            Ok(v)
        }
        "project.plan" => plan(params(p)?),
        "project.link_add" => link_add(params(p)?),
        "project.link_remove" => link_edit(params(p)?, false),
        "project.link_set_base" => link_edit(params(p)?, true),
        "project.link_to_base" => link_to_base(params(p)?),
        "project.remove" => remove(params(p)?),
        "project.prune" => prune(params(p)?),
        "project.set" => crate::project_settings::dispatch(p),
        "config.explain" => on_main(move || explain(params(p)?)),
        "config.trust" => on_main(move || trust(params(p)?)),
        _ => Err(Error::usage(format!("unknown method '{method}'"))),
    }
}

fn list(l: ListParams) -> Result<Value> {
    let app = app();
    let m = app.model.borrow();
    let scope = l.scope(&m)?;
    let projects: Vec<Value> = m
        .workspaces
        .iter()
        .filter(|w| scope.as_ref().is_none_or(|s| *s == w.id))
        .flat_map(|w| {
            project::list(&w.root)
                .into_iter()
                .map(|p| p.to_json(&w.id, &registered(&m, &w.id)))
                .collect::<Vec<_>>()
        })
        .collect();
    Ok(json!({ "projects": projects }))
}

/// Whether a source id is a Source of Workspace `ws`.
fn registered<'a>(m: &'a State, ws: &'a str) -> impl Fn(&str) -> bool + 'a {
    move |id| m.source(id).is_some_and(|s| s.workspace == ws)
}

/// The caller's Workspace, else the shown one.
pub(crate) fn caller_workspace(m: &State, caller: Option<&str>) -> String {
    caller
        .and_then(|c| m.locate(c))
        .and_then(|l| m.source(&l.source))
        .map(|s| s.workspace.clone())
        .unwrap_or_else(|| m.active_workspace().to_string())
}

/// A Project by id or name: the caller's Workspace first, then any.
pub(crate) fn find(m: &State, q: &str, caller: Option<&str>) -> Result<(String, Project)> {
    let ws = caller_workspace(m, caller);
    let p = m
        .find_project(Some(&ws), q)
        .or_else(|_| m.find_project(None, q))?;
    let owner = m
        .workspaces
        .iter()
        .filter(|w| p.file.starts_with(&w.root))
        .max_by_key(|w| w.root.components().count())
        .map(|w| w.id.clone())
        .unwrap_or(ws);
    Ok((owner, p))
}

#[derive(Deserialize)]
struct Show {
    project: String,
    caller: Option<String>,
}

/// `realpath` of the longest existing ancestor, the rest appended: a
/// Project checkout not created yet still compares with created ones.
pub(crate) fn canon(p: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut cur = p.to_path_buf();
    loop {
        if let Ok(c) = cur.canonicalize() {
            return rest.iter().rev().fold(c, |acc: PathBuf, n| acc.join(n));
        }
        match (cur.file_name().map(|n| n.to_os_string()), cur.parent()) {
            (Some(n), Some(parent)) => {
                rest.push(n);
                cur = parent.to_path_buf();
            }
            _ => return p.to_path_buf(),
        }
    }
}

/// What resolving a Workspace's links needs, read on the GTK thread.
pub(crate) struct Scene {
    pub ws: String,
    pub root: PathBuf,
    pub sources: Vec<(String, PathBuf)>,
    pub projects: Vec<Project>,
    pub records: Vec<LinkCheckout>,
    pub folder: PathBuf,
    pub home: PathBuf,
    /// The Workspace's Project order (R104.4).
    pub order: Vec<String>,
}

impl Scene {
    /// Project names in shown order (plan lines, also-in labels).
    pub fn names(&self) -> rade_core::linkview::Names {
        rade_core::linkview::Names(
            rade_core::navigator::ordered(&self.projects, &self.order)
                .into_iter()
                .map(|p| (p.id, p.name))
                .collect(),
        )
    }
}

pub(crate) fn scene(m: &State, ws: &str) -> Scene {
    let w = m.workspace(ws);
    let root = w.map(|w| w.root.clone()).unwrap_or_default();
    Scene {
        ws: ws.to_string(),
        sources: m
            .sources
            .iter()
            .filter(|s| s.workspace == ws && !s.is_general())
            .map(|s| (s.id.clone(), s.path.clone()))
            .collect(),
        projects: project::list(&root),
        records: w.map(|w| w.link_checkouts.clone()).unwrap_or_default(),
        folder: m.worktrees_folder(
            ws,
            &paths::data_dir(&paths::process_env),
            paths::Flavor::current(),
        ),
        home: paths::home_dir(&paths::process_env),
        order: w.map(|w| w.project_order.clone()).unwrap_or_default(),
        root,
    }
}

/// A resolve of every link of the Workspace (off the GTK thread).
pub(crate) struct Resolution {
    pub scene: Scene,
    pub repos: BTreeMap<String, checkout::Repo>,
    pub resolved: Vec<checkout::Resolved>,
}

impl Resolution {
    pub fn get(&self, project: &str, source: &str) -> Option<&checkout::Resolved> {
        self.resolved
            .iter()
            .find(|r| r.project == project && r.source == source)
    }

    /// The branch the main checkout of `source` is on.
    pub fn main_head(&self, source: &str) -> Option<String> {
        let r = self.repos.get(source)?;
        r.checkout(&r.main, &canon)?.branch.clone()
    }
}

/// Resolve the Workspace's links; `extra` replaces (or adds) one Project
/// (`project.plan`, a link edit being checked). Runs git: off the GTK thread.
pub(crate) fn resolve(mut sc: Scene, extra: Option<Project>) -> Resolution {
    if let Some(p) = extra {
        sc.projects.retain(|x| x.id != p.id);
        sc.projects.push(p);
    }
    let repos = checkout::observe(&sc.sources);
    let mains: BTreeMap<String, String> = sc
        .sources
        .iter()
        .map(|(id, path)| {
            let b = git::default_branch(path)
                .or_else(|| git::branch(path))
                .unwrap_or_else(|| "main".into());
            (id.clone(), b)
        })
        .collect();
    let ids: Vec<&String> = sc.sources.iter().map(|s| &s.0).collect();
    let links = checkout::links_of(&sc.projects, &|s| ids.iter().any(|i| *i == s), &|s| {
        mains.get(s).cloned().unwrap_or_else(|| "main".into())
    });
    let exists = |p: &Path| p.exists();
    let world = checkout::World {
        repos,
        folder: sc.folder.clone(),
        home: sc.home.clone(),
        canon: &canon,
        exists: &exists,
    };
    let resolved = checkout::resolve_checkout(&links, &world, &sc.records);
    Resolution {
        repos: world.repos,
        resolved,
        scene: sc,
    }
}

/// Keep what a resolve chose in the Workspace's `link_checkouts` (R98.1).
pub(crate) fn record(r: &Resolution) {
    let Some(new) = checkout::merge_records(&r.scene.records, &r.resolved) else {
        return;
    };
    let ws = r.scene.ws.clone();
    let _ = on_main(move || {
        let app = app();
        app.update(|m| {
            if let Some(w) = m.workspaces.iter_mut().find(|w| w.id == ws) {
                w.link_checkouts = new;
            }
        });
        app.save_now();
        Ok(())
    });
}

/// Forget a link's recorded checkout (link removed, base changed).
pub(crate) fn drop_record(ws: &str, project: &str, source: &str) {
    let (ws, project, source) = (ws.to_string(), project.to_string(), source.to_string());
    let _ = on_main(move || {
        let app = app();
        let changed = app.update(|m| {
            let Some(w) = m.workspaces.iter_mut().find(|w| w.id == ws) else {
                return false;
            };
            let n = w.link_checkouts.len();
            w.link_checkouts
                .retain(|r| !(r.project == project && r.source == source));
            n != w.link_checkouts.len()
        });
        if changed {
            app.save_now();
        }
        Ok(())
    });
}

/// `project.show`: `project.list`'s entry with each link's checkout,
/// HEAD, counts and drift (R98, R100.3); first resolutions are recorded.
fn show(s: Show) -> Result<Value> {
    let (ws, p, sc) = on_main(move || {
        let app = app();
        let m = app.model.borrow();
        let (ws, p) = find(&m, &s.project, s.caller.as_deref())?;
        let sc = scene(&m, &ws);
        Ok((ws, p, sc))
    })?;
    let r = resolve(sc, None);
    record(&r);
    let reg: Vec<String> = r.scene.sources.iter().map(|s| s.0.clone()).collect();
    let mut v = p.to_json(&ws, &|id| reg.iter().any(|s| s == id));
    for l in v["links"].as_array_mut().into_iter().flatten() {
        let src = l["source"].as_str().unwrap_or_default().to_string();
        let (Some(res), Some(repo)) = (r.get(&p.id, &src), r.repos.get(&src)) else {
            continue;
        };
        let facts = match &res.path {
            Some(path) if res.refused.is_none() => checkout::facts(repo, path, &res.base, &canon),
            _ => checkout::Facts::default(),
        };
        if let (Some(o), Value::Object(extra)) =
            (l.as_object_mut(), checkout::show_json(res, &facts))
        {
            o.extend(extra);
        }
    }
    Ok(v)
}

#[derive(Deserialize)]
struct RemoveParams {
    project: String,
    caller: Option<String>,
    #[serde(default)]
    dry_run: bool,
    /// Leave every Project checkout on disk.
    #[serde(default)]
    keep_checkouts: bool,
}

#[derive(Deserialize)]
struct PruneParams {
    workspace: Option<String>,
    caller: Option<String>,
    #[serde(default)]
    dry_run: bool,
}

/// The paths the Workspace's links use now (recorded, resolved or planned).
fn used_paths(r: &Resolution) -> Vec<PathBuf> {
    r.resolved.iter().filter_map(|x| x.path.clone()).collect()
}

fn orphan_json(o: &checkout::Orphan) -> Value {
    json!({"source": o.source, "path": o.path, "branch": o.branch, "dirty": o.dirty})
}

/// `git worktree remove` of each clean orphan (never `--force`); the
/// dirty ones and failures are kept, with why.
fn remove_orphans(r: &Resolution, found: &[checkout::Orphan]) -> (Vec<Value>, Vec<Value>) {
    let (mut removed, mut kept) = (Vec::new(), Vec::new());
    for o in found {
        let mut v = orphan_json(o);
        if o.dirty > 0 {
            v["why"] = format!("{} uncommitted change{}", o.dirty, if o.dirty == 1 { "" } else { "s" }).into();
            kept.push(v);
            continue;
        }
        let Some(main) = r.repos.get(&o.source).map(|x| x.main.clone()) else {
            continue;
        };
        match git::remove_worktree(&main, &o.path, false) {
            Ok(()) => removed.push(v),
            Err(e) => {
                v["why"] = e.message.into();
                kept.push(v);
            }
        }
    }
    (removed, kept)
}

/// `project.prune` (R133.2): the Project checkouts under
/// `<folder>/.projects` that no link of the Workspace uses; the clean ones
/// are removed (`dry_run`: only listed), the dirty ones never touched.
fn prune(pp: PruneParams) -> Result<Value> {
    let sc = on_main(move || {
        let app = app();
        let m = app.model.borrow();
        let ws = target_workspace(&m, pp.workspace.as_deref(), pp.caller.as_deref())?;
        Ok(scene(&m, &ws))
    })?;
    let r = resolve(sc, None);
    let found = checkout::orphans(&r.repos, &r.scene.folder, &used_paths(&r), &canon);
    if pp.dry_run {
        let (clean, dirty): (Vec<_>, Vec<_>) = found.iter().partition(|o| o.dirty == 0);
        return Ok(json!({
            "workspace": r.scene.ws, "dry_run": true,
            "remove": clean.iter().map(|o| orphan_json(o)).collect::<Vec<_>>(),
            "keep": dirty.iter().map(|o| orphan_json(o)).collect::<Vec<_>>(),
        }));
    }
    let (removed, kept) = remove_orphans(&r, &found);
    Ok(json!({"workspace": r.scene.ws, "removed": removed, "kept": kept}))
}

/// The Panes running in one of `dirs` (a Project checkout about to go).
fn panes_in(m: &State, dirs: &[PathBuf]) -> Vec<(String, String)> {
    m.sources
        .iter()
        .flat_map(|s| s.worktrees.iter())
        .flat_map(|w| w.tabs.iter())
        .flat_map(|t| t.panes())
        .filter(|p| {
            let cwd = canon(p.cwd());
            dirs.iter().any(|d| cwd.starts_with(canon(d)))
        })
        .map(|p| {
            let name = p
                .name
                .clone()
                .or_else(|| p.agent_name().map(String::from))
                .unwrap_or_else(|| p.id.clone());
            (p.id.clone(), name)
        })
        .collect()
}

/// `project.remove` (R133.1): delete the Project's folder (`project.toml`
/// with it), remove its Project checkouts no other link uses (clean ones;
/// dirty ones are kept and listed), close the Panes running in those, put
/// its Worktrees in Scratch with their base. `dry_run` only says so.
fn remove(rp: RemoveParams) -> Result<Value> {
    let q = rp.project.clone();
    let caller = rp.caller.clone();
    let (ws, p, sc) = on_main(move || {
        let app = app();
        let m = app.model.borrow();
        let (ws, p) = find(&m, &q, caller.as_deref())?;
        let sc = scene(&m, &ws);
        Ok((ws, p, sc))
    })?;
    // This Project's checkouts: its records and its `.projects/<id>` folder.
    let mine: Vec<PathBuf> = sc
        .records
        .iter()
        .filter(|r| r.project == p.id)
        .map(|r| canon(&r.path))
        .collect();
    let own_dir = canon(&sc.folder.join(checkout::PROJECTS).join(&p.id));
    let mut rest = sc;
    rest.projects.retain(|x| x.id != p.id);
    rest.records.retain(|x| x.project != p.id);
    let r = resolve(rest, None);
    let found: Vec<checkout::Orphan> =
        checkout::orphans(&r.repos, &r.scene.folder, &used_paths(&r), &canon)
            .into_iter()
            .filter(|o| {
                let c = canon(&o.path);
                mine.contains(&c) || c.starts_with(&own_dir)
            })
            .collect();
    let going: Vec<PathBuf> = if rp.keep_checkouts {
        Vec::new()
    } else {
        found.iter().filter(|o| o.dirty == 0).map(|o| o.path.clone()).collect()
    };
    let (pid, g) = (p.id.clone(), going.clone());
    let (panes, worktrees) = on_main(move || {
        let app = app();
        let m = app.model.borrow();
        let wts: Vec<(String, String)> = m
            .sources
            .iter()
            .flat_map(|s| s.worktrees.iter().map(move |w| (s, w)))
            .filter(|(_, w)| w.project.as_deref() == Some(pid.as_str()))
            .map(|(s, w)| (s.id.clone(), w.name.clone()))
            .collect();
        Ok((panes_in(&m, &g), wts))
    })?;
    let mut v = json!({
        "project": p.id, "name": p.name, "workspace": ws,
        "file": p.file, "dry_run": rp.dry_run,
        "panes": panes.iter().map(|(id, n)| json!({"pane": id, "name": n})).collect::<Vec<_>>(),
        "worktrees": worktrees.iter().map(|(s, w)| json!({"source": s, "worktree": w})).collect::<Vec<_>>(),
    });
    if rp.dry_run {
        let (clean, dirty): (Vec<_>, Vec<_>) = found
            .iter()
            .partition(|o| o.dirty == 0 && !rp.keep_checkouts);
        v["remove"] = clean.iter().map(|o| orphan_json(o)).collect::<Vec<Value>>().into();
        v["keep"] = dirty.iter().map(|o| orphan_json(o)).collect::<Vec<Value>>().into();
        return Ok(v);
    }
    let ids: Vec<String> = panes.into_iter().map(|(id, _)| id).collect();
    on_main(move || {
        let app = app();
        for id in &ids {
            let _ = app.close_pane(id, false);
        }
        Ok(())
    })?;
    let (removed, kept) = if rp.keep_checkouts {
        (Vec::new(), found.iter().map(orphan_json).collect())
    } else {
        remove_orphans(&r, &found)
    };
    let dir = p
        .file
        .parent()
        .ok_or_else(|| Error::internal("project.toml has no folder"))?
        .to_path_buf();
    std::fs::remove_dir_all(&dir)
        .map_err(|e| Error::internal(format!("{}: {e}", dir.display())))?;
    let (pid, wsid) = (p.id.clone(), ws.clone());
    on_main(move || {
        let app = app();
        app.update(|m| {
            for s in m.sources.iter_mut().filter(|s| s.workspace == wsid) {
                for w in s.worktrees.iter_mut() {
                    if w.project.as_deref() == Some(pid.as_str()) {
                        w.project = None;
                    }
                }
            }
            if let Some(w) = m.workspaces.iter_mut().find(|w| w.id == wsid) {
                w.link_checkouts.retain(|r| r.project != pid);
                w.project_order.retain(|x| *x != pid);
            }
        });
        app.save_now();
        crate::navdata::refresh();
        crate::home_view::close();
        Ok(())
    })?;
    v["removed"] = removed.into();
    v["kept"] = kept.into();
    Ok(v)
}

#[derive(Deserialize)]
struct PlanSource {
    source: String,
    base: Option<String>,
    checkout: Option<String>,
}

#[derive(Deserialize)]
struct Plan {
    name: String,
    /// An existing Project whose links these are (the dialogs' edits).
    id: Option<String>,
    workspace: Option<String>,
    caller: Option<String>,
    #[serde(default)]
    sources: Vec<PlanSource>,
}

/// The Workspace for a new Project: `workspace`, else the caller's / shown.
fn target_workspace(m: &State, q: Option<&str>, caller: Option<&str>) -> Result<String> {
    Ok(match q {
        Some(q) => m.find_workspace(q)?.id.clone(),
        None => caller_workspace(m, caller),
    })
}

/// A Source of Workspace `ws` (not General), else `USAGE`.
fn ws_source(m: &State, ws: &str, q: &str) -> Result<String> {
    match m.find_source(q) {
        Ok(s) if s.workspace == ws && !s.is_general() => Ok(s.id.clone()),
        _ => Err(Error::usage(format!(
            "source '{q}' is not in workspace '{ws}' (rade source add <path> --workspace {ws})"
        ))),
    }
}

/// `project.plan` (R98.3): the checkout plan of a Project not written yet.
fn plan(pl: Plan) -> Result<Value> {
    let name = pl.name.trim().to_string();
    if name.is_empty() {
        return Err(Error::usage("a Project needs a name"));
    }
    let given_id = pl.id.clone();
    let (sc, links) = on_main(move || {
        let app = app();
        let m = app.model.borrow();
        let ws = target_workspace(&m, pl.workspace.as_deref(), pl.caller.as_deref())?;
        let mut links = Vec::new();
        for s in &pl.sources {
            let checkout = match s.checkout.as_deref() {
                None => project::Checkout::Auto,
                Some(c) => project::Checkout::parse(c).map_err(Error::usage)?,
            };
            links.push(project::ProjectSource {
                source: ws_source(&m, &ws, &s.source)?,
                base: s.base.clone().filter(|b| !b.trim().is_empty()),
                checkout,
            });
        }
        Ok((scene(&m, &ws), links))
    })?;
    let id = given_id.unwrap_or_else(|| rade_core::model::slug(&name));
    let extra = Project {
        id: id.clone(),
        file: Project::path(&sc.root, &id),
        name,
        sources: links,
        table: Default::default(),
        warnings: Vec::new(),
        error: None,
    };
    let r = resolve(sc, Some(extra));
    let root = canon(&r.scene.root);
    let names = r.scene.names();
    let links: Vec<Value> = r
        .resolved
        .iter()
        .filter(|x| x.project == id)
        .map(|x| {
            let mut v = json!({
                "source": x.source,
                "base": x.base,
                "kind": x.kind.map(checkout::Kind::as_str),
                "path": x.path,
                "create": x.create,
                "shared_with": x.shared_with.iter()
                    .map(|(p, b)| json!({"project": p, "base": b})).collect::<Vec<_>>(),
                "text": x.text(r.main_head(&x.source).as_deref(), &root),
            });
            let (kind, detail) = x.plan_line(r.main_head(&x.source).as_deref(), &names);
            v["plan_kind"] = kind.clone().into();
            v["plan"] = if detail.is_empty() {
                kind
            } else {
                format!("{kind} · {detail}")
            }
            .into();
            if let Some(why) = &x.refused {
                v["refused"] = why.clone().into();
            }
            v
        })
        .collect();
    Ok(json!({"project": id, "workspace": r.scene.ws, "links": links}))
}

#[derive(Deserialize)]
struct LinkParams {
    project: String,
    source: String,
    base: Option<String>,
    checkout: Option<String>,
    caller: Option<String>,
}

/// The Project, its Workspace and the Source id of a link edit.
fn link_target(l: &LinkParams) -> Result<(String, Project, String)> {
    let (q, src, caller) = (l.project.clone(), l.source.clone(), l.caller.clone());
    on_main(move || {
        let app = app();
        let m = app.model.borrow();
        let (ws, p) = find(&m, &q, caller.as_deref())?;
        if let Some(e) = &p.error {
            return Err(Error::usage(e.clone()));
        }
        let id = match m.find_source(&src) {
            Ok(s) if s.workspace == ws => s.id.clone(),
            _ if p.lists(&src) => src.clone(),
            _ => ws_source(&m, &ws, &src)?,
        };
        Ok((ws, p, id))
    })
}

fn link_result(ws: &str, p: &Project, source: &str) -> Value {
    json!({"project": p.id, "workspace": ws, "file": p.file, "source": source})
}

/// `project.link_add` (R97.3): append a `[[source]]`, refused when the
/// Source is linked already, not in the Workspace, or its `checkout`
/// can't be used.
fn link_add(l: LinkParams) -> Result<Value> {
    let (ws, p, src) = link_target(&l)?;
    let checkout = match l.checkout.as_deref() {
        None => None,
        Some(c) => Some(project::Checkout::parse(c).map_err(Error::usage)?),
    };
    let old = project::read_file(&p.file)
        .map_err(|e| Error::usage(format!("{}: {e}", p.file.display())))?;
    let new = project::link_add(&p.id, &old, &src, l.base.as_deref(), checkout.as_ref())?;
    if checkout
        .as_ref()
        .is_some_and(|c| *c != project::Checkout::Auto)
    {
        let ws2 = ws.clone();
        let sc = on_main(move || Ok(scene(&app().model.borrow(), &ws2)))?;
        let r = resolve(sc, Some(Project::parse(&p.id, &p.file, &new)));
        if let Some(e) = r.get(&p.id, &src).and_then(|x| x.error()) {
            return Err(e);
        }
    }
    {
        let p = p.clone();
        on_main(move || crate::project_settings::write_text(&p, &old, &new))?;
    }
    Ok(link_result(&ws, &p, &src))
}

/// `project.link_remove` / `project.link_set_base`: one link's lines, and
/// its recorded checkout forgotten (never a checkout or Worktree touched).
fn link_edit(l: LinkParams, set_base: bool) -> Result<Value> {
    let (ws, p, src) = link_target(&l)?;
    let old = project::read_file(&p.file)
        .map_err(|e| Error::usage(format!("{}: {e}", p.file.display())))?;
    let new = if set_base {
        let base = l
            .base
            .as_deref()
            .ok_or_else(|| Error::usage("set-base needs a branch"))?;
        project::link_set_base(&p.id, &old, &src, base)?
    } else {
        project::link_remove(&p.id, &old, &src)?
    };
    {
        let p = p.clone();
        on_main(move || crate::project_settings::write_text(&p, &old, &new))?;
    }
    let before = on_main({
        let ws = ws.clone();
        move || Ok(scene(&app().model.borrow(), &ws))
    })?;
    let was = before
        .records
        .iter()
        .find(|r| r.project == p.id && r.source == src)
        .map(|r| canon(&r.path));
    drop_record(&ws, &p.id, &src);
    let mut v = link_result(&ws, &p, &src);
    // Its old Project checkout, if no link uses it now (R133.2).
    if let Some(was) = was {
        let sc = on_main({
            let ws = ws.clone();
            move || Ok(scene(&app().model.borrow(), &ws))
        })?;
        let r = resolve(sc, None);
        let unused: Vec<Value> = checkout::orphans(&r.repos, &r.scene.folder, &used_paths(&r), &canon)
            .iter()
            .filter(|o| canon(&o.path) == was)
            .map(orphan_json)
            .collect();
        v["unused"] = unused.into();
    }
    Ok(v)
}

/// `project.link_to_base` (R100.2, Back to base): `git switch <base>` in
/// the link's drifted, clean checkout; refused when dirty or when the base
/// is checked out elsewhere; a no-op when not drifted.
fn link_to_base(l: LinkParams) -> Result<Value> {
    let (ws, p, src) = link_target(&l)?;
    if !p.lists(&src) {
        return Err(Error::usage(format!(
            "source '{src}' is not linked in project '{}'",
            p.id
        )));
    }
    let sc = on_main(move || Ok(scene(&app().model.borrow(), &ws)))?;
    let r = resolve(sc, None);
    record(&r);
    let res = r
        .get(&p.id, &src)
        .ok_or_else(|| Error::usage(format!("source '{src}' is not a Source of the Workspace")))?;
    if let Some(e) = res.error() {
        return Err(e);
    }
    let path = res.path.clone().unwrap_or_default();
    let repo = &r.repos[&src];
    let f = checkout::facts(repo, &path, &res.base, &canon);
    let d = f
        .head
        .as_ref()
        .filter(|_| f.exists)
        .and_then(|h| drift::drift(&res.base, h, f.dirty));
    let other = checkout::elsewhere(repo, &res.base, &path, &canon);
    let unused = other.as_deref().is_some_and(|o| {
        checkout::orphans(&r.repos, &r.scene.folder, &used_paths(&r), &canon)
            .iter()
            .any(|x| canon(&x.path) == canon(o))
    });
    let switched = drift::to_base(&src, d.as_ref(), other.as_deref()).map_err(|mut e| {
        if unused {
            e.message += " — an unused Project checkout; rade project prune removes it";
        }
        e
    })?;
    if switched {
        git::run(&path, &["switch", &res.base])?;
    }
    let head = if f.exists { git::branch(&path) } else { None };
    Ok(json!({"project": p.id, "source": src, "path": path, "head": head, "switched": switched}))
}

#[derive(Deserialize)]
struct AddSource {
    source: String,
    base: Option<String>,
    /// `auto` / `main` / a path (the New Project dialog writes `auto`).
    checkout: Option<String>,
}

#[derive(Deserialize)]
struct Add {
    name: String,
    workspace: Option<String>,
    caller: Option<String>,
    #[serde(default)]
    sources: Vec<AddSource>,
}

/// What looks like a path to `rade project add` (R31-a): the M15 alias.
pub fn looks_like_path(arg: &str) -> bool {
    arg.starts_with(['/', '.', '~']) || Path::new(arg).is_dir()
}

/// `project.add` (R31.1): writes the file unless it exists (idempotent).
fn add(a: Add) -> Result<(Value, String)> {
    if looks_like_path(&a.name) {
        return Err(Error::usage(
            "'rade project add <path>' is now 'rade source add <path>'",
        ));
    }
    let app = app();
    let m = app.model.borrow();
    let ws = match &a.workspace {
        Some(q) => m.find_workspace(q)?.id.clone(),
        None => caller_workspace(&m, a.caller.as_deref()),
    };
    let root = m.workspace(&ws).unwrap().root.clone();
    let mut sources = Vec::new();
    for s in &a.sources {
        let src = m.find_source(&s.source)?;
        if src.workspace != ws || src.is_general() {
            return Err(Error::usage(format!(
                "source '{}' is not in workspace '{ws}' (rade source add <path> --workspace {ws})",
                s.source
            )));
        }
        if let Some(c) = &s.checkout {
            project::Checkout::parse(c).map_err(Error::usage)?;
        }
        sources.push((
            src.id.clone(),
            s.base.clone().filter(|b| !b.trim().is_empty()),
            s.checkout.clone(),
        ));
    }
    let id = rade_core::model::slug(&a.name);
    let file = Project::path(&root, &id);
    if file.exists() {
        return Ok((json!({"project": id, "file": file, "existing": true}), ws));
    }
    let text = project::template_links(a.name.trim(), &sources);
    let io = |e: std::io::Error| Error::internal(format!("{}: {e}", file.display()));
    std::fs::create_dir_all(file.parent().unwrap()).map_err(io)?;
    std::fs::write(&file, text).map_err(io)?;
    drop(m);
    // A new Project goes last in the Navigator (R104.4).
    if app.update(|m| m.note_projects(&ws, std::slice::from_ref(&id))) {
        app.save_now();
    }
    crate::sidebar::projects_changed();
    Ok((json!({"project": id, "file": file, "existing": false}), ws))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Explain {
    key: Option<String>,
    pane: Option<String>,
    source: Option<String>,
    worktree: Option<String>,
    project: Option<String>,
    workspace: Option<String>,
    caller: Option<String>,
}

/// The layer context of `config.explain`: an explicit target, else the
/// caller's Pane, else the Source on screen.
fn context(m: &State, e: &Explain) -> Result<layers::Context> {
    if e.source.is_some() || e.project.is_some() || e.workspace.is_some() {
        return m.config_context(
            e.source.as_deref(),
            e.worktree.as_deref(),
            e.project.as_deref(),
            e.workspace.as_deref(),
        );
    }
    let loc = match e.pane.as_deref().or(e.caller.as_deref()) {
        Some(p) => Some(m.locate(&m.resolve_pane(p, e.caller.as_deref())?).unwrap()),
        None => None,
    };
    match loc {
        Some(l) => m.config_context(Some(&l.source), Some(&l.worktree), None, None),
        None => {
            let (s, w) = m.active();
            m.config_context(Some(&s.id), Some(&w.id), None, None)
        }
    }
}

/// `config.explain` (R29.3).
fn explain(e: Explain) -> Result<Value> {
    let app = app();
    let ctx = context(&app.model.borrow(), &e)?;
    let layers = layers::load(&app.paths.config, &ctx);
    let key = e.key.as_deref().unwrap_or("");
    let leaf = !key.is_empty()
        && layers
            .iter()
            .any(|l| layers::get(&l.table, key).is_some_and(|v| !v.is_table()));
    let mut v = if leaf
        || (!key.is_empty() && !layers.iter().any(|l| layers::get(&l.table, key).is_some()))
    {
        layers::explain(&layers, key)
    } else {
        layers::explain_all(&layers, key)
    };
    v["context"] = ctx.to_json();
    v["context_line"] = ctx.line().into();
    Ok(v)
}

#[derive(Deserialize)]
struct Trust {
    source: String,
}

/// `config.trust` (R29.4): accept the current `.rade.toml` content.
fn trust(t: Trust) -> Result<Value> {
    let app = app();
    let (id, file) = {
        let m = app.model.borrow();
        let s = m.find_source(&t.source)?;
        (s.id.clone(), s.path.join(layers::SOURCE_FILE))
    };
    let text =
        project::read_file(&file).map_err(|e| Error::usage(format!("{}: {e}", file.display())))?;
    project::check_toml(&text).map_err(|e| Error::usage(format!("{}: {e}", file.display())))?;
    app.update(|m| m.trusted_configs.insert(id.clone(), text));
    app.save_now();
    Ok(json!({"source": id, "file": file, "trusted": true}))
}

// ---- The sidebar's Project header (R30.3) ----------------------------------

/// `<ws>/<project>` → the Workspace id, the Project (read now).
pub(crate) fn lookup(key: &str) -> Option<(String, Project)> {
    let (ws, id) = key.split_once('/')?;
    let root = app().model.borrow().workspace(ws)?.root.clone();
    let p = project::list(&root).into_iter().find(|p| p.id == id)?;
    Some((ws.to_string(), p))
}

/// The header's right-click menu (actions of the sidebar's `sb` group).
pub fn menu(key: &str) -> gio::Menu {
    let menu = gio::Menu::new();
    for (label, action) in [
        ("Project settings…", "sb.project-settings"),
        ("Add source…", "sb.link-add"),
        ("New Worktree…", "sb.project-new-worktree"),
        ("Open project.toml", "sb.project-open"),
        ("Customize Project…", "sb.project-customize"),
    ] {
        let i = gio::MenuItem::new(Some(label), None);
        i.set_action_and_target_value(Some(action), Some(&format!("{key}\u{1f}").to_variant()));
        menu.append_item(&i);
    }
    let danger = gio::Menu::new();
    let i = gio::MenuItem::new(Some("Remove Project…"), None);
    i.set_action_and_target_value(
        Some("sb.project-remove"),
        Some(&format!("{key}\u{1f}").to_variant()),
    );
    danger.append_item(&i);
    menu.append_section(None, &danger);
    menu
}

/// The text of the Remove Project… confirmation from a `dry_run` answer;
/// paths relative to the Workspace root.
pub(crate) fn remove_body(v: &Value) -> String {
    let list = |k: &str| v[k].as_array().cloned().unwrap_or_default();
    let file = Path::new(v["file"].as_str().unwrap_or_default());
    // `<root>/.rade/projects/<id>/project.toml`
    let root = file.ancestors().nth(4).unwrap_or(Path::new("/"));
    let rel = |p: &str| {
        Path::new(p)
            .strip_prefix(root)
            .map_or_else(|_| p.to_string(), |r| r.display().to_string())
    };
    let path = |o: &Value| rel(o["path"].as_str().unwrap_or_default());
    let mut out = format!(
        "Deletes {} and its folder. This can't be undone.",
        rel(&file.display().to_string())
    );
    let remove = list("remove");
    if !remove.is_empty() {
        out += "\n\nRemoves its Project checkouts (clean, no other Project uses them):";
        for o in &remove {
            out += &format!("\n{}", path(o));
        }
    }
    let keep = list("keep");
    if !keep.is_empty() {
        out += "\n\nKeeps, with uncommitted changes:";
        for o in &keep {
            out += &format!("\n{} ({} changes)", path(o), o["dirty"]);
        }
    }
    let panes = list("panes");
    if !panes.is_empty() {
        let names: Vec<&str> = panes.iter().filter_map(|p| p["name"].as_str()).collect();
        out += &format!("\n\nCloses the Panes running there: {}.", names.join(", "));
    }
    let n = list("worktrees").len();
    if n > 0 {
        out += &if n == 1 {
            "\n\nIts Worktree moves to Scratch and keeps its base.".to_string()
        } else {
            format!("\n\nIts {n} Worktrees move to Scratch and keep their base.")
        };
    }
    out
}

/// Prune Unused Project Checkouts… (R133.2): the list first, one
/// confirmation, then `project.prune` for the shown Workspace.
pub fn prune_dialog() {
    let ws = app().model.borrow().active_workspace().to_string();
    let w = ws.clone();
    crate::worktrees::in_background(
        move || dispatch("project.prune", json!({"workspace": w, "dry_run": true})),
        move |r| {
            let v = match r {
                Ok(v) => v,
                Err(e) => return crate::worktrees::alert("Prune", &e.message),
            };
            let paths = |k: &str| -> Vec<String> {
                v[k].as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|o| o["path"].as_str().map(String::from))
                    .collect()
            };
            let (remove, keep) = (paths("remove"), paths("keep"));
            if remove.is_empty() {
                let mut body = "Every Project checkout is used by a link.".to_string();
                if !keep.is_empty() {
                    body = format!(
                        "The unused ones have uncommitted changes and stay:\n{}",
                        keep.join("\n")
                    );
                }
                return crate::worktrees::alert("Nothing to prune", &body);
            }
            let mut body = format!(
                "Removes {} clean Project checkout{} no link uses:\n{}",
                remove.len(),
                if remove.len() == 1 { "" } else { "s" },
                remove.join("\n")
            );
            if !keep.is_empty() {
                body += &format!("\n\nKeeps, with uncommitted changes:\n{}", keep.join("\n"));
            }
            crate::changes_ops::confirm("Prune Project checkouts?", &body, "delete", "Remove", move || {
                let w = ws.clone();
                crate::worktrees::in_background(
                    move || dispatch("project.prune", json!({"workspace": w})),
                    |r| {
                        if let Err(e) = r {
                            crate::worktrees::alert("Could not prune", &e.message);
                        }
                    },
                );
            });
        },
    );
}

/// Remove Project… (R133.1): the plan first (`dry_run`), one confirmation.
pub fn remove_dialog(key: &str) {
    let Some((_, p)) = lookup(key) else { return };
    let id = p.id.clone();
    crate::worktrees::in_background(
        move || dispatch("project.remove", json!({"project": id, "dry_run": true})),
        move |r| {
            let v = match r {
                Ok(v) => v,
                Err(e) => return crate::worktrees::alert("Remove Project", &e.message),
            };
            let d = adw::AlertDialog::new(
                Some(&format!("Remove Project “{}”?", p.name)),
                Some(&remove_body(&v)),
            );
            d.add_response("cancel", "Cancel");
            d.add_response("delete", "Remove Project");
            d.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            d.set_default_response(Some("cancel"));
            d.set_close_response("cancel");
            let id = p.id.clone();
            d.connect_response(None, move |_, r| {
                if r != "delete" {
                    return;
                }
                let id = id.clone();
                crate::worktrees::in_background(
                    move || dispatch("project.remove", json!({"project": id})),
                    |r| {
                        if let Err(e) = r {
                            crate::worktrees::alert("Could not remove the Project", &e.message);
                        }
                    },
                );
            });
            crate::menus::present(d, &["cancel", "delete"]);
        },
    );
}

/// "Open project.toml": a Viewer in the Workspace's General.
pub fn open_file(key: &str) {
    let Some((ws, p)) = lookup(key) else { return };
    let app = app();
    let r = app.update(|m| {
        let g = rade_core::general_id(&ws);
        let w = m
            .source(&g)
            .ok_or_else(|| Error::not_found("no General"))?
            .active_worktree
            .clone();
        m.select_source(&g)?;
        m.open_viewer_in(&g, &w, &p.file, None, true)
    });
    if let Err(e) = r {
        crate::worktrees::alert("Could not open project.toml", &e.message);
    }
}

/// The prompt of "Customize Project…" (R30.3, R32).
pub fn customize_prompt(p: &Project) -> String {
    format!(
        "Help me customize the RADE Project {name} ({id}); its file is {file}. \
         Use the rade-config skill: edit only RADE's files (this project.toml, the \
         Workspace's workspace.toml or a Source's .rade.toml), then run \
         `rade project show {id}` and `rade config explain --project {id}` until \
         both are clean. First ask me what to change.",
        name = p.name,
        id = p.id,
        file = p.file.display()
    )
}

/// "Customize Project…": an Agent in a new Tab of the Workspace's General
/// (rooted at the Workspace root), prompted to use the `rade-config` skill.
pub fn customize(key: &str) {
    let Some((ws, p)) = lookup(key) else { return };
    let app = app();
    let general = rade_core::general_id(&ws);
    let s = crate::handlers::Spawn {
        source: Some(general.clone()),
        tab: Some("new".into()),
        agent: Some("claude".into()),
        name: Some(format!("customize-{}", p.id)),
        prompt: Some(customize_prompt(&p)),
        ..Default::default()
    };
    match crate::handlers::spawn(&app, s) {
        Ok(v) => {
            let pane = v["pane"].as_str().unwrap_or_default().to_string();
            let _ = app.update(|m| {
                m.select_source(&general)?;
                m.focus_pane(&pane)
            });
        }
        Err(e) => crate::worktrees::alert("Could not start the Agent", &e.message),
    }
}

/// "New Worktree…" on a Project (R30.3): a Source dropdown and a name;
/// the Worktree starts from the link's base and records it (R99.1).
pub fn new_worktree_dialog(key: &str) {
    let Some((ws, p)) = lookup(key) else { return };
    let app = app();
    let sources: Vec<(String, String)> = {
        let m = app.model.borrow();
        p.sources
            .iter()
            .filter_map(|s| m.source(&s.source).filter(|x| x.workspace == ws))
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect()
    };
    if let Some(e) = &p.error {
        return crate::worktrees::alert("New Worktree", e);
    }
    if sources.is_empty() {
        return crate::worktrees::alert(
            "New Worktree",
            &format!("Project {} lists no Source of this Workspace.", p.name),
        );
    }
    let src =
        gtk::DropDown::from_strings(&sources.iter().map(|s| s.1.as_str()).collect::<Vec<_>>());
    src.add_css_class("rade-field-source");
    let name = gtk::Entry::new();
    name.set_placeholder_text(Some("branch name, e.g. fix-login"));
    name.set_activates_default(true);
    name.add_css_class("rade-worktree-name");
    name.add_css_class("rade-field-name");
    let hint = gtk::Label::new(None);
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.add_css_class("caption");
    hint.add_css_class("dim-label");
    hint.add_css_class("rade-field-hint");
    let form = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let row = |label: &str, w: &gtk::Widget| {
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let l = gtk::Label::new(Some(label));
        l.set_width_chars(7);
        l.set_xalign(0.0);
        w.set_hexpand(true);
        b.append(&l);
        b.append(w);
        b
    };
    form.append(&row("Source", src.upcast_ref()));
    form.append(&row("Name", name.upcast_ref()));
    form.append(&hint);

    let d = adw::AlertDialog::new(
        Some(&format!("New Worktree in {}", p.name)),
        Some("A new branch and checkout for this Project, from the Source link's base."),
    );
    d.set_extra_child(Some(&form));
    d.add_response("cancel", "Cancel");
    d.add_response("create", "Create");
    d.set_response_appearance("create", adw::ResponseAppearance::Suggested);
    d.set_default_response(Some("create"));
    d.set_close_response("cancel");
    d.set_response_enabled("create", false);

    let p = std::rc::Rc::new(p);
    let sources = std::rc::Rc::new(sources);
    // What the choice starts from, and whether the name is valid.
    let update = {
        let (d, src, name, hint, p, sources) = (
            d.clone(),
            src.clone(),
            name.clone(),
            hint.clone(),
            p.clone(),
            sources.clone(),
        );
        move || {
            let n = name.text().to_string();
            let valid = git::validate_name(&n);
            d.set_response_enabled("create", valid.is_ok());
            let s = sources.get(src.selected() as usize).map(|s| s.0.clone());
            let from = match s.map(|s| project::resolve_base(&p, &s, None)) {
                Some(Ok(Some(br))) => format!("Starts from {br}."),
                Some(Err(e)) => e.message,
                _ => "Starts from the Source's main branch.".into(),
            };
            hint.set_label(&match &valid {
                Err(e) if !n.is_empty() => e.message.clone(),
                _ => from,
            });
        }
    };
    update();
    {
        let u = update.clone();
        name.connect_changed(move |_| u());
        src.connect_selected_notify(move |_| update());
    }
    let entry = name.clone();
    d.connect_response(None, move |_, r| {
        if r != "create" {
            return;
        }
        let n = entry.text().to_string();
        let Some((sid, _)) = sources.get(src.selected() as usize).cloned() else {
            return;
        };
        let pid = p.id.clone();
        crate::worktrees::in_background(
            move || crate::worktrees::create(&sid, Some(&n), None, None, Some(&pid)),
            |r| match r {
                Ok(c) => crate::worktrees::activate(&c.source, &c.worktree),
                Err(e) => crate::worktrees::alert("Could not create the Worktree", &e.message),
            },
        );
    });
    crate::menus::present(d, &["cancel", "create"]);
    glib::idle_add_local_once(move || {
        name.grab_focus();
    });
}
