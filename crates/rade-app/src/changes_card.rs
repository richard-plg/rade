//! One Changes card (PRD R120, R121; spec A.59–A.86): a Source link's
//! checkout as VS Code shows a repository — header (fold, name, label,
//! meta, Sync / Publish, `…`), the commit area, then the groups Merge
//! Changes, Staged Changes and Changes in one multi-select list (list or
//! tree), the Publish / Sync action button and the error line. The git work
//! behind the buttons is `changes_ops.rs`; the group / row / menu logic is
//! `rade_core::scm_view`.

use crate::app::try_app;
use crate::files;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::config::{ScmSort, ScmView};
use rade_core::home;
use rade_core::scm::{self, Section, Snapshot};
use rade_core::scm_view::{self, CommitState, Group, History, Item, Row};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Rows per group (a build output committed by mistake lists 50k files).
pub(crate) const MAX_ROWS: usize = 500;

thread_local! {
    /// Card folds the owner chose, by checkout path, until the app quits.
    static FOLDS: RefCell<HashMap<PathBuf, bool>> = RefCell::default();
}

/// What one checkout holds (read off the GTK thread).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Read {
    pub snap: Result<Snapshot, String>,
    /// This checkout's identity warning (R107.5).
    pub identity: Option<String>,
}

/// What a ListBox row stands for.
#[derive(Clone, Debug)]
pub(crate) enum Entry {
    Section(Group),
    Folder { group: Group, path: String },
    File { group: Group, item: Item },
    More,
}

/// A built row and the buttons it carries (found by tooltip in `debug.scm`).
pub(crate) struct RowW {
    row: gtk::ListBoxRow,
    depth: usize,
    stack: Option<gtk::Stack>,
    acts: Vec<gtk::Button>,
}

#[derive(Default)]
pub(crate) struct CardState {
    pub read: Option<Read>,
    pub entries: Vec<Entry>,
    pub rows: Vec<RowW>,
    pub folded: bool,
    pub folded_groups: HashSet<Group>,
    pub folded_folders: HashSet<(Group, String)>,
    pub history: Option<History>,
    pub primary: bool,
    /// The last context menu: label, enabled, its action.
    pub ctx: Vec<(String, bool, gio::SimpleAction)>,
    /// The `…` menu's items: label, action name.
    pub card_menu: Vec<(&'static str, &'static str)>,
    pub drift: Option<(String, home::DriftCard)>,
    /// The error line of the last operation.
    pub error: Option<String>,
    pub anchor: Option<usize>,
    /// Source mode: the focused checkout's identity warning (R107.5).
    pub focus_identity: Option<String>,
}

pub struct Card {
    pub(crate) path: PathBuf,
    pub(crate) source: String,
    pub(crate) name: String,
    pub(crate) project: String,
    group_label: RefCell<String>,
    pub(crate) outer: gtk::Box,
    root: gtk::Box,
    head: gtk::Box,
    chevron: gtk::Image,
    label: gtk::Label,
    meta: gtk::Label,
    sync_btn: gtk::Button,
    sync_label: gtk::Label,
    publish_btn: gtk::Button,
    more: gtk::MenuButton,
    body: gtk::Box,
    identity: gtk::Label,
    commit_row: gtk::Box,
    pub(crate) message: gtk::TextView,
    hint: gtk::Label,
    warning: gtk::Label,
    draft: gtk::Button,
    commit: gtk::Button,
    commit_more: gtk::MenuButton,
    pub(crate) list: gtk::ListBox,
    action: gtk::Button,
    action_label: gtk::Label,
    action_icon: gtk::Image,
    error: gtk::Label,
    drift_slot: gtk::Box,
    pub(crate) actions: gio::SimpleActionGroup,
    pub(crate) st: RefCell<CardState>,
    pub(crate) ops: Cell<u32>,
    pub(crate) drafting: RefCell<Option<Arc<AtomicBool>>>,
    pub(crate) pr_form: RefCell<Option<crate::changes_ops::PrForm>>,
    /// Set while the app itself writes the message box.
    setting: Cell<bool>,
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

fn set_class(w: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if on {
        w.add_css_class(class);
    } else {
        w.remove_css_class(class);
    }
}

fn act_button(icon: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.add_css_class("scm-act");
    b.set_tooltip_text(Some(tip));
    b.set_valign(gtk::Align::Center);
    b.set_focus_on_click(false);
    b
}

fn small_icon(name: &str) -> gtk::Image {
    let i = gtk::Image::from_icon_name(name);
    i.set_pixel_size(12);
    i.add_css_class("rade-dim");
    i
}

pub(crate) fn text_of(v: &gtk::TextView) -> String {
    let b = v.buffer();
    b.text(&b.start_iter(), &b.end_iter(), false).to_string()
}

/// The CSS class of a status letter (`!` cannot be a class name).
fn letter_class(l: char) -> String {
    if l == '!' {
        "st-conflict".into()
    } else {
        format!("st-{l}")
    }
}

pub(crate) fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub(crate) fn group_key(g: Group) -> &'static str {
    match g {
        Group::Merge => "merge",
        Group::Staged => "staged",
        Group::Changes => "changes",
    }
}

pub(crate) fn parse_group(s: &str) -> Option<Group> {
    match s {
        "merge" => Some(Group::Merge),
        "staged" => Some(Group::Staged),
        "changes" => Some(Group::Changes),
        _ => None,
    }
}

impl Card {
    /// A card for `g`'s checkout; `name` is the Source's display name.
    pub(crate) fn new(g: &home::Group, name: &str) -> Rc<Card> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("changes-group");
        root.set_widget_name(&format!("changes-group-{}", g.source));

        // Header (A.60).
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        head.add_css_class("changes-group-header");
        let chevron = small_icon("ph-caret-down-symbolic");
        head.append(&chevron);
        head.append(&label(name, &["changes-group-title"]));
        let lab = label(&g.label, &["changes-group-label"]);
        lab.set_hexpand(true);
        lab.set_ellipsize(gtk::pango::EllipsizeMode::End);
        head.append(&lab);
        let meta = label("", &["changes-group-meta"]);
        head.append(&meta);
        let sync_btn = gtk::Button::new();
        sync_btn.add_css_class("scm-sync");
        sync_btn.set_valign(gtk::Align::Center);
        let sb = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        sb.append(&gtk::Image::from_icon_name("ph-arrows-clockwise-symbolic"));
        let sync_label = gtk::Label::new(None);
        sb.append(&sync_label);
        sync_btn.set_child(Some(&sb));
        sync_btn.set_visible(false);
        head.append(&sync_btn);
        let publish_btn = gtk::Button::from_icon_name("ph-cloud-arrow-up-symbolic");
        publish_btn.add_css_class("scm-publish");
        publish_btn.set_valign(gtk::Align::Center);
        publish_btn.set_tooltip_text(Some("Publish Branch…"));
        publish_btn.set_visible(false);
        head.append(&publish_btn);
        let more = gtk::MenuButton::new();
        more.set_icon_name("ph-dots-three-symbolic");
        more.add_css_class("flat");
        more.add_css_class("changes-tool");
        more.set_tooltip_text(Some("More Actions…"));
        more.set_valign(gtk::Align::Center);
        head.append(&more);
        root.append(&head);

        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let identity = label("", &["changes-identity"]);
        identity.set_wrap(true);
        identity.set_visible(false);
        body.append(&identity);

