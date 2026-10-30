//! The Schedules page (PRD R139): the main-area page the Statusline cell
//! `⏱ n`, the palette (`> schedules`, `New Schedule…`) and a Project's
//! Navigator menu open. A list grouped by Project then Scratch (filters:
//! Workspace, Project, on / off) and the selected Schedule's detail (fields,
//! the next 5 times, its Runs with Resume, Run now / Edit / On-Off /
//! Delete…), plus the New / Edit Schedule form.
//!
//! Everything shown is the `schedule.*` answers (the same as `rade
//! schedule`), asked off the main loop: those methods wait on it.
//! `debug.ui {"op": "schedules"}` reads and drives it.

use crate::app::try_app;
use adw::prelude::*;
use gtk::glib;
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::rpc::{Error, Result};
use rade_core::schedule::{check_name, fmt_time, Cron};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The page's name in the center stack.
pub const PAGE: &str = "schedules";

type Focus = (String, String, Option<String>);

/// What the viewer chose.
#[derive(Clone, Debug, Default, PartialEq)]
struct Choice {
    /// `None` = the shown Workspace; `Some("")` = every Workspace.
    workspace: Option<String>,
    /// `None` = every Project; `Some("")` = Scratch only.
    project: Option<String>,
    /// `None` = on and off.
    on: Option<bool>,
    /// (workspace, name).
    selected: Option<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Data {
    choice: Choice,
    shown_workspace: String,
    workspaces: Vec<(String, String)>,
    /// The filtered rows.
    rows: Vec<Value>,
    /// Project id → name of the shown Workspace(s), for group titles.
    projects: Vec<(String, String, String)>,
    detail: Option<Value>,
    error: Option<String>,
    /// The detail goes under the list (narrow window or large text).
    below: bool,
    large: bool,
}

thread_local! {
    static SHOWN: RefCell<Option<Focus>> = const { RefCell::new(None) };
    static ROOT: RefCell<Option<gtk::ScrolledWindow>> = const { RefCell::new(None) };
    static CHOICE: RefCell<Choice> = RefCell::new(Choice::default());
    static DATA: RefCell<Option<Data>> = const { RefCell::new(None) };
    static RENDERED: RefCell<Option<Data>> = const { RefCell::new(None) };
    static GEN: Cell<u64> = const { Cell::new(0) };
    static FORM: RefCell<Option<Rc<Form>>> = const { RefCell::new(None) };
    static DELETE: RefCell<Option<adw::AlertDialog>> = const { RefCell::new(None) };
    static BUILT: Cell<(bool, bool)> = const { Cell::new((false, false)) };
}

/// The page's padding left and right, the gap, the detail's width and the
/// list's least width.
const PAGE_PAD: i32 = 48;
const LIST_MIN: i32 = 660;
const DETAIL_W: i32 = 400;

/// (detail below the list, large text) now (R111).
fn shape() -> (bool, bool) {
    let mode = crate::layout_mode::mode();
    let content = crate::layout_mode::center_width() - PAGE_PAD;
    (
        mode.large_text || (content > 0 && content - 16 - DETAIL_W < LIST_MIN),
        mode.large_text,
    )
}

/// The window's width or text scale changed: build again when the shape did.
pub fn relayout() {
    if BUILT.with(Cell::get) != shape() {
        refresh();
    }
}

fn focus_key(m: &rade_core::State) -> Focus {
    let (s, w) = m.active();
    (
        s.id.clone(),
        w.id.clone(),
        m.active_tab().map(|t| t.id.clone()),
    )
}

/// The page widget, added to the center stack by `main.rs`.
pub fn install() -> gtk::ScrolledWindow {
    let sc = gtk::ScrolledWindow::new();
    sc.set_hscrollbar_policy(gtk::PolicyType::Automatic);
    sc.add_css_class("usage-scroll");
    sc.add_css_class("sched-scroll");
    // Up / Down move the selection.
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(|_, k, _, _| {
        let down = match k {
            gtk::gdk::Key::Down | gtk::gdk::Key::KP_Down => true,
            gtk::gdk::Key::Up | gtk::gdk::Key::KP_Up => false,
            _ => return glib::Propagation::Proceed,
        };
        step(down);
        glib::Propagation::Stop
    });
    sc.add_controller(keys);
    ROOT.with(|r| *r.borrow_mut() = Some(sc.clone()));
    // Next times and Run statuses stay fresh.
    glib::timeout_add_seconds_local(15, || {
        refresh();
        glib::ControlFlow::Continue
    });
    sc
}

/// Whether the page shows (closes it when the focus moved since it opened).
pub fn is_shown(m: &rade_core::State) -> bool {
    let now = focus_key(m);
    SHOWN.with(|s| {
        let mut s = s.borrow_mut();
        match &*s {
            Some(at) if *at == now => true,
            Some(_) => {
                *s = None;
                false
            }
            None => false,
        }
    })
}

pub(crate) fn shown_now() -> bool {
    try_app().is_some_and(|a| is_shown(&a.model.borrow()))
}

/// Open the page, on a Schedule when given.
pub fn open(on: Option<(&str, &str)>) {
    let Some(app) = try_app() else { return };
    crate::home_view::close();
    crate::usage_view::close();
    if let Some((ws, name)) = on {
        CHOICE.with(|c| {
            let mut c = c.borrow_mut();
            c.selected = Some((ws.to_string(), name.to_string()));
            if ws != app.model.borrow().active_workspace() {
                c.workspace = Some(String::new());
            }
        });
    }
    let at = focus_key(&app.model.borrow());
    SHOWN.with(|s| *s.borrow_mut() = Some(at));
    app.sync();
    refresh();
    if let Some(r) = ROOT.with(|r| r.borrow().clone()) {
        r.grab_focus();
    }
}

/// Open the page filtered to Project `project` (its Navigator menu).
pub fn open_project(project: &str) {
    CHOICE.with(|c| {
        let mut c = c.borrow_mut();
        c.workspace = None;
        c.project = Some(project.to_string());
    });
    open(None);
}

/// Leave the page (a Navigator row was clicked).
pub fn close() {
    SHOWN.with(|s| s.borrow_mut().take());
}

/// ✕ or Esc: back to the Pane that had the focus.
pub(crate) fn leave() {
    close();
    if let Some(app) = try_app() {
        app.sync();
        app.restore_focus();
    }
}

/// Select the row under / over the selected one.
fn step(down: bool) {
    let Some(d) = DATA.with(|d| d.borrow().clone()) else {
        return;
    };
    let keys: Vec<(String, String)> = groups(&d)
        .into_iter()
        .flat_map(|(_, rows)| rows)
        .map(|r| (s(&r["workspace"]).to_string(), s(&r["name"]).to_string()))
        .collect();
    let at = keys
        .iter()
        .position(|k| Some(k) == d.choice.selected.as_ref());
    let to = match (at, down) {
        (Some(i), true) => (i + 1).min(keys.len().saturating_sub(1)),
        (Some(i), false) => i.saturating_sub(1),
        (None, _) => 0,
    };
    if let Some(k) = keys.get(to).cloned() {
        set_choice(move |c| c.selected = Some(k));
    }
}

fn call(method: &str, p: Value) -> std::result::Result<Value, String> {
    crate::schedules::dispatch(method, p).map_err(|e| e.message)
}

/// Run `method` off the main loop, then `done` on it.
fn ask(
    method: &'static str,
    p: Value,
    done: impl FnOnce(std::result::Result<Value, String>) + 'static,
) {
    let done = glib::thread_guard::ThreadGuard::new(done);
    std::thread::spawn(move || {
        let r = call(method, p);
        glib::MainContext::default().invoke(move || (done.into_inner())(r));
    });
}

/// Load again (off the main loop) and redraw when it changed.
pub fn refresh() {
    let Some(app) = try_app() else { return };
    if !shown_now() {
        return;
    }
    let choice = CHOICE.with(|c| c.borrow().clone());
    let (shown_ws, workspaces, projects) = {
        let m = app.model.borrow();
        let names: Vec<(String, String)> = m
            .workspace_looks()
            .into_iter()
            .map(|l| (l.id, l.name))
            .collect();
        let projects: Vec<(String, String, String)> = m
            .workspaces
            .iter()
            .flat_map(|w| {
                rade_core::project::list(&w.root)
                    .into_iter()
                    .map(|p| (w.id.clone(), p.id, p.name))
                    .collect::<Vec<_>>()
            })
            .collect();
        (m.active_workspace().to_string(), names, projects)
    };
    let gen = GEN.with(|g| {
        g.set(g.get() + 1);
        g.get()
    });
    let (below, large) = shape();
    std::thread::spawn(move || {
        let mut data = load(choice, shown_ws, workspaces, projects);
        data.below = below;
        data.large = large;
        glib::MainContext::default().invoke(move || {
            if GEN.with(|g| g.get()) != gen {
                return;
            }
            DATA.with(|d| *d.borrow_mut() = Some(data));
            render();
        });
    });
}

fn load(
    choice: Choice,
    shown_ws: String,
    workspaces: Vec<(String, String)>,
    projects: Vec<(String, String, String)>,
) -> Data {
    let ws = match &choice.workspace {
        Some(w) if w.is_empty() => None,
        Some(w) => Some(w.clone()),
        None => Some(shown_ws.clone()),
    };
    let mut p = match &ws {
        Some(w) => json!({"workspace": w}),
        None => json!({"all": true}),
    };
    match choice.project.as_deref() {
        Some("") => p["scratch"] = json!(true),
        Some(id) => p["project"] = json!(id),
        None => {}
    }
    let (rows, error) = match call("schedule.list", p) {
        Ok(v) => (
            v["schedules"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|r| {
                    choice
                        .on
                        .is_none_or(|on| r["enabled"].as_bool() == Some(on))
                })
                .collect::<Vec<_>>(),
            None,
        ),
        Err(e) => (Vec::new(), Some(e)),
    };
    let key = |r: &Value| (s(&r["workspace"]).to_string(), s(&r["name"]).to_string());
    let selected = choice
        .selected
        .clone()
        .filter(|sel| rows.iter().any(|r| key(r) == *sel))
        .or_else(|| rows.first().map(key));
    let detail = selected
        .as_ref()
        .and_then(|(w, n)| call("schedule.get", json!({"workspace": w, "name": n})).ok());
    let mut choice = choice;
    choice.selected = selected;
    Data {
        choice,
        shown_workspace: shown_ws,
        workspaces,
        rows,
        projects,
        detail,
        error,
        below: false,
        large: false,
    }
}

fn set_choice(f: impl FnOnce(&mut Choice)) {
    CHOICE.with(|c| f(&mut c.borrow_mut()));
    refresh();
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

fn render() {
    let Some(root) = ROOT.with(|r| r.borrow().clone()) else {
        return;
    };
    let Some(d) = DATA.with(|d| d.borrow().clone()) else {
        return;
    };
    if RENDERED.with(|r| r.borrow().as_ref() == Some(&d)) {
        return;
    }
    BUILT.with(|b| b.set((d.below, d.large)));
    root.set_child(Some(&build(&d)));
    RENDERED.with(|r| *r.borrow_mut() = Some(d));
}

fn now() -> i64 {
    crate::schedules::now()
}

/// `Oct 31 02:00`, or `—`.
fn time(v: &Value) -> String {
    v.as_i64()
        .map(|t| fmt_time(t, now(), crate::schedules::tz()))
        .unwrap_or_else(|| "—".into())
}

/// `on` / `off` / `done` / `failed` (the last Run failed): the row's LED.
fn state(r: &Value) -> &'static str {
    if r["last"]["outcome"].as_str() == Some("failed") {
        "failed"
    } else if r["done"].as_bool() == Some(true) {
        "done"
    } else if r["enabled"].as_bool() == Some(true) {
        "on"
    } else {
        "off"
    }
}

fn led(state: &str) -> gtk::Box {
    let l = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    l.add_css_class("led");
    l.add_css_class(match state {
        // Armed, not running: the accent, no pulse.
        "on" => "led-sched-on",
        "failed" => "led-error",
        "done" => "led-done",
        // `off`: the hollow LED.
        _ => "led-sched-off",
    });
    l.set_valign(gtk::Align::Center);
    l
}

fn account_text(r: &Value) -> String {
    match r["account"].as_str() {
        Some(a) => a.to_string(),
        None => match r["account_now"].as_str() {
            Some(a) => format!("Default ({a})"),
            None => "Default".into(),
        },
    }
}

/// The groups of R139.1: (title, rows) by Project in list order, then Scratch.
fn groups(d: &Data) -> Vec<(String, Vec<&Value>)> {
    let all = d.choice.workspace.as_deref() == Some("");
    let mut out: Vec<(String, Vec<&Value>)> = Vec::new();
    for r in &d.rows {
        let ws = s(&r["workspace"]);
        let title = match r["project"].as_str() {
            Some(p) => d
                .projects
                .iter()
                .find(|(w, id, _)| w == ws && id == p)
                .map_or_else(|| p.to_string(), |x| x.2.clone()),
            None => "Scratch".into(),
        };
        let title = if all {
            let wname = d
                .workspaces
                .iter()
                .find(|(id, _)| id == ws)
                .map_or(ws.to_string(), |x| x.1.clone());
            format!("{wname} · {title}")
        } else {
            title
        };
        match out.last_mut() {
            Some((t, rows)) if *t == title => rows.push(r),
            _ => out.push((title, vec![r])),
        }
    }
    out
}

fn icon(name: &str, classes: &[&str]) -> gtk::Image {
    let i = gtk::Image::from_icon_name(name);
    for c in classes {
        i.add_css_class(c);
    }
    i
}

/// A secondary button (the Usage page's, A.15): icon, text, optional caret.
fn secondary(icon_name: &str, text: &str, class: &str, caret: bool) -> gtk::Button {
    let b = gtk::Button::new();
    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    if !icon_name.is_empty() {
        inner.append(&icon(icon_name, &["btn-icon"]));
    }
    if !text.is_empty() {
        inner.append(&gtk::Label::new(Some(text)));
    }
    if caret {
        inner.append(&icon("ph-caret-down-symbolic", &["btn-caret"]));
    }
    b.set_child(Some(&inner));
    b.add_css_class("secondary-button");
    b.add_css_class(class);
    b.set_valign(gtk::Align::Center);
    b
}

type Items = Vec<(String, Box<dyn Fn()>)>;

/// A menu of `items` under `anchor`; a `!` first in a text = destructive.
fn popover(anchor: &gtk::Button, items: Rc<Items>) {
    let pop = gtk::Popover::new();
    pop.add_css_class("menu");
    let col = gtk::Box::new(gtk::Orientation::Vertical, 0);
    for (i, (t, _)) in items.iter().enumerate() {
        let row = gtk::Button::with_label(t.trim_start_matches('!'));
        row.add_css_class("flat");
        if t.starts_with('!') {
            row.add_css_class("sched-menu-danger");
        }
        let (items, pop2) = (items.clone(), pop.clone());
        row.connect_clicked(move |_| {
            pop2.popdown();
            (items[i].1)();
        });
        col.append(&row);
    }
    pop.set_child(Some(&col));
    pop.set_parent(anchor);
    pop.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    pop.popup();
}

fn menu_button(icon_name: &str, text: &str, class: &str, items: Items) -> gtk::Button {
    let b = secondary(icon_name, text, class, true);
    b.add_css_class("sched-filter");
    let items = Rc::new(items);
    b.connect_clicked(move |anchor| popover(anchor, items.clone()));
    b
}

/// `in 2h 10m`.
fn rel(t: i64) -> String {
    rade_core::schedule::relative(t, now())
}

fn header(d: &Data) -> gtk::Widget {
    let h = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    h.add_css_class("usage-header");
    h.append(&icon("ph-clock-symbolic", &["usage-icon"]));
    let titles = gtk::Box::new(gtk::Orientation::Vertical, 2);
    titles.append(&label("Schedules", &["usage-title", "sched-title"]));
    // Switched on (a failed last Run is still on), like the Statusline cell.
    let on = d
        .rows
        .iter()
        .filter(|r| r["enabled"].as_bool() == Some(true) && r["done"].as_bool() != Some(true))
        .count();
    let next = d
        .rows
        .iter()
        .filter_map(|r| Some((r["next"].as_i64()?, s(&r["name"]))))
        .min();
    let total = d.rows.len();
    let sub = match next {
        Some((t, n)) => format!("{total} schedules · {on} on · next {n} {}", rel(t)),
        None => format!("{total} schedules · {on} on"),
    };
    titles.append(&label(&sub, &["usage-sub", "sched-sub"]));
    titles.set_hexpand(true);
    titles.set_valign(gtk::Align::Center);
    h.append(&titles);

    // On / off: a segmented control like the Usage page's period.
    let seg = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    seg.add_css_class("usage-period");
    seg.add_css_class("sched-filter-on");
    seg.set_valign(gtk::Align::Center);
    for (v, text) in [(None, "All"), (Some(true), "On"), (Some(false), "Off")] {
        let b = gtk::Button::with_label(text);
        b.add_css_class("usage-period-seg");
        b.add_css_class("flat");
        if d.choice.on == v {
            b.add_css_class("active");
        }
        b.set_focus_on_click(false);
        b.connect_clicked(move |_| set_choice(move |c| c.on = v));
        seg.append(&b);
    }

    let ws_label = match d.choice.workspace.as_deref() {
        Some("") => "All workspaces".to_string(),
        Some(w) => ws_name(d, w),
        None => ws_name(d, &d.shown_workspace),
    };
    let mut ws_items: Items = vec![(
        "All workspaces".into(),
        Box::new(|| {
            set_choice(|c| {
                c.workspace = Some(String::new());
                c.project = None;
            })
        }),
    )];
    for (id, name) in &d.workspaces {
        let id = id.clone();
        let mine = id == d.shown_workspace;
        ws_items.push((
            name.clone(),
            Box::new(move || {
                let id = id.clone();
                set_choice(move |c| {
                    c.workspace = if mine { None } else { Some(id) };
                    c.project = None;
                })
            }),
        ));
    }
    let ws = menu_button(
        "ph-circles-four-symbolic",
        &ws_label,
        "sched-filter-ws",
        ws_items,
    );

    let proj_label = match d.choice.project.as_deref() {
        None => "All projects".to_string(),
        Some("") => "Scratch".to_string(),
        Some(p) => d
            .projects
            .iter()
            .find(|x| x.1 == p)
            .map_or(p.to_string(), |x| x.2.clone()),
    };
    let ws_now = match d.choice.workspace.as_deref() {
        Some("") => None,
        Some(w) => Some(w.to_string()),
        None => Some(d.shown_workspace.clone()),
    };
    let mut p_items: Items = vec![(
        "All projects".into(),
        Box::new(|| set_choice(|c| c.project = None)),
    )];
    for (w, id, name) in &d.projects {
        if ws_now.as_ref().is_some_and(|x| x != w) {
            continue;
        }
        let id = id.clone();
        p_items.push((
            name.clone(),
            Box::new(move || {
                let id = id.clone();
                set_choice(move |c| c.project = Some(id))
            }),
        ));
    }
    p_items.push((
        "Scratch".into(),
        Box::new(|| set_choice(|c| c.project = Some(String::new()))),
    ));
    let proj = menu_button(
        "ph-cube-symbolic",
        &proj_label,
        "sched-filter-project",
        p_items,
    );
    let new = secondary("ph-plus-symbolic", "New schedule", "sched-new", false);
    new.set_tooltip_text(Some("New Schedule…"));
    new.connect_clicked(|_| new_form(None));
    let close = gtk::Button::from_icon_name("ph-x-symbolic");
    close.add_css_class("dialog-close");
    close.add_css_class("flat");
    close.add_css_class("usage-close");
    close.add_css_class("sched-close");
    close.set_tooltip_text(Some("Close (Esc)"));
    close.set_valign(gtk::Align::Center);
    close.connect_clicked(|_| leave());
    let controls: [&gtk::Widget; 4] = [
        seg.upcast_ref(),
        ws.upcast_ref(),
        proj.upcast_ref(),
        new.upcast_ref(),
    ];
    if d.large {
        // Large text: the controls go under the title (no sideways scroll).
        h.append(&close);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.add_css_class("usage-header-controls");
        for w in controls {
            row.append(w);
        }
        let top = gtk::Box::new(gtk::Orientation::Vertical, 10);
        top.append(&h);
        top.append(&row);
        return top.upcast();
    }
    for w in controls {
        h.append(w);
    }
    h.append(&close);
    h.upcast()
}

fn ws_name(d: &Data, id: &str) -> String {
    d.workspaces
        .iter()
        .find(|x| x.0 == id)
        .map_or(id.to_string(), |x| x.1.clone())
}

/// The list's columns (head text, width; -1 = the rest).
const COLUMNS: [(&str, i32); 5] = [
    ("SCHEDULE", -1),
    ("WHEN", 150),
    ("AGENT", 130),
    ("LAST RUN", 120),
    ("NEXT", 110),
];

/// A column's width; at large text it grows with the text (R111.2).
fn sized(w: &impl IsA<gtk::Widget>, width: i32) {
    let px = if crate::layout_mode::mode().large_text {
        (f64::from(width) * crate::layout_mode::text_scale()).round() as i32
    } else {
        width
    };
    w.set_size_request(px, -1);
}

fn badge(agent: &str, size: i32) -> gtk::Label {
    let b = gtk::Label::new(Some(match agent {
        "codex" => "X",
        "claude" => "C",
        a => &a[..a.len().min(1)],
    }));
    b.add_css_class("agent-badge");
    b.add_css_class(&format!("agent-badge-{agent}"));
    b.set_size_request(size, size);
    b.set_valign(gtk::Align::Center);
    b.set_halign(gtk::Align::Center);
    b
}

fn build(d: &Data) -> gtk::Widget {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.add_css_class("usage-page");
    page.add_css_class("sched-page");
    page.append(&header(d));
    if let Some(e) = &d.error {
        page.append(&label(e, &["sched-error", "field-error"]));
    }
    if d.rows.is_empty() {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
        card.add_css_class("usage-accounts-card");
        card.add_css_class("sched-empty");
        card.append(&icon("ph-clock-symbolic", &["sched-empty-icon"]));
        let filtered = d.choice.project.is_some() || d.choice.on.is_some();
        card.append(&label(
            if filtered {
                "No schedules match these filters"
            } else {
                "No schedules yet"
            },
            &["sched-empty-title"],
        ));
        let hint = label(
            "A schedule starts an Agent with a prompt — on a cron expression, or once at a time.\nIt runs in a Project or a folder, while RADE is open.",
            &["sched-empty-hint"],
        );
        hint.set_justify(gtk::Justification::Center);
        hint.set_ellipsize(gtk::pango::EllipsizeMode::None);
        card.append(&hint);
        let b = secondary("ph-plus-symbolic", "New schedule", "sched-empty-new", false);
        b.set_halign(gtk::Align::Center);
        b.connect_clicked(|_| new_form(None));
        card.append(&b);
        let mut c = card.first_child();
        while let Some(w) = c {
            w.set_halign(gtk::Align::Center);
            c = w.next_sibling();
        }
        let grid = gtk::Box::new(gtk::Orientation::Vertical, 0);
        grid.add_css_class("usage-grid");
        grid.append(&card);
        page.append(&grid);
        return page.upcast();
    }
    let body = gtk::Box::new(
        if d.below {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        },
        16,
    );
    body.add_css_class("usage-grid");
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list.add_css_class("usage-accounts-card");
    list.add_css_class("sched-list");
    list.set_hexpand(true);
    list.set_valign(gtk::Align::Start);
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    head.add_css_class("usage-table-head");
    for (text, w) in COLUMNS {
        let l = label(text, &["usage-col"]);
        if w < 0 {
            l.set_hexpand(true);
            // The LED's room, so the head lines up with the names.
            l.set_margin_start(13);
        } else {
            sized(&l, w);
        }
        if text == "NEXT" {
            l.set_xalign(1.0);
        }
        head.append(&l);
    }
    list.append(&head);
    for (title, rows) in groups(d) {
        list.append(&group_row(
            &title,
            rows.first().is_some_and(|r| r["project"].is_null()),
            rows.len(),
        ));
        for r in rows {
            list.append(&row(d, r));
        }
    }
    body.append(&list);
    if let Some(det) = &d.detail {
        let card = detail(det);
        if d.below {
            card.set_size_request(-1, -1);
            card.set_hexpand(true);
        }
        body.append(&card);
    }
    page.append(&body);
    page.upcast()
}

/// A group's title row: the Project (cube) or Scratch (terminal), a count.
fn group_row(title: &str, scratch: bool, n: usize) -> gtk::Box {
    let g = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    g.add_css_class("sched-group");
    g.append(&icon(
        if scratch {
            "ph-terminal-window-symbolic"
        } else {
            "ph-cube-symbolic"
        },
        &["sched-group-icon"],
    ));
    g.append(&label(title, &["sched-group-title"]));
    g.append(&label(&n.to_string(), &["sched-group-count"]));
    g
}

fn row(d: &Data, r: &Value) -> gtk::Box {
    let key = (s(&r["workspace"]).to_string(), s(&r["name"]).to_string());
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.add_css_class("usage-row");
    b.add_css_class("sched-row");
    b.set_widget_name(&format!("sched-row-{}", key.1));
    if d.choice.selected.as_ref() == Some(&key) {
        b.add_css_class("selected");
    }
    let st = state(r);
    if st == "off" || st == "done" {
        b.add_css_class("sched-row-off");
    }
    let cell = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    cell.add_css_class("usage-cell");
    cell.add_css_class("usage-cell-account");
    cell.append(&led(st));
    cell.append(&label(s(&r["name"]), &["usage-account-name", "sched-name"]));
    cell.set_hexpand(true);
    cell.set_size_request(120, -1);
    b.append(&cell);

    let when = label(s(&r["when_text"]), &["usage-cell", "sched-when"]);
    when.set_tooltip_text(r["cron"].as_str());
    sized(&when, COLUMNS[1].1);
    b.append(&when);

    let who = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    who.add_css_class("usage-cell");
    who.append(&badge(s(&r["agent"]), 16));
    let acct = match r["account"].as_str() {
        Some(a) => label(a, &["sched-account"]),
        None => label("default", &["sched-account", "sched-dim"]),
    };
    acct.set_tooltip_text(Some(&account_text(r)));
    who.append(&acct);
    sized(&who, COLUMNS[2].1);
    b.append(&who);

    let last = match (r["last"]["at"].as_i64(), s(&r["last"]["outcome"])) {
        // A past time, even a few seconds ahead of the clock.
        (Some(t), "started") => label(&rel(t.min(now())), &["usage-cell", "sched-mono"]),
        (Some(t), o) => label(
            &format!("{o} · {}", rel(t.min(now()))),
            &["usage-cell", "sched-mono", &format!("sched-last-{o}")],
        ),
        _ => label("—", &["usage-cell", "sched-mono", "sched-dim"]),
    };
    if let Some(t) = r["last"]["at"].as_i64() {
        last.set_tooltip_text(Some(&format!(
            "Run #{} · {}",
            r["last"]["n"],
            time(&json!(t))
        )));
    }
    sized(&last, COLUMNS[3].1);
    b.append(&last);

    let next = match (st, r["next"].as_i64()) {
        ("off", _) => label("off", &["usage-cell", "sched-mono", "sched-dim"]),
        ("done", _) => label("done", &["usage-cell", "sched-mono", "sched-dim"]),
        (_, Some(t)) => {
            let l = label(&rel(t), &["usage-cell", "sched-mono", "sched-next"]);
            if t - now() < 3600 {
                l.add_css_class("soon");
            }
            l.set_tooltip_text(Some(&time(&json!(t))));
            l
        }
        _ => label("—", &["usage-cell", "sched-mono", "sched-dim"]),
    };
    next.set_xalign(1.0);
    sized(&next, COLUMNS[4].1);
    b.append(&next);
    let click = gtk::GestureClick::new();
    click.connect_released(move |_, n, _, _| {
        let key = key.clone();
        if n == 2 {
            // Double-click: Edit.
            new_form(Some(key));
            return;
        }
        set_choice(move |c| c.selected = Some(key));
    });
    b.add_controller(click);
    b
}

/// One `name  value` line of the detail.
fn kv(card: &gtk::Box, name: &str, value: &gtk::Widget) {
    let r = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    r.add_css_class("sched-kv");
    let n = label(name, &["sched-kv-name"]);
    n.set_valign(gtk::Align::Start);
    sized(&n, 84);
    r.append(&n);
    value.set_hexpand(true);
    r.append(value);
    card.append(&r);
}

fn kv_text(card: &gtk::Box, name: &str, value: &str) {
    let v = label(value, &["sched-kv-value"]);
    v.set_wrap(true);
    v.set_ellipsize(gtk::pango::EllipsizeMode::None);
    kv(card, name, v.upcast_ref());
}

fn section_head(card: &gtk::Box, title: &str, note: Option<&str>) {
    let h = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    h.add_css_class("usage-limit-group");
    let l = label(title, &["section-label"]);
    l.set_hexpand(true);
    h.append(&l);
    if let Some(n) = note {
        h.append(&label(n, &["usage-chart-note"]));
    }
    card.append(&h);
}

fn detail(det: &Value) -> gtk::Box {
    let ws = s(&det["workspace"]).to_string();
    let name = s(&det["name"]).to_string();
    let st = state(det);
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("usage-detail");
    card.add_css_class("sched-detail");
    sized(&card, 400);
    card.set_valign(gtk::Align::Start);

    // Head: agent, name, where; the on / off switch; ⋯ (Edit, Delete).
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    head.add_css_class("usage-detail-header");
    head.append(&badge(s(&det["agent"]), 24));
    let titles = gtk::Box::new(gtk::Orientation::Vertical, 1);
    titles.append(&label(&name, &["usage-detail-name", "sched-detail-name"]));
    let target = match (det["project"].as_str(), det["dir"].as_str()) {
        (Some(p), _) if det["link"].as_str().is_some_and(|l| l != p) => {
            format!("{p} / {}", s(&det["link"]))
        }
        (Some(p), _) => p.to_string(),
        (_, Some(dir)) => format!("Scratch · {dir}"),
        _ => String::new(),
    };
    let sub = label(&target, &["usage-detail-sub"]);
    sub.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    titles.append(&sub);
    titles.set_hexpand(true);
    head.append(&titles);
    let sw = gtk::Switch::new();
    sw.add_css_class("settings-switch");
    sw.add_css_class("sched-switch");
    sw.set_valign(gtk::Align::Center);
    sw.set_active(det["enabled"].as_bool() == Some(true));
    sw.set_tooltip_text(Some(if sw.is_active() {
        "On — click to turn off"
    } else {
        "Off — click to turn on"
    }));
    {
        let (w, n) = (ws.clone(), name.clone());
        sw.connect_state_set(move |_, on| {
            set_enabled(&w, &n, on);
            glib::Propagation::Proceed
        });
    }
    head.append(&sw);
    let more = secondary("ph-dots-three-symbolic", "", "sched-more", false);
    more.set_tooltip_text(Some("More"));
    {
        let (w, n) = (ws.clone(), name.clone());
        more.connect_clicked(move |b| {
            let (w1, n1, w2, n2) = (w.clone(), n.clone(), w.clone(), n.clone());
            let items: Items = vec![
                (
                    "Edit…".into(),
                    Box::new(move || new_form(Some((w1.clone(), n1.clone())))),
                ),
                ("!Delete…".into(), Box::new(move || delete(&w2, &n2))),
            ];
            popover(b, Rc::new(items));
        });
    }
    head.append(&more);
    card.append(&head);

    // What happens next, in one line.
    let next_t = det["next"].as_i64();
    let last = det["runs"].as_array().and_then(|r| r.first());
    let (text, class) = match st {
        "off" => ("Off — it won't run until you turn it on".to_string(), "off"),
        "done" => (
            match last.and_then(|r| r["at"].as_i64()) {
                Some(t) => format!("Done — ran once, {}", time(&json!(t))),
                None => "Done".to_string(),
            },
            "off",
        ),
        "failed" => (
            format!(
                "Last run failed{}",
                last.and_then(|r| r["reason"].as_str())
                    .map(|x| format!(": {x}"))
                    .unwrap_or_default()
            ),
            "failed",
        ),
        _ => match next_t {
            Some(t) => (format!("Next run {} · {}", rel(t), time(&json!(t))), "on"),
            None => ("No next run".to_string(), "off"),
        },
    };
    let banner = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    banner.add_css_class("sched-status");
    banner.add_css_class(&format!("sched-status-{class}"));
    banner.append(&led(st));
    let bl = label(&text, &["sched-status-text"]);
    bl.set_wrap(true);
    bl.set_ellipsize(gtk::pango::EllipsizeMode::None);
    bl.set_hexpand(true);
    banner.append(&bl);
    card.append(&banner);
    if st == "failed" {
        if let Some(t) = next_t {
            card.append(&label(
                &format!("Still on · next run {} · {}", rel(t), time(&json!(t))),
                &["sched-status-sub"],
            ));
        }
    }

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    actions.add_css_class("sched-actions");
    {
        let (w, n) = (ws.clone(), name.clone());
        let b = secondary("ph-arrow-clockwise-symbolic", "Run now", "sched-run", false);
        b.set_tooltip_text(Some("Start one run now, in a new Pane"));
        b.connect_clicked(move |_| run_now(&w, &n));
        actions.append(&b);
    }
    {
        let (w, n) = (ws.clone(), name.clone());
        let b = secondary("", "Edit…", "sched-edit", false);
        b.connect_clicked(move |_| new_form(Some((w.clone(), n.clone()))));
        actions.append(&b);
    }
    card.append(&actions);

    section_head(&card, "SETTINGS", None);
    let when = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    when.append(&label(s(&det["when_text"]), &["sched-kv-value"]));
    if let Some(c) = det["cron"].as_str() {
        when.append(&label(c, &["chip", "sched-cron-chip"]));
    }
    kv(&card, "When", when.upcast_ref());
    let runs_in = match (det["project"].as_str(), det["dir"].as_str()) {
        (Some(p), _) => format!("Project {p} · link {}", s(&det["link"])),
        (_, Some(dir)) => format!("{dir} (Scratch)"),
        _ => String::new(),
    };
    kv_text(&card, "Runs in", &runs_in);
    kv_text(
        &card,
        "Agent",
        &format!("{} · {}", s(&det["agent"]), account_text(det)),
    );
    kv_text(
        &card,
        "If missed",
        if s(&det["catch_up"]) == "once" {
            "Run once when RADE opens"
        } else {
            "Skip it"
        },
    );
    kv_text(
        &card,
        "Keep open",
        &format!("the last {} run Panes", det["keep_runs"]),
    );

    section_head(&card, "PROMPT", None);
    let prompt = label(s(&det["prompt"]), &["sched-prompt"]);
    prompt.set_wrap(true);
    prompt.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    prompt.set_lines(8);
    prompt.set_selectable(true);
    prompt.set_tooltip_text(det["prompt"].as_str());
    card.append(&prompt);

    let next: Vec<i64> = det["next_times"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_i64)
        .collect();
    if !next.is_empty() {
        section_head(&card, "UPCOMING", None);
        let up = gtk::Box::new(gtk::Orientation::Vertical, 0);
        up.add_css_class("sched-upcoming");
        for t in next {
            let r = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            r.add_css_class("sched-up-row");
            let a = label(&time(&json!(t)), &["sched-mono"]);
            a.set_hexpand(true);
            r.append(&a);
            r.append(&label(&rel(t), &["sched-mono", "sched-dim"]));
            up.append(&r);
        }
        card.append(&up);
    }

    let runs = det["runs"].as_array().cloned().unwrap_or_default();
    section_head(&card, "RUNS", (!runs.is_empty()).then_some("newest first"));
    if runs.is_empty() {
        card.append(&label(
            "No runs yet — Run now starts one.",
            &["usage-detail-empty", "sched-no-runs"],
        ));
    }
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list.add_css_class("sched-runs");
    for r in runs {
        list.append(&run_row(&ws, &name, &r));
    }
    card.append(&list);
    card
}

fn run_row(ws: &str, name: &str, r: &Value) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    b.add_css_class("sched-run-row");
    b.set_widget_name(&format!("sched-run-{}", r["n"]));
    let n = label(&format!("#{}", r["n"]), &["sched-mono", "sched-dim"]);
    sized(&n, 26);
    b.append(&n);
    let t = label(&time(&r["at"]), &["sched-mono"]);
    sized(&t, 92);
    b.append(&t);
    let outcome: rade_core::schedule::Outcome = serde_json::from_value(r["outcome"].clone())
        .unwrap_or(rade_core::schedule::Outcome::Started);
    let status: Option<rade_core::AgentStatus> = serde_json::from_value(r["status"].clone()).ok();
    let open = r["pane"].is_string() || r["resumed"].is_string();
    let (text, kind) = rade_core::schedule::run_chip(outcome, status, open);
    let chip = label(
        text,
        &["rade-chip", &format!("rade-chip-{kind}"), "sched-run-chip"],
    );
    chip.set_valign(gtk::Align::Center);
    b.append(&chip);
    let mut why = r["reason"].as_str().unwrap_or("").to_string();
    if r["resumed"].is_string() {
        why = "resumed".into();
    }
    let w = label(&why, &["sched-run-why"]);
    w.set_hexpand(true);
    w.set_tooltip_text(r["reason"].as_str());
    b.append(&w);
    let n = r["n"].as_u64().unwrap_or(0) as u32;
    let (w, nm) = (ws.to_string(), name.to_string());
    if open {
        let s = secondary("ph-arrow-square-out-symbolic", "Show", "sched-show", false);
        s.set_tooltip_text(Some("Go to this run's Pane"));
        s.connect_clicked(move |_| resume(&w, &nm, n));
        b.append(&s);
    } else if outcome == rade_core::schedule::Outcome::Started && r["session"].is_string() {
        let s = secondary(
            "ph-arrow-counter-clockwise-symbolic",
            "Resume",
            "sched-resume",
            false,
        );
        s.set_tooltip_text(Some("Resume this run's session in a new Pane"));
        s.connect_clicked(move |_| resume(&w, &nm, n));
        b.append(&s);
    }
    b
}

