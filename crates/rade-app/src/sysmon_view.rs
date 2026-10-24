//! The system monitor (R116): a Statusline cell `cpu 34% · mem 61% · gpu
//! 12% · ↓1.2M ↑312K` and its popover (totals, load, the busiest Panes).
//!
//! Cost: one worker thread samples `/proc` (and the GPU) every 2 s while
//! the window is active, every 5 s while it is not, and not at all while it
//! is minimized, the Statusline is hidden or `system_monitor = false`. The
//! per-Pane scan of every process runs only while the popover is open. The
//! UI thread only sets a label when its text changed; the cell is one
//! persistent widget, so a tick never rebuilds the Statusline.

use crate::app::try_app;
use gtk::glib;
use gtk4 as gtk;
use gtk4::prelude::*;
use rade_core::sysmon::{self, Heat, Level, Roots, Sample};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Sampling mode the UI asks for.
const OFF: u8 = 0;
const FAST: u8 = 1;
const SLOW: u8 = 2;

/// What the UI and the worker share.
#[derive(Default)]
struct Shared {
    mode: AtomicU8,
    popover: AtomicBool,
    /// (Pane, its process = session id) while the popover is open.
    panes: Mutex<Vec<(String, i32)>>,
    samples: AtomicU64,
    /// The last tick's work on the worker, in µs.
    last_us: AtomicU64,
}

/// What a tick sends to the UI.
#[derive(Clone, Debug, Default)]
struct Snapshot {
    sample: Sample,
    levels: (Level, Level, Level),
    cores: usize,
    load: Option<[f64; 3]>,
    swap: Option<(u64, u64)>,
    mem_kb: (u64, u64),
    gpu_name: Option<String>,
    ifaces: Vec<(String, f64, f64)>,
    /// (Pane, CPU % of a core, RSS bytes), busiest first.
    panes: Vec<(String, f64, u64)>,
    /// Outside RADE's Panes: (CPU % of the machine, RSS bytes).
    outside: Option<(f64, u64)>,
}

/// One figure of the cell: its icon and value (R116.2).
struct Figure {
    root: gtk::Box,
    icon: gtk::Image,
    value: gtk::Label,
}

impl Figure {
    fn new(icon: &str, tooltip: &str) -> Self {
        let i = gtk::Image::from_icon_name(icon);
        i.add_css_class("sysmon-icon");
        let value = gtk::Label::new(Some("  –%"));
        value.add_css_class("sysmon-text");
        value.add_css_class("sysmon-value");
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        root.add_css_class("sysmon-figure");
        root.append(&i);
        root.append(&value);
        root.set_tooltip_text(Some(tooltip));
        Self {
            root,
            icon: i,
            value,
        }
    }

    /// Its value and loudness (`wait` / `err` on icon and value).
    fn set(&self, text: &str, level: Level) {
        if self.value.label() != text {
            self.value.set_label(text);
        }
        for c in ["wait", "err"] {
            let on = level_class(level) == Some(c);
            for w in [
                self.icon.upcast_ref::<gtk::Widget>(),
                self.value.upcast_ref(),
            ] {
                if on {
                    w.add_css_class(c);
                } else {
                    w.remove_css_class(c);
                }
            }
        }
    }
}

struct Monitor {
    root: gtk::Box,
    /// CPU, memory, GPU, download, upload.
    figures: [Figure; 5],
    /// The cell as text (`debug.ui monitor`, sysmon.sh): `cpu  34% · …`.
    text: RefCell<String>,
    popover: gtk::Popover,
    body: gtk::Box,
    shared: Arc<Shared>,
    started: std::cell::Cell<bool>,
    last: RefCell<Option<Snapshot>>,
    ui_us: std::cell::Cell<u64>,
}

thread_local! {
    static MON: RefCell<Option<Monitor>> = const { RefCell::new(None) };
}

/// `RADE_SYSTEM_MONITOR=0` (the E2E harness) turns it off whatever the config.
fn env_off() -> bool {
    std::env::var("RADE_SYSTEM_MONITOR").is_ok_and(|v| v == "0")
}

/// The monitor is on (config and environment).
pub fn enabled() -> bool {
    !env_off() && try_app().is_none_or(|a| a.config.borrow().system_monitor)
}

