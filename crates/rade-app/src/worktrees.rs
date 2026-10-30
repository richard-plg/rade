//! Worktrees in the app (PRD R12): `worktree.*` socket methods, `spawn
//! --worktree new`, importing git's existing worktrees, and the New /
//! Remove dialogs. Git runs off the GTK thread (a socket thread or
//! `gio::spawn_blocking`); model changes run on it through [`on_main`].

use crate::app::{app, try_app};
use crate::handlers::{on_main, params, Spawn};
use adw::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::rpc::{Error, Result};
use rade_core::{git, paths};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Deserialize)]
pub struct AddParams {
    pub source: String,
    pub name: Option<String>,
    pub base: Option<String>,
    pub project: Option<String>,
}

#[derive(Deserialize)]
struct RemoveParams {
    source: String,
    name: String,
    #[serde(default)]
    force: bool,
    #[serde(default)]
    delete_branch: bool,
}

#[derive(Deserialize)]
struct MoveParams {
    source: String,
    worktree: String,
    /// `None` = Scratch.
    project: Option<String>,
    /// Compare against the new link's base from now on.
    #[serde(default)]
    rebase: bool,
}

#[derive(Deserialize)]
struct ListParams {
    source: Option<String>,
    caller: Option<String>,
}

/// Socket methods that run git: called on the socket thread.
pub fn dispatch(method: &str, p: Value) -> Result<Value> {
    match method {
        "worktree.add" => {
            let a: AddParams = params(p)?;
            let project = match a.project.clone() {
                Some(p) => Some(p),
                None => infer_project(&a.source)?,
            };
            let c = create(
                &a.source,
                a.name.as_deref(),
                a.base.as_deref(),
                None,
                project.as_deref(),
            )?;
            Ok(c.to_json())
        }
        "worktree.remove" => {
            let r: RemoveParams = params(p)?;
            remove(&r.source, &r.name, r.force, r.delete_branch)
        }
        "worktree.move" => {
            let a: MoveParams = params(p)?;
            on_main(move || move_to(&a.source, &a.worktree, a.project, a.rebase))
        }
        "worktree.list" => {
            let l: ListParams = params(p)?;
            let source = match l.source {
                Some(p) => p,
                None => on_main(move || {
                    let app = app();
                    let m = app.model.borrow();
                    Ok(match l.caller.and_then(|c| m.locate(&c)) {
                        Some(loc) => loc.source,
                        None => m.active().0.id.clone(),
                    })
                })?,
            };
            import(&source);
            on_main(move || list(&source))
        }
        _ => Err(Error::usage(format!("unknown method '{method}'"))),
    }
}

/// The Project of a new Worktree of `source` when `worktree.add` names
/// none (R99.1): the only Project linking it; several → `USAGE`.
fn infer_project(source: &str) -> Result<Option<String>> {
    let q = source.to_string();
    on_main(move || {
        let app = app();
        let m = app.model.borrow();
        let s = m.find_source(&q)?;
        let Some(ws) = m.workspace(&s.workspace) else {
            return Ok(None);
        };
        rade_core::project::infer(&rade_core::project::list(&ws.root), &s.id)
    })
}

/// The base a Worktree of `source` compares against (R99.2): its recorded
/// one, else its Project link's, else the Source's main branch; with
/// `project` given, the base that Project's link would give it.
pub(crate) fn comparison_base(
    m: &rade_core::State,
    source: &str,
    worktree: &str,
    project: Option<&str>,
) -> Option<String> {
    let s = m.source(source)?;
    let w = s.worktrees.iter().find(|w| w.id == worktree)?;
    let ws = m.workspace(&s.workspace)?;
    let projects = rade_core::project::list(&ws.root);
    let link_base = |p: &str| {
        projects
            .iter()
            .find(|x| x.id == p)
            .and_then(|x| x.source(&s.id))
            .map(|l| l.base.clone())
    };
    let main = || git::default_branch(&s.path);
    match project {
        Some(p) => link_base(p)?.or_else(main),
        None => w
            .base
            .clone()
            .or_else(|| w.project.as_deref().and_then(link_base).flatten())
            .or_else(main),
    }
}

