//! The `+` menu (PRD R108.6, spec F rows F.1–F.14, F.21): a 340 px popover
//! under the titlebar `plus-square` button (or the tab strip's `+`) that
//! opens a Shell, an Agent under one Agent Account, the Fan-out form or a
//! Browser where the new Pane goes (`New in <Project> / <source> /
//! <worktree>`). The rows come from `rade_core::new_menu`; activating an
//! account row is a `pane.spawn` with `agent` + `account`, so the Workspace
//! lock stays in `rade-core` (`resolve_spawn`).
//!
//! Drawn as an overlay inside the window (like the palette), not a
//! `GtkPopover` surface: `debug.ui geometry` and `debug screenshot` see it,
//! and headless Weston needs no seat grab. A transparent scrim closes it
//! on a click outside.

use crate::app::{app, try_app};
use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::new_menu::{self as core, Row, RowKind};
use rade_core::rpc::{Error, Result};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Popover width (F.1).
const WIDTH: i32 = 340;

pub struct NewMenu {
    overlay: gtk::Overlay,
    scrim: gtk::Box,
    root: gtk::Box,
    header: gtk::Label,
    list: gtk::Box,
    /// Scrolls the rows when they are taller than the window − 80 px (R111.2).
    scroll: gtk::ScrolledWindow,
    /// The width in force (340 px; at large text up to the window − 32).
    width: Cell<i32>,
    rows: RefCell<Vec<(Row, gtk::Button)>>,
    selected: Cell<Option<usize>>,
    open: Cell<bool>,
    /// The button it was opened from (`open` class while it shows).
    anchor: RefCell<Option<gtk::Widget>>,
    workspace: RefCell<String>,
}

thread_local! {
    static MENU: RefCell<Option<Rc<NewMenu>>> = const { RefCell::new(None) };
}

pub fn get() -> Option<Rc<NewMenu>> {
    MENU.with(|m| m.borrow().clone())
}

/// Build the (hidden) menu on top of `overlay`'s child.
pub fn install(overlay: &gtk::Overlay) {
    let scrim = gtk::Box::new(gtk::Orientation::Vertical, 0);
    scrim.add_css_class("new-menu-scrim");
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    scrim.set_visible(false);
    let click = gtk::GestureClick::new();
    click.connect_pressed(|_, _, _, _| {
        if let Some(m) = get() {
            m.close(true);
        }
    });
    scrim.add_controller(click);
    overlay.add_overlay(&scrim);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("new-menu");
    root.set_halign(gtk::Align::Start);
    root.set_valign(gtk::Align::Start);
    root.set_width_request(WIDTH);
    root.set_visible(false);
    let header = gtk::Label::new(None);
    header.add_css_class("new-menu-header");
    header.set_xalign(0.0);
    header.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    root.append(&header);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroll.set_propagate_natural_height(true);
    scroll.set_propagate_natural_width(true);
    scroll.set_child(Some(&list));
    root.append(&scroll);
    overlay.add_overlay(&root);

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(|_, keyval, _, _| {
        let name = match keyval {
            gtk::gdk::Key::Up | gtk::gdk::Key::KP_Up => "up",
            gtk::gdk::Key::Down | gtk::gdk::Key::KP_Down => "down",
            gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter => "enter",
            gtk::gdk::Key::Escape => "escape",
            _ => return gtk::glib::Propagation::Proceed,
        };
        match get() {
            Some(m) if m.key(name) => gtk::glib::Propagation::Stop,
            _ => gtk::glib::Propagation::Proceed,
        }
    });
    root.add_controller(keys);

    let m = Rc::new(NewMenu {
        overlay: overlay.clone(),
        scrim,
        root,
        header,
        list,
        scroll,
        width: Cell::new(WIDTH),
        rows: RefCell::default(),
        selected: Cell::new(None),
        open: Cell::new(false),
        anchor: RefCell::default(),
        workspace: RefCell::default(),
    });
    MENU.with(|s| *s.borrow_mut() = Some(m));
}

/// A click on a `+` button: open under it, or close when open.
pub fn toggle(anchor: &gtk::Widget) {
    let Some(m) = get() else { return };
    if m.open.get() {
        m.close(true);
    } else {
        m.show(anchor);
    }
}

