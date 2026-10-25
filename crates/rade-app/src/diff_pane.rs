//! The Diff Pane (PRD R122, ADR-0045): one file's uncommitted changes —
//! working tree vs index, index vs `HEAD`, or an untracked file against
//! nothing — in the centre, opened from the Changes page. Rows are the
//! Review Pane's (same classes, same highlighting through
//! `diff_highlight`) in a smaller view: inline or side by side, three
//! lines of context or the whole file, hunks and selected lines staged,
//! unstaged or discarded, F7 / Shift+F7 between changes. Refreshes every
//! 2 s while shown and right after its own operations.

use crate::app::try_app;
use crate::worktrees::in_background;
use adw::prelude::*;
use gtk::{gdk, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::config::DiffLayout;
use rade_core::diff::{FileDiff, LineKind};
use rade_core::diff_view::{self, Pair};
use rade_core::rpc::Result;
use rade_core::scm::{self, Change, Section};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

/// Pane width from which `auto` goes side by side (D17).
const SIDE_MIN_WIDTH: i32 = 900;

/// Context lines of "Show Full File" (D18).
const FULL_CONTEXT: u32 = 999_999;

const REFRESH: Duration = Duration::from_secs(2);

const ROW_CLASSES: [&str; 3] = ["rade-diff-hunk", "rade-diff-added", "rade-diff-removed"];

/// One list row; indices point into the file's hunks and their lines.
#[derive(Clone, Copy, Debug)]
enum DRow {
    Hunk(usize),
    Line(usize, usize),
    Pair(usize, Pair),
    /// "Binary file not shown".
    Note,
}

#[derive(Default)]
struct St {
    root: PathBuf,
    path: String,
    section: Option<Section>,
    /// `[scm] diff_layout`, overridden by the header toggle (this Pane only).
    mode: DiffLayout,
    /// The layout in use.
    side: bool,
    /// `debug.diff width`: a width to decide `auto` by.
    width_override: Option<i32>,
    full: bool,
    change: Option<Change>,
    initial: bool,
    raw: String,
    file: Option<FileDiff>,
    /// The other section holding this file, for the empty state's button.
    other: Option<Section>,
    rows: Vec<DRow>,
    /// Highlighted lines of every hunk, one after the other.
    markup: Vec<String>,
    hunk_starts: Vec<usize>,
    /// The current change, as its index among the change blocks.
    cur: Option<usize>,
    error: Option<String>,
    loading: bool,
    again: bool,
    loaded: bool,
}

pub struct DiffPane {
    pub id: String,
    pub root: gtk::Box,
    icon: gtk::Image,
    title: gtk::Label,
    chip: gtk::Label,
    prev: gtk::Button,
    next: gtk::Button,
    layout: gtk::Button,
    full: gtk::Button,
    stage: gtk::Button,
    discard: gtk::Button,
    open: gtk::Button,
    close: gtk::Button,
    error: gtk::Label,
    stack: gtk::Stack,
    empty_button: gtk::Button,
    list: gtk::ListView,
    model: gtk::StringList,
    selection: gtk::MultiSelection,
    scroll: gtk::ScrolledWindow,
    highlighter: sourceview5::Buffer,
    st: RefCell<St>,
}

use sourceview5::prelude::*;

fn icon_button(icon: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.set_tooltip_text(Some(tip));
    b.add_css_class("flat");
    b.set_focus_on_click(false);
    b
}

fn section_word(s: Section) -> &'static str {
    match s {
        Section::Unstaged => "Working Tree",
        Section::Staged => "Staged",
        Section::Untracked => "Untracked",
    }
}

/// The sign column of a diff line.
fn sign(kind: LineKind) -> &'static str {
    match kind {
        LineKind::Added => "+",
        LineKind::Removed => "−",
        LineKind::Context => "",
    }
}

/// Ask before a destructive action (§5.2): Cancel / the button.
fn ask(heading: &str, body: &str, label: &str, yes: impl Fn() + 'static) {
    let d = adw::AlertDialog::new(Some(heading), Some(body));
    d.add_response("cancel", "Cancel");
    d.add_response("discard", label);
    d.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
    d.set_default_response(Some("cancel"));
    d.set_close_response("cancel");
    d.connect_response(None, move |_, r| {
        if r == "discard" {
            yes();
        }
    });
    crate::menus::present(d, &["cancel", "discard"]);
}

/// What one refresh reads.
struct Fresh {
    change: Option<Change>,
    initial: bool,
    raw: String,
    other: Option<Section>,
}

fn read(root: &std::path::Path, path: &str, section: Section, full: bool) -> Result<Fresh> {
    let st = scm::status(root)?;
    let change = st
        .files
        .iter()
        .find(|c| c.path == path && c.in_section(section))
        .cloned();
    let raw = match &change {
        Some(c) => scm::file_diff_ctx(root, c, section, full.then_some(FULL_CONTEXT))?,
        None => String::new(),
    };
    let other = if raw.trim().is_empty() {
        scm::SECTIONS
            .into_iter()
            .filter(|s| *s != section)
            .find(|s| st.files.iter().any(|c| c.path == path && c.in_section(*s)))
    } else {
        None
    };
    Ok(Fresh {
        change,
        initial: st.initial,
        raw,
        other,
    })
}

