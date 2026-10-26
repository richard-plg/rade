//! Render one Tab's Layout as nested `gtk::Paned` (R1.3) or, when Zoomed,
//! just the zoomed Pane (R1.7). Agent Panes get a one-line caption (name,
//! Preset, Agent Status, Unread); Agent Grid overflow renders as Title Bars
//! (R2.4); other Panes show Unread as a badge in their corner (R3.4).

use crate::app::try_app;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use rade_core::notify::UnreadCounts;
use rade_core::{AgentStatus, Layout, Pane, SplitDir, Tab};
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

/// One built caption's widgets (R20.4).
struct Caption {
    pane: String,
    root: gtk::Box,
    glyph: crate::status::Glyph,
    /// An Agent Pane's glyph in its status colour (R115).
    mark: crate::agent_glyph::Glyph,
    name: gtk::Label,
    /// `· agent · model` (R103.3).
    meta: gtk::Label,
    title: gtk::Label,
    unread: gtk::Label,
    status_icon: gtk::Image,
    status: gtk::Label,
    zoom: Option<gtk::Button>,
    /// The mic button (R124), shown on Agent Panes.
    voice: Option<gtk::Button>,
    /// Close Pane (R19.9), in a Tab with 2 or more Panes.
    close: Option<gtk::Button>,
}

/// The widgets of one built Tab that change without a rebuild: captions,
/// corner badges and the unfocused-Pane dimming, by Pane.
#[derive(Default)]
pub struct Captions {
    captions: Vec<Caption>,
    badges: Vec<(String, gtk::Label)>,
    dims: Vec<(String, gtk::Box)>,
    /// Each Pane's frame (`pane`, `focused`; R103.3).
    frames: Vec<(String, gtk::Box)>,
    /// The Tab's Pane count: with 2 or more every Terminal has a header
    /// and headers have a ✕ (R19.9).
    panes: usize,
}

impl Captions {
    pub fn update(&self, tab: &Tab, unread: &UnreadCounts) {
        for c in &self.captions {
            let Some(p) = tab.pane(&c.pane) else { continue };
            c.glyph.set(p.status);
            c.glyph.root.set_visible(false);
            c.mark.root.set_visible(p.mark().is_some());
            if p.mark().is_some() {
                c.mark.set_pane(p);
            }
            // `name · agent · model ··· status` (R103.3, chrome::pane_header).
            let parts = rade_core::chrome::pane_header(
                rade_core::chrome::header_name(p),
                p.agent_name(),
                rade_core::chrome::pane_model(p).as_deref(),
                None,
            );
            c.name.set_label(&parts[0]);
            let meta: String = parts[1..].iter().map(|x| format!(" · {x}")).collect();
            c.meta.set_label(&meta);
            c.meta.set_visible(!meta.is_empty());
            let title = p.shown_title().unwrap_or_default();
            c.title.set_label(&title);
            c.title.set_tooltip_text(p.title.as_deref());
            let n = unread.pane(&p.id);
            c.unread.set_label(&format!("{n} unread"));
            c.unread.set_visible(n > 0);
            match p.status {
                Some(s) => {
                    c.status.set_label(s.words());
                    set_class(&c.status, "pane-status-", s.as_str());
                    c.status.set_visible(true);
                    c.status_icon.set_visible(s == AgentStatus::Waiting);
                }
                None => {
                    c.status.set_visible(false);
                    c.status_icon.set_visible(false);
                }
            }
            c.root.set_tooltip_text(Some(&caption_tooltip(p)));
            if tab.focused == p.id {
                c.root.add_css_class("focused");
            } else {
                c.root.remove_css_class("focused");
            }
            if let Some(v) = &c.voice {
                v.set_visible(p.agent_name().is_some() || p.mark().is_some());
            }
            if let Some(z) = &c.zoom {
                let zoomed = tab.zoomed.as_deref() == Some(&p.id);
                z.set_icon_name(if zoomed {
                    "view-restore-symbolic"
                } else {
                    "view-fullscreen-symbolic"
                });
                z.set_tooltip_text(Some(if zoomed {
                    "Restore the Layout (Ctrl+Shift+Z)"
                } else {
                    "Zoom (Ctrl+Shift+Z)"
                }));
            }
        }
        for (id, badge) in &self.badges {
            let n = unread.pane(id);
            badge.set_label(&n.to_string());
            badge.set_visible(n > 0);
        }
        for (id, frame) in &self.frames {
            if tab.focused == *id {
                frame.add_css_class("focused");
            } else {
                frame.remove_css_class("focused");
            }
        }
        let several = tab.zoomed.is_none() && tab.layout.panes().len() > 1;
        for (id, dim) in &self.dims {
            dim.set_visible(several && tab.focused != *id);
        }
    }