/// The Statusline's monitor cell (built once).
pub fn widget() -> gtk::Widget {
    MON.with(|m| {
        if let Some(m) = m.borrow().as_ref() {
            return m.root.clone().upcast();
        }
        // Icons instead of words (owner, 2026-10-24): CPU, memory, GPU,
        // down / up; the figures in mono with tabular digits.
        let figures = [
            Figure::new("ph-cpu-symbolic", "CPU"),
            Figure::new("ph-memory-symbolic", "Memory"),
            Figure::new("ph-graphics-card-symbolic", "GPU"),
            Figure::new("ph-arrow-down-symbolic", "Download (physical interfaces)"),
            Figure::new("ph-arrow-up-symbolic", "Upload (physical interfaces)"),
        ];
        figures[2].root.set_visible(false);
        let inner = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        for f in &figures {
            inner.append(&f.root);
        }
        let button = gtk::Button::new();
        button.set_child(Some(&inner));
        button.add_css_class("flat");
        button.add_css_class("status-sysmon");
        button.set_focus_on_click(false);
        button.set_tooltip_text(Some(
            "CPU, memory, GPU and network (bytes/s on physical interfaces) — click for the busiest Panes",
        ));
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("status-cell");
        root.add_css_class("status-cell-monitor");
        root.set_widget_name("status-monitor");
        root.append(&button);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.add_css_class("sysmon-body");
        let popover = gtk::Popover::new();
        popover.add_css_class("sysmon-popover");
        popover.set_child(Some(&body));
        popover.set_parent(&button);
        popover.set_has_arrow(false);
        popover.set_position(gtk::PositionType::Top);
        button.connect_clicked(|_| open_popover());
        popover.connect_closed(|_| {
            MON.with(|m| {
                if let Some(m) = m.borrow().as_ref() {
                    m.shared.popover.store(false, Ordering::Relaxed);
                }
            })
        });
        // After the signal: `is_mapped` has its new value then.
        root.connect_map(|_| {
            glib::idle_add_local_once(refresh_mode);
        });
        root.connect_unmap(|_| {
            glib::idle_add_local_once(refresh_mode);
        });
        let mon = Monitor {
            root: root.clone(),
            figures,
            text: RefCell::default(),
            popover,
            body,
            shared: Arc::default(),
            started: std::cell::Cell::new(false),
            last: RefCell::default(),
            ui_us: std::cell::Cell::new(0),
        };
        *m.borrow_mut() = Some(mon);
        root.set_visible(enabled());
        root.upcast()
    })
}

/// Follow the window: call on activity / minimize / config changes.
pub fn watch(window: &impl IsA<gtk::Window>) {
    let w: &gtk::Window = window.upcast_ref();
    w.connect_is_active_notify(|_| refresh_mode());
    w.connect_suspended_notify(|_| refresh_mode());
    w.upcast_ref::<gtk::Widget>()
        .connect_visible_notify(|_| refresh_mode());
}

/// Pick the sampling mode from the window and the config; start the
/// worker the first time it is needed.
pub fn refresh_mode() {
    let Some(app) = try_app() else { return };
    let on = enabled();
    let win = &app.window;
    MON.with(|m| {
        let m = m.borrow();
        let Some(m) = m.as_ref() else { return };
        m.root.set_visible(on);
        let mode = if !on || !m.root.is_mapped() || win.is_suspended() || !win.is_visible() {
            OFF
        } else if win.is_active() || m.shared.popover.load(Ordering::Relaxed) {
            FAST
        } else {
            SLOW
        };
        m.shared.mode.store(mode, Ordering::Relaxed);
        if mode != OFF && !m.started.replace(true) {
            start(m.shared.clone());
        }
    });
}

fn start(shared: Arc<Shared>) {
    let roots = Roots::from_env(&|k| std::env::var(k).ok());
    let _ = std::thread::Builder::new()
        .name("rade-sysmon".into())
        .spawn(move || worker(shared, roots));
}

