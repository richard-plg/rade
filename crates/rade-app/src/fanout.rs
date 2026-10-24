//! Fan-out, Judge, Winner in the app (PRD R15): the `fanout.*` socket
//! methods, also run by the Fan-out bar and the palette form. Git runs on
//! the calling thread (a socket thread or `gio::spawn_blocking`), never on
//! the GTK thread; the model changes through [`on_main`].

use crate::app::app;
use crate::handlers::{on_main, params, preset, type_when_settled};
use crate::panes::now;
use rade_core::account::{Account, AgentKind};
use rade_core::fanout::{self, Candidate, JudgeSpawn, MemberSpawn, Phase, Start, Strategy, Winner};
use rade_core::preset::{new_uuid, Preset, PromptMode};
use rade_core::rpc::{Error, ErrorKind, Result};
use rade_core::{diff, git, merge, paths};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Deserialize)]
pub struct StartParams {
    pub agents: Vec<String>,
    pub prompt: String,
    pub source: Option<String>,
    pub base: Option<String>,
    pub name: Option<String>,
    pub caller: Option<String>,
}

#[derive(Deserialize, Clone)]
struct Ref {
    fanout: String,
    source: Option<String>,
}

#[derive(Deserialize)]
struct JudgeParams {
    #[serde(flatten)]
    at: Ref,
    agent: Option<String>,
}

#[derive(Deserialize)]
struct ChooseParams {
    #[serde(flatten)]
    at: Ref,
    winner: Option<String>,
    strategy: Option<String>,
    #[serde(default)]
    cleanup: bool,
}

#[derive(Deserialize)]
struct WaitParams {
    #[serde(flatten)]
    at: Ref,
    #[serde(default = "until_done")]
    until: String,
    #[serde(default = "timeout")]
    timeout: u64,
}

fn until_done() -> String {
    "done".into()
}

