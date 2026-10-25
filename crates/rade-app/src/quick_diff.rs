//! Quick Diff (PRD R123, ADR-0045): the Viewer's gutter marks of the lines
//! of a tracked file changed against the index — a bar for added lines
//! (`add`), another for modified ones (`run`), a triangle for a deletion
//! (`del`). A click on a mark opens a peek with that change's hunk and
//! Stage Change / Revert Change… / Open Changes; one popover at a time
//! (R90). The marks follow the file: on every (re)load of the Viewer and
//! every 3 s while it is on screen.

use crate::app::try_app;
use crate::worktrees::in_background;
use gtk::{gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use libadwaita::prelude::*;
use rade_core::diff_view::{self, Mark, MarkKind};
use rade_core::rpc::{Error, Result};
use rade_core::scm::{self, HunkOp, Section};
use serde_json::{json, Value};
use sourceview5::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

const POLL: Duration = Duration::from_secs(3);

/// Lines of a hunk a peek shows before it says `…`.
const PEEK_LINES: usize = 30;

const CATEGORIES: [(&str, MarkKind); 3] = [
    ("rade-qd-added", MarkKind::Added),
    ("rade-qd-modified", MarkKind::Modified),
    ("rade-qd-deleted", MarkKind::Deleted),
];

fn category(k: MarkKind) -> &'static str {
    CATEGORIES.iter().find(|c| c.1 == k).map_or("", |c| c.0)
}

fn kind_name(k: MarkKind) -> &'static str {
    match k {
        MarkKind::Added => "added",
        MarkKind::Modified => "modified",
        MarkKind::Deleted => "deleted",
    }
}

/// The gutter icon of a mark kind, painted in the colour of its token.
fn icon(k: MarkKind) -> gio::BytesIcon {
    let svg = |body: String| {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\" viewBox=\"0 0 16 16\">{body}</svg>"
        )
    };
    let svg = match k {
        MarkKind::Added => svg(format!(
            "<rect x=\"0\" y=\"0\" width=\"3\" height=\"16\" fill=\"{}\"/>",
            crate::tokens::rgb_now("add")
        )),
        MarkKind::Modified => svg(format!(
            "<rect x=\"0\" y=\"0\" width=\"3\" height=\"16\" fill=\"{}\"/>",
            crate::tokens::rgb_now("run")
        )),
        MarkKind::Deleted => svg(format!(
            "<path d=\"M0 0 L6 4 L0 8 Z\" fill=\"{}\"/>",
            crate::tokens::rgb_now("del")
        )),
    };
    gio::BytesIcon::new(&glib::Bytes::from_owned(svg.into_bytes()))
}

/// What a peek holds, for `debug.viewer peek`.
struct Peek {
    popover: gtk::Popover,
    lines: Vec<String>,
    buttons: Vec<(String, gtk::Button)>,
}

#[derive(Default)]
struct St {
    path: Option<PathBuf>,
    marks: Vec<Mark>,
}

pub struct QuickDiff {
    /// The Viewer Pane's id.
    id: String,
    view: sourceview5::View,
    buffer: sourceview5::Buffer,
    st: RefCell<St>,
    peek: RefCell<Option<Peek>>,
    /// Bumped by each read: a late answer for an older one is dropped.
    gen: Cell<u64>,
}

