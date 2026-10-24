//! Review Pane rows (M11, R118): the GtkListView factory — one recycled row
//! widget per visible row (file header with its Viewed box, hunk header,
//! diff line, binary note, comment card, the composer) — and per-file
//! syntax highlighting into Pango markup, made by a hidden GtkSourceBuffer
//! the first time a file's rows are shown.

use super::{status_class, Markup, ReviewView, State};
use gtk::glib;
use gtk4 as gtk;
use rade_core::diff::{FileDiff, LineKind, Row};
use sourceview5::prelude::*;
use std::rc::{Rc, Weak};
use std::time::Instant;

/// Files with more diff lines than this are shown without highlighting.
/// ponytail: highlighting runs on the UI thread on first show; move it off
/// thread if someone reviews 20k-line files often.
const HIGHLIGHT_MAX: usize = 20_000;

const ROW_CLASSES: [&str; 5] = [
    "rade-diff-file",
    "rade-diff-hunk",
    "rade-diff-added",
    "rade-diff-removed",
    "rade-diff-binary",
];

/// `+a −r` (or "binary") for a file, as markup.
pub(super) fn counts(f: &FileDiff) -> String {
    if f.binary {
        "<small>binary</small>".into()
    } else {
        format!(
            "<small><span foreground=\"{}\">+{}</span> <span foreground=\"{}\">−{}</span></small>",
            crate::tokens::rgb_now("add"),
            f.added,
            crate::tokens::rgb_now("del"),
            f.removed
        )
    }
}

struct Cells {
    old: gtk::Label,
    new: gtk::Label,
    sign: gtk::Label,
    code: gtk::Label,
    /// A file header's Viewed box (R118.3).
    viewed: gtk::CheckButton,
    /// A card's or the composer's slot (R118.4).
    extra: gtk::Box,
}

fn cells(row: &gtk::Box) -> Cells {
    let l = |w: Option<gtk::Widget>| w.and_downcast::<gtk::Label>().unwrap();
    let old = l(row.first_child());
    let new = l(old.next_sibling());
    let sign = l(new.next_sibling());
    let code = l(sign.next_sibling());
    let viewed = code
        .next_sibling()
        .and_downcast::<gtk::CheckButton>()
        .unwrap();
    let extra = viewed.next_sibling().and_downcast::<gtk::Box>().unwrap();
    Cells {
        old,
        new,
        sign,
        code,
        viewed,
        extra,
    }
}

pub(super) fn factory(v: &Rc<ReviewView>) -> gtk::SignalListItemFactory {
    let f = gtk::SignalListItemFactory::new();
    let weak = Rc::downgrade(v);
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
        let sign = gtk::Label::new(None);
        sign.set_width_chars(2);
        sign.set_yalign(0.0);
        sign.add_css_class("rade-diff-sign");
        let code = gtk::Label::new(None);
        code.set_xalign(0.0);
        code.set_hexpand(true);
        code.set_wrap(true);
        code.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        code.add_css_class("rade-diff-code");
        let viewed = gtk::CheckButton::with_label("Viewed");
        viewed.add_css_class("review-viewed-check");
        viewed.set_valign(gtk::Align::Center);
        viewed.set_visible(false);
        let extra = gtk::Box::new(gtk::Orientation::Vertical, 0);
        extra.add_css_class("review-card-slot");
        extra.set_hexpand(true);
        extra.set_visible(false);
        row.append(&num());
        row.append(&num());
        row.append(&sign);
        row.append(&code);
        row.append(&viewed);
        row.append(&extra);
        // The Viewed box of a file header (R118.3); deferred: marking it
        // rebuilds the rows this one belongs to.
        let (weak2, item_ref) = (weak.clone(), item.downgrade());
        viewed.connect_toggled(move |b| {
            let (Some(v), Some(item)) = (weak2.upgrade(), item_ref.upgrade()) else {
                return;
            };
            let on = b.is_active();
            let pos = item.position() as usize;
            let path = {
                let st = v.state.borrow();
                match st.rows.get(pos) {
                    Some(Row::File(fi)) => v.file_path_in(&st, *fi),
                    _ => None,
                }
            };
            let Some(path) = path else { return };
            let i = v
                .state
                .borrow()
                .changes
                .as_ref()
                .and_then(|c| c.files.iter().position(|f| f.path == path));
            let now = i.is_some_and(|i| v.viewed_flags().get(i).copied().unwrap_or(false));
            if now != on {
                glib::idle_add_local_once(move || v.set_viewed(&path, on));
            }
        });
        // A click on a file or hunk header folds it; on a line, it puts the
        // point there (Ctrl: its definition, R87.1).
        let click = gtk::GestureClick::new();
        let (weak, item_ref) = (weak.clone(), item.downgrade());
        click.connect_released(move |g, n, x, y| {
            let (Some(v), Some(item)) = (weak.upgrade(), item_ref.upgrade()) else {
                return;
            };
            if n == 1 && g.current_button() == 1 {
                let pos = item.position() as usize;
                if let Some(row) = g.widget() {
                    v.click_line(&row, pos, x, y, false);
                }
                v.toggle_row(pos);
            }
        });
        row.add_controller(click);
        item.set_child(Some(&row));
    });
    let weak: Weak<ReviewView> = Rc::downgrade(v);
    f.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let (Some(v), Some(row)) = (weak.upgrade(), item.child().and_downcast::<gtk::Box>()) else {
            return;
        };
        let pos = item.position();
        row.set_widget_name(&pos.to_string());
        // A card or the composer is no list row to select: a click in it
        // must not move the keyboard to the row (found live: typing into
        // the composer after clicking it went nowhere).
        let card = matches!(
            v.state.borrow().rows.get(pos as usize),
            Some(Row::Comment(_) | Row::Composer)
        );
        item.set_selectable(!card);
        item.set_activatable(!card);
        item.set_focusable(!card);
        v.bind(&row, pos as usize);
    });
    f
}