/// The rows and header for the checkout on screen.
fn compute() -> (String, String, Vec<Row>) {
    let app = app();
    let m = app.model.borrow();
    let (s, w) = m.active();
    let linked = !m.is_main_worktree(&s.id, &w.id);
    let header = core::header(
        crate::titlebar::project_name(&m).as_deref(),
        &s.name,
        linked.then_some(w.name.as_str()),
    );
    let ws = s.workspace.clone();
    let rows = core::rows_with(&ws, &crate::accounts::lists_cached(&m), &|a| {
        crate::accounts::menu_detail(a)
    });
    (ws, header, rows)
}

fn icon(name: &str, class: &str) -> gtk::Image {
    let i = gtk::Image::from_icon_name(&format!("ph-{name}-symbolic"));
    i.add_css_class(class);
    i
}

fn label(text: &str, class: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class(class);
    l.set_xalign(0.0);
    l
}

/// One row as a flat button (F.6): badge or icon, name, detail, then key
/// hints, the default `check` or the `lock-simple` at the right.
fn build_row(r: &Row, i: usize, two_line: bool) -> gtk::Button {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    // Large text, a row too wide: the detail goes under the name (F "Text
    // scale 200 %").
    let names = if two_line {
        gtk::Box::new(gtk::Orientation::Vertical, 2)
    } else {
        b.clone()
    };
    match (r.kind, r.agent) {
        // The agent's glyph, no status (R115).
        (RowKind::Account | RowKind::Agent, Some(agent)) => {
            let icon = crate::agent_glyph::icon_of(Some(agent.as_str()));
            let g = crate::agent_glyph::Glyph::plain(agent.into(), icon.as_deref());
            g.add_css_class("new-menu-glyph");
            b.append(&g.root);
        }
        _ => {
            if let Some(n) = r.icon {
                b.append(&icon(n, "new-menu-icon"));
            }
        }
    }
    let name = label(&r.name, "new-menu-item-name");
    names.append(&name);
    if let Some(d) = &r.detail {
        // ≥ 80 %: `wait` with `warning-diamond` (F.10).
        let inner = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        if r.hot {
            inner.append(&icon("warning-diamond-fill", "new-menu-hot"));
        }
        let d = label(d, "new-menu-item-detail");
        d.set_ellipsize(gtk::pango::EllipsizeMode::End);
        if r.hot {
            d.add_css_class("hot");
        }
        inner.append(&d);
        names.append(&inner);
    }
    if two_line {
        b.append(&names);
    }
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    b.append(&spacer);
    if let Some(k) = r.keys {
        let k = label(k, "kbd");
        k.set_valign(gtk::Align::Center);
        b.append(&k);
    }
    if r.default {
        b.append(&icon("check", "new-menu-default"));
    }
    if r.locked {
        b.append(&icon("lock-simple", "new-menu-lock"));
    }
    let button = gtk::Button::new();
    button.set_child(Some(&b));
    button.add_css_class("new-menu-item");
    button.add_css_class("flat");
    if r.default {
        button.add_css_class("default");
    }
    button.set_focus_on_click(false);
    button.set_tooltip_text(r.tooltip.as_deref());
    if r.locked {
        button.set_opacity(0.45);
    }
    button.set_sensitive(r.sensitive);
    button.connect_clicked(move |_| {
        if let Some(m) = get() {
            m.activate(i);
        }
    });
    button
}

