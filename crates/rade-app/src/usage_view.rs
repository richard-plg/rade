//! The Usage page (PRD R110.2, spec e-usage.md): the main-area page the
//! Statusline usage cell, the titlebar `gauge` button and palette `> usage`
//! open. Header (gauge, title, sub-line, period Today / 7 days / 30 days,
//! Workspace filter, Add account), the account table sorted by Tightest
//! limit (no data last, ≥ 80 % in `wait` with `warning-diamond`), Next
//! resets, and the detail card of the selected account (limit groups, a
//! 7-day tokens chart without cache reads, tokens by Project / model).
//!
//! The figures are the `usage.list` / `usage.get` answers (the same as
//! `rade account usage`), read off the main loop. Shown until the focus
//! moves to a checkout; the breadcrumb reads `Agent usage`.
//! `debug.ui {"op": "usage"}` reads what it shows.

use crate::app::try_app;
use adw::prelude::*;
use gtk::glib;
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::account::AgentKind;
use rade_core::rpc::{Error, Result};
use rade_core::usage::report::Period;
use rade_core::usage::view as uv;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};

/// The page's name in the center stack.
pub const PAGE: &str = "usage";
/// The account table's meter (E.14) and the detail's limit bar (E.28).
const METER: u32 = 56;
const BAR: u32 = 156;
/// The chart's bar area under its 18 px top padding (E.32).
const CHART_H: f64 = 92.0;
/// A chart value label's height (JetBrains Mono 10.5 px).
const VALUE_H: i32 = 14;

/// (source, worktree, tab) in focus when the page opened.
type Focus = (String, String, Option<String>);

/// What the viewer chose: period, Workspace filter, selected account.
#[derive(Clone, Debug, Default, PartialEq)]
struct Choice {
    period: Option<String>,
    /// `None` = the shown Workspace; `Some("")` = All workspaces.
    filter: Option<String>,
    /// (agent, name, workspace).
    selected: Option<(String, String, String)>,
}

/// One load's answers.
#[derive(Clone, Debug, Default, PartialEq)]
struct Data {
    choice: Choice,
    /// The Workspace the filter resolved to (`None` = all).
    workspace: Option<String>,
    shown_workspace: String,
    /// (id, name) of every Workspace, for the filter menu.
    workspaces: Vec<(String, String)>,
    list: Value,
    detail: Option<Value>,
    week: Option<Value>,
    selected: Option<(String, String, String)>,
    home: String,
    now: i64,
}

thread_local! {
    static SHOWN: RefCell<Option<Focus>> = const { RefCell::new(None) };
    static ROOT: RefCell<Option<gtk::ScrolledWindow>> = const { RefCell::new(None) };
    static CHOICE: RefCell<Choice> = RefCell::new(Choice::default());
    static DATA: RefCell<Option<Data>> = const { RefCell::new(None) };
    static RENDERED: RefCell<Option<Data>> = const { RefCell::new(None) };
    static GEN: Cell<u64> = const { Cell::new(0) };
    static LOADING: Cell<bool> = const { Cell::new(false) };
    /// (detail below the list, large text) the page was built for (R111).
    static BUILT: Cell<(bool, bool)> = const { Cell::new((false, false)) };
}

/// The page's padding left and right (spec E.2: 24 px each side).
const PAGE_PAD: i32 = 48;

/// How the page lays out now (R111): the detail below the list, large text.
fn shape() -> (bool, bool) {
    let mode = crate::layout_mode::mode();
    let content = crate::layout_mode::center_width() - PAGE_PAD;
    (
        rade_core::chrome::usage_detail_below(content, mode),
        mode.large_text,
    )
}

