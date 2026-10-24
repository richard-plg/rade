//! Basic actions (PRD R19, M13): the Tab, Source and Terminal context
//! menus, what their items run (also from the palette and keys), the
//! dialogs they ask with, and `debug.menu` / `debug.dialog` /
//! `debug.search` for the E2E harness.
//!
//! Every item is an action of the window's `m` group with a string target
//! (a Tab, Source or Pane id; `\x1f`-separated extras), so a menu built for
//! a right-click and the one `debug.menu` builds run the same code.

use crate::app::{app, try_app};
use adw::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::basic::CloseMode;
use rade_core::rpc::{Error, Result};
use rade_core::{paths, Source, SplitDir};
use serde_json::{json, Value};
use std::cell::RefCell;

const SEP: char = '\u{1f}';

thread_local! {
    /// The context menu last opened by RADE (Terminal, `debug.menu open`).
    static OPEN_MENU: RefCell<Option<gtk::PopoverMenu>> = const { RefCell::new(None) };
    /// The confirmation on screen and its responses (`debug.dialog`).
    static DIALOG: RefCell<Option<(adw::AlertDialog, Vec<String>)>> = const { RefCell::new(None) };
}

fn split2(v: &str) -> (&str, &str) {
    v.split_once(SEP).unwrap_or((v, ""))
}

/// The `m` action group on the window.
pub fn install(window: &adw::ApplicationWindow) {
    let group = gio::SimpleActionGroup::new();
    let add = |name: &str, f: fn(&str)| {
        let a = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
        a.connect_activate(move |_, v| {
            let v: String = v.and_then(|v| v.get()).unwrap_or_default();
            // After the menu that ran it has closed.
            glib::idle_add_local_once(move || f(&v));
        });
        group.add_action(&a);
    };
    // Tabs (R19.1).
    add("tab-rename", |t| {
        // Shown and selected first: the entry points at its label.
        let app = app();
        let shown = app.update(|m| {
            let (p, w) = m.locate_tab(t)?;
            let focused = m.worktree(&p, &w)?.tab(t)?.focused.clone();
            m.focus_pane(&focused).ok()
        });
        if shown.is_some() {
            glib::idle_add_local_once(crate::tabs::rename_selected);
        }
    });
    add("tab-duplicate", |t| {
        if let Err(e) = app().update(|m| m.duplicate_tab(t)) {
            crate::worktrees::alert("Could not duplicate the Tab", &e.message);
        }
    });
    add("tab-close", |t| close_tabs_ui(t, CloseMode::Only));
    add("tab-close-others", |t| close_tabs_ui(t, CloseMode::Others));
    add("tab-close-right", |t| close_tabs_ui(t, CloseMode::Right));
    // Terminals (R19.6): Ghostty's own binding actions on that surface.
    add("pane-copy", |p| binding(p, "copy_to_clipboard"));
    add("pane-paste", |p| binding(p, "paste_from_clipboard"));
    add("pane-select-all", |p| binding(p, "select_all"));
    add("pane-search", |p| binding(p, "start_search"));
    add("pane-clear", |p| binding(p, "clear_screen"));
    add("pane-split-right", |p| split(p, SplitDir::Right));
    add("pane-split-down", |p| split(p, SplitDir::Down));
    add("pane-close", close_pane_ui);
    // Zoom that Pane (R1.7): it takes its Tab, or the Layout comes back.
    add("pane-zoom", |p| {
        let app = app();
        let _ = app.update(|m| {
            m.focus_pane(p)?;
            let tab = m
                .locate(p)
                .ok_or_else(|| Error::not_found("no such Pane"))?
                .tab;
            m.toggle_zoom(&tab)
        });
    });
    add("pane-rename", |v| {
        let (p, xy) = split2(v);
        let at = xy
            .split_once(SEP)
            .and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)));
        rename_pane_ui(p, at);
    });
    add("pane-open", |v| {
        let (p, word) = split2(v);
        crate::panes::open_link_at(p, word, false);
    });
    add("pane-open-side", |v| {
        let (p, word) = split2(v);
        crate::panes::open_link_at(p, word, true);
    });
    // Sources (R19.2).
    add("source-rename", rename_source_ui);
    add("source-pause", pause_ui);
    add("source-resume", |p| {
        if let Err(e) = app().resume_source(p) {
            crate::worktrees::alert("Could not resume the Source", &e.message);
        }
    });
    add("source-remove", remove_source_dialog);
    add("source-editor", open_in_editor);
    add("source-files", |p| {
        if let Some(path) = source_path(p) {
            crate::viewer::launch_uri(&gio::File::for_path(&path).uri());
        }
    });
    window.insert_action_group("m", Some(&group));
}

