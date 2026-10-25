//! Voice prompts, the controller (PRD R124, PLAN-voice.md T7): the recorder,
//! speech-to-text requests and the voice agent process, driven through
//! `rade_core::voice::composer`. One Voice Composer in the whole app; the
//! state machine decides, this module carries out its [`Action`]s and turns
//! what happens (keys, audio, curl and agent output, the clock) into
//! [`Event`]s. The widget is `voice_view`, called only through its small
//! interface.

use crate::app::{app, try_app};
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use rade_core::config::{VoiceAgent, VoiceConfig};
use rade_core::rpc::{Error, Result};
use rade_core::voice::agent::{self, AgentEvent};
use rade_core::voice::composer::{Action, Composer, Event, Mode};
use rade_core::voice::{context, stt};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// Bars of the level meter (PLAN §5.1).
const METER_BARS: usize = 28;
/// Live words: one request per this long while recording (D6).
const PARTIAL_EVERY_MS: u64 = 1_200;
/// 50 ms of 16 kHz audio: one meter level (D4).
const LEVEL_SAMPLES: usize = 800;
/// `stt_key_command` may take this long.
const KEY_TIMEOUT: Duration = Duration::from_secs(10);

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

thread_local! {
    static CTL: RefCell<Ctl> = RefCell::new(Ctl::new());
    /// The `voice` key is down (so a second press is an auto-repeat).
    static HELD: Cell<bool> = const { Cell::new(false) };
    static EPOCH: Instant = Instant::now();
}

fn now_ms() -> u64 {
    EPOCH.with(|e| e.elapsed().as_millis() as u64)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Partial,
    Final,
}

/// Something that happened off the main thread.
enum Msg {
    Levels {
        rec: u64,
        levels: Vec<f32>,
    },
    RecEnded {
        rec: u64,
        stderr: String,
    },
    Stt {
        session: u64,
        kind: Kind,
        result: std::result::Result<String, String>,
    },
    Agent {
        id: u64,
        events: Vec<AgentEvent>,
    },
    AgentExited {
        id: u64,
        ok: bool,
        stderr: String,
    },
    Context {
        session: u64,
        text: String,
        vocabulary: String,
    },
}

fn post(m: Msg) {
    glib::MainContext::default().invoke(move || on_msg(m));
}

struct Recorder {
    id: u64,
    pgid: i32,
    child: Child,
}

struct AgentProc {
    id: u64,
    pgid: i32,
    tx: Option<mpsc::Sender<String>>,
    turns: u32,
    argv: Vec<String>,
}

struct Ctl {
    composer: Composer,
    cfg: VoiceConfig,
    /// The Pane the open composer belongs to.
    pane: Option<String>,
    /// Bumped whenever a session opens or closes: stale replies are dropped.
    session: u64,
    dir: PathBuf,
    env: Vec<(String, String)>,
    recorder: Option<Recorder>,
    pcm: Arc<Mutex<Vec<i16>>>,
    levels: VecDeque<f32>,
    agent: Option<AgentProc>,
    stt_busy: bool,
    final_pending: bool,
    stt_requests: u32,
    last_partial_ms: u64,
    /// The process group of the running `curl`, 0 = none.
    curl: Arc<AtomicI32>,
    /// `stt_key_command`'s answer, once per session.
    key: Arc<Mutex<Option<Option<String>>>>,
    vocabulary: Arc<Mutex<String>>,
    /// The draft was changed by the user since the composer last set it.
    edited: bool,
    seen_version: u64,
    seen_draft: String,
}

impl Ctl {
    fn new() -> Self {
        Self {
            composer: Composer::new(&VoiceConfig::default(), "F8"),
            cfg: VoiceConfig::default(),
            pane: None,
            session: 0,
            dir: PathBuf::new(),
            env: Vec::new(),
            recorder: None,
            pcm: Arc::default(),
            levels: VecDeque::new(),
            agent: None,
            stt_busy: false,
            final_pending: false,
            stt_requests: 0,
            last_partial_ms: 0,
            curl: Arc::default(),
            key: Arc::default(),
            vocabulary: Arc::default(),
            edited: false,
            seen_version: 0,
            seen_draft: String::new(),
        }
    }
}

