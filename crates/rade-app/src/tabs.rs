//! One Worktree's Tabs: RADE's flat tab strip (PRD R103.2, spec A.39–A.44)
//! over an `adw::TabView` that holds the pages, kept in step with the
//! model. Switching Sources swaps whole views, so each Worktree keeps its
//! exact Tabs, Layout, and focus (R1-c).

use crate::app::try_app;
use crate::layout_view;
use adw::prelude::*;
use gtk::{gdk, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::fanout::Fanout;
use rade_core::notify::UnreadCounts;
use rade_core::{AgentStatus, Worktree};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

/// One tab in the strip (spec A.40–A.43).
struct TabWidget {
    /// The tab with its 2 px active bar on top.
    root: gtk::Overlay,
    tab: gtk::Box,
    bar: gtk::Box,
    /// The lead Agent's glyph in the Tab's status colour (R115).
    glyph: crate::agent_glyph::Glyph,
    /// The other kinds' glyphs (R115.3), 12 px, shown from the 2nd kind.
    extra: [crate::agent_glyph::Glyph; 2],
    icon: gtk::Image,
    label: gtk::Label,
    count: gtk::Label,
}

struct TabEntry {
    id: String,
    page: adw::TabPage,
    /// The page: the Fan-out bar (if any) above `container`.
    outer: gtk::Box,
    container: gtk::Box,
    signature: String,
    captions: layout_view::Captions,
    widget: TabWidget,
    /// What `debug.tabs` reports: LED status, label, Agent count, kind.
    shown: Shown,
}

#[derive(Clone, Default)]
struct Shown {
    status: Option<AgentStatus>,
    led: Option<&'static str>,
    mark: Option<rade_core::design::AgentMark>,
    marks: Vec<&'static str>,
    count: Option<usize>,
    kind: &'static str,
}

type Entries = Rc<RefCell<Vec<TabEntry>>>;

/// Each Worktree's tab strip, view, and pages, for renaming the shown Tab
/// from the palette and `debug.tabs` (R1).
type Registered = (
    glib::WeakRef<gtk::Box>,
    glib::WeakRef<adw::TabView>,
    Weak<RefCell<Vec<TabEntry>>>,
);
thread_local! {
    static REGISTRY: RefCell<HashMap<String, Registered>> = RefCell::default();
}

/// The shown Worktree's tab strip, view, and pages.
fn shown() -> Option<(gtk::Box, adw::TabView, Entries)> {
    let wt = try_app()?.model.borrow().active().1.id.clone();
    REGISTRY.with(|r| {
        let r = r.borrow();
        let (bar, view, entries) = r.get(&wt)?;
        Some((bar.upgrade()?, view.upgrade()?, entries.upgrade()?))
    })
}

/// The selected tab's widget in the shown strip.
fn selected_tab(entries: &Entries, view: &adw::TabView) -> Option<gtk::Widget> {
    let sel = view.selected_page()?;
    entries
        .borrow()
        .iter()
        .find(|e| e.page == sel)
        .map(|e| e.widget.root.clone().upcast())
}

/// Rename the shown Worktree's selected Tab (double-click on it, palette
/// "Rename Tab…"): an entry pointing at the tab.
pub fn rename_selected() {
    let Some((bar, view, entries)) = shown() else {
        return;
    };
    let Some(tab) = view.selected_page().and_then(|p| tab_id_of(&entries, &p)) else {
        return;
    };
    let at = selected_tab(&entries, &view)
        .and_then(|w| w.compute_bounds(&bar))
        .map(|b| {
            gdk::Rectangle::new(
                b.x() as i32,
                b.y() as i32,
                b.width() as i32,
                b.height() as i32,
            )
        });
    crate::titles::rename(bar.upcast_ref(), &tab, at);
}

/// The selected tab's bounds in window coordinates (live harness clicks).
pub fn selected_bounds() -> Option<serde_json::Value> {
    let (bar, view, entries) = shown()?;
    let b = selected_tab(&entries, &view)?.compute_bounds(&bar.root()?)?;
    Some(serde_json::json!({"x": b.x(), "y": b.y(), "width": b.width(), "height": b.height()}))
}

/// `debug.tabs`: the shown Worktree's tabs as the strip shows them.
pub fn debug_pages() -> Vec<serde_json::Value> {
    let Some((_, view, entries)) = shown() else {
        return Vec::new();
    };
    let selected = view.selected_page();
    let pages = entries
        .borrow()
        .iter()
        .map(|e| {
            let s = &e.shown;
            serde_json::json!({
                "tab": e.id, "label": e.widget.label.label().as_str(),
                "tooltip": e.widget.tab.tooltip_markup().map(|t| t.to_string()),
                "selected": selected.as_ref() == Some(&e.page),
                "needs_attention": s.status == Some(AgentStatus::Waiting),
                "loading": s.status == Some(AgentStatus::Running),
                "icon": s.status.filter(|s| *s != AgentStatus::Running).map(|s| s.as_str()),
                "led": s.led,
                "glyph": s.mark.map(|m| m.as_str()),
                "marks": s.marks,
                "count": s.count,
                "kind": s.kind,
            })
        })
        .collect();
    pages
}

/// A Pane's header text as shown (`debug.ui pane`, R103.3).
pub fn pane_header(pane: &str) -> Option<String> {
    REGISTRY.with(|r| {
        r.borrow().values().find_map(|(_, _, entries)| {
            let entries = entries.upgrade()?;
            let entries = entries.borrow();
            entries.iter().find_map(|e| e.captions.header(pane))
        })
    })
}

/// A Pane's header ✕ as built (`debug.ui pane` / `pane_close`, R19.9).
pub fn pane_close_button(pane: &str) -> Option<gtk::Button> {
    REGISTRY.with(|r| {
        r.borrow().values().find_map(|(_, _, entries)| {
            let entries = entries.upgrade()?;
            let entries = entries.borrow();
            entries.iter().find_map(|e| e.captions.close_button(pane))
        })
    })
}

/// A Pane's header glyph as shown (`debug.ui pane`, R115).
pub fn pane_glyph(pane: &str) -> Option<serde_json::Value> {
    REGISTRY.with(|r| {
        r.borrow().values().find_map(|(_, _, entries)| {
            let entries = entries.upgrade()?;
            let entries = entries.borrow();
            entries.iter().find_map(|e| e.captions.glyph(pane))
        })
    })
}

/// Whether a Pane is dimmed as built (`debug.ui`; the Attention Ring is retired, ADR-0036).
pub fn pane_overlays(pane: &str) -> Option<bool> {
    REGISTRY.with(|r| {
        r.borrow().values().find_map(|(_, _, entries)| {
            let entries = entries.upgrade()?;
            let entries = entries.borrow();
            entries.iter().find_map(|e| e.captions.dimmed(pane))
        })
    })
}

/// Close Tab `id` as its close button does: a Fan-out Tab asks to discard
/// the Fan-out first (R15), any other asks before closing (R19.9).
fn close_tab(id: &str) {
    let id = id.to_string();
    glib::idle_add_local_once(move || {
        if try_app().is_some() {
            crate::menus::close_tabs_ui(&id, rade_core::basic::CloseMode::Only);
        }
    });
}

/// A new tab widget for Tab `id`: click selects, double-click renames,
/// middle-click closes, right-click opens the Tab menu (R19.1), drag
/// reorders.
fn tab_widget(id: &str, strip: &gtk::Box) -> TabWidget {
    let tab = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    tab.add_css_class("tab");
    let glyph = crate::agent_glyph::Glyph::new(rade_core::design::AgentMark::Other, None);
    glyph.add_css_class("tab-glyph");
    let icon = gtk::Image::new();
    icon.add_css_class("tab-icon");
    let label = gtk::Label::new(None);
    label.add_css_class("tab-label");
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_max_width_chars(28);
    let count = gtk::Label::new(None);
    count.add_css_class("tab-count");
    let close = gtk::Button::from_icon_name("ph-x-symbolic");
    close.add_css_class("tab-close");
    close.add_css_class("flat");
    close.set_valign(gtk::Align::Center);
    close.set_focus_on_click(false);
    close.set_tooltip_text(Some("Close Tab"));
    {
        let id = id.to_string();
        close.connect_clicked(move |_| close_tab(&id));
    }
    let extra = [0, 1].map(|_| {
        let g = crate::agent_glyph::Glyph::new(rade_core::design::AgentMark::Other, None);
        g.add_css_class("tab-glyph-extra");
        g.root.set_visible(false);
        g
    });
    let marks = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    marks.append(&glyph.root);
    for g in &extra {
        marks.append(&g.root);
    }
    tab.append(&marks);
    tab.append(&icon);
    tab.append(&label);
    tab.append(&count);
    tab.append(&close);
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bar.add_css_class("tab-activebar");
    bar.set_valign(gtk::Align::Start);
    bar.set_can_target(false);
    let root = gtk::Overlay::new();
    root.set_child(Some(&tab));
    root.add_overlay(&bar);

    let click = gtk::GestureClick::new();
    click.set_button(0);
    {
        let (id, strip) = (id.to_string(), strip.clone());
        click.connect_pressed(move |g, n, x, y| {
            let Some(app) = try_app() else { return };
            match g.current_button() {
                1 => {
                    let _ = app.update(|m| m.select_tab(&id));
                    if n == 2 {
                        glib::idle_add_local_once(rename_selected);
                    }
                }
                2 => close_tab(&id),
                3 => {
                    g.set_state(gtk::EventSequenceState::Claimed);
                    let Some(w) = g.widget() else { return };
                    let p = w
                        .compute_point(&strip, &gtk::graphene::Point::new(x as f32, y as f32))
                        .unwrap_or(gtk::graphene::Point::new(x as f32, y as f32));
                    let at = gdk::Rectangle::new(p.x() as i32, p.y() as i32, 1, 1);
                    crate::menus::popup_tab_menu(&id, strip.upcast_ref(), at);
                }
                _ => {}
            }
        });
    }
    tab.add_controller(click);
    // Drag a tab onto another to move it there.
    let drag = gtk::DragSource::new();
    drag.set_actions(gdk::DragAction::MOVE);
    {
        let id = id.to_string();
        drag.connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&id.to_value())));
    }
    tab.add_controller(drag);
    let drop = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
    {
        let id = id.to_string();
        drop.connect_drop(move |_, v, _, _| {
            let (Ok(moved), Some(app)) = (v.get::<String>(), try_app()) else {
                return false;
            };
            let at = {
                let m = app.model.borrow();
                let at = m
                    .worktrees()
                    .find_map(|(_, w)| w.tabs.iter().position(|t| t.id == id));
                at
            };
            if let (Some(at), false) = (at, moved == id) {
                let _ = app.update(|m| m.move_tab(&moved, at));
            }
            true
        });
    }
    tab.add_controller(drop);
    TabWidget {
        root,
        tab,
        bar,
        glyph,
        extra,
        icon,
        label,
        count,
    }
}

