//! Notifications in the app (PRD R3): the panel at the bottom of the left
//! sidebar, where Terminal events become Notifications, and desktop
//! notifications through GIO. Decisions (dedupe, Unread counts, `smart`
//! visibility, newest unread) live in `rade_core::notify`.

use crate::app::{app, try_app, App};
use crate::panes::now;
use adw::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::notify::{desktop_decision, relative_time};
use rade_core::State;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Rows shown in the panel; older ones stay in state.json (cap 500).
/// ponytail: fixed window; add paging if anyone scrolls that far.
const PANEL_ROWS: usize = 100;
/// A banner is replaced no sooner than this after it was sent (see
/// [`Desktop::after_update`]).
const REPLACE_AFTER: std::time::Duration = std::time::Duration::from_millis(400);
/// Entries kept in the `rade debug desktop` log.
const LOG_MAX: usize = 200;

/// What one panel row shows; the panel is rebuilt when these change.
#[derive(Clone, PartialEq)]
struct RowData {
    id: u64,
    path: String,
    status: Option<rade_core::AgentStatus>,
    /// The live Pane's agent glyph (R115); none for a shell or a closed Pane.
    mark: Option<rade_core::design::AgentMark>,
    /// Its Preset's own icon (R115.5).
    icon: Option<std::path::PathBuf>,
    title: Option<String>,
    text: String,
    time: String,
    read: bool,
    gone: bool,
    /// The Agent's finished turn (or its "Ready for review") offers its
    /// Worktree's Review (R119.5): the Pane.
    review: Option<String>,
}

pub struct Panel {
    pub root: gtk::Box,
    list: gtk::ListBox,
    scroller: gtk::ScrolledWindow,
    empty: gtk::Label,
    arrow: gtk::Image,
    title: gtk::Label,
    ids: Rc<RefCell<Vec<u64>>>,
    rendered: RefCell<Option<Vec<RowData>>>,
}

fn icon_button(icon: &str, tooltip: &str, op: &'static str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.set_tooltip_text(Some(tooltip));
    b.add_css_class("flat");
    b.connect_clicked(move |_| {
        let _ = panel_op(op, None);
    });
    b
}

impl Panel {
    pub fn new() -> Self {
        // The Inbox (R3.2, M14): "Inbox · n", Mark all read, Clear.
        let arrow = gtk::Image::from_icon_name("pan-down-symbolic");
        let title = gtk::Label::new(Some("Inbox"));
        title.add_css_class("rade-inbox-title");
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        head.append(&arrow);
        head.append(&title);
        let toggle = gtk::Button::new();
        toggle.set_child(Some(&head));
        toggle.add_css_class("flat");
        toggle.set_hexpand(true);
        toggle.set_tooltip_text(Some("Show or hide the Inbox"));
        toggle.connect_clicked(|_| {
            let _ = panel_op("toggle", None);
        });
        let read = gtk::Button::with_label("Mark all read");
        read.add_css_class("flat");
        read.add_css_class("rade-link");
        read.set_tooltip_text(Some("Mark all Notifications read"));
        read.connect_clicked(|_| {
            let _ = panel_op("mark_all_read", None);
        });

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        header.add_css_class("rade-inbox-head");
        header.append(&toggle);
        header.append(&read);
        header.append(&icon_button(
            "edit-clear-all-symbolic",
            "Clear the Inbox",
            "clear",
        ));

        let list = gtk::ListBox::new();
        list.add_css_class("navigation-sidebar");
        list.add_css_class("rade-inbox");
        list.set_selection_mode(gtk::SelectionMode::None);
        let ids: Rc<RefCell<Vec<u64>>> = Rc::default();
        {
            let ids = ids.clone();
            list.connect_row_activated(move |_, row| {
                let id = ids.borrow().get(row.index() as usize).copied();
                if let Some(id) = id {
                    let _ = panel_op("activate", Some(id));
                }
            });
        }
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroller.set_propagate_natural_height(true);
        scroller.set_max_content_height(150);
        scroller.set_child(Some(&list));

        let empty = gtk::Label::new(Some("No notifications"));
        empty.add_css_class("dim-label");
        empty.add_css_class("caption");
        empty.set_margin_top(6);
        empty.set_margin_bottom(10);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        root.append(&header);
        root.append(&scroller);
        root.append(&empty);

        // Keep relative times fresh.
        glib::timeout_add_seconds_local(30, || {
            if let Some(app) = try_app() {
                app.sidebar.panel.sync(&app.model.borrow());
            }
            glib::ControlFlow::Continue
        });

        Self {
            root,
            list,
            scroller,
            empty,
            arrow,
            title,
            ids,
            rendered: RefCell::default(),
        }
    }

