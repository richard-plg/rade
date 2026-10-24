//! Project home (PRD R105, spec b-project-home.md): the page a Project row
//! (or `Alt+1..9`) opens in the center. Header (cube, name, sub-line,
//! Settings, Open terminal = the one primary button), 3 KPIs, the link
//! table (base, checkout, HEAD, vs base, state chip, Open terminal and the
//! drift actions), the Worktrees card and Recent activity. Shown until the
//! focus moves to a checkout; the breadcrumb reads `Shop / Home` (M43: was `Overview`, now Agent Overview's word).

use crate::app::{app, try_app};
use adw::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::drift::Action;
use rade_core::home::{self, Kpi, WorktreeRow};
use rade_core::linkview::LinkView;
use serde::Serialize;
use serde_json::{json, Value};
use std::cell::RefCell;

/// The page's name in the center stack.
pub const PAGE: &str = "project-home";
/// Recent activity rows (spec B content: the 3 newest).
const ACTIVITY_ROWS: usize = 3;

/// One link table row as shown (`debug.ui home` → `links`).
#[derive(Clone, Debug, PartialEq, Serialize)]
struct LinkRow {
    source: String,
    name: String,
    base: String,
    checkout: String,
    path: Option<String>,
    head: String,
    vs_base: String,
    chip: String,
    tone: &'static str,
    drift: bool,
    dirty: usize,
    /// The drift actions (label, action) of the row menu.
    actions: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct ActRow {
    kind: String,
    actor: String,
    text: String,
    ago: String,
    pane: Option<String>,
}

/// Everything the page shows; rebuilt when it changes.
#[derive(Clone, Debug, PartialEq, Serialize)]
struct View {
    project: String,
    name: String,
    color: usize,
    sub: String,
    kpis: Vec<Kpi>,
    links: Vec<LinkRow>,
    worktrees: Vec<WorktreeRow>,
    activity: Vec<ActRow>,
    buttons: Vec<String>,
}

/// (source, worktree, tab) in focus.
type Focus = (String, String, Option<String>);

thread_local! {
    /// The Project shown and the (source, worktree, tab) focused when it
    /// opened: moving the focus elsewhere closes the page.
    static SHOWN: RefCell<Option<(String, Focus)>> = const { RefCell::new(None) };
    static ROOT: RefCell<Option<gtk::ScrolledWindow>> = const { RefCell::new(None) };
    static RENDERED: RefCell<Option<View>> = const { RefCell::new(None) };
    /// The header's `Open terminal` (the page's one primary button).
    static PRIMARY: RefCell<Option<gtk::Button>> = const { RefCell::new(None) };
    /// The KPI row and whether the page was built for large text (R111).
    static KPIS: RefCell<Option<gtk::Box>> = const { RefCell::new(None) };
    static BUILT_LARGE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The page's padding left and right (spec B.4: 28 px each side).
const PAGE_PAD: i32 = 56;

/// The window's width or text scale changed (R111): rebuild for large text
/// or back, and wrap the KPI cards when one would be narrower than its
/// value and detail.
pub fn relayout() {
    let large = crate::layout_mode::mode().large_text;
    if BUILT_LARGE.with(|b| b.get()) != large {
        RENDERED.with(|r| r.borrow_mut().take());
        refresh();
    }
    fit_kpis();
}

fn fit_kpis() {
    let Some(kpis) = KPIS.with(|k| k.borrow().clone()) else {
        return;
    };
    let mut need = 0;
    let mut n = 0;
    let mut c = kpis.first_child();
    while let Some(card) = c {
        c = card.next_sibling();
        n += 1;
        need = need.max(card.measure(gtk::Orientation::Horizontal, -1).1);
    }
    // Only large text wraps (B "Text scale 200 %"); at 100 % the cards
    // shrink equally (B "1280 px").
    let content = crate::layout_mode::center_width() - PAGE_PAD;
    let one = crate::layout_mode::mode().large_text
        && rade_core::chrome::kpis_one_per_line(content, n, need);
    let want = if one {
        gtk::Orientation::Vertical
    } else {
        gtk::Orientation::Horizontal
    };
    if kpis.orientation() != want {
        kpis.set_orientation(want);
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
    sc.set_hscrollbar_policy(gtk::PolicyType::Never);
    sc.add_css_class("project-home-scroll");
    ROOT.with(|r| *r.borrow_mut() = Some(sc.clone()));
    // Ages and KPIs stay fresh.
    glib::timeout_add_seconds_local(20, || {
        refresh();
        glib::ControlFlow::Continue
    });
    sc
}

/// The Project whose home is shown, if any (closes it when the focus
/// moved since it opened).
pub fn shown(m: &rade_core::State) -> Option<String> {
    let now = focus_key(m);
    SHOWN.with(|s| {
        let mut s = s.borrow_mut();
        match &*s {
            Some((p, at)) if *at == now => Some(p.clone()),
            Some(_) => {
                *s = None;
                None
            }
            None => None,
        }
    })
}

/// Show Project `project`'s home (a Project row, `Alt+n`).
pub fn open(project: &str) {
    let Some(app) = try_app() else { return };
    crate::usage_view::close();
    let at = focus_key(&app.model.borrow());
    SHOWN.with(|s| *s.borrow_mut() = Some((project.to_string(), at)));
    RENDERED.with(|r| r.borrow_mut().take());
    app.sync();
    crate::navdata::refresh();
}

/// Leave the page (a checkout row was clicked).
pub fn close() {
    SHOWN.with(|s| s.borrow_mut().take());
}

/// Focus a Pane wherever it is (an Activity row).
pub fn focus_pane(pane: &str) {
    let Some(app) = try_app() else { return };
    close();
    let source = app.model.borrow().locate(pane).map(|l| l.source);
    let _ = app.update(|m| {
        if let Some(p) = &source {
            m.select_source(p)?;
        }
        m.focus_pane(pane)
    });
}

fn view(project: &str) -> Option<View> {
    let app = try_app()?;
    let m = app.model.borrow();
    let projects = crate::navdata::projects(&m);
    let (i, p) = projects.iter().enumerate().find(|(_, p)| p.id == project)?;
    let links: Vec<LinkView> = crate::navdata::links(&m)
        .into_iter()
        .filter(|l| l.project == project)
        .collect();
    let ws = m.workspace(m.active_workspace())?;
    let root = ws.root.clone();
    let canon = crate::projects::canon;
    let facts = |path: &std::path::Path| crate::navdata::worktree_facts(&m, path);
    let kpis = home::kpis(&m, project, &links, &|p| facts(p).1, &canon);
    let rows = links
        .iter()
        .map(|l| {
            let chip = l.chip();
            let (checkout, path) = l.checkout_cell(&root);
            let actions = l
                .drift
                .as_ref()
                .map(|d| {
                    home::drift_card(&l.source, &l.project_name, d)
                        .buttons
                        .into_iter()
                        .map(|(a, label)| (label, action_id(a).to_string()))
                        .collect()
                })
                .unwrap_or_default();
            LinkRow {
                source: l.source.clone(),
                name: m
                    .source(&l.source)
                    .map_or(l.source.clone(), |s| s.name.clone()),
                base: l.base.clone(),
                checkout,
                path,
                head: l.head.clone().unwrap_or_else(|| "—".into()),
                vs_base: l.vs_base(),
                chip: chip.text,
                tone: chip.tone.class(),
                drift: l.drift.is_some(),
                dirty: l.dirty,
                actions,
            }
        })
        .collect();
    let now = crate::panes::now();
    let activity = crate::activity_view::of_project(project, ACTIVITY_ROWS)
        .into_iter()
        .map(|e| ActRow {
            kind: format!("{:?}", e.kind).to_lowercase(),
            actor: e.actor.clone(),
            text: e.text.clone(),
            ago: rade_core::activity::ago(now.saturating_sub(e.at)),
            pane: e.pane.clone(),
        })
        .collect();
    Some(View {
        project: project.to_string(),
        name: p.name.clone(),
        color: i,
        sub: home::sub_line(&ws.name, p, links.len(), &root),
        kpis,
        links: rows,
        worktrees: home::worktree_rows(&m, project, &links, &facts),
        activity,
        buttons: vec!["Settings".into(), "Open terminal".into()],
    })
}

fn action_id(a: Action) -> &'static str {
    match a {
        Action::BackToBase => "back_to_base",
        Action::Keep => "keep",
        Action::MoveToWorktree => "move_to_worktree",
    }
}

/// A dialog or the palette opened / closed: Open terminal keeps the
/// primary class only while neither is open (one `.suggested-action`).
pub fn sync_primary() {
    let modal = crate::project_dialogs::is_open() || crate::palette::is_open();
    PRIMARY.with(|p| {
        if let Some(b) = p.borrow().as_ref() {
            if modal {
                b.remove_css_class("suggested-action");
            } else {
                b.add_css_class("suggested-action");
            }
        }
    });
}

/// Re-render the shown page when what it shows changed.
pub fn refresh() {
    let Some(app) = try_app() else { return };
    let Some(project) = shown(&app.model.borrow()) else {
        return;
    };
    let Some(v) = view(&project) else { return };
    if RENDERED.with(|r| r.borrow().as_ref() == Some(&v)) {
        return;
    }
    if let Some(root) = ROOT.with(|r| r.borrow().clone()) {
        root.set_child(Some(&build(&v)));
    }
    RENDERED.with(|r| *r.borrow_mut() = Some(v));
    fit_kpis();
}

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

/// A secondary button with an icon (spec B.12).
fn secondary(icon_name: &str, text: &str, class: &str) -> gtk::Button {
    let b = gtk::Button::new();
    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    inner.append(&icon(icon_name, &["btn-icon"]));
    inner.append(&label(text, &[]));
    b.set_child(Some(&inner));
    b.add_css_class("secondary-button");
    b.add_css_class(class);
    b
}

fn section(text: &str, extra: Option<&gtk::Widget>) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    b.add_css_class("home-section");
    let l = label(text, &["section-label"]);
    l.set_hexpand(true);
    b.append(&l);
    if let Some(w) = extra {
        b.append(w);
    }
    b
}

fn build(v: &View) -> gtk::Widget {
    let large = crate::layout_mode::mode().large_text;
    BUILT_LARGE.with(|b| b.set(large));
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.add_css_class("project-home");
    let key = format!("{}/{}", app().model.borrow().active_workspace(), v.project);

    // Header (B.6–B.12).
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    header.add_css_class("home-header");
    header.append(&icon(
        "ph-cube-symbolic",
        &["home-icon", &format!("proj-{}", v.color % 3 + 1)],
    ));
    let titles = gtk::Box::new(gtk::Orientation::Vertical, 2);
    titles.append(&label(&v.name, &["home-title"]));
    titles.append(&label(&v.sub, &["home-sub"]));
    titles.set_hexpand(true);
    titles.set_valign(gtk::Align::Center);
    header.append(&titles);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_valign(gtk::Align::Center);
    let settings = secondary("ph-gear-six-symbolic", "Settings", "home-settings");
    let k = key.clone();
    settings.connect_clicked(move |_| crate::project_dialogs::settings(&k));
    actions.append(&settings);
    let open = gtk::Button::new();
    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    inner.append(&icon("ph-terminal-window-symbolic", &["btn-icon"]));
    inner.append(&label("Open terminal", &[]));
    open.set_child(Some(&inner));
    if !crate::project_dialogs::is_open() && !crate::palette::is_open() {
        open.add_css_class("suggested-action");
    }
    open.add_css_class("home-open-terminal");
    PRIMARY.with(|p| *p.borrow_mut() = Some(open.clone()));
    match v.links.first() {
        Some(first) => {
            let (p, s) = (v.project.clone(), first.source.clone());
            open.connect_clicked(move |_| open_terminal(&p, &s));
        }
        None => open.set_sensitive(false),
    }
    actions.append(&open);
    header.append(&actions);
    page.append(&header);

    // KPIs (B.13–B.17).
    let kpis = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    kpis.set_homogeneous(true);
    kpis.add_css_class("home-kpis");
    for k in &v.kpis {
        let c = gtk::Box::new(gtk::Orientation::Vertical, 2);
        c.add_css_class("kpi");
        c.append(&label(
            &k.label.to_uppercase(),
            &["section-label", "kpi-label"],
        ));
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&label(&k.value, &["kpi-value"]));
        let d = label(&k.detail, &["kpi-detail"]);
        d.set_valign(gtk::Align::Baseline);
        d.set_ellipsize(gtk::pango::EllipsizeMode::End);
        row.append(&d);
        c.append(&row);
        kpis.append(&c);
    }
    KPIS.with(|k| *k.borrow_mut() = Some(kpis.clone()));
    page.append(&kpis);