pub struct WorktreeView {
    root: gtk::Stack,
    tab_view: adw::TabView,
    /// The tabs of the strip, in order.
    tabs_box: gtk::Box,
    entries: Rc<RefCell<Vec<TabEntry>>>,
}

fn tab_id_of(entries: &RefCell<Vec<TabEntry>>, page: &adw::TabPage) -> Option<String> {
    entries
        .borrow()
        .iter()
        .find(|e| &e.page == page)
        .map(|e| e.id.clone())
}

/// A Worktree with no Tab (R103.6, spec A "Empty state"): one mono line
/// and key hints, in place of the old StatusPage.
fn empty_page() -> gtk::Box {
    let e = crate::chrome::empty_state(&rade_core::chrome::EMPTY_TAB);
    e.add_css_class("center-empty");
    e.set_halign(gtk::Align::Center);
    e.set_valign(gtk::Align::Center);
    e
}

impl WorktreeView {
    pub fn new(worktree: &str) -> Self {
        let tab_view = adw::TabView::new();
        tab_view.set_vexpand(true);
        let tabs_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let add = gtk::Button::from_icon_name("ph-plus-symbolic");
        add.add_css_class("tab-add");
        add.add_css_class("flat");
        add.set_focus_on_click(false);
        add.set_tooltip_text(Some("New: Shell, Agent, Fan-out, Browser"));
        add.connect_clicked(|b| crate::new_menu::toggle(b.upcast_ref()));
        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        strip.add_css_class("tab-strip");
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_policy(gtk::PolicyType::External, gtk::PolicyType::Never);
        scroller.set_propagate_natural_width(true);
        scroller.set_child(Some(&tabs_box));
        strip.append(&scroller);
        strip.append(&add);
        let tabs = gtk::Box::new(gtk::Orientation::Vertical, 0);
        tabs.append(&strip);
        tabs.append(&tab_view);

        let root = gtk::Stack::new();
        root.add_named(&tabs, Some("tabs"));
        root.add_named(&empty_page(), Some("empty"));

        let entries: Rc<RefCell<Vec<TabEntry>>> = Rc::default();
        REGISTRY.with(|r| {
            r.borrow_mut().insert(
                worktree.to_string(),
                (
                    strip.downgrade(),
                    tab_view.downgrade(),
                    Rc::downgrade(&entries),
                ),
            )
        });
        {
            let entries = entries.clone();
            tab_view.connect_selected_page_notify(move |view| {
                let Some(app) = try_app() else { return };
                if app.syncing.get() {
                    return;
                }
                if let Some(id) = view.selected_page().and_then(|p| tab_id_of(&entries, &p)) {
                    app.update(|m| m.select_tab(&id).ok());
                }
            });
        }
        Self {
            root,
            tab_view,
            tabs_box,
            entries,
        }
    }

