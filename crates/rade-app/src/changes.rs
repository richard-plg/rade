//! The Right Panel's Changes page (PRD R21, ADR-0013): the active
//! Worktree's Staged / Changes / Untracked files with stage, unstage and
//! discard per file and per hunk, the commit box with Generate, Pull /
//! Push and Create PR. git and `gh` run off the GTK thread and only on a
//! click (or the palette action standing for it); Push, Discard and Create
//! PR ask first. The git work itself is `rade_core::scm`.

use crate::app::try_app;
use adw::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::diff;
use rade_core::rpc::{Error, Result};
use rade_core::scm::{self, Change, DraftKind, HunkOp, PushPlan, Section, Status, SECTIONS};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Lines of one file's diff shown in the panel; the Review shows the rest.
const MAX_LINES: usize = 400;
/// Rows per list (a build output committed by mistake lists 50k files).
const MAX_ROWS: usize = 500;
const POLL: Duration = Duration::from_secs(3);

type Key = (String, Section);

#[derive(Default)]
struct State {
    root: Option<PathBuf>,
    /// What the list shows: the status (or why there is none) and the
    /// raw diffs of the expanded files.
    shown: Option<(std::result::Result<Status, String>, HashMap<Key, String>)>,
    expanded: HashSet<Key>,
    /// The ListBox rows that are files, by index.
    rows: Vec<Option<Key>>,
}

/// The open Create PR dialog's fields (`debug.scm`).
struct PrForm {
    title: gtk::Entry,
    base: gtk::Entry,
    body: gtk::TextView,
    generate: gtk::Button,
    note: gtk::Label,
}

pub struct Changes {
    pub root: gtk::Stack,
    branch: gtk::Label,
    sync: gtk::Label,
    pull: gtk::Button,
    push: gtk::Button,
    pr: gtk::Button,
    message: gtk::TextView,
    hint: gtk::Label,
    generate: gtk::Button,
    commit: gtk::Button,
    error: gtk::Label,
    spinner: adw::Spinner,
    list: gtk::ListBox,
    scroll: gtk::ScrolledWindow,
    /// The message box and its buttons: hidden while nothing can be committed.
    commit_box: gtk::Box,
    /// The clean checkout's empty state (R103.6).
    clean: gtk::Box,
    state: RefCell<State>,
    /// Bumped when the Worktree changes: older answers are dropped.
    generation: Cell<u64>,
    refreshing: Cell<bool>,
    again: Cell<bool>,
    ops: Cell<u32>,
    drafting: RefCell<Option<Arc<AtomicBool>>>,
    pr_form: RefCell<Option<PrForm>>,
    git_monitor: RefCell<Option<gio::FileMonitor>>,
    panel_shown: Cell<bool>,
    /// The focused checkout is in a Project: Changes by Project (R107.1).
    project_mode: Cell<bool>,
    /// …but the single-checkout page was asked for (`Details`).
    detail: Cell<bool>,
    /// Back from the single-checkout page to the Project.
    back: gtk::Button,
    /// The focused checkout's identity warning above its Commit (R107.5).
    identity: gtk::Label,
}

thread_local! {
    static CHANGES: RefCell<Option<Rc<Changes>>> = const { RefCell::new(None) };
}

/// Re-check Commit's primary state (palette or a dialog opened / closed).
pub fn sync_primary() {
    if let Some(c) = get() {
        c.sync_primary();
    }
}

pub fn get() -> Option<Rc<Changes>> {
    CHANGES.with(|c| c.borrow().clone())
}

fn icon_button(icon: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.add_css_class("flat");
    b.set_tooltip_text(Some(tip));
    b.set_valign(gtk::Align::Center);
    b
}

fn text_of(v: &gtk::TextView) -> String {
    let b = v.buffer();
    b.text(&b.start_iter(), &b.end_iter(), false).to_string()
}

fn git_class(letter: char) -> &'static str {
    match letter {
        'A' => "rade-git-added",
        'D' => "rade-git-deleted",
        'U' | '?' => "rade-git-untracked",
        '!' => "rade-git-conflicted",
        _ => "rade-git-modified",
    }
}

