//! The Voice Composer widget (PRD R124, PLAN-voice.md T8): the card over an
//! Agent Pane, drawn only from `rade_core::voice::composer::ComposerView`.
//! Keys and buttons inside the card go to the controller as
//! `crate::voice::on_ui(Event)`; the mic button of a Pane header calls
//! `crate::voice::tap`.
//!
//! The card is built when it opens and mounted as an overlay child of the
//! Pane's body (`attach`, called by `layout_view` for every Pane it builds, so
//! a relayout moves the card to the new body). Code spans (`` `x` ``) and
//! `[?] "…"` marks are found in the draft text here; `changed` ranges come
//! from the view model.
#![allow(dead_code)]

use crate::app::try_app;
use adw::prelude::*;
use gtk::glib;
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::voice::composer::{ActivityIcon, ComposerView, Event, Mode, State};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

/// Byte ranges of the draft text that get a `code`, `changed` or `unsure`
/// tag. A `code` range includes its two backticks (they are hidden).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DraftTags {
    pub code: Vec<Range<usize>>,
    pub changed: Vec<Range<usize>>,
    pub unsure: Vec<Range<usize>>,
}

/// The meter: 28 bars, 2 px wide, 2 px apart (110 × 16 px).
const BARS: usize = 28;
/// Below this body height the card fills the body (D15).
const COMPACT_BELOW: i32 = 300;

struct Card {
    pane: String,
    root: gtk::Box,
    composer: gtk::Box,
    dot: gtk::Box,
    state: gtk::Label,
    time: gtk::Label,
    meter: gtk::DrawingArea,
    levels: RefCell<Vec<f32>>,
    target_name: gtk::Label,
    target_where: gtk::Label,
    heard_box: gtk::Box,
    heard: gtk::Label,
    heard_toggle: gtk::Button,
    /// The view model's fold, and the user's override of it.
    model_fold: Cell<bool>,
    fold_override: Cell<Option<bool>>,
    scroller: gtk::ScrolledWindow,
    draft: gtk::TextView,
    buffer: gtk::TextBuffer,
    placeholder: gtk::Label,
    /// The `draft_version` the buffer shows.
    version: Cell<Option<u64>>,
    programmatic: Cell<bool>,
    tags: RefCell<DraftTags>,
    caret: RefCell<Option<gtk::TextChildAnchor>>,
    activity: gtk::Box,
    act_icon: gtk::Image,
    act_text: gtk::Label,
    act_link: gtk::Button,
    act_step: gtk::Label,
    hints: gtk::Box,
    hint_texts: RefCell<Vec<String>>,
    paste: gtk::Button,
    send: gtk::Button,
    send_kbd: gtk::Label,
    last: RefCell<Option<ComposerView>>,
    compact: Cell<bool>,
    editable: Cell<bool>,
    /// The height `size_draft` last gave the draft box.
    draft_h: Cell<i32>,
}

thread_local! {
    static CARD: RefCell<Option<Rc<Card>>> = const { RefCell::new(None) };
    static SLOTS: RefCell<Vec<(String, glib::WeakRef<gtk::Overlay>)>> = const { RefCell::new(Vec::new()) };
    static MICS: RefCell<Vec<(String, glib::WeakRef<gtk::Button>)>> = const { RefCell::new(Vec::new()) };
}

fn card() -> Option<Rc<Card>> {
    CARD.with(|c| c.borrow().clone())
}

/// Milliseconds on the monotonic clock (the `at_ms` of key events).
pub fn now_ms() -> u64 {
    (glib::monotonic_time() / 1000).max(0) as u64
}

/// Whether the composer is shown (Send is then the window's one primary
/// button, D16).
pub fn is_open() -> bool {
    CARD.with(|c| c.borrow().is_some())
}

/// The Pane the composer belongs to.
pub fn pane() -> Option<String> {
    card().map(|c| c.pane.clone())
}