    // Link table (B.18–B.30).
    let add = secondary("ph-plus-symbolic", "Add source", "home-add-source");
    let k = key.clone();
    add.connect_clicked(move |_| crate::project_dialogs::add_source(&k, None));
    page.append(&section("SOURCES", Some(add.upcast_ref())));
    if v.links.is_empty() {
        page.append(&label(
            &format!("No sources in {}.", v.name),
            &["home-empty"],
        ));
    } else {
        page.append(&link_table(v, large));
    }

    // Lower grid (B.31–B.36).
    // Two columns 1.25 fr : 1 fr with a 22 px gap (B.31): 9 equal grid
    // columns, 5 + 4, each side giving 11 px of the gap.
    let lower = gtk::Grid::new();
    lower.set_column_homogeneous(true);
    lower.add_css_class("home-lower");
    let left = gtk::Box::new(gtk::Orientation::Vertical, 0);
    left.add_css_class("home-worktrees");
    left.set_hexpand(true);
    left.append(&section("WORKTREES", None));
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("home-card");
    if v.worktrees.is_empty() {
        let r = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        r.add_css_class("home-card-row");
        r.append(&label("No Worktrees ·", &["home-empty"]));
        let b = gtk::Button::with_label("New Worktree…");
        b.add_css_class("flat");
        b.add_css_class("home-link-button");
        let k = key.clone();
        b.connect_clicked(move |_| crate::projects::new_worktree_dialog(&k));
        r.append(&b);
        card.append(&r);
    }
    for w in &v.worktrees {
        card.append(&worktree_row(w));
    }
    left.append(&card);
    left.set_margin_end(11);
    lower.attach(&left, 0, 0, 5, 1);
    let right = gtk::Box::new(gtk::Orientation::Vertical, 0);
    right.add_css_class("home-activity");
    right.set_hexpand(true);
    right.append(&section("RECENT ACTIVITY", None));
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("home-card");
    if v.activity.is_empty() {
        let r = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        r.add_css_class("home-card-row");
        r.append(&label("No activity since RADE started", &["home-empty"]));
        card.append(&r);
    } else {
        let now = crate::panes::now();
        for e in crate::activity_view::of_project(&v.project, ACTIVITY_ROWS) {
            card.append(&crate::activity_view::row(&e, now, "home-card-row"));
        }
    }
    right.append(&card);
    if large {
        // One column at large text (B "Text scale 200 %").
        left.set_margin_end(0);
        lower.attach(&right, 0, 1, 9, 1);
    } else {
        right.set_margin_start(11);
        lower.attach(&right, 5, 0, 4, 1);
    }
    page.append(&lower);
    page.upcast()
}

