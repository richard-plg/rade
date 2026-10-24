//! Code popovers on the Viewer or the Review Pane, one at a time (PRD R90,
//! R84.2 / R87.3 / R91 1.0.0 amendments): the hover (type and docs of the
//! name at a place: `textDocument/hover`, else its declaring line as a
//! text match), the peek (the lines of its definition with an Open
//! button), and the results of a navigation (references, callers tree,
//! implementations, outline, the server's state: indexing, install hint,
//! Restart — built by [`crate::code_results`]). Esc, a click in the Pane
//! or another navigation closes it; a hover opened by the pointer also
//! closes when it moves away. Asked off the GTK thread; a late answer for
//! an older ask is dropped.

use crate::app::try_app;
use gtk::prelude::*;
use gtk::{gdk, glib};
use gtk4 as gtk;
use rade_core::code::nav::{self, Answer, Query};
use rade_core::rpc::{Error, Result};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

/// Lines of the definition a peek shows (before, after).
const PEEK: (u32, u32) = (2, 10);

struct Pop {
    popover: gtk::Popover,
    kind: &'static str,
    title: String,
    text: String,
    note: Option<String>,
    /// Opened by the pointer resting (closes when it moves away).
    by_pointer: Option<(f64, f64)>,
}

impl Pop {
    fn results(&self) -> bool {
        self.kind == "results"
    }
}

thread_local! {
    static POP: RefCell<Option<Pop>> = const { RefCell::new(None) };
    static GEN: Cell<u64> = const { Cell::new(0) };
}

/// Where a popover points: a widget and a rectangle in its coordinates.
pub struct Anchor {
    pub widget: gtk::Widget,
    pub rect: gdk::Rectangle,
    /// Opened by the pointer at this point (widget coordinates).
    pub pointer: Option<(f64, f64)>,
}

/// Close the open popover; true when one was open (Esc is then used).
pub fn close() -> bool {
    GEN.with(|g| g.set(g.get() + 1));
    let Some(p) = POP.with(|p| p.borrow_mut().take()) else {
        return false;
    };
    p.popover.popdown();
    // The results widget lives on and goes into the next results popover.
    if p.results() {
        p.popover.set_child(None::<&gtk::Widget>);
    }
    let w = p.popover.clone();
    glib::idle_add_local_once(move || w.unparent());
    true
}

/// Close a hover or peek if it is on `widget` (its file changed, it
/// scrolled); results stay while their rows are opened one by one.
pub fn close_on(widget: &gtk::Widget) {
    let mine = POP.with(|p| {
        p.borrow()
            .as_ref()
            .is_some_and(|p| !p.results() && p.popover.parent().as_ref() == Some(widget))
    });
    if mine {
        close();
    }
}

/// The pointer moved to (x, y) of `widget`: a popover it opened closes
/// once the pointer is 24 px away.
pub fn pointer_moved(widget: &gtk::Widget, x: f64, y: f64) {
    let far = POP.with(|p| {
        p.borrow().as_ref().is_some_and(|p| {
            p.popover.parent().as_ref() == Some(widget)
                && p.by_pointer
                    .is_some_and(|(px, py)| (px - x).hypot(py - y) > 24.0)
        })
    });
    if far {
        close();
    }
}

/// Ask about the name at `path:line:col` from `pane`: a hover, or with
/// `peek` its definition; the answer opens a popover at `at`.
pub fn ask(pane: &str, peek: bool, at: Anchor, path: &Path, line: u32, col: u32) {
    let Some(app) = try_app() else { return };
    let q = if peek { Query::Def } else { Query::Hover };
    let (ctx, user) = match crate::code::context(&app, q, path, line, col) {
        Ok(c) => c,
        Err(e) => return show_error(at, &e.message),
    };
    run(pane, peek, at, move || crate::code::run(&ctx, &user));
}

/// The same for a name on a removed diff line (R87.2): the current tree's
/// declaration, else text at the base.
pub fn ask_removed(pane: &str, peek: bool, at: Anchor, file: &Path, word: &str, base: &str) {
    let Some(app) = try_app() else { return };
    let q = if peek { Query::Def } else { Query::Hover };
    let (ctx, user) = match crate::code::context(&app, q, file, 1, 1) {
        Ok(c) => c,
        Err(e) => return show_error(at, &e.message),
    };
    let (word, base) = (word.to_string(), base.to_string());
    run(pane, peek, at, move || {
        let pool = crate::code::pool();
        pool.lock().unwrap().configure(&user);
        rade_core::code::run_removed(pool, &ctx, &word, &base)
    });
}

fn run(pane: &str, peek: bool, at: Anchor, f: impl FnOnce() -> Answer + Send + 'static) {
    close();
    let gen = GEN.with(|g| g.get());
    let pane = pane.to_string();
    crate::worktrees::in_background(f, move |a| {
        if GEN.with(|g| g.get()) != gen || !at.widget.is_mapped() {
            return;
        }
        if peek {
            show_peek(&pane, at, &a);
        } else {
            show_hover(at, &a);
        }
    });
}

fn esc(s: &str) -> String {
    glib::markup_escape_text(s).to_string()
}

