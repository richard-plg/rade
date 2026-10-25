//! The Inspector's Changes page (PRD R120, R121; ADR-0013, ADR-0045; spec
//! A.58–A.86): one card per Source link of the focused Project — a
//! checkout outside every Project shows one card for itself — under the
//! header `PROJECT · SHOP` / `SOURCE · SHOP-API` with the totals, the list /
//! tree toggle, Refresh and `…` (Fetch All, Sort by). This is the shell:
//! which cards, the reads of their checkouts, the polls, the opt-in
//! auto-fetch and `debug.scm`. A card is `changes_card.rs`, what its buttons
//! do `changes_ops.rs`, the logic `rade_core::scm_view` / `rade_core::scm`.
//! git runs off the GTK thread, on a click, a poll of the visible page or
//! the auto-fetch timer when the owner turned it on.

use crate::app::try_app;
use crate::changes_card::{parse_group, Card, Read};
use adw::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::config::{ScmSort, ScmView};
use rade_core::home::{self, Group};
use rade_core::linkview::LinkView;
use rade_core::rpc::{Error, Result};
use rade_core::scm;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Project,
    Source,
}

/// What the page shows; cards are rebuilt when it changes.
#[derive(Clone, Debug, PartialEq)]
struct Shown {
    mode: Mode,
    title: String,
    groups: Vec<Group>,
    /// The Sources' display names, by group.
    names: Vec<String>,
}

pub struct Changes {
    pub root: gtk::Stack,
    body: gtk::Box,
    title: gtk::Label,
    add: gtk::Label,
    del: gtk::Label,
    view_btn: gtk::Button,
    page_actions: gio::SimpleActionGroup,
    empty_project: gtk::Box,
    empty_source: gtk::Box,
    cards: RefCell<Vec<Rc<Card>>>,
    shown: RefCell<Option<Shown>>,
    reads: RefCell<HashMap<PathBuf, Read>>,
    messages: RefCell<HashMap<PathBuf, String>>,
    monitors: RefCell<HashMap<PathBuf, gio::FileMonitor>>,
    last_fetch: RefCell<HashMap<PathBuf, Instant>>,
    /// Bumped when the cards change: older answers are dropped.
    generation: Cell<u64>,
    refreshing: Cell<bool>,
    again: Cell<bool>,
    autofetching: Cell<bool>,
    panel_shown: Cell<bool>,
}

thread_local! {
    static CHANGES: RefCell<Option<Rc<Changes>>> = const { RefCell::new(None) };
}

pub fn get() -> Option<Rc<Changes>> {
    CHANGES.with(|c| c.borrow().clone())
}

/// Re-check Commit's primary state (palette or a dialog opened / closed).
pub fn sync_primary() {
    if let Some(c) = get() {
        c.sync_primary();
    }
}

/// Read the checkouts again (after an operation).
pub(crate) fn refresh_soon() {
    if let Some(c) = get() {
        c.refresh();
    }
}