        // Commit area (A.64–A.66, A.85).
        let commit_row = gtk::Box::new(gtk::Orientation::Vertical, 6);
        commit_row.add_css_class("commit-row");
        let message = gtk::TextView::new();
        message.set_wrap_mode(gtk::WrapMode::WordChar);
        message.set_accepts_tab(false);
        message.set_top_margin(4);
        message.set_bottom_margin(4);
        message.set_left_margin(0);
        message.set_right_margin(0);
        let msg_scroll = gtk::ScrolledWindow::new();
        msg_scroll.add_css_class("commit-entry");
        msg_scroll.set_child(Some(&message));
        // External: an overlay scrollbar's own minimum would make the box 58 px.
        msg_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::External);
        msg_scroll.set_propagate_natural_height(true);
        msg_scroll.set_max_content_height(10 * 16 + 8);
        let hint = label(
            &scm_view::placeholder(None),
            &["rade-dim"],
        );
        hint.set_valign(gtk::Align::Start);
        hint.set_margin_start(9);
        hint.set_margin_top(5);
        hint.set_can_target(false);
        hint.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&msg_scroll));
        overlay.add_overlay(&hint);
        commit_row.append(&overlay);
        let warning = label("", &["commit-warning"]);
        warning.set_visible(false);
        commit_row.append(&warning);
        let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let draft = gtk::Button::from_icon_name("ph-sparkle-symbolic");
        draft.add_css_class("draft-button");
        draft.set_tooltip_text(Some("Draft with agent"));
        bottom.append(&draft);
        let fill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        fill.set_hexpand(true);
        bottom.append(&fill);
        let split = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        split.add_css_class("linked");
        split.add_css_class("commit-split");
        let commit = gtk::Button::with_label("Commit");
        commit.add_css_class("commit-button");
        commit.set_tooltip_text(Some("Commit (Ctrl+Enter)"));
        let commit_more = gtk::MenuButton::new();
        commit_more.set_icon_name("ph-caret-down-symbolic");
        commit_more.add_css_class("commit-more");
        commit_more.set_tooltip_text(Some("More Commit Actions"));
        split.append(&commit);
        split.append(&commit_more);
        bottom.append(&split);
        commit_row.append(&bottom);
        body.append(&commit_row);

        // Groups.
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Multiple);
        list.set_activate_on_single_click(false);
        list.add_css_class("scm-list");
        body.append(&list);

        let action_icon = gtk::Image::from_icon_name("ph-cloud-arrow-up-symbolic");
        let action_label = gtk::Label::new(None);
        let ab = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        ab.set_halign(gtk::Align::Center);
        ab.append(&action_icon);
        ab.append(&action_label);
        let action = gtk::Button::new();
        action.set_child(Some(&ab));
        action.add_css_class("scm-action-button");
        action.set_hexpand(true);
        action.set_visible(false);
        body.append(&action);

        let error = label("", &["scm-card-error"]);
        error.set_wrap(true);
        error.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        error.set_selectable(true);
        error.set_visible(false);
        body.append(&error);
        root.append(&body);

        let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        outer.append(&root);
        let drift_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);
        outer.append(&drift_slot);

        let card = Rc::new(Card {
            path: g.path.clone(),
            source: g.source.clone(),
            name: name.to_string(),
            project: g.project.clone(),
            group_label: RefCell::new(g.label.clone()),
            outer,
            root,
            head,
            chevron,
            label: lab,
            meta,
            sync_btn,
            sync_label,
            publish_btn,
            more,
            body,
            identity,
            commit_row,
            message,
            hint,
            warning,
            draft,
            commit,
            commit_more,
            list,
            action,
            action_label,
            action_icon,
            error,
            drift_slot,
            actions: gio::SimpleActionGroup::new(),
            st: RefCell::default(),
            ops: Cell::new(0),
            drafting: RefCell::default(),
            pr_form: RefCell::default(),
            setting: Cell::new(false),
        });
        card.wire();
        card
    }

    fn weak(self: &Rc<Self>) -> Weak<Self> {
        Rc::downgrade(self)
    }

    /// A named action of the card's `scm` group.
    fn add_action(self: &Rc<Self>, name: &str, f: impl Fn(&Rc<Card>) + 'static) {
        let a = gio::SimpleAction::new(name, None);
        let w = self.weak();
        a.connect_activate(move |_, _| {
            if let Some(c) = w.upgrade() {
                f(&c);
            }
        });
        self.actions.add_action(&a);
    }

    fn wire(self: &Rc<Self>) {
        self.root.insert_action_group("scm", Some(&self.actions));

        // The `…` menu and Commit ▾ items.
        self.add_action("fetch", |c| c.fetch(true));
        self.add_action("pull", |c| c.pull());
        self.add_action("push", |c| c.push_flow());
        self.add_action("sync", |c| c.sync_flow());
        self.add_action("publish", |c| c.publish_flow());
        self.add_action("pr", |c| c.pr_clicked());
        self.add_action("stage-all", |c| c.stage_all());
        self.add_action("unstage-all", |c| c.unstage_all());
        self.add_action("discard-all", |c| c.discard_all());
        self.add_action("c-commit", |c| c.commit_clicked());
        self.add_action("c-amend", |c| c.commit_variant("amend"));
        self.add_action("c-push", |c| c.commit_variant("push"));
        self.add_action("c-all", |c| c.commit_variant("all"));
        self.add_action("c-undo", |c| c.undo());
        let menu = gio::Menu::new();
        for (label, action) in [
            ("Commit", "c-commit"),
            ("Commit (Amend)", "c-amend"),
            ("Commit & Push", "c-push"),
            ("Commit All", "c-all"),
            ("Undo Last Commit", "c-undo"),
        ] {
            menu.append(Some(label), Some(&format!("scm.{action}")));
        }
        self.commit_more.set_menu_model(Some(&menu));

        // Header: a click folds; the buttons do their own.
        let g = gtk::GestureClick::new();
        g.set_button(1);
        let w = self.weak();
        g.connect_released(move |_, _, _, _| {
            if let Some(c) = w.upgrade() {
                c.toggle_fold();
            }
        });
        self.head.add_controller(g);
        let w = self.weak();
        self.sync_btn.connect_clicked(move |_| {
            if let Some(c) = w.upgrade() {
                c.sync_flow();
            }
        });
        let w = self.weak();
        self.publish_btn.connect_clicked(move |_| {
            if let Some(c) = w.upgrade() {
                c.publish_flow();
            }
        });
        let w = self.weak();
        self.action.connect_clicked(move |_| {
            if let Some(c) = w.upgrade() {
                c.action_clicked();
            }
        });

        // Message box.
        let w = self.weak();
        self.message.buffer().connect_changed(move |_| {
            let Some(c) = w.upgrade() else { return };
            if !c.setting.get() {
                c.st.borrow_mut().history = None;
            }
            if let Some(ch) = crate::changes::get() {
                ch.remember_message(&c.path, &text_of(&c.message));
            }
            c.update_commit();
            // The commit area shows while the box holds text.
            c.update_visibility();
        });
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let w = self.weak();
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(c) = w.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let mods = state
                & (gdk::ModifierType::CONTROL_MASK
                    | gdk::ModifierType::SHIFT_MASK
                    | gdk::ModifierType::ALT_MASK);
            let enter = matches!(key, gdk::Key::Return | gdk::Key::KP_Enter);
            if enter && mods == gdk::ModifierType::CONTROL_MASK {
                c.commit_clicked();
                return glib::Propagation::Stop;
            }
            if mods.is_empty() {
                let b = c.message.buffer();
                let line = b.iter_at_mark(&b.get_insert()).line();
                if key == gdk::Key::Up && line == 0 && c.history_key(true) {
                    return glib::Propagation::Stop;
                }
                if key == gdk::Key::Down && line == b.line_count() - 1 && c.history_key(false) {
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
        self.message.add_controller(keys);
        let w = self.weak();
        self.commit.connect_clicked(move |_| {
            if let Some(c) = w.upgrade() {
                c.commit_clicked();
            }
        });
        let w = self.weak();
        self.draft.connect_clicked(move |_| {
            if let Some(c) = w.upgrade() {
                c.generate();
            }
        });

        // The list: clicks, right click, keys.
        let click = gtk::GestureClick::new();
        click.set_button(1);
        let w = self.weak();
        click.connect_pressed(move |g, n, x, y| {
            if let Some(c) = w.upgrade() {
                c.list_pressed(g, n, x, y);
            }
        });
        self.list.add_controller(click);
        let right = gtk::GestureClick::new();
        right.set_button(3);
        let w = self.weak();
        right.connect_pressed(move |g, _, x, y| {
            let Some(c) = w.upgrade() else { return };
            g.set_state(gtk::EventSequenceState::Claimed);
            if let Some(row) = c.list.row_at_y(y as i32) {
                c.right_click(row.index() as usize, Some((x, y)));
            }
        });
        self.list.add_controller(right);
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let w = self.weak();
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(c) = w.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
            let other = state.intersects(gdk::ModifierType::SHIFT_MASK | gdk::ModifierType::ALT_MASK);
            let name = match key {
                gdk::Key::space if !ctrl && !other => "space",
                gdk::Key::Delete | gdk::Key::KP_Delete if !ctrl && !other => "delete",
                gdk::Key::Return | gdk::Key::KP_Enter if !ctrl && !other => "enter",
                gdk::Key::a if ctrl && !other => "ctrl+a",
                _ => return glib::Propagation::Proceed,
            };
            if c.key(name) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        self.list.add_controller(keys);
    }

    // ---- state ------------------------------------------------------------

    pub(crate) fn snap(&self) -> Option<Snapshot> {
        match &self.st.borrow().read {
            Some(Read { snap: Ok(s), .. }) => Some(s.clone()),
            _ => None,
        }
    }

    pub(crate) fn read_error(&self) -> Option<String> {
        match &self.st.borrow().read {
            Some(Read { snap: Err(e), .. }) => Some(e.clone()),
            _ => None,
        }
    }

    pub(crate) fn has_read(&self) -> bool {
        self.st.borrow().read.is_some()
    }

    pub(crate) fn file_count(&self) -> usize {
        self.snap().map_or(0, |s| s.status.files.len())
    }

    /// `+a` / `−d` over the rows of the card's groups (the header totals).
    pub(crate) fn totals(&self) -> (usize, usize) {
        let Some(snap) = self.snap() else { return (0, 0) };
        let mut t = (0, 0);
        for (g, items) in scm_view::groups(&snap.status) {
            for it in &items {
                let (a, d) = counts(&snap, g, it);
                t.0 += a;
                t.1 += d;
            }
        }
        t
    }

    pub(crate) fn has_changes(&self) -> bool {
        self.file_count() > 0
    }

    pub(crate) fn set_label(&self, label: &str) {
        *self.group_label.borrow_mut() = label.to_string();
    }

    /// New data from the checkout (or none changed: nothing to do).
    pub(crate) fn set_read(self: &Rc<Self>, read: Read) {
        if self.st.borrow().read.as_ref() == Some(&read) {
            return;
        }
        self.st.borrow_mut().read = Some(read);
        self.render();
    }

    /// A card of a Project reads its checkout's `user.email`; the card of a
    /// checkout outside every Project shows the focused one's warning.
    pub(crate) fn set_focus_identity(&self, w: Option<String>) {
        if self.st.borrow().focus_identity == w {
            return;
        }
        self.st.borrow_mut().focus_identity = w;
        self.refresh_identity();
    }

    fn refresh_identity(&self) {
        let text = {
            let st = self.st.borrow();
            if self.project.is_empty() {
                st.focus_identity.clone()
            } else {
                st.read.as_ref().and_then(|r| r.identity.clone())
            }
        };
        self.identity.set_label(text.as_deref().unwrap_or(""));
        self.update_visibility();
    }

    /// The drift card below this one (A.67), rebuilt only when it changed.
    pub(crate) fn set_drift(&self, drift: Option<home::DriftCard>) {
        let key = drift.as_ref().map(|d| (self.source.clone(), d.clone()));
        if self.st.borrow().drift == key {
            return;
        }
        while let Some(c) = self.drift_slot.first_child() {
            self.drift_slot.remove(&c);
        }
        if let Some(d) = &drift {
            self.drift_slot
                .append(&drift_card(&self.project, &self.source, d));
        }
        self.st.borrow_mut().drift = key;
    }

    pub(crate) fn set_primary(&self, on: bool) {
        self.st.borrow_mut().primary = on;
        let primary = on && self.has_changes();
        if primary {
            self.commit.add_css_class("suggested-action");
        } else {
            self.commit.remove_css_class("suggested-action");
        }
        if let Some(split) = self.commit.parent() {
            if primary {
                split.add_css_class("primary");
            } else {
                split.remove_css_class("primary");
            }
        }
    }

    pub(crate) fn show_error(&self, e: Option<String>) {
        self.st.borrow_mut().error = e;
        self.update_error();
    }

    fn update_error(&self) {
        let text = self.st.borrow().error.clone().or_else(|| self.read_error());
        self.error.set_visible(text.is_some());
        self.error.set_label(text.as_deref().unwrap_or(""));
    }

    fn folded_default(&self) -> bool {
        let Some(snap) = self.snap() else { return false };
        let has = !snap.status.files.is_empty();
        !has && scm_view::action_button(&snap.status, has, snap.head.is_some()).is_none()
    }

    pub(crate) fn is_folded(&self) -> bool {
        self.st.borrow().folded
    }

    fn apply_fold(&self) {
        let folded = FOLDS
            .with_borrow(|f| f.get(&self.path).copied())
            .unwrap_or_else(|| self.folded_default());
        self.st.borrow_mut().folded = folded;
        self.body.set_visible(!folded);
        self.chevron.set_icon_name(Some(if folded {
            "ph-caret-right-symbolic"
        } else {
            "ph-caret-down-symbolic"
        }));
        set_class(&self.root, "folded", folded);
    }

    pub(crate) fn toggle_fold(&self) {
        let now = !self.is_folded();
        FOLDS.with_borrow_mut(|f| {
            f.insert(self.path.clone(), now);
        });
        self.apply_fold();
    }

    /// Unfold (the palette's Commit shows the message box).
    pub(crate) fn unfold(&self) {
        if self.is_folded() {
            self.toggle_fold();
        }
    }

    pub(crate) fn busy(&self) -> bool {
        self.ops.get() > 0
    }

    pub(crate) fn commit_state(&self) -> CommitState {
        let Some(snap) = self.snap() else {
            return CommitState::default();
        };
        let (mut staged, mut changes) = (0, 0);
        for (g, items) in scm_view::groups(&snap.status) {
            match g {
                Group::Staged => staged += items.len(),
                _ => changes += items.len(),
            }
        }
        CommitState {
            staged,
            changes,
            message: !text_of(&self.message).trim().is_empty(),
            head: snap.head.is_some(),
            head_pushed: snap.head_pushed,
        }
    }

    // ---- rendering ----------------------------------------------------------

    pub(crate) fn render(self: &Rc<Self>) {
        let Some(read) = self.st.borrow().read.clone() else {
            return;
        };
        self.refresh_identity();
        let Ok(snap) = &read.snap else {
            // Not a repository (or git failed): the header and the error.
            self.meta.set_label("");
            self.sync_btn.set_visible(false);
            self.publish_btn.set_visible(false);
            self.commit_row.set_visible(false);
            self.list.set_visible(false);
            self.action.set_visible(false);
            self.identity.set_visible(false);
            self.root.add_css_class("clean");
            self.update_error();
            self.apply_fold();
            return;
        };
        let files = snap.status.files.len();
        let has = files > 0;
        let action = scm_view::action_button(&snap.status, has, snap.head.is_some());
        let branch = snap.status.branch.clone();

        let lab = self.group_label.borrow().clone();
        self.label.set_label(&if lab.is_empty() {
            branch.clone().unwrap_or_else(|| "(detached)".into())
        } else {
            lab
        });
        self.meta
            .set_label(&home::group_meta(snap.ahead_base, files));
        set_class(&self.root, "clean", !has);
        self.root
            .set_opacity(if !has && action.is_none() { 0.75 } else { 1.0 });

        // Sync / Publish (A.86).
        match scm_view::sync_label(&snap.status) {
            Some(("sync", l)) => {
                self.sync_label.set_label(&l);
                self.sync_btn.set_tooltip_text(Some(&format!(
                    "Sync Changes: pull {}, push {}…",
                    snap.status.behind, snap.status.ahead
                )));
                self.sync_btn.set_visible(true);
                self.publish_btn.set_visible(false);
            }
            Some(_) => {
                self.sync_btn.set_visible(false);
                self.publish_btn.set_visible(true);
            }
            None => {
                self.sync_btn.set_visible(false);
                self.publish_btn.set_visible(false);
            }
        }
        self.hint
            .set_label(&scm_view::placeholder(branch.as_deref()));

        // The `…` menu.
        let own = snap.status.own_upstream().is_some();
        let menu = gio::Menu::new();
        let mut items: Vec<(&'static str, &'static str)> = vec![
            ("Fetch", "fetch"),
            ("Pull", "pull"),
            ("Push…", "push"),
            ("Sync…", "sync"),
        ];
        if !own {
            items.push(("Publish Branch…", "publish"));
        }
        items.push(("Create Pull Request…", "pr"));
        let sec1 = gio::Menu::new();
        for (l, a) in &items {
            sec1.append(Some(l), Some(&format!("scm.{a}")));
        }
        menu.append_section(None, &sec1);
        let sec2 = gio::Menu::new();
        let all = [
            ("Stage All Changes", "stage-all"),
            ("Unstage All Changes", "unstage-all"),
            ("Discard All Changes…", "discard-all"),
        ];
        for (l, a) in &all {
            sec2.append(Some(l), Some(&format!("scm.{a}")));
        }
        menu.append_section(None, &sec2);
        items.extend(all);
        self.more.set_menu_model(Some(&menu));
        self.st.borrow_mut().card_menu = items;

        // The action button (A.84).
        match &action {
            Some(t) => {
                self.action_label.set_label(t);
                self.action_icon.set_icon_name(Some(if t.starts_with("Publish") {
                    "ph-cloud-arrow-up-symbolic"
                } else {
                    "ph-arrows-clockwise-symbolic"
                }));
                self.action.set_visible(true);
            }
            None => self.action.set_visible(false),
        }

        self.apply_fold();
        self.rebuild_list();
        self.update_commit();
        self.update_visibility();
        self.update_error();
        if let Some(ch) = crate::changes::get() {
            ch.sync_primary();
        }
    }

    /// What shows by content: the commit area (changes, or a message in the
    /// box), the list, the identity line.
    fn update_visibility(&self) {
        let Some(snap) = self.snap() else { return };
        let has = !snap.status.files.is_empty();
        let msg = self.message.buffer().char_count() > 0;
        self.commit_row.set_visible(has || msg);
        self.list.set_visible(has);
        self.identity
            .set_visible(!self.identity.label().is_empty() && (has || msg));
    }

    /// Commit's sensitivity, the menu's items, the subject warning.
    pub(crate) fn update_commit(&self) {
        let s = self.commit_state();
        let free = !self.busy();
        self.commit
            .set_sensitive(scm_view::commit_enabled(&s) && free);
        for (item, name) in scm_view::commit_menu(&s)
            .into_iter()
            .zip(["c-commit", "c-amend", "c-push", "c-all", "c-undo"])
        {
            if let Some(a) = self
                .actions
                .lookup_action(name)
                .and_downcast::<gio::SimpleAction>()
            {
                a.set_enabled(item.enabled && free);
            }
        }
        self.hint
            .set_visible(self.message.buffer().char_count() == 0);
        let w = scm_view::subject_warning(&text_of(&self.message));
        self.warning.set_visible(w.is_some());
        self.warning.set_label(w.as_deref().unwrap_or(""));
        self.draft.set_sensitive(true);
    }

    fn selected_keys(&self) -> HashSet<(Group, String)> {
        let st = self.st.borrow();
        self.list
            .selected_rows()
            .iter()
            .filter_map(|r| match st.entries.get(r.index() as usize) {
                Some(Entry::File { group, item }) => Some((*group, item.path.clone())),
                _ => None,
            })
            .collect()
    }

    fn rebuild_list(self: &Rc<Self>) {
        let Some(snap) = self.snap() else { return };
        let selected = self.selected_keys();
        // The focused row, to give the focus back after the rebuild.
        let focused: Option<(Group, String)> = {
            let st = self.st.borrow();
            self.list
                .root()
                .and_then(|r| r.focus())
                .and_then(|f| f.ancestor(gtk::ListBoxRow::static_type()))
                .and_then(|f| f.downcast::<gtk::ListBoxRow>().ok())
                .filter(|r| r.parent().as_ref() == Some(self.list.upcast_ref()))
                .and_then(|r| match st.entries.get(r.index() as usize) {
                    Some(Entry::File { group, item }) => Some((*group, item.path.clone())),
                    _ => None,
                })
        };
        let scroll = self
            .list
            .ancestor(gtk::ScrolledWindow::static_type())
            .and_downcast::<gtk::ScrolledWindow>();
        let pos = scroll.as_ref().map(|s| s.vadjustment().value());
        let (view, sort) = match try_app() {
            Some(app) => {
                let c = app.config.borrow();
                (c.scm.view, c.scm.sort)
            }
            None => (ScmView::default(), ScmSort::default()),
        };
        let (fgroups, ffolders) = {
            let st = self.st.borrow();
            (st.folded_groups.clone(), st.folded_folders.clone())
        };

        while let Some(r) = self.list.first_child() {
            self.list.remove(&r);
        }
        let mut entries: Vec<Entry> = Vec::new();
        let mut rows: Vec<RowW> = Vec::new();
        for (group, items) in scm_view::groups(&snap.status) {
            let gfold = fgroups.contains(&group);
            let w = self.section_row(group, items.len(), gfold);
            self.list.append(&w.row);
            entries.push(Entry::Section(group));
            rows.push(w);
            if gfold {
                continue;
            }
            let folded: HashSet<String> = ffolders
                .iter()
                .filter(|(g, _)| *g == group)
                .map(|(_, p)| p.clone())
                .collect();
            let mut built = scm_view::rows(&items, view, sort, &folded);
            let more = built.len().saturating_sub(MAX_ROWS);
            built.truncate(MAX_ROWS);
            for r in built {
                let (entry, w) = match r {
                    Row::Folder {
                        path,
                        name,
                        depth,
                        folded,
                    } => {
                        let w = self.folder_row(group, &path, &name, depth, folded);
                        (Entry::Folder { group, path }, w)
                    }
                    Row::File {
                        index,
                        name,
                        dir,
                        depth,
                    } => {
                        let it = &items[index];
                        let (a, d) = counts(&snap, group, it);
                        let w = self.file_row(group, it, &name, &dir, depth, a, d);
                        (
                            Entry::File {
                                group,
                                item: it.clone(),
                            },
                            w,
                        )
                    }
                };
                self.list.append(&w.row);
                entries.push(entry);
                rows.push(w);
            }
            if more > 0 {
                let l = label(&format!("… and {more} more"), &["rade-meta"]);
                l.set_margin_start(28);
                let row = gtk::ListBoxRow::new();
                row.set_child(Some(&l));
                row.set_activatable(false);
                row.set_selectable(false);
                self.list.append(&row);
                entries.push(Entry::More);
                rows.push(RowW {
                    row,
                    depth: 0,
                    stack: None,
                    acts: vec![],
                });
            }
        }
        {
            let mut st = self.st.borrow_mut();
            st.entries = entries;
            st.rows = rows;
        }
        // Selection and focus by path.
        let (mut focus_row, mut first) = (None, None);
        {
            let st = self.st.borrow();
            for (i, e) in st.entries.iter().enumerate() {
                if let Entry::File { group, item } = e {
                    let key = (*group, item.path.clone());
                    if selected.contains(&key) {
                        self.list.select_row(Some(&st.rows[i].row));
                    }
                    if focused.as_ref() == Some(&key) {
                        focus_row = Some(st.rows[i].row.clone());
                    }
                    if first.is_none() {
                        first = Some(st.rows[i].row.clone());
                    }
                }
            }
        }
        if focused.is_some() {
            if let Some(r) = focus_row.or(first) {
                r.grab_focus();
            }
        }
        if let (Some(s), Some(p)) = (scroll, pos) {
            let adj = s.vadjustment();
            glib::idle_add_local_once(move || adj.set_value(p));
        }
    }

    fn push_acts(b: &gtk::Box, acts: &mut Vec<gtk::Button>, btn: gtk::Button) {
        b.append(&btn);
        acts.push(btn);
    }

    /// Section row (A.81): chevron, title, count, the group's actions.
    fn section_row(self: &Rc<Self>, group: Group, count: usize, folded: bool) -> RowW {
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        b.add_css_class("scm-section");
        b.append(&small_icon(if folded {
            "ph-caret-right-symbolic"
        } else {
            "ph-caret-down-symbolic"
        }));
        b.append(&label(group.title(), &[]));
        b.append(&label(&count.to_string(), &["scm-count"]));
        let fill = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        fill.set_hexpand(true);
        b.append(&fill);
        let mut acts = Vec::new();
        let mut add = |icon: &str, tip: &str, f: fn(&Rc<Card>)| {
            let btn = act_button(icon, tip);
            let w = self.weak();
            btn.connect_clicked(move |_| {
                if let Some(c) = w.upgrade() {
                    f(&c);
                }
            });
            Self::push_acts(&b, &mut acts, btn);
        };
        match group {
            Group::Merge => add("ph-plus-symbolic", "Stage All Merge Changes", |c| {
                c.stage_group(Group::Merge)
            }),
            Group::Staged => add("ph-minus-symbolic", "Unstage All Changes", |c| {
                c.unstage_group()
            }),
            Group::Changes => {
                add(
                    "ph-arrow-counter-clockwise-symbolic",
                    "Discard All Changes…",
                    |c| c.discard_group(),
                );
                add("ph-plus-symbolic", "Stage All Changes", |c| {
                    c.stage_group(Group::Changes)
                });
            }
        }
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&b));
        row.set_selectable(false);
        row.set_activatable(false);
        RowW {
            row,
            depth: 0,
            stack: None,
            acts,
        }
    }

    /// Folder row of the tree view (`.scm-folder`): hover shows the group's
    /// actions on the folder's files.
    fn folder_row(
        self: &Rc<Self>,
        group: Group,
        path: &str,
        name: &str,
        depth: usize,
        folded: bool,
    ) -> RowW {
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        b.add_css_class("scm-folder");
        b.set_margin_start(12 * depth as i32);
        b.append(&small_icon(if folded {
            "ph-caret-right-symbolic"
        } else {
            "ph-caret-down-symbolic"
        }));
        let icon = gtk::Image::from_icon_name("folder-symbolic");
        files::set_icon(
            &icon,
            &files::icon_for(name.trim_end_matches('/'), true, !folded),
        );
        icon.set_pixel_size(14);
        b.append(&icon);
        let l = label(name, &[]);
        l.set_hexpand(true);
        l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        b.append(&l);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        actions.add_css_class("scm-row-actions");
        actions.set_visible(false);
        let mut acts = Vec::new();
        let prefix = format!("{path}/");
        let mut add = |icon: &str, tip: &str, op: fn(&Rc<Card>, Group, &str)| {
            let btn = act_button(icon, tip);
            let (w, p) = (self.weak(), prefix.clone());
            btn.connect_clicked(move |_| {
                if let Some(c) = w.upgrade() {
                    op(&c, group, &p);
                }
            });
            Self::push_acts(&actions, &mut acts, btn);
        };
        match group {
            Group::Merge => add("ph-plus-symbolic", "Stage Folder", |c, g, p| {
                c.stage_folder(g, p)
            }),
            Group::Staged => add("ph-minus-symbolic", "Unstage Folder", |c, g, p| {
                c.unstage_folder(g, p)
            }),
            Group::Changes => {
                add(
                    "ph-arrow-counter-clockwise-symbolic",
                    "Discard Folder…",
                    |c, g, p| c.discard_folder(g, p),
                );
                add("ph-plus-symbolic", "Stage Folder", |c, g, p| {
                    c.stage_folder(g, p)
                });
            }
        }
        b.append(&actions);
        let motion = gtk::EventControllerMotion::new();
        let a = actions.clone();
        motion.connect_enter(move |_, _, _| a.set_visible(true));
        let a = actions.clone();
        motion.connect_leave(move |_| a.set_visible(false));
        b.add_controller(motion);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&b));
        row.set_selectable(false);
        row.set_activatable(false);
        RowW {
            row,
            depth,
            stack: None,
            acts,
        }
    }

    /// File row (A.61–A.63, A.82–A.83).
    #[allow(clippy::too_many_arguments)]
    fn file_row(
        self: &Rc<Self>,
        group: Group,
        it: &Item,
        name: &str,
        dir: &str,
        depth: usize,
        add: usize,
        del: usize,
    ) -> RowW {
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        b.set_margin_start(12 * depth as i32);
        let icon = gtk::Image::from_icon_name("text-x-generic-symbolic");
        files::set_icon(&icon, &files::icon_for(name, false, false));
        icon.set_pixel_size(14);
        b.append(&icon);
        let n = label(name, &["file-row-name"]);
        n.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        if it.letter == 'D' {
            n.add_css_class("rade-strike");
        }
        b.append(&n);
        if dir.is_empty() {
            n.set_hexpand(true);
        } else {
            let d = label(dir, &["file-row-dir"]);
            d.set_hexpand(true);
            d.set_ellipsize(gtk::pango::EllipsizeMode::Start);
            b.append(&d);
        }

        let stack = gtk::Stack::new();
        let stat = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        stat.add_css_class("file-stat");
        if add > 0 {
            stat.append(&label(&format!("+{add}"), &["file-stat-add"]));
        }
        if del > 0 {
            stat.append(&label(&format!("−{del}"), &["file-stat-del"]));
        }
        stat.set_halign(gtk::Align::End);
        stack.add_named(&stat, Some("stat"));
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        actions.add_css_class("scm-row-actions");
        actions.set_halign(gtk::Align::End);
        stack.add_named(&actions, Some("actions"));
        stack.set_visible_child_name("stat");
        b.append(&stack);

        let mut acts = Vec::new();
        let mut btn = |icon: &str, tip: &str, f: fn(&Rc<Card>, Group, &Item)| {
            let button = act_button(icon, tip);
            let (w, it) = (self.weak(), it.clone());
            button.connect_clicked(move |_| {
                if let Some(c) = w.upgrade() {
                    f(&c, group, &it);
                }
            });
            Self::push_acts(&actions, &mut acts, button);
        };
        btn("ph-file-symbolic", "Open File", |c, _, it| {
            c.open_file(&it.path, false)
        });
        match group {
            Group::Merge => btn(
                "ph-plus-symbolic",
                "Mark as Resolved (Stage)",
                |c, g, it| c.stage_items(&[(g, it.clone())]),
            ),
            Group::Staged => btn("ph-minus-symbolic", "Unstage Changes", |c, g, it| {
                c.unstage_items(&[(g, it.clone())])
            }),
            Group::Changes => {
                if it.section == Some(Section::Untracked) {
                    btn("ph-trash-symbolic", "Delete File", |c, g, it| {
                        c.discard_items(&[(g, it.clone())])
                    });
                } else {
                    btn(
                        "ph-arrow-counter-clockwise-symbolic",
                        "Discard Changes",
                        |c, g, it| c.discard_items(&[(g, it.clone())]),
                    );
                }
                btn("ph-plus-symbolic", "Stage Changes", |c, g, it| {
                    c.stage_items(&[(g, it.clone())])
                });
            }
        }

        let l = label(&it.letter.to_string(), &["file-status"]);
        l.add_css_class(&letter_class(it.letter));
        b.append(&l);

        let row = gtk::ListBoxRow::new();
        row.add_css_class("file-row");
        row.set_child(Some(&b));
        row.set_activatable(false);
        row.set_tooltip_text(Some(&match &it.orig {
            Some(o) => format!("{o} → {}", it.path),
            None => it.path.clone(),
        }));
        // The counts give way to the actions while hovered, selected or
        // holding the focus; the cell keeps its width (no layout shift).
        let s = stack.clone();
        row.connect_state_flags_changed(move |row, _| {
            let f = row.state_flags();
            let on = f.intersects(
                gtk::StateFlags::PRELIGHT | gtk::StateFlags::SELECTED | gtk::StateFlags::FOCUS_WITHIN,
            );
            s.set_visible_child_name(if on { "actions" } else { "stat" });
        });
        RowW {
            row,
            depth,
            stack: Some(stack),
            acts,
        }
    }

    // ---- selection and clicks -------------------------------------------------

    pub(crate) fn entry_at(&self, index: usize) -> Option<Entry> {
        self.st.borrow().entries.get(index).cloned()
    }

    /// The selected file rows, in list order.
    pub(crate) fn selection(&self) -> Vec<(Group, Item)> {
        let st = self.st.borrow();
        let mut rows = self.list.selected_rows();
        rows.sort_by_key(|r| r.index());
        rows.iter()
            .filter_map(|r| match st.entries.get(r.index() as usize) {
                Some(Entry::File { group, item }) => Some((*group, item.clone())),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn has_file(&self, group: Group, path: &str) -> bool {
        self.index_of(group, path).is_some()
    }

    pub(crate) fn index_of(&self, group: Group, path: &str) -> Option<usize> {
        self.st.borrow().entries.iter().position(|e| {
            matches!(e, Entry::File { group: g, item } if *g == group && item.path == path)
        })
    }

    fn row_widget(&self, index: usize) -> Option<gtk::ListBoxRow> {
        self.st.borrow().rows.get(index).map(|r| r.row.clone())
    }

    /// A press on the list (D3). The ListBox itself selects (Ctrl toggles,
    /// Shift extends); a plain click also opens the diff, a double click
    /// focuses it. Clicks on a row's own buttons are theirs.
    fn list_pressed(self: &Rc<Self>, g: &gtk::GestureClick, n: i32, x: f64, y: f64) {
        if let Some(t) = self.list.pick(x, y, gtk::PickFlags::DEFAULT) {
            if t.ancestor(gtk::Button::static_type()).is_some() {
                return;
            }
        }
        let Some(row) = self.list.row_at_y(y as i32) else {
            return;
        };
        let index = row.index() as usize;
        let mods = g.current_event_state();
        let extend = mods.intersects(gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK);
        match self.entry_at(index) {
            Some(Entry::Section(group)) => self.toggle_group(group),
            Some(Entry::Folder { group, path }) => self.toggle_folder(group, &path),
            Some(Entry::File { group, item }) => {
                self.st.borrow_mut().anchor = Some(index);
                if extend {
                    return;
                }
                // After the ListBox has selected the row.
                let w = self.weak();
                glib::idle_add_local_once(move || {
                    if let Some(c) = w.upgrade() {
                        c.open_diff_of(group, &item, n >= 2);
                    }
                });
            }
            _ => {}
        }
    }

    pub(crate) fn toggle_group(self: &Rc<Self>, group: Group) {
        {
            let mut st = self.st.borrow_mut();
            if !st.folded_groups.remove(&group) {
                st.folded_groups.insert(group);
            }
        }
        self.rebuild_list();
    }

    pub(crate) fn toggle_folder(self: &Rc<Self>, group: Group, path: &str) {
        {
            let mut st = self.st.borrow_mut();
            let k = (group, path.to_string());
            if !st.folded_folders.remove(&k) {
                st.folded_folders.insert(k);
            }
        }
        self.rebuild_list();
    }

    /// Open the diff of a row (D3). A conflicted file has no diff side.
    pub(crate) fn open_diff_of(self: &Rc<Self>, group: Group, item: &Item, focus: bool) {
        if group == Group::Merge {
            self.open_file(&item.path, false);
            return;
        }
        if let Some(section) = item.section {
            self.open_diff(&item.path, section, focus);
        }
    }

    /// `debug.scm click`: what the mouse does, without the ListBox's help.
    pub(crate) fn click_row(self: &Rc<Self>, group: Group, path: &str, mode: &str) {
        let Some(index) = self.index_of(group, path) else {
            return;
        };
        let Some(row) = self.row_widget(index) else {
            return;
        };
        let item = match self.entry_at(index) {
            Some(Entry::File { item, .. }) => item,
            _ => return,
        };
        match mode {
            "ctrl" => {
                if row.is_selected() {
                    self.list.unselect_row(&row);
                } else {
                    self.list.select_row(Some(&row));
                }
                self.st.borrow_mut().anchor = Some(index);
            }
            "shift" => {
                let anchor = self.st.borrow().anchor.unwrap_or(index);
                let (a, b) = (anchor.min(index), anchor.max(index));
                self.list.unselect_all();
                let st = self.st.borrow();
                for i in a..=b {
                    if matches!(st.entries.get(i), Some(Entry::File { .. })) {
                        self.list.select_row(Some(&st.rows[i].row));
                    }
                }
            }
            _ => {
                self.list.unselect_all();
                self.list.select_row(Some(&row));
                row.grab_focus();
                self.st.borrow_mut().anchor = Some(index);
                self.open_diff_of(group, &item, mode == "double");
            }
        }
    }

    /// Space / Delete / Enter / Ctrl+A in the list (D4); whether handled.
    pub(crate) fn key(self: &Rc<Self>, key: &str) -> bool {
        let sel = self.selection();
        match key {
            "space" => {
                if sel.is_empty() {
                    return false;
                }
                if sel.iter().all(|(g, _)| *g == Group::Staged) {
                    self.unstage_items(&sel);
                } else {
                    let to_stage: Vec<_> =
                        sel.into_iter().filter(|(g, _)| *g != Group::Staged).collect();
                    self.stage_items(&to_stage);
                }
                true
            }
            "delete" => {
                self.discard_items(&sel);
                true
            }
            "enter" => {
                if let Some((g, it)) = sel.first() {
                    self.open_diff_of(*g, it, false);
                }
                true
            }
            "ctrl+a" => {
                let st = self.st.borrow();
                self.list.unselect_all();
                for (e, r) in st.entries.iter().zip(&st.rows) {
                    if matches!(e, Entry::File { .. }) {
                        self.list.select_row(Some(&r.row));
                    }
                }
                true
            }
            "up" | "down" => {
                self.move_cursor(key == "down");
                true
            }
            _ => false,
        }
    }

    /// The Up / Down the keyboard does natively (for `debug.scm key`).
    fn move_cursor(&self, down: bool) {
        let st = self.st.borrow();
        let files: Vec<usize> = st
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e, Entry::File { .. }))
            .map(|(i, _)| i)
            .collect();
        if files.is_empty() {
            return;
        }
        let at = self
            .list
            .selected_rows()
            .iter()
            .map(|r| r.index() as usize)
            .max();
        let next = match (at, down) {
            (None, _) => files[0],
            (Some(a), true) => files.iter().copied().find(|i| *i > a).unwrap_or(a),
            (Some(a), false) => files.iter().rev().copied().find(|i| *i < a).unwrap_or(a),
        };
        self.list.unselect_all();
        self.list.select_row(Some(&st.rows[next].row));
        st.rows[next].row.grab_focus();
    }

    // ---- context menu -----------------------------------------------------------

    /// R120.5. The clicked row is selected first when it was not; the items
    /// act on the selected rows of its group. `at` = where to pop up.
    pub(crate) fn right_click(self: &Rc<Self>, index: usize, at: Option<(f64, f64)>) -> Vec<String> {
        let Some(Entry::File { group, item }) = self.entry_at(index) else {
            return vec![];
        };
        let Some(row) = self.row_widget(index) else {
            return vec![];
        };
        if !row.is_selected() {
            self.list.unselect_all();
            self.list.select_row(Some(&row));
        }
        row.grab_focus();
        let sel: Vec<(Group, Item)> = self
            .selection()
            .into_iter()
            .filter(|(g, _)| *g == group)
            .collect();
        let untracked = item.section == Some(Section::Untracked);

        type Op = Box<dyn Fn(&Rc<Card>)>;
        let mut items: Vec<(&str, Op)> = Vec::new();
        let (p, s2) = (item.clone(), item.section);
        if group != Group::Merge {
            let (p, s) = (p.clone(), s2);
            items.push((
                "Open Changes",
                Box::new(move |c| {
                    if let Some(s) = s {
                        c.open_diff(&p.path, s, false);
                    }
                }),
            ));
        }
        let pa = item.path.clone();
        items.push(("Open File", {
            let pa = pa.clone();
            Box::new(move |c| c.open_file(&pa, false))
        }));
        items.push(("Open File to the Side", {
            let pa = pa.clone();
            Box::new(move |c| c.open_file(&pa, true))
        }));
        match group {
            Group::Merge => {
                let sel = sel.clone();
                items.push(("Mark as Resolved", Box::new(move |c| c.stage_items(&sel))));
            }
            Group::Staged => {
                let sel = sel.clone();
                items.push(("Unstage Changes", Box::new(move |c| c.unstage_items(&sel))));
            }
            Group::Changes => {
                let s = sel.clone();
                items.push(("Stage Changes", Box::new(move |c| c.stage_items(&s))));
                let s = sel.clone();
                if untracked {
                    items.push(("Delete File…", Box::new(move |c| c.discard_items(&s))));
                    let s = sel.clone();
                    items.push((
                        "Add to .gitignore",
                        Box::new(move |c| c.gitignore_items(&s)),
                    ));
                } else {
                    items.push(("Discard Changes…", Box::new(move |c| c.discard_items(&s))));
                }
            }
        }
        let s = sel.clone();
        items.push(("Copy Path", Box::new(move |c| c.copy_paths(&s, false))));
        let s = sel.clone();
        items.push((
            "Copy Relative Path",
            Box::new(move |c| c.copy_paths(&s, true)),
        ));

        let menu = gio::Menu::new();
        let group_ = gio::SimpleActionGroup::new();
        let mut built = Vec::new();
        let labels: Vec<String> = items.iter().map(|(l, _)| l.to_string()).collect();
        for (i, (label, op)) in items.into_iter().enumerate() {
            let name = format!("i{i}");
            let a = gio::SimpleAction::new(&name, None);
            let w = self.weak();
            a.connect_activate(move |_, _| {
                if let Some(c) = w.upgrade() {
                    op(&c);
                }
            });
            group_.add_action(&a);
            menu.append(Some(label), Some(&format!("ctx.{name}")));
            built.push((label.to_string(), true, a));
        }
        self.st.borrow_mut().ctx = built;
        self.list.insert_action_group("ctx", Some(&group_));
        if let Some((x, y)) = at {
            let pop = gtk::PopoverMenu::from_model(Some(&menu));
            pop.set_parent(&self.list);
            pop.set_has_arrow(false);
            pop.set_halign(gtk::Align::Start);
            pop.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            pop.connect_closed(|p| {
                let p = p.clone();
                glib::idle_add_local_once(move || p.unparent());
            });
            pop.popup();
        }
        labels
    }

    pub(crate) fn ctx_activate(&self, label: &str) -> bool {
        let a = self
            .st
            .borrow()
            .ctx
            .iter()
            .find(|(l, _, _)| l == label)
            .map(|(_, _, a)| a.clone());
        match a {
            Some(a) => {
                a.activate(None);
                true
            }
            None => false,
        }
    }

    // ---- history, buttons -------------------------------------------------------

    pub(crate) fn set_message(&self, text: &str) {
        self.setting.set(true);
        self.message.buffer().set_text(text);
        self.setting.set(false);
    }

    /// Up / Down in the message box (D8); whether it moved through the history.
    pub(crate) fn history_key(&self, up: bool) -> bool {
        if self.st.borrow().history.is_none() {
            if !up {
                return false;
            }
            let msgs = scm::recent_messages(&self.path, 20);
            self.st.borrow_mut().history = Some(History::new(msgs, text_of(&self.message)));
        }
        let next = {
            let mut st = self.st.borrow_mut();
            let h = st.history.as_mut().expect("history");
            let t = if up { h.up() } else { h.down() };
            t.map(String::from)
        };
        match next {
            Some(t) => {
                self.set_message(&t);
                // Caret at the end.
                let b = self.message.buffer();
                b.place_cursor(&b.end_iter());
                true
            }
            None => false,
        }
    }

    pub(crate) fn action_text(&self) -> Option<String> {
        let s = self.snap()?;
        scm_view::action_button(&s.status, !s.status.files.is_empty(), s.head.is_some())
    }

    pub(crate) fn action_button_clicked(self: &Rc<Self>) {
        self.action_clicked();
    }

    fn action_clicked(self: &Rc<Self>) {
        match self.action_text() {
            Some(t) if t.starts_with("Publish") => self.publish_flow(),
            Some(_) => self.sync_flow(),
            None => {}
        }
    }

    pub(crate) fn sync_button_clicked(self: &Rc<Self>) {
        if self.sync_btn.is_visible() {
            self.sync_flow();
        } else if self.publish_btn.is_visible() {
            self.publish_flow();
        }
    }

    pub(crate) fn focus_message(&self) {
        self.unfold();
        self.commit_row.set_visible(true);
        self.message.grab_focus();
    }

    // ---- debug --------------------------------------------------------------------

    /// A row's or section's button by tooltip.
    pub(crate) fn find_act(&self, group: Group, path: Option<&str>, tip: &str) -> Option<gtk::Button> {
        let st = self.st.borrow();
        for (e, r) in st.entries.iter().zip(&st.rows) {
            let hit = match (e, path) {
                (Entry::File { group: g, item }, Some(p)) => *g == group && item.path == p,
                (Entry::Folder { group: g, path: fp }, Some(p)) => *g == group && fp == p,
                (Entry::Section(g), None) => *g == group,
                _ => false,
            };
            if hit {
                return r
                    .acts
                    .iter()
                    .find(|b| b.tooltip_text().as_deref() == Some(tip))
                    .cloned();
            }
        }
        None
    }

    pub(crate) fn menu_labels(&self) -> Vec<&'static str> {
        self.st.borrow().card_menu.iter().map(|(l, _)| *l).collect()
    }

    pub(crate) fn card_menu_activate(&self, label: &str) -> bool {
        let name = self
            .st
            .borrow()
            .card_menu
            .iter()
            .find(|(l, _)| *l == label)
            .map(|(_, a)| *a);
        match name {
            Some(n) => {
                self.actions.activate_action(n, None);
                true
            }
            None => false,
        }
    }

    pub(crate) fn commit_variant_enabled(&self, variant: &str) -> bool {
        let idx = match variant {
            "commit" => 0,
            "amend" => 1,
            "push" => 2,
            "all" => 3,
            "undo" => 4,
            _ => return false,
        };
        scm_view::commit_menu(&self.commit_state())
            .get(idx)
            .is_some_and(|m| m.enabled)
            && !self.busy()
    }

    pub(crate) fn info(&self) -> Value {
        let st = self.st.borrow();
        let snap = match &st.read {
            Some(Read { snap: Ok(s), .. }) => Some(s.clone()),
            _ => None,
        };
        let cs = self.commit_state();
        let menu: Vec<Value> = scm_view::commit_menu(&cs)
            .iter()
            .map(|m| json!({"label": m.label, "enabled": m.enabled && !self.busy()}))
            .collect();
        let groups: Vec<Value> = match &snap {
            None => vec![],
            Some(s) => scm_view::groups(&s.status)
                .into_iter()
                .map(|(g, items)| {
                    let mut rows = Vec::new();
                    let mut skip = false;
                    for (e, r) in st.entries.iter().zip(&st.rows) {
                        match e {
                            Entry::Section(sg) => {
                                skip = false;
                                if *sg != g {
                                    skip = true;
                                }
                            }
                            _ if skip => {}
                            Entry::Folder { path, .. } => {
                                let name = path.rsplit('/').next().unwrap_or(path);
                                rows.push(json!({
                                    "kind": "folder", "path": path, "name": name,
                                    "dir": "", "letter": "", "add": 0, "del": 0,
                                    "depth": r.depth,
                                    "selected": false, "actions_shown": false,
                                }));
                            }
                            Entry::File { item, .. } => {
                                let (a, d) = counts(s, g, item);
                                let (dir, name) = match item.path.rfind('/') {
                                    Some(i) => (&item.path[..=i], &item.path[i + 1..]),
                                    None => ("", item.path.as_str()),
                                };
                                let tree = try_app()
                                    .is_some_and(|app| app.config.borrow().scm.view == ScmView::Tree);
                                rows.push(json!({
                                    "kind": "file", "path": item.path, "name": name,
                                    "dir": if tree { "" } else { dir },
                                    "letter": item.letter.to_string(), "add": a, "del": d,
                                    "depth": r.depth,
                                    "selected": r.row.is_selected(),
                                    "actions_shown": r.stack.as_ref()
                                        .is_some_and(|s| s.visible_child_name().as_deref() == Some("actions")),
                                }));
                            }
                            _ => {}
                        }
                    }
                    json!({
                        "kind": group_key(g), "title": g.title(), "count": items.len(),
                        "folded": st.folded_groups.contains(&g), "rows": rows,
                    })
                })
                .collect(),
        };
        let sync = snap.as_ref().and_then(|s| {
            scm_view::sync_label(&s.status).map(|(k, l)| json!({"kind": k, "label": l}))
        });
        let action = self.action.is_visible().then(|| self.action_label.label().to_string());
        let meta = self.meta.label().to_string();
        let clean = snap.as_ref().is_none_or(|s| s.status.files.is_empty());
        json!({
            "source": self.source, "name": self.name, "label": self.label.label().as_str(),
            "path": self.path, "meta": meta, "folded": st.folded, "clean": clean,
            "sync": sync, "action": action,
            "message": text_of(&self.message),
            "warning": self.warning.is_visible().then(|| self.warning.label().to_string()),
            "commit": {
                "shown": self.commit_row.is_visible(),
                "enabled": self.commit.is_sensitive(),
                "primary": self.commit.has_css_class("suggested-action"),
                "menu": menu,
            },
            "identity": self.identity.is_visible().then(|| self.identity.label().to_string()),
            "error": self.error.is_visible().then(|| self.error.label().to_string()),
            "drafting": self.drafting.borrow().is_some(),
            "drift": st.drift.as_ref().map(|(_, d)| json!({
                "text": d.text, "sub": d.sub, "full": d.full(),
                "buttons": d.buttons.iter().map(|(_, l)| l.clone()).collect::<Vec<_>>(),
            })),
            "groups": groups,
        })
    }

    /// Where the window's keyboard focus is, for `info.focus`.
    pub(crate) fn focus_in(&self) -> (bool, bool) {
        let Some(app) = try_app() else {
            return (false, false);
        };
        let Some(f) = gtk::prelude::GtkWindowExt::focus(&app.window) else {
            return (false, false);
        };
        let inside = |w: &gtk::Widget| f == *w || f.is_ancestor(w);
        (
            inside(self.list.upcast_ref()),
            inside(self.message.upcast_ref()),
        )
    }
}