fn item(menu: &gio::Menu, label: &str, action: &str, target: &str) {
    let i = gio::MenuItem::new(Some(label), None);
    i.set_action_and_target_value(Some(&format!("m.{action}")), Some(&target.to_variant()));
    set_accel(&i, action);
    menu.append_item(&i);
}

/// Show the key that runs `action` at the item's right (R19.8).
pub(crate) fn set_accel(i: &gio::MenuItem, action: &str) {
    if let Some(k) = try_app().and_then(|a| a.config.borrow().menu_accel(action)) {
        i.set_attribute_value("accel", Some(&k.to_variant()));
    }
}

/// A Pane caption's menu button (R20.4).
pub fn pane_menu(pane: &str) -> gio::Menu {
    let menu = gio::Menu::new();
    let top = gio::Menu::new();
    item(&top, "Rename Pane…", "pane-rename", pane);
    item(&top, "Split Right", "pane-split-right", pane);
    item(&top, "Split Down", "pane-split-down", pane);
    item(&top, "Zoom", "pane-zoom", pane);
    let close = gio::Menu::new();
    item(&close, "Close Pane", "pane-close", pane);
    menu.append_section(None, &top);
    menu.append_section(None, &close);
    menu
}

// ---- Tabs ------------------------------------------------------------------

/// The Tab menu for `tab` (AdwTabView's menu model, refilled as it opens).
/// Items that would do nothing are left out.
pub fn fill_tab_menu(menu: &gio::Menu, tab: &str) {
    menu.remove_all();
    let Some(app) = try_app() else { return };
    let m = app.model.borrow();
    let Some((p, w)) = m.locate_tab(tab) else {
        return;
    };
    // Only what would do something: nothing opens in a paused Source, and
    // Fan-out Tabs are never closed on the side.
    let some = |mode| {
        m.tab_close_set(tab, mode)
            .is_ok_and(|s| !s.close.is_empty())
    };
    let top = gio::Menu::new();
    item(&top, "Rename…", "tab-rename", tab);
    if !m.is_paused(&p) {
        item(&top, "Duplicate", "tab-duplicate", tab);
    }
    let close = gio::Menu::new();
    item(&close, "Close", "tab-close", tab);
    if some(CloseMode::Others) {
        item(&close, "Close Other Tabs", "tab-close-others", tab);
    }
    if some(CloseMode::Right) {
        item(&close, "Close Tabs to the Right", "tab-close-right", tab);
    }
    let _ = w;
    menu.append_section(None, &top);
    menu.append_section(None, &close);
}

/// Close one Tab, the others, or those to its right (R19.1). A Fan-out Tab
/// alone asks to discard its Fan-out (as its close button does); in Others /
/// Right it is left open. Always asks first (R19.9).
pub fn close_tabs_ui(tab: &str, mode: CloseMode) {
    let app = app();
    let (set, prompt, fanout) = {
        let m = app.model.borrow();
        let fanout = m
            .fanout_of_tab(tab)
            .map(|f| (f.id.clone(), f.source.clone()));
        match m.tab_close_set(tab, mode) {
            Ok(set) => {
                let prompt = m.close_tabs_prompt(&set);
                (set, Some(prompt), None)
            }
            Err(_) => (Default::default(), None, fanout),
        }
    };
    if let Some((f, p)) = fanout {
        return crate::fanout_view::discard_dialog(&f, &p);
    }
    let Some(prompt) = prompt.filter(|_| !set.close.is_empty()) else {
        return;
    };
    let tabs = set.close;
    confirm_close(prompt, move || {
        let _ = app.close_tabs(&tabs, false);
    });
}

/// Close Pane `pane` from the UI (its ✕, the close key, menus, palette):
/// a Fan-out Agent asks to discard first (R15.6), a Terminal asks (R19.9),
/// a Viewer / Review / Browser closes at once.
pub fn close_pane_ui(pane: &str) {
    let app = app();
    if app.model.borrow().check_close(pane).is_err() {
        return crate::fanout_view::close_member_dialog(pane);
    }
    let prompt = app.model.borrow().close_pane_prompt(pane);
    let pane = pane.to_string();
    match prompt {
        Some(prompt) => confirm_close(prompt, move || {
            let _ = app.close_pane(&pane, false);
        }),
        None => {
            let _ = app.close_pane(&pane, false);
        }
    }
}

/// The close confirmation: Cancel (default, Esc) or a destructive Close.
fn confirm_close(prompt: rade_core::basic::ClosePrompt, yes: impl Fn() + 'static) {
    let d = adw::AlertDialog::new(Some(&prompt.heading), Some(&prompt.body));
    d.add_response("cancel", "Cancel");
    d.add_response("close", &prompt.action);
    d.set_response_appearance("close", adw::ResponseAppearance::Destructive);
    d.set_default_response(Some("cancel"));
    d.set_close_response("cancel");
    d.connect_response(None, move |_, r| {
        if r == "close" {
            yes();
        }
    });
    present(d, &["cancel", "close"]);
}

