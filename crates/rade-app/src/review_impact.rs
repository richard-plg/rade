//! The Review Pane's Impact section (PRD R88.2): the **Impact** toggle in
//! the header computes the Worktree's Impact (`code.impact`, on a worker
//! thread; it starts the Worktree's language server like any navigation)
//! and shows it in a rail right of the diff (R118.7; an overlay in a narrow Pane): a summary line (`2 changed · 3 call
//! sites in 2 files (1 in tests) · 1 removed, still used`, where it came
//! from), then each changed / added / removed symbol with its callers
//! outside the diff (`test`, `text match` tags). A click on a row opens
//! the Tab's Viewer there (Back returns). While a server indexes it says
//! so and asks again by itself; it is recomputed when the diff changes
//! while shown.

use super::ReviewView;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::code::impact::{self, Impact};
use rade_core::code::nav;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

/// How long the Review waits for the Impact (then text matches).
const BUDGET: Duration = Duration::from_secs(30);

/// One row of the section: its text (for `debug.review`) and where a click goes.
#[derive(Clone, Debug)]
pub(super) struct ImpactRow {
    pub text: String,
    pub at: Option<(PathBuf, u32, u32)>,
}

pub(super) struct ImpactUi {
    pub toggle: gtk::ToggleButton,
    /// The rail right of the diff (R118.7).
    pub panel: gtk::Box,
    summary: gtk::Label,
    source: gtk::Label,
    hint: gtk::Label,
    spinner: adw::Spinner,
    retry: gtk::Button,
    list: gtk::ListBox,
    running: std::cell::Cell<bool>,
    /// The diff changed while a computation ran: run again after it.
    stale: std::cell::Cell<bool>,
    retry_queued: std::cell::Cell<bool>,
}

impl ImpactUi {
    pub fn new() -> Self {
        let toggle = gtk::ToggleButton::with_label("Impact");
        toggle.add_css_class("flat");
        toggle.add_css_class("rade-impact-toggle");
        toggle.set_tooltip_text(Some(
            "Changed symbols and their callers outside the diff (starts the language server)",
        ));
        let summary = gtk::Label::new(None);
        summary.set_xalign(0.0);
        summary.set_wrap(true);
        summary.add_css_class("rade-impact-summary");
        let source = gtk::Label::new(None);
        source.set_xalign(0.0);
        source.set_hexpand(true);
        source.set_wrap(true);
        source.add_css_class("rade-meta");
        let spinner = adw::Spinner::new();
        spinner.set_visible(false);
        let retry = gtk::Button::with_label("Retry");
        retry.add_css_class("flat");
        retry.set_visible(false);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        line.append(&source);
        line.append(&spinner);
        line.append(&retry);
        let hint = gtk::Label::new(None);
        hint.set_xalign(0.0);
        hint.set_wrap(true);
        hint.add_css_class("rade-code-hint");
        hint.set_visible(false);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        list.add_css_class("navigation-sidebar");
        list.add_css_class("rade-impact-list");
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_vexpand(true);
        scroll.set_child(Some(&list));
        let b = gtk::Box::new(gtk::Orientation::Vertical, 4);
        b.add_css_class("rade-impact");
        b.append(&summary);
        b.append(&line);
        b.append(&hint);
        b.append(&scroll);
        b.set_width_request(320);
        Self {
            toggle,
            panel: b,
            summary,
            source,
            hint,
            spinner,
            retry,
            list,
            running: Default::default(),
            stale: Default::default(),
            retry_queued: Default::default(),
        }
    }
}

fn esc(s: &str) -> String {
    glib::markup_escape_text(s).to_string()
}

fn tag(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("rade-code-textmatch");
    l
}

impl ReviewView {
    pub(super) fn connect_impact(self: &Rc<Self>) {
        let w = Rc::downgrade(self);
        self.impact_ui.toggle.connect_toggled(move |t| {
            let Some(v) = w.upgrade() else { return };
            if t.is_active() && v.state.borrow().impact.is_none() {
                v.compute_impact(Duration::ZERO);
            }
        });
        let w = Rc::downgrade(self);
        self.impact_ui.retry.connect_clicked(move |_| {
            if let Some(v) = w.upgrade() {
                v.compute_impact(BUDGET);
            }
        });
        let w = Rc::downgrade(self);
        self.impact_ui.list.connect_row_activated(move |_, row| {
            let Some(v) = w.upgrade() else { return };
            v.activate_impact(row.index() as usize);
        });
    }