const COLUMNS: [&str; 6] = ["SOURCE", "BASE", "CHECKOUT", "HEAD", "VS BASE", "STATE"];

/// Columns that move to a row's second line at large text (B "Text scale
/// 200 %": CHECKOUT and VS BASE).
fn second_line_column(i: usize) -> bool {
    i == 2 || i == 4
}

fn link_table(v: &View, large: bool) -> gtk::Widget {
    let table = gtk::Box::new(gtk::Orientation::Vertical, 0);
    table.add_css_class("link-table");
    let groups: Vec<gtk::SizeGroup> = (0..COLUMNS.len() + 1)
        .map(|_| gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal))
        .collect();
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    head.add_css_class("link-table-head");
    for (i, c) in COLUMNS.iter().enumerate() {
        if large && second_line_column(i) {
            continue;
        }
        let l = label(c, &["link-table-col"]);
        if i == 2 || (large && i == 0) {
            l.set_hexpand(true);
        }
        groups[i].add_widget(&l);
        head.append(&l);
    }
    let pad = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    groups[COLUMNS.len()].add_widget(&pad);
    head.append(&pad);
    table.append(&head);
    for r in &v.links {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        row.add_css_class("link-row");
        let src = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        src.add_css_class("link-cell");
        src.append(&icon("ph-git-fork-symbolic", &["link-row-icon"]));
        src.append(&label(&r.name, &["link-cell-source"]));
        groups[0].add_widget(&src);
        if large {
            src.set_hexpand(true);
        }
        row.append(&src);
        let base = label(&r.base, &["link-cell", "link-cell-mono", "link-cell-base"]);
        groups[1].add_widget(&base);
        row.append(&base);
        let co = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        co.add_css_class("link-cell");
        co.add_css_class("link-cell-checkout");
        co.set_hexpand(true);
        co.append(&label(&r.checkout, &["link-cell-kind"]));
        if let Some(p) = &r.path {
            let pl = label(p, &["link-cell-mono", "link-cell-path"]);
            pl.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            pl.set_hexpand(true);
            co.append(&pl);
        }
        if !large {
            groups[2].add_widget(&co);
            row.append(&co);
        }
        let head = label(&r.head, &["link-cell", "link-cell-mono", "link-cell-head"]);
        if r.drift {
            head.add_css_class("drift");
        }
        groups[3].add_widget(&head);
        row.append(&head);
        let vs = label(&r.vs_base, &["link-cell", "link-cell-mono", "link-cell-vs"]);
        if !large {
            groups[4].add_widget(&vs);
            row.append(&vs);
        }
        let chip_cell = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        chip_cell.add_css_class("link-cell");
        chip_cell.append(&chip(r));
        groups[5].add_widget(&chip_cell);
        row.append(&chip_cell);
        let acts = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        acts.add_css_class("link-row-actions");
        let term = gtk::Button::from_icon_name("ph-terminal-window-symbolic");
        term.add_css_class("flat");
        term.add_css_class("link-row-action");
        term.set_widget_name(&format!("home-terminal-{}", r.source));
        term.set_tooltip_text(Some(&format!("Open terminal on {}", r.name)));
        let (p, s) = (v.project.clone(), r.source.clone());
        term.connect_clicked(move |_| open_terminal(&p, &s));
        acts.append(&term);
        acts.append(&row_menu(v, r));
        groups[6].add_widget(&acts);
        row.append(&acts);
        if large {
            // CHECKOUT and VS BASE on a second line (mono, fg3), wrapping.
            row.remove_css_class("link-row");
            outer.add_css_class("link-row");
            outer.add_css_class("link-row-two-line");
            outer.append(&row);
            let mut second = r.checkout.clone();
            if let Some(p) = &r.path {
                second.push_str(&format!("  {p}"));
            }
            second.push_str(&format!("  vs base {}", r.vs_base));
            let l = label(&second, &["link-cell-mono", "link-row-second"]);
            l.set_wrap(true);
            l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            outer.append(&l);
            table.append(&outer);
        } else {
            table.append(&row);
        }
    }
    table.upcast()
}

