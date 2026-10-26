//! The Settings page (PRD R112, ADR-0037): app-wide `config.toml` settings
//! the VS Code way — a search field, categories, one row per setting with
//! its description, an `acc` bar and ↺ on a modified row — over a scrim.
//! Every change is one `rade_core::settings::set` / `reset` of the file
//! (comments kept), applied at once: Terminals reload their Ghostty config,
//! the color scheme follows. The Terminal category ends with a live
//! preview, a real Terminal. `debug.ui {"op": "app_settings…"}` drives it.

use crate::app::{app, try_app};
use adw::prelude::*;
use gtk::glib;
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::config::Config;
use rade_core::rpc::{Error, Result};
use rade_core::settings::{self as st, Kind, Setting};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::rc::Rc;

thread_local! {
    static SCRIM: RefCell<Option<gtk::Box>> = const { RefCell::new(None) };
    static OPEN: RefCell<Option<Rc<View>>> = const { RefCell::new(None) };
}

struct View {
    search: gtk::SearchEntry,
    categories: gtk::ListBox,
    rows: gtk::Box,
    scroller: gtk::ScrolledWindow,
    modified: gtk::Label,
    error: gtk::Label,
    /// The category the list shows selected (scrolls the rows to it).
    category: RefCell<Option<&'static str>>,
}

/// The scrim (hidden) over the window's content.
pub fn install(overlay: &gtk::Overlay) {
    let scrim = gtk::Box::new(gtk::Orientation::Vertical, 0);
    scrim.add_css_class("dialog-scrim");
    scrim.set_hexpand(true);
    scrim.set_vexpand(true);
    scrim.set_visible(false);
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(|_, k, _, _| {
        if k == gtk::gdk::Key::Escape {
            let popover = try_app()
                .and_then(|a| gtk::prelude::GtkWindowExt::focus(&a.window))
                .is_some_and(|f| f.ancestor(gtk::Popover::static_type()).is_some());
            if !popover {
                close();
                return glib::Propagation::Stop;
            }
        }
        glib::Propagation::Proceed
    });
    scrim.add_controller(keys);
    // A click on the scrim outside the page closes it.
    let click = gtk::GestureClick::new();
    click.connect_pressed(|g, _, x, y| {
        let Some(w) = g.widget() else { return };
        if w.pick(x, y, gtk::PickFlags::DEFAULT).as_ref() == Some(&w) {
            close();
        }
    });
    scrim.add_controller(click);
    overlay.add_overlay(&scrim);
    SCRIM.with(|s| *s.borrow_mut() = Some(scrim));
}

pub fn is_open() -> bool {
    OPEN.with(|o| o.borrow().is_some())
}

/// `Ctrl+,` and the palette's Settings: open, or close when open.
pub fn toggle() {
    if is_open() {
        close();
    } else {
        open();
    }
}

pub fn close() {
    let Some(_v) = OPEN.with(|o| o.borrow_mut().take()) else {
        return;
    };
    if let Some(s) = SCRIM.with(|s| s.borrow().clone()) {
        s.set_visible(false);
        while let Some(c) = s.first_child() {
            s.remove(&c);
        }
    }
    if let Some(app) = try_app() {
        app.restore_focus();
    }
}

