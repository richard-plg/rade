//! Agent Overview (PRD R117, ADR-0039, spec G): every Agent of every
//! Workspace over the whole window, in bands from `rade_core::overview`,
//! most urgent first. Typing filters, arrows / Tab move, Enter jumps the
//! way a Notification click does; holding `Ctrl+Shift` and tapping `Space`
//! flips through the cards and releasing `Ctrl` jumps. Cards show the
//! last screen lines (read once a second, only while open). The window
//! behind shrinks and the cards rise in through CSS transitions, which GTK
//! skips when `gtk-enable-animations` is off. A card the pointer rests on,
//! or the keyboard selection, gets a Preview (R117.9): more of its screen,
//! larger, floating beside it without moving any card. `debug.ui {"op":
//! "overview"}` drives it.

use crate::app::{app, try_app, App};
use adw::prelude::*;
use gtk::gdk::{Key, ModifierType};
use gtk::glib;
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::overview::{self as ov, Band, Card, Fact, Flip, Group, Move, Overview, Pos, Query};
use rade_core::rpc::{Error, Result};
use rade_ghostty_sys::GHOSTTY_POINT_ACTIVE;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

/// Motion (R117.6, ADR-0039), in ms; 0 while animations are off.
const IN_MS: u32 = 200;
const JUMP_MS: u32 = 220;
const ESC_MS: u32 = 140;
/// Cards that rise in one after another (15 ms apart).
const STAGGERED: usize = 12;
/// Preview (R117.9), in ms: the pointer rests on a card, the selection
/// rests, a filter keystroke (the list is changing: wait for a pause), the
/// pointer has left both the card and the Preview.
const HOVER_MS: u64 = 350;
const KEYS_MS: u64 = 250;
const TYPED_MS: u64 = 800;
const HIDE_MS: u64 = 150;

thread_local! {
    static ROOT: RefCell<Option<Root>> = const { RefCell::new(None) };
    static OPEN: RefCell<Option<Rc<View>>> = const { RefCell::new(None) };
    /// Kept across opens (R117.4: `Alt+G` is remembered).
    static GROUP: Cell<Group> = const { Cell::new(Group::Status) };
    /// The last open / close durations, for `debug.ui overview`.
    static LAST_MS: Cell<(u32, u32)> = const { Cell::new((0, 0)) };
}

#[derive(Clone)]
struct Root {
    layer: gtk::Box,
    /// The window content behind (shrinks while open).
    behind: gtk::Widget,
}

struct View {
    filter: gtk::Entry,
    count: gtk::Label,
    group_status: gtk::Label,
    group_ws: gtk::Label,
    /// Count chips (left) and Workspace chips (right); each wraps.
    chips: gtk::FlowBox,
    ws_chips: gtk::FlowBox,
    body: gtk::Box,
    scroller: gtk::ScrolledWindow,
    /// The header, chips and body; the Preview floats over it.
    stage: gtk::Overlay,
    preview: Preview,
    query: RefCell<Query>,
    overview: RefCell<Overview>,
    facts: RefCell<Vec<Fact>>,
    tails: RefCell<HashMap<String, Vec<String>>>,
    sel: Cell<Option<Pos>>,
    flip: Cell<Flip>,
    cols: Cell<usize>,
    /// Card widgets by position, to move the selection without a rebuild.
    cards: RefCell<HashMap<Pos, gtk::Box>>,
    timer: RefCell<Option<glib::SourceId>>,
    /// The cards have risen in once (later rebuilds don't animate).
    shown_once: Cell<bool>,
}

fn animations() -> bool {
    gtk::Settings::default().is_some_and(|s| s.is_gtk_enable_animations())
}

fn ms(full: u32) -> u32 {
    if animations() {
        full
    } else {
        0
    }
}