/// Row widgets on screen whose gutter shows a Review Comment mark.
pub(super) fn marks_shown(list: &gtk::ListView) -> usize {
    let mut n = 0;
    let mut child = list.first_child();
    while let Some(c) = child {
        let sign = c
            .first_child()
            .and_then(|r| r.first_child()?.next_sibling()?.next_sibling());
        if c.is_child_visible() && sign.is_some_and(|s| s.has_css_class("rade-diff-marked")) {
            n += 1;
        }
        child = c.next_sibling();
    }
    n
}

/// The row widget bound to `pos`, if it is laid out (it is named so).
pub(super) fn row_widget(list: &gtk::ListView, pos: u32) -> Option<gtk::Widget> {
    let name = pos.to_string();
    let mut child = list.first_child();
    while let Some(c) = child {
        let shown = c.is_child_visible() && c.is_mapped();
        if let Some(row) = c.first_child().filter(|r| shown && r.widget_name() == name) {
            return Some(row);
        }
        child = c.next_sibling();
    }
    None
}

impl ReviewView {
    fn bind(&self, row: &gtk::Box, pos: usize) {
        let st = self.state.borrow();
        let (Some(c), Some(r)) = (st.changes.as_ref(), st.rows.get(pos).copied()) else {
            return;
        };
        let cells = cells(row);
        let mut classes = vec!["rade-diff-row", "monospace"];
        for l in [&cells.old, &cells.new] {
            l.set_label("");
        }
        cells.code.set_visible(true);
        cells.viewed.set_visible(false);
        cells.extra.set_visible(false);
        while let Some(w) = cells.extra.first_child() {
            cells.extra.remove(&w);
        }
        cells.sign.set_css_classes(&["rade-diff-sign"]);
        cells.sign.set_label("");
        cells
            .sign
            .set_yalign(if matches!(r, Row::File(_)) { 0.5 } else { 0.0 });
        match r {
            Row::File(fi) => {
                let f = &c.files[fi];
                classes.push(ROW_CLASSES[0]);
                let marks = self.review_state().unwrap_or_default();
                let viewed = st
                    .hashes
                    .get(&f.path)
                    .is_some_and(|h| marks.is_viewed(&f.path, *h));
                cells.viewed.set_active(viewed);
                cells.viewed.set_visible(true);
                let folded = st.folded_files.contains(&f.path);
                cells.sign.set_label(&f.status.to_string());
                cells.sign.add_css_class("rade-git-mark");
                cells.sign.add_css_class(status_class(f.status));
                let name = match &f.old_path {
                    Some(o) => format!("{o} → {}", f.path),
                    None => f.path.clone(),
                };
                cells.code.set_markup(&format!(
                    "{} <b>{}</b>  {}",
                    if folded { "▸" } else { "▾" },
                    glib::markup_escape_text(&name),
                    counts(f)
                ));
            }
            Row::Hunk(fi, hi) => {
                classes.push(ROW_CLASSES[1]);
                let f = &c.files[fi];
                let folded = st
                    .folded_hunks
                    .contains(&(f.path.clone(), f.hunks[hi].header.clone()));
                cells.sign.set_label(if folded { "▸" } else { "▾" });
                cells.code.set_label(&f.hunks[hi].header);
            }
            Row::Binary(_) => {
                classes.push(ROW_CLASSES[4]);
                cells.code.set_markup("<i>Binary file not shown</i>");
            }
            Row::Comment(id) => {
                classes.push("review-card-row");
                cells.code.set_visible(false);
                cells.extra.set_visible(true);
                if let Some(cm) = st.all_comments.iter().find(|x| x.id == id) {
                    cells.extra.append(&self.card(cm));
                }
            }
            Row::Composer => {
                classes.push("review-card-row");
                cells.code.set_visible(false);
                cells.extra.set_visible(true);
                let w = &self.composer.root;
                if let Some(p) = w.parent().and_downcast::<gtk::Box>() {
                    p.remove(w);
                }
                cells.extra.append(w);
            }
            Row::Line(fi, hi, li) => {
                let f = &c.files[fi];
                let l = &f.hunks[hi].lines[li];
                let num = |x: Option<u32>| x.map_or(String::new(), |x| x.to_string());
                cells.old.set_label(&num(l.old));
                cells.new.set_label(&num(l.new));
                match l.kind {
                    LineKind::Added => classes.push(ROW_CLASSES[2]),
                    LineKind::Removed => classes.push(ROW_CLASSES[3]),
                    LineKind::Context => {}
                }
                let marked = marked(&st, &f.path, l);
                if marked {
                    cells.sign.add_css_class("rade-diff-marked");
                }
                cells.sign.set_label(sign(l.kind, marked));
                let m = self.markup_of(&st, fi);
                let text = m
                    .hunk_starts
                    .get(hi)
                    .and_then(|s| m.lines.get(s + li))
                    .map_or("", String::as_str);
                cells.code.set_markup(text);
            }
        }
        // A recycled row never keeps another line's link.
        cells.code.set_attributes(None);
        row.set_css_classes(&classes);
    }

