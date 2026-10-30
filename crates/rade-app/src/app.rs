//! Application state: the rade-core model plus the GTK views that render it.
//!
//! Every change goes through [`App::update`]: mutate the model, then
//! [`App::sync`] reconciles widgets with it and a debounced save writes
//! `state.json`. Widgets never hold state the model doesn't have.

use crate::panes;
use crate::sidebar::Sidebar;
use crate::tabs::WorktreeView;
use crate::terminal::TerminalWidget;
use crate::viewer::Viewer;
use adw::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::config::Config;
use rade_core::{process, store, FocusAction, PaneKind, State};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

/// Save at most this long after a change (R11.1 asks for ≤ 5 s).
const SAVE_DELAY: Duration = Duration::from_secs(1);

thread_local! {
    static APP: RefCell<Option<Rc<App>>> = const { RefCell::new(None) };
}

/// The app, from GTK callbacks on the main thread.
pub fn try_app() -> Option<Rc<App>> {
    APP.with(|a| a.borrow().clone())
}

pub fn app() -> Rc<App> {
    try_app().expect("app initialized")
}

pub fn install(app: Rc<App>) {
    APP.with(|a| *a.borrow_mut() = Some(app));
}

/// A widget the user types text into (an entry, an editable text view).
fn is_text_entry(w: &gtk::Widget) -> bool {
    w.is::<gtk::Text>()
        || w.downcast_ref::<gtk::TextView>()
            .is_some_and(|t| t.is_editable())
}

/// What a paused Source's Pane shows (R19.3): a note and Resume.
fn paused_placeholder(source: &str) -> gtk::Widget {
    let button = gtk::Button::with_label("Resume");
    button.add_css_class("pill");
    button.add_css_class("rade-resume");
    button.set_halign(gtk::Align::Center);
    let id = source.to_string();
    button.connect_clicked(move |_| {
        if let Some(app) = try_app() {
            if let Err(e) = app.resume_source(&id) {
                crate::worktrees::alert("Could not resume the Source", &e.message);
            }
        }
    });
    let page = adw::StatusPage::builder()
        .icon_name("media-playback-pause-symbolic")
        .title("Paused")
        .description(
            "This Source is paused: nothing runs in it. Resume starts it again \
             (Agents resume their sessions).",
        )
        .child(&button)
        .build();
    page.add_css_class("compact");
    page.add_css_class("rade-paused-pane");
    page.set_hexpand(true);
    page.set_vexpand(true);
    page.upcast()
}

/// The Pane whose Terminal or Viewer contains widget `w`, if any.
pub fn pane_of_widget(app: &App, w: &gtk::Widget) -> Option<String> {
    let inside = |root: &gtk::Widget| w == root || w.is_ancestor(root);
    app.terminals
        .borrow()
        .iter()
        .find(|(_, t)| inside(t.root.upcast_ref()))
        .map(|(id, _)| id.clone())
        .or_else(|| {
            app.viewers
                .borrow()
                .iter()
                .find(|(_, v)| inside(v.root.upcast_ref()))
                .map(|(id, _)| id.clone())
        })
        .or_else(|| {
            app.reviews
                .borrow()
                .iter()
                .find(|(_, v)| inside(v.root.upcast_ref()))
                .map(|(id, _)| id.clone())
        })
        .or_else(|| {
            app.diffs
                .borrow()
                .iter()
                .find(|(_, v)| inside(v.root.upcast_ref()))
                .map(|(id, _)| id.clone())
        })
        .or_else(|| {
            app.browsers
                .borrow()
                .iter()
                .find(|(_, v)| inside(v.root.upcast_ref()))
                .map(|(id, _)| id.clone())
        })
}

pub struct Paths {
    pub state: PathBuf,
    pub config: PathBuf,
    pub socket: PathBuf,
}

pub struct SessionRecovery {
    pub load_error: String,
    pub backup: Option<PathBuf>,
}

