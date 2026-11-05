//! Branch set dialogs (PRD R150.1, R151.3, ADR-0054) over the R106 scrim:
//! **New branch across sources…** (name, a checkbox per link with its base,
//! an optional Agent and prompt, the live plan from `branch_set::plan` on
//! facts read off the GTK thread, Create = `worktree.add` with `sources`
//! then `pane.spawn` with `set`) and **Create Pull Requests for Set…** (a
//! row per Source with commits, one shared title, body, Generate on the
//! combined diff, Create = push + `gh pr create` per Source in order, then
//! `gh pr edit --body` with the cross-links; Retry creates only the rest).
//! The scrim, Esc and `debug.ui dialog…` belong to `project_dialogs`.

use crate::app::{app, try_app};
use crate::changes_card::text_of;
use crate::worktrees::in_background;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use rade_core::branch_set::{self, PrRef, PrRow};
use rade_core::rpc::{Error, Result};
use rade_core::scm::{self, DraftKind};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

type Env = Vec<(String, String)>;

enum Open {
    Form(Rc<Form>),
    Prs(Rc<Prs>),
}

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
}

/// A Branch set dialog is up.
pub fn is_open() -> bool {
    OPEN.with(|o| o.borrow().is_some())
}

/// Forget the open dialog (`project_dialogs::close` hides the scrim);
/// whether one was open.
pub(crate) fn take() -> bool {
    let d = OPEN.with(|o| o.borrow_mut().take());
    if let Some(Open::Prs(p)) = &d {
        if let Some(c) = p.drafting.borrow_mut().take() {
            c.store(true, Ordering::Relaxed);
        }
    }
    d.is_some()
}

fn close() {
    crate::project_dialogs::close();
}

fn form() -> Option<Rc<Form>> {
    OPEN.with(|o| match &*o.borrow() {
        Some(Open::Form(f)) => Some(f.clone()),
        _ => None,
    })
}