// ---- Sources --------------------------------------------------------------

fn source_path(source: &str) -> Option<std::path::PathBuf> {
    let app = try_app()?;
    let m = app.model.borrow();
    m.source(source).map(|p| p.path.clone())
}

/// "Open in VS Code" (`[viewer] editor`; its name for another editor), when
/// that editor is on PATH.
fn editor_label() -> Option<String> {
    let ed = crate::viewer::gui_editor()?;
    let bin = ed
        .split_whitespace()
        .next()?
        .rsplit('/')
        .next()?
        .to_string();
    Some(if bin == "code" {
        "Open in VS Code".into()
    } else {
        format!("Open in {bin}")
    })
}

/// The Source row's menu (R12, R13, R19.2). General: Rename and Open in
/// Files only (it can't be paused or removed; it is `$HOME`).
pub fn source_menu(p: &Source) -> gio::Menu {
    let menu = gio::Menu::new();
    let top = gio::Menu::new();
    item(&top, "Rename…", "source-rename", &p.id);
    if !p.is_general() {
        if p.paused {
            item(&top, "Resume", "source-resume", &p.id);
        } else {
            item(&top, "Pause", "source-pause", &p.id);
        }
    }
    let open = gio::Menu::new();
    if !p.is_general() {
        if let Some(label) = editor_label() {
            item(&open, &label, "source-editor", &p.id);
        }
    }
    item(&open, "Open in Files", "source-files", &p.id);
    menu.append_section(None, &top);
    menu.append_section(None, &open);
    if !p.is_general() {
        // The sidebar's own Worktree actions (`sb` group on the list).
        let git = gio::Menu::new();
        for (label, action) in [
            ("New Worktree…", "sb.new-worktree"),
            ("Review Changes", "sb.review"),
        ] {
            let i = gio::MenuItem::new(Some(label), None);
            i.set_action_and_target_value(
                Some(action),
                Some(&format!("{}{SEP}", p.id).to_variant()),
            );
            set_accel(&i, action.trim_start_matches("sb."));
            git.append_item(&i);
        }
        menu.append_section(None, &git);
        let remove = gio::Menu::new();
        item(&remove, "Remove Source…", "source-remove", &p.id);
        menu.append_section(None, &remove);
    }
    menu
}

fn open_in_editor(source: &str) {
    let (Some(ed), Some(path)) = (crate::viewer::gui_editor(), source_path(source)) else {
        return;
    };
    let mut argv: Vec<std::ffi::OsString> = ed.split_whitespace().map(Into::into).collect();
    argv.push(path.into());
    let argv: Vec<&std::ffi::OsStr> = argv.iter().map(|a| a.as_os_str()).collect();
    if let Err(e) = gio::Subprocess::newv(&argv, gio::SubprocessFlags::NONE) {
        eprintln!("rade: {}: {e}", argv[0].to_string_lossy());
    }
}

/// Rename a Source in a popover on its sidebar row.
pub fn rename_source_ui(source: &str) {
    let app = app();
    let Some(name) = app.model.borrow().source(source).map(|p| p.name.clone()) else {
        return;
    };
    let (parent, at) = match app.sidebar.source_row(source) {
        Some((w, r)) => (w, Some(r)),
        None => (app.window.clone().upcast(), None),
    };
    let id = source.to_string();
    crate::titles::rename_popover(
        &parent,
        at,
        &name,
        &name,
        "The Source's display name; its id stays",
        move |n| app.update(|m| m.rename_source(&id, n)).map(|_| ()),
    );
}

/// Pause (R19.3): at once when nothing is working, else after a dialog
/// naming the Agents whose turns it cuts.
pub fn pause_ui(source: &str) {
    let app = app();
    let (name, busy, fanouts) = {
        let m = app.model.borrow();
        let Some(p) = m.source(source) else { return };
        let tabs: Vec<String> = p
            .worktrees
            .iter()
            .flat_map(|w| w.tabs.iter().map(|t| t.id.clone()))
            .collect();
        let fanouts: Vec<String> = m
            .fanouts
            .iter()
            .filter(|f| f.source == p.id)
            .filter(|f| {
                matches!(
                    f.phase(),
                    rade_core::fanout::Phase::Running | rade_core::fanout::Phase::Judging
                )
            })
            .map(|f| f.id.clone())
            .collect();
        (p.name.clone(), m.busy_agents(&tabs), fanouts)
    };
    let id = source.to_string();
    if busy.is_empty() && fanouts.is_empty() {
        if let Err(e) = app.pause_source(&id, false) {
            crate::worktrees::alert("Could not pause the Source", &e.message);
        }
        return;
    }
    let mut body = String::from(
        "Every process in it stops; its Tabs and Panes stay. Resume starts them again: \
         Agents resume their sessions, and a cut turn comes back interrupted.\n\nStill working:",
    );
    for (_, n, s) in &busy {
        body += &format!("\n• {n} — {}", s.as_str());
    }
    for f in &fanouts {
        body += &format!("\n• fan-out {f}");
    }
    let d = adw::AlertDialog::new(Some(&format!("Pause “{name}”?")), Some(&body));
    d.add_response("cancel", "Cancel");
    d.add_response("pause", "Pause");
    d.set_response_appearance("pause", adw::ResponseAppearance::Destructive);
    d.set_default_response(Some("cancel"));
    d.set_close_response("cancel");
    d.connect_response(None, move |_, r| {
        if r == "pause" {
            if let Err(e) = app.pause_source(&id, true) {
                crate::worktrees::alert("Could not pause the Source", &e.message);
            }
        }
    });
    present(d, &["cancel", "pause"]);
}

