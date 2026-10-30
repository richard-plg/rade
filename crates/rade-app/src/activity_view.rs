//! Activity in the app (PRD R107.2): one in-memory
//! [`rade_core::activity::Activity`], fed by Agent status changes (waiting /
//! done, with the Agent's own last message), PRs created and commits from
//! the Changes page; the Inspector's Activity tab lists it and Project home
//! reads its Project's newest events (R105.4). Nothing is written.

use crate::app::try_app;
use gtk::glib;
use gtk4 as gtk;
use gtk4::prelude::*;
use rade_core::activity::{ago, Activity, Event, Kind};
use rade_core::{AgentStatus, State};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

/// How long after a status change its event is written: an Agent's turn
/// message (OSC 9) arrives just after its `done` hook.
const SETTLE: Duration = Duration::from_millis(1000);

thread_local! {
    static LIST: RefCell<Activity> = RefCell::default();
    /// Agent statuses at the last update (`None` = not learnt yet).
    static PREV: RefCell<Option<HashMap<String, AgentStatus>>> = const { RefCell::new(None) };
    /// Each Pane's last own message (OSC 9 / 777 text) and when.
    static MSG: RefCell<HashMap<String, (String, u64)>> = RefCell::default();
    static PAGE: RefCell<Option<gtk::Box>> = const { RefCell::new(None) };
}

/// A Terminal's own message (OSC), the text a waiting / done event quotes.
pub fn message(pane: &str, text: &str) {
    MSG.with(|m| {
        m.borrow_mut()
            .insert(pane.into(), (text.into(), crate::panes::now()))
    });
}

/// After every update: an event for each Agent that went waiting or done.
pub fn after_update(ws: &State) {
    let now: HashMap<String, AgentStatus> = ws
        .worktrees()
        .flat_map(|(_, w)| w.tabs.iter().flat_map(|t| t.panes()))
        .filter(|p| p.agent.is_some())
        .filter_map(|p| p.status.map(|s| (p.id.clone(), s)))
        .collect();
    let Some(before) = PREV.with(|p| p.replace(Some(now.clone()))) else {
        return; // the first update only learns (Session Restore)
    };
    let at = crate::panes::now();
    for (pane, s) in now {
        if before.get(&pane) == Some(&s) || !matches!(s, AgentStatus::Waiting | AgentStatus::Done) {
            continue;
        }
        glib::timeout_add_local_once(SETTLE, move || status_event(&pane, s, at));
    }
}

fn status_event(pane: &str, s: AgentStatus, at: u64) {
    let Some(app) = try_app() else { return };
    let m = app.model.borrow();
    let Some(p) = m.pane(pane) else { return };
    let name = p
        .name
        .clone()
        .or_else(|| p.agent.clone())
        .unwrap_or_else(|| pane.to_string());
    let msg = MSG.with(|m| {
        m.borrow()
            .get(pane)
            .filter(|(_, t)| *t + 10 >= at)
            .map(|(x, _)| x.clone())
    });
    let e = match s {
        AgentStatus::Waiting => Event::waiting(&name, msg.as_deref(), at),
        _ => Event::done(&name, msg.as_deref(), at),
    };
    let project = project_of_pane(&m, pane);
    drop(m);
    push(e.in_project(project).on_pane(Some(pane.to_string())));
}

/// The Project a Pane is shown under: its Worktree's recorded Project, else
/// the first Project (shown order) whose link holds it.
pub fn project_of_pane(m: &State, pane: &str) -> Option<String> {
    let links = crate::navdata::links(m);
    crate::navdata::projects(m)
        .into_iter()
        .find(|p| {
            rade_core::navigator::project_panes(m, &p.id, &links, &crate::projects::canon)
                .iter()
                .any(|x| x.id == pane)
        })
        .map(|p| p.id)
}

/// The Project of the Worktree at `path` (a PR or commit there).
pub fn project_of_path(m: &State, path: &std::path::Path) -> Option<(String, Option<String>)> {
    let (s, w) = m
        .worktrees()
        .find(|(_, w)| rade_core::git::same(&w.path, path))?;
    let project = w.project.clone().or_else(|| {
        crate::navdata::links(m)
            .iter()
            .find(|l| {
                l.source == s.id
                    && l.path
                        .as_deref()
                        .is_some_and(|p| rade_core::git::same(p, path))
            })
            .map(|l| l.project.clone())
    });
    Some((w.name.clone(), project))
}

/// A PR was created from the checkout at `path` (`url` from `gh`).
pub fn pr_created(path: &std::path::Path, url: &str) {
    let Some(app) = try_app() else { return };
    let digits: String = url
        .trim()
        .trim_end_matches('/')
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let Ok(n) = digits.parse::<u64>() else { return };
    let found = project_of_path(&app.model.borrow(), path);
    let (wt, project) = found.unwrap_or_else(|| (path.display().to_string(), None));
    push(Event::pr(n, &wt, crate::panes::now()).in_project(project));
}