fn prs() -> Option<Rc<Prs>> {
    OPEN.with(|o| match &*o.borrow() {
        Some(Open::Prs(p)) => Some(p.clone()),
        _ => None,
    })
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

fn icon(name: &str, classes: &[&str]) -> gtk::Image {
    let i = gtk::Image::from_icon_name(name);
    for c in classes {
        i.add_css_class(c);
    }
    i
}

/// The header row (C.4–C.6): icon, title, context, `x`.
fn header(icon_name: &str, title: &str, context: &str) -> gtk::Box {
    let h = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    h.add_css_class("dialog-header");
    h.append(&icon(icon_name, &["dialog-icon"]));
    h.append(&label(title, &["dialog-title"]));
    let ctx = label(context, &["dialog-context"]);
    ctx.set_hexpand(true);
    h.append(&ctx);
    let x = gtk::Button::from_icon_name("ph-x-symbolic");
    x.add_css_class("flat");
    x.add_css_class("dialog-close");
    x.set_tooltip_text(Some("Close (Esc)"));
    x.connect_clicked(|_| close());
    h.append(&x);
    h
}

/// The footer (C.23–C.26): a note, Cancel and the primary button.
fn footer(note: &str, primary: &str) -> (gtk::Box, gtk::Button) {
    let f = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    f.add_css_class("dialog-footer");
    let n = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    n.add_css_class("dialog-note");
    n.set_hexpand(true);
    n.append(&icon("ph-info-symbolic", &["dialog-note-icon"]));
    let nl = label(note, &["dialog-note-text"]);
    nl.set_wrap(true);
    nl.set_max_width_chars(40);
    nl.set_hexpand(true);
    n.append(&nl);
    f.append(&n);
    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("secondary-button");
    cancel.add_css_class("dialog-cancel");
    cancel.set_valign(gtk::Align::Center);
    cancel.connect_clicked(|_| close());
    f.append(&cancel);
    let p = gtk::Button::with_label(primary);
    p.add_css_class("suggested-action");
    p.add_css_class("dialog-primary");
    p.set_valign(gtk::Align::Center);
    f.append(&p);
    (f, p)
}

/// A multi-line text field (the prompt, a PR body).
fn text_box(class: &str, height: i32) -> (gtk::ScrolledWindow, gtk::TextView) {
    let v = gtk::TextView::new();
    v.set_wrap_mode(gtk::WrapMode::WordChar);
    v.set_top_margin(6);
    v.set_bottom_margin(6);
    v.set_left_margin(8);
    v.set_right_margin(8);
    v.add_css_class("set-text");
    let s = gtk::ScrolledWindow::new();
    s.set_child(Some(&v));
    s.set_min_content_height(height);
    s.add_css_class("set-text-box");
    s.add_css_class(class);
    (s, v)
}

/// Put `dialog` in the scrim (closing whatever is there) and show it.
fn show(dialog: &gtk::Box) -> bool {
    close();
    crate::palette::get().inspect(|p| p.close(false));
    let Some(scrim) = crate::project_dialogs::scrim() else {
        return false;
    };
    while let Some(c) = scrim.first_child() {
        scrim.remove(&c);
    }
    let w = crate::project_dialogs::dialog_width();
    dialog.add_css_class("new-project-dialog");
    dialog.add_css_class("set-dialog");
    dialog.set_size_request(w, -1);
    if crate::layout_mode::mode().large_text {
        crate::layout_mode::wrap_labels(dialog.upcast_ref());
    }
    let holder = libadwaita::Clamp::new();
    holder.set_unit(libadwaita::LengthUnit::Px);
    holder.set_maximum_size(w);
    holder.set_tightening_threshold(w);
    holder.set_valign(gtk::Align::Start);
    holder.set_margin_top(92);
    holder.set_child(Some(dialog));
    holder.add_css_class("dialog-holder");
    scrim.append(&holder);
    scrim.set_visible(true);
    true
}

/// The shown Workspace's Project `query` (id or name).
fn project_named(query: &str) -> Option<rade_core::project::Project> {
    let app = try_app()?;
    let m = app.model.borrow();
    crate::navdata::projects(&m)
        .into_iter()
        .find(|p| p.id == query || p.name == query)
}

/// The Project `New Branch Across Sources…` opens on: the shown Project
/// home, else the focused checkout's Project, else the first one.
pub fn current_project() -> Option<String> {
    let app = try_app()?;
    let m = app.model.borrow();
    crate::home_view::shown(&m)
        .or_else(|| crate::changes::focused(&m).map(|(p, _, _)| p))
        .or_else(|| crate::navdata::projects(&m).first().map(|p| p.id.clone()))
}

/// `New Branch Across Sources…` from the palette.
pub fn new_set_here() {
    match current_project() {
        Some(p) => new_set(&p),
        None => crate::worktrees::alert(
            "New branch across sources",
            "This Workspace has no Project yet: create one first (New Project…).",
        ),
    }
}

// ---- New branch across sources (R150.1) -------------------------------------------

struct LinkRow {
    source: String,
    name: String,
    /// The link's base as written (empty: the Source's main branch).
    base: String,
    include: bool,
    plan: String,
    refused: Option<String>,
    check: gtk::Button,
    plan_label: gtk::Label,
}

struct Form {
    project: String,
    project_name: String,
    name: gtk::Entry,
    rows: RefCell<Vec<LinkRow>>,
    /// "No Agent", then the Presets.
    presets: Vec<String>,
    preset: gtk::DropDown,
    accounts: RefCell<Vec<String>>,
    account_list: gtk::StringList,
    account: gtk::DropDown,
    prompt: gtk::TextView,
    prompt_box: gtk::ScrolledWindow,
    summary: gtk::Label,
    error: gtk::Label,
    primary: gtk::Button,
    gen: Cell<u64>,
    planning: Cell<bool>,
    plan_ok: Cell<bool>,
    busy: Cell<bool>,
}

const NO_AGENT: &str = "No Agent";
const DEFAULT_ACCOUNT: &str = "Default account";

/// Open the form for Project `project` (id or name) of the shown Workspace.
pub fn new_set(project: &str) {
    let Some(p) = project_named(project) else {
        return crate::worktrees::alert("New branch across sources", "The Project is gone.");
    };
    if let Some(e) = &p.error {
        return crate::worktrees::alert("New branch across sources", e);
    }
    let app = app();
    let names: Vec<(String, String, String)> = {
        let m = app.model.borrow();
        p.sources
            .iter()
            .map(|s| {
                let n = m
                    .source(&s.source)
                    .map_or(s.source.clone(), |x| x.name.clone());
                (s.source.clone(), n, s.base.clone().unwrap_or_default())
            })
            .collect()
    };
    let presets: Vec<String> = std::iter::once(NO_AGENT.to_string())
        .chain(
            app.config
                .borrow()
                .presets()
                .unwrap_or_default()
                .into_keys(),
        )
        .collect();

    let dialog = gtk::Box::new(gtk::Orientation::Vertical, 0);
    dialog.append(&header(
        "ph-git-branch-symbolic",
        "New branch across sources",
        &format!("in {}", p.name),
    ));
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.add_css_class("dialog-body");
    body.append(&label("Branch name", &["field-label"]));
    let name = gtk::Entry::new();
    name.add_css_class("project-name-entry");
    name.add_css_class("set-name-entry");
    name.set_placeholder_text(Some("feat-x"));
    body.append(&name);

    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    head.add_css_class("field-row");
    let l = label("Sources", &["field-label"]);
    l.set_hexpand(true);
    head.append(&l);
    head.append(&label(
        "Each Worktree is cut from its own link's base.",
        &["field-hint"],
    ));
    body.append(&head);
    let editor = gtk::Box::new(gtk::Orientation::Vertical, 0);
    editor.add_css_class("link-editor");
    body.append(&editor);
    let mut rows = Vec::new();
    for (source, n, base) in names {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.add_css_class("link-editor-row");
        row.add_css_class("set-link-row");
        let check = gtk::Button::new();
        check.add_css_class("flat");
        check.add_css_class("link-check");
        check.set_valign(gtk::Align::Center);
        row.append(&check);
        let src = label(&n, &["link-source"]);
        src.set_ellipsize(gtk::pango::EllipsizeMode::End);
        src.set_size_request(150, -1);
        src.set_valign(gtk::Align::Center);
        row.append(&src);
        let plan = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        plan.add_css_class("link-plan");
        plan.set_hexpand(true);
        plan.set_valign(gtk::Align::Center);
        let plan_label = label("", &["link-plan-detail"]);
        plan_label.set_wrap(true);
        plan_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        plan_label.set_hexpand(true);
        plan_label.set_max_width_chars(24);
        plan.append(&plan_label);
        row.append(&plan);
        let b = label(
            if base.is_empty() {
                "main branch"
            } else {
                &base
            },
            &["set-link-base"],
        );
        b.set_valign(gtk::Align::Center);
        b.set_tooltip_text(Some("The link's base (R97)"));
        row.append(&b);
        editor.append(&row);
        rows.push(LinkRow {
            source,
            name: n,
            base,
            include: true,
            plan: String::new(),
            refused: None,
            check,
            plan_label,
        });
    }

    let agent_head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    agent_head.add_css_class("field-row");
    let l = label("Agent", &["field-label"]);
    l.set_hexpand(true);
    agent_head.append(&l);
    agent_head.append(&label(
        "Optional: one Agent in the lead Worktree, the others added to it.",
        &["field-hint"],
    ));
    body.append(&agent_head);
    let agent_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let refs: Vec<&str> = presets.iter().map(String::as_str).collect();
    let preset = gtk::DropDown::from_strings(&refs);
    preset.add_css_class("link-base");
    preset.add_css_class("set-preset");
    agent_row.append(&preset);
    let account_list = gtk::StringList::new(&[DEFAULT_ACCOUNT]);
    let account = gtk::DropDown::new(Some(account_list.clone()), gtk::Expression::NONE);
    account.add_css_class("link-base");
    account.add_css_class("set-account");
    account.set_visible(false);
    agent_row.append(&account);
    body.append(&agent_row);
    let pl = label("Prompt", &["field-label"]);
    pl.add_css_class("field-row");
    body.append(&pl);
    let (prompt_box, prompt) = text_box("set-prompt", 72);
    prompt_box.set_sensitive(false);
    body.append(&prompt_box);
    let summary = label("", &["set-plan-summary"]);
    summary.set_wrap(true);
    summary.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    body.append(&summary);
    let error = label("", &["field-error"]);
    error.set_wrap(true);
    error.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    error.set_visible(false);
    body.append(&error);
    dialog.append(&body);
    let (foot, primary) = footer(
        "RADE writes nothing before Create; a failure halfway undoes what it made.",
        "Create",
    );
    dialog.append(&foot);

    let f = Rc::new(Form {
        project: p.id.clone(),
        project_name: p.name.clone(),
        name,
        rows: RefCell::new(rows),
        presets,
        preset,
        accounts: RefCell::default(),
        account_list,
        account,
        prompt,
        prompt_box,
        summary,
        error,
        primary,
        gen: Cell::new(0),
        planning: Cell::new(false),
        plan_ok: Cell::new(false),
        busy: Cell::new(false),
    });
    for (i, r) in f.rows.borrow().iter().enumerate() {
        let w = Rc::downgrade(&f);
        r.check.connect_clicked(move |_| {
            if let Some(f) = w.upgrade() {
                let inc = !f.rows.borrow()[i].include;
                f.set_include(i, inc);
            }
        });
    }
    let w = Rc::downgrade(&f);
    f.name.connect_changed(move |_| {
        if let Some(f) = w.upgrade() {
            f.schedule_plan();
        }
    });
    let w = Rc::downgrade(&f);
    f.name.connect_activate(move |_| {
        if let Some(f) = w.upgrade() {
            f.create();
        }
    });
    let w = Rc::downgrade(&f);
    f.preset.connect_selected_notify(move |_| {
        if let Some(f) = w.upgrade() {
            f.preset_changed();
        }
    });
    let w = Rc::downgrade(&f);
    f.primary.connect_clicked(move |_| {
        if let Some(f) = w.upgrade() {
            f.create();
        }
    });
    if !show(&dialog) {
        return;
    }
    OPEN.with(|o| *o.borrow_mut() = Some(Open::Form(f.clone())));
    crate::project_dialogs::sync_primaries();
    f.refresh_rows();
    f.schedule_plan();
    f.name.grab_focus();
}

impl Form {
    fn set_include(self: &Rc<Self>, i: usize, include: bool) {
        if let Some(r) = self.rows.borrow_mut().get_mut(i) {
            r.include = include;
        }
        self.refresh_rows();
        self.schedule_plan();
    }

    fn refresh_rows(&self) {
        for r in self.rows.borrow().iter() {
            r.check.set_icon_name(if r.include {
                "ph-check-square-fill-symbolic"
            } else {
                "ph-square-symbolic"
            });
            if r.include {
                r.check.add_css_class("checked");
            } else {
                r.check.remove_css_class("checked");
            }
            match (&r.refused, r.include) {
                (Some(why), true) => {
                    r.plan_label.set_label(why);
                    r.plan_label.add_css_class("refused");
                }
                (_, true) => {
                    r.plan_label.set_label(&r.plan);
                    r.plan_label.remove_css_class("refused");
                }
                (_, false) => {
                    r.plan_label.set_label("not in the set");
                    r.plan_label.remove_css_class("refused");
                }
            }
        }
    }

    fn validate(&self) {
        self.primary
            .set_sensitive(self.plan_ok.get() && !self.planning.get() && !self.busy.get());
    }

    /// The Preset chosen (None = no Agent).
    fn agent(&self) -> Option<String> {
        let i = self.preset.selected() as usize;
        (i > 0).then(|| self.presets.get(i).cloned()).flatten()
    }

    fn account_choice(&self) -> Option<String> {
        let i = self.account.selected() as usize;
        (i > 0)
            .then(|| self.accounts.borrow().get(i - 1).cloned())
            .flatten()
    }

    /// A Preset chosen: its accounts (R108, as the `+` menu lists them:
    /// this Workspace's, the default first) and the prompt field.
    fn preset_changed(&self) {
        let agent = self.agent();
        self.prompt_box.set_sensitive(agent.is_some());
        let names: Vec<String> = match (&agent, try_app()) {
            (Some(a), Some(app)) => {
                let m = app.model.borrow();
                let ws = m.active_workspace().to_string();
                let mut rows: Vec<_> =
                    rade_core::new_menu::rows(&ws, &crate::accounts::lists_cached(&m))
                        .into_iter()
                        .filter(|r| {
                            r.kind == rade_core::new_menu::RowKind::Account
                                && !r.locked
                                && r.agent.map(|k| k.as_str()) == Some(a.as_str())
                        })
                        .collect();
                rows.sort_by_key(|r| !r.default);
                rows.into_iter().map(|r| r.name).collect()
            }
            _ => Vec::new(),
        };
        let mut all = vec![DEFAULT_ACCOUNT.to_string()];
        all.extend(names.iter().cloned());
        let refs: Vec<&str> = all.iter().map(String::as_str).collect();
        self.account_list
            .splice(0, self.account_list.n_items(), &refs);
        self.account.set_selected(0);
        self.account.set_visible(!names.is_empty());
        *self.accounts.borrow_mut() = names;
    }

    fn sources(&self) -> Vec<String> {
        self.rows
            .borrow()
            .iter()
            .filter(|r| r.include)
            .map(|r| r.source.clone())
            .collect()
    }

    /// Plan again 50 ms after the last change (older answers are dropped).
    fn schedule_plan(self: &Rc<Self>) {
        let gen = self.gen.get() + 1;
        self.gen.set(gen);
        self.planning.set(true);
        self.validate();
        let w = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(50), move || {
            let Some(f) = w.upgrade() else { return };
            if f.gen.get() != gen {
                return;
            }
            let name = f.name.text().trim().to_string();
            if name.is_empty() {
                f.take_plan(Err(Error::usage("Type a branch name.")), "");
                return;
            }
            let (project, sources) = (f.project.clone(), json!(f.sources()));
            let w = Rc::downgrade(&f);
            in_background(
                move || {
                    let n = name.clone();
                    (crate::worktrees::plan_set(&project, sources, &n), name)
                },
                move |(r, name)| {
                    let Some(f) = w.upgrade() else { return };
                    if f.gen.get() == gen {
                        f.take_plan(r, &name);
                    }
                },
            );
        });
    }

    fn take_plan(&self, r: Result<branch_set::Plan>, name: &str) {
        self.planning.set(false);
        let home = rade_core::paths::home_dir(&rade_core::paths::process_env);
        {
            let mut rows = self.rows.borrow_mut();
            for row in rows.iter_mut() {
                row.plan.clear();
                row.refused = None;
            }
            match &r {
                Ok(plan) => {
                    for s in &plan.steps {
                        if let Some(row) = rows.iter_mut().find(|x| x.source == s.source) {
                            row.plan = branch_set::step_text(s, name, &home);
                        }
                    }
                }
                Err(e) => {
                    if let Some(src) = branch_set::refused_source(&e.message) {
                        let why = e
                            .message
                            .split_once("': ")
                            .map_or(e.message.as_str(), |(_, w)| w);
                        if let Some(row) = rows.iter_mut().find(|x| x.source == src) {
                            row.refused = Some(why.to_string());
                        }
                    }
                }
            }
        }
        match &r {
            Ok(plan) => {
                let n = plan.steps.len();
                self.summary.set_label(&format!(
                    "Creates {n} Worktree{} on branch {name}, each from its link's base.",
                    if n == 1 { "" } else { "s" }
                ));
                self.summary.remove_css_class("refused");
            }
            Err(e) => {
                self.summary.set_label(&e.message);
                self.summary.add_css_class("refused");
            }
        }
        self.plan_ok.set(r.is_ok());
        self.refresh_rows();
        self.validate();
    }

    fn show_error(&self, msg: Option<&str>) {
        self.error.set_label(msg.unwrap_or_default());
        self.error.set_visible(msg.is_some());
    }

    /// Create: the set through `worktree.add` (off the GTK thread), then the
    /// set's Agent through `pane.spawn` with `set` when one is chosen.
    fn create(self: &Rc<Self>) {
        self.validate();
        if !self.primary.is_sensitive() {
            return;
        }
        self.busy.set(true);
        self.validate();
        self.show_error(None);
        let name = self.name.text().trim().to_string();
        let params = json!({"project": self.project, "name": name, "sources": self.sources()});
        let agent = self.agent();
        let account = self.account_choice();
        let prompt = text_of(&self.prompt);
        let project = self.project.clone();
        let w = Rc::downgrade(self);
        in_background(
            move || crate::worktrees::dispatch("worktree.add", params),
            move |r| {
                let f = w.upgrade();
                match r {
                    Err(e) => {
                        if let Some(f) = f {
                            f.busy.set(false);
                            f.show_error(Some(&e.message));
                            f.schedule_plan();
                        }
                    }
                    Ok(v) => {
                        close();
                        crate::navdata::refresh();
                        let app = app();
                        match agent {
                            Some(a) => {
                                let s = crate::handlers::Spawn {
                                    agent: Some(a),
                                    account,
                                    prompt: (!prompt.trim().is_empty()).then_some(prompt),
                                    project: Some(project),
                                    set: Some(name),
                                    ..Default::default()
                                };
                                match crate::handlers::spawn(&app, s) {
                                    Ok(v) => {
                                        if let Some(p) = v["pane"].as_str() {
                                            let _ = app.update(|m| m.focus_pane(p));
                                        }
                                    }
                                    Err(e) => crate::worktrees::alert(
                                        "Could not start the set's Agent",
                                        &e.message,
                                    ),
                                }
                            }
                            None => {
                                // The lead Worktree (first link in Project order).
                                let lead = v["worktrees"][0]["source"].as_str().and_then(|s| {
                                    let m = app.model.borrow();
                                    let src = m.source(s)?;
                                    let w = src.worktrees.iter().find(|w| w.name == name)?;
                                    Some((s.to_string(), w.id.clone()))
                                });
                                if let Some((s, w)) = lead {
                                    crate::home_view::select_worktree(&s, &w);
                                }
                            }
                        }
                    }
                }
            },
        );
    }

    fn info(&self) -> Value {
        let rows: Vec<Value> = self
            .rows
            .borrow()
            .iter()
            .map(|r| {
                json!({"source": r.source, "name": r.name, "base": r.base,
                       "include": r.include, "plan": r.plan, "refused": r.refused})
            })
            .collect();
        json!({
            "open": true, "kind": "branch_set", "project": self.project,
            "title": "New branch across sources",
            "context": format!("in {}", self.project_name),
            "name": self.name.text().as_str(),
            "rows": rows,
            "summary": self.summary.label().as_str(),
            "refused": !self.plan_ok.get() && !self.planning.get(),
            "presets": self.presets[1..],
            "agent": self.agent(),
            "accounts": *self.accounts.borrow(),
            "account": self.account_choice(),
            "prompt": text_of(&self.prompt),
            "error": self.error.is_visible().then(|| self.error.label().to_string()),
            "buttons": ["Cancel", "Create"],
            "primary_enabled": self.primary.is_sensitive(),
            "planning": self.planning.get(), "busy": self.busy.get(),
        })
    }
}

