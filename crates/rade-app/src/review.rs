//! Review Comments → Agent (PRD R13.2, R119): `review.*` socket methods,
//! *Request changes* (a Round with a snapshot of the files) and *Approve ▾*
//! from the Review Pane. The prompt goes through the same path as
//! `rade send --paste --enter` (`handlers::send_text`).

use crate::app::{app, App};
use crate::handlers::{on_main, params, send_text, spawn, Spawn};
use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use rade_core::diff::LineKind;
use rade_core::review::{compose_round, ReviewComment};
use rade_core::review_loop::{comment_state, Approve, CommentState, Round, RoundState};
use rade_core::rpc::{Error, ErrorKind, Result};
use rade_core::State;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Deserialize, Default)]
#[serde(default)]
struct Target {
    source: Option<String>,
    worktree: Option<String>,
    caller: Option<String>,
}

#[derive(Deserialize)]
struct Add {
    #[serde(flatten)]
    at: Target,
    path: PathBuf,
    line: u32,
    end_line: Option<u32>,
    text: String,
}

#[derive(Deserialize)]
struct RequestParams {
    #[serde(flatten)]
    at: Target,
    summary: Option<String>,
}

#[derive(Deserialize)]
struct SendParams {
    #[serde(flatten)]
    at: Target,
    pane: Option<String>,
    agent: Option<String>,
}

/// (source, worktree) ids: `--source`/`--worktree`, else the caller's
/// Worktree, else the one on screen.
fn target(m: &State, t: &Target) -> Result<(String, String)> {
    let caller = t.caller.as_deref().and_then(|c| m.locate(c));
    let source = match (&t.source, &caller) {
        (Some(p), _) => m.find_source(p)?.id.clone(),
        (None, Some(l)) => l.source.clone(),
        (None, None) => m.active().0.id.clone(),
    };
    match (&t.worktree, &caller) {
        (Some(w), _) => m.find_worktree(&source, Some(w)),
        // The Worktree the caller runs in (a Fan-out Agent's own, R15.2).
        (None, Some(_)) if t.source.is_none() => m
            .pane_worktree(t.caller.as_deref().unwrap())
            .ok_or_else(|| Error::not_found("caller gone")),
        _ => m.find_worktree(&source, None),
    }
}

/// Socket methods (on the socket thread; git runs here, the model on main).
pub fn dispatch(method: &str, p: Value) -> Result<Value> {
    match method {
        "review.list" => {
            let t: Target = params(p)?;
            on_main(move || {
                let app = app();
                let (pid, wid) = target(&app.model.borrow(), &t)?;
                reanchor(&app, &pid, &wid);
                let comments = comments_json(&app, &pid, &wid);
                let m = app.model.borrow();
                let wt = m.worktree(&pid, &wid).unwrap();
                let drafts: Vec<ReviewComment> = wt
                    .comments
                    .iter()
                    .filter(|c| c.round.is_none())
                    .cloned()
                    .collect();
                let n = m.next_round(&pid, &wid);
                Ok(json!({
                    "source": pid, "worktree": wt.name, "comments": comments,
                    "prompt": (!drafts.is_empty())
                        .then(|| compose_round(&wt.name, Some(n), &drafts, &wt.comments)),
                }))
            })
        }
        "review.open" => {
            let t: Target = params(p)?;
            on_main(move || {
                let app = app();
                let (pid, wid) = target(&app.model.borrow(), &t)?;
                let pane = app.update(|m| m.open_review(&pid, &wid))?;
                Ok(json!({"pane": pane}))
            })
        }
        "review.add" => add(params(p)?),
        "review.send" => {
            let s: SendParams = params(p)?;
            on_main(move || {
                let app = app();
                let (pid, wid) = target(&app.model.borrow(), &s.at)?;
                send(&app, &pid, &wid, s.pane, s.agent, s.at.caller)
            })
        }
        "review.status" => status(params(p)?),
        "review.request" => {
            let r: RequestParams = params(p)?;
            on_main(move || {
                let app = app();
                let (pid, wid) = target(&app.model.borrow(), &r.at)?;
                let name = app
                    .model
                    .borrow()
                    .worktree(&pid, &wid)
                    .unwrap()
                    .name
                    .clone();
                let pane = r.at.caller.clone().or_else(|| {
                    app.model
                        .borrow()
                        .agent_panes(&pid, &wid)
                        .first()
                        .map(|p| p.id.clone())
                });
                app.update(|m| -> Result<()> {
                    m.set_summary(&pid, &wid, r.summary.as_deref().unwrap_or(""))?;
                    if let Some(pane) = &pane {
                        m.notify(
                            pane,
                            Some("Ready for review".into()),
                            r.summary
                                .as_deref()
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .unwrap_or(&name),
                            crate::panes::now(),
                        );
                    }
                    Ok(())
                })?;
                Ok(json!({"worktree": name, "notified": pane.is_some()}))
            })
        }
        _ => Err(Error::usage(format!("unknown method '{method}'"))),
    }
}

