//! An Agent's glyph coloured by its Agent Status (R115, ADR-0038): RADE's
//! own mark per agent, where the LED stood on Navigator rows, Tabs, Pane
//! headers, the Inbox, the `+` menu and the system monitor's Panes. The
//! Statusline counts keep their LEDs: they show a status, not an agent.
//! A Preset's `icon` (R115.5) is drawn in its own colours instead, with the
//! status as a small LED at its lower right.

use crate::app::try_app;
use crate::layout_view::set_class;
use gtk::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use rade_core::design::AgentMark;
use rade_core::{AgentStatus, Pane};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// One glyph: the icon, and the status LED shown over a custom icon.
#[derive(Clone)]
pub struct Glyph {
    pub root: gtk::Overlay,
    image: gtk::Image,
    dot: gtk::Box,
}

impl Glyph {
    /// A glyph for `mark` in `status`'s colour (`idle` when none).
    pub fn new(mark: AgentMark, status: Option<AgentStatus>) -> Self {
        let image = gtk::Image::new();
        image.add_css_class("agent-glyph");
        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dot.add_css_class("glyph-dot");
        dot.set_halign(gtk::Align::End);
        dot.set_valign(gtk::Align::End);
        dot.set_can_target(false);
        let root = gtk::Overlay::new();
        root.set_child(Some(&image));
        root.add_overlay(&dot);
        root.set_valign(gtk::Align::Center);
        root.set_halign(gtk::Align::Center);
        let g = Self { root, image, dot };
        g.set(mark, None, status);
        g
    }

    /// Pane `p`'s glyph: its agent, its Preset's icon, its status.
    pub fn of_pane(p: &Pane) -> Self {
        let g = Self::new(AgentMark::Other, None);
        g.set_pane(p);
        g
    }

    /// A glyph without a status: `fg2` (the `+` menu's rows).
    pub fn plain(mark: AgentMark, icon: Option<&Path>) -> Self {
        let g = Self::new(mark, None);
        g.set(mark, icon, None);
        set_class(&g.image, "glyph-", "plain");
        g.dot.set_visible(false);
        g
    }

    pub fn add_css_class(&self, class: &str) {
        self.image.add_css_class(class);
    }

    pub fn set_pane(&self, p: &Pane) {
        let mark = p.mark().unwrap_or(AgentMark::Other);
        self.set(mark, icon_of(p.agent_name()).as_deref(), p.status);
    }

    /// Show `mark` (or the user's `icon`) in `status`'s colour.
    pub fn set(&self, mark: AgentMark, icon: Option<&Path>, status: Option<AgentStatus>) {
        let status = status.map_or("idle", |s| s.as_str());
        match icon {
            Some(path) => {
                let gicon = gio::FileIcon::new(&gio::File::for_path(path));
                if self.image.gicon().is_none_or(|g| !g.equal(Some(&gicon))) {
                    self.image.set_from_gicon(&gicon);
                }
                self.image.add_css_class("custom");
                set_class(&self.image, "glyph-", "custom");
                set_class(&self.dot, "led-", status);
                self.dot.add_css_class("led");
                self.dot.set_visible(true);
            }
            None => {
                if self.image.icon_name().as_deref() != Some(mark.icon()) {
                    self.image.set_icon_name(Some(mark.icon()));
                }
                self.image.remove_css_class("custom");
                set_class(&self.image, "glyph-", status);
                self.dot.set_visible(false);
            }
        }
        set_class(&self.image, "mark-", mark.as_str());
        self.root.set_widget_name(&format!("glyph-{status}"));
    }

    /// `debug.ui` fields: `{"glyph": <mark>, "status": <status>, "custom"}`.
    pub fn debug(&self) -> serde_json::Value {
        let class = |prefix: &str| {
            self.image
                .css_classes()
                .iter()
                .find_map(|c| c.strip_prefix(prefix).map(String::from))
        };
        let custom = self.image.has_css_class("custom");
        let status = self
            .root
            .widget_name()
            .strip_prefix("glyph-")
            .map(String::from);
        serde_json::json!({"glyph": class("mark-"), "status": status, "custom": custom})
    }
}

/// The user's icon of Preset `preset` (R115.5), when its file is readable.
pub fn icon_of(preset: Option<&str>) -> Option<PathBuf> {
    let app = try_app()?;
    let presets = app.config.borrow().presets().ok()?;
    let path = presets.get(preset?)?.icon.clone()?;
    path.is_file().then_some(path)
}

/// R115.4: every 3 s, the Preset whose program runs in each shell's
/// foreground marks its Pane (`detected`) until it ends; a Pane a hook or
/// a spawn made an Agent is left alone.
pub fn watch() {
    glib::timeout_add_local(Duration::from_secs(3), || {
        if let Some(app) = try_app() {
            detect(&app);
        }
        glib::ControlFlow::Continue
    });
}

fn detect(app: &crate::app::App) {
    let Ok(presets) = app.config.borrow().presets() else {
        return;
    };
    let panes: Vec<(String, Option<String>)> = {
        let m = app.model.borrow();
        m.worktrees()
            .flat_map(|(_, w)| w.tabs.iter().flat_map(|t| t.panes()))
            .filter(|p| p.agent.is_none() && matches!(p.kind, rade_core::PaneKind::Terminal { .. }))
            .map(|p| (p.id.clone(), p.detected.clone()))
            .collect()
    };
    if panes.is_empty() {
        return;
    }
    let pids = rade_core::process::pane_pids();
    let proc = Path::new("/proc");
    let changes: Vec<(String, Option<String>)> = panes
        .into_iter()
        .filter_map(|(id, was)| {
            let now = pids
                .get(&id)
                .and_then(|pid| rade_core::process::foreground_argv(proc, *pid as u32))
                .and_then(|argv| rade_core::preset::detect(&presets, &argv));
            (now != was).then_some((id, now))
        })
        .collect();
    if !changes.is_empty() {
        app.update(|m| {
            for (id, preset) in changes {
                m.set_detected(&id, preset);
            }
        });
    }
}
