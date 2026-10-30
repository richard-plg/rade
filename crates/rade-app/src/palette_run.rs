//! What the command palette does on Enter (R14.4): focus a Source /
//! Worktree / Tab / Pane, open a file in the Viewer, run an action, or show
//! an action's inline form and submit it; plus `debug.palette`.

use crate::app::app;
use crate::handlers::{on_main, params, Spawn};
use crate::palette::{get, Palette};
use crate::worktrees::in_background;
use gtk::prelude::*;
use gtk4 as gtk;
use rade_core::palette::{self, Target, BROWSER_PREFIX, SPAWN_PREFIX};
use rade_core::palette_form::{self, FieldKind, Form, FormContext};
use rade_core::rpc::{Error, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

/// A form on screen: the model plus its widgets.
pub struct FormView {
    pub form: Form,
    widgets: Vec<(&'static str, gtk::Widget)>,
    error: gtk::Label,
    submit: gtk::Button,
}

fn context() -> (Vec<String>, bool) {
    let app = app();
    let presets = app
        .config
        .borrow()
        .presets()
        .unwrap_or_default()
        .into_keys()
        .collect();
    let general = app.model.borrow().active().0.is_general();
    (presets, general)
}

impl FormView {
    pub fn build(form: Form, into: &gtk::Box) -> Self {
        let title = gtk::Label::new(Some(&form.title));
        title.set_xalign(0.0);
        title.add_css_class("heading");
        title.set_margin_start(6);
        title.set_margin_top(4);
        into.append(&title);
        let mut widgets = Vec::new();
        for f in &form.fields {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.set_margin_start(6);
            row.set_margin_end(6);
            let l = gtk::Label::new(Some(f.label));
            l.set_width_chars(7);
            l.set_xalign(0.0);
            row.append(&l);
            let key = f.key;
            let w: gtk::Widget = match &f.kind {
                FieldKind::Text => {
                    let e = gtk::Entry::new();
                    e.set_placeholder_text(Some(&f.placeholder));
                    e.set_text(&f.value);
                    e.connect_changed(move |e| set_field(key, &e.text()));
                    e.connect_activate(|_| {
                        crate::palette::key("enter");
                    });
                    e.upcast()
                }
                FieldKind::Counts(opts) => counts_widget(key, opts, &f.value),
                FieldKind::Choice(opts) => {
                    let strs: Vec<&str> = opts.iter().map(String::as_str).collect();
                    let d = gtk::DropDown::from_strings(&strs);
                    let opts = opts.clone();
                    d.connect_selected_notify(move |d| {
                        if let Some(v) = opts.get(d.selected() as usize) {
                            set_field(key, v);
                        }
                    });
                    d.upcast()
                }
            };
            w.set_hexpand(true);
            w.add_css_class(&format!("rade-palette-field-{key}"));
            row.append(&w);
            into.append(&row);
            widgets.push((key, w));
        }
        let error = gtk::Label::new(None);
        error.set_xalign(0.0);
        error.set_wrap(true);
        error.add_css_class("error");
        error.add_css_class("caption");
        error.set_margin_start(6);
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        bar.set_margin_start(6);
        bar.set_margin_end(6);
        bar.set_margin_bottom(4);
        let hint = gtk::Label::new(Some("Enter: run · Tab: next field · Esc: back"));
        hint.add_css_class("dim-label");
        hint.add_css_class("caption");
        hint.set_hexpand(true);
        hint.set_xalign(0.0);
        let submit = gtk::Button::with_label(form.submit);
        submit.add_css_class("suggested-action");
        submit.connect_clicked(|_| {
            if let Some(p) = get() {
                self::submit(&p);
            }
        });
        bar.append(&hint);
        bar.append(&submit);
        into.append(&error);
        into.append(&bar);
        let v = Self {
            form,
            widgets,
            error,
            submit,
        };
        v.check(false);
        v
    }

    pub fn focus_first(&self) {
        if let Some((_, w)) = self.widgets.first() {
            w.grab_focus();
        }
    }

    /// Tab past the end wraps to the first field, Shift+Tab before the
    /// start to the submit button.
    pub fn focus_end(&self, first: bool) {
        if first {
            self.focus_first();
        } else {
            self.submit.grab_focus();
        }
    }

    /// Show why it can't be submitted yet; a fresh form stays quiet until
    /// `submitting`.
    fn check(&self, submitting: bool) -> bool {
        let (presets, general) = context();
        let ctx = FormContext {
            presets: &presets,
            general,
        };
        let r = self.form.validate(&ctx);
        let msg = match &r {
            Err(e) if submitting || !self.form.pristine() || general => e.as_str(),
            _ => "",
        };
        self.error.set_label(msg);
        self.error.set_visible(!msg.is_empty());
        self.submit.set_sensitive(r.is_ok());
        r.is_ok()
    }

    pub fn info(&self) -> Value {
        let fields: Vec<Value> = self
            .form
            .fields
            .iter()
            .map(|f| json!({"key": f.key, "label": f.label, "kind": crate::palette::field_kind(&f.kind), "value": f.value}))
            .collect();
        let focus = gtk::prelude::GtkWindowExt::focus(&app().window);
        let focused = self
            .widgets
            .iter()
            .find(|(_, w)| focus.as_ref().is_some_and(|f| f == w || f.is_ancestor(w)))
            .map(|(k, _)| *k);
        json!({"action": self.form.action, "title": self.form.title, "fields": fields,
               "error": self.error.label().as_str(), "can_submit": self.submit.is_sensitive(), "focus": focused})
    }
}

/// Set a form field as the user would (`debug.palette field key=value`):
/// through its widget, so the same signals run.
fn put(p: &Palette, key: &str, value: &str) -> Result<()> {
    let (w, kind) = {
        let f = p.form.borrow();
        let v = f.as_ref().ok_or_else(|| Error::usage("no form open"))?;
        let w = v
            .widgets
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, w)| w.clone());
        let kind = v
            .form
            .fields
            .iter()
            .find(|f| f.key == key)
            .map(|f| f.kind.clone());
        w.zip(kind)
            .ok_or_else(|| Error::usage(format!("no field '{key}'")))?
    };
    if let FieldKind::Counts(_) = kind {
        let mut f = p.form.borrow().as_ref().unwrap().form.clone();
        f.set(key, value).map_err(Error::usage)?;
        let agents = f.agents(key);
        for (name, spin) in spins(&w) {
            spin.set_value(agents.iter().filter(|a| **a == name).count() as f64);
        }
        return Ok(());
    }
    match (w.downcast_ref::<gtk::DropDown>(), kind) {
        (Some(d), FieldKind::Choice(opts)) => {
            let i = opts.iter().position(|o| o == value).ok_or_else(|| {
                Error::usage(format!("{key} must be one of: {}", opts.join(", ")))
            })?;
            d.set_selected(i as u32);
        }
        _ => w.downcast_ref::<gtk::Entry>().unwrap().set_text(value),
    }
    Ok(())
}

