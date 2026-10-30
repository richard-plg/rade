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
    std::thread::spawn(move || {
        let data = load(choice, shown_ws, workspaces, projects);
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

fn button(text: &str, classes: &[&str], f: impl Fn() + 'static) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    for c in classes {
        b.add_css_class(c);
    }
    b.connect_clicked(move |_| f());
    b
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

fn header(d: &Data) -> gtk::Box {
    let h = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    h.add_css_class("usage-head");
    let icon = gtk::Image::from_icon_name("ph-clock-symbolic");
    icon.add_css_class("usage-icon");
    h.append(&icon);
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
    let sub = match next {
        Some((t, n)) => format!(
            "{on} on · next {n} {}",
            fmt_time(t, now(), crate::schedules::tz())
        ),
        None => format!("{on} on"),
    };
    titles.append(&label(&sub, &["usage-sub", "sched-sub"]));
    titles.set_hexpand(true);
    h.append(&titles);

    let ws_label = match d.choice.workspace.as_deref() {
        Some("") => "All workspaces".to_string(),
        Some(w) => d
            .workspaces
            .iter()
            .find(|x| x.0 == w)
            .map_or(w.to_string(), |x| x.1.clone()),
        None => d
            .workspaces
            .iter()
            .find(|x| x.0 == d.shown_workspace)
            .map_or(d.shown_workspace.clone(), |x| x.1.clone()),
    };
    let mut ws_items: Vec<(String, Box<dyn Fn()>)> = vec![(
        "This workspace".into(),
        Box::new(|| {
            set_choice(|c| {
                c.workspace = None;
                c.project = None;
            })
        }),
    )];
    ws_items.push((
        "All workspaces".into(),
        Box::new(|| {
            set_choice(|c| {
                c.workspace = Some(String::new());
                c.project = None;
            })
        }),
    ));
    for (id, name) in &d.workspaces {
        let id = id.clone();
        ws_items.push((
            name.clone(),
            Box::new(move || {
                let id = id.clone();
                set_choice(move |c| {
                    c.workspace = Some(id);
                    c.project = None;
                })
            }),
        ));
    }
    h.append(&menu_button(&ws_label, "sched-filter-ws", ws_items));

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
    let mut p_items: Vec<(String, Box<dyn Fn()>)> = vec![(
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
    h.append(&menu_button(&proj_label, "sched-filter-project", p_items));

    let on_label = match d.choice.on {
        None => "On and off",
        Some(true) => "On",
        Some(false) => "Off",
    };
    h.append(&menu_button(
        on_label,
        "sched-filter-on",
        vec![
            (
                "On and off".into(),
                Box::new(|| set_choice(|c| c.on = None)),
            ),
            ("On".into(), Box::new(|| set_choice(|c| c.on = Some(true)))),
            (
                "Off".into(),
                Box::new(|| set_choice(|c| c.on = Some(false))),
            ),
        ],
    ));
    h.append(&button(
        "New Schedule…",
        &["suggested-action", "sched-new"],
        || new_form(None),
    ));
    let close = gtk::Button::from_icon_name("ph-x-symbolic");
    close.add_css_class("flat");
    close.add_css_class("sched-close");
    close.set_tooltip_text(Some("Close (Esc)"));
    close.connect_clicked(|_| leave());
    h.append(&close);
    h
}

fn menu_button(text: &str, class: &str, items: Vec<(String, Box<dyn Fn()>)>) -> gtk::Button {
    let b = gtk::Button::new();
    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    inner.append(&gtk::Label::new(Some(text)));
    inner.append(&gtk::Image::from_icon_name("ph-caret-down-symbolic"));
    b.set_child(Some(&inner));
    b.add_css_class("flat");
    b.add_css_class("sched-filter");
    b.add_css_class(class);
    let items = Rc::new(items);
    b.connect_clicked(move |anchor| {
        let pop = gtk::Popover::new();
        let col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        for (i, (t, _)) in items.iter().enumerate() {
            let row = gtk::Button::with_label(t);
            row.add_css_class("flat");
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
    });
    b
}

fn build(d: &Data) -> gtk::Widget {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.add_css_class("usage-page");
    page.add_css_class("sched-page");
    page.append(&header(d));
    if let Some(e) = &d.error {
        page.append(&label(e, &["sched-error", "error"]));
    }
    if d.rows.is_empty() {
        let empty = gtk::Box::new(gtk::Orientation::Vertical, 8);
        empty.add_css_class("sched-empty");
        empty.set_valign(gtk::Align::Center);
        empty.set_vexpand(true);
        let l = label("No Schedules here yet", &["usage-title"]);
        l.set_halign(gtk::Align::Center);
        empty.append(&l);
        let hint = label(
            "A Schedule starts an Agent with a prompt on a cron expression or once at a time.",
            &["usage-sub"],
        );
        hint.set_halign(gtk::Align::Center);
        empty.append(&hint);
        let b = button("New Schedule…", &["suggested-action"], || new_form(None));
        b.set_halign(gtk::Align::Center);
        empty.append(&b);
        page.append(&empty);
        return page.upcast();
    }
    let body = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    body.add_css_class("usage-grid");
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list.add_css_class("usage-accounts-card");
    list.add_css_class("sched-list");
    list.set_hexpand(true);
    list.set_valign(gtk::Align::Start);
    for (title, rows) in groups(d) {
        let g = label(&title.to_uppercase(), &["usage-col", "sched-group"]);
        list.append(&g);
        for r in rows {
            list.append(&row(d, r));
        }
    }
    body.append(&list);
    if let Some(det) = &d.detail {
        body.append(&detail(det));
    }
    page.append(&body);
    page.upcast()
}

fn row(d: &Data, r: &Value) -> gtk::Box {
    let key = (s(&r["workspace"]).to_string(), s(&r["name"]).to_string());
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    b.add_css_class("usage-row");
    b.add_css_class("sched-row");
    b.set_widget_name(&format!("sched-row-{}", key.1));
    if d.choice.selected.as_ref() == Some(&key) {
        b.add_css_class("selected");
    }
    let st = state(r);
    let cell = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    cell.add_css_class("usage-cell");
    cell.append(&led(st));
    cell.append(&label(s(&r["name"]), &["usage-account-name", "sched-name"]));
    cell.set_size_request(170, -1);
    b.append(&cell);
    let when = label(s(&r["when_text"]), &["usage-cell", "sched-when"]);
    when.set_size_request(150, -1);
    b.append(&when);
    let who = label(
        &format!("{} · {}", s(&r["agent"]), account_text(r)),
        &["usage-cell", "sched-agent"],
    );
    who.set_hexpand(true);
    b.append(&who);
    let next = match st {
        "off" => "off".to_string(),
        "done" => "done".to_string(),
        _ => time(&r["next"]),
    };
    b.append(&label(&next, &["usage-cell", "sched-next"]));
    let click = gtk::GestureClick::new();
    click.connect_released(move |_, _, _, _| {
        let key = key.clone();
        set_choice(move |c| c.selected = Some(key));
    });
    b.add_controller(click);
    b
}

fn field(grid: &gtk::Grid, row: i32, name: &str, value: &str) {
    grid.attach(
        &label(name, &["usage-col", "sched-field-name"]),
        0,
        row,
        1,
        1,
    );
    let v = label(value, &["usage-cell", "sched-field"]);
    v.set_wrap(true);
    v.set_ellipsize(gtk::pango::EllipsizeMode::None);
    grid.attach(&v, 1, row, 1, 1);
}

fn detail(det: &Value) -> gtk::Box {
    let ws = s(&det["workspace"]).to_string();
    let name = s(&det["name"]).to_string();
    let card = gtk::Box::new(gtk::Orientation::Vertical, 10);
    card.add_css_class("usage-accounts-card");
    card.add_css_class("sched-detail");
    card.set_size_request(380, -1);
    card.set_valign(gtk::Align::Start);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    top.add_css_class("sched-detail-head");
    top.append(&led(state(det)));
    let t = label(&name, &["usage-account-name", "sched-detail-name"]);
    t.set_hexpand(true);
    top.append(&t);
    top.append(&label(state(det), &["usage-ws-chip", "sched-state"]));
    card.append(&top);

    let grid = gtk::Grid::new();
    grid.set_column_spacing(12);
    grid.set_row_spacing(4);
    grid.add_css_class("sched-fields");
    let target = match (det["project"].as_str(), det["dir"].as_str()) {
        (Some(p), _) => format!("{p} / {}", s(&det["link"])),
        (_, Some(d)) => format!("{d} (Scratch)"),
        _ => String::new(),
    };
    let when = match det["cron"].as_str() {
        Some(c) => format!("{} ({c})", s(&det["when_text"])),
        None => s(&det["when_text"]).to_string(),
    };
    let rows = [
        ("Target", target),
        ("When", when),
        ("Agent", s(&det["agent"]).to_string()),
        ("Account", account_text(det)),
        (
            "If missed",
            if s(&det["catch_up"]) == "once" {
                "Run once when the app opens".into()
            } else {
                "Skip".into()
            },
        ),
        ("Keep", format!("last {} run Panes", det["keep_runs"])),
        ("Prompt", s(&det["prompt"]).to_string()),
    ];
    for (i, (k, v)) in rows.iter().enumerate() {
        field(&grid, i as i32, k, v);
    }
    card.append(&grid);

    let next: Vec<String> = det["next_times"]
        .as_array()
        .into_iter()
        .flatten()
        .map(time)
        .collect();
    card.append(&label("NEXT", &["usage-col"]));
    card.append(&label(
        &if next.is_empty() {
            "—".to_string()
        } else {
            next.join(" · ")
        },
        &["usage-cell", "sched-next-times"],
    ));

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    actions.add_css_class("sched-actions");
    {
        let (w, n) = (ws.clone(), name.clone());
        actions.append(&button("Run now", &["sched-run"], move || run_now(&w, &n)));
    }
    {
        let (w, n) = (ws.clone(), name.clone());
        actions.append(&button("Edit…", &["sched-edit"], move || {
            new_form(Some((w.clone(), n.clone())))
        }));
    }
    {
        let (w, n) = (ws.clone(), name.clone());
        let on = det["enabled"].as_bool() == Some(true);
        actions.append(&button(
            if on { "Turn off" } else { "Turn on" },
            &["sched-toggle"],
            move || set_enabled(&w, &n, !on),
        ));
    }
    {
        let (w, n) = (ws.clone(), name.clone());
        actions.append(&button(
            "Delete…",
            &["destructive-action", "sched-delete"],
            move || delete(&w, &n),
        ));
    }
    card.append(&actions);

    card.append(&label("RUNS", &["usage-col"]));
    let runs = det["runs"].as_array().cloned().unwrap_or_default();
    if runs.is_empty() {
        card.append(&label("No runs yet", &["usage-sub"]));
    }
    for r in runs {
        card.append(&run_row(&ws, &name, &r));
    }
    card
}

fn run_row(ws: &str, name: &str, r: &Value) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    b.add_css_class("sched-run-row");
    b.set_widget_name(&format!("sched-run-{}", r["n"]));
    b.append(&label(&format!("#{}", r["n"]), &["usage-sub"]));
    b.append(&label(&time(&r["at"]), &["usage-sub"]));
    let outcome = s(&r["outcome"]);
    let o = label(
        outcome,
        &["usage-ws-chip", &format!("sched-outcome-{outcome}")],
    );
    b.append(&o);
    let mut text = r["status"].as_str().unwrap_or("").to_string();
    if let Some(reason) = r["reason"].as_str() {
        text = if text.is_empty() {
            reason.to_string()
        } else {
            format!("{text} · {reason}")
        };
    }
    let t = label(&text, &["usage-cell"]);
    t.set_hexpand(true);
    t.set_tooltip_text(Some(&text));
    b.append(&t);
    if outcome == "started" && r["session"].is_string() {
        let n = r["n"].as_u64().unwrap_or(0) as u32;
        let (w, nm) = (ws.to_string(), name.to_string());
        b.append(&button("Resume", &["flat", "sched-resume"], move || {
            resume(&w, &nm, n)
        }));
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

fn resume(ws: &str, name: &str, n: u32) {
    ask(
        "schedule.resume",
        json!({"workspace": ws, "name": name, "n": n}),
        report("Could not resume the run"),
    );
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
}

fn strings(list: &[String]) -> gtk::StringList {
    let v: Vec<&str> = list.iter().map(String::as_str).collect();
    gtk::StringList::new(&v)
}

fn section(title: &str) -> gtk::Label {
    label(title, &["usage-col", "sched-form-section"])
}

fn toggle_pair(a: &str, b: &str, class: &str) -> (gtk::Box, gtk::ToggleButton, gtk::ToggleButton) {
    let x = gtk::ToggleButton::with_label(a);
    let y = gtk::ToggleButton::with_label(b);
    y.set_group(Some(&x));
    x.set_active(true);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.add_css_class("linked");
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
        let kind = match self.agent().as_str() {
            "codex" => "codex",
            _ => "claude",
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
                        .set_label(&format!("Next: {}", times.join(" · ")));
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
            match rade_core::schedule::parse_at(&text, tz) {
                Ok(t) if t > now() => {}
                _ => ok = false,
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
        if self.repeat.is_active() {
            p["cron"] = json!(self.cron.text().trim());
        } else {
            p["at"] = json!(format!(
                "{} {}",
                self.date.text().trim(),
                self.clock.text().trim()
            ));
        }
        let pick = self.account.selected() as usize;
        p["account"] = match pick {
            0 => Value::Null,
            i => json!(self.account_names.borrow().get(i - 1)),
        };
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

    let dialog = adw::Dialog::new();
    dialog.set_title(if editing.is_some() {
        "Edit Schedule"
    } else {
        "New Schedule"
    });
    dialog.set_content_width(520);
    dialog.add_css_class("sched-form");
    let view = adw::ToolbarView::new();
    let bar = adw::HeaderBar::new();
    bar.set_show_end_title_buttons(false);
    bar.set_show_start_title_buttons(false);
    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("sched-form-cancel");
    let create = gtk::Button::with_label(if editing.is_some() { "Save" } else { "Create" });
    create.add_css_class("suggested-action");
    create.add_css_class("sched-form-create");
    bar.pack_start(&cancel);
    bar.pack_end(&create);
    view.add_top_bar(&bar);

    let col = gtk::Box::new(gtk::Orientation::Vertical, 8);
    col.add_css_class("sched-form-body");
    col.set_margin_start(18);
    col.set_margin_end(18);
    col.set_margin_top(12);
    col.set_margin_bottom(18);

    col.append(&section("NAME"));
    let name = gtk::Entry::new();
    name.set_placeholder_text(Some("nightly-deps"));
    name.add_css_class("sched-form-name");
    col.append(&name);

    col.append(&section("RUNS IN"));
    let (pair, by_project, by_folder) = toggle_pair("Project", "Folder", "sched-form-target");
    col.append(&pair);
    let project_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let pnames: Vec<String> = projects.iter().map(|p| p.0.clone()).collect();
    let project = gtk::DropDown::new(Some(strings(&pnames)), gtk::Expression::NONE);
    project.add_css_class("sched-form-project");
    project.set_hexpand(true);
    let link = gtk::DropDown::new(None::<gtk::StringList>, gtk::Expression::NONE);
    link.add_css_class("sched-form-link");
    project_box.append(&project);
    project_box.append(&link);
    col.append(&project_box);
    let folder_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let frow = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let folder = gtk::Entry::new();
    folder.set_placeholder_text(Some("/home/you/notes"));
    folder.set_hexpand(true);
    folder.add_css_class("sched-form-folder");
    let choose = gtk::Button::with_label("Choose…");
    frow.append(&folder);
    frow.append(&choose);
    folder_box.append(&frow);
    folder_box.append(&label("Runs in Scratch", &["usage-sub"]));
    col.append(&folder_box);
    if projects.is_empty() {
        by_folder.set_active(true);
    }

    col.append(&section("WHEN"));
    let (pair, repeat, once) = toggle_pair("Repeat", "Once", "sched-form-when");
    col.append(&pair);
    let cron_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let cron = gtk::Entry::new();
    cron.set_placeholder_text(Some("0 9 * * 1-5"));
    cron.add_css_class("sched-form-cron");
    cron_box.append(&cron);
    let presets_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    for (t, expr) in [
        ("Hourly", "0 * * * *"),
        ("Daily 09:00", "0 9 * * *"),
        ("Weekdays 09:00", "0 9 * * 1-5"),
    ] {
        let b = gtk::Button::with_label(t);
        b.add_css_class("flat");
        b.add_css_class("sched-form-preset");
        let c = cron.clone();
        b.connect_clicked(move |_| c.set_text(expr));
        presets_row.append(&b);
    }
    cron_box.append(&presets_row);
    let preview = label("", &["usage-sub", "sched-form-preview"]);
    preview.set_wrap(true);
    cron_box.append(&preview);
    col.append(&cron_box);
    let once_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let date = gtk::Entry::new();
    date.set_placeholder_text(Some("YYYY-MM-DD"));
    date.add_css_class("sched-form-date");
    let clock = gtk::Entry::new();
    clock.set_placeholder_text(Some("HH:MM"));
    clock.add_css_class("sched-form-time");
    once_box.append(&date);
    once_box.append(&clock);
    col.append(&once_box);

    col.append(&section("AGENT"));
    let arow = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let agent = gtk::DropDown::new(Some(strings(&presets)), gtk::Expression::NONE);
    agent.add_css_class("sched-form-agent");
    if let Some(i) = presets.iter().position(|p| p == "claude") {
        agent.set_selected(i as u32);
    }
    let account = gtk::DropDown::new(None::<gtk::StringList>, gtk::Expression::NONE);
    account.add_css_class("sched-form-account");
    account.set_hexpand(true);
    arow.append(&agent);
    arow.append(&account);
    col.append(&arow);

    col.append(&section("PROMPT"));
    let prompt = gtk::TextView::new();
    prompt.set_wrap_mode(gtk::WrapMode::WordChar);
    prompt.add_css_class("sched-form-prompt");
    let psc = gtk::ScrolledWindow::new();
    psc.set_min_content_height(90);
    psc.set_child(Some(&prompt));
    psc.add_css_class("sched-form-prompt-box");
    col.append(&psc);

    let catch_up = gtk::CheckButton::with_label("Run once when the app opens after a missed time");
    catch_up.add_css_class("sched-form-catchup");
    col.append(&catch_up);
    let krow = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    krow.append(&label("Keep the last", &["usage-cell"]));
    let keep = gtk::SpinButton::with_range(1.0, 20.0, 1.0);
    keep.set_value(3.0);
    keep.add_css_class("sched-form-keep");
    krow.append(&keep);
    krow.append(&label("run Panes open", &["usage-cell"]));
    col.append(&krow);
    let error = label("", &["error", "sched-form-error"]);
    error.set_wrap(true);
    error.set_visible(false);
    col.append(&error);

    let sc = gtk::ScrolledWindow::new();
    sc.set_propagate_natural_height(true);
    sc.set_max_content_height(640);
    sc.set_child(Some(&col));
    view.set_content(Some(&sc));
    dialog.set_child(Some(&view));

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
    dialog.connect_closed(|_| {
        FORM.with(|f| f.borrow_mut().take());
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
