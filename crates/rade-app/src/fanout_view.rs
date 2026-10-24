//! The Fan-out bar (PRD R15.2–R15.4) above a Fan-out Tab's Agents: the
//! prompt, progress (`2/3 done`), the Judge's recommendation, one chip per
//! Agent (status glyph, files / +/− against the base, Diff = its Review
//! Pane), and Judge / Choose Winner… / Discard… buttons, plus Continue when
//! a restart interrupted some Agents (M11).

use crate::app::{app, try_app};
use crate::fanout;
use crate::worktrees::{alert, in_background};
use adw::prelude::*;
use gtk::glib;
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::fanout::{Fanout, Phase, Strategy};
use rade_core::rpc::{Error, Result};
use rade_core::Tab;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

struct Chip {
    label: String,
    dot: crate::status::Glyph,
    stats: gtk::Label,
    root: gtk::Box,
}

pub struct Bar {
    fanout: String,
    source: String,
    pub root: gtk::Box,
    title: gtk::Label,
    progress: gtk::Label,
    verdict: gtk::Label,
    chips: Vec<Chip>,
    judge: gtk::Button,
    choose: gtk::Button,
    discard: gtk::Button,
    resume: gtk::Button,
    /// label → (files, added, removed), refreshed in the background.
    stats: RefCell<HashMap<String, (usize, u32, u32)>>,
    /// Finished Agents when the sizes were last asked for.
    seen_done: std::cell::Cell<usize>,
    /// A size refresh is running (slow repos must not pile them up).
    measuring: std::cell::Cell<bool>,
}

thread_local! {
    /// Fan-out Tab id → its bar.
    static BARS: RefCell<HashMap<String, Rc<Bar>>> = Default::default();
}

fn button(label: &str, class: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.add_css_class(class);
    b.set_tooltip_text(Some(tip));
    b.set_valign(gtk::Align::Center);
    b
}

/// The bar of a Fan-out Tab, made on first use.
pub fn bar(tab: &str, f: &Fanout) -> gtk::Widget {
    if let Some(b) = BARS.with(|b| b.borrow().get(tab).cloned()) {
        return b.root.clone().upcast();
    }
    let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
    root.add_css_class("rade-fanout-bar");
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let icon = gtk::Image::from_icon_name("view-grid-symbolic");
    let title = gtk::Label::new(None);
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.add_css_class("rade-fanout-title");
    let progress = gtk::Label::new(None);
    progress.add_css_class("rade-fanout-progress");
    let verdict = gtk::Label::new(None);
    verdict.add_css_class("rade-fanout-verdict");
    let judge = button(
        "Judge",
        "rade-fanout-judge",
        "Ask a read-only Judge Agent to compare the diffs",
    );
    let choose = button("Choose Winner…", "rade-fanout-choose", "Keep one result");
    choose.add_css_class("suggested-action");
    let discard = button(
        "Discard…",
        "rade-fanout-discard",
        "Remove every Worktree of this Fan-out",
    );
    let resume = button(
        "Continue",
        "rade-fanout-continue",
        "A restart cut these Agents' turns short: ask them to continue",
    );
    for w in [
        icon.upcast_ref::<gtk::Widget>(),
        title.upcast_ref(),
        progress.upcast_ref(),
        verdict.upcast_ref(),
        resume.upcast_ref(),
        judge.upcast_ref(),
        choose.upcast_ref(),
        discard.upcast_ref(),
    ] {
        head.append(w);
    }
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let mut chips = Vec::new();
    for m in &f.members {
        let chip = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        chip.add_css_class("rade-fanout-chip");
        let dot = crate::status::Glyph::new();
        let name = gtk::Label::new(Some(&m.label));
        name.add_css_class("rade-fanout-name");
        let stats = gtk::Label::new(Some("…"));
        stats.add_css_class("rade-fanout-stats");
        let diff = gtk::Button::with_label("Diff");
        diff.add_css_class("flat");
        diff.add_css_class("rade-fanout-diff");
        diff.set_tooltip_text(Some(&format!(
            "Review {}'s changes ({})",
            m.label, m.worktree
        )));
        let (source, wt) = (f.source.clone(), m.worktree.clone());
        diff.connect_clicked(move |_| open_diff(&source, &wt));
        chip.append(&dot.root);
        chip.append(&name);
        chip.append(&stats);
        chip.append(&diff);
        chip.set_tooltip_text(Some(&format!(
            "{} · Worktree {} · Pane {}",
            m.agent, m.worktree, m.pane
        )));
        row.append(&chip);
        chips.push(Chip {
            label: m.label.clone(),
            dot,
            stats,
            root: chip,
        });
    }
    root.append(&head);
    root.append(&row);
    let b = Rc::new(Bar {
        fanout: f.id.clone(),
        source: f.source.clone(),
        root,
        title,
        progress,
        verdict,
        chips,
        judge,
        choose,
        discard,
        resume,
        stats: Default::default(),
        seen_done: Default::default(),
        measuring: Default::default(),
    });
    let (id, p) = (f.id.clone(), f.source.clone());
    b.judge.connect_clicked(move |_| run_judge(&id, &p));
    let (id, p) = (f.id.clone(), f.source.clone());
    b.choose
        .connect_clicked(move |_| choose_dialog(&id, &p, None));
    let (id, p) = (f.id.clone(), f.source.clone());
    b.discard.connect_clicked(move |_| discard_dialog(&id, &p));
    let id = f.id.clone();
    b.resume.connect_clicked(move |_| continue_interrupted(&id));
    BARS.with(|m| m.borrow_mut().insert(tab.to_string(), b.clone()));
    refresh_stats(&b);
    // Diff sizes change while Agents work: refresh while it is on screen.
    let weak = Rc::downgrade(&b);
    glib::timeout_add_local(Duration::from_secs(4), move || {
        let Some(b) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if b.root.is_mapped() {
            refresh_stats(&b);
        }
        glib::ControlFlow::Continue
    });
    b.root.clone().upcast()
}