/// A Counts field (Fan-out's Agents, R15.1): a spin button (0–8) per
/// Preset; the value is `preset=n,…` of the non-zero ones.
fn counts_widget(key: &'static str, opts: &[String], value: &str) -> gtk::Widget {
    // Four Presets per row, so many Presets wrap instead of widening it.
    let b = gtk::Grid::new();
    b.set_column_spacing(6);
    b.set_row_spacing(4);
    for (i, o) in opts.iter().enumerate() {
        let n = value
            .split(',')
            .filter_map(|p| p.split_once('='))
            .find(|(k, _)| k.trim() == o)
            .and_then(|(_, n)| n.trim().parse::<f64>().ok())
            .unwrap_or(0.0);
        let l = gtk::Label::new(Some(o));
        let s = gtk::SpinButton::with_range(0.0, 8.0, 1.0);
        s.set_value(n);
        s.set_numeric(true);
        s.set_widget_name(o);
        s.connect_value_changed(move |s| {
            let Some(parent) = s.parent() else { return };
            let v: Vec<String> = spins(&parent)
                .into_iter()
                .filter(|(_, s)| s.value_as_int() > 0)
                .map(|(n, s)| format!("{n}={}", s.value_as_int()))
                .collect();
            set_field(key, &v.join(","));
        });
        l.set_xalign(1.0);
        let (row, col) = ((i / 4) as i32, (i % 4) as i32 * 2);
        b.attach(&l, col, row, 1, 1);
        b.attach(&s, col + 1, row, 1, 1);
    }
    b.upcast()
}