/// `worktree.move` (R132.1): a Worktree to a Project linking its Source
/// (`None` = Scratch); `rebase` re-records its base as the new link's.
pub fn move_to(source: &str, worktree: &str, project: Option<String>, rebase: bool) -> Result<Value> {
    let app = app();
    let (sid, wid, base) = {
        let m = app.model.borrow();
        let (sid, wid) = m.find_worktree(source, Some(worktree))?;
        if let Some(p) = &project {
            let ws = &m.source(&sid).unwrap().workspace;
            let root = m.workspace(ws).map(|w| w.root.clone()).unwrap_or_default();
            let listed = rade_core::project::list(&root)
                .iter()
                .any(|x| x.id == *p && x.lists(&sid));
            if !listed {
                return Err(Error::usage(format!(
                    "project '{p}' doesn't link source '{sid}'"
                )));
            }
        }
        let base = match (&project, rebase) {
            (Some(p), true) => Some(
                comparison_base(&m, &sid, &wid, Some(p))
                    .ok_or_else(|| Error::usage(format!("no base for '{sid}' in '{p}'")))?,
            ),
            _ => None,
        };
        (sid, wid, base)
    };
    app.update(|m| m.move_worktree(&sid, &wid, project.clone(), base))?;
    let m = app.model.borrow();
    let w = m.worktree(&sid, &wid).unwrap();
    Ok(json!({"source": sid, "worktree": w.name, "project": w.project, "base": w.base}))
}

/// A Terminal on a Project's Source link (R98.1): resolve the link's
/// checkout, create a Project checkout when the plan says so (a notice
/// tells), record it, then open a new Tab of the Source whose Terminal
/// starts in that checkout. M36 reaches it only through the test-only
/// `debug.action` `link_terminal`; M37's Project home calls it. Off the
/// GTK thread.
pub fn open_on_link(project: &str, source: &str) -> Result<Value> {
    let (q, src) = (project.to_string(), source.to_string());
    let (ws, pid, sid, sc) = on_main(move || {
        let app = app();
        let m = app.model.borrow();
        let (ws, p) = crate::projects::find(&m, &q, None)?;
        let sid = m.find_source(&src)?.id.clone();
        if !p.lists(&sid) {
            return Err(Error::usage(format!(
                "source '{sid}' is not linked in project '{}'",
                p.id
            )));
        }
        m.check_not_paused(&sid)?;
        Ok((
            ws.clone(),
            p.id.clone(),
            sid,
            crate::projects::scene(&m, &ws),
        ))
    })?;
    let r = crate::projects::resolve(sc, None);
    let res = r
        .get(&pid, &sid)
        .cloned()
        .ok_or_else(|| Error::usage(format!("source '{sid}' is not a Source of '{ws}'")))?;
    if let Some(e) = res.error() {
        return Err(e);
    }
    for c in &res.commands {
        rade_core::checkout::apply(c)?;
    }
    let path = res.path.clone().unwrap_or_default();
    // Created now: resolve again so the new checkout is recorded.
    let r = if res.create {
        let ws2 = ws.clone();
        let sc = on_main(move || Ok(crate::projects::scene(&app().model.borrow(), &ws2)))?;
        crate::projects::resolve(sc, None)
    } else {
        r
    };
    crate::projects::record(&r);
    let (created, p2) = (res.create, path.clone());
    let pane = on_main(move || {
        let app = app();
        let pane = app.update(|m| {
            let s = m
                .source(&sid)
                .ok_or_else(|| Error::not_found("no source"))?;
            let w = s.worktrees[0].id.clone();
            let (tab, pane) = m.new_tab(&sid, &w, None, Some(format!("{pid}/{sid}")))?;
            m.set_pane_cwd(&pane, &p2);
            m.select_source(&sid)?;
            m.select_tab(&tab)?;
            Ok::<_, Error>(pane)
        })?;
        let what = if created {
            "Created the Project checkout"
        } else {
            "Terminal in"
        };
        app.chrome.toast(
            &glib::markup_escape_text(&format!("{what} {}", p2.display())),
            None,
        );
        Ok(pane)
    })?;
    Ok(
        json!({"pane": pane, "path": path, "created": created, "kind": res.kind.map(|k| k.as_str())}),
    )
}