fn chip(r: &LinkRow) -> gtk::Widget {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    b.add_css_class("chip");
    b.add_css_class(r.tone);
    let name = match r.tone {
        "chip-warn" => "ph-warning-diamond-fill-symbolic",
        "chip-info" if r.chip.starts_with("also in") => "ph-link-simple-symbolic",
        "chip-info" => "ph-info-symbolic",
        _ => "ph-check-symbolic",
    };
    b.append(&icon(name, &["chip-icon"]));
    b.append(&label(&r.chip, &["chip-label"]));
    b.set_valign(gtk::Align::Center);
    b.set_halign(gtk::Align::Start);
    b.upcast()
}

/// The `dots-three` menu: Open terminal and the drift actions (R100.2).
fn row_menu(v: &View, r: &LinkRow) -> gtk::MenuButton {
    let menu = gio::Menu::new();
    let target = format!("{}\u{1f}{}", v.project, r.source);
    let item = |label: &str, action: &str| {
        let i = gio::MenuItem::new(Some(label), None);
        i.set_action_and_target_value(Some(&format!("home.{action}")), Some(&target.to_variant()));
        menu.append_item(&i);
    };
    item("Open terminal", "open_terminal");
    item("Change base…", "change_base");
    for (label, action) in &r.actions {
        item(label, action);
    }
    let b = gtk::MenuButton::new();
    b.set_icon_name("ph-dots-three-symbolic");
    b.add_css_class("flat");
    b.add_css_class("link-row-action");
    b.set_widget_name(&format!("home-menu-{}", r.source));
    b.set_menu_model(Some(&menu));
    b.insert_action_group("home", Some(&actions()));
    b
}