pub struct App {
    pub gtk_app: adw::Application,
    pub window: adw::ApplicationWindow,
    /// Header buttons, Right Panel, Status Bar, Toasts (R20).
    pub chrome: crate::chrome::Chrome,
    pub sidebar: Sidebar,
    pub stack: gtk::Stack,
    pub model: RefCell<State>,
    pub config: RefCell<Config>,
    pub paths: Paths,
    views: RefCell<HashMap<String, WorktreeView>>,
    pub terminals: RefCell<HashMap<String, TerminalWidget>>,
    pub viewers: RefCell<HashMap<String, Rc<Viewer>>>,
    /// Review Panes (R13).
    pub reviews: RefCell<HashMap<String, Rc<crate::review_view::ReviewView>>>,
    /// Diff Panes (R122).
    pub diffs: RefCell<HashMap<String, Rc<crate::diff_pane::DiffPane>>>,
    /// Browser Panes (R18).
    pub browsers: RefCell<HashMap<String, Rc<crate::browser_view::BrowserView>>>,
    /// "Paused — Resume" placeholders of a paused Source's Panes (R19.3).
    placeholders: RefCell<HashMap<String, gtk::Widget>>,
    /// Right sidebar file tree (R8) and the split that shows it.
    pub files: crate::files::FileTree,
    pub files_split: adw::OverlaySplitView,
    /// True while [`App::sync`] runs; widget signals it causes are ignored.
    pub syncing: Cell<bool>,
    save_source: RefCell<Option<glib::SourceId>>,
    pub session_recovery: RefCell<Option<SessionRecovery>>,
    save_error: RefCell<Option<String>>,
    pub config_monitor: RefCell<Option<gio::FileMonitor>>,
    pub desktop: crate::notifications::Desktop,
    /// Restored Terminals not started yet, in start order (R130).
    restore: RefCell<rade_core::restore_queue::Queue>,
    /// When the queue started (ms on the monotonic clock) and each Pane's
    /// start time since then, for `debug.restore`.
    restore_log: RefCell<(i64, Vec<(String, u64)>)>,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        gtk_app: adw::Application,
        window: adw::ApplicationWindow,
        chrome: crate::chrome::Chrome,
        sidebar: Sidebar,
        stack: gtk::Stack,
        (files, files_split): (crate::files::FileTree, adw::OverlaySplitView),
        model: State,
        config: Config,
        paths: Paths,
    ) -> Self {
        let desktop = crate::notifications::Desktop::default();
        desktop.start_after(&model);
        Self {
            gtk_app,
            window,
            chrome,
            sidebar,
            stack,
            model: RefCell::new(model),
            config: RefCell::new(config),
            paths,
            views: RefCell::default(),
            terminals: RefCell::default(),
            viewers: RefCell::default(),
            reviews: RefCell::default(),
            diffs: RefCell::default(),
            browsers: RefCell::default(),
            placeholders: RefCell::default(),
            files,
            files_split,
            syncing: Cell::new(false),
            save_source: RefCell::default(),
            session_recovery: RefCell::default(),
            save_error: RefCell::default(),
            config_monitor: RefCell::default(),
            desktop,
            restore: RefCell::default(),
            restore_log: RefCell::default(),
        }
    }

    /// Mutate the model, then re-render, schedule a save, and send or
    /// withdraw desktop notifications.
    pub fn update<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let r = {
            let mut m = self.model.borrow_mut();
            let shown = |m: &State| m.active_tab().map(|t| t.focused.clone());
            let before = shown(&m);
            let r = f(&mut m);
            // A Pane that comes on screen focused (Tab or Source switch,
            // close) is focused: its Notifications are read (R3.4).
            if let Some(p) = shown(&m).filter(|p| Some(p) != before.as_ref()) {
                m.mark_read(&p);
            }
            r
        };
        self.schedule_save();
        self.sync();
        self.desktop.after_update(self);
        self.chrome.after_update(&self.model.borrow());
        crate::activity_view::after_update(&self.model.borrow());
        r
    }

    /// The keyboard went into Pane `id`'s widget (a click, Tab): make it the
    /// focused Pane and re-render when that changes what is lit (focus
    /// ring, dimming, Navigator row, Statusline) or reads Notifications
    /// (R3.4, R103-k); nothing while a sync moves the focus itself.
    pub fn focus_entered(&self, id: &str) {
        if self.syncing.get() {
            return;
        }
        let same = self
            .model
            .borrow()
            .active_tab()
            .is_some_and(|t| t.focused == id);
        if same && self.model.borrow().unread(id) == 0 {
            return;
        }
        self.update(|m| m.focus_pane(id).ok());
    }

    /// Model-only change that needs no re-render (cwd, split ratio).
    pub fn touch<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        let r = f(&mut self.model.borrow_mut());
        self.schedule_save();
        r
    }

    pub fn schedule_save(&self) {
        if self.save_source.borrow().is_some() {
            return;
        }
        let id = glib::timeout_add_local_once(SAVE_DELAY, || {
            if let Some(app) = try_app() {
                app.save_source.borrow_mut().take();
                app.save_now();
            }
        });
        *self.save_source.borrow_mut() = Some(id);
    }

    pub fn save_now(&self) {
        if let Some(id) = self.save_source.borrow_mut().take() {
            id.remove();
        }
        let result = (|| {
            if let Some(recovery) = self.session_recovery.borrow_mut().as_mut() {
                if recovery.backup.is_none() {
                    recovery.backup = Some(store::recovery_backup(&self.paths.state)?);
                }
            }
            store::save(&self.paths.state, &self.model.borrow())
        })();
        *self.save_error.borrow_mut() = result.err().map(|e| {
            eprintln!("rade: saving {}: {e}", self.paths.state.display());
            e.to_string()
        });
        self.show_session_feedback();
    }

    pub fn show_session_feedback(&self) {
        let recovery = self.session_recovery.borrow();
        let message = recovery.as_ref().map(|r| match &r.backup {
            Some(path) => format!("Previous session could not be restored: {}. RADE opened a fresh session; repositories and task files are unchanged. Recovery copy: {}. To restore it, close RADE and replace state.json with a repaired copy (or use a compatible RADE version).", r.load_error, path.display()),
            None => format!("Previous session could not be restored: {}. RADE opened a fresh session in memory. The original state is untouched; saving is blocked until a recovery copy can be made. Fix access to the state folder, then Retry.", r.load_error),
        });
        let save_error = self.save_error.borrow();
        let blocked = recovery.as_ref().is_some_and(|r| r.backup.is_none());
        let error = save_error
            .as_deref()
            .or(blocked.then_some("Recovery backup has not succeeded"));
        let warning = error.map(|e| format!(
            "Session changes are only in memory and may be lost when RADE closes. Could not save {}: {e}. Fix the state folder's permissions or free disk space, then Retry.", self.paths.state.display()
        ));
        self.chrome
            .session_feedback(message.as_deref(), warning.as_deref());
    }

    pub fn show_state_location(&self) {
        let recovery = self.session_recovery.borrow();
        let backup = recovery.as_ref().and_then(|r| r.backup.as_ref());
        let body = format!("State: {}\nRecovery copy: {}\n\nClose RADE before replacing state.json. Repositories are stored separately.",
            self.paths.state.display(), backup.map_or_else(|| "None".into(), |p| p.display().to_string()));
        let dialog = adw::AlertDialog::new(Some("Session state location"), Some(&body));
        dialog.add_response("close", "Close");
        dialog.present(Some(&self.window));
    }

    /// Reconcile all widgets with the model.
    pub fn sync(&self) {
        if self.syncing.replace(true) {
            return;
        }
        let model = self.model.borrow();

        // 1. A Terminal for every Terminal Pane (started even if never
        //    shown, R2.8); a Viewer for every Viewer Pane (R7.2).
        {
            let mut terms = self.terminals.borrow_mut();
            let mut viewers = self.viewers.borrow_mut();
            let mut reviews = self.reviews.borrow_mut();
            let mut diffs = self.diffs.borrow_mut();
            let mut browsers = self.browsers.borrow_mut();
            let mut placeholders = self.placeholders.borrow_mut();
            for (p, w) in model.worktrees() {
                for tab in &w.tabs {
                    for pane in tab.panes() {
                        // A paused Source runs nothing (R19.3).
                        if p.paused {
                            placeholders
                                .entry(pane.id.clone())
                                .or_insert_with(|| paused_placeholder(&p.id));
                            continue;
                        }
                        // Resumed: its widget replaces the placeholder below.
                        if let Some(w) = placeholders.remove(&pane.id) {
                            crate::layout_view::detach(&w);
                        }
                        match &pane.kind {
                            PaneKind::Viewer {
                                path,
                                line,
                                back,
                                forward,
                            } => {
                                let v = viewers
                                    .entry(pane.id.clone())
                                    .or_insert_with(|| Viewer::new(&pane.id));
                                v.show(
                                    path,
                                    *line,
                                    (!back.is_empty(), !forward.is_empty()),
                                    &w.path,
                                );
                                v.set_zoomed(tab.zoomed.as_deref() == Some(&pane.id));
                            }
                            PaneKind::Review { .. } => reviews
                                .entry(pane.id.clone())
                                .or_insert_with(|| crate::review_view::ReviewView::new(&pane.id))
                                .sync(),
                            PaneKind::Diff {
                                root,
                                path,
                                section,
                            } => diffs
                                .entry(pane.id.clone())
                                .or_insert_with(|| crate::diff_pane::DiffPane::new(&pane.id))
                                .show(root, path, *section),
                            // Navigation is the view's own (links, history);
                            // the model only records where it is.
                            PaneKind::Browser { url, annotations } => {
                                browsers
                                    .entry(pane.id.clone())
                                    .or_insert_with(|| {
                                        crate::browser_view::BrowserView::new(&pane.id, url)
                                    })
                                    .show_annotations(annotations);
                            }
                            PaneKind::Terminal { .. } => {
                                terms.entry(pane.id.clone()).or_insert_with(|| {
                                    // A Fan-out Agent runs in its own Worktree (R15.2).
                                    let w = pane
                                        .runs_in
                                        .as_deref()
                                        .and_then(|r| p.worktree(r))
                                        .unwrap_or(w);
                                    let env = self.terminal_env(&model, pane, &p.id, &w.id);
                                    panes::create(pane, p, w, env)
                                });
                            }
                        }
                    }
                }
            }
        }

        // 2. One view per Worktree; each reconciles its Tabs and Layouts.
        let unread = model.unread_counts();
        {
            let mut views = self.views.borrow_mut();
            let roots = self.pane_roots();
            for (p, w) in model.worktrees() {
                let view = views.entry(w.id.clone()).or_insert_with(|| {
                    let v = WorktreeView::new(&w.id);
                    self.stack.add_named(v.root(), Some(&w.id));
                    v
                });
                view.sync(w, &model.fanouts, &roots, &unread, p.paused);
            }
            views.retain(|id, v| {
                let keep = model.worktrees().any(|(_, w)| &w.id == id);
                if !keep {
                    // Its Terminals are released in step 3.
                    self.stack.remove(v.root());
                }
                keep
            });
        }

        // 3. Stop Terminals whose Pane is gone or paused (after layouts
        //    released them).
        let live: Vec<String> = model
            .pane_infos()
            .into_iter()
            .filter(|i| !model.is_paused(&i.source))
            .map(|i| i.pane)
            .collect();
        self.placeholders.borrow_mut().retain(|id, w| {
            let keep = model.locate(id).is_some_and(|l| model.is_paused(&l.source));
            if !keep {
                crate::layout_view::detach(w);
            }
            keep
        });
        let dead: Vec<(String, TerminalWidget)> = {
            let mut terms = self.terminals.borrow_mut();
            let ids: Vec<String> = terms
                .keys()
                .filter(|id| !live.contains(id))
                .cloned()
                .collect();
            ids.into_iter()
                .filter_map(|id| terms.remove(&id).map(|t| (id, t)))
                .collect()
        };
        for (id, t) in dead {
            crate::layout_view::detach(&t.root);
            panes::retire(&id, t);
            // R124: a Voice Composer on a Pane that is gone ends (not inside
            // the sync: closing it moves focus).
            glib::idle_add_local_once(move || crate::voice::pane_closed(&id));
        }
        self.viewers.borrow_mut().retain(|id, v| {
            let keep = live.contains(id);
            if !keep {
                crate::layout_view::detach(v.root.upcast_ref());
            }
            keep
        });
        self.reviews.borrow_mut().retain(|id, v| {
            let keep = live.contains(id);
            if !keep {
                crate::layout_view::detach(v.root.upcast_ref());
            }
            keep
        });
        self.diffs.borrow_mut().retain(|id, v| {
            let keep = live.contains(id);
            if !keep {
                crate::layout_view::detach(v.root.upcast_ref());
            }
            keep
        });
        self.browsers.borrow_mut().retain(|id, v| {
            let keep = live.contains(id);
            if !keep {
                crate::layout_view::detach(v.root.upcast_ref());
            }
            keep
        });

        // 4. Show the active Worktree, sidebar, title, focus.
        let (_, worktree) = model.active();
        // Project home (R105) until the focus moves to a checkout.
        if crate::usage_view::is_shown(&model) {
            self.stack.set_visible_child_name(crate::usage_view::PAGE);
        } else if crate::home_view::shown(&model).is_some() {
            self.stack.set_visible_child_name(crate::home_view::PAGE);
        } else {
            self.stack.set_visible_child_name(&worktree.id);
        }
        crate::navdata::focus_moved(&model);
        self.sidebar.sync(&model, &unread);
        self.chrome.sync(self, &model);
        // The command palette (entry, form fields, drop-downs) keeps it.
        // So do menus (a popover has the keyboard: R19.1, R19.6).
        let typing = crate::palette::is_open()
            || gtk::prelude::GtkWindowExt::focus(&self.window).is_some_and(|w| {
                is_text_entry(&w)
                    || w.ancestor(gtk::Popover::static_type()).is_some()
                    || crate::voice_view::holds(&w)
            });
        self.focus_pane_widget(&model, typing);
        crate::layout_mode::show_inspector(
            &self.files_split,
            self.window.upcast_ref(),
            model.file_tree,
        );
        self.files.set_root(&worktree.path, model.file_tree);
        if let Some(c) = crate::changes::get() {
            c.follow(&worktree.path, model.file_tree);
            c.follow_project(&model);
        }
        if let Some(s) = crate::search_view::get() {
            s.follow(&worktree.path);
            s.follow_scope(&model);
        }
        drop(model);
        self.syncing.set(false);
        crate::home_view::refresh();

        // Realize Terminals in hidden Tabs so their processes start now;
        // restored ones wait for their turn in the queue (R130).
        glib::idle_add_local_once(|| {
            if let Some(app) = try_app() {
                let queue = app.restore.borrow();
                for (id, t) in app.terminals.borrow().iter() {
                    if !queue.contains(id) {
                        t.handle.ensure_started();
                    }
                }
            }
        });
    }

    /// Queue every restored Terminal (R130): call before the first sync,
    /// then [`Self::start_restore`] once the window is shown.
    pub fn plan_restore(&self) {
        let order = rade_core::restore_queue::order(&self.model.borrow());
        *self.restore.borrow_mut() = rade_core::restore_queue::Queue::new(order);
    }

    /// Start the queued Terminals one per main-loop turn at low priority,
    /// [`rade_core::restore_queue::AGENT_GAP_MS`] after each Agent (R130.3).
    pub fn start_restore(&self) {
        self.restore_log.borrow_mut().0 = glib::monotonic_time();
        fn pump() -> glib::ControlFlow {
            let Some(app) = try_app() else {
                return glib::ControlFlow::Break;
            };
            let now = ((glib::monotonic_time() - app.restore_log.borrow().0) / 1000) as u64;
            let step = {
                let model = app.model.borrow();
                let terms = app.terminals.borrow();
                let mut q = app.restore.borrow_mut();
                // Shown (GTK realized it), started by a socket call, or
                // closed: no longer waiting, and no gap for it.
                let gone: Vec<String> = q
                    .pending()
                    .filter(|p| terms.get(*p).is_none_or(|t| t.handle.is_started()))
                    .map(String::from)
                    .collect();
                gone.iter().for_each(|p| {
                    q.promote(p);
                });
                q.next(now, |p| model.pane(p).is_some_and(|p| p.agent.is_some()))
            };
            use rade_core::restore_queue::Step;
            match step {
                Step::Start(id) => {
                    if let Some(t) = app.terminals.borrow().get(&id) {
                        t.handle.ensure_started();
                    }
                    app.restore_log.borrow_mut().1.push((id, now));
                    glib::idle_add_local_full(glib::Priority::LOW, pump);
                }
                Step::Wait(ms) => {
                    glib::timeout_add_local_full(
                        Duration::from_millis(ms),
                        glib::Priority::LOW,
                        pump,
                    );
                }
                Step::Done => {}
            }
            glib::ControlFlow::Break
        }
        glib::idle_add_local_full(glib::Priority::LOW, pump);
    }

    /// Pane `id` is needed now (R130.4: a socket call targets it): take it
    /// out of the restore queue and start its Terminal.
    pub fn start_now(&self, id: &str) {
        if self.restore.borrow_mut().promote(id) {
            if let Some(t) = self.terminals.borrow().get(id) {
                t.handle.ensure_started();
            }
            let now = ((glib::monotonic_time() - self.restore_log.borrow().0) / 1000) as u64;
            self.restore_log.borrow_mut().1.push((id.to_string(), now));
        }
    }

    /// `debug.restore` (test mode): the Panes still queued and when each
    /// queued Pane started (ms after the queue began).
    pub fn restore_debug(&self) -> serde_json::Value {
        let log = self.restore_log.borrow();
        let since = (glib::monotonic_time() - log.0) / 1000;
        serde_json::json!({
            "since_ms": since,
            "pending": self.restore.borrow().pending().collect::<Vec<_>>(),
            "started": log.1.iter().map(|(p, t)| serde_json::json!({"pane": p, "ms": t})).collect::<Vec<_>>(),
        })
    }

    /// Keyboard focus back to the focused Pane of the Tab on screen, e.g.
    /// when the command palette closes (R14.1).
    pub fn restore_focus(&self) {
        // Like sync: the model already has this focus; its focus-in handler
        // must not run while the model is borrowed.
        let was = self.syncing.replace(true);
        self.focus_pane_widget(&self.model.borrow(), false);
        self.syncing.set(was);
    }

    /// Give keyboard focus to the model's focused Pane (see
    /// [`FocusAction::decide`]), unless `typing` in a text entry.
    fn focus_pane_widget(&self, model: &State, typing: bool) {
        let focused = model
            .active_tab()
            .and_then(|t| Some((t.focused.clone(), t.pane(&t.focused)?)));
        match (
            FocusAction::decide(focused.as_ref().map(|f| f.1), typing),
            &focused,
        ) {
            // Unconditionally: see FocusAction::decide (A2 live bug).
            (FocusAction::Terminal, Some((id, _))) => {
                if let Some(t) = self.terminals.borrow().get(id) {
                    t.handle.gl_area().grab_focus();
                }
            }
            (FocusAction::Viewer, Some((id, _))) => {
                if let Some(v) = self.viewers.borrow().get(id) {
                    v.focus();
                }
            }
            (FocusAction::Review, Some((id, _))) => {
                if let Some(v) = self.reviews.borrow().get(id) {
                    v.focus();
                }
            }
            (FocusAction::Diff, Some((id, _))) => {
                if let Some(v) = self.diffs.borrow().get(id) {
                    v.focus();
                }
            }
            (FocusAction::Browser, Some((id, _))) => {
                if let Some(v) = self.browsers.borrow().get(id) {
                    v.focus();
                }
            }
            _ => {}
        }
    }

    /// Every Pane's widget (Terminal or Viewer), for the Layout views.
    fn pane_roots(&self) -> HashMap<String, gtk::Widget> {
        let mut roots: HashMap<String, gtk::Widget> = self
            .terminals
            .borrow()
            .iter()
            .map(|(id, t)| (id.clone(), t.root.clone().upcast()))
            .collect();
        for (id, v) in self.viewers.borrow().iter() {
            roots.insert(id.clone(), v.root.clone().upcast());
        }
        for (id, v) in self.reviews.borrow().iter() {
            roots.insert(id.clone(), v.root.clone().upcast());
        }
        for (id, v) in self.diffs.borrow().iter() {
            roots.insert(id.clone(), v.root.clone().upcast());
        }
        for (id, v) in self.browsers.borrow().iter() {
            roots.insert(id.clone(), v.root.clone().upcast());
        }
        for (id, w) in self.placeholders.borrow().iter() {
            roots.insert(id.clone(), w.clone());
        }
        roots
    }

    /// Explicit close (`rade close`, the close key): the Pane leaves the model
    /// and `state.json` now, before its process is asked to stop, so it can
    /// never come back on restore (R11). `force` = SIGKILL instead of waiting
    /// for the process to exit after SIGHUP.
    pub fn close_pane(&self, id: &str, force: bool) -> rade_core::rpc::Result<()> {
        // Before the SIGHUP: once the group leader is gone it is no longer
        // our child, but the rest of its group may live on.
        let groups = if force {
            process::pane_groups(id)
        } else {
            vec![]
        };
        self.update(|m| m.close_pane(id))?;
        self.save_now();
        for g in groups {
            process::signal_group(g, process::SIGKILL);
        }
        Ok(())
    }

    /// Close Tabs (R19.1: `rade tab close`, the Tab menu) like closing each
    /// Pane: gone from `state.json` first, then SIGHUP (`force`: SIGKILL).
    /// Returns the Panes closed.
    pub fn close_tabs(&self, tabs: &[String], force: bool) -> rade_core::rpc::Result<Vec<String>> {
        let groups: Vec<i32> = if force {
            let m = self.model.borrow();
            tabs.iter()
                .filter_map(|t| m.worktrees().find_map(|(_, w)| w.tab(t)))
                .flat_map(|t| t.pane_ids())
                .flat_map(|p| process::pane_groups(&p))
                .collect()
        } else {
            vec![]
        };
        let panes = self.touch(|m| -> rade_core::rpc::Result<Vec<String>> {
            let mut all = Vec::new();
            for t in tabs {
                all.extend(m.close_tab(t)?);
            }
            Ok(all)
        });
        // state.json first, then the sync that stops their processes.
        self.save_now();
        self.update(|_| ());
        let panes = panes?;
        for g in groups {
            process::signal_group(g, process::SIGKILL);
        }
        Ok(panes)
    }

    /// The layered config (R29.2) for a Source / Worktree: user file, its
    /// Workspace's, its Project's and its trusted `.rade.toml`. An unknown
    /// Source gets the user file's.
    pub fn config_for(&self, source: &str, worktree: Option<&str>) -> Config {
        let ctx = self
            .model
            .borrow()
            .config_context(Some(source), worktree, None, None);
        match ctx {
            Ok(c) => rade_core::layers::effective(&rade_core::layers::load(&self.paths.config, &c)),
            Err(_) => self.config.borrow().clone(),
        }
    }

    /// A new Terminal's env: its Workspace's [`workspace_env`], then its
    /// Agent Account's variable and `RADE_ACCOUNT`, which win over `[env]`
    /// (R108.2, R108.7). An account no longer declared adds nothing (the
    /// spawn or restore that made the Pane already said why).
    pub(crate) fn terminal_env(
        &self,
        model: &State,
        pane: &rade_core::Pane,
        source: &str,
        worktree: &str,
    ) -> Vec<(String, String)> {
        let mut env = workspace_env(model, source);
        if pane.account.is_none() {
            return env;
        }
        let kind = pane.agent.as_deref().and_then(|a| {
            let presets = self.config_for(source, Some(worktree)).presets().ok()?;
            rade_core::account::AgentKind::of_hooks(presets.get(a)?.hooks)
        });
        let home = rade_core::paths::home_dir(&rade_core::paths::process_env);
        match model.pane_account(&pane.id, kind, &home) {
            Some(Ok(a)) => {
                // R125.2: the spawn / restore already refused an unreadable
                // env file; one gone since is reported, the login still set.
                if let Err(e) = rade_core::account::apply_env(&mut env, &a) {
                    eprintln!("rade: pane {}: {}", pane.id, e.message);
                }
            }
            Some(Err(e)) => eprintln!("rade: pane {}: {}", pane.id, e.message),
            None => {}
        }
        env
    }

    /// Pause a Source (R19.3): the model keeps its Panes; the next sync
    /// retires their Terminals (SIGHUP, SIGKILL after 12 s) and shows
    /// placeholders. Saved at once, so a crash keeps it paused.
    pub fn pause_source(&self, source: &str, force: bool) -> rade_core::rpc::Result<String> {
        let by_worktree = {
            let m = self.model.borrow();
            let id = m.find_source(source)?.id.clone();
            m.presets_by_worktree(&self.paths.config, Some(&id))
        };
        let presets = |s: &str, w: &str| {
            by_worktree
                .get(&(s.to_string(), w.to_string()))
                .cloned()
                .unwrap_or_default()
        };
        // state.json first, then the sync that stops its processes.
        let home = rade_core::paths::home_dir(&rade_core::paths::process_env);
        let id = self.touch(|m| {
            let id = m.pause_source(source, &presets, force, crate::panes::now())?;
            // R108.7: the same refusal as Session Restore.
            m.refuse_restores_in(&presets, &home, Some(&id));
            Ok::<_, rade_core::rpc::Error>(id)
        })?;
        self.save_now();
        self.update(|_| ());
        Ok(id)
    }

    /// Resume (R19.3). A process still stopping from the pause (within its
    /// SIGHUP grace) is killed first: its restart resumes the same session,
    /// and two Agents on one session would fork it.
    pub fn resume_source(&self, source: &str) -> rade_core::rpc::Result<String> {
        let panes: Vec<String> = {
            let m = self.model.borrow();
            let id = m.find_source(source)?.id.clone();
            m.pane_infos()
                .into_iter()
                .filter(|i| i.source == id)
                .map(|i| i.pane)
                .collect()
        };
        for p in &panes {
            for g in process::pane_groups(p) {
                process::signal_group(g, process::SIGKILL);
            }
        }
        let id = self.update(|m| m.resume_source(source))?;
        self.save_now();
        Ok(id)
    }

    /// Whether `pane` is the focused Pane of the Tab on screen.
    pub fn is_focused_visible(&self, pane: &str) -> bool {
        self.model
            .borrow()
            .active_tab()
            .is_some_and(|t| t.focused == pane)
    }

    /// Show a Source; opens a first Tab if its Worktree has none.
    pub fn activate_source(&self, id: &str) {
        self.update(|m| {
            if m.select_source(id).is_err() {
                return;
            }
            let (p, w) = m.active();
            if w.tabs.is_empty() {
                let (p, w) = (p.id.clone(), w.id.clone());
                let _ = m.new_tab(&p, &w, None, None);
            }
        });
    }

    /// Show a Workspace (R23.5): its last Source, Worktree, Tab, focus.
    pub fn switch_workspace(&self, id: &str) {
        self.update(|m| {
            if m.switch_workspace(id).is_err() {
                return;
            }
            let (p, w) = m.active();
            if w.tabs.is_empty() {
                let (p, w) = (p.id.clone(), w.id.clone());
                let _ = m.new_tab(&p, &w, None, None);
            }
        });
    }

    pub fn quit(&self) {
        crate::voice::shutdown();
        self.save_now();
        self.gtk_app.quit();
    }
}

/// A Source's Workspace `[env]` + `RADE_WORKSPACE` for a new Terminal or
/// one of the app's own `git` / `gh` / Draft commands (R23.3). A broken
/// `workspace.toml` is reported and left out.
pub fn workspace_env(model: &State, source: &str) -> Vec<(String, String)> {
    let home = rade_core::paths::home_dir(&rade_core::paths::process_env);
    model.env_for(source, &home).unwrap_or_else(|e| {
        eprintln!("rade: workspace env: {e}");
        model
            .source_workspace(source)
            .map(|w| vec![("RADE_WORKSPACE".into(), w.id.clone())])
            .unwrap_or_default()
    })
}

/// [`workspace_env`] of the Source a Worktree folder belongs to.
pub fn workspace_env_at(model: &State, root: &std::path::Path) -> Vec<(String, String)> {
    model
        .worktrees()
        .find(|(_, w)| w.path == root)
        .map(|(s, _)| workspace_env(model, &s.id))
        .unwrap_or_default()
}