/// Remove Source… (R1.5, R19.2): confirm; files are never touched.
pub fn remove_source_dialog(source: &str) {
    let app = app();
    let (name, path, panes, wts) = {
        let m = app.model.borrow();
        let Some(p) = m.source(source).filter(|p| !p.is_general()) else {
            return;
        };
        let panes: usize = p
            .worktrees
            .iter()
            .flat_map(|w| &w.tabs)
            .map(|t| t.panes().len())
            .sum();
        let data = paths::data_dir(&paths::process_env);
        let wts = m.worktree_dir(&p.id, "", &data, paths::Flavor::current());
        (p.name.clone(), p.path.clone(), panes, wts)
    };
    let mut body = format!(
        "RADE forgets it and closes its {panes} Pane{} (Agents are stopped).\n\n\
         No files are deleted: {} stays as it is",
        if panes == 1 { "" } else { "s" },
        path.display()
    );
    if wts.is_dir() {
        body += &format!(
            ", and so do the Worktrees RADE created for it in {}",
            wts.display()
        );
    }
    body.push('.');
    let d = adw::AlertDialog::new(Some(&format!("Remove Source “{name}”?")), Some(&body));
    d.add_response("cancel", "Cancel");
    d.add_response("remove", "Remove");
    d.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
    d.set_default_response(Some("cancel"));
    d.set_close_response("cancel");
    let id = source.to_string();
    d.connect_response(None, move |_, r| {
        if r == "remove" {
            if let Err(e) = app.update(|m| m.remove_source(&id)) {
                crate::worktrees::alert("Could not remove the Source", &e.message);
            }
            app.save_now();
        }
    });
    present(d, &["cancel", "remove"]);
}

pub(crate) fn present(d: adw::AlertDialog, responses: &[&str]) {
    d.connect_closed(|d| {
        DIALOG.with(|x| {
            let mut x = x.borrow_mut();
            if x.as_ref().is_some_and(|(o, _)| o == d) {
                x.take();
            }
        });
    });
    let window = app().window.clone();
    DIALOG.with(|x| {
        *x.borrow_mut() = Some((d.clone(), responses.iter().map(|r| r.to_string()).collect()))
    });
    d.present(Some(&window));
}

// ---- Terminals -------------------------------------------------------------

fn binding(pane: &str, action: &str) {
    let app = app();
    let handle = app.terminals.borrow().get(pane).map(|t| t.handle.clone());
    if let Some(h) = handle {
        h.gl_area().grab_focus();
        h.perform_binding_action(action);
    }
}

fn split(pane: &str, dir: SplitDir) {
    if let Err(e) = app().update(|m| m.split(pane, dir, None, None)) {
        crate::worktrees::alert("Could not split", &e.message);
    }
}

/// Rename a Pane (R19.4) in a popover over its Terminal (at `at`, else
/// its top).
pub fn rename_pane_ui(pane: &str, at: Option<(f64, f64)>) {
    let app = app();
    let Some(name) = app.model.borrow().pane(pane).map(|p| p.name.clone()) else {
        return;
    };
    let parent: gtk::Widget = match app.terminals.borrow().get(pane) {
        Some(t) => t.handle.gl_area().clone().upcast(),
        None => app
            .window
            .content()
            .unwrap_or_else(|| app.window.clone().upcast()),
    };
    let rect = match at {
        Some((x, y)) => gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1),
        None => gtk::gdk::Rectangle::new(parent.width() / 2, 0, 1, 1),
    };
    let id = pane.to_string();
    crate::titles::rename_popover(
        &parent,
        Some(rect),
        name.as_deref().unwrap_or(""),
        pane,
        "Addresses the Pane in `rade` commands; empty: no name",
        move |n| app.update(|m| m.rename_pane(&id, Some(n))),
    );
}