    /// `pane`'s header as shown: `name · agent · model · status` (`debug.ui`).
    pub fn header(&self, pane: &str) -> Option<String> {
        let c = self.captions.iter().find(|c| c.pane == pane)?;
        let mut t = format!("{}{}", c.name.label(), c.meta.label());
        if c.status.is_visible() {
            t += &format!(" · {}", c.status.label());
        }
        Some(t)
    }

    /// `pane`'s ✕ (R19.9), when its header has one.
    pub fn close_button(&self, pane: &str) -> Option<gtk::Button> {
        self.captions.iter().find(|c| c.pane == pane)?.close.clone()
    }

    /// `pane`'s header glyph (R115): `{"glyph", "status"}`, null for a shell.
    pub fn glyph(&self, pane: &str) -> Option<serde_json::Value> {
        let c = self.captions.iter().find(|c| c.pane == pane)?;
        Some(if c.mark.root.is_visible() {
            c.mark.debug()
        } else {
            serde_json::Value::Null
        })
    }

    /// Whether `pane` is dimmed as an unfocused split (`debug.ui`).
    pub fn dimmed(&self, pane: &str) -> Option<bool> {
        self.dims
            .iter()
            .find(|(id, _)| id == pane)
            .map(|(_, d)| d.is_visible())
    }
}

/// Replace `widget`'s `<prefix>*` class with `<prefix><name>`.
pub fn set_class(widget: &impl IsA<gtk::Widget>, prefix: &str, name: &str) {
    let want = format!("{prefix}{name}");
    for c in widget.css_classes() {
        if c.starts_with(prefix) && c != want {
            widget.remove_css_class(&c);
        }
    }
    widget.add_css_class(&want);
}

/// The caption's tooltip: Agent Status details and the Pane's folder.
fn caption_tooltip(p: &Pane) -> String {
    let mut t = if p.agent.is_some() {
        crate::status::tooltip(p)
    } else {
        String::new()
    };
    if let Some(title) = &p.title {
        t = format!("{title}\n{t}");
    }
    format!("{}\n{}", t.trim(), p.cwd().display())
        .trim()
        .to_string()
}

/// Remove a Terminal's root from whatever container holds it, so it can be
/// placed again. Its Ghostty surface survives (unrealize keeps the PTY).
pub fn detach(w: &gtk::Widget) {
    let Some(parent) = w.parent() else { return };
    if let Some(p) = parent.downcast_ref::<gtk::Paned>() {
        if p.start_child().as_ref() == Some(w) {
            p.set_start_child(None::<&gtk::Widget>);
        } else {
            p.set_end_child(None::<&gtk::Widget>);
        }
    } else if let Some(b) = parent.downcast_ref::<gtk::Box>() {
        b.remove(w);
    } else if let Some(o) = parent.downcast_ref::<gtk::Overlay>() {
        o.set_child(None::<&gtk::Widget>);
    }
}

/// Agent Panes, Subagents and named Panes (a rename shows, R19.4).
/// Signature that changes exactly when the Tab's widgets must be rebuilt.
pub fn signature(tab: &Tab) -> String {
    let captions: Vec<(bool, bool)> = tab
        .panes()
        .into_iter()
        .map(|p| {
            (
                rade_core::chrome::has_header(p, tab.panes().len()),
                p.agent_name().is_some(),
            )
        })
        .collect();
    let bars: Vec<&str> = tab.title_bars.iter().map(|p| p.id.as_str()).collect();
    // One main Pane (R111.2): the focused one is the Pane on screen.
    let one = crate::layout_mode::mode()
        .one_pane
        .then_some(tab.focused.as_str());
    format!(
        "{}|{:?}|{bars:?}|{captions:?}|{one:?}",
        tab.layout.signature(),
        tab.zoomed
    )
}

