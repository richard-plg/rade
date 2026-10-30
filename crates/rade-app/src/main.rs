//! RADE app: GTK4/libadwaita host for embedded Ghostty Terminals, driven by
//! the rade-core model and the `rade` socket API.

// std's `eprintln!`/`println!` panic when the write fails, e.g. EIO once the
// terminal that started the app is closed; inside a GLib callback that panic
// aborts the app. These shadow them in every module below: log, never panic.
macro_rules! eprintln {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($t)*);
    }};
}
macro_rules! println {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($t)*);
    }};
}

mod accounts;
mod activity_view;
mod agent_glyph;
mod annotate_view;
mod app;
mod browser;
mod browser_view;
mod changes;
mod changes_card;
mod changes_ops;
mod chrome;
mod code;
mod code_pop;
mod code_results;
mod debug;
mod diff_highlight;
mod diff_pane;
mod doc_view;
mod fanout;
mod fanout_view;
mod files;
mod fonts;
mod geometry;
mod handlers;
mod home_view;
mod keys;
mod layout_mode;
mod layout_view;
mod markdown_view;
mod menus;
mod navdata;
mod new_menu;
mod notifications;
mod overview_view;
mod palette;
mod palette_run;
mod panes;
mod perf;
mod project_dialogs;
mod project_settings;
mod projects;
mod quick_diff;
mod review;
mod review_view;
mod schedules;
mod schedules_view;
mod search_view;
mod settings_accounts;
mod settings_view;
mod sidebar;
mod status;
mod switcher;
mod syntax;
mod sysmon_view;
mod tabs;
mod terminal;
mod titlebar;
mod titles;
mod tokens;
#[cfg(test)]
mod tokens_gen;
#[cfg(test)]
mod tokens_tests;
mod update;
mod update_view;
mod usage_view;
mod viewer;
mod voice;
mod voice_view;
mod worktrees;

use adw::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::paths::Flavor;
use rade_core::{paths, server, store, State};
use std::rc::Rc;
use std::sync::Arc;

const CSS: &str = include_str!("style.css");
const CSS_DARK: &str = include_str!("style-dark.css");

thread_local! {
    static DARK: gtk::CssProvider = gtk::CssProvider::new();
}

/// RADE's dark tokens (R102.6) and dark rules over the base stylesheet while
/// the style is dark (libadwaita's own `style-dark.css` pattern).
fn style_dark(dark: bool) {
    let display = gtk::gdk::Display::default().expect("display");
    DARK.with(|p| {
        if dark {
            gtk::style_context_add_provider_for_display(
                &display,
                p,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
            );
        } else {
            gtk::style_context_remove_provider_for_display(&display, p);
        }
    });
}

/// The one-time freeze of the 0.3.1 sidebar rule (R30.5, ADR-0028): each
/// linked Worktree the old branch rule showed under exactly one Project
/// gets that Project recorded, so nothing visible moves on upgrade.
fn freeze_sidebar(model: &mut State) {
    if model.sidebar_rule >= rade_core::model::SIDEBAR_RULE {
        return;
    }
    let roots: std::collections::HashMap<String, std::path::PathBuf> = model
        .workspaces
        .iter()
        .map(|w| (w.id.clone(), w.root.clone()))
        .collect();
    let projects_of = |ws: &str| {
        roots
            .get(ws)
            .map(|r| rade_core::project::list(r))
            .unwrap_or_default()
    };
    let mut br = rade_core::design::Branches::default();
    for s in &model.sources {
        let projects = projects_of(&s.workspace);
        for w in s.worktrees.iter().filter(|w| w.path.exists()) {
            if let Some(b) = rade_core::git::branch(&w.path) {
                br.current.insert(w.id.clone(), b);
            }
        }
        let on_main = projects
            .iter()
            .any(|p| p.source(&s.id).is_some_and(|x| x.base.is_none()));
        if let (true, Some(w)) = (on_main, s.worktrees.first()) {
            if let Some(m) = rade_core::git::default_branch(&w.path) {
                br.main.insert(s.id.clone(), m);
            }
        }
    }
    for (s, w, p) in model.freeze_sidebar(&projects_of, &br) {
        eprintln!(
            "rade: sidebar: {s}/{w} recorded under Project {p} (was shown there by its branch)"
        );
    }
}