impl QuickDiff {
    /// Hook the marks into a Viewer's source view.
    pub fn attach(id: &str, view: &sourceview5::View, buffer: &sourceview5::Buffer) -> Rc<Self> {
        let q = Rc::new(Self {
            id: id.into(),
            view: view.clone(),
            buffer: buffer.clone(),
            st: RefCell::default(),
            peek: RefCell::default(),
            gen: Cell::new(0),
        });
        q.restyle();
        let weak = Rc::downgrade(&q);
        view.connect_line_mark_activated(move |_, iter, button, _, _| {
            if button != 1 {
                return;
            }
            if let Some(q) = weak.upgrade() {
                q.clicked(iter.line() as u32 + 1);
            }
        });
        // A click in the Viewer closes a peek (the mark's own click opens the next).
        let click = gtk::GestureClick::new();
        click.set_button(0);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(&q);
        click.connect_pressed(move |_, _, _, _| {
            if let Some(q) = weak.upgrade() {
                q.close_peek();
            }
        });
        view.add_controller(click);
        let weak = Rc::downgrade(&q);
        glib::timeout_add_local(POLL, move || match weak.upgrade() {
            Some(q) => {
                if q.view.is_mapped() && try_app().is_some_and(|a| a.window.is_active()) {
                    q.refresh();
                }
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        q
    }

    /// The file the Viewer shows now (`None`: not a code file).
    pub fn set_file(self: &Rc<Self>, path: Option<&Path>) {
        self.close_peek();
        self.st.borrow_mut().path = path.map(Path::to_path_buf);
        self.st.borrow_mut().marks.clear();
        self.apply_marks();
        if path.is_some() {
            self.refresh();
        }
    }

    /// Light / dark changed: the icons follow the tokens.
    pub fn restyle(&self) {
        for (cat, kind) in CATEGORIES {
            let a = sourceview5::MarkAttributes::new();
            a.set_gicon(&icon(kind));
            self.view.set_mark_attributes(cat, &a, 10);
        }
    }

    pub fn marks(&self) -> Value {
        json!(self
            .st
            .borrow()
            .marks
            .iter()
            .map(|m| json!({"line": m.line, "kind": kind_name(m.kind)}))
            .collect::<Vec<_>>())
    }

    /// Read the file's diff against the index off the GTK thread; the
    /// marks change only if the answer differs.
    pub fn refresh(self: &Rc<Self>) {
        let Some(path) = self.st.borrow().path.clone() else {
            return;
        };
        let on = try_app().is_none_or(|a| a.config.borrow().scm.quick_diff);
        if !on {
            if !self.st.borrow().marks.is_empty() {
                self.st.borrow_mut().marks.clear();
                self.apply_marks();
            }
            return;
        }
        let gen = self.gen.get() + 1;
        self.gen.set(gen);
        let weak = Rc::downgrade(self);
        let p = path.clone();
        in_background(
            move || -> Vec<Mark> {
                match scm::quick_diff(&p) {
                    Ok(Some(u0)) => diff_view::quick_marks(&u0),
                    _ => vec![],
                }
            },
            move |marks| {
                let Some(q) = weak.upgrade() else { return };
                let same_file = q.st.borrow().path.as_deref() == Some(path.as_path());
                if q.gen.get() != gen || !same_file || q.st.borrow().marks == marks {
                    return;
                }
                q.st.borrow_mut().marks = marks;
                q.apply_marks();
            },
        );
    }

    /// Put the marks on the buffer (its text was replaced: they are gone).
    fn apply_marks(&self) {
        let (start, end) = self.buffer.bounds();
        for (cat, _) in CATEGORIES {
            self.buffer.remove_source_marks(&start, &end, Some(cat));
        }
        let marks = self.st.borrow().marks.clone();
        let last = self.buffer.line_count() - 1;
        for m in &marks {
            let line = (m.line as i32 - 1).clamp(0, last.max(0));
            if let Some(it) = self.buffer.iter_at_line(line) {
                self.buffer.create_source_mark(None, category(m.kind), &it);
            }
        }
        self.view.set_show_line_marks(!marks.is_empty());
    }

    /// A click on the marks gutter at `line` (1-based).
    fn clicked(self: &Rc<Self>, line: u32) {
        if !self.st.borrow().marks.iter().any(|m| m.line == line) {
            return;
        }
        self.peek_line(line, None);
    }

    /// Close the peek; false when none was open (Esc goes on).
    pub fn close_peek(&self) -> bool {
        let Some(p) = self.peek.borrow_mut().take() else {
            return false;
        };
        p.popover.popdown();
        let w = p.popover.clone();
        glib::idle_add_local_once(move || w.unparent());
        true
    }

    /// Open the peek for the change at `line`; `hunk` = its text already
    /// read (the debug op reads it on the spot), else read off-thread.
    fn peek_line(self: &Rc<Self>, line: u32, hunk: Option<(scm::FilePatch, usize)>) {
        let Some(path) = self.st.borrow().path.clone() else {
            return;
        };
        if let Some(h) = hunk {
            self.show_peek(line, h);
            return;
        }
        let weak = Rc::downgrade(self);
        in_background(
            move || scm::hunk_at_line(&path, line),
            move |r| {
                if let (Some(q), Ok(Some(h))) = (weak.upgrade(), r) {
                    q.show_peek(line, h);
                }
            },
        );
    }

    fn show_peek(self: &Rc<Self>, line: u32, (fp, hi): (scm::FilePatch, usize)) {
        // One popover at a time (R90).
        crate::code_pop::close();
        self.close_peek();
        let Some(path) = self.st.borrow().path.clone() else {
            return;
        };
        let text = fp.hunks[hi].clone();
        let lines: Vec<String> = text.lines().skip(1).map(str::to_string).collect();

        let pop = gtk::Popover::new();
        pop.add_css_class("quick-diff-peek");
        // Closed by Esc and a click in the Viewer, like the code popovers (R90).
        pop.set_autohide(false);
        pop.set_parent(&self.view);
        pop.set_position(gtk::PositionType::Right);
        if let Some(it) = self.buffer.iter_at_line(line as i32 - 1) {
            let r = self.view.iter_location(&it);
            let (_, y) =
                self.view
                    .buffer_to_window_coords(gtk::TextWindowType::Widget, r.x(), r.y());
            pop.set_pointing_to(Some(&gtk::gdk::Rectangle::new(0, y, 24, r.height().max(1))));
        }
        let col = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        for l in lines.iter().take(PEEK_LINES) {
            let lab = gtk::Label::new(Some(l));
            lab.set_xalign(0.0);
            lab.add_css_class("monospace");
            match l.as_bytes().first() {
                Some(b'+') => lab.add_css_class("rade-diff-added"),
                Some(b'-') => lab.add_css_class("rade-diff-removed"),
                _ => {}
            }
            body.append(&lab);
        }
        if lines.len() > PEEK_LINES {
            let more = gtk::Label::new(Some("…"));
            more.set_xalign(0.0);
            body.append(&more);
        }
        col.append(&body);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let mut buttons = Vec::new();
        for label in ["Stage Change", "Revert Change…", "Open Changes"] {
            let b = gtk::Button::with_label(label);
            b.add_css_class("flat");
            let (weak, patch, p, pop2) = (
                Rc::downgrade(self),
                scm::hunk_patch(&fp, hi),
                path.clone(),
                pop.downgrade(),
            );
            b.connect_clicked(move |_| {
                if let Some(pop) = pop2.upgrade() {
                    pop.popdown();
                }
                let Some(q) = weak.upgrade() else { return };
                match label {
                    "Stage Change" => {
                        q.apply(patch.clone(), p.clone(), HunkOp::Stage, "Stage Change")
                    }
                    "Revert Change…" => {
                        let (patch, p) = (patch.clone(), p.clone());
                        let weak = Rc::downgrade(&q);
                        let d = adw::AlertDialog::new(
                            Some("Discard this hunk?"),
                            Some("Its changes are removed from the working tree."),
                        );
                        d.add_response("cancel", "Cancel");
                        d.add_response("discard", "Discard");
                        d.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
                        d.set_default_response(Some("cancel"));
                        d.set_close_response("cancel");
                        d.connect_response(None, move |_, r| {
                            if r == "discard" {
                                if let Some(q) = weak.upgrade() {
                                    q.apply(
                                        patch.clone(),
                                        p.clone(),
                                        HunkOp::Discard,
                                        "Revert Change",
                                    );
                                }
                            }
                        });
                        crate::menus::present(d, &["cancel", "discard"]);
                    }
                    _ => q.open_changes(&p),
                }
            });
            row.append(&b);
            buttons.push((label.to_string(), b));
        }
        col.append(&row);
        pop.set_child(Some(&col));
        pop.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        pop.popup();
        *self.peek.borrow_mut() = Some(Peek {
            popover: pop,
            lines,
            buttons,
        });
    }

    /// Stage or revert one hunk of the file, then read the marks again.
    fn apply(
        self: &Rc<Self>,
        patch: Option<String>,
        file: PathBuf,
        op: HunkOp,
        what: &'static str,
    ) {
        let Some(patch) = patch else { return };
        let weak = Rc::downgrade(self);
        in_background(
            move || -> Result<()> {
                let top = rade_core::git::run(
                    file.parent().unwrap_or(Path::new(".")),
                    &["rev-parse", "--show-toplevel"],
                )?;
                scm::apply_hunk(Path::new(top.trim()), &patch, op)
            },
            move |r| {
                if let Some(q) = weak.upgrade() {
                    if let Err(e) = r {
                        eprintln!("rade: {what}: {}", e.message);
                    }
                    q.refresh();
                }
            },
        );
    }

    /// Open Changes: the Diff Pane (working tree) on this file.
    fn open_changes(&self, file: &Path) {
        let Some(app) = try_app() else { return };
        let Some(parent) = file.parent() else { return };
        let Ok(top) = rade_core::git::run(parent, &["rev-parse", "--show-toplevel"]) else {
            return;
        };
        let root = PathBuf::from(top.trim());
        let Ok(rel) = file
            .canonicalize()
            .unwrap_or_else(|_| file.to_path_buf())
            .strip_prefix(&root)
            .map(|p| p.to_string_lossy().into_owned())
        else {
            return;
        };
        let loc = app.model.borrow().locate(&self.id);
        let Some(loc) = loc else { return };
        let _ = app.update(|m| {
            m.open_diff(
                &loc.source,
                &loc.worktree,
                &root,
                &rel,
                Section::Unstaged,
                true,
            )
        });
    }

    // ---- debug ------------------------------------------------------------

    fn peek_info(&self) -> Value {
        match self.peek.borrow().as_ref() {
            Some(p) => json!({"open": p.popover.is_visible(), "lines": p.lines,
                              "buttons": p.buttons.iter().map(|b| b.0.clone()).collect::<Vec<_>>()}),
            None => json!({"open": false, "lines": [], "buttons": []}),
        }
    }

    /// `debug.viewer peek <line>` = a click on that line's mark.
    pub fn debug_peek(self: &Rc<Self>, line: u32) -> Result<Value> {
        let path = self
            .st
            .borrow()
            .path
            .clone()
            .ok_or_else(|| Error::usage("not showing a file"))?;
        if !self.st.borrow().marks.iter().any(|m| m.line == line) {
            return Err(Error::usage(format!("no Quick Diff mark on line {line}")));
        }
        let hunk = scm::hunk_at_line(&path, line)?
            .ok_or_else(|| Error::usage(format!("no change at line {line}")))?;
        self.peek_line(line, Some(hunk));
        Ok(json!({"peek": self.peek_info()}))
    }

    /// `debug.viewer peek_shot <png>`: the open peek (a surface of its own,
    /// not in the window's screenshot) rendered to a PNG.
    pub fn debug_peek_shot(&self, png: &Path) -> Result<Value> {
        let pop = self
            .peek
            .borrow()
            .as_ref()
            .map(|p| p.popover.clone())
            .ok_or_else(|| Error::usage("no peek is open"))?;
        crate::debug::screenshot_widget(pop.upcast_ref(), png)?;
        Ok(json!({"path": png}))
    }

    /// `debug.viewer peek_action <label>`= a click on that peek button.
    pub fn debug_peek_action(&self, label: &str) -> Result<Value> {
        let b = self
            .peek
            .borrow()
            .as_ref()
            .and_then(|p| p.buttons.iter().find(|b| b.0 == label).map(|b| b.1.clone()))
            .ok_or_else(|| Error::usage(format!("no peek button '{label}'")))?;
        b.emit_clicked();
        Ok(json!({"peek": self.peek_info()}))
    }
}

/// `debug.viewer` ops `peek`, `peek_action` and `peek_shot` (`{"pane", "op", "arg"}`);
/// `None` for any other op.
pub fn debug(p: &Value) -> Option<Result<Value>> {
    let op = p.get("op").and_then(Value::as_str)?;
    if !matches!(op, "peek" | "peek_action" | "peek_shot") {
        return None;
    }
    let p = p.clone();
    Some(crate::handlers::on_main(move || {
        let app = crate::app::app();
        let q = p.get("pane").and_then(Value::as_str).unwrap_or("");
        let id = app.model.borrow().resolve_pane(q, None)?;
        let v = app.viewers.borrow().get(&id).cloned();
        let v = v.ok_or_else(|| Error::usage(format!("pane {id} is not a Viewer")))?;
        let arg = p.get("arg").and_then(Value::as_str).unwrap_or("");
        match p.get("op").and_then(Value::as_str) {
            Some("peek") => v.quick_diff().debug_peek(arg.parse().unwrap_or(0)),
            Some("peek_shot") => v.quick_diff().debug_peek_shot(Path::new(arg)),
            _ => v.quick_diff().debug_peek_action(arg),
        }
    }))
}