fn dim_label(classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(None);
    l.set_xalign(0.0);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

/// A caption (R20.4): glyph, name, Preset, title, Unread, status chip, and
/// (unless it sits in a Title Bar button) Zoom and menu buttons, and Close
/// when the Tab has 2 or more Panes (R19.9).
fn caption_box(pane: &Pane, tab: &str, buttons: bool, captions: &mut Captions) -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.add_css_class("rade-caption");
    root.add_css_class("pane-header");
    let glyph = crate::status::Glyph::new();
    glyph.root.set_visible(false);
    let mark = crate::agent_glyph::Glyph::of_pane(pane);
    mark.root.add_css_class("pane-glyph");
    mark.root.set_visible(pane.mark().is_some());
    let name = dim_label(&["rade-caption-name", "pane-name"]);
    let meta = dim_label(&["pane-meta"]);
    let title = dim_label(&["pane-title"]);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_hexpand(true);
    title.set_margin_start(8);
    let unread = dim_label(&["pane-unread"]);
    let status_icon = gtk::Image::from_icon_name("ph-hand-palm-fill-symbolic");
    status_icon.add_css_class("pane-status-icon");
    status_icon.set_visible(false);
    let status = dim_label(&["pane-status"]);
    status.set_valign(gtk::Align::Center);
    root.append(&glyph.root);
    root.append(&mark.root);
    root.append(&name);
    root.append(&meta);
    root.append(&title);
    root.append(&unread);
    root.append(&status_icon);
    root.append(&status);
    let mut zoom = None;
    let mut voice = None;
    let mut close_button = None;
    if buttons {
        let z = gtk::Button::from_icon_name("view-fullscreen-symbolic");
        z.add_css_class("flat");
        z.add_css_class("pane-button");
        z.set_valign(gtk::Align::Center);
        let (id, t) = (pane.id.clone(), tab.to_string());
        z.connect_clicked(move |_| {
            if let Some(app) = try_app() {
                let _ = app.update(|m| {
                    m.focus_pane(&id)?;
                    m.toggle_zoom(&t)
                });
            }
        });
        let menu = gtk::MenuButton::new();
        menu.set_icon_name("ph-dots-three-symbolic");
        menu.add_css_class("flat");
        menu.add_css_class("pane-button");
        menu.set_valign(gtk::Align::Center);
        menu.set_tooltip_text(Some("Pane menu"));
        menu.set_menu_model(Some(&crate::menus::pane_menu(&pane.id)));
        // Pointer only: the keyboard stays on the Terminal (a relayout must
        // not hand focus to a caption button); the palette has the same
        // actions (Zoom, Rename Pane…, Split, Close Pane).
        z.set_focusable(false);
        menu.set_focusable(false);
        menu.set_focus_on_click(false);
        z.set_focus_on_click(false);
        // Voice prompt (R124, D3): before Zoom, Agent Panes only.
        let mic = crate::voice_view::mic_button(&pane.id);
        mic.set_visible(pane.agent_name().is_some() || pane.mark().is_some());
        root.append(&mic);
        voice = Some(mic);
        root.append(&z);
        root.append(&menu);
        if captions.panes >= 2 {
            let close = gtk::Button::from_icon_name("ph-x-symbolic");
            close.add_css_class("flat");
            close.add_css_class("pane-button");
            close.add_css_class("pane-close");
            close.set_valign(gtk::Align::Center);
            close.set_tooltip_text(Some("Close Pane (Ctrl+Shift+W)"));
            close.set_focusable(false);
            close.set_focus_on_click(false);
            let id = pane.id.clone();
            close.connect_clicked(move |_| crate::menus::close_pane_ui(&id));
            root.append(&close);
            close_button = Some(close);
        }
        zoom = Some(z);
    }
    captions.captions.push(Caption {
        pane: pane.id.clone(),
        mark: mark.clone(),
        root: root.clone(),
        glyph,
        name,
        meta,
        title,
        unread,
        status_icon,
        status,
        zoom,
        voice,
        close: close_button,
    });
    root
}

