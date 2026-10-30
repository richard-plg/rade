//! New Project, Add Source, Change base… and Project settings (PRD R106,
//! spec c-new-project.md): one dialog body — a link table of rows (include
//! check, Source, checkout plan, base select, remove) — over a scrim. The
//! plan text of every row comes from `project.plan` (asked off the GTK
//! thread within 200 ms of a change; nothing is written before the primary
//! button), and the primary button writes through the M36 socket methods
//! only: `project.add` (Create), `project.link_add` (Add), `project.
//! link_set_base` (Change base) and, in Project settings, `project.set`
//! `name` / `link_remove` / `link_set_base` / `link_add`.

use crate::app::{app, try_app};
use gtk::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use rade_core::rpc::{Error, Result};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

/// The refusal of a link on a folder that is not a git repository.
const NOT_GIT: &str = ": not a git repository";

/// Which dialog the body is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    NewProject,
    AddSource { project: String },
    ChangeBase { project: String, source: String },
    Settings { project: String },
}

impl Kind {
    fn as_str(&self) -> &'static str {
        match self {
            Kind::NewProject => "new_project",
            Kind::AddSource { .. } => "add_source",
            Kind::ChangeBase { .. } => "change_base",
            Kind::Settings { .. } => "settings",
        }
    }

    fn project(&self) -> Option<&str> {
        match self {
            Kind::NewProject => None,
            Kind::AddSource { project }
            | Kind::ChangeBase { project, .. }
            | Kind::Settings { project } => Some(project),
        }
    }
}

/// One link row.
struct Row {
    source: String,
    name: String,
    base: String,
    /// The base the file has now (Settings / Change base: a change writes).
    old_base: Option<String>,
    checkout: Option<String>,
    include: bool,
    /// Already in the Project (Add Source shows it read-only).
    existing: bool,
    removed: bool,
    plan: String,
    plan_kind: String,
    refused: Option<String>,
    widgets: Option<RowWidgets>,
}

struct RowWidgets {
    root: gtk::Box,
    check: gtk::Button,
    base: gtk::DropDown,
    branches: gtk::StringList,
    plan_kind: gtk::Label,
    plan: gtk::Label,
    /// `Initialize` on a folder that is not a git repository (R131.2).
    init: gtk::Button,
}

struct Dialog {
    kind: Kind,
    ws: String,
    ws_name: String,
    /// The Project's id and name being edited (not New Project).
    project_name: String,
    name: gtk::Entry,
    name_error: gtk::Label,
    editor: gtk::Box,
    add_row: gtk::Button,
    primary: gtk::Button,
    rows: RefCell<Vec<Row>>,
    /// Bumped per plan request: older answers are dropped.
    gen: Cell<u64>,
    planning: Cell<bool>,
    /// Existing Project names (New Project refuses one).
    taken: Vec<String>,
    /// Sources this dialog registered (a folder, `owner/repo`): forgotten
    /// again unless a saved link uses them (R131.1).
    added: RefCell<Vec<String>>,
    /// The primary's writes succeeded.
    saved: Cell<bool>,
}

thread_local! {
    static SCRIM: RefCell<Option<gtk::Box>> = const { RefCell::new(None) };
    static OPEN: RefCell<Option<Rc<Dialog>>> = const { RefCell::new(None) };
}

/// Whether a dialog is open (Commit / Open terminal drop the primary
/// class meanwhile: one `.suggested-action` on screen, spec C.26).
pub fn is_open() -> bool {
    OPEN.with(|o| o.borrow().is_some())
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

/// The scrim (hidden) over the window's content; the dialog goes inside.
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
            // An open drop-down list keeps its Esc.
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
    overlay.add_overlay(&scrim);
    SCRIM.with(|s| *s.borrow_mut() = Some(scrim));
}

/// The shown Workspace's (id, name).
fn workspace() -> (String, String) {
    let app = app();
    let m = app.model.borrow();
    let ws = m.active_workspace().to_string();
    let name = m
        .workspace_looks()
        .into_iter()
        .find(|l| l.id == ws)
        .map(|l| l.name)
        .unwrap_or_else(|| ws.clone());
    (ws, name)
}

/// `<ws>/<project>` → the Project.
fn project_of(key: &str) -> Option<rade_core::project::Project> {
    crate::projects::lookup(key).map(|(_, p)| p)
}

/// New Project… (the Navigator's `+`, the palette).
pub fn new_project() {
    open(Kind::NewProject);
}

/// Add Source to a Project (`<ws>/<project>`), optionally with one Source
/// row already added (`Add <source> to Project…`).
pub fn add_source(key: &str, source: Option<&str>) {
    let Some(p) = project_of(key) else { return };
    open(Kind::AddSource { project: p.id });
    if let Some(s) = source {
        let _ = add(s, None);
    }
}

/// `Add <source> to Project…` from the palette: the first Project not
/// linking it yet, else New Project with that row.
pub fn add_to_project(source: &str) {
    let app = app();
    let ws = app.model.borrow().active_workspace().to_string();
    let projects = crate::navdata::projects(&app.model.borrow());
    match projects.iter().find(|p| !p.lists(source)) {
        Some(p) => add_source(&format!("{ws}/{}", p.id), Some(source)),
        None => {
            new_project();
            let _ = add(source, None);
        }
    }
}

/// Change base… of one link.
pub fn change_base(key: &str, source: &str) {
    let Some(p) = project_of(key) else { return };
    open(Kind::ChangeBase {
        project: p.id,
        source: source.to_string(),
    });
}

/// Project settings (Name and the link table, R106.3).
pub fn settings(key: &str) {
    let Some(p) = project_of(key) else {
        return crate::worktrees::alert("Project settings", "The Project is gone.");
    };
    if let Some(e) = &p.error {
        return crate::worktrees::alert("Project settings", e);
    }
    open(Kind::Settings { project: p.id });
}

