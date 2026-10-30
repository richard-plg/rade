//! The window's chrome around the Tabs (PRD R20, M14): the content header
//! bar's buttons (R20.11), the Right Panel with its Files / Changes / Search
//! switcher (R20.10), the Status Bar (R20.9) and the Toast raised when an
//! Agent starts waiting off screen (R20.9). `debug.ui` reads it all.

use crate::app::{try_app, App};
use crate::titlebar::Facts;
use adw::prelude::*;
use gtk::glib;
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::chrome::{
    count_text, inspector_label, statusline_cells, toast_lift, Cell, CellKind, Empty, Rect,
    StatusInput, EMPTY_ACTIVITY, HINTS, INSPECTOR_TABS,
};
use rade_core::usage::view::{ForecastToast, UsageCell};
use rade_core::State;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::time::Duration;

/// The Inspector's pages (R103.4): the text tabs, then Activity.
pub const PAGES: [(&str, &str); 3] = INSPECTOR_TABS;

/// Width of the Inspector (spec A.53).
pub const INSPECTOR_WIDTH: i32 = 340;

/// An empty state (R103.6): one mono line, then its key hints as key caps.
pub fn empty_state(e: &Empty) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 10);
    b.add_css_class("empty-state");
    b.set_valign(gtk::Align::Start);
    b.set_halign(gtk::Align::Fill);
    let line = gtk::Label::new(Some(e.line));
    line.add_css_class("empty-line");
    line.set_xalign(0.0);
    line.set_wrap(true);
    b.append(&line);
    let hints = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    hints.add_css_class("empty-hints");
    for (i, (keys, words)) in e.hints.iter().enumerate() {
        if i > 0 {
            let sep = gtk::Label::new(Some("·"));
            sep.add_css_class("empty-hint-words");
            hints.append(&sep);
        }
        let k = gtk::Label::new(Some(keys));
        k.add_css_class("kbd");
        k.set_valign(gtk::Align::Center);
        hints.append(&k);
        let w = gtk::Label::new(Some(words));
        w.add_css_class("empty-hint-words");
        hints.append(&w);
    }
    b.append(&hints);
    b
}

/// One Inspector text tab: the button, its count and its active bar.
struct InspectorTab {
    name: &'static str,
    title: &'static str,
    button: gtk::Button,
    count: gtk::Label,
    bar: gtk::Box,
}

pub struct RightPanel {
    pub root: gtk::Box,
    tabs: Vec<InspectorTab>,
    activity: gtk::Button,
    pub stack: gtk::Stack,
}

fn tab_icon(name: &str) -> &'static str {
    match name {
        "changes" => "ph-git-diff-symbolic",
        "files" => "ph-tree-structure-symbolic",
        _ => "ph-magnifying-glass-symbolic",
    }
}

impl RightPanel {
    fn new(files: &gtk::Widget) -> Self {
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.set_hhomogeneous(false);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        row.add_css_class("inspector-tabs");
        let mut tabs = Vec::new();
        for (name, title) in PAGES {
            let inner = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            let icon = gtk::Image::from_icon_name(tab_icon(name));
            icon.add_css_class("inspector-tab-icon");
            inner.append(&icon);
            let l = gtk::Label::new(Some(title));
            l.add_css_class("inspector-tab-label");
            inner.append(&l);
            let count = gtk::Label::new(None);
            count.add_css_class("inspector-tab-count");
            count.set_visible(false);
            inner.append(&count);
            let button = gtk::Button::new();
            button.set_child(Some(&inner));
            button.add_css_class("inspector-tab");
            button.set_widget_name(&format!("inspector-tab-{name}"));
            button.set_focus_on_click(false);
            button.set_tooltip_text(Some(&format!("{title} (Inspector)")));
            button.connect_clicked(move |_| {
                if let Some(app) = try_app() {
                    app.chrome.right.show(name);
                }
            });
            // The 2 px bar sits on the tab's bottom edge, inset 10 px.
            let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            bar.add_css_class("inspector-tab-bar");
            bar.set_valign(gtk::Align::End);
            bar.set_margin_start(10);
            bar.set_margin_end(10);
            bar.set_can_target(false);
            let slot = gtk::Overlay::new();
            slot.set_child(Some(&button));
            slot.add_overlay(&bar);
            row.append(&slot);
            if name == "files" {
                stack.add_named(files, Some(name));
            } else {
                stack.add_named(&gtk::Box::new(gtk::Orientation::Vertical, 0), Some(name));
            }
            tabs.push(InspectorTab {
                name,
                title,
                button,
                count,
                bar,
            });
        }
        let fill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        fill.set_hexpand(true);
        row.append(&fill);
        // Activity (R107, M37): an icon tab, empty until then.
        let activity = gtk::Button::from_icon_name("ph-pulse-symbolic");
        activity.add_css_class("inspector-tab");
        activity.add_css_class("inspector-activity");
        activity.set_focus_on_click(false);
        activity.set_tooltip_text(Some("Activity (Inspector)"));
        activity.connect_clicked(|_| {
            if let Some(app) = try_app() {
                app.chrome.right.show("activity");
            }
        });
        row.append(&activity);
        let act_page = empty_state(&EMPTY_ACTIVITY);
        act_page.add_css_class("inspector-page");
        stack.add_named(&act_page, Some("activity"));
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("inspector");
        root.set_size_request(INSPECTOR_WIDTH, -1);
        root.append(&row);
        root.append(&stack);
        let p = Self {
            root,
            tabs,
            activity,
            stack,
        };
        p.show("files");
        p
    }

    /// The tab states for page `name`.
    fn mark(&self, name: &str) {
        for t in &self.tabs {
            let on = t.name == name;
            for w in [t.button.upcast_ref::<gtk::Widget>(), t.bar.upcast_ref()] {
                if on {
                    w.add_css_class("active");
                } else {
                    w.remove_css_class("active");
                }
            }
        }
        if name == "activity" {
            self.activity.add_css_class("active");
        } else {
            self.activity.remove_css_class("active");
        }
    }

