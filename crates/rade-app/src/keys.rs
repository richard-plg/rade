//! Keybindings (R17.3) from `config.toml` (R17.1, live-reloaded) and the
//! actions they run. Matching is exact on Ctrl/Shift/Alt/Super, so keys the
//! config doesn't bind — plain Ctrl+<key>, Shift+Enter — reach the program.

use crate::app::{app, try_app};
use adw::prelude::*;
use gtk::gdk::{Key, ModifierType};
use gtk::{gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::basic::CloseMode;
use rade_core::config::Config;
use rade_core::{Direction, SplitDir};
use std::cell::RefCell;
use std::path::Path;

const MODS: ModifierType = ModifierType::CONTROL_MASK
    .union(ModifierType::SHIFT_MASK)
    .union(ModifierType::ALT_MASK)
    .union(ModifierType::SUPER_MASK);

thread_local! {
    static KEYMAP: RefCell<Vec<(Key, ModifierType, String)>> = const { RefCell::new(Vec::new()) };
}

pub fn load_config(path: &Path) -> Config {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Config::default(),
        Err(e) => {
            eprintln!("rade: {}: {e}", path.display());
            return Config::default();
        }
    };
    Config::parse(&text).unwrap_or_else(|e| {
        eprintln!("rade: {}: {e} (using defaults)", path.display());
        Config::default()
    })
}

fn apply_keymap(config: &Config) {
    let map = config
        .keybindings()
        .into_iter()
        .filter_map(|(action, accel)| match gtk::accelerator_parse(&accel) {
            Some((key, mods)) => Some((key.to_lower(), mods & MODS, action)),
            None => {
                eprintln!("rade: keybinding {action} = {accel:?} is not a GTK accelerator");
                None
            }
        })
        .collect();
    KEYMAP.with(|k| *k.borrow_mut() = map);
}

/// Capture-phase key handler on the window, so bindings win over the
/// focused Terminal; plus live reload of `config.toml`.
pub fn install(window: &adw::ApplicationWindow, config_path: &Path) -> gio::FileMonitor {
    apply_keymap(&app().config.borrow());
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(|_, keyval, _, state| on_key(keyval, state));
    // R124 (D2): letting go of the `voice` key ends a take, whatever the
    // modifiers are by then.
    keys.connect_key_released(|_, keyval, _, _| on_key_released(keyval));
    window.add_controller(keys);
    // A take also ends when the window loses focus; no process outlives it.
    window.connect_is_active_notify(|w| {
        if !w.is_active() {
            crate::voice::focus_lost();
        }
    });
    window.connect_close_request(|_| {
        crate::voice::shutdown();
        glib::Propagation::Proceed
    });
    watch_config(config_path)
}