    pub fn sync(&self, ws: &State) {
        // The shown Workspace's (and closed Panes') Notifications (R23.4).
        let shown = |n: &rade_core::notify::Notification| {
            !ws.pane_exists(&n.pane) || ws.pane_visible(&n.pane)
        };
        let total = ws
            .notifications
            .iter()
            .filter(|n| !n.read && shown(n))
            .count();
        self.title.set_label(&if total > 0 {
            format!("Inbox · {total}")
        } else {
            "Inbox".into()
        });
        let collapsed = ws.notifications_collapsed;
        self.arrow.set_icon_name(Some(if collapsed {
            "pan-end-symbolic"
        } else {
            "pan-down-symbolic"
        }));
        let any = ws.notifications.iter().any(shown);
        self.scroller.set_visible(!collapsed && any);
        self.empty.set_visible(!collapsed && !any);

        let t = now();
        let rows: Vec<RowData> = ws
            .notifications
            .iter()
            .filter(|n| shown(n))
            .rev()
            .take(PANEL_ROWS)
            .map(|n| {
                let live = ws.pane_path(&n.pane);
                RowData {
                    id: n.id,
                    gone: live.is_none(),
                    path: live.unwrap_or_else(|| format!("{} (closed)", n.path)),
                    status: n.status_kind(),
                    mark: ws.pane(&n.pane).and_then(|p| p.mark()),
                    icon: crate::agent_glyph::icon_of(
                        ws.pane(&n.pane).and_then(|p| p.agent_name()),
                    ),
                    title: n.title.clone(),
                    text: n.text.clone(),
                    time: relative_time(n.time, t),
                    read: n.read,
                    review: (n.status_kind() == Some(rade_core::AgentStatus::Done)
                        || n.title.as_deref() == Some("Ready for review"))
                    .then(|| ws.review_target(&n.pane).map(|_| n.pane.clone()))
                    .flatten(),
                }
            })
            .collect();
        if self.rendered.borrow().as_ref() == Some(&rows) {
            return;
        }
        while let Some(r) = self.list.row_at_index(0) {
            self.list.remove(&r);
        }
        for r in &rows {
            self.list.append(&row(r));
        }
        *self.ids.borrow_mut() = rows.iter().map(|r| r.id).collect();
        *self.rendered.borrow_mut() = Some(rows);
    }
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

impl Panel {
    /// `debug.ui` `inbox`: the header and rows as shown.
    pub fn debug(&self) -> Value {
        let rows = self.rendered.borrow();
        json!({
            "title": self.title.label().as_str(),
            "rows": rows.iter().flatten().map(|r| json!({
                "id": r.id, "glyph": r.status.map(|s| s.as_str()),
                "mark": r.mark.map(|m| m.as_str()), "text": r.text,
                "path": breadcrumb(&r.path), "unread": !r.read, "review": r.review.is_some(),
            })).collect::<Vec<_>>(),
        })
    }
}

/// `Source › Worktree` of a "Source / Worktree / Tab / Pane" path.
fn breadcrumb(path: &str) -> String {
    path.split(" / ").take(2).collect::<Vec<_>>().join(" › ")
}

fn row(r: &RowData) -> gtk::ListBoxRow {
    let text = match &r.title {
        Some(t) => format!("{t}: {}", r.text),
        None => r.text.clone(),
    };
    let body = label(&text, &["rade-notif-text"]);
    body.set_hexpand(true);
    body.set_wrap(true);
    body.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    body.set_lines(2);
    body.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let time = label(&r.time, &["rade-notif-time"]);
    time.set_valign(gtk::Align::Start);
    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class("rade-unread-dot");
    dot.set_valign(gtk::Align::Start);
    dot.set_margin_top(4);
    dot.set_visible(!r.read);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    top.append(&body);
    if let Some(pane) = &r.review {
        let b = gtk::Button::with_label("Review");
        b.add_css_class("flat");
        b.add_css_class("rade-notif-review");
        b.set_valign(gtk::Align::Start);
        let pane = pane.clone();
        b.connect_clicked(move |_| crate::review::open_for(&pane));
        top.append(&b);
    }
    top.append(&time);
    top.append(&dot);
    // Never cut from the left: the Source comes first (R3.2).
    let path = label(&breadcrumb(&r.path), &["rade-notif-path"]);
    path.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let text_col = gtk::Box::new(gtk::Orientation::Vertical, 1);
    text_col.set_hexpand(true);
    text_col.append(&top);
    text_col.append(&path);
    // An Agent's glyph in the event's colour (R115), else the status glyph.
    let glyph: gtk::Widget = match r.mark {
        Some(m) => {
            let g = crate::agent_glyph::Glyph::new(m, r.status);
            g.set(m, r.icon.as_deref(), r.status);
            g.root.upcast()
        }
        None => {
            let g = crate::status::Glyph::of(r.status);
            g.root
                .set_opacity(if r.status.is_some() { 1.0 } else { 0.0 });
            g.root.upcast()
        }
    };
    glyph.set_valign(gtk::Align::Start);
    glyph.set_margin_top(2);
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    b.append(&glyph);
    b.append(&text_col);
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&b));
    row.add_css_class("rade-notif");
    row.set_tooltip_text(Some(&r.path));
    if !r.read {
        row.add_css_class("rade-notif-unread");
    }
    if r.gone {
        row.add_css_class("rade-notif-gone");
        row.set_tooltip_text(Some(&format!("{}\nThis Pane was closed", r.path)));
    }
    row
}