/// The Terminal menu (R19.6). `link` = a path or URL under the pointer.
fn terminal_menu(pane: &str, selection: bool, link: Option<&str>, x: f64, y: f64) -> gio::Menu {
    let menu = gio::Menu::new();
    if let Some(word) = link {
        let s = gio::Menu::new();
        let shown = rade_core::title::ellipsize(word, 40);
        item(
            &s,
            &format!("Open “{shown}”"),
            "pane-open",
            &format!("{pane}{SEP}{word}"),
        );
        // A file: also in a new Viewer (R113.2).
        if matches!(
            crate::panes::link_target(pane, word),
            Some(rade_core::link::Link::File(_))
        ) {
            item(
                &s,
                "Open to the Side",
                "pane-open-side",
                &format!("{pane}{SEP}{word}"),
            );
        }
        menu.append_section(None, &s);
    }
    let edit = gio::Menu::new();
    if selection {
        item(&edit, "Copy", "pane-copy", pane);
    }
    item(&edit, "Paste", "pane-paste", pane);
    item(&edit, "Select All", "pane-select-all", pane);
    let find = gio::Menu::new();
    item(&find, "Search…", "pane-search", pane);
    item(&find, "Clear", "pane-clear", pane);
    let layout = gio::Menu::new();
    item(&layout, "Split Right", "pane-split-right", pane);
    item(&layout, "Split Down", "pane-split-down", pane);
    item(&layout, "Zoom", "pane-zoom", pane);
    let name = gio::Menu::new();
    item(
        &name,
        "Rename Pane…",
        "pane-rename",
        &format!("{pane}{SEP}{x}{SEP}{y}"),
    );
    item(&name, "Close Pane", "pane-close", pane);
    for s in [edit, find, layout, name] {
        menu.append_section(None, &s);
    }
    menu
}

/// (menu, its parent widget) for a right-click at (x, y) on `pane`.
fn terminal_menu_at(pane: &str, x: f64, y: f64) -> Option<(gio::Menu, gtk::Widget)> {
    let app = try_app()?;
    let handle = app.terminals.borrow().get(pane)?.handle.clone();
    let link = handle
        .word_at(x, y)
        .filter(|w| crate::panes::link_target(pane, w).is_some());
    let menu = terminal_menu(pane, handle.has_selection(), link.as_deref(), x, y);
    Some((menu, handle.gl_area().clone().upcast()))
}

/// A right-click on Tab `tab` (R19.1): its menu at `at` on `parent`.
pub fn popup_tab_menu(tab: &str, parent: &gtk::Widget, at: gtk::gdk::Rectangle) {
    let menu = gio::Menu::new();
    fill_tab_menu(&menu, tab);
    popup(&menu, parent, at, true);
}

/// A right-click Ghostty did not take (`on_context_menu`).
pub fn show_terminal_menu(pane: &str, x: f64, y: f64) {
    if let Some((menu, parent)) = terminal_menu_at(pane, x, y) {
        popup(
            &menu,
            &parent,
            gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1),
            true,
        );
    }
}

fn popup(menu: &gio::Menu, parent: &gtk::Widget, at: gtk::gdk::Rectangle, autohide: bool) {
    close_menu();
    let pop = gtk::PopoverMenu::from_model(Some(menu));
    // Headless Weston has no seat to grab: `debug.menu open` shows it without.
    pop.set_autohide(autohide);
    pop.set_parent(parent);
    pop.set_has_arrow(false);
    pop.set_halign(gtk::Align::Start);
    pop.set_position(gtk::PositionType::Bottom);
    pop.set_pointing_to(Some(&at));
    pop.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || {
            OPEN_MENU.with(|m| {
                let mut m = m.borrow_mut();
                if m.as_ref() == Some(&p) {
                    m.take();
                }
            });
            p.unparent();
        });
    });
    keyboard(&pop);
    OPEN_MENU.with(|m| *m.borrow_mut() = Some(pop.clone()));
    pop.popup();
    // The keyboard into it, on its first item (M14: the Terminal menu kept
    // it on the Terminal).
    let p = pop.clone();
    glib::idle_add_local_once(move || {
        if let Some(first) = menu_buttons(&p).first() {
            first.grab_focus();
        }
    });
}

/// A menu's items in order (GtkModelButton is private to GTK: by type name).
fn menu_buttons(pop: &gtk::PopoverMenu) -> Vec<gtk::Widget> {
    let mut out = Vec::new();
    let mut stack = vec![pop.clone().upcast::<gtk::Widget>()];
    while let Some(w) = stack.pop() {
        if w.type_().name() == "GtkModelButton" {
            if w.is_visible() && w.is_sensitive() {
                out.push(w);
            }
            continue;
        }
        let mut c = w.last_child();
        while let Some(x) = c {
            c = x.prev_sibling();
            stack.push(x);
        }
    }
    out
}