/// A Pane's widget: its caption (Agent and named Panes) above it, and on
/// top, never taking clicks, the Attention Ring, the unfocused dimming and
/// (Panes without a caption) the Unread badge.
fn pane_widget(
    pane: &Pane,
    tab: &str,
    roots: &HashMap<String, gtk::Widget>,
    captions: &mut Captions,
) -> gtk::Widget {
    let Some(root) = roots.get(&pane.id) else {
        return gtk::Label::new(Some("Pane failed to start")).upcast();
    };
    // The Pane's frame (spec A.46–A.48): border, radius, `focused` glow.
    let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
    b.add_css_class("pane");
    b.set_widget_name(&format!("pane-frame-{}", pane.id));
    b.set_overflow(gtk::Overflow::Hidden);
    captions.frames.push((pane.id.clone(), b.clone()));
    let header = rade_core::chrome::has_header(pane, captions.panes);
    if header {
        b.append(&caption_box(pane, tab, true, captions));
    }
    // The dimming covers the content only: the caption stays readable.
    let content = gtk::Overlay::new();
    content.set_child(Some(root));
    content.set_vexpand(true);
    // The Voice Composer's slot over the body (R124, D15).
    crate::voice_view::attach(&pane.id, &content);
    let layer = |o: &gtk::Overlay, class: &str| {
        let w = gtk::Box::new(gtk::Orientation::Vertical, 0);
        w.add_css_class(class);
        w.set_can_target(false);
        o.add_overlay(&w);
        w
    };
    let dim = layer(&content, "rade-unfocused-pane");
    dim.set_visible(false);
    captions.dims.push((pane.id.clone(), dim));
    b.append(&content);
    let o = gtk::Overlay::new();
    o.set_child(Some(&b));
    if !header {
        let badge = gtk::Label::new(None);
        badge.add_css_class("rade-count");
        badge.add_css_class("rade-pane-badge");
        badge.set_halign(gtk::Align::End);
        badge.set_valign(gtk::Align::Start);
        badge.set_can_target(false);
        badge.set_visible(false);
        badge.set_tooltip_text(Some("Unread Notifications"));
        captions.badges.push((pane.id.clone(), badge.clone()));
        o.add_overlay(&badge);
    }
    o.upcast()
}

/// One collapsed Subagent (R2.4); clicking it Zooms it.
fn title_bar(pane: &Pane, tab: &str, captions: &mut Captions) -> gtk::Widget {
    let button = gtk::Button::new();
    button.set_child(Some(&caption_box(pane, tab, false, captions)));
    button.add_css_class("flat");
    button.add_css_class("rade-title-bar");
    let id = pane.id.clone();
    button.connect_clicked(move |_| {
        if let Some(app) = try_app() {
            let _ = app.update(|m| m.focus_pane(&id));
        }
    });
    button.upcast()
}

/// Replace `container`'s content with the Tab's Layout.
pub fn rebuild(container: &gtk::Box, tab: &Tab, roots: &HashMap<String, gtk::Widget>) -> Captions {
    for pane in tab.panes() {
        if let Some(w) = roots.get(&pane.id) {
            detach(w);
        }
    }
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
    let mut captions = Captions {
        panes: tab.panes().len(),
        ..Default::default()
    };
    let zoomed = tab.zoomed.as_deref().and_then(|z| tab.pane(z));
    let ids: Vec<&str> = tab.panes().iter().map(|p| p.id.as_str()).collect();
    let one = (crate::layout_mode::mode().one_pane && ids.len() > 1)
        .then(|| rade_core::chrome::one_main_pane(&ids, Some(&tab.focused)))
        .flatten();
    let content = match (zoomed, one) {
        (Some(p), _) => pane_widget(p, &tab.id, roots, &mut captions),
        // Large text (R111.2): the focused Pane, the others as Title Bars
        // under it (R2.4 overflow), whatever the Layout.
        (None, Some((main, others))) => {
            let whole = gtk::Box::new(gtk::Orientation::Vertical, 0);
            if let Some(p) = tab.pane(main) {
                let w = pane_widget(p, &tab.id, roots, &mut captions);
                w.set_vexpand(true);
                whole.append(&w);
            }
            let bars = gtk::Box::new(gtk::Orientation::Vertical, 0);
            bars.add_css_class("one-pane-bars");
            for id in others {
                if let Some(p) = tab.pane(id) {
                    bars.append(&title_bar(p, &tab.id, &mut captions));
                }
            }
            whole.append(&bars);
            whole.upcast()
        }
        (None, None) if tab.title_bars.is_empty() => {
            build(&tab.id, &tab.layout, &mut Vec::new(), roots, &mut captions)
        }
        (None, None) => {
            let bars = gtk::Box::new(gtk::Orientation::Vertical, 0);
            for p in &tab.title_bars {
                bars.append(&title_bar(p, &tab.id, &mut captions));
            }
            // Agent Grid: Title Bars under the tiled Subagents on the right.
            match &tab.layout {
                Layout::Split {
                    dir: SplitDir::Right,
                    ratio,
                    a,
                    b,
                } => {
                    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
                    paned.set_shrink_start_child(false);
                    paned.set_shrink_end_child(false);
                    paned.set_start_child(Some(&build(
                        &tab.id,
                        a,
                        &mut vec![false],
                        roots,
                        &mut captions,
                    )));
                    let right = gtk::Box::new(gtk::Orientation::Vertical, 0);
                    let tiles = build(&tab.id, b, &mut vec![true], roots, &mut captions);
                    tiles.set_vexpand(true);
                    right.append(&tiles);
                    right.append(&bars);
                    paned.set_end_child(Some(&right));
                    track_ratio(&paned, &tab.id, Vec::new(), *ratio);
                    paned.upcast()
                }
                layout => {
                    let whole = gtk::Box::new(gtk::Orientation::Vertical, 0);
                    let tiles = build(&tab.id, layout, &mut Vec::new(), roots, &mut captions);
                    tiles.set_vexpand(true);
                    whole.append(&tiles);
                    whole.append(&bars);
                    whole.upcast()
                }
            }
        }
    };
    content.set_vexpand(true);
    container.append(&content);
    // Terminals not on screen (Title Bars, or everything but the Zoomed
    // one) stay parented, hidden, so their processes start (R2.8).
    let parking = gtk::Box::new(gtk::Orientation::Vertical, 0);
    parking.set_visible(false);
    for pane in tab.panes() {
        if let Some(w) = roots.get(&pane.id) {
            if w.parent().is_none() {
                parking.append(w);
            }
        }
    }
    container.append(&parking);
    captions
}