    /// Put `widget` on page `name` (`files`, `changes`, `search`), in place
    /// of what is there.
    pub fn set_page(&self, name: &str, widget: &impl IsA<gtk::Widget>) {
        let shown = self.stack.visible_child_name();
        if let Some(old) = self.stack.child_by_name(name) {
            self.stack.remove(&old);
        }
        self.stack.add_named(widget, Some(name));
        if let Some(s) = shown {
            self.stack.set_visible_child_name(&s);
        }
    }

    /// Show page `name`.
    pub fn show(&self, name: &str) {
        if self.stack.child_by_name(name).is_some() {
            self.stack.set_visible_child_name(name);
            self.mark(name);
        }
    }

    /// The page shown.
    pub fn page(&self) -> Option<String> {
        self.stack.visible_child_name().map(|s| s.to_string())
    }

    /// A tab's count (`Changes 3`; none for 0, R21.1).
    pub fn set_count(&self, name: &str, n: usize) {
        if let Some(t) = self.tabs.iter().find(|t| t.name == name) {
            let text = n.to_string();
            if t.count.text() != text {
                t.count.set_text(&text);
            }
            t.count.set_visible(n > 0);
        }
    }

    /// The text tabs' labels as read: `Changes 7`, `Files`, `Search`.
    pub fn labels(&self) -> Vec<String> {
        self.tabs
            .iter()
            .map(|t| {
                let n = if t.count.is_visible() {
                    t.count.text().parse().unwrap_or(0)
                } else {
                    0
                };
                inspector_label(t.title, n)
            })
            .collect()
    }
}

/// The Statusline (R103.5, spec A.70–A.77): Workspace (accent cell),
/// Project, Worktree `↑a ↓b`, Agent counts, ports, identity, key hints.
struct Statusline {
    root: gtk::Box,
    cells: gtk::Box,
    /// The system monitor cell (R116), between the cells and the hints.
    monitor: gtk::Widget,
    /// The cells shown now (rebuilt when they change).
    shown: RefCell<Vec<Cell>>,
    /// The focused checkout's listening ports (scanned every 3 s).
    ports: RefCell<Vec<u16>>,
    /// The identity warning shown on the identity cell (R107.5).
    warned: RefCell<Option<String>>,
    /// The usage cell shown (R110.3): the focused Agent's account.
    usage: RefCell<Option<UsageCell>>,
    hints: gtk::Box,
    /// The REC cell (R124, D18) and its text.
    rec: gtk::Box,
    rec_text: gtk::Label,
    /// The Update cell (R127, D16), between the cells and the monitor.
    update: crate::update_view::UpdateCell,
    /// (window width, cells generation) of the last fit (R111.1).
    fitted: std::cell::Cell<(i32, u64)>,
    generation: std::cell::Cell<u64>,
    /// Kinds of the cells dropped to fit.
    dropped: RefCell<Vec<&'static str>>,
}

/// The usage cell (A.74): `gauge` + `claude 5h` + a 36 × 5 px meter +
/// `62% · wk 41%`; a click opens the Usage page.
fn usage_cell(u: &UsageCell) -> gtk::Widget {
    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let img = gtk::Image::from_icon_name("ph-gauge-symbolic");
    img.add_css_class("status-icon");
    inner.append(&img);
    inner.append(&gtk::Label::new(Some(&u.lead)));
    let meter = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    meter.add_css_class("usage-meter");
    meter.set_size_request(36, 5);
    meter.set_valign(gtk::Align::Center);
    let fill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    fill.add_css_class("usage-meter-fill");
    let w = rade_core::usage::view::meter_fill(36, u.percent as f64) as i32;
    fill.set_size_request(w, 5);
    fill.set_halign(gtk::Align::Start);
    fill.set_visible(w > 0);
    meter.append(&fill);
    inner.append(&meter);
    inner.append(&gtk::Label::new(Some(&u.tail)));
    let button = gtk::Button::new();
    button.set_child(Some(&inner));
    button.add_css_class("flat");
    button.add_css_class("status-usage");
    button.set_focus_on_click(false);
    button.set_tooltip_text(Some(&u.tooltip));
    button.connect_clicked(|_| crate::keys::run_action("open_usage"));
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.add_css_class("status-cell");
    b.add_css_class("status-cell-usage");
    if u.hot {
        b.add_css_class("hot");
    }
    b.set_tooltip_text(Some(&u.tooltip));
    b.append(&button);
    b.upcast()
}

/// The Workspace's `[identity] git_email` against the focused checkout's
/// `git config user.email` (R107.5): the warning naming both, or `None`
/// (also while the checkout's facts are not read yet).
pub fn identity_warning(ws: &State) -> Option<String> {
    let app = try_app()?;
    let facts = app.chrome.facts.try_borrow().ok()?;
    if facts.path != ws.active().1.path {
        return None;
    }
    let root = ws.workspace(ws.active_workspace())?.root.clone();
    let email = rade_core::workspace::WorkspaceFile::read(&root)
        .ok()?
        .identity
        .git_email;
    rade_core::chrome::identity_warning(email.as_deref(), facts.identity.as_deref())
}

fn cell_box(kind: CellKind, icon: Option<&str>, text: &str) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    b.add_css_class("status-cell");
    b.add_css_class(&format!("status-cell-{}", kind.as_str()));
    if let Some(i) = icon {
        let img = gtk::Image::from_icon_name(i);
        img.add_css_class("status-icon");
        b.append(&img);
    }
    let l = gtk::Label::new(Some(text));
    l.add_css_class("status-text");
    b.append(&l);
    b
}