impl DiffPane {
    pub fn new(id: &str) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.add_css_class("diff-pane");
        root.add_css_class("view");

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        header.add_css_class("diff-header");
        header.add_css_class("rade-viewer-header");
        let icon = gtk::Image::new();
        icon.add_css_class("rade-viewer-icon");
        let title = gtk::Label::new(None);
        title.set_xalign(0.0);
        title.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        title.add_css_class("rade-viewer-title");
        let chip = gtk::Label::new(None);
        chip.add_css_class("diff-section-chip");
        chip.set_margin_start(8);
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        let prev = icon_button("ph-arrow-line-up-symbolic", "Previous Change (Shift+F7)");
        let next = icon_button("ph-arrow-line-down-symbolic", "Next Change (F7)");
        let layout = icon_button("ph-columns-symbolic", "Side by Side");
        let full = icon_button("ph-arrows-out-line-vertical-symbolic", "Show Full File");
        let stage = icon_button("ph-plus-symbolic", "Stage File");
        let discard = icon_button("ph-arrow-counter-clockwise-symbolic", "Discard File…");
        let open = icon_button("ph-file-symbolic", "Open File");
        let close = icon_button("ph-x-symbolic", "Close (Esc)");
        for w in [
            icon.upcast_ref::<gtk::Widget>(),
            title.upcast_ref(),
            chip.upcast_ref(),
            spacer.upcast_ref(),
            prev.upcast_ref(),
            next.upcast_ref(),
            layout.upcast_ref(),
            full.upcast_ref(),
            stage.upcast_ref(),
            discard.upcast_ref(),
            open.upcast_ref(),
            close.upcast_ref(),
        ] {
            header.append(w);
        }
        let error = gtk::Label::new(None);
        error.set_xalign(0.0);
        error.set_wrap(true);
        error.set_selectable(true);
        error.add_css_class("scm-card-error");
        error.set_visible(false);

        let model = gtk::StringList::new(&[]);
        let selection = gtk::MultiSelection::new(Some(model.clone()));
        let list = gtk::ListView::new(Some(selection.clone()), None::<gtk::ListItemFactory>);
        list.add_css_class("diff-list");
        list.set_enable_rubberband(true);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_child(Some(&list));
        let empty = adw::StatusPage::new();
        empty.set_icon_name(Some("ph-git-diff-symbolic"));
        empty.set_title("No changes");
        empty.add_css_class("compact");
        let empty_button = gtk::Button::new();
        empty_button.add_css_class("pill");
        empty_button.set_halign(gtk::Align::Center);
        empty_button.set_visible(false);
        empty.set_child(Some(&empty_button));
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.set_hexpand(true);
        stack.add_named(&scroll, Some("diff"));
        stack.add_named(&empty, Some("empty"));
        stack.set_visible_child_name("empty");

        root.append(&header);
        root.append(&error);
        root.append(&stack);

        let highlighter = sourceview5::Buffer::new(None);
        highlighter.set_highlight_matching_brackets(false);