fn open(kind: Kind) {
    let Some(scrim) = SCRIM.with(|s| s.borrow().clone()) else {
        return;
    };
    close();
    crate::palette::get().inspect(|p| p.close(false));
    let (ws, ws_name) = workspace();
    // The files as they are now (a link edit may have just landed).
    let projects = {
        let app = app();
        let m = app.model.borrow();
        let order = m
            .workspace(&ws)
            .map(|w| w.project_order.clone())
            .unwrap_or_default();
        rade_core::navigator::ordered(&crate::sidebar::projects(&m), &order)
    };
    let project = kind
        .project()
        .and_then(|id| projects.iter().find(|p| p.id == id).cloned());
    if kind.project().is_some() && project.is_none() {
        return crate::worktrees::alert("Project", "The Project is gone.");
    }
    let project_name = project.as_ref().map(|p| p.name.clone()).unwrap_or_default();

    let dialog = gtk::Box::new(gtk::Orientation::Vertical, 0);
    dialog.add_css_class("new-project-dialog");
    dialog.set_halign(gtk::Align::Center);
    dialog.set_valign(gtk::Align::Start);
    dialog.set_margin_top(92);
    // 680 px; at large text min(window − 32, 1000) px (spec C, R111.2).
    let dialog_w = dialog_width();
    dialog.set_size_request(dialog_w, -1);

    // Header (C.4–C.6).
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    header.add_css_class("dialog-header");
    header.append(&icon("ph-cube-symbolic", &["dialog-icon"]));
    let title = match &kind {
        Kind::NewProject => "New Project".to_string(),
        Kind::AddSource { .. } => format!("Add Source to {project_name}"),
        Kind::ChangeBase { source, .. } => {
            let name = app()
                .model
                .borrow()
                .source(source)
                .map_or(source.clone(), |s| s.name.clone());
            format!("Change base of {name} in {project_name}")
        }
        Kind::Settings { .. } => format!("{project_name} settings"),
    };
    header.append(&label(&title, &["dialog-title"]));
    let ctx = label(&format!("in {ws_name}"), &["dialog-context"]);
    ctx.set_hexpand(true);
    header.append(&ctx);
    if let Kind::Settings { project } = &kind {
        // The file itself, for what the form doesn't cover (R106.3).
        let menu = gio::Menu::new();
        menu.append(Some("Open project.toml"), Some("dlg.open-file"));
        menu.append(Some("Customize with Claude…"), Some("dlg.customize"));
        let more = gtk::MenuButton::new();
        more.set_icon_name("ph-dots-three-symbolic");
        more.add_css_class("flat");
        more.add_css_class("dialog-menu");
        more.set_tooltip_text(Some("More"));
        more.set_menu_model(Some(&menu));
        let group = gio::SimpleActionGroup::new();
        let key = format!("{ws}/{project}");
        for (name, f) in [
            ("open-file", crate::projects::open_file as fn(&str)),
            ("customize", crate::projects::customize),
        ] {
            let a = gio::SimpleAction::new(name, None);
            let k = key.clone();
            a.connect_activate(move |_, _| {
                close();
                f(&k);
            });
            group.add_action(&a);
        }
        more.insert_action_group("dlg", Some(&group));
        header.append(&more);
    }
    let x = gtk::Button::from_icon_name("ph-x-symbolic");
    x.add_css_class("flat");
    x.add_css_class("dialog-close");
    x.set_tooltip_text(Some("Close (Esc)"));
    x.connect_clicked(|_| close());
    header.append(&x);
    dialog.append(&header);

    // Body (C.7–C.22).
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.add_css_class("dialog-body");
    let name = gtk::Entry::new();
    name.add_css_class("project-name-entry");
    name.set_placeholder_text(Some("Project name"));
    let name_error = label("", &["field-error"]);
    name_error.set_visible(false);
    let with_name = matches!(kind, Kind::NewProject | Kind::Settings { .. });
    if with_name {
        body.append(&label("Name", &["field-label"]));
        body.append(&name);
        body.append(&name_error);
        if kind != Kind::NewProject {
            name.set_text(&project_name);
        }
    }
    let links_head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    links_head.add_css_class("field-row");
    let l = label("Sources and base branch", &["field-label"]);
    l.set_hexpand(true);
    links_head.append(&l);
    links_head.append(&label(
        "Each source gets one base. A source can be in many Projects.",
        &["field-hint"],
    ));
    body.append(&links_head);
    let editor = gtk::Box::new(gtk::Orientation::Vertical, 0);
    editor.add_css_class("link-editor");
    body.append(&editor);
    let add_row = gtk::Button::new();
    add_row.add_css_class("flat");
    add_row.add_css_class("link-add-row");
    let inner = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    inner.append(&icon("ph-plus-symbolic", &["link-add-icon"]));
    inner.append(&label("Add source…", &["link-add-label"]));
    let hint = gtk::Label::new(None);
    hint.set_markup(&format!(
        "from {}, a folder, or <tt>owner/repo</tt>",
        glib::markup_escape_text(&ws_name)
    ));
    hint.add_css_class("link-add-hint");
    inner.append(&hint);
    add_row.set_child(Some(&inner));
    add_row.set_visible(!matches!(kind, Kind::ChangeBase { .. }));
    editor.append(&add_row);
    dialog.append(&body);

    // Footer (C.23–C.26).
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.add_css_class("dialog-footer");
    let note = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    note.add_css_class("dialog-note");
    note.set_hexpand(true);
    note.append(&icon("ph-info-symbolic", &["dialog-note-icon"]));
    let nl = label(
        "RADE never switches a dirty checkout. It makes a Worktree when a base is taken.",
        &["dialog-note-text"],
    );
    nl.set_wrap(true);
    nl.set_max_width_chars(40);
    nl.set_hexpand(true);
    note.append(&nl);
    footer.append(&note);
    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("secondary-button");
    cancel.add_css_class("dialog-cancel");
    cancel.set_valign(gtk::Align::Center);
    cancel.connect_clicked(|_| close());
    footer.append(&cancel);
    let primary = gtk::Button::with_label(match &kind {
        Kind::NewProject => "Create Project",
        Kind::AddSource { .. } => "Add",
        Kind::ChangeBase { .. } => "Change base",
        Kind::Settings { .. } => "Save",
    });
    primary.add_css_class("suggested-action");
    primary.add_css_class("dialog-primary");
    primary.set_valign(gtk::Align::Center);
    footer.append(&primary);
    dialog.append(&footer);

    let d = Rc::new(Dialog {
        kind: kind.clone(),
        ws: ws.clone(),
        ws_name,
        project_name,
        name,
        name_error,
        editor,
        add_row,
        primary,
        rows: RefCell::default(),
        gen: Cell::new(0),
        planning: Cell::new(false),
        taken: projects
            .iter()
            .filter(|p| Some(p.id.as_str()) != kind.project())
            .flat_map(|p| [p.name.to_lowercase(), p.id.clone()])
            .collect(),
        added: RefCell::default(),
        saved: Cell::new(false),
    });
    // The Project's links as rows.
    if let Some(p) = &project {
        let names: Vec<(String, String)> = {
            let app = app();
            let m = app.model.borrow();
            p.sources
                .iter()
                .map(|s| {
                    let n = m
                        .source(&s.source)
                        .map_or(s.source.clone(), |x| x.name.clone());
                    (s.source.clone(), n)
                })
                .collect()
        };
        for (s, (_, n)) in p.sources.iter().zip(names) {
            if let Kind::ChangeBase { source, .. } = &kind {
                if *source != s.source {
                    continue;
                }
            }
            let base = s.base.clone().unwrap_or_default();
            d.rows.borrow_mut().push(Row {
                source: s.source.clone(),
                name: n,
                base: base.clone(),
                old_base: Some(base),
                checkout: Some(s.checkout.as_str().to_string()),
                include: true,
                existing: true,
                removed: false,
                plan: String::new(),
                plan_kind: String::new(),
                refused: None,
                widgets: None,
            });
        }
    }
    let w = Rc::downgrade(&d);
    d.name.connect_changed(move |_| {
        if let Some(d) = w.upgrade() {
            d.validate();
            if d.kind == Kind::NewProject {
                d.schedule_plan();
            }
        }
    });
    let w = Rc::downgrade(&d);
    d.add_row.connect_clicked(move |b| {
        if let Some(d) = w.upgrade() {
            d.add_menu(b);
        }
    });
    let w = Rc::downgrade(&d);
    d.primary.connect_clicked(move |_| {
        if let Some(d) = w.upgrade() {
            d.apply();
        }
    });
    let w = Rc::downgrade(&d);
    d.name.connect_activate(move |_| {
        if let Some(d) = w.upgrade() {
            if d.primary.is_sensitive() {
                d.apply();
            }
        }
    });

    while let Some(c) = scrim.first_child() {
        scrim.remove(&c);
    }
    if crate::layout_mode::mode().large_text {
        crate::layout_mode::wrap_labels(dialog.upcast_ref());
    }
    // 680 px wide (C.2): no label's natural width asks for more (they
    // wrap within short widths), the size request asks for that much.
    dialog.set_halign(gtk::Align::Fill);
    dialog.set_margin_top(0);
    let holder = libadwaita::Clamp::new();
    // px, not the default sp (which grows with the text scale).
    holder.set_unit(libadwaita::LengthUnit::Px);
    holder.set_maximum_size(dialog_w);
    holder.set_tightening_threshold(dialog_w);
    holder.set_valign(gtk::Align::Start);
    holder.set_margin_top(92);
    holder.set_child(Some(&dialog));
    holder.add_css_class("dialog-holder");
    scrim.append(&holder);
    scrim.set_visible(true);
    OPEN.with(|o| *o.borrow_mut() = Some(d.clone()));
    d.render_rows();
    d.validate();
    d.schedule_plan();
    // The scrim takes every click meant for the window under it.
    sync_primaries();
    match &kind {
        Kind::ChangeBase { .. } => {
            if let Some(w) = d.rows.borrow().first().and_then(|r| r.widgets.as_ref()) {
                w.base.grab_focus();
            }
        }
        Kind::AddSource { .. } => {
            d.add_row.grab_focus();
        }
        _ => {
            d.name.grab_focus();
        }
    }
}