/// Forget the bar of a closed Tab.
pub fn drop_bar(tab: &str) {
    BARS.with(|b| b.borrow_mut().remove(tab));
}

fn refresh_stats(b: &Rc<Bar>) {
    if b.measuring.replace(true) {
        return;
    }
    let (id, source) = (b.fanout.clone(), b.source.clone());
    let weak = Rc::downgrade(b);
    in_background(
        move || fanout::member_stats(&id, &source),
        move |r| {
            let Some(b) = weak.upgrade() else { return };
            b.measuring.set(false);
            let Ok(stats) = r else { return };
            *b.stats.borrow_mut() = stats.into_iter().collect();
            if let Some(app) = try_app() {
                let m = app.model.borrow();
                if let (Some(f), Some(t)) = (
                    m.fanout(&b.fanout),
                    m.fanout(&b.fanout).and_then(|f| {
                        let (p, w) = m.locate_tab(&f.tab)?;
                        m.worktree(&p, &w)?.tab(&f.tab)
                    }),
                ) {
                    b.update(f, t);
                }
            }
        },
    );
}

fn stats_text(s: Option<&(usize, u32, u32)>) -> String {
    match s {
        Some((0, _, _)) => "no changes".into(),
        Some((n, a, r)) => format!("{n} file{} +{a} −{r}", if *n == 1 { "" } else { "s" }),
        None => "…".into(),
    }
}