/// Build the page; the Right Panel shows it (R20.10).
pub fn install() -> gtk::Widget {
    let branch = gtk::Label::new(None);
    branch.set_xalign(0.0);
    branch.set_ellipsize(gtk::pango::EllipsizeMode::End);
    branch.set_max_width_chars(24);
    // A short branch name is never cut for "not pushed" (that one ellipsizes).
    branch.set_width_chars(10);
    branch.add_css_class("heading");
    let sync = gtk::Label::new(None);
    sync.add_css_class("rade-meta");
    sync.set_hexpand(true);
    sync.set_xalign(0.0);
    sync.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let pull = icon_button("rade-pull-symbolic", "Pull (git pull --ff-only)");
    let push = icon_button("rade-push-symbolic", "Push…");
    let pr = icon_button("rade-pr-symbolic", "Create Pull Request…");
    let back = icon_button("ph-cube-symbolic", "Back to the Project's changes");
    back.add_css_class("changes-back");
    back.set_visible(false);
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    bar.add_css_class("rade-scm-bar");
    bar.append(&back);
    let glyph = gtk::Image::from_icon_name("rade-branch-symbolic");
    glyph.add_css_class("rade-dim");
    bar.append(&glyph);
    bar.append(&branch);
    bar.append(&sync);
    bar.append(&pull);
    bar.append(&push);
    bar.append(&pr);

    let message = gtk::TextView::new();
    message.set_wrap_mode(gtk::WrapMode::WordChar);
    message.set_accepts_tab(false);
    message.add_css_class("rade-commit-msg");
    message.set_top_margin(6);
    message.set_bottom_margin(6);
    message.set_left_margin(8);
    message.set_right_margin(8);
    let hint = gtk::Label::new(Some("Message (Ctrl+Enter to commit)"));
    hint.add_css_class("rade-dim");
    hint.set_halign(gtk::Align::Start);
    hint.set_valign(gtk::Align::Start);
    hint.set_margin_start(9);
    hint.set_margin_top(6);
    hint.set_can_target(false);
    let msg_scroll = gtk::ScrolledWindow::new();
    msg_scroll.set_child(Some(&message));
    msg_scroll.set_min_content_height(56);
    msg_scroll.set_max_content_height(140);
    msg_scroll.set_propagate_natural_height(true);
    msg_scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    let msg_overlay = gtk::Overlay::new();
    msg_overlay.set_child(Some(&msg_scroll));
    msg_overlay.add_overlay(&hint);
    msg_overlay.add_css_class("rade-commit-box");

    let generate = gtk::Button::new();
    generate.set_child(Some(
        &adw::ButtonContent::builder()
            .icon_name("document-edit-symbolic")
            .label("Generate")
            .build(),
    ));
    generate.add_css_class("flat");
    generate.set_tooltip_text(Some(
        "Ask an Agent to draft the message from the staged diff ([scm] draft_command)",
    ));
    let spinner = adw::Spinner::new();
    spinner.set_visible(false);
    let commit = gtk::Button::with_label("Commit");
    commit.add_css_class("commit-button");
    commit.set_tooltip_text(Some("Commit the staged changes (Ctrl+Enter)"));
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    actions.append(&generate);
    actions.append(&spinner);
    let fill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    fill.set_hexpand(true);
    actions.append(&fill);
    actions.append(&commit);

    let error = gtk::Label::new(None);
    error.add_css_class("rade-scm-error");
    error.set_wrap(true);
    error.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    error.set_xalign(0.0);
    error.set_selectable(true);
    error.set_visible(false);

    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("navigation-sidebar");
    list.add_css_class("rade-scm-list");
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroll.set_child(Some(&list));

    let top = gtk::Box::new(gtk::Orientation::Vertical, 6);
    top.add_css_class("rade-scm-top");
    top.append(&bar);
    let identity = gtk::Label::new(None);
    identity.add_css_class("changes-identity");
    identity.set_wrap(true);
    identity.set_xalign(0.0);
    identity.set_visible(false);
    let commit_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    commit_box.append(&identity);
    commit_box.append(&msg_overlay);
    commit_box.append(&actions);
    top.append(&commit_box);
    top.append(&error);
    let clean = crate::chrome::empty_state(&rade_core::chrome::EMPTY_CHANGES);
    clean.set_visible(false);
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.add_css_class("rade-changes");
    page.append(&top);
    page.append(&clean);
    page.append(&scroll);

    let none = crate::chrome::empty_state(&rade_core::chrome::EMPTY_NO_REPO);
    let root = gtk::Stack::new();
    root.add_named(&page, Some("repo"));
    root.add_named(&none, Some("none"));
    root.add_named(&crate::changes_project::install(), Some("project"));

    let c = Rc::new(Changes {
        root,
        branch,
        sync,
        pull,
        push,
        pr,
        message,
        hint,
        generate,
        commit,
        error,
        spinner,
        list,
        scroll,
        commit_box,
        clean,
        state: RefCell::default(),
        generation: Cell::new(0),
        refreshing: Cell::new(false),
        again: Cell::new(false),
        ops: Cell::new(0),
        drafting: RefCell::default(),
        pr_form: RefCell::default(),
        git_monitor: RefCell::default(),
        panel_shown: Cell::new(false),
        project_mode: Cell::new(false),
        detail: Cell::new(false),
        back,
        identity,
    });
    c.connect();
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
    c.root.clone().upcast()
}