fn sync_primaries() {
    crate::changes::sync_primary();
    crate::home_view::sync_primary();
}

/// Close the open dialog (Cancel, Esc, the `x`, after the primary).
pub fn close() {
    let Some(d) = OPEN.with(|o| o.borrow_mut().take()) else {
        return;
    };
    d.forget_added();
    if let Some(s) = SCRIM.with(|s| s.borrow().clone()) {
        s.set_visible(false);
        while let Some(c) = s.first_child() {
            s.remove(&c);
        }
    }
    sync_primaries();
    if let Some(app) = try_app() {
        app.restore_focus();
    }
}

/// Add a Source row (`source` = id or name; `base` = its base, else the
/// first Project base of that Source, else its main branch).
fn add(source: &str, base: Option<&str>) -> Result<()> {
    let d = OPEN
        .with(|o| o.borrow().clone())
        .ok_or_else(|| Error::usage("no dialog open"))?;
    d.add_source_row(source, base)
}

impl Dialog {
    fn add_source_row(self: &Rc<Self>, source: &str, base: Option<&str>) -> Result<()> {
        let (id, name, path) = {
            let app = app();
            let m = app.model.borrow();
            let s = m
                .find_source(source)
                .ok()
                .filter(|s| s.workspace == self.ws && !s.is_general())
                .ok_or_else(|| Error::not_found(format!("no source '{source}' in {}", self.ws)))?;
            (s.id.clone(), s.name.clone(), s.path.clone())
        };
        {
            let mut rows = self.rows.borrow_mut();
            if let Some(r) = rows.iter_mut().find(|r| r.source == id) {
                // Re-adding a removed link brings it back.
                r.removed = false;
                r.include = true;
                if let Some(b) = base {
                    r.base = b.to_string();
                }
                drop(rows);
                self.render_rows();
                self.schedule_plan();
                return Ok(());
            }
            let base = base.map(String::from).unwrap_or_else(|| {
                rade_core::git::default_branch(&path)
                    .or_else(|| rade_core::git::branch(&path))
                    .unwrap_or_else(|| "main".into())
            });
            rows.push(Row {
                source: id,
                name,
                base,
                old_base: None,
                checkout: None,
                include: true,
                existing: false,
                removed: false,
                plan: String::new(),
                plan_kind: String::new(),
                refused: None,
                widgets: None,
            });
        }
        self.render_rows();
        self.validate();
        self.schedule_plan();
        Ok(())
    }