fn report(heading: &'static str) -> impl FnOnce(std::result::Result<Value, String>) {
    move |r| {
        if let Err(e) = r {
            crate::worktrees::alert(heading, &e);
        }
        refresh();
    }
}

fn run_now(ws: &str, name: &str) {
    ask(
        "schedule.run",
        json!({"workspace": ws, "name": name}),
        report("Could not run the Schedule"),
    );
}

/// Show (an open run Pane) or Resume (its session in a new Pane), then go
/// there: the page closes and the Pane has the focus (R139.2).
fn resume(ws: &str, name: &str, n: u32) {
    ask(
        "schedule.resume",
        json!({"workspace": ws, "name": name, "n": n}),
        |r| match r {
            Ok(v) => {
                if let Some(p) = v["pane"].as_str() {
                    jump(p);
                }
            }
            Err(e) => {
                crate::worktrees::alert("Could not resume the run", &e);
                refresh();
            }
        },
    );
}

/// Leave the page for Pane `pane`, focused (its Workspace, Tab shown).
fn jump(pane: &str) {
    let Some(app) = try_app() else { return };
    close();
    let _ = app.update(|m| m.focus_pane(pane));
    app.restore_focus();
}

fn set_enabled(ws: &str, name: &str, on: bool) {
    ask(
        "schedule.set_enabled",
        json!({"workspace": ws, "name": name, "enabled": on}),
        report("Could not change the Schedule"),
    );
}