impl NewMenu {
    fn show(&self, anchor: &gtk::Widget) {
        let app = app();
        if adw::prelude::AdwApplicationWindowExt::visible_dialog(&app.window).is_some() {
            return;
        }
        if crate::palette::is_open() {
            if let Some(p) = crate::palette::get() {
                p.close(false);
            }
        }
        let shown = std::time::Instant::now();
        let t = std::time::Instant::now();
        let (ws, header, rows) = compute();
        crate::perf::record("new_menu_compute", t.elapsed(), false);
        *self.workspace.borrow_mut() = ws;
        self.header.set_label(&header);
        while let Some(c) = self.list.first_child() {
            self.list.remove(&c);
        }
        let mut built = Vec::new();
        let mut section = "";
        let large = crate::layout_mode::mode().large_text;
        let room = crate::layout_mode::window_width() - 32;
        let mut widest = 0;
        for (i, r) in rows.into_iter().enumerate() {
            if r.section != section && !r.section.is_empty() {
                let s = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                s.add_css_class("new-menu-section");
                s.append(&label(r.section, "section-label"));
                self.list.append(&s);
            }
            section = r.section;
            let mut b = build_row(&r, i, false);
            self.list.append(&b);
            if large {
                let nat = b.measure(gtk::Orientation::Horizontal, -1).1;
                if nat > room {
                    self.list.remove(&b);
                    b = build_row(&r, i, true);
                    self.list.append(&b);
                }
                widest = widest.max(b.measure(gtk::Orientation::Horizontal, -1).1);
            }
            built.push((r, b));
        }
        *self.rows.borrow_mut() = built;
        // 340 px; at large text as wide as the longest row, at most the
        // window − 32 px, scrolling past the window − 80 px (R111.2).
        let w = if large {
            widest.clamp(WIDTH, room.max(WIDTH))
        } else {
            WIDTH
        };
        self.width.set(w);
        self.root.set_width_request(w);
        let win_h = crate::layout_mode::window_height();
        let header_h = self.header.measure(gtk::Orientation::Vertical, w).1;
        self.scroll.set_max_content_height(if win_h > 0 {
            (win_h - 80 - header_h).max(100)
        } else {
            -1
        });
        self.place(anchor);
        if let Some(old) = self.anchor.replace(Some(anchor.clone())) {
            old.remove_css_class("open");
        }
        anchor.add_css_class("open");
        self.scrim.set_visible(true);
        self.root.set_visible(true);
        self.open.set(true);
        let first = core::initial(&self.row_data());
        self.select(first);
        crate::perf::record("new_menu_show", shown.elapsed(), false);
        crate::perf::ready("new_menu", &self.root);
    }

    /// F.1 / F.2: top at the titlebar bottom (for the titlebar button; the
    /// anchor's bottom otherwise); horizontally over the anchor's centre,
    /// its right edge 50 px right of it (the mockup), kept 8 px inside the
    /// window.
    fn place(&self, anchor: &gtk::Widget) {
        let over = self.overlay.upcast_ref::<gtk::Widget>();
        let Some(b) = anchor.compute_bounds(over) else {
            return;
        };
        let top = anchor
            .ancestor(gtk::WindowHandle::static_type())
            .filter(|t| t.has_css_class("titlebar"))
            .and_then(|t| t.compute_bounds(over))
            .map_or(b.y() + b.height(), |t| t.y() + t.height());
        let centre = b.x() + b.width() / 2.0;
        let width = self.width.get();
        let max = (over.width() - 8 - width).max(8) as f32;
        let x = (centre + 50.0 - width as f32).clamp(8.0, max);
        self.root.set_margin_start(x.round() as i32);
        self.root.set_margin_top(top.round() as i32);
    }

    fn row_data(&self) -> Vec<Row> {
        self.rows.borrow().iter().map(|(r, _)| r.clone()).collect()
    }

    /// The keyboard selection (F.8 style) on row `i`, with focus.
    fn select(&self, i: Option<usize>) {
        let rows = self.rows.borrow();
        for (j, (_, b)) in rows.iter().enumerate() {
            if Some(j) == i {
                b.add_css_class("selected");
                b.grab_focus();
            } else {
                b.remove_css_class("selected");
            }
        }
        self.selected.set(i);
    }

    pub fn close(&self, restore: bool) {
        if !self.open.get() {
            return;
        }
        self.open.set(false);
        self.root.set_visible(false);
        self.scrim.set_visible(false);
        if let Some(a) = self.anchor.take() {
            a.remove_css_class("open");
        }
        if restore {
            if let Some(app) = try_app() {
                app.restore_focus();
            }
        }
    }

    /// `↑` `↓` `Enter` `Esc`; false = not handled.
    pub fn key(&self, name: &str) -> bool {
        if !self.open.get() {
            return false;
        }
        match name {
            "up" | "down" => {
                let to = core::step(&self.row_data(), self.selected.get(), name == "down");
                self.select(to);
            }
            "enter" => {
                if let Some(i) = self.selected.get() {
                    self.activate(i);
                }
            }
            "escape" => self.close(true),
            _ => return false,
        }
        true
    }