/// The layer (hidden) over the window content `behind`.
pub fn install(overlay: &gtk::Overlay, behind: &impl IsA<gtk::Widget>) {
    let layer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    layer.set_widget_name("overview");
    layer.add_css_class("overview");
    layer.set_hexpand(true);
    layer.set_vexpand(true);
    layer.set_visible(false);
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(|_, k, _, m| {
        if key(k, m) {
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    keys.connect_key_released(|_, k, _, _| release(k));
    layer.add_controller(keys);
    // A click on nothing in particular closes it, as on macOS (R117.1).
    let click = gtk::GestureClick::new();
    click.connect_released(|_, _, x, y| {
        click_outside(x, y);
    });
    layer.add_controller(click);
    overlay.add_overlay(&layer);
    behind.add_css_class("ov-behind");
    ROOT.with(|r| {
        *r.borrow_mut() = Some(Root {
            layer,
            behind: behind.clone().upcast(),
        })
    });
}

pub fn is_open() -> bool {
    OPEN.with(|o| o.borrow().is_some())
}

fn view() -> Option<Rc<View>> {
    OPEN.with(|o| o.borrow().clone())
}

/// `Ctrl+Shift+Space`: open (the flip starts), or — held, while open —
/// the next card (R117.4).
pub fn shortcut() {
    match view() {
        Some(v) => {
            let mut f = v.flip.get();
            if f.press() {
                v.flip.set(f);
                step(&v, Move::Next);
            } else {
                close(Exit::Esc);
            }
        }
        None => open(Query::default(), true),
    }
}

/// The titlebar button and the palette: open, or close when open.
pub fn toggle() {
    if is_open() {
        close(Exit::Esc);
    } else {
        open(Query::default(), false);
    }
}

/// The switcher's `!` (Needs you) and a Statusline count (that band).
pub fn open_band(band: Option<Band>) {
    if is_open() {
        close(Exit::Esc);
    }
    open(
        Query {
            band,
            ..Default::default()
        },
        false,
    );
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

fn led(s: Option<rade_core::AgentStatus>) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.add_css_class("led");
    b.add_css_class(&format!("led-{}", s.map_or("idle", |s| s.as_str())));
    b.set_valign(gtk::Align::Center);
    b
}

fn open(mut q: Query, flip: bool) {
    let Some(root) = ROOT.with(|r| r.borrow().clone()) else {
        return;
    };
    crate::perf::start("overview");
    crate::palette::get().inspect(|p| p.close(false));
    crate::settings_view::close();
    crate::project_dialogs::close();
    let app = app();
    q.group = GROUP.with(Cell::get);

    let layer = &root.layer;
    while let Some(c) = layer.first_child() {
        layer.remove(&c);
    }
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    content.add_css_class("overview-content");
    content.set_vexpand(true);

    // Header: title, count, filter, group toggle.
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    head.add_css_class("overview-head");
    head.append(&label("Agents", &["overview-title"]));
    let count = label("", &["overview-count"]);
    count.set_visible(crate::layout_mode::text_scale() < 1.5);
    head.append(&count);
    let filter = gtk::Entry::new();
    filter.add_css_class("overview-filter");
    filter.set_placeholder_text(Some("Filter by name, Project, branch, status…"));
    filter.set_primary_icon_name(Some("ph-magnifying-glass-symbolic"));
    let big = crate::layout_mode::text_scale() >= 1.5;
    filter.set_width_chars(if big { 16 } else { 36 });
    filter.set_hexpand(big);
    head.append(&filter);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    head.append(&spacer);
    let seg = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    seg.add_css_class("overview-group");
    let group_status = label("by status", &["overview-seg"]);
    let group_ws = label("by Workspace", &["overview-seg"]);
    for (l, g) in [
        (&group_status, Group::Status),
        (&group_ws, Group::Workspace),
    ] {
        let c = gtk::GestureClick::new();
        c.connect_released(move |_, _, _, _| set_group(g));
        l.add_controller(c);
        seg.append(l);
    }
    head.append(&seg);
    if crate::layout_mode::text_scale() < 1.5 {
        head.append(&label("Alt G", &["overview-key"]));
    }
    content.append(&head);

    let flow = |halign| {
        let f = gtk::FlowBox::new();
        f.set_selection_mode(gtk::SelectionMode::None);
        f.set_column_spacing(6);
        f.set_row_spacing(6);
        f.set_max_children_per_line(12);
        f.set_homogeneous(false);
        f.set_halign(halign);
        f.set_valign(gtk::Align::Start);
        f.set_can_focus(false);
        f
    };
    let chips = flow(gtk::Align::Start);
    let ws_chips = flow(gtk::Align::End);
    chips.set_hexpand(true);
    let chip_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    chip_row.add_css_class("overview-chips");
    chip_row.append(&chips);
    chip_row.append(&ws_chips);
    content.append(&chip_row);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 18);
    body.add_css_class("overview-body");
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_vexpand(true);
    scroller.set_child(Some(&body));
    content.append(&scroller);
    let stage = gtk::Overlay::new();
    stage.set_vexpand(true);
    stage.set_child(Some(&content));
    let preview = Preview::new();
    stage.add_overlay(&preview.root);
    layer.append(&stage);

    // Key hints: they wrap rather than widen the window (R117.7).
    let hints = gtk::FlowBox::new();
    hints.set_selection_mode(gtk::SelectionMode::None);
    hints.set_column_spacing(16);
    hints.set_max_children_per_line(8);
    hints.set_can_focus(false);
    hints.set_halign(gtk::Align::Start);
    hints.set_hexpand(true);
    for h in [
        "←↑↓→ move",
        "Tab next group",
        "↵ jump",
        "⇧↵ jump + Zoom",
        "Ctrl+Alt+1..9 one Workspace",
        "Alt+G group",
        "Esc back to where you were",
    ] {
        hints.append(&label(h, &["overview-hint"]));
    }
    // The bar spans the window; the hints sit at its start.
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bar.set_widget_name("overview-hints");
    bar.add_css_class("overview-hints");
    bar.append(&hints);
    layer.append(&bar);

    let facts = facts(&app);
    let previous = app.model.borrow().active_tab().map(|t| t.focused.clone());
    let wss = workspaces(&app);
    let mut built = Overview::build(&facts, &q, &wss);
    q.quiet_cap = quiet_cap(&built, cols(), body_height(None));
    if q.quiet_cap.is_some() {
        built = Overview::build(&facts, &q, &wss);
    }
    let sel = ov::initial(&built, previous.as_deref());
    let v = Rc::new(View {
        filter: filter.clone(),
        count,
        group_status,
        group_ws,
        chips,
        ws_chips,
        body,
        scroller,
        stage,
        preview,
        query: RefCell::new(q),
        overview: RefCell::new(built),
        facts: RefCell::new(facts),
        tails: RefCell::default(),
        sel: Cell::new(sel),
        flip: Cell::new(if flip {
            Flip::opened()
        } else {
            Flip {
                presses: 1,
                over: true,
            }
        }),
        cols: Cell::new(cols()),
        cards: RefCell::default(),
        timer: RefCell::default(),
        shown_once: Cell::new(false),
    });
    OPEN.with(|o| *o.borrow_mut() = Some(v.clone()));
    read_tails(&v);
    render(&v);
    keys_preview(&v, KEYS_MS);
    filter.connect_changed(|e| {
        if let Some(v) = view() {
            v.query.borrow_mut().text = e.text().to_string();
            requery(&v, true);
            hide_preview(&v);
            keys_preview(&v, TYPED_MS);
        }
    });
    // The cards scroll under the Preview: it follows its card.
    v.scroller.vadjustment().connect_value_changed(|_| {
        if let Some(v) = view() {
            place_later(&v, false);
        }
    });
    filter.connect_activate(|_| {
        if let Some(v) = view() {
            jump(&v, false);
        }
    });

    // In (R117.6): the window behind shrinks, the layer fades in.
    let d = ms(IN_MS);
    LAST_MS.with(|l| l.set((d, l.get().1)));
    root.behind.add_css_class("ov-open");
    layer.remove_css_class("ov-out");
    layer.remove_css_class("ov-out-jump");
    layer.set_visible(true);
    layer.add_css_class("ov-in");
    filter.grab_focus();
    // R117.8: first frame with the cards, and each frame of the motion.
    crate::perf::ready("overview", &app.window);
    crate::perf::frames("overview", &app.window, u64::from(IN_MS) + 200);

    let id = glib::timeout_add_local(Duration::from_secs(1), || match view() {
        Some(v) => {
            tick(&v);
            glib::ControlFlow::Continue
        }
        None => glib::ControlFlow::Break,
    });
    *v.timer.borrow_mut() = Some(id);
}

#[derive(Clone, Copy, PartialEq)]
enum Exit {
    Esc,
    Jump,
}

fn close(how: Exit) {
    let Some(v) = OPEN.with(|o| o.borrow_mut().take()) else {
        return;
    };
    if let Some(id) = v.timer.borrow_mut().take() {
        id.remove();
    }
    hide_preview(&v);
    let Some(root) = ROOT.with(|r| r.borrow().clone()) else {
        return;
    };
    let d = ms(if how == Exit::Jump { JUMP_MS } else { ESC_MS });
    LAST_MS.with(|l| l.set((l.get().0, d)));
    root.behind.remove_css_class("ov-open");
    root.layer.remove_css_class("ov-in");
    let out = if how == Exit::Jump {
        "ov-out-jump"
    } else {
        "ov-out"
    };
    root.layer.add_css_class(out);
    let layer = root.layer.clone();
    let hide = move || {
        // Opened again meanwhile: keep it.
        if is_open() {
            return;
        }
        layer.set_visible(false);
        layer.remove_css_class("ov-out");
        layer.remove_css_class("ov-out-jump");
        while let Some(c) = layer.first_child() {
            layer.remove(&c);
        }
    };
    if d == 0 {
        hide();
    } else {
        // The layer can't take clicks or keys while it fades out.
        root.layer.set_can_target(false);
        let l = root.layer.clone();
        glib::timeout_add_local_once(Duration::from_millis(d as u64), move || {
            l.set_can_target(true);
            hide();
        });
    }
    if how == Exit::Esc {
        if let Some(app) = try_app() {
            app.restore_focus();
        }
    }
}

/// Grid columns for the window's width and text scale (R117.7).
fn cols() -> usize {
    let w = crate::layout_mode::window_width().max(640) - 80;
    // Cards at least 300 px wide: 4 columns at 1440 px, 3 at 1280, 6 at 2000.
    let mut n = ((w + 10) / 310).clamp(1, 8) as usize;
    if crate::layout_mode::text_scale() >= 1.5 {
        n = n.min(2);
    }
    n
}

fn workspaces(app: &App) -> Vec<(String, String)> {
    app.model
        .borrow()
        .workspace_looks()
        .into_iter()
        .map(|l| (l.id, l.name))
        .collect()
}

/// `Project / link / Worktree` of a Pane, `Scratch` for none.
fn place(
    m: &rade_core::State,
    projects: &HashMap<String, Vec<rade_core::project::Project>>,
    pane: &str,
) -> String {
    let Some(loc) = m.locate(pane) else {
        return String::new();
    };
    let Some(s) = m.source(&loc.source) else {
        return String::new();
    };
    let Some(w) = s.worktree(&loc.worktree) else {
        return String::new();
    };
    let linked = !m.is_main_worktree(&s.id, &w.id);
    if s.id == rade_core::model::general_id(&s.workspace) {
        return "Scratch".into();
    }
    let list = projects.get(&s.workspace).map(Vec::as_slice).unwrap_or(&[]);
    let project = m
        .project_of_worktree(&s.id, w, list)
        .or_else(|| {
            (!linked)
                .then(|| list.iter().find(|p| p.lists(&s.id)))
                .flatten()
        })
        .map(|p| p.name.clone());
    let mut parts: Vec<&str> = Vec::new();
    if let Some(p) = &project {
        parts.push(p);
    }
    parts.push(&s.name);
    if linked {
        parts.push(&w.name);
    }
    parts.join(" / ")
}

fn facts(app: &App) -> Vec<Fact> {
    let m = app.model.borrow();
    let projects: HashMap<String, Vec<rade_core::project::Project>> = m
        .workspaces
        .iter()
        .map(|w| (w.id.clone(), rade_core::project::list(&w.root)))
        .collect();
    let mut facts = ov::facts(&m, &|p| place(&m, &projects, p));
    for f in &mut facts {
        f.changed_at = crate::status::last_change(&f.pane);
        if let Some(loc) = m.locate(&f.pane) {
            if let Some(w) = m.worktree(&loc.source, &loc.worktree) {
                let (ahead, changed) = crate::navdata::worktree_facts(&m, &w.path);
                f.git = match (ahead, changed) {
                    (0, 0) => None,
                    (0, c) => Some(format!("{c} changed")),
                    (a, 0) => Some(format!("↑{a}")),
                    (a, c) => Some(format!("↑{a} · {c} changed")),
                };
            }
        }
    }
    facts
}

/// The last 3 non-blank screen lines of the cards that show a tail.
fn read_tails(v: &View) {
    let Some(app) = try_app() else { return };
    let want: Vec<String> = v
        .overview
        .borrow()
        .sections
        .iter()
        .flat_map(|s| s.cards.iter())
        .filter(|c| c.band() != Band::Quiet)
        .filter_map(|c| match c {
            Card::Agent(f) => Some(f.pane.clone()),
            Card::Fanout { .. } => None,
        })
        .collect();
    let terms = app.terminals.borrow();
    let mut tails = v.tails.borrow_mut();
    tails.retain(|k, _| want.contains(k));
    for id in want {
        if let Some(text) = terms
            .get(&id)
            .and_then(|t| t.handle.read_text(GHOSTTY_POINT_ACTIVE))
        {
            tails.insert(id, ov::tail_of(&text, 3));
        }
    }
}

/// A card's tail: one single-line label per screen line, each ellipsized
/// at the card's width, in a box that clips (a multi-line label with
/// `set_lines` but no wrapping drew past its allocation).
fn tail_box(lines: &[String]) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
    b.set_widget_name("ov-tail");
    b.add_css_class("overview-card-tail");
    b.set_overflow(gtk::Overflow::Hidden);
    fill_tail(&b, lines);
    b
}

fn fill_tail(b: &gtk::Box, lines: &[String]) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
    for line in lines {
        let l = label(line, &["overview-card-tail-line"]);
        l.set_single_line_mode(true);
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        b.append(&l);
    }
}