/// Delete… asks; the check box closes its Panes too (R139.2).
fn delete(ws: &str, name: &str) {
    let Some(app) = try_app() else { return };
    let dlg = adw::AlertDialog::new(
        Some(&format!("Delete Schedule {name}?")),
        Some("Its Runs are forgotten. Its Tab and Panes stay unless you close them."),
    );
    let close = gtk::CheckButton::with_label("Close its Panes");
    close.add_css_class("sched-delete-close");
    dlg.set_extra_child(Some(&close));
    dlg.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);
    dlg.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    dlg.set_default_response(Some("cancel"));
    let (ws, name) = (ws.to_string(), name.to_string());
    dlg.connect_response(None, move |_, r| {
        DELETE.with(|d| d.borrow_mut().take());
        if r != "delete" {
            return;
        }
        ask(
            "schedule.remove",
            json!({"workspace": ws, "name": name, "close_panes": close.is_active()}),
            report("Could not delete the Schedule"),
        );
    });
    DELETE.with(|d| *d.borrow_mut() = Some(dlg.clone()));
    dlg.present(Some(&app.window));
}

// ---- the New / Edit Schedule form (R139.3) ----

/// An edited Schedule's `(cron, at text, account)` as loaded.
type Loaded = (Option<String>, Option<String>, Option<String>);