impl Changes {
    fn connect(self: &Rc<Self>) {
        let w = Rc::downgrade(self);
        self.back.connect_clicked(move |_| {
            if let Some(c) = w.upgrade() {
                c.set_detail(false);
            }
        });
        let w = Rc::downgrade(self);
        self.message.buffer().connect_changed(move |_| {
            if let Some(c) = w.upgrade() {
                c.update_buttons();
            }
        });
        let keys = gtk::EventControllerKey::new();
        let w = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, state| {
            let enter = matches!(key, gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter);
            let mods = state
                & (gtk::gdk::ModifierType::CONTROL_MASK
                    | gtk::gdk::ModifierType::SHIFT_MASK
                    | gtk::gdk::ModifierType::ALT_MASK);
            if enter && mods == gtk::gdk::ModifierType::CONTROL_MASK {
                if let Some(c) = w.upgrade() {
                    c.commit_clicked();
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        self.message.add_controller(keys);
        let w = Rc::downgrade(self);
        self.commit.connect_clicked(move |_| {
            if let Some(c) = w.upgrade() {
                c.commit_clicked();
            }
        });
        let w = Rc::downgrade(self);
        self.generate.connect_clicked(move |_| {
            if let Some(c) = w.upgrade() {
                c.generate_commit();
            }
        });
        let w = Rc::downgrade(self);
        self.pull.connect_clicked(move |_| {
            if let Some(c) = w.upgrade() {
                c.pull_clicked();
            }
        });
        let w = Rc::downgrade(self);
        self.push.connect_clicked(move |_| {
            if let Some(c) = w.upgrade() {
                c.push_clicked();
            }
        });
        let w = Rc::downgrade(self);
        self.pr.connect_clicked(move |_| {
            if let Some(c) = w.upgrade() {
                c.pr_clicked();
            }
        });
        let w = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, row| {
            let Some(c) = w.upgrade() else { return };
            let key = c
                .state
                .borrow()
                .rows
                .get(row.index() as usize)
                .cloned()
                .flatten();
            if let Some(k) = key {
                c.toggle(&k);
            }
        });
    }

    /// Whether the page shows the focused Project (R107.1).
    pub fn project_mode(&self) -> bool {
        self.project_mode.get() && !self.detail.get()
    }

    /// `project` (Changes by Project), `checkout` (one checkout's page) or
    /// `none` (not a repository).
    pub fn mode(&self) -> &'static str {
        match self.root.visible_child_name().as_deref() {
            Some("project") => "project",
            Some("none") => "none",
            _ => "checkout",
        }
    }

    /// The single-checkout page (`Details`) or back to the Project.
    pub fn set_detail(self: &Rc<Self>, on: bool) {
        self.detail.set(on);
        self.show_page();
        self.sync_primary();
    }

    /// Every sync: is the focused checkout in a Project?
    pub fn follow_project(self: &Rc<Self>, ws: &rade_core::State) {
        let Some(pc) = crate::changes_project::get() else {
            return;
        };
        // This page shows the focused checkout: its identity warning.
        let warn = crate::chrome::identity_warning(ws);
        self.identity.set_label(warn.as_deref().unwrap_or(""));
        self.identity.set_visible(warn.is_some());
        let was = self.project_mode.get();
        let on = pc.follow(ws);
        if on != was {
            self.detail.set(false);
        }
        self.project_mode.set(on);
        self.show_page();
    }

    /// Which page the stack shows.
    fn show_page(self: &Rc<Self>) {
        self.back.set_visible(self.project_mode.get());
        if self.project_mode() {
            self.root.set_visible_child_name("project");
            if let Some(pc) = crate::changes_project::get() {
                pc.render();
            }
            return;
        }
        let repo = !matches!(self.state.borrow().shown, Some((Err(_), _)));
        self.root
            .set_visible_child_name(if repo { "repo" } else { "none" });
        if let Some(st) = self.status() {
            if let Some(app) = try_app() {
                app.chrome.right.set_count("changes", st.files.len());
            }
        }
    }

    fn root_dir(&self) -> Option<PathBuf> {
        self.state.borrow().root.clone()
    }

    /// Follow the active Worktree (every sync); refresh when it changes or
    /// the Right Panel comes into view (R21.8).
    pub fn follow(self: &Rc<Self>, root: &Path, panel_shown: bool) {
        let was_shown = self.panel_shown.replace(panel_shown);
        if self.state.borrow().root.as_deref() != Some(root) {
            self.generation.set(self.generation.get() + 1);
            {
                let mut st = self.state.borrow_mut();
                st.root = Some(root.to_path_buf());
                st.shown = None;
                st.expanded.clear();
            }
            self.show_error(None);
            self.watch_git(root);
            self.refresh();
        } else if panel_shown && !was_shown {
            self.refresh();
        }
    }

    /// Refresh at once when git's own folder changes (commit, checkout,
    /// an Agent's `git add`). A linked worktree's `.git` is a file naming it.
    fn watch_git(self: &Rc<Self>, root: &Path) {
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
        });
        if let Some(m) = &monitor {
            m.set_rate_limit(500);
            let w = Rc::downgrade(self);
            m.connect_changed(move |_, f, _, _| {
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
        }
        *self.git_monitor.borrow_mut() = monitor;
    }

    /// Re-read the status (and the expanded files' diffs) off the GTK
    /// thread; the list is rebuilt only when something changed.
    pub fn refresh(self: &Rc<Self>) {
        let Some(root) = self.root_dir() else { return };
        if self.refreshing.replace(true) {
            self.again.set(true);
            return;
        }
        let generation = self.generation.get();
        let expanded: Vec<Key> = self.state.borrow().expanded.iter().cloned().collect();
        let w = Rc::downgrade(self);
        crate::worktrees::in_background(
            move || {
                let st = scm::status(&root).map_err(|e| e.message);
                let mut diffs = HashMap::new();
                if let Ok(s) = &st {
                    for (path, sec) in expanded {
                        if let Some(c) =
                            s.files.iter().find(|c| c.path == path && c.in_section(sec))
                        {
                            let d = scm::file_diff(&root, c, sec).unwrap_or_else(|e| e.message);
                            diffs.insert((path, sec), d);
                        }
                    }
                }
                (st, diffs)
            },
            move |(st, diffs)| {
                let Some(c) = w.upgrade() else { return };
                c.refreshing.set(false);
                if c.generation.get() == generation {
                    c.apply(st, diffs);
                }
                if c.again.take() {
                    c.refresh();
                }
            },
        );
    }

    fn apply(
        self: &Rc<Self>,
        st: std::result::Result<Status, String>,
        diffs: HashMap<Key, String>,
    ) {
        let same = self
            .state
            .borrow()
            .shown
            .as_ref()
            .is_some_and(|(s, d)| *s == st && *d == diffs);
        if same {
            return;
        }
        // Expanded files that are gone close.
        if let Ok(s) = &st {
            self.state
                .borrow_mut()
                .expanded
                .retain(|(p, sec)| s.files.iter().any(|c| &c.path == p && c.in_section(*sec)));
        }
        self.state.borrow_mut().shown = Some((st, diffs));
        self.render();
    }

    fn status(&self) -> Option<Status> {
        match &self.state.borrow().shown {
            Some((Ok(s), _)) => Some(s.clone()),
            _ => None,
        }
    }

    fn render(self: &Rc<Self>) {
        let (st, diffs) = match &self.state.borrow().shown {
            Some((s, d)) => (s.clone(), d.clone()),
            None => return,
        };
        let Ok(st) = st else {
            if !self.project_mode() {
                self.root.set_visible_child_name("none");
                if let Some(app) = try_app() {
                    app.chrome.right.set_count("changes", 0);
                }
            }
            return;
        };
        if self.project_mode() {
            self.root.set_visible_child_name("project");
        } else {
            self.root.set_visible_child_name("repo");
            if let Some(app) = try_app() {
                app.chrome.right.set_count("changes", st.files.len());
            }
        }
        self.branch
            .set_label(st.branch.as_deref().unwrap_or("(detached)"));
        self.branch.set_tooltip_text(Some(&match &st.upstream {
            Some(u) => format!("Upstream: {u}"),
            None => "No upstream: the first Push sets one".into(),
        }));
        self.sync
            .set_label(&match (&st.upstream, st.own_upstream()) {
                (Some(_), Some(_)) => format!("↑{} ↓{}", st.ahead, st.behind),
                _ => "not pushed".into(),
            });

        let vadj = self.scroll.vadjustment();
        let pos = vadj.value();
        while let Some(r) = self.list.first_child() {
            self.list.remove(&r);
        }
        let mut rows: Vec<Option<Key>> = Vec::new();
        let expanded = self.state.borrow().expanded.clone();
        for sec in SECTIONS {
            let items = st.section(sec);
            if items.is_empty() {
                continue;
            }
            self.list.append(&self.section_row(sec, &items));
            rows.push(None);
            let more = items.len().saturating_sub(MAX_ROWS);
            for c in items.into_iter().take(MAX_ROWS) {
                let key = (c.path.clone(), sec);
                let open = expanded.contains(&key);
                self.list.append(&self.file_row(c, sec, open));
                rows.push(Some(key.clone()));
                if open {
                    if let Some(raw) = diffs.get(&key) {
                        self.list.append(&self.diff_row(c, sec, raw));
                        rows.push(None);
                    }
                }
            }
            if more > 0 {
                let l = gtk::Label::new(Some(&format!("… and {more} more")));
                l.add_css_class("rade-meta");
                l.set_xalign(0.0);
                l.set_margin_start(28);
                self.list.append(&plain_row(&l));
                rows.push(None);
            }
        }
        // Clean: one line and key hints instead of the commit box (R103.6).
        let clean = st.files.is_empty();
        self.clean.set_visible(clean);
        self.commit_box.set_visible(!clean);
        self.scroll.set_visible(!clean);
        self.state.borrow_mut().rows = rows;
        // Keep the scroll position across the rebuild.
        glib::idle_add_local_once(move || vadj.set_value(pos));
        self.update_buttons();
    }

    fn section_row(self: &Rc<Self>, sec: Section, items: &[&Change]) -> gtk::ListBoxRow {
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let title = gtk::Label::new(Some(sec.title()));
        title.add_css_class("rade-scm-section");
        let n = gtk::Label::new(Some(&items.len().to_string()));
        n.add_css_class("rade-meta");
        n.set_hexpand(true);
        n.set_xalign(0.0);
        b.append(&title);
        b.append(&n);
        let paths: Vec<String> = items.iter().flat_map(|c| scm::paths_of(c)).collect();
        let (icon, tip) = match sec {
            Section::Staged => ("list-remove-symbolic", "Unstage All"),
            _ => ("list-add-symbolic", "Stage All"),
        };
        let all = icon_button(icon, tip);
        let w = Rc::downgrade(self);
        all.connect_clicked(move |_| {
            if let Some(c) = w.upgrade() {
                c.stage_paths(paths.clone(), sec != Section::Staged);
            }
        });
        b.append(&all);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&b));
        row.set_activatable(false);
        row.add_css_class("rade-scm-head");
        row
    }

    fn file_row(self: &Rc<Self>, c: &Change, sec: Section, open: bool) -> gtk::ListBoxRow {
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let chevron = gtk::Image::from_icon_name(if open {
            "pan-down-symbolic"
        } else {
            "pan-end-symbolic"
        });
        chevron.add_css_class("rade-dim");
        let letter = c.letter(sec);
        let mark = gtk::Label::new(Some(&letter.to_string()));
        mark.add_css_class("rade-git-mark");
        mark.add_css_class(git_class(letter));
        let (dir, name) = match c.path.rsplit_once('/') {
            Some((d, n)) => (d.to_string(), n.to_string()),
            None => (String::new(), c.path.clone()),
        };
        let name_l = gtk::Label::new(Some(&name));
        name_l.set_xalign(0.0);
        name_l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        name_l.add_css_class(git_class(letter));
        if letter == 'D' {
            name_l.add_css_class("rade-strike");
        }
        let dir_l = gtk::Label::new(Some(&dir));
        dir_l.set_xalign(0.0);
        dir_l.set_hexpand(true);
        dir_l.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        dir_l.add_css_class("rade-meta");
        b.append(&chevron);
        b.append(&name_l);
        b.append(&dir_l);
        b.append(&mark);
        let tip = match &c.orig {
            Some(o) => format!("{o} → {}", c.path),
            None => c.path.clone(),
        };
        b.set_tooltip_text(Some(&tip));
        let key = (c.path.clone(), sec);
        let w = Rc::downgrade(self);
        let add = |icon: &str, tip: &str, op: &'static str| {
            let btn = icon_button(icon, tip);
            btn.add_css_class("rade-scm-act");
            let (w, key) = (w.clone(), key.clone());
            btn.connect_clicked(move |_| {
                if let Some(c) = w.upgrade() {
                    c.file_op(&key, op);
                }
            });
            b.append(&btn);
        };
        match sec {
            Section::Staged => add("list-remove-symbolic", "Unstage", "unstage"),
            Section::Unstaged => {
                add("edit-undo-symbolic", "Discard Changes…", "discard");
                add("list-add-symbolic", "Stage", "stage");
            }
            Section::Untracked => {
                add("user-trash-symbolic", "Delete File…", "discard");
                add("list-add-symbolic", "Stage", "stage");
            }
        }
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&b));
        row.add_css_class("rade-scm-file");
        row
    }

    fn diff_row(self: &Rc<Self>, c: &Change, sec: Section, raw: &str) -> gtk::ListBoxRow {
        let v = gtk::Box::new(gtk::Orientation::Vertical, 0);
        v.add_css_class("rade-scm-diff");
        let tools = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        tools.set_halign(gtk::Align::End);
        let open = gtk::Button::new();
        open.set_child(Some(
            &adw::ButtonContent::builder()
                .icon_name("document-open-symbolic")
                .label("Open File")
                .build(),
        ));
        open.add_css_class("flat");
        open.add_css_class("rade-scm-small");
        let path = self.root_dir().unwrap_or_default().join(&c.path);
        open.connect_clicked(move |_| crate::files::open_in_viewer(&path));
        tools.append(&open);
        let review = gtk::Button::new();
        review.set_child(Some(
            &adw::ButtonContent::builder()
                .icon_name("rade-review-symbolic")
                .label("Review")
                .build(),
        ));
        review.add_css_class("flat");
        review.add_css_class("rade-scm-small");
        review.set_tooltip_text(Some("The Worktree's full diff, full width (Ctrl+Shift+R)"));
        review.connect_clicked(|_| crate::keys::run_action("open_review"));
        tools.append(&review);
        v.append(&tools);
        let files = diff::parse(raw, &[]);
        let patches = scm::split_patch(raw);
        let Some(f) = files.first() else {
            let l = gtk::Label::new(Some(if raw.starts_with("git ") {
                raw
            } else {
                "No textual changes (mode or empty file)"
            }));
            l.add_css_class("rade-meta");
            l.set_wrap(true);
            l.set_xalign(0.0);
            v.append(&l);
            return plain_row(&v);
        };
        if f.binary {
            let l = gtk::Label::new(Some("Binary file"));
            l.add_css_class("rade-meta");
            v.append(&l);
            return plain_row(&v);
        }
        let per_hunk = scm::hunks_apply(f) && sec != Section::Untracked;
        let mut shown = 0;
        for (hi, h) in f.hunks.iter().enumerate() {
            if shown >= MAX_LINES {
                let more = gtk::Label::new(Some("… more in the Review"));
                more.add_css_class("rade-meta");
                v.append(&more);
                break;
            }
            let head = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            head.add_css_class("rade-diff-hunk");
            let hl = gtk::Label::new(Some(&h.header));
            hl.set_xalign(0.0);
            hl.set_hexpand(true);
            hl.set_ellipsize(gtk::pango::EllipsizeMode::End);
            hl.add_css_class("rade-scm-mono");
            head.append(&hl);
            if per_hunk {
                let patch = patches
                    .first()
                    .and_then(|p| scm::hunk_patch(p, hi))
                    .unwrap_or_default();
                let add = |icon: &str, tip: &str, op: HunkOp| {
                    let b = icon_button(icon, tip);
                    b.add_css_class("rade-scm-act");
                    let (w, patch) = (Rc::downgrade(self), patch.clone());
                    b.connect_clicked(move |_| {
                        if let Some(c) = w.upgrade() {
                            c.hunk_op(patch.clone(), op);
                        }
                    });
                    head.append(&b);
                };
                match sec {
                    Section::Staged => add("list-remove-symbolic", "Unstage Hunk", HunkOp::Unstage),
                    _ => {
                        add("edit-undo-symbolic", "Discard Hunk…", HunkOp::Discard);
                        add("list-add-symbolic", "Stage Hunk", HunkOp::Stage);
                    }
                }
            }
            v.append(&head);
            for l in h.lines.iter().take(MAX_LINES.saturating_sub(shown)) {
                let (sign, class) = match l.kind {
                    diff::LineKind::Added => ('+', Some("rade-diff-added")),
                    diff::LineKind::Removed => ('-', Some("rade-diff-removed")),
                    diff::LineKind::Context => (' ', None),
                };
                let t = gtk::Label::new(Some(&format!("{sign}{}", l.text)));
                t.set_xalign(0.0);
                t.set_ellipsize(gtk::pango::EllipsizeMode::End);
                t.add_css_class("rade-scm-mono");
                if let Some(c) = class {
                    t.add_css_class(c);
                }
                v.append(&t);
            }
            shown += h.lines.len();
        }
        plain_row(&v)
    }

    fn toggle(self: &Rc<Self>, key: &Key) {
        {
            let mut st = self.state.borrow_mut();
            if !st.expanded.remove(key) {
                st.expanded.insert(key.clone());
            }
            // Force the rebuild with the diff.
            st.shown = st.shown.take().map(|(s, mut d)| {
                d.remove(key);
                (s, d)
            });
        }
        self.render();
        self.refresh();
    }

    /// Commit is the screen's one primary button (R103.6) while there is
    /// something to commit and no modal surface (palette, dialog) is over
    /// the screen, which then holds the primary (decision log M33).
    pub fn sync_primary(&self) {
        // Changes by Project draws its own primary Commit (one per page).
        if let Some(pc) = crate::changes_project::get() {
            pc.repaint();
        }
        let committable =
            self.status().is_some_and(|s| !s.files.is_empty()) && !self.project_mode();
        let modal = crate::palette::is_open()
            || crate::project_dialogs::is_open()
            || try_app().is_some_and(|app| {
                libadwaita::prelude::AdwApplicationWindowExt::visible_dialog(&app.window).is_some()
            });
        if committable && !modal {
            self.commit.add_css_class("suggested-action");
        } else {
            self.commit.remove_css_class("suggested-action");
        }
    }

    fn update_buttons(&self) {
        let st = self.status();
        let staged = st
            .as_ref()
            .is_some_and(|s| !s.section(Section::Staged).is_empty());
        let msg = !text_of(&self.message).trim().is_empty();
        self.hint
            .set_visible(self.message.buffer().char_count() == 0);
        self.commit
            .set_sensitive(staged && msg && self.ops.get() == 0);
        self.sync_primary();
        let branch = st.as_ref().is_some_and(|s| s.branch.is_some());
        self.push.set_sensitive(branch);
        self.pr.set_sensitive(branch);
        let upstream = st.as_ref().is_some_and(|s| s.upstream.is_some());
        self.pull.set_sensitive(upstream);
        self.pull.set_tooltip_text(Some(if upstream {
            "Pull (git pull --ff-only)"
        } else {
            "Pull: no upstream yet (Push sets one)"
        }));
        let drafting = self.drafting.borrow().is_some();
        if let Some(c) = self.generate.child().and_downcast::<adw::ButtonContent>() {
            c.set_label(if drafting { "Cancel" } else { "Generate" });
        }
        self.spinner.set_visible(self.ops.get() > 0 || drafting);
    }

    fn show_error(&self, e: Option<String>) {
        self.error.set_visible(e.is_some());
        self.error.set_label(e.as_deref().unwrap_or(""));
    }

    /// Run git work off the GTK thread, then `ok` with its result; an
    /// error shows in the page (R21.8). The list refreshes either way.
    /// The Workspace env for this Worktree's push / pull / gh / Draft
    /// (R23.3).
    fn env(&self) -> Vec<(String, String)> {
        match (self.root_dir(), try_app()) {
            (Some(root), Some(app)) => crate::app::workspace_env_at(&app.model.borrow(), &root),
            _ => Vec::new(),
        }
    }

    fn run<T: Send + 'static>(
        self: &Rc<Self>,
        what: &'static str,
        work: impl FnOnce(PathBuf) -> Result<T> + Send + 'static,
        ok: impl FnOnce(&Rc<Self>, T) + 'static,
    ) {
        let Some(root) = self.root_dir() else { return };
        self.show_error(None);
        self.ops.set(self.ops.get() + 1);
        self.update_buttons();
        let w = Rc::downgrade(self);
        crate::worktrees::in_background(
            move || work(root),
            move |r| {
                let Some(c) = w.upgrade() else { return };
                c.ops.set(c.ops.get() - 1);
                match r {
                    Ok(v) => ok(&c, v),
                    Err(e) => c.show_error(Some(format!("{what}: {}", e.message))),
                }
                c.update_buttons();
                c.refresh();
            },
        );
    }

    fn change(&self, key: &Key) -> Option<(Change, bool)> {
        let st = self.status()?;
        let c = st
            .files
            .iter()
            .find(|c| c.path == key.0 && c.in_section(key.1))?;
        Some((c.clone(), st.initial))
    }

    fn stage_paths(self: &Rc<Self>, paths: Vec<String>, stage: bool) {
        let initial = self.status().is_some_and(|s| s.initial);
        if stage {
            self.run("Stage", move |r| scm::stage(&r, &paths), |_, _| ());
        } else {
            self.run(
                "Unstage",
                move |r| scm::unstage(&r, &paths, initial),
                |_, _| (),
            );
        }
    }

    /// A file row's button (R21.2): stage, unstage, discard (asks).
    fn file_op(self: &Rc<Self>, key: &Key, op: &str) {
        let Some((c, _)) = self.change(key) else {
            return;
        };
        match op {
            "stage" => self.stage_paths(scm::paths_of(&c), true),
            "unstage" => self.stage_paths(scm::paths_of(&c), false),
            _ => {
                let (heading, body) = if c.untracked() {
                    (
                        format!("Delete “{}”?", c.path),
                        "The file is not tracked by git: deleting it can't be undone.".to_string(),
                    )
                } else {
                    (
                        format!("Discard changes in “{}”?", c.path),
                        "Its unstaged changes are lost; staged changes stay.".to_string(),
                    )
                };
                let w = Rc::downgrade(self);
                confirm(
                    &heading,
                    &body,
                    "discard",
                    if c.untracked() { "Delete" } else { "Discard" },
                    move || {
                        if let Some(me) = w.upgrade() {
                            let c = c.clone();
                            me.run("Discard", move |r| scm::discard(&r, &c), |_, _| ());
                        }
                    },
                );
            }
        }
    }

    /// A hunk's button (R21.3); discard asks first.
    fn hunk_op(self: &Rc<Self>, patch: String, op: HunkOp) {
        let go = {
            let w = Rc::downgrade(self);
            move |patch: String| {
                if let Some(me) = w.upgrade() {
                    me.run("Hunk", move |r| scm::apply_hunk(&r, &patch, op), |_, _| ());
                }
            }
        };
        if op == HunkOp::Discard {
            confirm(
                "Discard this hunk?",
                "Its changes are removed from the working tree.",
                "discard",
                "Discard",
                move || go(patch.clone()),
            );
        } else {
            go(patch);
        }
    }

    fn commit_clicked(self: &Rc<Self>) {
        let msg = text_of(&self.message);
        if !self.commit.is_sensitive() {
            if msg.trim().is_empty() {
                self.show_error(Some(
                    "Commit: write a message first (or Generate one)".into(),
                ));
            } else if self.ops.get() == 0 {
                self.show_error(Some("Commit: nothing is staged".into()));
            }
            return;
        }
        self.run(
            "Commit",
            move |r| scm::commit(&r, &msg),
            |c, done| {
                c.message.buffer().set_text("");
                if let Some(root) = c.root_dir() {
                    let (short, subject) = done.split_once(' ').unwrap_or((&done, ""));
                    let source = crate::app::app()
                        .model
                        .borrow()
                        .worktrees()
                        .find(|(_, w)| rade_core::git::same(&w.path, &root))
                        .map(|(s, _)| s.name.clone())
                        .unwrap_or_default();
                    crate::activity_view::committed(&root, &source, short, subject);
                }
                toast(&format!(
                    "Committed <b>{}</b>",
                    glib::markup_escape_text(&done)
                ));
            },
        );
    }

    /// Generate (R21.7): the Draft command's reply fills `done`; a second
    /// click cancels it.
    fn draft(self: &Rc<Self>, kind: DraftKind, done: impl FnOnce(String, String) + 'static) {
        if let Some(flag) = self.drafting.borrow_mut().take() {
            flag.store(true, Ordering::Relaxed);
            self.update_buttons();
            return;
        }
        let Some(root) = self.root_dir() else { return };
        let Some(app) = try_app() else { return };
        // The Worktree's layered `[scm] draft_command` (R29.2).
        let at = app
            .model
            .borrow()
            .worktrees()
            .find(|(_, w)| w.path == root)
            .map(|(s, w)| (s.id.clone(), w.id.clone()));
        let command = match &at {
            Some((s, w)) => app.config_for(s, Some(w)).scm.draft_command,
            None => app.config.borrow().scm.draft_command.clone(),
        };
        let wt_base = app.model.borrow().comparison_base(&root);
        let env = self.env();
        let cancel = Arc::new(AtomicBool::new(false));
        *self.drafting.borrow_mut() = Some(cancel.clone());
        self.show_error(None);
        self.update_buttons();
        let w = Rc::downgrade(self);
        crate::worktrees::in_background(
            move || {
                let base = wt_base.or_else(|| rade_core::git::default_branch(&root));
                let input = scm::gather_draft(&root, kind, base.as_deref())?;
                let prompt = scm::draft_prompt(kind, &input);
                scm::run_draft(&command, &root, &prompt, scm::DRAFT_TIMEOUT, &cancel, &env)
                    .map(|out| scm::parse_draft(kind, &out))
            },
            move |r: Result<(String, String)>| {
                let Some(c) = w.upgrade() else { return };
                c.drafting.borrow_mut().take();
                c.update_buttons();
                match r {
                    Ok((a, b)) => done(a, b),
                    Err(e) => c.show_error(Some(format!("Generate: {}", e.message))),
                }
            },
        );
    }

    fn generate_commit(self: &Rc<Self>) {
        let w = Rc::downgrade(self);
        self.draft(DraftKind::Commit, move |msg, _| {
            if let Some(c) = w.upgrade() {
                c.message.buffer().set_text(&msg);
                c.message.grab_focus();
            }
        });
    }

    fn pull_clicked(self: &Rc<Self>) {
        let env = self.env();
        self.run("Pull", move |r| scm::pull(&r, &env), |_, _| toast("Pulled"));
    }

    fn push_clicked(self: &Rc<Self>) {
        self.run(
            "Push",
            |r| scm::push_plan(&r, &scm::status(&r)?),
            |c, plan| c.confirm_push(plan),
        );
    }

    fn confirm_push(self: &Rc<Self>, plan: PushPlan) {
        let n = plan.commits;
        let mut body = format!(
            "{n} commit{} on {} → {}/{}.",
            if n == 1 { "" } else { "s" },
            plan.branch,
            plan.remote,
            plan.branch
        );
        if plan.set_upstream {
            body.push_str(&format!(
                "\nSets {}/{} as the branch's upstream.",
                plan.remote, plan.branch
            ));
        }
        let w = Rc::downgrade(self);
        confirm(
            &format!("Push to {}?", plan.remote),
            &body,
            "push",
            "Push",
            move || {
                let Some(me) = w.upgrade() else { return };
                let plan = plan.clone();
                let env = me.env();
                me.run(
                    "Push",
                    move |r| scm::push(&r, &plan, &env).map(|_| plan),
                    |_, p| {
                        toast(&format!(
                            "Pushed <b>{}</b> to {}",
                            glib::markup_escape_text(&p.branch),
                            glib::markup_escape_text(&p.remote)
                        ))
                    },
                );
            },
        );
    }

    fn pr_clicked(self: &Rc<Self>) {
        // A Worktree's recorded base (R28.3) is its PR's base.
        let wt_base = self
            .root_dir()
            .zip(try_app())
            .and_then(|(r, a)| a.model.borrow().comparison_base(&r));
        self.run(
            "Create PR",
            |r| {
                let st = scm::status(&r)?;
                let branch = st
                    .branch
                    .clone()
                    .ok_or_else(|| Error::usage("HEAD is detached: check out a branch"))?;
                let base = scm::pr_base(
                    wt_base
                        // A Fan-out Worktree's base is a commit: not a PR base.
                        .filter(|b| !(b.len() >= 7 && b.bytes().all(|c| c.is_ascii_hexdigit())))
                        .or_else(|| rade_core::git::default_branch(&r))
                        .as_deref(),
                );
                let plan = if st.own_upstream().is_none() || st.ahead > 0 {
                    Some(scm::push_plan(&r, &st)?)
                } else {
                    None
                };
                Ok((branch, base, plan))
            },
            |c, (branch, base, plan)| c.pr_dialog(branch, base, plan),
        );
    }

    /// Create PR (R21.6): the form is the confirmation.
    fn pr_dialog(self: &Rc<Self>, branch: String, base: String, plan: Option<PushPlan>) {
        let title = gtk::Entry::new();
        title.set_placeholder_text(Some("Title"));
        title.set_activates_default(true);
        let base_e = gtk::Entry::new();
        base_e.set_text(&base);
        let base_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let bl = gtk::Label::new(Some(&format!("{branch} →")));
        bl.add_css_class("rade-dim");
        base_e.set_hexpand(true);
        base_e.set_tooltip_text(Some("Base branch"));
        base_row.append(&bl);
        base_row.append(&base_e);
        let body = gtk::TextView::new();
        body.set_wrap_mode(gtk::WrapMode::WordChar);
        body.set_top_margin(6);
        body.set_left_margin(6);
        body.set_right_margin(6);
        body.add_css_class("rade-commit-msg");
        let bs = gtk::ScrolledWindow::new();
        bs.set_child(Some(&body));
        bs.set_min_content_height(160);
        bs.set_min_content_width(420);
        bs.add_css_class("rade-commit-box");
        let generate = gtk::Button::new();
        generate.set_child(Some(
            &adw::ButtonContent::builder()
                .icon_name("document-edit-symbolic")
                .label("Generate")
                .build(),
        ));
        generate.set_halign(gtk::Align::Start);
        generate.set_tooltip_text(Some(
            "Ask an Agent to draft the title and description from the branch's diff",
        ));
        let note = gtk::Label::new(Some(&match &plan {
            Some(p) if p.set_upstream => format!(
                "Pushes {branch} to {} first ({} commit{}, sets its upstream).",
                p.remote,
                p.commits,
                if p.commits == 1 { "" } else { "s" }
            ),
            Some(p) => format!(
                "Pushes {} commit{} to {} first.",
                p.commits,
                if p.commits == 1 { "" } else { "s" },
                p.remote
            ),
            None => "The branch is pushed.".into(),
        }));
        note.add_css_class("rade-meta");
        note.set_wrap(true);
        note.set_xalign(0.0);
        let label = |t: &str| {
            let l = gtk::Label::new(Some(t));
            l.add_css_class("rade-meta");
            l.set_xalign(0.0);
            l
        };
        let form = gtk::Box::new(gtk::Orientation::Vertical, 6);
        form.append(&label("Title"));
        form.append(&title);
        form.append(&label("Base branch"));
        form.append(&base_row);
        form.append(&label("Description"));
        form.append(&bs);
        form.append(&generate);
        form.append(&note);
        let w = Rc::downgrade(self);
        let (t2, b2) = (title.clone(), body.clone());
        generate.connect_clicked(move |_| {
            let Some(c) = w.upgrade() else { return };
            let (t, b) = (t2.clone(), b2.clone());
            c.draft(DraftKind::Pr, move |title, body| {
                t.set_text(&title);
                b.buffer().set_text(&body);
            });
        });
        let d = adw::AlertDialog::new(Some("Create Pull Request"), None);
        d.set_extra_child(Some(&form));
        d.add_response("cancel", "Cancel");
        d.add_response("create", "Create");
        d.set_response_appearance("create", adw::ResponseAppearance::Suggested);
        d.set_default_response(Some("create"));
        d.set_close_response("cancel");
        let w = Rc::downgrade(self);
        let (t3, b3, base3) = (title.clone(), body.clone(), base_e.clone());
        d.connect_response(None, move |_, r| {
            let Some(c) = w.upgrade() else { return };
            c.pr_form.borrow_mut().take();
            if r != "create" {
                return;
            }
            let (title, body, base) = (
                t3.text().to_string(),
                text_of(&b3),
                base3.text().to_string(),
            );
            let (branch, plan) = (branch.clone(), plan.clone());
            let env = c.env();
            c.run(
                "Create PR",
                move |r| {
                    if let Some(p) = &plan {
                        scm::push(&r, p, &env)?;
                    }
                    scm::create_pr(&r, "gh", &branch, base.trim(), &title, &body, &env)
                },
                move |c, url| {
                    if let Some(root) = c.root_dir() {
                        crate::activity_view::pr_created(&root, &url);
                    }
                    let u = url.clone();
                    crate::app::app().chrome.toast(
                        &format!("Pull request created: {}", glib::markup_escape_text(&url)),
                        Some(("Open", Box::new(move || crate::viewer::launch_uri(&u)))),
                    );
                },
            );
        });
        *self.pr_form.borrow_mut() = Some(PrForm {
            title: title.clone(),
            base: base_e,
            body,
            generate,
            note,
        });
        crate::menus::present(d, &["cancel", "create"]);
        title.grab_focus();
    }

    /// Show the page (opening the Right Panel); `focus` the message box.
    pub fn show(self: &Rc<Self>, focus_message: bool) {
        let Some(app) = try_app() else { return };
        if !app.model.borrow().file_tree {
            app.update(|m| m.file_tree = true);
        }
        app.chrome.right.show("changes");
        self.refresh();
        if focus_message {
            self.message.grab_focus();
        }
    }

    /// Palette actions (R21.9).
    pub fn action(self: &Rc<Self>, id: &str) {
        match id {
            "show_changes" => self.show(false),
            "scm_commit" => self.show(true),
            "scm_pull" => {
                self.show(false);
                self.pull_clicked();
            }
            "scm_push" => {
                self.show(false);
                self.push_clicked();
            }
            "scm_pr" => {
                self.show(false);
                self.pr_clicked();
            }
            _ => {}
        }
    }

    fn info(&self) -> Value {
        let st = self.state.borrow();
        let (status, error) = match &st.shown {
            Some((Ok(s), _)) => (Some(s), None),
            Some((Err(e), _)) => (None, Some(e.clone())),
            None => (None, None),
        };
        let sections: serde_json::Map<String, Value> = SECTIONS
            .iter()
            .map(|sec| {
                let rows: Vec<Value> = status.map_or(vec![], |s| {
                    s.section(*sec)
                        .iter()
                        .map(|c| json!({"path": c.path, "letter": c.letter(*sec).to_string()}))
                        .collect()
                });
                (
                    serde_json::to_value(sec)
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_string(),
                    json!(rows),
                )
            })
            .collect();
        let diffs: Vec<Value> = st
            .shown
            .iter()
            .flat_map(|(_, d)| d.iter())
            .map(|((p, s), raw)| {
                let f = diff::parse(raw, &[]);
                let hunks: Vec<&str> = f.first().map_or(vec![], |f| f.hunks.iter().map(|h| h.header.as_str()).collect());
                json!({"path": p, "section": s, "hunks": hunks, "per_hunk": f.first().is_some_and(|f| scm::hunks_apply(f) && *s != Section::Untracked)})
            })
            .collect();
        let pr = self.pr_form.borrow().as_ref().map(|f| {
            json!({"title": f.title.text().as_str(), "base": f.base.text().as_str(),
                   "body": text_of(&f.body), "note": f.note.label().as_str()})
        });
        json!({
            "root": st.root,
            "page": self.root.visible_child_name().map(|s| s.to_string()),
            "repo_error": error,
            "branch": status.and_then(|s| s.branch.clone()),
            "upstream": status.and_then(|s| s.upstream.clone()),
            "sync": self.sync.label().as_str(),
            "ahead": status.map(|s| s.ahead), "behind": status.map(|s| s.behind),
            "sections": sections,
            "expanded": diffs,
            "message": text_of(&self.message),
            "commit_enabled": self.commit.is_sensitive(),
            // Set for the focused checkout (drawn while the commit area is).
            "identity": self.identity.get_visible().then(|| self.identity.label().to_string()),
            "error": self.error.is_visible().then(|| self.error.label().to_string()),
            "busy": self.ops.get() > 0 || self.refreshing.get(),
            "drafting": self.drafting.borrow().is_some(),
            "pr": pr,
            "visible": self.root.is_mapped(),
            "focus_in_message": self.message.has_focus(),
        })
    }
}