/// A key hint (`Ctrl ⇧ P commands`): its key caps, then its words; a click
/// runs the action.
fn hint(text: &str, action: &'static str) -> gtk::Button {
    let toks: Vec<&str> = text.split(' ').collect();
    let (keys, words) = (
        toks[..3.min(toks.len())].join(" "),
        toks[3.min(toks.len())..].join(" "),
    );
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let k = gtk::Label::new(Some(&keys));
    k.add_css_class("kbd");
    k.set_valign(gtk::Align::Center);
    b.append(&k);
    b.append(&gtk::Label::new(Some(&words)));
    let button = gtk::Button::new();
    button.set_child(Some(&b));
    button.add_css_class("flat");
    button.add_css_class("status-hint");
    button.set_tooltip_text(Some(text));
    button.set_focus_on_click(false);
    button.connect_clicked(move |_| crate::keys::run_action(action));
    button
}

/// Every label under `w` but key caps ellipsizes at its end.
fn shrinkable(w: &gtk::Widget) {
    let mut c = w.first_child();
    while let Some(k) = c {
        c = k.next_sibling();
        if let Some(l) = k.downcast_ref::<gtk::Label>() {
            if !l.has_css_class("kbd") {
                l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            }
        }
        shrinkable(&k);
    }
}

impl Statusline {
    fn new() -> Self {
        let cells = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        // The REC cell follows the location cells (R124, D18).
        let rec = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        rec.add_css_class("status-cell");
        rec.add_css_class("status-voice");
        let rec_icon = gtk::Image::from_icon_name("ph-microphone-fill-symbolic");
        rec_icon.add_css_class("status-icon");
        let rec_text = gtk::Label::new(None);
        rec_text.add_css_class("status-text");
        rec.append(&rec_icon);
        rec.append(&rec_text);
        rec.set_visible(false);
        let left = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        left.set_hexpand(true);
        left.append(&cells);
        left.append(&rec);
        let hints = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        hints.add_css_class("status-cell");
        hints.add_css_class("status-cell-hints");
        hints.append(&hint(HINTS[0], "command_palette"));
        hints.append(&hint(HINTS[1], "jump_unread"));
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("statusline");
        root.append(&left);
        let update = crate::update_view::UpdateCell::new();
        root.append(&update.button);
        // The system monitor (R116): one persistent widget, updated in place.
        let monitor = crate::sysmon_view::widget();
        root.append(&monitor);
        root.append(&hints);
        Self {
            root,
            cells,
            monitor,
            shown: Default::default(),
            ports: Default::default(),
            warned: Default::default(),
            usage: Default::default(),
            hints,
            rec,
            rec_text,
            update,
            fitted: Default::default(),
            generation: Default::default(),
            dropped: Default::default(),
        }
    }

    /// Drop cells until the Statusline fits the window (spec A "1280 px":
    /// hints, port, identity; never Workspace, counts, usage).
    fn fit(&self, width: i32) {
        let key = (width, self.generation.get());
        if width <= 0 || self.fitted.get() == key {
            return;
        }
        self.fitted.set(key);
        let monitor = crate::sysmon_view::enabled();
        // The Update cell counts whenever it has something to say (D16),
        // never judged by its visibility, which this fit sets itself.
        let update = self.update.last.borrow().is_some();
        let mut widgets: Vec<gtk::Widget> = Vec::new();
        for parent in [&self.cells, &self.hints] {
            if parent == &self.hints {
                if update {
                    widgets.push(self.update.button.clone().upcast());
                }
                if monitor {
                    widgets.push(self.monitor.clone());
                }
            }
            let mut c = parent.first_child();
            while let Some(w) = c {
                c = w.next_sibling();
                widgets.push(w);
            }
        }
        let mut cells: Vec<Cell> = self.shown.borrow().clone();
        cells.retain(|c| c.kind != CellKind::Usage || self.usage.borrow().is_some());
        if update {
            cells.push(Cell {
                kind: CellKind::Update,
                text: String::new(),
            });
        }
        if monitor {
            cells.push(Cell {
                kind: CellKind::Monitor,
                text: String::new(),
            });
        }
        cells.extend(HINTS.iter().map(|h| Cell {
            kind: CellKind::Hint,
            text: h.to_string(),
        }));
        if cells.len() != widgets.len() {
            return;
        }
        for w in &widgets {
            w.set_visible(true);
        }
        let widths: Vec<i32> = widgets
            .iter()
            .map(|w| w.measure(gtk::Orientation::Horizontal, -1).1)
            .collect();
        let (_, root_nat, _, _) = self.root.measure(gtk::Orientation::Horizontal, -1);
        // The Statusline's own padding and the hints' spacing.
        let chrome = root_nat - widths.iter().sum::<i32>();
        let keep = rade_core::chrome::statusline_fit(&cells, &widths, width - chrome.max(0));
        let mut dropped = Vec::new();
        for ((w, k), c) in widgets.iter().zip(&keep).zip(&cells) {
            w.set_visible(*k);
            if !*k {
                dropped.push(c.kind.as_str());
            }
        }
        *self.dropped.borrow_mut() = dropped;
    }