fn build(
    tab: &str,
    layout: &Layout,
    path: &mut Vec<bool>,
    roots: &HashMap<String, gtk::Widget>,
    captions: &mut Captions,
) -> gtk::Widget {
    match layout {
        Layout::Leaf { pane } => pane_widget(pane, tab, roots, captions),
        Layout::Split { dir, ratio, a, b } => {
            let orientation = match dir {
                SplitDir::Right => gtk::Orientation::Horizontal,
                SplitDir::Down => gtk::Orientation::Vertical,
            };
            let paned = gtk::Paned::new(orientation);
            paned.set_wide_handle(false);
            paned.set_shrink_start_child(false);
            paned.set_shrink_end_child(false);
            path.push(false);
            paned.set_start_child(Some(&build(tab, a, path, roots, captions)));
            path.pop();
            path.push(true);
            paned.set_end_child(Some(&build(tab, b, path, roots, captions)));
            path.pop();
            track_ratio(&paned, tab, path.clone(), *ratio);
            paned.upcast()
        }
    }
}

fn extent(p: &gtk::Paned) -> i32 {
    match p.orientation() {
        gtk::Orientation::Horizontal => p.width(),
        _ => p.height(),
    }
}

/// Apply the model's ratio once the Paned has a size, then record changes
/// back into the model. A change at an unchanged size is the user dragging
/// the handle: a manual resize, which stops Agent Grid auto-arranging (R2.4);
/// one caused by a new size (window resize) is not.
fn track_ratio(paned: &gtk::Paned, tab: &str, path: Vec<bool>, ratio: f64) {
    let ready = Rc::new(Cell::new(false));
    let last_size = Rc::new(Cell::new(0));
    {
        let (ready, last_size) = (ready.clone(), last_size.clone());
        paned.add_tick_callback(move |p, _| {
            let size = extent(p);
            if size <= 0 {
                return glib::ControlFlow::Continue;
            }
            p.set_position((size as f64 * ratio).round() as i32);
            last_size.set(size);
            ready.set(true);
            glib::ControlFlow::Break
        });
    }
    let tab = tab.to_string();
    paned.connect_position_notify(move |p| {
        let size = extent(p);
        if !ready.get() || size <= 0 {
            return;
        }
        let manual = size == last_size.replace(size);
        let Some(app) = try_app() else { return };
        if !app.syncing.get() {
            let r = p.position() as f64 / size as f64;
            app.touch(|m| m.set_split_ratio(&tab, &path, r, manual));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caption_tooltip_has_title_status_and_folder() {
        let mut p = Pane::new("p_3".into(), "/w/x".into(), None, Some("rev".into()));
        assert_eq!(caption_tooltip(&p), "/w/x");
        p.agent = Some("codex".into());
        p.title = Some("Fix bug".into());
        let t = caption_tooltip(&p);
        assert!(t.starts_with("Fix bug\nno status yet\n/w/x"), "{t}");
    }
}