/// `spawn --worktree new` (API.md placement rule 4), on the socket thread:
/// check the Preset and limits, create the Worktree (named `--name`, else
/// `wt-<agent>-<n>`), then spawn in a new Tab there.
pub fn spawn_in_new_worktree(mut s: Spawn) -> Result<Value> {
    let check = Spawn {
        source: s.source.clone(),
        agent: s.agent.clone(),
        cmd: s.cmd.clone(),
        prompt: s.prompt.clone(),
        account: s.account.clone(),
        caller: s.caller.clone(),
        ..Default::default()
    };
    let pid = on_main(move || {
        let app = app();
        let limits = app.config.borrow().limits.clone();
        let pid = {
            let m = app.model.borrow();
            m.check_limits(check.caller.as_deref(), &limits)?;
            let pid = match (
                &check.source,
                check.caller.as_deref().and_then(|c| m.locate(c)),
            ) {
                (Some(q), _) => m.find_source(q)?.id.clone(),
                (None, Some(loc)) => loc.source,
                (None, None) => m.active().0.id.clone(),
            };
            m.check_not_paused(&pid)?; // before any git (R19.3)
            pid
        };
        // The Preset, the account lock and the command check, before any
        // git: a refused spawn leaves no Worktree behind (R108.3).
        crate::handlers::plan_spawn(&app, &check, Some(&(pid.clone(), None)))?;
        Ok(pid)
    })?;
    let c = create(&pid, s.name.as_deref(), None, s.agent.as_deref(), None)?;
    s.source = Some(c.source);
    s.worktree = Some(c.worktree);
    s.tab = Some("new".into());
    on_main(move || crate::handlers::spawn(&app(), s))
}

/// A created Worktree.
pub struct Created {
    pub source: String,
    pub worktree: String,
    pub name: String,
    pub path: PathBuf,
    pub added: git::Added,
    pub project: Option<String>,
}

impl Created {
    fn to_json(&self) -> Value {
        json!({
            "source": self.source, "worktree": self.name, "id": self.worktree,
            "path": self.path, "branch": self.name,
            "created_branch": self.added.created_branch, "base": self.added.base,
            "project": self.project,
        })
    }
}