/// (Preset, spin button) pairs of a Counts field.
fn spins(w: &gtk::Widget) -> Vec<(String, gtk::SpinButton)> {
    let mut out = Vec::new();
    let mut c = w.first_child();
    while let Some(x) = c {
        if let Some(s) = x.downcast_ref::<gtk::SpinButton>() {
            out.push((s.widget_name().to_string(), s.clone()));
        }
        c = x.next_sibling();
    }
    out
}

fn set_field(key: &str, value: &str) {
    let Some(p) = get() else { return };
    let mut f = p.form.borrow_mut();
    if let Some(v) = f.as_mut() {
        let _ = v.form.set(key, value);
        v.check(false);
    }
}

/// Open the palette on the inline form of action `id` (the `+` menu's
/// `Fan-out…`, R108.6).
pub fn open_form(id: &str) {
    let Some(p) = crate::palette::get() else {
        return;
    };
    if !p.open.get() {
        crate::palette::toggle();
    }
    if !p.open.get() {
        return;
    }
    let (presets, general) = context();
    let ctx = FormContext {
        presets: &presets,
        general,
    };
    if let Some(form) = palette_form::form_for(id, &ctx) {
        p.show_form(form);
    }
}

/// Enter on the list: run the selected row (an action that takes input
/// shows its form instead).
/// `Ctrl+Enter`: a file row opens to the side (R113.2); any other row
/// runs like Enter.
pub fn run_selected_side(p: &Palette) {
    let target = {
        let (items, hits) = (p.items.borrow(), p.hits.borrow());
        p.selected_hit()
            .and_then(|i| hits.get(i))
            .map(|h| items[h.index].target.clone())
    };
    match target {
        Some(Target::File { path }) => {
            if !palette::key_exists(
                &app().model.borrow(),
                &Target::File { path: path.clone() }.key(),
            ) {
                return p.say("That is gone (closed since the palette opened)");
            }
            app().touch(|m| {
                palette::remember(
                    &mut m.palette_recent,
                    Target::File { path: path.clone() }.key(),
                )
            });
            p.close(true);
            crate::files::open_side(&path);
        }
        _ => run_selected(p),
    }
}

pub fn run_selected(p: &Palette) {
    let target = {
        let (items, hits) = (p.items.borrow(), p.hits.borrow());
        let Some(h) = p.selected_hit().and_then(|i| hits.get(i)) else {
            return;
        };
        items[h.index].target.clone()
    };
    let app = app();
    if !palette::key_exists(&app.model.borrow(), &target.key()) {
        return p.say("That is gone (closed since the palette opened)");
    }
    if let Target::Action { id } = &target {
        // `Fan-out on <source>…`: that Source on screen, then the form.
        if let Some(src) = id.strip_prefix(palette::FANOUT_ON_PREFIX) {
            app.activate_source(src);
            let (presets, general) = context();
            let ctx = FormContext {
                presets: &presets,
                general,
            };
            if let Some(form) = palette_form::form_for("fanout", &ctx) {
                return p.show_form(form);
            }
        }
        let (presets, general) = context();
        let ctx = FormContext {
            presets: &presets,
            general,
        };
        if let Some(form) = palette_form::form_for(id, &ctx) {
            return p.show_form(form); // remembered when submitted
        }
    }
    app.touch(|m| palette::remember(&mut m.palette_recent, target.key()));
    // Focus back to the Pane first: the target then moves it on (or not).
    p.close(true);
    run(&target);
}

fn active() -> (String, String) {
    let app = app();
    let m = app.model.borrow();
    let (p, w) = m.active();
    (p.id.clone(), w.id.clone())
}

