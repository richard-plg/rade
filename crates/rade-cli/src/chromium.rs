//! `rade browser --engine chromium` (PRD R72, ADR-0026): the same commands
//! and refs as a Browser Pane, on a real Chromium driven over the Chrome
//! DevTools Protocol. A small daemon (`rade __chromium <socket>`, started by
//! the first `open`) owns the Chromium — launched with
//! `--remote-debugging-pipe` (no TCP port), a temp profile it deletes, and
//! headless unless `--headed` — and serves the CLI the `browser.*` methods
//! on its own same-UID socket beside RADE's. Page-side code is RADE's
//! `browser.js` + `annotate.js` in an isolated world (`rade`), so refs and
//! snapshots match the WebKit engine; clicks and keys are trusted input.

use rade_core::annotate::{compose, Annotation, Pick, MAX_ERRORS};
use rade_core::browser::{automation_body, eval_bodies, normalize_url, parse_key, WaitFor};
use rade_core::cdp;
use rade_core::paths;
use rade_core::rpc::{Error, ErrorKind, Request, Response, Result};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::FromRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// What `pane` says in results.
const PANE: &str = "chromium";
/// A headless Chromium nobody used for this long is quit.
const IDLE: Duration = Duration::from_secs(30 * 60);
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CONSOLE: usize = 500;

/// The daemon's socket: beside RADE's (`rade-dev.sock` →
/// `rade-dev-chromium.sock`), so each RADE flavor / test instance has its own.
pub fn socket() -> PathBuf {
    let app = paths::socket_path(&paths::process_env);
    let stem = app
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "rade".into());
    app.with_file_name(format!("{stem}-chromium.sock"))
}

/// A `rade browser --engine chromium …` call: to the daemon, starting it
/// (and Chromium) on `open`.
pub fn call(method: &str, mut params: Value, headed: bool) -> Result<Value> {
    let sock = socket();
    if method == "browser.open" {
        params["headed"] = json!(headed);
    }
    match crate::call_at(&sock, method, params.clone(), None) {
        Err(e) if e.kind == ErrorKind::NotRunning => match method {
            "browser.open" => {
                start(&sock, headed)?;
                crate::call_at(&sock, method, params, None)
            }
            "browser.close" => Ok(json!({"pane": PANE, "closed": false})),
            _ => Err(Error::not_found(
                "no Chromium is running: start one with `rade browser --engine chromium open <url>`",
            )),
        },
        r => r,
    }
}