    /// Rebuild the rows' widgets (the `Add source…` row stays last).
    fn render_rows(self: &Rc<Self>) {
        while let Some(c) = self.editor.first_child() {
            self.editor.remove(&c);
        }
        let n = self.rows.borrow().len();
        for i in 0..n {
            let w = self.row_widgets(i);
            self.editor.append(&w.root);
            self.rows.borrow_mut()[i].widgets = Some(w);
        }
        self.editor.append(&self.add_row);
        self.refresh_rows();
    }

    fn row_widgets(self: &Rc<Self>, i: usize) -> RowWidgets {
        let (source, name, base, existing, removed) = {
            let r = &self.rows.borrow()[i];
            (
                r.source.clone(),
                r.name.clone(),
                r.base.clone(),
                r.existing,
                r.removed,
            )
        };
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.add_css_class("link-editor-row");
        root.set_visible(!removed);
        // Large text: check, Source, base, remove on line 1; the plan on
        // line 2, full width (spec C "Text scale 200 %").
        let large = crate::layout_mode::mode().large_text;
        let line = if large {
            gtk::Box::new(gtk::Orientation::Horizontal, 8)
        } else {
            root.clone()
        };
        let check = gtk::Button::new();
        check.add_css_class("flat");
        check.add_css_class("link-check");
        check.add_css_class("link-editor-cell");
        check.set_valign(gtk::Align::Center);
        let read_only = existing && matches!(self.kind, Kind::AddSource { .. });
        check.set_sensitive(!read_only && !matches!(self.kind, Kind::ChangeBase { .. }));
        let w = Rc::downgrade(self);
        let s = source.clone();
        check.connect_clicked(move |_| {
            if let Some(d) = w.upgrade() {
                d.set_row(&s, |r| r.include = !r.include);
            }
        });
        line.append(&check);
        let src = label(&name, &["link-source", "link-editor-cell"]);
        src.set_ellipsize(gtk::pango::EllipsizeMode::End);
        src.set_size_request(150, -1);
        src.set_valign(gtk::Align::Center);
        if large {
            src.set_hexpand(true);
        }
        line.append(&src);
        let plan = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        plan.add_css_class("link-plan");
        plan.add_css_class("link-editor-cell");
        plan.set_hexpand(true);
        plan.set_valign(gtk::Align::Center);
        // One paragraph that wraps (C.15): the kind is a `fg2` 500 span, a
        // drifted HEAD a `wait` span (C.18); `plan_kind` names the kind.
        let plan_kind = label("", &["link-plan-kind"]);
        plan_kind.set_visible(false);
        let plan_text = label("", &["link-plan-detail"]);
        plan_text.set_wrap(true);
        plan_text.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        plan_text.set_hexpand(true);
        plan_text.set_max_width_chars(24);
        plan_text.set_valign(gtk::Align::Center);
        plan.append(&plan_kind);
        plan.append(&plan_text);
        if large {
            plan_text.set_max_width_chars(-1);
            plan.add_css_class("link-plan-second");
        } else {
            line.append(&plan);
        }
        let branches = gtk::StringList::new(&[&base]);
        let dd = gtk::DropDown::new(Some(branches.clone()), gtk::Expression::NONE);
        dd.add_css_class("link-base");
        dd.add_css_class("link-editor-cell");
        dd.set_size_request(190, -1);
        dd.set_valign(gtk::Align::Center);
        dd.set_sensitive(!read_only);
        let w = Rc::downgrade(self);
        let s = source.clone();
        dd.connect_selected_notify(move |dd| {
            let Some(d) = w.upgrade() else { return };
            let Some(b) = dd
                .selected_item()
                .and_downcast::<gtk::StringObject>()
                .map(|o| o.string().to_string())
            else {
                return;
            };
            let same = d.rows.borrow().iter().any(|r| r.source == s && r.base == b);
            if !same {
                d.set_row(&s, |r| r.base = b.clone());
            }
        });
        line.append(&dd);
        let rm = gtk::Button::from_icon_name("ph-x-symbolic");
        rm.add_css_class("flat");
        rm.add_css_class("link-remove");
        rm.add_css_class("link-editor-cell");
        rm.set_valign(gtk::Align::Center);
        rm.set_tooltip_text(Some("Remove"));
        rm.set_visible(!read_only && !matches!(self.kind, Kind::ChangeBase { .. }));
        let w = Rc::downgrade(self);
        let s = source.clone();
        rm.connect_clicked(move |_| {
            if let Some(d) = w.upgrade() {
                d.remove_row(&s);
            }
        });
        // A folder that is not a git repository (R131.2).
        let init = gtk::Button::with_label("Initialize");
        init.add_css_class("link-init");
        init.add_css_class("link-editor-cell");
        init.set_valign(gtk::Align::Center);
        init.set_visible(false);
        let w = Rc::downgrade(self);
        let s = source.clone();
        init.connect_clicked(move |_| {
            if let Some(d) = w.upgrade() {
                d.ask_init(&s);
            }
        });
        plan.append(&init);
        line.append(&rm);
        if large {
            root.set_orientation(gtk::Orientation::Vertical);
            root.add_css_class("link-editor-row-two-line");
            root.append(&line);
            root.append(&plan);
        }
        // The Source's local and remote branches, off the GTK thread.
        let path = app().model.borrow().source(&source).map(|s| s.path.clone());
        if let Some(path) = path {
            let (bl, dd2, keep) = (branches.clone(), dd.clone(), base.clone());
            crate::worktrees::in_background(
                move || branch_names(&path),
                move |names| {
                    let mut all = vec![keep.clone()];
                    all.extend(names.into_iter().filter(|n| *n != keep));
                    let refs: Vec<&str> = all.iter().map(String::as_str).collect();
                    bl.splice(0, bl.n_items(), &refs);
                    dd2.set_selected(0);
                },
            );
        }
        RowWidgets {
            root,
            check,
            base: dd,
            branches,
            plan_kind,
            plan: plan_text,
            init,
        }
    }

    fn set_row(self: &Rc<Self>, source: &str, f: impl FnOnce(&mut Row)) {
        if let Some(r) = self
            .rows
            .borrow_mut()
            .iter_mut()
            .find(|r| r.source == source)
        {
            f(r);
        }
        self.refresh_rows();
        self.validate();
        self.schedule_plan();
    }