// ---- Create Pull Requests for Set (R151.3) ----------------------------------------

/// One Source with commits.
struct PrLine {
    row: PrRow,
    name: String,
    dir: PathBuf,
    branch: String,
    env: Env,
    title: gtk::Entry,
    state: gtk::Label,
}

/// What the gathering read for one Worktree of the set.
struct Gathered {
    row: PrRow,
    name: String,
    dir: PathBuf,
    branch: Option<String>,
    env: Env,
}

struct Prs {
    project: String,
    set: String,
    lines: RefCell<Vec<PrLine>>,
    list: gtk::Box,
    title: gtk::Entry,
    body: gtk::TextView,
    generate: gtk::Button,
    status: gtk::Label,
    primary: gtk::Button,
    made: RefCell<Vec<PrRef>>,
    urls: RefCell<Vec<(String, String)>>,
    edited: RefCell<Vec<u64>>,
    failed: RefCell<Option<(String, String)>>,
    loading: Cell<bool>,
    busy: Cell<bool>,
    drafting: RefCell<Option<Arc<AtomicBool>>>,
    /// The lead's `[scm] draft_command` and Workspace env (Generate).
    draft: (String, Env),
}

/// Open the PR dialog for set `set` of Project `project` (id or name).
pub fn set_prs(project: &str, set: &str) {
    let Some(p) = project_named(project) else {
        return crate::worktrees::alert("Create Pull Requests for Set", "The Project is gone.");
    };
    let app = app();
    let found = {
        let m = app.model.borrow();
        let ws = m.active_workspace().to_string();
        crate::worktrees::set_members(&m, Some(&ws), Some(&p.id), set).map(|found| {
            found
                .into_iter()
                .map(|(s, w, mem)| {
                    let env = {
                        let e = crate::app::workspace_env_at(&m, &mem.path);
                        if e.is_empty() {
                            crate::app::workspace_env(&m, &s)
                        } else {
                            e
                        }
                    };
                    let name = m.source(&s).map_or(s.clone(), |x| x.name.clone());
                    (s, w, mem, env, name)
                })
                .collect::<Vec<_>>()
        })
    };
    let found = match found {
        Ok(f) => f,
        Err(e) => return crate::worktrees::alert("Create Pull Requests for Set", &e.message),
    };
    let draft = found
        .first()
        .map(|(s, w, _, env, _)| (app.config_for(s, Some(w)).scm.draft_command, env.clone()))
        .unwrap_or_default();

    let dialog = gtk::Box::new(gtk::Orientation::Vertical, 0);
    dialog.append(&header(
        "ph-git-pull-request-symbolic",
        &format!("Create Pull Requests for {set}"),
        &format!("in {}", p.name),
    ));
    let body_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body_box.add_css_class("dialog-body");
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let l = label("Pull requests", &["field-label"]);
    l.set_hexpand(true);
    head.append(&l);
    head.append(&label(
        "One per Source with commits, in Project order.",
        &["field-hint"],
    ));
    body_box.append(&head);
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list.add_css_class("link-editor");
    let loading = label(
        "Reading the set's branches…",
        &["link-plan", "set-pr-empty"],
    );
    loading.add_css_class("link-editor-row");
    list.append(&loading);
    body_box.append(&list);
    let tl = label("Title", &["field-label"]);
    tl.add_css_class("field-row");
    body_box.append(&tl);
    let title = gtk::Entry::new();
    title.add_css_class("project-name-entry");
    title.add_css_class("set-pr-title");
    title.set_placeholder_text(Some("One title for every pull request"));
    body_box.append(&title);
    let bh = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    bh.add_css_class("field-row");
    let bl = label("Description", &["field-label"]);
    bl.set_hexpand(true);
    bh.append(&bl);
    let generate = gtk::Button::new();
    let gi = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    gi.append(&icon("ph-sparkle-symbolic", &["btn-icon"]));
    gi.append(&label("Generate", &[]));
    generate.set_child(Some(&gi));
    generate.add_css_class("secondary-button");
    generate.add_css_class("set-pr-generate");
    generate.set_tooltip_text(Some(
        "Ask an Agent to draft the title and description from the set's combined diff",
    ));
    bh.append(&generate);
    body_box.append(&bh);
    let (bs, body) = text_box("set-pr-body", 110);
    body_box.append(&bs);
    let status = label("", &["set-plan-summary"]);
    status.set_wrap(true);
    status.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    body_box.append(&status);
    dialog.append(&body_box);
    let (foot, primary) = footer(
        &format!("Pushes each branch when needed; every PR body ends with “Part of {set}: …”."),
        "Create",
    );
    dialog.append(&foot);

    let d = Rc::new(Prs {
        project: p.id.clone(),
        set: set.to_string(),
        lines: RefCell::default(),
        list,
        title,
        body,
        generate,
        status,
        primary,
        made: RefCell::default(),
        urls: RefCell::default(),
        edited: RefCell::default(),
        failed: RefCell::default(),
        loading: Cell::new(true),
        busy: Cell::new(false),
        drafting: RefCell::default(),
        draft,
    });
    let w = Rc::downgrade(&d);
    d.title.connect_changed(move |_| {
        if let Some(d) = w.upgrade() {
            d.sync_titles();
            d.validate();
        }
    });
    let w = Rc::downgrade(&d);
    d.generate.connect_clicked(move |_| {
        if let Some(d) = w.upgrade() {
            d.run_generate();
        }
    });
    let w = Rc::downgrade(&d);
    d.primary.connect_clicked(move |_| {
        if let Some(d) = w.upgrade() {
            d.create();
        }
    });
    if !show(&dialog) {
        return;
    }
    OPEN.with(|o| *o.borrow_mut() = Some(Open::Prs(d.clone())));
    crate::project_dialogs::sync_primaries();
    d.validate();
    d.title.grab_focus();
    // Each Worktree's branch and commits ahead of its base, off the GTK thread.
    let w = Rc::downgrade(&d);
    in_background(
        move || {
            found
                .into_iter()
                .map(|(s, wid, mem, env, name)| {
                    let start = rade_core::git::start_point(&mem.path, &mem.base);
                    let ahead = rade_core::git::run(
                        &mem.path,
                        &["rev-list", "--count", &format!("{start}..HEAD")],
                    )
                    .ok()
                    .and_then(|o| o.trim().parse().ok())
                    .unwrap_or(0);
                    Gathered {
                        row: PrRow {
                            source: s,
                            worktree: wid,
                            ahead,
                            base: scm::pr_base(Some(&mem.base)),
                        },
                        name,
                        branch: rade_core::git::branch(&mem.path),
                        dir: mem.path,
                        env,
                    }
                })
                .collect::<Vec<_>>()
        },
        move |g| {
            if let Some(d) = w.upgrade() {
                d.take_rows(g);
            }
        },
    );
}

