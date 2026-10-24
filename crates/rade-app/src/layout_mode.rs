//! The window's layout below 1440 px and at large text (PRD R111, spec
//! sheets `## 1280 px` / `## Text scale 200 %`). The rules are
//! `rade_core::chrome::layout_mode` and friends; this module watches the
//! window's width and GTK's `gtk-xft-dpi` (GNOME's text-scaling factor) and
//! applies them: the Inspector floats over the center (narrow), the
//! Navigator becomes a 48 px icon rail and the center shows one Pane
//! (large text), and RADE's px font sizes follow the text scale.

use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::chrome::{
    layout_mode, scale_sheet, text_scale_of_dpi, LayoutMode, NAVIGATOR_WIDTH, RAIL_WIDTH,
};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};

struct Ui {
    window: gtk::Window,
    nav: adw::OverlaySplitView,
    inspector: adw::OverlaySplitView,
    rail: gtk::Widget,
    provider: gtk::CssProvider,
    css: String,
    /// The model's Inspector toggle from before the window went narrow,
    /// given back when it is wide again.
    saved_inspector: Cell<Option<bool>>,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
    static MODE: Cell<LayoutMode> = Cell::new(LayoutMode::default());
    static SCALE: Cell<f64> = const { Cell::new(1.0) };
    static WIDTH: Cell<i32> = const { Cell::new(0) };
    static HEIGHT: Cell<i32> = const { Cell::new(0) };
    static QUEUED: Cell<bool> = const { Cell::new(false) };
}

/// The layout in force.
pub fn mode() -> LayoutMode {
    MODE.with(Cell::get)
}

/// The text scale in force (1.0 = 100 %).
pub fn text_scale() -> f64 {
    SCALE.with(Cell::get)
}

/// The window's width as asked for (0 before the first allocation): its
/// allocation, or the size it was given when a minimum wider than that
/// made it grow (the layout must then shrink to fit it).
pub fn window_width() -> i32 {
    let laid = WIDTH.with(Cell::get);
    let asked = UI.with(|u| {
        u.borrow().as_ref().and_then(|u| {
            let w = &u.window;
            (!w.is_maximized() && !w.is_fullscreen() && w.default_width() > 0)
                .then(|| w.default_width())
        })
    });
    match asked {
        Some(a) if laid > 0 => laid.min(a),
        _ => laid,
    }
}

/// The window's height as laid out (0 before the first allocation).
pub fn window_height() -> i32 {
    HEIGHT.with(Cell::get)
}

/// The center's width: the window less the Navigator (or rail) and a docked
/// Inspector.
pub fn center_width() -> i32 {
    let m = mode();
    let nav = if m.rail { RAIL_WIDTH } else { NAVIGATOR_WIDTH };
    let docked = UI.with(|u| {
        u.borrow()
            .as_ref()
            .is_some_and(|u| !u.inspector.is_collapsed() && u.inspector.shows_sidebar())
    });
    window_width()
        - nav
        - if docked {
            crate::chrome::INSPECTOR_WIDTH
        } else {
            0
        }
}

/// Wire the layout to the window. `sizer` is an invisible overlay child
/// whose `resize` tells the window's width; `css` is the light sheet the
/// `provider` holds at scale 1.
pub fn install(
    window: &gtk::Window,
    sizer: &gtk::DrawingArea,
    nav: &adw::OverlaySplitView,
    inspector: &adw::OverlaySplitView,
    rail: &gtk::Widget,
    provider: &gtk::CssProvider,
    css: String,
) {
    // Widths in px: the default `sp` grows with the text scale.
    for s in [nav, inspector] {
        s.set_sidebar_width_unit(adw::LengthUnit::Px);
    }
    rail.set_visible(false);
    UI.with(|u| {
        *u.borrow_mut() = Some(Ui {
            window: window.clone(),
            nav: nav.clone(),
            inspector: inspector.clone(),
            rail: rail.clone(),
            provider: provider.clone(),
            css,
            saved_inspector: Cell::new(None),
        })
    });
    sizer.connect_resize(|_, w, h| {
        HEIGHT.with(|c| c.set(h));
        if WIDTH.with(|c| c.replace(w)) != w {
            queue();
        }
    });
    if let Some(settings) = gtk::Settings::default() {
        settings.connect_gtk_xft_dpi_notify(|_| queue());
    }
    window.connect_default_width_notify(|_| queue());
    // The floating Inspector is not modal (as when docked): clicks on the
    // center reach it instead of libadwaita's shield, which would close the
    // Inspector and swallow the click.
    let content_bin = inspector.content().and_then(|c| c.parent());
    let sidebar_bin = inspector.sidebar().and_then(|c| c.parent());
    let mut c = inspector.first_child();
    while let Some(k) = c {
        c = k.next_sibling();
        if Some(&k) != content_bin.as_ref() && Some(&k) != sidebar_bin.as_ref() {
            k.set_can_target(false);
        }
    }
    // Escape inside the floating Inspector closes it: the model follows.
    inspector.connect_show_sidebar_notify(|s| {
        let Some(app) = crate::app::try_app() else {
            return;
        };
        if s.is_collapsed()
            && !app.syncing.get()
            && app.model.borrow().file_tree != s.shows_sidebar()
        {
            let on = s.shows_sidebar();
            app.update(|m| m.file_tree = on);
        }
    });
    queue();
}

/// Re-evaluate after the current allocation (never change widgets inside it).
fn queue() {
    if QUEUED.with(|q| q.replace(true)) {
        return;
    }
    gtk::glib::idle_add_local_once(|| {
        QUEUED.with(|q| q.set(false));
        update();
    });
}

