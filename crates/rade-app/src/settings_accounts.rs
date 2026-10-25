//! Settings → Accounts (PRD R126, ADR-0047): a card per Agent Account the
//! shown Workspace may use. User accounts (`accounts.toml`) are edited in
//! place — config dir, Default, `.env` file, variables one by one — each
//! change one `rade_core::account::edit_in_toml` write; the Workspace's own
//! accounts are read-only with *Open file* (R112.6). The model is
//! `rade_core::settings_accounts`; values of an env file are never shown.

use crate::app::app;
use adw::prelude::*;
use gtk::gio;
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::account::{self, AgentKind, Edit};
use rade_core::rpc::{Error, Result};
use rade_core::settings_accounts::{self as sa, Card, EnvFileState};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

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
    (sa::cards(&user, &own, own_only, &home), err)
}

/// One change of a user account, written now; then the page redraws.
pub fn edit(agent: AgentKind, name: &str, e: Edit) -> Result<()> {
    account::edit_user_file(&crate::accounts::user_file(), |t| {
        account::edit_in_toml(t, agent, name, &e)
    })?;
    crate::accounts::forget_lists();
    Ok(())
}

/// A new user account (agent, name, config dir).
pub fn add(agent: &str, name: &str, dir: &str) -> Result<()> {
    let decl = account::AccountDecl {
        agent: agent.into(),
        name: name.into(),
        config_dir: dir.into(),
        ..Default::default()
    };
    account::edit_user_file(&crate::accounts::user_file(), |t| {
        account::add_to_toml(t, &decl)
    })?;
    crate::accounts::forget_lists();
    Ok(())
}