/// `git worktree add` under `<Workspace root>/.worktrees[-dev]/<source>/<name>`
/// (`$XDG_DATA_HOME/rade/worktrees/…` in `default`; ADR-0014), then the
/// model. `name` `None` = `wt-<agent>-<n>`. With `project` (R28.2) a Named
/// Base `base` (none: the default) resolves to the Source's branch, and the
/// Worktree records both. Must not run on the GTK thread.
pub fn create(
    source: &str,
    name: Option<&str>,
    base: Option<&str>,
    agent: Option<&str>,
    project: Option<&str>,
) -> Result<Created> {
    let base_given = base.is_some();
    let (q, name, agent, pq, bq) = (
        source.to_string(),
        name.map(String::from),
        agent.map(String::from),
        project.map(String::from),
        base.map(String::from),
    );
    let (pid, repo, name, setup, dir, project, base) = on_main(move || {
        let app = app();
        let m = app.model.borrow();
        let p = m.find_source(&q)?;
        if p.is_general() {
            return Err(Error::usage("General has no git repository; pick a Source"));
        }
        let taken = |n: &str| p.worktrees.iter().any(|w| w.name == n);
        let name = name.unwrap_or_else(|| git::auto_name(agent.as_deref(), &taken));
        if taken(&name) {
            return Err(Error::usage(format!(
                "source '{}' already has a worktree '{name}'",
                p.name
            )));
        }
        let mut ctx = m.config_context(Some(&p.id), None, None, None)?;
        let (project, base) = match &pq {
            Some(q) => {
                let proj = m.find_project(Some(&p.workspace), q)?;
                let b = rade_core::project::resolve_base(&proj, &p.id, bq.as_deref())?;
                let id = proj.id.clone();
                ctx.project = Some(proj);
                (Some(id), b)
            }
            None => (None, bq),
        };
        let setup = setup_command(&app, &ctx, &p.id, &p.name);
        let dir = m.worktree_dir(
            &p.id,
            &name,
            &paths::data_dir(&paths::process_env),
            paths::Flavor::current(),
        );
        Ok((
            p.id.clone(),
            p.path.clone(),
            name,
            setup,
            dir,
            project,
            base,
        ))
    })?;
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::internal(format!("{}: {e}", parent.display())))?;
    }
    // A Project's default base is only recorded for an existing branch
    // (it is checked out, not created).
    let pass = match (&project, base.as_deref()) {
        (Some(_), b) if git::branches(&repo).contains(&name) => b.filter(|_| base_given),
        (_, b) => b,
    };
    let added = git::add_worktree(&repo, &dir, &name, pass)?;
    // R99.1: in a Project, a link without `base` records the main branch the
    // Worktree was cut from, so a later set-base does not move it (R99.2).
    let main = (project.is_some() && base.is_none())
        .then(|| git::default_branch(&repo))
        .flatten();
    let base =
        rade_core::worktree::recorded_base(project.is_some(), base.as_deref(), main.as_deref());
    let (p2, n2, d2, pr2, b2) = (
        pid.clone(),
        name.clone(),
        dir.clone(),
        project.clone(),
        base.clone(),
    );
    let (source, worktree) = on_main(move || {
        app().update(|m| {
            let (s, w) = m.add_worktree(&p2, &n2, &d2, setup)?;
            m.set_worktree_origin(&s, &w, pr2, b2)?;
            Ok::<_, Error>((s, w))
        })
    })?;
    Ok(Created {
        source,
        worktree,
        name,
        path: dir,
        added,
        project,
    })
}