        let mode = try_app().map_or(DiffLayout::Auto, |a| a.config.borrow().scm.diff_layout);
        let v = Rc::new(Self {
            id: id.into(),
            root,
            icon,
            title,
            chip,
            prev,
            next,
            layout,
            full,
            stage,
            discard,
            open,
            close,
            error,
            stack,
            empty_button,
            list,
            model,
            selection,
            scroll,
            highlighter,
            st: RefCell::new(St {
                mode,
                side: mode == DiffLayout::SideBySide,
                ..Default::default()
            }),
        });
        v.list.set_factory(Some(&v.factory()));
        v.connect();
        v.restyle();
        let weak = Rc::downgrade(&v);
        adw::StyleManager::default().connect_dark_notify(move |_| {
            if let Some(v) = weak.upgrade() {
                v.restyle();
            }
        });
        let weak = Rc::downgrade(&v);
        glib::timeout_add_local(REFRESH, move || match weak.upgrade() {
            Some(v) => {
                if v.root.is_mapped() && try_app().is_some_and(|a| a.window.is_active()) {
                    v.refresh();
                }
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        v
    }

    // ---- wiring -----------------------------------------------------------

    fn connect(self: &Rc<Self>) {
        let on = |b: &gtk::Button, f: fn(&Rc<DiffPane>)| {
            let weak = Rc::downgrade(self);
            b.connect_clicked(move |_| {
                if let Some(v) = weak.upgrade() {
                    f(&v);
                }
            });
        };
        on(&self.prev, |v| v.step(false));
        on(&self.next, |v| v.step(true));
        on(&self.layout, |v| v.toggle_layout());
        on(&self.full, |v| v.toggle_full());
        on(&self.stage, |v| v.file_stage());
        on(&self.discard, |v| v.file_discard());
        on(&self.open, |v| v.open_file());
        on(&self.close, |v| v.close_pane());
        on(&self.empty_button, |v| v.empty_click());

        // F7 / Shift+F7, Esc, Ctrl+C.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(v) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            use gdk::ModifierType as M;
            let mods = state & (M::CONTROL_MASK | M::ALT_MASK | M::SHIFT_MASK);
            let handled = match key {
                gdk::Key::F7 if mods.is_empty() => {
                    v.step(true);
                    true
                }
                gdk::Key::F7 if mods == M::SHIFT_MASK => {
                    v.step(false);
                    true
                }
                gdk::Key::Escape if mods.is_empty() => {
                    v.close_pane();
                    true
                }
                gdk::Key::c if mods == M::CONTROL_MASK => v.copy_selection(),
                _ => false,
            };
            if handled {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        self.root.add_controller(keys);

        // Focus tracking like a Viewer: clicking in focuses the Pane.
        let focus = gtk::EventControllerFocus::new();
        let id = self.id.clone();
        focus.connect_enter(move |_| {
            if let Some(app) = try_app() {
                app.focus_entered(&id);
            }
        });
        self.root.add_controller(focus);

        // `auto` follows the Pane's width (D17): checked each frame, rows
        // rebuilt only when the side of 900 px changes.
        let weak = Rc::downgrade(self);
        self.root
            .add_tick_callback(move |_, _| match weak.upgrade() {
                Some(v) => {
                    v.check_width();
                    glib::ControlFlow::Continue
                }
                None => glib::ControlFlow::Break,
            });
    }

    pub fn focus(&self) {
        self.list.grab_focus();
    }

    /// Light/dark changed: re-highlight with the matching scheme.
    pub fn restyle(self: &Rc<Self>) {
        self.highlighter
            .set_style_scheme(crate::viewer::scheme_for(crate::viewer::is_dark()).as_ref());
        self.rehighlight();
        self.rebind();
    }

    // ---- model → view -----------------------------------------------------

    /// Called by the app's sync (the model is borrowed: read nothing from it).
    pub fn show(self: &Rc<Self>, root: &std::path::Path, path: &str, section: Section) {
        {
            let mut st = self.st.borrow_mut();
            if st.loaded && st.root == root && st.path == path && st.section == Some(section) {
                return;
            }
            st.loaded = true;
            st.root = root.into();
            st.path = path.into();
            st.section = Some(section);
            st.change = None;
            st.raw.clear();
            st.file = None;
            st.other = None;
            st.rows.clear();
            st.markup.clear();
            st.cur = None;
            st.error = None;
        }
        self.reset_model(0, false);
        self.update_chrome();
        self.refresh();
    }

    fn effective_width(&self) -> i32 {
        self.st
            .borrow()
            .width_override
            .unwrap_or_else(|| self.root.width())
    }

    fn side_for(&self, mode: DiffLayout, width: i32) -> bool {
        match mode {
            DiffLayout::Inline => false,
            DiffLayout::SideBySide => true,
            DiffLayout::Auto => width >= SIDE_MIN_WIDTH,
        }
    }

    fn check_width(self: &Rc<Self>) {
        if !self.root.is_mapped() {
            return;
        }
        let (mode, side) = {
            let st = self.st.borrow();
            (st.mode, st.side)
        };
        let want = self.side_for(mode, self.effective_width());
        if want != side && (self.effective_width() > 0) {
            self.st.borrow_mut().side = want;
            self.render();
        }
    }

    /// Re-read the diff in the background; rebuild only if it changed.
    pub fn refresh(self: &Rc<Self>) {
        let (root, path, section, full) = {
            let mut st = self.st.borrow_mut();
            let Some(section) = st.section else { return };
            if st.loading {
                st.again = true;
                return;
            }
            st.loading = true;
            (st.root.clone(), st.path.clone(), section, st.full)
        };
        let weak = Rc::downgrade(self);
        let (r2, p2) = (root.clone(), path.clone());
        in_background(
            move || read(&r2, &p2, section, full),
            move |r| {
                let Some(v) = weak.upgrade() else { return };
                let again = {
                    let mut st = v.st.borrow_mut();
                    st.loading = false;
                    std::mem::take(&mut st.again)
                };
                // Navigated meanwhile: the answer is about another file.
                let same = {
                    let st = v.st.borrow();
                    st.root == root && st.path == path && st.section == Some(section)
                };
                if same {
                    v.apply(r);
                }
                if again || !same {
                    v.refresh();
                }
            },
        );
    }

    fn apply(self: &Rc<Self>, r: Result<Fresh>) {
        let f = match r {
            Ok(f) => f,
            Err(e) => {
                self.st.borrow_mut().error = Some(e.message);
                self.update_chrome();
                return;
            }
        };
        {
            let st = self.st.borrow();
            if st.raw == f.raw
                && st.change == f.change
                && st.other == f.other
                && (st.file.is_some() || f.raw.trim().is_empty())
            {
                drop(st);
                self.update_chrome();
                return;
            }
        }
        {
            let mut st = self.st.borrow_mut();
            let untracked: Vec<String> = f
                .change
                .iter()
                .filter(|c| c.untracked())
                .map(|c| c.path.clone())
                .collect();
            let mut file = rade_core::diff::parse(&f.raw, &untracked)
                .into_iter()
                .next();
            if f.raw.trim().is_empty() {
                file = None;
            }
            st.file = file;
            st.raw = f.raw;
            st.change = f.change;
            st.initial = f.initial;
            st.other = f.other;
        }
        self.rehighlight();
        self.render();
    }

    /// Highlight every line of the file (once per diff and scheme).
    fn rehighlight(&self) {
        let mut st = self.st.borrow_mut();
        let Some(f) = st.file.as_ref() else {
            st.markup.clear();
            st.hunk_starts.clear();
            return;
        };
        let mut starts = Vec::new();
        let mut texts: Vec<std::borrow::Cow<str>> = Vec::new();
        for h in &f.hunks {
            starts.push(texts.len());
            texts.extend(
                h.lines
                    .iter()
                    .map(|l| crate::diff_highlight::shorten(&l.text)),
            );
        }
        let markup = crate::diff_highlight::highlight(&f.path, &texts, &self.highlighter);
        st.markup = markup;
        st.hunk_starts = starts;
    }

    /// The rows for the layout in use.
    fn build_rows(st: &St) -> Vec<DRow> {
        let Some(f) = st.file.as_ref() else {
            return vec![];
        };
        if f.binary {
            return vec![DRow::Note];
        }
        let mut rows = Vec::new();
        for (hi, h) in f.hunks.iter().enumerate() {
            rows.push(DRow::Hunk(hi));
            if st.side {
                rows.extend(
                    diff_view::side_by_side(h)
                        .into_iter()
                        .map(|p| DRow::Pair(hi, p)),
                );
            } else {
                rows.extend((0..h.lines.len()).map(|li| DRow::Line(hi, li)));
            }
        }
        rows
    }

    /// The kind of each row, for [`diff_view::change_starts`].
    fn row_kinds(st: &St) -> Vec<Option<LineKind>> {
        let Some(f) = st.file.as_ref() else {
            return vec![];
        };
        st.rows
            .iter()
            .map(|r| match *r {
                DRow::Line(hi, li) => Some(f.hunks[hi].lines[li].kind),
                DRow::Pair(hi, p) => {
                    let h = &f.hunks[hi];
                    let changed = p.left.is_some_and(|i| h.lines[i].kind == LineKind::Removed)
                        || p.right.is_some_and(|i| h.lines[i].kind == LineKind::Added);
                    Some(if changed {
                        LineKind::Added
                    } else {
                        LineKind::Context
                    })
                }
                DRow::Hunk(_) | DRow::Note => None,
            })
            .collect()
    }

    fn starts(&self) -> Vec<usize> {
        let st = self.st.borrow();
        diff_view::change_starts(&Self::row_kinds(&st))
    }

    /// Rebuild the rows (the diff or the layout changed), keeping the scroll.
    fn render(self: &Rc<Self>) {
        let n = {
            let mut st = self.st.borrow_mut();
            st.rows = Self::build_rows(&st);
            st.rows.len()
        };
        self.reset_model(n, true);
        self.update_chrome();
    }

    fn reset_model(&self, n: usize, keep_scroll: bool) {
        let adj = self.scroll.vadjustment();
        let keep = adj.value();
        self.selection.unselect_all();
        self.model.splice(0, self.model.n_items(), &vec![""; n]);
        if keep_scroll {
            adj.set_value(keep);
            let adj = adj.clone();
            let frames = std::cell::Cell::new(0);
            self.list.add_tick_callback(move |_, _| {
                frames.set(frames.get() + 1);
                if frames.get() < 2 {
                    return glib::ControlFlow::Continue;
                }
                adj.set_value(keep);
                glib::ControlFlow::Break
            });
        }
    }

    /// Re-run the factory on every row (highlighting changed).
    fn rebind(&self) {
        let n = self.model.n_items() as usize;
        if n == 0 {
            return;
        }
        let sel = self.selection.selection();
        let adj = self.scroll.vadjustment();
        let keep = adj.value();
        self.model.splice(0, self.model.n_items(), &vec![""; n]);
        if !sel.is_empty() {
            let (a, b) = (sel.minimum(), sel.maximum());
            self.selection.select_range(a, b - a + 1, true);
        }
        adj.set_value(keep);
    }

    /// Header, empty state and error line from the state.
    fn update_chrome(&self) {
        let st = self.st.borrow();
        let section = st.section.unwrap_or(Section::Unstaged);
        let (dir, name) = match st.path.rfind('/') {
            Some(i) => st.path.split_at(i + 1),
            None => ("", st.path.as_str()),
        };
        self.title.set_markup(&format!(
            "<span alpha=\"62%\">{}</span>{}",
            glib::markup_escape_text(dir),
            glib::markup_escape_text(name)
        ));
        self.title.set_tooltip_text(Some(&st.path));
        self.chip.set_label(section_word(section));
        crate::files::set_icon(&self.icon, &crate::files::icon_for(name, false, false));

        let has = st.file.is_some();
        self.stack
            .set_visible_child_name(if has { "diff" } else { "empty" });
        match st.other {
            Some(o) if !has => {
                self.empty_button.set_label(match o {
                    Section::Staged => "Open Staged Changes",
                    _ => "Open Working Tree Changes",
                });
                self.empty_button.set_visible(true);
            }
            _ => self.empty_button.set_visible(false),
        }
        self.error.set_label(st.error.as_deref().unwrap_or(""));
        self.error.set_visible(st.error.is_some());

        // Layout and full-file toggles show what a click switches to.
        if st.side {
            self.layout.set_icon_name("ph-rows-symbolic");
            self.layout.set_tooltip_text(Some("Inline"));
        } else {
            self.layout.set_icon_name("ph-columns-symbolic");
            self.layout.set_tooltip_text(Some("Side by Side"));
        }
        if st.full {
            self.full
                .set_icon_name("ph-arrows-in-line-vertical-symbolic");
            self.full
                .set_tooltip_text(Some("Collapse Unchanged Regions"));
        } else {
            self.full
                .set_icon_name("ph-arrows-out-line-vertical-symbolic");
            self.full.set_tooltip_text(Some("Show Full File"));
        }
        if section == Section::Staged {
            self.stage.set_icon_name("ph-minus-symbolic");
            self.stage.set_tooltip_text(Some("Unstage File"));
        } else {
            self.stage.set_icon_name("ph-plus-symbolic");
            self.stage.set_tooltip_text(Some("Stage File"));
        }
        self.discard.set_visible(section != Section::Staged);
        let live = st.change.is_some();
        for b in [&self.stage, &self.discard, &self.prev, &self.next] {
            b.set_sensitive(live && has);
        }
        self.stage.set_sensitive(live);
        self.discard.set_sensitive(live);
        drop(st);
        self.mark_current();
    }

    /// `.diff-current` on the current change's first row.
    fn mark_current(&self) {
        let pos = self.current_row();
        let mut child = self.list.first_child();
        while let Some(c) = child {
            if let Some(row) = c.first_child() {
                let is = pos.is_some_and(|p| row.widget_name() == p.to_string());
                if is {
                    row.add_css_class("diff-current");
                } else {
                    row.remove_css_class("diff-current");
                }
            }
            child = c.next_sibling();
        }
    }

    fn current_row(&self) -> Option<usize> {
        let ord = self.st.borrow().cur?;
        self.starts().get(ord).copied()
    }

    // ---- selection --------------------------------------------------------

    /// The selected rows.
    fn selected_rows(&self) -> Vec<usize> {
        let sel = self.selection.selection();
        let mut out = Vec::new();
        if let Some((iter, first)) = gtk::BitsetIter::init_first(&sel) {
            out.push(first as usize);
            out.extend(iter.map(|n| n as usize));
        }
        out
    }

    fn copy_selection(&self) -> bool {
        let rows = self.selected_rows();
        if rows.is_empty() {
            return false;
        }
        let st = self.st.borrow();
        let Some(f) = st.file.as_ref() else {
            return false;
        };
        let mut lines = Vec::new();
        for p in rows {
            match st.rows.get(p).copied() {
                Some(DRow::Line(hi, li)) => lines.push(f.hunks[hi].lines[li].text.clone()),
                Some(DRow::Pair(hi, pair)) => {
                    let l = pair.right.or(pair.left);
                    if let Some(i) = l {
                        lines.push(f.hunks[hi].lines[i].text.clone());
                    }
                }
                _ => {}
            }
        }
        self.root.clipboard().set_text(&lines.join("\n"));
        true
    }

    // ---- moves ------------------------------------------------------------

    /// F7 (`forward`) / Shift+F7: the next / previous change, wrapping.
    fn step(&self, forward: bool) {
        let starts = self.starts();
        let cur = self.current_row();
        let to = if forward {
            diff_view::next_change(&starts, cur)
        } else {
            diff_view::prev_change(&starts, cur)
        };
        let Some(row) = to else { return };
        self.st.borrow_mut().cur = starts.iter().position(|s| *s == row);
        self.list
            .scroll_to(row as u32, gtk::ListScrollFlags::NONE, None);
        self.mark_current();
    }

    fn toggle_layout(self: &Rc<Self>) {
        {
            let mut st = self.st.borrow_mut();
            st.mode = if st.side {
                DiffLayout::Inline
            } else {
                DiffLayout::SideBySide
            };
            st.side = st.mode == DiffLayout::SideBySide;
        }
        self.render();
    }

    fn toggle_full(self: &Rc<Self>) {
        {
            let mut st = self.st.borrow_mut();
            st.full = !st.full;
            // The other context makes other hunks: start over.
            st.raw.clear();
        }
        self.refresh();
    }

    // ---- operations -------------------------------------------------------

    /// Run `work` off the GTK thread, then show its error and re-read.
    fn run(
        self: &Rc<Self>,
        what: &'static str,
        work: impl FnOnce() -> Result<()> + Send + 'static,
    ) {
        let weak = Rc::downgrade(self);
        in_background(work, move |r| {
            if let Some(v) = weak.upgrade() {
                v.st.borrow_mut().error = r.err().map(|e| format!("{what}: {}", e.message));
                v.refresh();
            }
        });
    }

    fn file_stage(self: &Rc<Self>) {
        let (root, change, section, initial) = {
            let st = self.st.borrow();
            let (Some(c), Some(s)) = (st.change.clone(), st.section) else {
                return;
            };
            (st.root.clone(), c, s, st.initial)
        };
        let paths = scm::paths_of(&change);
        if section == Section::Staged {
            self.run("Unstage", move || scm::unstage(&root, &paths, initial));
        } else {
            self.run("Stage", move || scm::stage(&root, &paths));
        }
    }

    fn file_discard(self: &Rc<Self>) {
        let (root, change) = {
            let st = self.st.borrow();
            let Some(c) = st.change.clone() else { return };
            if st.section == Some(Section::Staged) {
                return;
            }
            (st.root.clone(), c)
        };
        let name = change
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&change.path)
            .to_string();
        let untracked = change.untracked();
        let (heading, body, label) = rade_core::scm_view::discard_dialog(
            &[&name],
            usize::from(!untracked),
            usize::from(untracked),
        );
        let weak = Rc::downgrade(self);
        ask(&heading, &body, &label, move || {
            let Some(v) = weak.upgrade() else { return };
            let (root, change) = (root.clone(), change.clone());
            let paths = vec![change.path.clone()];
            let tracked: Vec<Change> = if change.untracked() {
                vec![]
            } else {
                vec![change]
            };
            let untracked: Vec<String> = if tracked.is_empty() { paths } else { vec![] };
            let weak = Rc::downgrade(&v);
            let root2 = root.clone();
            in_background(
                move || {
                    let trash = |p: &std::path::Path| {
                        gtk::gio::File::for_path(p)
                            .trash(gtk::gio::Cancellable::NONE)
                            .map_err(|e| std::io::Error::other(e.message().to_string()))
                    };
                    scm::discard_many(&root, &tracked, &untracked, &trash)
                },
                move |r| {
                    let Some(v) = weak.upgrade() else { return };
                    match r {
                        Ok(failed) if !failed.is_empty() => {
                            v.trash_failed(root2, failed);
                            v.refresh();
                        }
                        Ok(_) => {
                            v.st.borrow_mut().error = None;
                            v.refresh();
                        }
                        Err(e) => {
                            v.st.borrow_mut().error = Some(format!("Discard: {}", e.message));
                            v.refresh();
                        }
                    }
                },
            );
        });
    }

    /// The trash failed (§5.2): offer to delete permanently.
    fn trash_failed(self: &Rc<Self>, root: PathBuf, failed: Vec<(String, String)>) {
        let n = failed.len();
        let d = adw::AlertDialog::new(
            Some(&format!(
                "Can't move {n} {} to the trash",
                if n == 1 { "file" } else { "files" }
            )),
            Some(&format!("{}. Delete permanently?", failed[0].1)),
        );
        d.add_response("cancel", "Cancel");
        d.add_response("delete", "Delete Permanently");
        d.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        d.set_default_response(Some("cancel"));
        d.set_close_response("cancel");
        let weak = Rc::downgrade(self);
        d.connect_response(None, move |_, r| {
            if r != "delete" {
                return;
            }
            if let Some(v) = weak.upgrade() {
                let (root, paths) = (
                    root.clone(),
                    failed.iter().map(|f| f.0.clone()).collect::<Vec<_>>(),
                );
                v.run("Delete", move || scm::delete_untracked(&root, &paths));
            }
        });
        crate::menus::present(d, &["cancel", "delete"]);
    }

    fn open_file(&self) {
        let (root, path) = {
            let st = self.st.borrow();
            (st.root.clone(), st.path.clone())
        };
        crate::files::open_in_viewer(&root.join(path));
    }

    fn close_pane(&self) {
        if let Some(app) = try_app() {
            let _ = app.close_pane(&self.id, false);
        }
    }

    fn empty_click(&self) {
        let (root, path, other) = {
            let st = self.st.borrow();
            (st.root.clone(), st.path.clone(), st.other)
        };
        let (Some(other), Some(app)) = (other, try_app()) else {
            return;
        };
        let id = self.id.clone();
        let _ = app.update(|m| m.diff_navigate(&id, &root, &path, other));
    }

    // ---- rows -------------------------------------------------------------

    fn factory(self: &Rc<Self>) -> gtk::SignalListItemFactory {
        let f = gtk::SignalListItemFactory::new();
        f.connect_setup(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            row.add_css_class("rade-diff-row");
            let num = || {
                let n = gtk::Label::new(None);
                n.set_width_chars(5);
                n.set_xalign(1.0);
                n.set_yalign(0.0);
                n.add_css_class("rade-diff-num");
                n
            };
            let code = || {
                let c = gtk::Label::new(None);
                c.set_xalign(0.0);
                c.set_yalign(0.0);
                c.set_hexpand(true);
                c.set_wrap(true);
                c.set_wrap_mode(gtk::pango::WrapMode::WordChar);
                c.add_css_class("rade-diff-code");
                c
            };
            // Inline: old, new, sign, code.
            let inl = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            inl.set_hexpand(true);
            let sign = gtk::Label::new(None);
            sign.set_width_chars(2);
            sign.set_yalign(0.0);
            sign.add_css_class("rade-diff-sign");
            inl.append(&num());
            inl.append(&num());
            inl.append(&sign);
            inl.append(&code());
            // Side by side: old | code, new | code.
            let sbs = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            sbs.set_homogeneous(true);
            sbs.set_hexpand(true);
            for _ in 0..2 {
                let half = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                half.set_hexpand(true);
                half.append(&num());
                half.append(&code());
                sbs.append(&half);
            }
            row.append(&inl);
            row.append(&sbs);
            item.set_child(Some(&row));
        });
        let weak: Weak<DiffPane> = Rc::downgrade(self);
        f.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().unwrap();
            let (Some(v), Some(row)) = (weak.upgrade(), item.child().and_downcast::<gtk::Box>())
            else {
                return;
            };
            let pos = item.position();
            row.set_widget_name(&pos.to_string());
            let hunk = matches!(v.st.borrow().rows.get(pos as usize), Some(DRow::Hunk(_)));
            item.set_selectable(!hunk);
            item.set_activatable(!hunk);
            v.bind(&row, pos as usize);
        });
        f
    }

    fn bind(&self, row: &gtk::Box, pos: usize) {
        let st = self.st.borrow();
        let (Some(f), Some(r)) = (st.file.as_ref(), st.rows.get(pos).copied()) else {
            return;
        };
        let inl = row.first_child().and_downcast::<gtk::Box>().unwrap();
        let sbs = inl.next_sibling().and_downcast::<gtk::Box>().unwrap();
        let l = |w: Option<gtk::Widget>| w.and_downcast::<gtk::Label>().unwrap();
        let (old, new) = {
            let o = l(inl.first_child());
            (o.clone(), l(o.next_sibling()))
        };
        let sign_l = l(new.next_sibling());
        let code = l(sign_l.next_sibling());
        let halves: Vec<gtk::Box> = {
            let a = sbs.first_child().and_downcast::<gtk::Box>().unwrap();
            let b = a.next_sibling().and_downcast::<gtk::Box>().unwrap();
            vec![a, b]
        };
        let mut classes = vec!["rade-diff-row", "monospace"];
        for h in &halves {
            h.set_css_classes(&[]);
        }
        for w in [&old, &new, &sign_l, &code] {
            w.set_label("");
        }
        inl.set_visible(true);
        sbs.set_visible(false);
        let markup = |hi: usize, li: usize| {
            st.hunk_starts
                .get(hi)
                .and_then(|s| st.markup.get(s + li))
                .map_or("", String::as_str)
        };
        let num = |x: Option<u32>| x.map_or(String::new(), |x| x.to_string());
        match r {
            DRow::Hunk(hi) => {
                classes.push(ROW_CLASSES[0]);
                code.set_label(&f.hunks[hi].header);
                code.set_hexpand(true);
            }
            DRow::Note => code.set_markup("<i>Binary file not shown</i>"),
            DRow::Line(hi, li) => {
                let dl = &f.hunks[hi].lines[li];
                old.set_label(&num(dl.old));
                new.set_label(&num(dl.new));
                match dl.kind {
                    LineKind::Added => classes.push(ROW_CLASSES[1]),
                    LineKind::Removed => classes.push(ROW_CLASSES[2]),
                    LineKind::Context => {}
                }
                sign_l.set_label(sign(dl.kind));
                code.set_markup(markup(hi, li));
            }
            DRow::Pair(hi, p) => {
                inl.set_visible(false);
                sbs.set_visible(true);
                let lines = &f.hunks[hi].lines;
                for (half, idx, left) in [(&halves[0], p.left, true), (&halves[1], p.right, false)]
                {
                    let n = l(half.first_child());
                    let c = l(n.next_sibling());
                    match idx {
                        Some(i) => {
                            let dl = &lines[i];
                            n.set_label(&num(if left { dl.old } else { dl.new }));
                            c.set_markup(markup(hi, i));
                            match dl.kind {
                                LineKind::Removed => half.add_css_class(ROW_CLASSES[2]),
                                LineKind::Added => half.add_css_class(ROW_CLASSES[1]),
                                LineKind::Context => {}
                            }
                        }
                        None => {
                            n.set_label("");
                            c.set_label("");
                            half.add_css_class("diff-filler");
                        }
                    }
                }
            }
        }
        if self.current_row_in(&st) == Some(pos) {
            classes.push("diff-current");
        }
        row.set_css_classes(&classes);
    }

    fn current_row_in(&self, st: &St) -> Option<usize> {
        let ord = st.cur?;
        diff_view::change_starts(&Self::row_kinds(st))
            .get(ord)
            .copied()
    }

    // ---- debug ------------------------------------------------------------

    /// The text of row `r`: inline `old new sign code`, side by side
    /// `old code │ new code`.
    fn row_text(st: &St, r: DRow) -> String {
        let Some(f) = st.file.as_ref() else {
            return String::new();
        };
        let n = |x: Option<u32>| x.map_or("  ".to_string(), |x| format!("{x:>2}"));
        match r {
            DRow::Hunk(hi) => f.hunks[hi].header.clone(),
            DRow::Note => "Binary file not shown".into(),
            DRow::Line(hi, li) => {
                let l = &f.hunks[hi].lines[li];
                format!("{} {} {}{}", n(l.old), n(l.new), sign(l.kind), l.text)
            }
            DRow::Pair(hi, p) => {
                let side = |i: Option<usize>, old: bool| match i {
                    Some(i) => {
                        let l = &f.hunks[hi].lines[i];
                        format!("{} {}", n(if old { l.old } else { l.new }), l.text)
                    }
                    None => String::new(),
                };
                format!("{} │ {}", side(p.left, true), side(p.right, false))
            }
        }
    }

    pub fn info(&self) -> Value {
        let st = self.st.borrow();
        let section = st.section.unwrap_or(Section::Unstaged);
        let name = st.path.rsplit('/').next().unwrap_or(&st.path);
        let hunks: Vec<Value> = st
            .file
            .as_ref()
            .map(|f| {
                f.hunks
                    .iter()
                    .enumerate()
                    .map(|(i, h)| json!({"index": i, "header": h.header}))
                    .collect()
            })
            .unwrap_or_default();
        let selected = self.selected_rows();
        let current = st.cur.and_then(|o| {
            diff_view::change_starts(&Self::row_kinds(&st))
                .get(o)
                .copied()
        });
        let width = st.width_override.unwrap_or_else(|| self.root.width());
        let empty_button = self
            .empty_button
            .is_visible()
            .then(|| self.empty_button.label().map(|l| l.to_string()))
            .flatten();
        json!({
            "pane": self.id,
            "root": st.root,
            "path": st.path,
            "section": section,
            "title": format!("{name} ({})", section_word(section)),
            "layout": if st.side { "side_by_side" } else { "inline" },
            "layout_mode": match st.mode {
                DiffLayout::Auto => "auto",
                DiffLayout::Inline => "inline",
                DiffLayout::SideBySide => "side_by_side",
            },
            "full": st.full,
            "empty": st.file.is_none(),
            "empty_button": empty_button,
            "hunks": hunks,
            "rows": st.rows.len(),
            "text": st.rows.iter().map(|r| Self::row_text(&st, *r)).collect::<Vec<_>>(),
            "selected": selected,
            "current": current,
            "width": width,
            "error": st.error,
        })
    }

    fn debug(self: &Rc<Self>, p: &Value) -> Result<Value> {
        use rade_core::rpc::Error;
        let s = |k: &str| p.get(k).and_then(Value::as_str);
        let op = s("op").unwrap_or("info");
        let act = s("action").unwrap_or("");
        match op {
            "info" => {}
            "refresh" => self.refresh(),
            "layout" => self.toggle_layout(),
            "full" => self.toggle_full(),
            "next" => self.step(true),
            "prev" => self.step(false),
            "file" => match act {
                "stage" | "unstage" => self.file_stage(),
                "discard" => self.file_discard(),
                "open" => self.open_file(),
                a => return Err(Error::usage(format!("unknown action '{a}'"))),
            },
            "empty_button" => self.empty_click(),
            "escape" => self.close_pane(),
            "width" => {
                let px: i32 = s("arg")
                    .and_then(|a| a.parse().ok())
                    .ok_or_else(|| Error::usage("width wants arg = px"))?;
                self.st.borrow_mut().width_override = Some(px);
                self.check_width();
            }
            "open" => {}
            o => return Err(Error::usage(format!("unknown diff op '{o}'"))),
        }
        Ok(self.info())
    }
}

