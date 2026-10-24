//! Code navigation on the Review Pane's diff lines (PRD R87): a click puts
//! the "point" on a character of a line (the footer names the word there),
//! F12 / Ctrl+click → definition, Shift+F12 → references, Alt+Shift+H →
//! callers, Ctrl+F12 → implementations. Added and context lines ask the
//! Worktree's server at their new line; a removed line looks its name up
//! in the current tree, else at the base (`code::navigate_removed`).
//! Results open in a popover on the Review Pane; a single definition opens
//! in the Tab's Viewer (Back returns, R87.3).

use super::{rows, ReviewView};
use gtk::prelude::*;
use gtk::{gdk, glib, pango};
use gtk4 as gtk;
use rade_core::code::nav::{word_at, Query};
use rade_core::diff::{LineKind, Row};
use std::time::Duration;

/// A name on a diff line: (file, line, word, its 1-based column, row).
type Target = (String, rade_core::diff::DiffLine, String, u32, usize);

/// The 1-based character column under (x, y) of a label (its coordinates).
pub(super) fn label_col(label: &gtk::Label, x: f64, y: f64) -> Option<u32> {
    let layout = label.layout();
    let (ox, oy) = label.layout_offsets();
    let s = pango::SCALE as f64;
    let (inside, index, _) =
        layout.xy_to_index(((x - ox as f64) * s) as i32, ((y - oy as f64) * s) as i32);
    if !inside {
        return None;
    }
    let text = layout.text();
    let i = (index.max(0) as usize).min(text.len());
    Some(text.get(..i).map_or(0, |p| p.chars().count()) as u32 + 1)
}

/// The middle of character `col` (1-based) of a label, in its coordinates.
pub(super) fn label_point(label: &gtk::Label, col: u32) -> Option<(f64, f64)> {
    let layout = label.layout();
    let text = layout.text();
    let i = text.char_indices().nth(col.max(1) as usize - 1)?.0;
    let r = layout.index_to_pos(i as i32);
    let (ox, oy) = label.layout_offsets();
    let s = pango::SCALE as f64;
    Some((
        ox as f64 + (r.x() as f64 + r.width() as f64 / 4.0) / s,
        oy as f64 + (r.y() as f64 + r.height() as f64 / 2.0) / s,
    ))
}

/// The code label of a diff row widget (old, new, sign, code).
pub(super) fn code_label(row: &gtk::Widget) -> Option<gtk::Label> {
    row.first_child()?
        .next_sibling()?
        .next_sibling()?
        .next_sibling()
        .and_downcast()
}

impl ReviewView {
    /// A click on row `pos` at (x, y) of its widget: the point goes there;
    /// with Ctrl, its definition.
    pub(super) fn click_line(&self, row: &gtk::Widget, pos: usize, x: f64, y: f64, ctrl: bool) {
        crate::code_pop::close();
        if !matches!(self.state.borrow().rows.get(pos), Some(Row::Line(..))) {
            return;
        }
        let Some(code) = code_label(row) else { return };
        let Some(p) = row.compute_point(&code, &gtk::graphene::Point::new(x as f32, y as f32))
        else {
            return;
        };
        let Some(col) = label_col(&code, p.x() as f64, p.y() as f64) else {
            return;
        };
        self.set_point(pos, col);
        // Ctrl+click only on a name, as the link shows it (not a keyword).
        let text = code.text();
        let on = text
            .chars()
            .nth(col as usize - 1)
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$')
            && word_at(&text, col as usize - 1)
                .is_some_and(|(w, _)| !rade_core::code::nav::is_keyword(&w));
        if ctrl && on {
            // This line's name, whatever rows are selected (they stay so).
            self.nav_to(Query::Def, self.target_at(pos, Some(col)));
        }
    }

    /// The name under (x, y) of the list on a diff line: (its code label,
    /// the name's byte range in the label's text).
    fn name_under(&self, x: f64, y: f64) -> Option<(gtk::Label, u32, u32)> {
        let mut w = self.list.pick(x, y, gtk::PickFlags::DEFAULT);
        while let Some(c) = w.clone() {
            if c.has_css_class("rade-diff-row") {
                break;
            }
            w = c.parent();
        }
        let row = w?;
        let pos = row.widget_name().parse::<usize>().ok()?;
        if !matches!(self.state.borrow().rows.get(pos), Some(Row::Line(..))) {
            return None;
        }
        let code = code_label(&row)?;
        let p = self
            .list
            .compute_point(&code, &gtk::graphene::Point::new(x as f32, y as f32))?;
        let col = label_col(&code, p.x() as f64, p.y() as f64)? as usize - 1;
        let text = code.text();
        let on = text
            .chars()
            .nth(col)
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$');
        let (w, s) = on
            .then(|| word_at(&text, col))
            .flatten()
            .filter(|(w, _)| !rade_core::code::nav::is_keyword(w))?;
        let byte = |n: usize| text.char_indices().nth(n).map_or(text.len(), |(b, _)| b) as u32;
        Some((code.clone(), byte(s), byte(s + w.chars().count())))
    }