    pub fn root(&self) -> &gtk::Stack {
        &self.root
    }

    pub fn sync(
        &self,
        wt: &Worktree,
        fanouts: &[Fanout],
        roots: &HashMap<String, gtk::Widget>,
        unread: &UnreadCounts,
        paused: bool,
    ) {
        let mut entries = self.entries.borrow_mut();
        // Pages whose Tab is gone.
        entries.retain(|e| {
            let keep = wt.tab(&e.id).is_some();
            if !keep {
                self.tab_view.close_page(&e.page);
                self.tabs_box.remove(&e.widget.root);
                crate::fanout_view::drop_bar(&e.id);
            }
            keep
        });
        let strip = self
            .tabs_box
            .ancestor(gtk::Box::static_type())
            .and_then(|w| w.downcast::<gtk::Box>().ok())
            .unwrap_or_else(|| self.tabs_box.clone());
        for (i, tab) in wt.tabs.iter().enumerate() {
            let pos = match entries.iter().position(|e| e.id == tab.id) {
                Some(pos) => pos,
                None => {
                    let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
                    container.set_hexpand(true);
                    container.set_vexpand(true);
                    container.add_css_class("panes");
                    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
                    outer.append(&container);
                    let page = self.tab_view.append(&outer);
                    let widget = tab_widget(&tab.id, &strip);
                    self.tabs_box.append(&widget.root);
                    entries.push(TabEntry {
                        id: tab.id.clone(),
                        page,
                        outer,
                        container,
                        signature: String::new(),
                        captions: Default::default(),
                        widget,
                        shown: Shown::default(),
                    });
                    entries.len() - 1
                }
            };
            let e = &mut entries[pos];
            let panes = tab.panes();
            // R103.2: the user's name, the Agent's, a Review / Browser,
            // else the Worktree's; never `Tab <n>`.
            let agents: Vec<_> = panes.iter().filter(|p| p.agent_name().is_some()).collect();
            let lead_pane = tab
                .grid
                .as_deref()
                .and_then(|g| agents.iter().find(|p| p.id == g))
                .or(agents.first());
            let lead = lead_pane.map(|p| {
                p.name
                    .clone()
                    .or_else(|| p.agent_name().map(String::from))
                    .unwrap_or_default()
            });
            let mark = lead_pane.and_then(|p| p.mark());
            let special = rade_core::chrome::tab_special(&panes);
            let label = rade_core::chrome::tab_label(
                tab.renamed.then_some(tab.name.as_str()),
                lead.as_deref(),
                special.as_deref(),
                &wt.name,
            );
            if e.widget.label.label() != label {
                e.widget.label.set_label(&label);
            }
            let count = rade_core::chrome::tab_count(agents.len());
            e.widget
                .count
                .set_label(&count.map(|n| n.to_string()).unwrap_or_default());
            e.widget.count.set_visible(count.is_some());
            // The Tab's LED: its worst Agent Status (spec A.30); a Browser
            // or Review Tab shows its type icon instead (A.43).
            let status = tab.status();
            let kind = if !agents.is_empty() {
                "agent"
            } else if panes.iter().all(|p| p.is_review()) {
                "review"
            } else if panes.iter().all(|p| p.is_diff()) {
                "diff"
            } else if panes
                .iter()
                .all(|p| matches!(p.kind, rade_core::PaneKind::Browser { .. }))
            {
                "browser"
            } else {
                "terminal"
            };
            let led = (kind == "agent").then(|| status.map_or("idle", |s| s.as_str()));
            e.widget.glyph.root.set_visible(led.is_some());
            if let (Some(_), Some(m)) = (led, mark) {
                let icon = crate::agent_glyph::icon_of(lead_pane.and_then(|p| p.agent_name()));
                e.widget.glyph.set(m, icon.as_deref(), status);
            }
            // R115.3: the other kinds' glyphs, each in its worst status.
            let marks = rade_core::chrome::tab_marks(&panes, tab.grid.as_deref());
            for (i, g) in e.widget.extra.iter().enumerate() {
                match marks.get(i + 1) {
                    Some((m, st)) if kind == "agent" => {
                        g.set(*m, None, *st);
                        g.root.set_visible(true);
                    }
                    _ => g.root.set_visible(false),
                }
            }
            if marks.len() > 1 {
                if let Some((m, st)) = marks.first() {
                    let icon = crate::agent_glyph::icon_of(lead_pane.and_then(|p| p.agent_name()));
                    e.widget.glyph.set(*m, icon.as_deref(), *st);
                }
            }
            let icon = match kind {
                "review" | "diff" => Some("ph-git-diff-symbolic"),
                "browser" => Some("ph-globe-simple-symbolic"),
                "terminal" => Some("ph-terminal-window-symbolic"),
                _ => None,
            };
            e.widget.icon.set_visible(icon.is_some());
            if let Some(i) = icon {
                e.widget.icon.set_icon_name(Some(i));
            }
            e.shown = Shown {
                status,
                led,
                mark,
                marks: marks.iter().map(|(m, _)| m.as_str()).collect(),
                count,
                kind,
            };
            // Unread is in the tooltip (R3.4), not the label.
            let n = unread.tab(&tab.id);
            let mut tip = crate::status::aggregate_tooltip(panes.iter().copied());
            if n > 0 {
                let line = format!("{n} unread");
                tip = Some(tip.map_or(line.clone(), |t| format!("{t}\n{line}")));
            }
            if marks.len() > 1 {
                let roster = agents
                    .iter()
                    .map(|p| {
                        format!(
                            "{} · {} · {}",
                            p.name.as_deref().unwrap_or(&p.id),
                            p.agent_name().unwrap_or_default(),
                            p.status.map_or("no status yet", |s| s.as_str())
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                tip = Some(tip.map_or(roster.clone(), |t| format!("{t}\n{roster}")));
            }
            if tab.zoomed.is_some() {
                let line = "Zoomed (Ctrl+Shift+Z to restore)".to_string();
                tip = Some(tip.map_or(line.clone(), |t| format!("{t}\n{line}")));
            }
            e.widget
                .tab
                .set_tooltip_markup(Some(&crate::titles::tab_tooltip(
                    tab.title(),
                    tip.as_deref(),
                )));
            if self.tab_view.page_position(&e.page) != i as i32 {
                self.tab_view.reorder_page(&e.page, i as i32);
            }
            // The strip in model order.
            let before = (i > 0).then(|| wt.tabs[i - 1].id.clone());
            let prev = before.and_then(|b| {
                entries
                    .iter()
                    .find(|x| x.id == b)
                    .map(|x| x.widget.root.clone())
            });
            let e = &mut entries[pos];
            if e.widget
                .root
                .prev_sibling()
                .map(|w| w.upcast::<gtk::Widget>())
                != prev.clone().map(|w| w.upcast::<gtk::Widget>())
            {
                self.tabs_box
                    .reorder_child_after(&e.widget.root, prev.as_ref());
            }
            // Paused ↔ running swaps every Pane's widget (R19.3).
            let sig = format!("{}|{paused}", layout_view::signature(tab));
            if sig != e.signature {
                e.captions = layout_view::rebuild(&e.container, tab, roots);
                e.signature = sig;
            }
            e.captions.update(tab, unread);
            // A Fan-out Tab shows its bar above the Agents (R15.2).
            if let Some(f) = fanouts.iter().find(|f| f.tab == tab.id) {
                if e.outer.first_child().as_ref() == Some(e.container.upcast_ref()) {
                    e.outer.prepend(&crate::fanout_view::bar(&tab.id, f));
                }
                crate::fanout_view::update(tab, f);
            }
        }
        let active = wt.active_tab.clone();
        for e in entries.iter() {
            let on = active.as_deref() == Some(e.id.as_str());
            if on {
                e.widget.tab.add_css_class("active");
                e.widget.bar.add_css_class("active");
                self.tab_view.set_selected_page(&e.page);
            } else {
                e.widget.tab.remove_css_class("active");
                e.widget.bar.remove_css_class("active");
            }
        }
        self.root
            .set_visible_child_name(if wt.tabs.is_empty() { "empty" } else { "tabs" });
    }
}