fn with<T>(f: impl FnOnce(&mut Ctl) -> T) -> T {
    CTL.with(|c| f(&mut c.borrow_mut()))
}

// ---------------------------------------------------------------- entry points

/// An event from the Voice Composer's widget (keys inside the card, the
/// close, Send and Paste only buttons, edits of the draft).
#[allow(dead_code)] // called by the widget (voice_view)
pub fn on_ui(event: Event) {
    feed(event);
}

/// The mic button of Pane `pane_id` (and the palette's Voice Prompt on the
/// focused Pane): a tap (D2 toggle, D3).
pub fn tap(pane_id: &str) {
    if route(pane_id) {
        feed(Event::Tap);
    }
}

/// The palette action `voice`: a tap on the focused Pane.
pub fn tap_focused() {
    if let Some(p) = focused_pane() {
        tap(&p);
    }
}

/// The `voice` key went down (capture handler). `false` = not ours now
/// (palette, Settings or Overview open): the key goes on.
pub fn key_down() -> bool {
    if crate::palette::is_open()
        || crate::settings_view::is_open()
        || crate::overview_view::is_open()
    {
        return false;
    }
    // Auto-repeat presses are ignored (D2).
    if HELD.with(|h| h.replace(true)) {
        return true;
    }
    if let Some(p) = focused_pane() {
        if route(&p) {
            feed(Event::KeyDown {
                repeat: false,
                at_ms: 0,
            });
        }
    }
    true
}

/// The `voice` key went up.
pub fn key_up() {
    if HELD.with(|h| h.replace(false)) {
        feed(Event::KeyUp { at_ms: 0 });
    }
}

/// The window lost focus: a take ends like a release (D2).
pub fn focus_lost() {
    HELD.with(|h| h.set(false));
    feed(Event::FocusLost);
}

/// A Pane's Terminal went away (closed, paused).
pub fn pane_closed(id: &str) {
    if with(|c| c.pane.as_deref() == Some(id)) {
        feed(Event::PaneClosed);
    }
}

/// App quit: no process of ours outlives it.
pub fn shutdown() {
    with(|c| {
        c.session = next_id();
        kill_recorder(c);
        kill_agent(c);
        kill_curl(c);
    });
}

// ------------------------------------------------------------------- routing

fn focused_pane() -> Option<String> {
    let app = try_app()?;
    let m = app.model.borrow();
    m.active_tab().map(|t| t.focused.clone())
}

fn pane_label(id: &str) -> String {
    try_app()
        .and_then(|a| {
            a.model
                .borrow()
                .pane(id)
                .map(|p| p.name.clone().unwrap_or_else(|| p.id.clone()))
        })
        .unwrap_or_else(|| id.to_string())
}

fn toast(text: &str) {
    if let Some(app) = try_app() {
        app.chrome.toast(&glib::markup_escape_text(text), None);
    }
}

fn is_agent_pane(id: &str) -> bool {
    try_app().is_some_and(|a| {
        a.model.borrow().pane(id).is_some_and(|p| {
            p.agent_name().is_some() && matches!(p.kind, rade_core::PaneKind::Terminal { .. })
        })
    })
}

/// May `pane_id` use the Voice Composer now? Opens it when none is open.
fn route(pane_id: &str) -> bool {
    match with(|c| c.pane.clone()) {
        Some(p) if p == pane_id => true,
        Some(p) => {
            toast(&format!("Voice is open on {}", pane_label(&p)));
            false
        }
        None => {
            if !is_agent_pane(pane_id) {
                toast("Voice works on Agent Panes");
                return false;
            }
            open_session(pane_id)
        }
    }
}

fn key_label(config: &rade_core::config::Config) -> String {
    config
        .keybindings()
        .into_iter()
        .find(|(a, _)| a == "voice")
        .and_then(|(_, accel)| {
            let (key, mods) = gtk::accelerator_parse(&accel)?;
            Some(gtk::accelerator_get_label(key, mods).to_string())
        })
        .unwrap_or_else(|| "F8".into())
}

fn home_dir() -> PathBuf {
    rade_core::paths::home_dir(&rade_core::paths::process_env)
}

