//! The Navigator (PRD R104, ADR-0034 decision 8, spec A.17–A.38): the left
//! column. Header `PROJECTS` with filter / add buttons; the tree of
//! `rade_core::navigator::build` — Projects → Source links (name + base)
//! → Worktrees → Agents, then Scratch; the footer with the Inbox pill
//! (`<agent> needs input`, `Ctrl ⇧ G`) and the bell, both opening the
//! Inbox popover (R104.3). Rows are rebuilt only when the pure row list
//! changes; the links' git facts come from `navdata` (off the UI thread).

use crate::agent_glyph::{icon_of, Glyph};
use crate::app::try_app;
use crate::notifications::Panel;
use adw::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::design::{AgentMark, Branches};
use rade_core::navigator::{self, Row, RowKind, Target};
use rade_core::notify::UnreadCounts;
use rade_core::project::Project;
use rade_core::{git, AgentStatus, State};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

pub struct Sidebar {
    pub root: gtk::Widget,
    list: gtk::ListBox,
    rows: Rc<RefCell<Vec<Row>>>,
    pub panel: Panel,
    /// The Workspace switcher (its button sits in the titlebar, R103.1).
    pub switcher: crate::switcher::Switcher,
    pill: gtk::Button,
    pill_name: gtk::Label,
    pill_glyph: Glyph,
    popover: gtk::Popover,
    footer: gtk::Box,
    /// Holds the bell at the right while the pill is hidden.
    spacer: gtk::Box,
    /// The 48 px icon rail of large text (R111.2): Project cubes, Scratch,
    /// bell; each opens the full Navigator over the center.
    pub rail: gtk::Box,
    rail_items: gtk::Box,
    /// What the rail shows: (title, color) per Project.
    rail_shown: RefCell<Vec<(String, usize)>>,
}

thread_local! {
    /// The shown Workspace's Projects: (root, file signature, Projects).
    static PROJECTS: RefCell<Option<ProjectCache>> = const { RefCell::new(None) };
}

type ProjectCache = (PathBuf, rade_core::project::Signature, Rc<Vec<Project>>);

/// The shown Workspace's Projects (by id), re-read when a `project.toml`
/// changed (size or mtime) or another Workspace is shown.
pub fn projects(ws: &State) -> Rc<Vec<Project>> {
    let Some(root) = ws.workspace(ws.active_workspace()).map(|w| w.root.clone()) else {
        return Rc::default();
    };
    let sig = rade_core::project::signature(&root);
    PROJECTS.with(|c| {
        let mut c = c.borrow_mut();
        match &*c {
            Some((r, s, p)) if *r == root && *s == sig => p.clone(),
            _ => {
                let p = Rc::new(rade_core::project::list(&root));
                *c = Some((root, sig, p.clone()));
                p
            }
        }
    })
}

/// Sources in the old sidebar order (`Alt+<n>` with no Projects, R30.2).
pub fn source_order(ws: &State) -> Vec<String> {
    ws.sidebar_order()
}

thread_local! {
    /// A Source path's main branch (`git::default_branch`), read once.
    // ponytail: cached for the app's lifetime; a changed origin/HEAD needs a restart.
    static MAIN_BRANCH: RefCell<HashMap<PathBuf, Option<String>>> = RefCell::default();
}

/// Current branches of the shown Workspace's Worktrees and, for Projects
/// on a Source's main branch, that branch: the Scratch suggestions.
pub fn branches(ws: &State, projects: &[Project]) -> Branches {
    let mut br = Branches::default();
    if projects.is_empty() {
        return br;
    }
    for s in ws.visible_sources() {
        for w in &s.worktrees {
            if let Some(b) = git::branch(&w.path) {
                br.current.insert(w.id.clone(), b);
            }
        }
        let on_main = projects
            .iter()
            .any(|p| p.source(&s.id).is_some_and(|x| x.base.is_none()));
        if let (true, Some(w)) = (on_main, s.worktrees.first()) {
            let main = MAIN_BRANCH.with(|c| {
                c.borrow_mut()
                    .entry(w.path.clone())
                    .or_insert_with(|| git::default_branch(&w.path))
                    .clone()
            });
            if let Some(m) = main {
                br.main.insert(s.id.clone(), m);
            }
        }
    }
    br
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

fn led(s: Option<AgentStatus>) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.add_css_class("led");
    b.add_css_class(&format!("led-{}", s.map_or("idle", |s| s.as_str())));
    b.set_valign(gtk::Align::Center);
    b.set_halign(gtk::Align::Center);
    b
}

/// The suggestion button's text (`→ Shop?`).
fn suggest_text(r: &Row) -> Option<String> {
    r.suggest.as_ref().map(|(_, n)| format!("\u{2192} {n}?"))
}