fn actions() -> gio::SimpleActionGroup {
    let group = gio::SimpleActionGroup::new();
    let act = |name: &str, f: fn(&str, &str)| {
        let a = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
        a.connect_activate(move |_, v| {
            let v: String = v.and_then(|v| v.get()).unwrap_or_default();
            let (p, s) = v.split_once('\u{1f}').unwrap_or((&v, ""));
            f(p, s);
        });
        group.add_action(&a);
    };
    act("open_terminal", open_terminal);
    act("change_base", |p, s| {
        let ws = app().model.borrow().active_workspace().to_string();
        crate::project_dialogs::change_base(&format!("{ws}/{p}"), s);
    });
    act("back_to_base", back_to_base);
    act("keep", keep);
    act("move_to_worktree", move_to_worktree);
    group
}

fn worktree_row(w: &WorktreeRow) -> gtk::Widget {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    b.add_css_class("home-card-row");
    b.append(&icon("ph-git-branch-symbolic", &["home-wt-icon"]));
    b.append(&label(&w.name, &["home-wt-name"]));
    let sub = label(&w.sub, &["home-wt-sub"]);
    sub.set_hexpand(true);
    b.append(&sub);
    if let Some(s) = w.led {
        let led = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        led.add_css_class("led");
        led.add_css_class(&format!("led-{}", s.as_str()));
        led.set_valign(gtk::Align::Center);
        b.append(&led);
    }
    b.append(&label(&w.meta, &["home-wt-meta"]));
    let (s, wt) = (w.source.clone(), w.worktree.clone());
    let click = gtk::GestureClick::new();
    click.connect_released(move |_, _, _, _| select_worktree(&s, &wt));
    b.add_controller(click);
    b.upcast()
}

