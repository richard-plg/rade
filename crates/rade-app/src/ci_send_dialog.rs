//! Send CI Failure to Agent (PRD R147.3, ADR-0053): a preview over the
//! R106 scrim. The prompt of the failed jobs is built off the GTK thread
//! (`pr_status::fetch_prompt` → `ci_log::fetch_failures`) while the dialog
//! says so; then it is editable. The target line names who
//! `review::deliver` will give it to: the Worktree's (or its Branch set's)
//! only Agent, a choice when there are several, else the Preset it starts.
//! Send delivers the edited text (pasted, then Enter).
//!
//! Opened from the failing Notification's Inbox row, the Navigator
//! Worktree row's menu, the Changes card's PR chip menu, the palette and
//! Agent Overview (`Alt+F`). `debug.ui dialog_open {kind: "ci_send",
//! source, worktree}`, `dialog`, `dialog_text {text}`, `dialog_target
//! {choice}`, `dialog_primary`, `dialog_cancel` drive it.

use crate::app::{app, try_app};
use crate::changes_card::text_of;
use crate::set_dialogs::{footer, header, label, show, text_box};
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use rade_core::rpc::{Error, Result};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Who the prompt goes to.
#[derive(Clone, Debug, PartialEq)]
enum Choice {
    /// An Agent Pane: (id, `name · agent`).
    Agent(String, String),
    /// No Agent: spawn this Preset with the prompt.
    Preset(String),
}

impl Choice {
    fn label(&self) -> String {
        match self {
            Choice::Agent(_, l) => l.clone(),
            Choice::Preset(p) => format!("starts {p}"),
        }
    }
}

pub struct Ci {
    pid: String,
    wid: String,
    number: Cell<u64>,
    text: gtk::TextView,
    note: gtk::Label,
    target: gtk::Label,
    pick: Option<gtk::DropDown>,
    choices: Vec<Choice>,
    primary: gtk::Button,
    loading: Cell<bool>,
    error: RefCell<Option<String>>,
}

thread_local! {
    static OPEN: RefCell<Option<Rc<Ci>>> = const { RefCell::new(None) };
}

/// The preview is up.
pub fn is_open() -> bool {
    OPEN.with(|o| o.borrow().is_some())
}

/// Forget it (`project_dialogs::close` hides the scrim); whether it was up.
pub(crate) fn take() -> bool {
    OPEN.with(|o| o.borrow_mut().take()).is_some()
}

fn get() -> Option<Rc<Ci>> {
    OPEN.with(|o| o.borrow().clone())
}

fn close() {
    crate::project_dialogs::close();
}

/// Who `review::deliver` would pick: the Worktree's (or its set's) Agents,
/// else the Presets (`claude` first).
fn choices(pid: &str, wid: &str) -> Vec<Choice> {
    let app = app();
    let agents = crate::review::agents_for(&app, pid, wid, None);
    if !agents.is_empty() {
        return agents
            .into_iter()
            .map(|(id, l, _)| Choice::Agent(id, l))
            .collect();
    }
    let mut presets: Vec<String> = app
        .config
        .borrow()
        .presets()
        .unwrap_or_default()
        .into_keys()
        .collect();
    if let Some(i) = presets.iter().position(|p| p == "claude") {
        let c = presets.remove(i);
        presets.insert(0, c);
    }
    presets.into_iter().map(Choice::Preset).collect()
}