impl Prs {
    fn take_rows(&self, gathered: Vec<Gathered>) {
        self.loading.set(false);
        while let Some(c) = self.list.first_child() {
            self.list.remove(&c);
        }
        let rows: Vec<PrRow> = gathered.iter().map(|g| g.row.clone()).collect();
        let keep = branch_set::pr_rows(&rows);
        let mut lines = Vec::new();
        for g in gathered {
            if !keep.iter().any(|r| r.source == g.row.source) {
                continue;
            }
            let Some(branch) = g.branch else { continue };
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.add_css_class("link-editor-row");
            row.add_css_class("set-pr-row");
            let src = label(&g.name, &["link-source"]);
            src.set_size_request(120, -1);
            src.set_ellipsize(gtk::pango::EllipsizeMode::End);
            src.set_valign(gtk::Align::Center);
            row.append(&src);
            let info = gtk::Box::new(gtk::Orientation::Vertical, 2);
            info.set_valign(gtk::Align::Center);
            info.append(&label(
                &format!("{branch} → {}", g.row.base),
                &["set-link-base"],
            ));
            let state = label(
                &format!(
                    "{} commit{} ahead",
                    g.row.ahead,
                    if g.row.ahead == 1 { "" } else { "s" }
                ),
                &["link-plan", "set-pr-state"],
            );
            state.set_wrap(true);
            state.set_wrap_mode(gtk::pango::WrapMode::WordChar);
            state.set_max_width_chars(28);
            info.append(&state);
            info.set_size_request(220, -1);
            row.append(&info);
            let title = gtk::Entry::new();
            title.add_css_class("project-name-entry");
            title.add_css_class("set-pr-row-title");
            title.set_hexpand(true);
            title.set_valign(gtk::Align::Center);
            title.set_tooltip_text(Some("This PR's title (empty: the shared title)"));
            row.append(&title);
            self.list.append(&row);
            lines.push(PrLine {
                row: g.row,
                name: g.name,
                dir: g.dir,
                branch,
                env: g.env,
                title,
                state,
            });
        }
        if lines.is_empty() {
            let r = label(
                &format!(
                    "No Source of {} has commits ahead of its base: commit first.",
                    self.set
                ),
                &["link-plan", "set-pr-empty"],
            );
            r.add_css_class("link-editor-row");
            self.list.append(&r);
        }
        *self.lines.borrow_mut() = lines;
        self.sync_titles();
        self.validate();
    }