/// A commit was made in the checkout at `path`.
pub fn committed(path: &std::path::Path, source: &str, short: &str, subject: &str) {
    let Some(app) = try_app() else { return };
    let project = project_of_path(&app.model.borrow(), path).and_then(|x| x.1);
    push(Event::commit(source, short, subject, crate::panes::now()).in_project(project));
}

pub fn push(e: Event) {
    LIST.with(|l| l.borrow_mut().push(e));
    render();
    crate::home_view::refresh();
}

/// A Project's newest `n` events.
pub fn of_project(project: &str, n: usize) -> Vec<Event> {
    LIST.with(|l| {
        l.borrow()
            .of_project(project, n)
            .into_iter()
            .cloned()
            .collect()
    })
}

fn kind_str(k: Kind) -> &'static str {
    match k {
        Kind::Waiting => "waiting",
        Kind::Done => "done",
        Kind::PrOpened => "pr_opened",
        Kind::Commit => "commit",
        Kind::Limit => "limit",
        Kind::Update => "update",
        Kind::Ignored => "ignored",
        Kind::Schedule => "schedule",
    }
}

/// An event's leading mark: the LED of a status, else an icon.
pub fn mark(k: Kind) -> gtk::Widget {
    match k {
        Kind::Waiting | Kind::Done => {
            let led = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            led.add_css_class("led");
            led.add_css_class(if k == Kind::Waiting {
                "led-waiting"
            } else {
                "led-done"
            });
            led.set_valign(gtk::Align::Center);
            led.upcast()
        }
        Kind::PrOpened => icon("ph-git-pull-request-symbolic", "activity-pr"),
        Kind::Commit => icon("ph-git-branch-symbolic", "activity-commit"),
        Kind::Limit => icon("ph-gauge-symbolic", "activity-limit"),
        Kind::Update => icon("ph-arrow-circle-up-symbolic", "activity-update"),
        Kind::Ignored => icon("ph-robot-bold-symbolic", "activity-ignored"),
        Kind::Schedule => icon("ph-clock-symbolic", "activity-schedule"),
    }
}

fn icon(name: &str, class: &str) -> gtk::Widget {
    let i = gtk::Image::from_icon_name(name);
    i.add_css_class("activity-icon");
    i.add_css_class(class);
    i.upcast()
}

/// One row: mark, actor, text, age (spec B.35); a click opens its Pane.
pub fn row(e: &Event, now: u64, class: &str) -> gtk::Widget {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    b.add_css_class(class);
    b.append(&mark(e.kind));
    let actor = gtk::Label::new(Some(&e.actor));
    actor.add_css_class("activity-actor");
    b.append(&actor);
    let text = gtk::Label::new(Some(&e.text));
    text.add_css_class("activity-text");
    text.set_xalign(0.0);
    text.set_hexpand(true);
    text.set_ellipsize(gtk::pango::EllipsizeMode::End);
    b.append(&text);
    let t = gtk::Label::new(Some(&ago(now.saturating_sub(e.at))));
    t.add_css_class("activity-time");
    b.append(&t);
    if let Some(pane) = e.pane.clone() {
        let click = gtk::GestureClick::new();
        click.connect_released(move |_, _, _, _| crate::home_view::focus_pane(&pane));
        b.add_controller(click);
    }
    b.upcast()
}

/// The Inspector's Activity page (R107.2).
pub fn install() -> gtk::ScrolledWindow {
    let list = gtk::Box::new(gtk::Orientation::Vertical, 0);
    list.add_css_class("activity-list");
    let sc = gtk::ScrolledWindow::new();
    sc.set_hscrollbar_policy(gtk::PolicyType::Never);
    sc.set_vexpand(true);
    sc.set_child(Some(&list));
    sc.add_css_class("inspector-page");
    PAGE.with(|p| *p.borrow_mut() = Some(list));
    render();
    // Ages stay fresh.
    glib::timeout_add_seconds_local(30, || {
        render();
        glib::ControlFlow::Continue
    });
    sc
}

fn render() {
    let Some(list) = PAGE.with(|p| p.borrow().clone()) else {
        return;
    };
    while let Some(c) = list.first_child() {
        list.remove(&c);
    }
    let now = crate::panes::now();
    let empty = LIST.with(|l| {
        let l = l.borrow();
        for e in l.iter() {
            list.append(&row(e, now, "activity-row"));
        }
        l.is_empty()
    });
    if empty {
        list.append(&crate::chrome::empty_state(
            &rade_core::chrome::EMPTY_ACTIVITY,
        ));
    }
}

/// `debug.ui activity`: the rows, newest first.
pub fn debug() -> Value {
    let now = crate::panes::now();
    LIST.with(|l| {
        json!(l
            .borrow()
            .iter()
            .map(|e| json!({
                "kind": kind_str(e.kind), "actor": e.actor, "text": e.text,
                "line": e.line(), "ago": ago(now.saturating_sub(e.at)),
                "project": e.project, "pane": e.pane,
            }))
            .collect::<Vec<_>>())
    })
}