/// Arrows and Tab move through the menu's items, wrapping, whatever the
/// compositor did with the popup's keyboard grab (live, M14: without one,
/// Down / Tab moved focus out into the window and the menu was lost).
fn keyboard(pop: &gtk::PopoverMenu) {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let p = pop.clone();
    keys.connect_key_pressed(move |_, key, _, state| {
        use gtk::gdk::Key;
        let back = match key {
            Key::Down | Key::KP_Down => false,
            Key::Up | Key::KP_Up | Key::ISO_Left_Tab => true,
            Key::Tab => state.contains(gtk::gdk::ModifierType::SHIFT_MASK),
            _ => return glib::Propagation::Proceed,
        };
        let items = menu_buttons(&p);
        if items.is_empty() {
            return glib::Propagation::Proceed;
        }
        let root = p.root();
        let focus = root.and_then(|r| r.focus());
        let at = items.iter().position(|w| Some(w) == focus.as_ref());
        let n = items.len();
        let next = match (at, back) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(i), false) => (i + 1) % n,
            (Some(i), true) => (i + n - 1) % n,
        };
        items[next].grab_focus();
        glib::Propagation::Stop
    });
    pop.add_controller(keys);
}

/// A context menu RADE opened is on screen (`debug.window`, live harness).
pub fn menu_open() -> bool {
    OPEN_MENU.with(|m| m.borrow().as_ref().is_some_and(|p| p.is_visible()))
}

fn close_menu() {
    if let Some(p) = OPEN_MENU.with(|m| m.borrow_mut().take()) {
        p.popdown();
    }
}

// ---- debug ----------------------------------------------------------------

/// `debug.menu` (`{"menu": "tab"|"source"|"scratch"|"terminal", "target", "op",
/// "item"?, "path"?, "text"?}`): the menu RADE shows for that target —
/// `items` lists it, `activate` runs the item labelled `item` through its
/// action, `open` pops it up (a Terminal's at the first `text` on its
/// screen, else its centre), `shot` renders the open one to `path`, `close`.
pub fn debug_menu(p: &Value) -> Result<Value> {
    let app = app();
    let s = |k: &str| p.get(k).and_then(Value::as_str);
    let kind = s("menu").unwrap_or("");
    let target = s("target").unwrap_or("");
    let op = s("op").unwrap_or("items");
    if op == "shot" {
        let path = s("path").ok_or_else(|| Error::usage("shot needs a path"))?;
        let pop = OPEN_MENU
            .with(|m| m.borrow().clone())
            .ok_or_else(|| Error::usage("no menu is open"))?;
        crate::debug::screenshot_widget(pop.upcast_ref(), std::path::Path::new(path))?;
        // Headless Weston may allocate a popup shorter than it asks (seen:
        // the last item cut): say so; the items are checked in `items`.
        let (_, natural, _, _) = pop.measure(gtk::Orientation::Vertical, -1);
        return Ok(json!({"path": path, "complete": pop.height() >= natural}));
    }
    if op == "close" {
        close_menu();
        return Ok(json!({}));
    }
    let (menu, parent, at) = match kind {
        "tab" => {
            let tab = app.model.borrow().resolve_tab(target, None)?;
            let menu = gio::Menu::new();
            fill_tab_menu(&menu, &tab);
            let bar: gtk::Widget = app.window.clone().upcast();
            (menu, bar, gtk::gdk::Rectangle::new(320, 60, 1, 1))
        }
        "source" => {
            let m = app.model.borrow();
            let proj = m.find_source(target)?;
            let menu = source_menu(proj);
            let (parent, at) = app
                .sidebar
                .source_row(&proj.id)
                .ok_or_else(|| Error::internal("no sidebar row"))?;
            (menu, parent, at)
        }
        // A Source's Scratch row (R104.5); `target` = its id or name.
        "scratch" => {
            let m = app.model.borrow();
            let src = m.find_source(target)?;
            let menu = crate::sidebar::scratch_menu(&m, src);
            let (parent, at) = app
                .sidebar
                .scratch_row(&src.id)
                .ok_or_else(|| Error::not_found(format!("no Scratch row '{target}'")))?;
            (menu, parent, at)
        }
        // A Project header (R30.3); `target` = its id.
        "project" => {
            let (parent, at) = app
                .sidebar
                .project_row(target)
                .ok_or_else(|| Error::not_found(format!("no Project row '{target}'")))?;
            let key = format!("{}/{target}", app.model.borrow().active_workspace());
            (crate::projects::menu(&key), parent, at)
        }
        // A Worktree row (R30.4); `target` = `<source>/<worktree>` (ids or names).
        "worktree" => {
            let (src, wt) = target
                .split_once('/')
                .ok_or_else(|| Error::usage("worktree target is <source>/<worktree>"))?;
            let (src, wt) = app.model.borrow().find_worktree(src, Some(wt))?;
            let menu = crate::sidebar::worktree_menu(&app.model.borrow(), &src, &wt);
            let (parent, at) = app
                .sidebar
                .worktree_row(&src, &wt)
                .ok_or_else(|| Error::not_found(format!("no sidebar row for '{target}'")))?;
            (menu, parent, at)
        }
        "terminal" => {
            let (id, t) = crate::handlers::terminal(&app, target, None)?;
            let (x, y) = match s("text") {
                Some(text) => {
                    let screen = t
                        .handle
                        .read_text(rade_ghostty_sys::GHOSTTY_POINT_VIEWPORT)
                        .unwrap_or_default();
                    let cols = t.handle.health().columns.max(1) as usize;
                    let (row, col) = crate::debug::find_cell(&screen, text, cols)
                        .ok_or_else(|| Error::not_found(format!("'{text}' is not on screen")))?;
                    t.handle
                        .cell_point(row as u32, col as u32)
                        .ok_or_else(|| Error::internal("pane has not started"))?
                }
                None => (60.0, 20.0),
            };
            drop(t);
            let (menu, parent) =
                terminal_menu_at(&id, x, y).ok_or_else(|| Error::internal("no terminal"))?;
            (
                menu,
                parent,
                gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1),
            )
        }
        o => {
            return Err(Error::usage(format!(
                "menu must be tab|source|scratch|project|worktree|terminal, not '{o}'"
            )))
        }
    };
    let items = menu_items(menu.upcast_ref());
    match op {
        "items" => {}
        // A right-click at that point through Ghostty (R19.6): the menu opens
        // only when the program did not take it; `arg` = "shift".
        "rightclick" if kind == "terminal" => {
            let id = app.model.borrow().resolve_pane(target, None)?;
            let handle = app.terminals.borrow().get(&id).map(|t| t.handle.clone());
            let consumed = handle
                .and_then(|h| {
                    h.right_click(at.x() as f64, at.y() as f64, s("arg") == Some("shift"))
                })
                .ok_or_else(|| Error::internal("pane has not started"))?;
            if !consumed {
                show_terminal_menu(&id, at.x() as f64, at.y() as f64);
            }
            let open = OPEN_MENU.with(|m| m.borrow().is_some());
            return Ok(json!({"consumed": consumed, "menu_open": open}));
        }
        "open" => popup(&menu, &parent, at, false),
        "activate" => {
            let want = s("item").ok_or_else(|| Error::usage("activate needs an item"))?;
            let (_, action, target, _) = items
                .iter()
                .find(|(l, _, _, _)| l == want || l.starts_with(want))
                .ok_or_else(|| Error::not_found(format!("no menu item '{want}'")))?;
            let on: gtk::Widget = if action.starts_with("sb.") {
                parent.clone()
            } else {
                app.window.clone().upcast()
            };
            WidgetExt::activate_action(&on, action, Some(&target.to_variant()))
                .map_err(|e| Error::internal(e.to_string()))?;
        }
        o => return Err(Error::usage(format!("unknown op '{o}'"))),
    }
    let items: Vec<Value> = items
        .into_iter()
        .map(|(label, action, target, accel)| {
            json!({"label": label, "action": action, "target": target, "accel": accel})
        })
        .collect();
    Ok(json!({ "items": items }))
}

