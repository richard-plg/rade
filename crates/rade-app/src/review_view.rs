//! The Review Pane (PRD R13, R118): a Worktree's changes against its base
//! (`rade_core::diff`) as one scrolling list of every file — a file header
//! (status, path, +/−, Viewed; a click folds the file), its hunks' `@@`
//! headers (a click folds the hunk) and their lines: syntax highlighting,
//! green/red line backgrounds (translucent: readable in light and dark),
//! old/new line numbers. The list is a GtkListView (M11): only the rows on
//! screen exist, so a diff of any size scrolls smoothly.
//! Around it (R118): the header says whose work it is (`review_head.rs`),
//! the files rail is a tree with Viewed marks (`review_rail.rs`), comments
//! are written and shown inline (`review_comments.rs`), the Pane's own
//! keys move by hunk / file / comment, and Impact is a rail on the right.
//! Refreshes every few seconds while shown. Code navigation on diff lines
//! and the Impact section (R87, R88): `review_nav.rs`, `review_impact.rs`.

use crate::app::try_app;
use crate::worktrees::in_background;
use adw::prelude::BreakpointBinExt;
use gtk::{gdk, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::diff::{Changes, Row};
use rade_core::review::ReviewComment;
use sourceview5::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[path = "review_comments.rs"]
mod comments;
#[path = "review_debug.rs"]
mod debug;
#[path = "review_head.rs"]
mod head;
#[path = "review_impact.rs"]
mod impact;
#[path = "review_nav.rs"]
mod nav;
#[path = "review_rail.rs"]
mod rail;
#[path = "review_rows.rs"]
mod rows;

/// ponytail: polls `git diff` while shown; a file monitor on the whole
/// Worktree would cost more watches than it saves on typical repos.
const REFRESH: Duration = Duration::from_secs(3);

/// Where the list should be once laid out ([`ReviewView::scroll_later`]).
#[derive(Clone, Copy)]
enum Scroll {
    /// Keep this adjustment value (the rows were rebuilt).
    Value(f64),
    /// Row at the top (a jump).
    Top(u32),
}

/// A selection waiting for its comment text (or a draft being edited).
#[derive(Clone, Debug)]
struct Pending {
    path: String,
    line: u32,
    end_line: Option<u32>,
    excerpt: Vec<String>,
    hunk: String,
    /// The draft this edits.
    edit: Option<u64>,
    /// Shown at the top of its file (an outdated draft being edited).
    outdated: bool,
    /// The sent comment this follows up (R119.3).
    reply_to: Option<u64>,
}

/// What a refresh reads off the UI thread.
struct Fresh {
    changes: Changes,
    /// The last Round's number and the changes since its snapshot (R119.2).
    since: Option<(u32, Changes)>,
    /// The checked-out branch (the header's first word).
    branch: Option<String>,
    commits: Vec<rade_core::review_loop::Commit>,
    generated: HashSet<String>,
    hashes: HashMap<String, u64>,
}

#[derive(Default)]
struct State {
    changes: Option<Changes>,
    error: Option<String>,
    /// The file last jumped to (file list, comment location, `debug select`).
    selected: Option<String>,
    rows: Vec<Row>,
    folded_files: HashSet<String>,
    /// (path, `@@` header).
    folded_hunks: HashSet<(String, String)>,
    pending: Option<Pending>,
    /// Comments not outdated, as last rendered (the gutter marks their lines).
    comments: Vec<ReviewComment>,
    /// Every comment, for the cards.
    all_comments: Vec<ReviewComment>,
    /// Their ids, places and outdated flags (text excluded: editing must not
    /// rebuild the list).
    comments_sig: Vec<comments::Sig>,
    /// Time the last rebuild of the rows took (`debug review info`).
    render_ms: f64,
    /// The last clicked place on a diff line: (row, 1-based character
    /// column); code navigation keys ask about the name there (R87.1).
    point: Option<(usize, u32)>,
    /// The row the Pane's keys move from (R118.5).
    cursor: Option<usize>,
    /// The last Impact (R88.2) and its rows.
    impact: Option<rade_core::code::impact::Impact>,
    impact_rows: Vec<impact::ImpactRow>,
    /// Header (R118.1).
    head: rade_core::review_loop::Head,
    commits: Vec<rade_core::review_loop::Commit>,
    branch: Option<String>,
    /// Lock / generated files (R118.2), each file's content hash (R118.3).
    generated: HashSet<String>,
    hashes: HashMap<String, u64>,
    /// Folded rail folders (and the generated group).
    folded_rail: HashSet<String>,
    filter: String,
    /// Files folded because they were viewed (unfolded when unread again)
    /// or generated (folded once, when first seen).
    auto_folded: HashSet<String>,
    seen: HashSet<String>,
    ignore_space: bool,
    /// Every change against the base, and those since the last Round
    /// (R119.2); `changes` is the one shown.
    all: Option<Changes>,
    since: Option<(u32, Changes)>,
    since_active: bool,
    /// Asked for Since (before the diff arrived): applied with it.
    prefer_since: bool,
    /// The last Round the scope was chosen for (on a new Round, again).
    scoped_round: Option<u32>,
    /// Each comment's state (R119.3).
    states: HashMap<u64, rade_core::review_loop::CommentState>,
}

/// A file's lines as Pango markup (syntax-highlighted), in diff order, and
/// where each hunk starts in them.
struct Markup {
    lines: Vec<String>,
    hunk_starts: Vec<usize>,
}

pub struct ReviewView {
    pub id: String,
    pub root: gtk::Box,
    head: head::HeadUi,
    rail: rail::RailUi,
    note: gtk::Label,
    stack: gtk::Stack,
    message: adw::StatusPage,
    list: gtk::ListView,
    model: gtk::StringList,
    selection: gtk::MultiSelection,
    scroll: gtk::ScrolledWindow,
    split: adw::OverlaySplitView,
    composer: comments::ComposerUi,
    drafts: gtk::Label,
    discard: gtk::Button,
    discard_dialog: RefCell<Option<adw::AlertDialog>>,
    send: gtk::Button,
    /// *Approve ▾* (R119.4): shown instead of `send` without drafts.
    approve: gtk::MenuButton,
    approve_list: gtk::Box,
    state: RefCell<State>,
    /// File index → its highlighted lines, made on first show.
    /// Keyed by path; kept across refreshes for files whose diff is the same.
    markup: RefCell<HashMap<String, Rc<Markup>>>,
    /// A hidden buffer that highlights one file at a time.
    highlighter: sourceview5::Buffer,
    loading: Cell<bool>,
    /// The slowest file highlighting so far (`debug review info`).
    highlight_ms: Cell<f64>,
    /// The scroll target applied after the next layout.
    scroll_req: Rc<Cell<Option<Scroll>>>,
    /// The name shown as a link while Ctrl is held over it (0.7.1): its
    /// code label and byte range; the pointer's last place on the list.
    link: RefCell<Option<(gtk::Label, u32, u32)>>,
    pointer: Cell<Option<(f64, f64)>>,
    /// `debug review perf`: a scroll through the list and its frame times.
    perf: RefCell<Option<serde_json::Value>>,
    impact_ui: impact::ImpactUi,
}

fn button(icon: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.set_tooltip_text(Some(tip));
    b.add_css_class("flat");
    b
}

fn status_class(s: char) -> &'static str {
    match s {
        'A' => "rade-git-added",
        'D' => "rade-git-deleted",
        'U' => "rade-git-untracked",
        _ => "rade-git-modified",
    }
}

