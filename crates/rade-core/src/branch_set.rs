//! Branch sets (PRD R149–R151, ADR-0054): one branch name cut in several
//! links of a Project. Pure logic only — the app gathers the facts, runs
//! the commands this module lists and records the result in the model.
//!
//! Order of a `worktree.add` with `"sources"`:
//! 1. gather a [`LinkFacts`] per *selected* link ([`select`]; no git write),
//! 2. [`plan`] — any refusal is `USAGE` naming the link, nothing created,
//! 3. run each [`Step::argv`] with git in [`Step::repo`], in order; after
//!    each success `State::add_worktree` + `set_worktree_origin(project,
//!    base)` + `set_worktree_set(name)`; on a failure at step k call
//!    [`rollback`], run its [`Undo`]s and return its `message`,
//! 4. the result is [`Plan::result_json`].

use crate::agents::AgentStatus;
use crate::git;
use crate::preset::{shell_quote, Preset};
use crate::project::{self, Project};
use crate::rpc::{Error, Result};
use serde_json::json;
use std::path::PathBuf;

/// Which links a call asks for (`"sources": [...] | "all"`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sources {
    All,
    Named(Vec<String>),
}

/// What the app read about one selected link before planning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkFacts {
    /// The Source id (as in `project.toml`).
    pub source: String,
    /// The Source's main checkout: where git runs.
    pub repo: PathBuf,
    /// `State::worktree_dir(source, name, ..)` (R12 / ADR-0014 path).
    pub dir: PathBuf,
    /// `git::branches(repo)` holds the name.
    pub branch_exists: bool,
    /// The Source already has a Worktree of that name (model or git).
    pub worktree_exists: bool,
    /// `git::default_branch(repo)`: the base of a link without one (R99.1).
    pub main_branch: Option<String>,
    /// `git::start_point(repo, base)`; `None` = the base itself. Needs the
    /// base, so the app reads it with `Project`'s `resolve_base` first.
    pub start_point: Option<String>,
    /// R98 `resolve_checkout` for the link: `Err(why)` = unresolvable.
    pub checkout: std::result::Result<(), String>,
}

/// One Worktree to create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub source: String,
    pub repo: PathBuf,
    pub dir: PathBuf,
    /// The base recorded on the Worktree (R99.1).
    pub base: String,
    /// `git` arguments, run in `repo` (what `git::add_worktree` runs).
    pub argv: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub project: String,
    pub name: String,
    /// In Project order.
    pub steps: Vec<Step>,
}

impl Plan {
    /// The `worktree.add` result; `made` = `(source id, worktree id)` per
    /// step, in step order (R149.2).
    pub fn result_json(&self, made: &[(String, String)]) -> serde_json::Value {
        let list: Vec<_> = self
            .steps
            .iter()
            .zip(made)
            .map(|(s, (_, w))| {
                json!({"source": s.source, "worktree": w, "path": s.dir, "base": s.base})
            })
            .collect();
        json!({"project": self.project, "set": self.name, "worktrees": list})
    }
}

fn refuse(source: &str, why: impl std::fmt::Display) -> Error {
    Error::usage(format!("source '{source}': {why}"))
}

/// The links a call selects, in Project order; an unknown name is `USAGE`.
pub fn select<'a>(project: &'a Project, asked: &Sources) -> Result<Vec<&'a str>> {
    if let Some(e) = &project.error {
        return Err(Error::usage(e.clone()));
    }
    match asked {
        Sources::All => Ok(project.sources.iter().map(|s| s.source.as_str()).collect()),
        Sources::Named(names) => {
            if names.is_empty() {
                return Err(Error::usage("--sources needs at least one source"));
            }
            for n in names {
                if project.source(n).is_none() {
                    return Err(Error::usage(format!(
                        "source '{n}' is not in project '{}' (rade project link add {} {n})",
                        project.id, project.id
                    )));
                }
            }
            Ok(project
                .sources
                .iter()
                .map(|s| s.source.as_str())
                .filter(|s| names.iter().any(|n| n == s))
                .collect())
        }
    }
}