/// `+a` / `−d` of a row against the group's side (R120.2).
fn counts(snap: &Snapshot, group: Group, it: &Item) -> (usize, usize) {
    match group {
        Group::Merge => (0, 0),
        Group::Staged => snap.staged.get(&it.path).copied().unwrap_or((0, 0)),
        Group::Changes => {
            if it.section == Some(Section::Untracked) {
                (snap.untracked_lines.get(&it.path).copied().unwrap_or(0), 0)
            } else {
                snap.unstaged.get(&it.path).copied().unwrap_or((0, 0))
            }
        }
    }
}

/// The drift card (A.67–A.68): its buttons do what Project home's do.
fn drift_card(project: &str, source: &str, d: &home::DriftCard) -> gtk::Widget {
    let card = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    card.add_css_class("drift-card");
    let icon = gtk::Image::from_icon_name("ph-warning-diamond-fill-symbolic");
    icon.add_css_class("drift-icon");
    icon.set_valign(gtk::Align::Start);
    card.append(&icon);
    let col = gtk::Box::new(gtk::Orientation::Vertical, 0);
    col.set_hexpand(true);
    let t = label(&d.text, &["drift-text"]);
    t.set_wrap(true);
    col.append(&t);
    let s = label(&d.sub, &["drift-sub"]);
    s.set_wrap(true);
    col.append(&s);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    actions.add_css_class("drift-actions");
    for (a, text) in &d.buttons {
        let b = gtk::Button::with_label(text);
        b.add_css_class("btn");
        let (p, s, a) = (project.to_string(), source.to_string(), *a);
        b.connect_clicked(move |_| match a {
            rade_core::drift::Action::BackToBase => crate::home_view::back_to_base(&p, &s),
            rade_core::drift::Action::Keep => crate::home_view::keep(&p, &s),
            rade_core::drift::Action::MoveToWorktree => crate::home_view::move_to_worktree(&p, &s),
        });
        actions.append(&b);
    }
    col.append(&actions);
    card.append(&col);
    card.upcast()
}