struct Form {
    dialog: adw::Dialog,
    /// (workspace, name) when editing.
    editing: Option<(String, String)>,
    workspace: String,
    name: gtk::Entry,
    by_project: gtk::ToggleButton,
    by_folder: gtk::ToggleButton,
    project_box: gtk::Box,
    folder_box: gtk::Box,
    project: gtk::DropDown,
    link: gtk::DropDown,
    folder: gtk::Entry,
    repeat: gtk::ToggleButton,
    once: gtk::ToggleButton,
    cron_box: gtk::Box,
    once_box: gtk::Box,
    cron: gtk::Entry,
    preview: gtk::Label,
    date: gtk::Entry,
    clock: gtk::Entry,
    agent: gtk::DropDown,
    account: gtk::DropDown,
    prompt: gtk::TextView,
    catch_up: gtk::CheckButton,
    keep: gtk::SpinButton,
    error: gtk::Label,
    create: gtk::Button,
    /// (id, links) of the Workspace's Projects.
    projects: Vec<(String, Vec<String>)>,
    presets: Vec<String>,
    /// (agent kind, name, default) of the Workspace's accounts.
    accounts: RefCell<Vec<(String, String, bool)>>,
    /// The account names in the dropdown after `Default (…)`.
    account_names: RefCell<Vec<String>>,
    /// An edited Schedule's `(cron, at text, account)` as loaded: only what
    /// changed is sent, so a prompt edit keeps a passed `at` and `checked`.
    loaded: RefCell<Option<Loaded>>,
}