    fn remove_row(self: &Rc<Self>, source: &str) {
        {
            let mut rows = self.rows.borrow_mut();
            let Some(i) = rows.iter().position(|r| r.source == source) else {
                return;
            };
            if rows[i].existing {
                rows[i].removed = true;
                if let Some(w) = &rows[i].widgets {
                    w.root.set_visible(false);
                }
            } else {
                let r = rows.remove(i);
                if let Some(w) = r.widgets {
                    self.editor.remove(&w.root);
                }
            }
        }
        self.refresh_rows();
        self.validate();
        self.schedule_plan();
    }

    /// Check icons, plan texts, the base select's choice, the `x` on the
    /// last row of Project settings.
    fn refresh_rows(&self) {
        let rows = self.rows.borrow();
        let shown = rows.iter().filter(|r| !r.removed).count();
        for r in rows.iter() {
            let Some(w) = &r.widgets else { continue };
            w.check.set_icon_name(if r.include {
                "ph-check-square-fill-symbolic"
            } else {
                "ph-square-symbolic"
            });
            if r.include {
                w.check.add_css_class("checked");
            } else {
                w.check.remove_css_class("checked");
            }
            w.base.set_sensitive(
                r.include && !(r.existing && matches!(self.kind, Kind::AddSource { .. })),
            );
            w.init.set_visible(
                r.include && r.refused.as_deref().is_some_and(|why| why.contains(NOT_GIT)),
            );
            match &r.refused {
                Some(why) if why.contains(NOT_GIT) => {
                    w.plan_kind.set_label("");
                    w.plan_kind.set_visible(false);
                    w.plan.set_label("Not a git repository");
                    w.plan.add_css_class("refused");
                }
                Some(why) => {
                    w.plan_kind.set_label("");
                    w.plan_kind.set_visible(false);
                    w.plan.set_label(why);
                    w.plan.add_css_class("refused");
                }
                None => {
                    w.plan.remove_css_class("refused");
                    w.plan_kind.set_label(&r.plan_kind);
                    w.plan.set_markup(&plan_markup(&r.plan, &r.plan_kind));
                }
            }
            // A drifted main checkout's HEAD in `wait` (C.18).
            if r.plan.contains("main checkout is on ") {
                w.plan.add_css_class("drift");
            } else {
                w.plan.remove_css_class("drift");
            }
            let n = w.branches.n_items();
            let at = (0..n).find(|i| w.branches.string(*i).is_some_and(|s| s == r.base));
            match at {
                Some(i) if w.base.selected() != i => w.base.set_selected(i),
                Some(_) => {}
                None => {
                    w.branches.append(&r.base);
                    w.base.set_selected(n);
                }
            }
            if let Some(rm) = w.root.last_child() {
                if matches!(self.kind, Kind::Settings { .. }) {
                    rm.set_visible(shown > 1);
                }
            }
        }
    }

    /// Name check (C.26 disabled state) and the primary's sensitivity.
    fn validate(&self) {
        let name = self.name.text().trim().to_string();
        let mut error = None;
        let with_name = matches!(self.kind, Kind::NewProject | Kind::Settings { .. });
        if with_name && !name.is_empty() {
            let id = rade_core::model::slug(&name);
            if self.taken.contains(&name.to_lowercase()) || self.taken.contains(&id) {
                error = Some(format!("A Project named {name} exists in {}", self.ws_name));
            }
        }
        self.name_error
            .set_label(error.as_deref().unwrap_or_default());
        self.name_error.set_visible(error.is_some());
        let rows = self.rows.borrow();
        // A refused checkout blocks adding that link (New Project, Add
        // Source); Change base and settings show it and save anyway (the
        // link edit itself never touches a checkout).
        let refused = matches!(self.kind, Kind::NewProject | Kind::AddSource { .. })
            && rows
                .iter()
                .any(|r| r.include && !r.removed && !r.existing && r.refused.is_some());
        let changed = match &self.kind {
            Kind::NewProject => true,
            Kind::AddSource { .. } => rows.iter().any(|r| !r.existing && r.include),
            Kind::ChangeBase { .. } => rows
                .first()
                .is_some_and(|r| Some(&r.base) != r.old_base.as_ref() && !r.base.is_empty()),
            Kind::Settings { .. } => true,
        };
        let ok = (!with_name || !name.is_empty()) && error.is_none() && !refused && changed;
        self.primary.set_sensitive(ok);
    }

    /// The links `project.plan` sees: the included, not removed rows.
    fn plan_sources(&self) -> Vec<Value> {
        self.rows
            .borrow()
            .iter()
            .filter(|r| r.include && !r.removed)
            .map(|r| {
                let mut v = json!({"source": r.source, "base": r.base});
                if let Some(c) = r.checkout.as_ref().filter(|c| *c != "auto") {
                    v["checkout"] = c.clone().into();
                }
                v
            })
            .collect()
    }