/// Open the preview for Worktree `wid` of Source `pid`.
pub fn open(pid: &str, wid: &str) {
    let app = app();
    let (name, number) = {
        let m = app.model.borrow();
        let Some(w) = m.worktree(pid, wid) else {
            return crate::worktrees::alert("Send CI Failure to Agent", "The Worktree is gone.");
        };
        let n = crate::pr_status::status(pid, wid)
            .map(|s| s.number)
            .or_else(|| w.pr.as_ref().map(|p| p.number))
            .unwrap_or(0);
        (w.name.clone(), n)
    };
    let dialog = gtk::Box::new(gtk::Orientation::Vertical, 0);
    dialog.add_css_class("ci-send-dialog");
    let context = if number > 0 {
        format!("{name} · #{number}")
    } else {
        name.clone()
    };
    dialog.append(&header(
        "ph-git-pull-request-symbolic",
        "Send CI Failure to Agent",
        &context,
    ));
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.add_css_class("dialog-body");
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let l = label("Prompt", &["field-label"]);
    l.set_hexpand(true);
    head.append(&l);
    head.append(&label(
        "The failed jobs' log tails; edit before sending.",
        &["field-hint"],
    ));
    body.append(&head);
    let (scroll, text) = text_box("ci-send-text", 260);
    scroll.set_max_content_height(360);
    text.add_css_class("ci-send-prompt");
    text.set_monospace(true);
    body.append(&scroll);
    let note = label(
        "Reading the failed jobs' logs\u{2026}",
        &["set-plan-summary"],
    );
    note.set_wrap(true);
    note.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    note.add_css_class("ci-send-note");
    body.append(&note);

    // The target line (R13.2).
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.add_css_class("field-row");
    row.add_css_class("ci-send-target");
    row.append(&label("Send to", &["field-label"]));
    let choices = choices(pid, wid);
    let target = label("", &["ci-send-target-text"]);
    target.set_hexpand(true);
    target.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let pick = (choices.len() > 1).then(|| {
        let labels: Vec<String> = choices.iter().map(Choice::label).collect();
        let d = gtk::DropDown::from_strings(&labels.iter().map(String::as_str).collect::<Vec<_>>());
        d.add_css_class("rade-field-agent");
        d.add_css_class("ci-send-pick");
        d
    });
    match (&pick, choices.first()) {
        (Some(p), _) => {
            target.set_label(if matches!(choices[0], Choice::Preset(_)) {
                "No Agent in this Worktree:"
            } else {
                "Several Agents:"
            });
            target.set_hexpand(false);
            row.append(&target);
            p.set_hexpand(true);
            row.append(p);
        }
        (None, Some(Choice::Agent(_, l))) => {
            target.set_label(l);
            row.append(&target);
        }
        (None, Some(Choice::Preset(p))) => {
            target.set_label(&format!("No Agent in this Worktree: starts {p}"));
            row.append(&target);
        }
        (None, None) => {
            target.set_label("No Agent and no Agent Preset to start");
            row.append(&target);
        }
    }
    body.append(&row);
    dialog.append(&body);
    let (foot, primary) = footer("The Agent gets the text pasted, then Enter.", "Send");
    dialog.append(&foot);

    let d = Rc::new(Ci {
        pid: pid.to_string(),
        wid: wid.to_string(),
        number: Cell::new(number),
        text,
        note,
        target,
        pick,
        choices,
        primary,
        loading: Cell::new(true),
        error: RefCell::default(),
    });
    let w = Rc::downgrade(&d);
    d.primary.connect_clicked(move |_| {
        if let Some(d) = w.upgrade() {
            d.send();
        }
    });
    let w = Rc::downgrade(&d);
    d.text.buffer().connect_changed(move |_| {
        if let Some(d) = w.upgrade() {
            d.validate();
        }
    });
    if !show(&dialog) {
        return;
    }
    OPEN.with(|o| *o.borrow_mut() = Some(d.clone()));
    crate::project_dialogs::sync_primaries();
    d.text.set_sensitive(false);
    d.validate();
    let w = Rc::downgrade(&d);
    crate::pr_status::fetch_prompt(pid, wid, move |r| {
        if let Some(d) = w.upgrade() {
            d.take_prompt(r);
        }
    });
}

impl Ci {
    fn take_prompt(&self, r: std::result::Result<(String, u64), String>) {
        // Closed meanwhile, or another preview took its place.
        if !get().is_some_and(|o| std::ptr::eq(&*o, self)) {
            return;
        }
        self.loading.set(false);
        match r {
            Ok((prompt, number)) => {
                self.number.set(number);
                self.text.buffer().set_text(&prompt);
                self.text.set_sensitive(true);
                self.note.set_visible(false);
                self.text.grab_focus();
                self.text
                    .buffer()
                    .place_cursor(&self.text.buffer().start_iter());
            }
            Err(e) => {
                self.note
                    .set_label(&format!("Could not read the failed jobs: {e}"));
                self.note.add_css_class("error");
                *self.error.borrow_mut() = Some(e);
            }
        }
        self.validate();
    }