fn strings(list: &[String]) -> gtk::StringList {
    let v: Vec<&str> = list.iter().map(String::as_str).collect();
    gtk::StringList::new(&v)
}

/// A field's label, with a hint after it.
fn field_label(title: &str, hint: Option<&str>) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    b.append(&label(title, &["field-label"]));
    if let Some(h) = hint {
        b.append(&label(h, &["field-hint"]));
    }
    b
}

/// A field's label with a control (a segmented pair) at its right.
fn field_head(title: &str, right: &impl IsA<gtk::Widget>) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    b.add_css_class("field-row");
    b.add_css_class("sched-field-head");
    let l = label(title, &["field-label"]);
    l.set_hexpand(true);
    l.set_valign(gtk::Align::Center);
    b.append(&l);
    right.set_valign(gtk::Align::Center);
    b.append(right);
    b
}

/// Two exclusive choices as a segmented control (the Usage page's period).
fn toggle_pair(a: &str, b: &str, class: &str) -> (gtk::Box, gtk::ToggleButton, gtk::ToggleButton) {
    let x = gtk::ToggleButton::with_label(a);
    let y = gtk::ToggleButton::with_label(b);
    y.set_group(Some(&x));
    x.set_active(true);
    for t in [&x, &y] {
        t.add_css_class("usage-period-seg");
        t.add_css_class("flat");
        t.set_focus_on_click(false);
    }
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.add_css_class("usage-period");
    row.add_css_class(class);
    row.append(&x);
    row.append(&y);
    (row, x, y)
}

impl Form {
    fn selected_project(&self) -> Option<&(String, Vec<String>)> {
        self.projects.get(self.project.selected() as usize)
    }

    fn agent(&self) -> String {
        self.presets
            .get(self.agent.selected() as usize)
            .cloned()
            .unwrap_or_else(|| "claude".into())
    }

    fn prompt_text(&self) -> String {
        let b = self.prompt.buffer();
        b.text(&b.start_iter(), &b.end_iter(), false).to_string()
    }

    /// The account dropdown for the chosen agent: `Default (…)` first.
    fn fill_accounts(&self, keep: Option<&str>) {
        // Accounts are claude's or codex's; other agents run without one.
        let kind = match self.agent().as_str() {
            "codex" => "codex",
            "claude" => "claude",
            _ => "",
        };
        let accs = self.accounts.borrow();
        let mine: Vec<&(String, String, bool)> = accs.iter().filter(|a| a.0 == kind).collect();
        let default = mine
            .iter()
            .find(|a| a.2)
            .or(mine.first())
            .map(|a| a.1.clone());
        let mut items = vec![match &default {
            Some(d) => format!("Default ({d})"),
            None => "Default".into(),
        }];
        let names: Vec<String> = mine.iter().map(|a| a.1.clone()).collect();
        items.extend(names.iter().cloned());
        self.account.set_model(Some(&strings(&items)));
        let pos = keep
            .and_then(|k| names.iter().position(|n| n == k))
            .map_or(0, |p| p as u32 + 1);
        self.account.set_selected(pos);
        *self.account_names.borrow_mut() = names;
    }

    fn fill_links(&self) {
        let links = self
            .selected_project()
            .map(|p| p.1.clone())
            .unwrap_or_default();
        self.link.set_model(Some(&strings(&links)));
        self.link.set_visible(links.len() > 1);
    }