/// A row's tooltip: the also-in list, the drift, an Agent's status.
fn tooltip(r: &Row) -> Option<String> {
    match r.kind {
        RowKind::Project if r.also_in.is_some() => r.also_in.clone(),
        RowKind::Project => {
            Some("Click: Project home · right-click: Project settings…, New Worktree…".into())
        }
        RowKind::Link => {
            let mut lines = Vec::new();
            if r.drift {
                lines.push(format!("On {}, not on its base", r.meta));
            }
            if let Some(a) = &r.also_in {
                lines.push(a.clone());
            }
            (!lines.is_empty()).then(|| lines.join("\n"))
        }
        RowKind::Agent => try_app().and_then(|app| {
            let m = app.model.borrow();
            m.pane(&r.key).map(crate::status::tooltip)
        }),
        _ => None,
    }
}

fn build_row(r: &Row) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    if r.kind == RowKind::Section {
        // The separator and the `SCRATCH` label (A.35, A.36).
        let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let sep = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        sep.add_css_class("nav-sep");
        b.append(&sep);
        let h = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        h.add_css_class("nav-header");
        h.add_css_class("nav-header-scratch");
        h.append(&label(&r.title.to_uppercase(), &["section-label"]));
        b.append(&h);
        row.set_child(Some(&b));
        row.set_activatable(false);
        row.set_selectable(false);
        row.add_css_class("nav-section");
        return row;
    }
    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    // Chevron column (12 px, A.26).
    let chev = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    chev.add_css_class("nav-chevron");
    if let Some(c) = r.collapsed {
        let b = gtk::Button::from_icon_name(if c {
            "ph-caret-right-symbolic"
        } else {
            "ph-caret-down-symbolic"
        });
        b.add_css_class("flat");
        b.add_css_class("nav-chevron-button");
        b.set_focus_on_click(false);
        b.set_tooltip_text(Some(if c { "Expand" } else { "Collapse" }));
        let key = r.key.clone();
        b.connect_clicked(move |_| toggle_key(&key, c));
        chev.append(&b);
    }
    hbox.append(&chev);
    match r.kind {
        RowKind::Project => {
            let i = icon("ph-cube-symbolic", &["nav-row-icon"]);
            i.add_css_class(&format!("proj-{}", r.color.unwrap_or(0) % 3 + 1));
            hbox.append(&i);
        }
        RowKind::Link => hbox.append(&icon("ph-git-fork-symbolic", &["nav-row-icon"])),
        RowKind::Worktree => {
            hbox.append(&icon("ph-git-branch-symbolic", &["nav-row-icon", "nav-wt"]))
        }
        // The agent's glyph in its status colour (R115).
        RowKind::Agent => match r.mark {
            Some(m) => {
                let g = Glyph::new(m, r.led);
                g.set(m, icon_of(r.preset.as_deref()).as_deref(), r.led);
                hbox.append(&g.root);
            }
            None => hbox.append(&led(r.led)),
        },
        RowKind::Scratch => hbox.append(&icon("ph-terminal-window-symbolic", &["nav-row-icon"])),
        _ => {}
    }
    let title = label(
        &r.title,
        if r.kind == RowKind::Empty {
            &["nav-row-title", "nav-empty"]
        } else {
            &["nav-row-title"]
        },
    );
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_hexpand(true);
    hbox.append(&title);
    if let (Some(text), Some((pid, _))) = (suggest_text(r), &r.suggest) {
        // A one-click suggestion, never applied by RADE (R104.1).
        let b = gtk::Button::with_label(&text);
        b.add_css_class("flat");
        b.add_css_class("nav-suggest");
        b.set_valign(gtk::Align::Center);
        if let Target::Worktree { source, worktree } = &r.target {
            let (s, w, p) = (source.clone(), worktree.clone(), pid.clone());
            b.connect_clicked(move |_| move_to_project(&s, &w, Some(p.clone())));
        }
        hbox.append(&b);
    }
    let meta = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    meta.add_css_class("nav-row-meta-box");
    if !r.meta.is_empty() {
        let m = label(&r.meta, &["nav-row-meta"]);
        if r.drift || matches!(r.led, Some(AgentStatus::Waiting)) && r.kind == RowKind::Agent {
            m.add_css_class("warn");
        }
        meta.append(&m);
    }
    if let Some(t) = &r.review {
        let c = label(t, &["nav-review"]);
        if t == "approved" {
            c.add_css_class("approved");
        } else if t.ends_with("changed") {
            c.add_css_class("changed");
        }
        c.set_valign(gtk::Align::Center);
        meta.append(&c);
    }
    if r.drift {
        meta.append(&icon("ph-warning-diamond-fill-symbolic", &["nav-drift"]));
    }
    if r.kind == RowKind::Link && r.also_in.is_some() {
        meta.append(&icon("ph-link-simple-symbolic", &["nav-also-in"]));
    }
    if r.badge > 0 {
        let b = label(&r.badge.to_string(), &["nav-badge"]);
        b.set_valign(gtk::Align::Center);
        meta.append(&b);
    }
    if r.kind != RowKind::Agent && r.led.is_some() {
        meta.append(&led(r.led));
    }
    hbox.append(&meta);
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bar.add_css_class("nav-row-selbar");
    bar.set_halign(gtk::Align::Start);
    bar.set_valign(gtk::Align::Center);
    bar.set_can_target(false);
    bar.set_visible(r.selected);
    // The indent lives on the row's content so the bar sits at the row's
    // left edge (A.23, A.28).
    hbox.add_css_class("nav-row-inner");
    hbox.add_css_class(&format!("depth-{}", r.depth.min(4)));
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&hbox));
    overlay.add_overlay(&bar);
    row.set_child(Some(&overlay));
    row.add_css_class("nav-row");
    row.add_css_class(&format!("nav-{}", kind_str(r.kind)));
    if r.selected {
        row.add_css_class("selected");
    }
    if r.dim {
        row.add_css_class("dim");
    }
    if r.kind == RowKind::Empty {
        row.set_activatable(false);
    }
    if let Some(t) = tooltip(r) {
        row.set_tooltip_text(Some(&t));
    }
    row
}