    /// Ctrl held over a name on a diff line (0.7.1): underlined, the
    /// pointer a hand; `None` (or not on a name) = plain again.
    pub(super) fn set_link(&self, at: Option<(f64, f64)>) {
        let name = at.and_then(|(x, y)| self.name_under(x, y));
        let same = match (&*self.link.borrow(), &name) {
            // A recycled row lost its attributes: apply them again.
            (Some(a), Some(b)) => {
                a.0 == b.0 && a.1 == b.1 && a.2 == b.2 && a.0.attributes().is_some()
            }
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        if let Some((l, _, _)) = self.link.borrow_mut().take() {
            l.set_attributes(None);
        }
        match &name {
            Some((l, a, b)) => {
                let attrs = pango::AttrList::new();
                let mut u = pango::AttrInt::new_underline(pango::Underline::Single);
                u.set_start_index(*a);
                u.set_end_index(*b);
                attrs.insert(u);
                let rgba = crate::viewer::link_rgba();
                let ch = |v: f32| (v * 65535.0) as u16;
                let mut c = pango::AttrColor::new_foreground(
                    ch(rgba.red()),
                    ch(rgba.green()),
                    ch(rgba.blue()),
                );
                c.set_start_index(*a);
                c.set_end_index(*b);
                attrs.insert(c);
                l.set_attributes(Some(&attrs));
                self.list.set_cursor_from_name(Some("pointer"));
            }
            None => self.list.set_cursor(None),
        }
        *self.link.borrow_mut() = name;
    }

    /// The linked name (`debug review info`).
    pub(super) fn link_word(&self) -> Option<String> {
        let l = self.link.borrow();
        let (label, a, b) = l.as_ref()?;
        label.text().get(*a as usize..*b as usize).map(String::from)
    }

    /// The point: the footer names the word there.
    pub(super) fn set_point(&self, pos: usize, col: u32) {
        self.state.borrow_mut().point = Some((pos, col));
        let word = self.target().map(|t| t.2);
        self.set_note(&match word {
            Some(w) => format!("{w} — F12 definition · Ctrl+I docs"),
            None => String::new(),
        });
        self.note.set_tooltip_text(Some(
            "F12 or Ctrl+click: definition · Shift+F12: references · Alt+Shift+H: callers · Ctrl+F12: implementations · Ctrl+I: type and docs · Alt+F12: peek",
        ));
    }

    /// What the keys ask about: the point (on a selected row, or with
    /// nothing selected), else the first name on the first selected line.
    /// (file, diff line, word, column, row).
    fn target(&self) -> Option<Target> {
        let sel = self.selection.selection();
        let (pos, col) = match self.state.borrow().point {
            Some((p, col)) if sel.is_empty() || sel.contains(p as u32) => (p, Some(col)),
            _ if !sel.is_empty() => (sel.minimum() as usize, None),
            _ => return None,
        };
        self.target_at(pos, col)
    }

    /// The name at character `col` of row `pos` (none: its first name).
    fn target_at(&self, pos: usize, col: Option<u32>) -> Option<Target> {
        let st = self.state.borrow();
        let c = st.changes.as_ref()?;
        let Some(Row::Line(fi, hi, li)) = st.rows.get(pos).copied() else {
            return None;
        };
        let f = &c.files[fi];
        let l = f.hunks[hi].lines[li].clone();
        let (word, start) = match col {
            Some(col) => word_at(&l.text, col as usize - 1)?,
            // The first name on the line.
            None => (0..l.text.chars().count()).find_map(|i| word_at(&l.text, i))?,
        };
        Some((f.path.clone(), l, word, start as u32 + 1, pos))
    }

    /// Ask about the target (R87.1).
    pub(super) fn nav(&self, q: Query) {
        self.nav_to(q, self.target());
    }

    fn nav_to(&self, q: Query, target: Option<Target>) {
        let Some((_, _, root)) = self.place() else {
            return;
        };
        let Some((path, l, word, col, _)) = target else {
            self.set_note("Click a name on a diff line first");
            return;
        };
        crate::code_pop::close();
        let file = root.join(&path);
        match (l.kind, l.new) {
            (LineKind::Removed, _) | (_, None) => {
                let base = self
                    .state
                    .borrow()
                    .changes
                    .as_ref()
                    .map(|c| c.base.clone())
                    .unwrap_or_default();
                crate::code::navigate_removed(&self.id, q, &file, &word, &base);
            }
            (_, Some(line)) => crate::code::navigate(&self.id, q, &file, line, col, Duration::ZERO),
        }
    }

    /// Hover (Ctrl+I) or peek (Alt+F12) at the target, in a popover on
    /// its line (R90).
    pub(super) fn pop(&self, peek: bool) {
        let Some((_, _, root)) = self.place() else {
            return;
        };
        let Some((path, l, word, col, _)) = self.target() else {
            self.set_note("Click a name on a diff line first");
            return;
        };
        let at = self.target_anchor();
        let file = root.join(&path);
        match (l.kind, l.new) {
            (LineKind::Removed, _) | (_, None) => {
                let base = self
                    .state
                    .borrow()
                    .changes
                    .as_ref()
                    .map(|c| c.base.clone())
                    .unwrap_or_default();
                crate::code_pop::ask_removed(&self.id, peek, at, &file, &word, &base);
            }
            (_, Some(line)) => crate::code_pop::ask(&self.id, peek, at, &file, line, col),
        }
    }

    /// Where a popover at the target name points: the row's code label at
    /// the name (the list's own corner if it scrolled away).
    pub(crate) fn target_anchor(&self) -> crate::code_pop::Anchor {
        let Some((_, _, _, col, pos)) = self.target() else {
            return crate::code_pop::Anchor {
                widget: self.list.clone().upcast(),
                rect: gtk::gdk::Rectangle::new(0, 0, 8, 8),
                pointer: None,
            };
        };
        let label = rows::row_widget(&self.list, pos as u32)
            .filter(|w| w.is_mapped())
            .and_then(|w| code_label(&w));
        // Anchored on the list, not the row: rows are recycled while it shows.
        let p = label.as_ref().and_then(|c| {
            let (x, y) = label_point(c, col)?;
            c.compute_point(&self.list, &gtk::graphene::Point::new(x as f32, y as f32))
        });
        match p {
            Some(p) => crate::code_pop::Anchor {
                widget: self.list.clone().upcast(),
                rect: gtk::gdk::Rectangle::new(p.x() as i32 - 4, p.y() as i32 - 8, 8, 16),
                pointer: None,
            },
            None => crate::code_pop::Anchor {
                widget: self.list.clone().upcast(),
                rect: gtk::gdk::Rectangle::new(0, 0, 8, 8),
                pointer: None,
            },
        }
    }

    /// The navigation keys; true = handled.
    pub(super) fn on_nav_key(&self, key: gdk::Key, state: gdk::ModifierType) -> bool {
        use gdk::ModifierType as M;
        let mods = state & (M::CONTROL_MASK | M::SHIFT_MASK | M::ALT_MASK);
        if key == gdk::Key::Escape && crate::code_pop::close() {
            return true;
        }
        match (key.to_lower(), mods) {
            (gdk::Key::i, M::CONTROL_MASK) => {
                self.pop(false);
                return true;
            }
            (gdk::Key::F12, M::ALT_MASK) => {
                self.pop(true);
                return true;
            }
            _ => {}
        }
        let q = match (key.to_lower(), mods) {
            (gdk::Key::F12, m) if m.is_empty() => Query::Def,
            (gdk::Key::F12, M::SHIFT_MASK) => Query::Refs,
            (gdk::Key::F12, M::CONTROL_MASK) => Query::Impl,
            (gdk::Key::h, m) if m == M::ALT_MASK | M::SHIFT_MASK => Query::Callers,
            _ => return false,
        };
        self.nav(q);
        true
    }

    /// The row containing `text` on screen (scrolled there), for debug
    /// clicks: (row index, its widget).
    pub(super) fn shown_row(&self, text: &str) -> rade_core::rpc::Result<(usize, gtk::Widget)> {
        use rade_core::rpc::Error;
        let i = self.row_of(text)?;
        self.scroll_to_top(i as u32);
        let ctx = glib::MainContext::default();
        let mut row = None;
        for _ in 0..100 {
            while ctx.iteration(false) {}
            row = rows::row_widget(&self.list, i as u32).filter(|w| w.is_mapped());
            if row.is_some() && self.scroll_req.get().is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok((i, row.ok_or_else(|| Error::internal("not on screen"))?))
    }

    /// (row widget, its point on `word`) of the row containing `text`.
    pub(super) fn word_point(
        &self,
        spec: &str,
    ) -> rade_core::rpc::Result<(usize, gtk::Widget, f64, f64)> {
        use rade_core::rpc::Error;
        let (text, word) = spec
            .split_once('|')
            .ok_or_else(|| Error::usage("want ROW TEXT|WORD"))?;
        let (i, row) = self.shown_row(text)?;
        let line = self.row_text(self.state.borrow().rows[i]);
        let at = line
            .find(word)
            .ok_or_else(|| Error::not_found(format!("'{word}' is not on that line")))?;
        let col = line[..at].chars().count() as u32 + 1;
        let code = code_label(&row).ok_or_else(|| Error::internal("no code label"))?;
        let (x, y) = label_point(&code, col).ok_or_else(|| Error::internal("no such column"))?;
        let p = code
            .compute_point(&row, &gtk::graphene::Point::new(x as f32, y as f32))
            .ok_or_else(|| Error::internal("not on screen"))?;
        Ok((i, row, p.x() as f64, p.y() as f64))
    }
}