    /// Each row's title field shows the shared title until it is typed in.
    fn sync_titles(&self) {
        let t = self.title.text();
        for l in self.lines.borrow().iter() {
            l.title
                .set_placeholder_text(Some(if t.is_empty() { "Title" } else { t.as_str() }));
        }
    }

    fn title_of(&self, l: &PrLine) -> String {
        let own = l.title.text().trim().to_string();
        if own.is_empty() {
            self.title.text().trim().to_string()
        } else {
            own
        }
    }

    fn rows(&self) -> Vec<PrRow> {
        self.lines.borrow().iter().map(|l| l.row.clone()).collect()
    }

    fn validate(&self) {
        let lines = self.lines.borrow();
        let titled = lines.iter().all(|l| !self.title_of(l).is_empty());
        let done = self.made.borrow().len() == lines.len()
            && branch_set::retry_plan(&self.rows(), &self.made.borrow(), &self.edited.borrow())
                .edit
                .is_empty();
        self.primary.set_sensitive(
            !self.loading.get() && !self.busy.get() && !lines.is_empty() && titled && !done,
        );
        self.generate
            .set_sensitive(!self.loading.get() && !self.busy.get() && !lines.is_empty());
        self.primary.set_label(if self.failed.borrow().is_some() {
            "Retry"
        } else {
            "Create"
        });
    }