/// Each comment of a Worktree with its state (R119.3) for `review.list`
/// and `review.status`.
fn comments_json(app: &App, pid: &str, wid: &str) -> Vec<Value> {
    let states = comment_states(app, pid, wid);
    let m = app.model.borrow();
    let Some(wt) = m.worktree(pid, wid) else {
        return vec![];
    };
    wt.comments
        .iter()
        .map(|c| {
            let mut v = serde_json::to_value(c).unwrap_or_default();
            let st = states.get(&c.id).copied().unwrap_or(CommentState::Draft);
            v["state"] = json!(st);
            v
        })
        .collect()
}

/// The state of each comment (R119.3): reads the commented files (few).
pub(crate) fn comment_states(app: &App, pid: &str, wid: &str) -> HashMap<u64, CommentState> {
    let m = app.model.borrow();
    let Some(wt) = m.worktree(pid, wid) else {
        return HashMap::new();
    };
    let at = |n: u32| wt.review.rounds.iter().find(|r| r.n == n).map(|r| r.at);
    let mut files: HashMap<&str, Option<String>> = HashMap::new();
    wt.comments
        .iter()
        .map(|c| {
            let ended = c
                .round
                .and_then(at)
                .is_some_and(|t| m.turn_ended_since(pid, wid, t));
            let file = files
                .entry(c.path.as_str())
                .or_insert_with(|| std::fs::read_to_string(wt.path.join(&c.path)).ok());
            (c.id, comment_state(c, ended, file.as_deref()))
        })
        .collect()
}

/// `review.status` (R119.6): Rounds with their state (git runs here, off
/// the UI thread), comments, Viewed progress, the Agent's summary.
fn status(t: Target) -> Result<Value> {
    let (pid, wid, root, last, base) = on_main(move || {
        let app = app();
        let m = app.model.borrow();
        let (p, w) = target(&m, &t)?;
        let wt = m.worktree(&p, &w).unwrap();
        let base = m.comparison_base(&wt.path);
        Ok((
            p,
            w,
            wt.path.clone(),
            wt.review.rounds.last().cloned(),
            base,
        ))
    })?;
    let changed_since = last.as_ref().is_some_and(|r| {
        rade_core::diff::since(&root, None, &r.snapshot, false).is_ok_and(|c| !c.files.is_empty())
    });
    // Viewed against the files now.
    let hashes: Vec<(String, u64)> = rade_core::diff::changes(&root, base.as_deref())
        .map(|c| {
            c.files
                .iter()
                .map(|f| (f.path.clone(), rade_core::review_loop::file_hash(&root, f)))
                .collect()
        })
        .unwrap_or_default();
    on_main(move || {
        let app = app();
        let comments = comments_json(&app, &pid, &wid);
        let m = app.model.borrow();
        let wt = m
            .worktree(&pid, &wid)
            .ok_or_else(|| Error::not_found("worktree gone"))?;
        let rounds: Vec<Value> = wt
            .review
            .rounds
            .iter()
            .map(|r| {
                let last = Some(r.n) == wt.review.rounds.last().map(|l| l.n);
                json!({"n": r.n, "at": r.at,
                    "state": RoundState::of(r, last && changed_since)})
            })
            .collect();
        let refs: Vec<(&str, u64)> = hashes.iter().map(|(p, h)| (p.as_str(), *h)).collect();
        let (done, total) = wt.review.clone().progress(&refs);
        let open = comments
            .iter()
            .filter(|c| !c["round"].is_null() && c["resolved"] != true)
            .count();
        let drafts = comments.iter().filter(|c| c["round"].is_null()).count();
        Ok(json!({
            "source": pid, "worktree": wt.name, "base": wt.base,
            "rounds": rounds, "comments": comments, "open": open, "drafts": drafts,
            "viewed": {"done": done, "total": total}, "summary": wt.review.summary,
        }))
    })
}