    /// Ask `project.plan` again (50 ms after the last change; one at a time).
    fn schedule_plan(self: &Rc<Self>) {
        let gen = self.gen.get() + 1;
        self.gen.set(gen);
        let w = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(50), move || {
            let Some(d) = w.upgrade() else { return };
            if d.gen.get() != gen || !is_open() {
                return;
            }
            d.planning.set(true);
            let name = match &d.kind {
                Kind::NewProject => {
                    let n = d.name.text().trim().to_string();
                    if n.is_empty() {
                        "new project".to_string()
                    } else {
                        n
                    }
                }
                _ => d.project_name.clone(),
            };
            let mut params = json!({"name": name, "workspace": d.ws, "sources": d.plan_sources()});
            if let Some(p) = d.kind.project() {
                params["id"] = p.into();
            }
            let w = Rc::downgrade(&d);
            crate::worktrees::in_background(
                move || crate::projects::dispatch("project.plan", params),
                move |r| {
                    let Some(d) = w.upgrade() else { return };
                    if d.gen.get() != gen {
                        return;
                    }
                    d.planning.set(false);
                    if let Ok(v) = r {
                        d.take_plan(&v);
                    }
                },
            );
        });
    }

    fn take_plan(&self, v: &Value) {
        {
            let mut rows = self.rows.borrow_mut();
            for l in v["links"].as_array().into_iter().flatten() {
                let Some(r) = rows
                    .iter_mut()
                    .find(|r| Some(r.source.as_str()) == l["source"].as_str())
                else {
                    continue;
                };
                r.plan = l["plan"].as_str().unwrap_or_default().to_string();
                r.plan_kind = l["plan_kind"].as_str().unwrap_or_default().to_string();
                r.refused = l["refused"].as_str().map(String::from);
            }
            for r in rows.iter_mut().filter(|r| !r.include || r.removed) {
                r.plan.clear();
                r.plan_kind.clear();
                r.refused = None;
            }
        }
        self.refresh_rows();
        self.validate();
    }

    /// `Add source…`: the Workspace's Sources not listed, a folder, or
    /// `owner/repo`.
    fn add_menu(self: &Rc<Self>, at: &gtk::Button) {
        let pop = gtk::Popover::new();
        pop.add_css_class("menu");
        let list = gtk::Box::new(gtk::Orientation::Vertical, 2);
        list.add_css_class("link-add-menu");
        let listed: Vec<String> = self
            .rows
            .borrow()
            .iter()
            .filter(|r| !r.removed)
            .map(|r| r.source.clone())
            .collect();
        let sources: Vec<(String, String)> = {
            let app = app();
            let m = app.model.borrow();
            m.sources
                .iter()
                .filter(|s| s.workspace == self.ws && !s.is_general())
                .filter(|s| !listed.contains(&s.id))
                .map(|s| (s.id.clone(), s.name.clone()))
                .collect()
        };
        if sources.is_empty() {
            list.append(&label(
                &format!("No other sources in {}", self.ws_name),
                &["dim-label"],
            ));
        }
        for (id, name) in sources {
            let b = gtk::Button::with_label(&name);
            b.add_css_class("flat");
            let (w, p) = (Rc::downgrade(self), pop.clone());
            b.connect_clicked(move |_| {
                p.popdown();
                if let Some(d) = w.upgrade() {
                    let _ = d.add_source_row(&id, None);
                }
            });
            list.append(&b);
        }
        list.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let folder = gtk::Button::with_label("A folder…");
        folder.add_css_class("flat");
        let (w, p) = (Rc::downgrade(self), pop.clone());
        folder.connect_clicked(move |_| {
            p.popdown();
            let w = w.clone();
            let fd = gtk::FileDialog::new();
            fd.set_title("Add a folder as a Source");
            fd.select_folder(Some(&app().window), gio::Cancellable::NONE, move |r| {
                let (Some(d), Ok(f)) = (w.upgrade(), r) else {
                    return;
                };
                let Some(path) = f.path() else { return };
                d.add_folder(&path);
            });
        });
        list.append(&folder);
        let repo = gtk::Entry::new();
        repo.set_placeholder_text(Some("owner/repo"));
        repo.add_css_class("link-add-repo");
        let (w, p) = (Rc::downgrade(self), pop.clone());
        repo.connect_activate(move |e| {
            p.popdown();
            if let Some(d) = w.upgrade() {
                d.add_repo(e.text().trim());
            }
        });
        list.append(&repo);
        pop.set_child(Some(&list));
        pop.set_parent(at);
        pop.connect_closed(|p| p.unparent());
        pop.popup();
    }

    /// A folder: added to the Workspace as a Source, then as a row.
    fn add_folder(self: &Rc<Self>, path: &std::path::Path) {
        let app = app();
        let ws = self.ws.clone();
        let r = app.update(|m| m.add_source_in(path, None, Some(&ws)));
        match r {
            Ok(id) => {
                app.save_now();
                self.added.borrow_mut().push(id.clone());
                let _ = self.add_source_row(&id, None);
            }
            Err(e) => crate::worktrees::alert("Could not add the folder", &e.message),
        }
    }

    /// `owner/repo`: cloned with `rade source add` under the Workspace's
    /// env (gh), then a row.
    fn add_repo(self: &Rc<Self>, repo: &str) {
        if repo.split('/').count() != 2 {
            return crate::worktrees::alert("Add source", "Type owner/repo.");
        }
        let rade = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join("rade")))
            .filter(|p| p.exists())
            .unwrap_or_else(|| "rade".into());
        let (ws, repo) = (self.ws.clone(), repo.to_string());
        let w = Rc::downgrade(self);
        crate::worktrees::in_background(
            move || {
                std::process::Command::new(rade)
                    .args(["source", "add", &repo, "--workspace", &ws, "--json"])
                    .stdin(std::process::Stdio::null())
                    .output()
            },
            move |out| {
                let Some(d) = w.upgrade() else { return };
                match out {
                    Ok(o) if o.status.success() => {
                        let v: Value = serde_json::from_slice(&o.stdout).unwrap_or_default();
                        let id = v["source"]
                            .as_str()
                            .or(v["id"].as_str())
                            .or(v["name"].as_str())
                            .unwrap_or_default()
                            .to_string();
                        d.added.borrow_mut().push(id.clone());
                        let _ = d.add_source_row(&id, None);
                    }
                    Ok(o) => crate::worktrees::alert(
                        "Could not clone",
                        String::from_utf8_lossy(&o.stderr).trim(),
                    ),
                    Err(e) => crate::worktrees::alert("Could not clone", &e.to_string()),
                }
            },
        );
    }

    /// The primary button: write through the socket methods (off the GTK
    /// thread), close on success, else say why.
    fn apply(self: &Rc<Self>) {
        self.validate();
        if !self.primary.is_sensitive() {
            return;
        }
        let ws = self.ws.clone();
        let mut calls: Vec<(&'static str, Value)> = Vec::new();
        let rows = self.rows.borrow();
        match &self.kind {
            Kind::NewProject => {
                let sources: Vec<Value> = rows
                    .iter()
                    .filter(|r| r.include && !r.removed)
                    .map(|r| json!({"source": r.source, "base": r.base, "checkout": "auto"}))
                    .collect();
                calls.push((
                    "project.add",
                    json!({"name": self.name.text().trim(), "workspace": ws, "sources": sources}),
                ));
            }
            Kind::AddSource { project } => {
                for r in rows
                    .iter()
                    .filter(|r| !r.existing && r.include && !r.removed)
                {
                    calls.push((
                        "project.link_add",
                        json!({"project": project, "source": r.source, "base": r.base}),
                    ));
                }
            }
            Kind::ChangeBase { project, source } => {
                if let Some(r) = rows.first() {
                    calls.push((
                        "project.link_set_base",
                        json!({"project": project, "source": source, "base": r.base}),
                    ));
                }
            }
            Kind::Settings { project } => {
                let name = self.name.text().trim().to_string();
                if name != self.project_name {
                    calls.push((
                        "project.set",
                        json!({"project": project, "key": "name", "value": name}),
                    ));
                }
                for r in rows.iter() {
                    let p = project.clone();
                    match (r.existing, r.removed || !r.include) {
                        (true, true) => calls.push((
                            "project.link_remove",
                            json!({"project": p, "source": r.source}),
                        )),
                        (true, false) if Some(&r.base) != r.old_base.as_ref() => calls.push((
                            "project.link_set_base",
                            json!({"project": p, "source": r.source, "base": r.base}),
                        )),
                        (false, false) => calls.push((
                            "project.link_add",
                            json!({"project": p, "source": r.source, "base": r.base}),
                        )),
                        _ => {}
                    }
                }
            }
        }
        drop(rows);
        let workspace = ws.clone();
        self.primary.set_sensitive(false);
        let w = Rc::downgrade(self);
        crate::worktrees::in_background(
            move || {
                for (m, p) in calls {
                    if m == "project.set" {
                        crate::project_settings::dispatch(p)?;
                    } else {
                        crate::projects::dispatch(m, p)?;
                    }
                }
                Ok::<_, Error>(workspace)
            },
            move |r| match r {
                Ok(_) => {
                    if let Some(d) = w.upgrade() {
                        d.saved.set(true);
                    }
                    let opened = w.upgrade().map(|d| d.kind.clone());
                    close();
                    crate::navdata::refresh();
                    // A new Project: show its home.
                    if opened == Some(Kind::NewProject) {
                        crate::sidebar::projects_changed();
                    }
                }
                Err(e) => {
                    if let Some(d) = w.upgrade() {
                        d.validate();
                    }
                    crate::worktrees::alert("Could not save", &e.message);
                }
            },
        );
    }

    /// Forget the Sources this dialog registered that no saved link uses
    /// (Cancel, or a row removed before Create; R131.1).
    fn forget_added(&self) {
        let added = self.added.take();
        if added.is_empty() {
            return;
        }
        let kept: Vec<String> = if self.saved.get() {
            self.rows
                .borrow()
                .iter()
                .filter(|r| r.include && !r.removed)
                .map(|r| r.source.clone())
                .collect()
        } else {
            Vec::new()
        };
        let Some(app) = try_app() else { return };
        for id in added.iter().filter(|id| !kept.contains(id)) {
            let _ = app.update(|m| m.remove_source(id).map(|_| ()));
        }
        app.save_now();
    }

    /// `Initialize`: one confirmation, then `git init -b <base>` only — no
    /// `git add`, no commit (R131.2).
    fn ask_init(self: &Rc<Self>, source: &str) {
        let Some((path, base)) = self.init_target(source) else {
            return;
        };
        let w = Rc::downgrade(self);
        let s = source.to_string();
        crate::changes_ops::confirm(
            "Initialize a git repository?",
            &format!(
                "Runs git init -b {base} in {}.\nNo file is added or committed.",
                path.display()
            ),
            "init",
            "Initialize",
            move || {
                if let Some(d) = w.upgrade() {
                    d.init(&s);
                }
            },
        );
    }

    fn init_target(&self, source: &str) -> Option<(std::path::PathBuf, String)> {
        let base = self
            .rows
            .borrow()
            .iter()
            .find(|r| r.source == source)?
            .base
            .clone();
        let path = app().model.borrow().source(source)?.path.clone();
        Some((path, base))
    }

    fn init(self: &Rc<Self>, source: &str) {
        let Some((path, base)) = self.init_target(source) else {
            return;
        };
        let w = Rc::downgrade(self);
        crate::worktrees::in_background(
            move || rade_core::git::init(&path, &base),
            move |r| {
                let Some(d) = w.upgrade() else { return };
                match r {
                    Ok(()) => d.schedule_plan(),
                    Err(e) => crate::worktrees::alert("Could not initialize", &e.message),
                }
            },
        );
    }

    /// `debug.ui dialog`: what the dialog shows.
    fn info(&self) -> Value {
        let rows: Vec<Value> = self
            .rows
            .borrow()
            .iter()
            .filter(|r| !r.removed)
            .map(|r| {
                json!({"source": r.source, "name": r.name, "base": r.base,
                       "include": r.include, "existing": r.existing,
                       "checkout": r.checkout, "plan": r.plan, "plan_kind": r.plan_kind,
                       "plan_markup": plan_markup(&r.plan, &r.plan_kind),
                       "refused": r.refused,
                       "init": r.include
                           && r.refused.as_deref().is_some_and(|w| w.contains(NOT_GIT))})
            })
            .collect();
        let focus = try_app()
            .and_then(|a| gtk::prelude::GtkWindowExt::focus(&a.window))
            .and_then(|f| {
                ["project-name-entry", "link-base", "link-add-row"]
                    .into_iter()
                    .find(|c| {
                        std::iter::successors(Some(f.clone()), |w| w.parent())
                            .any(|w| w.has_css_class(c))
                    })
            });
        let buttons = ["Cancel", self.primary.label().unwrap_or_default().as_str()]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        json!({
            "open": true, "kind": self.kind.as_str(), "project": self.kind.project(),
            "title": match &self.kind {
                Kind::NewProject => "New Project".to_string(),
                Kind::AddSource { .. } => format!("Add Source to {}", self.project_name),
                Kind::ChangeBase { .. } => format!("Change base in {}", self.project_name),
                Kind::Settings { .. } => format!("{} settings", self.project_name),
            },
            "name": self.name.text().as_str(),
            "name_error": self.name_error.is_visible().then(|| self.name_error.label().to_string()),
            "rows": rows, "buttons": buttons,
            "primary_enabled": self.primary.is_sensitive(),
            "planning": self.planning.get(), "focus": focus,
        })
    }
}