fn timeout() -> u64 {
    1800
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ListParams {
    source: Option<String>,
}

/// Socket methods (on a socket thread).
pub fn dispatch(method: &str, p: Value) -> Result<Value> {
    match method {
        "fanout.start" => start(params(p)?),
        "fanout.list" => {
            let l: ListParams = params(p)?;
            on_main(move || {
                let app = app();
                let m = app.model.borrow();
                let source = l
                    .source
                    .map(|q| m.find_source(&q).map(|p| p.id.clone()))
                    .transpose()?;
                let list: Vec<Value> = m
                    .fanouts
                    .iter()
                    .filter(|f| source.as_ref().is_none_or(|p| &f.source == p))
                    .map(|f| fanout::summary(&m, f))
                    .collect();
                Ok(json!({ "fanouts": list }))
            })
        }
        "fanout.status" => status(&params::<Ref>(p)?),
        "fanout.judge" => {
            let j: JudgeParams = params(p)?;
            judge(&j.at.fanout, j.at.source.as_deref(), j.agent)
        }
        "fanout.choose" => {
            let c: ChooseParams = params(p)?;
            let strategy = Strategy::parse(c.strategy.as_deref().unwrap_or("squash"))?;
            choose(
                &c.at.fanout,
                c.at.source.as_deref(),
                c.winner,
                strategy,
                c.cleanup,
            )
        }
        "fanout.discard" => {
            let r: Ref = params(p)?;
            discard(&r.fanout, r.source.as_deref())
        }
        "fanout.wait" => wait(params(p)?),
        "debug.fanout" => {
            #[derive(Deserialize)]
            struct D {
                fanout: String,
                op: String,
            }
            let d: D = params(p)?;
            on_main(move || crate::fanout_view::debug(&d.fanout, &d.op))
        }
        _ => Err(Error::usage(format!("unknown method '{method}'"))),
    }
}

/// A Preset that exists and whose program is on `PATH` (R15.1).
fn usable(name: &str, source: &str) -> Result<Preset> {
    let p = preset(&app(), name, Some((source, None)))?;
    if !p.available(&std::env::var("PATH").unwrap_or_default()) {
        return Err(Error::usage(format!(
            "agent '{name}' is not available: '{}' is not on PATH",
            p.binary()
        )));
    }
    Ok(p)
}

/// A Fan-out Agent's Agent Account (R108.2, R108.3, R108.7): the caller
/// Pane's, else the Workspace default for its Preset's agent; its Preset
/// command checked against the account's config dir.
fn account_of(
    m: &rade_core::State,
    source: &str,
    name: &str,
    p: &Preset,
    caller: Option<&str>,
) -> Result<Option<Account>> {
    let home = paths::home_dir(&paths::process_env);
    m.agent_account(
        source,
        name,
        AgentKind::of_hooks(p.hooks),
        &p.command,
        caller,
        &home,
    )
}

/// `fanout.start` (R15.1–R15.2): check Presets, availability and limits,
/// plan the names, create one Worktree per Agent from the same base commit
/// (all removed again if one fails), then open the Fan-out Tab.
pub fn start(s: StartParams) -> Result<Value> {
    if s.prompt.trim().is_empty() {
        return Err(Error::usage("a Fan-out needs a prompt"));
    }
    let (agents, caller, q) = (s.agents.clone(), s.caller.clone(), s.source.clone());
    let (pid, repo, taken, limits, wt_base, accounts) = on_main(move || {
        let app = app();
        let limits = app.config.borrow().limits.clone();
        let m = app.model.borrow();
        let p = match (q, caller.as_deref().and_then(|c| m.locate(c))) {
            (Some(q), _) => m.find_source(&q)?,
            (None, Some(loc)) => m.source(&loc.source).unwrap(),
            (None, None) => m.active().0,
        };
        if p.is_general() {
            return Err(Error::usage("General has no git repository; pass --source"));
        }
        m.check_not_paused(&p.id)?; // before any git (R19.3)
        let mut accounts = BTreeMap::new();
        for a in &agents {
            let preset = usable(a, &p.id)?;
            accounts.insert(
                a.clone(),
                account_of(&m, &p.id, a, &preset, caller.as_deref())?,
            );
        }
        m.check_fanout_limits(caller.as_deref(), agents.len(), &limits)?;
        let mut taken: Vec<String> = p.worktrees.iter().map(|w| w.name.clone()).collect();
        taken.extend(
            m.fanouts
                .iter()
                .filter(|f| f.source == p.id)
                .map(|f| f.id.clone()),
        );
        let wt_base = m.worktree_dir(
            &p.id,
            "",
            &paths::data_dir(&paths::process_env),
            paths::Flavor::current(),
        );
        Ok((
            p.id.clone(),
            p.path.clone(),
            taken,
            limits,
            wt_base,
            accounts,
        ))
    })?;
    git::require_toplevel(&repo)?;
    let base = match s.base.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) if b.starts_with('-') => return Err(Error::usage(format!("invalid base '{b}'"))),
        Some(b) => b.to_string(),
        None => git::default_branch(&repo).unwrap_or_else(|| "HEAD".into()),
    };
    let base_commit = git::run(
        &repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{base}^{{commit}}"),
        ],
    )
    .map(|c| c.trim().to_string())
    .map_err(|_| Error::usage(format!("unknown base '{base}'")))?;
    let plan = fanout::plan(&s.agents, s.name.as_deref(), &limits, &|n| {
        taken.iter().any(|t| t == n) || git::has_branch(&repo, n) || wt_base.join(n).exists()
    })?;
    let mut created: Vec<(String, String)> = Vec::new(); // (name, worktree id)
    for (_, _, name) in &plan.members {
        match crate::worktrees::create(&pid, Some(name), Some(&base_commit), None, None) {
            Ok(c) => created.push((c.name, c.worktree)),
            Err(e) => {
                undo(&pid, &created);
                return Err(e);
            }
        }
    }
    let (plan2, created2, prompt, caller) =
        (plan.clone(), created.clone(), s.prompt.clone(), s.caller);
    let (p2, base2) = (pid.clone(), base.clone());
    let r = on_main(move || {
        let app = app();
        let mut members = Vec::new();
        let mut typed = Vec::new();
        for ((label, agent, _), (_, wid)) in plan2.members.iter().zip(&created2) {
            let p = preset(&app, agent, Some((&p2, None)))?;
            let sid = p.session_id_flag.as_ref().map(|_| new_uuid());
            if p.prompt == PromptMode::Type {
                typed.push(label.clone());
            }
            members.push(MemberSpawn {
                label: label.clone(),
                agent: agent.clone(),
                worktree: wid.clone(),
                command: p.launch(Some(&prompt), sid.as_deref()),
                session_id: sid,
                account: accounts.get(agent).cloned().flatten(),
            });
        }
        let start = Start {
            source: p2,
            id: plan2.id.clone(),
            prompt: prompt.clone(),
            base: base2,
            base_commit,
            members,
            caller,
            now: now(),
        };
        let (tab, panes) = app.update(|m| m.start_fanout(start, &limits))?;
        // The Review header's task (R118.1).
        app.update(|m| {
            for id in &panes {
                let _ = m.set_task(id, &prompt);
            }
        });
        // `prompt = "type"` Presets get it typed once they settle (after setup).
        for (id, label) in panes.iter().zip(&plan2.members) {
            if typed.contains(&label.0) {
                type_when_settled(id.clone(), prompt.clone(), true);
            }
        }
        app.save_now();
        Ok(json!({
            "fanout": plan2.id, "tab": tab, "panes": panes,
            "worktrees": plan2.members.iter().map(|m| &m.2).collect::<Vec<_>>(),
            "labels": plan2.members.iter().map(|m| &m.0).collect::<Vec<_>>(),
            "base": base,
        }))
    });
    if r.is_err() {
        undo(&pid, &created);
    }
    r
}