/// R12.3: refuse a dirty Worktree unless `force`, close its Panes, `git
/// worktree remove`, then optionally `git branch -d` (`-D` with `force`).
/// A Worktree whose folder is gone is pruned instead. Off the GTK thread.
pub fn remove(source: &str, name: &str, force: bool, delete_branch: bool) -> Result<Value> {
    let (q, n) = (source.to_string(), name.to_string());
    let (pid, wid, repo, path) = on_main(move || {
        let app = app();
        let m = app.model.borrow();
        let (p, w) = m.find_worktree(&q, Some(&n))?;
        if m.is_main_worktree(&p, &w) {
            return Err(Error::usage("the main worktree can't be removed"));
        }
        let wt = m.worktree(&p, &w).unwrap();
        Ok((
            p.clone(),
            w,
            m.source(&p).unwrap().path.clone(),
            wt.path.clone(),
        ))
    })?;
    let missing = !path.exists();
    // git's record: `None` when detached (never delete a commit hash).
    let branch = if missing {
        None
    } else {
        git::worktree_branch(&repo, &path)
    };
    if !missing {
        if let Some(reason) = git::lock_reason(&repo, &path)? {
            return Err(Error::usage(format!(
                "worktree '{name}' is locked{}; run `git worktree unlock {}` first",
                if reason.is_empty() {
                    String::new()
                } else {
                    format!(" ({reason})")
                },
                path.display()
            )));
        }
        if !force {
            let dirty = git::dirty(&path)?;
            if !dirty.is_empty() {
                let shown: Vec<&str> = dirty.iter().take(5).map(String::as_str).collect();
                return Err(Error::usage(format!(
                    "worktree '{name}' has {} uncommitted or untracked file(s) ({}{}); pass --force to discard them",
                    dirty.len(),
                    shown.join(", "),
                    if dirty.len() > 5 { ", …" } else { "" }
                )));
            }
            // git refuses these without --force; say so before closing Panes.
            if path.join(".gitmodules").exists() {
                return Err(Error::usage(format!(
                    "worktree '{name}' contains submodules; pass --force to remove it"
                )));
            }
        }
    }
    // Its language servers (R83.3), its Panes (SIGHUP, like `rade close`), then git.
    crate::code::stop_under(&path);
    let (p2, w2) = (pid.clone(), wid.clone());
    on_main(move || {
        let app = app();
        app.update(|m| {
            let tabs: Vec<String> = m
                .worktree(&p2, &w2)
                .map(|w| w.tabs.iter().map(|t| t.id.clone()).collect())
                .unwrap_or_default();
            for t in tabs {
                let _ = m.close_tab(&t);
            }
            // Fan-out Agents shown in another Worktree's Tab (R15.2).
            let elsewhere: Vec<String> =
                m.panes_in(&p2, &w2).iter().map(|x| x.id.clone()).collect();
            for id in elsewhere {
                let _ = m.close_pane(&id);
            }
        });
        app.save_now();
        Ok(())
    })?;
    if missing {
        git::prune(&repo)?;
    } else {
        git::remove_worktree(&repo, &path, force)?;
    }
    // Its Review Rounds' snapshots go with it (R119.1).
    rade_core::review_loop::drop_refs(&repo, name, None);
    // A name with `/` (`task/T-12-…`, R35.1) made a parent folder: drop it
    // once empty (remove_dir fails otherwise, which is fine).
    if name.contains('/') {
        if let Some(parent) = path.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }
    let (p2, w2) = (pid.clone(), wid.clone());
    on_main(move || app().update(|m| m.remove_worktree(&p2, &w2)).map(drop))?;
    let mut branch_deleted = false;
    let mut branch_error = None;
    if let (true, Some(b)) = (delete_branch, &branch) {
        match git::delete_branch(&repo, b, force) {
            Ok(()) => branch_deleted = true,
            Err(e) => branch_error = Some(e.message),
        }
    }
    Ok(json!({
        "source": pid, "worktree": name, "pruned": missing, "branch": branch,
        "branch_deleted": branch_deleted, "branch_error": branch_error,
    }))
}

/// Add git's worktrees the model lacks (created outside RADE). Off the
/// GTK thread; quietly does nothing outside git.
pub fn import(source: &str) {
    let q = source.to_string();
    let Ok((repo, folder, records)) = on_main(move || {
        let app = app();
        let m = app.model.borrow();
        let p = m.find_source(&q)?;
        if p.is_general() {
            return Err(Error::usage("general"));
        }
        let folder = m.worktrees_folder(
            &p.workspace,
            &paths::data_dir(&paths::process_env),
            paths::Flavor::current(),
        );
        let records: Vec<_> = m
            .workspaces
            .iter()
            .flat_map(|w| w.link_checkouts.clone())
            .collect();
        Ok((p.path.clone(), folder, records))
    }) else {
        return;
    };
    let Ok(mut listed) = git::list_worktrees(&repo) else {
        return;
    };
    // Project checkouts are a link's, never Worktrees (R98.1).
    listed.retain(|w| {
        !rade_core::checkout::is_project_checkout(
            &w.path,
            &folder,
            &records,
            &crate::projects::canon,
        )
    });
    let q = source.to_string();
    let _ = on_main(move || {
        let app = app();
        let n = app.model.borrow().find_source(&q).map(|p| p.id.clone())?;
        if app
            .model
            .borrow_mut()
            .import_worktrees(&n, &listed, &git::same)
            .is_empty()
        {
            return Ok(());
        }
        app.update(|_| {});
        Ok(())
    });
}