    /// A row's place → the Tab's Viewer there.
    pub(super) fn activate_impact(&self, i: usize) {
        let at = self
            .state
            .borrow()
            .impact_rows
            .get(i)
            .and_then(|r| r.at.clone());
        if let Some((p, l, c)) = at {
            crate::code::jump_to(&self.id, &p, l, c);
        }
    }

    /// The diff changed: a shown Impact is computed again.
    pub(super) fn impact_diff_changed(self: &Rc<Self>) {
        if !self.impact_ui.toggle.is_active() {
            // Stale: computed again when shown.
            self.state.borrow_mut().impact = None;
            return;
        }
        if self.impact_ui.running.get() {
            self.impact_ui.stale.set(true);
        } else {
            self.compute_impact(Duration::ZERO);
        }
    }

    /// Compute it on a worker thread, waiting up to `wait` for a server
    /// that is still loading (0: text matches for its files, then the
    /// automatic retry waits).
    pub(super) fn compute_impact(self: &Rc<Self>, wait: Duration) {
        let Some((_, _, root)) = self.place() else {
            return;
        };
        if self.impact_ui.running.replace(true) {
            return;
        }
        let ui = &self.impact_ui;
        ui.spinner.set_visible(true);
        ui.retry.set_visible(false);
        if self.state.borrow().impact.is_none() {
            ui.summary.set_label("Impact");
            ui.source.set_label("Asking the language server…");
        }
        let w = Rc::downgrade(self);
        crate::worktrees::in_background(
            move || crate::code::impact_of(root, None, BUDGET, wait),
            move |r| {
                let Some(v) = w.upgrade() else { return };
                v.impact_ui.running.set(false);
                v.impact_ui.spinner.set_visible(false);
                match r {
                    Ok(i) => v.show_impact(i),
                    Err(e) => {
                        v.impact_ui.summary.set_label("Impact");
                        v.impact_ui.source.set_label(&e.message);
                        v.impact_ui.retry.set_visible(true);
                    }
                }
                if v.impact_ui.stale.replace(false) {
                    v.compute_impact(Duration::ZERO);
                }
            },
        );
    }