/// Remove Worktrees made for a Fan-out that could not start.
fn undo(source: &str, created: &[(String, String)]) {
    for (name, _) in created {
        if let Err(e) = crate::worktrees::remove(source, name, true, true) {
            eprintln!("rade: fan-out cleanup of {name}: {}", e.message);
        }
    }
}

/// (fan-out, member worktree paths) for git work off the GTK thread.
fn snapshot(r: &Ref) -> Result<(fanout::Fanout, PathBuf, Vec<PathBuf>)> {
    let r = r.clone();
    on_main(move || {
        let app = app();
        let m = app.model.borrow();
        let f = m.find_fanout(&r.fanout, r.source.as_deref())?.clone();
        let p = m
            .source(&f.source)
            .ok_or_else(|| Error::not_found(format!("no source '{}'", f.source)))?;
        let paths = f
            .members
            .iter()
            .map(|x| {
                p.worktrees
                    .iter()
                    .find(|w| w.name == x.worktree)
                    .map(|w| w.path.clone())
                    .unwrap_or_default()
            })
            .collect();
        Ok((f, p.path.clone(), paths))
    })
}

/// Files changed, lines added, lines removed.
pub type Size = (usize, u32, u32);

/// Files changed, lines added and removed per Member (the bar's
/// comparison, `rade fanout status`); `None` for a Worktree git can't read.
pub fn stats(f: &fanout::Fanout, paths: &[PathBuf]) -> Vec<Option<Size>> {
    paths
        .iter()
        .map(|p| {
            let c = diff::changes_since(p, None, &f.base_commit).ok()?;
            let (a, r) = c
                .files
                .iter()
                .fold((0, 0), |(a, r), x| (a + x.added, r + x.removed));
            Some((c.files.len(), a, r))
        })
        .collect()
}

/// (label, (files, added, removed)) per Member that git could read (the
/// bar; off the GTK thread).
pub fn member_stats(id: &str, source: &str) -> Result<Vec<(String, Size)>> {
    let (f, _, paths) = snapshot(&Ref {
        fanout: id.into(),
        source: Some(source.into()),
    })?;
    Ok(f.members
        .iter()
        .zip(stats(&f, &paths))
        .filter_map(|(m, s)| Some((m.label.clone(), s?)))
        .collect())
}