    /// Live checks (R139.3): the cron's next 3 times or its error; Create
    /// only when everything needed is there.
    fn validate(&self) -> bool {
        let tz = crate::schedules::tz();
        let mut ok = check_name(self.name.text().trim()).is_ok();
        self.project_box.set_visible(self.by_project.is_active());
        self.folder_box.set_visible(self.by_folder.is_active());
        self.cron_box.set_visible(self.repeat.is_active());
        self.once_box.set_visible(self.once.is_active());
        if self.by_project.is_active() {
            ok &= self.selected_project().is_some();
        } else {
            ok &= std::path::Path::new(self.folder.text().trim()).is_absolute();
        }
        if self.repeat.is_active() {
            match Cron::parse(self.cron.text().trim()) {
                Ok(c) => {
                    let mut t = now();
                    let mut times = Vec::new();
                    for _ in 0..3 {
                        match c.next_after(t, tz) {
                            Some(n) => {
                                times.push(fmt_time(n, now(), tz));
                                t = n;
                            }
                            None => break,
                        }
                    }
                    self.preview
                        .set_label(&format!("Next  {}", times.join("  ·  ")));
                    self.preview.remove_css_class("error");
                }
                Err(e) => {
                    self.preview.set_label(&e);
                    self.preview.add_css_class("error");
                    ok = false;
                }
            }
        } else {
            let text = format!("{} {}", self.date.text().trim(), self.clock.text().trim());
            let unchanged = self
                .loaded
                .borrow()
                .as_ref()
                .is_some_and(|l| l.1.as_deref() == Some(text.as_str()));
            let (msg, bad) = match rade_core::schedule::parse_at(&text, tz) {
                Ok(t) if t > now() || unchanged => (format!("Runs once, {}", rel(t)), false),
                Ok(_) => ("That time has passed".to_string(), true),
                Err(_) if self.date.text().trim().is_empty() => {
                    ("Enter a date and a time".to_string(), true)
                }
                Err(_) => ("Date as YYYY-MM-DD, time as HH:MM".to_string(), true),
            };
            self.preview.set_label(&msg);
            if bad {
                self.preview.add_css_class("error");
                ok = false;
            } else {
                self.preview.remove_css_class("error");
            }
        }
        ok &= !self.prompt_text().trim().is_empty();
        self.create.set_sensitive(ok);
        ok
    }

    fn params(&self) -> Value {
        let mut p = json!({
            "workspace": self.workspace,
            "agent": self.agent(),
            "prompt": self.prompt_text(),
            "catch_up": if self.catch_up.is_active() { "once" } else { "skip" },
            "keep_runs": self.keep.value() as u8,
        });
        match &self.editing {
            Some((_, old)) => {
                p["name"] = json!(old);
                let new = self.name.text().trim().to_string();
                if new != *old {
                    p["rename"] = json!(new);
                }
            }
            None => p["name"] = json!(self.name.text().trim()),
        }
        if self.by_project.is_active() {
            if let Some((id, links)) = self.selected_project() {
                p["project"] = json!(id);
                if let Some(l) = links.get(self.link.selected() as usize) {
                    p["link"] = json!(l);
                }
            }
        } else {
            p["dir"] = json!(self.folder.text().trim());
        }
        let loaded = self.loaded.borrow().clone();
        if self.repeat.is_active() {
            let c = self.cron.text().trim().to_string();
            if loaded
                .as_ref()
                .is_none_or(|l| l.0.as_deref() != Some(c.as_str()))
            {
                p["cron"] = json!(c);
            }
        } else {
            let a = format!("{} {}", self.date.text().trim(), self.clock.text().trim());
            if loaded
                .as_ref()
                .is_none_or(|l| l.1.as_deref() != Some(a.as_str()))
            {
                p["at"] = json!(a);
            }
        }
        let pick = self.account.selected() as usize;
        let chosen = match pick {
            0 => None,
            i => self.account_names.borrow().get(i - 1).cloned(),
        };
        // Before the account list arrives the dropdown says only Default:
        // an edit then leaves the account alone.
        let listed = !self.account_names.borrow().is_empty();
        match &loaded {
            Some((_, _, acct)) if *acct == chosen || (!listed && acct.is_some()) => {}
            _ => p["account"] = json!(chosen),
        }
        p
    }

    fn submit(self: &Rc<Self>) {
        if !self.validate() {
            return;
        }
        let method = if self.editing.is_some() {
            "schedule.edit"
        } else {
            "schedule.add"
        };
        let p = self.params();
        self.create.set_sensitive(false);
        let me = self.clone();
        ask(method, p, move |r| match r {
            Ok(v) => {
                me.dialog.close();
                FORM.with(|f| f.borrow_mut().take());
                let key = (s(&v["workspace"]).to_string(), s(&v["name"]).to_string());
                set_choice(move |c| c.selected = Some(key));
            }
            Err(e) => {
                me.error.set_label(&e);
                me.error.set_visible(true);
                me.validate();
            }
        });
    }
}