#[cfg(test)]
mod tests {
    #[test]
    fn breadcrumb_keeps_the_source_first() {
        assert_eq!(
            super::breadcrumb("payments-api / feat-refunds / Tab 1 / codex-tests"),
            "payments-api › feat-refunds"
        );
        assert_eq!(super::breadcrumb("General / main"), "General › main");
    }
}

/// The panel's buttons and rows (`rade debug panel` drives the same code).
pub fn panel_op(op: &str, id: Option<u64>) -> rade_core::rpc::Result<Value> {
    let app = app();
    match (op, id) {
        // R119.5: a row's Review button.
        ("review", Some(id)) => {
            let pane = app
                .model
                .borrow()
                .notifications
                .iter()
                .find(|n| n.id == id)
                .map(|n| n.pane.clone())
                .ok_or_else(|| rade_core::rpc::Error::not_found(format!("no notification {id}")))?;
            crate::review::open_for(&pane);
            Ok(json!({"pane": pane}))
        }
        ("activate", Some(id)) => {
            let pane = app.update(|m| m.activate_notification(id))?;
            Ok(json!({"pane": pane}))
        }
        ("toggle", _) => {
            let c = app.update(|m| {
                m.notifications_collapsed ^= true;
                m.notifications_collapsed
            });
            Ok(json!({"collapsed": c}))
        }
        ("mark_all_read", _) => {
            app.update(|m| m.mark_all_read());
            Ok(json!({}))
        }
        // Unread counts as the badges show them.
        ("counts", _) => {
            let m = app.model.borrow();
            let u = m.unread_counts();
            let (mut panes, mut tabs, mut wts, mut sources) = Default::default();
            let put = |map: &mut serde_json::Map<String, Value>, id: &str, n: u32| {
                if n > 0 {
                    map.insert(id.into(), n.into());
                }
            };
            for (p, w) in m.worktrees() {
                put(&mut sources, &p.id, u.source(&p.id));
                put(&mut wts, &w.id, u.worktree(&w.id));
                for t in &w.tabs {
                    put(&mut tabs, &t.id, u.tab(&t.id));
                    for x in t.panes() {
                        put(&mut panes, &x.id, u.pane(&x.id));
                    }
                }
            }
            Ok(json!({"total": u.total, "panes": panes, "tabs": tabs,
                      "worktrees": wts, "sources": sources}))
        }
        ("clear", _) => {
            app.update(|m| m.clear_notifications());
            Ok(json!({}))
        }
        _ => Err(rade_core::Error::usage(format!(
            "panel op must be activate <id>|toggle|mark_all_read|clear|counts, not '{op}'"
        ))),
    }
}

