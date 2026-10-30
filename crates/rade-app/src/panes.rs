//! Terminal widgets for Panes: environment (API.md), cwd tracking, close.

use crate::app::try_app;
use crate::terminal::{self, TerminalCallbacks, TerminalOptions, TerminalWidget};
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use rade_core::link::Link;
use rade_core::{process, AgentStatus, Pane, Source, Worktree};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Environment every Terminal gets (API.md "Environment injected into every
/// Terminal"): its Workspace's `[env]` and `RADE_WORKSPACE` (`ws_env`,
/// R23.3) first, so the `RADE_*` variables and the `PATH` prefix after it
/// win. `PATH` is prefixed with the app's directory so `rade` resolves.
pub fn pane_env(
    socket: &Path,
    pane: &str,
    source: &Source,
    worktree: &Worktree,
    ws_env: Vec<(String, String)>,
) -> Vec<(String, String)> {
    let base_path = ws_env
        .iter()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.clone());
    let mut env = ws_env;
    // A Workspace's env can't turn test mode on for a Pane's children (ADR-0021).
    env.retain(|(k, _)| k != "PATH" && k != rade_core::rpc::DEBUG_ENV);
    env.extend([
        ("RADE_SOCKET".into(), socket.display().to_string()),
        ("RADE_PANE_ID".into(), pane.into()),
        ("RADE_SOURCE".into(), source.id.clone()),
        ("RADE_WORKTREE".into(), worktree.name.clone()),
        (
            "RADE_WORKTREE_PATH".into(),
            worktree.path.display().to_string(),
        ),
    ]);
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        let path = base_path.unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());
        env.push(("PATH".into(), format!("{}:{path}", dir.display())));
    }
    env
}

/// Unix seconds (Notification time).
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub fn create(
    pane: &Pane,
    source: &Source,
    worktree: &Worktree,
    ws_env: Vec<(String, String)>,
) -> TerminalWidget {
    let app = try_app().expect("app");
    let id = pane.id.clone();
    let (startup, mut initial_input) = match &pane.setup {
        Some(setup) => rade_core::worktree::setup_launch(setup, pane.command()),
        None => (pane.command().map(str::to_string), None),
    };
    // A restored Pane whose recorded invocation was refused (R108.7): its
    // plain shell prints why.
    if let (Some(n), None) = (&pane.notice, &startup) {
        let typed = rade_core::account::notice_input(n);
        initial_input = Some(typed + initial_input.as_deref().unwrap_or(""));
    }
    let options = TerminalOptions {
        // A new Worktree's first Terminal runs the Source's setup first (R12.2).
        startup_command: startup,
        initial_input,
        extra_env: pane_env(&app.paths.socket, &id, source, worktree, ws_env),
        // Agents and `--cmd` programs: large scrollback (R2.7), and the Pane
        // stays after the process exits so its output can still be read.
        agent_scrollback: pane.is_program(),
        wait_after_command: pane.is_program(),
        focus_on_realize: Rc::new({
            let id = id.clone();
            move || try_app().is_some_and(|a| a.is_focused_visible(&id))
        }),
        ..Default::default()
    };
    let callbacks = TerminalCallbacks {
        on_pwd_changed: Box::new({
            let id = id.clone();
            move |pwd| {
                let Some(app) = try_app() else { return };
                if !app.syncing.get() {
                    app.touch(|m| m.set_pane_cwd(&id, Path::new(pwd)));
                }
            }
        }),
        on_child_exited: Box::new({
            let id = id.clone();
            move |_code| {
                let Some(app) = try_app() else { return };
                app.update(|m| {
                    if m.pane(&id).is_some_and(|p| p.is_program()) {
                        let _ = m.set_status(&id, Some(AgentStatus::Exited), None, None, now());
                    }
                });
            }
        }),
        // OSC 0/2 → Tab label and caption (R1); throttled there.
        on_title_changed: Box::new({
            let id = id.clone();
            move |t| crate::titles::intake(&id, t)
        }),
        on_desktop_notification: Box::new({
            let id = id.clone();
            move |title, body, _| crate::notifications::from_terminal(&id, title, body)
        }),
        on_bell: Box::new({
            let id = id.clone();
            move |_| crate::notifications::bell(&id)
        }),
        // A hand-started Agent's command ended in its shell (R10.2).
        on_command_finished: Box::new({
            let id = id.clone();
            // Not inside Ghostty's callback (its surface map is borrowed):
            // the model update re-renders.
            move || {
                let id = id.clone();
                glib::idle_add_local_once(move || {
                    crate::status::signal(&id, rade_core::status::Signal::CommandFinished)
                });
            }
        }),
        on_open_url: Box::new({
            let id = id.clone();
            move |url| {
                let (id, url) = (id.clone(), url.to_string());
                // Not inside Ghostty's callback: opening changes the Layout.
                glib::idle_add_local_once(move || open_link(&id, &url));
            }
        }),
        // Which join of a wrapped Ctrl+click token opens something (R134).
        link_resolves: Box::new({
            let id = id.clone();
            move |t| link_target(&id, t).is_some()
        }),
        // A right-click the program did not take (R19.6).
        on_context_menu: Box::new({
            let id = id.clone();
            move |x, y| crate::menus::show_terminal_menu(&id, x, y)
        }),
        on_close: Box::new({
            let id = id.clone();
            // The surface closed (shell exited, or a key after a program
            // exited): close the Pane (collapses its split).
            move || {
                let id = id.clone();
                glib::idle_add_local_once(move || {
                    if let Some(app) = try_app() {
                        let _ = app.update(|m| m.close_pane(&id));
                    }
                });
            }
        }),
    };
    let term =
        terminal::create_terminal(Some(&pane.cwd().display().to_string()), options, callbacks);

    // Clicking a Terminal focuses its Pane in the model.
    let focus = gtk::EventControllerFocus::new();
    focus.connect_enter(move |_| {
        if let Some(app) = try_app() {
            app.focus_entered(&id);
        }
    });
    term.handle.gl_area().add_controller(focus);
    term
}