/// The sampler: owns its buffers and the previous counters.
fn worker(shared: Arc<Shared>, roots: Roots) {
    let (hz, page) = sysmon::clock_and_page();
    let mut heat = Heat::default();
    let mut prev_cpu = None;
    let mut prev_net: HashMap<String, (u64, u64)> = HashMap::new();
    let mut prev_at = Instant::now();
    let mut ifaces: Vec<String> = Vec::new();
    let mut ifaces_at: Option<Instant> = None;
    let mut gpu = Gpu::default();
    let mut prev_sessions: Option<HashMap<i32, (u64, u64)>> = None;
    let mut prev_total_ticks: Option<u64> = None;
    let mut buf = String::new();
    let read = |p: &std::path::Path, buf: &mut String| -> bool {
        buf.clear();
        std::fs::File::open(p)
            .and_then(|mut f| std::io::Read::read_to_string(&mut f, buf))
            .is_ok()
    };
    loop {
        let mode = shared.mode.load(Ordering::Relaxed);
        if mode == OFF {
            // Forget deltas: the next sample after a pause starts fresh.
            prev_cpu = None;
            prev_sessions = None;
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }
        let t0 = Instant::now();
        let secs = t0.duration_since(prev_at).as_secs_f64();
        prev_at = t0;
        let mut snap = Snapshot::default();
        if read(&roots.proc.join("stat"), &mut buf) {
            if let Some((t, cores)) = sysmon::parse_stat(&buf) {
                snap.cores = cores;
                if let Some(p) = prev_cpu {
                    snap.sample.cpu = sysmon::cpu_percent(p, t);
                }
                prev_cpu = Some(t);
            }
        }
        if read(&roots.proc.join("meminfo"), &mut buf) {
            if let Some(m) = sysmon::parse_meminfo(&buf) {
                snap.sample.mem = m.percent();
                snap.mem_kb = (m.total - m.available.min(m.total), m.total);
                snap.swap = (m.swap_total > 0)
                    .then(|| (m.swap_total - m.swap_free.min(m.swap_total), m.swap_total));
            }
        }
        if ifaces_at.is_none_or(|a| a.elapsed() > Duration::from_secs(60)) {
            ifaces = sysmon::physical_ifaces(&roots.sys);
            ifaces_at = Some(Instant::now());
        }
        if read(&roots.proc.join("net/dev"), &mut buf) {
            for (name, rx, tx) in sysmon::parse_net_dev(&buf) {
                if !ifaces.contains(&name) {
                    continue;
                }
                let (r, t) = match prev_net.insert(name.clone(), (rx, tx)) {
                    Some((pr, pt)) if secs > 0.0 => (
                        rx.saturating_sub(pr) as f64 / secs,
                        tx.saturating_sub(pt) as f64 / secs,
                    ),
                    _ => (0.0, 0.0),
                };
                snap.sample.rx += r;
                snap.sample.tx += t;
                snap.ifaces.push((name, r, t));
            }
        }
        let (busy, name) = gpu.sample(&roots);
        snap.sample.gpu = busy;
        snap.gpu_name = name;
        snap.levels = heat.update(&snap.sample);
        if shared.popover.load(Ordering::Relaxed) {
            if read(&roots.proc.join("loadavg"), &mut buf) {
                snap.load = sysmon::parse_loadavg(&buf);
            }
            let (now, all) = sysmon::sessions(&roots.proc);
            let panes = shared.panes.lock().map(|p| p.clone()).unwrap_or_default();
            if let Some(before) = &prev_sessions {
                let rows = sysmon::pane_loads(&panes, before, &now, hz, secs, page);
                let mine_cpu: f64 = rows.iter().map(|r| r.cpu).sum();
                let mine_rss: u64 = rows.iter().map(|r| r.rss_bytes).sum();
                // Everything else: all processes' ticks minus the Panes'.
                if let Some(pt) = prev_total_ticks {
                    let all_pct = 100.0 * all.0.saturating_sub(pt) as f64 / hz / secs.max(0.001);
                    let machine = (all_pct - mine_cpu).max(0.0) / snap.cores.max(1) as f64;
                    snap.outside = Some((machine, (all.1 * page).saturating_sub(mine_rss)));
                }
                snap.panes = rows
                    .into_iter()
                    .take(5)
                    .map(|r| (r.pane, r.cpu, r.rss_bytes))
                    .collect();
            }
            prev_total_ticks = Some(all.0);
            prev_sessions = Some(now);
        } else {
            prev_sessions = None;
            prev_total_ticks = None;
        }
        shared.samples.fetch_add(1, Ordering::Relaxed);
        shared
            .last_us
            .store(t0.elapsed().as_micros() as u64, Ordering::Relaxed);
        glib::MainContext::default().invoke(move || apply(snap));
        let wait = if mode == FAST { 2000 } else { 5000 };
        // Wake early when the mode changes (the popover opened).
        let until = Instant::now() + Duration::from_millis(wait);
        while Instant::now() < until {
            std::thread::sleep(Duration::from_millis(100));
            if shared.mode.load(Ordering::Relaxed) != mode
                || (shared.popover.load(Ordering::Relaxed) && prev_sessions.is_none())
            {
                break;
            }
        }
    }
}