/// A Worktree row: the Worktree with its last Tab (R105-f).
pub fn select_worktree(source: &str, worktree: &str) {
    close();
    crate::worktrees::activate(source, worktree);
}

/// Run `f` off the GTK thread, then refresh the link data; an error is an
/// alert titled `title`.
fn off_thread(
    title: &'static str,
    f: impl FnOnce() -> rade_core::rpc::Result<Value> + Send + 'static,
) {
    crate::worktrees::in_background(f, move |r| {
        if let Err(e) = r {
            crate::worktrees::alert(title, &e.message);
        }
        crate::navdata::refresh();
    });
}

/// Open terminal on a link (R105.3): its checkout, created first when the
/// plan says so (`open_on_link`, R98.3).
pub fn open_terminal(project: &str, source: &str) {
    close();
    let (p, s) = (project.to_string(), source.to_string());
    off_thread("Could not open a terminal", move || {
        crate::worktrees::open_on_link(&p, &s)
    });
}

pub(crate) fn back_to_base(project: &str, source: &str) {
    let (p, s) = (project.to_string(), source.to_string());
    off_thread("Back to base", move || {
        crate::projects::dispatch("project.link_to_base", json!({"project": p, "source": s}))
    });
}

pub(crate) fn keep(project: &str, source: &str) {
    let head = try_app().and_then(|app| {
        crate::navdata::links(&app.model.borrow())
            .into_iter()
            .find(|l| l.project == project && l.source == source)
            .and_then(|l| l.drift.map(|d| d.head))
    });
    if let Some(h) = head {
        crate::navdata::keep(project, source, &h);
    }
}