fn plain_row(w: &impl IsA<gtk::Widget>) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(w));
    row.set_activatable(false);
    row.set_selectable(false);
    row.add_css_class("rade-scm-diffrow");
    row
}

fn toast(markup: &str) {
    if let Some(app) = try_app() {
        app.chrome.toast(markup, None);
    }
}

/// Ask before a destructive or publishing action (ADR-0013).
fn confirm(heading: &str, body: &str, id: &'static str, label: &str, yes: impl Fn() + 'static) {
    let d = adw::AlertDialog::new(Some(heading), Some(body));
    d.add_response("cancel", "Cancel");
    d.add_response(id, label);
    d.set_response_appearance(
        id,
        if id == "discard" {
            adw::ResponseAppearance::Destructive
        } else {
            adw::ResponseAppearance::Suggested
        },
    );
    d.set_default_response(Some("cancel"));
    d.set_close_response("cancel");
    d.connect_response(None, move |_, r| {
        if r == id {
            yes();
        }
    });
    crate::menus::present(d, &["cancel", id]);
}

/// `debug.scm` (`{"op", "path"?, "section"?, "hunk"?, "text"?, …}`, E2E):
/// drives the page as clicks do. Ops: `info`, `show`, `refresh`, `click`
/// (a file row), `stage` / `unstage` / `discard` (its buttons), `stage_all`
/// / `unstage_all` (`section`), `hunk` (`hunk` index, `action` stage /
/// unstage / discard), `message` (`text`), `generate`, `commit`, `pull`,
/// `push`, `pr`, `pr_fill` (`title`, `body`, `base`), `pr_generate`,
/// `open` (Open File). Dialogs are answered with `debug.dialog`.
pub fn debug(p: &Value) -> Result<Value> {
    let c = get().ok_or_else(|| Error::internal("no Changes page"))?;
    let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("");
    let section = || -> Result<Section> {
        serde_json::from_value(json!(s("section")))
            .map_err(|_| Error::usage("section must be staged, unstaged or untracked"))
    };
    let key = || -> Result<Key> { Ok((s("path").to_string(), section()?)) };
    match s("op") {
        "" | "info" => {}
        "show" => c.show(false),
        "refresh" => c.refresh(),
        "click" => c.toggle(&key()?),
        "stage" | "unstage" | "discard" => {
            let k = key()?;
            if c.change(&k).is_none() {
                return Err(Error::not_found(format!("no row {} in {:?}", k.0, k.1)));
            }
            c.file_op(&k, s("op"));
        }
        "stage_all" | "unstage_all" => {
            let sec = section()?;
            let paths: Vec<String> = c
                .status()
                .map(|st| {
                    st.section(sec)
                        .iter()
                        .flat_map(|c| scm::paths_of(c))
                        .collect()
                })
                .unwrap_or_default();
            c.stage_paths(paths, s("op") == "stage_all");
        }
        "hunk" => {
            let k = key()?;
            let raw = c
                .state
                .borrow()
                .shown
                .as_ref()
                .and_then(|(_, d)| d.get(&k).cloned())
                .ok_or_else(|| Error::usage("click the file first (its diff is not shown)"))?;
            let i = p.get("hunk").and_then(Value::as_u64).unwrap_or(0) as usize;
            let patch = scm::split_patch(&raw)
                .first()
                .and_then(|f| scm::hunk_patch(f, i))
                .ok_or_else(|| Error::not_found(format!("no hunk {i}")))?;
            let op = match s("action") {
                "stage" => HunkOp::Stage,
                "unstage" => HunkOp::Unstage,
                "discard" => HunkOp::Discard,
                a => return Err(Error::usage(format!("unknown hunk action '{a}'"))),
            };
            c.hunk_op(patch, op);
        }
        "message" => c.message.buffer().set_text(s("text")),
        "generate" => c.generate.emit_clicked(),
        "commit" => c.commit_clicked(),
        "pull" => c.pull.emit_clicked(),
        "push" => c.push.emit_clicked(),
        "pr" => c.pr.emit_clicked(),
        "pr_fill" | "pr_generate" => {
            let f = c.pr_form.borrow();
            let f = f
                .as_ref()
                .ok_or_else(|| Error::usage("the Create PR dialog is not open"))?;
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
        "open" => {
            let root = c.root_dir().unwrap_or_default();
            crate::files::open_in_viewer(&root.join(s("path")));
        }
        o => return Err(Error::usage(format!("unknown op '{o}'"))),
    }
    Ok(c.info())
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