fn open_session(pane_id: &str) -> bool {
    let app = app();
    let (input, env, cfg, target, label) = {
        let m = app.model.borrow();
        let Some(pane) = m.pane(pane_id) else {
            return false;
        };
        let Some(loc) = m.locate(pane_id) else {
            return false;
        };
        let (Some(source), Some(wt)) = (
            m.source(&loc.source),
            m.worktree(&loc.source, &loc.worktree),
        ) else {
            return false;
        };
        let worktree = pane
            .runs_in
            .as_deref()
            .and_then(|r| source.worktree(r))
            .unwrap_or(wt);
        let env = app.terminal_env(&m, pane, &source.id, &worktree.id);
        let get = |var: &str| {
            env.iter()
                .find(|(k, _)| k == var)
                .map(|(_, v)| PathBuf::from(v))
                .or_else(|| std::env::var_os(var).map(PathBuf::from))
        };
        let home = home_dir();
        let viewer = wt.tab(&loc.tab).and_then(|t| {
            t.panes().into_iter().find_map(|p| match &p.kind {
                rade_core::PaneKind::Viewer { path, .. } => Some((path.clone(), None)),
                _ => None,
            })
        });
        let input = context::GatherInput {
            dir: pane.cwd().clone(),
            agent: pane.agent_name().map(str::to_string),
            model: rade_core::chrome::pane_model(pane),
            session_id: pane.session_id.clone(),
            claude_config_dir: get("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home.join(".claude")),
            codex_home: get("CODEX_HOME").unwrap_or_else(|| home.join(".codex")),
            screen: app
                .terminals
                .borrow()
                .get(pane_id)
                .and_then(|t| t.handle.read_text(rade_ghostty_sys::GHOSTTY_POINT_SCREEN)),
            where_label: format!("{} / {}", source.name, worktree.name),
            viewer,
        };
        (
            input,
            env,
            app.config.borrow().voice.clone(),
            worktree.name.clone(),
            key_label(&app.config.borrow()),
        )
    };
    let session = next_id();
    let mut composer = Composer::new(&cfg, &label);
    composer.set_target(&target);
    with(|c| {
        kill_recorder(c);
        kill_agent(c);
        kill_curl(c);
        *c = Ctl {
            composer,
            cfg,
            pane: Some(pane_id.to_string()),
            session,
            dir: input.dir.clone(),
            env,
            ..Ctl::new()
        };
    });
    crate::voice_view::open(pane_id);
    // The context is built off the UI thread while the owner speaks (D8).
    std::thread::spawn(move || {
        let bundle = context::gather(&input);
        post(Msg::Context {
            session,
            text: context::render(&bundle),
            vocabulary: bundle.vocabulary,
        });
    });
    glib::timeout_add_local(Duration::from_millis(100), move || {
        if with(|c| c.session != session) {
            return glib::ControlFlow::Break;
        }
        tick();
        glib::ControlFlow::Continue
    });
    true
}

/// End the session: every process of ours stops, the widget goes, focus
/// returns to the Pane's Terminal.
fn close_session() {
    let pane = with(|c| {
        let pane = c.pane.take();
        c.session = next_id();
        kill_recorder(c);
        kill_agent(c);
        kill_curl(c);
        c.levels.clear();
        c.stt_busy = false;
        c.final_pending = false;
        pane
    });
    crate::voice_view::close();
    if let (Some(pane), Some(app)) = (pane, try_app()) {
        // Not under the `terminals` borrow: the focus-in handler syncs.
        let area = app
            .terminals
            .borrow()
            .get(&pane)
            .map(|t| t.handle.gl_area().clone());
        if let Some(area) = area {
            area.grab_focus();
        }
    }
}

// --------------------------------------------------------------------- events

fn normalize(ev: Event) -> Event {
    let at_ms = now_ms();
    match ev {
        Event::KeyDown { repeat, .. } => Event::KeyDown { repeat, at_ms },
        Event::KeyUp { .. } => Event::KeyUp { at_ms },
        Event::Escape { .. } => Event::Escape { at_ms },
        Event::Tick { .. } => Event::Tick { at_ms },
        e => e,
    }
}

fn feed(ev: Event) {
    let ev = normalize(ev);
    let actions = with(|c| {
        if let Event::DraftEdited(t) = &ev {
            if *t != c.seen_draft {
                c.edited = true;
            }
        }
        c.composer.on(ev)
    });
    apply(actions);
    sync_recorder();
}

/// The recorder runs only while listening.
fn sync_recorder() {
    with(|c| {
        if c.recorder.is_some()
            && c.composer.state() != Some(rade_core::voice::composer::State::Listening)
        {
            kill_recorder(c);
        }
    });
}

fn apply(actions: Vec<Action>) {
    for a in actions {
        match a {
            Action::Render => render(),
            Action::StartRecorder => start_recorder(),
            Action::StopRecorder { .. } => with(kill_recorder),
            Action::SttPartial => stt_request(Kind::Partial),
            Action::SttFinal => stt_request(Kind::Final),
            Action::SpawnAgent => spawn_agent(),
            Action::SendTurn(text) => send_turn(&text),
            Action::KillAgent => with(kill_agent),
            Action::SendToPane { enter, text } => send_to_pane(enter, &text),
            Action::Close => close_session(),
            // The widget focuses the draft when it becomes editable.
            Action::FocusDraft => {}
        }
    }
}

fn render() {
    let view = with(|c| {
        if !c.composer.is_open() {
            return None;
        }
        let v = c.composer.view();
        if v.draft_version != c.seen_version {
            c.seen_version = v.draft_version;
            c.seen_draft = v.draft.clone();
            c.edited = false;
        }
        Some(v)
    });
    if let Some(v) = view {
        crate::voice_view::render(&v);
    }
}

fn tick() {
    feed(Event::Tick { at_ms: 0 });
    let now = now_ms();
    let go = with(|c| {
        let live = c.cfg.live_preview
            && c.composer.state() == Some(rade_core::voice::composer::State::Listening)
            && c.recorder.is_some()
            && !c.stt_busy
            && now.saturating_sub(c.last_partial_ms) >= PARTIAL_EVERY_MS
            && c.pcm.lock().is_ok_and(|p| !p.is_empty());
        if live {
            c.last_partial_ms = now;
        }
        live
    });
    if go {
        stt_request(Kind::Partial);
    }
}

fn on_msg(m: Msg) {
    match m {
        Msg::Levels { rec, levels } => {
            let ring = with(|c| {
                if c.recorder.as_ref().map(|r| r.id) != Some(rec) {
                    return None;
                }
                for l in levels {
                    c.levels.push_back(l);
                    if c.levels.len() > METER_BARS {
                        c.levels.pop_front();
                    }
                }
                Some(c.levels.iter().copied().collect::<Vec<f32>>())
            });
            if let Some(ring) = ring {
                crate::voice_view::set_levels(&ring);
                feed(Event::Levels);
            }
        }
        Msg::RecEnded { rec, stderr } => {
            if with(|c| c.recorder.as_ref().map(|r| r.id) == Some(rec)) {
                with(|c| c.recorder = None);
                let why = if stderr.trim().is_empty() {
                    "the record command exited".to_string()
                } else {
                    stderr
                };
                feed(Event::RecordError(why));
            }
        }
        Msg::Stt {
            session,
            kind,
            result,
        } => {
            if with(|c| c.session != session) {
                return;
            }
            let pending = with(|c| {
                c.stt_busy = false;
                std::mem::take(&mut c.final_pending)
            });
            match (kind, result) {
                (Kind::Partial, Ok(t)) => feed(Event::Partial(t)),
                (Kind::Final, Ok(t)) => feed(Event::Final(t)),
                (_, Err(e)) => feed(Event::SttError(e)),
            }
            if pending && with(|c| c.session == session) {
                stt_request(Kind::Final);
            }
        }
        Msg::Agent { id, events } => {
            if with(|c| c.agent.as_ref().map(|a| a.id) != Some(id)) {
                return;
            }
            for e in events {
                feed(Event::AgentEvent(e));
            }
        }
        Msg::AgentExited { id, ok, stderr } => {
            if with(|c| c.agent.as_ref().map(|a| a.id) != Some(id)) {
                return;
            }
            with(|c| c.agent = None);
            feed(Event::AgentExited { ok, stderr });
        }
        Msg::Context {
            session,
            text,
            vocabulary,
        } => {
            if with(|c| c.session != session) {
                return;
            }
            let actions = with(|c| {
                if let Ok(mut v) = c.vocabulary.lock() {
                    *v = vocabulary;
                }
                c.composer.set_context(text)
            });
            apply(actions);
            sync_recorder();
        }
    }
}

// ------------------------------------------------------------------- recorder

fn kill_group(pgid: i32, mut child: Child) {
    rade_core::process::signal_group(pgid, rade_core::process::SIGKILL);
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

fn kill_recorder(c: &mut Ctl) {
    if let Some(r) = c.recorder.take() {
        kill_group(r.pgid, r.child);
    }
}

fn start_recorder() {
    use std::os::unix::process::CommandExt;
    let (cmd, cap) = with(|c| {
        kill_recorder(c);
        c.pcm = Arc::default();
        c.levels.clear();
        c.last_partial_ms = now_ms();
        (
            c.cfg.record_command.clone(),
            c.cfg.max_recording().as_secs() as usize * stt::SAMPLE_RATE as usize,
        )
    });
    let spawned = Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            feed(Event::RecordError(e.to_string()));
            return;
        }
    };
    let id = next_id();
    let pgid = child.id() as i32;
    let mut out = child.stdout.take().expect("piped");
    let mut err = child.stderr.take().expect("piped");
    let pcm = with(|c| c.pcm.clone());
    let errs = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut carry: Vec<u8> = Vec::new();
        let mut block: Vec<i16> = Vec::new();
        loop {
            let n = match out.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            };
            carry.extend_from_slice(&buf[..n]);
            let even = carry.len() & !1;
            let samples: Vec<i16> = carry[..even]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b))
                .collect();
            carry.drain(..even);
            if let Ok(mut p) = pcm.lock() {
                let room = cap.saturating_sub(p.len());
                p.extend_from_slice(&samples[..samples.len().min(room)]);
            }
            block.extend(samples);
            let mut levels = Vec::new();
            while block.len() >= LEVEL_SAMPLES {
                levels.push(stt::level(&block[..LEVEL_SAMPLES]));
                block.drain(..LEVEL_SAMPLES);
            }
            if !levels.is_empty() {
                post(Msg::Levels { rec: id, levels });
            }
        }
        let stderr = errs.join().unwrap_or_default();
        post(Msg::RecEnded { rec: id, stderr });
    });
    with(|c| c.recorder = Some(Recorder { id, pgid, child }));
}

