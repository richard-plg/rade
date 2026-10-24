//! Review Pane comments (R13.2, M11, R118.4): the composer opens as a row
//! right under the selected lines; each comment is a card under its last
//! line (an outdated one at the top of its file) — drafts with Edit and
//! Delete. The footer counts drafts; `Discard…` asks before dropping them.

use super::{Pending, ReviewView};
use crate::app::try_app;
use adw::prelude::*;
use gtk::{gdk, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::diff::{DiffLine, Row};
use rade_core::review::ReviewComment;
use rade_core::review_loop::{Approve, Card, CommentState};

/// The comment composer: one widget, moved into whichever row shows it.
pub(super) struct ComposerUi {
    pub root: gtk::Box,
    pub title: gtk::Label,
    pub text: gtk::TextView,
}

impl ComposerUi {
    pub fn new() -> Self {
        let title = gtk::Label::new(None);
        title.set_xalign(0.0);
        title.add_css_class("review-card-head");
        let text = gtk::TextView::new();
        text.set_wrap_mode(gtk::WrapMode::WordChar);
        text.set_accepts_tab(false);
        text.add_css_class("review-composer-text");
        text.set_size_request(-1, 58);
        let hint = gtk::Label::new(Some("Enter add · Shift+Enter newline · Esc cancel"));
        hint.set_xalign(0.0);
        hint.add_css_class("review-hint");
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.add_css_class("review-card");
        root.add_css_class("review-composer");
        root.append(&title);
        root.append(&text);
        root.append(&hint);
        Self { root, title, text }
    }

    pub fn get(&self) -> String {
        let b = self.text.buffer();
        b.text(&b.start_iter(), &b.end_iter(), false).to_string()
    }

    pub fn set(&self, t: &str) {
        self.text.buffer().set_text(t);
    }
}

/// What the cards show of the comments (text excluded: typing in a comment
/// must not rebuild them).
pub(super) fn signature(comments: &[ReviewComment]) -> Vec<Sig> {
    comments
        .iter()
        .map(|c| (c.id, c.location(), c.outdated, c.round, c.resolved))
        .collect()
}

/// A comment as the cards show it: id, place, outdated, Round, resolved.
pub(super) type Sig = (u64, String, bool, Option<u32>, bool);

/// A comment's card position.
pub(super) fn card_of(c: &ReviewComment) -> Card {
    Card {
        id: c.id,
        path: c.path.clone(),
        line: c.end_line.unwrap_or(c.line),
        outdated: c.outdated,
    }
}

/// The id the composer takes among the cards.
pub(super) const COMPOSER: u64 = u64::MAX;

impl ReviewView {
    pub(super) fn connect_composer(self: &std::rc::Rc<Self>) {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = std::rc::Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(v) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
            match key {
                gdk::Key::Return | gdk::Key::KP_Enter if !shift => {
                    v.add_pending();
                    glib::Propagation::Stop
                }
                gdk::Key::Escape => {
                    v.cancel_comment();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        self.composer.text.add_controller(keys);
        let weak = std::rc::Rc::downgrade(self);
        self.discard.connect_clicked(move |_| {
            if let Some(v) = weak.upgrade() {
                v.confirm_discard();
            }
        });
    }

    /// The selected rows as a pending comment; opens the composer.
    pub(super) fn start_comment(&self) {
        let sel = self.selection.selection();
        if sel.is_empty() {
            self.note.set_label("Select changed lines to comment on");
            return;
        }
        self.comment_rows(sel.minimum() as usize, sel.maximum() as usize);
    }

    /// Rows `a..=b` (the lines of the first file among them).
    pub(super) fn comment_rows(&self, a: usize, b: usize) {
        let pending = {
            let st = self.state.borrow();
            let Some(c) = st.changes.as_ref() else { return };
            let picked: Vec<(usize, usize, usize)> = st
                .rows
                .get(a..=b.min(st.rows.len().saturating_sub(1)))
                .unwrap_or_default()
                .iter()
                .filter_map(|r| match *r {
                    Row::Line(f, h, l) => Some((f, h, l)),
                    _ => None,
                })
                .collect();
            let Some(&(fi, hi, _)) = picked.first() else {
                self.note.set_label("Select changed lines to comment on");
                return;
            };
            let f = &c.files[fi];
            // One hunk: its lines are one block of the file (an anchor
            // across hunks could never be found again).
            let lines: Vec<&DiffLine> = picked
                .iter()
                .filter(|(x, h, _)| *x == fi && *h == hi)
                .map(|&(_, h, l)| &f.hunks[h].lines[l])
                .collect();
            rade_core::review::anchor(&lines).map(|(line, end_line, excerpt)| Pending {
                path: f.path.clone(),
                line,
                end_line,
                excerpt,
                hunk: f.hunks[hi].header.clone(),
                edit: None,
                outdated: false,
                reply_to: None,
            })
        };
        let Some(p) = pending else { return };
        self.open_composer(p, "");
    }

    /// Show the composer for `p` with `text`, focused.
    fn open_composer(&self, p: Pending, text: &str) {
        let loc = match p.end_line {
            Some(e) => format!("{}:{}-{e}", p.path, p.line),
            None => format!("{}:{}", p.path, p.line),
        };
        let verb = match (p.edit, p.reply_to) {
            (Some(_), _) => "Edit comment on",
            (None, Some(_)) => "Follow up on",
            _ => "Comment on",
        };
        self.composer.title.set_label(&format!("{verb} {loc}"));
        self.composer.set(text);
        self.state.borrow_mut().pending = Some(p);
        self.render_diff();
        // The composer row is laid out on the next frames: focus it then.
        let (text, list) = (self.composer.text.clone(), self.list.clone());
        let pos = self
            .state
            .borrow()
            .rows
            .iter()
            .position(|r| *r == Row::Composer);
        if let Some(pos) = pos {
            list.scroll_to(
                pos as u32,
                gtk::ListScrollFlags::NONE,
                None::<gtk::ScrollInfo>,
            );
        }
        glib::idle_add_local_once(move || {
            text.grab_focus();
        });
    }

    pub(super) fn cancel_comment(&self) {
        if self.state.borrow_mut().pending.take().is_some() {
            self.render_diff();
        }
        self.list.grab_focus();
    }

    pub(super) fn add_pending(&self) {
        let text = self.composer.get();
        let Some(p) = self.state.borrow().pending.clone() else {
            return;
        };
        let (Some((pid, wid, root)), Some(app)) = (self.place(), try_app()) else {
            return;
        };
        if text.trim().is_empty() {
            self.note.set_label("Type a comment, or Esc to cancel");
            return;
        }
        // Closed before the model changes: the app's sync renders the rows.
        self.state.borrow_mut().pending = None;
        let r = match p.edit {
            Some(id) => app
                .update(|m| m.edit_comment(&pid, &wid, id, &text))
                .map(|_| id),
            None => {
                let mut c = ReviewComment {
                    id: 0,
                    path: p.path.clone(),
                    line: p.line,
                    end_line: p.end_line,
                    excerpt: p.excerpt.clone(),
                    text,
                    reply_to: p.reply_to,
                    ..Default::default()
                };
                // Its anchor, so it can follow its lines when the file
                // changes (M11).
                if let Ok(file) = std::fs::read_to_string(root.join(&c.path)) {
                    c.capture(&file, Some(&p.hunk));
                }
                app.update(|m| m.add_comment(&pid, &wid, c))
            }
        };
        match r {
            Ok(_) => {
                self.composer.set("");
                self.selection.unselect_all();
                self.render_diff();
                self.render_files();
                self.list.grab_focus();
            }
            Err(e) => {
                self.state.borrow_mut().pending = Some(p);
                self.note.set_label(&e.message);
            }
        }
    }

    /// Edit a draft in the composer, where its card was.
    fn edit_comment(&self, c: &ReviewComment) {
        self.open_composer(
            Pending {
                path: c.path.clone(),
                line: c.line,
                end_line: c.end_line,
                excerpt: c.excerpt.clone(),
                hunk: String::new(),
                edit: Some(c.id),
                outdated: c.outdated,
                reply_to: c.reply_to,
            },
            &c.text,
        );
    }

    /// Follow up a sent comment: a draft on the same lines (R119.3).
    pub(super) fn follow_up(&self, c: &ReviewComment) {
        // The lines as they are now (the Agent may have changed them).
        let now: Option<Vec<String>> = self.place().and_then(|(_, _, root)| {
            let text = std::fs::read_to_string(root.join(&c.path)).ok()?;
            let lines: Vec<&str> = text.lines().collect();
            let (a, b) = (c.line as usize, c.end_line.unwrap_or(c.line) as usize);
            let got: Vec<String> = lines
                .get(a.checked_sub(1)?..b.min(lines.len()))?
                .iter()
                .map(|l| format!(" {l}"))
                .collect();
            (!got.is_empty()).then_some(got)
        });
        self.open_composer(
            Pending {
                path: c.path.clone(),
                line: c.line,
                end_line: c.end_line,
                excerpt: now.unwrap_or_else(|| c.excerpt.clone()),
                hunk: c
                    .context
                    .as_ref()
                    .map(|x| x.hunk.clone())
                    .unwrap_or_default(),
                edit: None,
                outdated: false,
                reply_to: Some(c.id),
            },
            "",
        );
    }

    pub(super) fn resolve(&self, id: u64, on: bool) {
        if let (Some((p, w, _)), Some(app)) = (self.place(), try_app()) {
            if let Err(e) = app.update(|m| m.resolve_comment(&p, &w, id, on)) {
                self.note.set_label(&e.message);
            }
            self.render_diff();
            self.render_files();
        }
    }

    fn delete_comment(&self, id: u64) {
        if let (Some((p, w, _)), Some(app)) = (self.place(), try_app()) {
            let _ = app.update(|m| m.edit_comment(&p, &w, id, ""));
        }
    }

    fn confirm_discard(&self) {
        let n = self
            .comments_now()
            .iter()
            .filter(|c| c.round.is_none())
            .count();
        if n == 0 {
            return;
        }
        let d = adw::AlertDialog::new(
            Some(&format!(
                "Discard {n} draft comment{}?",
                if n == 1 { "" } else { "s" }
            )),
            Some("They have not been sent to the Agent."),
        );
        d.add_response("cancel", "Cancel");
        d.add_response("discard", "Discard");
        d.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        d.set_default_response(Some("cancel"));
        d.set_close_response("cancel");
        let pane = self.id.clone();
        d.connect_response(None, move |_, r| {
            if r != "discard" {
                return;
            }
            let Some(app) = try_app() else { return };
            let v = app.reviews.borrow().get(&pane).cloned();
            if let Some(v) = v {
                v.clear_comments();
            }
        });
        if let Some(app) = try_app() {
            d.present(Some(&app.window));
            *self.discard_dialog.borrow_mut() = Some(d);
        }
    }

    pub(super) fn clear_comments(&self) {
        let (Some((pid, wid, _)), Some(app)) = (self.place(), try_app()) else {
            return;
        };
        let _ = app.update(|m| m.take_comments(&pid, &wid));
        self.sync_now();
    }

    /// The footer (R118.8, R119.4): drafts count, Discard…, and one
    /// primary button — *Request changes (n)* with drafts, else
    /// *Approve ▾*.
    pub(super) fn render_footer(&self, comments: &[ReviewComment]) {
        let n = comments.iter().filter(|c| c.round.is_none()).count();
        let open = comments
            .iter()
            .filter(|c| c.round.is_some() && !c.resolved)
            .count();
        self.send.set_label(&format!("Request changes ({n})"));
        self.send.set_visible(n > 0);
        self.approve.set_visible(n == 0);
        self.discard.set_sensitive(n > 0);
        self.drafts.set_label(&match (n, open) {
            (0, 0) => String::new(),
            (0, o) => format!("{o} open"),
            (1, 0) => "1 draft".into(),
            (n, 0) => format!("{n} drafts"),
            (n, o) => format!("{n} draft{} · {o} open", if n == 1 { "" } else { "s" }),
        });
    }

    /// The *Approve ▾* items for this Worktree (R119.4).
    pub(super) fn approve_items(&self) -> Vec<Approve> {
        let (Some((p, w, _)), Some(app)) = (self.place(), try_app()) else {
            return vec![];
        };
        let m = app.model.borrow();
        let live = !m.agent_panes(&p, &w).is_empty();
        let fanout = crate::fanout_view::candidate_of(&m, &p, &w).is_some();
        Approve::items(live, fanout)
    }

    /// Fill the Approve popover (when it opens).
    pub(super) fn fill_approve(&self) {
        while let Some(c) = self.approve_list.first_child() {
            self.approve_list.remove(&c);
        }
        for item in self.approve_items() {
            let b = gtk::Button::with_label(item.label());
            b.add_css_class("flat");
            b.add_css_class("review-approve-item");
            if let Some(l) = b.child().and_downcast::<gtk::Label>() {
                l.set_xalign(0.0);
            }
            let pane = self.id.clone();
            let pop = self.approve.popover();
            b.connect_clicked(move |_| {
                if let Some(p) = &pop {
                    p.popdown();
                }
                approve_now(&pane, item);
            });
            self.approve_list.append(&b);
        }
    }

    /// A comment's card (built on bind; cards are few).
    pub(super) fn card(&self, c: &ReviewComment) -> gtk::Widget {
        let state = self
            .state
            .borrow()
            .states
            .get(&c.id)
            .copied()
            .unwrap_or(CommentState::Draft);
        let b = gtk::Box::new(gtk::Orientation::Vertical, 4);
        b.add_css_class("review-card");
        if state == CommentState::Draft {
            b.add_css_class("draft");
        }
        if c.resolved {
            b.add_css_class("resolved");
        }
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let who = match c.round {
            Some(n) => format!("You · round {n} · {}", c.location()),
            None if c.reply_to.is_some() => format!("You · follow-up · {}", c.location()),
            None => format!("You · {}", c.location()),
        };
        let who = gtk::Label::new(Some(&who));
        who.set_xalign(0.0);
        who.add_css_class("review-card-head");
        top.append(&who);
        let chip = gtk::Label::new(Some(if c.resolved {
            "resolved"
        } else {
            state.words()
        }));
        chip.add_css_class("review-chip");
        chip.add_css_class(match (c.resolved, state) {
            (true, _) => "chip-resolved",
            (_, CommentState::Draft) => "chip-draft",
            (_, CommentState::Sent) => "chip-sent",
            (_, CommentState::Changed) => "chip-changed",
            (_, CommentState::Untouched) => "chip-untouched",
        });
        top.append(&chip);
        if c.outdated && !c.resolved {
            let badge = gtk::Label::new(Some("outdated"));
            badge.add_css_class("review-chip");
            badge.add_css_class("chip-outdated");
            badge.set_tooltip_text(Some(
                "Its lines changed or are gone; it is still sent, with the code it quoted",
            ));
            top.append(&badge);
        }
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        top.append(&spacer);
        let action = |label: &str| {
            let b = gtk::Button::with_label(label);
            b.add_css_class("flat");
            b.add_css_class("review-card-action");
            top.append(&b);
            b
        };
        let pane = self.id.clone();
        let view = move || try_app().and_then(|a| a.reviews.borrow().get(&pane).cloned());
        let id = c.id;
        if c.round.is_some() {
            if c.resolved {
                let v = view.clone();
                action("Reopen").connect_clicked(move |_| {
                    if let Some(v) = v() {
                        v.resolve(id, false);
                    }
                });
            } else {
                let (v, cm) = (view.clone(), c.clone());
                action("Follow up").connect_clicked(move |_| {
                    if let Some(v) = v() {
                        v.follow_up(&cm);
                    }
                });
                let v = view.clone();
                action("Resolve").connect_clicked(move |_| {
                    if let Some(v) = v() {
                        v.resolve(id, true);
                    }
                });
            }
        } else {
            let (v, cm) = (view.clone(), c.clone());
            action("Edit").connect_clicked(move |_| {
                if let Some(v) = v() {
                    v.edit_comment(&cm);
                }
            });
            let v = view.clone();
            action("Delete").connect_clicked(move |_| {
                if let Some(v) = v() {
                    v.delete_comment(id);
                }
            });
        }
        b.append(&top);
        // A resolved comment is one line (R119.3).
        if !c.resolved {
            let text = gtk::Label::new(Some(&c.text));
            text.set_xalign(0.0);
            text.set_wrap(true);
            text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            text.add_css_class("review-card-text");
            b.append(&text);
        }
        b.upcast()
    }
}

/// Run an *Approve ▾* item from Review Pane `pane` (R119.4).
pub(super) fn approve_now(pane: &str, item: Approve) {
    let note = match crate::review::approve(pane, item) {
        Ok(t) => t,
        Err(e) => e.message,
    };
    if let Some(v) = try_app().and_then(|a| a.reviews.borrow().get(pane).cloned()) {
        v.note.set_label(&note);
        v.sync();
    }
}