/// GPU sampling: AMD's sysfs every tick; NVIDIA through `nvidia-smi` at
/// most every 10 s and only while the card is awake (never wakes it).
#[derive(Default)]
struct Gpu {
    nv_at: Option<Instant>,
    nv: Option<(f64, String)>,
}

impl Gpu {
    fn sample(&mut self, roots: &Roots) -> (Option<f64>, Option<String>) {
        if let Some(b) = sysmon::sysfs_gpu_busy(&roots.sys) {
            return (Some(b), None);
        }
        if !roots.real() || !std::path::Path::new("/proc/driver/nvidia").exists() {
            return (None, None);
        }
        if self
            .nv_at
            .is_none_or(|a| a.elapsed() > Duration::from_secs(10))
        {
            self.nv_at = Some(Instant::now());
            self.nv = if nvidia_awake() { nvidia_smi() } else { None };
        }
        match &self.nv {
            Some((b, n)) => (Some(*b), Some(n.clone())),
            None => (None, None),
        }
    }
}

/// An NVIDIA card is not runtime-suspended (querying would wake it).
fn nvidia_awake() -> bool {
    std::fs::read_dir("/sys/bus/pci/drivers/nvidia")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(':'))
        .any(|e| {
            std::fs::read_to_string(e.path().join("power/runtime_status"))
                .map_or(true, |s| s.trim() != "suspended")
        })
}

fn nvidia_smi() -> Option<(f64, String)> {
    let mut c = std::process::Command::new("nvidia-smi");
    c.args([
        "--query-gpu=utilization.gpu,name",
        "--format=csv,noheader,nounits",
    ]);
    let out = rade_core::shell_env::bounded_output(&mut c, Duration::from_secs(2)).ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next()?;
    let (busy, name) = line.split_once(',')?;
    Some((busy.trim().parse().ok()?, name.trim().to_string()))
}

fn level_class(l: Level) -> Option<&'static str> {
    match l {
        Level::Normal => None,
        Level::High => Some("wait"),
        Level::Critical => Some("err"),
    }
}

/// A tick's result on the UI thread: the label only when its text changed.
fn apply(snap: Snapshot) {
    let t = Instant::now();
    MON.with(|m| {
        let m = m.borrow();
        let Some(m) = m.as_ref() else { return };
        // Icons fg3, figures fg2; only a high figure (its icon and value)
        // takes wait / err (R116.3), so the cell never turns loud as a whole.
        let s = &snap.sample;
        let pct = |v: f64| format!("{:>3}%", v.round().clamp(0.0, 100.0) as u64);
        let [cpu, mem, gpu, down, up] = &m.figures;
        cpu.set(&pct(s.cpu), snap.levels.0);
        mem.set(&pct(s.mem), snap.levels.1);
        gpu.root.set_visible(s.gpu.is_some());
        if let Some(g) = s.gpu {
            gpu.set(&pct(g), snap.levels.2);
        }
        down.set(&sysmon::fmt_rate(s.rx), Level::Normal);
        up.set(&sysmon::fmt_rate(s.tx), Level::Normal);
        let text = sysmon::cell_text(s);
        if *m.text.borrow() != text {
            *m.text.borrow_mut() = text;
        }
        let worst = [snap.levels.0, snap.levels.1, snap.levels.2]
            .into_iter()
            .max_by_key(|l| *l as u8)
            .unwrap_or_default();
        for c in ["wait", "err"] {
            if Some(c) == level_class(worst) {
                m.root.add_css_class(c);
            } else {
                m.root.remove_css_class(c);
            }
        }
        if m.popover.is_visible() {
            fill_popover(m, &snap);
        }
        *m.last.borrow_mut() = Some(snap);
        m.ui_us.set(t.elapsed().as_micros() as u64);
    });
}