// ----------------------------------------------------------- speech-to-text

fn kill_curl(c: &mut Ctl) {
    let g = c.curl.swap(0, Ordering::SeqCst);
    if g > 0 {
        rade_core::process::signal_group(g, rade_core::process::SIGKILL);
    }
}

fn stt_request(kind: Kind) {
    let job = with(|c| {
        if !c.composer.is_open() {
            return None;
        }
        if c.stt_busy {
            // One request at a time: the final waits for the live one.
            if kind == Kind::Final {
                c.final_pending = true;
            }
            return None;
        }
        c.stt_busy = true;
        c.stt_requests += 1;
        let pcm = c.pcm.lock().map(|p| p.clone()).unwrap_or_default();
        Some((
            c.session,
            c.cfg.clone(),
            pcm,
            c.vocabulary.lock().map(|v| v.clone()).unwrap_or_default(),
            c.key.clone(),
            c.curl.clone(),
        ))
    });
    let Some((session, cfg, pcm, vocabulary, key, curl)) = job else {
        return;
    };
    std::thread::spawn(move || {
        let result = transcribe(&cfg, &pcm, &vocabulary, &key, &curl);
        post(Msg::Stt {
            session,
            kind,
            result,
        });
    });
}

/// `stt_key_command`'s stdout, trimmed, once per session; `None` = unset or
/// failed (the request goes without a key).
fn api_key(cfg: &VoiceConfig, cache: &Mutex<Option<Option<String>>>) -> Option<String> {
    let cmd = cfg.stt_key_command.trim();
    if cmd.is_empty() {
        return None;
    }
    let mut cache = cache.lock().ok()?;
    cache
        .get_or_insert_with(|| {
            let out = rade_core::shell_env::bounded_output(
                Command::new("sh").arg("-c").arg(cmd),
                KEY_TIMEOUT,
            )
            .ok()?;
            let k = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (out.status.success() && !k.is_empty()).then_some(k)
        })
        .clone()
}