/// `~/…` for a path under the home folder.
fn tilde(path: &std::path::Path) -> String {
    let home = glib::home_dir();
    match path.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
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

fn open() {
    let Some(scrim) = SCRIM.with(|s| s.borrow().clone()) else {
        return;
    };
    crate::project_dialogs::close();
    crate::palette::get().inspect(|p| p.close(false));
    let app = app();

    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.add_css_class("settings-page");
    page.set_halign(gtk::Align::Center);
    page.set_valign(gtk::Align::Start);
    page.set_margin_top(56);
    let win_w = app.window.width().max(640);
    let win_h = app.window.height().max(480);
    page.set_size_request((win_w - 64).min(920), (win_h - 112).min(680));

    // Header: gear, title, the config file, close.
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    header.add_css_class("dialog-header");
    let gear = gtk::Image::from_icon_name("ph-gear-six-symbolic");
    gear.add_css_class("dialog-icon");
    header.append(&gear);
    header.append(&label("Settings", &["dialog-title"]));
    let file = label(
        &tilde(&app.paths.config),
        &["dialog-context", "settings-file"],
    );
    file.set_hexpand(true);
    file.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    header.append(&file);
    let close_b = gtk::Button::from_icon_name("ph-x-symbolic");
    close_b.add_css_class("dialog-close");
    close_b.add_css_class("flat");
    close_b.set_tooltip_text(Some("Close (Esc)"));
    close_b.connect_clicked(|_| close());
    header.append(&close_b);
    page.append(&header);

    // Search + "n modified".
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    bar.add_css_class("settings-search-bar");
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search settings"));
    search.add_css_class("settings-search");
    search.set_hexpand(true);
    bar.append(&search);
    let modified = label("", &["settings-modified"]);
    bar.append(&modified);
    page.append(&bar);
    let error = label("", &["settings-error"]);
    error.set_wrap(true);
    error.set_visible(false);
    page.append(&error);

    // Categories | rows.
    let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    body.set_vexpand(true);
    let categories = gtk::ListBox::new();
    categories.add_css_class("settings-categories");
    categories.set_selection_mode(gtk::SelectionMode::Single);
    body.append(&categories);
    let rows = gtk::Box::new(gtk::Orientation::Vertical, 0);
    rows.add_css_class("settings-rows");
    let scroller = gtk::ScrolledWindow::new();
    scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);
    scroller.set_child(Some(&rows));
    body.append(&scroller);
    page.append(&body);

    let view = Rc::new(View {
        search: search.clone(),
        categories: categories.clone(),
        rows,
        scroller,
        modified,
        error,
        category: RefCell::new(None),
    });
    search.connect_search_changed(|_| render());
    categories.connect_row_activated(|_, row| {
        let Some(v) = OPEN.with(|o| o.borrow().clone()) else {
            return;
        };
        let name = row.widget_name().to_string();
        *v.category.borrow_mut() = st::CATEGORIES.iter().copied().find(|c| *c == name);
        scroll_to(&v, &name);
    });

    scrim.append(&page);
    scrim.set_visible(true);
    OPEN.with(|o| *o.borrow_mut() = Some(view));
    render();
    search.grab_focus();
}

/// Rebuilds the categories and rows for the search text and the config
/// the app runs with now.
fn render() {
    let Some(v) = OPEN.with(|o| o.borrow().clone()) else {
        return;
    };
    let config = app().config.borrow().clone();
    let query = v.search.text().to_string();
    let shown = st::categories(&query);

    while let Some(c) = v.categories.first_child() {
        v.categories.remove(&c);
    }
    for c in &shown {
        let row = gtk::ListBoxRow::new();
        row.set_widget_name(c);
        let n = st::SETTINGS
            .iter()
            .filter(|s| s.category == *c && st::modified(&config, s.key))
            .count();
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        line.append(&label(c, &["settings-category"]));
        if n > 0 {
            let dot = label("●", &["settings-category-dot"]);
            dot.set_tooltip_text(Some(&format!("{n} modified")));
            line.append(&dot);
        }
        row.set_child(Some(&line));
        v.categories.append(&row);
    }

    while let Some(c) = v.rows.first_child() {
        v.rows.remove(&c);
    }
    if shown.is_empty() {
        let none = label(
            &format!("No settings match \u{201c}{query}\u{201d}"),
            &["empty-line", "settings-none"],
        );
        v.rows.append(&none);
    }
    for c in &shown {
        let head = label(&c.to_uppercase(), &["section-label", "settings-section"]);
        head.set_widget_name(&format!("section:{c}"));
        v.rows.append(&head);
        for s in st::SETTINGS
            .iter()
            .filter(|s| s.category == *c && st::matches(s, &query))
        {
            v.rows.append(&row(s, &config));
        }
        if *c == "Terminal" && config.terminal.theme == rade_core::config::TerminalTheme::Rade {
            v.rows.append(&preview());
        }
        if *c == "Code" {
            v.rows.append(&code_preview());
        }
    }
    // The category in view stays selected (the first by default).
    let want = v
        .category
        .borrow()
        .filter(|c| shown.contains(c))
        .or(shown.first().copied());
    let mut child = v.categories.first_child();
    while let Some(c) = child {
        if let Some(row) = c.downcast_ref::<gtk::ListBoxRow>() {
            if Some(row.widget_name().as_str()) == want {
                v.categories.select_row(Some(row));
            }
        }
        child = c.next_sibling();
    }
    let total = st::SETTINGS
        .iter()
        .filter(|s| st::modified(&config, s.key))
        .count();
    v.modified.set_text(&match total {
        0 => String::new(),
        n => format!("{n} modified"),
    });
}

fn scroll_to(v: &View, category: &str) {
    let name = format!("section:{category}");
    let mut child = v.rows.first_child();
    while let Some(c) = child {
        if c.widget_name() == name {
            if let Some(p) = c.compute_point(&v.rows, &gtk::graphene::Point::new(0.0, 0.0)) {
                v.scroller.vadjustment().set_value(p.y() as f64);
            }
            return;
        }
        child = c.next_sibling();
    }
}