    /// Generate (R21.7) once on the combined diff; a second click cancels.
    fn run_generate(self: &Rc<Self>) {
        if let Some(flag) = self.drafting.borrow_mut().take() {
            flag.store(true, Ordering::Relaxed);
            return;
        }
        let parts: Vec<(String, PathBuf, String)> = self
            .lines
            .borrow()
            .iter()
            .map(|l| (l.name.clone(), l.dir.clone(), l.row.base.clone()))
            .collect();
        let Some(lead) = parts.first().map(|p| p.1.clone()) else {
            return;
        };
        let (command, env) = self.draft.clone();
        let set = self.set.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        *self.drafting.borrow_mut() = Some(cancel.clone());
        self.status
            .set_label("Generating the title and description…");
        self.status.remove_css_class("refused");
        let w = Rc::downgrade(self);
        in_background(
            move || {
                let mut inputs = Vec::new();
                for (name, dir, base) in &parts {
                    inputs.push((
                        name.clone(),
                        scm::gather_draft(dir, DraftKind::Pr, Some(base))?,
                    ));
                }
                let input = branch_set::combined_draft(&set, &inputs);
                let prompt = scm::draft_prompt(DraftKind::Pr, &input);
                scm::run_draft(&command, &lead, &prompt, scm::DRAFT_TIMEOUT, &cancel, &env)
                    .map(|out| scm::parse_draft(DraftKind::Pr, &out))
            },
            move |r: Result<(String, String)>| {
                let Some(d) = w.upgrade() else { return };
                d.drafting.borrow_mut().take();
                match r {
                    Ok((t, b)) => {
                        d.title.set_text(&t);
                        d.body.buffer().set_text(&b);
                        d.status.set_label("");
                    }
                    Err(e) => {
                        d.status.set_label(&format!("Generate: {}", e.message));
                        d.status.add_css_class("refused");
                    }
                }
            },
        );
    }

