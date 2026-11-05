//! Render one Tab's Layout as nested `gtk::Paned` (R1.3) or, when Zoomed,
//! just the zoomed Pane (R1.7). Agent Panes get a one-line caption (name,
//! Preset, Agent Status, Unread); Agent Grid overflow renders as Title Bars
//! (R2.4); other Panes show Unread as a badge in their corner (R3.4).
//!
//! A Layout change keeps every Pane frame and split widget it can and moves
//! only the Panes whose place changed (R142, `rade_core::layout_diff`):
//! taking a Terminal out of the window unrealizes its GL area.

use crate::app::try_app;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use rade_core::layout_diff::{reconcile, Content, FrameKey, Shape, Step};
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

/// One Pane's frame as built (spec A.46–A.48): kept while its
/// [`FrameKey`] holds, so a Layout change moves it instead of rebuilding it
/// (R142).
struct Frame {
    key: FrameKey,
    /// The outer widget placed in the Layout.
    widget: gtk::Widget,
    /// Holds the Pane's root (Terminal, Viewer, …); `None` = it failed to start.
    content: Option<gtk::Overlay>,
    /// The bordered box (`focused`; R103.3).
    frame: Option<gtk::Box>,
    /// The unfocused-split dimming over the content.
    dim: Option<gtk::Box>,
    /// The Unread badge of a Pane without a header (R3.4).
    badge: Option<gtk::Label>,
    caption: Option<Caption>,
}

/// One node of the built Layout: its widgets mirror a [`Shape`] node.
enum Node {
    Leaf(String),
    Split {
        dir: SplitDir,
        paned: gtk::Paned,
        /// The model ratio last applied (the first-size tick reads it too).
        ratio: Rc<Cell<f64>>,
        a: Box<Node>,
        b: Box<Node>,
    },
}

impl Node {
    fn shape(&self) -> Shape {
        match self {
            Node::Leaf(p) => Shape::Leaf(p.clone()),
            Node::Split { dir, a, b, .. } => {
                Shape::Split(*dir, Box::new(a.shape()), Box::new(b.shape()))
            }
        }
    }
}

/// The Tab's outer widgets for its [`Content`].
#[derive(Default)]
enum Top {
    #[default]
    Empty,
    /// Zoomed or one main Pane: rebuilt on each change.
    Other(gtk::Widget),
    Tiled(Node),
    /// An Agent Grid with Title Bars: `paned` = the top right split.
    Grid {
        paned: gtk::Paned,
        ratio: Rc<Cell<f64>>,
        a: Node,
        b: Node,
        right: gtk::Box,
        bars: gtk::Box,
    },
    Stacked {
        whole: gtk::Box,
        tiles: Node,
        bars: gtk::Box,
    },
}

/// One built Tab's widgets, kept across Layout changes (R142): the frames
/// of the Panes on screen, the split widgets, the Title Bars and the parking
/// box; and what changes without a rebuild: captions, corner badges and the
/// unfocused-Pane dimming.
#[derive(Default)]
pub struct Captions {
    /// The frames of the Panes shown (in the Layout, Zoomed, or main).
    frames: HashMap<String, Frame>,
    /// Title Bar captions (R2.4).
    bars: Vec<Caption>,
    content: Option<Content>,
    top: Top,
    /// Terminals not on screen stay parented here, hidden (R2.8).
    parking: Option<gtk::Box>,
}

impl Captions {
    fn captions(&self) -> impl Iterator<Item = &Caption> {
        self.bars
            .iter()
            .chain(self.frames.values().filter_map(|f| f.caption.as_ref()))
    }