fn load_model(
    env: paths::Env,
    config: &rade_core::config::Config,
) -> (State, Option<app::SessionRecovery>) {
    let home = paths::home_dir(env);
    let path = paths::state_file(env);
    let mut recovery = None;
    let mut model = match store::load(&path) {
        Ok(Some(m)) => m,
        Ok(None) => State::new(&home),
        Err(e) => {
            eprintln!("rade: {e}; starting with a fresh state");
            recovery = Some(app::SessionRecovery {
                load_error: e,
                backup: store::recovery_backup(&path).ok(),
            });
            State::new(&home)
        }
    };
    model.ensure_general(&home);
    model.user_accounts_file = Some(paths::accounts_file(env));
    freeze_sidebar(&mut model);
    // No process survived the restart (ADR-0005): resume Agents, never
    // replay their prompts (R11.2).
    let user = paths::config_file(env);
    let fallback = config.presets().unwrap_or_default();
    let by_worktree = model.presets_by_worktree(&user, None);
    model.prepare_restore_with(
        &|s, w| {
            by_worktree
                .get(&(s.to_string(), w.to_string()))
                .cloned()
                .unwrap_or_else(|| fallback.clone())
        },
        crate::panes::now(),
    );
    // R108.7: a Pane whose account's dir its restart command overrides
    // (or whose account is gone) comes back as a plain shell saying why.
    model.refuse_restores(
        &|s, w| {
            by_worktree
                .get(&(s.to_string(), w.to_string()))
                .cloned()
                .unwrap_or_else(|| fallback.clone())
        },
        &home,
    );
    // Something to type into on first start.
    let (p, w) = model.active();
    if w.tabs.is_empty() {
        let (p, w) = (p.id.clone(), w.id.clone());
        let _ = model.new_tab(&p, &w, None, None);
    }
    (model, recovery)
}