/// One setting: the modified bar, label + key + description, the control, ↺.
fn row(s: &'static Setting, config: &Config) -> gtk::Widget {
    let modified = st::modified(config, s.key);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    line.add_css_class("settings-row");
    line.set_widget_name(s.key);
    if modified {
        line.add_css_class("modified");
    }
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    let title = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    title.append(&label(s.label, &["settings-label"]));
    title.append(&label(s.key, &["settings-key"]));
    text.append(&title);
    let desc = label(s.description, &["settings-description"]);
    desc.set_wrap(true);
    text.append(&desc);
    line.append(&text);

    let control = control(s, config);
    control.set_valign(gtk::Align::Center);
    line.append(&control);

    let reset = gtk::Button::from_icon_name("ph-arrow-counter-clockwise-symbolic");
    reset.add_css_class("flat");
    reset.add_css_class("settings-reset");
    reset.set_valign(gtk::Align::Center);
    reset.set_tooltip_text(Some(&format!(
        "Reset to {}",
        show(&st::default_value(s.key))
    )));
    reset.set_opacity(if modified { 1.0 } else { 0.0 });
    reset.set_sensitive(modified);
    let key = s.key;
    reset.connect_clicked(move |_| apply(key, None));
    line.append(&reset);
    line.upcast()
}

/// The first choice of the code font's family / style: empty in the file.
const SAME: &str = "Same as Terminal";

fn show(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Float(f) if f.fract() == 0.0 => format!("{f:.0}"),
        other => other.to_string(),
    }
}

fn control(s: &'static Setting, config: &Config) -> gtk::Widget {
    let current = st::value(config, s.key);
    let key = s.key;
    match s.kind {
        Kind::Font | Kind::FontStyle | Kind::Choice(_) => {
            let options: Vec<String> = match s.kind {
                Kind::Font => monospace_families(),
                Kind::FontStyle if key.starts_with("code_font.") => {
                    styles_of(&config.code_font().family)
                }
                Kind::FontStyle => styles_of(&config.terminal.font),
                Kind::Choice(list) => list.iter().map(|c| c.to_string()).collect(),
                _ => Vec::new(),
            };
            let mut now = show(&current);
            let mut options = options;
            if !now.is_empty() && !options.contains(&now) {
                options.insert(0, now.clone());
            }
            // An empty code font follows the Terminal (R112.4).
            let same = key.starts_with("code_font.");
            if same {
                options.insert(0, SAME.to_string());
                if now.is_empty() {
                    now = SAME.to_string();
                }
            }
            let model =
                gtk::StringList::new(&options.iter().map(String::as_str).collect::<Vec<_>>());
            let drop = gtk::DropDown::new(Some(model), gtk::Expression::NONE);
            drop.add_css_class("settings-drop");
            if matches!(s.kind, Kind::Font) {
                drop.set_enable_search(true);
                drop.set_search_match_mode(gtk::StringFilterMatchMode::Substring);
                drop.set_expression(Some(gtk::PropertyExpression::new(
                    gtk::StringObject::static_type(),
                    gtk::Expression::NONE,
                    "string",
                )));
            }
            if let Some(i) = options.iter().position(|o| *o == now) {
                drop.set_selected(i as u32);
            }
            drop.connect_selected_notify(move |d| {
                let Some(item) = d.selected_item().and_downcast::<gtk::StringObject>() else {
                    return;
                };
                let mut v = item.string().to_string();
                if v == SAME {
                    v.clear();
                }
                if v != show(&st::value(&app().config.borrow(), key)) {
                    apply(key, Some(toml::Value::String(v)));
                }
            });
            drop.upcast()
        }
        Kind::Number { min, max, step } => {
            let spin = gtk::SpinButton::with_range(min, max, step);
            spin.set_digits(if step < 1.0 { 2 } else { 0 });
            spin.set_value(
                current
                    .as_float()
                    .or(current.as_integer().map(|i| i as f64))
                    .unwrap_or(min),
            );
            spin.add_css_class("settings-spin");
            spin.connect_value_changed(move |sp| {
                let v = (sp.value() * 1000.0).round() / 1000.0;
                apply(key, Some(toml::Value::Float(v)));
            });
            spin.upcast()
        }
        Kind::Integer { min, max } => {
            let spin = gtk::SpinButton::with_range(min as f64, max as f64, 1.0);
            spin.set_value(current.as_integer().unwrap_or(min) as f64);
            spin.add_css_class("settings-spin");
            spin.connect_value_changed(move |sp| {
                apply(key, Some(toml::Value::Integer(sp.value() as i64)));
            });
            spin.upcast()
        }
        Kind::Bool => {
            let sw = gtk::Switch::new();
            sw.set_active(current.as_bool().unwrap_or(false));
            sw.add_css_class("settings-switch");
            sw.connect_active_notify(move |sw| {
                apply(key, Some(toml::Value::Boolean(sw.is_active())));
            });
            sw.upcast()
        }
    }
}