fn status(r: &Ref) -> Result<Value> {
    let (f, _, paths) = snapshot(r)?;
    let st = stats(&f, &paths);
    let id = f.id.clone();
    let source = f.source.clone();
    let mut v = on_main(move || {
        let app = app();
        let m = app.model.borrow();
        let f = m.find_fanout(&id, Some(&source))?;
        Ok(fanout::summary(&m, f))
    })?;
    for (i, s) in st.into_iter().enumerate() {
        if let Some((files, added, removed)) = s {
            v["members"][i]["files"] = json!(files);
            v["members"][i]["added"] = json!(added);
            v["members"][i]["removed"] = json!(removed);
        }
    }
    Ok(v)
}

/// `fanout.judge` (R15.3): write each Agent's diff (against the shared base)
/// to a folder, then start the Judge — read-only, in the main Worktree —
/// with a prompt holding the task and the diffs in random order.
pub fn judge(id: &str, source: Option<&str>, agent: Option<String>) -> Result<Value> {
    let r = Ref {
        fanout: id.into(),
        source: source.map(Into::into),
    };
    let (f, _, paths) = snapshot(&r)?;
    let fp = f.source.clone();
    on_main(move || app().model.borrow().check_not_paused(&fp))?; // R19.3
    f.check_judge()?;
    let _busy = Busy::claim(&f)?;
    if let Some((m, _)) = f.members.iter().zip(&paths).find(|(_, p)| !p.exists()) {
        return Err(Error::usage(format!(
            "{}'s Worktree {} is gone; can't judge it",
            m.label, m.worktree
        )));
    }
    let agent = match agent {
        Some(a) => a,
        None => {
            let s = f.source.clone();
            on_main(move || Ok(app().config_for(&s, None).fanout.judge))?
        }
    };
    let (a2, s2) = (agent.clone(), f.source.clone());
    let (preset, account) = on_main(move || {
        let p = usable(&a2, &s2)?;
        let account = account_of(&app().model.borrow(), &s2, &a2, &p, None)?;
        Ok((p, account))
    })?;
    let dir = paths::data_dir(&paths::process_env)
        .join("fanout")
        .join(&f.source)
        .join(&f.id);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| Error::internal(format!("{}: {e}", dir.display())))?;
    let mut cands = Vec::new();
    for (m, p) in f.members.iter().zip(&paths) {
        let files = diff::changes_since(p, None, &f.base_commit)
            .map(|c| c.files)
            .unwrap_or_default();
        let diff = fanout::render_patch(&files);
        let file = dir.join(format!("{}.diff", m.worktree));
        std::fs::write(&file, &diff)
            .map_err(|e| Error::internal(format!("{}: {e}", file.display())))?;
        cands.push(Candidate {
            name: m.worktree.clone(),
            files: files.len(),
            added: files.iter().map(|x| x.added).sum(),
            removed: files.iter().map(|x| x.removed).sum(),
            diff,
            diff_file: file,
        });
    }
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64);
    let mut slots: Vec<Option<Candidate>> = cands.into_iter().map(Some).collect();
    let order: Vec<Candidate> = fanout::shuffle(slots.len(), seed)
        .into_iter()
        .filter_map(|i| slots[i].take())
        .collect();
    // Each candidate's Impact (R88.4), in parallel; the Judge's only
    // section after the diffs (the verify rubric went with R63–R70, 1.0.0).
    let base = f.base_commit.clone();
    let rubric: String = std::thread::scope(|sc| {
        let hs: Vec<_> = f
            .members
            .iter()
            .zip(&paths)
            .map(|(m, p)| {
                let (title, base) = (format!(" of {}", m.worktree), base.clone());
                let name = m.worktree.clone();
                sc.spawn(move || {
                    (
                        name,
                        crate::code::prompt_impact(p.clone(), Some(base), &title),
                    )
                })
            })
            .collect();
        let mut done: Vec<(String, String)> =
            hs.into_iter().filter_map(|h| h.join().ok()).collect();
        // In the candidates' random order.
        done.sort_by_key(|(n, _)| order.iter().position(|c| &c.name == n));
        done.into_iter().map(|(_, s)| s).collect()
    });
    let prompt = fanout::judge_prompt(&f.prompt, &order, &rubric);
    let _ = std::fs::write(dir.join("prompt.md"), &prompt);
    let (command, read_only) = fanout::judge_command(&preset, &dir, &paths);
    let sid = preset.session_id_flag.as_ref().map(|_| new_uuid());
    let judged = Preset {
        command,
        ..preset.clone()
    };
    let mut launch = judged.launch(Some(&prompt), sid.as_deref());
    // One argument is capped at 128 KiB (quoting grows it): point to the file.
    let mut prompt = prompt;
    if launch.len() > 100_000 {
        prompt = format!(
            "Read {} and do what it says (read-only).",
            dir.join("prompt.md").display()
        );
        launch = judged.launch(Some(&prompt), sid.as_deref());
    }
    let typed = preset.prompt == PromptMode::Type;
    let fid = f.id.clone();
    on_main(move || {
        let app = app();
        let spawn = JudgeSpawn {
            agent: agent.clone(),
            command: launch,
            session_id: sid,
            dir,
            read_only,
            now: now(),
            account,
        };
        let pane = app.update(|m| m.add_judge(&fid, spawn))?;
        if typed {
            type_when_settled(pane.clone(), prompt, false);
        }
        app.save_now();
        Ok(json!({"fanout": fid, "pane": pane, "agent": agent, "read_only": read_only}))
    })
}