fn build(gtk_app: &adw::Application) {
    let env: paths::Env = &paths::process_env;
    let app_paths = app::Paths {
        state: paths::state_file(env),
        config: paths::config_file(env),
        socket: paths::socket_path(env),
    };
    let config = keys::load_config(&app_paths.config);
    let (model, recovery) = load_model(env, &config);
    rade_core::paste::cleanup(&paths::paste_dir(env), std::time::SystemTime::now()); // R5.4
    rade_core::paste::cleanup(&annotate_view::shots_dir(), std::time::SystemTime::now()); // R71

    fonts::register(); // R102.3: before the first widget is styled
    syntax::install(); // R112.5: the token code schemes, before any Viewer
    let (light_tokens, dark_tokens) = tokens::css_halves();
    let provider = gtk::CssProvider::new();
    let light_css = format!("{light_tokens}\n{CSS}");
    provider.load_from_string(&light_css);
    let display = gtk::gdk::Display::default().expect("display");
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    DARK.with(|p| p.load_from_string(&format!("{dark_tokens}\n{CSS_DARK}")));
    // R102.6 / R112: `[appearance] color_scheme`, by default dark unless the
    // system asks for light, live.
    keys::apply_color_scheme(&config);
    fonts::apply_code_font(&config.code_font()); // R112.4
    style_dark(adw::StyleManager::default().is_dark());
    // RADE's symbolic icons (status glyphs, R20.2), by name.
    gio::resources_register_include!("rade-icons.gresource").expect("rade icons resource");
    gtk::IconTheme::for_display(&display).add_resource_path("/rade/icons");

    let sidebar = sidebar::Sidebar::new();
    let stack = gtk::Stack::new();
    // Hidden pages must not impose their minimum width on the current page.
    stack.set_hhomogeneous(false);
    stack.set_vexpand(true);
    stack.set_hexpand(true);
    // File tree (R8) in the Right Panel (R20.10), right of the Tabs; the
    // Tabs under Toasts (R20.9).
    let files = files::FileTree::new();
    let chrome = chrome::Chrome::new(&stack, files.widget().upcast_ref());
    // The Workspace button leads the titlebar (R103.1).
    chrome.titlebar.ws_slot.append(&sidebar.switcher.button);
    // Changes (R21) and Search (R22) pages.
    chrome.right.set_page("changes", &changes::install());
    chrome.right.set_page("search", &search_view::install());
    // Activity (R107.2) and Project home (R105) pages.
    chrome.right.set_page("activity", &activity_view::install());
    // Usage: every declared account's logs every 30 s (R109.2).
    accounts::install();
    stack.add_named(&home_view::install(), Some(home_view::PAGE));
    stack.add_named(&usage_view::install(), Some(usage_view::PAGE));
    stack.add_named(&schedules_view::install(), Some(schedules_view::PAGE));
    // Code navigation results open in a popover on the Pane (R85).
    code_results::install();
    let files_split = adw::OverlaySplitView::new();
    files_split.set_sidebar_position(gtk::PackType::End);
    files_split.set_sidebar(Some(&chrome.right.root));
    files_split.set_content(Some(&chrome.center));
    // The Inspector is 340 px (spec A.53).
    let w = f64::from(chrome::INSPECTOR_WIDTH);
    files_split.set_min_sidebar_width(w);
    files_split.set_max_sidebar_width(w);
    files_split.set_show_sidebar(false);
    let content = adw::ToolbarView::new();
    content.set_content(Some(&files_split));
    status::install(&content);

    // The icon rail sits left of the content at large text (R111.2); the
    // full Navigator then floats over it.
    let with_rail = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    with_rail.append(&sidebar.rail);
    content.set_hexpand(true);
    with_rail.append(&content);
    let split = adw::OverlaySplitView::new();
    split.set_sidebar(Some(&sidebar.root));
    split.set_content(Some(&with_rail));
    // The Navigator is 268 px (spec A.17).
    split.set_min_sidebar_width(268.0);
    split.set_max_sidebar_width(268.0);
    split.set_vexpand(true);
    // Titlebar (40 px) · body · Statusline (26 px), R103.
    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.append(&chrome.titlebar.root);
    column.append(&split);
    column.append(chrome.statusline());
    // Opaque window background under the content (also for `debug screenshot`).
    column.add_css_class("background");

    // Command palette (R14) floats over everything.
    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&column));
    // R111: an invisible full-size child tells the window's width.
    let sizer = gtk::DrawingArea::new();
    sizer.set_can_target(false);
    sizer.set_can_focus(false);
    overlay.add_overlay(&sizer);
    overlay.set_measure_overlay(&sizer, false);
    palette::install(&overlay);
    project_dialogs::install(&overlay); // R106 dialogs over a scrim
    settings_view::install(&overlay); // R112 Settings over a scrim
    new_menu::install(&overlay); // R108.6 the + menu
    overview_view::install(&overlay, &column); // R117 Agent Overview

    let window = adw::ApplicationWindow::builder()
        .application(gtk_app)
        .title(Flavor::current().title())
        .default_width(1400)
        .default_height(850)
        .content(&overlay)
        .build();
    // A dialog takes the one primary button while it shows (R103.6).
    window.connect_notify_local(Some("visible-dialog"), |_, _| changes::sync_primary());
    // Focus moving out of the palette (a click elsewhere) closes it.
    window.connect_focus_widget_notify(|w| {
        // A new split's first allocation can give one side all the room and
        // hide the other (`gtk_paned_calc_position`); if that side held the
        // focus (the new Pane), GTK moves it to the Paned. Nothing else
        // focuses a Paned (Terminals take every key): give it back to the
        // model's focused Pane, after GTK's own focus change finished.
        if gtk::prelude::GtkWindowExt::focus(w).is_some_and(|f| f.is::<gtk::Paned>()) {
            glib::idle_add_local_once(|| {
                if let Some(app) = app::try_app() {
                    app.restore_focus();
                }
            });
        }
        if let (Some(p), Some(f)) = (palette::get(), gtk::prelude::GtkWindowExt::focus(w)) {
            if p.open.get() && !f.is_ancestor(&p.root) {
                p.close(false);
            }
        }
    });

    layout_mode::install(
        window.upcast_ref(),
        &sizer,
        &split,
        &files_split,
        sidebar.rail.upcast_ref(),
        &provider,
        light_css,
    );
    let app = Rc::new(app::App::new(
        gtk_app.clone(),
        window.clone(),
        chrome,
        sidebar,
        stack,
        (files, files_split),
        model,
        config,
        app_paths,
    ));
    app::install(app.clone());
    // Restored Terminals start in stages once the window is up (R130).
    app.plan_restore();
    *app.session_recovery.borrow_mut() = recovery;
    app.show_session_feedback();
    // Persist the sidebar migration and restore changes through the guarded save.
    app.schedule_save();

    notifications::install_focus_action(gtk_app);

    if let Err(e) = server::listen(&app.paths.socket, Arc::new(handlers::dispatch)) {
        eprintln!("rade: cannot serve {}: {e}", app.paths.socket.display());
        gtk_app.quit();
        return;
    }
    menus::install(&window); // context-menu actions (R19)
    navdata::install(); // the Navigator's git facts (R104.4)
    navdata::refresh();
    let monitor = keys::install(&window, &app.paths.config);
    *app.config_monitor.borrow_mut() = Some(monitor);

    // Back to the window: the Changes page may be stale (R21.8).
    window.connect_is_active_notify(|w| {
        if let (true, Some(c)) = (w.is_active(), changes::get()) {
            c.refresh();
        }
    });
    window.connect_close_request(|_| {
        app::app().save_now();
        glib::Propagation::Proceed
    });

    let style = adw::StyleManager::default();
    terminal::sync_color_scheme(style.is_dark());
    style.connect_dark_notify(|style| {
        terminal::sync_color_scheme(style.is_dark());
        style_dark(style.is_dark());
        // Tab glyphs are drawn per style (R20.1).
        if let Some(app) = app::try_app() {
            app.sync();
        }
        // Viewers follow the system appearance (R7.3).
        if let Some(app) = app::try_app() {
            let viewers: Vec<_> = app.viewers.borrow().values().cloned().collect();
            viewers.iter().for_each(|v| v.restyle());
            let reviews: Vec<_> = app.reviews.borrow().values().cloned().collect();
            reviews.iter().for_each(|v| v.restyle());
        }
    });

    app.sync();
    sysmon_view::watch(&window);
    agent_glyph::watch();
    window.present();
    app.start_restore();
    // Worktrees made outside RADE show up in the sidebar (R12).
    worktrees::import_all();
    fanout::watch_verdicts();
    update::start();
    log_renders();
}