/// What `raw` (a word on Terminal `pane`'s screen) links to (R7.1): a file
/// that exists (relative to the Terminal's cwd, then its Worktree root) or
/// a URL; `None` otherwise.
pub fn link_target(pane: &str, raw: &str) -> Option<Link> {
    let app = try_app()?;
    let (cwd, root) = {
        let m = app.model.borrow();
        let (source, worktree) = m.pane_worktree(pane)?;
        let root = m.worktree(&source, &worktree)?.path.clone();
        (m.pane(pane)?.cwd().clone(), root)
    };
    let home = rade_core::paths::home_dir(&rade_core::paths::process_env);
    rade_core::link::open_target(raw, &cwd, &root, &home)
}

/// Ctrl+click in Terminal `pane` (R7.1), or its menu's "Open …" (R19.6):
/// web links → system browser, an existing file → the Tab's Viewer,
/// focused (Ctrl+Shift+click: a new Viewer, R113.2); anything else does
/// nothing.
pub fn open_link(pane: &str, raw: &str) {
    let side = crate::terminal::take_side_click();
    open_link_at(pane, raw, side);
}

/// [`open_link`]; `side` = Open to the Side (a new Viewer, R113.2).
pub fn open_link_at(pane: &str, raw: &str, side: bool) {
    let Some(app) = try_app() else { return };
    eprintln!("rade: link '{raw}' from {pane}");
    match link_target(pane, raw) {
        Some(Link::File(t)) => {
            let r = app.update(|m| {
                if side {
                    m.open_viewer_side(pane, &t.path, t.line, true)
                } else {
                    m.open_viewer(pane, &t.path, t.line, true)
                }
            });
            if let Err(e) = r {
                eprintln!("rade: open {}: {e}", t.path.display());
            }
        }
        Some(Link::Web(u) | Link::Other(u)) => crate::viewer::launch_uri(&u),
        None => eprintln!("rade: no file for link '{raw}'"),
    }
}

/// Stop a closed Pane's Terminal without blocking the UI. Freeing a Ghostty
/// surface waits (up to 12 s SIGHUP grace + 3 s SIGKILL) until the process
/// group is gone, so SIGHUP it here and free the surface once it has exited
/// (SIGKILL after the same grace).
pub fn retire(pane: &str, t: TerminalWidget) {
    // Its exit must not touch the model: a paused Pane stays (R19.3), and
    // its shell's close would otherwise remove it.
    t.handle
        .replace_callbacks(TerminalCallbacks::disconnected());
    let groups = process::pane_groups(pane);
    for g in &groups {
        process::signal_group(*g, process::SIGHUP);
    }
    let start = Instant::now();
    let mut killed = false;
    glib::timeout_add_local(Duration::from_millis(50), move || {
        // Hold the whole widget: dropping its root would destroy it, and the
        // destroy fallback frees the surface right here, blocking.
        let t = &t;
        let alive: Vec<i32> = groups
            .iter()
            .copied()
            .filter(|g| process::signal_group(*g, 0))
            .collect();
        if alive.is_empty() || start.elapsed() > Duration::from_secs(15) {
            t.handle.shutdown();
            return glib::ControlFlow::Break;
        }
        if !killed && start.elapsed() > Duration::from_secs(12) {
            killed = true;
            for g in &alive {
                process::signal_group(*g, process::SIGKILL);
            }
        }
        glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rade_core::State;

    #[test]
    fn env_has_every_api_variable() {
        let mut ws = State::new(Path::new("/home/u"));
        let id = ws.add_source(Path::new("/code/rade"), None).unwrap();
        let p = ws.source(&id).unwrap();
        let ws_env = vec![
            ("GH_CONFIG_DIR".to_string(), "/g/a".to_string()),
            ("PATH".into(), "/ws/bin".into()),
            ("RADE_WORKSPACE".into(), "a".into()),
        ];
        let env = pane_env(
            Path::new("/run/rade.sock"),
            "p_4",
            p,
            &p.worktrees[0],
            ws_env,
        );
        let get = |k: &str| env.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
        assert_eq!(get("RADE_SOCKET"), Some("/run/rade.sock"));
        assert_eq!(get("RADE_PANE_ID"), Some("p_4"));
        assert_eq!(get("RADE_SOURCE"), Some("rade"));
        assert_eq!(get("RADE_WORKTREE"), Some("main"));
        assert_eq!(get("RADE_WORKTREE_PATH"), Some("/code/rade"));
        // R23.3: the Workspace's env first; RADE's PATH prefix on its PATH.
        assert_eq!(get("GH_CONFIG_DIR"), Some("/g/a"));
        assert_eq!(get("RADE_WORKSPACE"), Some("a"));
        assert_eq!(get("RADE_PROJECT"), None, "removed in M16 (R31.2)");
        assert!(get("PATH").unwrap().ends_with(":/ws/bin"));
        assert_eq!(env.iter().filter(|(k, _)| k == "PATH").count(), 1);
        assert_eq!(env[0].0, "GH_CONFIG_DIR");
    }
}