/// `debug.diff` (`{"pane"?, "op", "arg"?, "rows"?, "action"?}`): see
/// `DiffPane::debug`. `open` (`path`, `section`?) opens a Diff Pane in the
/// active Worktree, as the Changes page does.
pub fn debug(p: &Value) -> Result<Value> {
    use rade_core::rpc::Error;
    let app = crate::app::app();
    let s = |k: &str| p.get(k).and_then(Value::as_str);
    if s("op") == Some("open") {
        let path = s("path").ok_or_else(|| Error::usage("open wants a path"))?;
        let section: Section = serde_json::from_value(json!(s("section").unwrap_or("unstaged")))
            .map_err(|_| Error::usage("section is staged | unstaged | untracked"))?;
        let (source, wt, root) = {
            let m = app.model.borrow();
            let (p, w) = m.active();
            (p.id.clone(), w.id.clone(), w.path.clone())
        };
        let id = app.update(|m| m.open_diff(&source, &wt, &root, path, section, true))?;
        let v = app.diffs.borrow().get(&id).cloned();
        return Ok(v.map_or(json!({"pane": id}), |v| v.info()));
    }
    let id = match s("pane").filter(|x| !x.is_empty()) {
        Some(q) => app.model.borrow().resolve_pane(q, None)?,
        None => {
            let m = app.model.borrow();
            m.active_tab()
                .and_then(|t| {
                    t.panes()
                        .into_iter()
                        .find(|p| p.is_diff())
                        .map(|p| p.id.clone())
                })
                .ok_or_else(|| Error::usage("no Diff Pane in the active Tab"))?
        }
    };
    let v = app.diffs.borrow().get(&id).cloned();
    let v = v.ok_or_else(|| Error::usage(format!("pane {id} is not a Diff Pane")))?;
    v.debug(p)
}