impl ReviewView {
    pub fn new(id: &str) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.add_css_class("rade-review");
        root.add_css_class("view");

        let impact_ui = impact::ImpactUi::new();
        let head = head::HeadUi::new(&impact_ui.toggle);
        let rail = rail::RailUi::new();

        // Every file's rows in one virtualized list.
        let model = gtk::StringList::new(&[]);
        let selection = gtk::MultiSelection::new(Some(model.clone()));
        let list = gtk::ListView::new(Some(selection.clone()), None::<gtk::ListItemFactory>);
        list.add_css_class("rade-review-list");
        list.set_enable_rubberband(true);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_child(Some(&list));
        let message = adw::StatusPage::new();
        message.set_icon_name(Some("emblem-ok-symbolic"));
        message.add_css_class("compact");
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.set_hexpand(true);
        stack.add_named(&scroll, Some("diff"));
        stack.add_named(&message, Some("message"));

        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.set_start_child(Some(&rail.root));
        paned.set_end_child(Some(&stack));
        paned.set_resize_start_child(false);
        paned.set_shrink_start_child(true);
        paned.set_position(240);
        paned.set_vexpand(true);
        // Impact: a rail right of the diff (R118.7), an overlay when narrow.
        let split = adw::OverlaySplitView::new();
        split.set_sidebar_position(gtk::PackType::End);
        split.set_min_sidebar_width(320.0);
        split.set_max_sidebar_width(320.0);
        split.set_show_sidebar(false);
        // Shown or hidden by the Impact toggle only, never by the Pane
        // getting narrower (a Viewer opened beside it).
        split.set_pin_sidebar(true);
        split.set_content(Some(&paned));
        split.set_sidebar(Some(&impact_ui.panel));
        impact_ui
            .toggle
            .bind_property("active", &split, "show-sidebar")
            .bidirectional()
            .build();
        let narrow = adw::BreakpointBin::new();
        narrow.set_size_request(200, 120);
        narrow.set_vexpand(true);
        narrow.set_child(Some(&split));
        let bp = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            1100.0,
            adw::LengthUnit::Px,
        ));
        bp.add_setter(&split, "collapsed", Some(&true.to_value()));
        narrow.add_breakpoint(bp);
        // A narrow Review (a Viewer opened beside it) gives the diff the
        // room: the files rail hides (its files have headers in the diff).
        let bp = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            560.0,
            adw::LengthUnit::Px,
        ));
        bp.add_setter(&split, "collapsed", Some(&true.to_value()));
        bp.add_setter(&rail.root, "visible", Some(&false.to_value()));
        narrow.add_breakpoint(bp);

        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        footer.add_css_class("review-footer");
        let hints = gtk::Label::new(Some(
            "n/p hunk · N/P file · ]/[ comment · v viewed · c comment",
        ));
        hints.add_css_class("review-hint");
        hints.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let note = gtk::Label::new(None);
        note.set_xalign(0.0);
        note.set_hexpand(true);
        note.set_ellipsize(gtk::pango::EllipsizeMode::End);
        note.add_css_class("review-note");
        let drafts = gtk::Label::new(None);
        drafts.add_css_class("review-drafts");
        let comment = gtk::Button::with_label("Comment");
        comment.add_css_class("flat");
        comment.set_tooltip_text(Some("Comment on the selected lines (c)"));
        let discard = gtk::Button::with_label("Discard…");
        discard.add_css_class("flat");
        discard.add_css_class("review-discard");
        discard.set_tooltip_text(Some("Discard the draft comments"));
        let send = gtk::Button::with_label("Request changes");
        send.add_css_class("suggested-action");
        send.set_tooltip_text(Some(
            "Send the draft comments to the Agent as one prompt and start a new round (Ctrl+Enter)",
        ));
        let approve_list = gtk::Box::new(gtk::Orientation::Vertical, 0);
        approve_list.add_css_class("review-approve-list");
        let approve_pop = gtk::Popover::new();
        approve_pop.set_child(Some(&approve_list));
        approve_pop.set_position(gtk::PositionType::Top);
        let approve = gtk::MenuButton::new();
        approve.set_label("Approve");
        approve.set_always_show_arrow(true);
        approve.add_css_class("suggested-action");
        approve.add_css_class("review-approve");
        approve.set_popover(Some(&approve_pop));
        for w in [
            hints.upcast_ref::<gtk::Widget>(),
            note.upcast_ref(),
            drafts.upcast_ref(),
            comment.upcast_ref(),
            discard.upcast_ref(),
            send.upcast_ref(),
            approve.upcast_ref(),
        ] {
            footer.append(w);
        }

        root.append(&head.root);
        root.append(&narrow);
        root.append(&footer);

        let highlighter = sourceview5::Buffer::new(None);
        highlighter.set_highlight_matching_brackets(false);
        let (refresh, close) = (head.refresh.clone(), head.close.clone());
        let v = Rc::new(Self {
            id: id.into(),
            root,
            head,
            rail,
            note,
            stack,
            message,
            list,
            model,
            selection,
            scroll,
            split,
            composer: comments::ComposerUi::new(),
            drafts,
            discard,
            discard_dialog: RefCell::default(),
            send,
            approve,
            approve_list,
            state: RefCell::new(State {
                folded_rail: HashSet::from([rade_core::review_loop::GENERATED.to_string()]),
                ..Default::default()
            }),
            markup: RefCell::default(),
            highlighter,
            loading: Cell::new(false),
            highlight_ms: Cell::new(0.0),
            scroll_req: Rc::default(),
            link: RefCell::default(),
            pointer: Cell::new(None),
            perf: RefCell::default(),
            impact_ui,
        });
        v.list.set_factory(Some(&rows::factory(&v)));
        v.connect(&refresh, &close, &comment);
        v.connect_rail();
        v.connect_composer();
        v.connect_impact();
        let weak = Rc::downgrade(&v);
        if let Some(pop) = v.approve.popover() {
            pop.connect_show(move |_| {
                if let Some(v) = weak.upgrade() {
                    v.fill_approve();
                }
            });
        }
        v.restyle();
        v.refresh();
        let weak = Rc::downgrade(&v);
        glib::timeout_add_local(REFRESH, move || match weak.upgrade() {
            Some(v) => {
                if v.root.is_mapped() && v.state.borrow().pending.is_none() {
                    v.refresh();
                }
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        v
    }

    fn connect(self: &Rc<Self>, refresh: &gtk::Button, close: &gtk::Button, comment: &gtk::Button) {
        let weak = Rc::downgrade(self);
        let with = move |f: fn(&Rc<Self>)| {
            let weak = weak.clone();
            move || {
                if let Some(v) = weak.upgrade() {
                    f(&v)
                }
            }
        };
        let f = with(|v| v.refresh());
        refresh.connect_clicked(move |_| f());
        let id = self.id.clone();
        close.connect_clicked(move |_| {
            if let Some(app) = try_app() {
                let _ = app.close_pane(&id, false);
            }
        });
        let f = with(|v| v.start_comment());
        comment.connect_clicked(move |_| f());
        let f = with(|v| crate::review::send_from_view(&v.id, None));
        self.send.connect_clicked(move |_| f());
        let weak = Rc::downgrade(self);
        self.head.scope_since.connect_toggled(move |b| {
            if let Some(v) = weak.upgrade() {
                v.set_scope(b.is_active());
            }
        });
        let weak = Rc::downgrade(self);
        self.head.whitespace.connect_toggled(move |b| {
            if let Some(v) = weak.upgrade() {
                v.state.borrow_mut().ignore_space = b.is_active();
                v.state.borrow_mut().changes = None;
                v.refresh();
            }
        });

        // A popover points at a row: scrolled, it would point at another.
        let list = self.list.clone();
        self.scroll
            .vadjustment()
            .connect_value_changed(move |_| crate::code_pop::close_on(list.upcast_ref()));
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(v) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if matches!(key, gdk::Key::Control_L | gdk::Key::Control_R) {
                if let Some(p) = v.pointer.get() {
                    v.set_link(Some(p));
                }
                return glib::Propagation::Proceed;
            }
            // Typing in the composer is text, never a Review key.
            if v.composer.text.has_focus() {
                return glib::Propagation::Proceed;
            }
            if v.on_loop_key(key, state) || v.on_nav_key(key, state) {
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        // Ctrl let go: names are plain text again.
        let weak = Rc::downgrade(self);
        keys.connect_key_released(move |_, key, _, _| {
            if let (Some(v), gdk::Key::Control_L | gdk::Key::Control_R) = (weak.upgrade(), key) {
                v.set_link(None);
            }
        });
        self.list.add_controller(keys);
        // Ctrl held over a name: it shows as a link (0.7.1).
        let motion = gtk::EventControllerMotion::new();
        let weak = Rc::downgrade(self);
        motion.connect_motion(move |m, x, y| {
            let Some(v) = weak.upgrade() else { return };
            v.pointer.set(Some((x, y)));
            let ctrl = m
                .current_event_state()
                .contains(gdk::ModifierType::CONTROL_MASK);
            v.set_link(ctrl.then_some((x, y)));
        });
        let weak = Rc::downgrade(self);
        motion.connect_leave(move |_| {
            if let Some(v) = weak.upgrade() {
                v.pointer.set(None);
                v.set_link(None);
            }
        });
        self.list.add_controller(motion);
        // Ctrl+click on a diff line: its definition (R87.1). On the list in
        // the capture phase: the list's own item gesture would take it to
        // toggle the row's selection.
        let ctrl = gtk::GestureClick::new();
        ctrl.set_button(gdk::BUTTON_PRIMARY);
        ctrl.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        ctrl.connect_pressed(move |g, _, x, y| {
            if !g
                .current_event_state()
                .contains(gdk::ModifierType::CONTROL_MASK)
            {
                return;
            }
            let Some(v) = weak.upgrade() else { return };
            let mut w = v.list.pick(x, y, gtk::PickFlags::DEFAULT);
            while let Some(c) = w.clone() {
                if c.has_css_class("rade-diff-row") {
                    break;
                }
                w = c.parent();
            }
            let Some(row) = w else { return };
            let Ok(pos) = row.widget_name().parse::<usize>() else {
                return;
            };
            let Some(p) = v
                .list
                .compute_point(&row, &gtk::graphene::Point::new(x as f32, y as f32))
            else {
                return;
            };
            g.set_state(gtk::EventSequenceState::Claimed);
            v.click_line(&row, pos, p.x() as f64, p.y() as f64, true);
        });
        self.list.add_controller(ctrl);
        // Focus tracking like a Viewer: clicking in focuses the Pane.
        let focus = gtk::EventControllerFocus::new();
        let id = self.id.clone();
        focus.connect_enter(move |_| {
            if let Some(app) = try_app() {
                app.focus_entered(&id);
            }
        });
        self.root.add_controller(focus);
    }

    pub fn focus(&self) {
        if self.state.borrow().pending.is_none() {
            self.list.grab_focus();
        }
    }

    /// The Review Pane's own keys (R118.5); `false` when not one of them.
    pub(super) fn on_loop_key(&self, key: gdk::Key, state: gdk::ModifierType) -> bool {
        use gdk::ModifierType as M;
        use rade_core::review_loop::Move;
        let mods = state & (M::CONTROL_MASK | M::ALT_MASK);
        if mods == M::CONTROL_MASK && matches!(key, gdk::Key::Return | gdk::Key::KP_Enter) {
            if self.send.is_visible() {
                self.send.emit_clicked();
            } else {
                self.approve.popup();
            }
            return true;
        }
        if !mods.is_empty() {
            return false;
        }
        // Esc closes the composer wherever the keyboard is in the Review.
        if key == gdk::Key::Escape && self.state.borrow().pending.is_some() {
            self.cancel_comment();
            return true;
        }
        let m = match key {
            gdk::Key::n => Move::NextHunk,
            gdk::Key::p => Move::PrevHunk,
            gdk::Key::N => Move::NextFile,
            gdk::Key::P => Move::PrevFile,
            gdk::Key::bracketright => Move::NextComment,
            gdk::Key::bracketleft => Move::PrevComment,
            gdk::Key::v => {
                self.toggle_viewed_here();
                return true;
            }
            gdk::Key::c => {
                self.start_comment();
                return true;
            }
            gdk::Key::w => {
                let b = &self.head.whitespace;
                b.set_active(!b.is_active());
                return true;
            }
            gdk::Key::i => {
                let t = &self.impact_ui.toggle;
                t.set_active(!t.is_active());
                return true;
            }
            _ => return false,
        };
        self.move_by(m);
        true
    }

    /// Move the cursor row (R118.5): selected and brought to the top.
    pub(super) fn move_by(&self, m: rade_core::review_loop::Move) {
        let to = {
            let st = self.state.borrow();
            let from = st.cursor.or_else(|| {
                let sel = self.selection.selection();
                (!sel.is_empty()).then(|| sel.minimum() as usize)
            });
            rade_core::review_loop::step(&st.rows, from, m)
        };
        let Some(i) = to else {
            self.note.set_label("Nothing further that way");
            return;
        };
        self.note.set_label("");
        let file = match self.state.borrow().rows[i] {
            Row::File(f) => self.file_path(f),
            _ => None,
        };
        if let Some(path) = file {
            self.select(&path);
        } else {
            self.scroll_to_top(i as u32);
        }
        self.state.borrow_mut().cursor = Some(i);
        self.selection.select_item(i as u32, true);
    }

    /// Light/dark changed: re-highlight with the matching scheme.
    pub fn restyle(&self) {
        self.highlighter
            .set_style_scheme(crate::viewer::scheme_for(crate::viewer::is_dark()).as_ref());
        self.markup.borrow_mut().clear();
        self.rebind();
    }

    /// Re-run the factory on every row (marks, highlighting) without
    /// changing the rows.
    fn rebind(&self) {
        self.reset_model(self.model.n_items() as usize, true);
    }

    /// Fresh items for `n` rows (row data lives in `State::rows`; the list
    /// reuses a row's widget without binding it again when its item stays
    /// the same), keeping the scroll position.
    fn reset_model(&self, n: usize, keep_selection: bool) {
        let adj = self.scroll.vadjustment();
        let keep = adj.value();
        let sel = self.selection.selection();
        self.model.splice(0, self.model.n_items(), &vec![""; n]);
        if keep_selection && !sel.is_empty() {
            let (a, b) = (sel.minimum(), sel.maximum());
            self.selection.select_range(a, b - a + 1, true);
        }
        adj.set_value(keep);
        if self.scroll_req.get().is_none() {
            self.scroll_later(Scroll::Value(keep));
        }
    }

    /// The Pane's (source, worktree) ids and root.
    fn place(&self) -> Option<(String, String, PathBuf)> {
        let app = try_app()?;
        let m = app.model.borrow();
        let loc = m.locate(&self.id)?;
        let root = m.worktree(&loc.source, &loc.worktree)?.path.clone();
        Some((loc.source, loc.worktree, root))
    }

    fn comments_now(&self) -> Vec<ReviewComment> {
        let Some((p, w, _)) = self.place() else {
            return vec![];
        };
        let app = try_app().unwrap();
        let m = app.model.borrow();
        m.comments(&p, &w).map(<[_]>::to_vec).unwrap_or_default()
    }

    /// The Worktree's Review Loop state (Viewed marks, R118.3).
    fn review_state(&self) -> Option<rade_core::review_loop::Loop> {
        let (p, w, _) = self.place()?;
        let app = try_app()?;
        let m = app.model.borrow();
        Some(m.worktree(&p, &w)?.review.clone())
    }

    /// Recompute the diff in the background; re-render only if it changed.
    pub fn refresh(self: &Rc<Self>) {
        let Some((_, _, root)) = self.place() else {
            return;
        };
        if self.loading.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        let base = try_app().and_then(|a| a.model.borrow().comparison_base(&root));
        let ws = self.state.borrow().ignore_space;
        let last = self.review_state().and_then(|r| r.rounds.last().cloned());
        in_background(
            move || -> rade_core::rpc::Result<Fresh> {
                use rade_core::review_loop as rl;
                let mut changes = rade_core::diff::changes_with(&root, base.as_deref(), ws)?;
                let since = last.and_then(|r| {
                    let c = rade_core::diff::since(&root, changes.branch.clone(), &r.snapshot, ws)
                        .ok()?;
                    Some((r.n, c))
                });
                let paths: Vec<&str> = changes.files.iter().map(|f| f.path.as_str()).collect();
                let attrs = rl::generated_attrs(&root, &paths);
                let generated: HashSet<String> = changes
                    .files
                    .iter()
                    .filter(|f| rl::generated_by_name(&f.path) || attrs.contains(&f.path))
                    .map(|f| f.path.clone())
                    .collect();
                // Lock / generated files last, in the rail and the diff.
                let reorder = |c: &mut Changes| {
                    let order = rl::file_order(&c.files, &|p| generated.contains(p));
                    let mut files: Vec<Option<_>> = c.files.drain(..).map(Some).collect();
                    c.files = order.iter().filter_map(|&i| files[i].take()).collect();
                };
                reorder(&mut changes);
                let since = since.map(|(n, mut c)| {
                    reorder(&mut c);
                    (n, c)
                });
                let hashes = changes
                    .files
                    .iter()
                    .map(|f| (f.path.clone(), rl::file_hash(&root, f)))
                    .collect();
                let commits = rl::commits(&root, &changes.base);
                Ok(Fresh {
                    branch: rade_core::git::branch(&root),
                    since,
                    changes,
                    commits,
                    generated,
                    hashes,
                })
            },
            move |r| {
                if let Some(v) = weak.upgrade() {
                    v.loading.set(false);
                    v.apply(r);
                }
            },
        );
    }

    fn apply(self: &Rc<Self>, r: rade_core::rpc::Result<Fresh>) {
        {
            let mut st = self.state.borrow_mut();
            match r {
                Ok(f)
                    if st.all.as_ref() == Some(&f.changes)
                        && st.since == f.since
                        && st.error.is_none()
                        && st.hashes == f.hashes
                        && st.commits == f.commits
                        && st.branch == f.branch =>
                {
                    return
                }
                Ok(f) => {
                    // A new Round with changes since: shown first (R119.2).
                    let round = f.since.as_ref().map(|s| s.0);
                    if round.is_some() && st.scoped_round != round {
                        st.scoped_round = round;
                        st.since_active = f.since.as_ref().is_some_and(|s| !s.1.files.is_empty());
                    }
                    if std::mem::take(&mut st.prefer_since)
                        && f.since.as_ref().is_some_and(|s| !s.1.files.is_empty())
                    {
                        st.since_active = true;
                    }
                    if f.since.is_none() {
                        st.since_active = false;
                    }
                    st.all = Some(f.changes.clone());
                    st.since = f.since.clone();
                    let c = match (&f.since, st.since_active) {
                        (Some((_, s)), true) => s.clone(),
                        _ => f.changes,
                    };
                    st.error = None;
                    let keep = st
                        .selected
                        .clone()
                        .filter(|s| c.files.iter().any(|f| &f.path == s));
                    st.selected = keep.or_else(|| c.files.first().map(|f| f.path.clone()));
                    // Highlighting stays for files whose diff is unchanged
                    // (an Agent editing one file re-highlights only that one).
                    let old = st.changes.as_ref();
                    self.markup.borrow_mut().retain(|path, _| {
                        let find = |c: &Changes| c.files.iter().find(|f| &f.path == path).cloned();
                        old.and_then(find).is_some_and(|o| Some(o) == find(&c))
                    });
                    // Generated files start folded, once (R118.2).
                    for g in &f.generated {
                        if st.seen.insert(g.clone()) {
                            st.folded_files.insert(g.clone());
                        }
                    }
                    st.changes = Some(c);
                    st.commits = f.commits;
                    st.branch = f.branch;
                    st.generated = f.generated;
                    st.hashes = f.hashes;
                }
                Err(e) => {
                    st.error = Some(e.message);
                    st.changes = None;
                    st.all = None;
                    st.since = None;
                    self.markup.borrow_mut().clear();
                }
            }
        }
        self.fold_viewed();
        // The Navigator's chip follows what this Review sees (R119.5).
        let changed = self
            .state
            .borrow()
            .since
            .as_ref()
            .is_some_and(|s| !s.1.files.is_empty());
        if let (Some((p, w, _)), Some(app)) = (self.place(), try_app()) {
            // Shown by the app's sync at the end of this refresh.
            app.touch(|m| m.set_changed_since(&p, &w, changed));
        }
        // The files changed: comments follow their lines (M11).
        if let (Some((p, w, _)), Some(app)) = (self.place(), try_app()) {
            crate::review::reanchor(&app, &p, &w);
        }
        self.prune_viewed();
        self.render_head();
        self.render_files();
        self.render_diff();
        self.impact_diff_changed();
        // Its Tab's label counts the files (R1.8).
        if let Some(app) = try_app() {
            app.sync();
        }
    }

    /// The palette's *Review: Since Last Round* (R119.5).
    pub fn show_since(&self) {
        let ready = self
            .state
            .borrow()
            .since
            .as_ref()
            .is_some_and(|s| !s.1.files.is_empty());
        if ready {
            self.set_scope(true);
        } else {
            // The diff is not read yet (a new Review): with it.
            self.state.borrow_mut().prefer_since = true;
        }
    }

    /// Show every change (`false`) or those since the last Round (R119.2).
    pub(super) fn set_scope(&self, since: bool) {
        {
            let mut st = self.state.borrow_mut();
            if st.since_active == since || (since && st.since.is_none()) {
                return;
            }
            st.since_active = since;
            st.changes = if since {
                st.since.as_ref().map(|s| s.1.clone())
            } else {
                st.all.clone()
            };
            st.cursor = None;
            let keep = st.selected.clone().filter(|p| {
                st.changes
                    .as_ref()
                    .is_some_and(|c| c.files.iter().any(|f| &f.path == p))
            });
            st.selected =
                keep.or_else(|| st.changes.as_ref()?.files.first().map(|f| f.path.clone()));
        }
        // Same paths, other hunks: highlight again.
        self.markup.borrow_mut().clear();
        self.render_files();
        self.render_diff();
    }

    /// Viewed files are folded; a file folded for being viewed unfolds
    /// once it is unread again (R118.3).
    fn fold_viewed(&self) {
        let viewed = self.viewed_flags();
        let mut st = self.state.borrow_mut();
        let Some(c) = st.changes.clone() else { return };
        for (f, on) in c.files.iter().zip(viewed) {
            if on && st.auto_folded.insert(f.path.clone()) {
                st.folded_files.insert(f.path.clone());
            } else if !on && st.auto_folded.remove(&f.path) {
                st.folded_files.remove(&f.path);
            }
        }
    }

    /// The header's line 1 (branch, Agent, task, commits).
    fn render_head(&self) {
        let head = match (self.place(), try_app()) {
            (Some((p, w, _)), Some(app)) => app.model.borrow().review_head(&p, &w),
            _ => Default::default(),
        };
        let st = self.state.borrow();
        let branch = st.branch.clone().unwrap_or_else(|| "HEAD".into());
        let base = st.all.as_ref().and_then(|c| c.branch.clone());
        self.head.show(&branch, base.as_deref(), &head, &st.commits);
        let review = self.review_state().unwrap_or_default();
        let since = st.since.as_ref().map(|s| s.1.files.len());
        let last = review.rounds.last().map(|r| {
            (
                r.n,
                rade_core::review_loop::RoundState::of(r, since.unwrap_or(0) > 0),
            )
        });
        self.head.rounds(last, since, review.summary.as_deref());
        let since_active = st.since_active;
        // Released first: the toggles' handlers read the state.
        drop(st);
        self.head.scope_since.set_active(since_active);
        self.head.scope_all.set_active(!since_active);
        self.state.borrow_mut().head = head;
    }

    /// Jump to a file: its header goes to the top of the list.
    fn select(&self, path: &str) {
        self.state.borrow_mut().selected = Some(path.into());
        let mut i = 0;
        let name = format!("file:{path}");
        while let Some(r) = self.rail.list.row_at_index(i) {
            if r.widget_name() == name {
                self.rail.list.select_row(Some(&r));
            }
            i += 1;
        }
        let pos = self.state.borrow().rows.iter().position(
            |r| matches!(r, Row::File(fi) if self.file_path(*fi).as_deref() == Some(path)),
        );
        if let Some(pos) = pos {
            self.state.borrow_mut().cursor = Some(pos);
            self.scroll_to_top(pos as u32);
        }
    }

    fn file_path(&self, fi: usize) -> Option<String> {
        self.file_path_in(&self.state.borrow(), fi)
    }

    fn file_path_in(&self, st: &State, fi: usize) -> Option<String> {
        Some(st.changes.as_ref()?.files.get(fi)?.path.clone())
    }

    /// Bring row `pos` to the top of the view: GtkListView scrolls it into
    /// view; once laid out, the adjustment moves it to the top.
    fn scroll_to_top(&self, pos: u32) {
        self.list
            .scroll_to(pos, gtk::ListScrollFlags::NONE, None::<gtk::ScrollInfo>);
        self.scroll_later(Scroll::Top(pos));
    }

    /// Apply a scroll target two frames from now (after the list laid out
    /// its new rows); a later target replaces a pending one.
    fn scroll_later(&self, s: Scroll) {
        if self.scroll_req.replace(Some(s)).is_some() {
            return; // a tick is already waiting
        }
        let (list, scroll, req) = (
            self.list.clone(),
            self.scroll.clone(),
            self.scroll_req.clone(),
        );
        let frames = Cell::new(0);
        self.list.add_tick_callback(move |_, _| {
            frames.set(frames.get() + 1);
            if frames.get() < 2 {
                return glib::ControlFlow::Continue;
            }
            let adj = scroll.vadjustment();
            match req.get() {
                Some(Scroll::Value(v)) => adj.set_value(v),
                // Rows change height once highlighted: align again until
                // it holds (a few frames at most).
                Some(Scroll::Top(pos)) if frames.get() < 8 => {
                    let top = rows::row_widget(&list, pos).and_then(|w| {
                        w.compute_point(&scroll, &gtk::graphene::Point::new(0.0, 0.0))
                    });
                    match top {
                        Some(p) if p.y().abs() >= 1.0 => {
                            adj.set_value(adj.value() + p.y() as f64);
                            return glib::ControlFlow::Continue;
                        }
                        None => return glib::ControlFlow::Continue,
                        Some(_) => {}
                    }
                }
                _ => {}
            }
            req.set(None);
            glib::ControlFlow::Break
        });
    }

    /// Rebuild the rows from the diff, folds and comments; the scroll
    /// position is kept.
    fn render_diff(&self) {
        let comments = self.comments_now();
        let states = match (self.place(), try_app()) {
            (Some((p, w, _)), Some(app)) => crate::review::comment_states(&app, &p, &w),
            _ => HashMap::new(),
        };
        self.state.borrow_mut().states = states;
        let t0 = Instant::now();
        let old_rows = self.state.borrow().rows.clone();
        {
            let mut st = self.state.borrow_mut();
            let st = &mut *st;
            st.comments = comments.iter().filter(|c| !c.outdated).cloned().collect();
            st.all_comments = comments.clone();
            let message = match (&st.error, &st.changes) {
                (Some(e), _) => Some(("No diff", e.clone(), "dialog-information-symbolic")),
                (None, Some(c)) if !c.files.is_empty() => None,
                (None, Some(_)) if st.since_active => Some((
                    "Nothing changed since the last round",
                    "All changes shows the whole diff.".to_string(),
                    "emblem-ok-symbolic",
                )),
                (None, Some(c)) => Some((
                    "No changes",
                    format!(
                        "Nothing differs from {} yet.",
                        c.branch.as_deref().unwrap_or("the base")
                    ),
                    "emblem-ok-symbolic",
                )),
                (None, None) => Some(("Loading…", String::new(), "ph-git-diff-symbolic")),
            };
            if let Some((title, text, icon)) = message {
                self.message.set_title(title);
                self.message.set_description(Some(&text));
                self.message.set_icon_name(Some(icon));
                self.stack.set_visible_child_name("message");
                st.rows.clear();
            } else {
                self.stack.set_visible_child_name("diff");
                let files = &st.changes.as_ref().unwrap().files;
                let base = rade_core::diff::rows(files, &st.folded_files, &st.folded_hunks);
                // Cards under their lines, the composer where it opened
                // (a draft being edited takes its card's place).
                let editing = st.pending.as_ref().and_then(|p| p.edit);
                let mut cards: Vec<_> = comments
                    .iter()
                    .filter(|c| Some(c.id) != editing)
                    .map(comments::card_of)
                    .collect();
                if let Some(p) = &st.pending {
                    cards.push(rade_core::review_loop::Card {
                        id: comments::COMPOSER,
                        path: p.path.clone(),
                        line: p.end_line.unwrap_or(p.line),
                        outdated: p.outdated,
                    });
                }
                st.rows = rade_core::review_loop::place_cards(&base, files, &cards, None)
                    .into_iter()
                    .map(|r| match r {
                        Row::Comment(comments::COMPOSER) => Row::Composer,
                        r => r,
                    })
                    .collect();
            }
        }
        // Same rows (only text changed): the selected lines stay selected.
        let same = self.state.borrow().rows == old_rows;
        if !same {
            self.state.borrow_mut().point = None;
        }
        self.reset_model(self.state.borrow().rows.len(), same);
        self.state.borrow_mut().render_ms = t0.elapsed().as_secs_f64() * 1000.0;
        self.state.borrow_mut().comments_sig = comments::signature(&comments);
        self.render_footer(&comments);
    }

    /// A click on a file header folds or unfolds the file; on a hunk
    /// header, the hunk.
    fn toggle_row(&self, pos: usize) {
        self.state.borrow_mut().cursor = Some(pos);
        {
            let row = self.state.borrow().rows.get(pos).copied();
            let mut st = self.state.borrow_mut();
            match row {
                Some(Row::File(fi)) => {
                    let Some(p) = self.file_path_in(&st, fi) else {
                        return;
                    };
                    if !st.folded_files.remove(&p) {
                        st.folded_files.insert(p);
                    }
                }
                Some(Row::Hunk(fi, hi)) => {
                    let Some(p) = self.file_path_in(&st, fi) else {
                        return;
                    };
                    let header = st.changes.as_ref().unwrap().files[fi].hunks[hi]
                        .header
                        .clone();
                    let key = (p, header);
                    if !st.folded_hunks.remove(&key) {
                        st.folded_hunks.insert(key);
                    }
                }
                _ => return,
            }
        }
        self.render_diff();
    }

    pub fn set_note(&self, text: &str) {
        self.note.set_label(text);
    }

    /// Called by the app's sync (inside it: read the model only):
    /// comments changed elsewhere (CLI, sent), the Agent's status.
    pub fn sync(&self) {
        let comments = self.comments_now();
        if comments::signature(&comments) != self.state.borrow().comments_sig {
            self.render_diff();
            self.render_files();
        } else {
            self.render_footer(&comments);
        }
        self.render_head();
        self.update_progress();
    }

    /// [`Self::sync`] after this view changed the model itself.
    fn sync_now(&self) {
        self.sync();
    }
}