/// `debug.ui overview`: how a card's tail is built.
fn tail_widget(card: &gtk::Box) -> Value {
    let Some(b) = named(card.upcast_ref(), "ov-tail") else {
        return Value::Null;
    };
    let mut lines = 0;
    let mut single = true;
    let mut c = b.first_child();
    while let Some(x) = c {
        let l = x.clone().downcast::<gtk::Label>().ok();
        single &= l.is_some_and(|l| {
            l.is_single_line_mode() && l.ellipsize() == gtk::pango::EllipsizeMode::End
        });
        lines += 1;
        c = x.next_sibling();
    }
    json!({"lines": lines, "single": single, "clip": b.overflow() == gtk::Overflow::Hidden})
}

/// Once a second while open: statuses update in place, tails re-read.
fn tick(v: &Rc<View>) {
    let Some(app) = try_app() else { return };
    let before = shape(&v.overview.borrow());
    *v.facts.borrow_mut() = facts(&app);
    requery(v, false);
    read_tails(v);
    if shape(&v.overview.borrow()) == before {
        update_in_place(v);
    } else {
        render(v);
    }
    refresh_preview(v);
}

/// Which cards sit where: a tick that keeps it updates texts in place.
/// Per section: each card's key and status, and the folded count.
type Shape = Vec<(Vec<(String, Option<rade_core::AgentStatus>)>, usize)>;

/// A status change rebuilds the cards too (each glyph takes its colour).
fn shape(o: &Overview) -> Shape {
    o.sections
        .iter()
        .map(|s| {
            let cards = s.cards.iter().map(|c| (c.key(), card_status(c))).collect();
            (cards, s.more)
        })
        .collect()
}

/// The descendant of `w` with widget name `name`.
fn named(w: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
    let mut c = w.first_child();
    while let Some(x) = c {
        if x.widget_name() == name {
            return Some(x);
        }
        if let Some(found) = named(&x, name) {
            return Some(found);
        }
        c = x.next_sibling();
    }
    None
}

fn status_class(s: Option<rade_core::AgentStatus>) -> String {
    s.map_or("idle", |s| s.as_str()).to_string()
}

/// Statuses, ages and tails of the cards on screen, without a rebuild.
fn update_in_place(v: &Rc<View>) {
    let o = v.overview.borrow();
    let tails = v.tails.borrow();
    let cards = v.cards.borrow();
    let now = now();
    for (pos, w) in cards.iter() {
        let Some(c) = o.card(*pos) else { continue };
        let st = card_status(c);
        if let Some(l) = named(w.upcast_ref(), "ov-status").and_downcast::<gtk::Label>() {
            l.set_label(&c.status_text(now));
            l.set_css_classes(&["overview-card-status", &format!("st-{}", status_class(st))]);
        }
        if let (Card::Agent(f), Some(b)) = (
            c,
            named(w.upcast_ref(), "ov-tail").and_downcast::<gtk::Box>(),
        ) {
            if let Some(t) = tails.get(&f.pane) {
                fill_tail(&b, t);
            }
        }
    }
    drop((o, tails, cards));
    render_chips(v);
}

/// New filter / group / band: the frozen order, re-filtered.
fn requery(v: &Rc<View>, reselect: bool) {
    let Some(app) = try_app() else { return };
    let wss = workspaces(&app);
    let refresh = |q: &Query| v.overview.borrow().refresh(&v.facts.borrow(), q, &wss);
    // Quiet's room is measured on the unfolded layout (R117.2).
    let mut q = v.query.borrow().clone();
    q.quiet_cap = None;
    let mut next = refresh(&q);
    q.quiet_cap = quiet_cap(&next, v.cols.get(), body_height(Some(v)));
    if q.quiet_cap.is_some() {
        next = refresh(&q);
    }
    v.query.borrow_mut().quiet_cap = q.quiet_cap;
    let keep = v
        .sel
        .get()
        .and_then(|p| v.overview.borrow().card(p).map(Card::key));
    let sel = if reselect {
        ov::initial(&next, None)
    } else {
        keep.and_then(|k| {
            next.sections
                .iter()
                .enumerate()
                .find_map(|(i, s)| s.cards.iter().position(|c| c.key() == k).map(|j| (i, j)))
        })
        .or_else(|| (next.shown() > 0).then_some((0, 0)))
    };
    *v.overview.borrow_mut() = next;
    v.sel.set(
        sel.filter(|p| v.overview.borrow().card(*p).is_some())
            .or_else(|| {
                let o = v.overview.borrow();
                o.sections
                    .iter()
                    .position(|s| !s.cards.is_empty())
                    .map(|i| (i, 0))
            }),
    );
    if reselect {
        render(v);
    }
}