/// Monospace families GTK sees (installed and bundled), sorted, once.
fn monospace_families() -> Vec<String> {
    thread_local! { static CACHE: RefCell<Option<Vec<String>>> = const { RefCell::new(None) }; }
    CACHE.with(|c| {
        c.borrow_mut()
            .get_or_insert_with(|| {
                let ctx = gtk::Label::new(None).pango_context();
                let mut names: Vec<String> = ctx
                    .list_families()
                    .iter()
                    .filter(|f| f.is_monospace())
                    .map(|f| f.name().to_string())
                    .filter(|n| !n.eq_ignore_ascii_case("monospace"))
                    .collect();
                names.sort_by_key(|n| n.to_lowercase());
                names.dedup();
                names
            })
            .clone()
    })
}

/// The styles (faces) of a family: Regular, Medium, Bold… (not italics).
fn styles_of(family: &str) -> Vec<String> {
    let ctx = gtk::Label::new(None).pango_context();
    let mut faces: Vec<(i32, String)> = ctx
        .list_families()
        .iter()
        .filter(|f| f.name().eq_ignore_ascii_case(family))
        .flat_map(|f| f.list_faces())
        .filter_map(|face| {
            let d = face.describe();
            (d.style() == gtk::pango::Style::Normal)
                .then(|| (weight_rank(d.weight()), face.face_name().to_string()))
        })
        .collect();
    faces.sort();
    faces.dedup_by(|a, b| a.1 == b.1);
    let mut out: Vec<String> = faces.into_iter().map(|(_, n)| n).collect();
    if out.is_empty() {
        out.push("Regular".into());
    }
    out
}

fn weight_rank(w: gtk::pango::Weight) -> i32 {
    use gtk::pango::Weight as W;
    match w {
        W::Thin => 100,
        W::Ultralight => 200,
        W::Light => 300,
        W::Semilight => 350,
        W::Book => 380,
        W::Normal => 400,
        W::Medium => 500,
        W::Semibold => 600,
        W::Bold => 700,
        W::Ultrabold => 800,
        W::Heavy => 900,
        W::Ultraheavy => 1000,
        _ => 450,
    }
}

/// Write one setting (`None` = reset) and apply it now; the file watcher
/// sees the same file later and finds nothing new.
fn apply(key: &'static str, value: Option<toml::Value>) {
    match write_setting(key, value) {
        Ok(config) => {
            crate::keys::apply_config(config);
            if let Some(v) = OPEN.with(|o| o.borrow().clone()) {
                v.error.set_visible(false);
            }
            // A new font has other styles; everything else just redraws.
            glib::idle_add_local_once(render);
        }
        Err(e) => {
            if let Some(v) = OPEN.with(|o| o.borrow().clone()) {
                v.error.set_text(&e);
                v.error.set_visible(true);
            }
        }
    }
}