/// `review.add`: a comment on new-file lines of a file in the Worktree;
/// the excerpt comes from the diff (with +/− signs) or, for lines the diff
/// doesn't show, from the file itself.
fn add(a: Add) -> Result<Value> {
    if a.end_line.is_some_and(|e| e < a.line) || a.line == 0 {
        return Err(Error::usage("bad line range"));
    }
    let t = a.at;
    let (pid, wid, root, base) = on_main(move || {
        let app = app();
        let m = app.model.borrow();
        let (p, w) = target(&m, &t)?;
        let wt = m.worktree(&p, &w).unwrap();
        Ok((p, w, wt.path.clone(), m.comparison_base(&wt.path)))
    })?;
    let canon = |p: &std::path::Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let rel = canon(&a.path)
        .strip_prefix(canon(&root))
        .map(|r| r.to_string_lossy().into_owned())
        .map_err(|_| {
            Error::usage(format!(
                "{} is not inside worktree {}",
                a.path.display(),
                root.display()
            ))
        })?;
    let end = a.end_line.unwrap_or(a.line);
    let changes = rade_core::diff::changes(&root, base.as_deref()).ok();
    let hunks: Vec<&rade_core::diff::Hunk> = changes
        .iter()
        .flat_map(|c| &c.files)
        .filter(|f| f.path == rel)
        .flat_map(|f| &f.hunks)
        .collect();
    let hunk = hunks
        .iter()
        .find(|h| h.lines.iter().any(|l| l.new == Some(a.line)))
        .map(|h| h.header.clone());
    // Each line of the range: its diff line (`+` added, ` ` context), else
    // the file's line (the diff may cover only part of the range).
    let file = std::fs::read_to_string(root.join(&rel)).ok();
    let file_lines: Vec<&str> = file.as_deref().unwrap_or_default().lines().collect();
    let excerpt: Vec<String> = (a.line..=end)
        .map_while(|n| {
            let diff = hunks
                .iter()
                .flat_map(|h| &h.lines)
                .find(|l| l.new == Some(n))
                .map(|l| {
                    let sign = if l.kind == LineKind::Added { '+' } else { ' ' };
                    format!("{sign}{}", l.text)
                });
            diff.or_else(|| Some(format!(" {}", file_lines.get(n as usize - 1)?)))
        })
        .collect();
    let mut c = ReviewComment {
        id: 0,
        path: rel,
        line: a.line,
        end_line: a.end_line.filter(|e| *e != a.line),
        excerpt,
        text: a.text,
        ..Default::default()
    };
    if let Some(f) = &file {
        c.capture(f, hunk.as_deref());
    }
    let loc = c.location();
    let id = on_main(move || app().update(|m| m.add_comment(&pid, &wid, c)))?;
    Ok(json!({"id": id, "location": loc}))
}