impl Bar {
    fn update(&self, f: &Fanout, tab: &Tab) {
        let first = f.prompt.lines().next().unwrap_or("");
        self.title.set_markup(&format!(
            "<b>{}</b>  {}",
            glib::markup_escape_text(&f.id),
            glib::markup_escape_text(first)
        ));
        self.title
            .set_tooltip_text(Some(&format!("{}\n\nbase {}", f.prompt, f.base)));
        let phase = f.phase();
        let interrupted = try_app().map_or(0, |a| a.model.borrow().interrupted_members(f).len());
        self.progress.set_label(&match interrupted {
            0 => format!("{}/{} done", f.done(), f.members.len()),
            n => format!("{}/{} done · {n} interrupted", f.done(), f.members.len()),
        });
        self.resume
            .set_visible(interrupted > 0 && f.winner.is_none());
        let pick = f.judge.as_ref().and_then(|j| j.pick.clone());
        let verdict = match (&f.winner, phase, &pick) {
            (Some(w), _, _) if !w.conflicts.is_empty() => format!("Winner {} · conflicts", w.label),
            (Some(w), _, _) => format!("Winner {}", w.label),
            (None, Phase::Judging, _) => "Judging…".into(),
            (None, _, Some(p)) => format!("Judge picks {p}"),
            (None, _, None) if f.judge.as_ref().is_some_and(|j| j.finished) => {
                "Judge gave no verdict".into()
            }
            _ => String::new(),
        };
        self.verdict.set_label(&verdict);
        self.verdict.set_visible(!verdict.is_empty());
        let stats = self.stats.borrow();
        for c in &self.chips {
            let m = f.members.iter().find(|m| m.label == c.label);
            // After a restart statuses are unknown until a hook reports:
            // the latched end of turn still says done.
            let status = m
                .and_then(|m| tab.pane(&m.pane))
                .and_then(|p| p.status)
                .or(m
                    .filter(|m| m.finished)
                    .map(|_| rade_core::AgentStatus::Done));
            c.dot.set(status);
            c.dot.root.set_visible(status.is_some());
            c.dot
                .root
                .set_tooltip_text(Some(status.map_or("no status", |s| s.as_str())));
            c.stats.set_label(&stats_text(stats.get(&c.label)));
            let win = f.winner.as_ref().map(|w| &w.label) == Some(&c.label);
            let picked = pick.as_ref() == Some(&c.label);
            for (cls, on) in [
                ("rade-fanout-picked", picked && !win),
                ("rade-fanout-winner", win),
            ] {
                if on {
                    c.root.add_css_class(cls);
                } else {
                    c.root.remove_css_class(cls);
                }
            }
        }
        self.judge
            .set_sensitive(matches!(phase, Phase::Finished | Phase::Judged));
        self.judge.set_label(if f.judge.is_some() {
            "Judge Again"
        } else {
            "Judge"
        });
        self.choose.set_visible(phase != Phase::Chosen);
        self.choose
            .set_sensitive(f.members.iter().any(|m| m.finished) && phase != Phase::Judging);
        self.discard.set_label(if phase == Phase::Chosen {
            "Clean Up…"
        } else {
            "Discard…"
        });
    }

    fn info(&self) -> Value {
        let btn = |b: &gtk::Button| json!({"label": b.label().map(|l| l.to_string()), "sensitive": b.is_sensitive(), "visible": b.is_visible()});
        let chips: Vec<Value> = self
            .chips
            .iter()
            .map(|c| json!({"label": c.label, "status": c.dot.root.tooltip_text().map(|t| t.to_string()), "stats": c.stats.label().to_string(),
                "picked": c.root.has_css_class("rade-fanout-picked"), "winner": c.root.has_css_class("rade-fanout-winner")}))
            .collect();
        json!({"fanout": self.fanout, "title": self.title.text().to_string(), "progress": self.progress.label().to_string(),
            "verdict": self.verdict.label().to_string(), "members": chips, "mapped": self.root.is_mapped(),
            "buttons": {"judge": btn(&self.judge), "choose": btn(&self.choose), "discard": btn(&self.discard), "continue": btn(&self.resume)}})
    }
}

/// Re-render a Fan-out Tab's bar from the model (every sync).
pub fn update(tab: &Tab, f: &Fanout) {
    if let Some(b) = BARS.with(|b| b.borrow().get(&tab.id).cloned()) {
        b.update(f, tab);
        // An Agent finished: its final diff size, now.
        if b.seen_done.replace(f.done()) != f.done() {
            refresh_stats(&b);
        }
    }
}

fn open_diff(source: &str, worktree: &str) {
    let app = app();
    let r = app.update(|m| {
        let (p, w) = m.find_worktree(source, Some(worktree))?;
        m.open_review(&p, &w)
    });
    if let Err(e) = r {
        alert("Could not open the diff", &e.message);
    }
}

fn run_judge(id: &str, source: &str) {
    let (id, source) = (id.to_string(), source.to_string());
    in_background(
        move || fanout::judge(&id, Some(&source), None),
        |r| match r {
            Ok(v) => {
                if let Some(p) = v["pane"].as_str() {
                    let _ = app().update(|m| m.focus_pane(p));
                }
            }
            Err(e) => alert("Could not start the Judge", &e.message),
        },
    );
}

