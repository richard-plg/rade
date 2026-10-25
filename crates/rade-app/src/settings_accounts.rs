//! Settings → Accounts (PRD R126, ADR-0047): a list of the Agent Accounts
//! the shown Workspace may use, and the selected one's detail — header,
//! Default, a menu, and one Environment table in the order an Agent gets it
//! (the login row `CLAUDE_CONFIG_DIR` / `CODEX_HOME`, the `.env` file and
//! its keys, then the account's own variables). User accounts
//! (`accounts.toml`) are edited in place, each change one
//! `rade_core::account::edit_in_toml` write; a Workspace's own accounts are
//! read-only (R112.6). Only the list and the detail are rebuilt after a
//! change, so the page keeps its scroll position and the cursor goes where
//! the work continues (R126.4). An env file's values are never read here.

use crate::app::app;
use adw::prelude::*;
use gtk::{gio, glib};
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::account::{self, AgentKind, Edit};
use rade_core::rpc::{Error, Result};
use rade_core::settings_accounts::{self as sa, Card, EnvFileState, EnvSource};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

thread_local! {
    static PANE: RefCell<Option<Rc<Pane>>> = const { RefCell::new(None) };
}

/// Where the cursor goes after the next rebuild.
#[derive(Clone, Copy, PartialEq)]
enum Focus {
    None,
    NewVarKey,
    DraftName,
    AddVariable,
}

/// The account being drafted by + New account (nothing written yet).
#[derive(Clone, Default)]
struct Draft {
    agent: Option<AgentKind>,
    name: String,
    dir: String,
}

struct Pane {
    root: gtk::Box,
    list: gtk::ListBox,
    detail: gtk::Box,
    query: RefCell<String>,
    selected: RefCell<Option<(AgentKind, String)>>,
    draft: RefCell<Option<Draft>>,
    /// The + Add variable row being typed (key, value), kept over a refused write.
    new_var: RefCell<Option<(String, String)>>,
    /// The last write's error, shown in the detail.
    error: RefCell<Option<String>>,
    focus: RefCell<Focus>,
    /// The widget to focus after a rebuild.
    focus_widget: RefCell<Option<gtk::Widget>>,
    saved: RefCell<Option<gtk::Label>>,
    cards: RefCell<Vec<Card>>,
    list_error: RefCell<Option<String>>,
}

fn home() -> PathBuf {
    rade_core::paths::home_dir(&rade_core::paths::process_env)
}

/// The cards for the shown Workspace, and an error to show (a broken
/// `accounts.toml` or `workspace.toml`).
pub fn cards() -> (Vec<Card>, Option<String>) {
    let home = home();
    let (id, root) = {
        let app = app();
        let m = app.model.borrow();
        let id = m.active_workspace().to_string();
        let root = m.workspace(&id).map(|w| w.root.clone());
        (id, root)
    };
    let mut errors = Vec::new();
    let user = account::user_accounts(&crate::accounts::user_file(), &home).unwrap_or_else(|e| {
        errors.push(e.message);
        vec![]
    });
    let (own, own_only) = match &root {
        Some(r) => {
            let own_only = rade_core::workspace::WorkspaceFile::read(r)
                .ok()
                .and_then(|f| f.accounts)
                .is_some_and(|a| a.trim() == "own");
            let own = account::workspace_accounts(&id, r, None, &home).unwrap_or_else(|e| {
                errors.push(e.message);
                vec![]
            });
            (own, own_only)
        }
        None => (vec![], false),
    };
    let err = (!errors.is_empty()).then(|| errors.join("\n"));
    let mut cards = sa::cards(&user, &own, own_only, &home);
    // The ✓ is the default this Workspace really uses (R125.3).
    if let Some(r) = &root {
        if let Ok(merged) =
            account::workspace_accounts(&id, r, Some(&crate::accounts::user_file()), &home)
        {
            for c in &mut cards {
                c.effective_default = merged
                    .iter()
                    .any(|a| a.agent == c.agent && a.name == c.name && a.default);
            }
        }
    }
    (cards, err)
}

fn roots() -> Vec<(String, PathBuf)> {
    app()
        .model
        .borrow()
        .workspaces
        .iter()
        .map(|w| (w.id.clone(), w.root.clone()))
        .collect()
}

/// Several changes of one user account, in one write.
pub fn edit_many(agent: AgentKind, name: &str, edits: &[Edit]) -> Result<()> {
    account::edit_user_file(&crate::accounts::user_file(), |t| {
        let mut t = t.to_string();
        for e in edits {
            t = account::edit_in_toml(&t, agent, name, e)?;
        }
        Ok(t)
    })?;
    crate::accounts::forget_lists();
    Ok(())
}

