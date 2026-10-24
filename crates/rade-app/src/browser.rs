//! `browser.*` socket methods (docs/API.md, PRD R18): open a Browser Pane
//! and drive it the way coding agents drive Playwright — a text snapshot
//! with element refs, then click / fill / press by ref, eval, screenshot,
//! console, wait. Runs on the socket thread: each step hops to the GTK
//! main loop, and the WebView answers asynchronously, so waits never block
//! the UI. Also `open_browser` (Ctrl+Shift+O, palette) and port listing.

use crate::app::{app, App};
use crate::browser_view::BrowserView;
use crate::handlers::params;
use gtk::glib;
use gtk4 as gtk;
use rade_core::browser::{eval_bodies, normalize_url, parse_key, WaitFor, BLANK};
use rade_core::rpc::{Error, ErrorKind, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use webkit6::prelude::*;

/// Longest a single WebView call may take.
const CALL_TIMEOUT: Duration = Duration::from_secs(15);
/// `open` / `wait` default timeout (seconds).
const LOAD_TIMEOUT: u64 = 30;

#[derive(Deserialize, Default)]
#[serde(default)]
struct Sel {
    pane: Option<String>,
    caller: Option<String>,
}

#[derive(Deserialize)]
struct Open {
    url: String,
    #[serde(flatten)]
    sel: Sel,
}

#[derive(Deserialize)]
struct Snapshot {
    #[serde(default)]
    interactive: bool,
    #[serde(flatten)]
    sel: Sel,
}

#[derive(Deserialize)]
struct RefArg {
    r#ref: String,
    text: Option<String>,
    #[serde(flatten)]
    sel: Sel,
}

#[derive(Deserialize)]
struct Press {
    key: String,
    #[serde(flatten)]
    sel: Sel,
}

#[derive(Deserialize)]
struct Eval {
    js: String,
    #[serde(flatten)]
    sel: Sel,
}

#[derive(Deserialize)]
struct Shot {
    path: PathBuf,
    #[serde(default)]
    full: bool,
    #[serde(flatten)]
    sel: Sel,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Annotations {
    /// Send them (and clear them) instead of only listing them.
    send: bool,
    /// The Agent Pane to send to / the Preset to spawn when there is none.
    to: Option<String>,
    agent: Option<String>,
    #[serde(flatten)]
    sel: Sel,
}

#[derive(Deserialize)]
struct Wait {
    #[serde(rename = "for")]
    what: WaitFor,
    #[serde(default = "default_timeout")]
    timeout: u64,
    #[serde(flatten)]
    sel: Sel,
}

fn default_timeout() -> u64 {
    LOAD_TIMEOUT
}

pub fn dispatch(method: &str, p: Value) -> Result<Value> {
    match method {
        "browser.open" => open(params(p)?),
        "browser.snapshot" => snapshot(params(p)?),
        "browser.click" => act(params(p)?, "click"),
        "browser.fill" => act(params(p)?, "fill"),
        "browser.press" => press(params(p)?),
        "browser.eval" => eval(params(p)?),
        "browser.screenshot" => screenshot(params(p)?),
        "browser.console" => {
            let s: Sel = params(p)?;
            on_view(s, |v, reply| {
                let (messages, dropped) = v.take_console();
                reply(Ok(
                    json!({"pane": v.id, "messages": messages, "dropped": dropped}),
                ))
            })
        }
        "browser.back" | "browser.forward" | "browser.reload" => {
            let s: Sel = params(p)?;
            let op = method["browser.".len()..].to_string();
            let (id, before) = on_view(s, move |v, reply| {
                let n = v.loads.get();
                match op.as_str() {
                    "back" if !v.web.can_go_back() => {
                        return reply(Err(Error::usage("no page to go back to")))
                    }
                    "forward" if !v.web.can_go_forward() => {
                        return reply(Err(Error::usage("no page to go forward to")))
                    }
                    "back" => v.web.go_back(),
                    "forward" => v.web.go_forward(),
                    _ => v.web.reload(),
                }
                reply(Ok((v.id.clone(), n)))
            })?;
            settle(&id, Some(before), LOAD_TIMEOUT)
        }
        "browser.wait" => wait(params(p)?),
        "browser.annotations" => annotations(params(p)?),
        "browser.annotate" => {
            let on = p["on"].as_bool().unwrap_or(true);
            let s: Sel = params(p)?;
            on_view(s, move |v, reply| {
                v.annotate.set_active(on);
                reply(Ok(json!({"pane": v.id, "on": v.annotating()})))
            })
        }
        "browser.close" => Err(Error::usage(
            "close is for --engine chromium; a Browser Pane closes with `rade close <pane>`",
        )),
        "debug.browser" => debug(p),
        "browser.ports" => Ok(json!({ "ports": ports_json(&ports()?) })),
        _ => Err(Error::usage(format!("unknown method '{method}'"))),
    }
}

/// Run `f` on the main loop with the selected Browser; it answers through
/// `reply` (possibly later, from a WebView callback). Waits on this thread.
fn on_view<T: Send + 'static>(
    sel: Sel,
    f: impl FnOnce(Rc<BrowserView>, Box<dyn FnOnce(Result<T>)>) + Send + 'static,
) -> Result<T> {
    let (tx, rx) = mpsc::channel::<Result<T>>();
    glib::MainContext::default().invoke(move || {
        let app = app();
        let view = app
            .model
            .borrow()
            .find_browser(sel.pane.as_deref(), sel.caller.as_deref())
            .and_then(|id| {
                app.browsers
                    .borrow()
                    .get(&id)
                    .cloned()
                    .ok_or_else(|| Error::internal(format!("Browser {id} has no view")))
            });
        match view {
            Ok(v) => f(v, Box::new(move |r| drop(tx.send(r)))),
            Err(e) => drop(tx.send(Err(e))),
        }
    });
    rx.recv_timeout(CALL_TIMEOUT)
        .unwrap_or_else(|_| Err(Error::new(ErrorKind::Timeout, "the Browser did not answer")))
}

/// A JS exception from the automation library → an RPC error.
fn js_error(e: String) -> Error {
    let e = e.trim_start_matches("Error: ").to_string();
    if e.contains("unknown ref") {
        Error::not_found(e)
    } else {
        Error::usage(e)
    }
}

/// `browser.open`: show `url` in the caller's Tab's Browser (or the one
/// `--pane` names), else a new one split right of the caller (no caller:
/// beside the focused Pane on screen). Focus does not move. Waits for the
/// load to finish.
fn open(o: Open) -> Result<Value> {
    let url = normalize_url(&o.url)?;
    if crate::browser_view::unsupported_embedded_url(&url) {
        return Err(Error::usage(format!(
            "{} Use `rade browser --engine chromium open <url>`.",
            crate::browser_view::UNSUPPORTED_MESSAGE
        )));
    }
    let u = url.clone();
    let (id, before) = crate::handlers::on_main(move || {
        let app = app();
        let caller = o
            .sel
            .caller
            .filter(|c| app.model.borrow().locate(c).is_some());
        let target = match &o.sel.pane {
            Some(_) => Some(
                app.model
                    .borrow()
                    .find_browser(o.sel.pane.as_deref(), caller.as_deref())?,
            ),
            None => None,
        };
        let existing = |app: &App, id: &str| app.browsers.borrow().get(id).cloned();
        let (id, created) = app.update(|m| match (&target, &caller) {
            (Some(b), _) => m.set_browser_url(b, &u).map(|_| (b.clone(), false)),
            (None, Some(c)) => m.open_browser(c, &u, false),
            (None, None) => {
                let (p, w) = m.active();
                let (p, w) = (p.id.clone(), w.id.clone());
                m.open_browser_in(&p, &w, &u, false)
            }
        })?;
        let v = existing(&app, &id).ok_or_else(|| Error::internal("no Browser view"))?;
        // A new view starts loading its URL itself; an old one navigates.
        let before = if created { 0 } else { v.loads.get() };
        if !created {
            v.load(&u);
        }
        Ok((id, before))
    })?;
    let mut r = settle(&id, Some(before), LOAD_TIMEOUT)?;
    r["url"] = json!(r["url"].as_str().unwrap_or(&url));
    Ok(r)
}

/// Wait until the Browser `id` finished a load after `before` loads — or,
/// after 500 ms, is simply not loading (a same-document move: `#hash`,
/// `pushState`, never finishes a load) — or with `before` None until it is
/// not loading; at most `timeout` s. Then report where it is.
fn settle(id: &str, before: Option<u64>, timeout: u64) -> Result<Value> {
    let start = Instant::now();
    let deadline = start + Duration::from_secs(timeout);
    let grace = start + Duration::from_millis(500);
    loop {
        let sel = Sel {
            pane: Some(id.to_string()),
            caller: None,
        };
        let late = Instant::now() > grace;
        let (done, info) = on_view(sel, move |v, reply| {
            let done = match before {
                Some(n) => v.loads.get() > n || (late && !v.web.is_loading()),
                None => !v.web.is_loading(),
            };
            let info = json!({
                "pane": v.id, "url": v.preview_url(),
                "title": v.web.title().map(|u| u.to_string()),
                "loading": v.web.is_loading(), "error": *v.load_error.borrow(),
            });
            reply(Ok((done, info)))
        })?;
        if done {
            return Ok(info);
        }
        if Instant::now() > deadline {
            return Err(Error::new(
                ErrorKind::Timeout,
                format!("{id} is still loading {}", info["url"]),
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// After a click or key press: let a navigation it started begin, then
/// wait for it (so a following snapshot sees the new page).
fn after_action(id: &str, before: u64, mut r: Value) -> Result<Value> {
    std::thread::sleep(Duration::from_millis(150));
    let s = settle(id, None, LOAD_TIMEOUT)?;
    let sel = Sel {
        pane: Some(id.into()),
        caller: None,
    };
    let navigated = on_view(sel, move |v, reply| reply(Ok(v.loads.get() > before)))?;
    r["navigated"] = json!(navigated);
    r["url"] = s["url"].clone();
    Ok(r)
}

fn snapshot(s: Snapshot) -> Result<Value> {
    let interactive = s.interactive;
    let page: Value = on_view(s.sel, move |v, reply| {
        let id = v.id.clone();
        v.automation("snapshot", &[], move |r| {
            reply(r.map_err(js_error).map(|mut p| {
                p["pane"] = json!(id);
                p
            }))
        })
    })?;
    let mut r = rade_core::browser::snapshot_result(&page, interactive)?;
    r["pane"] = page["pane"].clone();
    Ok(r)
}

/// An action starts: JS dialogs it raises in the next 10 s are answered.
fn automating(v: &BrowserView) {
    v.automating_until
        .set(Some(Instant::now() + Duration::from_secs(10)));
}

/// `click <ref>` / `fill <ref> <text>`.
fn act(a: RefArg, f: &'static str) -> Result<Value> {
    let mut args = vec![json!(a.r#ref)];
    if f == "fill" {
        args.push(json!(a
            .text
            .ok_or_else(|| Error::usage("fill needs text"))?));
    }
    let r#ref = a.r#ref.clone();
    let (id, before, el) = on_view(a.sel, move |v, reply| {
        automating(&v);
        let (id, before) = (v.id.clone(), v.loads.get());
        v.automation(f, &args, move |r| {
            reply(r.map_err(js_error).map(|el| (id, before, el)))
        })
    })?;
    after_action(
        &id,
        before,
        json!({"pane": id, "ref": r#ref, "element": el}),
    )
}

fn press(p: Press) -> Result<Value> {
    let key = serde_json::to_value(parse_key(&p.key)?).expect("serializable");
    let (id, before, el) = on_view(p.sel, move |v, reply| {
        automating(&v);
        let (id, before) = (v.id.clone(), v.loads.get());
        v.automation("press", &[key], move |r| {
            reply(r.map_err(js_error).map(|el| (id, before, el)))
        })
    })?;
    after_action(
        &id,
        before,
        json!({"pane": id, "key": p.key, "focused": el}),
    )
}

/// `eval`: run JavaScript in the page's own world (it sees the page's
/// globals) and return the result as JSON.
fn eval(e: Eval) -> Result<Value> {
    let [probe, expr, stmts] = eval_bodies(&e.js);
    let (id, text) = on_view(e.sel, move |v, reply| {
        automating(&v);
        let view = v.clone();
        let id = v.id.clone();
        // The probe parses in RADE's world: nothing runs, nothing reaches
        // the page's error handlers.
        v.call(&probe, Some(crate::browser_view::WORLD), move |r| {
            let body = if r.is_ok() { expr } else { stmts };
            view.call(&body, None, move |r| {
                reply(r.map(|t| (id, t)).map_err(Error::usage))
            })
        })
    })?;
    let result: Value = serde_json::from_str(&text).unwrap_or(Value::String(text));
    Ok(json!({"pane": id, "result": result}))
}

fn screenshot(s: Shot) -> Result<Value> {
    if !s.path.is_absolute() {
        return Err(Error::usage("screenshot path must be absolute"));
    }
    let path = s.path.clone();
    let region = if s.full {
        webkit6::SnapshotRegion::FullDocument
    } else {
        webkit6::SnapshotRegion::Visible
    };
    let (id, w, h) = on_view(s.sel, move |v, reply| {
        let id = v.id.clone();
        v.web.snapshot(
            region,
            webkit6::SnapshotOptions::NONE,
            gtk::gio::Cancellable::NONE,
            move |r| {
                use gtk::prelude::*;
                reply(
                    r.map_err(|e| {
                        Error::internal(format!("snapshot: {e} (is the Browser on screen?)"))
                    })
                    .and_then(|t| {
                        t.save_to_png(&path)
                            .map_err(|e| Error::internal(format!("{}: {e}", path.display())))?;
                        Ok((id, t.width(), t.height()))
                    }),
                )
            },
        )
    })?;
    Ok(json!({"pane": id, "path": s.path, "width": w, "height": h}))
}

fn wait(w: Wait) -> Result<Value> {
    let pane = on_view(w.sel, |v, reply| reply(Ok(v.id.clone())))?;
    if w.what == WaitFor::Load {
        return settle(&pane, None, w.timeout);
    }
    let arg = match &w.what {
        WaitFor::Text(t) => json!({"text": t}),
        WaitFor::Ref(r) => json!({"ref": r}),
        WaitFor::Load => unreachable!(),
    };
    let deadline = Instant::now() + Duration::from_secs(w.timeout);
    loop {
        let a = arg.clone();
        let sel = Sel {
            pane: Some(pane.clone()),
            caller: None,
        };
        // A page between loads has no document to ask: not yet.
        let met = on_view(sel, move |v, reply| {
            v.automation("check", &[a], move |r| {
                reply(Ok(r.is_ok_and(|t| t == true)))
            })
        })?;
        if met {
            return Ok(json!({"pane": pane, "met": arg}));
        }
        if Instant::now() > deadline {
            return Err(Error::new(
                ErrorKind::Timeout,
                format!("{pane}: timed out waiting for {arg}"),
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Listening ports per Pane of this RADE (its socket, as Panes get it).
pub fn scan_ports() -> std::collections::BTreeMap<String, std::collections::BTreeSet<u16>> {
    let socket = rade_core::paths::socket_path(&rade_core::paths::process_env);
    rade_core::ports::pane_ports(std::path::Path::new("/proc"), &socket.to_string_lossy())
}

/// A listening TCP port of a live Pane (R18.4).
#[derive(Clone)]
pub struct PortInfo {
    pub port: u16,
    pub pane: String,
    /// The Pane's name, else its Agent Preset, else its id.
    pub name: String,
    pub source: String,
    pub worktree: String,
    /// Source / Worktree ids.
    pub at: (String, String),
}

/// Listening ports of live Panes, sorted by port (scans `/proc` here, then
/// labels them on the main thread).
pub fn ports() -> Result<Vec<PortInfo>> {
    let by_pane = scan_ports();
    crate::handlers::on_main(move || Ok(label_ports(&app(), by_pane)))
}

/// Attach each Pane's ports to its place in the model (main thread).
pub fn label_ports(
    app: &App,
    by_pane: std::collections::BTreeMap<String, std::collections::BTreeSet<u16>>,
) -> Vec<PortInfo> {
    let m = app.model.borrow();
    let mut out = Vec::new();
    for (pane, ports) in by_pane {
        let (Some(loc), Some(p)) = (m.locate(&pane), m.pane(&pane)) else {
            continue;
        };
        let pr = m.source(&loc.source).expect("located");
        let wt = pr.worktree(&loc.worktree).expect("located");
        let name = p
            .name
            .clone()
            .or_else(|| p.agent.clone())
            .unwrap_or_else(|| pane.clone());
        for port in ports {
            out.push(PortInfo {
                port,
                pane: pane.clone(),
                name: name.clone(),
                source: pr.name.clone(),
                worktree: wt.name.clone(),
                at: (loc.source.clone(), loc.worktree.clone()),
            });
        }
    }
    out.sort_by(|a, b| (a.port, &a.pane).cmp(&(b.port, &b.pane)));
    out
}

fn ports_json(ports: &[PortInfo]) -> Vec<Value> {
    ports
        .iter()
        .map(|x| {
            json!({"port": x.port, "url": format!("http://localhost:{}/", x.port), "pane": x.pane,
                   "name": x.name, "source": x.source, "worktree": x.worktree})
        })
        .collect()
}

/// Ctrl+Shift+O / palette "Open Browser" (`url` None) and "Open
/// localhost:<port>": the Tab on screen's Browser (navigated to `url` if
/// given), else a new one split right of the focused Pane — on the first
/// port listening in this Worktree, else blank with the URL bar focused.
pub fn open_browser(url: Option<String>) {
    let app = app();
    let (p, w) = {
        let m = app.model.borrow();
        let (p, w) = m.active();
        (p.id.clone(), w.id.clone())
    };
    let existing = app
        .model
        .borrow()
        .find_browser(None, None)
        .ok()
        .filter(|b| {
            let m = app.model.borrow();
            m.active_tab().is_some_and(|t| t.pane(b).is_some())
        });
    let url = url.or_else(|| {
        existing.is_none().then(|| {
            let by_pane = scan_ports();
            label_ports(&app, by_pane)
                .into_iter()
                .find(|x| x.at == (p.clone(), w.clone()))
                .map(|x| format!("http://localhost:{}/", x.port))
        })?
    });
    let r = app.update(|m| match &existing {
        Some(b) => {
            if let Some(u) = &url {
                m.set_browser_url(b, u)?;
            }
            m.focus_pane(b).map(|_| (b.clone(), false))
        }
        None => m.open_browser_in(&p, &w, url.as_deref().unwrap_or(BLANK), true),
    });
    let Ok((id, created)) = r else { return };
    let view = app.browsers.borrow().get(&id).cloned();
    if let Some(v) = view {
        if let (false, Some(u)) = (created, &url) {
            v.load(u);
        }
        v.focus();
    }
}

/// `browser.annotations` (R71.5): a Browser's pending Annotations and the
/// prompt they make; with `send`, sent like the panel's Send to Agent.
fn annotations(a: Annotations) -> Result<Value> {
    crate::handlers::on_main(move || {
        let app = app();
        let id = app
            .model
            .borrow()
            .find_browser(a.sel.pane.as_deref(), a.sel.caller.as_deref())?;
        if a.send {
            return crate::annotate_view::send(&id, a.to, a.agent, a.sel.caller.as_deref());
        }
        let m = app.model.borrow();
        let list = m.annotations(&id)?;
        Ok(json!({
            "pane": id, "annotations": list,
            "prompt": (!list.is_empty()).then(|| rade_core::annotate::compose(list, false)),
        }))
    })
}

/// `debug.browser` (test mode, ADR-0021): drive annotate mode like the
/// owner — `toggle` (the header button), `pick` (`annotate.js` picks what
/// is at x,y / a region x,y,w,h / a ref, and types `comment`), `state`,
/// `edit` / `delete` a row, `send` / `clear` (the panel's buttons).
fn debug(p: Value) -> Result<Value> {
    let op = p["op"].as_str().unwrap_or("state").to_string();
    let sel = Sel {
        pane: p["pane"].as_str().map(String::from),
        caller: None,
    };
    match op.as_str() {
        "key" => on_view(sel, move |v, reply| {
            v.automation("annotateKey", &[p["key"].clone()], move |r| {
                reply(r.map_err(js_error))
            })
        }),
        "pick" => {
            let arg = p.clone();
            let r = on_view(sel, move |v, reply| {
                v.automation("annotateAt", &[arg], move |r| reply(r.map_err(js_error)))
            })?;
            // The page posts after two frames; the crop is async.
            std::thread::sleep(Duration::from_millis(600));
            Ok(r)
        }
        _ => on_view(sel, move |v, reply| {
            let r = match op.as_str() {
                "toggle" => {
                    v.annotate.emit_clicked();
                    Ok(v.debug_state())
                }
                "state" => Ok(v.debug_state()),
                "send" => {
                    v.panel.send.emit_clicked();
                    Ok(v.debug_state())
                }
                "clear" => {
                    v.panel.clear.emit_clicked();
                    Ok(v.debug_state())
                }
                "edit" | "delete" => {
                    let id = p["id"].as_u64().unwrap_or(0);
                    if v.debug_row(id, p["text"].as_str().filter(|_| op == "edit")) {
                        Ok(v.debug_state())
                    } else {
                        Err(Error::not_found(format!("no annotation row {id}")))
                    }
                }
                _ => Err(Error::usage(format!("unknown op '{op}'"))),
            };
            reply(r)
        }),
    }
}

/// Keep Chromium separate from the embedded engine and link feedback to this Tab's Agent.
pub(crate) fn open_chromium(view: &Rc<BrowserView>, button: &gtk::Button) {
    use std::os::unix::fs::PermissionsExt;
    button.set_sensitive(false);
    let app = app();
    let linked = {
        let m = app.model.borrow();
        m.locate(&view.id).and_then(|loc| {
            let agents = m.agent_panes(&loc.source, &loc.worktree);
            let here: Vec<_> = agents
                .iter()
                .filter(|p| m.locate(&p.id).is_some_and(|l| l.tab == loc.tab))
                .collect();
            if here.len() == 1 {
                Some(here[0].id.clone())
            } else if agents.len() == 1 {
                Some(agents[0].id.clone())
            } else {
                None
            }
        })
    };
    let url = view.preview_url();
    let binary = std::env::current_exe()
        .ok()
        .map(|p| p.with_file_name("rade"))
        .filter(|p| {
            std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
        .unwrap_or_else(|| "rade".into());
    let socket = app.paths.socket.clone();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut cmd = std::process::Command::new(binary);
        cmd.args(["browser", "--engine", "chromium", "--headed", "open", &url])
            .env("RADE_SOCKET", socket)
            .env_remove("RADE_PANE_ID")
            .stdin(std::process::Stdio::null());
        if let Some(linked) = linked {
            cmd.env("RADE_PANE_ID", linked);
        }
        let _ = tx.send(
            rade_core::shell_env::bounded_output(&mut cmd, Duration::from_secs(45))
                .map_err(|e| e.to_string())
                .and_then(|o| {
                    if o.status.success() {
                        Ok(())
                    } else {
                        Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
                    }
                }),
        );
    });
    let weak = Rc::downgrade(view);
    let button = button.clone();
    glib::timeout_add_local(Duration::from_millis(100), move || match rx.try_recv() {
        Ok(result) => {
            button.set_sensitive(true);
            if let Some(view) = weak.upgrade() {
                view.fallback_status(&match result {
                        Ok(()) => "Opened a separate Chromium window. Use its Annotate toolbar; existing WebKit annotations stay in this pane. If no single Agent is available, send via `rade browser --engine chromium annotations --send --to <agent-pane>`.".into(),
                        Err(e) => format!("Could not open Chromium: {e}. Use Open in default browser, or close an existing headless engine with `rade browser --engine chromium close`."),
                    });
            }
            glib::ControlFlow::Break
        }
        Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
        Err(_) => {
            button.set_sensitive(true);
            glib::ControlFlow::Break
        }
    });
    view.fallback_status("Opening Chromium in a separate window…");
}