/// Run a target (the palette is already closed).
pub fn run(t: &Target) {
    let app = app();
    match t {
        Target::Source { source } => app.activate_source(source),
        // A link: a Terminal on its checkout (R107.3, `open_on_link`).
        Target::Link { project, source } => crate::home_view::open_terminal(project, source),
        Target::Project { project } => crate::home_view::open(project),
        Target::Worktree { source, worktree } => crate::worktrees::activate(source, worktree),
        Target::Tab { tab } => {
            let _ = app.update(|m| {
                let loc = m.locate_tab(tab);
                let focused =
                    loc.and_then(|(p, w)| m.worktree(&p, &w)?.tab(tab).map(|t| t.focused.clone()));
                focused.map(|f| m.focus_pane(&f))
            });
        }
        Target::Pane { pane } => {
            let _ = app.update(|m| m.focus_pane(pane));
        }
        Target::File { path } => crate::files::open_in_viewer(path),
        Target::Symbol { path, line, col } => crate::code::open_at(path, *line, *col),
        Target::Action { id } => match id.as_str() {
            "mark_all_read" => app.update(|m| m.mark_all_read()),
            "clear_notifications" => app.update(|m| m.clear_notifications()),
            "new_project" => crate::project_dialogs::new_project(),
            "prune_project_checkouts" => crate::projects::prune_dialog(),
            id if id.starts_with(palette::LINK_ADD_PREFIX) => {
                crate::project_dialogs::add_to_project(&id[palette::LINK_ADD_PREFIX.len()..])
            }
            id if id.starts_with(palette::LINK_BASE_PREFIX) => {
                let rest = &id[palette::LINK_BASE_PREFIX.len()..];
                if let Some((p, s)) = rest.split_once('/') {
                    let ws = app.model.borrow().active_workspace().to_string();
                    crate::project_dialogs::change_base(&format!("{ws}/{p}"), s);
                }
            }
            id if id.starts_with(SPAWN_PREFIX) => {
                let (source, worktree) = active();
                spawn(Spawn {
                    agent: Some(id[SPAWN_PREFIX.len()..].to_string()),
                    source: Some(source),
                    worktree: Some(worktree),
                    tab: Some("new".into()),
                    ..Default::default()
                });
            }
            id if id.starts_with(BROWSER_PREFIX) => {
                crate::browser::open_browser(Some(id[BROWSER_PREFIX.len()..].to_string()))
            }
            id => crate::keys::run_action(id),
        },
    }
}

/// Spawn and focus the new Pane; an error is shown in a dialog.
fn spawn(s: Spawn) {
    let app = app();
    match crate::handlers::spawn(&app, s) {
        Ok(v) => {
            if let Some(p) = v["pane"].as_str() {
                let _ = app.update(|m| m.focus_pane(p));
            }
        }
        Err(e) => crate::worktrees::alert("Could not spawn the Agent", &e.message),
    }
}

/// Enter in a form: check it, then run its action.
pub fn submit(p: &Palette) {
    // Digits typed into a count but not applied yet (Enter submits from
    // there): apply them first; their signal updates the form.
    let ws: Vec<gtk::Widget> = p
        .form
        .borrow()
        .as_ref()
        .map(|v| v.widgets.iter().map(|(_, w)| w.clone()).collect())
        .unwrap_or_default();
    for w in ws {
        for (_, s) in spins(&w) {
            s.update();
        }
    }
    let form = {
        let f = p.form.borrow();
        let Some(v) = f.as_ref() else { return };
        if !v.check(true) {
            return;
        }
        v.form.clone()
    };
    let key = Target::Action {
        id: form.action.clone(),
    }
    .key();
    app().touch(|m| palette::remember(&mut m.palette_recent, key));
    p.close_form();
    p.close(true);
    let (source, worktree) = active();
    let v = |k: &str| {
        let s = form.value(k).trim();
        (!s.is_empty()).then(|| s.to_string())
    };
    match form.action.as_str() {
        "spawn_agent" => {
            let mut s = Spawn {
                agent: v("agent"),
                prompt: v("prompt"),
                source: Some(source),
                ..Default::default()
            };
            match form.value("where") {
                palette_form::WHERE_SPLIT => {
                    s.worktree = Some(worktree);
                    s.split = Some("right".into());
                }
                palette_form::WHERE_WORKTREE => {
                    s.worktree = Some("new".into());
                    return in_background(
                        move || crate::worktrees::spawn_in_new_worktree(s),
                        |r| match r {
                            Ok(v) => {
                                if let Some(p) = v["pane"].as_str() {
                                    let _ = app().update(|m| m.focus_pane(p));
                                }
                            }
                            Err(e) => {
                                crate::worktrees::alert("Could not spawn the Agent", &e.message)
                            }
                        },
                    );
                }
                _ => {
                    s.worktree = Some(worktree);
                    s.tab = Some("new".into());
                }
            }
            spawn(s);
        }
        "new_worktree" => {
            let (name, base) = (v("name"), v("base"));
            in_background(
                move || {
                    crate::worktrees::create(&source, name.as_deref(), base.as_deref(), None, None)
                },
                |r| match r {
                    Ok(c) => crate::worktrees::activate(&c.source, &c.worktree),
                    Err(e) => crate::worktrees::alert("Could not create the Worktree", &e.message),
                },
            );
        }
        "fanout" => {
            let start = crate::fanout::StartParams {
                agents: form.agents("agents"),
                prompt: form.value("prompt").trim().to_string(),
                source: Some(source),
                base: v("base"),
                name: v("name"),
                caller: None,
            };
            in_background(
                move || crate::fanout::start(start),
                |r| match r {
                    // Show the Fan-out Tab, keyboard on its first Agent.
                    Ok(v) => {
                        if let Some(p) = v["panes"][0].as_str() {
                            let _ = app().update(|m| m.focus_pane(p));
                        }
                    }
                    Err(e) => crate::worktrees::alert("Could not start the Fan-out", &e.message),
                },
            );
        }
        // R25.6: setup runs in a visible Terminal (the gh login prompts there).
        "create_workspace" => spawn(Spawn {
            cmd: Some(palette_form::setup_command(&form)),
            name: Some("workspace setup".into()),
            source: Some("general".into()),
            tab: Some("new".into()),
            ..Default::default()
        }),
        a => eprintln!("rade: palette form '{a}' has no handler"),
    }
}

