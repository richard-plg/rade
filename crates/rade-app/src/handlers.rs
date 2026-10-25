//! Socket methods (docs/API.md), run on the GTK main thread.

use crate::app::{app, App};
use crate::panes::now;
use gtk::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::preset::{new_uuid, PromptMode};
use rade_core::rpc::{Error, ErrorKind, Request, Response, Result};
use rade_core::{git, AgentStatus, SpawnSpec, SplitDir, TabChoice, WaitUntil};
use rade_ghostty_sys::{GHOSTTY_POINT_SCREEN, GHOSTTY_POINT_VIEWPORT};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Run `f` on the GTK main loop and wait for its result.
pub(crate) fn on_main<T: std::marker::Send + 'static>(
    f: impl FnOnce() -> Result<T> + std::marker::Send + 'static,
) -> Result<T> {
    let (tx, rx) = mpsc::channel();
    glib::MainContext::default().invoke(move || {
        let _ = tx.send(f());
    });
    rx.recv()
        .unwrap_or_else(|_| Err(Error::internal("app is shutting down")))
}

/// `rade-app` was started with `RADE_DEBUG_SOCKET=1`: serve `debug.*`
/// (ADR-0021). Read once in `main` before the variable is removed.
pub static DEBUG_SOCKET: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Called on a socket thread: run the request on the main loop and wait.
/// `pane.wait` polls from here instead, so it never blocks the UI.
pub fn dispatch(req: Request) -> Response {
    let id = req.id.clone();
    let enabled = DEBUG_SOCKET.get().copied().unwrap_or(false);
    // Methods removed in 1.0.0 say so, in test mode or not (R101.2).
    let r = if let Some(e) = rade_core::rpc::removed(&req.method) {
        Err(e)
    } else if let Err(e) = rade_core::rpc::gate_debug(&req.method, enabled) {
        Err(e)
    } else if req.method == "debug.action"
        && req.params.get("action").and_then(Value::as_str) == Some("link_terminal")
    {
        // Test-only (ADR-0021): a Terminal on a Project's Source link (R98.1).
        let s = |k: &str| req.params.get(k).and_then(Value::as_str).unwrap_or("");
        crate::worktrees::open_on_link(s("project"), s("source"))
    } else if req.method == "pane.wait" {
        params(req.params).and_then(wait)
    } else if req.method.starts_with("project.") || req.method.starts_with("config.") {
        crate::projects::dispatch(&req.method, req.params)
    } else if req.method.starts_with("account.") || req.method.starts_with("usage.") {
        crate::accounts::dispatch(&req.method, req.params)
    } else if req.method.starts_with("worktree.") {
        crate::worktrees::dispatch(&req.method, req.params)
    } else if req.method.starts_with("fanout.") || req.method == "debug.fanout" {
        crate::fanout::dispatch(&req.method, req.params)
    } else if req.method.starts_with("browser.") || req.method == "debug.browser" {
        crate::browser::dispatch(&req.method, req.params)
    } else if req.method.starts_with("review.") {
        crate::review::dispatch(&req.method, req.params)
    } else if req.method == "pane.spawn"
        && req.params.get("worktree").and_then(Value::as_str) == Some("new")
    {
        params(req.params).and_then(crate::worktrees::spawn_in_new_worktree)
    } else if req.method.starts_with("code.") || req.method == "debug.code" {
        crate::code::dispatch(&req.method, req.params)
    } else if req.method == "debug.palette" {
        crate::palette_run::debug(req.params)
    } else if req.method == "debug.viewer" {
        params(req.params).and_then(crate::debug::viewer)
    } else {
        on_main(move || handle(&req.method, req.params))
    };
    match r {
        Ok(v) => Response::ok(id, v),
        Err(e) => Response::err(id, &e),
    }
}

pub(crate) fn params<T: for<'de> Deserialize<'de>>(v: Value) -> Result<T> {
    let v = if v.is_null() { json!({}) } else { v };
    serde_json::from_value(v).map_err(|e| Error::usage(format!("bad params: {e}")))
}

#[derive(Deserialize)]
struct PaneParams {
    pane: String,
    caller: Option<String>,
}

#[derive(Deserialize)]
struct Close {
    pane: String,
    caller: Option<String>,
    #[serde(default)]
    force: bool,
}

#[derive(Deserialize)]
struct SourceAdd {
    path: PathBuf,
    name: Option<String>,
    workspace: Option<String>,
    caller: Option<String>,
}

#[derive(Deserialize)]
struct WorkspaceAdd {
    root: PathBuf,
    name: Option<String>,
}

#[derive(Deserialize)]
struct WorkspaceRef {
    workspace: String,
}

#[derive(Deserialize)]
struct SourceRef {
    source: String,
}

#[derive(Deserialize)]
struct SourceRename {
    source: String,
    name: String,
}

#[derive(Deserialize)]
struct SourcePause {
    source: String,
    #[serde(default)]
    force: bool,
}

#[derive(Deserialize)]
struct PaneRename {
    pane: String,
    name: Option<String>,
    caller: Option<String>,
}

#[derive(Deserialize)]
struct TabRename {
    tab: String,
    name: String,
    caller: Option<String>,
}