/// Move a Worktree's Review Comments to where their lines are now (M11),
/// reading its files here (comments are few, files are source files).
pub(crate) fn reanchor(app: &App, pid: &str, wid: &str) -> bool {
    let Some(root) = app
        .model
        .borrow()
        .worktree(pid, wid)
        .map(|w| w.path.clone())
    else {
        return false;
    };
    let read = |p: &str| std::fs::read_to_string(root.join(p)).ok();
    let moved = app
        .touch(|m| m.reanchor_comments(pid, wid, &read))
        .unwrap_or(false);
    if moved {
        app.sync(); // open Review Panes show the new places
    }
    moved
}

/// Live Agent Panes of a Worktree: (id, "name · preset"), those in `tab`
/// first.
fn agents_of(app: &App, pid: &str, wid: &str, tab: Option<&str>) -> Vec<(String, String, bool)> {
    let m = app.model.borrow();
    let mut v: Vec<(String, String, bool)> = m
        .agent_panes(pid, wid)
        .iter()
        .map(|p| {
            let label = format!(
                "{} · {}",
                p.name.as_deref().unwrap_or(&p.id),
                p.agent.as_deref().unwrap_or("?")
            );
            let here = tab.is_some_and(|t| m.locate(&p.id).is_some_and(|l| l.tab == t));
            (p.id.clone(), label, here)
        })
        .collect();
    v.sort_by_key(|x| !x.2);
    v
}

/// Where a prompt for a Worktree's Agent goes (R13.2, R71.4): `pane`; else
/// the only live Agent Pane in `tab` (a Browser's own Tab), else the
/// Worktree's only one; none → spawn Preset `agent` with the prompt.
/// Returns (Pane, spawned).
#[allow(clippy::too_many_arguments)]
pub(crate) fn deliver(
    app: &App,
    pid: &str,
    wid: &str,
    tab: Option<&str>,
    pane: Option<String>,
    agent: Option<String>,
    caller: Option<&str>,
    prompt: &str,
) -> Result<(String, bool)> {
    let agents = agents_of(app, pid, wid, tab);
    let here: Vec<&(String, String, bool)> = agents.iter().filter(|a| a.2).collect();
    let name = app
        .model
        .borrow()
        .worktree(pid, wid)
        .map(|w| w.name.clone())
        .unwrap_or_default();
    let target = match (pane, here.len(), agents.len(), agent) {
        (Some(q), ..) => {
            // Only into an Agent: the prompt quotes page / repository text
            // that a shell would run (acceptance H1).
            let m = app.model.borrow();
            let id = m.resolve_pane(&q, caller)?;
            if !m.pane(&id).is_some_and(|p| p.agent.is_some()) {
                return Err(Error::usage(format!(
                    "pane {id} is not an Agent Pane: send to an Agent (its text would run in a shell)"
                )));
            }
            id
        }
        (None, 1, _, _) => here[0].0.clone(),
        (None, _, 1, _) => agents[0].0.clone(),
        (None, _, 0, Some(preset)) => {
            let r = spawn(
                app,
                Spawn {
                    source: Some(pid.into()),
                    worktree: Some(wid.into()),
                    agent: Some(preset),
                    prompt: Some(prompt.to_string()),
                    split: Some("right".into()),
                    ..Default::default()
                },
            )?;
            return Ok((r["pane"].as_str().unwrap_or_default().to_string(), true));
        }
        (None, _, 0, None) => {
            return Err(Error::not_found(format!(
                "no Agent Pane in worktree '{name}'; name one (--pane / --to), or --agent <preset> to spawn one"
            )))
        }
        (None, ..) => {
            let list: Vec<String> = agents
                .iter()
                .map(|(id, n, _)| format!("{n} ({id})"))
                .collect();
            return Err(Error::new(
                ErrorKind::Ambiguous,
                format!(
                    "several Agent Panes in worktree '{name}': {}; name one (--pane / --to)",
                    list.join(", ")
                ),
            ));
        }
    };
    send_text(app, &target, prompt, true, true)?;
    Ok((target, false))
}