/// The window's capture-phase key handling: a bound action, else RADE's
/// chrome-only plain-Ctrl keys; everything else goes on to the focus.
fn on_key(keyval: Key, state: ModifierType) -> glib::Propagation {
    // Agent Overview is modal (R117.1): its shortcut flips, other bound
    // keys go to its own handler (arrows, Enter) or its filter field.
    if crate::overview_view::is_open() {
        return match lookup(keyval, state).as_deref() {
            Some("agent_overview") => {
                crate::overview_view::shortcut();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        };
    }
    match lookup(keyval, state) {
        // While the palette is open only its own key acts; the rest are
        // the search entry's (R14.4).
        Some(a) if crate::palette::is_open() && a != "command_palette" => {
            glib::Propagation::Proceed
        }
        // Hold / tap / repeat of the voice key (R124, D2): not a one-shot
        // action. Ignored while Settings is open.
        Some(a) if a == "voice" => {
            if crate::voice::key_down() {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
        Some(a) => {
            run_action(&a);
            glib::Propagation::Stop
        }
        // Plain Ctrl+K / Ctrl+B act only from RADE's own chrome; in a
        // Terminal or a Browser page they reach the program (R17
        // amendment, R103.1, R103.4).
        None if crate::files::key(keyval, state) => glib::Propagation::Stop,
        None => match chrome_key(keyval, state) {
            Some(a) if !crate::palette::is_open() && !focus_in_program() => {
                run_action(a);
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        },
    }
}

/// The `voice` key was released (R124): matched by key alone.
fn on_key_released(keyval: Key) {
    let voice = KEYMAP.with(|k| {
        k.borrow()
            .iter()
            .any(|(key, _, a)| a == "voice" && *key == keyval.to_lower())
    });
    if voice {
        crate::voice::key_up();
    }
}

/// `debug.ui {"op": "key", "keys": "ctrl+k" | "ctrl+shift+p" | "escape" | "x"}`
/// (test mode): the key goes the way a real press does — the palette's own
/// keys while it is open, RADE's bindings, else the focused Terminal.
pub fn debug_key(spec: &str) -> rade_core::rpc::Result<serde_json::Value> {
    use serde_json::json;
    // `release:ctrl` = Ctrl let go (Agent Overview's flip, R117.4).
    if spec.eq_ignore_ascii_case("release:ctrl") {
        crate::overview_view::release(Key::Control_L);
        return Ok(json!({"handled": "overview"}));
    }
    // `press:F8` / `release:F8` (R124, D22): a key held without being let go;
    // a second `press:` while held is an auto-repeat.
    let (phase, spec) = if let Some(rest) = spec.strip_prefix("press:") {
        (Some(true), rest)
    } else if let Some(rest) = spec.strip_prefix("release:") {
        (Some(false), rest)
    } else {
        (None, spec)
    };
    let mut mods = ModifierType::empty();
    let mut name = "";
    for part in spec.split('+') {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" => mods |= ModifierType::CONTROL_MASK,
            "shift" => mods |= ModifierType::SHIFT_MASK,
            "alt" => mods |= ModifierType::ALT_MASK,
            _ => name = part,
        }
    }
    let key = match name.to_ascii_lowercase().as_str() {
        "escape" | "esc" => Key::Escape,
        "enter" | "return" => Key::Return,
        n => Key::from_name(name)
            .or_else(|| Key::from_name(n))
            .ok_or_else(|| rade_core::rpc::Error::usage(format!("unknown key '{spec}'")))?,
    };
    if phase == Some(false) {
        on_key_released(key);
        return Ok(json!({"handled": "release"}));
    }
    // The Settings page takes Esc from its own field (its capture handler).
    if crate::settings_view::is_open() && key == Key::Escape {
        crate::settings_view::close();
        return Ok(json!({"handled": "settings"}));
    }
    // The Voice Composer takes Esc / Enter / Alt+Enter (R124, D12, D13).
    if crate::voice_view::is_open() && crate::voice_view::handle_key(key, mods) {
        return Ok(json!({"handled": "voice"}));
    }
    if crate::overview_view::is_open() {
        if lookup(key, mods).as_deref() == Some("agent_overview") {
            crate::overview_view::shortcut();
            return Ok(json!({"handled": "overview"}));
        }
        let handled = crate::overview_view::key(key, mods);
        return Ok(json!({"handled": if handled { "overview" } else { "none" }}));
    }
    if crate::palette::is_open() {
        let handled = key == Key::Escape && crate::palette::key("escape");
        if handled || lookup(key, mods).as_deref() == Some("command_palette") {
            if !handled {
                run_action("command_palette");
            }
            return Ok(json!({"handled": "palette"}));
        }
        return Ok(json!({"handled": "none"}));
    }
    if on_key(key, mods) == glib::Propagation::Stop {
        return Ok(json!({"handled": "rade"}));
    }
    let app = app();
    let focus = gtk::prelude::GtkWindowExt::focus(&app.window);
    let pane = focus
        .as_ref()
        .and_then(|f| crate::app::pane_of_widget(&app, f));
    if let Some(id) = &pane {
        if let Some(t) = app.terminals.borrow().get(id) {
            t.handle.press_key(key, mods);
            return Ok(json!({"handled": "terminal", "pane": id}));
        }
    }
    Ok(json!({"handled": "none", "pane": pane,
        "focus": focus.map(|f| f.css_name().to_string())}))
}

/// A new `config.toml` (the file watcher, Settings): keys, color scheme,
/// and the Terminals' Ghostty config when `[terminal]` changed.
pub fn apply_config(config: Config) {
    apply_keymap(&config);
    apply_color_scheme(&config);
    crate::fonts::apply_code_font(&config.code_font());
    if let Some(app) = try_app() {
        let old = app.config.replace(config);
        if old.terminal != app.config.borrow().terminal {
            crate::terminal::reload_config();
        }
        if old.system_monitor != app.config.borrow().system_monitor {
            crate::sysmon_view::refresh_mode();
        }
        if old.update != app.config.borrow().update {
            crate::update::config_changed();
        }
    }
}

/// `[appearance] color_scheme` (R112): `system` = dark unless the system
/// asks for light (R102.6).
pub fn apply_color_scheme(config: &Config) {
    use rade_core::config::ColorScheme;
    adw::StyleManager::default().set_color_scheme(match config.appearance.color_scheme {
        ColorScheme::System => adw::ColorScheme::PreferDark,
        ColorScheme::Dark => adw::ColorScheme::ForceDark,
        ColorScheme::Light => adw::ColorScheme::ForceLight,
    });
}

fn watch_config(config_path: &Path) -> gio::FileMonitor {
    let monitor = gio::File::for_path(config_path)
        .monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
        .expect("monitor config.toml");
    let path = config_path.to_path_buf();
    monitor.connect_changed(move |_, _, _, event| {
        use gio::FileMonitorEvent as E;
        if matches!(
            event,
            E::ChangesDoneHint | E::Created | E::Deleted | E::MovedIn | E::Renamed
        ) {
            apply_config(load_config(&path));
            eprintln!("rade: reloaded {}", path.display());
        }
    });
    monitor
}

/// RADE's plain-Ctrl keys, which only act while the focus is in its own
/// chrome: Ctrl+K = the palette, Ctrl+B = the Inspector, Ctrl+G = the
/// newest unread Notification (R104.3).
fn chrome_key(keyval: Key, state: ModifierType) -> Option<&'static str> {
    if state & MODS != ModifierType::CONTROL_MASK {
        return None;
    }
    match keyval.to_lower() {
        Key::k => Some("command_palette"),
        Key::b => Some("toggle_file_tree"),
        Key::g => Some("jump_unread"),
        _ => None,
    }
}

/// Whether the keyboard focus is in a Terminal or a Browser page (a
/// program's keys), not in RADE's own chrome.
fn focus_in_program() -> bool {
    let Some(app) = try_app() else { return false };
    let Some(f) = gtk::prelude::GtkWindowExt::focus(&app.window) else {
        return false;
    };
    let Some(pane) = crate::app::pane_of_widget(&app, &f) else {
        return false;
    };
    let m = app.model.borrow();
    m.pane(&pane).is_some_and(|p| {
        matches!(
            p.kind,
            rade_core::PaneKind::Terminal { .. } | rade_core::PaneKind::Browser { .. }
        )
    })
}

/// The bound action for a key press, if any: exact modifiers, so an unbound
/// combination (Shift+Enter, Ctrl+V) goes on to the Terminal.
fn lookup(keyval: Key, state: ModifierType) -> Option<String> {
    let (key, mods) = (keyval.to_lower(), state & MODS);
    KEYMAP.with(|k| {
        k.borrow()
            .iter()
            .find(|(k, m, _)| *k == key && *m == mods)
            .map(|b| b.2.clone())
    })
}

pub fn run_action(action: &str) {
    let app = app();
    let focused = app
        .model
        .borrow()
        .active_tab()
        .map(|t| (t.id.clone(), t.focused.clone()));
    match action {
        "new_tab" => app.update(|m| {
            let (p, w) = m.active();
            let (p, w) = (p.id.clone(), w.id.clone());
            // Beside the focused Pane: a Terminal on a link stays on it.
            let beside = focused.as_ref().map(|(_, pane)| pane.as_str());
            if let Ok((tab, _)) = m.new_tab_beside(&p, &w, beside) {
                let _ = m.select_tab(&tab);
            }
        }),
        "close_pane" => {
            if let Some((_, pane)) = focused {
                crate::menus::close_pane_ui(&pane);
            }
        }
        "split_right" | "split_down" => {
            let dir = if action == "split_right" {
                SplitDir::Right
            } else {
                SplitDir::Down
            };
            if let Some((_, pane)) = focused {
                let _ = app.update(|m| m.split(&pane, dir, None, None));
            }
        }
        "focus_left" | "focus_right" | "focus_up" | "focus_down" => {
            let dir = match action {
                "focus_left" => Direction::Left,
                "focus_right" => Direction::Right,
                "focus_up" => Direction::Up,
                _ => Direction::Down,
            };
            app.update(|m| m.focus_direction(dir));
        }
        "zoom" => {
            if let Some((tab, _)) = focused {
                let _ = app.update(|m| m.toggle_zoom(&tab));
            }
        }
        "next_tab" => app.update(|m| m.cycle_tab(1)),
        "prev_tab" => app.update(|m| m.cycle_tab(-1)),
        "add_source" => add_source_dialog(),
        "jump_unread" => crate::notifications::jump_unread(),
        "command_palette" => crate::palette::toggle(),
        "toggle_file_tree" => {
            // From RADE's chrome (Navigator, a button) the keyboard stays
            // there, so a second Ctrl+B toggles back (R103.4); from a
            // Terminal it stays in the Terminal as always.
            let keep = (!focus_in_program())
                .then(|| gtk::prelude::GtkWindowExt::focus(&app.window))
                .flatten();
            app.update(|m| m.file_tree = !m.file_tree);
            if let Some(w) = keep.filter(|w| w.is_mapped()) {
                w.grab_focus();
            }
        }
        "toggle_status_bar" => app.chrome.toggle_status_bar(&app),
        "open_usage" => crate::usage_view::open(),
        "open_settings" => crate::settings_view::toggle(),
        "about" => crate::settings_view::open_at("Updates"),
        "check_updates" => crate::update::check_now(),
        "restart_update" => crate::update::restart_dialog(),
        "agent_overview" => crate::overview_view::shortcut(),
        // The palette's Voice Prompt: a tap on the focused Pane (R124).
        "voice" => crate::voice::tap_focused(),
        "rename_tab" => crate::tabs::rename_selected(),
        "new_worktree" => {
            let p = app.model.borrow().active_source.clone();
            crate::worktrees::new_dialog(&p);
        }
        "open_review" | "review_since" => {
            let (p, w) = {
                let m = app.model.borrow();
                let (p, w) = m.active();
                (p.id.clone(), w.id.clone())
            };
            // General is no repository: nothing to review.
            if p != rade_core::GENERAL_ID {
                let pane = app.update(|m| m.open_review(&p, &w));
                // R119.5: only what changed since the last Round.
                if let (Ok(pane), "review_since") = (pane, action) {
                    if let Some(v) = app.reviews.borrow().get(&pane).cloned() {
                        v.show_since();
                    }
                }
            }
        }
        "open_browser" => crate::browser::open_browser(None),
        // Right Panel pages (R21.9, R22.5).
        "show_changes" | "scm_commit" | "scm_pull" | "scm_push" | "scm_pr" | "scm_fetch"
        | "scm_sync" | "scm_publish" | "scm_undo_commit" | "scm_view_tree" | "scm_view_list" => {
            if let Some(c) = crate::changes::get() {
                c.action(action);
            }
        }
        "search_worktree" => {
            if let Some(s) = crate::search_view::get() {
                s.show_page();
            }
        }
        // Basic actions without a default key (palette, R19).
        "duplicate_tab" | "close_tab" | "close_other_tabs" | "close_tabs_right" => {
            let Some((tab, _)) = focused else { return };
            match action {
                "duplicate_tab" => {
                    if let Err(e) = app.update(|m| m.duplicate_tab(&tab)) {
                        crate::worktrees::alert("Could not duplicate the Tab", &e.message);
                    }
                }
                "close_tab" => crate::menus::close_tabs_ui(&tab, CloseMode::Only),
                "close_other_tabs" => crate::menus::close_tabs_ui(&tab, CloseMode::Others),
                _ => crate::menus::close_tabs_ui(&tab, CloseMode::Right),
            }
        }
        "rename_pane" => {
            if let Some((_, pane)) = focused {
                crate::menus::rename_pane_ui(&pane, None);
            }
        }
        "find_in_terminal" => {
            let handle =
                focused.and_then(|(_, p)| app.terminals.borrow().get(&p).map(|t| t.handle.clone()));
            if let Some(h) = handle {
                h.perform_binding_action("start_search");
            }
        }
        "rename_source" | "pause_source" | "resume_source" => {
            let p = app.model.borrow().active_source.clone();
            match action {
                "rename_source" => crate::menus::rename_source_ui(&p),
                "pause_source" => crate::menus::pause_ui(&p),
                _ => {
                    if let Err(e) = app.resume_source(&p) {
                        crate::worktrees::alert("Could not resume the Source", &e.message);
                    }
                }
            }
        }
        _ => {
            // Alt+<n>: the n-th Project's home in shown order (R104.4);
            // with no Projects, the n-th Source as before (R23.4).
            if let Some(n) = action
                .strip_prefix("source_")
                .and_then(|n| n.parse::<usize>().ok())
            {
                let projects = crate::navdata::projects(&app.model.borrow());
                if !projects.is_empty() {
                    let id = app
                        .model
                        .borrow()
                        .alt_projects(&projects)
                        .get(n.wrapping_sub(1))
                        .cloned();
                    if let Some(id) = id {
                        crate::home_view::open(&id);
                    }
                    return;
                }
                let id = crate::sidebar::source_order(&app.model.borrow())
                    .get(n.wrapping_sub(1))
                    .cloned();
                if let Some(id) = id {
                    app.activate_source(&id);
                }
            }
            // Ctrl+Alt+<n>: the n-th Workspace (R23.5).
            if let Some(n) = action
                .strip_prefix("workspace_")
                .and_then(|n| n.parse::<usize>().ok())
            {
                let id = app
                    .model
                    .borrow()
                    .workspaces
                    .get(n.wrapping_sub(1))
                    .map(|w| w.id.clone());
                if let Some(id) = id {
                    app.switch_workspace(&id);
                }
            }
        }
    }
}

/// Add Source via a folder picker (R1.5).
fn add_source_dialog() {
    let app = app();
    let dialog = gtk::FileDialog::builder()
        .title("Add Source")
        .modal(true)
        .build();
    dialog.select_folder(Some(&app.window), gio::Cancellable::NONE, |res| {
        let Some(path) = res.ok().and_then(|f| f.path()) else {
            return;
        };
        let app = crate::app::app();
        match app.update(|m| m.add_source(&path, None)) {
            Ok(id) => app.activate_source(&id),
            Err(e) => eprintln!("rade: add source: {e}"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R6 / R16.5: Shift+Enter is never captured; R5.3: nor is Ctrl+V.
    #[test]
    fn shift_enter_and_ctrl_v_reach_the_terminal() {
        // apply_keymap's parser, minus the gtk::init check (needs a display).
        use gtk::glib::translate::FromGlib;
        let map = Config::default()
            .keybindings()
            .into_iter()
            .map(|(action, accel)| unsafe {
                let accel = std::ffi::CString::new(accel).unwrap();
                let (mut key, mut mods) = (0, 0);
                gtk::ffi::gtk_accelerator_parse(accel.as_ptr(), &mut key, &mut mods);
                let (key, mods) = (Key::from_glib(key), ModifierType::from_glib(mods));
                (key.to_lower(), mods & MODS, action)
            })
            .collect();
        KEYMAP.with(|k| *k.borrow_mut() = map);
        let (ctrl, shift) = (ModifierType::CONTROL_MASK, ModifierType::SHIFT_MASK);
        assert_eq!(lookup(Key::Return, shift), None);
        assert_eq!(lookup(Key::KP_Enter, shift), None);
        assert_eq!(lookup(Key::v, ctrl), None);
        assert_eq!(lookup(Key::V, ctrl | shift), None, "Ghostty's paste");
        // Sanity: the map is loaded.
        assert_eq!(
            lookup(Key::Return, ctrl | shift).as_deref(),
            Some("split_right")
        );
        assert_eq!(lookup(Key::T, ctrl | shift).as_deref(), Some("new_tab"));
    }
}