fn tool(icon: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.add_css_class("flat");
    b.add_css_class("changes-tool");
    b.set_tooltip_text(Some(tip));
    b.set_valign(gtk::Align::Center);
    b
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

/// Build the page; the Inspector shows it (R20.10).
pub fn install() -> gtk::Widget {
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.add_css_class("changes-project");
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    header.add_css_class("changes-header");
    let title = label("", &["section-label", "changes-title"]);
    title.set_hexpand(true);
    header.append(&title);
    let add = label("", &["changes-total-add"]);
    let del = label("", &["changes-total-del"]);
    header.append(&add);
    header.append(&del);
    let view_btn = tool("ph-tree-view-symbolic", "View as Tree");
    header.append(&view_btn);
    let refresh = tool("ph-arrows-clockwise-symbolic", "Refresh");
    header.append(&refresh);
    let more = gtk::MenuButton::new();
    more.set_icon_name("ph-dots-three-symbolic");
    more.add_css_class("flat");
    more.add_css_class("changes-tool");
    more.set_tooltip_text(Some("More Actions…"));
    more.set_valign(gtk::Align::Center);
    header.append(&more);
    body.append(&header);
    let empty_project = crate::chrome::empty_state(&rade_core::chrome::EMPTY_PROJECT_CHANGES);
    let empty_source = crate::chrome::empty_state(&rade_core::chrome::EMPTY_CHANGES);
    empty_project.set_visible(false);
    empty_source.set_visible(false);
    body.append(&empty_project);
    body.append(&empty_source);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroll.set_vexpand(true);
    scroll.set_child(Some(&body));

    let none = crate::chrome::empty_state(&rade_core::chrome::EMPTY_NO_REPO);
    let root = gtk::Stack::new();
    root.add_named(&scroll, Some("page"));
    root.add_named(&none, Some("none"));

    let c = Rc::new(Changes {
        root,
        body,
        title,
        add,
        del,
        view_btn: view_btn.clone(),
        page_actions: gio::SimpleActionGroup::new(),
        empty_project,
        empty_source,
        cards: RefCell::default(),
        shown: RefCell::default(),
        reads: RefCell::default(),
        messages: RefCell::default(),
        monitors: RefCell::default(),
        last_fetch: RefCell::default(),
        generation: Cell::new(0),
        refreshing: Cell::new(false),
        again: Cell::new(false),
        autofetching: Cell::new(false),
        panel_shown: Cell::new(false),
    });

    // Header: the toggle, Refresh and the `…` menu (Fetch All, Sort by).
    let w = Rc::downgrade(&c);
    view_btn.connect_clicked(move |_| {
        if let Some(c) = w.upgrade() {
            let next = match current_view() {
                ScmView::List => "tree",
                ScmView::Tree => "list",
            };
            c.set_view(next);
        }
    });
    let w = Rc::downgrade(&c);
    refresh.connect_clicked(move |_| {
        if let Some(c) = w.upgrade() {
            c.refresh();
        }
    });
    let fetch = gio::SimpleAction::new("fetch-all", None);
    let w = Rc::downgrade(&c);
    fetch.connect_activate(move |_, _| {
        if let Some(c) = w.upgrade() {
            c.fetch_all();
        }
    });
    c.page_actions.add_action(&fetch);
    let sort = gio::SimpleAction::new_stateful(
        "sort",
        Some(glib::VariantTy::STRING),
        &"path".to_variant(),
    );
    let w = Rc::downgrade(&c);
    sort.connect_activate(move |_, v| {
        if let (Some(c), Some(v)) = (w.upgrade(), v.and_then(|v| v.get::<String>())) {
            c.set_sort(&v);
        }
    });
    c.page_actions.add_action(&sort);
    let menu = gio::Menu::new();
    let top = gio::Menu::new();
    top.append(Some("Fetch All"), Some("page.fetch-all"));
    menu.append_section(None, &top);
    let sorts = gio::Menu::new();
    for (l, v) in [
        ("Sort by Path", "path"),
        ("Sort by Name", "name"),
        ("Sort by Status", "status"),
    ] {
        let i = gio::MenuItem::new(Some(l), None);
        i.set_action_and_target_value(Some("page.sort"), Some(&v.to_variant()));
        sorts.append_item(&i);
    }
    menu.append_section(None, &sorts);
    more.set_menu_model(Some(&menu));
    header.insert_action_group("page", Some(&c.page_actions));
    c.update_chrome();

    CHANGES.with(|x| *x.borrow_mut() = Some(c.clone()));
    // While the page is on screen and the window active (R21.8).
    glib::timeout_add_local(POLL, || {
        if let (Some(c), Some(app)) = (get(), try_app()) {
            if c.root.is_mapped() && app.window.is_active() {
                c.refresh();
            }
        }
        glib::ControlFlow::Continue
    });
    // The opt-in auto-fetch (R121.4), whether or not the page is visible.
    glib::timeout_add_seconds_local(30, || {
        if let Some(c) = get() {
            c.autofetch_tick();
        }
        glib::ControlFlow::Continue
    });
    c.root.clone().upcast()
}

fn current_view() -> ScmView {
    try_app().map_or(ScmView::default(), |a| a.config.borrow().scm.view)
}

fn current_sort() -> ScmSort {
    try_app().map_or(ScmSort::default(), |a| a.config.borrow().scm.sort)
}

/// The Project of the focused checkout and its Changes groups, if it is
/// in one (a linked Worktree's recorded Project, else the first Project
/// whose link uses this checkout).
pub fn focused(ws: &rade_core::State) -> Option<(String, String, Vec<home::Group>)> {
    let links: Vec<LinkView> = crate::navdata::links(ws);
    let projects = crate::navdata::projects(ws);
    let (s, w) = ws.active();
    if s.is_general() {
        return None;
    }
    let canon = crate::projects::canon;
    let here = canon(&w.path);
    let project = if ws.is_main_worktree(&s.id, &w.id) {
        projects
            .iter()
            .find(|p| {
                links.iter().any(|l| {
                    l.project == p.id
                        && l.source == s.id
                        && l.path.as_deref().is_some_and(|x| canon(x) == here)
                })
            })
            .or_else(|| {
                // A Terminal on a link's Project checkout lives in the
                // main Worktree: the link whose checkout holds its cwd.
                let cwd = ws
                    .active_tab()
                    .and_then(|t| t.pane(&t.focused))
                    .and_then(|p| match &p.kind {
                        rade_core::PaneKind::Terminal { cwd, .. } => Some(canon(cwd)),
                        _ => None,
                    });
                projects.iter().find(|p| {
                    links.iter().any(|l| {
                        l.project == p.id
                            && l.source == s.id
                            && l.path
                                .as_deref()
                                .zip(cwd.as_deref())
                                .is_some_and(|(x, c)| c.starts_with(canon(x)))
                    })
                })
            })
    } else {
        w.project
            .as_deref()
            .and_then(|id| projects.iter().find(|p| p.id == id))
            .filter(|p| links.iter().any(|l| l.project == p.id && l.source == s.id))
    }?;
    let groups = home::change_groups(ws, &project.id, &links, Some((&s.id, &w.id)));
    Some((project.id.clone(), project.name.clone(), groups))
}

/// What the page shows for the focused checkout (D1).
fn shown_for(ws: &rade_core::State) -> Shown {
    let name_of = |id: &str| ws.source(id).map_or(id.to_string(), |s| s.name.clone());
    if let Some((_, pname, groups)) = focused(ws) {
        let names = groups.iter().map(|g| name_of(&g.source)).collect();
        return Shown {
            mode: Mode::Project,
            title: rade_core::scm_view::header_title(Some(&pname), ""),
            groups,
            names,
        };
    }
    let (s, w) = ws.active();
    let group = Group {
        project: String::new(),
        source: s.id.clone(),
        path: w.path.clone(),
        worktree: Some((s.id.clone(), w.id.clone())),
        // The card shows the branch itself.
        label: String::new(),
        // Empty: the read resolves the default branch off the GTK thread.
        base: ws.comparison_base(&w.path).unwrap_or_default(),
    };
    Shown {
        mode: Mode::Source,
        title: rade_core::scm_view::header_title(None, &s.name),
        groups: vec![group],
        names: vec![s.name.clone()],
    }
}

impl Changes {
    /// Every sync: what is in focus; the cards follow it.
    pub fn follow_project(self: &Rc<Self>, ws: &rade_core::State) {
        let now = shown_for(ws);
        if self.shown.borrow().as_ref() != Some(&now) {
            *self.shown.borrow_mut() = Some(now);
            self.generation.set(self.generation.get() + 1);
            self.rebuild_cards();
            self.refresh();
        }
        self.sync_drift(ws);
        // The focused checkout's identity warning (R107.5) for a checkout
        // outside every Project.
        let warn = crate::chrome::identity_warning(ws);
        for c in self.cards.borrow().iter() {
            c.set_focus_identity(warn.clone());
        }
        self.sync_primary();
    }

    /// Every sync: refresh when the Inspector comes into view (R21.8).
    pub fn follow(self: &Rc<Self>, _root: &Path, panel_shown: bool) {
        let was = self.panel_shown.replace(panel_shown);
        if panel_shown && !was {
            self.refresh();
        }
    }

    pub(crate) fn remember_message(&self, path: &Path, text: &str) {
        let mut m = self.messages.borrow_mut();
        if text.is_empty() {
            m.remove(path);
        } else {
            m.insert(path.to_path_buf(), text.to_string());
        }
    }

    fn rebuild_cards(self: &Rc<Self>) {
        let Some(shown) = self.shown.borrow().clone() else {
            return;
        };
        let old: Vec<Rc<Card>> = std::mem::take(&mut *self.cards.borrow_mut());
        for c in &old {
            self.body.remove(&c.outer);
        }
        let mut cards = Vec::new();
        for (g, name) in shown.groups.iter().zip(&shown.names) {
            let reused = old.iter().find(|c| {
                c.path == g.path && c.source == g.source && c.project == g.project && c.name == *name
            });
            let card = match reused {
                Some(c) => {
                    c.set_label(&g.label);
                    c.clone()
                }
                None => {
                    let c = Card::new(g, name);
                    c.set_label(&g.label);
                    if let Some(t) = self.messages.borrow().get(&g.path) {
                        c.set_message(t);
                    }
                    c
                }
            };
            if let Some(r) = self.reads.borrow().get(&g.path) {
                card.set_read(r.clone());
            }
            self.body.append(&card.outer);
            cards.push(card);
        }
        // A `.git` monitor per card (an Agent's `git add`, a commit).
        let paths: Vec<PathBuf> = shown.groups.iter().map(|g| g.path.clone()).collect();
        self.monitors.borrow_mut().retain(|p, _| paths.contains(p));
        for p in &paths {
            if !self.monitors.borrow().contains_key(p) {
                if let Some(m) = self.watch_git(p) {
                    self.monitors.borrow_mut().insert(p.clone(), m);
                }
            }
        }
        *self.cards.borrow_mut() = cards;
        self.update_header();
    }

    /// Refresh at once when git's own folder changes (commit, checkout, an
    /// Agent's `git add`). A linked worktree's `.git` is a file naming it.
    fn watch_git(self: &Rc<Self>, root: &Path) -> Option<gio::FileMonitor> {
        let dot = root.join(".git");
        let dir = if dot.is_file() {
            std::fs::read_to_string(&dot)
                .ok()
                .and_then(|t| t.strip_prefix("gitdir:").map(|d| root.join(d.trim())))
        } else {
            Some(dot)
        };
        let monitor = dir.filter(|d| d.is_dir()).and_then(|d| {
            gio::File::for_path(d)
                .monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
                .ok()
        })?;
        monitor.set_rate_limit(500);
        let w = Rc::downgrade(self);
        monitor.connect_changed(move |_, f, _, _| {
            // Lock files come and go during every git call.
            if f.basename()
                .is_some_and(|n| n.to_string_lossy().ends_with(".lock"))
            {
                return;
            }
            if let Some(c) = w.upgrade() {
                if c.panel_shown.get() {
                    c.refresh();
                }
            }
        });
        Some(monitor)
    }

    /// The drift card of a drifted link, below its card (A.67).
    fn sync_drift(&self, ws: &rade_core::State) {
        let Some(shown) = self.shown.borrow().clone() else {
            return;
        };
        if shown.mode != Mode::Project {
            for c in self.cards.borrow().iter() {
                c.set_drift(None);
            }
            return;
        }
        let links = crate::navdata::links(ws);
        for (c, g) in self.cards.borrow().iter().zip(&shown.groups) {
            let drift = links
                .iter()
                .find(|l| l.project == g.project && l.source == g.source)
                .and_then(|l| {
                    l.drift
                        .as_ref()
                        .filter(|_| g.worktree.is_none())
                        .map(|d| home::drift_card(&l.source, &l.project_name, d))
                });
            c.set_drift(drift);
        }
    }

    /// Read each card's checkout again (one read at a time).
    pub fn refresh(self: &Rc<Self>) {
        let Some(shown) = self.shown.borrow().clone() else {
            return;
        };
        if shown.groups.is_empty() {
            self.update_header();
            return;
        }
        if self.refreshing.replace(true) {
            self.again.set(true);
            return;
        }
        let generation = self.generation.get();
        // Each checkout's `user.email` is read under its Source's
        // Workspace env (R23.3), as its Commit runs.
        let (email, envs) = match try_app() {
            Some(app) => {
                let m = app.model.borrow();
                let email = m
                    .workspace(m.active_workspace())
                    .and_then(|w| rade_core::workspace::WorkspaceFile::read(&w.root).ok())
                    .and_then(|f| f.identity.git_email);
                let envs: Vec<_> = shown
                    .groups
                    .iter()
                    .map(|g| crate::app::workspace_env(&m, &g.source))
                    .collect();
                (email, envs)
            }
            None => (None, vec![Vec::new(); shown.groups.len()]),
        };
        let groups = shown.groups.clone();
        let w = Rc::downgrade(self);
        crate::worktrees::in_background(
            move || {
                groups
                    .iter()
                    .zip(envs)
                    .map(|(g, env)| {
                        let base = if g.base.is_empty() {
                            rade_core::git::default_branch(&g.path).unwrap_or_else(|| "main".into())
                        } else {
                            g.base.clone()
                        };
                        let snap = scm::snapshot(&g.path, &base).map_err(|e| e.message);
                        let identity = (snap.is_ok() && !g.project.is_empty()).then(|| {
                            rade_core::chrome::identity_warning(
                                email.as_deref(),
                                crate::titlebar::read_email(&g.path, env).as_deref(),
                            )
                        });
                        (
                            g.path.clone(),
                            Read {
                                snap,
                                identity: identity.flatten(),
                            },
                        )
                    })
                    .collect::<HashMap<_, _>>()
            },
            move |reads| {
                let Some(c) = w.upgrade() else { return };
                c.refreshing.set(false);
                if c.generation.get() == generation {
                    c.apply_reads(reads);
                }
                if c.again.take() {
                    c.refresh();
                }
            },
        );
    }

    fn apply_reads(self: &Rc<Self>, reads: HashMap<PathBuf, Read>) {
        for c in self.cards.borrow().iter() {
            if let Some(r) = reads.get(&c.path) {
                c.set_read(r.clone());
            }
        }
        self.reads.borrow_mut().extend(reads);
        self.update_header();
        self.sync_primary();
    }

    /// The header, the empty states, the Inspector's count and which page
    /// shows.
    fn update_header(&self) {
        let Some(shown) = self.shown.borrow().clone() else {
            return;
        };
        let cards = self.cards.borrow();
        self.title.set_label(&shown.title);
        let (a, d) = cards.iter().fold((0, 0), |(a, d), c| {
            let t = c.totals();
            (a + t.0, d + t.1)
        });
        self.add.set_label(&format!("+{a}"));
        self.del.set_label(&format!("−{d}"));
        let read = cards.iter().filter(|c| c.has_read()).count();
        let clean = cards.iter().all(|c| !c.has_changes());
        let all_clean = read == cards.len() && clean;
        self.empty_project
            .set_visible(all_clean && shown.mode == Mode::Project);
        self.empty_source
            .set_visible(all_clean && shown.mode == Mode::Source);
        // Not a repository: the single card of Source mode cannot read.
        let none = shown.mode == Mode::Source
            && cards
                .first()
                .is_some_and(|c| c.read_error().is_some());
        self.root
            .set_visible_child_name(if none { "none" } else { "page" });
        if let Some(app) = try_app() {
            let n = if none {
                0
            } else {
                cards.iter().map(|c| c.file_count()).sum()
            };
            app.chrome.right.set_count("changes", n);
        }
    }

    /// Commit is the screen's one primary button (R103.6): the focused
    /// checkout's card while it has changes and no modal surface (palette,
    /// dialog) is over the screen, which then holds the primary (decision
    /// log M33).
    pub fn sync_primary(&self) {
        let Some(app) = try_app() else { return };
        let modal = crate::palette::is_open()
            || crate::project_dialogs::is_open()
            || adw::prelude::AdwApplicationWindowExt::visible_dialog(&app.window).is_some();
        let (fs, fw) = {
            let m = app.model.borrow();
            let (s, w) = m.active();
            (s.id.clone(), w.path.clone())
        };
        for c in self.cards.borrow().iter() {
            c.set_primary(!modal && c.source == fs && rade_core::git::same(&c.path, &fw));
        }
    }

    /// The toggle's icon, the sort items' state, from the config.
    fn update_chrome(&self) {
        let view = current_view();
        match view {
            ScmView::List => {
                self.view_btn.set_icon_name("ph-tree-view-symbolic");
                self.view_btn.set_tooltip_text(Some("View as Tree"));
            }
            ScmView::Tree => {
                self.view_btn.set_icon_name("ph-list-symbolic");
                self.view_btn.set_tooltip_text(Some("View as List"));
            }
        }
        if let Some(a) = self
            .page_actions
            .lookup_action("sort")
            .and_downcast::<gio::SimpleAction>()
        {
            a.set_enabled(view == ScmView::List);
            a.set_state(
                &match current_sort() {
                    ScmSort::Path => "path",
                    ScmSort::Name => "name",
                    ScmSort::Status => "status",
                }
                .to_variant(),
            );
        }
    }

    /// `[scm] view` / `sort` through the Settings writer (comments kept),
    /// then every card from its last read.
    fn write_scm(self: &Rc<Self>, key: &str, value: &str) {
        match crate::settings_view::write_setting(key, Some(toml::Value::String(value.into()))) {
            Ok(config) => {
                crate::keys::apply_config(config);
                self.update_chrome();
                for c in self.cards.borrow().iter() {
                    c.render();
                }
            }
            Err(e) => {
                if let Some(c) = self.cards.borrow().first() {
                    c.show_error(Some(format!("View: {e}")));
                }
            }
        }
    }

    pub(crate) fn set_view(self: &Rc<Self>, v: &str) {
        self.write_scm("scm.view", v);
    }

    pub(crate) fn set_sort(self: &Rc<Self>, v: &str) {
        self.write_scm("scm.sort", v);
    }

    /// Fetch every card's remote.
    pub(crate) fn fetch_all(self: &Rc<Self>) {
        for c in self.cards.borrow().clone() {
            c.fetch(true);
        }
    }

    /// The auto-fetch timer's callback (D14): each card whose last fetch is
    /// older than the period, one at a time, in one background job.
    pub fn autofetch_tick(self: &Rc<Self>) {
        let Some(app) = try_app() else { return };
        let scm_cfg = app.config.borrow().scm.clone();
        if !scm_cfg.autofetch || self.autofetching.get() {
            return;
        }
        let period = Duration::from_secs(u64::from(scm_cfg.autofetch_secs()));
        let now = Instant::now();
        let mut due = Vec::new();
        for c in self.cards.borrow().iter() {
            let last = self.last_fetch.borrow().get(&c.path).copied();
            if last.is_some_and(|t| now.duration_since(t) < period) {
                continue;
            }
            if let Some(snap) = c.snap() {
                due.push((c.path.clone(), snap.status, c.env()));
            }
        }
        if due.is_empty() {
            return;
        }
        for (p, _, _) in &due {
            self.last_fetch.borrow_mut().insert(p.clone(), now);
        }
        self.autofetching.set(true);
        let w = Rc::downgrade(self);
        crate::worktrees::in_background(
            move || {
                due.into_iter()
                    .map(|(p, st, env)| {
                        let r = scm::fetch_remote(&p, &st).and_then(|r| scm::fetch(&p, &r, &env));
                        (p, r.map_err(|e| e.message))
                    })
                    .collect::<Vec<_>>()
            },
            move |results| {
                let Some(c) = w.upgrade() else { return };
                c.autofetching.set(false);
                for (p, r) in results {
                    if let Err(e) = r {
                        if let Some(card) = c.cards.borrow().iter().find(|x| x.path == p) {
                            card.show_error(Some(format!("Fetch: {e}")));
                        }
                    }
                }
                c.refresh();
            },
        );
    }

    /// The card palette actions and Ctrl+Shift+M mean: the active
    /// Worktree's, else the first with changes, else the first.
    fn focused_card(&self) -> Option<Rc<Card>> {
        let cards = self.cards.borrow();
        let app = try_app()?;
        let (fs, fw) = {
            let m = app.model.borrow();
            let (s, w) = m.active();
            (s.id.clone(), w.path.clone())
        };
        cards
            .iter()
            .find(|c| c.source == fs && rade_core::git::same(&c.path, &fw))
            .or_else(|| cards.iter().find(|c| c.has_changes()))
            .or_else(|| cards.first())
            .cloned()
    }

    /// Show the page (opening the Inspector); `focus` the message box.
    pub fn show(self: &Rc<Self>, focus_message: bool) {
        let Some(app) = try_app() else { return };
        if !app.model.borrow().file_tree {
            app.update(|m| m.file_tree = true);
        }
        app.chrome.right.show("changes");
        self.refresh();
        if focus_message {
            if let Some(c) = self.focused_card() {
                c.focus_message();
            }
        }
    }

    /// Palette actions (R21.9, R120, R121).
    pub fn action(self: &Rc<Self>, id: &str) {
        match id {
            "show_changes" => self.show(false),
            "scm_commit" => self.show(true),
            "scm_view_tree" => {
                self.show(false);
                self.set_view("tree");
            }
            "scm_view_list" => {
                self.show(false);
                self.set_view("list");
            }
            _ => {
                self.show(false);
                let Some(c) = self.focused_card() else { return };
                match id {
                    "scm_pull" => c.pull(),
                    "scm_push" => c.push_flow(),
                    "scm_pr" => c.pr_clicked(),
                    "scm_fetch" => c.fetch(true),
                    "scm_sync" => c.sync_flow(),
                    "scm_publish" => c.publish_flow(),
                    "scm_undo_commit" => c.undo(),
                    _ => {}
                }
            }
        }
    }

    fn card(&self, key: &str) -> Result<Rc<Card>> {
        if key.is_empty() {
            return self
                .focused_card()
                .ok_or_else(|| Error::not_found("no Changes card"));
        }
        self.cards
            .borrow()
            .iter()
            .find(|c| c.source == key || c.name == key)
            .cloned()
            .ok_or_else(|| Error::not_found(format!("no Changes card '{key}'")))
    }

    // ---- debug.scm -------------------------------------------------------------------

    fn info(&self) -> Value {
        let cards = self.cards.borrow();
        let shown = self.shown.borrow();
        let page = self.root.visible_child_name().map(|s| s.to_string());
        let mode = match (page.as_deref(), shown.as_ref().map(|s| s.mode)) {
            (Some("none"), _) | (_, None) => "none",
            (_, Some(Mode::Project)) => "project",
            (_, Some(Mode::Source)) => "source",
        };
        let focus = cards.iter().find_map(|c| {
            let (list, msg) = c.focus_in();
            (list || msg).then(|| json!({"source": c.source, "in_list": list, "in_message": msg}))
        });
        let pr = cards.iter().find_map(|c| {
            c.pr_form.borrow().as_ref().map(|f| {
                json!({"title": f.title.text().as_str(), "base": f.base.text().as_str(),
                       "body": crate::changes_card::text_of(&f.body),
                       "note": f.note.label().as_str()})
            })
        });
        json!({
            "mode": mode,
            "header": self.title.label().as_str(),
            "add": self.add.label().as_str(),
            "del": self.del.label().as_str(),
            "view": match current_view() { ScmView::List => "list", ScmView::Tree => "tree" },
            "sort": match current_sort() {
                ScmSort::Path => "path", ScmSort::Name => "name", ScmSort::Status => "status",
            },
            "visible": self.root.is_mapped(),
            "busy": self.refreshing.get() || cards.iter().any(|c| c.busy()),
            "focus": focus,
            "cards": cards.iter().map(|c| c.info()).collect::<Vec<_>>(),
            "pr": pr,
        })
    }
}

/// `debug.scm` (§6 of the M45 plan, E2E): every op drives the widgets' own
/// handlers — clicks, keys, menu items — and answers with `info`. Ops:
/// `info`, `show`, `refresh`, `view`, `sort`, `fetch_all`, `fold`,
/// `group_fold`, `folder_fold`, `click`, `rightclick`, `menu_activate`,
/// `row_action`, `group_action`, `card_menu`, `card_menu_activate`, `key`,
/// `message`, `history`, `generate`, `commit`, `action_button`,
/// `sync_button`, `pr_fill`, `pr_generate`, `autofetch_tick`. Dialogs are
/// answered with `debug.dialog`.
pub fn debug(p: &Value) -> Result<Value> {
    let c = get().ok_or_else(|| Error::internal("no Changes page"))?;
    let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("");
    let group = || -> Result<rade_core::scm_view::Group> {
        parse_group(s("group"))
            .ok_or_else(|| Error::usage("group must be merge, staged or changes"))
    };
    let mut extra: Option<(&str, Value)> = None;
    match s("op") {
        "" | "info" => {}
        "show" => c.show(false),
        "refresh" => c.refresh(),
        "view" => c.set_view(s("arg")),
        "sort" => c.set_sort(s("arg")),
        "fetch_all" => c.fetch_all(),
        "fold" => c.card(s("source"))?.toggle_fold(),
        "group_fold" => c.card(s("source"))?.toggle_group(group()?),
        "folder_fold" => c.card(s("source"))?.toggle_folder(group()?, s("path")),
        "click" => {
            let card = c.card(s("source"))?;
            let g = group()?;
            if !card.has_file(g, s("path")) {
                return Err(Error::not_found(format!("no row {} in {}", s("path"), s("group"))));
            }
            card.click_row(g, s("path"), s("arg"));
        }
        "rightclick" => {
            let card = c.card(s("source"))?;
            let g = group()?;
            let index = card
                .index_of(g, s("path"))
                .ok_or_else(|| Error::not_found(format!("no row {} in {}", s("path"), s("group"))))?;
            extra = Some(("menu", json!(card.right_click(index, None))));
        }
        "menu_activate" => {
            let card = c.card(s("source"))?;
            if !card.ctx_activate(s("item")) {
                return Err(Error::not_found(format!("no menu item '{}'", s("item"))));
            }
        }
        "row_action" => {
            let card = c.card(s("source"))?;
            let b = card
                .find_act(group()?, Some(s("path")), s("arg"))
                .ok_or_else(|| Error::not_found(format!("no row action '{}'", s("arg"))))?;
            b.emit_clicked();
        }
        "group_action" => {
            let card = c.card(s("source"))?;
            let b = card
                .find_act(group()?, None, s("arg"))
                .ok_or_else(|| Error::not_found(format!("no group action '{}'", s("arg"))))?;
            b.emit_clicked();
        }
        "card_menu" => {
            let card = c.card(s("source"))?;
            extra = Some(("menu", json!(card.menu_labels())));
        }
        "card_menu_activate" => {
            let card = c.card(s("source"))?;
            if !card.card_menu_activate(s("item")) {
                return Err(Error::not_found(format!("no card menu item '{}'", s("item"))));
            }
        }
        "key" => {
            let card = c.card(s("source"))?;
            if !card.key(s("key")) {
                return Err(Error::usage(
                    "key must be space, delete, enter, up, down or ctrl+a",
                ));
            }
        }
        "message" => c.card(s("source"))?.message.buffer().set_text(s("text")),
        "history" => {
            let card = c.card(s("source"))?;
            card.history_key(s("arg") == "up");
        }
        "generate" => c.card(s("source"))?.generate(),
        "commit" => {
            let card = c.card(s("source"))?;
            match s("variant") {
                "" | "commit" => card.commit_clicked(),
                v @ ("amend" | "push" | "all" | "undo") => {
                    if !card.commit_variant_enabled(v) {
                        return Err(Error::usage(format!("Commit ({v}) is disabled")));
                    }
                    if v == "undo" {
                        card.undo();
                    } else {
                        card.commit_variant(v);
                    }
                }
                v => return Err(Error::usage(format!("unknown commit variant '{v}'"))),
            }
        }
        "action_button" => c.card(s("source"))?.action_button_clicked(),
        "sync_button" => c.card(s("source"))?.sync_button_clicked(),
        "pr_fill" | "pr_generate" => {
            let cards = c.cards.borrow();
            let card = cards
                .iter()
                .find(|x| x.pr_form.borrow().is_some())
                .ok_or_else(|| Error::usage("the Create PR dialog is not open"))?;
            let f = card.pr_form.borrow();
            let f = f.as_ref().expect("form");
            if s("op") == "pr_generate" {
                f.generate.emit_clicked();
            } else {
                for (k, e) in [("title", &f.title), ("base", &f.base)] {
                    if let Some(v) = p.get(k).and_then(Value::as_str) {
                        e.set_text(v);
                    }
                }
                if let Some(v) = p.get("body").and_then(Value::as_str) {
                    f.body.buffer().set_text(v);
                }
            }
        }
        "autofetch_tick" => c.autofetch_tick(),
        o => return Err(Error::usage(format!("unknown op '{o}'"))),
    }
    let mut info = c.info();
    if let (Some((k, v)), Some(o)) = (extra, info.as_object_mut()) {
        o.insert(k.to_string(), v);
    }
    Ok(info)
}

/// *Approve ▾ → Commit myself…* (R119.4): the Changes page, message focused.
pub(crate) fn commit_myself() {
    if let Some(c) = get() {
        c.action("scm_commit");
    }
}

/// *Approve ▾ → Create PR…* (R119.4): the Create PR dialog.
pub(crate) fn create_pr() {
    if let Some(c) = get() {
        c.action("scm_pr");
    }
}