/// R149.2: validate every selected link first; any refusal → `USAGE`
/// naming the link (no commands). `facts` has one entry per selected link.
pub fn plan(project: &Project, asked: &Sources, name: &str, facts: &[LinkFacts]) -> Result<Plan> {
    let selected = select(project, asked)?;
    let mut steps: Vec<Step> = Vec::new();
    for src in selected {
        let f = facts
            .iter()
            .find(|f| f.source == src)
            .ok_or_else(|| Error::internal(format!("no facts for source '{src}'")))?;
        git::validate_name(name).map_err(|e| refuse(src, e.message))?;
        if let Err(why) = &f.checkout {
            return Err(refuse(
                src,
                format!("its checkout can't be resolved: {why}"),
            ));
        }
        if f.worktree_exists {
            return Err(refuse(src, format!("already has a worktree '{name}'")));
        }
        if f.branch_exists {
            return Err(refuse(src, format!("branch '{name}' already exists")));
        }
        // That link's own base (R97 rules), else the Source's main branch.
        let base = project::resolve_base(project, src, None)?
            .or_else(|| f.main_branch.clone())
            .ok_or_else(|| refuse(src, "has no base and no main branch to cut from"))?;
        if base.starts_with('-') || base.is_empty() {
            return Err(refuse(src, format!("invalid base '{base}'")));
        }
        if steps.iter().any(|s| s.dir == f.dir) {
            return Err(refuse(src, "its Worktree folder is used by another link"));
        }
        let start = f.start_point.clone().unwrap_or_else(|| base.clone());
        let argv = [
            "worktree",
            "add",
            "--no-track",
            "-b",
            name,
            &f.dir.to_string_lossy(),
            &start,
        ]
        .map(String::from)
        .to_vec();
        steps.push(Step {
            source: src.to_string(),
            repo: f.repo.clone(),
            dir: f.dir.clone(),
            base,
            argv,
        });
    }
    Ok(Plan {
        project: project.id.clone(),
        name: name.into(),
        steps,
    })
}

/// The commands that undo one created Worktree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Undo {
    pub source: String,
    pub repo: PathBuf,
    /// `git worktree remove --force <dir>`, then `git branch -D <name>`.
    pub commands: Vec<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rollback {
    /// Newest first.
    pub undo: Vec<Undo>,
    /// The error to return: names the failing link and what was undone.
    pub message: String,
}