/// New Schedule… (`None`), or Edit… of `(workspace, name)`.
pub fn new_form(editing: Option<(String, String)>) {
    let Some(app) = try_app() else { return };
    let workspace = editing
        .as_ref()
        .map(|e| e.0.clone())
        .or_else(|| match CHOICE.with(|c| c.borrow().workspace.clone()) {
            Some(w) if !w.is_empty() => Some(w),
            _ => None,
        })
        .unwrap_or_else(|| app.model.borrow().active_workspace().to_string());
    let (projects, presets) = {
        let m = app.model.borrow();
        let projects: Vec<(String, Vec<String>)> = m
            .workspace(&workspace)
            .map(|w| {
                rade_core::project::list(&w.root)
                    .into_iter()
                    .map(|p| (p.id, p.sources.into_iter().map(|s| s.source).collect()))
                    .collect()
            })
            .unwrap_or_default();
        let presets: Vec<String> = app
            .config
            .borrow()
            .presets()
            .map(|p| p.keys().cloned().collect())
            .unwrap_or_default();
        (projects, presets)
    };

    let ws_name = app
        .model
        .borrow()
        .workspace_looks()
        .into_iter()
        .find(|l| l.id == workspace)
        .map_or(workspace.clone(), |l| l.name);
    let dialog = adw::Dialog::new();
    dialog.set_title(if editing.is_some() {
        "Edit Schedule"
    } else {
        "New Schedule"
    });
    dialog.set_content_width(580);
    dialog.add_css_class("sched-form");
    let shell = gtk::Box::new(gtk::Orientation::Vertical, 0);
    shell.add_css_class("sched-form-shell");

    // Header (the New Project dialog's): icon, title, Workspace, ✕.
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    header.add_css_class("dialog-header");
    header.append(&icon("ph-clock-symbolic", &["dialog-icon"]));
    header.append(&label(&dialog.title(), &["dialog-title"]));
    let ctx = label(&format!("in {ws_name}"), &["dialog-context"]);
    ctx.set_hexpand(true);
    header.append(&ctx);
    let x = gtk::Button::from_icon_name("ph-x-symbolic");
    x.add_css_class("flat");
    x.add_css_class("dialog-close");
    x.set_tooltip_text(Some("Close (Esc)"));
    header.append(&x);
    shell.append(&header);

    let col = gtk::Box::new(gtk::Orientation::Vertical, 0);
    col.add_css_class("dialog-body");
    col.add_css_class("sched-form-body");

    col.append(&field_label("Name", None));
    let name = gtk::Entry::new();
    name.set_placeholder_text(Some("e.g. nightly-deps"));
    name.add_css_class("sched-form-name");
    col.append(&name);

    let (pair, by_project, by_folder) = toggle_pair("Project", "Folder", "sched-form-target");
    col.append(&field_head("Runs in", &pair));
    let project_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let pnames: Vec<String> = projects.iter().map(|p| p.0.clone()).collect();
    let project = gtk::DropDown::new(Some(strings(&pnames)), gtk::Expression::NONE);
    project.add_css_class("sched-form-project");
    project.add_css_class("sched-drop");
    project.set_hexpand(true);
    let link = gtk::DropDown::new(None::<gtk::StringList>, gtk::Expression::NONE);
    link.add_css_class("sched-form-link");
    link.add_css_class("sched-drop");
    link.set_tooltip_text(Some("The Project's link (Source) to run in"));
    project_box.append(&project);
    project_box.append(&link);
    col.append(&project_box);
    let folder_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let frow = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let folder = gtk::Entry::new();
    folder.set_placeholder_text(Some("/home/you/notes"));
    folder.set_hexpand(true);
    folder.add_css_class("sched-form-folder");
    let choose = secondary(
        "ph-folder-simple-symbolic",
        "Choose…",
        "sched-form-choose",
        false,
    );
    frow.append(&folder);
    frow.append(&choose);
    folder_box.append(&frow);
    folder_box.append(&label(
        "Runs in Scratch, started in this folder.",
        &["field-hint", "sched-form-hint"],
    ));
    col.append(&folder_box);
    if projects.is_empty() {
        by_folder.set_active(true);
    }

    let (pair, repeat, once) = toggle_pair("Repeat", "Once", "sched-form-when");
    col.append(&field_head("When", &pair));
    let cron_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let cron = gtk::Entry::new();
    cron.set_placeholder_text(Some("minute hour day month weekday — e.g. 0 9 * * 1-5"));
    cron.add_css_class("sched-form-cron");
    cron_box.append(&cron);
    let presets_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    presets_row.add_css_class("sched-presets");
    for (t, expr) in [
        ("Hourly", "0 * * * *"),
        ("Daily 09:00", "0 9 * * *"),
        ("Weekdays 09:00", "0 9 * * 1-5"),
        ("Mondays 09:00", "0 9 * * 1"),
    ] {
        let b = gtk::Button::with_label(t);
        b.add_css_class("sched-form-preset");
        b.set_tooltip_text(Some(expr));
        let c = cron.clone();
        b.connect_clicked(move |_| c.set_text(expr));
        presets_row.append(&b);
    }
    cron_box.append(&presets_row);
    col.append(&cron_box);
    let once_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let date = gtk::Entry::new();
    date.set_placeholder_text(Some("YYYY-MM-DD"));
    date.add_css_class("sched-form-date");
    date.set_hexpand(true);
    let clock = gtk::Entry::new();
    clock.set_placeholder_text(Some("HH:MM"));
    clock.add_css_class("sched-form-time");
    clock.set_width_chars(8);
    once_box.append(&date);
    once_box.append(&clock);
    col.append(&once_box);
    let preview = label("", &["sched-form-preview"]);
    preview.set_wrap(true);
    preview.set_ellipsize(gtk::pango::EllipsizeMode::None);
    col.append(&preview);

    let who = gtk::Grid::new();
    who.set_column_spacing(12);
    who.set_column_homogeneous(true);
    who.add_css_class("field-row");
    who.attach(&field_label("Agent", None), 0, 0, 1, 1);
    who.attach(&field_label("Account", None), 1, 0, 1, 1);
    let agent = gtk::DropDown::new(Some(strings(&presets)), gtk::Expression::NONE);
    agent.add_css_class("sched-form-agent");
    agent.add_css_class("sched-drop");
    if let Some(i) = presets.iter().position(|p| p == "claude") {
        agent.set_selected(i as u32);
    }
    let account = gtk::DropDown::new(None::<gtk::StringList>, gtk::Expression::NONE);
    account.add_css_class("sched-form-account");
    account.add_css_class("sched-drop");
    account.set_tooltip_text(Some(
        "Default = the Workspace's default account when it runs",
    ));
    who.attach(&agent, 0, 1, 1, 1);
    who.attach(&account, 1, 1, 1, 1);
    col.append(&who);

    let ph = field_label("Prompt", Some("what the Agent is asked at each run"));
    ph.add_css_class("field-row");
    col.append(&ph);
    let prompt = gtk::TextView::new();
    prompt.set_wrap_mode(gtk::WrapMode::WordChar);
    prompt.add_css_class("sched-form-prompt");
    prompt.set_top_margin(8);
    prompt.set_bottom_margin(8);
    prompt.set_left_margin(10);
    prompt.set_right_margin(10);
    let psc = gtk::ScrolledWindow::new();
    psc.set_min_content_height(76);
    psc.set_max_content_height(200);
    psc.set_propagate_natural_height(true);
    psc.set_hscrollbar_policy(gtk::PolicyType::Never);
    psc.set_child(Some(&prompt));
    psc.add_css_class("sched-form-prompt-box");
    col.append(&psc);

    let oh = field_label("Options", None);
    oh.add_css_class("field-row");
    col.append(&oh);
    let opts = gtk::Box::new(gtk::Orientation::Vertical, 6);
    opts.add_css_class("sched-form-options");
    let catch_up = gtk::CheckButton::with_label(
        "If a time passes while RADE is closed, run once when it opens",
    );
    catch_up.add_css_class("sched-form-catchup");
    opts.append(&catch_up);
    let krow = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    krow.append(&label("Keep the last", &["sched-form-text"]));
    let keep = gtk::SpinButton::with_range(1.0, 20.0, 1.0);
    keep.set_value(3.0);
    keep.add_css_class("sched-form-keep");
    keep.set_valign(gtk::Align::Center);
    krow.append(&keep);
    krow.append(&label(
        "finished run Panes open; older ones close",
        &["sched-form-text"],
    ));
    opts.append(&krow);
    col.append(&opts);
    let error = label("", &["field-error", "sched-form-error"]);
    error.set_wrap(true);
    error.set_ellipsize(gtk::pango::EllipsizeMode::None);
    error.set_visible(false);
    col.append(&error);

    let sc = gtk::ScrolledWindow::new();
    sc.set_propagate_natural_height(true);
    sc.set_max_content_height(660);
    sc.set_hscrollbar_policy(gtk::PolicyType::Never);
    sc.set_child(Some(&col));
    shell.append(&sc);

    // Footer: a note, Cancel, the primary button.
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.add_css_class("dialog-footer");
    let note = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    note.add_css_class("dialog-note");
    note.set_hexpand(true);
    note.append(&icon("ph-info-symbolic", &["dialog-note-icon"]));
    let nl = label(
        "Runs only while RADE is open, each in a new Pane of its own Tab.",
        &["dialog-note-text"],
    );
    nl.set_wrap(true);
    nl.set_ellipsize(gtk::pango::EllipsizeMode::None);
    nl.set_hexpand(true);
    note.append(&nl);
    footer.append(&note);
    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("secondary-button");
    cancel.add_css_class("dialog-cancel");
    cancel.add_css_class("sched-form-cancel");
    cancel.set_valign(gtk::Align::Center);
    footer.append(&cancel);
    let create = gtk::Button::with_label(if editing.is_some() {
        "Save"
    } else {
        "Create schedule"
    });
    create.add_css_class("suggested-action");
    create.add_css_class("dialog-primary");
    create.add_css_class("sched-form-create");
    create.set_valign(gtk::Align::Center);
    footer.append(&create);
    shell.append(&footer);
    dialog.set_child(Some(&shell));
    {
        let d = dialog.clone();
        x.connect_clicked(move |_| {
            d.close();
        });
    }

    let form = Rc::new(Form {
        dialog: dialog.clone(),
        editing: editing.clone(),
        workspace: workspace.clone(),
        name,
        by_project,
        by_folder,
        project_box,
        folder_box,
        project,
        link,
        folder,
        repeat,
        once,
        cron_box,
        once_box,
        cron,
        preview,
        date,
        clock,
        agent,
        account,
        prompt,
        catch_up,
        keep,
        error,
        create,
        projects,
        presets,
        accounts: RefCell::new(Vec::new()),
        account_names: RefCell::new(Vec::new()),
        loaded: RefCell::new(None),
    });
    form.fill_links();
    form.fill_accounts(None);

    // Wire every input to the live checks.
    {
        let f = form.clone();
        let v = move || {
            f.validate();
        };
        let v = Rc::new(v);
        for e in [
            &form.name,
            &form.folder,
            &form.cron,
            &form.date,
            &form.clock,
        ] {
            let v = v.clone();
            e.connect_changed(move |_| v());
        }
        for t in [&form.by_project, &form.by_folder, &form.repeat, &form.once] {
            let v = v.clone();
            t.connect_toggled(move |_| v());
        }
        let v2 = v.clone();
        form.prompt.buffer().connect_changed(move |_| v2());
    }
    {
        let f = form.clone();
        form.project.connect_selected_notify(move |_| {
            f.fill_links();
            f.validate();
        });
    }
    {
        let f = form.clone();
        form.agent
            .connect_selected_notify(move |_| f.fill_accounts(None));
    }
    {
        let f = form.clone();
        choose.connect_clicked(move |_| {
            let Some(app) = try_app() else { return };
            let fd = gtk::FileDialog::new();
            fd.set_title("Folder to run in");
            let f = f.clone();
            fd.select_folder(Some(&app.window), gtk::gio::Cancellable::NONE, move |r| {
                if let Ok(dir) = r {
                    if let Some(p) = dir.path() {
                        f.folder.set_text(&p.display().to_string());
                    }
                }
            });
        });
    }
    {
        let f = form.clone();
        form.create.connect_clicked(move |_| f.submit());
    }
    {
        let d = dialog.clone();
        cancel.connect_clicked(move |_| {
            d.close();
        });
    }
    // Closed after its animation: a form opened meanwhile stays the open one.
    dialog.connect_closed(|d| {
        FORM.with(|f| {
            let mut f = f.borrow_mut();
            if f.as_ref().is_some_and(|x| &x.dialog == d) {
                f.take();
            }
        });
    });

    // The Workspace's accounts (off the main loop).
    {
        let f = form.clone();
        let ws = workspace.clone();
        let done = glib::thread_guard::ThreadGuard::new(move |v: Value| {
            let accs: Vec<(String, String, bool)> = v["accounts"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|a| {
                    (
                        s(&a["agent"]).to_string(),
                        s(&a["name"]).to_string(),
                        a["default"].as_bool() == Some(true),
                    )
                })
                .collect();
            *f.accounts.borrow_mut() = accs;
            // An edited Schedule's account (`fill_from` may have run first).
            let name = f.dialog.widget_name();
            f.fill_accounts(name.strip_prefix("acct:"));
        });
        std::thread::spawn(move || {
            let v = crate::accounts::dispatch("account.list", json!({"workspace": ws}))
                .unwrap_or_else(|_| json!({}));
            glib::MainContext::default().invoke(move || (done.into_inner())(v));
        });
    }

    // Editing: the Schedule's values (off the main loop).
    if let Some((ws, n)) = editing {
        let f = form.clone();
        ask(
            "schedule.get",
            json!({"workspace": ws, "name": n}),
            move |r| {
                if let Ok(v) = r {
                    fill_from(&f, &v);
                }
            },
        );
    } else {
        form.cron.set_text("0 9 * * 1-5");
    }
    form.validate();
    FORM.with(|f| *f.borrow_mut() = Some(form.clone()));
    dialog.present(Some(&app.window));
}