/// (label, action, target) of every item, sections flattened.
fn menu_items(menu: &gio::MenuModel) -> Vec<(String, String, String, String)> {
    let mut out = Vec::new();
    for i in 0..menu.n_items() {
        if let Some(sub) = menu.item_link(i, "section") {
            out.extend(menu_items(&sub));
            continue;
        }
        let attr = |k: &str| {
            menu.item_attribute_value(i, k, None)
                .and_then(|v| v.get::<String>())
                .unwrap_or_default()
        };
        // A submenu's items as `Parent ▸ Item`.
        if let Some(sub) = menu.item_link(i, "submenu") {
            let parent = attr("label");
            out.extend(
                menu_items(&sub)
                    .into_iter()
                    .map(|(l, a, t, k)| (format!("{parent} ▸ {l}"), a, t, k)),
            );
            continue;
        }
        out.push((attr("label"), attr("action"), attr("target"), attr("accel")));
    }
    out
}

/// `debug.dialog` (`{"op": "info"|"respond", "response"?}`): the
/// confirmation on screen, or answer it like its button.
pub fn debug_dialog(p: &Value) -> Result<Value> {
    let d = DIALOG.with(|x| x.borrow().clone());
    let Some((d, responses)) = d else {
        return Ok(json!({"open": false}));
    };
    // A form field of the dialog (`rade-field-<field>`): an entry's text or
    // a dropdown's item (R30.3).
    if p.get("op").and_then(Value::as_str) == Some("set") {
        let field = p.get("field").and_then(Value::as_str).unwrap_or("");
        let value = p.get("value").and_then(Value::as_str).unwrap_or("");
        let w = d
            .extra_child()
            .and_then(|c| find_class(&c, &format!("rade-field-{field}")))
            .ok_or_else(|| Error::not_found(format!("no field '{field}'")))?;
        if let Some(e) = w.downcast_ref::<gtk::Entry>() {
            e.set_text(value);
        } else if let Some(t) = w.downcast_ref::<gtk::TextView>() {
            t.buffer().set_text(value);
        } else if let Some(c) = w.downcast_ref::<gtk::CheckButton>() {
            c.set_active(value == "true");
        } else if let Some(dd) = w.downcast_ref::<gtk::DropDown>() {
            let list = dd
                .model()
                .and_downcast::<gtk::StringList>()
                .ok_or_else(|| Error::internal("not a string list"))?;
            let i = (0..list.n_items())
                .find(|i| list.string(*i).is_some_and(|s| s == value))
                .ok_or_else(|| Error::not_found(format!("no item '{value}' in '{field}'")))?;
            dd.set_selected(i);
        } else {
            return Err(Error::usage(format!("field '{field}' can't be set")));
        }
    }
    if p.get("op").and_then(Value::as_str) == Some("respond") {
        let r = p.get("response").and_then(Value::as_str).unwrap_or("");
        if !responses.iter().any(|x| x == r) {
            return Err(Error::usage(format!(
                "response must be one of {}",
                responses.join(", ")
            )));
        }
        d.emit_by_name::<()>("response", &[&r]);
        d.force_close();
        return Ok(json!({"open": false, "responded": r}));
    }
    let mut fields = serde_json::Map::new();
    if let Some(c) = d.extra_child() {
        collect_fields(&c, &mut fields);
    }
    Ok(json!({
        "open": true, "heading": d.heading().map(|h| h.to_string()),
        "body": d.body().to_string(), "responses": responses,
        "fields": fields,
        "labels": responses.iter().map(|r| (r.clone(), json!(d.response_label(r).to_string()))).collect::<serde_json::Map<_, _>>(),
        "enabled": responses.iter().filter(|r| d.is_response_enabled(r)).collect::<Vec<_>>(),
    }))
}