/// The plan line as markup: the kind in `fg2` 500, a drifted main
/// checkout's branch (`main checkout is on <branch>`) in `wait` (C.18).
fn plan_markup(plan: &str, kind: &str) -> String {
    let esc = |s: &str| glib::markup_escape_text(s).to_string();
    let rest = plan.strip_prefix(kind).unwrap_or(plan);
    let rest = match rest.split_once("main checkout is on ") {
        Some((a, b)) => {
            let (branch, tail) = b.split_once(' ').unwrap_or((b, ""));
            let tail = if tail.is_empty() {
                String::new()
            } else {
                format!(" {}", esc(tail))
            };
            format!(
                "{}main checkout is on <span foreground=\"{}\">{}</span>{tail}",
                esc(a),
                crate::tokens::rgb_now("wait"),
                esc(branch)
            )
        }
        None => esc(rest),
    };
    if kind.is_empty() || !plan.starts_with(kind) {
        return rest;
    }
    format!(
        "<span weight=\"500\" foreground=\"{}\">{}</span>{rest}",
        crate::tokens::rgb_now("fg2"),
        esc(kind)
    )
}

/// Local and remote branch names of a repository (remote ones without
/// their `origin/` prefix), each once, local first.
fn branch_names(path: &std::path::Path) -> Vec<String> {
    let out = rade_core::git::run(
        path,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes",
        ],
    )
    .unwrap_or_default();
    let mut names: Vec<String> = Vec::new();
    for l in out.lines() {
        let n = if let Some(b) = l.strip_prefix("refs/heads/") {
            b.to_string()
        } else if let Some(r) = l.strip_prefix("refs/remotes/") {
            match r.split_once('/') {
                Some((_, b)) if b != "HEAD" => b.to_string(),
                _ => continue,
            }
        } else {
            continue;
        };
        if !names.contains(&n) {
            names.push(n);
        }
    }
    names
}