fn kind_str(k: RowKind) -> &'static str {
    match k {
        RowKind::Project => "project",
        RowKind::Link => "link",
        RowKind::Worktree => "worktree",
        RowKind::Agent => "agent",
        RowKind::Section => "section",
        RowKind::Scratch => "scratch",
        RowKind::Empty => "empty",
    }
}

/// Fold a parent shown unfolded, or unfold one shown folded (R104.2).
fn toggle_key(key: &str, folded: bool) {
    if let Some(app) = try_app() {
        app.update(|m| m.toggle_project(key, folded));
    }
}

/// Record (or clear) a Worktree's Project: Move to Project ▸ and a
/// suggestion click.
fn move_to_project(source: &str, wt: &str, project: Option<String>) {
    if let Some(app) = try_app() {
        let _ = app.update(|m| m.set_worktree_project(source, wt, project));
    }
}

/// What a click on a row opens (R104, spec A).
fn activate(r: &Row) {
    let Some(app) = try_app() else { return };
    crate::usage_view::close();
    if !matches!(r.target, Target::Project { .. }) {
        crate::home_view::close();
    }
    match &r.target {
        Target::Project { project } => crate::home_view::open(project),
        Target::Link { project, source } => open_link(project, source),
        Target::Worktree { source, worktree } => crate::worktrees::activate(source, worktree),
        Target::Pane { pane } => {
            let src = app.model.borrow().locate(pane).map(|l| l.source);
            let _ = app.update(|m| {
                if let Some(s) = &src {
                    m.select_source(s)?;
                }
                m.focus_pane(pane)
            });
        }
        Target::Scratch { source } => {
            let main = app
                .model
                .borrow()
                .source(source)
                .and_then(|s| s.worktrees.first().map(|w| w.id.clone()));
            match main {
                Some(w) => crate::worktrees::activate(source, &w),
                None => app.activate_source(source),
            }
        }
        Target::None => {}
    }
}

/// A link row: a Terminal already in its checkout, else a new one there
/// (`open_on_link`, creating the Project checkout when needed).
fn open_link(project: &str, source: &str) {
    let Some(app) = try_app() else { return };
    let pane = {
        let m = app.model.borrow();
        let links = crate::navdata::links(&m);
        let l = links
            .iter()
            .find(|l| l.project == project && l.source == source);
        match (l, m.source(source)) {
            (Some(l), Some(s)) => navigator::link_panes(s, l, &links, &crate::projects::canon)
                .into_iter()
                .find(|p| matches!(p.kind, rade_core::PaneKind::Terminal { .. }))
                .map(|p| p.id.clone()),
            _ => None,
        }
    };
    match pane {
        Some(p) => {
            let _ = app.update(|m| {
                m.select_source(source)?;
                m.focus_pane(&p)
            });
        }
        None => crate::home_view::open_terminal(project, source),
    }
}