/// R149.2: step `failed` (0-based) failed with `cause`. Undo the steps
/// before it, newest first; `plan.name` is a branch this call created in
/// each, because [`plan`] refuses an existing one. `left_behind`: the
/// failing step still made its Worktree (a failing `post-checkout` hook
/// exits non-zero after git created it), so it is undone too.
pub fn rollback(plan: &Plan, failed: usize, left_behind: bool, cause: &str) -> Rollback {
    let upto = (if left_behind { failed + 1 } else { failed }).min(plan.steps.len());
    let undo: Vec<Undo> = plan.steps[..upto]
        .iter()
        .rev()
        .map(|s| Undo {
            source: s.source.clone(),
            repo: s.repo.clone(),
            commands: vec![
                vec![
                    "worktree".into(),
                    "remove".into(),
                    "--force".into(),
                    s.dir.to_string_lossy().into(),
                ],
                vec!["branch".into(), "-D".into(), plan.name.clone()],
            ],
        })
        .collect();
    let link = plan.steps.get(failed).map_or("?", |s| s.source.as_str());
    let undone = if undo.is_empty() {
        "nothing to undo".to_string()
    } else {
        format!(
            "undone: removed the Worktree and branch '{}' in {}",
            plan.name,
            undo.iter()
                .map(|u| u.source.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    Rollback {
        message: format!(
            "branch set '{}' failed in source '{link}': {}; {undone}",
            plan.name,
            cause.trim()
        ),
        undo,
    }
}

/// What the app read about one Worktree of a set before removing it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoveFacts {
    pub source: String,
    pub worktree: String,
    pub repo: PathBuf,
    pub path: PathBuf,
    /// Its folder is gone: pruned instead of removed.
    pub missing: bool,
    /// `git::worktree_branch` (None when detached).
    pub branch: Option<String>,
    /// `git::dirty` paths.
    pub dirty: Vec<String>,
    /// The folder has a `.gitmodules`.
    pub submodules: bool,
    /// `git::lock_reason`.
    pub locked: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoveStep {
    pub source: String,
    pub worktree: String,
    pub repo: PathBuf,
    pub path: PathBuf,
    /// `git worktree prune` instead of `remove`.
    pub prune: bool,
    pub force: bool,
    /// The branch to delete afterwards (`git branch -d`, `-D` with force).
    pub delete_branch: Option<String>,
}

/// The refusal `worktree.remove` gives one Worktree (R12.3), or None.
pub fn remove_refusal(f: &RemoveFacts, force: bool) -> Option<String> {
    if f.missing {
        return None;
    }
    let name = &f.worktree;
    if let Some(reason) = &f.locked {
        return Some(format!(
            "worktree '{name}' is locked{}; run `git worktree unlock {}` first",
            if reason.is_empty() {
                String::new()
            } else {
                format!(" ({reason})")
            },
            f.path.display()
        ));
    }
    if force {
        return None;
    }
    if !f.dirty.is_empty() {
        let shown: Vec<&str> = f.dirty.iter().take(5).map(String::as_str).collect();
        return Some(format!(
            "worktree '{name}' has {} uncommitted or untracked file(s) ({}{}); pass --force to discard them",
            f.dirty.len(),
            shown.join(", "),
            if f.dirty.len() > 5 { ", …" } else { "" }
        ));
    }
    f.submodules
        .then(|| format!("worktree '{name}' contains submodules; pass --force to remove it"))
}

/// R149.3: every Worktree's refusal is checked before any is removed; the
/// first refusal is `USAGE` naming the Worktree and its Source.
pub fn plan_remove(
    set: &str,
    facts: &[RemoveFacts],
    force: bool,
    delete_branch: bool,
) -> Result<Vec<RemoveStep>> {
    if facts.is_empty() {
        return Err(Error::not_found(format!("no branch set '{set}'")));
    }
    for f in facts {
        if let Some(why) = remove_refusal(f, force) {
            return Err(Error::usage(format!(
                "branch set '{set}' not removed, source '{}': {why}",
                f.source
            )));
        }
    }
    Ok(facts
        .iter()
        .map(|f| RemoveStep {
            source: f.source.clone(),
            worktree: f.worktree.clone(),
            repo: f.repo.clone(),
            path: f.path.clone(),
            prune: f.missing,
            force,
            delete_branch: f.branch.clone().filter(|_| delete_branch),
        })
        .collect())
}

/// One Worktree of a set, for its Agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    /// The link (Source id).
    pub link: String,
    pub path: PathBuf,
    pub base: String,
}

/// Order members by their link's place in the Project (links the Project
/// no longer has go last).
pub fn in_project_order(project: &Project, members: &mut [Member]) {
    members.sort_by_key(|m| {
        project
            .sources
            .iter()
            .position(|s| s.source == m.link)
            .unwrap_or(usize::MAX)
    });
}

/// The lead: the first link in Project order (`members` already ordered).
pub fn lead(members: &[Member]) -> Option<&Member> {
    members.first()
}

/// R151.1: the paragraph before the user's prompt.
pub fn paragraph(name: &str, members: &[Member]) -> String {
    let list: Vec<String> = members
        .iter()
        .map(|m| format!("{} {} (base {})", m.link, m.path.display(), m.base))
        .collect();
    format!(
        "This task spans {} repositories, one Worktree each on branch {name}: {}. \
         Commit in each one; do not push unless asked.",
        members.len(),
        list.join(", ")
    )
}

/// The set paragraph, a blank line, then the user's prompt (none = the
/// paragraph alone).
pub fn prompt(name: &str, members: &[Member], user: &str) -> String {
    let p = paragraph(name, members);
    if user.trim().is_empty() {
        p
    } else {
        format!("{p}\n\n{user}")
    }
}

/// The set Agent's start (R151.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetAgent {
    /// cwd of the Pane: the lead's folder.
    pub cwd: PathBuf,
    /// Shell command (`Preset::launch` + the `add_dir` flags); in `arg`
    /// mode it already carries `prompt`, in `type` mode the caller types it.
    pub command: String,
    pub prompt: String,
}

/// Build the set Agent's command. `members` in Project order. The
/// `add_dir` flags come last, after the prompt: `claude --add-dir` takes a
/// variable number of values and would swallow a prompt put after it.
pub fn agent_launch(
    preset: &Preset,
    session_id: Option<&str>,
    name: &str,
    members: &[Member],
    user_prompt: &str,
) -> Option<SetAgent> {
    let lead = lead(members)?;
    let prompt = prompt(name, members, user_prompt);
    let mut command = preset.launch(Some(&prompt), session_id);
    if let Some(flag) = &preset.add_dir {
        for m in &members[1..] {
            command += &format!(" {flag} {}", shell_quote(&m.path.to_string_lossy()));
        }
    }
    Some(SetAgent {
        cwd: lead.path.clone(),
        command,
        prompt,
    })
}

/// Who receives a `deliver` (R151.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Deliver {
    /// Send to this Agent Pane.
    Agent(String),
    /// Several candidates: the user chooses.
    Choose(Vec<String>),
    /// No Agent: spawn the Preset (R13.2).
    Spawn,
}

fn pick(agents: &[String]) -> Option<Deliver> {
    match agents {
        [] => None,
        [one] => Some(Deliver::Agent(one.clone())),
        many => Some(Deliver::Choose(many.to_vec())),
    }
}