/// The first descendant (or `w`) with CSS class `class`.
fn find_class(w: &gtk::Widget, class: &str) -> Option<gtk::Widget> {
    if w.has_css_class(class) {
        return Some(w.clone());
    }
    let mut c = w.first_child();
    while let Some(x) = c {
        if let Some(f) = find_class(&x, class) {
            return Some(f);
        }
        c = x.next_sibling();
    }
    None
}

/// `rade-field-*` widgets' values: entry text, dropdown item, label text.
fn collect_fields(w: &gtk::Widget, out: &mut serde_json::Map<String, Value>) {
    if let Some(f) = w
        .css_classes()
        .iter()
        .find_map(|c| c.strip_prefix("rade-field-").map(String::from))
    {
        let v = if let Some(e) = w.downcast_ref::<gtk::Entry>() {
            e.text().to_string()
        } else if let Some(dd) = w.downcast_ref::<gtk::DropDown>() {
            dd.selected_item()
                .and_downcast::<gtk::StringObject>()
                .map(|s| s.string().to_string())
                .unwrap_or_default()
        } else if let Some(l) = w.downcast_ref::<gtk::Label>() {
            l.label().to_string()
        } else if let Some(t) = w.downcast_ref::<gtk::TextView>() {
            let b = t.buffer();
            b.text(&b.start_iter(), &b.end_iter(), false).to_string()
        } else {
            String::new()
        };
        out.insert(f, v.into());
    }
    let mut c = w.first_child();
    while let Some(x) = c {
        collect_fields(&x, out);
        c = x.next_sibling();
    }
}

/// `debug.search` (`{"pane", "op": "info"|"open"|"type"|"next"|"prev"|"close",
/// "text"?}`): the Terminal's search bar. `open` runs Ghostty's
/// `start_search` binding (what `Ctrl+Shift+F` is bound to); `type` sets
/// the entry's text as typing does.
pub fn debug_search(p: &Value) -> Result<Value> {
    let app = app();
    let pane = p.get("pane").and_then(Value::as_str).unwrap_or("");
    let (id, t) = crate::handlers::terminal(&app, pane, None)?;
    let bar = t.handle.search().clone();
    match p.get("op").and_then(Value::as_str).unwrap_or("info") {
        "info" => {}
        "open" => {
            t.handle.perform_binding_action("start_search");
        }
        "type" => bar
            .entry
            .set_text(p.get("text").and_then(Value::as_str).unwrap_or("")),
        "next" => bar.step(true),
        "prev" => bar.step(false),
        "close" => bar.close(),
        o => return Err(Error::usage(format!("unknown op '{o}'"))),
    }
    let (open, text, count) = bar.info();
    let focused = bar.entry.has_focus() || bar.entry.focus_child().is_some();
    Ok(json!({"pane": id, "open": open, "text": text, "count": count, "focused": focused}))
}