    fn sync(&self, ws: &State, facts: &Facts) {
        let (_, w) = ws.active();
        let fresh = facts.path == w.path;
        let workspace = ws
            .workspace_looks()
            .into_iter()
            .find(|l| l.id == ws.active_workspace())
            .map_or_else(|| ws.active_workspace().to_string(), |l| l.name);
        let input = StatusInput {
            workspace,
            project: crate::titlebar::project_name(ws),
            worktree: Some((
                w.name.clone(),
                fresh.then_some(facts.ahead_behind).flatten(),
            )),
            counts: ws.status_counts(),
            ports: self.ports.borrow().clone(),
            identity: fresh.then(|| facts.identity.clone()).flatten(),
            usage: None,
            schedules: crate::schedules::on_count(ws, ws.active_workspace()),
        };
        let usage = crate::accounts::focused_cell(ws);
        let input = StatusInput {
            usage: usage.as_ref().map(UsageCell::text),
            ..input
        };
        let cells: Vec<Cell> = statusline_cells(&input)
            .into_iter()
            .filter(|c| c.kind != CellKind::Hint)
            .collect();
        let warning = fresh
            .then(|| {
                let root = ws.workspace(ws.active_workspace())?.root.clone();
                let email = rade_core::workspace::WorkspaceFile::read(&root)
                    .ok()?
                    .identity
                    .git_email;
                rade_core::chrome::identity_warning(email.as_deref(), facts.identity.as_deref())
            })
            .flatten();
        if *self.shown.borrow() == cells
            && *self.warned.borrow() == warning
            && *self.usage.borrow() == usage
        {
            return;
        }
        *self.warned.borrow_mut() = warning.clone();
        *self.usage.borrow_mut() = usage.clone();
        while let Some(c) = self.cells.first_child() {
            self.cells.remove(&c);
        }
        for c in &cells {
            let w: gtk::Widget = match c.kind {
                CellKind::Workspace => {
                    cell_box(c.kind, Some("ph-circles-four-symbolic"), &c.text).upcast()
                }
                CellKind::Project => cell_box(c.kind, Some("ph-cube-symbolic"), &c.text).upcast(),
                CellKind::Worktree => {
                    cell_box(c.kind, Some("ph-git-branch-symbolic"), &c.text).upcast()
                }
                CellKind::Schedules => {
                    // `⏱ 3` → the Schedules page (R139.1).
                    let n = c.text.trim_start_matches("⏱ ").to_string();
                    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                    let img = gtk::Image::from_icon_name("ph-clock-symbolic");
                    img.add_css_class("status-icon");
                    inner.append(&img);
                    inner.append(&gtk::Label::new(Some(&n)));
                    let button = gtk::Button::new();
                    button.set_child(Some(&inner));
                    button.add_css_class("flat");
                    button.add_css_class("status-schedules");
                    button.set_focus_on_click(false);
                    button.set_tooltip_text(Some(&format!("{n} Schedules on · open Schedules")));
                    button.connect_clicked(|_| crate::schedules_view::open(None));
                    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                    b.add_css_class("status-cell");
                    b.add_css_class("status-cell-schedules");
                    b.append(&button);
                    b.upcast()
                }
                CellKind::Identity => {
                    let b = cell_box(c.kind, Some("ph-user-circle-symbolic"), &c.text);
                    // Commits would go out as someone else (R107.5).
                    if let Some(w) = &warning {
                        b.add_css_class("wait");
                        b.set_tooltip_text(Some(w));
                    }
                    b.upcast()
                }
                CellKind::Counts => {
                    let b = gtk::Box::new(gtk::Orientation::Horizontal, 10);
                    b.add_css_class("status-cell");
                    b.add_css_class("status-cell-counts");
                    for (s, n) in &input.counts {
                        let one = gtk::Box::new(gtk::Orientation::Horizontal, 5);
                        one.add_css_class("status-count");
                        one.add_css_class(&format!("status-count-{}", s.as_str()));
                        one.set_widget_name(&format!("count-{}", s.as_str()));
                        let led = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                        led.add_css_class("led");
                        led.add_css_class(&format!("led-{}", s.as_str()));
                        led.set_valign(gtk::Align::Center);
                        one.append(&led);
                        one.append(&gtk::Label::new(Some(&count_text(*s, *n))));
                        // A click opens Agent Overview on that band (R117.1).
                        let band = overview_band(*s);
                        let click = gtk::GestureClick::new();
                        click.connect_released(move |_, _, _, _| {
                            crate::overview_view::open_band(band)
                        });
                        one.add_controller(click);
                        b.append(&one);
                    }
                    b.upcast()
                }
                CellKind::Ports => {
                    let b = gtk::Box::new(gtk::Orientation::Horizontal, 2);
                    b.add_css_class("status-cell");
                    b.add_css_class("status-cell-ports");
                    for p in &input.ports {
                        let inner = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                        inner.append(&gtk::Image::from_icon_name("ph-plug-symbolic"));
                        inner.append(&gtk::Label::new(Some(&format!(":{p}"))));
                        let button = gtk::Button::new();
                        button.set_child(Some(&inner));
                        button.add_css_class("flat");
                        button.add_css_class("status-port");
                        button.set_focus_on_click(false);
                        button
                            .set_tooltip_text(Some(&format!("Open localhost:{p} in the Browser")));
                        let p = *p;
                        button.connect_clicked(move |_| {
                            crate::browser::open_browser(Some(format!("http://localhost:{p}/")))
                        });
                        b.append(&button);
                    }
                    b.upcast()
                }
                CellKind::Usage => match &usage {
                    Some(u) => usage_cell(u),
                    None => continue,
                },
                // Its own persistent widget (R116), never in `cells`.
                CellKind::Hint | CellKind::Monitor | CellKind::Update => continue,
            };
            self.cells.append(&w);
        }
        *self.shown.borrow_mut() = cells;
        self.generation.set(self.generation.get() + 1);
        // No cell's text widens the window (R111.1): under pressure a text
        // ellipsizes until `fit` drops whole cells.
        shrinkable(self.root.upcast_ref());
    }

    /// `REC m:ss` while a take records, nothing otherwise (R124, D18).
    fn set_rec(&self, time: Option<&str>) {
        match time {
            Some(t) => {
                self.rec_text.set_label(&format!("REC {t}"));
                self.rec.set_visible(true);
            }
            None => self.rec.set_visible(false),
        }
    }

    fn info(&self) -> Value {
        let shown = self.shown.borrow();
        let mut cells: Vec<String> = shown.iter().map(|c| c.text.clone()).collect();
        cells.extend(HINTS.iter().map(|h| h.to_string()));
        json!({
            "visible": self.root.is_visible() && self.root.is_mapped(),
            "cells": cells,
            "kinds": shown.iter().map(|c| c.kind.as_str()).chain(HINTS.iter().map(|_| "hint")).collect::<Vec<_>>(),
            "identity_warning": *self.warned.borrow(),
            "dropped": *self.dropped.borrow(),
            "usage": *self.usage.borrow(),
            "voice_rec": self.rec.is_visible().then(|| self.rec_text.label().to_string()),
            "update": self.update.shown_text(),
        })
    }