    fn show_impact(self: &Rc<Self>, i: Impact) {
        let ui = &self.impact_ui;
        ui.summary.set_label(&impact::summary(&i));
        ui.source.set_label(&impact::source_line(&i));
        ui.hint.set_label(i.hint.as_deref().unwrap_or(""));
        ui.hint.set_visible(i.hint.is_some());
        ui.retry
            .set_visible(matches!(i.state.as_str(), "indexing" | "partial"));
        let again = i.state == "indexing";
        while let Some(r) = ui.list.row_at_index(0) {
            ui.list.remove(&r);
        }
        let mut rows = Vec::new();
        for s in &i.symbols {
            let (sign, class) = match s.change.as_str() {
                "added" => ("+", "rade-git-added"),
                "removed" => ("−", "rade-git-deleted"),
                _ => ("~", "rade-git-modified"),
            };
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            let mark = gtk::Label::new(Some(sign));
            mark.add_css_class("rade-git-mark");
            mark.add_css_class(class);
            let name = gtk::Label::new(None);
            name.set_markup(&format!(
                "<b>{}</b>  <span alpha=\"60%\">{}</span>",
                esc(&s.name),
                esc(&s.kind)
            ));
            name.set_xalign(0.0);
            let at = gtk::Label::new(Some(&format!("{}:{}", nav::rel(&s.path, &i.root), s.line)));
            at.add_css_class("rade-meta");
            at.set_xalign(0.0);
            at.set_hexpand(true);
            at.set_ellipsize(gtk::pango::EllipsizeMode::Start);
            // The count; a removed symbol still used is the one to act on.
            let n = s.callers.len();
            let removed = s.change == "removed";
            let count = gtk::Label::new(Some(&match (removed, n) {
                (true, 0) => "no uses left".to_string(),
                (true, 1) => "1 use left".to_string(),
                (true, n) => format!("{n} uses left"),
                (false, 0) => String::new(),
                (false, 1) => "1 caller".to_string(),
                (false, n) => format!("{n} callers"),
            }));
            count.add_css_class("rade-search-count-pill");
            if removed && n > 0 {
                count.add_css_class("rade-impact-warn");
            }
            count.set_visible(!count.label().is_empty());
            b.append(&mark);
            b.append(&name);
            b.append(&at);
            if s.text && !removed {
                b.append(&tag("text match"));
            }
            b.append(&count);
            b.add_css_class("rade-impact-symbol");
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&b));
            ui.list.append(&row);
            let text = format!(
                "{sign} {} {} {}:{} {}",
                s.name,
                s.kind,
                nav::rel(&s.path, &i.root),
                s.line,
                count.label()
            )
            .trim_end()
            .to_string();
            // A removed one's place is at the base: nothing to open.
            let at = (s.change != "removed").then(|| (s.path.clone(), s.line, s.col));
            rows.push(ImpactRow { text, at });
            for c in &s.callers {
                let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                b.set_margin_start(22);
                // The code first, its place on the right (as the results popover).
                let at =
                    gtk::Label::new(Some(&format!("{}:{}", nav::rel(&c.path, &i.root), c.line)));
                at.add_css_class("rade-meta");
                at.set_xalign(1.0);
                at.set_ellipsize(gtk::pango::EllipsizeMode::Start);
                at.set_max_width_chars(28);
                let text = gtk::Label::new(None);
                text.set_markup(&match &c.name {
                    Some(n) => format!(
                        "<b>{}</b>  <span alpha=\"70%\">{}</span>",
                        esc(n),
                        esc(&c.preview)
                    ),
                    None => format!("<span alpha=\"70%\">{}</span>", esc(&c.preview)),
                });
                text.set_xalign(0.0);
                text.set_hexpand(true);
                text.set_ellipsize(gtk::pango::EllipsizeMode::End);
                text.add_css_class("monospace");
                b.append(&text);
                let mut tags = Vec::new();
                if c.test {
                    b.append(&tag("test"));
                    tags.push("test");
                }
                if c.text {
                    b.append(&tag("text match"));
                    tags.push("text match");
                }
                b.append(&at);
                let row = gtk::ListBoxRow::new();
                row.set_child(Some(&b));
                row.set_tooltip_text(Some(&c.preview));
                ui.list.append(&row);
                rows.push(ImpactRow {
                    text: format!(
                        "    {}:{} {}{} {}",
                        nav::rel(&c.path, &i.root),
                        c.line,
                        c.name
                            .as_deref()
                            .map(|n| format!("{n} "))
                            .unwrap_or_default(),
                        c.preview,
                        tags.iter()
                            .map(|t| format!("[{t}]"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    )
                    .trim_end()
                    .to_string(),
                    at: Some((c.path.clone(), c.line, c.col)),
                });
            }
        }
        {
            let mut st = self.state.borrow_mut();
            st.impact = Some(i);
            st.impact_rows = rows;
        }
        // A server still loading: ask again by itself while shown.
        if again && ui.toggle.is_active() && !ui.retry_queued.replace(true) {
            let w = Rc::downgrade(self);
            glib::timeout_add_local_once(Duration::from_secs(2), move || {
                if let Some(v) = w.upgrade() {
                    v.impact_ui.retry_queued.set(false);
                    if v.impact_ui.toggle.is_active() {
                        v.compute_impact(BUDGET);
                    }
                }
            });
        }
    }

    /// Window point of Impact row `i` (scrolled into view), for live clicks.
    pub(super) fn impact_locate(&self, i: usize) -> rade_core::rpc::Result<serde_json::Value> {
        use rade_core::rpc::Error;
        let row = self
            .impact_ui
            .list
            .row_at_index(i as i32)
            .ok_or_else(|| Error::not_found("no such row"))?;
        let ctx = glib::MainContext::default();
        while ctx.iteration(false) {}
        row.grab_focus();
        while ctx.iteration(false) {}
        let win = &crate::app::try_app().unwrap().window;
        let p = row
            .compute_point(
                win,
                &gtk::graphene::Point::new(60.0, row.height() as f32 / 2.0),
            )
            .ok_or_else(|| Error::internal("not on screen"))?;
        Ok(serde_json::json!({"row": i, "x": p.x(), "y": p.y()}))
    }

    /// `debug.review` info.
    pub(super) fn impact_info(&self) -> serde_json::Value {
        let st = self.state.borrow();
        serde_json::json!({
            "shown": self.impact_ui.toggle.is_active(),
            "running": self.impact_ui.running.get(),
            "summary": self.impact_ui.summary.label().as_str(),
            "source": self.impact_ui.source.label().as_str(),
            "retry": self.impact_ui.retry.is_visible(),
            "state": st.impact.as_ref().map(|i| i.state.clone()),
            "rows": st.impact_rows.iter().map(|r| r.text.clone()).collect::<Vec<_>>(),
        })
    }
}