#[derive(Deserialize)]
struct TabClose {
    tab: String,
    /// only (default) | others | right
    mode: Option<String>,
    #[serde(default)]
    force: bool,
    caller: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct ListParams {
    source: Option<String>,
    /// Scope (R23.4): this Workspace, else the caller's, else the shown one;
    /// `all` = every Workspace.
    workspace: Option<String>,
    all: bool,
    caller: Option<String>,
}

impl ListParams {
    /// The Workspace listed, `None` = all.
    pub(crate) fn scope(&self, m: &rade_core::State) -> Result<Option<String>> {
        if self.all {
            return Ok(None);
        }
        if let Some(q) = &self.workspace {
            return Ok(Some(m.find_workspace(q)?.id.clone()));
        }
        let caller = self
            .caller
            .as_deref()
            .and_then(|c| m.locate(c))
            .and_then(|l| m.source(&l.source))
            .map(|s| s.workspace.clone());
        Ok(Some(
            caller.unwrap_or_else(|| m.active_workspace().to_string()),
        ))
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct Spawn {
    pub source: Option<String>,
    pub worktree: Option<String>,
    pub tab: Option<String>,
    pub split: Option<String>,
    pub agent: Option<String>,
    pub cmd: Option<String>,
    pub name: Option<String>,
    pub prompt: Option<String>,
    /// Agent Account (R108.2).
    pub account: Option<String>,
    pub caller: Option<String>,
}

#[derive(Deserialize)]
struct Wait {
    pane: String,
    until: String,
    #[serde(default = "default_timeout")]
    timeout: u64,
    caller: Option<String>,
}

fn default_timeout() -> u64 {
    600
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SetStatus {
    pane: Option<String>,
    status: Option<String>,
    session_id: Option<String>,
    agent: Option<String>,
    caller: Option<String>,
    invocation: Option<rade_core::preset::Invocation>,
}

#[derive(Deserialize)]
struct Notify {
    text: String,
    title: Option<String>,
    pane: Option<String>,
    status: Option<String>,
    caller: Option<String>,
}

#[derive(Deserialize)]
struct Send {
    pane: String,
    text: String,
    #[serde(default)]
    enter: bool,
    #[serde(default)]
    paste: bool,
    caller: Option<String>,
}

#[derive(Deserialize)]
struct Read {
    pane: String,
    lines: Option<usize>,
    #[serde(default)]
    scrollback: bool,
    caller: Option<String>,
}

#[derive(Deserialize)]
struct Action {
    action: String,
}

#[derive(Deserialize)]
struct PanelOp {
    op: String,
    id: Option<u64>,
}

#[derive(Deserialize)]
struct OpenFile {
    path: PathBuf,
    line: Option<u32>,
    pane: Option<String>,
    caller: Option<String>,
    /// Open to the Side: a new Viewer (R113.2).
    #[serde(default)]
    side: bool,
}

#[derive(Deserialize)]
struct Click {
    pane: String,
    text: String,
}

#[derive(Deserialize)]
struct ClipboardSet {
    /// (mime type, file with the data) pairs, offered together.
    items: Vec<(String, PathBuf)>,
}

#[derive(Deserialize)]
struct DropParams {
    pane: String,
    #[serde(default)]
    paths: Vec<PathBuf>,
    image: Option<PathBuf>,
}

#[derive(Deserialize)]
struct TreeOp {
    op: String,
    path: Option<PathBuf>,
}

#[derive(Deserialize)]
struct Screenshot {
    path: PathBuf,
}

fn handle(method: &str, p: Value) -> Result<Value> {
    let app = app();
    match method {
        // A second launch shows this window (`hand_off_to_running_instance`).
        "app.present" => {
            app.window.present();
            Ok(json!({}))
        }
        "source.add" => source_add(&app, params(p)?),
        "source.list" => source_list(&app, params(p)?),
        "workspace.add" => {
            let a: WorkspaceAdd = params(p)?;
            if !a.root.is_dir() {
                return Err(Error::usage(format!(
                    "not a directory: {}",
                    a.root.display()
                )));
            }
            // Already registered: that Workspace, not an error (R25.2).
            let existing = app
                .model
                .borrow()
                .workspaces
                .iter()
                .find(|w| w.root == a.root)
                .map(|w| (w.id.clone(), w.name.clone()));
            if let Some((id, name)) = existing {
                return Ok(
                    json!({"workspace": id, "name": name, "root": a.root, "moved": [], "existing": true}),
                );
            }
            let name = rade_core::workspace::default_name(&a.root, a.name.as_deref());
            let (id, moved) = app.update(|m| m.add_workspace(&a.root, &name))?;
            app.save_now();
            Ok(
                json!({"workspace": id, "name": name, "root": a.root, "moved": moved, "existing": false}),
            )
        }
        "workspace.list" => {
            let l: ListParams = params(p)?;
            Ok(workspace_list(&app, l.caller.as_deref()))
        }
        "workspace.switch" => {
            let r: WorkspaceRef = params(p)?;
            let id = app.model.borrow().find_workspace(&r.workspace)?.id.clone();
            app.switch_workspace(&id);
            let source = app.model.borrow().active_source.clone();
            Ok(json!({"workspace": id, "source": source}))
        }
        "workspace.remove" => {
            let r: WorkspaceRef = params(p)?;
            let (id, _general) = app.update(|m| m.remove_workspace(&r.workspace))?;
            app.save_now();
            Ok(json!({"workspace": id}))
        }
        "source.remove" => {
            let r: SourceRef = params(p)?;
            let removed = app.update(|m| m.remove_source(&r.source))?;
            app.save_now();
            Ok(json!({"source": removed.id}))
        }
        "source.rename" => {
            let r: SourceRename = params(p)?;
            let id = app.update(|m| m.rename_source(&r.source, &r.name))?;
            let name = app.model.borrow().source(&id).unwrap().name.clone();
            Ok(json!({"source": id, "name": name}))
        }
        "source.pause" => {
            let r: SourcePause = params(p)?;
            let id = app.pause_source(&r.source, r.force)?;
            let m = app.model.borrow();
            let panes = m.pane_infos().iter().filter(|i| i.source == id).count();
            Ok(json!({"source": id, "paused": true, "panes": panes}))
        }
        "source.resume" => {
            let r: SourceRef = params(p)?;
            let id = app.resume_source(&r.source)?;
            Ok(json!({"source": id, "paused": false}))
        }
        "pane.rename" => {
            let r: PaneRename = params(p)?;
            let id = app
                .model
                .borrow()
                .resolve_pane(&r.pane, r.caller.as_deref())?;
            app.update(|m| m.rename_pane(&id, r.name.as_deref()))?;
            let name = app.model.borrow().pane(&id).and_then(|x| x.name.clone());
            Ok(json!({"pane": id, "name": name}))
        }
        "tab.list" => {
            let l: ListParams = params(p)?;
            Ok(tab_list(&app, l.source)?)
        }
        "tab.rename" => {
            let r: TabRename = params(p)?;
            let id = app
                .model
                .borrow()
                .resolve_tab(&r.tab, r.caller.as_deref())?;
            app.update(|m| m.rename_tab(&id, &r.name))?;
            let label = app
                .model
                .borrow()
                .worktrees()
                .find_map(|(_, w)| w.tab(&id).map(|t| t.label()));
            Ok(json!({"tab": id, "label": label}))
        }
        "tab.close" => {
            let r: TabClose = params(p)?;
            let mode = match r.mode.as_deref().unwrap_or("only") {
                "only" => rade_core::basic::CloseMode::Only,
                "others" => rade_core::basic::CloseMode::Others,
                "right" => rade_core::basic::CloseMode::Right,
                o => {
                    return Err(Error::usage(format!(
                        "mode must be only|others|right, not {o}"
                    )))
                }
            };
            let id = app
                .model
                .borrow()
                .resolve_tab(&r.tab, r.caller.as_deref())?;
            let set = app.model.borrow().tab_close_set(&id, mode)?;
            let panes = app.close_tabs(&set.close, r.force)?;
            let kept: Vec<Value> = set
                .kept
                .iter()
                .map(|(t, f)| json!({"tab": t, "fanout": f}))
                .collect();
            Ok(json!({"closed": set.close, "panes": panes, "kept": kept}))
        }
        "pane.list" => {
            let l: ListParams = params(p)?;
            let m = app.model.borrow();
            let scope = l.scope(&m)?;
            let source = l
                .source
                .map(|q| m.find_source(&q).map(|p| p.id.clone()))
                .transpose()?;
            let panes: Vec<_> = m
                .pane_infos()
                .into_iter()
                .filter(|i| source.as_ref().is_none_or(|p| &i.source == p))
                // A named Source is listed wherever it is.
                .filter(|i| source.is_some() || scope.as_ref().is_none_or(|w| &i.workspace == w))
                .collect();
            Ok(json!({ "panes": panes }))
        }
        "pane.spawn" => spawn(&app, params(p)?),
        "pane.set_status" => {
            let s: SetStatus = params(p)?;
            let pane = target(&app, s.pane, s.caller)?;
            let status = s.status.as_deref().map(AgentStatus::parse).transpose()?;
            if status == Some(AgentStatus::Interrupted) {
                return Err(Error::usage(
                    "'interrupted' is set only by Session Restore (a turn cut off by a restart)",
                ));
            }
            let sid = s.session_id.filter(|x| !x.is_empty());
            app.update(|m| -> Result<()> {
                m.set_status(&pane, status, sid, s.agent, now())?;
                m.record_hook(&pane, status, now())?;
                match s.invocation {
                    Some(inv) => m.set_invocation(&pane, inv),
                    None => Ok(()),
                }
            })?;
            // A turn ended: read its account's usage now (R109.2).
            if matches!(status, Some(AgentStatus::Done | AgentStatus::Waiting)) {
                crate::accounts::turn_ended(&app.model.borrow(), &pane);
            }
            Ok(json!({"pane": pane}))
        }
        "notify.post" => {
            let n: Notify = params(p)?;
            let pane = target(&app, n.pane, n.caller)?;
            let status = n.status.as_deref().map(AgentStatus::parse).transpose()?;
            let id = app.update(|m| -> Result<Option<u64>> {
                if status.is_some() {
                    m.set_status(&pane, status, None, None, now())?;
                    m.record_hook(&pane, status, now())?;
                }
                // One entry with the status Notification (R3.1 coalescing).
                Ok(m.notify(&pane, n.title, &n.text, now()))
            })?;
            Ok(json!({"pane": pane, "notification": id}))
        }
        "pane.send" => send(&app, params(p)?),
        "pane.read" => read(&app, params(p)?),
        "pane.focus" => {
            let r: PaneParams = params(p)?;
            let id = app
                .model
                .borrow()
                .resolve_pane(&r.pane, r.caller.as_deref())?;
            app.update(|m| m.focus_pane(&id))?;
            Ok(json!({"pane": id}))
        }
        "pane.close" => {
            let r: Close = params(p)?;
            let id = app
                .model
                .borrow()
                .resolve_pane(&r.pane, r.caller.as_deref())?;
            if !r.force {
                app.model.borrow().check_close(&id)?;
            }
            app.close_pane(&id, r.force)?;
            Ok(json!({"pane": id}))
        }
        "viewer.open" => open_file(&app, params(p)?),
        "debug.click" => {
            let c: Click = params(p)?;
            let (id, t) = terminal(&app, &c.pane, None)?;
            let screen = t
                .handle
                .read_text(GHOSTTY_POINT_VIEWPORT)
                .unwrap_or_default();
            let cols = t.handle.health().columns.max(1) as usize;
            let (row, col) = crate::debug::find_cell(&screen, &c.text, cols)
                .ok_or_else(|| Error::not_found(format!("'{}' is not on {id}'s screen", c.text)))?;
            if !t.handle.ctrl_click_cell(row as u32, col as u32) {
                return Err(Error::internal(format!("pane {id} has not started")));
            }
            Ok(json!({"pane": id, "row": row, "col": col}))
        }
        "debug.review" => crate::debug::review(&app, params(p)?),
        "debug.tree" => {
            let o: TreeOp = params(p)?;
            app.files
                .debug(&o.op, o.path.as_deref())
                .map_err(Error::usage)
        }
        "debug.state" => Ok(serde_json::to_value(&*app.model.borrow()).expect("serializable")),
        "debug.screenshot" => {
            let s: Screenshot = params(p)?;
            crate::debug::screenshot(&app.window, &s.path)?;
            Ok(json!({"path": s.path}))
        }
        "debug.clipboard" => {
            let c: ClipboardSet = params(p)?;
            let mut providers = Vec::new();
            for (mime, path) in &c.items {
                let data = std::fs::read(path)
                    .map_err(|e| Error::usage(format!("{}: {e}", path.display())))?;
                providers.push(gtk::gdk::ContentProvider::for_bytes(
                    mime,
                    &glib::Bytes::from_owned(data),
                ));
            }
            let display = gtk::gdk::Display::default().expect("display");
            display
                .clipboard()
                .set_content(Some(&gtk::gdk::ContentProvider::new_union(&providers)))
                .map_err(|e| Error::internal(e.to_string()))?;
            Ok(json!({}))
        }
        "debug.drop" => {
            // The same handler a real drop runs (R5.2).
            let d: DropParams = params(p)?;
            let (id, t) = terminal(&app, &d.pane, None)?;
            let value = match &d.image {
                Some(img) => gtk::gdk::Texture::from_filename(img)
                    .map_err(|e| Error::usage(e.to_string()))?
                    .to_value(),
                None => {
                    let files: Vec<_> = d.paths.iter().map(gio::File::for_path).collect();
                    gtk::gdk::FileList::from_array(&files).to_value()
                }
            };
            if !t.handle.drop_value(&value) {
                return Err(Error::internal(format!("pane {id}: nothing dropped")));
            }
            Ok(json!({"pane": id}))
        }
        "debug.action" if p.get("action").and_then(Value::as_str) == Some("paste") => {
            // Ghostty's paste binding on the focused Terminal: the exact path
            // of Ctrl+Shift+V (R5.1).
            let focused = app.model.borrow().active_tab().map(|t| t.focused.clone());
            let focused = focused.ok_or_else(|| Error::not_found("no focused pane"))?;
            let (id, t) = terminal(&app, &focused, None)?;
            t.handle.perform_binding_action("paste_from_clipboard");
            Ok(json!({"pane": id}))
        }
        "debug.action" => {
            let a: Action = params(p)?;
            // Window actions for the live harness (tests/live): no keybinding.
            let window_op = match a.action.as_str() {
                "present" => Some(gtk::Window::present as fn(&gtk::Window)),
                "fullscreen" => Some(gtk::Window::fullscreen as fn(&gtk::Window)),
                "unfullscreen" => Some(gtk::Window::unfullscreen as fn(&gtk::Window)),
                _ => None,
            };
            if let Some(op) = window_op {
                op(app.window.upcast_ref());
                return Ok(json!({}));
            }
            // R20.8 checks: switch light / dark live, as the system would.
            if let Some(scheme) = match a.action.as_str() {
                "color_scheme_light" => Some(adw::ColorScheme::ForceLight),
                "color_scheme_dark" => Some(adw::ColorScheme::ForceDark),
                _ => None,
            } {
                adw::StyleManager::default().set_color_scheme(scheme);
                return Ok(json!({}));
            }
            if a.action == "dismiss_banner" {
                crate::status::dismiss_banner();
                return Ok(json!({}));
            }
            if !rade_core::config::DEFAULT_KEYBINDINGS
                .iter()
                .chain(rade_core::palette::ACTIONS)
                .any(|(n, _)| *n == a.action)
            {
                return Err(Error::usage(format!("unknown action '{}'", a.action)));
            }
            crate::keys::run_action(&a.action);
            Ok(json!({}))
        }
        "debug.tabs" => crate::titles::debug(&p),
        "debug.ui" => crate::debug::ui(&app, &p),
        "debug.scm" => crate::changes::debug(&p),
        "debug.diff" => crate::diff_pane::debug(&p),
        "debug.find" => crate::search_view::debug(&p),
        "debug.menu" => crate::menus::debug_menu(&p),
        "debug.dialog" => crate::menus::debug_dialog(&p),
        "debug.rename" => crate::titles::debug_rename(&p),
        "debug.search" => crate::menus::debug_search(&p),
        "debug.panel" => {
            let o: PanelOp = params(p)?;
            crate::notifications::panel_op(&o.op, o.id)
        }
        "debug.desktop" => Ok(app.desktop.log()),
        "debug.window" => crate::debug::window_info(&app, params(p)?),
        "debug.quit" => {
            // Reply first, then quit.
            glib::idle_add_local_once(|| crate::app::app().quit());
            Ok(json!({}))
        }
        _ => Err(Error::usage(format!("unknown method '{method}'"))),
    }
}

/// `viewer.open` (`rade open`, R7): show a file in the Viewer of the Tab of
/// `--pane` (or the caller, else the Pane on screen), without moving focus.
fn open_file(app: &App, o: OpenFile) -> Result<Value> {
    if !o.path.is_absolute() {
        return Err(Error::usage(format!(
            "path must be absolute: {}",
            o.path.display()
        )));
    }
    if !o.path.exists() {
        return Err(Error::not_found(format!(
            "no such file: {}",
            o.path.display()
        )));
    }
    if o.path.is_dir() {
        return Err(Error::usage(format!("{} is a folder", o.path.display())));
    }
    let from = match o.pane {
        Some(q) => Some(app.model.borrow().resolve_pane(&q, o.caller.as_deref())?),
        None => o.caller.filter(|c| app.model.borrow().locate(c).is_some()),
    };
    let id = app.update(|m| match from {
        Some(from) if o.side => m.open_viewer_side(&from, &o.path, o.line, false),
        Some(from) => m.open_viewer(&from, &o.path, o.line, false),
        None => {
            let (p, w) = m.active();
            let (p, w) = (p.id.clone(), w.id.clone());
            m.open_viewer_at(&p, &w, &o.path, o.line, false, o.side)
        }
    })?;
    let m = app.model.borrow();
    let loc = m.locate(&id).unwrap();
    Ok(json!({"pane": id, "tab": loc.tab, "path": o.path, "line": o.line}))
}

fn source_add(app: &App, a: SourceAdd) -> Result<Value> {
    if !a.path.is_dir() {
        return Err(Error::usage(format!(
            "not a directory: {}",
            a.path.display()
        )));
    }
    // Idempotent (R24.2): an existing Source is returned as it is.
    let existing = app
        .model
        .borrow()
        .sources
        .iter()
        .find(|s| !s.is_general() && s.path == a.path)
        .map(|s| json!({"source": s.id, "name": s.name, "path": s.path, "workspace": s.workspace, "existing": true}));
    if let Some(v) = existing {
        return Ok(v);
    }
    let ws = app.model.borrow().workspace_for_new_source(
        &a.path,
        a.workspace.as_deref(),
        a.caller.as_deref(),
    )?;
    let id = app.update(|m| m.add_source_in(&a.path, a.name.as_deref(), Some(&ws)))?;
    // Worktrees made outside RADE show up too (R12).
    let pid = id.clone();
    std::thread::spawn(move || crate::worktrees::import(&pid));
    let m = app.model.borrow();
    let p = m.source(&id).unwrap();
    Ok(
        json!({"source": p.id, "name": p.name, "path": p.path, "workspace": p.workspace, "existing": false}),
    )
}

/// `workspace.list` (R23): every Workspace with what the switcher shows,
/// plus its env and clone owner for `rade source add owner/repo`.
fn workspace_list(app: &App, caller: Option<&str>) -> Value {
    let m = app.model.borrow();
    let home = rade_core::paths::home_dir(&rade_core::paths::process_env);
    let shown = m.active_workspace().to_string();
    let current = caller
        .and_then(|c| m.locate(c))
        .and_then(|l| m.source(&l.source))
        .map_or(shown.clone(), |s| s.workspace.clone());
    let rows: Vec<Value> = m
        .workspace_looks()
        .into_iter()
        .zip(m.workspaces.iter())
        .map(|(l, w)| {
            let sum = m.workspace_summary(&w.id);
            let file = rade_core::workspace::WorkspaceFile::read(&w.root);
            let mut row = json!({
                "workspace": w.id, "name": l.name, "root": w.root, "color": l.color,
                "key": l.key, "active": w.id == shown, "current": w.id == current,
                "sources": sum.sources, "unread": sum.unread, "waiting": sum.waiting,
                "status": sum.status.map(|s| s.as_str()),
                "env": file.as_ref().map(|f| f.env(&home).into_iter().collect::<std::collections::BTreeMap<_, _>>()).unwrap_or_default(),
                "default_owner": file.as_ref().ok().and_then(|f| f.clone.default_owner.clone()),
                "identity": file.as_ref().ok().map(|f| f.identity.clone()).unwrap_or_default(),
            });
            if let Err(e) = file {
                row["error"] = e.into();
            }
            row
        })
        .collect();
    json!({ "workspaces": rows })
}

/// `tab.list` (R19.1): every Tab (of `source`), in sidebar order.
fn tab_list(app: &App, source: Option<String>) -> Result<Value> {
    let m = app.model.borrow();
    let source = source
        .map(|q| m.find_source(&q).map(|p| p.id.clone()))
        .transpose()?;
    let tabs: Vec<Value> = m
        .worktrees()
        .filter(|(p, _)| source.as_ref().is_none_or(|q| &p.id == q))
        .flat_map(|(p, w)| {
            let m = &m;
            w.tabs.iter().map(move |t| {
                json!({
                    "tab": t.id, "name": t.name, "label": t.label(), "renamed": t.renamed,
                    "source": p.id, "worktree": w.name, "panes": t.pane_ids(),
                    "active": w.active_tab.as_deref() == Some(t.id.as_str()),
                    "fanout": m.fanout_of_tab(&t.id).map(|f| f.id.clone()),
                })
            })
        })
        .collect();
    Ok(json!({ "tabs": tabs }))
}

fn source_list(app: &App, l: ListParams) -> Result<Value> {
    let m = app.model.borrow();
    let scope = l.scope(&m)?;
    let sources: Vec<Value> = m
        .sources
        .iter()
        .filter(|p| scope.as_ref().is_none_or(|w| &p.workspace == w))
        .map(|p| {
            json!({
                "source": p.id,
                "name": p.name,
                "workspace": p.workspace,
                "path": p.path,
                "paused": p.paused,
                "branch": git::branch(&p.path),
                "worktrees": p.worktrees.iter().map(|w| json!({"worktree": w.name, "path": w.path, "branch": git::branch(&w.path), "tabs": w.tabs.len()})).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({ "sources": sources }))
}

/// `--pane`, else the caller (`RADE_PANE_ID`).
fn target(app: &App, pane: Option<String>, caller: Option<String>) -> Result<String> {
    match pane {
        Some(q) => app.model.borrow().resolve_pane(&q, caller.as_deref()),
        None => caller
            .filter(|c| app.model.borrow().locate(c).is_some())
            .ok_or_else(|| Error::usage("no pane: pass --pane or run inside a RADE Terminal")),
    }
}

/// An Agent Preset by name (R16), or `USAGE` listing the known ones.
/// Agent Preset `name` as configured for Source `source` (R29.2: its
/// Workspace, Project and trusted `.rade.toml` layers), else app-wide.
pub(crate) fn preset(
    app: &App,
    name: &str,
    source: Option<(&str, Option<&str>)>,
) -> Result<rade_core::preset::Preset> {
    let presets = match source {
        Some((s, w)) => app.config_for(s, w),
        None => app.config.borrow().clone(),
    }
    .presets()
    .map_err(Error::usage)?;
    presets.get(name).cloned().ok_or_else(|| {
        let names: Vec<&str> = presets.keys().map(String::as_str).collect();
        Error::usage(format!(
            "unknown agent '{name}' (Presets: {})",
            names.join(", ")
        ))
    })
}

/// The Source (and Worktree) a spawn lands in, for its config layers: its
/// `--source` / `--worktree`, else the caller's, else the one on screen.
fn spawn_target(app: &App, s: &Spawn) -> Option<(String, Option<String>)> {
    let m = app.model.borrow();
    if let Some(q) = &s.source {
        let src = m.find_source(q).ok()?.id.clone();
        let wt = s
            .worktree
            .as_deref()
            .and_then(|w| m.find_worktree(&src, Some(w)).ok())
            .map(|x| x.1);
        return Some((src, wt));
    }
    match s.caller.as_deref().and_then(|c| m.locate(c)) {
        Some(l) => Some((l.source, Some(l.worktree))),
        None => {
            let (p, w) = m.active();
            Some((p.id.clone(), Some(w.id.clone())))
        }
    }
}

/// `pane.spawn` (API.md): resolve the Agent Preset into a command (R16),
/// then place and limit it in the model (R2.3–R2.5).
pub(crate) fn spawn(app: &App, s: Spawn) -> Result<Value> {
    let split = match s.split.as_deref() {
        None => None,
        Some("right") => Some(SplitDir::Right),
        Some("down") => Some(SplitDir::Down),
        Some(o) => return Err(Error::usage(format!("--split must be right|down, not {o}"))),
    };
    let tab = match s.tab.as_deref() {
        None => None,
        Some("new") => Some(TabChoice::New),
        Some("current") => Some(TabChoice::Current),
        Some(o) => return Err(Error::usage(format!("--tab must be new|current, not {o}"))),
    };
    let at = spawn_target(app, &s);
    let plan = plan_spawn(app, &s, at.as_ref())?;
    let (command, session_id, typed) = (plan.command, plan.session_id, plan.typed);
    let spec = SpawnSpec {
        source: s.source,
        worktree: s.worktree,
        caller: s.caller,
        tab,
        split,
        command,
        name: s.name.clone(),
        agent: s.agent.clone(),
        session_id,
        status: (s.agent.is_some() && s.prompt.is_some()).then_some(AgentStatus::Running),
        now: now(),
        account: plan.account,
    };
    let limits = app.config.borrow().limits.clone();
    let pane = app.update(|m| m.spawn(spec, &limits))?;
    if let (Some(_), Some(prompt)) = (&s.agent, &s.prompt) {
        // The Review header's task (R118.1).
        app.update(|m| m.set_task(&pane, prompt))?;
    }
    if let Some(text) = typed {
        let setup = app
            .model
            .borrow()
            .pane(&pane)
            .is_some_and(|p| p.setup.is_some() && p.command().is_some());
        type_when_settled(pane.clone(), text, setup);
    }
    let m = app.model.borrow();
    let loc = m.locate(&pane).unwrap();
    let wt = m.worktree(&loc.source, &loc.worktree).unwrap();
    Ok(json!({
        "pane": pane, "name": s.name, "tab": loc.tab, "worktree": wt.name,
        "source": loc.source, "parent": m.pane(&pane).unwrap().parent,
        "agent": s.agent, "account": m.pane(&pane).unwrap().account,
    }))
}

/// What a spawn will run, decided before anything is placed or created.
pub(crate) struct SpawnPlan {
    pub command: Option<String>,
    pub session_id: Option<String>,
    /// Typed into the program once it settles (prompt mode `type`, `--cmd`).
    pub typed: Option<String>,
    pub account: Option<rade_core::account::Account>,
}

/// Resolve `--agent` / `--cmd` / `--account` of a spawn landing in `at`
/// (R16, R108): the Preset's command, or `--cmd` instead of it (the
/// Preset still gives the account and config layers); the account from
/// `--account`, the caller Pane's, else the Workspace default, locked to
/// the Workspace of the Pane being created (R108.3); the command actually
/// run checked against the account's config dir (R108.7).
pub(crate) fn plan_spawn(
    app: &App,
    s: &Spawn,
    at: Option<&(String, Option<String>)>,
) -> Result<SpawnPlan> {
    use rade_core::account::{check_command, resolve_spawn, AgentKind, SpawnAsk};
    let preset = s
        .agent
        .as_deref()
        .map(|name| preset(app, name, at.map(|(a, b)| (a.as_str(), b.as_deref()))))
        .transpose()?;
    let (command, session_id, typed) = match (&preset, &s.cmd) {
        // `--agent` with `--cmd`: an Agent Pane of that Preset running `--cmd`.
        (_, Some(cmd)) => (Some(cmd.clone()), None, s.prompt.clone()),
        (Some(p), None) => {
            let sid = p.session_id_flag.as_ref().map(|_| new_uuid());
            let typed = (p.prompt == PromptMode::Type)
                .then(|| s.prompt.clone())
                .flatten();
            (
                Some(p.launch(s.prompt.as_deref(), sid.as_deref())),
                sid,
                typed,
            )
        }
        // A shell: the prompt is typed into it.
        (None, None) => (None, None, s.prompt.clone()),
    };
    let home = rade_core::paths::home_dir(&rade_core::paths::process_env);
    let account = {
        let m = app.model.borrow();
        let workspace = at
            .and_then(|(src, _)| m.source(src))
            .map(|x| x.workspace.clone())
            .unwrap_or_else(|| m.active_workspace().to_string());
        let parent = s
            .caller
            .as_deref()
            .and_then(|c| m.pane(c))
            .and_then(|p| p.account.clone());
        let ask = SpawnAsk {
            workspace: &workspace,
            agent: preset.as_ref().and_then(|p| AgentKind::of_hooks(p.hooks)),
            explicit: s.account.as_deref(),
            parent: parent.as_deref(),
        };
        resolve_spawn(&ask, &m.account_lists(&home))?
    };
    if let (Some(a), Some(cmd), Some(name)) = (&account, &command, &s.agent) {
        check_command(name, cmd, a, &home)?;
    }
    Ok(SpawnPlan {
        command,
        session_id,
        typed,
        account,
    })
}

/// Prompt mode `type` (R16.1): once the new Terminal shows output that has
/// not changed for 0.75 s (or after 15 s), paste the prompt and press Enter.
/// With a Worktree `setup` running first (R12.2), only after it has ended.
pub(crate) fn type_when_settled(pane: String, text: String, mut setup: bool) {
    let mut last = String::new();
    let (mut stable, mut ticks) = (0, 0);
    glib::timeout_add_local(Duration::from_millis(250), move || {
        let Some(app) = crate::app::try_app() else {
            return glib::ControlFlow::Break;
        };
        let terms = app.terminals.borrow();
        let Some(t) = terms.get(&pane) else {
            return glib::ControlFlow::Break; // Pane closed
        };
        t.handle.ensure_started();
        if setup {
            let all = t.handle.read_text(GHOSTTY_POINT_SCREEN).unwrap_or_default();
            if !all.contains(rade_core::worktree::SETUP_DONE) && !all.contains("rade setup failed")
            {
                return glib::ControlFlow::Continue;
            }
            setup = false;
        }
        let screen = t
            .handle
            .read_text(GHOSTTY_POINT_VIEWPORT)
            .unwrap_or_default();
        ticks += 1;
        if !screen.trim().is_empty() && screen == last {
            stable += 1;
        } else {
            stable = 0;
            last = screen;
        }
        if stable < 3 && ticks < 60 {
            return glib::ControlFlow::Continue;
        }
        t.handle.send_text(&text);
        press_enter_later(t.handle.clone());
        glib::ControlFlow::Break
    });
}

/// `pane.wait` (R10.4), on the socket thread: poll the model (and screen
/// text when needed) every 200 ms until the condition holds or time runs out.
fn wait(w: Wait) -> Result<Value> {
    let until = WaitUntil::parse(&w.until)?;
    let deadline = Instant::now() + Duration::from_secs(w.timeout);
    let (pane, caller) = (w.pane.clone(), w.caller.clone());
    let (id, idle_secs) = on_main(move || {
        let app = app();
        let id = app.model.borrow().resolve_pane(&pane, caller.as_deref())?;
        let idle = app.config.borrow().idle_seconds;
        Ok((id, idle))
    })?;
    let point = if until.needs_text() {
        Some(GHOSTTY_POINT_SCREEN)
    } else if matches!(until, WaitUntil::Idle) {
        Some(GHOSTTY_POINT_VIEWPORT)
    } else {
        None
    };
    let mut last: Option<String> = None;
    let mut quiet_since = Instant::now();
    loop {
        let pid = id.clone();
        let snap = on_main(move || {
            let app = app();
            let status = app.model.borrow().pane(&pid).map(|p| p.status);
            let text = match (point, app.terminals.borrow().get(&pid)) {
                // Same text `read` returns (no trailing cell padding).
                (Some(pt), Some(t)) => tail(&t.handle.read_text(pt).unwrap_or_default(), None),
                _ => String::new(),
            };
            Ok(status.map(|s| (s, text)))
        })?;
        let Some((status, text)) = snap else {
            // The Pane closed: it can only have exited.
            return match until {
                WaitUntil::Status(AgentStatus::Exited) => {
                    Ok(json!({"status": "exited", "matched": "exited"}))
                }
                _ => Err(Error::not_found(format!("pane {id} closed while waiting"))),
            };
        };
        if last.as_ref() != Some(&text) {
            last = Some(text.clone());
            quiet_since = Instant::now();
        }
        let idle_after = Duration::from_secs(idle_secs);
        if let Some(m) = until.check(status, &text, quiet_since.elapsed(), idle_after) {
            return Ok(json!({"status": status.map(|s| s.as_str()), "matched": m}));
        }
        if Instant::now() >= deadline {
            return Err(Error::new(
                ErrorKind::Timeout,
                format!(
                    "timed out after {}s waiting for {id} --until {} (status: {})",
                    w.timeout,
                    w.until,
                    status.map_or("none", |s| s.as_str())
                ),
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub(crate) fn terminal<'a>(
    app: &'a App,
    query: &str,
    caller: Option<&str>,
) -> Result<(String, std::cell::Ref<'a, crate::terminal::TerminalWidget>)> {
    let id = app.model.borrow().resolve_pane(query, caller)?;
    if let Some(loc) = app.model.borrow().locate(&id) {
        // Nothing runs in a paused Source (R19.3).
        let m = app.model.borrow();
        if m.is_paused(&loc.source) {
            return Err(Error::usage(format!(
                "pane {id} is in paused source '{0}': resume it first (`rade source resume {0}`)",
                loc.source
            )));
        }
    }
    let terms = app.terminals.borrow();
    if !terms.contains_key(&id) {
        return Err(Error::internal(format!("pane {id} has no terminal")));
    }
    Ok((id.clone(), std::cell::Ref::map(terms, |t| &t[&id])))
}

/// Ghostty's `text:` binding action writes raw bytes to the PTY (no paste
/// semantics); its argument uses Zig string-literal escapes, so only `\`
/// needs escaping.
pub fn text_action(text: &str) -> String {
    format!("text:{}", text.replace('\\', "\\\\"))
}

fn send(app: &App, s: Send) -> Result<Value> {
    let id = app
        .model
        .borrow()
        .resolve_pane(&s.pane, s.caller.as_deref())?;
    send_text(app, &id, &s.text, s.paste, s.enter)?;
    Ok(json!({}))
}

/// Write text to a Pane's input (`rade send`): `paste` = bracketed paste,
/// `enter` = submit it, which starts a new Agent turn (R10.1).
pub(crate) fn send_text(app: &App, id: &str, text: &str, paste: bool, enter: bool) -> Result<()> {
    let (id, t) = terminal(app, id, None)?;
    t.handle.ensure_started();
    let started = if paste {
        t.handle.send_text(text)
    } else {
        t.handle.perform_binding_action(&text_action(text))
    };
    if !started {
        return Err(Error::internal(format!("pane {id} has not started yet")));
    }
    if enter {
        press_enter_later(t.handle.clone());
        drop(t);
        app.update(|m| {
            if m.pane(&id)
                .is_some_and(|p| p.agent.is_some() || p.status.is_some())
            {
                let _ = m.set_status(&id, Some(AgentStatus::Running), None, None, now());
            }
        });
    }
    Ok(())
}

/// Enter shortly after the text: TUIs like Codex treat an Enter arriving in
/// the same burst as the text as part of a paste (a newline), not a submit.
fn press_enter_later(handle: crate::terminal::TerminalHandle) {
    glib::timeout_add_local_once(Duration::from_millis(ENTER_DELAY_MS), move || {
        handle.perform_binding_action(&text_action("\r"));
    });
}

/// ponytail: fixed delay; raise it if an Agent still reads Enter as a newline.
const ENTER_DELAY_MS: u64 = 150;

/// Last `n` lines of `text`, ignoring trailing blank rows.
pub fn tail(text: &str, n: Option<usize>) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let end = lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map_or(0, |i| i + 1);
    let start = n.map_or(0, |n| end.saturating_sub(n));
    let mut out = lines[start..end].join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

fn read(app: &App, r: Read) -> Result<Value> {
    let (id, t) = terminal(app, &r.pane, r.caller.as_deref())?;
    let whole = r.scrollback || r.lines.is_some();
    let point = if whole {
        GHOSTTY_POINT_SCREEN
    } else {
        GHOSTTY_POINT_VIEWPORT
    };
    let text = t
        .handle
        .read_text(point)
        .ok_or_else(|| Error::internal(format!("pane {id}: read failed")))?;
    let cursor = t
        .handle
        .cursor()
        .map(|(row, col)| json!({"row": row, "col": col}));
    Ok(json!({"text": tail(&text, r.lines), "cursor": cursor}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_action_escapes_backslash_only() {
        assert_eq!(text_action("pwd"), "text:pwd");
        assert_eq!(text_action("a\\nb"), "text:a\\\\nb");
        assert_eq!(text_action("\r"), "text:\r");
        assert_eq!(text_action("Tiếng Việt: \"x\""), "text:Tiếng Việt: \"x\"");
    }

    #[test]
    fn tail_trims_blank_rows_and_limits() {
        assert_eq!(tail("a\nb  \nc\n\n\n", None), "a\nb\nc\n");
        assert_eq!(tail("a\nb\nc\n\n", Some(2)), "b\nc\n");
        assert_eq!(tail("\n\n", Some(3)), "");
    }
}