/// R151.2: `own` = the Agent Panes of the target Worktree, `lead` = those
/// of the set's lead Worktree. The Worktree's own Agent wins (R13.2's
/// rule: the only one, else a choice); none → the set's Agent in the lead.
pub fn deliver(own: &[String], lead: &[String]) -> Deliver {
    pick(own).or_else(|| pick(lead)).unwrap_or(Deliver::Spawn)
}

/// A Worktree of the set and its commits ahead of its base: a row of the
/// PR dialog when `ahead > 0`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrRow {
    pub source: String,
    pub worktree: String,
    pub ahead: u32,
    pub base: String,
}

/// R151.3: the rows are the Worktrees (Project order) with commits ahead.
pub fn pr_rows(candidates: &[PrRow]) -> Vec<PrRow> {
    candidates.iter().filter(|r| r.ahead > 0).cloned().collect()
}

/// A PR made for a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrRef {
    pub source: String,
    /// GitHub `owner/repo` when known (a cross-repository reference needs
    /// it), else the Source id is written.
    pub repo: Option<String>,
    pub number: u64,
}

impl PrRef {
    fn label(&self) -> String {
        format!(
            "{}#{}",
            self.repo.as_deref().unwrap_or(&self.source),
            self.number
        )
    }
}

/// `Part of <name>: <repo>#<n>, …` (R151.3); empty below two PRs: a lone
/// PR has nothing to link to.
pub fn cross_link(set: &str, made: &[PrRef]) -> String {
    if made.len() < 2 {
        return String::new();
    }
    let list: Vec<String> = made.iter().map(PrRef::label).collect();
    format!("Part of {set}: {}", list.join(", "))
}

/// A row's body with the cross-link appended after a blank line; a body
/// already carrying a `Part of <set>:` line is left as it is.
pub fn linked_body(body: &str, set: &str, suffix: &str) -> String {
    if suffix.is_empty() || body.contains(&format!("Part of {set}:")) {
        return body.to_string();
    }
    if body.trim().is_empty() {
        suffix.to_string()
    } else {
        format!("{}\n\n{suffix}", body.trim_end())
    }
}

/// `gh pr edit <n> --body <body>` arguments (run in the Source's checkout).
pub fn edit_argv(number: u64, body: &str) -> Vec<String> {
    ["pr", "edit", &number.to_string(), "--body", body]
        .map(String::from)
        .to_vec()
}

/// What (re)running Create does (R151.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Retry {
    /// Rows without a PR yet, in order.
    pub create: Vec<PrRow>,
    /// PR numbers to edit once `create` has run (empty below two rows or
    /// when `edited` has them all).
    pub edit: Vec<u64>,
}

/// `made` = PRs created so far, `edited` = numbers already edited. Never
/// creates or edits twice. The first run is `retry_plan(rows, &[], &[])`.
pub fn retry_plan(rows: &[PrRow], made: &[PrRef], edited: &[u64]) -> Retry {
    let create: Vec<PrRow> = rows
        .iter()
        .filter(|r| !made.iter().any(|m| m.source == r.source))
        .cloned()
        .collect();
    let edit = if rows.len() >= 2 {
        made.iter()
            .map(|m| m.number)
            .filter(|n| !edited.contains(n))
            .collect()
    } else {
        Vec::new()
    };
    Retry { create, edit }
}

/// The set row's text (R150.1).
pub fn row_text(name: &str, sources: usize) -> String {
    format!(
        "{name} · {sources} source{}",
        if sources == 1 { "" } else { "s" }
    )
}

/// The set row's status: the worst of its Worktrees' Agents.
pub fn row_status(statuses: impl IntoIterator<Item = Option<AgentStatus>>) -> Option<AgentStatus> {
    crate::status::worst(statuses)
}

/// A PR's checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Checks {
    /// Worst first: the `min` of some is their worst.
    Failing,
    Pending,
    Passing,
}

/// The chip of a set row (R151.4): `<k> PRs · <j> failing` and the worst
/// checks; `None` without a PR. `states`: one entry per Worktree, `None` =
/// no PR.
pub fn pr_chip(states: &[Option<Checks>]) -> Option<(String, Checks)> {
    let prs: Vec<Checks> = states.iter().flatten().copied().collect();
    let worst = prs.iter().min().copied()?;
    let failing = prs.iter().filter(|c| **c == Checks::Failing).count();
    Some((
        format!(
            "{} PR{} · {failing} failing",
            prs.len(),
            if prs.len() == 1 { "" } else { "s" }
        ),
        worst,
    ))
}

#[cfg(test)]
#[path = "branch_set_tests.rs"]
mod tests;
