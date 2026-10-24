//! Command palette overlay (PRD R14): `Ctrl+Shift+P` shows a search entry
//! over the window; results come from `rade_core::palette` (ranked off the
//! GTK thread), Enter runs one ([`crate::palette_run`]), an action that needs
//! input swaps the list for an inline form, Esc closes and focus returns to
//! the focused Pane. The search entry is a plain GTK text widget, so IBus
//! (Vietnamese) works through GTK's own input method.

use crate::app::{app, try_app};
use crate::worktrees::in_background;
use gtk::prelude::*;
use gtk::{gdk, glib};
use gtk4 as gtk;
use rade_core::palette::{self, Files, Filter, Hit, Item, Ranked, FILTERS};
use rade_core::palette_form::{FieldKind, Form};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Rows shown at most (the best matches). GtkListView builds a row widget
/// for every item up to 200 when its model changes (~45 µs each here), so
/// 60 keeps an update ~3 ms; 12 are visible, Page Down reaches the rest.
pub const LIMIT: usize = 60;
/// Files listed at most per Worktree (the walk stops there).
pub const FILE_CAP: usize = 100_000;
/// Rows shown without scrolling, and their heights (spec D.11, D.10).
const VISIBLE_ROWS: usize = 12;
const ROW_HEIGHT: i32 = 32;
const SECTION_HEIGHT: i32 = 27;
/// Rows Page Up / Page Down move.
const PAGE: u32 = 10;

thread_local! {
    static PALETTE: RefCell<Option<Rc<Palette>>> = const { RefCell::new(None) };
}

pub fn get() -> Option<Rc<Palette>> {
    PALETTE.with(|p| p.borrow().clone())
}

/// Timings of the last result update (R14.5), for `debug.palette info`.
#[derive(Default, Clone, Copy)]
pub struct Timing {
    /// Ranking on the worker thread.
    pub rank_ms: f64,
    /// Query change → rows replaced on screen.
    pub update_ms: f64,
    /// The rows swap itself (UI thread).
    pub apply_ms: f64,
    pub walk_ms: f64,
}

pub struct Palette {
    pub root: gtk::Box,
    /// The scrim under the palette (spec D.1).
    scrim: gtk::Box,
    /// List rows → hit index (`None` = a section label row), and back.
    row_hit: RefCell<Vec<Option<usize>>>,
    hit_row: RefCell<Vec<u32>>,
    pub entry: gtk::SearchEntry,
    chips: Vec<(Filter, gtk::Button)>,
    /// The scope row and the second line it wraps to at large text (R111.2).
    chip_row: gtk::Box,
    chip_row2: gtk::Box,
    scope_hint: gtk::Label,
    /// The row factory was built for large text.
    built_large: Cell<bool>,
    search: gtk::Box,
    rows: gtk::StringList,
    scroll: gtk::ScrolledWindow,
    pub sel: gtk::SingleSelection,
    pub list: gtk::ListView,
    status: gtk::Label,
    pub form_box: gtk::Box,
    pub items: RefCell<Arc<Vec<Item>>>,
    pub hits: RefCell<Vec<Hit>>,
    pub open: Cell<bool>,
    pub files: Cell<Files>,
    /// Items matching the query (more than the rows shown).
    total: Cell<usize>,
    /// Enter pressed while a ranking ran: run the row once it lands.
    pending_enter: Cell<bool>,
    /// The input method is composing in the search entry: its keys first.
    preedit: Cell<bool>,
    /// Bumped per query: a stale ranking stops early and is dropped.
    gen: Arc<AtomicU64>,
    /// Bumped per opening: a stale file walk is dropped.
    session: Cell<u64>,
    pub busy: Cell<bool>,
    pub timing: Cell<Timing>,
    pub form: RefCell<Option<crate::palette_run::FormView>>,
    /// `$` (R92): the model items and files while symbols are listed.
    base: RefCell<Option<Arc<Vec<Item>>>>,
    /// Where the symbols came from (`rust-analyzer`, `text matches (indexing)`).
    sym_note: RefCell<String>,
}

fn label(classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(None);
    l.set_xalign(0.0);
    classes.iter().for_each(|c| l.add_css_class(c));
    l
}