impl Sidebar {
    pub fn new() -> Self {
        let list = gtk::ListBox::new();
        list.add_css_class("nav-tree");
        list.set_selection_mode(gtk::SelectionMode::None);
        let rows: Rc<RefCell<Vec<Row>>> = Rc::default();
        {
            let rows = rows.clone();
            list.connect_row_activated(move |_, row| {
                let r = rows.borrow().get(row.index() as usize).cloned();
                if let Some(r) = r {
                    glib::idle_add_local_once(move || activate(&r));
                }
            });
        }
        install_menu(&list, rows.clone());

        let scroller = gtk::ScrolledWindow::new();
        scroller.set_vexpand(true);
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroller.set_child(Some(&list));

        // Header: `PROJECTS`, filter and add (A.18, A.19).
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        header.add_css_class("nav-header");
        let heading = label("PROJECTS", &["section-label"]);
        heading.set_hexpand(true);
        header.append(&heading);
        let filter = gtk::Button::from_icon_name("ph-funnel-simple-symbolic");
        filter.add_css_class("tb-button");
        filter.add_css_class("flat");
        filter.add_css_class("nav-filter");
        filter.set_tooltip_text(Some("Find a Source or Worktree"));
        filter.connect_clicked(|_| {
            crate::palette::toggle();
            if let Some(p) = crate::palette::get().filter(|p| p.open.get()) {
                p.entry.set_text("/");
                p.entry.set_position(-1);
            }
        });
        header.append(&filter);
        let add = gtk::Button::from_icon_name("ph-plus-symbolic");
        add.add_css_class("tb-button");
        add.add_css_class("flat");
        add.add_css_class("nav-add");
        add.set_tooltip_text(Some("New Project…"));
        add.connect_clicked(|_| crate::project_dialogs::new_project());
        header.append(&add);

        // Footer: Inbox pill and bell (A.37, A.38).
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        footer.add_css_class("nav-footer");
        let pill = gtk::Button::new();
        pill.add_css_class("inbox-pill");
        pill.set_hexpand(true);
        pill.set_focus_on_click(false);
        let inner = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        // The newest waiting Agent's glyph (R115), set by `sync`.
        let pill_glyph = Glyph::new(AgentMark::Other, Some(AgentStatus::Waiting));
        inner.append(&pill_glyph.root);
        let pill_name = label("", &["inbox-pill-name"]);
        inner.append(&pill_name);
        let words = label("needs input", &["inbox-pill-words"]);
        words.set_hexpand(true);
        inner.append(&words);
        inner.append(&label("Ctrl \u{21e7} G", &["kbd"]));
        pill.set_child(Some(&inner));
        pill.set_tooltip_text(Some("Inbox (Ctrl+Shift+G opens the newest)"));
        pill.connect_clicked(|_| open_inbox(false));
        footer.append(&pill);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        footer.append(&spacer);
        let bell = gtk::Button::from_icon_name("ph-bell-simple-symbolic");
        bell.add_css_class("tb-button");
        bell.add_css_class("flat");
        bell.add_css_class("nav-bell");
        bell.set_tooltip_text(Some("Inbox"));
        bell.connect_clicked(|_| open_inbox(false));
        footer.append(&bell);

        let panel = Panel::new();
        let popover = gtk::Popover::new();
        popover.add_css_class("inbox-popover");
        popover.set_child(Some(&panel.root));
        popover.set_parent(&footer);
        popover.set_position(gtk::PositionType::Top);
        popover.set_has_arrow(false);
        popover.connect_closed(|p| p.set_autohide(true));

        let switcher = crate::switcher::Switcher::new();
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        body.add_css_class("navigator");
        body.append(&header);
        body.append(&scroller);
        body.append(&footer);

        // The rail (A "Text scale 200 %"): shown only at large text.
        let rail = gtk::Box::new(gtk::Orientation::Vertical, 4);
        rail.add_css_class("nav-rail");
        rail.set_size_request(rade_core::chrome::RAIL_WIDTH, -1);
        rail.set_hexpand(false);
        let rail_items = gtk::Box::new(gtk::Orientation::Vertical, 4);
        rail_items.set_vexpand(true);
        rail.append(&rail_items);
        rail.append(&rail_button("ph-bell-simple-symbolic", "Inbox", None));

        Self {
            root: body.upcast(),
            list,
            rows,
            panel,
            switcher,
            pill,
            pill_name,
            pill_glyph,
            popover,
            footer,
            spacer,
            rail,
            rail_items,
            rail_shown: RefCell::default(),
        }
    }

    /// The rail's Project cubes and Scratch, rebuilt when the Projects change.
    fn sync_rail(&self) {
        let rows = self.rows.borrow();
        let projects: Vec<(String, usize)> = rows
            .iter()
            .filter(|r| r.kind == RowKind::Project)
            .map(|r| (r.title.clone(), r.color.unwrap_or(0)))
            .collect();
        if *self.rail_shown.borrow() == projects && self.rail_items.first_child().is_some() {
            return;
        }
        while let Some(c) = self.rail_items.first_child() {
            self.rail_items.remove(&c);
        }
        for (title, color) in &projects {
            let b = rail_button("ph-cube-symbolic", title, Some(*color));
            self.rail_items.append(&b);
        }
        self.rail_items
            .append(&rail_button("ph-terminal-window-symbolic", "Scratch", None));
        *self.rail_shown.borrow_mut() = projects;
    }

    /// The rows for the shown Workspace now.
    fn wanted(ws: &State) -> Vec<Row> {
        let projects = crate::navdata::projects(ws);
        let links = crate::navdata::links(ws);
        let br = branches(ws, &projects);
        let ports = ports();
        let port = |pane: &str| ports.iter().find(|p| p.pane == pane).map(|p| p.port);
        let wt = |p: &std::path::Path| crate::navdata::worktree_facts(ws, p);
        let home = rade_core::paths::home_dir(&rade_core::paths::process_env);
        let shown = crate::home_view::shown(ws);
        let selected = navigator::selection(ws, &links, shown.as_deref(), &crate::projects::canon);
        navigator::build(
            ws,
            &navigator::Input {
                projects: &projects,
                links: &links,
                worktree_facts: &wt,
                port: &port,
                branches: &br,
                canon: &crate::projects::canon,
                home: &home,
                selected: selected.as_ref(),
                now: crate::panes::now(),
            },
        )
    }

