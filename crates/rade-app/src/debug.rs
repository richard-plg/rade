//! Debug helpers for the E2E and live harnesses (`rade debug …`, ADR-0006):
//! screenshots, `debug.window`, `debug.viewer`.

use crate::app::{app, App};
use crate::handlers::terminal;
use adw::prelude::*;
use gtk::{gdk, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::rpc::{Error, ErrorKind, Result};
use rade_ghostty_sys::GHOSTTY_POINT_VIEWPORT;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

/// Render the window's content to a PNG (the render node of the window's
/// top child → the window's renderer → texture), including the Ghostty GL
/// areas and libadwaita dialogs (they live in that child, beside the
/// content, M11).
pub fn screenshot(window: &adw::ApplicationWindow, path: &Path) -> Result<()> {
    let content = window
        .content()
        .ok_or_else(|| Error::internal("window has no content"))?;
    let (w, h) = (content.width(), content.height());
    if w <= 0 || h <= 0 {
        return Err(Error::internal("window is not allocated yet"));
    }
    let mut top: gtk::Widget = content.upcast();
    while let Some(p) = top
        .parent()
        .filter(|p| p != window.upcast_ref::<gtk::Widget>())
    {
        top = p;
    }
    let snapshot = gtk::Snapshot::new();
    window.snapshot_child(&top, &snapshot);
    // A code hover / peek popover is a surface of its own: drawn on top.
    crate::code_pop::paint_over(window.upcast_ref(), &snapshot);
    let node = snapshot
        .to_node()
        .ok_or_else(|| Error::internal("nothing to render"))?;
    let renderer = window
        .renderer()
        .ok_or_else(|| Error::internal("window has no renderer"))?;
    let texture = renderer.render_texture(&node, None);
    texture
        .save_to_png(path)
        .map_err(|e| Error::internal(format!("{}: {e}", path.display())))
}

/// Render one widget (e.g. an open popover menu, its own surface) to a PNG.
pub fn screenshot_widget(w: &gtk::Widget, path: &Path) -> Result<()> {
    let native = w
        .native()
        .ok_or_else(|| Error::internal("widget is not shown"))?;
    // The whole surface (a popover's shadow sits outside its allocation).
    let surface = native
        .surface()
        .ok_or_else(|| Error::internal("widget is not shown"))?;
    let (sw, sh) = (surface.width(), surface.height());
    if sw <= 0 || sh <= 0 {
        return Err(Error::internal("widget is not allocated yet"));
    }
    let (tx, ty) = native.surface_transform();
    let snapshot = gtk::Snapshot::new();
    snapshot.translate(&gtk::graphene::Point::new(tx as f32, ty as f32));
    gtk::WidgetPaintable::new(Some(w)).snapshot(&snapshot, w.width() as f64, w.height() as f64);
    let node = snapshot
        .to_node()
        .ok_or_else(|| Error::internal("nothing to render"))?;
    let renderer = native
        .renderer()
        .ok_or_else(|| Error::internal("widget has no renderer"))?;
    let bounds = gtk::graphene::Rect::new(0.0, 0.0, sw as f32, sh as f32);
    renderer
        .render_texture(&node, Some(&bounds))
        .save_to_png(path)
        .map_err(|e| Error::internal(format!("{}: {e}", path.display())))
}

#[derive(Deserialize)]
pub struct ViewerOp {
    pane: String,
    op: String,
    arg: Option<String>,
}

#[derive(Deserialize)]
pub struct WindowQuery {
    pane: Option<String>,
    text: Option<String>,
    caller: Option<String>,
    /// A widget on screen by CSS class or tooltip text (e.g. `rade-title-bar`,
    /// "Open in VS Code"); `nth` picks among several (default 0).
    widget: Option<String>,
    #[serde(default)]
    nth: usize,
}

/// Drawable widgets under `w` whose CSS classes or tooltip include `key`,
/// in tree order.
fn find_widgets(w: &gtk::Widget, key: &str, out: &mut Vec<gtk::Widget>) {
    if !w.is_drawable() {
        return;
    }
    let label = w
        .downcast_ref::<gtk::Button>()
        .and_then(|b| b.label())
        .is_some_and(|l| l == key);
    if label || w.has_css_class(key) || w.tooltip_text().is_some_and(|t| t == key) {
        out.push(w.clone());
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        find_widgets(&child, key, out);
        c = child.next_sibling();
    }
}

/// `debug.window` (live harness, tests/live): is the window the active
/// toplevel, its size, where the real pointer is (window coordinates, null
/// when it is not over the window), and optionally a Terminal's bounds and
/// the centre of the first `text` on its screen, so real mouse input can be
/// aimed and checked before it is sent.
pub fn window_info(app: &App, q: WindowQuery) -> Result<Value> {
    let win = &app.window;
    let (sx, sy) = win.surface_transform();
    let pointer = win.surface().and_then(|s| {
        let dev = s.display().default_seat()?.pointer()?;
        let (under, x, y) = dev.surface_at_position();
        (under.as_ref() == Some(&s)).then(|| json!({"x": x - sx, "y": y - sy}))
    });
    let pages = app.stack.pages();
    let page_minimums: Vec<Value> = (0..pages.n_items()).filter_map(|i| {
        let page = pages.item(i)?.downcast::<gtk::StackPage>().ok()?;
        let child = page.child();
        Some(json!({"name": page.name().map(|name| name.to_string()), "minimum_width": child.measure(gtk::Orientation::Horizontal, -1).0}))
    }).collect();
    let mut r = json!({
        "minimum_width": win.measure(gtk::Orientation::Horizontal, -1).0,
        "minimum_height": win.measure(gtk::Orientation::Vertical, -1).0,
        "content_minimum_width": app.stack.measure(gtk::Orientation::Horizontal, -1).0,
        "page_minimum_widths": page_minimums,
        "active": win.is_active(),
        "width": win.width(),
        "height": win.height(),
        "scale": win.scale_factor(),
        "fullscreen": win.is_fullscreen(),
        "maximized": win.is_maximized(),
        "pointer": pointer,
        "focus": focus_info(app),
        "banner": crate::status::banner_info(),
        "menu_open": crate::menus::menu_open(),
    });
    if let Some(key) = &q.widget {
        let mut found = Vec::new();
        find_widgets(win.upcast_ref(), key, &mut found);
        let w = found
            .get(q.nth)
            .ok_or_else(|| Error::not_found(format!("no widget '{key}' #{} on screen", q.nth)))?;
        let b = w
            .compute_bounds(win)
            .ok_or_else(|| Error::internal("widget has no bounds"))?;
        r["widget"] = json!({"x": b.x(), "y": b.y(), "width": b.width(), "height": b.height(), "count": found.len()});
    }
    if let Some(pane) = &q.pane {
        let (id, t) = terminal(app, pane, q.caller.as_deref())?;
        let b = t
            .root
            .compute_bounds(win)
            .ok_or_else(|| Error::internal(format!("pane {id} is not on screen")))?;
        r["pane"] =
            json!({"pane": id, "x": b.x(), "y": b.y(), "width": b.width(), "height": b.height()});
        if let Some(text) = &q.text {
            let screen = t
                .handle
                .read_text(GHOSTTY_POINT_VIEWPORT)
                .unwrap_or_default();
            let cols = t.handle.health().columns.max(1) as usize;
            let (row, col) = find_cell(&screen, text, cols)
                .ok_or_else(|| Error::not_found(format!("'{text}' is not on {id}'s screen")))?;
            let (x, y) = t
                .handle
                .cell_point(row as u32, col as u32)
                .ok_or_else(|| Error::internal(format!("pane {id} has not started")))?;
            let pt = t
                .handle
                .gl_area()
                .compute_point(win, &gtk::graphene::Point::new(x as f32, y as f32))
                .ok_or_else(|| Error::internal("no common ancestor"))?;
            r["target"] = json!({"x": pt.x(), "y": pt.y(), "row": row, "col": col});
        }
    }
    Ok(r)
}

/// The window's focus widget (kept even while the window is inactive, so
/// headless E2E can check it): its GTK type and the Pane it belongs to.
fn focus_info(app: &App) -> Value {
    let Some(f) = gtk::prelude::GtkWindowExt::focus(&app.window) else {
        return Value::Null;
    };
    // A menu item's label (GtkModelButton:text), for keyboard checks.
    let label = f.find_property("text").and_then(|_| {
        f.property_value("text")
            .get::<Option<String>>()
            .ok()
            .flatten()
    });
    let classes: Vec<String> = f.css_classes().iter().map(|c| c.to_string()).collect();
    json!({"widget": f.type_().name(), "label": label, "classes": classes, "tooltip": f.tooltip_text().map(|t| t.to_string()), "pane": crate::app::pane_of_widget(app, &f)})
}

/// Viewport cell of the first `needle` in `screen` (as `read_text` returns
/// it: soft-wrapped rows joined), for a Terminal `cols` wide.
pub fn find_cell(screen: &str, needle: &str, cols: usize) -> Option<(usize, usize)> {
    let mut row = 0;
    for line in screen.lines() {
        let width = line.chars().count();
        if let Some(b) = line.find(needle) {
            let at = line[..b].chars().count();
            return Some((row + at / cols, at % cols));
        }
        row += width.max(1).div_ceil(cols);
    }
    None
}

/// `debug.viewer` on the socket thread: the Viewer answers asynchronously
/// (Markdown asks its WebView). Ops: info, escape, back, open (the header's
/// Open button), source (HTML's Rendered / Source), js <script>.
pub fn viewer(v: ViewerOp) -> Result<Value> {
    let (tx, rx) = mpsc::channel::<Result<Value>>();
    glib::MainContext::default().invoke(move || {
        let app = app();
        let id = match app.model.borrow().resolve_pane(&v.pane, None) {
            Ok(id) => id,
            Err(e) => return drop(tx.send(Err(e))),
        };
        let Some(viewer) = app.viewers.borrow().get(&id).cloned() else {
            return drop(tx.send(Err(Error::usage(format!("pane {id} is not a Viewer")))));
        };
        let reply = move |r: Result<Value>| drop(tx.send(r));
        match (v.op.as_str(), v.arg) {
            ("info", _) => viewer.info(Box::new(move |val| reply(Ok(val)))),
            ("escape", _) => {
                crate::viewer::close_viewer(&id);
                reply(Ok(json!({})))
            }
            ("open", _) => {
                viewer.open_external();
                reply(Ok(json!({})))
            }
            // An HTML file: the header's Rendered / Source toggle (R114.2).
            ("source", _) => {
                viewer.toggle_source();
                reply(Ok(json!({})))
            }
            // Content zoom (R113.5): in | out | reset, like the header's buttons.
            ("content_zoom", Some(d)) => {
                viewer.step_zoom(match d.as_str() {
                    "in" => 1,
                    "out" => -1,
                    _ => 0,
                });
                reply(Ok(json!({})))
            }
            // The header's Zoom button (R113.1).
            ("zoom", _) => {
                crate::viewer::toggle_zoom(&id);
                reply(Ok(json!({})))
            }
            ("back", _) => {
                crate::viewer::go_back(&id);
                reply(Ok(json!({})))
            }
            ("forward", _) => {
                crate::viewer::go_forward(&id);
                reply(Ok(json!({})))
            }
            // Code navigation (R84): `place` `line:col`, `key` (`F12`,
            // `shift+F12`, `ctrl+F12`, `alt+shift+h`, `alt+Left`,
            // `alt+Right`) through the key handler, `ctrl_click` `line:col`
            // through the click handler's coordinates.
            ("place", Some(at)) | ("ctrl_click", Some(at)) => {
                let (l, c) = at.split_once(':').unwrap_or((&at, "1"));
                let (l, c) = (l.parse().unwrap_or(1), c.parse().unwrap_or(1));
                if v.op == "place" {
                    viewer.place(l, c);
                } else {
                    match viewer.coords_of(l, c) {
                        Some((x, y)) => {
                            return reply(Ok(json!({"navigated": viewer.ctrl_click(x, y)})))
                        }
                        None => return reply(Err(Error::usage("no such position"))),
                    }
                }
                reply(Ok(json!({})))
            }
            // Ctrl held with the pointer moving over `line:col` (0.7.1): the
            // name there shows as a link; `link_off` = Ctrl let go.
            ("link", Some(at)) => {
                let (l, c) = at.split_once(':').unwrap_or((&at, "1"));
                match viewer.coords_of(l.parse().unwrap_or(1), c.parse().unwrap_or(1)) {
                    Some(xy) => {
                        viewer.set_link(Some(xy));
                        reply(Ok(json!({})))
                    }
                    None => reply(Err(Error::usage("no such position"))),
                }
            }
            ("link_off", _) => {
                viewer.set_link(None);
                reply(Ok(json!({})))
            }
            // A selection `L:C-L:C` (as a drag would make).
            ("select", Some(r)) => {
                let pos = |s: &str| {
                    let (l, c) = s.split_once(':')?;
                    Some((l.parse::<u32>().ok()?, c.parse::<u32>().ok()?))
                };
                match r
                    .split_once('-')
                    .and_then(|(a, b)| Some((pos(a)?, pos(b)?)))
                {
                    Some((a, b)) => {
                        viewer.select(a, b);
                        reply(Ok(json!({})))
                    }
                    None => reply(Err(Error::usage("want L:C-L:C"))),
                }
            }
            // Ctrl held with the pointer resting on `line:col` (R90.1).
            ("hover", Some(at)) => {
                let (l, c) = at.split_once(':').unwrap_or((&at, "1"));
                match viewer.coords_of(l.parse().unwrap_or(1), c.parse().unwrap_or(1)) {
                    Some((x, y)) => {
                        viewer.hover_at(x, y);
                        reply(Ok(json!({})))
                    }
                    None => reply(Err(Error::usage("no such position"))),
                }
            }
            ("outline", _) => {
                viewer.outline_click();
                reply(Ok(json!({})))
            }
            ("locate", Some(at)) => {
                let (l, c) = at.split_once(':').unwrap_or((&at, "1"));
                match viewer.window_point(
                    &app.window,
                    l.parse().unwrap_or(1),
                    c.parse().unwrap_or(1),
                ) {
                    Some((x, y)) => reply(Ok(json!({"x": x, "y": y}))),
                    None => reply(Err(Error::usage("no such position on screen"))),
                }
            }
            ("key", Some(k)) => {
                let mut mods = gdk::ModifierType::empty();
                let mut name = k.as_str();
                while let Some((m, rest)) = name.split_once('+') {
                    mods |= match m {
                        "ctrl" => gdk::ModifierType::CONTROL_MASK,
                        "shift" => gdk::ModifierType::SHIFT_MASK,
                        "alt" => gdk::ModifierType::ALT_MASK,
                        _ => gdk::ModifierType::empty(),
                    };
                    name = rest;
                }
                match gdk::Key::from_name(name) {
                    Some(key) => reply(Ok(json!({"handled": viewer.on_key(key, mods)}))),
                    None => reply(Err(Error::usage(format!("unknown key {name}")))),
                }
            }
            ("js", Some(js)) => match viewer.markdown() {
                Some(md) => md.eval(
                    &js,
                    Box::new(move |r| {
                        reply(r.map(|s| json!({"result": s})).map_err(Error::internal))
                    }),
                ),
                None => reply(Err(Error::usage("not showing Markdown"))),
            },
            (op, _) => reply(Err(Error::usage(format!("unknown viewer op '{op}'")))),
        }
    });
    rx.recv_timeout(Duration::from_secs(10))
        .unwrap_or_else(|_| Err(Error::new(ErrorKind::Timeout, "viewer did not answer")))
}

/// `debug.review` (`{"pane", "op", "arg"?}`): see `ReviewView::debug`.
pub fn review(app: &App, o: ViewerOp) -> Result<Value> {
    let id = app.model.borrow().resolve_pane(&o.pane, None)?;
    let v = app.reviews.borrow().get(&id).cloned();
    let v = v.ok_or_else(|| Error::usage(format!("pane {id} is not a Review")))?;
    v.debug(&o.op, o.arg.as_deref())
}

/// `debug.ui` (M14, R20): `{"op", "pane"?, "arg"?}` → the redesign as
/// shown. `sidebar` = its rows; `inbox` = the Inbox; `toggle_source`
/// (`arg`) = a click on that Source's chevron; `click_agent` (`pane`) = a click on that
/// Agent's row; `pane` (`pane`) = its Attention Ring and dimming; `chrome`
/// (default) = header title, Right Panel, Status Bar, Toasts; `toast_show`
/// clicks the last Toast's Show; `right_page` (`arg`) switches the Right
/// Panel; `geometry` (`widget`, `nth`?, `all`?) = a widget's measurements
/// (R102.5, crate::geometry) and only that. Each other answer includes
/// `chrome` (breadcrumb, Review count, Statusline cells, Inspector labels).
pub fn ui(app: &App, p: &Value) -> Result<Value> {
    let s = |k: &str| p.get(k).and_then(Value::as_str);
    let op = s("op").unwrap_or("chrome");
    // R117: Agent Overview.
    if op == "overview" {
        return crate::overview_view::debug(p);
    }
    // R112: the app's Settings page.
    if op == "app_settings_accounts" {
        return crate::settings_accounts::debug(p);
    }
    if op.starts_with("app_settings") {
        return crate::settings_view::debug(p);
    }
    if op.starts_with("settings") {
        return crate::project_settings::debug(p);
    }
    // R106: New Project / Add Source / Change base / Project settings.
    if op.starts_with("dialog") {
        return crate::project_dialogs::debug(p);
    }
    // R102.5: a widget's measurements (spec sheets).
    if op == "geometry" {
        return crate::geometry::op(app.window.upcast_ref(), p);
    }
    // R111: labels that do not fit, scrolled windows that scroll sideways.
    if op == "clipped" {
        return Ok(crate::geometry::clipped(app.window.upcast_ref()));
    }
    if op == "hscroll" {
        return Ok(crate::geometry::hscroll(app.window.upcast_ref()));
    }
    // R111-c, R111-d: timings. `arg` = (none) summary | reset [<name>] |
    // scroll [<frames>] (the Navigator, end to end) | usage (open the Usage
    // page, timed to its first frame with content) | new_menu (the same for
    // the + menu).
    if op == "perf" {
        let parts: Vec<&str> = s("arg").unwrap_or("").split_whitespace().collect();
        match parts.as_slice() {
            [] => {}
            ["reset"] => crate::perf::reset(None),
            ["reset", n] => crate::perf::reset(Some(n)),
            ["scroll", rest @ ..] => {
                let frames = rest.first().and_then(|f| f.parse().ok()).unwrap_or(120);
                let sw = app
                    .sidebar
                    .scroller()
                    .ok_or_else(|| Error::internal("no Navigator scroller"))?;
                crate::perf::scroll("nav_scroll", &sw, frames).map_err(Error::usage)?;
            }
            ["usage"] => {
                crate::usage_view::close();
                app.sync();
                crate::perf::start("usage_page");
                crate::usage_view::open();
            }
            ["new_menu"] => {
                crate::perf::start("new_menu");
                crate::new_menu::debug(&json!({"arg": "open"}))?;
            }
            _ => {
                return Err(Error::usage(
                    "perf arg: reset [name] | scroll [frames] | usage | new_menu",
                ))
            }
        }
        return Ok(crate::perf::summary());
    }
    // R108.6: the + menu.
    if op == "new_menu" {
        return crate::new_menu::debug(p);
    }
    // A key press through RADE's own key path (R103-c, R103-i).
    if op == "key" {
        return crate::keys::debug_key(s("keys").unwrap_or(""));
    }
    // Keyboard focus to the first focusable widget in `widget` (`navigator`).
    if op == "focus" {
        let sel = s("widget").unwrap_or("");
        let root = crate::geometry::find(app.window.upcast_ref(), sel)
            .into_iter()
            .find(|w| w.is_mapped())
            .ok_or_else(|| Error::not_found(format!("no shown widget '{sel}'")))?;
        if !root.child_focus(gtk::DirectionType::TabForward) && !root.grab_focus() {
            return Err(Error::internal(format!("'{sel}' takes no focus")));
        }
        let f = gtk::prelude::GtkWindowExt::focus(&app.window);
        return Ok(json!({"focus": f.map(|f| f.css_name().to_string())}));
    }
    // Keyboard focus into Pane `pane`'s widget, as a click in it gives
    // (`pane_focus`); `pane_frames`: which shown Pane frames are lit.
    if op == "pane_focus" {
        let id = app
            .model
            .borrow()
            .resolve_pane(s("pane").unwrap_or(""), None)?;
        let w = app
            .terminals
            .borrow()
            .get(&id)
            .map(|t| t.handle.gl_area().clone().upcast::<gtk::Widget>())
            .ok_or_else(|| Error::not_found(format!("no Terminal '{id}'")))?;
        w.grab_focus();
        return Ok(json!({"pane": id}));
    }
    if op == "pane_frames" {
        let frames = crate::geometry::find(app.window.upcast_ref(), "pane");
        let lit: Vec<String> = frames
            .iter()
            .filter(|w| w.is_mapped() && w.has_css_class("focused"))
            .filter_map(|w| {
                w.widget_name()
                    .strip_prefix("pane-frame-")
                    .map(String::from)
            })
            .collect();
        let focused = app.model.borrow().active_tab().map(|t| t.focused.clone());
        return Ok(json!({"lit": lit, "focused": focused}));
    }
    // A click on the first shown button matching `widget` (a CSS class).
    if op == "click" {
        let sel = s("widget").unwrap_or("");
        let b = crate::geometry::find(app.window.upcast_ref(), sel)
            .into_iter()
            .filter(|w| w.is_mapped())
            .find_map(|w| w.downcast::<gtk::Button>().ok())
            .ok_or_else(|| Error::not_found(format!("no shown button '{sel}'")))?;
        b.emit_clicked();
        return Ok(json!({"clicked": sel}));
    }
    let mut r = json!({});
    match op {
        "layout" => {
            let width = p.get("width").and_then(Value::as_i64).unwrap_or(1280);
            let height = p.get("height").and_then(Value::as_i64).unwrap_or(720);
            let scale = p.get("text_scale").and_then(Value::as_f64).unwrap_or(1.0);
            if !(640..=4096).contains(&width)
                || !(400..=2160).contains(&height)
                || !(1.0..=2.0).contains(&scale)
            {
                return Err(Error::usage(
                    "invalid debug layout dimensions or text scale",
                ));
            }
            if let Some(settings) = gtk::Settings::default() {
                settings.set_gtk_xft_dpi((96.0 * 1024.0 * scale).round() as i32);
            }
            app.window.set_default_size(width as i32, height as i32);
            // Again once the new text scale is styled: a minimum from the
            // old fonts may have grown the window past the size asked for.
            let win = app.window.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
                win.set_default_size(width as i32, height as i32);
            });
            r["layout"] = crate::layout_mode::debug();
        }
        // R111: the layout in force (mode, width, text scale, overlays);
        // `arg` navigator = open the full Navigator over the rail.
        // R116: the system monitor (`arg` open | close | click_first).
        "monitor" => r["monitor"] = crate::sysmon_view::debug(s("arg"), s("path")),
        "layout_info" => {
            match s("arg") {
                Some("navigator") => crate::layout_mode::open_navigator(),
                Some("navigator_close") => crate::layout_mode::close_navigator(),
                _ => {}
            }
            r["layout"] = crate::layout_mode::debug();
        }
        "sidebar" | "navigator" => r["sidebar"] = app.sidebar.debug_rows(),
        // R129: the Files page header; `arg` = refresh | collapse | title | ignored.
        "files" => r["files"] = app.files.debug_header(s("arg")),
        "inbox" => r["inbox"] = app.sidebar.inbox(s("arg")),
        // Project home (R105): `arg` = open <project> | worktree <name> |
        // terminal <source> | action <source> <action>.
        "home" => r["home"] = crate::home_view::debug(s("arg"))?,
        "activity" => r["activity"] = crate::activity_view::debug(),
        // The Usage page (R110.2): `arg` = open | close | period <p> |
        // filter <ws>|all|shown | select <agent> <name> <ws> | key up|down.
        "usage" => r["usage"] = crate::usage_view::debug(s("arg"))?,
        // The forecast Toast check now (`arg` reset: raise one again).
        "forecast_check" => crate::accounts::debug_forecast(s("arg") == Some("reset")),
        // A click on a Navigator row: `key`, or `kind` + `title` (R104).
        "nav_click" | "nav_toggle" => {
            let (key, kind) = match (s("key"), s("kind")) {
                (Some(k), _) => (k, None),
                (None, Some(kind)) => (s("title").unwrap_or(""), Some(kind)),
                _ => return Err(Error::usage("nav_click needs key, or kind and title")),
            };
            let hit = if op == "nav_click" {
                app.sidebar.click(key, kind)
            } else {
                app.sidebar.toggle(key, kind)
            };
            if !hit {
                return Err(Error::not_found(format!("no Navigator row '{key}'")));
            }
            r["sidebar"] = app.sidebar.debug_rows();
        }
        // The chevron of a Source row.
        "toggle_source" => {
            app.update(|m| m.toggle_collapsed(s("arg").unwrap_or("")))?;
            r["sidebar"] = app.sidebar.debug_rows();
        }
        // The chevron of a Project header (R30.1): `arg` = its id in the
        // shown Workspace.
        "toggle_project" => {
            let key = format!(
                "{}/{}",
                app.model.borrow().active_workspace(),
                s("arg").unwrap_or("")
            );
            let folded = app.sidebar.project_folded(s("arg").unwrap_or(""));
            app.update(|m| m.toggle_project(&key, folded));
            r["sidebar"] = app.sidebar.debug_rows();
        }
        // A No Project row's `→ Project?` (R30.1): `arg` = its Worktree id.
        "suggest" => {
            if !app.sidebar.click_suggest(s("arg").unwrap_or("")) {
                return Err(Error::not_found("no suggestion on that row"));
            }
            r["sidebar"] = app.sidebar.debug_rows();
        }
        "click_row" => {
            let (kind, id) = (s("kind").unwrap_or(""), s("id").unwrap_or(""));
            if !app.sidebar.click_row(kind, id) {
                return Err(Error::not_found(format!("no {kind} row '{id}'")));
            }
            r["sidebar"] = app.sidebar.debug_rows();
        }
        "click_agent" => {
            let pane = s("pane").unwrap_or("");
            if !app.sidebar.click_agent(pane) {
                return Err(Error::not_found(format!("no Agent row for '{pane}'")));
            }
        }
        "pane" => {
            let pane = s("pane").unwrap_or("");
            let dimmed = crate::tabs::pane_overlays(pane)
                .ok_or_else(|| Error::not_found(format!("pane '{pane}' is not built")))?;
            r["pane"] = json!({"pane": pane, "dimmed": dimmed,
                "header": crate::tabs::pane_header(pane),
                "glyph": crate::tabs::pane_glyph(pane),
                "close": crate::tabs::pane_close_button(pane).is_some_and(|b| b.is_drawable())});
        }
        // A click on a Pane header's ✕ (R19.9).
        "pane_close" => {
            let pane = s("pane").unwrap_or("");
            crate::tabs::pane_close_button(pane)
                .ok_or_else(|| Error::not_found(format!("pane '{pane}' has no ✕")))?
                .emit_clicked();
        }
        // The Workspace switcher (R23.5): `arg` = open | close | <ws> (a
        // click on its row).
        "workspaces" => {}
        "switcher" => match s("arg").unwrap_or("") {
            // The open popover rendered to `path` (a separate surface: the
            // window screenshot doesn't have it).
            "shot" => {
                let path = s("path").ok_or_else(|| Error::usage("shot needs a path"))?;
                app.sidebar.switcher.shot(std::path::Path::new(path))?;
            }
            arg => {
                if !app.sidebar.switcher.debug_op(arg) {
                    return Err(Error::not_found("no such workspace row"));
                }
            }
        },
        "chrome" | "toast_show" | "toast_review" | "toasts_clear" | "right_page" | "retry_save"
        | "state_location" | "forecast_action" => {}
        _ => return Err(Error::usage(format!("unknown debug.ui op '{op}'"))),
    }
    r["chrome"] = app.chrome.debug(op, s("arg"));
    r["workspaces"] = app.sidebar.switcher.debug();
    Ok(r)
}

#[cfg(test)]
mod tests {
    #[test]
    fn find_cell_counts_soft_wrapped_rows() {
        let screen = format!("{}\nx src/a.rs:4\n$ ", "a".repeat(25));
        assert_eq!(super::find_cell(&screen, "src/a.rs", 10), Some((3, 2)));
        assert_eq!(super::find_cell(&screen, "aaa", 10), Some((0, 0)));
        assert_eq!(
            super::find_cell("\n\nhello world", "world", 8),
            Some((2, 6))
        );
        assert_eq!(
            super::find_cell("\n\nhello world", "world", 4),
            Some((3, 2))
        );
        assert_eq!(super::find_cell("abc", "zz", 8), None);
    }
}