fn set_group(g: Group) {
    GROUP.with(|c| c.set(g));
    if let Some(v) = view() {
        v.query.borrow_mut().group = g;
        requery(&v, true);
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn render(v: &Rc<View>) {
    render_chips(v);
    render_body(v);
    // New card widgets: the Preview follows its card, or goes with it.
    let shown = v.preview.shown.borrow().as_ref().map(|s| s.0.clone());
    if let Some(k) = shown {
        if card_by_key(v, &k).is_some() {
            place_later(v, false);
        } else {
            hide_preview(v);
        }
    }
}

fn render_chips(v: &Rc<View>) {
    let Some(app) = try_app() else { return };
    let o = v.overview.borrow();
    let q = v.query.borrow();
    let looks = app.model.borrow().workspace_looks();
    v.count.set_label(&format!(
        "{} in {} Workspaces",
        o.total,
        o.workspaces.max(1)
    ));
    for (l, g) in [
        (&v.group_status, Group::Status),
        (&v.group_ws, Group::Workspace),
    ] {
        if q.group == g {
            l.add_css_class("on");
        } else {
            l.remove_css_class("on");
        }
    }
    // Chips: counts (a band filter), then Workspaces (one Workspace).
    v.chips.remove_all();
    v.ws_chips.remove_all();
    for (band, led_class, text) in o.counts.chips() {
        let chip = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        chip.add_css_class("overview-chip");
        if q.band == Some(band) {
            chip.add_css_class("on");
        }
        let l = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        l.add_css_class("led");
        l.add_css_class(&format!(
            "led-{}",
            match led_class {
                "wait" => "waiting",
                "err" => "error",
                "intr" => "interrupted",
                "done" => "done",
                "run" => "running",
                _ => "idle",
            }
        ));
        l.set_valign(gtk::Align::Center);
        chip.append(&l);
        chip.append(&label(&text, &[]));
        let c = gtk::GestureClick::new();
        c.connect_released(move |_, _, _, _| toggle_band(band));
        chip.add_controller(c);
        v.chips.append(&chip);
    }
    for (i, l) in looks.iter().enumerate() {
        let n = o.per_workspace.get(&l.id).copied().unwrap_or(0);
        let chip = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        chip.add_css_class("overview-chip");
        chip.add_css_class("overview-ws-chip");
        if q.workspace.as_deref() == Some(l.id.as_str()) {
            chip.add_css_class("on");
        }
        let dot = gtk::Label::new(None);
        dot.set_markup(&format!("<span foreground=\"{}\">■</span>", l.color));
        chip.append(&dot);
        chip.append(&label(&format!("{} {n}", l.name), &[]));
        if i < 9 {
            chip.append(&label(&format!("⌃⌥{}", i + 1), &["overview-key"]));
        }
        let id = l.id.clone();
        let c = gtk::GestureClick::new();
        c.connect_released(move |_, _, _, _| filter_workspace(&id));
        chip.add_controller(c);
        v.ws_chips.append(&chip);
    }
}

fn render_body(v: &Rc<View>) {
    let Some(app) = try_app() else { return };
    let o = v.overview.borrow();
    let q = v.query.borrow();
    let looks = app.model.borrow().workspace_looks();
    while let Some(c) = v.body.first_child() {
        v.body.remove(&c);
    }
    v.cards.borrow_mut().clear();
    if let Some(line) = o.empty_line(&q) {
        let l = label(&line, &["overview-empty"]);
        l.set_halign(gtk::Align::Center);
        l.set_margin_top(48);
        v.body.append(&l);
        return;
    }
    let now = now();
    let tails = v.tails.borrow();
    let sel = v.sel.get();
    let mut n = 0usize;
    let animate = !v.shown_once.replace(true);
    let mut card_of = |pos: Pos, c: &Card, compact: bool| {
        let w = card(c, compact, now, &tails, sel == Some(pos));
        if animate && n < STAGGERED {
            w.add_css_class(&format!("ov-rise-{n}"));
        }
        n += 1;
        let p = pos;
        let click = gtk::GestureClick::new();
        click.connect_released(move |_, _, _, _| {
            if let Some(v) = view() {
                v.sel.set(Some(p));
                jump(&v, false);
            }
        });
        w.add_controller(click);
        // Motion, not enter: a card that appears under a still pointer
        // shows nothing until the pointer moves and rests (R117.9).
        let hover = gtk::EventControllerMotion::new();
        let k = c.key();
        hover.connect_motion(move |_, _, _| {
            if let Some(v) = view() {
                hover_motion(&v, &k);
            }
        });
        let k = c.key();
        hover.connect_leave(move |_| {
            if let Some(v) = view() {
                hover_leave(&v, &k);
            }
        });
        w.add_controller(hover);
        v.cards.borrow_mut().insert(pos, w.clone());
        w
    };
    match q.group {
        Group::Status => {
            let cols = v.cols.get();
            for (i, s) in o.sections.iter().enumerate() {
                let sec = gtk::Box::new(gtk::Orientation::Vertical, 8);
                sec.add_css_class("overview-band");
                let h = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                h.add_css_class("overview-band-label");
                h.append(&label(&s.title.to_uppercase(), &["overview-band-title"]));
                h.append(&label(&s.total().to_string(), &["overview-band-count"]));
                sec.append(&h);
                let grid = gtk::Grid::new();
                grid.set_column_spacing(10);
                grid.set_row_spacing(10);
                grid.set_column_homogeneous(true);
                let compact = s.band == Some(Band::Quiet);
                for (j, c) in s.cards.iter().enumerate() {
                    let w = card_of((i, j), c, compact);
                    grid.attach(&w, (j % cols) as i32, (j / cols) as i32, 1, 1);
                }
                // The folded rest: a tile right after the last card.
                let mut slots = s.cards.len();
                if s.more > 0 {
                    let j = slots;
                    grid.attach(&more_tile(s), (j % cols) as i32, (j / cols) as i32, 1, 1);
                    slots += 1;
                }
                // Fill the row so a lone card keeps its column width.
                for k in slots..cols {
                    let pad = gtk::Box::new(gtk::Orientation::Vertical, 0);
                    grid.attach(&pad, k as i32, 0, 1, 1);
                }
                sec.append(&grid);
                v.body.append(&sec);
            }
        }
        Group::Workspace => {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            row.set_homogeneous(true);
            for (i, s) in o.sections.iter().enumerate() {
                let col = gtk::Box::new(gtk::Orientation::Vertical, 8);
                col.add_css_class("overview-col");
                let h = gtk::Box::new(gtk::Orientation::Horizontal, 7);
                h.add_css_class("overview-col-head");
                if let Some(l) = looks.iter().find(|l| Some(&l.id) == s.workspace.as_ref()) {
                    let dot = gtk::Label::new(None);
                    dot.set_markup(&format!("<span foreground=\"{}\">■</span>", l.color));
                    h.append(&dot);
                }
                h.append(&label(&s.title, &["overview-col-title"]));
                let need = s
                    .cards
                    .iter()
                    .filter(|c| c.band() == Band::NeedsYou)
                    .count();
                let meta = label(
                    &if need > 0 {
                        format!("{need} need you · {}", s.total())
                    } else {
                        s.total().to_string()
                    },
                    &["overview-band-count"],
                );
                meta.set_hexpand(true);
                meta.set_xalign(1.0);
                h.append(&meta);
                col.append(&h);
                for (j, c) in s.cards.iter().enumerate() {
                    let compact = c.band() == Band::Quiet;
                    col.append(&card_of((i, j), c, compact));
                }
                row.append(&col);
            }
            v.body.append(&row);
        }
    }
}

/// An Agent's glyph coloured by its status (R115); a Fan-out card keeps
/// an LED: it shows the group's status, not one agent.
fn glyph(c: &Card) -> gtk::Widget {
    let pane = match c {
        Card::Agent(f) => try_app().and_then(|a| a.model.borrow().pane(&f.pane).cloned()),
        Card::Fanout { .. } => None,
    };
    match &pane {
        Some(p) => crate::agent_glyph::Glyph::of_pane(p).root.upcast(),
        None => led(card_status(c)).upcast(),
    }
}

/// The Workspace's color square.
fn ws_dot(workspace: &str) -> gtk::Label {
    let dot = gtk::Label::new(None);
    if let Some(app) = try_app() {
        if let Some(l) = app
            .model
            .borrow()
            .workspace_looks()
            .into_iter()
            .find(|l| l.id == workspace)
        {
            dot.set_markup(&format!("<span foreground=\"{}\">■</span>", l.color));
        }
    }
    dot
}

fn card(
    c: &Card,
    compact: bool,
    now: u64,
    tails: &HashMap<String, Vec<String>>,
    selected: bool,
) -> gtk::Box {
    let f0 = c.facts()[0];
    let status = card_status(c);
    let w = gtk::Box::new(
        if compact {
            gtk::Orientation::Horizontal
        } else {
            gtk::Orientation::Vertical
        },
        if compact { 8 } else { 6 },
    );
    w.set_widget_name(&format!("card-{}", c.key()));
    w.add_css_class("overview-card");
    if compact {
        w.add_css_class("compact");
    }
    if selected {
        w.add_css_class("selected");
    }
    let r1 = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    r1.append(&glyph(c));
    let name = match c {
        Card::Agent(f) => f.name.clone(),
        Card::Fanout { id, .. } => format!("Fan-out {id}"),
    };
    let n = label(&name, &["overview-card-name"]);
    n.set_ellipsize(gtk::pango::EllipsizeMode::End);
    r1.append(&n);
    if let Card::Agent(f) = c {
        if !compact {
            if let Some(m) = &f.model {
                let l = label(m, &["overview-card-model"]);
                l.set_ellipsize(gtk::pango::EllipsizeMode::End);
                r1.append(&l);
            }
        }
    } else if let Card::Fanout { members, .. } = c {
        r1.append(&label(
            &format!("{} Agents", members.len()),
            &["overview-card-model"],
        ));
    }
    let st = label(
        &c.status_text(now),
        &[
            "overview-card-status",
            &format!("st-{}", status.map_or("idle", |s| s.as_str())),
        ],
    );
    st.set_widget_name("ov-status");
    st.set_hexpand(true);
    st.set_xalign(1.0);
    let where_ = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    where_.add_css_class("overview-card-where");
    where_.append(&ws_dot(&f0.workspace));
    where_.append(&label(&f0.workspace_name, &["overview-card-ws"]));
    let place = match &f0.parent {
        Some(p) => format!("· {} · ↳ {p}", f0.place),
        None => format!("· {}", f0.place),
    };
    let pl = label(&place, &["overview-card-place"]);
    pl.set_ellipsize(gtk::pango::EllipsizeMode::End);
    pl.set_hexpand(true);
    where_.append(&pl);
    if compact {
        w.append(&r1);
        where_.set_hexpand(true);
        w.append(&where_);
        st.set_hexpand(false);
        w.append(&st);
        return w;
    }
    r1.append(&st);
    w.append(&r1);
    w.append(&where_);
    match c {
        Card::Agent(f) => {
            if let Some(t) = tails.get(&f.pane).filter(|t| !t.is_empty()) {
                w.append(&tail_box(t));
            }
        }
        Card::Fanout { members, .. } => {
            let kids = gtk::Box::new(gtk::Orientation::Vertical, 3);
            kids.add_css_class("overview-card-kids");
            for m in members {
                let r = gtk::Box::new(gtk::Orientation::Horizontal, 7);
                r.append(&led(m.status));
                let text = Card::Agent(Box::new(m.clone())).status_text(now);
                r.append(&label(&format!("{} · {text}", m.name), &[]));
                kids.append(&r);
            }
            w.append(&kids);
        }
    }
    let foot = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    foot.add_css_class("overview-card-foot");
    if let Some(n) = &f0.note {
        let l = label(n, &["overview-card-note"]);
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        foot.append(&l);
    }
    for t in [&f0.git, &f0.account].into_iter().flatten() {
        let l = label(t, &[]);
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        foot.append(&l);
    }
    let k = label("↵ jump", &["overview-card-jump"]);
    k.set_hexpand(true);
    k.set_xalign(1.0);
    k.set_opacity(if selected { 1.0 } else { 0.0 });
    foot.append(&k);
    w.append(&foot);
    w
}

/// What a click leaves to its own handler: cards, the Preview (part of its
/// card), the `+n more` tile, chips, the filter field, the group toggle.
const CLICKABLE: [&str; 6] = [
    "overview-card",
    "overview-preview",
    "overview-more-tile",
    "overview-chip",
    "overview-filter",
    "overview-group",
];

/// A click at (`x`, `y`) on the layer (R117.1): anywhere but a card, a
/// tile, a chip, the filter, the group toggle or a scrollbar closes it like
/// `Esc`, as Mission Control does. `true` = it closed.
fn click_outside(x: f64, y: f64) -> bool {
    let Some(root) = ROOT.with(|r| r.borrow().clone()) else {
        return false;
    };
    if !is_open() {
        return false;
    }
    let layer: gtk::Widget = root.layer.clone().upcast();
    let mut w = layer.pick(x, y, gtk::PickFlags::DEFAULT);
    while let Some(x) = w {
        if x == layer {
            break;
        }
        if CLICKABLE.iter().any(|c| x.has_css_class(c)) || x.is::<gtk::Scrollbar>() {
            return false;
        }
        w = x.parent();
    }
    close(Exit::Esc);
    true
}

/// `+n more ▾` after a folded band's last card; a click unfolds it.
fn more_tile(s: &ov::Section) -> gtk::Box {
    let tile = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    tile.set_widget_name("overview-more");
    tile.add_css_class("overview-more-tile");
    tile.append(&label(&format!("+{} more ▾", s.more), &["overview-more"]));
    let b = s.band;
    let c = gtk::GestureClick::new();
    c.connect_released(move |_, _, _, _| {
        if let (Some(v), Some(b)) = (view(), b) {
            v.query.borrow_mut().unfolded.insert(b);
            requery(&v, false);
            render(&v);
        }
    });
    tile.add_controller(c);
    tile
}

/// Rough heights (px) of what the body stacks, for Quiet's room.
const BAND_HEAD: i32 = 26;
const BAND_GAP: i32 = 18;
const ROW_GAP: i32 = 10;
const CARD_H: i32 = 150;
const ROW_H: i32 = 36;

/// How many Quiet cards fit under the other bands (R117.2): `None` when
/// all of them do, else whole rows (at least one), one slot left for the
/// `+n more` tile.
fn quiet_cap(o: &Overview, cols: usize, height: i32) -> Option<usize> {
    let cols = cols.max(1);
    let rows = |n: usize| n.div_ceil(cols) as i32;
    let mut used = 0;
    let mut quiet = 0;
    for s in &o.sections {
        match s.band {
            Some(Band::Quiet) => quiet = s.total(),
            Some(_) => {
                let r = rows(s.cards.len() + usize::from(s.more > 0));
                used += BAND_HEAD + r * CARD_H + (r - 1).max(0) * ROW_GAP + BAND_GAP;
            }
            None => return None,
        }
    }
    let left = height - used - BAND_HEAD;
    let fit = ((left + ROW_GAP) / (ROW_H + ROW_GAP)).max(0) as usize;
    if quiet == 0 || rows(quiet) as usize <= fit {
        return None;
    }
    Some((fit.max(1) * cols).saturating_sub(1).max(1))
}

/// The body's height: the scroller's once laid out, else from the window.
fn body_height(v: Option<&View>) -> i32 {
    match v.map(|v| v.scroller.height()).filter(|h| *h > 0) {
        Some(h) => h - 36,
        None => crate::layout_mode::window_height() - 190,
    }
}

/// A card's LED status: the Agent's, a Fan-out's most urgent member's.
fn card_status(c: &Card) -> Option<rade_core::AgentStatus> {
    match c {
        Card::Agent(f) => f.status,
        Card::Fanout { members, .. } => members
            .iter()
            .min_by_key(|m| (ov::band_of(m), m.status.map(|s| s as u8)))
            .and_then(|m| m.status),
    }
}

/// Move the selection without rebuilding the cards.
fn select(v: &Rc<View>, to: Option<Pos>) {
    let cards = v.cards.borrow();
    if let Some(w) = v.sel.get().and_then(|p| cards.get(&p)) {
        w.remove_css_class("selected");
        set_jump_hint(w, false);
    }
    v.sel.set(to);
    if let Some(w) = to.and_then(|p| cards.get(&p)) {
        w.add_css_class("selected");
        set_jump_hint(w, true);
        // Keep it in view.
        if let Some(b) = w.compute_bounds(&v.body) {
            let adj = v.scroller.vadjustment();
            let (top, h) = (b.y() as f64, b.height() as f64);
            if top < adj.value() {
                adj.set_value(top - 8.0);
            } else if top + h > adj.value() + adj.page_size() {
                adj.set_value(top + h - adj.page_size() + 8.0);
            }
        }
    }
}

fn set_jump_hint(card: &gtk::Box, on: bool) {
    let mut c = card.last_child();
    // The foot's last label.
    if let Some(foot) = c.take() {
        if let Some(k) = foot.last_child() {
            if k.has_css_class("overview-card-jump") {
                k.set_opacity(if on { 1.0 } else { 0.0 });
            }
        }
    }
}

fn step(v: &Rc<View>, m: Move) {
    let Some(at) = v.sel.get() else { return };
    let to = {
        let o = v.overview.borrow();
        ov::step(&o, v.cols.get(), v.query.borrow().group, at, m)
    };
    select(v, Some(to));
    keys_preview(v, KEYS_MS);
}

/// A Workspace chip or `Ctrl+Alt+n`: only that Workspace, again = all.
/// The query is borrowed only to change it: `requery` reads it again.
fn filter_workspace(id: &str) {
    if let Some(v) = view() {
        {
            let mut q = v.query.borrow_mut();
            q.workspace = if q.workspace.as_deref() == Some(id) {
                None
            } else {
                Some(id.to_string())
            };
        }
        requery(&v, true);
    }
}

/// A count chip: only that band, again = all.
fn toggle_band(band: Band) {
    if let Some(v) = view() {
        {
            let mut q = v.query.borrow_mut();
            q.band = if q.band == Some(band) {
                None
            } else {
                Some(band)
            };
        }
        requery(&v, true);
    }
}

/// Jump to the selected card (R117.5): the Notification-click path.
fn jump(v: &Rc<View>, zoom: bool) {
    let Some(card) = v
        .sel
        .get()
        .and_then(|p| v.overview.borrow().card(p).cloned())
    else {
        return;
    };
    close(Exit::Jump);
    let Some(app) = try_app() else { return };
    let pane = card.pane().to_string();
    let _ = app.update(|m| {
        m.focus_pane(&pane)?;
        if zoom {
            if let Some(loc) = m.locate(&pane) {
                let zoomed = m.active_tab().and_then(|t| t.zoomed.clone());
                if zoomed.as_deref() != Some(pane.as_str()) {
                    m.toggle_zoom(&loc.tab)?;
                }
            }
        }
        Ok::<_, rade_core::Error>(())
    });
    app.restore_focus();
}

/// A key while open (the layer's capture handler and `debug.ui key`).
pub fn key(k: Key, m: ModifierType) -> bool {
    let Some(v) = view() else { return false };
    let ctrl = m.contains(ModifierType::CONTROL_MASK);
    let shift = m.contains(ModifierType::SHIFT_MASK);
    let alt = m.contains(ModifierType::ALT_MASK);
    match k {
        Key::Escape => {
            let has_filter = {
                let q = v.query.borrow();
                !q.text.is_empty() || q.band.is_some() || q.workspace.is_some()
            };
            if has_filter {
                {
                    let mut q = v.query.borrow_mut();
                    q.band = None;
                    q.workspace = None;
                }
                if v.filter.text().is_empty() {
                    requery(&v, true);
                } else {
                    v.filter.set_text("");
                }
            } else {
                close(Exit::Esc);
            }
            true
        }
        Key::Up => {
            step(&v, Move::Up);
            true
        }
        Key::Down => {
            step(&v, Move::Down);
            true
        }
        Key::Left if v.filter.text().is_empty() || v.query.borrow().group == Group::Workspace => {
            step(&v, Move::Left);
            true
        }
        Key::Right if v.filter.text().is_empty() || v.query.borrow().group == Group::Workspace => {
            step(&v, Move::Right);
            true
        }
        Key::Tab => {
            step(&v, Move::NextSection);
            true
        }
        Key::ISO_Left_Tab => {
            step(&v, Move::PrevSection);
            true
        }
        Key::Return | Key::KP_Enter => {
            jump(&v, shift);
            true
        }
        Key::p | Key::P if alt && !ctrl => {
            toggle_previews(&v);
            true
        }
        // R119.5: the selected Agent's Worktree Review.
        Key::r | Key::R if alt && !ctrl => {
            let pane = v
                .sel
                .get()
                .and_then(|p| v.overview.borrow().card(p).map(|c| c.pane().to_string()));
            if let Some(pane) = pane {
                jump(&v, false);
                crate::review::open_for(&pane);
            }
            true
        }
        Key::g | Key::G if alt && !ctrl => {
            let g = match v.query.borrow().group {
                Group::Status => Group::Workspace,
                Group::Workspace => Group::Status,
            };
            set_group(g);
            true
        }
        _ if ctrl && alt => {
            let digit = k
                .to_unicode()
                .and_then(|c| c.to_digit(10))
                .filter(|d| *d >= 1);
            match digit {
                Some(d) => {
                    let id = try_app().and_then(|a| {
                        a.model
                            .borrow()
                            .workspace_looks()
                            .get(d as usize - 1)
                            .map(|l| l.id.clone())
                    });
                    if let Some(id) = id {
                        filter_workspace(&id);
                    }
                    true
                }
                None => false,
            }
        }
        _ => false,
    }
}

/// `Ctrl` released: a flip of 2+ presses jumps (R117.4).
pub fn release(k: Key) {
    if !matches!(k, Key::Control_L | Key::Control_R) {
        return;
    }
    let Some(v) = view() else { return };
    let mut f = v.flip.get();
    let go = f.release();
    v.flip.set(f);
    if go {
        jump(&v, false);
    }
}

/// `debug.ui {"op": "overview", "arg": "open|toggle|band <b>|state|close|release"}`.
pub fn debug(p: &Value) -> Result<Value> {
    let arg = p.get("arg").and_then(Value::as_str).unwrap_or("state");
    let (cmd, rest) = arg.split_once(' ').unwrap_or((arg, ""));
    match cmd {
        "open" => {
            if !is_open() {
                open(Query::default(), false);
            }
        }
        "shortcut" => shortcut(),
        "toggle" => toggle(),
        "band" => open_band(match rest {
            "" | "all" => None,
            b => Some(Band::parse(b).ok_or_else(|| Error::usage(format!("no band '{b}'")))?),
        }),
        "close" => close(Exit::Esc),
        // A count chip's click (R117.1).
        "chip" => {
            toggle_band(Band::parse(rest).ok_or_else(|| Error::usage(format!("no band '{rest}'")))?)
        }
        "release" => release(Key::Control_L),
        // A click at layer coordinates `x y` (what the pointer does).
        "click" => {
            let mut xy = rest.split_whitespace().map(str::parse::<f64>);
            let (Some(Ok(x)), Some(Ok(y))) = (xy.next(), xy.next()) else {
                return Err(Error::usage("overview click needs x y"));
            };
            let closed = click_outside(x, y);
            let mut st = state();
            st["closed"] = json!(closed);
            return Ok(st);
        }
        "filter" => {
            let v = view().ok_or_else(|| Error::usage("Agent Overview is not open"))?;
            v.filter.set_text(rest);
        }
        // The pointer moves onto / rests on a Pane's card (R117.9).
        "hover" => {
            let v = view().ok_or_else(|| Error::usage("Agent Overview is not open"))?;
            let key = {
                let o = v.overview.borrow();
                o.find(rest)
                    .and_then(|p| o.card(p).map(Card::key))
                    .ok_or_else(|| Error::usage(format!("no card for '{rest}'")))?
            };
            hover_motion(&v, &key);
        }
        // The pointer leaves the card and the Preview.
        "leave" => {
            let v = view().ok_or_else(|| Error::usage("Agent Overview is not open"))?;
            v.preview.inside.set(false);
            let key = v.preview.shown.borrow().as_ref().map(|s| s.0.clone());
            let pending = v.preview.pending.borrow().as_ref().map(|p| p.0.clone());
            if let Some(k) = key.or(pending) {
                hover_leave(&v, &k);
            }
        }
        // A click on the Preview.
        "preview_click" => preview_jump(),
        "state" => {}
        _ => return Err(Error::usage(format!("overview op '{arg}'"))),
    }
    Ok(state())
}

fn state() -> Value {
    let (open_ms, close_ms) = LAST_MS.with(Cell::get);
    let durations = json!({
        "in": open_ms, "out": close_ms,
        "full": {"in": IN_MS, "jump": JUMP_MS, "esc": ESC_MS},
        "animations": animations(),
    });
    let Some(v) = view() else {
        return json!({"open": false, "durations": durations});
    };
    let o = v.overview.borrow();
    let q = v.query.borrow();
    let tails = v.tails.borrow();
    let widgets = v.cards.borrow();
    let sel = v.sel.get();
    let now = now();
    let sections: Vec<Value> = o
        .sections
        .iter()
        .enumerate()
        .map(|(i, s)| {
            json!({
                "title": s.title,
                "band": s.band.map(Band::id),
                "workspace": s.workspace,
                "more": s.more,
                "total": s.total(),
                "cards": s.cards.iter().enumerate().map(|(j, c)| {
                    let f = c.facts()[0];
                    json!({
                        "key": c.key(),
                        "panes": c.panes(),
                        "name": match c { Card::Agent(f) => f.name.clone(), Card::Fanout { id, .. } => format!("Fan-out {id}") },
                        "status": c.status_text(now),
                        "workspace": f.workspace_name,
                        "place": f.place,
                        "parent": f.parent,
                        "note": f.note,
                        "git": f.git,
                        "tail": tails.get(&f.pane).cloned().unwrap_or_default(),
                        "tail_widget": widgets.get(&(i, j)).map_or(Value::Null, tail_widget),
                        "selected": sel == Some((i, j)),
                    })
                }).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "open": true,
        "title": "Agents",
        "count": v.count.text().as_str(),
        "counts": o.counts.chips().into_iter().map(|c| c.2).collect::<Vec<_>>(),
        "group": q.group.as_str(),
        "filter": q.text,
        "band": q.band.map(Band::id),
        "workspace": q.workspace,
        "cols": v.cols.get(),
        "sections": sections,
        "empty": o.empty_line(&q),
        "flip": {"presses": v.flip.get().presses, "over": v.flip.get().over},
        "durations": durations,
        "preview": preview_state(&v),
    })
}

/// What showed the Preview: the pointer resting on a card, or the keyboard
/// selection resting.
#[derive(Clone, Copy, PartialEq)]
enum By {
    Hover,
    Keys,
}

/// The Preview (R117.9): one card's screen, larger, floating beside it on
/// the stage, over other cards but never its own.
struct Preview {
    root: gtk::Box,
    head: gtk::Box,
    lines: gtk::Box,
    scroll: gtk::ScrolledWindow,
    foot: gtk::Box,
    /// The card shown (its key) and what showed it.
    shown: RefCell<Option<(String, By)>>,
    /// Where it sits, its card and the body, in the stage's coordinates.
    place: Cell<Option<(ov::Placement, ov::Rect, ov::Rect)>>,
    /// Waiting to show this card.
    pending: RefCell<Option<(String, glib::SourceId)>>,
    hiding: RefCell<Option<glib::SourceId>>,
    /// The pointer is on it.
    inside: Cell<bool>,
    /// Its body follows new lines (scrolled to the end).
    stick: Rc<Cell<bool>>,
    /// The body's lines, for `debug.ui overview`.
    text: RefCell<Vec<String>>,
}

const SLIDES: [&str; 4] = ["pv-right", "pv-left", "pv-below", "pv-above"];

impl Preview {
    fn new() -> Preview {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_widget_name("overview-preview");
        root.add_css_class("overview-preview");
        root.set_halign(gtk::Align::Fill);
        root.set_valign(gtk::Align::Start);
        root.set_overflow(gtk::Overflow::Hidden);
        root.set_visible(false);
        let head = gtk::Box::new(gtk::Orientation::Vertical, 6);
        head.add_css_class("overview-preview-head");
        let lines = gtk::Box::new(gtk::Orientation::Vertical, 0);
        lines.set_widget_name("ov-preview-body");
        lines.add_css_class("overview-preview-body");
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_propagate_natural_height(true);
        scroll.set_child(Some(&lines));
        let foot = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        foot.add_css_class("overview-preview-foot");
        root.append(&head);
        root.append(&scroll);
        root.append(&foot);
        let stick = Rc::new(Cell::new(true));
        let adj = scroll.vadjustment();
        let s = stick.clone();
        adj.connect_changed(move |a| {
            if s.get() {
                a.set_value(a.upper() - a.page_size());
            }
        });
        let s = stick.clone();
        adj.connect_value_changed(move |a| s.set(a.value() + a.page_size() >= a.upper() - 2.0));
        // A click jumps like a click on its card; not on the scrollbar.
        let click = gtk::GestureClick::new();
        click.connect_released(|g, _, x, y| {
            let mut w = g
                .widget()
                .and_then(|w| w.pick(x, y, gtk::PickFlags::DEFAULT));
            while let Some(x) = w {
                if x.is::<gtk::Scrollbar>() {
                    return;
                }
                w = x.parent();
            }
            preview_jump();
        });
        root.add_controller(click);
        let motion = gtk::EventControllerMotion::new();
        motion.connect_enter(|_, _, _| {
            if let Some(v) = view() {
                v.preview.inside.set(true);
                cancel_hide(&v);
            }
        });
        motion.connect_leave(|_| {
            if let Some(v) = view() {
                v.preview.inside.set(false);
                if v.preview
                    .shown
                    .borrow()
                    .as_ref()
                    .is_some_and(|s| s.1 == By::Hover)
                {
                    schedule_hide(&v);
                }
            }
        });
        root.add_controller(motion);
        Preview {
            root,
            head,
            lines,
            scroll,
            foot,
            shown: RefCell::default(),
            place: Cell::default(),
            pending: RefCell::default(),
            hiding: RefCell::default(),
            inside: Cell::new(false),
            stick,
            text: RefCell::default(),
        }
    }
}

fn card_by_key(v: &View, key: &str) -> Option<Card> {
    v.overview
        .borrow()
        .sections
        .iter()
        .flat_map(|s| s.cards.iter())
        .find(|c| c.key() == key)
        .cloned()
}

fn pos_of_key(v: &View, key: &str) -> Option<Pos> {
    v.overview
        .borrow()
        .sections
        .iter()
        .enumerate()
        .find_map(|(i, s)| s.cards.iter().position(|c| c.key() == key).map(|j| (i, j)))
}

/// The pointer moves on a card: its Preview once the pointer rests there.
fn hover_motion(v: &Rc<View>, key: &str) {
    let shown = v.preview.shown.borrow().as_ref().map(|s| s.0.clone());
    if shown.as_deref() == Some(key) {
        cancel_hide(v);
        return;
    }
    want_preview(v, key, By::Hover, HOVER_MS);
}

/// The pointer leaves a card: no Preview for it unless the pointer goes
/// onto the Preview within 150 ms. A keyboard Preview stays.
fn hover_leave(v: &Rc<View>, key: &str) {
    let pending = v.preview.pending.borrow().as_ref().map(|p| p.0.clone());
    if pending.as_deref() == Some(key) {
        if let Some((_, id)) = v.preview.pending.borrow_mut().take() {
            id.remove();
        }
    }
    let hover = v
        .preview
        .shown
        .borrow()
        .as_ref()
        .is_some_and(|s| s.0 == key && s.1 == By::Hover);
    if hover {
        schedule_hide(v);
    }
}

/// The keyboard selection moved (or the Overview opened, or the filter
/// changed): its Preview once it rests for `delay` ms, unless `Alt+P`
/// turned them off. Another card's Preview goes at once.
fn keys_preview(v: &Rc<View>, delay: u64) {
    let off = try_app().is_none_or(|a| a.model.borrow().overview_no_preview);
    let key = v
        .sel
        .get()
        .and_then(|p| v.overview.borrow().card(p).map(Card::key));
    let shown = v.preview.shown.borrow().clone();
    match key {
        Some(k) if !off => {
            if shown.as_ref().is_some_and(|s| s.0 != k) {
                hide_preview(v);
            }
            want_preview(v, &k, By::Keys, delay);
        }
        _ => {
            if shown.is_some_and(|s| s.1 == By::Keys) {
                hide_preview(v);
            }
        }
    }
}

/// Show `key`'s Preview after `delay` ms (a newer request replaces it).
fn want_preview(v: &Rc<View>, key: &str, by: By, delay: u64) {
    let p = &v.preview;
    if let Some((_, id)) = p.pending.borrow_mut().take() {
        id.remove();
    }
    let same = p.shown.borrow().as_ref().is_some_and(|s| s.0 == key);
    if same {
        cancel_hide(v);
        if let Some(s) = p.shown.borrow_mut().as_mut() {
            s.1 = by;
        }
        return;
    }
    let k = key.to_string();
    let id = glib::timeout_add_local_once(Duration::from_millis(delay), move || {
        if let Some(v) = view() {
            v.preview.pending.borrow_mut().take();
            show_preview(&v, &k, by);
        }
    });
    *p.pending.borrow_mut() = Some((key.to_string(), id));
}

fn cancel_hide(v: &View) {
    if let Some(id) = v.preview.hiding.borrow_mut().take() {
        id.remove();
    }
}

fn schedule_hide(v: &Rc<View>) {
    cancel_hide(v);
    let id = glib::timeout_add_local_once(Duration::from_millis(HIDE_MS), || {
        if let Some(v) = view() {
            v.preview.hiding.borrow_mut().take();
            if !v.preview.inside.get() {
                hide_preview(&v);
            }
        }
    });
    *v.preview.hiding.borrow_mut() = Some(id);
}

fn hide_preview(v: &View) {
    let p = &v.preview;
    if let Some((_, id)) = p.pending.borrow_mut().take() {
        id.remove();
    }
    cancel_hide(v);
    p.shown.borrow_mut().take();
    p.place.set(None);
    p.inside.set(false);
    p.root.set_visible(false);
    for c in SLIDES {
        p.root.remove_css_class(c);
    }
}

fn show_preview(v: &Rc<View>, key: &str, by: By) {
    // `Alt+P` while it waited.
    if by == By::Keys && try_app().is_none_or(|a| a.model.borrow().overview_no_preview) {
        return;
    }
    let Some(card) = card_by_key(v, key) else {
        hide_preview(v);
        return;
    };
    let first = !v.preview.root.is_visible();
    *v.preview.shown.borrow_mut() = Some((key.to_string(), by));
    v.preview.stick.set(true);
    fill_preview(v, &card);
    place_later(v, first);
}

/// Once a second while shown: that one Pane re-read, the header updated.
fn refresh_preview(v: &Rc<View>) {
    let key = v.preview.shown.borrow().as_ref().map(|s| s.0.clone());
    let Some(key) = key else { return };
    match card_by_key(v, &key) {
        Some(card) => {
            fill_preview(v, &card);
            place_later(v, false);
        }
        None => hide_preview(v),
    }
}

/// The screen lines of a Pane, as a Preview shows them.
fn screen_of(pane: &str, n: usize) -> Vec<String> {
    let Some(app) = try_app() else {
        return Vec::new();
    };
    let terms = app.terminals.borrow();
    terms
        .get(pane)
        .and_then(|t| t.handle.read_text(GHOSTTY_POINT_ACTIVE))
        .map(|text| ov::preview_of(&text, n))
        .unwrap_or_default()
}

fn clear(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}

fn preview_line(text: &str, class: &str) -> gtk::Label {
    let l = label(text, &[class]);
    l.set_single_line_mode(true);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    l
}

/// Header (glyph, name, model, status · age; Workspace · place, `↳ Parent`),
/// up to 24 screen lines (a Fan-out: each member's last 6), foot (the last
/// Notification, git facts, the account, keys).
fn fill_preview(v: &View, c: &Card) {
    let p = &v.preview;
    let f0 = c.facts()[0];
    let status = card_status(c);
    clear(&p.head);
    let r1 = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    r1.append(&glyph(c));
    let (name, model) = match c {
        Card::Agent(f) => (f.name.clone(), f.model.clone()),
        Card::Fanout { id, members } => (
            format!("Fan-out {id}"),
            Some(format!("{} Agents", members.len())),
        ),
    };
    let n = label(&name, &["overview-card-name"]);
    n.set_ellipsize(gtk::pango::EllipsizeMode::End);
    r1.append(&n);
    if let Some(m) = model {
        r1.append(&label(&m, &["overview-card-model"]));
    }
    let st = label(
        &c.status_text(now()),
        &[
            "overview-card-status",
            &format!("st-{}", status_class(status)),
        ],
    );
    st.set_hexpand(true);
    st.set_xalign(1.0);
    r1.append(&st);
    p.head.append(&r1);
    let r2 = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    r2.add_css_class("overview-card-where");
    r2.append(&ws_dot(&f0.workspace));
    r2.append(&label(&f0.workspace_name, &["overview-card-ws"]));
    let pl = label(&format!("· {}", f0.place), &["overview-card-place"]);
    pl.set_ellipsize(gtk::pango::EllipsizeMode::End);
    pl.set_hexpand(true);
    r2.append(&pl);
    if let Some(parent) = &f0.parent {
        r2.append(&label(&format!("↳ {parent}"), &["overview-card-place"]));
    }
    p.head.append(&r2);

    clear(&p.lines);
    let mut text = Vec::new();
    match c {
        Card::Agent(f) => text = screen_of(&f.pane, ov::PREVIEW_LINES),
        Card::Fanout { members, .. } => {
            let now = now();
            for m in members {
                let st = Card::Agent(Box::new(m.clone())).status_text(now);
                p.lines.append(&preview_line(
                    &format!("{} · {st}", m.name),
                    "overview-preview-member",
                ));
                for l in screen_of(&m.pane, ov::PREVIEW_MEMBER_LINES) {
                    p.lines.append(&preview_line(&l, "overview-preview-line"));
                    text.push(l);
                }
            }
        }
    }
    if let Card::Agent(_) = c {
        for l in &text {
            p.lines.append(&preview_line(l, "overview-preview-line"));
        }
        if text.is_empty() {
            p.lines.append(&preview_line(
                "Nothing on screen yet",
                "overview-preview-empty",
            ));
        }
    }
    *p.text.borrow_mut() = text;

    clear(&p.foot);
    if let Some(n) = &f0.note {
        let l = label(n, &["overview-card-note"]);
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        p.foot.append(&l);
    }
    for t in [&f0.git, &f0.account].into_iter().flatten() {
        p.foot.append(&label(t, &[]));
    }
    let off = try_app().is_some_and(|a| a.model.borrow().overview_no_preview);
    let k = label(
        if off {
            "↵ jump · Alt+P show"
        } else {
            "↵ jump · Alt+P hide"
        },
        &["overview-card-jump"],
    );
    k.set_hexpand(true);
    k.set_xalign(1.0);
    p.foot.append(&k);
}

/// Place the Preview once the stage is laid out (next frame): its card
/// may have just been built, scrolled or resized.
fn place_later(v: &View, animate: bool) {
    if v.preview.shown.borrow().is_none() {
        return;
    }
    v.stage.add_tick_callback(move |_, _| {
        if let Some(v) = view() {
            place_preview(&v, animate);
        }
        glib::ControlFlow::Break
    });
}

fn rect_of(b: &gtk::graphene::Rect) -> ov::Rect {
    ov::Rect {
        x: b.x().round() as i32,
        y: b.y().round() as i32,
        w: b.width().round() as i32,
        h: b.height().round() as i32,
    }
}

/// Beside its card (`ov::preview_place`), as tall as its content within
/// the body; a card scrolled out of the body takes its Preview with it.
fn place_preview(v: &Rc<View>, animate: bool) {
    let p = &v.preview;
    let Some(key) = p.shown.borrow().as_ref().map(|s| s.0.clone()) else {
        return;
    };
    let widget = pos_of_key(v, &key).and_then(|pos| v.cards.borrow().get(&pos).cloned());
    let bounds = widget
        .and_then(|w| w.compute_bounds(&v.stage))
        .zip(v.scroller.compute_bounds(&v.stage));
    let Some((cb, bb)) = bounds else {
        hide_preview(v);
        return;
    };
    let (card, mut body) = (rect_of(&cb), rect_of(&bb));
    // What shows of the body: at large text it is laid out taller than
    // the stage, under the key hints.
    body.h = body.h.min(v.stage.height() - body.y);
    if card.y + card.h <= body.y || card.y >= body.y + body.h {
        hide_preview(v);
        return;
    }
    let stage_w = v.stage.width();
    let width = ov::preview_width(stage_w);
    // Measured visible (a hidden widget measures 0) and without the old
    // margins; nothing is drawn before this frame ends.
    p.root.set_visible(true);
    p.root.set_margin_start(0);
    p.root.set_margin_end(0);
    p.root.set_margin_top(0);
    p.scroll.set_max_content_height(-1);
    let (_, nat, _, _) = p.root.measure(gtk::Orientation::Vertical, width);
    let (_, inner, _, _) = p.scroll.measure(gtk::Orientation::Vertical, -1);
    let place = ov::preview_place(card, body, (width, nat));
    p.scroll
        .set_max_content_height((place.rect.h - (nat - inner)).max(1));
    p.root.set_margin_start(place.rect.x);
    p.root
        .set_margin_end((stage_w - place.rect.x - place.rect.w).max(0));
    p.root.set_margin_top(place.rect.y);
    p.place.set(Some((place, card, body)));
    if animate {
        for c in SLIDES {
            p.root.remove_css_class(c);
        }
        p.root.add_css_class(&format!("pv-{}", place.side.as_str()));
    }
}

/// A click on the Preview: jump to its Agent.
fn preview_jump() {
    let Some(v) = view() else { return };
    let key = v.preview.shown.borrow().as_ref().map(|s| s.0.clone());
    if let Some(pos) = key.and_then(|k| pos_of_key(&v, &k)) {
        v.sel.set(Some(pos));
        jump(&v, false);
    }
}

/// `Alt+P`: Previews for the keyboard selection off / on, kept in
/// `state.json`; hover keeps showing them.
fn toggle_previews(v: &Rc<View>) {
    let Some(app) = try_app() else { return };
    let off = {
        let mut m = app.model.borrow_mut();
        m.overview_no_preview = !m.overview_no_preview;
        m.overview_no_preview
    };
    app.schedule_save();
    if off {
        let keys = v
            .preview
            .shown
            .borrow()
            .as_ref()
            .is_some_and(|s| s.1 == By::Keys);
        if keys {
            hide_preview(v);
        } else {
            refresh_preview(v);
        }
    } else {
        keys_preview(v, 0);
        refresh_preview(v);
    }
}

/// `debug.ui overview` → `preview`: what it shows and where (the stage's
/// coordinates = the window's).
fn preview_state(v: &View) -> Value {
    let p = &v.preview;
    let keys = try_app().is_some_and(|a| !a.model.borrow().overview_no_preview);
    let pending = p.pending.borrow().as_ref().map(|x| x.0.clone());
    let shown = p.shown.borrow().clone();
    let Some((key, by)) = shown.filter(|_| p.root.is_visible()) else {
        return json!({"shown": false, "keys": keys, "pending": pending});
    };
    let r = |r: ov::Rect| json!({"x": r.x, "y": r.y, "w": r.w, "h": r.h});
    let at = p.root.compute_bounds(&v.stage).map(|b| rect_of(&b));
    let (place, card, body) = match p.place.get() {
        Some((p, c, b)) => (Some(p), Some(c), Some(b)),
        None => (None, None, None),
    };
    let scroll = p.scroll.vadjustment();
    json!({
        "shown": true,
        "keys": keys,
        "pending": pending,
        "key": key,
        "pane": card_by_key(v, &key).map(|c| c.pane().to_string()),
        "by": if by == By::Hover { "hover" } else { "keys" },
        "side": place.map(|p| p.side.as_str()),
        "rect": at.map_or(Value::Null, r),
        "card": card.map_or(Value::Null, r),
        "body": body.map_or(Value::Null, r),
        "lines": p.text.borrow().len(),
        "text": *p.text.borrow(),
        "at_end": scroll.value() + scroll.page_size() >= scroll.upper() - 2.0,
    })
}