    /// Create (or Retry): per Source without a PR yet, in order, push when
    /// needed and `gh pr create`; once all exist, `gh pr edit --body` with
    /// the cross-links on each not edited yet. Off the GTK thread.
    fn create(self: &Rc<Self>) {
        self.validate();
        if !self.primary.is_sensitive() {
            return;
        }
        let rows = self.rows();
        let plan = branch_set::retry_plan(&rows, &self.made.borrow(), &self.edited.borrow());
        let jobs: Vec<Job> = self
            .lines
            .borrow()
            .iter()
            .filter(|l| plan.create.iter().any(|r| r.source == l.row.source))
            .map(|l| Job {
                source: l.row.source.clone(),
                dir: l.dir.clone(),
                branch: l.branch.clone(),
                base: l.row.base.clone(),
                title: self.title_of(l),
                body: text_of(&self.body),
                env: l.env.clone(),
            })
            .collect();
        let places: Vec<(String, PathBuf, Env)> = self
            .lines
            .borrow()
            .iter()
            .map(|l| (l.row.source.clone(), l.dir.clone(), l.env.clone()))
            .collect();
        let run = Run {
            set: self.set.clone(),
            body: text_of(&self.body),
            rows,
            made: self.made.borrow().clone(),
            edited: self.edited.borrow().clone(),
        };
        self.busy.set(true);
        self.failed.borrow_mut().take();
        self.status.set_label("Creating pull requests…");
        self.status.remove_css_class("refused");
        self.validate();
        let w = Rc::downgrade(self);
        in_background(
            move || run.go(&jobs, &places),
            move |out| {
                let Some(d) = w.upgrade() else { return };
                d.busy.set(false);
                d.take_outcome(out);
            },
        );
    }

    fn take_outcome(&self, out: Outcome) {
        for (src, url) in &out.urls {
            if let Some(l) = self.lines.borrow().iter().find(|l| &l.row.source == src) {
                crate::activity_view::pr_created(&l.dir, url);
                crate::pr_status::created(&l.dir, url);
            }
            self.urls.borrow_mut().push((src.clone(), url.clone()));
        }
        *self.made.borrow_mut() = out.made;
        *self.edited.borrow_mut() = out.edited;
        *self.failed.borrow_mut() = out.error.clone();
        for l in self.lines.borrow().iter() {
            let made = self
                .made
                .borrow()
                .iter()
                .find(|m| m.source == l.row.source)
                .cloned();
            let text = match (&made, &out.error) {
                (Some(m), _) if self.edited.borrow().contains(&m.number) => {
                    format!("#{} created, linked", m.number)
                }
                (Some(m), _) => format!("#{} created", m.number),
                (None, Some((src, why))) if *src == l.row.source => format!("failed: {why}"),
                (None, _) => "not created yet".into(),
            };
            l.state.set_label(&text);
            // A made PR keeps its title: Retry never touches it.
            l.title.set_sensitive(made.is_none());
            if text.starts_with("failed") {
                l.state.add_css_class("refused");
            } else {
                l.state.remove_css_class("refused");
            }
        }
        let n = self.made.borrow().len();
        match &out.error {
            Some((src, why)) => {
                let name = self
                    .lines
                    .borrow()
                    .iter()
                    .find(|l| &l.row.source == src)
                    .map_or(src.clone(), |l| l.name.clone());
                self.status.set_label(&format!(
                    "{n} of {} pull request{} made. {name}: {why}. Retry does the rest; nothing is made twice.",
                    self.lines.borrow().len(),
                    if self.lines.borrow().len() == 1 { "" } else { "s" },
                ));
                self.status.add_css_class("refused");
                self.validate();
            }
            None => {
                self.validate();
                close();
                if let Some(app) = try_app() {
                    app.chrome.toast(
                        &format!(
                            "{n} pull request{} created for {}",
                            if n == 1 { "" } else { "s" },
                            glib::markup_escape_text(&self.set)
                        ),
                        None,
                    );
                }
            }
        }
    }

    fn info(&self) -> Value {
        let rows: Vec<Value> = self
            .lines
            .borrow()
            .iter()
            .map(|l| {
                json!({"source": l.row.source, "name": l.name, "worktree": l.row.worktree,
                       "branch": l.branch, "base": l.row.base, "ahead": l.row.ahead,
                       "title": self.title_of(l), "state": l.state.label().as_str()})
            })
            .collect();
        let made: Vec<Value> = self
            .made
            .borrow()
            .iter()
            .map(|m| json!({"source": m.source, "repo": m.repo, "number": m.number}))
            .collect();
        json!({
            "open": true, "kind": "set_prs", "project": self.project, "set": self.set,
            "title": self.title.text().as_str(), "body": text_of(&self.body),
            "rows": rows, "made": made, "urls": *self.urls.borrow(),
            "edited": *self.edited.borrow(),
            "failed": self.failed.borrow().as_ref().map(|(s, w)| json!({"source": s, "error": w})),
            "status": self.status.label().as_str(),
            "buttons": ["Cancel", self.primary.label().unwrap_or_default().as_str()],
            "primary_enabled": self.primary.is_sensitive(),
            "loading": self.loading.get(), "busy": self.busy.get(),
            "drafting": self.drafting.borrow().is_some(),
        })
    }
}

/// One PR to create.
struct Job {
    source: String,
    dir: PathBuf,
    branch: String,
    base: String,
    title: String,
    body: String,
    env: Env,
}

/// What a Create / Retry starts from.
struct Run {
    set: String,
    body: String,
    rows: Vec<PrRow>,
    made: Vec<PrRef>,
    edited: Vec<u64>,
}

struct Outcome {
    made: Vec<PrRef>,
    edited: Vec<u64>,
    /// URLs of the PRs this run made: (source, url).
    urls: Vec<(String, String)>,
    /// (source, why) of the step that failed.
    error: Option<(String, String)>,
}