/// `RADE_LOG_RENDERS=1`: print draws/s per Pane (R4.2 checks: hidden = 0).
fn log_renders() {
    if std::env::var("RADE_LOG_RENDERS").as_deref() != Ok("1") {
        return;
    }
    let mut last = std::collections::HashMap::<String, u64>::new();
    glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
        let app = app::app();
        let mut line = String::from("rade: draws/s");
        for (id, t) in app.terminals.borrow().iter() {
            let n = t.handle.draw_count();
            line += &format!(" {id}={}", n - last.insert(id.clone(), n).unwrap_or(0));
        }
        eprintln!("{line}");
        glib::ControlFlow::Continue
    });
}

/// Drop a parent agent's per-session markers (e.g. RADE launched from a
/// Claude Code shell) so Agents in RADE Terminals start as fresh sessions.
/// User config like CLAUDE_CONFIG_DIR is kept. `RADE_PROJECT` (removed in
/// M16, R31.2) from an enclosing older RADE's Terminal would leak into ours.
fn scrub_inherited_agent_session_env() {
    let stale: Vec<String> = std::env::vars()
        .map(|(k, _)| k)
        .filter(|k| {
            k == "CLAUDECODE"
                || k == "CLAUDE_PID"
                || k.starts_with("CLAUDE_CODE_")
                || k == "RADE_PROJECT"
                || k == rade_core::rpc::DEBUG_ENV
                || update::ENV_VARS.contains(&k.as_str())
        })
        .collect();
    for k in stale {
        std::env::remove_var(k); // single-threaded: runs before GTK/Ghostty start
    }
}