    pub fn update(&self, tab: &Tab, unread: &UnreadCounts) {
        for c in self.captions() {
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
        let several = tab.zoomed.is_none() && tab.layout.panes().len() > 1;
        for (id, f) in &self.frames {
            if let Some(badge) = &f.badge {
                let n = unread.pane(id);
                badge.set_label(&n.to_string());
                badge.set_visible(n > 0);
            }
            if let Some(frame) = &f.frame {
                if tab.focused == *id {
                    frame.add_css_class("focused");
                } else {
                    frame.remove_css_class("focused");
                }
            }
            if let Some(dim) = &f.dim {
                dim.set_visible(several && tab.focused != *id);
            }
        }
    }

    /// `pane`'s header as shown: `name · agent · model · status` (`debug.ui`).
    pub fn header(&self, pane: &str) -> Option<String> {
        let c = self.captions().find(|c| c.pane == pane)?;
        let mut t = format!("{}{}", c.name.label(), c.meta.label());
        if c.status.is_visible() {
            t += &format!(" · {}", c.status.label());
        }
        Some(t)
    }

    /// `pane`'s ✕ (R19.9), when its header has one.
    pub fn close_button(&self, pane: &str) -> Option<gtk::Button> {
        self.captions().find(|c| c.pane == pane)?.close.clone()
    }

    /// `pane`'s header glyph (R115): `{"glyph", "status"}`, null for a shell.
    pub fn glyph(&self, pane: &str) -> Option<serde_json::Value> {
        let c = self.captions().find(|c| c.pane == pane)?;
        Some(if c.mark.root.is_visible() {
            c.mark.debug()
        } else {
            serde_json::Value::Null
        })
    }

    /// Whether `pane` is dimmed as an unfocused split (`debug.ui`).
    pub fn dimmed(&self, pane: &str) -> Option<bool> {
        self.frames.get(pane)?.dim.as_ref().map(|d| d.is_visible())
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
fn caption_box(pane: &Pane, tab: &str, buttons: bool, with_close: bool) -> Caption {
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
        // Built on first open (R142): a menu model set here makes a popover
        // whose items watch their actions through every ancestor, and
        // placing a widget anywhere in the window then walks all of them.
        let id = pane.id.clone();
        menu.set_create_popup_func(move |b| {
            b.set_menu_model(Some(&crate::menus::pane_menu(&id)));
        });
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
        if with_close {
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
    Caption {
        pane: pane.id.clone(),
        mark,
        root,
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
    }
}

/// A Pane's frame: its caption (Agent and named Panes) above it, and on
/// top, never taking clicks, the unfocused dimming and (Panes without a
/// caption) the Unread badge. The root goes in with [`ensure_root`].
fn pane_frame(pane: &Pane, tab: &str, key: FrameKey, has_root: bool) -> Frame {
    if !has_root {
        return Frame {
            key,
            widget: gtk::Label::new(Some("Pane failed to start")).upcast(),
            content: None,
            frame: None,
            dim: None,
            badge: None,
            caption: None,
        };
    }
    // The Pane's frame (spec A.46–A.48): border, radius, `focused` glow.
    let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
    b.add_css_class("pane");
    b.set_widget_name(&format!("pane-frame-{}", pane.id));
    b.set_overflow(gtk::Overflow::Hidden);
    let caption = key.header.then(|| caption_box(pane, tab, true, key.close));
    if let Some(c) = &caption {
        b.append(&c.root);
    }
    // The dimming covers the content only: the caption stays readable.
    let content = gtk::Overlay::new();
    content.set_vexpand(true);
    // The Voice Composer's slot over the body (R124, D15).
    crate::voice_view::attach(&pane.id, &content);
    let dim = gtk::Box::new(gtk::Orientation::Vertical, 0);
    dim.add_css_class("rade-unfocused-pane");
    dim.set_can_target(false);
    content.add_overlay(&dim);
    dim.set_visible(false);
    b.append(&content);
    let o = gtk::Overlay::new();
    o.set_child(Some(&b));
    let mut badge = None;
    if !key.header {
        let l = gtk::Label::new(None);
        l.add_css_class("rade-count");
        l.add_css_class("rade-pane-badge");
        l.set_halign(gtk::Align::End);
        l.set_valign(gtk::Align::Start);
        l.set_can_target(false);
        l.set_visible(false);
        l.set_tooltip_text(Some("Unread Notifications"));
        o.add_overlay(&l);
        badge = Some(l);
    }
    Frame {
        key,
        widget: o.upcast(),
        content: Some(content),
        frame: Some(b),
        dim: Some(dim),
        badge,
        caption,
    }
}

/// Put `root` in the frame's content slot unless it is already there.
fn ensure_root(f: &Frame, root: &gtk::Widget) {
    let Some(content) = &f.content else { return };
    if content.child().as_ref() != Some(root) {
        detach(root);
        content.set_child(Some(root));
    }
}

/// One collapsed Subagent (R2.4); clicking it Zooms it.
fn title_bar(pane: &Pane, tab: &str, bars: &mut Vec<Caption>) -> gtk::Widget {
    let button = gtk::Button::new();
    let c = caption_box(pane, tab, false, false);
    button.set_child(Some(&c.root));
    bars.push(c);
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

/// A vertical box of Title Bars for `panes`.
fn bars_box(tab: &Tab, panes: &[String], bars: &mut Vec<Caption>) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 0);
    fill_bars(&b, tab, panes, bars);
    b
}

/// Replace `b`'s Title Bars (buttons only: no Terminal moves).
fn fill_bars(b: &gtk::Box, tab: &Tab, panes: &[String], bars: &mut Vec<Caption>) {
    while let Some(child) = b.first_child() {
        b.remove(&child);
    }
    for id in panes {
        if let Some(p) = tab.pane(id) {
            b.append(&title_bar(p, &tab.id, bars));
        }
    }
}

/// `w` as `slot`'s child unless it already is (a moved frame leaves its
/// old place first).
fn set_start(p: &gtk::Paned, w: &gtk::Widget) {
    if p.start_child().as_ref() != Some(w) {
        detach(w);
        p.set_start_child(Some(w));
    }
}

fn set_end(p: &gtk::Paned, w: &gtk::Widget) {
    if p.end_child().as_ref() != Some(w) {
        detach(w);
        p.set_end_child(Some(w));
    }
}

/// `w` as the first child of box `b` unless it already is.
fn set_first(b: &gtk::Box, w: &gtk::Widget) {
    if b.first_child().as_ref() == Some(w) {
        return;
    }
    if let Some(old) = b.first_child() {
        b.remove(&old);
    }
    detach(w);
    b.prepend(w);
}

/// Build or update the Tab's widgets for its Layout (R142): frames whose
/// [`FrameKey`] holds and split widgets whose subtree did not change stay
/// where they are; only the Panes whose place changed move.
pub fn rebuild(
    container: &gtk::Box,
    tab: &Tab,
    roots: &HashMap<String, gtk::Widget>,
    built: &mut Captions,
) {
    let panes = tab.panes();
    let n = panes.len();
    let content = Content::of(tab, crate::layout_mode::mode().one_pane);
    // The Panes shown in a frame: the rest are Title Bars or parked.
    let framed: Vec<&Pane> = match &content {
        Content::Zoomed(z) => tab.pane(z).into_iter().collect(),
        Content::One { main, .. } => tab.pane(main).into_iter().collect(),
        _ => tab.layout.panes(),
    };
    built
        .frames
        .retain(|id, _| framed.iter().any(|p| p.id == *id));
    for p in &framed {
        let key = FrameKey::of(p, n);
        let root = roots.get(&p.id);
        let stale = built
            .frames
            .get(&p.id)
            .is_none_or(|f| f.key != key || f.content.is_some() != root.is_some());
        if stale {
            let f = pane_frame(p, &tab.id, key, root.is_some());
            if let Some(old) = built.frames.insert(p.id.clone(), f) {
                detach(&old.widget);
            }
        }
        if let (Some(f), Some(root)) = (built.frames.get(&p.id), root) {
            ensure_root(f, root);
        }
    }
    built.bars.clear();
    let same = built
        .content
        .as_ref()
        .is_some_and(|old| old.same_frame(&content));
    let old_top = std::mem::take(&mut built.top);
    let frames = &built.frames;
    let bars = &mut built.bars;
    let top = match (&content, same, old_top) {
        (Content::Tiled, true, Top::Tiled(node)) => Top::Tiled(grow_from(
            Some(node),
            &tab.layout,
            &tab.id,
            &mut Vec::new(),
            frames,
        )),
        (Content::Tiled, _, _) => Top::Tiled(grow_from(
            None,
            &tab.layout,
            &tab.id,
            &mut Vec::new(),
            frames,
        )),
        (
            Content::Grid { bars: ids },
            true,
            Top::Grid {
                paned,
                ratio,
                a,
                b,
                right,
                bars: bars_w,
            },
        ) => {
            let (la, lb, r) = grid_parts(&tab.layout);
            let a = grow_from(Some(a), la, &tab.id, &mut vec![false], frames);
            let b = grow_from(Some(b), lb, &tab.id, &mut vec![true], frames);
            set_start(&paned, &node_widget(&a, frames));
            let tiles = node_widget(&b, frames);
            tiles.set_vexpand(true);
            set_first(&right, &tiles);
            fill_bars(&bars_w, tab, ids, bars);
            apply_ratio(&paned, &ratio, r);
            Top::Grid {
                paned,
                ratio,
                a,
                b,
                right,
                bars: bars_w,
            }
        }
        (Content::Grid { bars: ids }, _, _) => {
            let (la, lb, r) = grid_parts(&tab.layout);
            let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
            paned.set_shrink_start_child(false);
            paned.set_shrink_end_child(false);
            let a = grow_from(None, la, &tab.id, &mut vec![false], frames);
            set_start(&paned, &node_widget(&a, frames));
            let right = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let b = grow_from(None, lb, &tab.id, &mut vec![true], frames);
            let tiles = node_widget(&b, frames);
            tiles.set_vexpand(true);
            detach(&tiles);
            right.append(&tiles);
            let bars_w = bars_box(tab, ids, bars);
            right.append(&bars_w);
            paned.set_end_child(Some(&right));
            let ratio = track_ratio(&paned, &tab.id, Vec::new(), r);
            Top::Grid {
                paned,
                ratio,
                a,
                b,
                right,
                bars: bars_w,
            }
        }
        (
            Content::Stacked { bars: ids },
            true,
            Top::Stacked {
                whole,
                tiles,
                bars: bars_w,
            },
        ) => {
            let tiles = grow_from(Some(tiles), &tab.layout, &tab.id, &mut Vec::new(), frames);
            let w = node_widget(&tiles, frames);
            w.set_vexpand(true);
            set_first(&whole, &w);
            fill_bars(&bars_w, tab, ids, bars);
            Top::Stacked {
                whole,
                tiles,
                bars: bars_w,
            }
        }
        (Content::Stacked { bars: ids }, _, _) => {
            let whole = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let tiles = grow_from(None, &tab.layout, &tab.id, &mut Vec::new(), frames);
            let w = node_widget(&tiles, frames);
            w.set_vexpand(true);
            detach(&w);
            whole.append(&w);
            let bars_w = bars_box(tab, ids, bars);
            whole.append(&bars_w);
            Top::Stacked {
                whole,
                tiles,
                bars: bars_w,
            }
        }
        (Content::Zoomed(z), _, _) => Top::Other(frame_widget(z, frames)),
        // Large text (R111.2): the focused Pane, the others as Title Bars
        // under it (R2.4 overflow), whatever the Layout.
        (Content::One { main, others }, _, _) => {
            let whole = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let w = frame_widget(main, frames);
            w.set_vexpand(true);
            detach(&w);
            whole.append(&w);
            let b = bars_box(tab, others, bars);
            b.add_css_class("one-pane-bars");
            whole.append(&b);
            Top::Other(whole.upcast())
        }
    };
    let top_widget = match &top {
        Top::Empty => None,
        Top::Other(w) => Some(w.clone()),
        Top::Tiled(node) => Some(node_widget(node, frames)),
        Top::Grid { paned, .. } => Some(paned.clone().upcast()),
        Top::Stacked { whole, .. } => Some(whole.clone().upcast()),
    };
    built.top = top;
    built.content = Some(content);
    // Terminals not on screen (Title Bars, or everything but the Zoomed
    // one) stay parented, hidden, so their processes start (R2.8).
    let parking = built
        .parking
        .get_or_insert_with(|| {
            let p = gtk::Box::new(gtk::Orientation::Vertical, 0);
            p.set_visible(false);
            p
        })
        .clone();
    // The container holds [top, parking]: swap the top only (removing the
    // parking box would unrealize every parked Terminal).
    if let Some(w) = top_widget {
        w.set_vexpand(true);
        if container.first_child().as_ref() != Some(&w) {
            if let Some(old) = container
                .first_child()
                .filter(|c| c != parking.upcast_ref::<gtk::Widget>())
            {
                container.remove(&old);
            }
            detach(&w);
            container.prepend(&w);
        }
    }
    if parking.parent().as_ref() != Some(container.upcast_ref()) {
        detach(parking.upcast_ref());
        container.append(&parking);
    }
    for pane in &panes {
        if built.frames.contains_key(&pane.id) {
            continue;
        }
        if let Some(w) = roots.get(&pane.id) {
            if w.parent().as_ref() != Some(parking.upcast_ref()) {
                detach(w);
                parking.append(w);
            }
        }
    }
}

/// The Agent Grid's top right split: Parent side, tiles side, ratio.
fn grid_parts(layout: &Layout) -> (&Layout, &Layout, f64) {
    match layout {
        Layout::Split { a, b, ratio, .. } => (a, b, *ratio),
        // `Content::Grid` is only made for a right split at the top.
        leaf => (leaf, leaf, 0.5),
    }
}

fn frame_widget(pane: &str, frames: &HashMap<String, Frame>) -> gtk::Widget {
    frames
        .get(pane)
        .map(|f| f.widget.clone())
        .unwrap_or_else(|| gtk::Label::new(Some("Pane failed to start")).upcast())
}

fn node_widget(node: &Node, frames: &HashMap<String, Frame>) -> gtk::Widget {
    match node {
        Node::Leaf(p) => frame_widget(p, frames),
        Node::Split { paned, .. } => paned.clone().upcast(),
    }
}

/// The widgets for `layout`, from `old` (what was built at this place) by
/// [`reconcile`]'s steps.
fn grow_from(
    old: Option<Node>,
    layout: &Layout,
    tab: &str,
    path: &mut Vec<bool>,
    frames: &HashMap<String, Frame>,
) -> Node {
    let step = reconcile(old.as_ref().map(Node::shape).as_ref(), &Shape::of(layout));
    grow(step, old, layout, tab, path, frames)
}

fn grow(
    step: Step,
    old: Option<Node>,
    layout: &Layout,
    tab: &str,
    path: &mut Vec<bool>,
    frames: &HashMap<String, Frame>,
) -> Node {
    // A kept split is walked like a reused one: its children are the same,
    // but a frame rebuilt for a new header must go back in its slot.
    let step = match (step, layout) {
        (Step::Keep, Layout::Split { .. }) => {
            Step::Reuse(Box::new(Step::Keep), Box::new(Step::Keep))
        }
        (Step::Keep, Layout::Leaf { pane }) => Step::Place(pane.id.clone()),
        (s, _) => s,
    };
    match (step, old, layout) {
        (Step::Place(p), _, _) => Node::Leaf(p),
        (
            Step::Reuse(sa, sb),
            Some(Node::Split {
                dir,
                paned,
                ratio,
                a,
                b,
            }),
            Layout::Split {
                ratio: r,
                a: la,
                b: lb,
                ..
            },
        ) => {
            path.push(false);
            let a = grow(*sa, Some(*a), la, tab, path, frames);
            path.pop();
            path.push(true);
            let b = grow(*sb, Some(*b), lb, tab, path, frames);
            path.pop();
            set_start(&paned, &node_widget(&a, frames));
            set_end(&paned, &node_widget(&b, frames));
            apply_ratio(&paned, &ratio, *r);
            Node::Split {
                dir,
                paned,
                ratio,
                a: Box::new(a),
                b: Box::new(b),
            }
        }
        (_, _, Layout::Leaf { pane }) => Node::Leaf(pane.id.clone()),
        (_, _, Layout::Split { dir, ratio, a, b }) => {
            let orientation = match dir {
                SplitDir::Right => gtk::Orientation::Horizontal,
                SplitDir::Down => gtk::Orientation::Vertical,
            };
            let paned = gtk::Paned::new(orientation);
            paned.set_wide_handle(false);
            paned.set_shrink_start_child(false);
            paned.set_shrink_end_child(false);
            path.push(false);
            let na = grow_from(None, a, tab, path, frames);
            path.pop();
            path.push(true);
            let nb = grow_from(None, b, tab, path, frames);
            path.pop();
            set_start(&paned, &node_widget(&na, frames));
            set_end(&paned, &node_widget(&nb, frames));
            let cell = track_ratio(&paned, tab, path.clone(), *ratio);
            Node::Split {
                dir: *dir,
                paned,
                ratio: cell,
                a: Box::new(na),
                b: Box::new(nb),
            }
        }
    }
}

fn extent(p: &gtk::Paned) -> i32 {
    match p.orientation() {
        gtk::Orientation::Horizontal => p.width(),
        _ => p.height(),
    }
}

/// A kept split follows the model's ratio (an Agent Grid re-arranging its
/// tiles): set inside the sync, so it is not taken for a drag.
fn apply_ratio(paned: &gtk::Paned, applied: &Cell<f64>, ratio: f64) {
    if (applied.get() - ratio).abs() < 1e-9 {
        return;
    }
    applied.set(ratio);
    let size = extent(paned);
    if size > 0 {
        paned.set_position((size as f64 * ratio).round() as i32);
    }
}

/// Apply the model's ratio once the Paned has a size, then record changes
/// back into the model. A change at an unchanged size is the user dragging
/// the handle: a manual resize, which stops Agent Grid auto-arranging (R2.4);
/// one caused by a new size (window resize) is not. Returns the ratio cell
/// [`apply_ratio`] updates.
fn track_ratio(paned: &gtk::Paned, tab: &str, path: Vec<bool>, ratio: f64) -> Rc<Cell<f64>> {
    let applied = Rc::new(Cell::new(ratio));
    let ready = Rc::new(Cell::new(false));
    let last_size = Rc::new(Cell::new(0));
    {
        let (ready, last_size, applied) = (ready.clone(), last_size.clone(), applied.clone());
        paned.add_tick_callback(move |p, _| {
            let size = extent(p);
            if size <= 0 {
                return glib::ControlFlow::Continue;
            }
            p.set_position((size as f64 * applied.get()).round() as i32);
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
    applied
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