/// One transcription request. The WAV goes to curl's stdin, the URL, the
/// form and the key to a pipe curl reads as `-K /dev/fd/3`: never argv, env
/// or a log. The pipe is curl's inherited stderr, moved to fd 3 by a one-line
/// `sh` that sends curl's own messages to its stdout.
fn transcribe(
    cfg: &VoiceConfig,
    pcm: &[i16],
    vocabulary: &str,
    key: &Mutex<Option<Option<String>>>,
    curl: &AtomicI32,
) -> std::result::Result<String, String> {
    use std::os::unix::process::CommandExt;
    let key = api_key(cfg, key);
    let config = stt::curl_config(cfg, key.as_deref(), vocabulary);
    let wav = stt::wav(pcm);
    let (reader, mut writer) =
        std::io::pipe().map_err(|e| format!("Speech-to-text failed: {e}"))?;
    let mut child = Command::new("sh")
        .args(["-c", "exec 3<&2 2>&1; exec \"$@\"", "sh"])
        .args(stt::curl_argv())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(reader))
        .process_group(0)
        .spawn()
        .map_err(|e| format!("Speech-to-text failed: {e}"))?;
    curl.store(child.id() as i32, Ordering::SeqCst);
    let _ = writer.write_all(config.as_bytes());
    drop(writer);
    let mut stdin = child.stdin.take().expect("piped");
    let feeder = std::thread::spawn(move || {
        let _ = stdin.write_all(&wav);
    });
    let mut out = String::new();
    let mut stdout = child.stdout.take().expect("piped");
    let mut raw = Vec::new();
    let _ = stdout.read_to_end(&mut raw);
    out.push_str(&String::from_utf8_lossy(&raw));
    let status = child.wait();
    curl.store(0, Ordering::SeqCst);
    let _ = feeder.join();
    match status {
        Ok(s) if s.success() => stt::parse(&out),
        Ok(s) => Err(stt::error_text(&cfg.stt_url, s.code(), &out)),
        Err(e) => Err(format!("Speech-to-text failed: {e}")),
    }
}