fn choose_dialog(id: &str, source: &str, preselect: Option<&str>) {
    let app = app();
    let (f, repo) = {
        let m = app.model.borrow();
        let Some(f) = m.fanout(id).cloned() else {
            return;
        };
        let repo = m.source(source).map(|p| p.path.clone()).unwrap_or_default();
        (f, repo)
    };
    let finished: Vec<&rade_core::fanout::Member> =
        f.members.iter().filter(|m| m.finished).collect();
    let judged = f.judge.as_ref().and_then(|j| j.pick.clone());
    // The Review's *Choose as Winner…* (R119.4) selects its candidate.
    let pick = preselect.map(String::from).or(judged);
    let stats = BARS.with(|b| {
        b.borrow()
            .get(&f.tab)
            .map(|b| b.stats.borrow().clone())
            .unwrap_or_default()
    });
    let names: Vec<String> = finished
        .iter()
        .map(|m| {
            let star = if pick.as_ref() == Some(&m.label) {
                "★ "
            } else {
                ""
            };
            format!("{star}{} — {}", m.label, stats_text(stats.get(&m.label)))
        })
        .collect();
    let labels: Vec<String> = finished.iter().map(|m| m.label.clone()).collect();
    let who = gtk::DropDown::from_strings(&names.iter().map(String::as_str).collect::<Vec<_>>());
    who.add_css_class("rade-fanout-who");
    if let Some(i) = pick
        .as_ref()
        .and_then(|p| labels.iter().position(|l| l == p))
    {
        who.set_selected(i as u32);
    }
    let branch = rade_core::git::branch(&repo).unwrap_or_else(|| "HEAD".into());
    let squash = gtk::CheckButton::with_label(&format!(
        "Stage its changes on {branch} (squash merge, not committed)"
    ));
    squash.add_css_class("rade-fanout-squash");
    let keep = gtk::CheckButton::with_label("Keep its Worktree and branch as they are");
    keep.add_css_class("rade-fanout-keep");
    keep.set_group(Some(&squash));
    squash.set_active(true);
    let clean =
        gtk::CheckButton::with_label("Then remove the other Worktrees and close the Fan-out");
    clean.add_css_class("rade-fanout-cleanup");
    clean.set_active(true);
    let form = gtk::Box::new(gtk::Orientation::Vertical, 8);
    for w in [
        who.upcast_ref::<gtk::Widget>(),
        squash.upcast_ref(),
        keep.upcast_ref(),
        clean.upcast_ref(),
    ] {
        form.append(w);
    }
    let body = match &pick {
        Some(p) => format!(
            "The Judge recommends {p}. RADE never commits: you (or an Agent) commit the result."
        ),
        None => {
            "No Judge verdict yet. RADE never commits: you (or an Agent) commit the result.".into()
        }
    };
    let d = adw::AlertDialog::new(Some("Choose the Winner"), Some(&body));
    d.set_extra_child(Some(&form));
    d.add_response("cancel", "Cancel");
    d.add_response("choose", "Choose");
    d.set_response_appearance("choose", adw::ResponseAppearance::Suggested);
    d.set_default_response(Some("choose"));
    d.set_close_response("cancel");
    d.set_response_enabled("choose", !labels.is_empty());
    let (id, source) = (id.to_string(), source.to_string());
    d.connect_response(Some("choose"), move |_, _| {
        let Some(label) = labels.get(who.selected() as usize).cloned() else {
            return;
        };
        let strategy = if keep.is_active() {
            Strategy::Keep
        } else {
            Strategy::Squash
        };
        let (id, source, cleanup) = (id.clone(), source.clone(), clean.is_active());
        in_background(
            move || fanout::choose(&id, Some(&source), Some(label), strategy, cleanup),
            |r| match r {
                Ok(v) => alert("Winner chosen", v["next"].as_str().unwrap_or("")),
                Err(e) => alert("Could not apply the Winner", &e.message),
            },
        );
    });
    d.present(Some(&app.window));
}

/// Continue: send each interrupted Agent a prompt to go on (it works for a
/// resumed conversation and for one that started fresh).
fn continue_interrupted(id: &str) {
    let app = app();
    let (panes, text) = {
        let m = app.model.borrow();
        let Some(f) = m.fanout(id) else { return };
        (
            m.interrupted_members(f),
            rade_core::fanout::continue_prompt(f),
        )
    };
    for p in panes {
        if let Err(e) = crate::handlers::send_text(&app, &p, &text, true, true) {
            alert("Could not continue an Agent", &e.message);
        }
    }
}

/// The close key on a Fan-out Agent (`rade close` refuses it without
/// `--force`): asks, like closing the Fan-out Tab does.
pub fn close_member_dialog(pane: &str) {
    let app = app();
    let Some((label, id, source)) = ({
        let m = app.model.borrow();
        m.fanouts.iter().find_map(|f| {
            let mem = f.members.iter().find(|m| m.pane == pane)?;
            Some((mem.label.clone(), f.id.clone(), f.source.clone()))
        })
    }) else {
        return;
    };
    let d = adw::AlertDialog::new(
        Some(&format!("Close {label}?")),
        Some(&format!(
            "It is an Agent of Fan-out {id}. Closing it stops its work and drops it \
             from the comparison; its Worktree stays until the Fan-out is discarded."
        )),
    );
    d.add_css_class("rade-fanout-close");
    d.add_response("cancel", "Cancel");
    d.add_response("discard", "Discard Fan-out…");
    d.add_response("close", "Close Agent");
    d.set_response_appearance("close", adw::ResponseAppearance::Destructive);
    d.set_close_response("cancel");
    let pane = pane.to_string();
    d.connect_response(None, move |_, r| match r {
        "close" => {
            let _ = crate::app::app().close_pane(&pane, false);
        }
        "discard" => discard_dialog(&id, &source),
        _ => {}
    });
    d.present(Some(&app.window));
}