    /// Run row `i` (an insensitive one does nothing): false = not run.
    fn activate(&self, i: usize) -> bool {
        let Some(r) = self.rows.borrow().get(i).map(|(r, _)| r.clone()) else {
            return false;
        };
        if !r.sensitive {
            return false;
        }
        self.close(true);
        let app = app();
        match r.kind {
            RowKind::Shell => crate::keys::run_action("new_tab"),
            RowKind::Browser => crate::keys::run_action("open_browser"),
            RowKind::FanOut => crate::palette_run::open_form("fanout"),
            RowKind::Account | RowKind::Agent => {
                let s = crate::handlers::Spawn {
                    agent: r.agent.map(|a| a.as_str().to_string()),
                    account: (r.kind == RowKind::Account).then(|| r.name.clone()),
                    tab: Some("new".into()),
                    ..Default::default()
                };
                let before = app.model.borrow().active_tab().map(|t| t.id.clone());
                match crate::handlers::spawn(&app, s) {
                    Err(e) => crate::worktrees::alert("Could not start the Agent", &e.message),
                    // Right after the Tab it was opened from (R19.7).
                    Ok(v) => {
                        if let (Some(after), Some(tab)) = (before, v["tab"].as_str()) {
                            app.update(|m| m.move_tab_after(tab, &after));
                        }
                    }
                }
            }
        }
        true
    }

    fn info(&self) -> Value {
        let rows: Vec<Row> = if self.open.get() {
            self.row_data()
        } else {
            compute().2
        };
        json!({
            "open": self.open.get(),
            "header": self.header.label().as_str(),
            "workspace": self.workspace.borrow().as_str(),
            "selected": self.selected.get().filter(|_| self.open.get()),
            "rows": rows.iter().enumerate().map(|(i, r)| {
                let shown = self.rows.borrow().get(i).map(|(_, b)| b.clone()).filter(|_| self.open.get());
                json!({
                    "kind": r.kind, "section": r.section, "name": r.name,
                    "detail": r.detail, "keys": r.keys, "icon": r.icon,
                    "agent": r.agent, "workspace": r.workspace,
                    "default": r.default, "locked": r.locked, "hot": r.hot,
                    "sensitive": shown.as_ref().map_or(r.sensitive, |b| b.is_sensitive()),
                    "tooltip": r.tooltip,
                })
            }).collect::<Vec<_>>(),
        })
    }
}

/// `debug.ui {"op": "new_menu", "arg"?}`: the rows (kind, name, detail,
/// default, locked, sensitive) and whether it is open. `arg`: `open` (from
/// the titlebar button), `open_tab` (from the tab strip's `+`), `close`,
/// `key` + `key` (`up|down|enter|escape`), `activate` + `name` (+ `agent`).
pub fn debug(p: &Value) -> Result<Value> {
    let m = get().ok_or_else(|| Error::internal("no + menu"))?;
    let s = |k: &str| p.get(k).and_then(Value::as_str);
    let win = app().window.clone();
    let first = |sel: &str| {
        crate::geometry::find(win.upcast_ref(), sel)
            .into_iter()
            .find(|w| w.is_mapped())
            .ok_or_else(|| Error::not_found(format!("no shown '{sel}'")))
    };
    let mut out = json!({});
    match s("arg").unwrap_or("") {
        "" => {}
        "open" => {
            m.close(false);
            m.show(&first("tb-button-new")?);
        }
        "open_tab" => {
            m.close(false);
            m.show(&first("tab-add")?);
        }
        "close" => m.close(true),
        "key" => out["handled"] = json!(m.key(s("key").unwrap_or(""))),
        "activate" => {
            let name = s("name").unwrap_or("");
            let agent = s("agent");
            let i = m
                .row_data()
                .iter()
                .position(|r| {
                    r.name == name && agent.is_none_or(|a| r.agent.is_some_and(|k| k.as_str() == a))
                })
                .ok_or_else(|| {
                    Error::not_found(format!("no + menu row '{name}' (open it first)"))
                })?;
            out["activated"] = json!(m.activate(i));
        }
        a => return Err(Error::usage(format!("unknown new_menu arg '{a}'"))),
    }
    out["new_menu"] = m.info();
    Ok(out)
}
