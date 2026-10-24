//! Agent Status in the app (PRD R10.2, R10.3, R16.4): status dots and their
//! tooltips, the Terminal signals that feed the fallback rules
//! (`rade_core::status`), the output-idle watcher, and the one-time "Codex
//! hooks not trusted" banner.

use crate::app::try_app;
use crate::panes::now;
use adw::prelude::*;
use gtk::glib;
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::status::{summary, worst_pane, Signal};
use rade_core::{AgentStatus, Pane};
use rade_ghostty_sys::GHOSTTY_POINT_VIEWPORT;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

const CODEX_HINT: &str = "Codex hooks not trusted — run /hooks in codex and trust the RADE entries";

struct Watch {
    /// Per running Agent Pane: hash of its screen and when it last changed.
    screens: RefCell<HashMap<String, (u64, Instant)>>,
    banner: adw::Banner,
    hint_shown: Cell<bool>,
}

thread_local! {
    static WATCH: RefCell<Option<std::rc::Rc<Watch>>> = const { RefCell::new(None) };
}

fn watch() -> Option<std::rc::Rc<Watch>> {
    WATCH.with(|w| w.borrow().clone())
}

/// An Agent Status glyph (R20.2): shape and color — a spinner while
/// running, else a symbolic disc with a mark, a hollow ring for no status.
/// The same widget everywhere: captions, sidebar rows, Inbox, Status Bar.
#[derive(Clone)]
pub struct Glyph {
    pub root: gtk::Box,
    shown: std::rc::Rc<Cell<Option<Option<AgentStatus>>>>,
}

impl Glyph {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.set_valign(gtk::Align::Center);
        root.add_css_class("rade-glyph");
        let g = Self {
            root,
            shown: Default::default(),
        };
        g.set(None);
        g
    }

    pub fn of(s: Option<AgentStatus>) -> Self {
        let g = Self::new();
        g.set(s);
        g
    }

    pub fn set(&self, s: Option<AgentStatus>) {
        if self.shown.replace(Some(s)) == Some(s) {
            return;
        }
        while let Some(c) = self.root.first_child() {
            self.root.remove(&c);
        }
        let name = s.map_or("none", AgentStatus::as_str);
        let w: gtk::Widget = if s == Some(AgentStatus::Running) {
            let sp = adw::Spinner::new();
            sp.set_size_request(14, 14);
            sp.upcast()
        } else {
            let i = gtk::Image::from_icon_name(&format!("rade-status-{name}-symbolic"));
            i.set_pixel_size(14);
            i.upcast()
        };
        for c in self.root.css_classes() {
            if c.starts_with("rade-st-") {
                self.root.remove_css_class(&c);
            }
        }
        self.root.add_css_class(&format!("rade-st-{name}"));
        // `debug.ui` and the live harness read it.
        self.root.set_widget_name(&format!("glyph-{name}"));
        self.root.append(&w);
    }
}

/// "waiting since 14:32" (+ " (guessed)" for an R10.2 fallback).
pub fn tooltip(p: &Pane) -> String {
    let Some(s) = p.status else {
        return "no status yet".into();
    };
    let mut t = if s == AgentStatus::Done {
        "turn finished · work still needs verification/review"
    } else {
        s.as_str()
    }
    .to_string();
    if let Some(since) = p.status_since {
        if let Ok(d) = glib::DateTime::from_unix_local(since as i64) {
            let today = glib::DateTime::now_local().is_ok_and(|n| n.ymd() == d.ymd());
            let f = if today { "%H:%M" } else { "%b %e %H:%M" };
            if let Ok(when) = d.format(f) {
                t += &format!(" since {when}");
            }
        }
    }
    if p.status_heuristic {
        t += " (guessed: no hook reported it)";
    }
    if s == rade_core::AgentStatus::Interrupted {
        t += " — a restart cut its turn short; send it a prompt (\"continue\") to go on";
    }
    t
}