/// Ctrl+Shift+G: open the newest unread Notification (R3.3).
pub fn jump_unread() {
    let app = app();
    let id = app.model.borrow().newest_unread();
    if let Some(id) = id {
        let _ = app.update(|m| m.activate_notification(id));
    }
}

/// OSC 9 / 777 / 99 from a Terminal (Ghostty's desktop-notification action).
pub fn from_terminal(pane: &str, title: &str, body: &str) {
    let (title, text) = match (title.trim(), body.trim()) {
        ("", "") => return,
        (t, "") => (None, t.to_string()),
        ("", b) => (None, b.to_string()),
        (t, b) => (Some(t.to_string()), b.to_string()),
    };
    // The Agent's own words, quoted by its Activity event (R107.2).
    crate::activity_view::message(pane, &text);
    post_later(pane.into(), Some((title, text)));
}

/// BEL: a Notification and an Attention signal for the status fallbacks
/// (R10.2), unless the Pane is the focused one on screen.
pub fn bell(pane: &str) {
    post_later(pane.into(), None);
}

/// Ghostty calls back mid-tick: change the model from the main loop.
/// `message` = OSC (title, text); `None` = BEL. The fallback rule runs first
/// so its status Notification and this one coalesce into one entry (R3.1).
fn post_later(pane: String, message: Option<(Option<String>, String)>) {
    glib::idle_add_local_once(move || {
        let Some(app) = try_app() else { return };
        if !app.model.borrow().pane_exists(&pane) {
            return;
        }
        // A bell from the focused Pane on screen is nothing (R3.1), not
        // even an Attention signal: its fallback status would notify.
        if message.is_none() && app.is_focused_visible(&pane) {
            return;
        }
        crate::status::signal(&pane, rade_core::status::Signal::Attention);
        app.update(|m| match message {
            Some((title, text)) => m.notify(&pane, title, &text, now()),
            None => m.notify_bell(&pane, now()),
        });
    });
}

/// Desktop side, run after every model update: send new Notifications per
/// `desktop_notify` (R3.5), withdraw sent ones that got read in the app.
#[derive(Default)]
pub struct Desktop {
    /// Highest Notification id already considered.
    seen: std::cell::Cell<u64>,
    /// Sent and not yet withdrawn, with the text sent and when.
    sent: RefCell<Vec<(u64, String, std::time::Instant)>>,
    /// A replacement is waiting for the server's answer to the first send.
    retry: std::cell::Cell<bool>,
    log: RefCell<Vec<Value>>,
}

impl Desktop {
    /// Old Notifications (from state.json) never go to the desktop.
    pub fn start_after(&self, ws: &State) {
        self.seen.set(ws.notifications.last().map_or(0, |n| n.id));
    }