/// `debug.ui dialog…` ops (E2E): `dialog` (what is shown), `dialog_open`
/// (`kind` new_project | add_source | change_base | settings, `project`,
/// `source`), `dialog_name` (`text`), `dialog_add` (`source`, `base`?),
/// `dialog_row` (`source`, `base`?, `include`?, `remove`?),
/// `dialog_primary`, `dialog_cancel`. Answers `{"dialog": …}`.
pub fn debug(p: &Value) -> Result<Value> {
    let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("");
    let ws = app().model.borrow().active_workspace().to_string();
    let key = format!("{ws}/{}", s("project"));
    match s("op") {
        "dialog" => {}
        "dialog_open" => match s("kind") {
            "new_project" => new_project(),
            "add_source" => add_source(&key, None),
            "change_base" => change_base(&key, s("source")),
            "settings" => settings(&key),
            k => return Err(Error::usage(format!("unknown dialog kind '{k}'"))),
        },
        "dialog_name" => {
            let d = open_dialog()?;
            d.name.set_text(s("text"));
        }
        "dialog_add" => add(s("source"), p.get("base").and_then(Value::as_str))?,
        "dialog_row" => {
            let d = open_dialog()?;
            let id = {
                let app = app();
                let m = app.model.borrow();
                m.find_source(s("source"))
                    .map(|x| x.id.clone())
                    .unwrap_or_else(|_| s("source").to_string())
            };
            if !d.rows.borrow().iter().any(|r| r.source == id) {
                return Err(Error::not_found(format!("no row '{}'", s("source"))));
            }
            if p.get("remove").and_then(Value::as_bool) == Some(true) {
                d.remove_row(&id);
            } else {
                let base = p.get("base").and_then(Value::as_str).map(String::from);
                let include = p.get("include").and_then(Value::as_bool);
                d.set_row(&id, |r| {
                    if let Some(b) = base {
                        r.base = b;
                    }
                    if let Some(i) = include {
                        r.include = i;
                    }
                });
            }
        }
        "dialog_primary" => {
            let d = open_dialog()?;
            if !d.primary.is_sensitive() {
                return Err(Error::usage("the primary button is disabled"));
            }
            d.primary.emit_clicked();
        }
        "dialog_cancel" => close(),
        "dialog_folder" => open_dialog()?.add_folder(std::path::Path::new(s("path"))),
        "dialog_init" => {
            let d = open_dialog()?;
            let id = app()
                .model
                .borrow()
                .find_source(s("source"))
                .map(|x| x.id.clone())?;
            d.ask_init(&id);
        }
        op => return Err(Error::usage(format!("unknown debug.ui op '{op}'"))),
    }
    let v = OPEN
        .with(|o| o.borrow().clone())
        .map_or(json!({"open": false}), |d| d.info());
    Ok(json!({ "dialog": v }))
}

fn open_dialog() -> Result<Rc<Dialog>> {
    OPEN.with(|o| o.borrow().clone())
        .ok_or_else(|| Error::usage("no dialog open"))
}

/// `debug.ui settings`: the open Project settings' fields (R106-f): the
/// Name and one row per link (`source`, `base`, `checkout`, Remove).
pub fn settings_info() -> Value {
    let Some(d) = OPEN
        .with(|o| o.borrow().clone())
        .filter(|d| matches!(d.kind, Kind::Settings { .. }))
    else {
        return json!({"open": false, "fields": [], "links": []});
    };
    let links: Vec<Value> = d
        .rows
        .borrow()
        .iter()
        .filter(|r| !r.removed)
        .map(|r| {
            let rm = r
                .widgets
                .as_ref()
                .and_then(|w| w.root.last_child())
                .is_some_and(|b| b.is_visible());
            json!({"source": r.source, "base": r.base, "checkout": r.checkout, "remove": rm})
        })
        .collect();
    json!({
        "open": true,
        "project": d.kind.project(),
        "fields": [{"key": "name", "value": d.name.text().as_str()}],
        "links": links,
        "menu": ["Open project.toml", "Customize with Claude…"],
    })
}

/// The dialog's width (spec C.2 "1280 px" keeps 680 px; at large text
/// min(window − 32, 1000) px).
fn dialog_width() -> i32 {
    match crate::layout_mode::window_width() {
        w if w > 0 => rade_core::chrome::overlay_width(w, 680, 1000, crate::layout_mode::mode()),
        _ => 680,
    }
}