/// Import for every Source, in the background (app start).
pub fn import_all() {
    let ids: Vec<String> = app()
        .model
        .borrow()
        .sources
        .iter()
        .filter(|p| !p.is_general())
        .map(|p| p.id.clone())
        .collect();
    std::thread::spawn(move || ids.iter().for_each(|p| import(p)));
}

fn list(source: &str) -> Result<Value> {
    let app = app();
    let m = app.model.borrow();
    let p = m.find_source(source)?;
    let rows: Vec<Value> = p
        .worktrees
        .iter()
        .enumerate()
        .map(|(i, w)| {
            json!({
                "worktree": w.name, "id": w.id, "path": w.path, "main": i == 0,
                "branch": git::branch(&w.path), "missing": !w.path.exists(),
                "tabs": w.tabs.len(), "comments": w.comments.len(),
                "active": w.id == p.active_worktree,
                "project": w.project, "base": w.base,
            })
        })
        .collect();
    Ok(json!({"source": p.id, "worktrees": rows}))
}

/// Show a Worktree; opens a first Tab if it has none (like a Source).
pub fn activate(source: &str, worktree: &str) {
    let Some(app) = try_app() else { return };
    app.update(|m| {
        if m.select_worktree(source, worktree).is_err() {
            return;
        }
        let (p, w) = m.active();
        if w.tabs.is_empty() && w.path.exists() {
            let (p, w) = (p.id.clone(), w.id.clone());
            let _ = m.new_tab(&p, &w, None, None);
        }
    });
}

/// Run `f` off the GTK thread, then `done` with its result on it.
pub fn in_background<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T) + 'static,
) {
    glib::spawn_future_local(async move {
        if let Ok(r) = gio::spawn_blocking(f).await {
            done(r);
        }
    });
}

pub(crate) fn alert(heading: &str, body: &str) {
    let d = adw::AlertDialog::new(Some(heading), Some(body));
    d.add_response("ok", "OK");
    d.present(Some(&app().window));
}

/// "New Worktree…" (R12.1): name + base branch; an existing branch name is
/// checked out instead of created.
pub fn new_dialog(source: &str) {
    let app = app();
    let (pid, pname, repo) = {
        let m = app.model.borrow();
        let Ok(p) = m.find_source(source) else {
            return;
        };
        if p.is_general() {
            return alert(
                "New Worktree",
                "General has no git repository. Select a Source first.",
            );
        }
        (p.id.clone(), p.name.clone(), p.path.clone())
    };
    // Git runs off the GTK thread; the dialog opens when it answers.
    in_background(
        move || {
            git::require_toplevel(&repo)?;
            Ok::<_, Error>((git::branches(&repo), git::default_branch(&repo)))
        },
        move |r| match r {
            Ok((branches, default)) => show_new_dialog(pid, &pname, branches, default),
            Err(e) => alert("New Worktree", &e.message),
        },
    );
}