    fn validate(&self) {
        let ok = !self.loading.get()
            && self.error.borrow().is_none()
            && !text_of(&self.text).trim().is_empty()
            && !self.choices.is_empty();
        self.primary.set_sensitive(ok);
    }

    fn choice(&self) -> Option<Choice> {
        let i = self.pick.as_ref().map_or(0, |p| p.selected() as usize);
        self.choices.get(i).cloned()
    }

    fn send(&self) {
        if !self.primary.is_sensitive() {
            return;
        }
        let (pane, preset) = match self.choice() {
            Some(Choice::Agent(id, _)) => (Some(id), None),
            Some(Choice::Preset(p)) => (None, Some(p)),
            None => return,
        };
        let text = text_of(&self.text);
        match crate::pr_status::deliver(
            &self.pid,
            &self.wid,
            pane,
            preset,
            self.number.get(),
            &text,
        ) {
            Ok(pane) => {
                let name = try_app()
                    .and_then(|a| a.model.borrow().pane(&pane).and_then(|p| p.name.clone()))
                    .unwrap_or(pane);
                close();
                if let Some(app) = try_app() {
                    app.chrome.toast(
                        &format!("CI failure sent to {}", glib::markup_escape_text(&name)),
                        None,
                    );
                }
            }
            Err(e) => {
                self.note.set_label(&format!("Not sent: {e}"));
                self.note.add_css_class("error");
                self.note.set_visible(true);
            }
        }
    }

    fn info(&self) -> Value {
        json!({
            "open": true,
            "kind": "ci_send",
            "source": self.pid,
            "worktree": self.wid,
            "number": self.number.get(),
            "loading": self.loading.get(),
            "error": *self.error.borrow(),
            "note": self.note.is_visible().then(|| self.note.label().to_string()),
            "text": text_of(&self.text),
            "target": match &self.pick {
                Some(_) => self.choice().map(|c| c.label()).unwrap_or_default(),
                None => self.target.label().to_string(),
            },
            "choices": self.choices.iter().map(Choice::label).collect::<Vec<_>>(),
            "primary": "Send",
            "primary_enabled": self.primary.is_sensitive(),
        })
    }
}

/// `debug.ui dialog…` while the preview is up, or `dialog_open` with
/// `kind: "ci_send"` (`source`, `worktree` = ids or names).
pub fn debug(p: &Value) -> Result<Value> {
    let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("");
    match s("op") {
        "dialog" => {}
        "dialog_open" => {
            let (pid, wid) = {
                let app = app();
                let m = app.model.borrow();
                let pid = m.find_source(s("source"))?.id.clone();
                let wid = m.find_worktree(&pid, Some(s("worktree")))?.1;
                (pid, wid)
            };
            open(&pid, &wid);
        }
        "dialog_cancel" => close(),
        op => {
            let d = get().ok_or_else(|| Error::usage("no dialog open"))?;
            match op {
                "dialog_text" => d.text.buffer().set_text(s("text")),
                "dialog_target" => {
                    let i = d
                        .choices
                        .iter()
                        .position(|c| {
                            let want = s("choice");
                            c.label() == want
                                || matches!(c, Choice::Agent(id, _) if id == want)
                                || matches!(c, Choice::Preset(p) if p == want)
                        })
                        .ok_or_else(|| Error::not_found(format!("no target '{}'", s("choice"))))?;
                    if let Some(pk) = &d.pick {
                        pk.set_selected(i as u32);
                    }
                }
                "dialog_primary" => {
                    if !d.primary.is_sensitive() {
                        return Err(Error::usage("the primary button is disabled"));
                    }
                    d.send();
                }
                op => return Err(Error::usage(format!("unknown debug.ui op '{op}'"))),
            }
        }
    }
    let v = get().map_or_else(|| json!({"open": false}), |d| d.info());
    Ok(json!({ "dialog": v }))
}