    /// The pre-1.0 `status_bar` shape (counts in words, ports) for older checks.
    fn legacy(&self, ws: &State) -> Value {
        json!({
            "visible": self.root.is_visible() && self.root.is_mapped(),
            "counts": ws.status_counts().iter().map(|(s, n)| format!("{n} {}", s.words())).collect::<Vec<_>>(),
            "ports": *self.ports.borrow(),
        })
    }
}

/// The Statusline's REC cell (R124, D18): `Some("0:06")` shows `REC 0:06`,
/// `None` hides it.
pub fn set_voice_rec(time: Option<&str>) {
    if let Some(app) = try_app() {
        app.chrome.statusline.set_rec(time);
    }
}

/// A Toast's button: its label and what it does.
pub type ToastButton<'a> = (&'a str, Box<dyn Fn()>);

pub struct Chrome {
    pub titlebar: crate::titlebar::Titlebar,
    pub right: RightPanel,
    /// The Tabs' column: the Toast overlay.
    pub center: gtk::Box,
    toasts: adw::ToastOverlay,
    statusline: Statusline,
    session_box: gtk::Box,
    recovery_label: gtk::Label,
    save_label: gtk::Label,
    retry_save: gtk::Button,
    state_location: gtk::Button,
    /// Panes waiting at the last update; the Toasts raised (`debug.ui`).
    waiting: RefCell<Option<Vec<String>>>,
    /// Agents whose finished turn offers a Review (R119.5), as last seen.
    finished: RefCell<Option<Vec<String>>>,
    /// The Review Toasts raised: (Pane, Toast).
    reviews: RefCell<Vec<(String, adw::Toast)>>,
    raised: RefCell<Vec<(String, adw::Toast)>>,
    /// Other Toasts' titles, newest last (`debug.ui`).
    notes: RefCell<Vec<String>>,
    /// Toasts raised by `toast` (the 20 newest), for `toasts_clear`.
    shown: RefCell<Vec<adw::Toast>>,
    /// Forecast Toasts raised (R110.4, the 20 newest), for `debug.ui`.
    forecasts: RefCell<Vec<(ForecastToast, adw::Toast)>>,
    /// Statusline shown (config `status_bar`, palette toggle).
    bar_shown: std::cell::Cell<Option<bool>>,
    /// The focused checkout's git facts, and the one being read.
    facts: RefCell<Facts>,
    reading: RefCell<Option<(std::path::PathBuf, Option<String>)>>,
}

impl Chrome {
    /// The titlebar and Statusline, and the column that wraps `stack` (the
    /// Tabs) under Toasts; `files` = the file tree.
    pub fn new(stack: &gtk::Stack, files: &gtk::Widget) -> Self {
        let titlebar = crate::titlebar::Titlebar::new();
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(stack));
        toasts.set_vexpand(true);
        let statusline = Statusline::new();
        let center = gtk::Box::new(gtk::Orientation::Vertical, 0);
        center.add_css_class("center-column");
        let session_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .accessible_role(gtk::AccessibleRole::Alert)
            .build();
        session_box.add_css_class("card");
        session_box.set_margin_start(12);
        session_box.set_margin_end(12);
        session_box.set_margin_top(6);
        session_box.set_margin_bottom(6);
        let recovery_label = gtk::Label::new(None);
        let save_label = gtk::Label::new(None);
        for label in [&recovery_label, &save_label] {
            label.set_wrap(true);
            label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            label.set_xalign(0.0);
            label.set_selectable(true);
            session_box.append(label);
        }
        save_label.add_css_class("warning");
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let retry_save = gtk::Button::with_label("Retry saving");
        retry_save.connect_clicked(|_| {
            if let Some(app) = try_app() {
                app.save_now();
            }
        });
        let state_location = gtk::Button::with_label("State location…");
        state_location.connect_clicked(|_| {
            if let Some(app) = try_app() {
                app.show_state_location();
            }
        });
        actions.append(&retry_save);
        actions.append(&state_location);
        session_box.append(&actions);
        session_box.set_visible(false);
        center.append(&session_box);
        center.append(&toasts);

        // Listening ports and git facts change without a model update:
        // ports every 3 s while the Statusline shows (a /proc scan), git
        // facts while the window is in front.
        glib::timeout_add_local(Duration::from_secs(3), || {
            if let Some(app) = try_app() {
                if app.chrome.statusline.root.is_mapped() {
                    let wt = app.model.borrow().active().1.id.clone();
                    let mut ports: Vec<u16> = crate::sidebar::ports()
                        .into_iter()
                        .filter(|p| p.at.1 == wt)
                        .map(|p| p.port)
                        .collect();
                    ports.sort();
                    ports.dedup();
                    *app.chrome.statusline.ports.borrow_mut() = ports;
                }
                if app.window.is_active() {
                    app.chrome.refresh_facts(&app.model.borrow(), true);
                }
                app.chrome.render(&app.model.borrow());
            }
            glib::ControlFlow::Continue
        });

        // A Toast never covers an action (R103.6): checked while one shows.
        glib::timeout_add_local(Duration::from_millis(150), || {
            if let Some(app) = try_app() {
                app.chrome.lift_toasts(app.window.upcast_ref());
            }
            glib::ControlFlow::Continue
        });