/// `fanout.choose` (R15.4): the Winner (default: the Judge's pick) and a
/// strategy — `squash` stages its whole result on the Source's current
/// branch (refused on a detached HEAD, an operation in progress or
/// uncommitted changes; conflicts are left to resolve), `keep` keeps its
/// Worktree. RADE commits nothing (ADR-0008). `cleanup` then removes the
/// other Worktrees (and a squashed Winner's, unless it conflicted).
pub fn choose(
    id: &str,
    source: Option<&str>,
    winner: Option<String>,
    strategy: Strategy,
    cleanup: bool,
) -> Result<Value> {
    let r = Ref {
        fanout: id.into(),
        source: source.map(Into::into),
    };
    let (f, repo, paths) = snapshot(&r)?;
    let fp = f.source.clone();
    on_main(move || app().model.borrow().check_not_paused(&fp))?; // R19.3
    let _busy = Busy::claim(&f)?;
    let m = f.check_choose(winner.as_deref())?.clone();
    let i = f.members.iter().position(|x| x.label == m.label).unwrap();
    let wt = &paths[i];
    if !wt.exists() {
        return Err(Error::usage(format!(
            "{}'s Worktree {} is gone",
            m.label,
            wt.display()
        )));
    }
    let mut w = Winner {
        label: m.label.clone(),
        strategy,
        target: None,
        conflicts: Vec::new(),
    };
    let mut staged = 0;
    let next = match strategy {
        Strategy::Squash => {
            let changed = diff::changes_since(wt, None, &f.base_commit)?.files.len();
            let branch = merge::check_squash(&merge::target(&repo)?, changed, &m.label)?;
            let commit = merge::snapshot(wt)?;
            w.conflicts = merge::squash(&repo, &commit)?;
            w.target = Some(branch.clone());
            staged = changed;
            let rq = rade_core::preset::shell_quote(&repo.display().to_string());
            if w.conflicts.is_empty() {
                let msg = f.prompt.lines().next().unwrap_or("").trim();
                format!(
                    "{}'s changes are staged on {branch} (not committed). Review, then commit: git -C {rq} commit -m {}",
                    m.label,
                    rade_core::preset::shell_quote(msg)
                )
            } else {
                format!(
                    "{}'s changes are staged on {branch} with conflicts in {}: resolve them and commit, or undo with git -C {rq} reset --merge",
                    m.label,
                    w.conflicts.join(", ")
                )
            }
        }
        Strategy::Keep => format!(
            "Kept Worktree {} (branch {}) at {}. Its changes may be uncommitted: commit them there, then merge: git -C {} merge {}",
            m.worktree,
            m.worktree,
            wt.display(),
            rade_core::preset::shell_quote(&repo.display().to_string()),
            m.worktree
        ),
    };
    let (fid, pid, w2) = (f.id.clone(), f.source.clone(), w.clone());
    let pane = m.pane.clone();
    on_main(move || {
        let app = app();
        app.update(|m| -> Result<()> {
            m.set_winner(&fid, w2.clone())?;
            // A kept Winner's Agent lives on in its own Worktree.
            if w2.strategy == Strategy::Keep && m.pane(&pane).is_some() {
                m.move_to_own_worktree(&pane)?;
            }
            Ok(())
        })?;
        app.save_now();
        let _ = pid;
        Ok(())
    })?;
    let mut chosen = f.clone();
    chosen.winner = Some(w.clone());
    let cleaned = if cleanup {
        Some(clean(
            &chosen,
            chosen.kept_worktree().map(String::from).as_ref(),
        )?)
    } else {
        None
    };
    Ok(json!({
        "fanout": f.id, "winner": m.label, "worktree": m.worktree, "strategy": strategy,
        "target": w.target, "staged": staged, "conflicts": w.conflicts, "next": next,
        "cleaned": cleaned,
    }))
}