/// Start the daemon and wait until it serves (it binds only after
/// Chromium is up), or report why it could not.
fn start(sock: &Path, headed: bool) -> Result<()> {
    let exe = std::env::current_exe().map_err(|e| Error::internal(format!("rade: {e}")))?;
    let log = sock.with_extension("log");
    if let Some(d) = sock.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let out = File::create(&log).map_err(|e| Error::internal(format!("{}: {e}", log.display())))?;
    let mut cmd = Command::new(exe);
    cmd.arg("__chromium").arg(sock);
    if headed {
        cmd.arg("--headed");
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(
            out.try_clone()
                .map_err(|e| Error::internal(e.to_string()))?,
        )
        .stderr(out)
        // Its own process group: a Pane closing (RADE kills the Pane's
        // group) or Ctrl+C on this command does not take it down.
        .process_group(0)
        .spawn()
        .map_err(|e| Error::internal(format!("starting the Chromium daemon: {e}")))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if std::os::unix::net::UnixStream::connect(sock).is_ok() {
            return Ok(());
        }
        if let Ok(Some(_)) = child.try_wait() {
            let text = std::fs::read_to_string(&log).unwrap_or_default();
            let why = text
                .lines()
                .rev()
                .find(|l| l.starts_with("rade: "))
                .map(|l| l.trim_start_matches("rade: ").to_string())
                .unwrap_or_else(|| format!("the Chromium daemon exited (log: {})", log.display()));
            return Err(Error::not_found(why));
        }
        if Instant::now() > deadline {
            return Err(Error::new(
                ErrorKind::Timeout,
                "Chromium did not start in 30 s",
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The Chromium to run (R72.1): `[browser] chromium` in the user's
/// config.toml, else Playwright's, else one on PATH.
fn find_chromium() -> std::result::Result<PathBuf, String> {
    let env = &paths::process_env;
    let configured = std::fs::read_to_string(paths::config_file(env))
        .ok()
        .and_then(|t| rade_core::config::Config::parse(&t).ok())
        .and_then(|c| c.browser.chromium)
        .map(|p| match p.strip_prefix("~") {
            Ok(rest) => paths::home_dir(env).join(rest),
            Err(_) => p,
        });
    let list = |d: &Path| -> Vec<String> {
        std::fs::read_dir(d)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect()
    };
    let pw = env("PLAYWRIGHT_BROWSERS_PATH").map(PathBuf::from);
    let path = env("PATH").unwrap_or_default();
    let all = cdp::candidates(
        configured.as_deref(),
        &paths::home_dir(env),
        pw.as_deref(),
        &path,
        &list,
    );
    if let Some(c) = &configured {
        return if is_exe(c) {
            Ok(c.clone())
        } else {
            Err(format!(
                "[browser] chromium = {} is not an executable",
                c.display()
            ))
        };
    }
    all.into_iter().find(|p| is_exe(p)).ok_or_else(|| {
        "no Chromium found: install one (`npx playwright install chromium`) or set \
         [browser] chromium = \"/path/to/chrome\" in config.toml"
            .to_string()
    })
}

fn is_exe(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

// ---- the daemon -------------------------------------------------------------

#[derive(Default)]
struct Page {
    console: VecDeque<Value>,
    dropped: usize,
    errors: VecDeque<String>,
    loads: u64,
    loading: bool,
    load_error: Option<String>,
    main_frame: Option<String>,
    automating_until: Option<Instant>,
    annotating: bool,
    annotations: Vec<Annotation>,
    /// The Agent that opened it (`caller` of `open`): where annotations go.
    linked: Option<String>,
}

enum Job {
    Push,
    Dialog(Value),
    Binding(String),
}

struct Engine {
    headed: bool,
    /// Which Chromium, its PID and temp profile (`open` reports them).
    about: Value,
    writer: Mutex<File>,
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, mpsc::Sender<Value>>>,
    session: Mutex<Option<String>>,
    target: Mutex<Option<String>>,
    page: Mutex<Page>,
    jobs: Mutex<mpsc::Sender<Job>>,
    /// One `rade browser` command at a time.
    gate: Mutex<()>,
    last_use: Mutex<Instant>,
}

fn err_of(v: &Value) -> Option<String> {
    v.get("error")
        .map(|e| e["message"].as_str().unwrap_or("CDP error").to_string())
}

impl Engine {
    /// A CDP command (on the page's session unless `browser`); its result.
    fn cmd(&self, method: &str, params: Value, browser: bool) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let session = if browser {
            None
        } else {
            self.session.lock().unwrap().clone()
        };
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let bytes = cdp::frame(id, method, params, session.as_deref());
        if let Err(e) = self.writer.lock().unwrap().write_all(&bytes) {
            self.pending.lock().unwrap().remove(&id);
            return Err(Error::internal(format!("Chromium is gone ({e})")));
        }
        let reply = rx.recv_timeout(CALL_TIMEOUT).map_err(|e| {
            self.pending.lock().unwrap().remove(&id);
            match e {
                mpsc::RecvTimeoutError::Timeout => Error::new(
                    ErrorKind::Timeout,
                    format!("Chromium did not answer {method}"),
                ),
                mpsc::RecvTimeoutError::Disconnected => {
                    Error::internal(format!("Chromium exited during {method}"))
                }
            }
        })?;
        match err_of(&reply) {
            Some(e) => Err(Error::usage(format!("{method}: {e}"))),
            None => Ok(reply["result"].clone()),
        }
    }

    fn main_frame(&self) -> Result<String> {
        if let Some(f) = self.page.lock().unwrap().main_frame.clone() {
            return Ok(f);
        }
        let t = self.cmd("Page.getFrameTree", json!({}), false)?;
        let f = t["frameTree"]["frame"]["id"]
            .as_str()
            .ok_or_else(|| Error::internal("no main frame"))?
            .to_string();
        self.page.lock().unwrap().main_frame = Some(f.clone());
        Ok(f)
    }

    /// Evaluate an async function body; in RADE's world (`rade`) or the
    /// page's. Ok(value) / Err(JS error text).
    fn eval_body(&self, body: &str, isolated: bool) -> Result<std::result::Result<Value, String>> {
        let mut p = json!({
            "expression": format!("(async () => {{\n{body}\n}})()"),
            "awaitPromise": true, "returnByValue": true, "userGesture": true,
        });
        if isolated {
            let frame = self.main_frame()?;
            let w = self.cmd(
                "Page.createIsolatedWorld",
                json!({"frameId": frame, "worldName": "rade"}),
                false,
            )?;
            p["contextId"] = w["executionContextId"].clone();
        }
        let r = self.cmd("Runtime.evaluate", p, false)?;
        if let Some(d) = r.get("exceptionDetails") {
            let text = d["exception"]["description"]
                .as_str()
                .or(d["exception"]["value"].as_str())
                .or(d["text"].as_str())
                .unwrap_or("error");
            return Ok(Err(text.lines().next().unwrap_or(text).to_string()));
        }
        Ok(Ok(r["result"].get("value").cloned().unwrap_or(Value::Null)))
    }

    /// `__rade.<f>(args)` in RADE's world → its value or its error.
    fn automation(&self, f: &str, args: &[Value]) -> Result<Value> {
        let v = self
            .eval_body(&automation_body(f, args), true)?
            .map_err(|e| Error::usage(e.trim_start_matches("Error: ").to_string()))?;
        match v["error"].as_str() {
            Some(e) if e.contains("unknown ref") => Err(Error::not_found(e)),
            Some(e) => Err(Error::usage(e)),
            None => Ok(v["ok"].clone()),
        }
    }

    fn info(&self) -> Value {
        let target = self.target.lock().unwrap().clone();
        let t = self
            .cmd("Target.getTargetInfo", json!({"targetId": target}), true)
            .unwrap_or_default();
        let p = self.page.lock().unwrap();
        json!({
            "pane": PANE, "url": t["targetInfo"]["url"], "title": t["targetInfo"]["title"],
            "loading": p.loading, "error": p.load_error, "engine": "chromium",
        })
    }

    /// Like the WebKit engine's: until a load after `before` finished (or,
    /// after 500 ms, nothing is loading), or with None until not loading.
    fn settle(&self, before: Option<u64>, timeout: u64) -> Result<Value> {
        let start = Instant::now();
        loop {
            let late = start.elapsed() > Duration::from_millis(500);
            let done = {
                let p = self.page.lock().unwrap();
                match before {
                    Some(n) => p.loads > n || (late && !p.loading),
                    None => !p.loading,
                }
            };
            if done {
                return Ok(self.info());
            }
            if start.elapsed() > Duration::from_secs(timeout) {
                return Err(Error::new(ErrorKind::Timeout, "Chromium is still loading"));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn loads(&self) -> u64 {
        self.page.lock().unwrap().loads
    }

    fn automating(&self) {
        self.page.lock().unwrap().automating_until = Some(Instant::now() + Duration::from_secs(10));
    }

    fn after_action(&self, before: u64, mut r: Value) -> Result<Value> {
        std::thread::sleep(Duration::from_millis(150));
        let s = self.settle(None, 30)?;
        r["navigated"] = json!(self.loads() > before);
        r["url"] = s["url"].clone();
        Ok(r)
    }

    fn log(&self, m: Value) {
        let mut p = self.page.lock().unwrap();
        if m["level"] == "error" {
            if p.errors.len() == 20 {
                p.errors.pop_front();
            }
            let first = m["text"]
                .as_str()
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            p.errors.push_back(first);
        }
        if p.console.len() == MAX_CONSOLE {
            p.console.pop_front();
            p.dropped += 1;
        }
        p.console.push_back(m);
    }

    /// The overlay's state in the page (mode, toolbar when headed, marks);
    /// after a load only when there is something to show.
    fn push(&self) {
        self.push_if(true)
    }

    fn push_if(&self, always: bool) {
        let (on, items) = {
            let p = self.page.lock().unwrap();
            let items: Vec<Value> = p
                .annotations
                .iter()
                .map(|a| json!({"n": a.id, "url": a.url, "selector": a.selector, "kind": a.kind, "bbox": a.bbox}))
                .collect();
            (p.annotating, items)
        };
        if always || on || self.headed || !items.is_empty() {
            let _ = self.automation(
                "annotate",
                &[json!({"on": on, "toolbar": self.headed, "items": items})],
            );
        }
    }

    /// A message from `annotate.js` (the `radeAnnotate` binding).
    fn binding(&self, payload: &str) {
        let Ok(m) = serde_json::from_str::<Value>(payload) else {
            return;
        };
        match m["op"].as_str() {
            Some("toggle") => {
                self.page.lock().unwrap().annotating = m["on"] == true;
                self.push();
            }
            Some("add") => {
                let Ok(pick) = serde_json::from_value::<Pick>(m["pick"].clone()) else {
                    return;
                };
                if pick.comment.trim().is_empty() {
                    return;
                }
                let shot = self
                    .crop(&pick)
                    .map_err(|e| eprintln!("rade: crop: {}", e.message))
                    .ok();
                let mut p = self.page.lock().unwrap();
                let errors: Vec<String> = p
                    .errors
                    .iter()
                    .skip(p.errors.len().saturating_sub(MAX_ERRORS))
                    .cloned()
                    .collect();
                let id = p.annotations.iter().map(|a| a.id).max().unwrap_or(0) + 1;
                p.annotations
                    .push(Annotation::from_pick(id, pick, shot, errors));
                drop(p);
                self.push();
            }
            Some("send") => {
                if let Err(e) = self.send(None) {
                    eprintln!("rade: send: {}", e.message);
                }
            }
            Some("clear") => {
                self.page.lock().unwrap().annotations.clear();
                self.push();
            }
            _ => {}
        }
    }

    /// The annotation's box (+8 px) as a PNG, cut by Chromium itself.
    fn crop(&self, pick: &Pick) -> Result<PathBuf> {
        let r = pick.rect;
        let (x, y) = ((r.x - 8.0).max(0.0), (r.y - 8.0).max(0.0));
        let clip = json!({
            "x": x + pick.scroll.x, "y": y + pick.scroll.y,
            "width": (r.w + 16.0).min(pick.viewport.w - x).max(1.0),
            "height": (r.h + 16.0).min(pick.viewport.h - y).max(1.0), "scale": 1,
        });
        let s = self.cmd(
            "Page.captureScreenshot",
            json!({"format": "png", "clip": clip}),
            false,
        )?;
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let paste = paths::paste_dir(&paths::process_env);
        let path = paste
            .parent()
            .unwrap_or(&paste)
            .join("annotations")
            .join(format!("chromium-{millis}.png"));
        write_png(&s, &path)?;
        Ok(path)
    }

    /// Send the annotations to `to`, else the Agent that opened Chromium.
    fn send(&self, to: Option<String>) -> Result<Value> {
        let (list, linked) = {
            let p = self.page.lock().unwrap();
            (p.annotations.clone(), p.linked.clone())
        };
        if list.is_empty() {
            return Err(Error::usage("no Browser Annotations to send"));
        }
        let target = to.or(linked).ok_or_else(|| {
            Error::not_found(
                "no Agent to send to: pass --to <pane> (this Chromium was not opened from a RADE Pane)",
            )
        })?;
        // Only into an Agent Pane: the prompt holds page text a shell would run.
        let app = paths::socket_path(&paths::process_env);
        let panes = crate::call_at(
            &app,
            "pane.list",
            json!({"all": true}),
            Some(Duration::from_secs(10)),
        )?;
        let agent = panes["panes"].as_array().into_iter().flatten().any(|p| {
            (p["pane"] == target.as_str() || p["name"] == target.as_str()) && !p["agent"].is_null()
        });
        if !agent {
            return Err(Error::usage(format!(
                "{target} is not an Agent Pane: send to an Agent (its text would run in a shell)"
            )));
        }
        let prompt = compose(&list, true);
        crate::call_at(
            &paths::socket_path(&paths::process_env),
            "pane.send",
            json!({"pane": target, "text": prompt, "paste": true, "enter": true}),
            Some(Duration::from_secs(15)),
        )?;
        self.page.lock().unwrap().annotations.clear();
        self.push();
        Ok(
            json!({"pane": target, "browser": PANE, "annotations": list.len(), "prompt": prompt, "spawned": false}),
        )
    }

    // ---- browser.* ----

    fn dispatch(&self, method: &str, p: Value) -> Result<Value> {
        let _one = self.gate.lock().unwrap();
        *self.last_use.lock().unwrap() = Instant::now();
        match method {
            "browser.open" => {
                if p["headed"].as_bool().unwrap_or(false) != self.headed {
                    return Err(Error::usage(format!(
                        "Chromium is running {}: `rade browser --engine chromium close` first",
                        if self.headed { "headed" } else { "headless" }
                    )));
                }
                let url = normalize_url(p["url"].as_str().unwrap_or(""))?;
                if let Some(c) = p["caller"].as_str() {
                    self.page.lock().unwrap().linked = Some(c.to_string());
                }
                let before = self.loads();
                self.page.lock().unwrap().load_error = None;
                let r = self.cmd("Page.navigate", json!({"url": url}), false)?;
                if let Some(e) = r["errorText"].as_str() {
                    let msg = format!("{url}: {e}");
                    self.log(json!({"level": "error", "text": format!("Load failed: {msg}"), "source": url, "line": 0, "time": now()}));
                    self.page.lock().unwrap().load_error = Some(msg);
                }
                let mut r = self.settle(Some(before), 30)?;
                r["chromium"] = self.about.clone();
                Ok(r)
            }
            "browser.snapshot" => {
                let page = self.automation("snapshot", &[])?;
                let mut r = rade_core::browser::snapshot_result(&page, p["interactive"] == true)?;
                r["pane"] = json!(PANE);
                Ok(r)
            }
            "browser.click" => {
                let r#ref = p["ref"].as_str().unwrap_or("").to_string();
                let before = self.loads();
                self.automating();
                let pt = self.automation("point", &[json!(r#ref)])?;
                let (x, y) = (
                    pt["x"].as_f64().unwrap_or(0.0),
                    pt["y"].as_f64().unwrap_or(0.0),
                );
                for (t, buttons) in [("mouseMoved", 0), ("mousePressed", 1), ("mouseReleased", 0)] {
                    let mut e = json!({"type": t, "x": x, "y": y, "buttons": buttons});
                    if t != "mouseMoved" {
                        e["button"] = json!("left");
                        e["clickCount"] = json!(1);
                    }
                    self.cmd("Input.dispatchMouseEvent", e, false)?;
                }
                self.after_action(
                    before,
                    json!({"pane": PANE, "ref": r#ref, "element": pt["el"]}),
                )
            }
            "browser.fill" => {
                let r#ref = p["ref"].as_str().unwrap_or("").to_string();
                let text = p["text"]
                    .as_str()
                    .ok_or_else(|| Error::usage("fill needs text"))?;
                let before = self.loads();
                self.automating();
                let el = self.automation("fill", &[json!(r#ref), json!(text)])?;
                self.after_action(before, json!({"pane": PANE, "ref": r#ref, "element": el}))
            }
            "browser.press" => {
                let key = p["key"].as_str().unwrap_or("").to_string();
                let spec = parse_key(&key)?;
                let before = self.loads();
                self.automating();
                for e in cdp::key_events(&spec) {
                    self.cmd("Input.dispatchKeyEvent", e, false)?;
                }
                let el = self.automation("focused", &[])?;
                self.after_action(before, json!({"pane": PANE, "key": key, "focused": el}))
            }
            "browser.eval" => {
                let [probe, expr, stmts] = eval_bodies(p["js"].as_str().unwrap_or(""));
                self.automating();
                let body = if self.eval_body(&probe, true)?.is_ok() {
                    expr
                } else {
                    stmts
                };
                let v = self.eval_body(&body, false)?.map_err(Error::usage)?;
                Ok(json!({"pane": PANE, "result": v}))
            }
            "browser.screenshot" => {
                let path = PathBuf::from(p["path"].as_str().unwrap_or(""));
                if !path.is_absolute() {
                    return Err(Error::usage("screenshot path must be absolute"));
                }
                let mut q = json!({"format": "png"});
                if p["full"] == true {
                    let m = self.cmd("Page.getLayoutMetrics", json!({}), false)?;
                    let c = &m["cssContentSize"];
                    q["clip"] = json!({"x": 0, "y": 0, "width": c["width"], "height": c["height"], "scale": 1});
                    q["captureBeyondViewport"] = json!(true);
                }
                let s = self.cmd("Page.captureScreenshot", q, false)?;
                let (w, h) = write_png(&s, &path)?;
                Ok(json!({"pane": PANE, "path": path, "width": w, "height": h}))
            }
            "browser.console" => {
                let mut pg = self.page.lock().unwrap();
                let messages: Vec<Value> = pg.console.drain(..).collect();
                let dropped = std::mem::take(&mut pg.dropped);
                Ok(json!({"pane": PANE, "messages": messages, "dropped": dropped}))
            }
            "browser.wait" => {
                let what: WaitFor = serde_json::from_value(p["for"].clone())
                    .map_err(|e| Error::usage(format!("bad params: {e}")))?;
                let timeout = p["timeout"].as_u64().unwrap_or(30);
                if what == WaitFor::Load {
                    return self.settle(None, timeout);
                }
                let arg = match &what {
                    WaitFor::Text(t) => json!({"text": t}),
                    WaitFor::Ref(r) => json!({"ref": r}),
                    WaitFor::Load => unreachable!(),
                };
                let deadline = Instant::now() + Duration::from_secs(timeout);
                loop {
                    if self
                        .automation("check", std::slice::from_ref(&arg))
                        .is_ok_and(|v| v == true)
                    {
                        return Ok(json!({"pane": PANE, "met": arg}));
                    }
                    if Instant::now() > deadline {
                        return Err(Error::new(
                            ErrorKind::Timeout,
                            format!("{PANE}: timed out waiting for {arg}"),
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
            "browser.back" | "browser.forward" => {
                let delta = if method == "browser.back" { -1 } else { 1 };
                let h = self.cmd("Page.getNavigationHistory", json!({}), false)?;
                let id = cdp::history_step(&h, delta).ok_or_else(|| {
                    Error::usage(format!(
                        "no page to go {} to",
                        if delta < 0 { "back" } else { "forward" }
                    ))
                })?;
                let before = self.loads();
                self.cmd("Page.navigateToHistoryEntry", json!({"entryId": id}), false)?;
                self.settle(Some(before), 30)
            }
            "browser.reload" => {
                let before = self.loads();
                self.cmd("Page.reload", json!({}), false)?;
                self.settle(Some(before), 30)
            }
            "browser.annotations" => {
                if p["send"] == true {
                    return self.send(p["to"].as_str().map(String::from));
                }
                let list = self.page.lock().unwrap().annotations.clone();
                Ok(json!({
                    "pane": PANE, "annotations": list,
                    "prompt": (!list.is_empty()).then(|| compose(&list, true)),
                }))
            }
            "browser.annotate" => {
                self.page.lock().unwrap().annotating = p["on"].as_bool().unwrap_or(true);
                self.push();
                Ok(json!({"pane": PANE, "on": self.page.lock().unwrap().annotating}))
            }
            "browser.close" => {
                std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_millis(200));
                    SHUTDOWN.with_exit();
                });
                Ok(json!({"pane": PANE, "closed": true}))
            }
            "browser.ports" => Err(Error::usage("ports are RADE's: run it without --engine")),
            _ => Err(Error::usage(format!("unknown method '{method}'"))),
        }
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Decode `Page.captureScreenshot`'s base64 PNG into `path`; its size.
fn write_png(s: &Value, path: &Path) -> Result<(u32, u32)> {
    use base64::Engine as _;
    let png = base64::engine::general_purpose::STANDARD
        .decode(s["data"].as_str().unwrap_or(""))
        .map_err(|e| Error::internal(format!("screenshot: {e}")))?;
    if png.len() < 24 {
        return Err(Error::internal("screenshot: empty"));
    }
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    std::fs::write(path, &png).map_err(|e| Error::internal(format!("{}: {e}", path.display())))?;
    let w = u32::from_be_bytes(png[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(png[20..24].try_into().unwrap());
    Ok((w, h))
}

/// The daemon's exit: Chromium (our child only) and its profile go too.
struct Shutdown {
    tx: Mutex<Option<mpsc::Sender<()>>>,
}

impl Shutdown {
    fn with_exit(&self) {
        if let Some(tx) = self.tx.lock().unwrap().as_ref() {
            let _ = tx.send(());
        }
    }
}

static SHUTDOWN: Shutdown = Shutdown {
    tx: Mutex::new(None),
};
/// SIGTERM / SIGHUP / SIGINT arrived (a logout, `kill`): clean up and exit.
static SIGNALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    SIGNALLED.store(true, Ordering::SeqCst);
}

/// Profiles of daemons that died without cleaning up (`kill -9`):
/// `<prefix><pid>` whose process is gone.
fn sweep_profiles(prefix: &str) {
    for e in std::fs::read_dir(std::env::temp_dir())
        .into_iter()
        .flatten()
        .flatten()
    {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(pid) = name
            .strip_prefix(prefix)
            .and_then(|p| p.parse::<u32>().ok())
        else {
            continue;
        };
        if !Path::new(&format!("/proc/{pid}")).exists() {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// A pipe end moved above the low fds, close-on-exec.
fn high_fd(fd: i32) -> i32 {
    let h = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 10) };
    unsafe { libc::close(fd) };
    h
}

fn pipe() -> std::io::Result<(i32, i32)> {
    let mut f = [0i32; 2];
    if unsafe { libc::pipe2(f.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((high_fd(f[0]), high_fd(f[1])))
}

/// `rade __chromium <socket> [--headed]`: launch Chromium, attach, serve
/// until it exits, `close`, or (headless) 30 idle minutes.
pub fn daemon(sock: &Path, headed: bool) -> i32 {
    match run_daemon(sock, headed) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("rade: {e}");
            1
        }
    }
}

fn run_daemon(sock: &Path, headed: bool) -> std::result::Result<(), String> {
    let bin = find_chromium()?;
    let flavor = sock
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    sweep_profiles(&format!("{flavor}-profile-"));
    for sig in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
        unsafe { libc::signal(sig, on_signal as *const () as libc::sighandler_t) };
    }
    let profile = std::env::temp_dir().join(format!("{flavor}-profile-{}", std::process::id()));
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&profile)
            .map_err(|e| format!("{}: {e}", profile.display()))?;
    }
    let (to_r, to_w) = pipe().map_err(|e| e.to_string())?; // we write, Chromium reads fd 3
    let (from_r, from_w) = pipe().map_err(|e| e.to_string())?; // Chromium writes fd 4
    let mut cmd = Command::new(&bin);
    cmd.args(cdp::launch_args(
        &profile,
        headed,
        paths::process_env("WAYLAND_DISPLAY").is_some(),
    ))
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::inherit());
    unsafe {
        cmd.pre_exec(move || {
            if libc::dup2(to_r, 3) < 0 || libc::dup2(from_w, 4) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child: Child = cmd.spawn().map_err(|e| format!("{}: {e}", bin.display()))?;
    unsafe {
        libc::close(to_r);
        libc::close(from_w);
    }
    let writer = unsafe { File::from_raw_fd(to_w) };
    let mut reader = unsafe { File::from_raw_fd(from_r) };
    let cleanup = |child: &mut Child| {
        unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(3) {
            if let Ok(Some(_)) = child.try_wait() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&profile);
    };

    let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
    let (exit_tx, exit_rx) = mpsc::channel::<()>();
    *SHUTDOWN.tx.lock().unwrap() = Some(exit_tx.clone());
    let eng = Arc::new(Engine {
        headed,
        about: json!({"path": bin, "pid": child.id(), "daemon": std::process::id(), "profile": profile, "headed": headed}),
        writer: Mutex::new(writer),
        next_id: AtomicU64::new(1),
        pending: Mutex::default(),
        session: Mutex::default(),
        target: Mutex::default(),
        page: Mutex::default(),
        jobs: Mutex::new(jobs_tx),
        gate: Mutex::new(()),
        last_use: Mutex::new(Instant::now()),
    });

    // Reader: replies to their callers, events to the page state / jobs.
    let e2 = eng.clone();
    let exit2 = exit_tx.clone();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 65536];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
            for f in cdp::take_frames(&mut buf) {
                let Ok(m) = serde_json::from_slice::<Value>(&f) else {
                    continue;
                };
                if let Some(id) = m["id"].as_u64() {
                    if let Some(tx) = e2.pending.lock().unwrap().remove(&id) {
                        let _ = tx.send(m);
                    }
                } else {
                    e2.event(&m);
                }
            }
        }
        // Chromium exited (or its window was closed): nobody waits in vain.
        e2.pending.lock().unwrap().clear();
        let _ = exit2.send(());
    });
    // Jobs that need CDP calls of their own (not on the reader thread).
    let e3 = eng.clone();
    std::thread::spawn(move || {
        for j in jobs_rx {
            match j {
                Job::Push => e3.push_if(false),
                Job::Binding(p) => e3.binding(&p),
                Job::Dialog(d) => {
                    let _ = e3.cmd(
                        "Page.handleJavaScriptDialog",
                        json!({"accept": true, "promptText": d["defaultPrompt"].as_str().unwrap_or("")}),
                        false,
                    );
                }
            }
        }
    });

    if let Err(e) = eng.attach() {
        cleanup(&mut child);
        return Err(format!("attaching to Chromium: {}", e.message));
    }
    let e4 = eng.clone();
    let dispatch: Arc<rade_core::server::Dispatch> = Arc::new(move |req: Request| {
        let id = req.id.clone();
        match e4.dispatch(&req.method, req.params) {
            Ok(v) => Response::ok(id, v),
            Err(e) => Response::err(id, &e),
        }
    });
    if let Err(e) = rade_core::server::listen(sock, dispatch) {
        cleanup(&mut child);
        return Err(format!("{}: {e}", sock.display()));
    }
    eprintln!(
        "rade: Chromium {} ({}) on {}",
        bin.display(),
        if headed { "headed" } else { "headless" },
        sock.display()
    );
    loop {
        if SIGNALLED.load(Ordering::SeqCst) {
            break;
        }
        match exit_rx.recv_timeout(Duration::from_millis(500)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !headed && eng.last_use.lock().unwrap().elapsed() > IDLE {
                    break;
                }
                if let Ok(Some(_)) = child.try_wait() {
                    break;
                }
            }
        }
    }
    let _ = std::fs::remove_file(sock);
    cleanup(&mut child);
    Ok(())
}

impl Engine {
    /// Attach to Chromium's first page and set it up: domains, the
    /// annotation binding (RADE's world only) and the library on every
    /// new document of that world.
    fn attach(&self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(20);
        let target = loop {
            let t = self.cmd("Target.getTargets", json!({}), true)?;
            let page = t["targetInfos"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|x| x["type"] == "page")
                .and_then(|x| x["targetId"].as_str().map(String::from));
            if let Some(p) = page {
                break p;
            }
            if Instant::now() > deadline {
                return Err(Error::internal("Chromium has no page"));
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        let a = self.cmd(
            "Target.attachToTarget",
            json!({"targetId": target, "flatten": true}),
            true,
        )?;
        *self.session.lock().unwrap() = a["sessionId"].as_str().map(String::from);
        *self.target.lock().unwrap() = Some(target);
        self.cmd("Page.enable", json!({}), false)?;
        self.cmd("Runtime.enable", json!({}), false)?;
        self.cmd(
            "Runtime.addBinding",
            json!({"name": "radeAnnotate", "executionContextName": "rade"}),
            false,
        )?;
        let boot = format!(
            "(async () => {{\n{}\n}})();",
            automation_body(
                "annotate",
                &[json!({"on": false, "toolbar": self.headed, "items": []})]
            )
        );
        let boot = format!(
            "if (document.readyState === 'loading') addEventListener('DOMContentLoaded', () => {{ {boot} }}); else {{ {boot} }}"
        );
        self.cmd(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source": boot, "worldName": "rade", "runImmediately": true}),
            false,
        )?;
        self.main_frame()?;
        Ok(())
    }

    /// A CDP event (reader thread: no CDP calls here, they go to jobs).
    fn event(&self, m: &Value) {
        let p = &m["params"];
        let main = |f: &Value| {
            let cur = self.page.lock().unwrap().main_frame.clone();
            cur.is_none() || f.as_str() == cur.as_deref()
        };
        match m["method"].as_str().unwrap_or("") {
            "Runtime.consoleAPICalled" => {
                let mut c = cdp::console_message(p);
                c["time"] = json!(now());
                self.log(c);
            }
            "Runtime.exceptionThrown" => {
                let mut c = cdp::exception_message(p);
                c["time"] = json!(now());
                self.log(c);
            }
            "Page.frameStartedLoading" if main(&p["frameId"]) => {
                let mut pg = self.page.lock().unwrap();
                pg.loading = true;
                pg.load_error = None;
            }
            "Page.frameStoppedLoading" if main(&p["frameId"]) => {
                self.page.lock().unwrap().loading = false;
            }
            "Page.loadEventFired" => {
                self.page.lock().unwrap().loads += 1;
                let _ = self.jobs.lock().unwrap().send(Job::Push);
            }
            "Page.frameNavigated" if p["frame"]["parentId"].is_null() => {
                self.page.lock().unwrap().main_frame = p["frame"]["id"].as_str().map(String::from);
            }
            "Page.javascriptDialogOpening" => {
                let live = self
                    .page
                    .lock()
                    .unwrap()
                    .automating_until
                    .is_some_and(|t| Instant::now() < t);
                // Headless: nobody else could answer it.
                if live || !self.headed {
                    let kind = p["type"].as_str().unwrap_or("alert").to_string();
                    let kind = if kind == "beforeunload" {
                        "confirm".into()
                    } else {
                        kind
                    };
                    self.log(json!({"level": "dialog", "text": format!("{kind} \"{}\" → accepted", p["message"].as_str().unwrap_or("")), "source": p["url"], "line": 0, "time": now()}));
                    let _ = self.jobs.lock().unwrap().send(Job::Dialog(p.clone()));
                }
            }
            "Runtime.bindingCalled" if p["name"] == "radeAnnotate" => {
                if let Some(s) = p["payload"].as_str() {
                    let _ = self.jobs.lock().unwrap().send(Job::Binding(s.to_string()));
                }
            }
            "Target.detachedFromTarget" | "Inspector.detached" => SHUTDOWN.with_exit(),
            _ => {}
        }
    }
}