pub(crate) fn write_setting(
    key: &str,
    value: Option<toml::Value>,
) -> std::result::Result<Config, String> {
    let path = app().paths.config.clone();
    let old = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let new = match value {
        Some(v) => st::set(&old, key, v)?,
        None => st::reset(&old, key)?,
    };
    if new != old {
        // Through a symlinked config.toml to its target.
        let file = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let tmp = file.with_extension("toml.tmp");
        std::fs::write(&tmp, &new)
            .and_then(|_| std::fs::rename(&tmp, &file))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Config::parse(&new).map_err(|e| format!("config.toml: {e}"))
}

/// A code sample in the code font (the `monospace` class it styles).
fn code_preview() -> gtk::Widget {
    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.add_css_class("settings-preview");
    frame.append(&label(
        "PREVIEW",
        &["section-label", "settings-preview-label"],
    ));
    let code = label(CODE_SAMPLE, &["monospace", "settings-code-sample"]);
    frame.append(&code);
    frame.upcast()
}

const CODE_SAMPLE: &str = "fn resolve(link: &Link, base: &str) -> Result<Checkout> {\n    let drift = observe(link)?; // != >= => ::\n    Ok(Checkout::at(base, drift))  Il1|0O {}[]\n}";

/// A real Terminal printing a sample, in the font being chosen.
fn preview() -> gtk::Widget {
    let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
    frame.add_css_class("settings-preview");
    frame.append(&label(
        "PREVIEW",
        &["section-label", "settings-preview-label"],
    ));
    let script = glib::user_runtime_dir()
        .join("rade")
        .join("settings-preview.sh");
    let _ = std::fs::write(&script, PREVIEW_SH);
    let options = crate::terminal::TerminalOptions {
        startup_command: Some(format!("/bin/sh {}", script.display())),
        ..Default::default()
    };
    let term = crate::terminal::create_terminal(
        None,
        options,
        crate::terminal::TerminalCallbacks::disconnected(),
    );
    term.root.set_size_request(-1, 96);
    term.root.add_css_class("settings-preview-terminal");
    term.root.set_can_target(false);
    frame.append(&term.root);
    frame.upcast()
}

const PREVIEW_SH: &str = r#"printf '\033[?25l'
printf '\033[36m●\033[0m Plan for checkout-v2          \033[90mfn\033[0m \033[34mresolve\033[0m(link: &Link) -> Result<()> {\n'
printf '  1. Split \033[34mCheckoutForm\033[0m into steps    \033[35mlet\033[0m drift = \033[36mobserve\033[0m(link)?; \033[90m// != >=\033[0m\n'
printf '\033[36m●\033[0m Updated with \033[32m+84\033[0m \033[31m-31\033[0m             \033[1mbold\033[0m \033[3mitalic\033[0m Il1|0O {}[]\n'
printf '\033[33m? Which fixtures should the contract test use?\033[0m\n'
exec sleep 2147483647
"#;

/// `debug.ui {"op": "app_settings" | "app_settings_open" | "app_settings_close"
/// | "app_settings_search" (arg) | "app_settings_set" (key, value: JSON) |
/// "app_settings_reset" (key)}` (test mode): what the page shows, plus each
/// Terminal's columns and rows.
pub fn debug(p: &Value) -> Result<Value> {
    let s = |k: &str| p.get(k).and_then(Value::as_str);
    match s("op").unwrap_or("app_settings") {
        "app_settings_open" => {
            if !is_open() {
                open();
            }
        }
        "app_settings_close" => close(),
        "app_settings_search" => {
            let v = OPEN
                .with(|o| o.borrow().clone())
                .ok_or_else(|| Error::usage("Settings is not open"))?;
            v.search.set_text(s("arg").unwrap_or(""));
            render();
        }
        "app_settings_set" | "app_settings_reset" => {
            let key = s("key").ok_or_else(|| Error::usage("want key"))?;
            let setting =
                st::find(key).ok_or_else(|| Error::usage(format!("unknown setting '{key}'")))?;
            let value = match p.get("value") {
                None => None,
                Some(v) => Some(
                    toml::Value::try_from(v.clone()).map_err(|e| Error::usage(e.to_string()))?,
                ),
            };
            apply(setting.key, value);
            render();
        }
        "app_settings" => {}
        op => return Err(Error::usage(format!("unknown op '{op}'"))),
    }
    Ok(state())
}

fn state() -> Value {
    let app = app();
    let config = app.config.borrow().clone();
    let terminals: Vec<Value> = app
        .terminals
        .borrow()
        .iter()
        .map(|(id, t)| {
            let h = t.handle.health();
            json!({"pane": id, "columns": h.columns, "rows": h.rows})
        })
        .collect();
    let Some(v) = OPEN.with(|o| o.borrow().clone()) else {
        return json!({"open": false, "dark": crate::tokens::dark(), "terminals": terminals});
    };
    let query = v.search.text().to_string();
    let rows: Vec<Value> = st::SETTINGS
        .iter()
        .filter(|s| st::matches(s, &query))
        .map(|s| {
            json!({"key": s.key, "category": s.category, "label": s.label,
                "value": show(&st::value(&config, s.key)), "modified": st::modified(&config, s.key)})
        })
        .collect();
    json!({
        "open": true,
        "query": query,
        "categories": st::categories(&query),
        "rows": rows,
        "modified": v.modified.text().to_string(),
        "error": v.error.is_visible().then(|| v.error.text().to_string()),
        "fonts": monospace_families(),
        "styles": styles_of(&config.terminal.font),
        "dark": crate::tokens::dark(),
        "terminals": terminals,
        "code_css": crate::fonts::code_css(&config.code_font()),
    })
}