/// Move to a Worktree… (R100.2): one confirmation naming both steps, then
/// Back to base and a Worktree of the drifted branch on the link; when
/// dirty the dialog says to commit first and runs nothing.
pub fn move_to_worktree(project: &str, source: &str) {
    let Some(app) = try_app() else { return };
    let link = crate::navdata::links(&app.model.borrow())
        .into_iter()
        .find(|l| l.project == project && l.source == source);
    let Some((l, d)) = link.and_then(|l| l.drift.clone().map(|d| (l, d))) else {
        return;
    };
    // Not offered on a detached HEAD (`drift()`): there is no branch to move.
    if !d.actions.contains(&Action::MoveToWorktree) {
        return;
    }
    let name = app
        .model
        .borrow()
        .source(source)
        .map_or(source.to_string(), |s| s.name.clone());
    if d.dirty > 0 {
        let dlg = adw::AlertDialog::new(
            Some("Commit first"),
            Some(&format!(
                "{name} has {}. Commit them on {}, then move it to a Worktree.",
                rade_core::linkview::changes_short(d.dirty)
                    .replace("changes", "uncommitted changes"),
                d.head
            )),
        );
        dlg.add_response("ok", "OK");
        crate::menus::present(dlg, &["ok"]);
        return;
    }
    let dlg = adw::AlertDialog::new(
        Some(&format!("Move {} to a Worktree?", d.head)),
        Some(&format!(
            "RADE switches {name}'s checkout back to {}, then adds a Worktree of branch {} under {}.",
            l.base, d.head, l.project_name
        )),
    );
    dlg.add_response("cancel", "Cancel");
    dlg.add_response("move", "Move");
    dlg.set_response_appearance("move", adw::ResponseAppearance::Suggested);
    dlg.set_default_response(Some("move"));
    dlg.set_close_response("cancel");
    let (p, s, branch) = (project.to_string(), source.to_string(), d.head.clone());
    dlg.connect_response(None, move |_, r| {
        if r != "move" {
            return;
        }
        let (p, s, branch) = (p.clone(), s.clone(), branch.clone());
        off_thread("Move to a Worktree", move || {
            crate::projects::dispatch("project.link_to_base", json!({"project": p, "source": s}))?;
            crate::worktrees::dispatch(
                "worktree.add",
                json!({"source": s, "name": branch, "project": p}),
            )
        });
    });
    crate::menus::present(dlg, &["cancel", "move"]);
}

/// `debug.ui home` (`arg`: `open <project>`, `worktree <name>`,
/// `terminal <source>`, `action <source> <keep|back_to_base|move_to_worktree>`):
/// the page as shown.
pub fn debug(arg: Option<&str>) -> rade_core::rpc::Result<Value> {
    let app = app();
    if let Some(a) = arg.filter(|a| !a.is_empty()) {
        let (op, rest) = a.split_once(' ').unwrap_or((a, ""));
        let project = shown(&app.model.borrow());
        match op {
            "open" => {
                let m = app.model.borrow();
                let id = crate::navdata::projects(&m)
                    .into_iter()
                    .find(|p| p.id == rest || p.name == rest)
                    .map(|p| p.id)
                    .ok_or_else(|| rade_core::Error::not_found(format!("no project '{rest}'")))?;
                drop(m);
                open(&id);
            }
            "worktree" => {
                let v = RENDERED.with(|r| r.borrow().clone());
                let w = v
                    .iter()
                    .flat_map(|v| v.worktrees.iter())
                    .find(|w| w.name == rest)
                    .cloned()
                    .ok_or_else(|| {
                        rade_core::Error::not_found(format!("no Worktree row '{rest}'"))
                    })?;
                select_worktree(&w.source, &w.worktree);
            }
            "terminal" | "action" => {
                let p = project.ok_or_else(|| rade_core::Error::usage("no Project home shown"))?;
                let (src, act) = rest.split_once(' ').unwrap_or((rest, ""));
                match (op, act) {
                    ("terminal", _) => open_terminal(&p, src),
                    (_, "keep") => keep(&p, src),
                    (_, "back_to_base") => back_to_base(&p, src),
                    (_, "move_to_worktree") => move_to_worktree(&p, src),
                    _ => return Err(rade_core::Error::usage(format!("unknown action '{act}'"))),
                }
            }
            _ => return Err(rade_core::Error::usage(format!("unknown home op '{op}'"))),
        }
    }
    refresh();
    let v = RENDERED.with(|r| r.borrow().clone());
    let shown = shown(&app.model.borrow());
    Ok(match (shown, v) {
        (Some(p), Some(v)) if v.project == p => {
            let mut j = serde_json::to_value(&v).unwrap_or(Value::Null);
            j["shown"] = json!(true);
            if v.activity.is_empty() {
                j["activity_empty"] = json!("No activity since RADE started");
            }
            j["columns"] = json!(COLUMNS);
            j
        }
        _ => json!({"shown": false}),
    })
}