    pub fn sync(&self, ws: &State, _unread: &UnreadCounts) {
        let started = std::time::Instant::now();
        let wanted = Self::wanted(ws);
        // Only the rows that changed are rebuilt (R111-c).
        let change = navigator::splice(&self.rows.borrow(), &wanted);
        if let Some((start, removed, inserted)) = change {
            for _ in 0..removed {
                if let Some(r) = self.list.row_at_index(start as i32) {
                    self.list.remove(&r);
                }
            }
            for (i, r) in wanted[start..start + inserted].iter().enumerate() {
                self.list.insert(&build_row(r), (start + i) as i32);
            }
            *self.rows.borrow_mut() = wanted;
        }
        crate::perf::record("navigator_sync", started.elapsed(), change.is_some());
        self.sync_rail();
        self.panel.sync(ws);
        self.switcher.sync(ws);
        // The pill: the newest waiting Agent of the shown Workspace.
        let newest = ws
            .waiting_panes()
            .into_iter()
            .filter(|p| ws.pane_visible(p))
            .filter_map(|p| ws.pane(&p).map(|x| (x.status_since.unwrap_or(0), x)))
            .max_by_key(|(t, _)| *t)
            .map(|(_, p)| {
                let name = p
                    .name
                    .clone()
                    .or_else(|| p.agent.clone())
                    .unwrap_or_else(|| p.id.clone());
                let icon = icon_of(p.agent_name());
                (name, p.mark().unwrap_or(AgentMark::Other), icon)
            });
        self.pill.set_visible(newest.is_some());
        self.spacer.set_visible(newest.is_none());
        let (name, mark, icon) = newest.unwrap_or((String::new(), AgentMark::Other, None));
        self.pill_name.set_label(&name);
        self.pill_glyph
            .set(mark, icon.as_deref(), Some(AgentStatus::Waiting));
    }

    /// The scrolled window around the rows (R111-c timing).
    pub fn scroller(&self) -> Option<gtk::ScrolledWindow> {
        self.list
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_then(|w| w.downcast().ok())
    }

    /// The row of `f`'s first match and its bounds in the list.
    fn row_bounds(&self, f: impl Fn(&Row) -> bool) -> Option<(gtk::Widget, gtk::gdk::Rectangle)> {
        let i = self.rows.borrow().iter().position(f)?;
        let r = self.list.row_at_index(i as i32)?;
        let b = r.compute_bounds(&self.list)?;
        Some((
            self.list.clone().upcast(),
            gtk::gdk::Rectangle::new(
                b.x() as i32,
                b.y() as i32,
                b.width() as i32,
                b.height() as i32,
            ),
        ))
    }

    /// A Source's row (a link or Scratch row; popovers point at it).
    pub fn source_row(&self, source: &str) -> Option<(gtk::Widget, gtk::gdk::Rectangle)> {
        self.row_bounds(|r| match &r.target {
            Target::Link { source: s, .. } | Target::Scratch { source: s } => s == source,
            _ => false,
        })
    }

    /// A Source's Scratch row (`debug.menu scratch`).
    pub fn scratch_row(&self, source: &str) -> Option<(gtk::Widget, gtk::gdk::Rectangle)> {
        self.row_bounds(|r| matches!(&r.target, Target::Scratch { source: s } if s == source))
    }

    /// A Project's row (`debug.menu project`).
    pub fn project_row(&self, project: &str) -> Option<(gtk::Widget, gtk::gdk::Rectangle)> {
        self.row_bounds(|r| matches!(&r.target, Target::Project { project: p } if p == project))
    }

    /// A Worktree's row (`debug.menu worktree`).
    pub fn worktree_row(
        &self,
        source: &str,
        wt: &str,
    ) -> Option<(gtk::Widget, gtk::gdk::Rectangle)> {
        self.row_bounds(|r| {
            matches!(&r.target, Target::Worktree { source: s, worktree: w } if s == source && w == wt)
        })
    }