/// Build the (hidden) overlay on top of `overlay`'s child (spec D).
pub fn install(overlay: &gtk::Overlay) {
    let scrim = gtk::Box::new(gtk::Orientation::Vertical, 0);
    scrim.add_css_class("palette-scrim");
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    scrim.set_visible(false);
    let click = gtk::GestureClick::new();
    click.connect_pressed(|_, _, _, _| {
        if let Some(p) = get() {
            p.close(true);
        }
    });
    scrim.add_controller(click);
    overlay.add_overlay(&scrim);

    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("rade-palette");
    root.add_css_class("palette");
    root.set_halign(gtk::Align::Center);
    root.set_valign(gtk::Align::Start);
    root.set_margin_top(64);
    root.set_width_request(720);
    root.set_visible(false);

    // Query row (D.4–D.6).
    let entry_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    entry_row.add_css_class("palette-entry-row");
    let glass = gtk::Image::from_icon_name("ph-magnifying-glass-symbolic");
    glass.add_css_class("palette-entry-icon");
    entry_row.append(&glass);
    let entry = gtk::SearchEntry::new();
    entry.set_placeholder_text(Some(
        "Search sources, worktrees, agents, files, actions   (> actions  @ agents  / sources  # files  $ symbols)",
    ));
    entry.add_css_class("rade-palette-entry");
    entry.add_css_class("palette-entry");
    entry.set_hexpand(true);
    entry.set_valign(gtk::Align::Center);
    entry_row.append(&entry);
    let esc = label(&["kbd", "palette-esc"]);
    esc.set_label("Esc");
    esc.set_valign(gtk::Align::Center);
    entry_row.append(&esc);

    // Scope chips (D.7–D.9).
    let chip_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    chip_row.add_css_class("palette-scopes");
    let chips: Vec<(Filter, gtk::Button)> = FILTERS
        .iter()
        .map(|f| {
            let b = gtk::Button::with_label(&f.chip());
            b.add_css_class("flat");
            b.add_css_class("rade-palette-chip");
            b.add_css_class("palette-scope");
            b.set_focusable(false);
            chip_row.append(&b);
            (*f, b)
        })
        .collect();
    let hint = label(&["palette-scope-hint"]);
    hint.set_label("Tab next");
    hint.set_hexpand(true);
    hint.set_xalign(1.0);
    chip_row.append(&hint);

    let rows = gtk::StringList::new(&[]);
    let sel = gtk::SingleSelection::new(Some(rows.clone()));
    sel.set_autoselect(false);
    sel.set_can_unselect(true);
    let list = gtk::ListView::new(Some(sel.clone()), Some(factory(false)));
    list.add_css_class("rade-palette-list");
    list.add_css_class("palette-list");
    list.set_single_click_activate(true);
    list.set_focusable(false);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_child(Some(&list));
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    // A height set per result count (see `apply`), not the list's natural
    // height: measuring that would build a widget for every row.
    scroll.set_propagate_natural_height(false);

    // Footer (D.20).
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    footer.add_css_class("palette-footer");
    for t in ["↑↓ move", "Enter run", "Ctrl Enter in new tab"] {
        let l = label(&["palette-footer-hint"]);
        l.set_label(t);
        footer.append(&l);
    }
    let status = label(&["palette-status"]);
    status.set_hexpand(true);
    status.set_xalign(1.0);
    status.set_ellipsize(gtk::pango::EllipsizeMode::Start);
    footer.append(&status);

    let search = gtk::Box::new(gtk::Orientation::Vertical, 0);
    search.append(&entry_row);
    search.append(&chip_row);
    let chip_row2 = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    chip_row2.add_css_class("palette-scopes");
    chip_row2.add_css_class("palette-scopes-wrapped");
    chip_row2.set_visible(false);
    search.append(&chip_row2);
    search.append(&scroll);
    search.append(&footer);
    let form_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    form_box.add_css_class("palette-form");
    form_box.set_visible(false);
    root.append(&search);
    root.append(&form_box);
    overlay.add_overlay(&root);

    let p = Rc::new(Palette {
        root,
        scrim,
        row_hit: RefCell::default(),
        hit_row: RefCell::default(),
        entry,
        chips,
        chip_row,
        chip_row2,
        scope_hint: hint,
        built_large: Cell::new(false),
        search,
        rows,
        scroll,
        sel,
        list,
        status,
        form_box,
        items: RefCell::new(Arc::new(Vec::new())),
        hits: RefCell::default(),
        open: Cell::new(false),
        files: Cell::new(Files::Off),
        total: Cell::new(0),
        pending_enter: Cell::new(false),
        preedit: Cell::new(false),
        gen: Arc::new(AtomicU64::new(0)),
        session: Cell::new(0),
        busy: Cell::new(false),
        timing: Cell::default(),
        form: RefCell::default(),
        base: RefCell::default(),
        sym_note: RefCell::default(),
    });
    PALETTE.with(|s| *s.borrow_mut() = Some(p.clone()));
    connect(&p);
}