/// The window's width or text scale changed: rebuild when the shape did.
pub fn relayout() {
    if BUILT.with(Cell::get) != shape() {
        RENDERED.with(|r| r.borrow_mut().take());
        render();
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
    // Narrower than the page (the Inspector open at 1440 px): scroll, never
    // grow the window (the 1280 px layout is M40).
    sc.set_hscrollbar_policy(gtk::PolicyType::Automatic);
    sc.add_css_class("usage-scroll");
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
    // "Updated" ages stay fresh.
    glib::timeout_add_seconds_local(20, || {
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

/// Open the page (the usage cell, the `gauge` button, palette `> usage`).
pub fn open() {
    let Some(app) = try_app() else { return };
    crate::home_view::close();
    let at = focus_key(&app.model.borrow());
    SHOWN.with(|s| *s.borrow_mut() = Some(at));
    app.sync();
    refresh();
    if let Some(r) = ROOT.with(|r| r.borrow().clone()) {
        r.grab_focus();
    }
}

/// Leave the page (a Navigator row was clicked).
pub fn close() {
    SHOWN.with(|s| s.borrow_mut().take());
}

/// The header's close button or Esc: back to the Pane that had the focus.
pub(crate) fn leave() {
    close();
    if let Some(app) = try_app() {
        app.sync();
        app.restore_focus();
    }
}

pub(crate) fn shown_now() -> bool {
    try_app().is_some_and(|a| is_shown(&a.model.borrow()))
}

/// Load the figures again (off the main loop) and redraw when they changed.
pub fn refresh() {
    let Some(app) = try_app() else { return };
    if !shown_now() {
        return;
    }
    let choice = CHOICE.with(|c| c.borrow().clone());
    let (shown_ws, workspaces) = {
        let m = app.model.borrow();
        let names = m
            .workspace_looks()
            .into_iter()
            .map(|l| (l.id, l.name))
            .collect::<Vec<_>>();
        (m.active_workspace().to_string(), names)
    };
    let gen = GEN.with(|g| {
        g.set(g.get() + 1);
        g.get()
    });
    LOADING.with(|l| l.set(true));
    let home = rade_core::paths::home_dir(&rade_core::paths::process_env)
        .display()
        .to_string();
    std::thread::spawn(move || {
        let t = std::time::Instant::now();
        let data = load(choice, shown_ws, workspaces, home);
        let loaded = t.elapsed();
        glib::MainContext::default().invoke(move || {
            crate::perf::record("usage_load", loaded, false);
            if GEN.with(|g| g.get()) != gen {
                return;
            }
            LOADING.with(|l| l.set(false));
            DATA.with(|d| *d.borrow_mut() = Some(data));
            render();
        });
    });
}

fn call(method: &str, p: Value) -> Value {
    crate::accounts::dispatch(method, p).unwrap_or_else(|e| json!({"error": e.message}))
}

fn load(choice: Choice, shown_ws: String, workspaces: Vec<(String, String)>, home: String) -> Data {
    let period = choice.period.clone().unwrap_or_else(|| "7d".into());
    let workspace = match &choice.filter {
        Some(w) if w.is_empty() => None,
        Some(w) => Some(w.clone()),
        None => Some(shown_ws.clone()),
    };
    let list = match &workspace {
        Some(w) => call("usage.list", json!({"workspace": w, "period": period})),
        None => call("usage.list", json!({"all": true, "period": period})),
    };
    let rows: Vec<(String, String, String)> = list["accounts"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| {
                    (
                        r["agent"].as_str().unwrap_or("").to_string(),
                        r["name"].as_str().unwrap_or("").to_string(),
                        r["workspace"].as_str().unwrap_or("").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let selected = choice
        .selected
        .clone()
        .filter(|s| rows.contains(s))
        .or_else(|| rows.first().cloned());
    let get = |per: &str| {
        selected.as_ref().map(|(agent, name, ws)| {
            call(
                "usage.get",
                json!({"account": name, "agent": agent, "workspace": ws, "period": per}),
            )
        })
    };
    let detail = get(&period);
    let week = if period == "7d" {
        detail.clone()
    } else {
        get("7d")
    };
    Data {
        choice,
        workspace,
        shown_workspace: shown_ws,
        workspaces,
        list,
        detail,
        week,
        selected,
        home,
        now: crate::panes::now() as i64,
    }
}

fn set_choice(f: impl FnOnce(&mut Choice)) {
    CHOICE.with(|c| f(&mut c.borrow_mut()));
    refresh();
}

/// `↑` / `↓`: the previous / next account row.
fn step(down: bool) {
    let Some(d) = DATA.with(|d| d.borrow().clone()) else {
        return;
    };
    let rows = row_keys(&d.list);
    let at = d
        .selected
        .as_ref()
        .and_then(|s| rows.iter().position(|r| r == s));
    let to = match at {
        Some(i) if down => (i + 1).min(rows.len().saturating_sub(1)),
        Some(i) => i.saturating_sub(1),
        None => 0,
    };
    if let Some(r) = rows.get(to).cloned() {
        set_choice(|c| c.selected = Some(r));
    }
}

fn row_keys(list: &Value) -> Vec<(String, String, String)> {
    list["accounts"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| {
                    (
                        s(&r["agent"]).to_string(),
                        s(&r["name"]).to_string(),
                        s(&r["workspace"]).to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

fn render() {
    let Some(d) = DATA.with(|d| d.borrow().clone()) else {
        return;
    };
    let root = ROOT.with(|r| r.borrow().clone());
    if RENDERED.with(|r| r.borrow().as_ref() == Some(&d)) {
        if let Some(root) = &root {
            crate::perf::ready("usage_page", root);
        }
        return;
    }
    if let Some(root) = root {
        let had_focus = root
            .root()
            .and_then(|r| r.focus())
            .is_some_and(|f| f.is_ancestor(&root) || f == root.clone().upcast::<gtk::Widget>());
        let t = std::time::Instant::now();
        root.set_child(Some(&build(&d)));
        crate::perf::record("usage_build", t.elapsed(), false);
        if had_focus {
            root.grab_focus();
        }
        crate::perf::ready("usage_page", &root);
    }
    RENDERED.with(|r| *r.borrow_mut() = Some(d));
}

// ---- Widgets -----------------------------------------------------------------

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

fn icon(name: &str, classes: &[&str]) -> gtk::Image {
    let i = gtk::Image::from_icon_name(name);
    for c in classes {
        i.add_css_class(c);
    }
    i
}

/// A secondary button (A.15 style): icon, text, optional caret.
fn secondary(icon_name: &str, text: &str, class: &str, caret: bool) -> gtk::Button {
    let b = gtk::Button::new();
    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    inner.append(&icon(icon_name, &["btn-icon"]));
    if !text.is_empty() {
        inner.append(&label(text, &[]));
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

fn badge(agent: &str, size: i32, extra: &str) -> gtk::Label {
    let b = gtk::Label::new(Some(if agent == "codex" { "X" } else { "C" }));
    b.add_css_class("agent-badge");
    b.add_css_class(&format!("agent-badge-{agent}"));
    if !extra.is_empty() {
        b.add_css_class(extra);
    }
    b.set_size_request(size, size);
    b.set_valign(gtk::Align::Center);
    b.set_halign(gtk::Align::Center);
    b
}

/// A fixed column width; at large text it grows with the text (R111.2).
fn sized(w: &impl IsA<gtk::Widget>, width: i32) {
    w.set_size_request(scaled(width), -1);
}

fn scaled(px: i32) -> i32 {
    if crate::layout_mode::mode().large_text {
        (f64::from(px) * crate::layout_mode::text_scale()).round() as i32
    } else {
        px
    }
}

/// A meter / bar: track `class`, fill `class-fill` of `fill` px.
fn meter(class: &str, width: u32, height: i32, fill: u32) -> gtk::Box {
    let m = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    m.add_css_class(class);
    m.set_size_request(width as i32, height);
    m.set_valign(gtk::Align::Center);
    let f = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    f.add_css_class(&format!("{class}-fill"));
    f.set_size_request(fill as i32, height);
    f.set_halign(gtk::Align::Start);
    f.set_visible(fill > 0);
    m.append(&f);
    m
}

fn period_of(d: &Data) -> Period {
    Period::parse(d.choice.period.as_deref()).unwrap_or_default()
}

fn filter_label(d: &Data) -> String {
    match &d.workspace {
        None => "All workspaces".into(),
        Some(w) => d
            .workspaces
            .iter()
            .find(|(id, _)| id == w)
            .map_or_else(|| w.clone(), |(_, n)| n.clone()),
    }
}

/// The text of a Tightest limit cell: (value, name, hot) or the empty text.
fn tightest(r: &Value) -> std::result::Result<(i64, String, bool), String> {
    match r["tightest"]["used_percent"].as_f64() {
        Some(p) => Ok((
            p.round() as i64,
            s(&r["tightest"]["name"]).to_string(),
            p >= rade_core::usage::limits::HOT_PERCENT,
        )),
        None if s(&r["state"]) == "format-changed" => Err(rade_core::usage::FORMAT_CHANGED.into()),
        None => Err(uv::NO_DATA_YET.into()),
    }
}

fn build(d: &Data) -> gtk::Widget {
    let (below, large) = shape();
    BUILT.with(|b| b.set((below, large)));
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.add_css_class("usage-page");
    let rows = d.list["accounts"].as_array().cloned().unwrap_or_default();

    // Header (E.3–E.7).
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    header.add_css_class("usage-header");
    header.append(&icon("ph-gauge-symbolic", &["usage-icon"]));
    let titles = gtk::Box::new(gtk::Orientation::Vertical, 2);
    titles.append(&label("Agent usage", &["usage-title"]));
    let n = rows.len();
    let sub = label(
        &format!(
            "{n} account{} · limits from each CLI · tokens from local session logs",
            if n == 1 { "" } else { "s" }
        ),
        &["usage-sub"],
    );
    sub.set_ellipsize(gtk::pango::EllipsizeMode::End);
    sub.set_tooltip_text(Some(&sub.text()));
    titles.append(&sub);
    titles.set_hexpand(true);
    titles.set_valign(gtk::Align::Center);
    header.append(&titles);
    let period = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    period.add_css_class("usage-period");
    period.set_valign(gtk::Align::Center);
    let current = period_of(d);
    for (p, text, key) in [
        (Period::Today, "Today", "today"),
        (Period::Week, "7 days", "7d"),
        (Period::Month, "30 days", "30d"),
    ] {
        let b = gtk::Button::with_label(text);
        b.add_css_class("usage-period-seg");
        b.add_css_class("flat");
        if p == current {
            b.add_css_class("active");
        }
        b.set_focus_on_click(false);
        b.connect_clicked(move |_| set_choice(|c| c.period = Some(key.into())));
        period.append(&b);
    }
    header.append(&period);
    let filter = secondary(
        "ph-circles-four-symbolic",
        &filter_label(d),
        "usage-ws-filter",
        true,
    );
    let menu_items: Vec<(String, String)> =
        std::iter::once((String::new(), "All workspaces".into()))
            .chain(d.workspaces.iter().cloned())
            .collect();
    filter.connect_clicked(move |b| filter_menu(b, &menu_items));
    header.append(&filter);
    let add = secondary("ph-plus-symbolic", "Add account", "usage-add", false);
    let ws_for_add = d.workspace.clone().unwrap_or(d.shown_workspace.clone());
    add.connect_clicked(move |_| add_account_dialog(&ws_for_add));
    header.append(&add);
    let close_b = gtk::Button::from_icon_name("ph-x-symbolic");
    close_b.add_css_class("dialog-close");
    close_b.add_css_class("flat");
    close_b.add_css_class("usage-close");
    close_b.set_tooltip_text(Some("Close (Esc)"));
    close_b.set_valign(gtk::Align::Center);
    close_b.connect_clicked(|_| leave());
    header.append(&close_b);
    if large {
        // Large text: the controls go under the title (no sideways scroll).
        let controls = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        controls.add_css_class("usage-header-controls");
        for w in [
            period.upcast_ref::<gtk::Widget>(),
            filter.upcast_ref(),
            add.upcast_ref(),
        ] {
            header.remove(w);
            controls.append(w);
        }
        let top = gtk::Box::new(gtk::Orientation::Vertical, 10);
        top.append(&header);
        top.append(&controls);
        page.append(&top);
    } else {
        page.append(&header);
    }

    // List · detail (E.8).
    let grid = gtk::Box::new(
        if below {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        },
        16,
    );
    grid.add_css_class("usage-grid");
    let left = gtk::Box::new(gtk::Orientation::Vertical, 0);
    left.add_css_class("usage-accounts");
    left.set_hexpand(true);
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("usage-accounts-card");
    if rows.is_empty() {
        let empty = label(
            &format!("No Agent Accounts in {}.", filter_label(d)),
            &["usage-empty"],
        );
        card.append(&empty);
        left.append(&card);
        grid.append(&left);
        page.append(&grid);
        return page.upcast();
    }
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    head.add_css_class("usage-table-head");
    for (text, w, right) in [
        ("ACCOUNT", -1, false),
        ("PLAN", 80, false),
        ("TIGHTEST LIMIT", 220, false),
        (uv::tokens_head(current), 90, true),
        ("UPDATED", 70, false),
    ] {
        // Large text: Plan and Updated live on each row's second line.
        if large && (text == "PLAN" || text == "UPDATED") {
            continue;
        }
        let l = label(text, &["usage-col"]);
        if w < 0 {
            l.set_hexpand(true);
        } else {
            sized(&l, w);
        }
        if right {
            l.set_xalign(1.0);
        }
        head.append(&l);
    }
    card.append(&head);
    for r in &rows {
        card.append(&account_row(d, r));
    }
    left.append(&card);

    // Next resets (E.21–E.24).
    let resets = d.list["resets"].as_array().cloned().unwrap_or_default();
    if !resets.is_empty() {
        let rh = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        rh.add_css_class("usage-resets-header");
        let l = label("NEXT RESETS", &["section-label"]);
        l.set_hexpand(true);
        rh.append(&l);
        rh.append(&label("quota coming back", &["usage-resets-note"]));
        left.append(&rh);
        let rc = gtk::Box::new(gtk::Orientation::Vertical, 0);
        rc.add_css_class("usage-resets");
        for (i, r) in resets.iter().enumerate() {
            rc.append(&reset_row(r, i == 0));
        }
        left.append(&rc);
    }
    grid.append(&left);
    if let Some(det) = &d.detail {
        let card = detail_card(d, det);
        if below {
            // Full width under Next resets (E "1280 px").
            card.set_size_request(-1, -1);
            card.set_hexpand(true);
        }
        grid.append(&card);
    }
    page.append(&grid);
    page.upcast()
}

fn select_on_click(w: &impl IsA<gtk::Widget>, key: (String, String, String)) {
    let click = gtk::GestureClick::new();
    click.connect_released(move |_, _, _, _| {
        let k = key.clone();
        set_choice(|c| c.selected = Some(k));
    });
    w.add_controller(click);
}

fn account_row(d: &Data, r: &Value) -> gtk::Box {
    let key = (
        s(&r["agent"]).to_string(),
        s(&r["name"]).to_string(),
        s(&r["workspace"]).to_string(),
    );
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    row.add_css_class("usage-row");
    row.set_widget_name(&format!("usage-row-{}-{}", key.0, key.1));
    if d.selected.as_ref() == Some(&key) {
        row.add_css_class("selected");
    }
    let t = tightest(r);
    if t.is_err() {
        row.add_css_class("no-data");
        row.set_opacity(0.6);
    }
    let cell = |classes: &[&str]| {
        let c = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        c.add_css_class("usage-cell");
        for k in classes {
            c.add_css_class(k);
        }
        c
    };
    let acct = cell(&["usage-cell-account"]);
    acct.set_hexpand(true);
    acct.append(&badge(&key.0, 18, ""));
    let name = label(&key.1, &["usage-account-name"]);
    name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    acct.append(&name);
    if key.2 != d.shown_workspace {
        let lock = icon("ph-lock-simple-symbolic", &["usage-lock"]);
        lock.set_tooltip_text(Some(&format!(
            "Declared in Workspace {}: read only here",
            key.2
        )));
        acct.append(&lock);
    }
    let ws_name = d
        .workspaces
        .iter()
        .find(|(id, _)| *id == key.2)
        .map_or(key.2.clone(), |(_, n)| n.clone());
    let chip = label(
        &if r["default"].as_bool() == Some(true) {
            format!("{ws_name} ✓")
        } else {
            ws_name
        },
        &["usage-ws-chip"],
    );
    chip.set_valign(gtk::Align::Center);
    acct.append(&chip);
    row.append(&acct);

    let large = crate::layout_mode::mode().large_text;
    let plan = cell(&["usage-plan"]);
    sized(&plan, 80);
    // The Plan column ellipsizes first (E "1280 px").
    let plan_text = r["plan"].as_str().unwrap_or("—").to_string();
    let pl = label(&plan_text, &[]);
    pl.set_ellipsize(gtk::pango::EllipsizeMode::End);
    plan.append(&pl);
    if !large {
        row.append(&plan);
    }

    let tl = cell(&["usage-tightest"]);
    sized(&tl, 220);
    match &t {
        Ok((p, name, hot)) => {
            if *hot {
                tl.add_css_class("hot");
            }
            tl.append(&meter(
                "usage-meter",
                METER,
                5,
                uv::meter_fill(METER, *p as f64),
            ));
            let v = gtk::Box::new(gtk::Orientation::Horizontal, 3);
            v.add_css_class("usage-tightest-box");
            if *hot {
                v.append(&icon(
                    "ph-warning-diamond-fill-symbolic",
                    &["usage-hot-icon"],
                ));
            }
            v.append(&label(&format!("{p}%"), &["usage-tightest-value"]));
            tl.append(&v);
            tl.append(&label(name, &["usage-tightest-name"]));
        }
        Err(text) => tl.append(&label(text, &["usage-tightest-none"])),
    }
    row.append(&tl);

    let tokens = cell(&["usage-tokens-cell"]);
    sized(&tokens, 90);
    let tk = label(
        &uv::tokens_text(r["tokens"].as_u64().unwrap_or(0)),
        &["usage-tokens"],
    );
    tk.set_xalign(1.0);
    // Fixed: an expanding child would take a share of the row's extra width.
    tk.set_size_request(70, -1);
    tokens.append(&tk);
    row.append(&tokens);

    let updated = uv::updated_text(r["updated"].as_i64(), d.now);
    if large {
        // Plan and Updated on a second line (fg3, E "Text scale 200 %").
        let outer = gtk::Box::new(gtk::Orientation::Vertical, 2);
        for c in row.css_classes() {
            outer.add_css_class(&c);
        }
        outer.set_widget_name(&row.widget_name());
        outer.set_opacity(row.opacity());
        row.set_css_classes(&["usage-row-line"]);
        row.set_opacity(1.0);
        outer.append(&row);
        let second = label(
            &format!("{plan_text} · updated {updated}"),
            &["usage-row-second", "usage-updated"],
        );
        second.set_wrap(true);
        outer.append(&second);
        select_on_click(&outer, key);
        return outer;
    }
    let upd = cell(&["usage-updated-cell"]);
    sized(&upd, 70);
    upd.append(&label(&updated, &["usage-updated"]));
    row.append(&upd);
    select_on_click(&row, key);
    row
}

fn reset_row(r: &Value, soonest: bool) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("usage-reset-row");
    let when = label(s(&r["when"]), &["usage-reset-time"]);
    if soonest {
        when.add_css_class("soon");
    }
    sized(&when, 86);
    row.append(&when);
    let agent = r["agent"].as_str().unwrap_or("claude");
    let b = badge(agent, 18, "");
    row.append(&b);
    let acct = label(s(&r["account"]), &["usage-reset-account"]);
    sized(&acct, 90);
    acct.set_ellipsize(gtk::pango::EllipsizeMode::End);
    row.append(&acct);
    let what = label(s(&r["what"]), &["usage-reset-what"]);
    what.set_hexpand(true);
    what.set_ellipsize(gtk::pango::EllipsizeMode::End);
    row.append(&what);
    let note = label(s(&r["note"]), &["usage-reset-forecast"]);
    note.set_xalign(1.0);
    row.append(&note);
    if let (Some(a), Some(w)) = (r["agent"].as_str(), r["workspace"].as_str()) {
        select_on_click(&row, (a.into(), s(&r["account"]).into(), w.into()));
    }
    row
}

/// The limit rows of the detail (E.27–E.31) as (group, rows of (name,
/// value, reset, percent, hot)); Claude without Detailed limits gets `—`
/// rows for Opus / Sonnet / OAuth apps (design §10.5 A).
type LimitRow = (String, String, String, Option<f64>, bool);

fn limit_groups(det: &Value, now: i64) -> Vec<(String, Vec<LimitRow>)> {
    let offset = rade_core::date::local_offset(now);
    let mut out: Vec<(String, Vec<LimitRow>)> = Vec::new();
    for g in det["groups"].as_array().into_iter().flatten() {
        let rows: Vec<LimitRow> = g["limits"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| {
                let mut v = l.clone();
                if let Some(o) = v.as_object_mut() {
                    if let Some(t) = o.remove("sampled_at") {
                        o.insert("t".into(), t);
                    }
                }
                serde_json::from_value::<rade_core::usage::Limit>(v).ok()
            })
            .map(|l| {
                let p = l.used_percent;
                (
                    l.name.clone(),
                    uv::value_text(&l),
                    uv::reset_text(&l, now, offset),
                    p,
                    p.is_some_and(|p| p >= rade_core::usage::limits::HOT_PERCENT),
                )
            })
            .collect();
        out.push((s(&g["group"]).to_string(), rows));
    }
    let claude = s(&det["agent"]) == "claude";
    if claude && det["detailed_limits"].as_bool() != Some(true) && s(&det["state"]) == "ok" {
        let off: Vec<LimitRow> = ["Opus", "Sonnet", "OAuth apps"]
            .iter()
            .map(|n| (n.to_string(), "—".into(), String::new(), None, false))
            .collect();
        match out.iter_mut().find(|(g, _)| g == "Weekly") {
            Some((_, rows)) => {
                for r in off {
                    if !rows.iter().any(|x| x.0 == r.0) {
                        rows.push(r);
                    }
                }
            }
            None => out.push(("Weekly".into(), off)),
        }
        out.sort_by_key(|(g, _)| match g.as_str() {
            "Session" => 0,
            "Weekly" => 1,
            "Extra usage" => 2,
            _ => 3,
        });
    }
    out
}

fn detail_card(d: &Data, det: &Value) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("usage-detail");
    card.set_size_request(440, -1);
    card.set_valign(gtk::Align::Start);
    let agent = s(&det["agent"]).to_string();
    let kind = AgentKind::parse(&agent).unwrap_or(AgentKind::Claude);
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    head.add_css_class("usage-detail-header");
    head.append(&badge(&agent, 24, "usage-detail-badge"));
    let titles = gtk::Box::new(gtk::Orientation::Vertical, 1);
    titles.append(&label(s(&det["account"]), &["usage-detail-name"]));
    let sub = uv::detail_sub(kind, det["plan"].as_str(), s(&det["config_dir"]), &d.home);
    let sub = label(&sub, &["usage-detail-sub"]);
    sub.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    titles.append(&sub);
    titles.set_hexpand(true);
    head.append(&titles);
    let menu = secondary("ph-dots-three-symbolic", "", "usage-detail-menu", false);
    let det2 = det.clone();
    menu.connect_clicked(move |b| detail_menu(b, &det2));
    head.append(&menu);
    card.append(&head);

    if s(&det["state"]) != "ok" {
        let what = if s(&det["state"]) == "format-changed" {
            rade_core::usage::FORMAT_CHANGED
        } else {
            uv::NO_DATA_YET
        };
        card.append(&label(what, &["usage-detail-empty"]));
        card.append(&label(uv::how_data_arrives(kind), &["usage-detail-how"]));
        return card;
    }

    let detailed_off = kind == AgentKind::Claude && det["detailed_limits"].as_bool() != Some(true);
    for (g, rows) in limit_groups(det, d.now) {
        card.append(&label(
            &g.to_uppercase(),
            &["section-label", "usage-limit-group"],
        ));
        for (name, value, reset, p, hot) in rows {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            row.add_css_class("usage-limit-row");
            if hot {
                row.add_css_class("hot");
            }
            let n = label(&name, &["usage-limit-name"]);
            sized(&n, 88);
            row.append(&n);
            let fill = p.map_or(0, |p| uv::meter_fill(BAR, p));
            row.append(&meter("usage-limit-bar", BAR, 6, fill));
            let v = label(&value, &["usage-limit-value"]);
            v.set_xalign(1.0);
            sized(&v, 40);
            row.append(&v);
            let r = label(&reset, &["usage-limit-reset"]);
            sized(&r, 92);
            row.append(&r);
            card.append(&row);
        }
        if let (true, Some(t)) = (g == "Weekly", det["detailed_note"].as_str()) {
            // R125.6: a token login can't ask for Detailed limits.
            card.append(&label(
                &format!("Detailed limits {t}"),
                &["usage-detailed-note"],
            ));
        } else if g == "Weekly" && detailed_off {
            let note = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            note.add_css_class("usage-detailed-off");
            note.append(&label("Detailed limits off ·", &["usage-detailed-note"]));
            let en = gtk::Button::with_label("Enable…");
            en.add_css_class("flat");
            en.add_css_class("home-link-button");
            let (name, ws) = (
                s(&det["account"]).to_string(),
                s(&det["workspace"]).to_string(),
            );
            en.connect_clicked(move |_| set_detailed(&ws, &name, true));
            note.append(&en);
            card.append(&note);
        }
    }

    // Tokens per day (E.32): the 7 days ending today, cache reads excluded.
    let th = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    th.add_css_class("usage-limit-group");
    let l = label("TOKENS PER DAY", &["section-label"]);
    l.set_hexpand(true);
    th.append(&l);
    th.append(&label("cache reads excluded", &["usage-chart-note"]));
    card.append(&th);
    let days: Vec<(String, u64)> = d
        .week
        .as_ref()
        .and_then(|w| w["tokens"]["by_day"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|x| (s(&x["day"]).to_string(), x["tokens"].as_u64().unwrap_or(0)))
        .collect();
    let max = days.iter().map(|x| x.1).max().unwrap_or(0);
    // Large text: a 160 px bar area (E "Text scale 200 %").
    let large = crate::layout_mode::mode().large_text;
    let chart_h = if large {
        f64::from(rade_core::chrome::USAGE_CHART_LARGE)
    } else {
        CHART_H
    };
    let value_h = scaled(VALUE_H);
    let chart = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    chart.add_css_class("usage-chart");
    if large {
        chart.set_size_request(-1, chart_h as i32);
    }
    chart.set_homogeneous(true);
    let letters = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    letters.add_css_class("usage-chart-days");
    letters.set_homogeneous(true);
    let max_at = days.iter().position(|x| x.1 == max && max > 0);
    // The max and today values sit above their bars, in the chart's top
    // padding for the tallest one: a label row over the chart.
    let values = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    values.add_css_class("usage-chart-values");
    values.set_homogeneous(true);
    values.set_valign(gtk::Align::Start);
    values.set_can_target(false);
    for (i, (day, n)) in days.iter().enumerate() {
        let today = i + 1 == days.len();
        let h = if max == 0 {
            0
        } else {
            ((*n as f64 / max as f64) * chart_h)
                .round()
                .max(if *n > 0 { 2.0 } else { 0.0 }) as i32
        };
        let col = gtk::Box::new(gtk::Orientation::Vertical, 0);
        col.add_css_class("usage-chart-col");
        col.set_valign(gtk::Align::End);
        let bar = gtk::Box::new(gtk::Orientation::Vertical, 0);
        bar.add_css_class("usage-chart-bar");
        if today {
            bar.add_css_class("today");
        }
        bar.set_size_request(-1, h);
        bar.set_visible(h > 0);
        bar.set_tooltip_text(Some(&format!("{day} {}", uv::tokens_text(*n))));
        col.append(&bar);
        chart.append(&col);
        let v = label(
            &if (Some(i) == max_at || today) && *n > 0 {
                uv::tokens_text(*n)
            } else {
                String::new()
            },
            &["usage-chart-value"],
        );
        v.set_xalign(0.5);
        // Its bottom 3 px above the bar's top (18 px padding + 92 px area).
        v.set_margin_top((18 + chart_h as i32 - h - value_h - 3).max(0));
        v.set_valign(gtk::Align::Start);
        values.append(&v);
        let dl = label(&uv::day_letter(day), &["usage-chart-day"]);
        dl.set_xalign(0.5);
        letters.append(&dl);
    }
    let over = gtk::Overlay::new();
    over.set_child(Some(&chart));
    over.add_overlay(&values);
    card.append(&over);
    card.append(&letters);

    // By Project · by model (E.33), for the page's period.
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    footer.add_css_class("usage-detail-footer");
    for p in det["tokens"]["by_project"]
        .as_array()
        .into_iter()
        .flatten()
        .take(3)
    {
        footer.append(&label(
            &format!(
                "{} {}",
                s(&p["project"]),
                uv::tokens_text(p["tokens"].as_u64().unwrap_or(0))
            ),
            &["usage-footer-item"],
        ));
    }
    let models: Vec<(String, u64)> = det["tokens"]["by_model"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| {
            (
                s(&m["model"]).to_string(),
                m["tokens"].as_u64().unwrap_or(0),
            )
        })
        .collect();
    let split = label(
        &uv::model_split(&models),
        &["usage-footer-item", "usage-footer-models"],
    );
    split.set_hexpand(true);
    split.set_xalign(1.0);
    footer.append(&split);
    card.append(&footer);
    card
}

/// A small popover of buttons under `anchor`.
fn popover(anchor: &gtk::Button, items: Vec<(String, Box<dyn Fn()>)>) {
    let pop = gtk::Popover::new();
    pop.add_css_class("menu");
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    for (text, f) in items {
        let b = gtk::Button::with_label(&text);
        b.add_css_class("flat");
        let p = pop.clone();
        b.connect_clicked(move |_| {
            p.popdown();
            f();
        });
        list.append(&b);
    }
    pop.set_child(Some(&list));
    pop.set_parent(anchor);
    pop.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    pop.popup();
}

fn filter_menu(anchor: &gtk::Button, items: &[(String, String)]) {
    let items = items
        .iter()
        .map(|(id, name)| {
            let id = id.clone();
            let f: Box<dyn Fn()> = Box::new(move || {
                let id = id.clone();
                set_choice(|c| c.filter = Some(id));
            });
            (name.clone(), f)
        })
        .collect();
    popover(anchor, items);
}

fn detail_menu(anchor: &gtk::Button, det: &Value) {
    let (name, ws, agent) = (
        s(&det["account"]).to_string(),
        s(&det["workspace"]).to_string(),
        s(&det["agent"]).to_string(),
    );
    let mut items: Vec<(String, Box<dyn Fn()>)> = Vec::new();
    let (n, w, a) = (name.clone(), ws.clone(), agent.clone());
    items.push((
        "Refresh now".into(),
        Box::new(move || {
            let p = json!({"account": n, "workspace": w, "agent": a});
            std::thread::spawn(move || {
                let _ = crate::accounts::dispatch("usage.refresh", p);
            });
        }),
    ));
    if agent == "claude" {
        let on = det["detailed_limits"].as_bool() == Some(true);
        items.push((
            if on {
                "Turn Detailed limits off".into()
            } else {
                "Turn Detailed limits on…".into()
            },
            Box::new(move || set_detailed(&ws, &name, !on)),
        ));
    }
    popover(anchor, items);
}

/// Detailed limits on / off for a Claude account (`workspace.toml`).
fn set_detailed(ws: &str, name: &str, on: bool) {
    let Some(app) = try_app() else { return };
    let root = app.model.borrow().workspace(ws).map(|w| w.root.clone());
    let Some(root) = root else { return };
    let found = crate::accounts::lists_cached(&app.model.borrow())
        .get(ws)
        .and_then(|l| l.as_ref().ok())
        .and_then(|l| {
            l.iter()
                .find(|a| a.agent == AgentKind::Claude && a.name == name)
                .cloned()
        });
    let res = match &found {
        Some(a) => crate::accounts::set_detailed(a, &root, on),
        None => {
            rade_core::account::set_detailed_file(&root, AgentKind::Claude, name, on).map(|_| ())
        }
    };
    if let Err(e) = res {
        crate::worktrees::alert("Could not change Detailed limits", &e.message);
    }
    crate::accounts::forget_lists();
    refresh();
}

/// Add account (E.6): agent, name, config dir, default → `account.add`.
fn add_account_dialog(ws: &str) {
    let Some(app) = try_app() else { return };
    let dlg = adw::AlertDialog::new(Some("Add Agent Account"), None);
    let form = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let agent = gtk::DropDown::from_strings(&["claude", "codex"]);
    let name = gtk::Entry::new();
    name.set_placeholder_text(Some("Name (acme-work)"));
    let dir = gtk::Entry::new();
    dir.set_placeholder_text(Some("Config dir (~/.claude-work)"));
    let def = gtk::CheckButton::with_label("Default for this agent");
    for w in [
        agent.upcast_ref::<gtk::Widget>(),
        name.upcast_ref(),
        dir.upcast_ref(),
        def.upcast_ref(),
    ] {
        form.append(w);
    }
    dlg.set_extra_child(Some(&form));
    dlg.add_responses(&[("cancel", "Cancel"), ("add", "Add")]);
    dlg.set_default_response(Some("add"));
    let ws = ws.to_string();
    dlg.connect_response(None, move |_, r| {
        if r != "add" {
            return;
        }
        let p = json!({
            "workspace": ws,
            "agent": if agent.selected() == 1 { "codex" } else { "claude" },
            "name": name.text().as_str(),
            "config_dir": dir.text().as_str(),
            "default": def.is_active(),
        });
        std::thread::spawn(move || {
            let r = crate::accounts::dispatch("account.add", p);
            glib::MainContext::default().invoke(move || match r {
                Ok(_) => refresh(),
                Err(e) => crate::worktrees::alert("Could not add the account", &e.message),
            });
        });
    });
    dlg.present(Some(&app.window));
}

// ---- debug.ui -----------------------------------------------------------------

/// What the page shows (`debug.ui usage`).
fn info() -> Value {
    let shown = shown_now();
    let loading = LOADING.with(|l| l.get());
    let Some(d) = RENDERED.with(|r| r.borrow().clone()) else {
        return json!({"shown": shown, "loading": loading});
    };
    let rows = d.list["accounts"].as_array().cloned().unwrap_or_default();
    let period = period_of(&d);
    let rows_json: Vec<Value> = rows
        .iter()
        .map(|r| {
            let t = tightest(r);
            json!({
                "agent": r["agent"], "name": r["name"], "workspace": r["workspace"],
                "default": r["default"], "plan": r["plan"].as_str().unwrap_or("—"),
                "tightest": match &t { Ok((p, n, _)) => format!("{p}% {n}"), Err(e) => e.clone() },
                "percent": t.as_ref().ok().map(|x| x.0),
                "hot": t.as_ref().is_ok_and(|x| x.2),
                "no_data": t.is_err(),
                "tokens": uv::tokens_text(r["tokens"].as_u64().unwrap_or(0)),
                "tokens_n": r["tokens"],
                "updated": uv::updated_text(r["updated"].as_i64(), d.now),
                "selected": d.selected.as_ref().is_some_and(|k| k.0 == s(&r["agent"]) && k.1 == s(&r["name"]) && k.2 == s(&r["workspace"])),
            })
        })
        .collect();
    let resets: Vec<Value> = d.list["resets"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| json!({"when": r["when"], "account": r["account"], "agent": r["agent"], "what": r["what"], "note": r["note"]}))
        .collect();
    let detail = d.detail.as_ref().map(|det| {
        let kind = AgentKind::parse(s(&det["agent"])).unwrap_or(AgentKind::Claude);
        let groups: Vec<Value> = if s(&det["state"]) == "ok" {
            limit_groups(det, d.now)
                .into_iter()
                .map(|(g, rows)| {
                    json!({"group": g.to_uppercase(), "rows": rows.iter().map(|r| json!({
                        "name": r.0, "value": r.1, "reset": r.2, "hot": r.4,
                    })).collect::<Vec<_>>()})
                })
                .collect()
        } else {
            Vec::new()
        };
        let days: Vec<Value> = d
            .week
            .as_ref()
            .and_then(|w| w["tokens"]["by_day"].as_array().cloned())
            .unwrap_or_default();
        let values: Vec<u64> = days.iter().map(|x| x["tokens"].as_u64().unwrap_or(0)).collect();
        json!({
            "account": det["account"], "agent": det["agent"], "state": det["state"],
            "sub": uv::detail_sub(kind, det["plan"].as_str(), s(&det["config_dir"]), &d.home),
            "groups": groups,
            "chart": {
                "days": days.iter().map(|x| x["day"].clone()).collect::<Vec<_>>(),
                "letters": days.iter().map(|x| uv::day_letter(s(&x["day"]))).collect::<Vec<_>>(),
                "values": values,
                "max": values.iter().max().map(|m| uv::tokens_text(*m)),
                "today": values.last().map(|m| uv::tokens_text(*m)),
            },
            "footer": {
                "projects": det["tokens"]["by_project"].as_array().into_iter().flatten().take(3)
                    .map(|p| format!("{} {}", s(&p["project"]), uv::tokens_text(p["tokens"].as_u64().unwrap_or(0))))
                    .collect::<Vec<_>>(),
                "models": uv::model_split(&det["tokens"]["by_model"].as_array().into_iter().flatten()
                    .map(|m| (s(&m["model"]).to_string(), m["tokens"].as_u64().unwrap_or(0))).collect::<Vec<_>>()),
            },
        })
    });
    json!({
        "shown": shown,
        "loading": loading,
        "period": match period { Period::Today => "today", Period::Week => "7d", Period::Month => "30d" },
        "filter": filter_label(&d),
        "header": {
            "title": "Agent usage",
            "sub": format!("{} account{} · limits from each CLI · tokens from local session logs", rows.len(), if rows.len() == 1 { "" } else { "s" }),
            "buttons": [filter_label(&d), "Add account"],
            "close": "Close (Esc)",
            "period": ["Today", "7 days", "30 days"],
        },
        "columns": ["ACCOUNT", "PLAN", "TIGHTEST LIMIT", uv::tokens_head(period), "UPDATED"],
        "rows": rows_json,
        "resets": resets,
        "detail": detail,
        "layout": {"detail_below": BUILT.with(Cell::get).0, "large_text": BUILT.with(Cell::get).1},
    })
}

/// `debug.ui {"op": "usage", "arg"?}`: `open`, `close`, `click_close`, `close_dialog`, `period
/// today|7d|30d`, `filter <workspace>|all|shown`, `select <agent> <name>
/// <workspace>`, `key up|down`; then what the page shows.
pub fn debug(arg: Option<&str>) -> Result<Value> {
    let parts: Vec<&str> = arg.unwrap_or("").split_whitespace().collect();
    match parts.as_slice() {
        [] => {}
        ["open"] => open(),
        ["close"] => {
            close();
            if let Some(app) = try_app() {
                app.sync();
            }
        }
        ["period", p] => {
            Period::parse(Some(p)).map_err(Error::usage)?;
            let p = p.to_string();
            set_choice(|c| c.period = Some(p));
        }
        ["filter", "all"] => set_choice(|c| c.filter = Some(String::new())),
        ["filter", "shown"] => set_choice(|c| c.filter = None),
        ["filter", w] => {
            let w = w.to_string();
            set_choice(|c| c.filter = Some(w));
        }
        ["select", a, n, w] => {
            let k = (a.to_string(), n.to_string(), w.to_string());
            set_choice(|c| c.selected = Some(k));
        }
        // Close the dialog on screen (Add account), as its Cancel does.
        ["close_dialog"] => {
            if let Some(app) = try_app() {
                if let Some(d) = adw::prelude::AdwApplicationWindowExt::visible_dialog(&app.window)
                {
                    adw::prelude::AdwDialogExt::close(&d);
                }
            }
        }
        // The header's close button, clicked (M51).
        ["click_close"] => {
            let root = ROOT
                .with(|r| r.borrow().clone())
                .ok_or_else(|| Error::usage("no Usage page"))?;
            let mut stack: Vec<gtk::Widget> = vec![root.upcast()];
            let mut hit = None;
            while let Some(w) = stack.pop() {
                if w.has_css_class("usage-close") {
                    hit = w.downcast::<gtk::Button>().ok();
                    break;
                }
                let mut c = w.first_child();
                while let Some(x) = c {
                    c = x.next_sibling();
                    stack.push(x);
                }
            }
            hit.ok_or_else(|| Error::usage("the Usage page is not shown"))?
                .emit_clicked();
        }
        ["key", "down"] => step(true),
        ["key", "up"] => step(false),
        _ => {
            return Err(Error::usage(format!(
                "unknown usage arg '{}'",
                arg.unwrap_or("")
            )))
        }
    }
    Ok(info())
}