    /// `debug.ui sidebar` / `navigator`: every row as shown.
    pub fn debug_rows(&self) -> Value {
        let rows = self.rows.borrow();
        let out: Vec<Value> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let w = self.list.row_at_index(i as i32);
                json!({
                    "kind": kind_str(r.kind), "depth": r.depth, "text": r.label(),
                    "title": r.title, "meta": r.meta, "drift": r.drift, "review": r.review,
                    "also_in": r.kind == RowKind::Link && r.also_in.is_some(),
                    "tooltip": w.as_ref().and_then(|w| w.tooltip_text()).map(|t| t.to_string()),
                    "suggest": suggest_text(r), "collapsed": r.collapsed,
                    "selected": r.selected, "led": r.led.map(|s| s.as_str()),
                    "glyph": r.mark.map(|m| m.as_str()),
                    "badge": r.badge, "dim": r.dim, "key": r.key, "target": r.target,
                    "height": w.as_ref().map(|w| w.height()),
                })
            })
            .collect();
        json!(out)
    }

    /// The row whose `key` (or, with `kind`, title) matches.
    fn find(&self, key: &str, kind: Option<&str>) -> Option<Row> {
        self.rows
            .borrow()
            .iter()
            .find(|r| match kind {
                Some(k) => kind_str(r.kind) == k && (r.title == key || r.label() == key),
                None => r.key == key,
            })
            .cloned()
    }

    /// `debug.ui nav_click` (`key`, or `kind` + `title`): a click on that row.
    pub fn click(&self, key: &str, kind: Option<&str>) -> bool {
        match self.find(key, kind) {
            Some(r) => {
                activate(&r);
                true
            }
            None => false,
        }
    }

    /// `debug.ui nav_toggle`: that row's chevron.
    pub fn toggle(&self, key: &str, kind: Option<&str>) -> bool {
        match self.find(key, kind) {
            Some(Row {
                key,
                collapsed: Some(c),
                ..
            }) => {
                toggle_key(&key, c);
                true
            }
            _ => false,
        }
    }

    /// `debug.ui` `click_row` (0.7 names): `worktree` <id>, `checkout` <source>.
    pub fn click_row(&self, kind: &str, id: &str) -> bool {
        let r = self
            .rows
            .borrow()
            .iter()
            .find(|r| match (kind, &r.target) {
                ("worktree", Target::Worktree { worktree, .. }) => worktree == id,
                ("checkout", Target::Scratch { source }) => source == id,
                ("checkout", Target::Link { source, .. }) => source == id,
                _ => false,
            })
            .cloned();
        match r {
            Some(r) => {
                activate(&r);
                true
            }
            None => false,
        }
    }

    /// Whether Project `id`'s row shows folded.
    pub fn project_folded(&self, id: &str) -> bool {
        self.rows.borrow().iter().any(|r| {
            matches!(&r.target, Target::Project { project } if project == id)
                && r.collapsed == Some(true)
        })
    }

    /// `debug.ui` `suggest`: a click on Worktree `wt`'s `→ Project?`.
    pub fn click_suggest(&self, wt: &str) -> bool {
        let hit = self
            .rows
            .borrow()
            .iter()
            .find_map(|r| match (&r.target, &r.suggest) {
                (Target::Worktree { source, worktree }, Some((p, _))) if worktree == wt => {
                    Some((source.clone(), worktree.clone(), p.clone()))
                }
                _ => None,
            });
        match hit {
            Some((s, w, p)) => {
                move_to_project(&s, &w, Some(p));
                true
            }
            None => false,
        }
    }

    /// `debug.ui` `click_agent`: what a click on that Agent's row does.
    pub fn click_agent(&self, pane: &str) -> bool {
        self.click(pane, None)
    }

    /// `debug.ui inbox`: the pill, the popover and its rows (newest first);
    /// `arg` `open` / `pill` / `bell` open it (without a grab: headless
    /// Weston has no seat), `close` / `escape` close it.
    pub fn inbox(&self, arg: Option<&str>) -> Value {
        match arg.unwrap_or("") {
            "open" | "pill" | "bell" => open_inbox(true),
            "close" | "escape" => self.popover.popdown(),
            _ => {}
        }
        let mut v = self.panel.debug();
        v["pill"] = json!(self
            .pill
            .is_visible()
            .then(|| format!("{} needs input", self.pill_name.label())));
        v["pill_glyph"] = self.pill_glyph.debug();
        v["open"] = json!(self.popover.is_visible());
        v
    }
}

/// Open the Inbox popover over the footer (`test`: without a grab).
fn open_inbox(test: bool) {
    let Some(app) = try_app() else { return };
    let s = &app.sidebar;
    if test {
        s.popover.set_autohide(false);
    }
    let w = s.footer.width().max(1);
    s.popover
        .set_pointing_to(Some(&gtk::gdk::Rectangle::new(0, 0, w, 1)));
    s.panel.sync(&app.model.borrow());
    s.popover.popup();
}

/// A `project.toml` changed: read the links again.
pub fn projects_changed() {
    crate::navdata::refresh();
    if let Some(app) = try_app() {
        resync(&app);
    }
}

/// Re-render the Navigator alone, like `App::sync`.
pub fn resync(app: &crate::app::App) {
    if app.syncing.replace(true) {
        return;
    }
    {
        let m = app.model.borrow();
        app.sidebar.sync(&m, &m.unread_counts());
    }
    app.syncing.set(false);
}

thread_local! {
    /// Last port scan (R18.4) and when: tooltips re-query on every motion.
    static PORTS: RefCell<Option<(std::time::Instant, Vec<crate::browser::PortInfo>)>> =
        const { RefCell::new(None) };
}

/// The listening ports (R18.4), scanned at most every 2 s.
pub fn ports() -> Vec<crate::browser::PortInfo> {
    let Some(app) = try_app() else {
        return Vec::new();
    };
    let fresh = |t: &std::time::Instant| t.elapsed() < Duration::from_secs(2);
    PORTS.with(|c| {
        let mut c = c.borrow_mut();
        if !c.as_ref().is_some_and(|(t, _)| fresh(t)) {
            let by_pane = crate::browser::scan_ports();
            *c = Some((
                std::time::Instant::now(),
                crate::browser::label_ports(&app, by_pane),
            ));
        }
        c.as_ref().unwrap().1.clone()
    })
}