fn connect(p: &Rc<Palette>) {
    p.entry.connect_search_changed(|_| {
        if let Some(p) = get() {
            p.refilter();
        }
    });
    p.entry.set_search_delay(0);
    // Enter comes through `activate`, after the input method had its say.
    p.entry.connect_activate(|_| {
        let _ = key("enter");
    });
    p.entry.connect_stop_search(|_| {
        let _ = key("escape");
    });
    // IBus composing (preedit) in the entry: Esc/arrows/Tab are its keys.
    let mut w = p.entry.first_child();
    while let Some(c) = w {
        if let Some(t) = c.downcast_ref::<gtk::Text>() {
            t.connect_preedit_changed(|_, s| {
                if let Some(p) = get() {
                    p.preedit.set(!s.is_empty());
                }
            });
        }
        w = c.next_sibling();
    }
    for (f, b) in &p.chips {
        let f = *f;
        b.connect_clicked(move |_| {
            if let Some(p) = get() {
                p.set_filter(f);
            }
        });
    }
    p.list.connect_activate(|_, pos| {
        if let Some(p) = get() {
            let hit = p.row_hit.borrow().get(pos as usize).copied().flatten();
            if let Some(h) = hit {
                p.select_hit(h);
                crate::palette_run::run_selected(&p);
            }
        }
    });
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(|_, keyval, _, state| {
        let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
        let name = match keyval {
            gdk::Key::Up | gdk::Key::KP_Up => "up",
            gdk::Key::Down | gdk::Key::KP_Down => "down",
            gdk::Key::Page_Up | gdk::Key::KP_Page_Up => "page_up",
            gdk::Key::Page_Down | gdk::Key::KP_Page_Down => "page_down",
            gdk::Key::Escape => "escape",
            gdk::Key::ISO_Left_Tab => "shift+tab",
            gdk::Key::Tab if shift => "shift+tab",
            gdk::Key::Tab => "tab",
            // Enter in the search entry and in form text fields comes
            // through `activate` (after the input method); elsewhere in a
            // form (a drop-down, a button) it submits.
            // Ctrl+Enter on a file row: Open to the Side (R113.2).
            gdk::Key::Return | gdk::Key::KP_Enter
                if state.contains(gdk::ModifierType::CONTROL_MASK)
                    && get().is_some_and(|p| p.form.borrow().is_none()) =>
            {
                "ctrl+enter"
            }
            gdk::Key::Return | gdk::Key::KP_Enter if form_focus_off_text() => "enter",
            _ => return glib::Propagation::Proceed,
        };
        if key(name) {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    p.root.add_controller(keys);
}

/// Keyboard focus is in a form, not in a text field nor a drop-down list.
fn form_focus_off_text() -> bool {
    let Some(p) = get() else { return false };
    let focus = gtk::prelude::GtkWindowExt::focus(&app().window);
    // A count's spin button is not a text field: Enter there submits too.
    let text = |f: &gtk::Widget| {
        f.is::<gtk::Text>() && f.ancestor(gtk::SpinButton::static_type()).is_none()
    };
    p.form.borrow().is_some() && !focus.is_some_and(|f| text(&f) || in_popover(&f))
}

fn in_popover(w: &gtk::Widget) -> bool {
    w.ancestor(gtk::Popover::static_type()).is_some()
}

/// A key in the palette (`debug.palette key <name>` runs the same path).
/// False = not handled (the widget's own handling goes on).
pub fn key(name: &str) -> bool {
    let Some(p) = get() else { return false };
    if !p.open.get() {
        return false;
    }
    if p.form.borrow().is_some() {
        let focus = gtk::prelude::GtkWindowExt::focus(&app().window);
        let popover = focus.as_ref().is_some_and(in_popover);
        let text = focus.as_ref().is_some_and(|f| f.is::<gtk::Text>());
        return match name {
            // An open drop-down list keeps its own keys.
            _ if popover => false,
            "escape" => {
                p.close_form();
                true
            }
            "enter" => {
                crate::palette_run::submit(&p);
                true
            }
            // Tab stays inside the form (wrapping), never out to the window.
            "tab" | "shift+tab" => {
                let dir = if name == "tab" {
                    gtk::DirectionType::TabForward
                } else {
                    gtk::DirectionType::TabBackward
                };
                if !p.form_box.child_focus(dir) {
                    if let Some(v) = p.form.borrow().as_ref() {
                        v.focus_end(name == "tab");
                    }
                }
                true
            }
            // Arrows move the cursor in a text field; elsewhere they would
            // move focus out of the palette.
            "up" | "down" | "page_up" | "page_down" => !text,
            _ => false,
        };
    }
    if p.preedit.get() {
        return false;
    }
    let n = p.hits.borrow().len() as u32;
    let at = p.selected_hit().unwrap_or(0) as u32;
    if let Some(to) = palette::step(at, n, name, PAGE) {
        p.select_hit(to as usize);
        return true;
    }
    match name {
        "up" | "down" | "page_up" | "page_down" => {}
        "tab" | "shift+tab" => {
            let (f, _) = palette::parse_query(&p.entry.text());
            p.set_filter(f.cycle(if name == "tab" { 1 } else { -1 }));
        }
        // The rows on screen may be for an older query: wait for this one.
        "enter" if p.busy.get() => p.pending_enter.set(true),
        "enter" => crate::palette_run::run_selected(&p),
        "ctrl+enter" if p.busy.get() => p.pending_enter.set(true),
        "ctrl+enter" => crate::palette_run::run_selected_side(&p),
        "escape" => p.close(true),
        _ => return false,
    }
    true
}

/// `Ctrl+Shift+P`: open, or close if open (focus back to the Pane).
pub fn toggle() {
    if let Some(p) = get() {
        if p.open.get() {
            p.close(true);
        } else {
            p.show();
        }
    }
}

pub fn is_open() -> bool {
    get().is_some_and(|p| p.open.get())
}

impl Palette {
    /// Width, scope row and rows for the layout in force (spec D "1280 px":
    /// 720 px; "Text scale 200 %": min(window − 32, 1100) px, the scope row
    /// on two lines, details under the titles).
    fn fit_layout(&self) {
        let mode = crate::layout_mode::mode();
        let win = crate::layout_mode::window_width();
        if win > 0 {
            self.root
                .set_width_request(rade_core::chrome::overlay_width(win, 720, 1100, mode));
        }
        let large = mode.large_text;
        let wrapped = self.chip_row2.is_visible();
        if large != wrapped {
            let (from, to) = if large {
                (&self.chip_row, &self.chip_row2)
            } else {
                (&self.chip_row2, &self.chip_row)
            };
            let movers: Vec<gtk::Widget> = self
                .chips
                .iter()
                .skip(3)
                .map(|(_, b)| b.clone().upcast())
                .chain(std::iter::once(self.scope_hint.clone().upcast()))
                .collect();
            for w in &movers {
                from.remove(w);
                to.append(w);
            }
            self.chip_row2.set_visible(large);
        }
        if self.built_large.replace(large) != large {
            self.list.set_factory(Some(&factory(large)));
        }
    }

    fn show(self: &Rc<Self>) {
        let app = app();
        // Not over a dialog: the keyboard is the dialog's.
        if libadwaita::prelude::AdwApplicationWindowExt::visible_dialog(&app.window).is_some() {
            return;
        }
        app.touch(|m| {
            let mut recent = std::mem::take(&mut m.palette_recent);
            palette::prune_recent(m, &mut recent);
            m.palette_recent = recent;
        });
        let (items, root) = gather();
        let session = self.session.get() + 1;
        self.session.set(session);
        *self.items.borrow_mut() = Arc::new(items);
        self.files.set(if root.is_some() {
            Files::Indexing
        } else {
            Files::Off
        });
        self.fit_layout();
        self.open.set(true);
        self.pending_enter.set(false);
        self.close_form();
        self.root.set_visible(true);
        self.scrim.set_visible(true);
        crate::changes::sync_primary();
        crate::home_view::sync_primary();
        if self.entry.text().is_empty() {
            self.refilter();
        } else {
            self.entry.set_text(""); // refilters
        }
        self.entry.grab_focus();
        let inside = gtk::prelude::GtkWindowExt::focus(&app.window)
            .is_some_and(|f| f.is_ancestor(&self.root));
        if !inside {
            return self.close(false);
        }
        let Some(root) = root else { return };
        let t0 = Instant::now();
        in_background(
            move || palette::file_items(&root, FILE_CAP),
            move |(files, cut)| {
                let Some(p) = get() else { return };
                if p.session.get() != session || !p.open.get() {
                    return;
                }
                let mut t = p.timing.get();
                t.walk_ms = t0.elapsed().as_secs_f64() * 1e3;
                p.timing.set(t);
                p.files.set(Files::Listed(files.len(), cut));
                // Listing symbols: the files join the stashed items.
                let stash = p.base.borrow().clone();
                match stash {
                    Some(b) => {
                        let mut all = b.as_ref().clone();
                        all.extend(files);
                        *p.base.borrow_mut() = Some(Arc::new(all));
                    }
                    None => {
                        let mut all = p.items.borrow().as_ref().clone();
                        all.extend(files);
                        *p.items.borrow_mut() = Arc::new(all);
                    }
                }
                // Re-rank only if files can show: the selection stays put.
                let q = p.entry.text();
                match palette::parse_query(&q) {
                    (Filter::Files, _) => p.refilter(),
                    (Filter::All, t) if !t.is_empty() => p.refilter(),
                    _ => p.update_status(),
                }
            },
        );
    }

    /// Hide; `restore` gives keyboard focus back to the model's focused Pane
    /// (not when focus already went elsewhere, e.g. a click).
    pub fn close(&self, restore: bool) {
        if !self.open.replace(false) {
            return;
        }
        self.gen.fetch_add(1, Ordering::SeqCst);
        self.root.set_visible(false);
        self.scrim.set_visible(false);
        crate::changes::sync_primary();
        crate::home_view::sync_primary();
        *self.items.borrow_mut() = Arc::new(Vec::new());
        *self.base.borrow_mut() = None;
        self.hits.borrow_mut().clear();
        let n = self.rows.n_items();
        self.rows.splice(0, n, &[]);
        if restore {
            if let Some(app) = try_app() {
                app.restore_focus();
            }
        }
    }

    fn set_filter(&self, f: Filter) {
        let q = palette::with_filter(&self.entry.text(), f);
        self.entry.set_text(&q);
        self.entry.set_position(-1);
    }

    /// Re-rank for the entry's query on a worker thread; the newest query wins.
    pub fn refilter(&self) {
        let q = self.entry.text().to_string();
        let (f, _) = palette::parse_query(&q);
        for (cf, b) in &self.chips {
            if *cf == f {
                b.add_css_class("rade-palette-chip-on");
                b.add_css_class("active");
            } else {
                b.remove_css_class("rade-palette-chip-on");
                b.remove_css_class("active");
            }
        }
        let gen = self.gen.fetch_add(1, Ordering::SeqCst) + 1;
        if f == Filter::Symbols {
            return self.refilter_symbols(gen, q);
        }
        // Back from `$`: the model items and files again.
        if let Some(b) = self.base.borrow_mut().take() {
            *self.items.borrow_mut() = b;
        }
        let items = self.items.borrow().clone();
        let recent = app().model.borrow().palette_recent.clone();
        let g = self.gen.clone();
        let t0 = Instant::now();
        self.busy.set(true);
        in_background(
            move || {
                let t = Instant::now();
                let ranked = palette::rank(&items, &q, &recent, LIMIT, &|| {
                    g.load(Ordering::Relaxed) != gen
                });
                (ranked, t.elapsed().as_secs_f64() * 1e3)
            },
            move |(ranked, rank_ms)| {
                let Some(p) = get() else { return };
                let (Some(ranked), true) = (ranked, p.gen.load(Ordering::SeqCst) == gen) else {
                    return;
                };
                let t = Instant::now();
                p.apply(ranked);
                let mut tm = p.timing.get();
                tm.rank_ms = rank_ms;
                tm.apply_ms = t.elapsed().as_secs_f64() * 1e3;
                tm.update_ms = t0.elapsed().as_secs_f64() * 1e3;
                p.timing.set(tm);
                p.busy.set(false);
                if p.pending_enter.take() {
                    crate::palette_run::run_selected(&p);
                }
            },
        );
    }

    /// The hit the selected row stands for.
    pub fn selected_hit(&self) -> Option<usize> {
        let r = self.sel.selected();
        self.row_hit.borrow().get(r as usize).copied().flatten()
    }

    /// Select hit `i`'s row and scroll to it.
    pub fn select_hit(&self, i: usize) {
        let Some(r) = self.hit_row.borrow().get(i).copied() else {
            return;
        };
        self.sel.set_selected(r);
        self.list.scroll_to(r, gtk::ListScrollFlags::NONE, None);
    }

    fn apply(&self, ranked: Ranked) {
        // Keep the selected item selected if it is still listed.
        let keep = {
            let (items, hits) = (self.items.borrow(), self.hits.borrow());
            self.selected_hit()
                .and_then(|i| hits.get(i))
                .and_then(|h| items.get(h.index))
                .map(|it| it.target.key())
        };
        let n = ranked.hits.len();
        let at = keep
            .and_then(|k| {
                let items = self.items.borrow();
                ranked
                    .hits
                    .iter()
                    .position(|h| items[h.index].target.key() == k)
            })
            .unwrap_or(0);
        // Section label rows (spec D.10) when a text is typed.
        let text = !palette::parse_query(&self.entry.text()).1.is_empty();
        let mut ids: Vec<String> = Vec::new();
        let mut row_hit = Vec::new();
        let mut hit_row = Vec::new();
        let mut height = 0;
        // Large text: rows grow with the text, a detail adds a line (R111.2).
        let scale = crate::layout_mode::text_scale();
        let large = crate::layout_mode::mode().large_text;
        let grown = |px: i32| (f64::from(px) * scale).round() as i32;
        let (row_h, row_h2, section_h) = if large {
            // Measured at 200 %: 44 / 78 / 40 px (title, title + detail,
            // section), the rows' 12 px padding included.
            (
                grown(ROW_HEIGHT - 10),
                grown(ROW_HEIGHT + 7),
                grown(SECTION_HEIGHT * 3 / 4),
            )
        } else {
            (ROW_HEIGHT, ROW_HEIGHT, SECTION_HEIGHT)
        };
        {
            let items = self.items.borrow();
            let mut last: Option<&str> = None;
            for (i, h) in ranked.hits.iter().enumerate() {
                let sec = palette::section(items[h.index].kind);
                if text && last != Some(sec) {
                    ids.push(format!("s:{sec}"));
                    row_hit.push(None);
                    if ids.len() <= VISIBLE_ROWS + 4 {
                        height += section_h;
                    }
                    last = Some(sec);
                }
                hit_row.push(ids.len() as u32);
                ids.push(format!("h:{i}"));
                row_hit.push(Some(i));
                if i < VISIBLE_ROWS {
                    height += if large && !items[h.index].detail.is_empty() {
                        row_h2
                    } else {
                        row_h
                    };
                }
            }
        }
        *self.hits.borrow_mut() = ranked.hits;
        *self.row_hit.borrow_mut() = row_hit;
        *self.hit_row.borrow_mut() = hit_row;
        self.total.set(ranked.total);
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        self.rows.splice(0, self.rows.n_items(), &refs);
        // At most the window − 128 px tall (spec D "1280 px": 800 − 128),
        // less the query row, scope row(s) and footer.
        let win_h = match crate::layout_mode::window_height() {
            h if h > 0 => h,
            _ => 900,
        };
        let chrome = grown(46 + 40 + 36) + if large { grown(40) } else { 0 };
        let h = height.min((win_h - 128 - chrome).max(ROW_HEIGHT));
        // Unset the max first: GTK refuses a min above the current max.
        self.scroll.set_max_content_height(-1);
        self.scroll.set_min_content_height(h);
        self.scroll.set_max_content_height(h);
        // A scrollbar has a minimum height of its own: none for a few rows.
        // Large text: the estimate may fall short; the list scrolls rather
        // than pushing the footer out of the window.
        self.scroll
            .set_vscrollbar_policy(if n > VISIBLE_ROWS || large {
                gtk::PolicyType::Automatic
            } else {
                gtk::PolicyType::Never
            });
        if n > 0 {
            self.select_hit(at);
        }
        self.update_status();
    }

    pub fn update_status(&self) {
        let n = self.hits.borrow().len();
        let mut line =
            palette::status_line(n, self.total.get(), &self.entry.text(), self.files.get());
        let note = self.sym_note.borrow();
        if palette::parse_query(&self.entry.text()).0 == Filter::Symbols && !note.is_empty() {
            line = if n == 0 && line == "No matches" {
                note.clone()
            } else {
                format!("{line} · {note}")
            };
        }
        self.status.set_label(&line);
    }

    /// `$text` (R92): the active Worktree's symbols from its language
    /// servers (text matches while none can answer), ranked on their
    /// names. Asked 150 ms after the last keystroke; an indexing answer is
    /// asked again by itself.
    fn refilter_symbols(&self, gen: u64, q: String) {
        let app = app();
        // The Worktree of the file the focused Viewer shows, else the one on screen.
        let root = {
            let m = app.model.borrow();
            let file = m
                .active_tab()
                .and_then(|t| app.viewers.borrow().get(&t.focused).and_then(|v| v.path()));
            let of_file = file.and_then(|f| {
                m.worktrees()
                    .filter(|(_, w)| f.starts_with(&w.path))
                    .max_by_key(|(_, w)| w.path.as_os_str().len())
                    .map(|(_, w)| w.path.clone())
            });
            of_file.or_else(|| {
                let (p, w) = m.active();
                (!p.is_general()).then(|| w.path.clone())
            })
        };
        let text = palette::parse_query(&q).1.to_string();
        let empty = |p: &Palette, note: &str| {
            *p.sym_note.borrow_mut() = note.to_string();
            p.stash();
            *p.items.borrow_mut() = Arc::new(Vec::new());
            p.apply(Ranked {
                hits: vec![],
                total: 0,
            });
            p.busy.set(false);
        };
        let Some(root) = root else {
            return empty(self, "Open a Worktree to search its symbols");
        };
        if text.is_empty() {
            return empty(self, "Type a symbol name");
        }
        let (mut ctx, user) =
            match crate::code::context(&app, rade_core::code::nav::Query::Workspace, &root, 1, 1) {
                Ok(c) => c,
                Err(e) => return empty(self, &e.message),
            };
        ctx.timeout = std::time::Duration::from_secs(5);
        let recent = app.model.borrow().palette_recent.clone();
        let g = self.gen.clone();
        let t0 = Instant::now();
        self.busy.set(true);
        in_background(
            move || {
                std::thread::sleep(std::time::Duration::from_millis(150));
                if g.load(Ordering::Relaxed) != gen {
                    return None;
                }
                let pool = crate::code::pool();
                pool.lock().unwrap().configure(&user);
                let a = rade_core::code::workspace(pool, &ctx, &text);
                let items: Vec<Item> = a.items.iter().map(|i| Item::symbol(&root, i)).collect();
                let t = Instant::now();
                let ranked = palette::rank(&items, &q, &recent, LIMIT, &|| false)?;
                Some((a, items, ranked, t.elapsed().as_secs_f64() * 1e3))
            },
            move |r| {
                let Some(p) = get() else { return };
                let Some((a, items, ranked, rank_ms)) = r else {
                    return;
                };
                if p.gen.load(Ordering::SeqCst) != gen {
                    return;
                }
                let mut note = a.summary_source();
                if a.text() {
                    if let Some(h) = &a.hint {
                        note = format!("{note} · {h}");
                    }
                }
                *p.sym_note.borrow_mut() = note;
                p.stash();
                *p.items.borrow_mut() = Arc::new(items);
                p.apply(ranked);
                let mut tm = p.timing.get();
                tm.rank_ms = rank_ms;
                tm.update_ms = t0.elapsed().as_secs_f64() * 1e3;
                p.timing.set(tm);
                p.busy.set(false);
                if p.pending_enter.take() {
                    crate::palette_run::run_selected(&p);
                }
                // Indexing: ask again shortly, while the query is the same.
                if a.state == "indexing" {
                    let q = p.entry.text().to_string();
                    glib::timeout_add_local_once(
                        std::time::Duration::from_millis(1500),
                        move || {
                            if let Some(p) = get().filter(|p| p.open.get() && p.entry.text() == q) {
                                p.refilter();
                            }
                        },
                    );
                }
            },
        );
    }

    /// Keep the model items and files aside while symbols are listed.
    fn stash(&self) {
        if self.base.borrow().is_none() {
            let items = self.items.borrow().clone();
            *self.base.borrow_mut() = Some(items);
        }
    }

    /// A message in the status line (e.g. a target that is gone).
    pub fn say(&self, text: &str) {
        self.status.set_label(text);
    }

    /// Show the inline form in place of the search (R14.4).
    pub fn show_form(&self, form: Form) {
        let view = crate::palette_run::FormView::build(form, &self.form_box);
        self.search.set_visible(false);
        self.form_box.set_visible(true);
        view.focus_first();
        *self.form.borrow_mut() = Some(view);
    }

    /// Back from a form to the list (the query is kept).
    pub fn close_form(&self) {
        if self.form.borrow_mut().take().is_none() {
            return;
        }
        while let Some(c) = self.form_box.first_child() {
            self.form_box.remove(&c);
        }
        self.form_box.set_visible(false);
        self.search.set_visible(true);
        if self.open.get() {
            self.entry.grab_focus();
        }
    }

    /// The section labels listed, in order (spec D.10).
    fn sections(&self) -> Vec<String> {
        let n = self.rows.n_items();
        (0..n)
            .filter_map(|i| self.rows.string(i))
            .filter_map(|s| s.strip_prefix("s:").map(String::from))
            .collect()
    }

    /// `debug.palette info`.
    pub fn info(&self) -> Value {
        let items = self.items.borrow();
        let hits = self.hits.borrow();
        let rows: Vec<Value> = hits
            .iter()
            .take(50)
            .map(|h| row_json(&items[h.index], h))
            .collect();
        let q = self.entry.text();
        let (f, text) = palette::parse_query(&q);
        let t = self.timing.get();
        let form = self.form.borrow().as_ref().map(|v| v.info());
        json!({
            "open": self.open.get(), "query": self.entry.text().as_str(), "filter": f, "text": text,
            "count": hits.len(), "selected": self.selected_hit().unwrap_or(0), "rows": rows,
            "sections": self.sections(),
            "scopes": self.chips.iter().map(|(f, _)| f.chip()).collect::<Vec<_>>(),
            "status": self.status.label().as_str(), "busy": self.busy.get(),
            "files": match self.files.get() { Files::Listed(n, _) => Some(n), _ => None },
            "files_truncated": matches!(self.files.get(), Files::Listed(_, true)),
            "file_search": self.files.get() != Files::Off, "total": self.total.get(),
            "timing": {"rank_ms": t.rank_ms, "update_ms": t.update_ms, "apply_ms": t.apply_ms, "walk_ms": t.walk_ms},
            "form": form,
        })
    }
}

/// Model items (without files), and the active Worktree's root for the
/// file walk (none in General, which is `$HOME`).
pub fn gather() -> (Vec<Item>, Option<std::path::PathBuf>) {
    let app = app();
    let m = app.model.borrow();
    let config = app.config.borrow();
    let presets: Vec<String> = config.presets().unwrap_or_default().into_keys().collect();
    // The shown Workspace's Projects and links (R107.3).
    let projects = crate::navdata::projects(&m);
    let links = crate::navdata::links(&m);
    let snap = crate::navdata::snap(&m);
    let ahead = |p: &std::path::Path| {
        snap.as_ref()
            .and_then(|s| s.wt.get(p).map(|f| f.0))
            .unwrap_or(0)
    };
    let home = rade_core::paths::home_dir(&rade_core::paths::process_env);
    let ctx = palette::Links {
        projects: &projects,
        links: &links,
        ahead: &ahead,
        canon: &crate::projects::canon,
        home: &home,
    };
    let mut items = palette::items(&m, &config.keybindings(), &presets, &ctx);
    drop(m);
    // "Open localhost:<port>" for dev servers running in Panes (R18.4).
    let by_pane = crate::browser::scan_ports();
    let ports: Vec<(u16, String)> = crate::browser::label_ports(&app, by_pane)
        .into_iter()
        .map(|x| {
            (
                x.port,
                format!("{} · {} / {}", x.name, x.source, x.worktree),
            )
        })
        .collect();
    items.extend(palette::port_items(&ports));
    let m = app.model.borrow();
    let (p, w) = m.active();
    (items, (!p.is_general()).then(|| w.path.clone()))
}

pub fn row_json(it: &Item, h: &Hit) -> Value {
    json!({"kind": it.kind, "title": it.title, "detail": it.detail, "keys": it.keys,
           "section": palette::section(it.kind),
           "target": it.target, "score": h.score, "positions": h.positions})
}

/// Title with the matched characters in bold.
fn markup(title: &str, positions: &[u32]) -> String {
    let mut s = String::new();
    for (i, c) in title.chars().enumerate() {
        let esc = glib::markup_escape_text(&c.to_string());
        if positions.contains(&(i as u32)) {
            s += &format!("<b>{esc}</b>");
        } else {
            s += &esc;
        }
    }
    s
}

/// The row icon of an item (spec D.12, D.18, D.19).
fn icon_of(it: &Item) -> (&'static str, Option<&'static str>) {
    use palette::Kind::*;
    match it.kind {
        Project => ("ph-cube-symbolic", None),
        Link | Source => ("ph-git-fork-symbolic", None),
        Worktree => ("ph-git-branch-symbolic", Some("wt")),
        Pane => ("ph-terminal-window-symbolic", None),
        Tab => ("ph-squares-four-symbolic", None),
        Symbol => ("ph-file-code-symbolic", None),
        File => {
            let ext = it.title.rsplit('.').next().unwrap_or("");
            match ext {
                "ts" | "tsx" => ("ph-file-ts-symbolic", None),
                "rs" | "js" | "jsx" | "py" | "go" | "c" | "h" | "cpp" | "java" | "rb" | "sh" => {
                    ("ph-file-code-symbolic", None)
                }
                _ => ("ph-file-symbolic", None),
            }
        }
        Action => match &it.target {
            palette::Target::Action { id } if id.starts_with(palette::LINK_ADD_PREFIX) => {
                ("ph-link-simple-symbolic", None)
            }
            palette::Target::Action { id } if id.starts_with(palette::LINK_BASE_PREFIX) => {
                ("ph-arrows-split-symbolic", None)
            }
            palette::Target::Action { id } if id.starts_with(palette::FANOUT_ON_PREFIX) => {
                ("ph-users-three-symbolic", None)
            }
            palette::Target::Action { id } if id == "fanout" => ("ph-users-three-symbolic", None),
            _ => ("ph-caret-right-symbolic", None),
        },
    }
}

/// `Ctrl+Shift+E` → key caps `Ctrl`, `⇧`, `E` (spec D.14).
fn caps(keys: &str) -> Vec<String> {
    keys.split('+')
        .map(|k| match k {
            "Shift" => "⇧".to_string(),
            k => k.to_string(),
        })
        .collect()
}

/// The rows: a grid whose children keep the same order in both layouts
/// (`bind` walks them as siblings); at large text the detail sits under the
/// title (spec D "Text scale 200 %").
fn factory(large: bool) -> gtk::SignalListItemFactory {
    let f = gtk::SignalListItemFactory::new();
    f.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let row = gtk::Grid::new();
        row.set_column_spacing(10);
        row.add_css_class("palette-item");
        let bar = gtk::Box::new(gtk::Orientation::Vertical, 0);
        bar.add_css_class("palette-item-selbar");
        bar.set_can_target(false);
        // The grid row takes the item's whole 32 px, so every cell centers in it.
        bar.set_vexpand(true);
        let icon = gtk::Image::new();
        icon.add_css_class("palette-item-icon");
        let title = label(&["rade-palette-title", "palette-item-title"]);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title.set_max_width_chars(48);
        let detail = label(&["rade-palette-detail", "palette-item-detail"]);
        detail.set_hexpand(true);
        detail.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        let keys = gtk::Box::new(gtk::Orientation::Horizontal, 3);
        keys.add_css_class("palette-item-keys");
        keys.set_valign(gtk::Align::Center);
        // The selected row's `Enter <what it does>` (D.15).
        let hint = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        hint.add_css_class("palette-item-hint");
        hint.set_valign(gtk::Align::Center);
        let enter = label(&["kbd"]);
        enter.set_label("Enter");
        hint.append(&enter);
        hint.append(&label(&["palette-item-hint-text"]));
        hint.set_visible(false);
        let section = label(&["palette-section-label"]);
        if large {
            row.add_css_class("palette-item-two-line");
            detail.set_ellipsize(gtk::pango::EllipsizeMode::None);
            detail.set_wrap(true);
            detail.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            title.set_max_width_chars(-1);
            title.set_hexpand(true);
        }
        row.attach(&bar, 0, 0, 1, if large { 2 } else { 1 });
        row.attach(&icon, 1, 0, 1, 1);
        row.attach(&title, 2, 0, 1, 1);
        if large {
            row.attach(&detail, 2, 1, 3, 1);
        } else {
            row.attach(&detail, 3, 0, 1, 1);
        }
        row.attach(&hint, 4, 0, 1, 1);
        row.attach(&keys, 5, 0, 1, 1);
        row.attach(&section, 6, 0, 1, 1);
        item.set_child(Some(&row));
        // `selected` class + the selection bar and the Enter hint (D.15).
        let h = hint.clone();
        item.connect_selected_notify(move |item| {
            if let Some(c) = item.child() {
                if item.is_selected() {
                    c.add_css_class("selected");
                } else {
                    c.remove_css_class("selected");
                }
                h.set_visible(item.is_selected() && c.has_css_class("palette-item"));
            }
        });
    });
    f.connect_bind(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let Some(p) = get() else { return };
        let Some(id) = item
            .item()
            .and_downcast::<gtk::StringObject>()
            .map(|s| s.string().to_string())
        else {
            return;
        };
        let row = item.child().unwrap();
        let bar = row.first_child().unwrap();
        let icon = bar.next_sibling().and_downcast::<gtk::Image>().unwrap();
        let title = icon.next_sibling().and_downcast::<gtk::Label>().unwrap();
        let detail = title.next_sibling().and_downcast::<gtk::Label>().unwrap();
        let hint = detail.next_sibling().and_downcast::<gtk::Box>().unwrap();
        let keys = hint.next_sibling().and_downcast::<gtk::Box>().unwrap();
        let section = keys.next_sibling().and_downcast::<gtk::Label>().unwrap();
        if item.is_selected() {
            row.add_css_class("selected");
        } else {
            row.remove_css_class("selected");
        }
        if let Some(sec) = id.strip_prefix("s:") {
            item.set_selectable(false);
            item.set_activatable(false);
            row.remove_css_class("palette-item");
            row.add_css_class("palette-section");
            for w in [
                &bar,
                icon.upcast_ref(),
                title.upcast_ref(),
                detail.upcast_ref(),
                hint.upcast_ref(),
                keys.upcast_ref(),
            ] {
                w.set_visible(false);
            }
            section.set_visible(true);
            section.set_label(sec);
            return;
        }
        item.set_selectable(true);
        item.set_activatable(true);
        row.remove_css_class("palette-section");
        row.add_css_class("palette-item");
        section.set_visible(false);
        for w in [
            &bar,
            icon.upcast_ref(),
            title.upcast_ref(),
            detail.upcast_ref(),
            keys.upcast_ref(),
        ] {
            w.set_visible(true);
        }
        let Some(i) = id.strip_prefix("h:").and_then(|i| i.parse::<usize>().ok()) else {
            return;
        };
        let (items, hits) = (p.items.borrow(), p.hits.borrow());
        let Some(h) = hits.get(i) else { return };
        let Some(it) = items.get(h.index) else { return };
        // Only what changed (a bind runs per row on every list update, R14.5).
        let (name, class) = icon_of(it);
        if matches!(it.kind, palette::Kind::File) {
            // Files: the file tree's icons (R8, ADR-0037).
            let file = it.title.rsplit('/').next().unwrap_or(&it.title);
            let stem = crate::files::icon_for(file, false, false);
            if icon.widget_name() != stem.as_str() || icon.icon_name().is_some() {
                crate::files::set_icon(&icon, &stem);
            }
        } else if icon.icon_name().as_deref() != Some(name) {
            icon.set_icon_name(Some(name));
            icon.set_widget_name("");
        }
        if class.is_some() != icon.has_css_class("wt") {
            if class.is_some() {
                icon.add_css_class("wt");
            } else {
                icon.remove_css_class("wt");
            }
        }
        title.set_markup(&markup(&it.title, &h.positions));
        if detail.label() != it.detail {
            detail.set_label(&it.detail);
        }
        // Large text: no empty second line under a title without a detail.
        if row.has_css_class("palette-item-two-line") {
            detail.set_visible(!it.detail.is_empty());
        }
        let want = it.keys.clone().unwrap_or_default();
        if keys.widget_name() != want {
            while let Some(c) = keys.first_child() {
                keys.remove(&c);
            }
            for c in caps(&want).into_iter().filter(|c| !c.is_empty()) {
                let l = label(&["kbd"]);
                l.set_label(&c);
                keys.append(&l);
            }
            keys.set_widget_name(&want);
        }
        keys.set_visible(it.keys.is_some());
        let words = it.target.enter_hint();
        if let Some(t) = hint.last_child().and_downcast::<gtk::Label>() {
            if t.label() != words {
                t.set_label(words);
            }
        }
        hint.set_visible(item.is_selected());
    });
    f
}

/// Form fields for `debug.palette info` (kinds as in `rade_core`).
pub fn field_kind(k: &FieldKind) -> &'static str {
    match k {
        FieldKind::Text => "text",
        FieldKind::Choice(_) => "choice",
        FieldKind::Counts(_) => "counts",
    }
}
