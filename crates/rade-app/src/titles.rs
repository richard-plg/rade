//! Terminal titles → the model (R1, docs/checks/M12-titles.md): sanitize
//! each OSC 0/2 title and take at most 4 per second per Pane, so Agent
//! spinners don't re-render GTK. Plus renaming a Tab (double-click on its
//! label, palette "Rename Tab…") and `debug.tabs` for the E2E harness.

use crate::app::try_app;
use adw::prelude::*;
use gtk::glib;
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::rpc::Result;
use rade_core::title;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Default)]
struct Slot {
    /// When this Pane's title last changed the model.
    last: Option<Instant>,
    /// Newest title not applied yet; `Some` while a flush is scheduled.
    pending: Option<String>,
}

thread_local! {
    static SLOTS: RefCell<HashMap<String, Slot>> = RefCell::default();
    /// The open rename entry (debug.tabs / debug.rename drive it).
    static RENAME: RefCell<Option<gtk::Entry>> = const { RefCell::new(None) };
    /// Why its last Enter was refused.
    static RENAME_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// A title from Ghostty (inside its callback: only schedules).
pub fn intake(pane: &str, raw: &str) {
    let Some(t) = title::sanitize(raw) else {
        return;
    };
    SLOTS.with(|s| {
        let mut s = s.borrow_mut();
        let slot = s.entry(pane.to_string()).or_default();
        if slot.pending.replace(t).is_some() {
            return; // a flush is already scheduled; it takes the newest
        }
        let pane = pane.to_string();
        glib::timeout_add_local_once(title::delay(slot.last, Instant::now()), move || {
            flush(&pane)
        });
    });
}

fn flush(pane: &str) {
    let Some(app) = try_app() else { return };
    let Some(t) = SLOTS.with(|s| s.borrow_mut().get_mut(pane).and_then(|x| x.pending.take()))
    else {
        return;
    };
    let current = app.model.borrow().pane(pane).map(|p| p.title.clone());
    match current {
        None => {
            SLOTS.with(|s| s.borrow_mut().remove(pane)); // Pane closed
        }
        // Spinner frames sanitize to the same title: nothing to redraw.
        Some(c) if c.as_deref() == Some(t.as_str()) => {}
        Some(_) => {
            SLOTS.with(|s| {
                if let Some(x) = s.borrow_mut().get_mut(pane) {
                    x.last = Some(Instant::now());
                }
            });
            app.update(|m| m.set_pane_title(pane, &t));
        }
    }
}

/// Tooltip markup of a Tab: its full title above the Agent Status summary.
pub fn tab_tooltip(full: Option<&str>, status: Option<&str>) -> String {
    let parts: Vec<String> = [full, status]
        .into_iter()
        .flatten()
        .map(|s| glib::markup_escape_text(s).to_string())
        .collect();
    parts.join("\n")
}

/// Rename Tab `tab`: an entry in a popover pointing at `at` (its label)
/// on `bar`. Enter applies; an empty name makes the label follow the
/// terminal title again; Esc cancels.
pub fn rename(bar: &gtk::Widget, tab: &str, at: Option<gtk::gdk::Rectangle>) {
    let Some(app) = try_app() else { return };
    let (name, label) = {
        let m = app.model.borrow();
        let Some(t) = m.worktrees().find_map(|(_, w)| w.tab(tab)) else {
            return;
        };
        (
            if t.renamed {
                t.name.clone()
            } else {
                String::new()
            },
            t.label(),
        )
    };
    let tab = tab.to_string();
    rename_popover(
        bar,
        at,
        &name,
        &label,
        "Empty: the Agent's or the Worktree's name",
        move |name| match try_app() {
            Some(app) => app.update(|m| m.rename_tab(&tab, name)),
            None => Ok(()),
        },
    );
}

/// A rename entry in a popover on `parent`, pointing at `at` (R1.8, R19.4).
/// Enter runs `apply`: done → closes; an error (a taken name) shows on the
/// entry, which stays open. Esc cancels.
pub fn rename_popover(
    parent: &gtk::Widget,
    at: Option<gtk::gdk::Rectangle>,
    text: &str,
    placeholder: &str,
    tip: &str,
    apply: impl Fn(&str) -> Result<()> + 'static,
) {
    let entry = gtk::Entry::new();
    entry.set_text(text);
    entry.set_placeholder_text(Some(placeholder));
    entry.set_tooltip_text(Some(tip));
    entry.set_width_chars(24);
    let pop = gtk::Popover::new();
    pop.set_child(Some(&entry));
    pop.set_parent(parent);
    pop.set_pointing_to(at.as_ref());
    pop.set_position(gtk::PositionType::Bottom);
    {
        let pop = pop.clone();
        entry.connect_activate(move |e| match apply(&e.text()) {
            Ok(()) => pop.popdown(),
            Err(err) => {
                e.add_css_class("error");
                e.set_tooltip_text(Some(&err.message));
                RENAME_ERROR.with(|r| *r.borrow_mut() = Some(err.message.clone()));
            }
        });
    }
    entry.connect_changed(|e| {
        e.remove_css_class("error");
        RENAME_ERROR.with(|r| r.borrow_mut().take());
    });
    pop.connect_closed(|p| {
        RENAME.with(|r| r.borrow_mut().take());
        let p = p.clone();
        // Unparent after the close animation's signal returns.
        glib::idle_add_local_once(move || p.unparent());
    });
    RENAME.with(|r| *r.borrow_mut() = Some(entry.clone()));
    RENAME_ERROR.with(|r| r.borrow_mut().take());
    pop.popup();
    entry.grab_focus();
}

/// `debug.rename` (`{"text"}`): type into the open rename entry and press
/// Enter. Result: whether one was open, still is, and its error.
pub fn debug_rename(p: &Value) -> Result<Value> {
    let entry = RENAME.with(|r| r.borrow().clone());
    let Some(entry) = entry else {
        return Err(rade_core::rpc::Error::usage("no rename entry is open"));
    };
    entry.set_text(p.get("text").and_then(Value::as_str).unwrap_or(""));
    entry.emit_activate();
    let error = RENAME_ERROR.with(|r| r.borrow().clone());
    Ok(json!({"applied": error.is_none(), "error": error}))
}

/// `debug.tabs`: the shown Worktree's Tab labels as the tab bar has them;
/// `{"op": "rename", "name"}` renames the selected Tab through the rename
/// entry (palette path), like typing the name and Enter.
pub fn debug(p: &Value) -> Result<Value> {
    if p.get("op").and_then(Value::as_str) == Some("rename") {
        crate::keys::run_action("rename_tab");
        let name = p.get("name").and_then(Value::as_str).unwrap_or("");
        let entry = RENAME.with(|r| r.borrow().clone());
        let Some(entry) = entry else {
            return Err(rade_core::rpc::Error::usage("no rename entry opened"));
        };
        entry.set_text(name);
        entry.emit_activate();
    }
    Ok(json!({
        "tabs": crate::tabs::debug_pages(),
        "selected_bounds": crate::tabs::selected_bounds(),
        "rename_open": RENAME.with(|r| r.borrow().is_some()),
    }))
}