/// Compose all comments into one prompt, paste + Enter it into the Agent
/// Pane (`pane`; else the Worktree's only live Agent Pane; none → spawn
/// Preset `agent` with it), then clear them.
fn send(
    app: &App,
    pid: &str,
    wid: &str,
    pane: Option<String>,
    agent: Option<String>,
    caller: Option<String>,
) -> Result<Value> {
    reanchor(app, pid, wid);
    let (prompt, n, round, name, root) = {
        let m = app.model.borrow();
        let wt = m.worktree(pid, wid).unwrap();
        let drafts: Vec<ReviewComment> = wt
            .comments
            .iter()
            .filter(|c| c.round.is_none())
            .cloned()
            .collect();
        if drafts.is_empty() {
            return Err(Error::usage("no Review Comments to send"));
        }
        let round = m.next_round(pid, wid);
        (
            compose_round(&wt.name, Some(round), &drafts, &wt.comments),
            drafts.len(),
            round,
            wt.name.clone(),
            wt.path.clone(),
        )
    };
    // The files as they are when the comments go (before the Agent starts
    // on them): "Since round n" diffs against this. ponytail: runs `git add
    // -A` into a throwaway index on the UI thread; move off it if a large
    // repository makes Send slow.
    let snapshot = rade_core::review_loop::snapshot(&root, &name, round)?;
    let (target, spawned) = deliver(app, pid, wid, None, pane, agent, caller.as_deref(), &prompt)?;
    record(app, pid, wid, &root, &name, round, snapshot, false)?;
    Ok(json!({"pane": target, "comments": n, "prompt": prompt, "spawned": spawned, "round": round}))
}

/// Record Round `n` and drop the refs of Rounds past the cap.
#[allow(clippy::too_many_arguments)]
fn record(
    app: &App,
    pid: &str,
    wid: &str,
    root: &std::path::Path,
    name: &str,
    n: u32,
    snapshot: String,
    approved: bool,
) -> Result<()> {
    let round = Round {
        n,
        at: crate::panes::now(),
        snapshot,
        approved,
    };
    let dropped = app.update(|m| m.record_round(pid, wid, round))?;
    rade_core::review_loop::drop_refs(root, name, Some(&dropped));
    Ok(())
}

/// *Approve ▾* (R119.4): record an approved Round, then the item's
/// hand-off — a prompt to the Agent, or one of the existing flows.
pub(crate) fn approve(review: &str, item: Approve) -> Result<String> {
    let app = app();
    let loc = app
        .model
        .borrow()
        .locate(review)
        .ok_or_else(|| Error::not_found("review gone"))?;
    let (pid, wid) = (loc.source, loc.worktree);
    let (name, root, round, base) = {
        let m = app.model.borrow();
        let wt = m.worktree(&pid, &wid).unwrap();
        let base = m
            .comparison_base(&wt.path)
            .or_else(|| wt.base.clone())
            .unwrap_or_else(|| "main".into());
        (
            wt.name.clone(),
            wt.path.clone(),
            m.next_round(&pid, &wid),
            base,
        )
    };
    let snapshot = rade_core::review_loop::snapshot(&root, &name, round)?;
    if let Some(prompt) = item.prompt(round, &base) {
        deliver(&app, &pid, &wid, None, None, None, None, &prompt)?;
    }
    record(&app, &pid, &wid, &root, &name, round, snapshot, true)?;
    match item {
        Approve::AskCommit | Approve::AskCommitPr => {}
        Approve::CommitMyself => crate::changes::commit_myself(),
        Approve::CreatePr => crate::changes::create_pr(),
        Approve::ChooseWinner => crate::fanout_view::choose_for(&pid, &wid),
    }
    Ok(format!("Approved round {round}: {}", item.label()))
}