    /// A file's highlighted lines, made once per file and diff.
    fn markup_of(&self, st: &State, fi: usize) -> Rc<Markup> {
        let f = &st.changes.as_ref().unwrap().files[fi];
        if let Some(m) = self.markup.borrow().get(&f.path) {
            return m.clone();
        }
        let t0 = Instant::now();
        let mut hunk_starts = Vec::new();
        let mut texts: Vec<std::borrow::Cow<str>> = Vec::new();
        for h in &f.hunks {
            hunk_starts.push(texts.len());
            texts.extend(h.lines.iter().map(|l| shorten(&l.text)));
        }
        let lang = sourceview5::LanguageManager::default().guess_language(Some(&f.path), None);
        let lines = match lang {
            Some(lang) if texts.len() <= HIGHLIGHT_MAX => {
                let b = &self.highlighter;
                b.set_language(Some(&lang));
                b.set_text(&texts.join("\n"));
                b.ensure_highlight(&b.start_iter(), &b.end_iter());
                let out = (0..texts.len() as i32).map(|i| line_markup(b, i)).collect();
                b.set_text("");
                out
            }
            _ => texts
                .iter()
                .map(|t| glib::markup_escape_text(t).to_string())
                .collect(),
        };
        let m = Rc::new(Markup { lines, hunk_starts });
        self.markup.borrow_mut().insert(f.path.clone(), m.clone());
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        self.highlight_ms.set(self.highlight_ms.get().max(ms));
        m
    }
}

/// A line shown at most this long (a minified file's one line would make
/// a screen-tall row and a slow layout).
const MAX_LINE_CHARS: usize = 2000;

fn shorten(t: &str) -> std::borrow::Cow<'_, str> {
    match t.char_indices().nth(MAX_LINE_CHARS) {
        Some((i, _)) => format!("{}…", &t[..i]).into(),
        None => t.into(),
    }
}

pub(super) fn sign(kind: LineKind, marked: bool) -> &'static str {
    match (marked, kind) {
        (true, _) => "●",
        (_, LineKind::Added) => "+",
        (_, LineKind::Removed) => "−",
        (_, LineKind::Context) => "",
    }
}

/// Whether a Review Comment covers this line (a removed line only if the
/// comment quotes it).
pub(super) fn marked(st: &State, path: &str, l: &rade_core::diff::DiffLine) -> bool {
    st.comments.iter().any(|c| {
        c.path == path
            && (c.line..=c.end_line.unwrap_or(c.line)).contains(&l.at)
            && (l.kind != LineKind::Removed
                || c.excerpt
                    .iter()
                    .any(|e| e.get(1..) == Some(l.text.as_str())))
    })
}

/// One line of the highlighter buffer as Pango markup: each run of text
/// with the colour, weight and style of its highest-priority tags.
fn line_markup(b: &sourceview5::Buffer, line: i32) -> String {
    let Some(mut it) = b.iter_at_line(line) else {
        return String::new();
    };
    let mut end = it;
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    let mut out = String::new();
    while it < end {
        let mut next = it;
        next.forward_to_tag_toggle(None::<&gtk::TextTag>);
        if next > end || next <= it {
            next = end;
        }
        let text = glib::markup_escape_text(&b.text(&it, &next, true));
        let (mut fg, mut bold, mut italic) = (None, false, false);
        for t in it.tags() {
            if t.is_foreground_set() {
                fg = t.foreground_rgba();
            }
            if t.is_weight_set() {
                bold = t.weight() >= 600;
            }
            if t.is_style_set() {
                italic = t.style() == gtk::pango::Style::Italic;
            }
        }
        let mut attrs = String::new();
        if let Some(c) = fg {
            let h = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
            attrs += &format!(
                " foreground=\"#{:02x}{:02x}{:02x}\"",
                h(c.red()),
                h(c.green()),
                h(c.blue())
            );
        }
        if bold {
            attrs += " weight=\"bold\"";
        }
        if italic {
            attrs += " style=\"italic\"";
        }
        if attrs.is_empty() {
            out += &text;
        } else {
            out += &format!("<span{attrs}>{text}</span>");
        }
        it = next;
    }
    out
}
