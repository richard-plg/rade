//! Workspace switcher (R23.5): the sidebar's section button — the shown
//! Workspace's color dot and name, an amber `!` while an Agent in a hidden
//! Workspace is `waiting` — and its popover listing every Workspace with
//! its `Ctrl+Alt+<n>` key, Unread count and worst Agent Status glyph.

use crate::app::try_app;
use crate::status::Glyph;
use gtk::gio;
use gtk::prelude::*;
use gtk4 as gtk;
use rade_core::workspace::{default_name, Look};
use rade_core::{AgentStatus, State};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// One popover row as shown.
#[derive(Clone, Debug, PartialEq)]
struct RowData {
    look: Look,
    active: bool,
    unread: u32,
    status: Option<AgentStatus>,
}

pub struct Switcher {
    pub button: gtk::MenuButton,
    dot: gtk::Label,
    name: gtk::Label,
    badge: Glyph,
    list: gtk::ListBox,
    popover: gtk::Popover,
    ids: Rc<RefCell<Vec<String>>>,
    rendered: RefCell<Option<Vec<RowData>>>,
}

fn dot_markup(color: &str) -> String {
    // A square, not a disc: status glyphs are discs (R20.2).
    format!("<span foreground=\"{color}\">■</span>")
}

impl Switcher {
    pub fn new() -> Self {
        // The Workspace mark: its initial on flat `acc` (spec A.6).
        let dot = gtk::Label::new(None);
        dot.add_css_class("ws-mark");
        dot.set_valign(gtk::Align::Center);
        let name = gtk::Label::new(None);
        name.add_css_class("ws-name");
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        // The `waiting` glyph itself (shape + color, R20.2): the one
        // signal an Agent needs input, here in a hidden Workspace.
        let badge = Glyph::of(Some(AgentStatus::Waiting));
        badge.root.add_css_class("rade-ws-badge");
        badge.root.set_visible(false);
        // The `!` opens Agent Overview on Needs you (R117.1), not the list.
        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(|g, _, _, _| {
            g.set_state(gtk::EventSequenceState::Claimed);
            crate::overview_view::open_band(Some(rade_core::overview::Band::NeedsYou));
        });
        badge.root.add_controller(click);
        let arrow = gtk::Image::from_icon_name("ph-caret-up-down-symbolic");
        arrow.add_css_class("ws-caret");
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        content.append(&dot);
        content.append(&name);
        content.append(&badge.root);
        content.append(&arrow);

        let list = gtk::ListBox::new();
        list.add_css_class("navigation-sidebar");
        list.add_css_class("rade-ws-list");
        list.set_selection_mode(gtk::SelectionMode::None);
        let ids: Rc<RefCell<Vec<String>>> = Rc::default();
        let popover = gtk::Popover::new();
        {
            let (ids, popover) = (ids.clone(), popover.clone());
            list.connect_row_activated(move |_, row| {
                let id = ids.borrow().get(row.index() as usize).cloned();
                popover.popdown();
                if let (Some(id), Some(app)) = (id, try_app()) {
                    app.switch_workspace(&id);
                }
            });
        }
        let add = gtk::Button::with_label("Add Workspace…");
        add.add_css_class("flat");
        add.set_tooltip_text(Some(
            "A folder that groups Sources under one identity (.rade/workspace.toml)",
        ));
        {
            let popover = popover.clone();
            add.connect_clicked(move |_| {
                popover.popdown();
                add_workspace_dialog();
            });
        }
        let body = gtk::Box::new(gtk::Orientation::Vertical, 4);
        let head = gtk::Label::new(Some("Workspaces"));
        head.add_css_class("rade-section");
        head.set_xalign(0.0);
        body.append(&head);
        body.append(&list);
        body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        body.append(&add);
        popover.set_child(Some(&body));
        popover.add_css_class("rade-ws-popover");

        let button = gtk::MenuButton::new();
        button.set_child(Some(&content));
        button.set_popover(Some(&popover));
        button.add_css_class("flat");
        button.add_css_class("rade-switcher");
        button.add_css_class("ws-button");
        button.set_valign(gtk::Align::Center);
        button.set_tooltip_text(Some("Switch Workspace (Ctrl+Alt+1…9)"));
        Self {
            button,
            dot,
            name,
            badge,
            list,
            popover,
            ids,
            rendered: RefCell::default(),
        }
    }