/// A new user account; a name a Workspace taking user accounts declares
/// is refused before writing (R125.3).
pub fn add(decl: &account::AccountDecl) -> Result<()> {
    let roots = roots();
    account::edit_user_file(&crate::accounts::user_file(), |t| {
        let new = account::add_to_toml(t, decl)?;
        account::check_user_text(&new, &roots)?;
        Ok(new)
    })?;
    crate::accounts::forget_lists();
    Ok(())
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

fn tilde(p: &Path) -> String {
    match p.strip_prefix(home()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

fn pane() -> Option<Rc<Pane>> {
    PANE.with(|p| p.borrow().clone())
}

/// The category matches the search (its name, or an account).
pub fn matches_query(query: &str) -> bool {
    let named = query
        .split_whitespace()
        .all(|w| sa::CATEGORY.to_lowercase().contains(&w.to_lowercase()));
    named || cards().0.iter().any(|c| sa::matches(c, query))
}

/// The Accounts section for the page (built once per open page, then
/// refreshed): the list beside the detail, or above it when narrow.
pub fn section(query: &str) -> gtk::Widget {
    let p = match pane() {
        Some(p) => p,
        None => {
            let wide = app().window.width() >= 1000;
            let root = gtk::Box::new(
                if wide {
                    gtk::Orientation::Horizontal
                } else {
                    gtk::Orientation::Vertical
                },
                0,
            );
            root.add_css_class("settings-accounts");
            let side = gtk::Box::new(gtk::Orientation::Vertical, 0);
            side.add_css_class("settings-accounts-side");
            if wide {
                side.set_size_request(236, -1);
            }
            let list = gtk::ListBox::new();
            list.add_css_class("settings-accounts-list");
            list.set_selection_mode(gtk::SelectionMode::Single);
            side.append(&list);
            let new = gtk::Button::with_label("+ New account");
            new.add_css_class("flat");
            new.add_css_class("settings-accounts-new");
            new.connect_clicked(|_| start_draft());
            side.append(&new);
            root.append(&side);
            let detail = gtk::Box::new(gtk::Orientation::Vertical, 8);
            detail.add_css_class("settings-account-detail");
            detail.set_hexpand(true);
            root.append(&detail);
            list.connect_row_activated(|_, row| {
                let name = row.widget_name().to_string();
                let Some(rest) = name.strip_prefix("account:") else {
                    return;
                };
                let Some((a, n)) = rest.split_once(':') else {
                    return;
                };
                let Some(p) = pane() else { return };
                if let Some(agent) = AgentKind::parse(a) {
                    *p.selected.borrow_mut() = Some((agent, n.to_string()));
                    *p.draft.borrow_mut() = None;
                    *p.new_var.borrow_mut() = None;
                    *p.error.borrow_mut() = None;
                    refresh();
                }
            });
            let p = Rc::new(Pane {
                root,
                list,
                detail,
                query: RefCell::new(String::new()),
                selected: RefCell::new(None),
                draft: RefCell::new(None),
                new_var: RefCell::new(None),
                error: RefCell::new(None),
                focus: RefCell::new(Focus::None),
                focus_widget: RefCell::new(None),
                saved: RefCell::new(None),
                cards: RefCell::new(vec![]),
                list_error: RefCell::new(None),
            });
            PANE.with(|x| *x.borrow_mut() = Some(p.clone()));
            p
        }
    };
    *p.query.borrow_mut() = query.to_string();
    if let Some(parent) = p.root.parent() {
        if let Some(b) = parent.downcast_ref::<gtk::Box>() {
            b.remove(&p.root);
        }
    }
    rebuild(&p);
    p.root.clone().upcast()
}

/// The page closed: drop the section (the next open builds it again).
pub fn forget() {
    PANE.with(|p| *p.borrow_mut() = None);
}

/// Esc while drafting an account or a variable drops that draft (instead
/// of closing Settings). True when it did.
pub fn escape() -> bool {
    let Some(p) = pane() else { return false };
    let inside =
        gtk::prelude::GtkWindowExt::focus(&app().window).is_some_and(|f| f.is_ancestor(&p.detail));
    if !inside {
        return false;
    }
    drop_draft(&p)
}

fn drop_draft(p: &Rc<Pane>) -> bool {
    if p.new_var.borrow_mut().take().is_some() {
        *p.error.borrow_mut() = None;
        *p.focus.borrow_mut() = Focus::AddVariable;
        refresh();
        return true;
    }
    if p.draft.borrow_mut().take().is_some() {
        *p.error.borrow_mut() = None;
        refresh();
        return true;
    }
    false
}

/// Rebuild the list and the detail, keeping the page's scroll position.
fn refresh() {
    let Some(p) = pane() else { return };
    let scroll = crate::settings_view::scroll_value();
    rebuild(&p);
    if let Some(v) = scroll {
        crate::settings_view::set_scroll_value(v);
        glib::idle_add_local_once(move || crate::settings_view::set_scroll_value(v));
    }
}

fn rebuild(p: &Rc<Pane>) {
    let (cards, err) = cards();
    *p.cards.borrow_mut() = cards;
    *p.list_error.borrow_mut() = err;
    // The selection: kept while it exists, else the first account.
    {
        let cards = p.cards.borrow();
        let mut sel = p.selected.borrow_mut();
        let ok = sel
            .as_ref()
            .is_some_and(|(a, n)| cards.iter().any(|c| c.agent == *a && c.name == *n));
        if !ok {
            *sel = cards.first().map(|c| (c.agent, c.name.clone()));
        }
    }
    build_list(p);
    build_detail(p);
    let focus = std::mem::replace(&mut *p.focus.borrow_mut(), Focus::None);
    if focus != Focus::None {
        if let Some(w) = p.focus_widget.borrow_mut().take() {
            glib::idle_add_local_once(move || {
                w.grab_focus();
            });
        }
    }
}

fn build_list(p: &Rc<Pane>) {
    while let Some(c) = p.list.first_child() {
        p.list.remove(&c);
    }
    let query = p.query.borrow().clone();
    let cards = p.cards.borrow();
    let sel = p.selected.borrow().clone();
    let draft = p.draft.borrow().clone();
    for agent in AgentKind::ALL {
        let shown: Vec<&Card> = cards
            .iter()
            .filter(|c| c.agent == agent && sa::matches(c, &query))
            .collect();
        let drafting = draft
            .as_ref()
            .is_some_and(|d| d.agent.unwrap_or(AgentKind::Claude) == agent);
        if shown.is_empty() && !drafting {
            continue;
        }
        let head = gtk::ListBoxRow::new();
        head.set_selectable(false);
        head.set_activatable(false);
        head.set_child(Some(&label(
            &agent.label().to_uppercase(),
            &["section-label", "settings-accounts-group"],
        )));
        p.list.append(&head);
        for c in shown {
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(&format!("account:{}:{}", c.agent.as_str(), c.name));
            row.add_css_class("settings-accounts-row");
            let b = gtk::Box::new(gtk::Orientation::Vertical, 1);
            let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            if !c.editable() {
                let lock = gtk::Image::from_icon_name("ph-lock-simple-symbolic");
                lock.add_css_class("settings-accounts-lock");
                lock.set_tooltip_text(Some("Declared by this Workspace (read-only here)"));
                top.append(&lock);
            }
            let name = label(&c.name, &["settings-accounts-name"]);
            name.set_hexpand(true);
            name.set_ellipsize(gtk::pango::EllipsizeMode::End);
            top.append(&name);
            if c.effective_default {
                let tick = label("✓", &["settings-accounts-default"]);
                tick.set_tooltip_text(Some("Default for new Agents of this kind here"));
                top.append(&tick);
            }
            b.append(&top);
            let dir = label(&c.dir_text(), &["settings-key"]);
            dir.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            b.append(&dir);
            let badge = c.badge();
            if !badge.is_empty() || !c.editable() {
                let text = if c.editable() {
                    badge
                } else if badge.is_empty() {
                    "workspace".into()
                } else {
                    format!("workspace · {badge}")
                };
                b.append(&label(&text, &["settings-accounts-badge"]));
            }
            row.set_child(Some(&b));
            p.list.append(&row);
            if draft.is_none() && sel.as_ref() == Some(&(c.agent, c.name.clone())) {
                p.list.select_row(Some(&row));
            }
        }
        if let (true, Some(d)) = (drafting, draft.as_ref()) {
            let row = gtk::ListBoxRow::new();
            row.set_widget_name("draft");
            row.add_css_class("settings-accounts-row");
            let n = if d.name.trim().is_empty() {
                "New account"
            } else {
                d.name.trim()
            };
            row.set_child(Some(&label(
                n,
                &["settings-accounts-name", "settings-accounts-draft"],
            )));
            p.list.append(&row);
            p.list.select_row(Some(&row));
        }
    }
}

/// The Environment grid: key · value · action per row.
struct Grid {
    grid: gtk::Grid,
    row: i32,
}

impl Grid {
    fn new() -> Self {
        let grid = gtk::Grid::new();
        grid.set_row_spacing(6);
        grid.set_column_spacing(8);
        grid.add_css_class("settings-account-env");
        Grid { grid, row: 0 }
    }
    fn add(
        &mut self,
        key: &impl IsA<gtk::Widget>,
        value: &impl IsA<gtk::Widget>,
        action: Option<&gtk::Widget>,
    ) {
        key.set_size_request(KEY_W, -1);
        key.set_valign(gtk::Align::Center);
        value.set_hexpand(true);
        self.grid.attach(key, 0, self.row, 1, 1);
        self.grid.attach(value, 1, self.row, 1, 1);
        if let Some(a) = action {
            self.grid.attach(a, 2, self.row, 1, 1);
        }
        self.row += 1;
    }
    fn full(&mut self, w: &impl IsA<gtk::Widget>) {
        self.grid.attach(w, 0, self.row, 3, 1);
        self.row += 1;
    }
}

/// Width of the key column.
const KEY_W: i32 = 210;

/// Commit `w`'s text on Enter and when focus leaves, if it changed.
fn committing<W: IsA<gtk::Editable> + IsA<gtk::Widget>>(w: &W, commit: impl Fn(String) + 'static) {
    let editable: gtk::Editable = w.clone().upcast();
    let start = Rc::new(RefCell::new(editable.text().to_string()));
    let fire = Rc::new(move |e: &gtk::Editable| {
        let now = e.text().to_string();
        if now != *start.borrow() {
            *start.borrow_mut() = now.clone();
            commit(now);
        }
    });
    let (f1, e1) = (fire.clone(), editable.clone());
    if let Some(entry) = w.dynamic_cast_ref::<gtk::Entry>() {
        entry.connect_activate(move |_| f1(&e1));
    } else if let Some(entry) = w.dynamic_cast_ref::<gtk::PasswordEntry>() {
        entry.connect_activate(move |_| f1(&e1));
    }
    let focus = gtk::EventControllerFocus::new();
    let weak = editable.downgrade();
    focus.connect_leave(move |_| {
        if let Some(e) = weak.upgrade() {
            fire(&e);
        }
    });
    w.add_controller(focus);
}

fn entry(text: &str, placeholder: &str) -> gtk::Entry {
    let e = gtk::Entry::new();
    e.set_text(text);
    e.set_placeholder_text(Some(placeholder));
    e.add_css_class("settings-account-entry");
    e
}

fn icon_button(icon: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.add_css_class("flat");
    b.add_css_class("settings-reset");
    b.set_tooltip_text(Some(tip));
    b.set_valign(gtk::Align::Center);
    b
}

/// A folder / file chooser that puts its pick into `entry` and commits.
fn chooser(folder: bool, entry: &gtk::Entry) -> gtk::Widget {
    let b = icon_button(
        if folder {
            "ph-folder-simple-symbolic"
        } else {
            "ph-file-symbolic"
        },
        if folder {
            "Choose a folder…"
        } else {
            "Choose a file…"
        },
    );
    let weak = entry.downgrade();
    b.connect_clicked(move |_| {
        let weak = weak.clone();
        let fd = gtk::FileDialog::new();
        let done = move |r: std::result::Result<gio::File, glib::Error>| {
            let (Some(e), Ok(f)) = (weak.upgrade(), r) else {
                return;
            };
            let Some(p) = f.path() else { return };
            e.set_text(&tilde(&p));
            e.emit_activate();
        };
        if folder {
            fd.set_title("Config dir of this login");
            fd.select_folder(Some(&app().window), gio::Cancellable::NONE, done);
        } else {
            fd.set_title(".env file");
            fd.open(Some(&app().window), gio::Cancellable::NONE, done);
        }
    });
    b.upcast()
}

/// Run one write from the detail: a `structural` change rebuilds the list
/// and the detail (`focus` says where the cursor goes); a value edit only
/// refreshes the list's badges. Both show `Saved`.
fn write(r: Result<()>, structural: bool, focus: Focus) {
    let Some(p) = pane() else { return };
    match r {
        Ok(()) => {
            *p.error.borrow_mut() = None;
            if structural {
                *p.focus.borrow_mut() = focus;
                refresh();
            } else {
                let (cards, _) = cards();
                *p.cards.borrow_mut() = cards;
                build_list(&p);
            }
            flash_saved();
        }
        Err(e) => {
            *p.error.borrow_mut() = Some(e.message);
            *p.focus.borrow_mut() = focus;
            refresh();
        }
    }
}

fn flash_saved() {
    let Some(l) = pane().and_then(|p| p.saved.borrow().clone()) else {
        return;
    };
    l.set_opacity(1.0);
    let w = l.downgrade();
    glib::timeout_add_local_once(std::time::Duration::from_millis(1600), move || {
        if let Some(l) = w.upgrade() {
            l.set_opacity(0.0);
        }
    });
}

fn selected_card(p: &Pane) -> Option<Card> {
    let sel = p.selected.borrow().clone()?;
    p.cards
        .borrow()
        .iter()
        .find(|c| c.agent == sel.0 && c.name == sel.1)
        .cloned()
}

fn build_detail(p: &Rc<Pane>) {
    while let Some(c) = p.detail.first_child() {
        p.detail.remove(&c);
    }
    *p.saved.borrow_mut() = None;
    if let Some(e) = p.list_error.borrow().as_ref() {
        let l = label(e, &["settings-account-error"]);
        l.set_wrap(true);
        p.detail.append(&l);
    }
    if let Some(d) = p.draft.borrow().clone() {
        draft_detail(p, &d);
        return;
    }
    let Some(c) = selected_card(p) else {
        p.detail.append(&label(
            "No accounts yet. + New account adds one for every Workspace.",
            &["empty-line"],
        ));
        return;
    };
    let edit_ok = c.editable();
    let (agent, name) = (c.agent, c.name.clone());

    // Header.
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    head.add_css_class("settings-account-head");
    let titles = gtk::Box::new(gtk::Orientation::Vertical, 2);
    titles.set_hexpand(true);
    titles.append(&label(&c.name, &["settings-account-title"]));
    let whence = if edit_ok {
        format!("{} · yours · every Workspace", c.agent.label())
    } else {
        format!("{} · declared in {}", c.agent.label(), tilde(&c.file))
    };
    let whence = label(&whence, &["settings-description"]);
    whence.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    titles.append(&whence);
    head.append(&titles);
    let saved = label("Saved ✓", &["settings-account-saved"]);
    saved.set_opacity(0.0);
    saved.set_valign(gtk::Align::Center);
    head.append(&saved);
    *p.saved.borrow_mut() = Some(saved);
    let dl = label("Default", &["settings-description"]);
    dl.set_valign(gtk::Align::Center);
    head.append(&dl);
    let def = gtk::Switch::new();
    def.add_css_class("settings-switch");
    def.set_active(c.default);
    def.set_sensitive(edit_ok);
    def.set_valign(gtk::Align::Center);
    def.set_tooltip_text(Some(if edit_ok {
        "Default among your accounts; a Workspace's own default wins there"
    } else {
        "Default for new Agents of this kind in this Workspace"
    }));
    {
        let name = name.clone();
        def.connect_state_set(move |_, on| {
            let n = name.clone();
            glib::idle_add_local_once(move || {
                write(
                    edit_many(agent, &n, &[Edit::Default(on)]),
                    true,
                    Focus::None,
                )
            });
            glib::Propagation::Proceed
        });
    }
    head.append(&def);
    head.append(&menu(&c));
    p.detail.append(&head);

    // Environment.
    let sec = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    sec.add_css_class("settings-account-env-head");
    sec.append(&label("ENVIRONMENT", &["section-label"]));
    sec.append(&label(
        "what new Agents get, top to bottom",
        &["settings-description"],
    ));
    p.detail.append(&sec);
    let mut g = Grid::new();

    for r in c.env_rows() {
        match r.source {
            EnvSource::Login => {
                let key = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                key.append(&label(&r.key, &["settings-key", "settings-account-key"]));
                key.append(&label("login", &["settings-account-tag"]));
                key.set_tooltip_text(Some(
                    "The login RADE knows the account by. Empty = the agent's default login.",
                ));
                let e = entry(
                    r.value.as_deref().unwrap_or(""),
                    sa::default_login_text(agent),
                );
                e.set_sensitive(edit_ok);
                let n = name.clone();
                committing(&e, move |v| {
                    write(
                        edit_many(agent, &n, &[Edit::ConfigDir(v)]),
                        false,
                        Focus::None,
                    )
                });
                let act = edit_ok.then(|| chooser(true, &e));
                g.add(&key, &e, act.as_ref());
                // The .env file row comes right after the login (a
                // read-only account without one has none).
                if !edit_ok && c.env_file.is_empty() {
                    continue;
                }
                let fkey = label(".env file", &["settings-account-key"]);
                let f = entry(&c.env_file, "~/.my-login.env");
                f.set_sensitive(edit_ok);
                let n = name.clone();
                committing(&f, move |v| {
                    let v = v.trim().to_string();
                    write(
                        edit_many(agent, &n, &[Edit::EnvFile((!v.is_empty()).then_some(v))]),
                        true,
                        Focus::None,
                    )
                });
                let act = edit_ok.then(|| chooser(false, &f));
                g.add(&fkey, &f, act.as_ref());
                if let EnvFileState::Error(e) = &c.env_file_state {
                    let l = label(e, &["settings-account-error"]);
                    l.set_wrap(true);
                    g.full(&l);
                }
            }
            EnvSource::File => {
                let mut kc = vec![
                    "settings-key",
                    "settings-account-key",
                    "settings-account-indent",
                ];
                if r.overridden {
                    kc.push("settings-account-overridden");
                }
                let k = label(&r.key, &kc);
                k.set_ellipsize(gtk::pango::EllipsizeMode::End);
                let v = label(
                    if r.overridden {
                        "overridden below"
                    } else {
                        "••••••••   from .env"
                    },
                    &["settings-account-file-value"],
                );
                g.add(&k, &v, None);
            }
            EnvSource::Own => {
                let k = label(&r.key, &["settings-key", "settings-account-key"]);
                k.set_ellipsize(gtk::pango::EllipsizeMode::End);
                let val = r.value.clone().unwrap_or_default();
                let (n, key) = (name.clone(), r.key.clone());
                let commit = move |v: String| {
                    write(
                        edit_many(agent, &n, &[Edit::SetEnv(key.clone(), v)]),
                        false,
                        Focus::None,
                    )
                };
                let w: gtk::Widget = if r.secret {
                    let pe = gtk::PasswordEntry::new();
                    pe.set_show_peek_icon(true);
                    pe.set_text(&val);
                    pe.add_css_class("settings-account-entry");
                    committing(&pe, commit);
                    pe.upcast()
                } else {
                    let e = entry(&val, "");
                    committing(&e, commit);
                    e.upcast()
                };
                w.set_sensitive(edit_ok);
                let act = edit_ok.then(|| {
                    let x = icon_button("ph-x-symbolic", &format!("Remove {}", r.key));
                    let (n, key) = (name.clone(), r.key.clone());
                    x.connect_clicked(move |_| {
                        write(
                            edit_many(agent, &n, &[Edit::UnsetEnv(key.clone())]),
                            true,
                            Focus::AddVariable,
                        )
                    });
                    x.upcast::<gtk::Widget>()
                });
                g.add(&k, &w, act.as_ref());
            }
        }
    }

    // The row being added.
    let new_var = p.new_var.borrow().clone();
    if let (true, Some((k0, v0))) = (edit_ok, new_var) {
        let k = entry(&k0, "KEY");
        k.add_css_class("settings-account-new-key");
        let v = entry(&v0, "value");
        let save = {
            let (k, v, n) = (k.downgrade(), v.downgrade(), name.clone());
            move || {
                let (Some(k), Some(v)) = (k.upgrade(), v.upgrade()) else {
                    return;
                };
                let Some(p) = pane() else { return };
                let key = k.text().trim().to_string();
                *p.new_var.borrow_mut() = Some((key.clone(), v.text().to_string()));
                if key.is_empty() {
                    return;
                }
                let r = edit_many(agent, &n, &[Edit::SetEnv(key, v.text().to_string())]);
                if r.is_ok() {
                    *p.new_var.borrow_mut() = None;
                }
                let keep = p.new_var.borrow().is_some();
                write(
                    r,
                    true,
                    if keep {
                        Focus::NewVarKey
                    } else {
                        Focus::AddVariable
                    },
                );
            }
        };
        let s1 = save.clone();
        v.connect_activate(move |_| s1());
        let s2 = save.clone();
        k.connect_activate(move |_| s2());
        // Typing keeps the draft, so a rebuild never loses it.
        for (e, is_key) in [(&k, true), (&v, false)] {
            e.connect_changed(move |e| {
                if let Some(p) = pane() {
                    if let Some(d) = p.new_var.borrow_mut().as_mut() {
                        if is_key {
                            d.0 = e.text().to_string();
                        } else {
                            d.1 = e.text().to_string();
                        }
                    }
                }
            });
        }
        let ok = icon_button("ph-check-symbolic", "Save (Enter) · Esc drops it");
        ok.connect_clicked(move |_| save());
        g.add(&k, &v, Some(ok.upcast_ref()));
        if *p.focus.borrow() == Focus::NewVarKey {
            *p.focus_widget.borrow_mut() = Some(k.clone().upcast());
        }
    }
    if let Some(e) = p.error.borrow().as_ref() {
        let l = label(e, &["settings-account-error"]);
        l.set_wrap(true);
        g.full(&l);
    }
    p.detail.append(&g.grid);

    if edit_ok {
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let add = gtk::Button::with_label("+ Add variable");
        add.add_css_class("flat");
        add.add_css_class("settings-account-add-var");
        add.connect_clicked(|_| {
            let Some(p) = pane() else { return };
            if p.new_var.borrow().is_none() {
                *p.new_var.borrow_mut() = Some((String::new(), String::new()));
            }
            *p.focus.borrow_mut() = Focus::NewVarKey;
            refresh();
        });
        if *p.focus.borrow() == Focus::AddVariable {
            *p.focus_widget.borrow_mut() = Some(add.clone().upcast());
        }
        actions.append(&add);
        let paste = gtk::Button::with_label("Paste KEY=VALUE lines");
        paste.add_css_class("flat");
        paste.add_css_class("settings-account-paste");
        paste.set_tooltip_text(Some("Adds every KEY=VALUE line of the clipboard"));
        let n = name.clone();
        paste.connect_clicked(move |b| {
            let n = n.clone();
            let clip = b.clipboard();
            glib::spawn_future_local(async move {
                let text = clip.read_text_future().await.ok().flatten();
                paste_text(agent, &n, text.as_deref().unwrap_or(""));
            });
        });
        actions.append(&paste);
        p.detail.append(&actions);
        p.detail.append(&label(
            "New Agents use these settings; running ones keep theirs.",
            &["settings-description", "settings-account-hint"],
        ));
    } else {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let l = label(
            &format!("Declared in {} — edit it there.", tilde(&c.file)),
            &["settings-description"],
        );
        l.set_hexpand(true);
        row.append(&l);
        let open = gtk::Button::with_label("Open file");
        open.add_css_class("flat");
        let f = c.file.clone();
        open.connect_clicked(move |_| {
            crate::settings_view::close();
            crate::files::open_in_viewer(&f);
        });
        row.append(&open);
        p.detail.append(&row);
    }
    if c.unused_here {
        p.detail.append(&label(
            "Not offered in this Workspace: it sets accounts = \"own\".",
            &["settings-description"],
        ));
    }
}

/// Every `KEY=VALUE` line of `text` into the account, in one write.
fn paste_text(agent: AgentKind, name: &str, text: &str) {
    let r = match rade_core::dotenv::parse(text) {
        Ok(v) if v.is_empty() => Err(Error::usage("the clipboard has no KEY=VALUE lines")),
        Ok(v) => edit_many(
            agent,
            name,
            &v.into_iter()
                .map(|(k, v)| Edit::SetEnv(k, v))
                .collect::<Vec<_>>(),
        ),
        Err((n, e)) => Err(Error::usage(format!("clipboard line {n}: {e}"))),
    };
    write(r, true, Focus::AddVariable);
}

/// The header's ⋯ menu: Duplicate, Delete…, Open the declaring file.
fn menu(c: &Card) -> gtk::Widget {
    let mb = gtk::MenuButton::new();
    mb.set_icon_name("ph-dots-three-symbolic");
    mb.add_css_class("flat");
    mb.set_valign(gtk::Align::Center);
    mb.set_tooltip_text(Some("More"));
    let pop = gtk::Popover::new();
    let col = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let item = |text: &str| {
        let b = gtk::Button::with_label(text);
        b.add_css_class("flat");
        if let Some(l) = b.child().and_downcast::<gtk::Label>() {
            l.set_xalign(0.0);
        }
        b
    };
    let (agent, name) = (c.agent, c.name.clone());
    if c.editable() {
        let dup = item("Duplicate");
        let card = c.clone();
        let pw = pop.downgrade();
        dup.connect_clicked(move |_| {
            if let Some(p) = pw.upgrade() {
                p.popdown();
            }
            duplicate(&card);
        });
        col.append(&dup);
        let del = item("Delete…");
        del.add_css_class("settings-account-delete");
        let pw = pop.downgrade();
        let n = name.clone();
        del.connect_clicked(move |_| {
            if let Some(p) = pw.upgrade() {
                p.popdown();
            }
            confirm_delete(agent, &n);
        });
        col.append(&del);
    }
    let open = item(if c.editable() {
        "Open accounts.toml"
    } else {
        "Open workspace.toml"
    });
    let f = c.file.clone();
    open.connect_clicked(move |_| {
        crate::settings_view::close();
        crate::files::open_in_viewer(&f);
    });
    col.append(&open);
    pop.set_child(Some(&col));
    mb.set_popover(Some(&pop));
    mb.upcast()
}

fn duplicate(c: &Card) {
    let taken: Vec<String> = pane()
        .map(|p| {
            p.cards
                .borrow()
                .iter()
                .filter(|x| x.agent == c.agent)
                .map(|x| x.name.clone())
                .collect()
        })
        .unwrap_or_default();
    let mut name = format!("{}-copy", c.name);
    let mut i = 2;
    while taken.contains(&name) {
        name = format!("{}-copy{i}", c.name);
        i += 1;
    }
    let decl = account::AccountDecl {
        agent: c.agent.as_str().into(),
        name: name.clone(),
        config_dir: c.config_dir.clone(),
        env_file: c.env_file.clone(),
        env: c
            .variables
            .iter()
            .map(|(k, v, _)| (k.clone(), v.clone()))
            .collect(),
        ..Default::default()
    };
    let r = add(&decl);
    if r.is_ok() {
        if let Some(p) = pane() {
            *p.selected.borrow_mut() = Some((c.agent, name));
        }
    }
    write(r, true, Focus::None);
}

fn confirm_delete(agent: AgentKind, name: &str) {
    let dlg = adw::AlertDialog::new(
        Some(&format!("Delete account '{name}'?")),
        Some(
            "Its table is removed from accounts.toml. The config dir and its login are left alone.",
        ),
    );
    dlg.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);
    dlg.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    let name = name.to_string();
    dlg.connect_response(None, move |_, r| {
        if r == "delete" {
            write(edit_many(agent, &name, &[Edit::Remove]), true, Focus::None);
        }
    });
    dlg.present(Some(&app().window));
}

fn start_draft() {
    let Some(p) = pane() else { return };
    let agent = p
        .selected
        .borrow()
        .as_ref()
        .map(|s| s.0)
        .unwrap_or(AgentKind::Claude);
    *p.draft.borrow_mut() = Some(Draft {
        agent: Some(agent),
        ..Default::default()
    });
    *p.new_var.borrow_mut() = None;
    *p.error.borrow_mut() = None;
    *p.focus.borrow_mut() = Focus::DraftName;
    refresh();
}

/// Write the draft; it becomes the selection.
fn create(d: &Draft) {
    let agent = d.agent.unwrap_or(AgentKind::Claude);
    let decl = account::AccountDecl {
        agent: agent.as_str().into(),
        name: d.name.trim().into(),
        config_dir: d.dir.trim().into(),
        ..Default::default()
    };
    let r = add(&decl);
    let Some(p) = pane() else { return };
    if r.is_ok() {
        *p.draft.borrow_mut() = None;
        *p.selected.borrow_mut() = Some((agent, d.name.trim().to_string()));
        write(r, true, Focus::AddVariable);
    } else {
        write(r, true, Focus::DraftName);
    }
}

fn set_draft(f: impl FnOnce(&mut Draft)) {
    if let Some(p) = pane() {
        if let Some(d) = p.draft.borrow_mut().as_mut() {
            f(d);
        }
    }
}

fn draft_detail(p: &Rc<Pane>, d: &Draft) {
    p.detail
        .append(&label("New account", &["settings-account-title"]));
    p.detail.append(&label(
        "Saved to accounts.toml and offered in every Workspace (unless it sets accounts = \"own\").",
        &["settings-description"],
    ));
    let mut g = Grid::new();
    let agent = gtk::DropDown::from_strings(&["Claude Code", "Codex"]);
    agent.add_css_class("settings-drop");
    agent.set_selected(u32::from(d.agent == Some(AgentKind::Codex)));
    agent.connect_selected_notify(|dd| {
        let kind = if dd.selected() == 1 {
            AgentKind::Codex
        } else {
            AgentKind::Claude
        };
        set_draft(|d| d.agent = Some(kind));
        refresh();
    });
    g.add(&label("Agent", &["settings-account-key"]), &agent, None);
    let name = entry(&d.name, "clw");
    name.add_css_class("settings-account-draft-name");
    name.connect_changed(|e| {
        let t = e.text().to_string();
        set_draft(|d| d.name = t);
    });
    g.add(&label("Name", &["settings-account-key"]), &name, None);
    let kind = d.agent.unwrap_or(AgentKind::Claude);
    let key = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    key.append(&label(
        kind.var(),
        &["settings-key", "settings-account-key"],
    ));
    key.append(&label("login", &["settings-account-tag"]));
    let dir = entry(&d.dir, sa::default_login_text(kind));
    dir.connect_changed(|e| {
        let t = e.text().to_string();
        set_draft(|d| d.dir = t);
    });
    let ch = chooser(true, &dir);
    g.add(&key, &dir, Some(&ch));
    if let Some(e) = p.error.borrow().as_ref() {
        let l = label(e, &["settings-account-error"]);
        l.set_wrap(true);
        g.full(&l);
    }
    p.detail.append(&g.grid);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let go = || {
        if let Some(d) = pane().and_then(|p| p.draft.borrow().clone()) {
            create(&d);
        }
    };
    let b = gtk::Button::with_label("Create");
    b.add_css_class("suggested-action");
    b.add_css_class("settings-account-create");
    b.connect_clicked(move |_| go());
    name.connect_activate(move |_| go());
    dir.connect_activate(move |_| go());
    let cancel = gtk::Button::with_label("Cancel");
    cancel.add_css_class("flat");
    cancel.connect_clicked(|_| {
        if let Some(p) = pane() {
            drop_draft(&p);
        }
    });
    row.append(&b);
    row.append(&cancel);
    p.detail.append(&row);
    if *p.focus.borrow() == Focus::DraftName {
        *p.focus_widget.borrow_mut() = Some(name.upcast());
    }
}

// ---- debug.ui ------------------------------------------------------------

fn state() -> Value {
    let Some(p) = pane() else {
        return json!({"open": false});
    };
    let cards = p.cards.borrow();
    let list: Vec<Value> = cards
        .iter()
        .map(|c| {
            json!({"agent": c.agent, "name": c.name, "scope": c.scope, "editable": c.editable(),
                "default": c.effective_default, "default_in_file": c.default, "dir": c.dir_text(), "badge": c.badge(),
                "unused_here": c.unused_here})
        })
        .collect();
    drop(cards);
    let sel = selected_card(&p);
    let env: Vec<Value> = sel
        .as_ref()
        .map(|c| {
            c.env_rows()
                .iter()
                .map(|r| {
                    json!({"key": r.key, "source": r.source, "overridden": r.overridden,
                        "secret": r.secret})
                })
                .collect()
        })
        .unwrap_or_default();
    let file = sel.as_ref().map(|c| {
        let (state, keys, error) = match &c.env_file_state {
            EnvFileState::None => ("none", vec![], None),
            EnvFileState::Loaded(k) => ("loaded", k.clone(), None),
            EnvFileState::Error(e) => ("error", vec![], Some(e.clone())),
        };
        json!({"path": c.env_file, "state": state, "text": c.env_file_state.text(),
            "keys": keys, "error": error})
    });
    let focus: Vec<String> = gtk::prelude::GtkWindowExt::focus(&app().window)
        .and_then(|f| f.ancestor(gtk::Entry::static_type()).or(Some(f)))
        .map(|w| w.css_classes().iter().map(|c| c.to_string()).collect())
        .unwrap_or_default();
    let draft = p
        .draft
        .borrow()
        .as_ref()
        .map(|d| json!({"agent": d.agent, "name": d.name, "dir": d.dir}));
    let new_var = p.new_var.borrow().as_ref().map(|(k, _)| k.clone());
    json!({
        "open": true,
        "file": crate::accounts::user_file(),
        "list": list,
        "selected": sel.as_ref().map(|c| json!({"agent": c.agent, "name": c.name})),
        "draft": draft,
        "new_var": new_var,
        "env": env,
        "env_file": file,
        "error": p.error.borrow().clone(),
        "list_error": p.list_error.borrow().clone(),
        "scroll": crate::settings_view::scroll_value(),
        "focus": focus,
    })
}

/// `debug.ui {"op": "app_settings_accounts", "do"?: "select" | "new" |
/// "create" | "add_var" | "edit" | "delete" | "paste" | "escape", …}`
/// (test mode; Settings open on Accounts): drives the section as the page
/// does and answers its state (never a value). `edit`: `{"agent", "name",
/// "edit": {"config_dir" | "env_file" (null clears) | "set_env": [k, v] |
/// "unset_env" | "default"}}`; `new`: `{"agent"?, "name"?, "config_dir"?}`
/// opens and fills the draft; `add_var`: `{"key", "value"}` saves a new
/// row of the selection; `paste`: `{"text"}`; `scroll`: `{"y"?}` scrolls
/// the page to the section.
pub fn debug(p: &Value) -> Result<Value> {
    let s = |k: &str| p.get(k).and_then(Value::as_str);
    let Some(pn) = pane() else {
        return match s("do") {
            None => Ok(state()),
            Some(_) => Err(Error::usage(
                "open Settings on Accounts first (app_settings_open, app_settings_search)",
            )),
        };
    };
    let agent = || {
        AgentKind::parse(s("agent").unwrap_or("claude"))
            .ok_or_else(|| Error::usage("agent must be claude or codex"))
    };
    let name = || s("name").ok_or_else(|| Error::usage("want name"));
    let selection = || {
        pn.selected
            .borrow()
            .clone()
            .ok_or_else(|| Error::usage("nothing selected"))
    };
    match s("do") {
        None => {}
        Some("select") => {
            *pn.selected.borrow_mut() = Some((agent()?, name()?.to_string()));
            *pn.draft.borrow_mut() = None;
            *pn.new_var.borrow_mut() = None;
            *pn.error.borrow_mut() = None;
            refresh();
        }
        Some("new") => {
            start_draft();
            set_draft(|d| {
                if let Some(a) = s("agent") {
                    d.agent = AgentKind::parse(a);
                }
                d.name = s("name").unwrap_or("").into();
                d.dir = s("config_dir").unwrap_or("").into();
            });
            *pn.focus.borrow_mut() = Focus::DraftName;
            refresh();
        }
        Some("create") => {
            let d = pn.draft.borrow().clone();
            create(&d.ok_or_else(|| Error::usage("no draft"))?);
        }
        Some("add_var") => {
            let (agent, n) = selection()?;
            let (k, v) = (s("key").unwrap_or(""), s("value").unwrap_or(""));
            *pn.new_var.borrow_mut() = Some((k.into(), v.into()));
            let r = edit_many(agent, &n, &[Edit::SetEnv(k.into(), v.into())]);
            if r.is_ok() {
                *pn.new_var.borrow_mut() = None;
            }
            let keep = pn.new_var.borrow().is_some();
            write(
                r,
                true,
                if keep {
                    Focus::NewVarKey
                } else {
                    Focus::AddVariable
                },
            );
        }
        Some("paste") => {
            let (agent, n) = selection()?;
            paste_text(agent, &n, s("text").unwrap_or(""));
        }
        Some("escape") => {
            drop_draft(&pn);
        }
        Some("scroll") => {
            // Scroll the page to the Accounts section (`y` = extra offset).
            let y = p.get("y").and_then(Value::as_f64).unwrap_or(0.0);
            if let Some(at) = pn
                .root
                .compute_point(
                    &pn.root.parent().ok_or_else(|| Error::usage("not shown"))?,
                    &gtk::graphene::Point::new(0.0, 0.0),
                )
                .map(|pt| pt.y() as f64)
            {
                crate::settings_view::set_scroll_value(at + y);
            }
        }
        Some("delete") => write(
            edit_many(agent()?, name()?, &[Edit::Remove]),
            true,
            Focus::None,
        ),
        Some("edit") => {
            let e = p.get("edit").ok_or_else(|| Error::usage("want edit"))?;
            let es = |k: &str| e.get(k).and_then(Value::as_str).map(String::from);
            let change = if let Some(d) = es("config_dir") {
                Edit::ConfigDir(d)
            } else if e.get("env_file").is_some() {
                Edit::EnvFile(es("env_file"))
            } else if let Some(kv) = e.get("set_env").and_then(Value::as_array) {
                let at = |i: usize| kv.get(i).and_then(Value::as_str).unwrap_or("").to_string();
                Edit::SetEnv(at(0), at(1))
            } else if let Some(k) = es("unset_env") {
                Edit::UnsetEnv(k)
            } else if let Some(d) = e.get("default").and_then(Value::as_bool) {
                Edit::Default(d)
            } else {
                return Err(Error::usage("unknown edit"));
            };
            let structural = !matches!(change, Edit::ConfigDir(_) | Edit::SetEnv(..));
            write(
                edit_many(agent()?, name()?, &[change]),
                structural,
                Focus::None,
            );
        }
        Some(x) => return Err(Error::usage(format!("unknown do '{x}'"))),
    }
    Ok(state())
}