/// A Worktree row's menu: Review Changes, New Worktree…, Remove Worktree…
/// / Prune…, and Move to Project ▸ when Projects list its Source.
pub fn worktree_menu(m: &State, p: &str, w: &str) -> gio::Menu {
    let menu = gio::Menu::new();
    let item = |menu: &gio::Menu, label: &str, action: &str, target: String| {
        let i = gio::MenuItem::new(Some(label), None);
        i.set_action_and_target_value(Some(&format!("sb.{action}")), Some(&target.to_variant()));
        crate::menus::set_accel(&i, action);
        menu.append_item(&i);
    };
    let wt = m.worktree(p, w);
    let missing = wt.is_some_and(|w| !w.path.exists());
    if !missing {
        item(&menu, "Review Changes", "review", format!("{p}\u{1f}{w}"));
    }
    item(&menu, "New Worktree…", "new-worktree", format!("{p}\u{1f}"));
    let main = m.is_main_worktree(p, w);
    if !main {
        let label = if missing {
            "Prune…"
        } else {
            "Remove Worktree…"
        };
        item(&menu, label, "remove-worktree", format!("{p}\u{1f}{w}"));
    }
    let listing: Vec<Project> = projects(m).iter().filter(|x| x.lists(p)).cloned().collect();
    // A main checkout never belongs to a Project (ADR-0028).
    if !listing.is_empty() && !main {
        let sub = gio::Menu::new();
        let current = wt
            .and_then(|w| w.project.as_deref())
            .filter(|c| listing.iter().any(|x| x.id == *c));
        for x in &listing {
            let mark = if current == Some(x.id.as_str()) {
                " ✓"
            } else {
                ""
            };
            item(
                &sub,
                &format!("{}{mark}", x.name),
                "move-to-project",
                format!("{p}\u{1f}{w}\u{1f}{}", x.id),
            );
        }
        let mark = if current.is_none() { " ✓" } else { "" };
        item(
            &sub,
            &format!("No Project{mark}"),
            "move-to-project",
            format!("{p}\u{1f}{w}\u{1f}"),
        );
        menu.append_submenu(Some("Move to Project"), &sub);
    }
    menu
}

/// A Scratch row's menu: "Remove from Scratch…" when it has Panes to
/// close (R104.5), then the Source's menu.
pub fn scratch_menu(m: &State, s: &rade_core::Source) -> gio::Menu {
    let menu = gio::Menu::new();
    let links = crate::navdata::links(m);
    if !navigator::scratch_panes(s, &links, &crate::projects::canon).is_empty() {
        let remove = gio::Menu::new();
        let i = gio::MenuItem::new(Some("Remove from Scratch…"), None);
        i.set_action_and_target_value(
            Some("sb.scratch-remove"),
            Some(&format!("{}\u{1f}", s.id).to_variant()),
        );
        remove.append_item(&i);
        menu.append_section(None, &remove);
    }
    menu.append_section(None, &crate::menus::source_menu(s));
    menu
}

/// Remove from Scratch (R104.5): close the Panes of a linked Source's main
/// checkout that no link claims, after one confirmation; the Source and
/// its Projects stay.
fn remove_from_scratch_dialog(source: &str) {
    let Some(app) = try_app() else { return };
    let (name, path, panes, busy) = {
        let m = app.model.borrow();
        let Some(s) = m.source(source) else { return };
        let links = crate::navdata::links(&m);
        let found = navigator::scratch_panes(s, &links, &crate::projects::canon);
        let panes: Vec<String> = found.iter().map(|p| p.id.clone()).collect();
        let busy: Vec<String> = found
            .iter()
            .filter(|p| p.agent.is_some())
            .map(|p| {
                p.name
                    .clone()
                    .or_else(|| p.agent.clone())
                    .unwrap_or_default()
            })
            .collect();
        (s.name.clone(), s.path.clone(), panes, busy)
    };
    if panes.is_empty() {
        return;
    }
    let n = panes.len();
    let mut body = format!(
        "Closes its {n} Pane{} in {}.",
        if n == 1 { "" } else { "s" },
        path.display()
    );
    if !busy.is_empty() {
        body += "\n\nThese Agents will be stopped:\n";
        for b in &busy {
            body += &format!("\n• {b}");
        }
    }
    body += "\n\nThe Source and its Projects stay; no files are deleted.";
    let d = adw::AlertDialog::new(Some(&format!("Remove “{name}” from Scratch?")), Some(&body));
    d.add_response("cancel", "Cancel");
    d.add_response("remove", "Close Panes");
    d.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
    d.set_default_response(Some("cancel"));
    d.set_close_response("cancel");
    d.connect_response(None, move |_, r| {
        if r == "remove" {
            for p in &panes {
                if let Err(e) = app.close_pane(p, false) {
                    crate::worktrees::alert("Could not close a Pane", &e.message);
                    break;
                }
            }
        }
    });
    crate::menus::present(d, &["cancel", "remove"]);
}