/// Run a change from the page: its error goes to the page's error line.
fn run(r: Result<()>) {
    crate::settings_view::show_result(r.map_err(|e| e.message));
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

/// An entry that commits on Enter and when focus leaves, if changed.
fn committing_entry(text: &str, commit: impl Fn(String) + 'static) -> gtk::Entry {
    let e = gtk::Entry::new();
    e.set_text(text);
    e.add_css_class("settings-account-entry");
    let start = std::rc::Rc::new(std::cell::RefCell::new(text.to_string()));
    let commit = std::rc::Rc::new(commit);
    let fire = {
        let (start, commit) = (start.clone(), commit.clone());
        move |e: &gtk::Entry| {
            let now = e.text().to_string();
            if now != *start.borrow() {
                *start.borrow_mut() = now.clone();
                commit(now);
            }
        }
    };
    let f1 = fire.clone();
    e.connect_activate(move |e| f1(e));
    let focus = gtk::EventControllerFocus::new();
    let weak = e.downgrade();
    focus.connect_leave(move |_| {
        if let Some(e) = weak.upgrade() {
            fire(&e);
        }
    });
    e.add_controller(focus);
    e
}

/// A label-column row: `name` on the left, `child` filling the rest.
fn field(name: &str, child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("settings-account-field");
    let l = label(name, &["settings-account-field-label"]);
    l.set_width_chars(10);
    row.append(&l);
    child.set_hexpand(true);
    row.append(child);
    row
}

fn chooser_button(folder: bool, entry: &gtk::Entry) -> gtk::Button {
    let b = gtk::Button::with_label("Browse…");
    b.add_css_class("flat");
    b.add_css_class("settings-account-browse");
    let weak = entry.downgrade();
    b.connect_clicked(move |_| {
        let weak = weak.clone();
        let fd = gtk::FileDialog::new();
        let done = move |r: std::result::Result<gio::File, gtk::glib::Error>| {
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
    b
}

fn tilde(p: &Path) -> String {
    match p.strip_prefix(home()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => p.display().to_string(),
    }
}

/// Width of a variable's key column (saved rows and the new-row entry).
const KEY_W: i32 = 196;

/// One account's card.
pub fn card(c: &Card) -> gtk::Widget {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 6);
    b.add_css_class("settings-account");
    b.set_widget_name(&format!("account:{}:{}", c.agent.as_str(), c.name));
    let (agent, name, edit_ok) = (c.agent, c.name.clone(), c.editable());

    // Head: agent, name, where it is declared, Default, Delete / Open file.
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    head.append(&label(
        c.agent.label(),
        &[
            "settings-account-agent",
            &format!("agent-badge-{}", c.agent.as_str()),
        ],
    ));
    head.append(&label(&c.name, &["settings-label"]));
    let whence = if edit_ok {
        "user · every Workspace".to_string()
    } else {
        format!("from {}", tilde(&c.file))
    };
    let whence = label(&whence, &["settings-key"]);
    whence.set_hexpand(true);
    whence.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    head.append(&whence);
    let def = gtk::Switch::new();
    def.add_css_class("settings-switch");
    def.set_active(c.default);
    def.set_sensitive(edit_ok);
    def.set_valign(gtk::Align::Center);
    def.set_tooltip_text(Some("Default for new Agents of this kind"));
    {
        let name = name.clone();
        def.connect_state_set(move |_, on| {
            run(edit(agent, &name, Edit::Default(on)));
            gtk::glib::Propagation::Stop
        });
    }
    head.append(&label("Default", &["settings-description"]));
    head.append(&def);
    if edit_ok {
        let del = gtk::Button::with_label("Delete…");
        del.add_css_class("flat");
        del.add_css_class("settings-account-delete");
        let name = name.clone();
        del.connect_clicked(move |_| confirm_delete(agent, &name));
        head.append(&del);
    } else {
        let open = gtk::Button::from_icon_name("ph-arrow-square-out-symbolic");
        open.add_css_class("flat");
        open.add_css_class("settings-reset");
        open.set_tooltip_text(Some("Open file"));
        let f = c.file.clone();
        open.connect_clicked(move |_| {
            crate::settings_view::close();
            crate::files::open_in_viewer(&f);
        });
        head.append(&open);
    }
    b.append(&head);
    if c.unused_here {
        b.append(&label(
            "Not offered in this Workspace (accounts = \"own\")",
            &["settings-description"],
        ));
    }

    // Config dir.
    let dir = {
        let name = name.clone();
        committing_entry(&c.config_dir, move |v| {
            run(edit(agent, &name, Edit::ConfigDir(v)))
        })
    };
    dir.set_sensitive(edit_ok);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    line.append(&dir);
    dir.set_hexpand(true);
    if edit_ok {
        line.append(&chooser_button(true, &dir));
    }
    b.append(&field("Config dir", &line));

    // .env file + its state.
    let envf = {
        let name = name.clone();
        committing_entry(&c.env_file, move |v| {
            let v = v.trim().to_string();
            run(edit(
                agent,
                &name,
                Edit::EnvFile((!v.is_empty()).then_some(v)),
            ))
        })
    };
    envf.set_placeholder_text(Some("~/.my-login.env"));
    envf.set_sensitive(edit_ok);
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    line.append(&envf);
    envf.set_hexpand(true);
    if edit_ok {
        line.append(&chooser_button(false, &envf));
    }
    b.append(&field(".env file", &line));
    match &c.env_file_state {
        EnvFileState::None => {}
        EnvFileState::Loaded(keys) => {
            let t = format!("{} · {}", c.env_file_state.text(), keys.join(", "));
            let l = label(&t, &["settings-account-env-state"]);
            l.set_wrap(true);
            b.append(&field("", &l));
        }
        EnvFileState::Error(e) => {
            let l = label(e, &["settings-account-env-error"]);
            l.set_wrap(true);
            b.append(&field("", &l));
        }
    }

    // Variables, one per row.
    let vars = gtk::Box::new(gtk::Orientation::Vertical, 4);
    for (k, v, secret) in &c.variables {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.add_css_class("settings-account-var");
        let key = label(k, &["settings-key", "settings-account-var-key"]);
        key.set_size_request(KEY_W, -1);
        key.set_valign(gtk::Align::Center);
        key.set_ellipsize(gtk::pango::EllipsizeMode::End);
        row.append(&key);
        let commit = {
            let (name, k) = (name.clone(), k.clone());
            move |v: String| run(edit(agent, &name, Edit::SetEnv(k.clone(), v)))
        };
        let value: gtk::Widget = if *secret {
            let p = gtk::PasswordEntry::new();
            p.set_show_peek_icon(true);
            p.set_text(v);
            p.add_css_class("settings-account-entry");
            let start = v.clone();
            p.connect_activate(move |p| {
                if p.text() != start.as_str() {
                    commit(p.text().to_string());
                }
            });
            p.upcast()
        } else {
            committing_entry(v, commit).upcast()
        };
        value.set_hexpand(true);
        value.set_sensitive(edit_ok);
        row.append(&value);
        if edit_ok {
            let x = gtk::Button::from_icon_name("ph-x-symbolic");
            x.add_css_class("flat");
            x.add_css_class("settings-reset");
            x.set_tooltip_text(Some(&format!("Remove {k}")));
            let (name, k) = (name.clone(), k.clone());
            x.connect_clicked(move |_| run(edit(agent, &name, Edit::UnsetEnv(k.clone()))));
            row.append(&x);
        }
        vars.append(&row);
    }
    if edit_ok {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        row.add_css_class("settings-account-var-new");
        let k = gtk::Entry::new();
        k.set_placeholder_text(Some("KEY"));
        k.set_size_request(KEY_W, -1);
        k.add_css_class("settings-account-entry");
        let v = gtk::Entry::new();
        v.set_placeholder_text(Some("value"));
        v.set_hexpand(true);
        v.add_css_class("settings-account-entry");
        let add_b = gtk::Button::from_icon_name("ph-plus-symbolic");
        add_b.add_css_class("flat");
        add_b.add_css_class("settings-reset");
        add_b.set_tooltip_text(Some("Add variable"));
        let go = {
            let (k, v, name) = (k.downgrade(), v.downgrade(), name.clone());
            move || {
                let (Some(k), Some(v)) = (k.upgrade(), v.upgrade()) else {
                    return;
                };
                let key = k.text().trim().to_string();
                if key.is_empty() {
                    return;
                }
                run(edit(agent, &name, Edit::SetEnv(key, v.text().to_string())));
            }
        };
        let g2 = go.clone();
        v.connect_activate(move |_| g2());
        add_b.connect_clicked(move |_| go());
        row.append(&k);
        row.append(&v);
        row.append(&add_b);
        vars.append(&row);
    }
    if !c.variables.is_empty() || edit_ok {
        b.append(&field("Variables", &vars));
    }
    if edit_ok {
        b.append(&label(
            "New Agents use these settings; running ones keep theirs.",
            &["settings-description", "settings-account-hint"],
        ));
    }
    b.upcast()
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
            run(edit(agent, &name, Edit::Remove));
        }
    });
    dlg.present(Some(&app().window));
}