fn show_new_dialog(pid: String, pname: &str, branches: Vec<String>, default: Option<String>) {
    let app = app();
    let mut bases = branches.clone();
    if let Some(d) = &default {
        bases.retain(|b| b != d);
        bases.insert(0, d.clone());
    }
    bases.push("HEAD".into());

    let name = gtk::Entry::new();
    name.set_placeholder_text(Some("branch name, e.g. feat-login"));
    name.set_activates_default(true);
    name.add_css_class("rade-worktree-name");
    let base = gtk::DropDown::from_strings(&bases.iter().map(String::as_str).collect::<Vec<_>>());
    base.set_tooltip_text(Some("Base branch"));
    let hint = gtk::Label::new(None);
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    hint.add_css_class("caption");
    hint.add_css_class("dim-label");
    let form = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let row = |label: &str, w: &gtk::Widget| {
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let l = gtk::Label::new(Some(label));
        l.set_width_chars(6);
        l.set_xalign(0.0);
        w.set_hexpand(true);
        b.append(&l);
        b.append(w);
        b
    };
    form.append(&row("Name", name.upcast_ref()));
    form.append(&row("Base", base.upcast_ref()));
    form.append(&hint);

    let d = adw::AlertDialog::new(
        Some("New Worktree"),
        Some(&format!(
            "A new branch and checkout of {pname}, next to it in RADE's data folder."
        )),
    );
    d.set_extra_child(Some(&form));
    d.add_response("cancel", "Cancel");
    d.add_response("create", "Create");
    d.set_response_appearance("create", adw::ResponseAppearance::Suggested);
    d.set_default_response(Some("create"));
    d.set_close_response("cancel");
    d.set_response_enabled("create", false);
    {
        let (d, base, hint) = (d.clone(), base.clone(), hint.clone());
        name.connect_changed(move |e| {
            let n = e.text().to_string();
            let valid = git::validate_name(&n);
            let exists = valid.is_ok() && branches.contains(&n);
            d.set_response_enabled("create", valid.is_ok());
            base.set_sensitive(!exists);
            hint.set_label(&match (&valid, exists) {
                (Err(e), _) if !n.is_empty() => e.message.clone(),
                (Ok(()), true) => format!("Branch '{n}' exists: it will be checked out."),
                _ => String::new(),
            });
        });
    }
    let entry = name.clone();
    d.connect_response(Some("create"), move |_, _| {
        let n = entry.text().to_string();
        let b = (base.is_sensitive())
            .then(|| bases.get(base.selected() as usize).cloned())
            .flatten();
        let pid = pid.clone();
        in_background(
            move || create(&pid, Some(&n), b.as_deref(), None, None),
            |r| match r {
                Ok(c) => activate(&c.source, &c.worktree),
                Err(e) => alert("Could not create the Worktree", &e.message),
            },
        );
    });
    d.present(Some(&app.window));
    name.grab_focus();
}

/// A new Worktree's `setup` (R12.2) from the layered config (R29.2); an
/// untrusted `.rade.toml` adds one line saying so (R29.4).
fn setup_command(
    app: &crate::app::App,
    ctx: &rade_core::layers::Context,
    id: &str,
    name: &str,
) -> Option<String> {
    let layers = rade_core::layers::load(&app.paths.config, ctx);
    let setup = rade_core::layers::effective(&layers).setup_for(id, name);
    if ctx.trusted != Some(false) {
        return setup;
    }
    let note = format!(
        "printf '%s\\n' {}",
        rade_core::preset::shell_quote(&format!(
            "rade: {}/{} untrusted: rade config trust {id}",
            ctx.source
                .as_ref()
                .map_or(String::new(), |s| s.1.display().to_string()),
            rade_core::layers::SOURCE_FILE
        ))
    );
    Some(match setup {
        Some(s) => format!("{note}; {s}"),
        None => note,
    })
}

/// "Remove Worktree…" (R12.3): confirm (force when dirty), then ask about
/// the branch; an unmerged branch needs a second confirmation for `-D`.
pub fn remove_dialog(source: &str, worktree: &str) {
    let app = app();
    let (pid, name, path, repo) = {
        let m = app.model.borrow();
        let Ok((p, w)) = m.find_worktree(source, Some(worktree)) else {
            return;
        };
        if m.is_main_worktree(&p, &w) {
            return;
        }
        let wt = m.worktree(&p, &w).unwrap();
        let repo = m.source(&p).unwrap().path.clone();
        (p, wt.name.clone(), wt.path.clone(), repo)
    };
    let missing = !path.exists();
    let p2 = path.clone();
    in_background(
        move || {
            if missing {
                return (vec![], None, false);
            }
            (
                git::dirty(&p2).unwrap_or_default(),
                git::worktree_branch(&repo, &p2),
                p2.join(".gitmodules").exists(),
            )
        },
        move |(dirty, branch, subs)| {
            show_remove_dialog(pid, name, path, missing, dirty, branch, subs)
        },
    );
}