/// Open the popover: the Panes' processes are looked up now (their pids
/// stay until the popover closes) and the worker starts scanning them.
pub fn open_popover() {
    let Some(app) = try_app() else { return };
    let ids: Vec<String> = app
        .model
        .borrow()
        .worktrees()
        .flat_map(|(_, w)| w.tabs.iter().flat_map(|t| t.pane_ids()))
        .collect();
    let panes: Vec<(String, i32)> = ids
        .into_iter()
        .filter_map(|p| {
            let pid = *rade_core::process::pane_groups(&p).first()?;
            Some((p, pid))
        })
        .collect();
    MON.with(|m| {
        let m = m.borrow();
        let Some(m) = m.as_ref() else { return };
        if let Ok(mut p) = m.shared.panes.lock() {
            *p = panes;
        }
        m.shared.popover.store(true, Ordering::Relaxed);
        if let Some(s) = m.last.borrow().clone() {
            fill_popover(m, &s);
        }
        m.popover.popup();
    });
    refresh_mode();
}

fn mono(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.add_css_class("sysmon-mono");
    for c in classes {
        l.add_css_class(c);
    }
    l
}

fn row(label: &str, value: &str, level: Level) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    b.add_css_class("sysmon-row");
    let k = mono(label, &["sysmon-key"]);
    k.set_width_chars(5);
    b.append(&k);
    let v = mono(value, &["sysmon-value"]);
    v.set_hexpand(true);
    if let Some(c) = level_class(level) {
        v.add_css_class(c);
    }
    b.append(&v);
    b
}

fn fill_popover(m: &Monitor, s: &Snapshot) {
    while let Some(c) = m.body.first_child() {
        m.body.remove(&c);
    }
    let mut head = format!("SYSTEM · {} cores", s.cores);
    if let Some(l) = s.load {
        head += &format!(" · load {:.1} {:.1} {:.1}", l[0], l[1], l[2]);
    }
    m.body.append(&mono(&head, &["sysmon-head"]));
    let pct = |v: f64| format!("{:.0}%", v);
    m.body.append(&row("cpu", &pct(s.sample.cpu), s.levels.0));
    let mut mem = format!(
        "{}  {} / {}",
        pct(s.sample.mem),
        sysmon::fmt_bytes(s.mem_kb.0 * 1024),
        sysmon::fmt_bytes(s.mem_kb.1 * 1024)
    );
    if let Some((u, t)) = s.swap {
        mem += &format!(
            " · swap {} / {}",
            sysmon::fmt_bytes(u * 1024),
            sysmon::fmt_bytes(t * 1024)
        );
    }
    m.body.append(&row("mem", &mem, s.levels.1));
    if let Some(g) = s.sample.gpu {
        let name = s.gpu_name.clone().unwrap_or_default();
        m.body
            .append(&row("gpu", &format!("{}  {name}", pct(g)), s.levels.2));
    }
    for (name, rx, tx) in &s.ifaces {
        m.body.append(&row(
            "net",
            &format!(
                "↓{} ↑{}  {name}",
                sysmon::fmt_rate(*rx),
                sysmon::fmt_rate(*tx)
            ),
            Level::Normal,
        ));
    }
    m.body
        .append(&mono("BY PANE", &["sysmon-head", "sysmon-section"]));
    let Some(app) = try_app() else { return };
    let model = app.model.borrow();
    if s.panes.is_empty() {
        m.body.append(&mono("measuring…", &["sysmon-dim"]));
    }
    for (pane, cpu, rss) in &s.panes {
        let Some(p) = model.pane(pane) else { continue };
        let name = p
            .name
            .clone()
            .or_else(|| p.agent.clone())
            .unwrap_or_else(|| "Terminal".into());
        let inner = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        // An Agent's glyph in its status colour (R115); a shell's LED.
        match p.mark() {
            Some(_) => inner.append(&crate::agent_glyph::Glyph::of_pane(p).root),
            None => {
                let led = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                led.add_css_class("led");
                led.add_css_class(&format!("led-{}", p.status.map_or("idle", |s| s.as_str())));
                led.set_valign(gtk::Align::Center);
                inner.append(&led);
            }
        }
        let n = mono(&name, &[]);
        n.set_hexpand(true);
        n.set_ellipsize(gtk::pango::EllipsizeMode::End);
        n.set_max_width_chars(18);
        inner.append(&n);
        inner.append(&mono(&format!("{:>4.0}%", cpu), &["sysmon-value"]));
        inner.append(&mono(
            &format!("{:>5}", sysmon::fmt_bytes(*rss)),
            &["sysmon-value"],
        ));
        let b = gtk::Button::new();
        b.set_child(Some(&inner));
        b.add_css_class("flat");
        b.add_css_class("sysmon-pane");
        b.set_widget_name(&format!("sysmon-pane-{pane}"));
        let id = pane.clone();
        let pop = m.popover.clone();
        b.connect_clicked(move |_| {
            pop.popdown();
            if let Some(app) = try_app() {
                let _ = app.update(|m| m.focus_pane(&id));
            }
        });
        m.body.append(&b);
    }
    if let Some((cpu, rss)) = s.outside {
        m.body.append(&mono(
            &format!("outside RADE  {:.0}% · {}", cpu, sysmon::fmt_bytes(rss)),
            &["sysmon-dim"],
        ));
    }
}