/// Right-click menus: a Project's (settings, New Worktree…, project.toml,
/// Customize), a link's / Scratch row's Source menu, a Worktree's, an
/// Agent's Pane menu.
fn install_menu(list: &gtk::ListBox, rows: Rc<RefCell<Vec<Row>>>) {
    let group = gio::SimpleActionGroup::new();
    let act = |name: &str, f: fn(&str, &str)| {
        let a = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
        a.connect_activate(move |_, v| {
            let v: String = v.and_then(|v| v.get()).unwrap_or_default();
            let (p, w) = v.split_once('\u{1f}').unwrap_or((&v, ""));
            f(p, w);
        });
        group.add_action(&a);
    };
    act("new-worktree", |p, _| crate::worktrees::new_dialog(p));
    act("project-new-worktree", |k, _| {
        crate::projects::new_worktree_dialog(k)
    });
    act("project-open", |k, _| crate::projects::open_file(k));
    act("project-settings", |k, _| crate::project_settings::open(k));
    // Link rows (`<ws>/<project>␟<source>`).
    act("link-base", |k, s| {
        crate::project_dialogs::change_base(k, s)
    });
    act("link-add", |k, _| {
        crate::project_dialogs::add_source(k, None)
    });
    act("link-settings", |k, _| crate::project_dialogs::settings(k));
    act("link-terminal", |k, s| {
        let p = k.split_once('/').map_or(k, |(_, p)| p);
        crate::home_view::open_terminal(p, s);
    });
    act("project-customize", |k, _| crate::projects::customize(k));
    act("scratch-remove", |p, _| remove_from_scratch_dialog(p));
    act("remove-worktree", |p, w| {
        crate::worktrees::remove_dialog(p, w)
    });
    // Target `source␟worktree␟project` ("" = no Project).
    act("move-to-project", |p, rest| {
        let (w, proj) = rest.split_once('\u{1f}').unwrap_or((rest, ""));
        let proj = (!proj.is_empty()).then(|| proj.to_string());
        move_to_project(p, w, proj);
    });
    act("review", |p, w| {
        if let Some(app) = try_app() {
            let w = if w.is_empty() { None } else { Some(w) };
            let found = app.model.borrow().find_worktree(p, w);
            if let Ok((p, w)) = found {
                let _ = app.update(|m| m.open_review(&p, &w));
            }
        }
    });
    list.insert_action_group("sb", Some(&group));

    let popover = gtk::PopoverMenu::from_model(None::<&gio::MenuModel>);
    popover.set_parent(list);
    popover.set_has_arrow(false);
    popover.set_halign(gtk::Align::Start);
    let click = gtk::GestureClick::new();
    click.set_button(3);
    let l = list.clone();
    click.connect_pressed(move |_, _, x, y| {
        let Some(app) = try_app() else { return };
        let Some(r) = l.row_at_y(y as i32) else {
            return;
        };
        let Some(row) = rows.borrow().get(r.index() as usize).cloned() else {
            return;
        };
        let m = app.model.borrow();
        let model = match &row.target {
            Target::Project { .. } => crate::projects::menu(&row.key),
            Target::Link { project, source } => {
                let Some(s) = m.source(source) else { return };
                // The link's own actions (R106.2), then the Source's menu.
                let menu = gio::Menu::new();
                let link = gio::Menu::new();
                let key = format!("{}/{project}\u{1f}{source}", m.active_workspace());
                for (label, action) in [
                    ("Open terminal", "sb.link-terminal"),
                    ("Change base…", "sb.link-base"),
                    ("Add source to Project…", "sb.link-add"),
                    ("Project settings…", "sb.link-settings"),
                ] {
                    let i = gio::MenuItem::new(Some(label), None);
                    i.set_action_and_target_value(Some(action), Some(&key.to_variant()));
                    link.append_item(&i);
                }
                menu.append_section(None, &link);
                menu.append_section(None, &crate::menus::source_menu(s));
                menu
            }
            Target::Scratch { source } => {
                let Some(s) = m.source(source) else { return };
                scratch_menu(&m, s)
            }
            Target::Pane { pane } => crate::menus::pane_menu(pane),
            Target::Worktree { source, worktree } => worktree_menu(&m, source, worktree),
            Target::None => return,
        };
        popover.set_menu_model(Some(&model));
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.popup();
    });
    list.add_controller(click);
}

/// A rail icon (tooltip = the name it stands for); a click opens the full
/// Navigator as an overlay (R111.2).
fn rail_button(icon_name: &str, tip: &str, color: Option<usize>) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon_name);
    b.add_css_class("flat");
    b.add_css_class("nav-rail-button");
    if let Some(c) = color {
        b.add_css_class(&format!("proj-{}", c % 3 + 1));
    }
    b.set_tooltip_text(Some(tip));
    b.set_focus_on_click(false);
    b.set_halign(gtk::Align::Center);
    b.connect_clicked(|_| crate::layout_mode::open_navigator());
    b
}