fn update() {
    let scale = gtk::Settings::default()
        .map(|s| text_scale_of_dpi(s.gtk_xft_dpi()))
        .unwrap_or(1.0);
    let width = window_width();
    if width <= 0 {
        return;
    }
    let new = layout_mode(width, scale);
    let old = MODE.with(|m| m.replace(new));
    let old_scale = SCALE.with(|s| s.replace(scale));
    UI.with(|u| {
        let u = u.borrow();
        let Some(u) = u.as_ref() else { return };
        if (old_scale - scale).abs() > f64::EPSILON {
            u.provider.load_from_string(&scale_sheet(&u.css, scale));
            // Measure again once the new fonts are styled.
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(150), || {
                if let Some(app) = crate::app::try_app() {
                    app.chrome.refit();
                }
                crate::home_view::relayout();
                crate::usage_view::relayout();
            });
        }
        if old != new {
            apply(u, old, new);
        }
    });
    // Widths inside the titlebar, the Statusline and the pages follow the
    // window on every change, not only a mode change.
    if let Some(app) = crate::app::try_app() {
        if old != new {
            app.sync();
        }
        app.chrome.fit();
    }
    crate::home_view::relayout();
    crate::usage_view::relayout();
}

fn apply(u: &Ui, old: LayoutMode, new: LayoutMode) {
    // The Navigator: the rail, the full one floating over it on demand.
    u.rail.set_visible(new.rail);
    for (split, class, on) in [
        (&u.nav, "navigator-overlay", new.rail),
        (&u.inspector, "inspector-overlay", new.inspector_overlay),
    ] {
        if let Some(w) = split.sidebar() {
            if on {
                w.add_css_class(class);
            } else {
                w.remove_css_class(class);
            }
        }
    }
    u.nav.set_collapsed(new.rail);
    u.nav.set_show_sidebar(!new.rail);
    // The Inspector floats, hidden until asked for; wide again, it comes
    // back as the user left it.
    if new.inspector_overlay != old.inspector_overlay {
        let Some(app) = crate::app::try_app() else {
            u.inspector.set_collapsed(new.inspector_overlay);
            return;
        };
        let shown = app.model.borrow().file_tree;
        if new.inspector_overlay {
            u.saved_inspector.set(Some(shown));
            u.inspector.set_collapsed(true);
            if shown {
                app.update(|m| m.file_tree = false);
            }
        } else {
            u.inspector.set_collapsed(false);
            if let Some(was) = u.saved_inspector.take() {
                if was != shown {
                    app.update(|m| m.file_tree = was);
                }
            }
        }
    }
}

/// Show or hide the Inspector. Floating (narrow window, large text) it is
/// not modal: the center stays focusable and keeps the keyboard, as when it
/// is docked. libadwaita's collapsed split view makes the content
/// unfocusable and moves the focus into the sidebar; both are undone here.
pub fn show_inspector(split: &adw::OverlaySplitView, window: &gtk::Window, on: bool) {
    let before = gtk::prelude::GtkWindowExt::focus(window);
    split.set_show_sidebar(on);
    if !split.is_collapsed() {
        return;
    }
    let Some(bin) = split.content().and_then(|c| c.parent()) else {
        return;
    };
    bin.set_can_focus(true);
    if let Some(f) = before {
        if f.is_ancestor(&bin) && gtk::prelude::GtkWindowExt::focus(window).as_ref() != Some(&f) {
            f.grab_focus();
        }
    }
}

/// A rail icon: the full Navigator floats over the center.
pub fn open_navigator() {
    UI.with(|u| {
        if let Some(u) = u.borrow().as_ref() {
            u.nav.set_show_sidebar(true);
        }
    });
}

/// Large text: every label under `w` that neither wraps nor ellipsizes
/// wraps at word boundaries instead of widening its container past the
/// window (R111.2; GTK still gives each its natural width when it fits).
pub fn wrap_labels(w: &gtk::Widget) {
    let mut c = w.first_child();
    while let Some(k) = c {
        c = k.next_sibling();
        if let Some(l) = k.downcast_ref::<gtk::Label>() {
            if !l.wraps() && l.ellipsize() == gtk::pango::EllipsizeMode::None {
                l.set_wrap(true);
                l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            }
        }
        wrap_labels(&k);
    }
}

/// Close the floating Navigator (rail mode).
pub fn close_navigator() {
    UI.with(|u| {
        if let Some(u) = u.borrow().as_ref() {
            if u.nav.is_collapsed() {
                u.nav.set_show_sidebar(false);
            }
        }
    });
}

/// `debug.ui layout` / `chrome`: what the layout is now.
pub fn debug() -> Value {
    let m = mode();
    let (nav_overlay, inspector_overlay_shown) = UI.with(|u| {
        u.borrow().as_ref().map_or((false, false), |u| {
            (
                u.nav.is_collapsed() && u.nav.shows_sidebar(),
                u.inspector.is_collapsed() && u.inspector.shows_sidebar(),
            )
        })
    });
    json!({
        "mode": m.name(),
        "width": window_width(),
        "text_scale": text_scale(),
        "inspector_overlay": m.inspector_overlay,
        "rail": m.rail,
        "one_pane": m.one_pane,
        "large_text": m.large_text,
        "navigator_overlay_shown": nav_overlay,
        "inspector_overlay_shown": inspector_overlay_shown,
        "center_width": center_width(),
    })
}
