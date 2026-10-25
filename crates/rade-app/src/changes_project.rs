//! Changes by Project (PRD R107.1, R100.3, R107.5; spec A.58–A.69): when
//! the focused checkout belongs to a Project, the Inspector's Changes page
//! shows that Project — the header `PROJECT · SHOP +a −r`, one card per
//! Source link (its Worktree in focus, else the link's checkout) with its
//! files, its own message box, Draft and Commit, the drift card of a
//! drifted link (its buttons from `drift()`) and clean links folded to
//! their header. A file row's click stages / unstages it; Commit commits
//! that checkout's index only. The single-checkout page (per-hunk, Push,
//! Pull, Create PR) stays one click away (`Details`) and is what a
//! checkout outside every Project shows. git runs off the GTK thread.

use crate::app::try_app;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use rade_core::home::{self, FileRow, Group};
use rade_core::linkview::LinkView;
use rade_core::rpc::{Error, Result};
use rade_core::scm::{self, DraftKind};
use serde::Serialize;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// What one group's checkout holds (read off the GTK thread).
#[derive(Clone, Debug, Default, PartialEq)]
struct Read {
    rows: Vec<FileRow>,
    ahead: usize,
    error: Option<String>,
    /// This checkout's identity warning (R107.5): its `user.email`
    /// against the Workspace's.
    identity: Option<String>,
}