#[derive(Deserialize)]
struct DebugParams {
    query: Option<String>,
    op: Option<String>,
    arg: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    20
}

/// `debug.palette` (socket thread). `{"query"}`: the ranked results for a
/// query, computed like the open palette does (model items + the active
/// Worktree's files) without showing it. `{"op", "arg"?}`: drive the
/// overlay — `info`, `type <text>` (the search entry), `key
/// up|down|page_up|page_down|enter|escape|tab|shift+tab`, `field
/// <key>=<value>` (a form field, as typed/picked).
pub fn debug(v: Value) -> Result<Value> {
    let d: DebugParams = params(v)?;
    let Some(op) = d.op else {
        let query = d.query.unwrap_or_default();
        let (mut items, recent, root) = on_main(|| {
            let (items, root) = crate::palette::gather();
            Ok((items, app().model.borrow().palette_recent.clone(), root))
        })?;
        let t = Instant::now();
        let (files, cut) = match &root {
            Some(r) => palette::file_items(r, crate::palette::FILE_CAP),
            None => (vec![], false),
        };
        let walk_ms = t.elapsed().as_secs_f64() * 1e3;
        let n_files = files.len();
        items.extend(files);
        let t = Instant::now();
        let ranked = palette::rank(&items, &query, &recent, d.limit, &|| false);
        let (hits, total) = ranked.map(|r| (r.hits, r.total)).unwrap_or_default();
        let rank_ms = t.elapsed().as_secs_f64() * 1e3;
        let (filter, text) = palette::parse_query(&query);
        let results: Vec<Value> = hits
            .iter()
            .map(|h| crate::palette::row_json(&items[h.index], h))
            .collect();
        return Ok(json!({"filter": filter, "text": text, "results": results,
            "items": items.len(), "total": total, "files": n_files, "files_truncated": cut,
            "walk_ms": walk_ms, "rank_ms": rank_ms}));
    };
    on_main(move || {
        let p = get().ok_or_else(|| Error::internal("no palette"))?;
        let arg = d.arg.unwrap_or_default();
        match op.as_str() {
            "info" => {}
            "type" => {
                if !p.open.get() {
                    return Err(Error::usage("the palette is not open"));
                }
                p.entry.set_text(&arg);
                p.entry.set_position(-1);
            }
            "key" => {
                if !crate::palette::key(&arg) {
                    return Err(Error::usage(format!("key '{arg}' not handled")));
                }
            }
            "field" => {
                let (k, val) = arg
                    .split_once('=')
                    .ok_or_else(|| Error::usage("field needs <key>=<value>"))?;
                put(&p, k, val)?;
            }
            o => return Err(Error::usage(format!("unknown palette op '{o}'"))),
        }
        Ok(p.info())
    })
}