/// `fanout.discard`: close the Fan-out Tab (its Agents and Judge), remove
/// every Worktree and branch it made (a kept Winner's excepted), drop it.
pub fn discard(id: &str, source: Option<&str>) -> Result<Value> {
    let r = Ref {
        fanout: id.into(),
        source: source.map(Into::into),
    };
    let (f, _, _) = snapshot(&r)?;
    let _busy = Busy::claim(&f)?;
    let keep = f.kept_worktree().map(String::from);
    clean(&f, keep.as_ref())
}

fn clean(f: &fanout::Fanout, keep: Option<&String>) -> Result<Value> {
    // Its Agents and Judge stop first (the Tab closes).
    let tab = f.tab.clone();
    on_main(move || {
        let app = app();
        app.update(|m| {
            if m.locate_tab(&tab).is_some() {
                let _ = m.close_tab(&tab);
            }
        });
        app.save_now();
        Ok(())
    })?;
    let mut removed = Vec::new();
    let mut errors = Vec::new();
    for m in f.members.iter().filter(|m| Some(&m.worktree) != keep) {
        match crate::worktrees::remove(&f.source, &m.worktree, true, true) {
            Ok(_) => removed.push(m.worktree.clone()),
            // Already gone (removed by hand) is fine.
            Err(e) if e.kind == ErrorKind::NotFound => {}
            Err(e) => errors.push(format!("{}: {}", m.worktree, e.message)),
        }
    }
    if let Some(j) = &f.judge {
        let _ = std::fs::remove_dir_all(&j.dir);
    }
    // Dropped only when every Worktree went: a refused one (locked) keeps
    // the Fan-out listed, so `discard` can be retried.
    if errors.is_empty() {
        let fid = f.id.clone();
        on_main(move || {
            let app = app();
            app.update(|m| m.remove_fanout(&fid))?;
            app.save_now();
            Ok(())
        })?;
    }
    Ok(json!({"fanout": f.id, "removed": removed, "kept": keep, "errors": errors}))
}