        Self {
            titlebar,
            right: RightPanel::new(files),
            center,
            toasts,
            statusline,
            session_box,
            recovery_label,
            save_label,
            retry_save,
            state_location,
            waiting: RefCell::default(),
            finished: RefCell::default(),
            reviews: RefCell::default(),
            raised: RefCell::default(),
            notes: RefCell::default(),
            shown: RefCell::default(),
            forecasts: RefCell::default(),
            bar_shown: Default::default(),
            facts: RefCell::default(),
            reading: RefCell::default(),
        }
    }

    /// R103.6: a shown Toast moves up above any visible button it would
    /// cover (`chrome::toast_lift`), through its bottom margin.
    pub fn lift_toasts(&self, window: &gtk::Widget) {
        let toasts: Vec<gtk::Widget> = crate::geometry::find(self.toasts.upcast_ref(), "toast")
            .into_iter()
            .filter(|w| w.is_mapped())
            .collect();
        if toasts.is_empty() {
            return;
        }
        let rect = |w: &gtk::Widget| {
            w.compute_bounds(window).map(|b| Rect {
                x: b.x() as f64,
                y: b.y() as f64,
                w: b.width() as f64,
                h: b.height() as f64,
            })
        };
        let top = rect(self.toasts.upcast_ref()).map_or(0.0, |r| r.y);
        let buttons: Vec<Rect> = crate::geometry::find(window, "button")
            .into_iter()
            .filter(|b| b.is_mapped() && b.opacity() > 0.0)
            .filter(|b| !toasts.iter().any(|t| b.is_ancestor(t)))
            .filter_map(|b| rect(&b))
            .collect();
        for t in toasts {
            let Some(mut r) = rect(&t) else { continue };
            // Where it sits with no lift.
            let had = t.margin_bottom();
            r.y += had as f64;
            let lift = toast_lift(&r, &buttons, top).round() as i32;
            if lift != had {
                t.set_margin_bottom(lift);
            }
        }
    }

    /// The Statusline's widget (placed under the whole window by `main.rs`).
    pub fn statusline(&self) -> &gtk::Box {
        &self.statusline.root
    }

    /// Read the focused checkout's git facts in the background when it
    /// changed (or `again`); [`Chrome::render`] shows them when done.
    fn refresh_facts(&self, ws: &State, again: bool) {
        let (s, w) = ws.active();
        let key = (w.path.clone(), w.base.clone());
        if !again && self.reading.borrow().as_ref() == Some(&key) {
            return;
        }
        *self.reading.borrow_mut() = Some(key.clone());
        let env = crate::app::workspace_env(ws, &s.id);
        let (path, base) = key;
        crate::worktrees::in_background(
            move || crate::titlebar::read_facts(&path, base, env),
            |f| {
                let Some(app) = try_app() else { return };
                if *app.chrome.facts.borrow() != f {
                    *app.chrome.facts.borrow_mut() = f;
                    app.chrome.render(&app.model.borrow());
                    // The identity line of Changes by Project (R107.5).
                    if let Some(c) = crate::changes::get() {
                        c.follow_project(&app.model.borrow());
                    }
                }
            },
        );
    }

    /// Breadcrumb, Review count, Workspace tooltip and Statusline from the
    /// model and the facts read so far.
    fn render(&self, ws: &State) {
        let facts = self.facts.borrow();
        let fresh = facts.path == ws.active().1.path;
        // Project home: `Shop / Home` (spec B.1; `Overview` until M43, R117).
        let home = crate::home_view::shown(ws).and_then(|id| {
            crate::navdata::projects(ws)
                .into_iter()
                .find(|p| p.id == id)
                .map(|p| p.name)
        });
        let usage_page = crate::usage_view::is_shown(ws);
        self.titlebar
            .review_button
            .set_visible(home.is_none() && !usage_page);
        let u = &self.titlebar.usage_button;
        u.set_visible(usage_page || crate::accounts::any_accounts(ws));
        if usage_page {
            u.add_css_class("on");
        } else {
            u.remove_css_class("on");
        }
        if usage_page {
            self.titlebar.show(
                rade_core::chrome::Crumbs {
                    project: None,
                    source: "Agent usage".into(),
                    worktree: None,
                    tail: None,
                },
                None,
            );
        } else {
            match home {
                Some(name) => self.titlebar.show(
                    rade_core::chrome::Crumbs {
                        project: Some(name),
                        source: "Home".into(),
                        worktree: None,
                        tail: None,
                    },
                    None,
                ),
                None => self.titlebar.show(
                    crate::titlebar::crumbs(ws, &facts),
                    fresh.then_some(facts.review).flatten(),
                ),
            }
        }
        self.statusline.sync(ws, &facts);
        // The Workspace button's tooltip carries the identity (R103.1).
        if let Some(app) = try_app() {
            let name = ws
                .workspace_looks()
                .into_iter()
                .find(|l| l.id == ws.active_workspace())
                .map_or_else(String::new, |l| l.name);
            let who = fresh
                .then(|| facts.identity.clone())
                .flatten()
                .map(|i| format!(" · {i}"))
                .unwrap_or_default();
            app.sidebar.switcher.button.set_tooltip_text(Some(&format!(
                "{name}{who}\nSwitch Workspace (Ctrl+Alt+1…9)"
            )));
        }
    }

    pub fn session_feedback(&self, recovery: Option<&str>, warning: Option<&str>) {
        self.recovery_label.set_text(recovery.unwrap_or(""));
        self.recovery_label.set_visible(recovery.is_some());
        self.save_label.set_text(warning.unwrap_or(""));
        self.save_label.set_visible(warning.is_some());
        self.retry_save.set_visible(warning.is_some());
        self.session_box
            .set_visible(recovery.is_some() || warning.is_some());
    }

    /// After every sync: breadcrumb, Inspector toggle, Statusline.
    pub fn sync(&self, app: &App, ws: &State) {
        self.titlebar.inspector.set_active(ws.file_tree);
        let want = self
            .bar_shown
            .get()
            .unwrap_or(app.config.borrow().status_bar);
        self.statusline.root.set_visible(want);
        self.refresh_facts(ws, false);
        self.render(ws);
        self.fit();
    }

    /// Fit again from scratch (the fonts changed: every width did).
    pub fn refit(&self) {
        self.statusline.fitted.set((0, 0));
        self.titlebar.unfit();
        self.fit();
    }

    /// The Update cell (R127), for `update.rs`.
    pub fn update_cell(&self) -> &crate::update_view::UpdateCell {
        &self.statusline.update
    }

    /// The Update cell appeared, vanished or changed its text: fit again.
    pub fn update_changed(&self) {
        self.statusline
            .generation
            .set(self.statusline.generation.get() + 1);
        self.fit();
    }

    /// Fit the titlebar and the Statusline to the window's width (R111.1).
    pub fn fit(&self) {
        let w = crate::layout_mode::window_width();
        self.titlebar.fit(w);
        self.statusline.fit(w);
    }

    /// A Toast with `text` (Pango markup) and an optional button.
    pub fn toast(&self, text: &str, button: Option<ToastButton>) {
        let t = adw::Toast::new(text);
        t.set_timeout(6);
        if let Some((label, f)) = button {
            t.set_button_label(Some(label));
            t.connect_button_clicked(move |_| f());
        }
        self.toasts.add_toast(t.clone());
        {
            let mut shown = self.shown.borrow_mut();
            shown.push(t);
            let extra = shown.len().saturating_sub(20);
            shown.drain(..extra);
        }
        let mut notes = self.notes.borrow_mut();
        notes.push(text.to_string());
        let extra = notes.len().saturating_sub(20);
        notes.drain(..extra);
    }

    /// The forecast Toast (R110.4, spec F.15–F.20): `warning-diamond`, the
    /// account in bold and when it hits its limit, a sub-line with the
    /// figures, and `Next agents use <other>`: only Agents of Workspace
    /// `ws` opened later default to the other account (in memory; running
    /// Panes keep theirs). Recorded in Activity; hides after 10 s.
    pub fn forecast_toast(&self, ws: &str, t: ForecastToast) {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.add_css_class("forecast-toast");
        let icon = gtk::Image::from_icon_name("ph-warning-diamond-fill-symbolic");
        icon.add_css_class("toast-icon");
        icon.set_valign(gtk::Align::Center);
        row.append(&icon);
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        let head = gtk::Label::new(None);
        head.set_markup(&format!(
            "<b>{}</b>{}",
            glib::markup_escape_text(&t.name),
            glib::markup_escape_text(&t.rest)
        ));
        head.add_css_class("toast-title");
        head.set_xalign(0.0);
        text.append(&head);
        let sub = gtk::Label::new(Some(&t.sub));
        sub.add_css_class("toast-sub");
        sub.set_xalign(0.0);
        text.append(&sub);
        row.append(&text);
        let toast = adw::Toast::builder().custom_title(&row).timeout(10).build();
        // Large text (F "Text scale 200 %"): the text wraps to at most 3
        // lines within the window − 64 px and the action sits under it.
        let large = crate::layout_mode::mode().large_text;
        if large {
            let room = rade_core::chrome::toast_max_width(crate::layout_mode::window_width());
            // Characters per line at the scaled 12.5 px Inter (≈ 0.55 em).
            let chars = (f64::from(room - 120) / (12.5 * 0.55 * crate::layout_mode::text_scale()))
                .max(10.0) as i32;
            for l in [&head, &sub] {
                l.set_wrap(true);
                l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                l.set_lines(3);
                l.set_ellipsize(gtk::pango::EllipsizeMode::End);
                l.set_max_width_chars(chars);
            }
        }
        if let (Some(label), Some(other)) = (t.action(), t.other.clone()) {
            let (ws, agent) = (ws.to_string(), t.agent);
            if large {
                let b = gtk::Button::with_label(&label);
                b.add_css_class("toast-action-under");
                b.set_halign(gtk::Align::Start);
                b.set_margin_top(6);
                let tw = toast.downgrade();
                b.connect_clicked(move |_| {
                    next_agents_use(&ws, agent, &other);
                    if let Some(t) = tw.upgrade() {
                        t.dismiss();
                    }
                });
                text.append(&b);
            } else {
                toast.set_button_label(Some(&label));
                toast.connect_button_clicked(move |_| next_agents_use(&ws, agent, &other));
            }
        }
        self.toasts.add_toast(toast.clone());
        crate::activity_view::push(rade_core::activity::Event::limit_line(
            &t.activity_line(),
            crate::panes::now(),
        ));
        let mut f = self.forecasts.borrow_mut();
        f.push((t, toast));
        let extra = f.len().saturating_sub(20);
        f.drain(..extra);
    }

    /// The palette's Toggle Status Bar (R20.9): for this session.
    pub fn toggle_status_bar(&self, app: &App) {
        let now = !self.statusline.root.is_visible();
        self.bar_shown.set(Some(now));
        self.sync(app, &app.model.borrow());
    }

    /// After every update: a Toast for each Agent that started waiting
    /// while its Pane is off screen (R20.9).
    pub fn after_update(&self, ws: &State) {
        let now = ws.waiting_panes();
        // Shown or answered: its Toast goes (queued ones would show stale).
        for (pane, t) in self.raised.borrow().iter() {
            if !now.contains(pane) || ws.pane_on_screen(pane) {
                t.dismiss();
            }
        }
        self.review_toasts(ws);
        crate::update::maybe_toast_in(ws);
        // The first update only learns who already waits (Session Restore).
        let Some(before) = self.waiting.replace(Some(now)) else {
            return;
        };
        for pane in rade_core::design::toasts(&before, ws) {
            let Some(p) = ws.pane(&pane) else { continue };
            let name = p.name.as_deref().unwrap_or(&p.id);
            // From a hidden Workspace: named first (R23.6).
            let ws_name = ws
                .hidden_workspace_name(&pane)
                .map(|w| format!("[{}] ", glib::markup_escape_text(&w)))
                .unwrap_or_default();
            let t = adw::Toast::new(&format!(
                "{ws_name}<b>{}</b> needs input",
                glib::markup_escape_text(name)
            ));
            t.set_button_label(Some("Show"));
            t.set_timeout(8);
            let id = pane.clone();
            t.connect_button_clicked(move |_| show_pane(&id));
            self.toasts.add_toast(t.clone());
            let mut raised = self.raised.borrow_mut();
            raised.push((pane, t));
            let extra = raised.len().saturating_sub(20);
            raised.drain(..extra);
        }
    }

    /// R119.5: an Agent that just finished a turn in a Worktree under
    /// review raises a Toast whose button opens that Review.
    fn review_toasts(&self, ws: &State) {
        let now = ws.finished_for_review();
        for (pane, t) in self.reviews.borrow().iter() {
            if !now.contains(pane) {
                t.dismiss();
            }
        }
        let Some(before) = self.finished.replace(Some(now.clone())) else {
            return;
        };
        for pane in now.iter().filter(|p| !before.contains(p)) {
            let Some(p) = ws.pane(pane) else { continue };
            let name = p.name.as_deref().or(p.agent_name()).unwrap_or(&p.id);
            let wt = ws
                .review_target(pane)
                .and_then(|(s, w)| ws.worktree(&s, &w).map(|w| w.name.clone()))
                .unwrap_or_default();
            let t = adw::Toast::new(&format!(
                "<b>{}</b> finished in {}",
                glib::markup_escape_text(name),
                glib::markup_escape_text(&wt)
            ));
            t.set_button_label(Some("Review"));
            t.set_timeout(8);
            let id = pane.clone();
            t.connect_button_clicked(move |_| crate::review::open_for(&id));
            self.toasts.add_toast(t.clone());
            let mut r = self.reviews.borrow_mut();
            r.push((pane.clone(), t));
            let extra = r.len().saturating_sub(20);
            r.drain(..extra);
        }
    }

    /// `debug.ui`: the chrome as shown; `op` `toast_show` clicks the last
    /// Toast's Show, `right_page` switches the Right Panel.
    pub fn debug(&self, op: &str, arg: Option<&str>) -> Value {
        match op {
            "toast_review" => {
                if let Some((_, t)) = self.reviews.borrow().last() {
                    t.emit_by_name::<()>("button-clicked", &[]);
                    t.dismiss();
                }
            }
            "toast_show" => {
                if let Some((_, t)) = self.raised.borrow().last() {
                    t.emit_by_name::<()>("button-clicked", &[]);
                    t.dismiss();
                }
            }
            "right_page" => self.right.show(arg.unwrap_or("files")),
            "retry_save" => self.retry_save.emit_clicked(),
            // The newest forecast Toast's action (F.18, F.20).
            "forecast_action" => {
                if let Some((_, t)) = self.forecasts.borrow().last() {
                    t.emit_by_name::<()>("button-clicked", &[]);
                    t.dismiss();
                }
            }
            // Dismiss every Toast (screenshots of golden scenes).
            "toasts_clear" => {
                for t in self.shown.borrow_mut().drain(..) {
                    t.dismiss();
                }
                for (_, t) in self.raised.borrow().iter() {
                    t.dismiss();
                }
                for (_, t) in self.reviews.borrow().iter() {
                    t.dismiss();
                }
                for (_, t) in self.forecasts.borrow().iter() {
                    t.dismiss();
                }
            }
            "state_location" => self.state_location.emit_clicked(),
            _ => {}
        }
        let mut t = self.titlebar.debug();
        let labels = self.right.labels();
        t["statusline"] = self.statusline.info();
        t["layout"] = crate::layout_mode::debug();
        t["inspector_labels"] = json!(labels);
        t["review_toasts"] = json!(self
            .reviews
            .borrow()
            .iter()
            .map(|(p, t)| json!({"pane": p, "title": t.title().map(|s| s.to_string())}))
            .collect::<Vec<_>>());
        t["inspector"] = json!({
            "labels": labels,
            "activity": self.right.activity.is_visible(),
            "page": self.right.page(),
            "shown": self.right.root.is_mapped(),
        });
        t["status_bar"] = try_app()
            .map(|a| self.statusline.legacy(&a.model.borrow()))
            .unwrap_or(Value::Null);
        t["right_panel"] = json!({
                "page": self.right.page(),
                "pages": PAGES.iter().map(|p| p.0).collect::<Vec<_>>(),
                "labels": labels,
                "toggle": self.titlebar.inspector.is_active(),
        });
        t["session"] = json!({
                "visible": self.session_box.is_visible(),
                "recovery": self.recovery_label.text().as_str(),
                "save_warning": self.save_label.text().as_str(),
        });
        t["toasts"] = self
            .raised
            .borrow()
            .iter()
            .map(|(p, t)| json!({"pane": p, "title": t.title().map(|t| t.to_string())}))
            .collect();
        t["forecast_toasts"] = self
            .forecasts
            .borrow()
            .iter()
            .rev()
            .map(|(f, _)| {
                json!({"title": f.title(), "name": f.name, "sub": f.sub,
                    "button": f.action(), "account": f.account, "key": f.key})
            })
            .collect();
        t["notes"] = json!(*self.notes.borrow());
        t
    }
}

/// The forecast Toast's action (F.20): Agents of `agent` opened later in
/// `ws` default to `other`; running Panes keep their account.
fn next_agents_use(ws: &str, agent: rade_core::account::AgentKind, other: &str) {
    let Some(app) = try_app() else { return };
    app.update(|m| m.set_next_default(ws, agent, other));
    crate::activity_view::push(rade_core::activity::Event::limit_line(
        &format!("account {other}: next {} agents use it", agent.as_str()),
        crate::panes::now(),
    ));
}

/// A Toast's Show: focus that Pane wherever it is.
fn show_pane(pane: &str) {
    let Some(app) = try_app() else { return };
    let source = app.model.borrow().locate(pane).map(|l| l.source);
    let _ = app.update(|m| {
        if let Some(p) = &source {
            m.select_source(p)?;
        }
        m.focus_pane(pane)
    });
}

/// The Agent Overview band a Statusline count opens (R117.1).
fn overview_band(s: rade_core::AgentStatus) -> Option<rade_core::overview::Band> {
    use rade_core::overview::Band;
    use rade_core::AgentStatus::*;
    match s {
        Waiting | Error | Interrupted => Some(Band::NeedsYou),
        Running => Some(Band::Running),
        Done | Exited => None,
    }
}