fn fill_from(f: &Rc<Form>, v: &Value) {
    f.name.set_text(s(&v["name"]));
    if let Some(p) = v["project"].as_str() {
        f.by_project.set_active(true);
        if let Some(i) = f.projects.iter().position(|x| x.0 == p) {
            f.project.set_selected(i as u32);
        }
        f.fill_links();
        if let (Some(l), Some((_, links))) = (v["link"].as_str(), f.selected_project()) {
            if let Some(i) = links.iter().position(|x| x == l) {
                f.link.set_selected(i as u32);
            }
        }
    } else if let Some(d) = v["dir"].as_str() {
        f.by_folder.set_active(true);
        f.folder.set_text(d);
    }
    if let Some(c) = v["cron"].as_str() {
        f.repeat.set_active(true);
        f.cron.set_text(c);
    } else if let Some(at) = v["at"].as_i64() {
        f.once.set_active(true);
        if let Ok(ts) = rade_core::schedule::jiff::Timestamp::from_second(at) {
            let z = ts.to_zoned(crate::schedules::tz().clone());
            f.date.set_text(&z.date().to_string());
            f.clock
                .set_text(&format!("{:02}:{:02}", z.hour(), z.minute()));
        }
    }
    if let Some(i) = f.presets.iter().position(|p| p == s(&v["agent"])) {
        f.agent.set_selected(i as u32);
    }
    let acct = v["account"].as_str().map(String::from);
    if let Some(a) = &acct {
        f.dialog.set_widget_name(&format!("acct:{a}"));
    }
    f.fill_accounts(acct.as_deref());
    f.prompt.buffer().set_text(s(&v["prompt"]));
    f.catch_up.set_active(s(&v["catch_up"]) == "once");
    f.keep.set_value(v["keep_runs"].as_f64().unwrap_or(3.0));
    let at_text =
        (f.once.is_active()).then(|| format!("{} {}", f.date.text().trim(), f.clock.text().trim()));
    *f.loaded.borrow_mut() = Some((v["cron"].as_str().map(String::from), at_text, acct));
    f.validate();
}

// ---- debug.ui -------------------------------------------------------------

fn info() -> Value {
    let d = DATA.with(|d| d.borrow().clone());
    let form = FORM.with(|f| f.borrow().clone());
    json!({
        "open": shown_now(),
        "groups": d.as_ref().map(|d| groups(d).into_iter().map(|(t, rows)| json!({
            "title": t,
            "rows": rows.iter().map(|r| json!({
                "name": r["name"], "state": state(r), "when": r["when_text"],
                "agent": r["agent"], "account": account_text(r), "next": r["next"],
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>()),
        "selected": d.as_ref().and_then(|d| d.choice.selected.clone()).map(|(w, n)| json!([w, n])),
        "detail": d.as_ref().and_then(|d| d.detail.clone()),
        "filter": d.as_ref().map(|d| json!({
            "workspace": d.choice.workspace, "project": d.choice.project, "on": d.choice.on,
        })),
        "delete_open": DELETE.with(|x| x.borrow().is_some()),
        "form": form.map(|f| json!({
            "title": f.dialog.title().to_string(),
            "create_enabled": f.create.is_sensitive(),
            "preview": f.preview.label().to_string(),
            "preview_error": f.preview.has_css_class("error"),
            "error": f.error.is_visible().then(|| f.error.label().to_string()),
            "params": f.params(),
            "cron": f.cron.text().to_string(),
            "link_visible": f.link.is_visible(),
            "accounts": (0..f.account.model().map_or(0, |m| m.n_items())).map(|i| {
                f.account.model().and_then(|m| m.item(i))
                    .and_then(|o| o.downcast::<gtk::StringObject>().ok())
                    .map(|o| o.string().to_string())
            }).collect::<Vec<_>>(),
        })),
    })
}

/// `debug.ui {"op": "schedules", "arg"?, …}`: `open` (`name`, `workspace`
/// optional) | `close` | `select` (`workspace`, `name`) | `filter` (`project`:
/// id, `""` = Scratch, null = all; `on`; `workspace`: id, `""` = all) |
/// `new` | `edit` (`workspace`, `name`) | `form_set` (`field`, `value`:
/// name, target project|folder, project, folder, when repeat|once, cron,
/// date, time, agent, account (index), prompt, catch_up, keep) |
/// `form_submit` | `form_cancel` | `run_now` | `resume` (`n`) | `toggle` |
/// `delete` (`close_panes`) | `delete_respond` (`response`, `close_panes`).
/// Answers what the page shows.
pub fn debug(p: &Value) -> Result<Value> {
    let s_ = |k: &str| p.get(k).and_then(Value::as_str).map(String::from);
    let sel = || {
        DATA.with(|d| d.borrow().as_ref().and_then(|d| d.choice.selected.clone()))
            .ok_or_else(|| Error::usage("no Schedule selected"))
    };
    match s_("arg").as_deref() {
        None | Some("info") => {}
        Some("open") => match (s_("workspace"), s_("name")) {
            (Some(w), Some(n)) => open(Some((&w, &n))),
            (None, Some(n)) => {
                let w = try_app()
                    .map(|a| a.model.borrow().active_workspace().to_string())
                    .unwrap_or_default();
                open(Some((&w, &n)))
            }
            _ => open(None),
        },
        Some("open_project") => open_project(&s_("project").unwrap_or_default()),
        Some("close") => leave(),
        Some("select") => {
            let key = (
                s_("workspace").unwrap_or_default(),
                s_("name").unwrap_or_default(),
            );
            set_choice(move |c| c.selected = Some(key));
        }
        Some("filter") => {
            let project = p.get("project").map(|v| v.as_str().map(String::from));
            let on = p.get("on").map(Value::as_bool);
            let ws = p.get("workspace").map(|v| v.as_str().map(String::from));
            set_choice(move |c| {
                if let Some(x) = project {
                    c.project = x;
                }
                if let Some(x) = on {
                    c.on = x;
                }
                if let Some(x) = ws {
                    c.workspace = x;
                }
            });
        }
        Some("new") => new_form(None),
        Some("edit") => new_form(Some(match (s_("workspace"), s_("name")) {
            (Some(w), Some(n)) => (w, n),
            // A name alone: in the shown Workspace.
            (None, Some(n)) => (
                try_app()
                    .map(|a| a.model.borrow().active_workspace().to_string())
                    .unwrap_or_default(),
                n,
            ),
            _ => sel()?,
        })),
        Some("form_set") => {
            let f = FORM
                .with(|f| f.borrow().clone())
                .ok_or_else(|| Error::usage("no Schedule form open"))?;
            let v = p.get("value").cloned().unwrap_or(Value::Null);
            let text = v.as_str().unwrap_or("").to_string();
            match s_("field").as_deref() {
                Some("name") => f.name.set_text(&text),
                Some("target") => {
                    if text == "folder" {
                        f.by_folder.set_active(true)
                    } else {
                        f.by_project.set_active(true)
                    }
                }
                Some("project") => {
                    if let Some(i) = f.projects.iter().position(|x| x.0 == text) {
                        f.project.set_selected(i as u32);
                    }
                }
                Some("folder") => f.folder.set_text(&text),
                Some("when") => {
                    if text == "once" {
                        f.once.set_active(true)
                    } else {
                        f.repeat.set_active(true)
                    }
                }
                Some("cron") => f.cron.set_text(&text),
                Some("preset") => {
                    let expr = match text.as_str() {
                        "Hourly" => "0 * * * *",
                        "Daily 09:00" => "0 9 * * *",
                        _ => "0 9 * * 1-5",
                    };
                    f.cron.set_text(expr);
                }
                Some("date") => f.date.set_text(&text),
                Some("time") => f.clock.set_text(&text),
                Some("agent") => {
                    if let Some(i) = f.presets.iter().position(|x| *x == text) {
                        f.agent.set_selected(i as u32);
                    }
                }
                Some("account") => f.account.set_selected(v.as_u64().unwrap_or(0) as u32),
                Some("prompt") => f.prompt.buffer().set_text(&text),
                Some("catch_up") => f.catch_up.set_active(v.as_bool().unwrap_or(false)),
                Some("keep") => f.keep.set_value(v.as_f64().unwrap_or(3.0)),
                o => return Err(Error::usage(format!("unknown form field {o:?}"))),
            }
            f.validate();
        }
        Some("form_submit") => {
            let f = FORM
                .with(|f| f.borrow().clone())
                .ok_or_else(|| Error::usage("no Schedule form open"))?;
            f.submit();
        }
        Some("form_cancel") => {
            if let Some(f) = FORM.with(|f| f.borrow_mut().take()) {
                f.dialog.close();
            }
        }
        Some("run_now") => {
            let (w, n) = sel()?;
            run_now(&w, &n);
        }
        Some("resume") => {
            let (w, n) = sel()?;
            resume(
                &w,
                &n,
                p.get("n").and_then(Value::as_u64).unwrap_or(0) as u32,
            );
        }
        Some("toggle") => {
            let (w, n) = sel()?;
            let on = DATA.with(|d| {
                d.borrow().as_ref().and_then(|d| {
                    d.detail
                        .as_ref()
                        .map(|x| x["enabled"].as_bool() == Some(true))
                })
            });
            set_enabled(&w, &n, !on.unwrap_or(false));
        }
        Some("delete") => {
            let (w, n) = sel()?;
            delete(&w, &n);
        }
        Some("delete_respond") => {
            let dlg = DELETE
                .with(|d| d.borrow().clone())
                .ok_or_else(|| Error::usage("no Delete dialog"))?;
            if let Some(cb) = dlg
                .extra_child()
                .and_then(|w| w.downcast::<gtk::CheckButton>().ok())
            {
                cb.set_active(
                    p.get("close_panes")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                );
            }
            let r = s_("response").unwrap_or_else(|| "delete".into());
            dlg.emit_by_name::<()>("response", &[&r]);
            dlg.close();
        }
        Some(o) => return Err(Error::usage(format!("unknown schedules op '{o}'"))),
    }
    Ok(info())
}