// ---------------------------------------------------------------- voice agent

fn kill_agent(c: &mut Ctl) {
    if let Some(a) = c.agent.take() {
        // The reader thread owns and reaps the child.
        rade_core::process::signal_group(a.pgid, rade_core::process::SIGKILL);
    }
}

fn spawn_agent() {
    use std::os::unix::process::CommandExt;
    let (argv, dir, env, codex) = with(|c| {
        kill_agent(c);
        (
            agent::command(&c.cfg),
            c.dir.clone(),
            agent::env_for(&c.env),
            c.cfg.agent == VoiceAgent::Codex,
        )
    });
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .current_dir(&dir)
        .envs(env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    // Nothing of the Pane's RADE_* reaches it, whatever the app inherited.
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("RADE_") {
            cmd.env_remove(k);
        }
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            feed(Event::AgentStartFailed(e.to_string()));
            return;
        }
    };
    let id = next_id();
    let pgid = child.id() as i32;
    let mut stdin = child.stdin.take().expect("piped");
    let stdout = child.stdout.take().expect("piped");
    let mut err = child.stderr.take().expect("piped");
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        while let Ok(text) = rx.recv() {
            if stdin.write_all(text.as_bytes()).is_err() || stdin.flush().is_err() {
                break;
            }
        }
    });
    let errs = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    std::thread::spawn(move || {
        let mut last: Option<String> = None;
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let events = if codex {
                agent::parse_codex_line(&line, &mut last)
            } else {
                agent::parse_claude_line(&line, &dir)
            };
            if !events.is_empty() {
                post(Msg::Agent { id, events });
            }
        }
        let stderr = errs.join().unwrap_or_default();
        let ok = child.wait().is_ok_and(|s| s.success());
        post(Msg::AgentExited { id, ok, stderr });
    });
    with(|c| {
        c.agent = Some(AgentProc {
            id,
            pgid,
            tx: Some(tx),
            turns: 0,
            argv,
        })
    });
}

fn send_turn(text: &str) {
    with(|c| {
        let codex = c.cfg.agent == VoiceAgent::Codex;
        let Some(a) = c.agent.as_mut() else { return };
        a.turns += 1;
        if codex {
            // The whole stdin, then EOF.
            if let Some(tx) = a.tx.take() {
                let _ = tx.send(text.to_string());
            }
        } else if let Some(tx) = &a.tx {
            let _ = tx.send(agent::claude_stdin_line(text) + "\n");
        }
    });
}