/// Started from the desktop, RADE only has the session's minimal PATH; load
/// the login shell's environment so Agents in ~/.local/bin etc. are found.
/// `RADE_SKIP_SHELL_ENV=1` disables it (tests).
fn import_login_shell_env() {
    if std::env::var_os("RADE_SKIP_SHELL_ENV").is_some() {
        return;
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    match rade_core::shell_env::login_shell_env(&shell) {
        // single-threaded: runs before GTK/Ghostty start
        Some(env) => env.into_iter().for_each(|(k, v)| std::env::set_var(k, v)),
        None => {
            eprintln!("rade: could not read the {shell} login environment; using the inherited one")
        }
    }
}

/// Another RADE already serves this socket (a second launch, the dock's
/// "New Window"): show its window and exit before loading or saving
/// anything, so its state and socket stay untouched.
fn hand_off_to_running_instance() {
    use std::io::{BufRead, Write};
    let path = paths::socket_path(&paths::process_env);
    let Ok(mut s) = std::os::unix::net::UnixStream::connect(&path) else {
        return;
    };
    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(2)));
    let req = r#"{"jsonrpc":"2.0","id":1,"method":"app.present","params":{}}"#;
    let _ = writeln!(s, "{req}");
    let _ = std::io::BufReader::new(s).read_line(&mut String::new());
    eprintln!(
        "rade: another RADE is serving {}; showed its window",
        path.display()
    );
    std::process::exit(0);
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("rade-app {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    // Test mode (ADR-0021): only the environment that launched us counts,
    // not the login shell's; the variable is then removed, so no Pane, git
    // or other child process inherits it.
    let _ =
        handlers::DEBUG_SOCKET.set(std::env::var(rade_core::rpc::DEBUG_ENV).as_deref() == Ok("1"));
    import_login_shell_env();
    // The Update overrides (R127, D27) are read once the login shell's
    // environment is in, before anything else uses them, then removed with
    // the rest.
    update::read_env();
    scrub_inherited_agent_session_env();
    hand_off_to_running_instance();
    // One app per state.json, even on another socket (ADR-0021); held until exit.
    let state = paths::state_file(&paths::process_env);
    match rade_core::store::lock(&state) {
        Ok(f) => std::mem::forget(f),
        Err(e) => {
            eprintln!(
                "rade: another RADE is using {} ({e}); not starting a second one",
                state.display()
            );
            std::process::exit(1);
        }
    }
    notifications::choose_backend();
    glib::set_application_name(Flavor::current().title());
    fonts::prepare(); // ADR-0037: Ghostty sees the bundled fonts
    terminal::prepare_ghostty_runtime();
    terminal::init_ghostty();

    let gtk_app = adw::Application::builder()
        .application_id(Flavor::current().app_id())
        .flags(adw::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    gtk_app.connect_activate(build);
    gtk_app.connect_shutdown(|_| {
        if let Some(app) = app::try_app() {
            app.save_now();
            let _ = std::fs::remove_file(&app.paths.socket);
            code::stop_all();
        }
        update::shutdown();
    });
    gtk_app.run_with_args::<&str>(&[]);
    // State is saved in `connect_shutdown`. Skip thread-local destructors:
    // they free Ghostty surfaces whose callbacks touch already-destroyed
    // thread-locals, aborting with "cannot access a Thread Local Storage
    // value during or after destruction" when the window is closed.
    extern "C" {
        fn _exit(code: i32) -> !;
    }
    unsafe { _exit(0) }
}