fn label(classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(None);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

fn kbd(text: &str) -> gtk::Label {
    let k = gtk::Label::new(Some(text));
    k.add_css_class("kbd");
    k.set_valign(gtk::Align::Center);
    k
}

fn rgba(token: &str) -> gtk::gdk::RGBA {
    gtk::gdk::RGBA::parse(crate::tokens::get(token, crate::tokens::dark()))
        .unwrap_or(gtk::gdk::RGBA::BLACK)
}

fn escape(text: &str) -> String {
    glib::markup_escape_text(text).to_string()
}

// ---------------------------------------------------------------------------
// Where the card and the mic buttons live
// ---------------------------------------------------------------------------

/// A Pane's body was built: remember its overlay, and mount the card in it
/// when the card belongs to this Pane.
pub fn attach(pane: &str, body: &gtk::Overlay) {
    SLOTS.with(|s| {
        let mut s = s.borrow_mut();
        s.retain(|(id, w)| id != pane && w.upgrade().is_some());
        s.push((pane.to_string(), body.downgrade()));
    });
    if let Some(c) = card().filter(|c| c.pane == pane) {
        mount(&c, body);
    }
}

fn mount(c: &Card, body: &gtk::Overlay) {
    if let Some(parent) = c.root.parent() {
        if parent == *body.upcast_ref::<gtk::Widget>() {
            return;
        }
        match parent.downcast::<gtk::Overlay>() {
            Ok(o) => o.remove_overlay(&c.root),
            Err(_) => c.root.unparent(),
        }
    }
    body.add_overlay(&c.root);
}

fn slot_of(pane: &str) -> Option<gtk::Overlay> {
    SLOTS.with(|s| {
        s.borrow()
            .iter()
            .find(|(id, _)| id == pane)
            .and_then(|(_, w)| w.upgrade())
    })
}

/// An Agent Pane header's mic button (D3): clicking taps (D2 toggle).
pub fn mic_button(pane: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name("ph-microphone-symbolic");
    b.add_css_class("flat");
    b.add_css_class("pane-button");
    b.add_css_class("voice-button");
    b.set_valign(gtk::Align::Center);
    b.set_tooltip_text(Some("Voice prompt (hold F8)"));
    b.set_focusable(false);
    b.set_focus_on_click(false);
    let id = pane.to_string();
    b.connect_clicked(move |_| crate::voice::tap(&id));
    MICS.with(|m| {
        let mut m = m.borrow_mut();
        m.retain(|(_, w)| w.upgrade().is_some());
        m.push((pane.to_string(), b.downgrade()));
    });
    paint_mic(&b, pane);
    b
}

/// `recording` while this Pane's take records, `open` while its composer is
/// open and not recording.
fn paint_mic(b: &gtk::Button, pane: &str) {
    let state = card().filter(|c| c.pane == pane).map(|c| {
        c.last
            .borrow()
            .as_ref()
            .is_some_and(|v| v.recording || v.listening)
    });
    let (recording, open) = match state {
        Some(true) => (true, false),
        Some(false) => (false, true),
        None => (false, false),
    };
    for (class, on) in [("recording", recording), ("open", open)] {
        if on {
            b.add_css_class(class);
        } else {
            b.remove_css_class(class);
        }
    }
    b.set_icon_name(if recording {
        "ph-microphone-fill-symbolic"
    } else {
        "ph-microphone-symbolic"
    });
}

fn refresh_mics() {
    let mics: Vec<(String, gtk::Button)> = MICS.with(|m| {
        let mut m = m.borrow_mut();
        m.retain(|(_, w)| w.upgrade().is_some());
        m.iter()
            .filter_map(|(id, w)| w.upgrade().map(|b| (id.clone(), b)))
            .collect()
    });
    for (id, b) in mics {
        paint_mic(&b, &id);
    }
}

// ---------------------------------------------------------------------------
// Building the card
// ---------------------------------------------------------------------------

fn build(pane_id: &str) -> Rc<Card> {
    // Header.
    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class("voice-dot");
    dot.set_valign(gtk::Align::Center);
    dot.set_halign(gtk::Align::Start);
    let state = label(&["voice-state"]);
    let time = label(&["voice-time"]);
    let meter = gtk::DrawingArea::new();
    meter.add_css_class("voice-meter");
    meter.set_content_width(110);
    meter.set_content_height(16);
    meter.set_valign(gtk::Align::Center);
    meter.set_halign(gtk::Align::Start);
    meter.set_margin_start(4);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let target = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    target.add_css_class("voice-target");
    target.append(&gtk::Label::new(Some("\u{2192}")));
    let target_name = label(&["pane-name"]);
    let target_where = label(&[]);
    let close = gtk::Button::from_icon_name("ph-x-symbolic");
    close.add_css_class("flat");
    close.add_css_class("voice-close");
    close.set_valign(gtk::Align::Center);
    close.set_tooltip_text(Some("Discard (Esc)"));
    close.set_focusable(false);
    close.set_focus_on_click(false);
    close.connect_clicked(|_| crate::voice::on_ui(Event::Discard));
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    header.add_css_class("voice-header");
    header.append(&dot);
    header.append(&state);
    header.append(&time);
    header.append(&meter);
    header.append(&spacer);
    header.append(&target);
    header.append(&close);

    // Heard.
    let quote = gtk::Image::from_icon_name("ph-quotes-symbolic");
    quote.add_css_class("voice-quote");
    quote.set_valign(gtk::Align::Start);
    let heard = gtk::Label::new(None);
    heard.add_css_class("voice-heard-text");
    heard.set_xalign(0.0);
    heard.set_hexpand(true);
    heard.set_use_markup(true);
    heard.set_wrap(true);
    heard.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    heard.set_width_chars(1);
    let heard_toggle = gtk::Button::with_label("show");
    heard_toggle.add_css_class("flat");
    heard_toggle.add_css_class("voice-heard-toggle");
    heard_toggle.set_valign(gtk::Align::Start);
    heard_toggle.set_focusable(false);
    heard_toggle.set_focus_on_click(false);
    let heard_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    heard_box.add_css_class("voice-heard");
    heard_box.append(&quote);
    heard_box.append(&heard);
    heard_box.append(&heard_toggle);

    // Draft.
    let draft = gtk::TextView::new();
    draft.add_css_class("voice-draft");
    draft.set_wrap_mode(gtk::WrapMode::WordChar);
    draft.set_accepts_tab(false);
    draft.set_vexpand(false);
    let buffer = draft.buffer();
    let scroller = gtk::ScrolledWindow::new();
    scroller.add_css_class("voice-draft-scroll");
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    // The height follows the text (`size_draft`), 70 px to 300 px (14 lines).
    scroller.set_propagate_natural_height(false);
    scroller.set_min_content_height(70);
    scroller.set_max_content_height(300);
    scroller.set_vexpand(false);
    scroller.set_child(Some(&draft));
    let placeholder = label(&["voice-placeholder"]);
    placeholder.set_xalign(0.0);
    placeholder.set_halign(gtk::Align::Start);
    placeholder.set_valign(gtk::Align::Start);
    placeholder.set_can_target(false);
    placeholder.set_wrap(true);
    placeholder.set_width_chars(1);
    let draft_overlay = gtk::Overlay::new();
    draft_overlay.add_css_class("voice-draft-wrap");
    draft_overlay.set_child(Some(&scroller));
    draft_overlay.add_overlay(&placeholder);

    // Activity.
    let act_icon = gtk::Image::new();
    act_icon.add_css_class("voice-activity-icon");
    let act_text = gtk::Label::new(None);
    act_text.set_xalign(0.0);
    act_text.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let act_link = gtk::Button::with_label("Set up voice\u{2026}");
    act_link.add_css_class("flat");
    act_link.add_css_class("voice-link");
    act_link.set_focusable(false);
    act_link.set_focus_on_click(false);
    act_link.connect_clicked(|_| setup_link());
    let act_step = label(&["voice-step"]);
    act_step.set_hexpand(true);
    act_step.set_xalign(1.0);
    let activity = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    activity.add_css_class("voice-activity");
    activity.append(&act_icon);
    activity.append(&act_text);
    activity.append(&act_link);
    activity.append(&act_step);

    // Footer.
    let hints = gtk::Box::new(gtk::Orientation::Horizontal, 14);
    hints.set_hexpand(true);
    let paste_kbd = kbd("Alt \u{21b5}");
    let paste_in = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    paste_in.append(&gtk::Label::new(Some("Paste only")));
    paste_in.append(&paste_kbd);
    let paste = gtk::Button::new();
    paste.set_child(Some(&paste_in));
    paste.add_css_class("voice-paste");
    paste.set_valign(gtk::Align::Center);
    paste.set_focusable(false);
    paste.set_focus_on_click(false);
    paste.connect_clicked(|_| crate::voice::on_ui(Event::Paste));
    let send_kbd = kbd("\u{21b5}");
    let send_in = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    send_in.append(&gtk::Image::from_icon_name(
        "ph-arrow-elbow-down-left-symbolic",
    ));
    send_in.append(&gtk::Label::new(Some("Send")));
    send_in.append(&send_kbd);
    let send = gtk::Button::new();
    send.set_child(Some(&send_in));
    send.add_css_class("voice-send");
    send.add_css_class("suggested-action");
    send.set_valign(gtk::Align::Center);
    send.set_focusable(false);
    send.set_focus_on_click(false);
    send.connect_clicked(|_| crate::voice::on_ui(Event::Send));
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.add_css_class("voice-footer");
    footer.append(&hints);
    footer.append(&paste);
    footer.append(&send);

    let composer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    composer.add_css_class("voice-composer");
    composer.set_focusable(true);
    composer.set_overflow(gtk::Overflow::Hidden);
    composer.append(&header);
    composer.append(&heard_box);
    composer.append(&draft_overlay);
    composer.append(&activity);
    composer.append(&footer);

    // Up to 760 px wide and centred; only the card takes clicks.
    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(760);
    clamp.set_tightening_threshold(760);
    clamp.set_child(Some(&composer));
    clamp.set_can_target(false);
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.add_css_class("voice-slot");
    root.set_valign(gtk::Align::End);
    root.set_halign(gtk::Align::Fill);
    root.set_margin_start(10);
    root.set_margin_end(10);
    root.set_margin_bottom(10);
    root.set_margin_top(10);
    root.set_can_target(false);
    root.append(&clamp);

    let c = Rc::new(Card {
        pane: pane_id.to_string(),
        root,
        composer: composer.clone(),
        dot,
        state,
        time,
        meter: meter.clone(),
        levels: RefCell::default(),
        target_name,
        target_where,
        heard_box,
        heard,
        heard_toggle: heard_toggle.clone(),
        model_fold: Cell::new(false),
        fold_override: Cell::new(None),
        scroller,
        draft: draft.clone(),
        buffer: buffer.clone(),
        placeholder,
        version: Cell::new(None),
        programmatic: Cell::new(false),
        tags: RefCell::default(),
        caret: RefCell::new(None),
        activity,
        act_icon,
        act_text,
        act_link,
        act_step,
        hints,
        hint_texts: RefCell::default(),
        paste,
        send,
        send_kbd,
        last: RefCell::new(None),
        compact: Cell::new(false),
        editable: Cell::new(false),
        draft_h: Cell::new(0),
    });
    c.target_name.set_ellipsize(gtk::pango::EllipsizeMode::End);
    c.target_where.set_ellipsize(gtk::pango::EllipsizeMode::End);
    target.append(&c.target_name);
    target.append(&gtk::Label::new(Some("\u{b7}")));
    target.append(&c.target_where);
    // The agent glyph (R115) follows the arrow.
    if let Some(app) = try_app() {
        if let Some(p) = app.model.borrow().pane(pane_id) {
            let g = crate::agent_glyph::Glyph::of_pane(p);
            g.root.add_css_class("voice-glyph");
            target.insert_child_after(&g.root, target.first_child().as_ref());
        }
    }

    {
        let w = Rc::downgrade(&c);
        meter.set_draw_func(move |_, cr, width, height| {
            if let Some(c) = w.upgrade() {
                draw_meter(&c, cr, width, height);
            }
        });
    }
    {
        let w = Rc::downgrade(&c);
        heard_toggle.connect_clicked(move |_| {
            if let Some(c) = w.upgrade() {
                let folded = c.fold_override.get().unwrap_or(c.model_fold.get());
                c.fold_override.set(Some(!folded));
                paint_heard(&c);
            }
        });
    }
    {
        let w = Rc::downgrade(&c);
        buffer.connect_changed(move |b| {
            let Some(c) = w.upgrade() else { return };
            if c.programmatic.get() {
                return;
            }
            let text = b.text(&b.start_iter(), &b.end_iter(), false).to_string();
            // Edited words are no longer "changed" (D11).
            retag(&c, &text, &[]);
            size_draft(&c);
            crate::voice::on_ui(Event::DraftEdited(text));
        });
    }
    {
        // Esc / Enter anywhere inside the card; nothing reaches the Terminal.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(|_, key, _, mods| {
            if handle_key(key, mods) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        composer.add_controller(keys);
    }
    {
        // A click on the card (not a button) puts the keyboard in it.
        let click = gtk::GestureClick::new();
        let w = Rc::downgrade(&c);
        click.connect_pressed(move |_, _, _, _| {
            if let Some(c) = w.upgrade() {
                if !c.draft.is_focus() && !c.editable.get() {
                    c.composer.grab_focus();
                }
            }
        });
        composer.add_controller(click);
    }
    c
}

/// A key pressed inside the card: Esc, Enter (Send), Alt+Enter (Paste only).
/// Shift+Enter is the draft's newline (not handled). `debug.ui key` may call
/// this while the composer is open.
pub fn handle_key(key: gtk::gdk::Key, mods: gtk::gdk::ModifierType) -> bool {
    use gtk::gdk::{Key, ModifierType};
    match key {
        Key::Escape => {
            crate::voice::on_ui(Event::Escape { at_ms: now_ms() });
            true
        }
        Key::Return | Key::KP_Enter | Key::ISO_Enter => {
            if mods.contains(ModifierType::SHIFT_MASK) {
                false
            } else if mods.contains(ModifierType::ALT_MASK) {
                crate::voice::on_ui(Event::Paste);
                true
            } else {
                crate::voice::on_ui(Event::Send);
                true
            }
        }
        _ => false,
    }
}

fn draw_meter(c: &Card, cr: &gtk::cairo::Context, _w: i32, h: i32) {
    let levels = c.levels.borrow();
    let col = rgba("acc");
    cr.set_source_rgba(
        col.red().into(),
        col.green().into(),
        col.blue().into(),
        col.alpha().into(),
    );
    let n = levels.len().min(BARS);
    let from = levels.len() - n;
    for i in 0..BARS {
        // The newest level is the right-most bar.
        let level = if i + n >= BARS {
            levels[from + i + n - BARS].clamp(0.0, 1.0)
        } else {
            0.0
        };
        let bar = (3.0 + level as f64 * 13.0).round();
        let x = (i * 4) as f64;
        let y = ((h as f64) - bar) / 2.0;
        cr.rectangle(x, y.max(0.0), 2.0, bar);
    }
    let _ = cr.fill();
}

// ---------------------------------------------------------------------------
// Draft text and its tags
// ---------------------------------------------------------------------------

/// Backtick spans (including the backticks) and `[?] "…"` marks in `text`.
pub fn compute_tags(text: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let mut code = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'`' {
            // Only a span on one line, with something in it.
            let mut j = i + 1;
            while j < b.len() && b[j] != b'`' && b[j] != b'\n' {
                j += 1;
            }
            if j < b.len() && b[j] == b'`' && j > i + 1 {
                code.push(i..j + 1);
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    let mut unsure = Vec::new();
    let mut from = 0;
    while let Some(p) = text[from..].find("[?] \"") {
        let start = from + p;
        let body = start + 5;
        match text[body..].find(['"', '\n']) {
            Some(q) if text[body + q..].starts_with('"') => {
                unsure.push(start..body + q + 1);
                from = body + q + 1;
            }
            _ => from = body,
        }
    }
    (code, unsure)
}

fn char_offset(text: &str, byte: usize) -> i32 {
    let mut b = byte.min(text.len());
    while !text.is_char_boundary(b) {
        b -= 1;
    }
    text[..b].chars().count() as i32
}

fn ensure_tags(c: &Card) {
    let table = c.buffer.tag_table();
    let make = |name: &str| -> gtk::TextTag {
        table.lookup(name).unwrap_or_else(|| {
            let t = gtk::TextTag::new(Some(name));
            table.add(&t);
            t
        })
    };
    let code = make("code");
    code.set_family(Some("JetBrains Mono"));
    code.set_scale(12.5 / 13.5);
    code.set_background_rgba(Some(&rgba("bg3")));
    let hide = make("hide");
    hide.set_invisible(true);
    let changed = make("changed");
    changed.set_background_rgba(Some(&rgba("acc-dim")));
    changed.set_underline(gtk::pango::Underline::Single);
    changed.set_underline_rgba(Some(&rgba("acc-bar")));
    let unsure = make("unsure");
    unsure.set_family(Some("JetBrains Mono"));
    unsure.set_weight(500);
    unsure.set_scale(12.0 / 13.5);
    unsure.set_foreground_rgba(Some(&rgba("wait")));
    unsure.set_background_rgba(Some(&rgba("wait-dim")));
}

/// Tag `text` (what the buffer holds) afresh.
fn retag(c: &Card, text: &str, changed: &[Range<usize>]) {
    ensure_tags(c);
    let (code, unsure) = compute_tags(text);
    let (s, e) = (c.buffer.start_iter(), c.buffer.end_iter());
    for t in ["code", "hide", "changed", "unsure"] {
        c.buffer.remove_tag_by_name(t, &s, &e);
    }
    let at = |byte: usize| c.buffer.iter_at_offset(char_offset(text, byte));
    let apply = |name: &str, r: &Range<usize>| {
        c.buffer.apply_tag_by_name(name, &at(r.start), &at(r.end));
    };
    for r in &code {
        apply("code", r);
        apply("hide", &(r.start..r.start + 1));
        apply("hide", &(r.end - 1..r.end));
    }
    let changed: Vec<Range<usize>> = changed
        .iter()
        .filter(|r| r.start < r.end && r.end <= text.len())
        .cloned()
        .collect();
    for r in &changed {
        apply("changed", r);
    }
    for r in &unsure {
        apply("unsure", r);
    }
    *c.tags.borrow_mut() = DraftTags {
        code,
        changed,
        unsure,
    };
}

fn remove_caret(c: &Card) {
    if let Some(a) = c.caret.borrow_mut().take() {
        if !a.is_deleted() {
            let mut s = c.buffer.iter_at_child_anchor(&a);
            let mut e = s;
            e.forward_char();
            c.programmatic.set(true);
            c.buffer.delete(&mut s, &mut e);
            c.programmatic.set(false);
        }
    }
}

fn add_caret(c: &Card) {
    if c.caret.borrow().is_some() {
        return;
    }
    let mut end = c.buffer.end_iter();
    c.programmatic.set(true);
    let anchor = c.buffer.create_child_anchor(&mut end);
    c.programmatic.set(false);
    let caret = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    caret.add_css_class("voice-caret");
    caret.set_size_request(2, 15);
    c.draft.add_child_at_anchor(&caret, &anchor);
    *c.caret.borrow_mut() = Some(anchor);
}

fn shown_text(c: &Card) -> String {
    c.buffer
        .text(&c.buffer.start_iter(), &c.buffer.end_iter(), false)
        .to_string()
}

/// Make the buffer hold `text` (appending when it only grew), tagged.
fn put_text(c: &Card, text: &str, changed: &[Range<usize>]) {
    remove_caret(c);
    c.programmatic.set(true);
    let now = shown_text(c);
    if now != text {
        if !now.is_empty() && text.starts_with(&now) {
            let mut end = c.buffer.end_iter();
            c.buffer.insert(&mut end, &text[now.len()..]);
        } else {
            c.buffer.set_text(text);
        }
    }
    c.programmatic.set(false);
    retag(c, text, changed);
}

// ---------------------------------------------------------------------------
// Rendering a ComposerView
// ---------------------------------------------------------------------------

fn paint_heard(c: &Card) {
    let v = c.last.borrow();
    let Some(v) = v.as_ref() else { return };
    let words = [v.heard.as_str(), v.tentative.as_str()];
    let mut markup = escape(words[0]);
    if !words[1].is_empty() {
        if !markup.is_empty() {
            markup.push(' ');
        }
        markup.push_str(&format!(
            "<span foreground=\"{}\">{}</span>",
            crate::tokens::rgb_now("fg3"),
            escape(words[1])
        ));
    }
    let any = !words[0].is_empty() || !words[1].is_empty();
    c.heard_box.set_visible(any);
    c.heard.set_markup(&markup);
    let folded = c.fold_override.get().unwrap_or(c.model_fold.get());
    c.heard.set_wrap(!folded);
    c.heard.set_ellipsize(if folded {
        gtk::pango::EllipsizeMode::End
    } else {
        gtk::pango::EllipsizeMode::None
    });
    c.heard.set_lines(if folded { 1 } else { -1 });
    // `show` / `hide` only when the draft is there to make room (folded by
    // the model), and while there are words.
    c.heard_toggle.set_visible(any && c.model_fold.get());
    c.heard_toggle
        .set_label(if folded { "show" } else { "hide" });
}

fn paint_hints(c: &Card, v: &ComposerView) {
    if *c.hint_texts.borrow() == v.hints {
        // Only the warning colour may differ.
        paint_warn(c, v.hint_warn);
        return;
    }
    *c.hint_texts.borrow_mut() = v.hints.clone();
    while let Some(ch) = c.hints.first_child() {
        c.hints.remove(&ch);
    }
    for h in &v.hints {
        let words: Vec<&str> = h.split(' ').collect();
        let key = match words.first().copied() {
            Some("Hold" | "Release" | "Press") if words.len() > 1 => Some(1),
            Some("Esc") => Some(0),
            _ => None,
        };
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        b.add_css_class("voice-hint");
        match key {
            Some(k) => {
                if k > 0 {
                    b.append(&gtk::Label::new(Some(&words[..k].join(" "))));
                }
                b.append(&kbd(words[k]));
                if words.len() > k + 1 {
                    b.append(&gtk::Label::new(Some(&words[k + 1..].join(" "))));
                }
            }
            None => b.append(&gtk::Label::new(Some(h))),
        }
        c.hints.append(&b);
    }
    paint_warn(c, v.hint_warn);
}

fn paint_warn(c: &Card, warn: bool) {
    // The second hint is the warning (`Esc again to discard`).
    let second = c.hints.first_child().and_then(|f| f.next_sibling());
    let mut w = c.hints.first_child();
    while let Some(h) = w {
        w = h.next_sibling();
        h.remove_css_class("warn");
    }
    if let (true, Some(s)) = (warn, second) {
        s.add_css_class("warn");
    }
}

fn set_class(w: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if on {
        w.add_css_class(class);
    } else {
        w.remove_css_class(class);
    }
}

fn target_texts(pane: &str) -> (String, String) {
    let Some(app) = try_app() else {
        return (String::new(), String::new());
    };
    let m = app.model.borrow();
    let Some(p) = m.pane(pane) else {
        return (pane.to_string(), String::new());
    };
    let wt = m
        .locate(pane)
        .and_then(|l| m.worktree(&l.source, &l.worktree).map(|w| w.name.clone()))
        .unwrap_or_default();
    (rade_core::chrome::header_name(p).to_string(), wt)
}

/// Show the composer on the Pane `pane_id`.
pub fn open(pane_id: &str) {
    if let Some(c) = card() {
        if c.pane == pane_id {
            return;
        }
        close();
    }
    let c = build(pane_id);
    let (name, wt) = target_texts(pane_id);
    c.target_name.set_label(&name);
    c.target_where.set_label(&wt);
    c.target_where.set_visible(!wt.is_empty());
    CARD.with(|s| *s.borrow_mut() = Some(c.clone()));
    if let Some(slot) = slot_of(pane_id) {
        mount(&c, &slot);
    }
    // Compact bodies: the card fills the body and scrolls (D15).
    {
        let w = Rc::downgrade(&c);
        glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
            let Some(c) = w.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if card().is_none_or(|k| !Rc::ptr_eq(&k, &c)) {
                return glib::ControlFlow::Break;
            }
            fit(&c);
            size_draft(&c);
            glib::ControlFlow::Continue
        });
    }
    // The card takes the keyboard.
    {
        let w = Rc::downgrade(&c);
        glib::idle_add_local_once(move || {
            if let Some(c) = w.upgrade() {
                focus(&c);
            }
        });
    }
    sync_primary();
    refresh_mics();
}

fn focus(c: &Card) {
    if c.editable.get() {
        c.draft.grab_focus();
    } else {
        c.composer.grab_focus();
    }
}

/// Give the draft box the height of its text at its width (70 px minimum,
/// 300 px maximum, then it scrolls).
fn size_draft(c: &Card) {
    if c.compact.get() {
        return;
    }
    let w = c.scroller.width();
    if w <= 0 {
        return;
    }
    let (_, nat, _, _) = c.draft.measure(gtk::Orientation::Vertical, w);
    let h = nat.clamp(70, 300);
    if h != c.draft_h.replace(h) {
        c.scroller.set_min_content_height(h);
    }
}

fn fit(c: &Card) {
    let h = c.root.parent().map_or(0, |p| p.height());
    let compact = h > 0 && h < COMPACT_BELOW;
    if compact == c.compact.get() {
        return;
    }
    c.compact.set(compact);
    set_class(&c.composer, "compact", compact);
    c.root.set_valign(if compact {
        gtk::Align::Fill
    } else {
        gtk::Align::End
    });
    c.root.set_vexpand(compact);
    if let Some(clamp) = c.root.first_child() {
        clamp.set_vexpand(compact);
    }
    c.composer.set_vexpand(compact);
    c.scroller.set_vexpand(compact);
    c.scroller
        .set_min_content_height(if compact { 0 } else { 70 });
    c.draft_h.set(0);
}

/// Remove the composer.
pub fn close() {
    let Some(c) = CARD.with(|s| s.borrow_mut().take()) else {
        return;
    };
    match c.root.parent().map(|p| p.downcast::<gtk::Overlay>()) {
        Some(Ok(o)) => o.remove_overlay(&c.root),
        Some(Err(_)) => c.root.unparent(),
        None => {}
    }
    crate::chrome::set_voice_rec(None);
    sync_primary();
    refresh_mics();
}

/// Send is the window's one primary button while the card is open (D16):
/// Commit and Open terminal re-check theirs.
fn sync_primary() {
    crate::changes::sync_primary();
    crate::home_view::sync_primary();
}

fn setup_link() {
    // The Settings page, filtered `voice`.
    let _ = crate::settings_view::debug(&json!({"op": "app_settings_open"}));
    let _ = crate::settings_view::debug(&json!({"op": "app_settings_search", "arg": "voice"}));
}

/// Draw `view`.
pub fn render(view: &ComposerView) {
    if !view.open {
        close();
        return;
    }
    let Some(c) = card() else { return };
    ensure_tags(&c);
    *c.last.borrow_mut() = Some(view.clone());
    c.model_fold.set(view.heard_folded);
    if c.fold_override.get().is_some() && c.fold_override.get() == Some(view.heard_folded) {
        c.fold_override.set(None);
    }

    set_class(&c.composer, "listening", view.listening);
    for d in ["rec", "run", "ready", "err", "idle"] {
        set_class(&c.dot, d, d == view.dot);
    }
    c.state.set_label(&view.label);
    c.time.set_label(&view.time);
    c.meter.set_visible(view.listening);
    c.meter.queue_draw();
    paint_heard(&c);

    // Draft text: replaced when the composer (not the user) changed it.
    let editable = view.draft_editable;
    if c.version.get() != Some(view.draft_version) {
        c.version.set(Some(view.draft_version));
        put_text(&c, &view.draft, &view.changed);
        if !editable {
            let end = c.buffer.end_iter();
            c.buffer.place_cursor(&end);
            let mut it = c.buffer.end_iter();
            c.draft.scroll_to_iter(&mut it, 0.0, false, 0.0, 1.0);
        }
    } else if shown_text(&c) == view.draft {
        // The same text: the changed marks may differ.
        let text = view.draft.clone();
        retag(&c, &text, &view.changed);
    }
    if view.caret {
        add_caret(&c);
    } else {
        remove_caret(&c);
    }
    c.draft.set_editable(editable);
    c.draft.set_cursor_visible(editable);
    set_class(&c.draft, "stale", view.stale);
    let was = c.editable.replace(editable);
    if editable && !was {
        c.draft.grab_focus();
        let end = c.buffer.end_iter();
        c.buffer.place_cursor(&end);
    } else if !editable && was {
        c.composer.grab_focus();
    }

    size_draft(&c);
    {
        let w = Rc::downgrade(&c);
        glib::idle_add_local_once(move || {
            if let Some(c) = w.upgrade() {
                size_draft(&c);
            }
        });
    }

    // Placeholder.
    match &view.placeholder {
        Some(t) if shown_text(&c).is_empty() => {
            c.placeholder.set_label(t);
            c.placeholder.set_visible(true);
        }
        _ => c.placeholder.set_visible(false),
    }

    // Activity line.
    let has_act = !view.activity.is_empty();
    c.activity.set_visible(has_act);
    set_class(&c.activity, "error", view.activity_error);
    c.act_text.set_label(&view.activity);
    let icon = match view.activity_icon {
        ActivityIcon::None => None,
        ActivityIcon::Read => Some("ph-file-text-symbolic"),
        ActivityIcon::Search => Some("ph-magnifying-glass-symbolic"),
        ActivityIcon::List => Some("ph-list-symbolic"),
        ActivityIcon::Info => Some("ph-info-symbolic"),
        ActivityIcon::Warning => Some("ph-warning-diamond-symbolic"),
    };
    c.act_icon.set_visible(icon.is_some());
    if let Some(i) = icon {
        c.act_icon.set_icon_name(Some(i));
    }
    c.act_link.set_visible(has_act && view.setup_link);
    match &view.step {
        Some(s) => {
            c.act_step.set_label(s);
            c.act_step.set_visible(true);
        }
        None => c.act_step.set_visible(false),
    }
    // The text takes the room left of the step: no step, the link follows it.
    c.act_text.set_hexpand(false);
    c.act_step.set_hexpand(true);

    // Footer.
    paint_hints(&c, view);
    c.send.set_sensitive(view.send_enabled);
    c.paste.set_sensitive(view.paste_enabled);
    c.send_kbd.set_visible(view.send_enabled);
    // Insensitive buttons at 0.45 (spec I.23).
    c.send
        .set_opacity(if view.send_enabled { 1.0 } else { 0.45 });
    c.paste
        .set_opacity(if view.paste_enabled { 1.0 } else { 0.45 });

    crate::chrome::set_voice_rec(view.recording.then_some(view.time.as_str()));
    refresh_mics();
}

/// The newest meter levels, 0..1, oldest first.
pub fn set_levels(levels: &[f32]) {
    let Some(c) = card() else { return };
    let from = levels.len().saturating_sub(BARS);
    *c.levels.borrow_mut() = levels[from..].to_vec();
    c.meter.queue_draw();
}

/// Streamed draft text after what is shown.
pub fn append_draft(text: &str) {
    let Some(c) = card() else { return };
    let mut now = shown_text(&c);
    now.push_str(text);
    let changed = c.tags.borrow().changed.clone();
    put_text(&c, &now, &changed);
}

/// Replace the draft text and its tags.
pub fn set_draft(text: &str, tags: &DraftTags) {
    let Some(c) = card() else { return };
    put_text(&c, text, &tags.changed);
}

/// The draft box's text, edits included.
pub fn draft_text() -> String {
    card().map(|c| shown_text(&c)).unwrap_or_default()
}

/// `debug.voice info`'s widget facts: `heard_folded`, `draft_tags`
/// (`code` / `changed` / `unsure` as `[start, end]` byte pairs), `hints`.
pub fn debug_info() -> Value {
    let Some(c) = card() else {
        return Value::Null;
    };
    let pairs = |v: &[Range<usize>]| {
        v.iter()
            .map(|r| json!([r.start, r.end]))
            .collect::<Vec<_>>()
    };
    let t = c.tags.borrow();
    json!({
        "heard_folded": c.fold_override.get().unwrap_or(c.model_fold.get()),
        "draft_tags": {
            "code": pairs(&t.code),
            "changed": pairs(&t.changed),
            "unsure": pairs(&t.unsure),
        },
        "hints": *c.hint_texts.borrow(),
        "compact": c.compact.get(),
        "focus": if c.draft.is_focus() { "draft" } else if c.composer.is_focus() { "card" } else { "elsewhere" },
    })
}

// ---------------------------------------------------------------------------
// debug.voice scene
// ---------------------------------------------------------------------------

const HEARD_A: &str = "\u{1eeb} th\u{ec} c\u{e1}i ph\u{1ea7}n promo code \u{e1}, n\u{f3} \u{111}ang b\u{1ecb} tr\u{1eeb} hai l\u{1ea7}n, \u{e0} \u{1edf} c\u{e1}i use totals";
const TENT_A: &str = "\u{e1} s\u{1eed}a cho n\u{f3}";
const HEARD_C: &str = "\u{1eeb} th\u{ec} c\u{e1}i ph\u{1ea7}n promo code \u{e1}, n\u{f3} \u{111}ang b\u{1ecb} tr\u{1eeb} hai l\u{1ea7}n, \u{e0} \u{1edf} c\u{e1}i useTotals \u{e1}, s\u{1eed}a cho n\u{f3} ch\u{1ec9} tr\u{1eeb} m\u{1ed9}t l\u{1ea7}n th\u{f4}i, \u{e0} m\u{e0} nh\u{1edb} vi\u{1ebf}t test cho c\u{e1}i tr\u{1b0}\u{1edd}ng h\u{1ee3}p percent lu\u{f4}n";
const HEARD_E: &str = "\u{e0} kh\u{f4}ng, test th\u{ec} \u{111}\u{1ec3} cho agent tests l\u{e0}m, b\u{1ecf} c\u{e1}i d\u{f2}ng test \u{111}i, th\u{ea}m l\u{e0} \u{111}\u{1eeb}ng \u{111}\u{1ed5}i c\u{e1}i API c\u{1ee7}a applyPromo";
const DRAFT_C: &str = "Fix the promo discount being applied twice: `useTotals()` subtracts it again after `applyPromo()` for `promo.kind === \"percent\"`.\n- Apply it once, in `applyPromo()` (`src/checkout/promo.ts:31`).\n- Add a test for a percent promo in `useTotals.test.ts`.";
const DRAFT_E_HEAD: &str = "Fix the promo discount being applied twice: `useTotals()` subtracts it again after `applyPromo()` for `promo.kind === \"percent\"`.\n- Apply it once, in `applyPromo()` (`src/checkout/promo.ts:31`).\n- ";
const BULLET_E1: &str = "Keep `applyPromo()`'s signature unchanged.";
const BULLET_E2: &str = "No tests in this change (the tests agent writes them).";
const UNSURE_E: &str = "[?] \"agent tests\"";

fn scene_view(arg: &str) -> Option<(ComposerView, Vec<f32>)> {
    let base = ComposerView {
        open: true,
        state: Some(State::Ready),
        mode: Some(Mode::Hold),
        label: "Ready".into(),
        dot: "ready",
        listening: false,
        time: String::new(),
        heard: String::new(),
        tentative: String::new(),
        heard_folded: true,
        draft: String::new(),
        draft_version: 1,
        changed: Vec::new(),
        draft_editable: true,
        stale: false,
        caret: false,
        placeholder: None,
        activity: String::new(),
        activity_icon: ActivityIcon::None,
        activity_error: false,
        setup_link: false,
        step: None,
        hints: vec!["Hold F8 to revise".into(), "Esc discard".into()],
        hint_warn: false,
        send_enabled: true,
        paste_enabled: true,
        revision: 0,
        error: None,
        recording: false,
    };
    let wt = try_app()
        .and_then(|a| {
            let m = a.model.borrow();
            let id = m.active_tab()?.focused.clone();
            let l = m.locate(&id)?;
            m.worktree(&l.source, &l.worktree).map(|w| w.name.clone())
        })
        .unwrap_or_default();
    Some(match arg {
        "a" => (
            ComposerView {
                state: Some(State::Listening),
                label: "Listening".into(),
                dot: "rec",
                listening: true,
                time: "0:06".into(),
                heard: HEARD_A.into(),
                tentative: TENT_A.into(),
                heard_folded: false,
                draft_editable: false,
                placeholder: Some("The prompt appears here when you let go of F8.".into()),
                activity: format!("Reading the session and {wt} while you speak"),
                activity_icon: ActivityIcon::Info,
                hints: vec!["Release F8 to draft".into(), "Esc cancel".into()],
                send_enabled: false,
                paste_enabled: false,
                recording: true,
                ..base
            },
            [
                3, 5, 8, 12, 9, 6, 10, 14, 11, 7, 4, 6, 9, 13, 10, 8, 5, 3, 6, 10, 12, 8, 5, 4, 3,
                5, 7, 4,
            ]
            .iter()
            .map(|h| (*h as f32 - 3.0) / 13.0)
            .collect(),
        ),
        "c" => (
            ComposerView {
                time: "0:11 \u{b7} 3.2 s".into(),
                heard: HEARD_C.into(),
                draft: DRAFT_C.into(),
                ..base
            },
            Vec::new(),
        ),
        "e" => {
            let mut draft = DRAFT_E_HEAD.to_string();
            let s1 = draft.len();
            draft.push_str(BULLET_E1);
            let e1 = draft.len();
            draft.push_str("\n- ");
            let s2 = draft.len();
            draft.push_str(BULLET_E2);
            let e2 = draft.len();
            draft.push(' ');
            draft.push_str(UNSURE_E);
            (
                ComposerView {
                    time: "revision 2 \u{b7} 1.9 s".into(),
                    heard: HEARD_E.into(),
                    draft,
                    changed: vec![s1..e1, s2..e2],
                    revision: 2,
                    ..base
                },
                Vec::new(),
            )
        }
        _ => return None,
    })
}

/// `debug.voice scene a|c|e`: fill the composer of the focused Agent Pane with
/// the mockup's data and state (no processes). `off` closes it.
pub fn debug_scene(arg: &str) -> rade_core::rpc::Result<serde_json::Value> {
    use rade_core::rpc::Error;
    if arg == "off" || arg == "close" {
        close();
        return Ok(json!({"scene": null}));
    }
    let (view, levels) = scene_view(arg)
        .ok_or_else(|| Error::usage(format!("unknown scene '{arg}' (a, c, e, off)")))?;
    let app = try_app().ok_or_else(|| Error::usage("the app is not running"))?;
    let pane = {
        let m = app.model.borrow();
        let id = m
            .active_tab()
            .map(|t| t.focused.clone())
            .ok_or_else(|| Error::usage("no focused Pane"))?;
        let p = m.pane(&id).ok_or_else(|| Error::usage("no focused Pane"))?;
        if p.agent_name().is_none() && p.mark().is_none() {
            return Err(Error::usage("Voice works on Agent Panes"));
        }
        id
    };
    close();
    open(&pane);
    set_levels(&levels);
    render(&view);
    Ok(json!({"scene": arg, "pane": pane}))
}