/// The *Add account* row at the end of the category.
pub fn add_row() -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.add_css_class("settings-account-add");
    let agent = gtk::DropDown::from_strings(&["claude", "codex"]);
    agent.add_css_class("settings-drop");
    let name = gtk::Entry::new();
    name.set_placeholder_text(Some("Name (clw)"));
    name.add_css_class("settings-account-entry");
    let dir = gtk::Entry::new();
    dir.set_placeholder_text(Some("Config dir (~/.claude-work)"));
    dir.set_hexpand(true);
    dir.add_css_class("settings-account-entry");
    let b = gtk::Button::with_label("Add account");
    b.add_css_class("settings-account-add-button");
    let go = {
        let (agent, name, dir) = (agent.downgrade(), name.downgrade(), dir.downgrade());
        move || {
            let (Some(a), Some(n), Some(d)) = (agent.upgrade(), name.upgrade(), dir.upgrade())
            else {
                return;
            };
            let kind = if a.selected() == 1 { "codex" } else { "claude" };
            run(add(kind, n.text().trim(), d.text().trim()));
        }
    };
    let g2 = go.clone();
    dir.connect_activate(move |_| g2());
    b.connect_clicked(move |_| go());
    row.append(&agent);
    row.append(&name);
    row.append(&dir);
    row.append(&b);
    row.upcast()
}

fn card_json(c: &Card) -> Value {
    let (state, keys, error) = match &c.env_file_state {
        EnvFileState::None => ("none", vec![], None),
        EnvFileState::Loaded(k) => ("loaded", k.clone(), None),
        EnvFileState::Error(e) => ("error", vec![], Some(e.clone())),
    };
    json!({
        "scope": c.scope, "agent": c.agent, "name": c.name, "config_dir": c.config_dir,
        "default": c.default, "editable": c.editable(), "file": c.file,
        "unused_here": c.unused_here,
        "env_file": c.env_file, "env_file_state": state, "env_file_text": c.env_file_state.text(),
        "env_file_keys": keys, "env_file_error": error,
        "variables": c.variables.iter().map(|(k, _, s)| json!({"key": k, "secret": s})).collect::<Vec<_>>(),
    })
}

/// `debug.ui {"op": "app_settings_accounts", "do"?: "add" | "edit" |
/// "delete", "agent"?, "name"?, "config_dir"?, "edit"?: {"config_dir" |
/// "env_file" (null clears) | "set_env": [k, v] | "unset_env" | "default"}}`
/// (test mode): runs the change as the page does and answers the cards
/// (never a value) and the page's error.
pub fn debug(p: &Value) -> Result<Value> {
    let s = |k: &str| p.get(k).and_then(Value::as_str);
    let agent = || {
        AgentKind::parse(s("agent").unwrap_or("claude"))
            .ok_or_else(|| Error::usage("agent must be claude or codex"))
    };
    let name = || s("name").ok_or_else(|| Error::usage("want name"));
    let r = match s("do") {
        None => Ok(()),
        Some("add") => add(
            s("agent").unwrap_or("claude"),
            name()?,
            s("config_dir").unwrap_or(""),
        ),
        Some("delete") => edit(agent()?, name()?, Edit::Remove),
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
            edit(agent()?, name()?, change)
        }
        Some(x) => return Err(Error::usage(format!("unknown do '{x}'"))),
    };
    let err = r.err().map(|e| e.message);
    if crate::settings_view::is_open() {
        crate::settings_view::show_result(match &err {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        });
    }
    let (cards, list_error) = cards();
    Ok(json!({
        "file": crate::accounts::user_file(),
        "cards": cards.iter().map(card_json).collect::<Vec<_>>(),
        "error": err,
        "list_error": list_error,
    }))
}