/// Everything the page shows; rebuilt when it changes.
#[derive(Clone, Debug, PartialEq, Serialize)]
struct View {
    project: String,
    title: String,
    add: String,
    del: String,
    groups: Vec<GroupView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct GroupView {
    source: String,
    name: String,
    label: String,
    meta: String,
    path: PathBuf,
    files: Vec<FileRow>,
    clean: bool,
    staged: usize,
    primary: bool,
    error: Option<String>,
    drift: Option<home::DriftCard>,
    /// The identity warning of this group's checkout (R107.5): each
    /// Commit commits as its own checkout's `user.email`.
    identity: Option<String>,
    project: String,
}

pub struct ProjectChanges {
    pub root: gtk::ScrolledWindow,
    body: gtk::Box,
    /// The Project shown and its groups (from the last sync).
    shown: RefCell<Option<(String, String, Vec<Group>)>>,
    reads: RefCell<HashMap<PathBuf, Read>>,
    rendered: RefCell<Option<View>>,
    /// The message boxes' texts per checkout (kept across rebuilds).
    messages: RefCell<HashMap<PathBuf, String>>,
    commits: RefCell<HashMap<PathBuf, gtk::Button>>,
    busy: Cell<bool>,
    again: Cell<bool>,
}

thread_local! {
    static PC: RefCell<Option<Rc<ProjectChanges>>> = const { RefCell::new(None) };
}

pub fn get() -> Option<Rc<ProjectChanges>> {
    PC.with(|p| p.borrow().clone())
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

/// Build the page (the Changes stack adds it as `project`).
pub fn install() -> gtk::ScrolledWindow {
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.add_css_class("changes-project");
    let root = gtk::ScrolledWindow::new();
    root.set_hscrollbar_policy(gtk::PolicyType::Never);
    root.set_vexpand(true);
    root.set_child(Some(&body));
    let pc = Rc::new(ProjectChanges {
        root: root.clone(),
        body,
        shown: RefCell::default(),
        reads: RefCell::default(),
        rendered: RefCell::default(),
        messages: RefCell::default(),
        commits: RefCell::default(),
        busy: Cell::new(false),
        again: Cell::new(false),
    });
    PC.with(|p| *p.borrow_mut() = Some(pc));
    // While shown and the window active: the checkouts may change.
    glib::timeout_add_local(std::time::Duration::from_secs(3), || {
        if let (Some(pc), Some(app)) = (get(), try_app()) {
            if pc.root.is_mapped() && app.window.is_active() {
                pc.refresh();
            }
        }
        glib::ControlFlow::Continue
    });
    root
}

/// The Project of the focused checkout and its Changes groups, if it is
/// in one (a linked Worktree's recorded Project, else the first Project
/// whose link uses this checkout).
pub fn focused(ws: &rade_core::State) -> Option<(String, String, Vec<Group>)> {
    let links: Vec<LinkView> = crate::navdata::links(ws);
    let projects = crate::navdata::projects(ws);
    let (s, w) = ws.active();
    if s.is_general() {
        return None;
    }
    let canon = crate::projects::canon;
    let here = canon(&w.path);
    let project = if ws.is_main_worktree(&s.id, &w.id) {
        projects
            .iter()
            .find(|p| {
                links.iter().any(|l| {
                    l.project == p.id
                        && l.source == s.id
                        && l.path.as_deref().is_some_and(|x| canon(x) == here)
                })
            })
            .or_else(|| {
                // A Terminal on a link's Project checkout lives in the
                // main Worktree: the link whose checkout holds its cwd.
                let cwd = ws
                    .active_tab()
                    .and_then(|t| t.pane(&t.focused))
                    .and_then(|p| match &p.kind {
                        rade_core::PaneKind::Terminal { cwd, .. } => Some(canon(cwd)),
                        _ => None,
                    });
                projects.iter().find(|p| {
                    links.iter().any(|l| {
                        l.project == p.id
                            && l.source == s.id
                            && l.path
                                .as_deref()
                                .zip(cwd.as_deref())
                                .is_some_and(|(x, c)| c.starts_with(canon(x)))
                    })
                })
            })
    } else {
        w.project
            .as_deref()
            .and_then(|id| projects.iter().find(|p| p.id == id))
            .filter(|p| links.iter().any(|l| l.project == p.id && l.source == s.id))
    }?;
    let groups = home::change_groups(ws, &project.id, &links, Some((&s.id, &w.id)));
    Some((project.id.clone(), project.name.clone(), groups))
}

impl ProjectChanges {
    /// Every sync: which Project is in focus (`None` = none, the
    /// single-checkout page shows). Returns whether a Project is shown.
    pub fn follow(self: &Rc<Self>, ws: &rade_core::State) -> bool {
        let now = focused(ws);
        let changed = *self.shown.borrow() != now;
        let on = now.is_some();
        if changed {
            *self.shown.borrow_mut() = now;
            self.refresh();
            self.render();
        }
        on
    }

    /// Read each group's checkout again (one read at a time).
    pub fn refresh(self: &Rc<Self>) {
        let Some((_, _, groups)) = self.shown.borrow().clone() else {
            return;
        };
        if self.busy.replace(true) {
            self.again.set(true);
            return;
        }
        // Each checkout's `user.email` is read under its Source's
        // Workspace env (R23.3), as its Commit runs.
        let (email, envs) = match try_app() {
            Some(app) => {
                let m = app.model.borrow();
                let email = m
                    .workspace(m.active_workspace())
                    .and_then(|w| rade_core::workspace::WorkspaceFile::read(&w.root).ok())
                    .and_then(|f| f.identity.git_email);
                let envs: Vec<_> = groups
                    .iter()
                    .map(|g| crate::app::workspace_env(&m, &g.source))
                    .collect();
                (email, envs)
            }
            None => (None, vec![Vec::new(); groups.len()]),
        };
        let w = Rc::downgrade(self);
        crate::worktrees::in_background(
            move || {
                groups
                    .iter()
                    .zip(envs)
                    .map(|(g, env)| {
                        let mut r = read(&g.path, &g.base);
                        r.identity = rade_core::chrome::identity_warning(
                            email.as_deref(),
                            crate::titlebar::read_email(&g.path, env).as_deref(),
                        );
                        (g.path.clone(), r)
                    })
                    .collect::<HashMap<_, _>>()
            },
            move |reads| {
                let Some(pc) = w.upgrade() else { return };
                pc.busy.set(false);
                if *pc.reads.borrow() != reads {
                    *pc.reads.borrow_mut() = reads;
                    pc.render();
                }
                if pc.again.take() {
                    pc.refresh();
                }
            },
        );
    }

    fn view(&self) -> Option<View> {
        let (project, name, groups) = self.shown.borrow().clone()?;
        let app = try_app()?;
        let m = app.model.borrow();
        let links = crate::navdata::links(&m);
        let reads = self.reads.borrow();
        let (fs, fw) = {
            let (s, w) = m.active();
            (s.id.clone(), w.path.clone())
        };
        let mut gv: Vec<GroupView> = groups
            .iter()
            .map(|g| {
                let r = reads.get(&g.path).cloned().unwrap_or_default();
                let link = links
                    .iter()
                    .find(|l| l.project == project && l.source == g.source);
                let drift = link.and_then(|l| {
                    l.drift
                        .as_ref()
                        .filter(|_| g.worktree.is_none())
                        .map(|d| home::drift_card(&l.source, &l.project_name, d))
                });
                let src_name = m
                    .source(&g.source)
                    .map_or(g.source.clone(), |s| s.name.clone());
                GroupView {
                    source: g.source.clone(),
                    name: src_name,
                    label: g.label.clone(),
                    meta: home::group_meta(r.ahead, r.rows.len()),
                    path: g.path.clone(),
                    clean: r.rows.is_empty() && r.error.is_none(),
                    staged: r.rows.iter().filter(|f| f.staged).count(),
                    files: r.rows,
                    primary: false,
                    error: r.error,
                    drift,
                    identity: r.identity,
                    project: project.clone(),
                }
            })
            .collect();
        // One primary Commit (A.65): the focused group's while it has
        // changes (none on a clean checkout, M35); none while a dialog or
        // the palette is open.
        let modal = crate::palette::is_open()
            || crate::project_dialogs::is_open()
            || crate::voice_view::is_open();
        if !modal {
            let at = gv
                .iter()
                .position(|g| !g.clean && g.source == fs && rade_core::git::same(&g.path, &fw));
            if let Some(i) = at {
                gv[i].primary = true;
            }
        }
        let (a, d) = home::totals(gv.iter().flat_map(|g| g.files.iter()));
        let (title, add, del) = home::changes_header(&name, a, d);
        Some(View {
            project,
            title,
            add,
            del,
            groups: gv,
        })
    }

    /// The palette or a dialog opened / closed: the primary Commit moves.
    pub fn repaint(self: &Rc<Self>) {
        self.rendered.borrow_mut().take();
        self.render();
    }

    /// Rebuild the page when what it shows changed.
    pub fn render(self: &Rc<Self>) {
        let Some(v) = self.view() else {
            *self.rendered.borrow_mut() = None;
            return;
        };
        if let Some(app) = try_app() {
            let n: usize = v.groups.iter().map(|g| g.files.len()).sum();
            if crate::changes::get().is_some_and(|c| c.project_mode()) {
                app.chrome.right.set_count("changes", n);
            }
        }
        if self.rendered.borrow().as_ref() == Some(&v) {
            return;
        }
        while let Some(c) = self.body.first_child() {
            self.body.remove(&c);
        }
        self.commits.borrow_mut().clear();
        // Header (A.58).
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        head.add_css_class("changes-header");
        let t = label(&v.title, &["section-label", "changes-title"]);
        t.set_hexpand(true);
        head.append(&t);
        head.append(&label(&v.add, &["changes-total-add"]));
        head.append(&label(&v.del, &["changes-total-del"]));
        let details = gtk::Button::from_icon_name("ph-git-diff-symbolic");
        details.add_css_class("flat");
        details.add_css_class("changes-details");
        details.set_tooltip_text(Some(
            "Details of the focused checkout: hunks, Push, Pull, PR",
        ));
        details.connect_clicked(|_| {
            if let Some(c) = crate::changes::get() {
                c.set_detail(true);
            }
        });
        head.append(&details);
        self.body.append(&head);
        if v.groups.iter().all(|g| g.clean) {
            self.body.append(&crate::chrome::empty_state(
                &rade_core::chrome::EMPTY_PROJECT_CHANGES,
            ));
        }
        for g in &v.groups {
            self.body.append(&self.group(g));
            if let Some(d) = &g.drift {
                self.body.append(&drift_card(&g.project, &g.source, d));
            }
        }
        *self.rendered.borrow_mut() = Some(v);
    }

    fn group(self: &Rc<Self>, g: &GroupView) -> gtk::Widget {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
        card.add_css_class("changes-group");
        if g.clean {
            // Folded to its header at 0.75 (A.69).
            card.add_css_class("clean");
            card.set_opacity(0.75);
        }
        card.set_widget_name(&format!("changes-group-{}", g.source));
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        head.add_css_class("changes-group-header");
        head.append(&label(&g.name, &["changes-group-title"]));
        let l = label(&g.label, &["changes-group-label"]);
        l.set_hexpand(true);
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        head.append(&l);
        head.append(&label(&g.meta, &["changes-group-meta"]));
        card.append(&head);
        if let Some(e) = &g.error {
            let l = label(e, &["changes-empty"]);
            l.set_wrap(true);
            card.append(&l);
        }
        if g.clean {
            return card.upcast();
        }
        for f in &g.files {
            card.append(&file_row(&g.path, f));
        }
        // The identity warning of this checkout, above its own Commit.
        if let Some(w) = &g.identity {
            let l = label(w, &["changes-identity"]);
            l.set_wrap(true);
            card.append(&l);
        }
        // Its own commit box (A.64–A.66).
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.add_css_class("commit-row");
        let entry = gtk::Entry::new();
        entry.add_css_class("commit-entry");
        entry.set_placeholder_text(Some("Message (Ctrl Enter)"));
        entry.set_hexpand(true);
        let text = self
            .messages
            .borrow()
            .get(&g.path)
            .cloned()
            .unwrap_or_default();
        entry.set_text(&text);
        row.append(&entry);
        let draft = gtk::Button::from_icon_name("ph-sparkle-symbolic");
        draft.add_css_class("draft-button");
        draft.set_tooltip_text(Some("Draft with agent"));
        row.append(&draft);
        let commit = gtk::Button::with_label("Commit");
        commit.add_css_class("commit-button");
        if g.primary {
            commit.add_css_class("suggested-action");
        }
        commit.set_sensitive(g.staged > 0 && !text.trim().is_empty());
        commit.set_tooltip_text(Some("Commit this checkout's staged changes (Ctrl+Enter)"));
        row.append(&commit);
        card.append(&row);
        self.commits
            .borrow_mut()
            .insert(g.path.clone(), commit.clone());
        let (w, path, staged) = (Rc::downgrade(self), g.path.clone(), g.staged);
        let c2 = commit.clone();
        entry.connect_changed(move |e| {
            if let Some(pc) = w.upgrade() {
                pc.messages
                    .borrow_mut()
                    .insert(path.clone(), e.text().to_string());
                c2.set_sensitive(staged > 0 && !e.text().trim().is_empty());
            }
        });
        let (w, path) = (Rc::downgrade(self), g.path.clone());
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(move |_, k, _, st| {
            let enter = matches!(k, gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter);
            if enter && st.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
                if let Some(pc) = w.upgrade() {
                    pc.commit(&path);
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        entry.add_controller(keys);
        let (w, path) = (Rc::downgrade(self), g.path.clone());
        commit.connect_clicked(move |_| {
            if let Some(pc) = w.upgrade() {
                pc.commit(&path);
            }
        });
        let (w, path, e2) = (Rc::downgrade(self), g.path.clone(), entry.clone());
        draft.connect_clicked(move |b| {
            if let Some(pc) = w.upgrade() {
                pc.draft(&path, &e2, b);
            }
        });
        card.upcast()
    }

    /// Commit the checkout at `path` (its index only) with its message.
    fn commit(self: &Rc<Self>, path: &Path) {
        let msg = self
            .messages
            .borrow()
            .get(path)
            .cloned()
            .unwrap_or_default();
        let enabled = self
            .commits
            .borrow()
            .get(path)
            .is_some_and(|b| b.is_sensitive());
        if !enabled || msg.trim().is_empty() {
            return;
        }
        let (w, p) = (Rc::downgrade(self), path.to_path_buf());
        crate::worktrees::in_background(
            move || scm::commit(&p, &msg).map(|d| (p, d)),
            move |r| {
                let Some(pc) = w.upgrade() else { return };
                match r {
                    Ok((p, done)) => {
                        pc.messages.borrow_mut().remove(&p);
                        let (short, subject) = done.split_once(' ').unwrap_or((&done, ""));
                        let source = try_app()
                            .and_then(|app| {
                                app.model
                                    .borrow()
                                    .worktrees()
                                    .find(|(_, w)| p.starts_with(&w.path))
                                    .map(|(s, _)| s.name.clone())
                            })
                            .unwrap_or_default();
                        crate::activity_view::committed(&p, &source, short, subject);
                        if let Some(app) = try_app() {
                            app.chrome.toast(
                                &format!("Committed <b>{}</b>", glib::markup_escape_text(&done)),
                                None,
                            );
                        }
                        pc.rendered.borrow_mut().take();
                        pc.refresh();
                    }
                    Err(e) => crate::worktrees::alert("Commit", &e.message),
                }
            },
        );
    }

    /// Draft with agent: the checkout's `[scm] draft_command` on its
    /// staged diff fills its message box.
    fn draft(self: &Rc<Self>, root: &Path, entry: &gtk::Entry, b: &gtk::Button) {
        let Some(app) = try_app() else { return };
        let at = app
            .model
            .borrow()
            .worktrees()
            .find(|(_, w)| root.starts_with(&w.path))
            .map(|(s, w)| (s.id.clone(), w.id.clone()));
        let command = match &at {
            Some((s, w)) => app.config_for(s, Some(w)).scm.draft_command,
            None => app.config.borrow().scm.draft_command.clone(),
        };
        let base = app.model.borrow().comparison_base(root);
        let env = crate::app::workspace_env_at(&app.model.borrow(), root);
        let root = root.to_path_buf();
        let (w, e, b) = (Rc::downgrade(self), entry.clone(), b.clone());
        b.set_sensitive(false);
        crate::worktrees::in_background(
            move || {
                let cancel = Arc::new(AtomicBool::new(false));
                let base = base.or_else(|| rade_core::git::default_branch(&root));
                let input = scm::gather_draft(&root, DraftKind::Commit, base.as_deref())?;
                let prompt = scm::draft_prompt(DraftKind::Commit, &input);
                scm::run_draft(&command, &root, &prompt, scm::DRAFT_TIMEOUT, &cancel, &env)
                    .map(|out| scm::parse_draft(DraftKind::Commit, &out))
            },
            move |r: Result<(String, String)>| {
                b.set_sensitive(true);
                let Some(_pc) = w.upgrade() else { return };
                match r {
                    Ok((subject, body)) => {
                        let text = if body.is_empty() {
                            subject
                        } else {
                            format!("{subject}\n\n{body}")
                        };
                        e.set_text(&text);
                    }
                    Err(err) => crate::worktrees::alert("Draft with agent", &err.message),
                }
            },
        );
    }

    /// `debug.ui changes`.
    pub fn info(&self) -> Value {
        let v = self.view();
        let mode = crate::changes::get().map(|c| c.mode()).unwrap_or("none");
        match v {
            None => json!({"mode": mode}),
            Some(v) => {
                let groups: Vec<Value> = v
                    .groups
                    .iter()
                    .map(|g| {
                        let enabled = self
                            .commits
                            .borrow()
                            .get(&g.path)
                            .is_some_and(|b| b.is_sensitive());
                        json!({
                            "source": g.source, "name": g.name, "label": g.label, "meta": g.meta,
                            "path": g.path, "clean": g.clean, "rows": g.files.len(),
                            "files": g.files, "staged": g.staged, "error": g.error,
                            "commit": {"shown": !g.clean, "primary": g.primary, "enabled": enabled,
                                       "message": self.messages.borrow().get(&g.path).cloned().unwrap_or_default()},
                            "identity": g.identity,
                            "drift": g.drift.as_ref().map(|d| json!({
                                "text": d.text, "sub": d.sub, "full": d.full(),
                                "buttons": d.buttons.iter().map(|(_, l)| l.clone()).collect::<Vec<_>>(),
                            })),
                        })
                    })
                    .collect();
                json!({"mode": mode, "project": v.project, "header": v.title, "add": v.add,
                       "del": v.del, "groups": groups})
            }
        }
    }

    fn group_path(&self, source: &str) -> Result<PathBuf> {
        self.shown
            .borrow()
            .as_ref()
            .and_then(|(_, _, gs)| gs.iter().find(|g| g.source == source))
            .map(|g| g.path.clone())
            .ok_or_else(|| Error::not_found(format!("no Changes group '{source}'")))
    }
}

/// A file row: status letter, folder, name, `+a −d`; a click stages /
/// unstages the file.
fn file_row(root: &Path, f: &FileRow) -> gtk::Widget {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let st = label(&f.letter.to_string(), &["file-status"]);
    st.add_css_class(&format!("st-{}", f.letter));
    b.append(&st);
    let names = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    names.set_hexpand(true);
    let dir = label(&f.dir, &["file-row-dir"]);
    dir.set_ellipsize(gtk::pango::EllipsizeMode::Start);
    names.append(&dir);
    names.append(&label(&f.name, &["file-row-name"]));
    b.append(&names);
    let stat = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    stat.add_css_class("file-stat");
    if f.add > 0 {
        stat.append(&label(&format!("+{}", f.add), &["file-stat-add"]));
    }
    if f.del > 0 {
        stat.append(&label(&format!("−{}", f.del), &["file-stat-del"]));
    }
    b.append(&stat);
    let row = gtk::Button::new();
    row.set_child(Some(&b));
    row.add_css_class("flat");
    row.add_css_class("file-row");
    if f.staged {
        row.add_css_class("staged");
    }
    row.set_tooltip_text(Some(if f.staged {
        "Staged · click to unstage"
    } else {
        "Click to stage"
    }));
    let (root, path, staged) = (root.to_path_buf(), f.path.clone(), f.staged);
    row.connect_clicked(move |_| stage(&root, &path, !staged));
    row.upcast()
}

fn stage(root: &Path, path: &str, on: bool) {
    let (root, path) = (root.to_path_buf(), path.to_string());
    crate::worktrees::in_background(
        move || {
            if on {
                scm::stage(&root, &[path])
            } else {
                let initial = scm::status(&root).map(|s| s.initial).unwrap_or(false);
                scm::unstage(&root, &[path], initial)
            }
        },
        |r| {
            if let Err(e) = r {
                crate::worktrees::alert("Stage", &e.message);
            }
            if let Some(pc) = get() {
                pc.refresh();
            }
        },
    );
}

/// The drift card (A.67–A.68): its buttons do what Project home's do.
fn drift_card(project: &str, source: &str, d: &home::DriftCard) -> gtk::Widget {
    let card = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    card.add_css_class("drift-card");
    let icon = gtk::Image::from_icon_name("ph-warning-diamond-fill-symbolic");
    icon.add_css_class("drift-icon");
    icon.set_valign(gtk::Align::Start);
    card.append(&icon);
    let col = gtk::Box::new(gtk::Orientation::Vertical, 0);
    col.set_hexpand(true);
    let t = label(&d.text, &["drift-text"]);
    t.set_wrap(true);
    col.append(&t);
    let s = label(&d.sub, &["drift-sub"]);
    s.set_wrap(true);
    col.append(&s);
    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    actions.add_css_class("drift-actions");
    for (a, text) in &d.buttons {
        let b = gtk::Button::with_label(text);
        b.add_css_class("btn");
        let (p, s, a) = (project.to_string(), source.to_string(), *a);
        b.connect_clicked(move |_| match a {
            rade_core::drift::Action::BackToBase => crate::home_view::back_to_base(&p, &s),
            rade_core::drift::Action::Keep => crate::home_view::keep(&p, &s),
            rade_core::drift::Action::MoveToWorktree => crate::home_view::move_to_worktree(&p, &s),
        });
        actions.append(&b);
    }
    col.append(&actions);
    card.append(&col);
    card.upcast()
}

/// Read a checkout: status entries, `+/−` against HEAD, commits ahead of
/// `base` (blocking: git).
fn read(path: &Path, base: &str) -> Read {
    let st = match scm::status(path) {
        Ok(s) => s,
        Err(e) => {
            return Read {
                error: Some(e.message),
                ..Default::default()
            }
        }
    };
    let entries: Vec<(String, char, char)> = st
        .files
        .iter()
        .map(|c| (c.path.clone(), c.x, c.y))
        .collect();
    let mut counts = HashMap::new();
    if !st.initial {
        if let Ok(out) = rade_core::git::run(path, &["diff", "HEAD", "--numstat", "-M"]) {
            counts = home::numstat(&out);
        }
    }
    let lines = |p: &str| -> usize {
        std::fs::read(path.join(p))
            .ok()
            .filter(|b| b.len() < 4 << 20 && !b.contains(&0))
            .map(|b| bytecount(&b))
            .unwrap_or(0)
    };
    let rows = home::file_rows(&entries, &counts, &lines);
    let ahead = rade_core::git::run(path, &["rev-list", "--count", &format!("{base}..HEAD")])
        .ok()
        .or_else(|| {
            rade_core::git::run(
                path,
                &["rev-list", "--count", &format!("origin/{base}..HEAD")],
            )
            .ok()
        })
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    Read {
        rows,
        ahead,
        error: None,
        identity: None,
    }
}

fn bytecount(b: &[u8]) -> usize {
    let n = b.iter().filter(|c| **c == b'\n').count();
    if b.last().is_some_and(|c| *c != b'\n') {
        n + 1
    } else {
        n
    }
}

/// `debug.ui changes…` ops: `changes` (what the page shows; starts a new
/// read of the checkouts),
/// `changes_stage` (`source`, `path`: a click on that file row),
/// `changes_message` (`source`, `text`), `changes_commit` (`source`),
/// `changes_details` (`arg` on | off).
pub fn debug(p: &Value) -> Result<Value> {
    let pc = get().ok_or_else(|| Error::internal("no Changes page"))?;
    let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("");
    match s("op") {
        // Read the checkouts again, as the 3 s poll of an active window
        // does (headless Weston never activates it); the answer shows the
        // last read, so callers poll.
        "changes" => pc.refresh(),
        "changes_stage" => {
            let root = pc.group_path(s("source"))?;
            let staged = pc
                .reads
                .borrow()
                .get(&root)
                .and_then(|r| {
                    r.rows
                        .iter()
                        .find(|f| f.path == s("path"))
                        .map(|f| f.staged)
                })
                .ok_or_else(|| Error::not_found(format!("no file row {}", s("path"))))?;
            stage(&root, s("path"), !staged);
        }
        "changes_message" => {
            let root = pc.group_path(s("source"))?;
            pc.messages
                .borrow_mut()
                .insert(root.clone(), s("text").to_string());
            pc.rendered.borrow_mut().take();
            pc.render();
        }
        "changes_commit" => {
            let root = pc.group_path(s("source"))?;
            if !pc
                .commits
                .borrow()
                .get(&root)
                .is_some_and(|b| b.is_sensitive())
            {
                return Err(Error::usage(
                    "Commit is disabled (nothing staged or no message)",
                ));
            }
            pc.commit(&root);
        }
        "changes_details" => {
            if let Some(c) = crate::changes::get() {
                c.set_detail(s("arg") != "off");
            }
        }
        op => return Err(Error::usage(format!("unknown debug.ui op '{op}'"))),
    }
    Ok(json!({"changes": pc.info()}))
}