/// The popover's first line: the server's name, or `text matches` and
/// the place of the declaring line; the hint (why text) goes under the body.
fn source_line(a: &Answer, place: bool) -> String {
    let mut s = a.summary_source();
    if let (true, true, Some(i)) = (a.text(), place, a.items.first()) {
        s = format!("{s} · {}:{}", nav::rel(&i.path, &a.root), i.line);
    }
    s
}

fn note(a: &Answer) -> Option<String> {
    a.hint.clone().filter(|_| a.text())
}

/// A thematic break between signature and docs reads as an underline.
fn no_rules(d: &str) -> String {
    d.lines()
        .map(|l| {
            let t = l.trim();
            if t.len() >= 3
                && ["-", "*", "_"]
                    .iter()
                    .any(|c| t.chars().all(|x| x.to_string() == *c))
            {
                ""
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn show_hover(at: Anchor, a: &Answer) {
    // The pointer resting on a keyword or an unknown name: nothing.
    if at.pointer.is_some() && a.doc.is_none() {
        return;
    }
    let (body, text) = match &a.doc {
        Some(d) => (rade_core::markdown::to_pango(&no_rules(d)), d.clone()),
        None => {
            let t = format!("No information about {}", a.word);
            (esc(&t), t)
        }
    };
    open(
        at,
        "hover",
        &source_line(a, true),
        &body,
        &text,
        note(a),
        None,
    );
}

fn show_peek(pane: &str, at: Anchor, a: &Answer) {
    let Some(it) = a.items.first() else {
        let t = format!("No definition found for {}", a.word);
        return open(
            at,
            "peek",
            &source_line(a, false),
            &esc(&t),
            &t,
            note(a),
            None,
        );
    };
    let text = std::fs::read(&it.path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let first = it.line.saturating_sub(PEEK.0).max(1);
    let last = first + PEEK.0 + PEEK.1;
    let digits = last.to_string().len();
    let (mut markup, mut plain) = (Vec::new(), Vec::new());
    for (n, l) in text
        .lines()
        .enumerate()
        .skip(first as usize - 1)
        .take((PEEK.0 + PEEK.1 + 1) as usize)
    {
        let n = n as u32 + 1;
        let num = format!("{n:>digits$}");
        // The definition's line on a tinted band, like the Viewer's current line.
        markup.push(if n == it.line {
            format!(
                "<span alpha=\"55%\">{}</span>  <span weight=\"bold\" background=\"{}\" bgalpha=\"18%\">{} </span>",
                esc(&num),
                crate::tokens::rgb_now("run"),
                esc(l)
            )
        } else {
            format!("<span alpha=\"45%\">{}</span>  {}", esc(&num), esc(l))
        });
        plain.push(format!("{num}  {l}"));
    }
    let mut title = format!("{}:{}", nav::rel(&it.path, &a.root), it.line);
    if a.items.len() > 1 {
        title += &format!(" · 1 of {} (F12 lists them)", a.items.len());
    }
    if a.text() {
        title += &format!(" · {}", a.summary_source());
    }
    let target = (pane.to_string(), it.path.clone(), it.line, it.col);
    open(
        at,
        "peek",
        &title,
        &format!("<tt>{}</tt>", markup.join("\n")),
        &plain.join("\n"),
        note(a),
        Some(target),
    );
}

fn show_error(at: Anchor, msg: &str) {
    open(at, "hover", "", &esc(msg), msg, None, None);
}

fn open(
    at: Anchor,
    kind: &'static str,
    title: &str,
    body: &str,
    text: &str,
    note: Option<String>,
    target: Option<(String, PathBuf, u32, u32)>,
) {
    close();
    let pop = gtk::Popover::new();
    pop.set_autohide(false);
    pop.set_position(gtk::PositionType::Top);
    pop.add_css_class("rade-code-pop");
    let col = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let t = gtk::Label::new(Some(title));
    t.set_xalign(0.0);
    t.set_hexpand(true);
    t.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    // The title never decides the popover's width.
    t.set_width_chars(1);
    t.add_css_class("rade-meta");
    head.append(&t);
    if let Some((pane, path, line, c)) = target {
        let b = gtk::Button::with_label("Open");
        b.add_css_class("flat");
        b.add_css_class("rade-code-pop-open");
        b.set_tooltip_text(Some("Go to the definition (F12)"));
        b.connect_clicked(move |_| {
            close();
            crate::code::jump_to(&pane, &path, line, c);
        });
        head.append(&b);
    }
    if !title.is_empty() || head.first_child() != head.last_child() {
        col.append(&head);
    }
    let l = gtk::Label::new(None);
    l.set_markup(body);
    l.set_xalign(0.0);
    l.set_yalign(0.0);
    let hover = kind == "hover";
    l.set_wrap(hover);
    l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    // Measured at the width it gets: no blank band under one line.
    let longest = text.lines().map(|x| x.chars().count()).max().unwrap_or(1);
    if hover {
        l.set_width_chars(longest.clamp(12, 80) as i32);
    }
    l.set_max_width_chars(80);
    l.set_selectable(true);
    l.add_css_class("rade-code-pop-body");
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_child(Some(&l));
    scroll.set_propagate_natural_height(true);
    scroll.set_propagate_natural_width(true);
    scroll.set_max_content_height(320);
    scroll.set_max_content_width(640);
    scroll.set_policy(
        if hover {
            gtk::PolicyType::Never
        } else {
            gtk::PolicyType::Automatic
        },
        // A short body sizes itself (height for its real width); a long
        // one scrolls.
        if text.lines().count() <= 16 {
            gtk::PolicyType::Never
        } else {
            gtk::PolicyType::Automatic
        },
    );
    col.append(&scroll);
    if let Some(n) = &note {
        let h = gtk::Label::new(Some(n));
        h.set_xalign(0.0);
        h.set_wrap(true);
        h.set_max_width_chars(60);
        h.set_width_chars(1);
        h.set_selectable(true);
        h.add_css_class("rade-code-hint");
        col.append(&h);
    }
    pop.set_child(Some(&col));
    pop.set_parent(&at.widget);
    pop.set_pointing_to(Some(&at.rect));
    pop.popup();
    POP.with(|p| {
        *p.borrow_mut() = Some(Pop {
            popover: pop,
            kind,
            title: title.to_string(),
            text: text.to_string(),
            note,
            by_pointer: at.pointer,
        })
    });
}

/// Show the results widget `child` (titled `title`) in the popover at
/// `at`; already open there: it stays (an answer refreshes it in place).
pub fn open_results(at: Anchor, child: &gtk::Widget, title: &str) {
    let same = POP.with(|p| {
        p.borrow_mut().as_mut().is_some_and(|p| {
            let same = p.results() && p.popover.parent().as_ref() == Some(&at.widget);
            if same {
                p.title = title.to_string();
                p.popover.set_pointing_to(Some(&at.rect));
            }
            same
        })
    });
    if same {
        return;
    }
    close();
    if let Some(old) = child.parent() {
        if let Ok(pop) = old.downcast::<gtk::Popover>() {
            pop.set_child(None::<&gtk::Widget>);
        }
    }
    let pop = gtk::Popover::new();
    pop.set_autohide(false);
    pop.set_position(gtk::PositionType::Bottom);
    pop.add_css_class("rade-code-pop");
    pop.add_css_class("rade-code-results");
    pop.set_child(Some(child));
    pop.set_parent(&at.widget);
    pop.set_pointing_to(Some(&at.rect));
    pop.popup();
    POP.with(|p| {
        *p.borrow_mut() = Some(Pop {
            popover: pop,
            kind: "results",
            title: title.to_string(),
            text: String::new(),
            note: None,
            by_pointer: None,
        })
    });
}

/// True while the results popover is open.
pub fn results_open() -> bool {
    POP.with(|p| {
        p.borrow()
            .as_ref()
            .is_some_and(|p| p.results() && p.popover.is_visible())
    })
}

/// `debug.code {"op": "popover"}`: what the open popover shows.
pub fn info() -> Value {
    POP.with(|p| match p.borrow().as_ref() {
        Some(p) => json!({"open": p.popover.is_visible(), "kind": p.kind, "title": p.title,
                          "text": p.text, "note": p.note, "by_pointer": p.by_pointer.is_some()}),
        None => json!({"open": false}),
    })
}

/// `debug.code {"op": "popover_open"}`: its Open button.
pub fn click_open() -> Result<Value> {
    let b = POP.with(|p| {
        let p = p.borrow();
        let col = p.as_ref()?.popover.child()?;
        let head = col.first_child()?;
        head.last_child()?.downcast::<gtk::Button>().ok()
    });
    b.ok_or_else(|| Error::not_found("no Open button"))?
        .emit_clicked();
    Ok(json!({}))
}

/// For `debug screenshot`: paint the open popover over the window's
/// snapshot where it shows (above the place it points to, or below for
/// results, centred).
pub fn paint_over(window: &gtk::Widget, snapshot: &gtk::Snapshot) {
    POP.with(|p| {
        let p = p.borrow();
        let Some(pop) = p.as_ref().map(|p| &p.popover).filter(|p| p.is_visible()) else {
            return;
        };
        let (Some(parent), (true, r)) = (pop.parent(), pop.pointing_to()) else {
            return;
        };
        let Some(at) = parent.compute_point(
            window,
            &gtk::graphene::Point::new(r.x() as f32, r.y() as f32),
        ) else {
            return;
        };
        let (w, h) = (pop.width() as f32, pop.height() as f32);
        let x = (at.x() + r.width() as f32 / 2.0 - w / 2.0)
            .clamp(0.0, (window.width() as f32 - w).max(0.0));
        let y = if pop.position() == gtk::PositionType::Bottom {
            (at.y() + r.height() as f32).min((window.height() as f32 - h).max(0.0))
        } else {
            (at.y() - h).max(0.0)
        };
        snapshot.save();
        snapshot.translate(&gtk::graphene::Point::new(x, y));
        gtk::WidgetPaintable::new(Some(pop)).snapshot(snapshot, w as f64, h as f64);
        snapshot.restore();
    });
}