/// `debug.ui monitor` (`arg` = open | close | click_first; `path` = save
/// the popover as PNG): the cell, the mode, counters
/// and timings, the popover's rows.
pub fn debug(arg: Option<&str>, path: Option<&str>) -> Value {
    if let Some(p) = path {
        MON.with(|m| {
            if let Some(m) = m.borrow().as_ref() {
                let _ = crate::debug::screenshot_widget(
                    m.popover.upcast_ref(),
                    std::path::Path::new(p),
                );
            }
        });
    }
    match arg {
        // Headless Weston has no seat for a popup's grab: open it without.
        Some("open") => {
            MON.with(|m| {
                if let Some(m) = m.borrow().as_ref() {
                    m.popover.set_autohide(false);
                }
            });
            open_popover();
        }
        Some("close") => MON.with(|m| {
            if let Some(m) = m.borrow().as_ref() {
                m.popover.popdown();
            }
        }),
        Some("click_first") => MON.with(|m| {
            let m = m.borrow();
            let Some(m) = m.as_ref() else { return };
            let mut c = m.body.first_child();
            while let Some(w) = c {
                if let Some(b) = w.downcast_ref::<gtk::Button>() {
                    b.emit_clicked();
                    break;
                }
                c = w.next_sibling();
            }
        }),
        _ => {}
    }
    MON.with(|m| {
        let m = m.borrow();
        let Some(m) = m.as_ref() else {
            return json!({"built": false});
        };
        let rows: Vec<String> = {
            let mut v = Vec::new();
            let mut c = m.body.first_child();
            while let Some(w) = c {
                v.push(text_of(&w));
                c = w.next_sibling();
            }
            v
        };
        let mode = match m.shared.mode.load(Ordering::Relaxed) {
            OFF => "off",
            FAST => "fast",
            _ => "slow",
        };
        let classes: Vec<String> = m.root.css_classes().iter().map(|c| c.to_string()).collect();
        json!({
            "built": true,
            "enabled": enabled(),
            "visible": m.root.is_visible(),
            "mapped": m.root.is_mapped(),
            "text": m.text.borrow().clone(),
            "icons": m.figures.iter().filter(|f| f.root.is_visible()).filter_map(|f| f.icon.icon_name().map(|n| n.to_string())).collect::<Vec<_>>(),
            "classes": classes,
            "mode": mode,
            "samples": m.shared.samples.load(Ordering::Relaxed),
            "worker_us": m.shared.last_us.load(Ordering::Relaxed),
            "ui_us": m.ui_us.get(),
            "popover": m.popover.is_visible(),
            "rows": rows,
        })
    })
}

/// A widget's labels, joined by spaces (debug rows).
fn text_of(w: &gtk::Widget) -> String {
    if let Some(l) = w.downcast_ref::<gtk::Label>() {
        return l.text().trim().to_string();
    }
    let mut parts = Vec::new();
    let mut c = w.first_child();
    while let Some(k) = c {
        let t = text_of(&k);
        if !t.is_empty() {
            parts.push(t);
        }
        c = k.next_sibling();
    }
    parts.join(" ")
}