impl Run {
    fn go(mut self, jobs: &[Job], places: &[(String, PathBuf, Env)]) -> Outcome {
        let mut urls = Vec::new();
        for j in jobs {
            match create_one(j) {
                Ok((pr, url)) => {
                    self.made.push(pr);
                    urls.push((j.source.clone(), url));
                }
                Err(e) => return self.outcome(urls, Some((j.source.clone(), e.message))),
            }
        }
        // Every row has its PR: append the cross-links (R151.3).
        let plan = branch_set::retry_plan(&self.rows, &self.made, &self.edited);
        if !plan.create.is_empty() {
            return self.outcome(urls, None);
        }
        let suffix = branch_set::cross_link(&self.set, &self.made);
        let body = branch_set::linked_body(&self.body, &self.set, &suffix);
        let todo: Vec<PrRef> = self
            .made
            .iter()
            .filter(|m| plan.edit.contains(&m.number))
            .cloned()
            .collect();
        for m in todo {
            let Some((_, dir, env)) = places.iter().find(|(s, _, _)| *s == m.source) else {
                continue;
            };
            if let Err(e) = scm::run_gh(dir, "gh", &branch_set::edit_argv(m.number, &body), env) {
                return self.outcome(urls, Some((m.source.clone(), e.message)));
            }
            self.edited.push(m.number);
        }
        self.outcome(urls, None)
    }

    fn outcome(self, urls: Vec<(String, String)>, error: Option<(String, String)>) -> Outcome {
        Outcome {
            made: self.made,
            edited: self.edited,
            urls,
            error,
        }
    }
}

/// Push when needed (no own upstream, or commits ahead of it), then
/// `gh pr create` (R21.6's code).
fn create_one(j: &Job) -> Result<(PrRef, String)> {
    let st = scm::status(&j.dir)?;
    if st.own_upstream().is_none() || st.ahead > 0 {
        let plan = scm::push_plan(&j.dir, &st)?;
        scm::push(&j.dir, &plan, &j.env)?;
    }
    let url = scm::create_pr(&j.dir, "gh", &j.branch, &j.base, &j.title, &j.body, &j.env)?;
    let pr = branch_set::pr_ref(&j.source, &url)
        .ok_or_else(|| Error::usage(format!("gh pr create printed no PR URL ({url})")))?;
    Ok((pr, url))
}

// ---- debug.ui dialog… ---------------------------------------------------------------

/// `debug.ui dialog…` for the set dialogs (E2E): `dialog_open` (`kind`
/// branch_set + `project`, or set_prs + `project` + `set`), `dialog`,
/// `dialog_name` (`text`), `dialog_row` (`source`, `include`), `dialog_agent`
/// (`preset` — "" = none —, `account`?), `dialog_prompt` (`text`),
/// `dialog_title` / `dialog_body` (`text`), `dialog_row_title` (`source`,
/// `text`), `dialog_generate`, `dialog_primary` (Create / Retry),
/// `dialog_cancel`. Answers `{"dialog": …}`.
pub fn debug(p: &Value) -> Result<Value> {
    let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("");
    match s("op") {
        "dialog" => {}
        "dialog_open" => match s("kind") {
            "branch_set" => {
                let project = match s("project") {
                    "" => current_project().unwrap_or_default(),
                    q => q.to_string(),
                };
                new_set(&project);
            }
            "set_prs" => set_prs(s("project"), s("set")),
            k => return Err(Error::usage(format!("unknown dialog kind '{k}'"))),
        },
        "dialog_cancel" => close(),
        op => {
            if let Some(f) = form() {
                form_op(&f, op, p)?;
            } else if let Some(d) = prs() {
                prs_op(&d, op, p)?;
            } else {
                return Err(Error::usage("no dialog open"));
            }
        }
    }
    let v = OPEN.with(|o| match &*o.borrow() {
        Some(Open::Form(f)) => f.info(),
        Some(Open::Prs(d)) => d.info(),
        None => json!({"open": false}),
    });
    Ok(json!({ "dialog": v }))
}

fn form_op(f: &Rc<Form>, op: &str, p: &Value) -> Result<()> {
    let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("");
    match op {
        "dialog_name" => f.name.set_text(s("text")),
        "dialog_row" => {
            let id = {
                let app = app();
                let m = app.model.borrow();
                m.find_source(s("source"))
                    .map(|x| x.id.clone())
                    .unwrap_or_else(|_| s("source").to_string())
            };
            let i = f
                .rows
                .borrow()
                .iter()
                .position(|r| r.source == id)
                .ok_or_else(|| Error::not_found(format!("no row '{}'", s("source"))))?;
            let include = p.get("include").and_then(Value::as_bool).unwrap_or(true);
            f.set_include(i, include);
        }
        "dialog_agent" => {
            let i = match s("preset") {
                "" => 0,
                n => f
                    .presets
                    .iter()
                    .position(|x| x == n)
                    .filter(|i| *i > 0)
                    .ok_or_else(|| Error::not_found(format!("no Preset '{n}'")))?,
            };
            f.preset.set_selected(i as u32);
            f.preset_changed();
            if let Some(a) = p.get("account").and_then(Value::as_str) {
                let at = f
                    .accounts
                    .borrow()
                    .iter()
                    .position(|x| x == a)
                    .ok_or_else(|| Error::not_found(format!("no account '{a}'")))?;
                f.account.set_selected(at as u32 + 1);
            }
        }
        "dialog_prompt" => f.prompt.buffer().set_text(s("text")),
        "dialog_primary" => {
            if !f.primary.is_sensitive() {
                return Err(Error::usage("the primary button is disabled"));
            }
            f.create();
        }
        op => return Err(Error::usage(format!("unknown debug.ui op '{op}'"))),
    }
    Ok(())
}

fn prs_op(d: &Rc<Prs>, op: &str, p: &Value) -> Result<()> {
    let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("");
    match op {
        "dialog_title" => d.title.set_text(s("text")),
        "dialog_body" => d.body.buffer().set_text(s("text")),
        "dialog_row_title" => {
            let lines = d.lines.borrow();
            let l = lines
                .iter()
                .find(|l| l.row.source == s("source") || l.name == s("source"))
                .ok_or_else(|| Error::not_found(format!("no row '{}'", s("source"))))?;
            l.title.set_text(s("text"));
        }
        "dialog_generate" => d.run_generate(),
        "dialog_primary" => {
            if !d.primary.is_sensitive() {
                return Err(Error::usage("the primary button is disabled"));
            }
            d.create();
        }
        op => return Err(Error::usage(format!("unknown debug.ui op '{op}'"))),
    }
    Ok(())
}