/// Tooltip for an aggregate dot (Tab, Source): the worst Agent and a count.
pub fn aggregate_tooltip<'a>(panes: impl IntoIterator<Item = &'a Pane> + Clone) -> Option<String> {
    let p = worst_pane(panes.clone())?;
    let name = p.name.as_deref().unwrap_or(&p.id);
    Some(format!(
        "{name}: {}\nAgents: {}",
        tooltip(p),
        summary(panes)
    ))
}

/// Apply a fallback rule for `pane` now; re-render if it changed a status.
pub fn signal(pane: &str, s: Signal) {
    let Some(app) = try_app() else { return };
    let presets = app.config.borrow().presets().unwrap_or_default();
    let changed = app
        .model
        .borrow_mut()
        .apply_signal(pane, s, &presets, now())
        .ok()
        .flatten();
    if changed.is_some() {
        app.update(|_| ());
    }
}

/// The banner (under the header bar) and the 1 s watcher: output idle →
/// [`Signal::Idle`], and the Codex hint (R16.4).
pub fn install(content: &adw::ToolbarView) {
    let banner = adw::Banner::new(CODEX_HINT);
    banner.set_button_label(Some("Dismiss"));
    banner.connect_button_clicked(|b| b.set_revealed(false));
    content.add_top_bar(&banner);
    let w = std::rc::Rc::new(Watch {
        screens: RefCell::default(),
        banner,
        hint_shown: Cell::new(false),
    });
    WATCH.with(|x| *x.borrow_mut() = Some(w));
    glib::timeout_add_local(Duration::from_secs(1), || {
        tick();
        glib::ControlFlow::Continue
    });
}

fn tick() {
    let (Some(app), Some(w)) = (try_app(), watch()) else {
        return;
    };
    let idle = Duration::from_secs(app.config.borrow().idle_seconds);
    // Running Agent Panes only: nothing to read for the rest (R4 idle CPU).
    let running: Vec<String> = {
        let m = app.model.borrow();
        m.worktrees()
            .flat_map(|(_, w)| w.tabs.iter().flat_map(|t| t.panes()))
            .filter(|p| p.agent.is_some() && p.status == Some(AgentStatus::Running))
            .map(|p| p.id.clone())
            .collect()
    };
    let mut quiet = Vec::new();
    {
        let terms = app.terminals.borrow();
        let mut screens = w.screens.borrow_mut();
        screens.retain(|id, _| running.contains(id));
        for id in &running {
            let Some(text) = terms
                .get(id)
                .and_then(|t| t.handle.read_text(GHOSTTY_POINT_VIEWPORT))
            else {
                continue;
            };
            let mut h = DefaultHasher::new();
            text.hash(&mut h);
            let h = h.finish();
            let e = screens.entry(id.clone()).or_insert((h, Instant::now()));
            if e.0 != h {
                *e = (h, Instant::now());
            } else if e.1.elapsed() >= idle {
                quiet.push(id.clone());
            }
        }
    }
    for id in quiet {
        signal(&id, Signal::Idle);
    }
    if !w.hint_shown.get() {
        let presets = app.config.borrow().presets().unwrap_or_default();
        if app
            .model
            .borrow()
            .codex_untrusted(&presets, now())
            .is_some()
        {
            w.hint_shown.set(true);
            w.banner.set_revealed(true);
        }
    }
}

/// When a running Agent's screen last changed (Unix seconds), as the idle
/// watcher saw it; `None` for one it does not watch (Agent Overview's
/// `quiet`, R117.2).
pub fn last_change(pane: &str) -> Option<u64> {
    let w = watch()?;
    let screens = w.screens.borrow();
    let (_, at) = screens.get(pane)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(now.saturating_sub(at.elapsed().as_secs()))
}

/// `debug.window` field: the hint banner's state.
pub fn banner_info() -> Value {
    match watch() {
        Some(w) => json!({"revealed": w.banner.is_revealed(), "title": w.banner.title().as_str()}),
        None => Value::Null,
    }
}

/// `debug.action dismiss_banner`: what the banner's Dismiss button does.
pub fn dismiss_banner() {
    if let Some(w) = watch() {
        w.banner.emit_by_name::<()>("button-clicked", &[]);
    }
}