    pub fn after_update(&self, app: &App) {
        let ws = app.model.borrow();
        let mode = app.config.borrow().desktop_notify;
        let window_active = app.window.is_active();
        for n in ws.notifications.iter().filter(|n| n.id > self.seen.get()) {
            let on_screen = ws.pane_on_screen(&n.pane);
            let (send, reason) = desktop_decision(mode, window_active, on_screen);
            self.push(json!({
                "event": if send { "send" } else { "skip" }, "id": n.id, "pane": n.pane,
                "reason": reason, "window_active": window_active, "on_screen": on_screen,
            }));
            if send {
                show_desktop(app, &ws, n);
                self.sent
                    .borrow_mut()
                    .push((n.id, n.text.clone(), std::time::Instant::now()));
            }
        }
        // A sent one that grew by coalescing (R3.1): replace its banner (same
        // id), don't send a second.
        // GLib replaces a banner only once the server answered its first
        // send: a replacement right after it would be a second banner, so
        // it waits a moment (an OSC arriving with its status hook).
        for (id, text, at) in self.sent.borrow_mut().iter_mut() {
            if let Some(n) = ws.notification(*id).filter(|n| !n.read && n.text != *text) {
                if at.elapsed() < REPLACE_AFTER {
                    if !self.retry.replace(true) {
                        glib::timeout_add_local_once(REPLACE_AFTER, || {
                            if let Some(app) = try_app() {
                                app.desktop.retry.set(false);
                                app.desktop.after_update(&app);
                            }
                        });
                    }
                    continue;
                }
                show_desktop(app, &ws, n);
                *text = n.text.clone();
                self.push(json!({"event": "replace", "id": id, "pane": n.pane}));
            }
        }
        self.seen.set(
            self.seen
                .get()
                .max(ws.notifications.last().map_or(0, |n| n.id)),
        );
        // Read in the app (or cleared) → gone from the desktop too.
        let done: Vec<u64> = self
            .sent
            .borrow()
            .iter()
            .map(|(id, _, _)| *id)
            .filter(|id| ws.notification(*id).is_none_or(|n| n.read))
            .collect();
        for id in done {
            app.gtk_app.withdraw_notification(&format!("n-{id}"));
            self.sent.borrow_mut().retain(|(x, _, _)| *x != id);
            self.push(json!({"event": "withdraw", "id": id}));
        }
    }

    fn push(&self, v: Value) {
        let mut log = self.log.borrow_mut();
        log.push(v);
        let extra = log.len().saturating_sub(LOG_MAX);
        log.drain(..extra);
    }

    /// `rade debug desktop`: what was sent, skipped, replaced, withdrawn, and why.
    pub fn log(&self) -> Value {
        let sent: Vec<u64> = self.sent.borrow().iter().map(|(id, _, _)| *id).collect();
        json!({"log": *self.log.borrow(), "sent": sent})
    }
}

/// Send (or, with the same id, replace) the desktop notification for `n`.
fn show_desktop(app: &App, ws: &State, n: &rade_core::notify::Notification) {
    let path = ws.pane_path(&n.pane).unwrap_or_default();
    let (summary, body) = match &n.title {
        Some(t) => (t.clone(), format!("{}\n{path}", n.text)),
        None => (path, n.text.clone()),
    };
    // From a hidden Workspace: its name leads the title (R23.6).
    let summary = match ws.hidden_workspace_name(&n.pane) {
        Some(w) => format!("[{w}] {summary}"),
        None => summary,
    };
    let g = gio::Notification::new(&summary);
    g.set_body(Some(&body));
    let target = n.pane.to_variant();
    g.set_default_action_and_target_value("app.focus-pane", Some(&target));
    g.add_button_with_target_value("Focus", "app.focus-pane", Some(&target));
    app.gtk_app
        .send_notification(Some(&format!("n-{}", n.id)), &g);
}

/// `app.focus-pane(<pane>)`: the desktop notification's default action and
/// its "Focus" button. Raises RADE and focuses the Pane (reads it); a gone
/// Pane's Notifications are just marked read.
pub fn install_focus_action(gtk_app: &adw::Application) {
    let action = gio::SimpleAction::new("focus-pane", Some(glib::VariantTy::STRING));
    action.connect_activate(|_, target| {
        let Some(pane) = target.and_then(|v| v.str().map(str::to_string)) else {
            return;
        };
        let Some(app) = try_app() else { return };
        app.window.present();
        app.update(|m| {
            if m.focus_pane(&pane).is_err() {
                m.mark_read(&pane);
            }
        });
    });
    gtk_app.add_action(&action);
}

/// `desktop_notify` goes through the freedesktop notification server
/// (org.freedesktop.Notifications, with actions), also when installed
/// (M10 decision): GLib's GNOME backend needs a unique bus name, and RADE
/// stays NON_UNIQUE. GLib sends the app id as the `desktop-entry` hint, so
/// the installed app still shows as RADE (`dev.rade.App.desktop`) in GNOME.
/// `GNOTIFICATION_BACKEND` overrides.
pub fn choose_backend() {
    if std::env::var_os("GNOTIFICATION_BACKEND").is_none() {
        std::env::set_var("GNOTIFICATION_BACKEND", "freedesktop"); // before threads
    }
}