fn show_remove_dialog(
    pid: String,
    name: String,
    path: std::path::PathBuf,
    missing: bool,
    dirty: Vec<String>,
    branch: Option<String>,
    subs: bool,
) {
    let mut body = format!("{}\n\nIts Terminals close.", path.display());
    if !dirty.is_empty() {
        body += &format!(
            " It has {} uncommitted or untracked file(s), which will be lost.",
            dirty.len()
        );
    }
    // git removes a Worktree with submodules only with --force; clean, it
    // loses nothing, so Remove passes it (the branch still gets `-d`).
    let subs = subs && !missing && dirty.is_empty();
    if subs {
        body += " Its submodules are removed with it.";
    }
    let d = adw::AlertDialog::new(Some(&format!("Remove Worktree “{name}”?")), Some(&body));
    let del = gtk::CheckButton::with_label(&format!(
        "Also delete branch {}",
        branch.as_deref().unwrap_or("-")
    ));
    del.set_sensitive(branch.is_some() && !missing);
    d.set_extra_child(Some(&del));
    d.add_response("cancel", "Cancel");
    let (resp, label) = match (missing, dirty.is_empty()) {
        (true, _) => ("remove", "Prune"),
        (false, true) if subs => ("submodules", "Remove"),
        (false, true) => ("remove", "Remove"),
        (false, false) => ("force", "Discard Changes and Remove"),
    };
    d.add_response(resp, label);
    d.set_response_appearance(resp, adw::ResponseAppearance::Destructive);
    d.set_close_response("cancel");
    d.connect_response(None, move |_, r| {
        if r == "cancel" {
            return;
        }
        let subs = r == "submodules";
        let force = r == "force" || subs;
        let (pid, name, del) = (pid.clone(), name.clone(), del.is_active());
        in_background(
            {
                let (pid, name) = (pid.clone(), name.clone());
                move || -> Result<Value> {
                    // `force` would make it `branch -D`: delete with `-d` here.
                    let mut v = remove(&pid, &name, force, del && !subs)?;
                    if let (true, Some(b)) = (del && subs, v["branch"].as_str()) {
                        let repo = on_main({
                            let pid = pid.clone();
                            move || {
                                Ok(crate::app::app()
                                    .model
                                    .borrow()
                                    .source(&pid)
                                    .map(|p| p.path.clone()))
                            }
                        })?;
                        if let Some(repo) = repo {
                            match git::delete_branch(&repo, b, false) {
                                Ok(()) => v["branch_deleted"] = json!(true),
                                Err(e) => v["branch_error"] = json!(e.message),
                            }
                        }
                    }
                    Ok(v)
                }
            },
            move |res| match res {
                Err(e) => alert("Could not remove the Worktree", &e.message),
                Ok(v) => {
                    if let (Some(err), Some(b)) = (v["branch_error"].as_str(), v["branch"].as_str())
                    {
                        confirm_force_branch(&pid, b, err);
                    }
                }
            },
        );
    });
    crate::menus::present(d, &["cancel", resp]);
}

/// `git branch -d` refused (unmerged): offer `-D`.
fn confirm_force_branch(source: &str, branch: &str, err: &str) {
    let app = app();
    let Some(repo) = app.model.borrow().source(source).map(|p| p.path.clone()) else {
        return;
    };
    let d = adw::AlertDialog::new(
        Some(&format!("Branch “{branch}” is not merged")),
        Some(&format!(
            "{err}\n\nDelete it anyway? Its commits will only be reachable through the reflog."
        )),
    );
    d.add_response("keep", "Keep Branch");
    d.add_response("delete", "Delete Anyway");
    d.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    d.set_close_response("keep");
    let branch = branch.to_string();
    d.connect_response(Some("delete"), move |_, _| {
        let (repo, branch) = (repo.clone(), branch.clone());
        in_background(
            move || git::delete_branch(&repo, &branch, true),
            |r| {
                if let Err(e) = r {
                    alert("Could not delete the branch", &e.message);
                }
            },
        );
    });
    d.present(Some(&app.window));
}