/// `fanout.wait` (the group's `rade wait`): until every Agent finished
/// (`done`) or the Judge answered (`judged`; its verdict, or none if its
/// answer had no WINNER line after a few seconds).
fn wait(w: WaitParams) -> Result<Value> {
    let judged = match w.until.as_str() {
        "done" => false,
        "judged" => true,
        o => {
            return Err(Error::usage(format!(
                "--until must be done|judged, not {o}"
            )))
        }
    };
    let deadline = Instant::now() + Duration::from_secs(w.timeout);
    let mut quiet_since: Option<Instant> = None;
    loop {
        let r = w.at.clone();
        let (phase, finished_judge) = on_main(move || {
            let app = app();
            let m = app.model.borrow();
            let f = m.find_fanout(&r.fanout, r.source.as_deref())?;
            if judged && f.judge.is_none() && f.winner.is_none() {
                return Err(Error::usage(format!(
                    "fan-out '{}' has no Judge (rade fanout judge {})",
                    f.id, f.id
                )));
            }
            Ok((f.phase(), f.judge.as_ref().is_some_and(|j| j.finished)))
        })?;
        let over = match (judged, phase) {
            (false, p) => p != Phase::Running,
            (true, Phase::Judged | Phase::Chosen) => true,
            (true, _) if finished_judge => {
                let t = *quiet_since.get_or_insert_with(Instant::now);
                t.elapsed() > Duration::from_secs(VERDICT_TRIES as u64 + 1)
            }
            _ => false,
        };
        if over {
            return status(&w.at);
        }
        if Instant::now() >= deadline {
            return Err(Error::new(
                ErrorKind::Timeout,
                format!(
                    "timed out after {}s waiting for fan-out {} --until {}",
                    w.timeout, w.at.fanout, w.until
                ),
            ));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// One operation at a time per Fan-out (judge / choose / discard): each
/// checks a snapshot of the model and then runs git for a while.
struct Busy(String);

static BUSY: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

impl Busy {
    fn claim(f: &fanout::Fanout) -> Result<Busy> {
        let key = format!("{}/{}", f.source, f.id);
        let mut b = BUSY.lock().unwrap();
        if b.contains(&key) {
            return Err(Error::usage(format!(
                "fan-out '{}' is busy with another operation; try again",
                f.id
            )));
        }
        b.push(key.clone());
        Ok(Busy(key))
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.lock().unwrap().retain(|k| k != &self.0);
    }
}

/// Seconds (one try each) to find a finished Judge's verdict on its screen.
const VERDICT_TRIES: u32 = 5;

thread_local! {
    /// Judge Pane → (status_since, tries) of the turn being parsed.
    static TRIES: std::cell::RefCell<std::collections::HashMap<String, (u64, u32)>> =
        Default::default();
}

/// Every second (GTK thread): read the verdict of each Judge that finished
/// its turn without one yet ([`fanout::parse_verdict`]) and pre-select it.
pub fn watch_verdicts() {
    glib_timeout(|| {
        let Some(app) = crate::app::try_app() else {
            return;
        };
        let todo: Vec<(String, String, u64)> = {
            let m = app.model.borrow();
            m.fanouts
                .iter()
                .filter_map(|f| {
                    let j = f
                        .judge
                        .as_ref()
                        .filter(|j| j.finished && j.pick.is_none())?;
                    let since = m.pane(&j.pane)?.status_since.unwrap_or(0);
                    Some((f.id.clone(), j.pane.clone(), since))
                })
                .collect()
        };
        for (fid, pane, since) in todo {
            let go = TRIES.with(|t| {
                let mut t = t.borrow_mut();
                let e = t.entry(pane.clone()).or_insert((since, 0));
                if e.0 != since {
                    *e = (since, 0);
                }
                e.1 += 1;
                e.1 <= VERDICT_TRIES
            });
            if !go {
                continue;
            }
            let text = match crate::handlers::terminal(&app, &pane, None) {
                Ok((_, t)) => t
                    .handle
                    .read_text(rade_ghostty_sys::GHOSTTY_POINT_SCREEN)
                    .unwrap_or_default(),
                Err(_) => continue,
            };
            let verdict = {
                let m = app.model.borrow();
                m.fanout(&fid).and_then(|f| fanout::parse_verdict(&text, f))
            };
            if let Some(v) = verdict {
                let _ = app.update(|m| m.set_verdict(&fid, v));
            }
        }
    });
}

fn glib_timeout(f: impl Fn() + 'static) {
    gtk4::glib::timeout_add_local(Duration::from_secs(1), move || {
        f();
        gtk4::glib::ControlFlow::Continue
    });
}