pub fn discard_dialog(id: &str, source: &str) {
    let app = app();
    let Some(f) = app.model.borrow().fanout(id).cloned() else {
        return;
    };
    let chosen = f.winner.is_some();
    let body = match f.kept_worktree() {
        Some(w) => format!("Closes the Fan-out and removes every Worktree and branch it made except {w}."),
        None if chosen => "Closes the Fan-out and removes every Worktree and branch it made (the Winner's changes are already staged).".into(),
        None => "Closes the Fan-out and removes every Worktree and branch it made. The Agents' work is lost.".into(),
    };
    let heading = if chosen {
        "Clean up the Fan-out?"
    } else {
        "Discard the Fan-out?"
    };
    let d = adw::AlertDialog::new(Some(heading), Some(&body));
    d.add_response("cancel", "Cancel");
    d.add_response("discard", if chosen { "Clean Up" } else { "Discard" });
    d.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
    d.set_close_response("cancel");
    let (id, source) = (id.to_string(), source.to_string());
    d.connect_response(Some("discard"), move |_, _| {
        let (id, source) = (id.clone(), source.clone());
        in_background(
            move || fanout::discard(&id, Some(&source)),
            |r| {
                if let Err(e) = r {
                    alert("Could not remove the Fan-out", &e.message);
                } else if let Ok(v) = r {
                    if let Some(errs) = v["errors"].as_array().filter(|e| !e.is_empty()) {
                        let text: Vec<&str> = errs.iter().filter_map(Value::as_str).collect();
                        alert("Some Worktrees were not removed", &text.join("\n"));
                    }
                }
            },
        );
    });
    d.present(Some(&app.window));
}

/// `debug.fanout` (E2E): the bar of a Fan-out's Tab — `info`, `click
/// judge|choose|discard` (the button's own handler; dialogs open), or
/// `diff <label>` (that Agent's Diff button).
pub fn debug(fanout: &str, op: &str) -> Result<Value> {
    let tab = app()
        .model
        .borrow()
        .fanout(fanout)
        .map(|f| f.tab.clone())
        .ok_or_else(|| Error::not_found(format!("no fan-out '{fanout}'")))?;
    let b = BARS
        .with(|b| b.borrow().get(&tab).cloned())
        .ok_or_else(|| Error::not_found("its Tab has no bar (never shown)"))?;
    match op {
        "info" => {}
        "click judge" => b.judge.emit_clicked(),
        "click choose" => b.choose.emit_clicked(),
        "click discard" => b.discard.emit_clicked(),
        "click continue" => b.resume.emit_clicked(),
        o if o.starts_with("diff ") => {
            let label = &o[5..];
            let f = app().model.borrow().fanout(fanout).cloned().unwrap();
            let m = f
                .member(label)
                .ok_or_else(|| Error::not_found(format!("no Agent '{label}'")))?;
            open_diff(&f.source, &m.worktree);
        }
        o => return Err(Error::usage(format!("unknown op '{o}'"))),
    }
    Ok(b.info())
}

/// The Fan-out a Worktree is a candidate of: (Fan-out id, its label).
pub(crate) fn candidate_of(
    m: &rade_core::State,
    source: &str,
    worktree: &str,
) -> Option<(String, String)> {
    let name = &m.worktree(source, worktree)?.name;
    m.fanouts
        .iter()
        .filter(|f| f.source == source)
        .find_map(|f| {
            f.members
                .iter()
                .find(|c| &c.worktree == name)
                .map(|c| (f.id.clone(), c.label.clone()))
        })
}

/// *Choose as Winner…* from a candidate's Review (R119.4).
pub(crate) fn choose_for(source: &str, worktree: &str) {
    let hit = candidate_of(&app().model.borrow(), source, worktree);
    if let Some((id, label)) = hit {
        choose_dialog(&id, source, Some(&label));
    }
}