fn send_to_pane(enter: bool, text: &str) {
    let Some(pane) = with(|c| c.pane.clone()) else {
        return;
    };
    let app = app();
    if let Err(e) = crate::handlers::send_text(&app, &pane, text, true, enter) {
        toast(&e.message);
    }
}

// ---------------------------------------------------------------------- debug

/// `debug.voice` (PLAN §6, test mode): `info`, `toggle`, `send`, `paste`,
/// `escape`, `type`, `heard_toggle`, `setup_link`, `scene`.
pub fn debug(p: &Value) -> Result<Value> {
    let s = |k: &str| p.get(k).and_then(Value::as_str);
    match s("op").unwrap_or("info") {
        "info" => Ok(info()),
        "toggle" => {
            let pane = match s("pane") {
                Some(q) => app().model.borrow().resolve_pane(q, None)?,
                None => with(|c| c.pane.clone())
                    .or_else(focused_pane)
                    .ok_or_else(|| Error::not_found("no pane"))?,
            };
            tap(&pane);
            Ok(json!({}))
        }
        "send" => {
            feed(Event::Send);
            Ok(json!({}))
        }
        "paste" => {
            feed(Event::Paste);
            Ok(json!({}))
        }
        "escape" => {
            feed(Event::Escape { at_ms: 0 });
            Ok(json!({}))
        }
        "type" => {
            let text = s("text").ok_or_else(|| Error::usage("type: needs text"))?;
            if !with(|c| c.composer.is_open()) {
                return Err(Error::usage("the Voice Composer is not open"));
            }
            let new = with(|c| format!("{}{}", c.composer.draft(), text));
            feed(Event::DraftEdited(new));
            Ok(json!({}))
        }
        "heard_toggle" => Err(Error::internal(
            "heard_toggle: needs the widget's show / hide (voice_view), not wired in this build",
        )),
        "setup_link" => {
            crate::settings_view::open_filtered("voice");
            Ok(json!({}))
        }
        "scene" => crate::voice_view::debug_scene(s("arg").unwrap_or("")),
        other => Err(Error::usage(format!("debug.voice: unknown op '{other}'"))),
    }
}

fn pairs(r: &[std::ops::Range<usize>]) -> Value {
    Value::Array(r.iter().map(|r| json!([r.start, r.end])).collect())
}

fn info() -> Value {
    let (v, pane, edited, agent, recorder, requests) = with(|c| {
        (
            c.composer.view(),
            c.pane.clone(),
            c.edited,
            c.agent
                .as_ref()
                .map(|a| json!({"pid": a.pgid, "turns": a.turns, "argv": a.argv})),
            c.recorder.as_ref().map(|r| {
                json!({"pid": r.pgid,
                    "bytes": c.pcm.lock().map_or(0, |p| p.len() * 2)})
            }),
            c.stt_requests,
        )
    });
    let mut o = json!({
        "open": v.open,
        "pane": pane.as_deref().map(pane_label),
        "state": v.state.map(|s| s.as_str()),
        "mode": v.mode.map(|m| match m { Mode::Hold => "hold", Mode::Toggle => "toggle" }),
        "revision": v.revision,
        "label": v.label,
        "time": v.time,
        "heard": v.heard,
        "tentative": v.tentative,
        "heard_folded": v.heard_folded,
        "draft": v.draft,
        "draft_edited": edited,
        "draft_tags": {"code": [], "changed": pairs(&v.changed), "unsure": []},
        "activity": v.activity,
        "step": v.step,
        "error": v.error,
        "hints": v.hints,
        "send_enabled": v.send_enabled,
        "paste_enabled": v.paste_enabled,
        "agent": agent,
        "recorder": recorder,
        "stt_requests": requests,
    });
    // The widget's own facts win: what is folded, tagged and shown.
    if let (Some(base), Value::Object(w)) = (o.as_object_mut(), crate::voice_view::debug_info()) {
        for (k, val) in w {
            if k == "draft_tags" {
                if let (Some(Value::Object(t)), Value::Object(wt)) = (base.get_mut(&k), val) {
                    t.extend(wt);
                }
            } else {
                base.insert(k, val);
            }
        }
    }
    o
}