    pub fn sync(&self, ws: &State) {
        let looks = ws.workspace_looks();
        let shown = ws.active_workspace().to_string();
        if let Some(l) = looks.iter().find(|l| l.id == shown) {
            let initial: String = l
                .name
                .chars()
                .next()
                .into_iter()
                .flat_map(char::to_uppercase)
                .collect();
            self.dot.set_label(&initial);
            self.name.set_label(&l.name);
        }
        // `!`: an Agent in a hidden Workspace needs input (R23.5).
        let waiting = ws.hidden_waiting();
        self.badge.root.set_visible(!waiting.is_empty());
        if !waiting.is_empty() {
            let names: Vec<String> = waiting
                .iter()
                .filter_map(|p| {
                    let name = ws.pane(p)?.name.clone().unwrap_or_else(|| p.clone());
                    Some(format!(
                        "{name} needs input in {}",
                        ws.hidden_workspace_name(p)?
                    ))
                })
                .collect();
            let text = names.join("\n");
            self.badge.root.set_tooltip_text(Some(&text));
            self.badge
                .root
                .update_property(&[gtk::accessible::Property::Label(&text)]);
        }
        let rows: Vec<RowData> = looks
            .into_iter()
            .map(|look| {
                let sum = ws.workspace_summary(&look.id);
                RowData {
                    active: look.id == shown,
                    unread: sum.unread,
                    status: sum.status,
                    look,
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
        *self.ids.borrow_mut() = rows.iter().map(|r| r.look.id.clone()).collect();
        *self.rendered.borrow_mut() = Some(rows);
    }

    /// `debug.ui` `workspaces`: the button and the popover's rows as shown.
    pub fn debug(&self) -> Value {
        let rows = self.rendered.borrow();
        json!({
            "name": self.name.label().as_str(),
            "badge": self.badge.root.is_visible(),
            "badge_tooltip": self.badge.root.tooltip_text().map(|t| t.to_string()),
            "open": self.popover.is_visible(),
            "rows": rows.iter().flatten().map(|r| json!({
                "workspace": r.look.id, "name": r.look.name, "color": r.look.color,
                "key": r.look.key, "active": r.active, "unread": r.unread,
                "glyph": r.status.map(|s| s.as_str()),
            })).collect::<Vec<_>>(),
        })
    }

    pub fn shot(&self, path: &std::path::Path) -> rade_core::rpc::Result<()> {
        crate::debug::screenshot_widget(self.popover.upcast_ref(), path)
    }

    /// `debug.ui` `switcher`: `open` / `close` the popover, or click the
    /// row of Workspace `op`.
    pub fn debug_op(&self, op: &str) -> bool {
        match op {
            // Without a grab: headless Weston has no seat (as `debug.menu`).
            "open" => {
                self.popover.set_autohide(false);
                self.popover.popup();
            }
            "close" => {
                self.popover.popdown();
                self.popover.set_autohide(true);
            }
            ws => {
                let i = self.ids.borrow().iter().position(|w| w == ws);
                let Some(r) = i.and_then(|i| self.list.row_at_index(i as i32)) else {
                    return false;
                };
                r.emit_by_name::<()>("activate", &[]);
            }
        }
        true
    }
}

fn row(r: &RowData) -> gtk::ListBoxRow {
    let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let dot = gtk::Label::new(None);
    dot.set_markup(&dot_markup(&r.look.color));
    let name = gtk::Label::new(Some(&r.look.name));
    name.set_xalign(0.0);
    name.set_hexpand(true);
    name.add_css_class("rade-row-title");
    if r.active {
        name.add_css_class("rade-ws-active");
        name.update_property(&[gtk::accessible::Property::Description("shown")]);
    }
    hbox.append(&dot);
    hbox.append(&name);
    let glyph = Glyph::of(r.status);
    glyph.root.set_visible(r.status.is_some());
    hbox.append(&glyph.root);
    let count = gtk::Label::new(Some(&r.unread.to_string()));
    count.add_css_class("rade-count");
    count.set_valign(gtk::Align::Center);
    count.set_visible(r.unread > 0);
    count.set_tooltip_text(Some(&format!("{} unread", r.unread)));
    hbox.append(&count);
    // The shown one: a check, as libadwaita marks a selected item.
    let check = gtk::Image::from_icon_name("object-select-symbolic");
    check.add_css_class("rade-dim");
    check.set_opacity(if r.active { 1.0 } else { 0.0 });
    hbox.append(&check);
    let key = gtk::Label::new(r.look.key.as_deref());
    key.add_css_class("rade-meta");
    hbox.append(&key);
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&hbox));
    row.add_css_class(&format!("rade-ws-{}", r.look.id));
    row.set_tooltip_text(Some(&format!("Show {}", r.look.name)));
    row
}

/// Add Workspace via a folder picker (R23.1): named after its
/// `workspace.toml`, else the folder.
fn add_workspace_dialog() {
    let Some(app) = try_app() else { return };
    let dialog = gtk::FileDialog::builder()
        .title("Add Workspace")
        .modal(true)
        .build();
    dialog.select_folder(Some(&app.window), gio::Cancellable::NONE, |res| {
        let Some(root) = res.ok().and_then(|f| f.path()) else {
            return;
        };
        let Some(app) = try_app() else { return };
        let name = default_name(&root, None);
        match app.update(|m| m.add_workspace(&root, &name)) {
            Ok((id, _)) => app.switch_workspace(&id),
            Err(e) => crate::worktrees::alert("Could not add the Workspace", &e.message),
        }
    });
}