/// The owner picks where a prompt goes (Review Pane, Browser Pane): one
/// Agent (in `tab`, else in the Worktree) → `go` at once; several →
/// choose; none → offer to spawn one from a Preset. `go(pane, preset)`.
pub(crate) fn choose_target(
    app: &App,
    pid: &str,
    wid: &str,
    tab: Option<&str>,
    what: &str,
    go: impl Fn(Option<String>, Option<String>) + 'static,
) {
    let agents = agents_of(app, pid, wid, tab);
    let here = agents.iter().filter(|a| a.2).count();
    if agents.len() == 1 || here == 1 {
        let id = agents
            .iter()
            .find(|a| a.2 || agents.len() == 1)
            .map(|a| a.0.clone());
        return go(id, None);
    }
    let d;
    if agents.is_empty() {
        let presets: Vec<String> = app
            .config
            .borrow()
            .presets()
            .unwrap_or_default()
            .into_keys()
            .collect();
        let pick =
            gtk::DropDown::from_strings(&presets.iter().map(String::as_str).collect::<Vec<_>>());
        pick.add_css_class("rade-field-agent");
        if let Some(i) = presets.iter().position(|p| p == "claude") {
            pick.set_selected(i as u32);
        }
        d = adw::AlertDialog::new(
            Some("No Agent in this Worktree"),
            Some(&format!("Spawn one with the {what} as its prompt?")),
        );
        d.set_extra_child(Some(&pick));
        d.add_response("cancel", "Cancel");
        d.add_response("spawn", "Spawn and Send");
        d.set_response_appearance("spawn", adw::ResponseAppearance::Suggested);
        d.connect_response(Some("spawn"), move |_, _| {
            go(None, presets.get(pick.selected() as usize).cloned());
        });
        d.set_close_response("cancel");
        crate::menus::present(d, &["cancel", "spawn"]);
        return;
    }
    d = adw::AlertDialog::new(Some("Send to which Agent?"), None);
    d.add_response("cancel", "Cancel");
    for (id, label, _) in &agents {
        d.add_response(id, label);
    }
    d.connect_response(None, move |_, r| {
        if r != "cancel" {
            go(Some(r.into()), None);
        }
    });
    d.set_close_response("cancel");
    let mut ids: Vec<&str> = vec!["cancel"];
    ids.extend(agents.iter().map(|a| a.0.as_str()));
    crate::menus::present(d, &ids);
}

/// "Send to Agent" in a Review Pane: one Agent → send; several → choose;
/// none → offer to spawn one from a Preset.
pub fn send_from_view(review: &str, pane: Option<String>) {
    let app = app();
    let Some(loc) = app.model.borrow().locate(review) else {
        return;
    };
    let (pid, wid) = (loc.source, loc.worktree);
    let review = review.to_string();
    let done = move |r: Result<Value>| {
        let app = crate::app::app();
        let v = app.reviews.borrow().get(&review).cloned();
        let text = match r {
            Ok(v) => format!(
                "Round {}: sent {} comment(s) to {}",
                v["round"],
                v["comments"],
                v["pane"].as_str().unwrap_or("?")
            ),
            Err(e) => e.message,
        };
        if let Some(v) = v {
            v.set_note(&text);
            v.sync();
        }
    };
    if pane.is_some() {
        return done(send(&app, &pid, &wid, pane, None, None));
    }
    let (p2, w2) = (pid.clone(), wid.clone());
    choose_target(&app, &pid, &wid, None, "comments", move |pane, preset| {
        done(send(&crate::app::app(), &p2, &w2, pane, preset, None))
    });
}

/// Open the Review of the Worktree an Agent Pane runs in (R119.5: a Toast,
/// an Inbox row, Agent Overview's `r`).
pub(crate) fn open_for(pane: &str) {
    let app = app();
    let target = app.model.borrow().review_target(pane);
    if let Some((p, w)) = target {
        let review = app.update(|m| {
            m.mark_read(pane);
            m.open_review(&p, &w)
        });
        // What changed since the last Round first (R119.2).
        if let Some(v) = review
            .ok()
            .and_then(|r| app.reviews.borrow().get(&r).cloned())
        {
            v.show_since();
        }
    }
}
