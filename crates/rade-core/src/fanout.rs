//! Fan-out, Judge, Winner (PRD R15): one prompt to N Agents, each in its own
//! new Worktree, shown side by side in one Fan-out Tab of the Source's main
//! Worktree; a Judge Agent compares their diffs and recommends a Winner; the
//! user picks one. GTK-free: planning (names, limits), the state machine,
//! the Judge's command, prompt and verdict. Git runs in the app
//! ([`crate::merge`] for the Winner).

use crate::agents::AgentStatus;
use crate::config::Limits;
use crate::layout::{Layout, SplitDir};
use crate::model::{State, Tab};
use crate::rpc::{Error, ErrorKind, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A Fan-out compares at least two Agents.
pub const MIN_AGENTS: usize = 2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fanout {
    /// `fanout-<n>` or the `--name`; its Worktrees are `<id>-<i>`.
    pub id: String,
    /// Source id.
    #[serde(alias = "project")] // v1 (R24.4)
    pub source: String,
    pub prompt: String,
    /// The ref the Worktrees started from, and its commit (diffs are
    /// against it).
    pub base: String,
    pub base_commit: String,
    /// The Fan-out Tab, in the Source's main Worktree.
    pub tab: String,
    pub members: Vec<Member>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judge: Option<Judge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub winner: Option<Winner>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Member {
    /// `<preset>-<i>`: the Pane's name.
    pub label: String,
    pub agent: String,
    /// Worktree name (= its branch), `<fanout id>-<i>`.
    pub worktree: String,
    pub pane: String,
    /// Its turn ended (done, error, exited, or its Pane closed); kept
    /// across Session Restore, which clears Agent Status.
    #[serde(default)]
    pub finished: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Judge {
    pub agent: String,
    pub pane: String,
    /// Where the candidates' diffs were written for it.
    pub dir: PathBuf,
    /// Its Preset could be made read-only (claude, codex); others only get
    /// told to be.
    pub read_only: bool,
    #[serde(default)]
    pub finished: bool,
    /// The recommended Member (label), once parsed from its answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pick: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ranking: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strategy {
    /// The Winner's whole result (commits, edits, new files) staged on the
    /// Source's current branch, not committed (`git merge --squash`).
    Squash,
    /// Keep the Winner's Worktree and branch as they are; remove the others.
    Keep,
}

impl Strategy {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "squash" => Ok(Self::Squash),
            "keep" => Ok(Self::Keep),
            o => Err(Error::usage(format!(
                "--strategy must be squash|keep, not {o}"
            ))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Winner {
    pub label: String,
    pub strategy: Strategy,
    /// Squash: the branch it was staged on, and files left in conflict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
}

/// Where a Fan-out is (derived; see [`Fanout::phase`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// Some Agents are still working.
    Running,
    /// Every Agent finished; no verdict yet.
    Finished,
    /// The Judge is working.
    Judging,
    /// The Judge recommended a Winner.
    Judged,
    /// A Winner was chosen and applied.
    Chosen,
}

impl Fanout {
    /// running → finished → judging → judged → chosen (the Judge is
    /// optional; a Judge that answered without a verdict leaves it
    /// `finished`, so it can be judged again). Discarding removes it.
    pub fn phase(&self) -> Phase {
        match (&self.winner, &self.judge) {
            (Some(_), _) => Phase::Chosen,
            // An Agent back at work outranks an older verdict.
            _ if self.done() < self.members.len() => Phase::Running,
            (None, Some(j)) if j.pick.is_some() => Phase::Judged,
            (None, Some(j)) if !j.finished => Phase::Judging,
            _ => Phase::Finished,
        }
    }

    /// The Worktree a discard keeps: a kept Winner's, or a squashed one's
    /// whose merge conflicted (its result is still needed).
    pub fn kept_worktree(&self) -> Option<&str> {
        let w = self
            .winner
            .as_ref()
            .filter(|w| w.strategy == Strategy::Keep || !w.conflicts.is_empty())?;
        self.member(&w.label).map(|m| m.worktree.as_str())
    }

    /// Members whose turn ended.
    pub fn done(&self) -> usize {
        self.members.iter().filter(|m| m.finished).count()
    }

    /// A Member by label, Worktree name, or `<id>-<i>`; case-insensitive.
    pub fn member(&self, q: &str) -> Option<&Member> {
        let q = q.trim();
        self.members
            .iter()
            .find(|m| m.label.eq_ignore_ascii_case(q) || m.worktree.eq_ignore_ascii_case(q))
    }

    /// The Member to choose: `q`, else the Judge's pick. Only a finished
    /// one, and only once.
    pub fn check_choose(&self, q: Option<&str>) -> Result<&Member> {
        if let Some(w) = &self.winner {
            return Err(Error::usage(format!(
                "fan-out '{}' already has a Winner ({})",
                self.id, w.label
            )));
        }
        let q = match (q, self.judge.as_ref().and_then(|j| j.pick.as_deref())) {
            (Some(q), _) => q,
            (None, Some(p)) => p,
            (None, None) => {
                return Err(Error::usage(
                    "name the Winner (no Judge recommendation to default to)",
                ))
            }
        };
        let m = self.member(q).ok_or_else(|| {
            let names: Vec<&str> = self.members.iter().map(|m| m.label.as_str()).collect();
            Error::not_found(format!(
                "no Agent '{q}' in fan-out '{}' ({})",
                self.id,
                names.join(", ")
            ))
        })?;
        if !m.finished {
            return Err(Error::usage(format!(
                "{} is still working; wait for it (rade fanout wait {})",
                m.label, self.id
            )));
        }
        Ok(m)
    }

    /// A Judge may start once every Agent finished (again after a Judge
    /// without a verdict, or to replace a verdict), not after a Winner.
    pub fn check_judge(&self) -> Result<()> {
        match self.phase() {
            Phase::Running => Err(Error::usage(format!(
                "{}/{} Agents done; wait for all (rade fanout wait {})",
                self.done(),
                self.members.len(),
                self.id
            ))),
            Phase::Judging => Err(Error::usage("the Judge is still working")),
            Phase::Chosen => Err(Error::usage("a Winner was already chosen")),
            Phase::Finished | Phase::Judged => Ok(()),
        }
    }
}

/// A planned Fan-out: its id and (label, preset, Worktree name) per Agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub id: String,
    pub members: Vec<(String, String, String)>,
}

/// Names and limits (R15.2, R2.5): 2..=`limits.max_children` Agents; id
/// `--name` or the first free `fanout-<n>`; Worktrees `<id>-<i>` (also
/// their branches); labels `<preset>-<i>`. `taken` says whether a name is
/// used (a Fan-out, a Worktree, a branch or a folder).
pub fn plan(
    agents: &[String],
    name: Option<&str>,
    limits: &Limits,
    taken: &dyn Fn(&str) -> bool,
) -> Result<Plan> {
    if agents.len() < MIN_AGENTS {
        return Err(Error::usage(format!(
            "a Fan-out needs at least {MIN_AGENTS} Agents (--agents a,b)"
        )));
    }
    if agents.len() > limits.max_children as usize {
        return Err(Error::new(
            ErrorKind::Limit,
            format!(
                "fan-out of {} Agents exceeds limits.max_children = {}",
                agents.len(),
                limits.max_children
            ),
        ));
    }
    let names =
        |id: &str| -> Vec<String> { (1..=agents.len()).map(|i| format!("{id}-{i}")).collect() };
    let free = |id: &str| !taken(id) && names(id).iter().all(|n| !taken(n));
    let id = match name {
        Some(n) => {
            let n = n.trim();
            crate::git::validate_name(n)?;
            for w in names(n) {
                crate::git::validate_name(&w)?;
            }
            if !free(n) {
                return Err(Error::usage(format!(
                    "fan-out name '{n}' is taken (a Fan-out, Worktree or branch '{n}-<i>' exists)"
                )));
            }
            n.to_string()
        }
        None => (1..)
            .map(|k| format!("fanout-{k}"))
            .find(|id| free(id))
            .expect("unbounded"),
    };
    let members = agents
        .iter()
        .zip(names(&id))
        .enumerate()
        .map(|(i, (a, w))| (format!("{}-{}", crate::model::slug(a), i + 1), a.clone(), w))
        .collect();
    Ok(Plan { id, members })
}

/// One Agent to start: the app resolved its Preset and created its Worktree.
pub struct MemberSpawn {
    pub label: String,
    pub agent: String,
    /// Worktree id.
    pub worktree: String,
    pub command: String,
    pub session_id: Option<String>,
    /// Its Agent Account ([`State::agent_account`], R108.2); refused unless
    /// the Source's Workspace declares it (R108.3).
    pub account: Option<crate::account::Account>,
}

/// The Judge to start ([`State::add_judge`]): the app resolved its
/// Preset ([`judge_command`]) and wrote the diffs to `dir`.
pub struct JudgeSpawn {
    pub agent: String,
    pub command: String,
    pub session_id: Option<String>,
    pub dir: PathBuf,
    pub read_only: bool,
    pub now: u64,
    /// Its Agent Account, as for a Member.
    pub account: Option<crate::account::Account>,
}

/// A new Fan-out for [`State::start_fanout`].
pub struct Start {
    pub source: String,
    pub id: String,
    pub prompt: String,
    pub base: String,
    pub base_commit: String,
    pub members: Vec<MemberSpawn>,
    pub caller: Option<String>,
    pub now: u64,
}

impl State {
    pub fn fanout(&self, id: &str) -> Option<&Fanout> {
        self.fanouts.iter().find(|f| f.id == id)
    }

    fn fanout_mut(&mut self, id: &str) -> Result<&mut Fanout> {
        self.fanouts
            .iter_mut()
            .find(|f| f.id == id)
            .ok_or_else(|| Error::not_found(format!("no fan-out '{id}'")))
    }

    /// A Fan-out by id; `source` narrows it (ids are per Source).
    pub fn find_fanout(&self, id: &str, source: Option<&str>) -> Result<&Fanout> {
        let source = source
            .map(|p| self.find_source(p).map(|p| p.id.clone()))
            .transpose()?;
        let hits: Vec<&Fanout> = self
            .fanouts
            .iter()
            .filter(|f| f.id == id && source.as_ref().is_none_or(|p| &f.source == p))
            .collect();
        match hits.len() {
            1 => Ok(hits[0]),
            0 => Err(Error::not_found(format!("no fan-out '{id}'"))),
            _ => Err(Error::new(
                ErrorKind::Ambiguous,
                format!("fan-out '{id}' exists in several Sources; pass --source"),
            )),
        }
    }

    /// Limits for `n` Agents started by `caller` (R2.5): its depth, and its
    /// live Subagents plus these.
    pub fn check_fanout_limits(
        &self,
        caller: Option<&str>,
        n: usize,
        limits: &Limits,
    ) -> Result<()> {
        self.check_limits(caller, limits)?;
        let Some(c) = caller.filter(|c| self.pane(c).is_some()) else {
            return Ok(());
        };
        let live = self.live_children(c);
        if live + n > limits.max_children as usize {
            return Err(Error::new(
                ErrorKind::Limit,
                format!(
                    "child limit: {c} has {live} live Subagents; {n} more exceed limits.max_children = {}",
                    limits.max_children
                ),
            ));
        }
        Ok(())
    }

    /// Open the Fan-out Tab (R15.2) in the Source's main Worktree: one
    /// Agent Pane per Member, tiled side by side, each running in its own
    /// Worktree (`runs_in`, cwd, that Worktree's `setup`). Not shown or
    /// focused (like a spawn). Returns (tab, panes).
    pub fn start_fanout(&mut self, s: Start, limits: &Limits) -> Result<(String, Vec<String>)> {
        self.check_fanout_limits(s.caller.as_deref(), s.members.len(), limits)?;
        let main = self
            .source(&s.source)
            .and_then(|p| p.worktrees.first())
            .map(|w| w.id.clone())
            .ok_or_else(|| Error::not_found(format!("no source '{}'", s.source)))?;
        let parent = s
            .caller
            .as_deref()
            .and_then(|c| self.pane(c))
            .map(|c| (c.id.clone(), c.depth + 1));
        for m in &s.members {
            self.check_account(&s.source, m.account.as_ref())?;
        }
        let mut panes = Vec::new();
        let mut members = Vec::new();
        for m in s.members {
            let root = self.worktree_mut(&s.source, &m.worktree)?.path.clone();
            let mut pane = self.new_pane(&root, Some(m.command), Some(m.label.clone()));
            self.take_setup(&s.source, &m.worktree, &mut pane);
            pane.agent = Some(m.agent.clone());
            pane.set_account(m.account);
            pane.session_id = m.session_id;
            pane.status = Some(AgentStatus::Running);
            pane.status_since = Some(s.now);
            pane.runs_in = Some(m.worktree.clone());
            if let Some((p, d)) = &parent {
                pane.parent = Some(p.clone());
                pane.depth = *d;
            }
            let worktree = self.worktree(&s.source, &m.worktree).unwrap().name.clone();
            members.push(Member {
                label: m.label,
                agent: m.agent,
                worktree,
                pane: pane.id.clone(),
                finished: false,
            });
            panes.push(pane);
        }
        let ids: Vec<String> = panes.iter().map(|p| p.id.clone()).collect();
        let tab = self.next("t");
        let wt = self.worktree_mut(&s.source, &main)?;
        wt.tabs.push(Tab {
            id: tab.clone(),
            name: s.id.clone(),
            focused: ids[0].clone(),
            layout: Layout::tiles(panes),
            zoomed: None,
            title_bars: Vec::new(),
            grid: None,
            renamed: true,
            last_viewer: None,
            schedule: None,
        });
        wt.active_tab.get_or_insert(tab.clone());
        self.fanouts.push(Fanout {
            id: s.id,
            source: s.source,
            prompt: s.prompt,
            base: s.base,
            base_commit: s.base_commit,
            tab: tab.clone(),
            members,
            judge: None,
            winner: None,
        });
        Ok((tab, ids))
    }

    /// Latch each Member's (and Judge's) end of turn from its Pane's Agent
    /// Status (called on every status change): done/error/exited finish
    /// it, a new turn (running/waiting) reopens it.
    pub(crate) fn track_fanout(&mut self, pane: &str, status: Option<AgentStatus>) {
        let Some(s) = status else { return };
        let finished = matches!(
            s,
            AgentStatus::Done | AgentStatus::Error | AgentStatus::Exited
        );
        for f in &mut self.fanouts {
            for m in f.members.iter_mut().filter(|m| m.pane == pane) {
                m.finished = finished;
                // Its result changes: a verdict on the old one is stale.
                if let Some(j) = f.judge.as_mut().filter(|_| !finished && f.winner.is_none()) {
                    j.pick = None;
                    j.ranking.clear();
                }
            }
            if let Some(j) = f.judge.as_mut().filter(|j| j.pane == pane) {
                j.finished = finished;
                if !finished {
                    j.pick = None; // a new turn gives a new verdict
                    j.ranking.clear();
                }
            }
        }
    }

    /// Start the Judge (R15.3): its Pane goes below the Agents in the
    /// Fan-out Tab (replacing an earlier Judge), running in the main
    /// Worktree. Returns its Pane.
    pub fn add_judge(&mut self, id: &str, j: JudgeSpawn) -> Result<String> {
        let f = self
            .fanout(id)
            .ok_or_else(|| Error::not_found(format!("no fan-out '{id}'")))?;
        f.check_judge()?;
        let (source, tab) = (f.source.clone(), f.tab.clone());
        self.check_not_paused(&source)?; // before anything changes (R19.3)
        self.check_account(&source, j.account.as_ref())?;
        let old = f.judge.as_ref().map(|j| j.pane.clone());
        let main = self
            .source(&source)
            .ok_or_else(|| Error::not_found(format!("no source '{source}'")))?
            .worktrees[0]
            .clone();
        let mut pane = self.new_pane(&main.path, Some(j.command), Some("judge".into()));
        pane.agent = Some(j.agent.clone());
        pane.set_account(j.account);
        pane.session_id = j.session_id;
        pane.status = Some(AgentStatus::Running);
        pane.status_since = Some(j.now);
        let pid = pane.id.clone();
        if let Some(old) = old.filter(|o| self.locate(o).is_some()) {
            self.close_pane(&old)?;
        }
        let loc = crate::model::PaneLoc {
            source: source.clone(),
            worktree: main.id.clone(),
            tab,
        };
        match self.tab_mut(&loc) {
            Ok(t) => {
                let grid = std::mem::replace(&mut t.layout, Layout::leaf(pane.clone()));
                t.layout = Layout::split_of(SplitDir::Down, 0.62, grid, Layout::leaf(pane));
                t.zoomed = None;
            }
            // The Fan-out Tab was closed: the Judge gets a Tab of its own.
            Err(_) => {
                let (t, _) = self.new_tab_with(&source, &main.id, pane)?;
                self.fanout_mut(id)?.tab = t;
            }
        }
        self.fanout_mut(id)?.judge = Some(Judge {
            agent: j.agent,
            pane: pid.clone(),
            dir: j.dir,
            read_only: j.read_only,
            finished: false,
            pick: None,
            ranking: Vec::new(),
        });
        Ok(pid)
    }

    /// Winner kept (`keep`): its Agent moves from the Fan-out Tab to a new
    /// Tab of its own Worktree (the Terminal keeps running), so cleaning up
    /// the Fan-out doesn't close it. Returns that Tab.
    pub fn move_to_own_worktree(&mut self, pane: &str) -> Result<String> {
        let (source, worktree) = self
            .pane_worktree(pane)
            .ok_or_else(|| Error::not_found(format!("no pane '{pane}'")))?;
        self.check_not_paused(&source)?; // before the Pane leaves its Tab (R19.3)
        let mut p = self.pane(pane).unwrap().clone();
        p.runs_in = None;
        self.close_pane(pane)?;
        let (tab, _) = self.new_tab_with(&source, &worktree, p)?;
        Ok(tab)
    }

    pub fn set_verdict(&mut self, id: &str, v: Verdict) -> Result<()> {
        let j = self
            .fanout_mut(id)?
            .judge
            .as_mut()
            .ok_or_else(|| Error::usage("no Judge"))?;
        j.pick = Some(v.pick);
        j.ranking = v.ranking;
        Ok(())
    }

    pub fn set_winner(&mut self, id: &str, w: Winner) -> Result<()> {
        self.fanout_mut(id)?.winner = Some(w);
        Ok(())
    }

    /// Drop a Fan-out (discard / cleanup): its Tab closes; the app removes
    /// its Worktrees. Returns it.
    pub fn remove_fanout(&mut self, id: &str) -> Result<Fanout> {
        let i = self
            .fanouts
            .iter()
            .position(|f| f.id == id)
            .ok_or_else(|| Error::not_found(format!("no fan-out '{id}'")))?;
        let f = self.fanouts.remove(i);
        if self.locate_tab(&f.tab).is_some() {
            self.close_tab(&f.tab)?;
        }
        Ok(f)
    }

    /// Session Restore (R15.5; after [`State::prepare_restore`]): each
    /// Member keeps its progress — one that was mid-turn comes back
    /// `interrupted`, not finished, until a prompt makes it finish; any
    /// other has no turn left and counts as finished. A Judge
    /// comes back as a plain shell and counts as finished: its verdict is
    /// kept, and its read-only launch can't be resumed safely (a Preset's
    /// resume command would drop it).
    /// `source`: only that Source's Fan-outs (pausing it, R19.3).
    pub(crate) fn restore_fanouts(&mut self, source: Option<&str>) {
        let interrupted: std::collections::HashSet<String> = self
            .fanouts
            .iter()
            .flat_map(|f| self.interrupted_members(f))
            .collect();
        let mut judges = Vec::new();
        for f in self
            .fanouts
            .iter_mut()
            .filter(|f| source.is_none_or(|p| f.source == p))
        {
            // One that can't resume (its Preset is gone: a plain shell now)
            // has no turn left to finish.
            for m in f
                .members
                .iter_mut()
                .filter(|m| !interrupted.contains(&m.pane))
            {
                m.finished = true;
            }
            if let Some(j) = f.judge.as_mut() {
                j.finished = true;
                judges.push(j.pane.clone());
            }
        }
        for id in judges {
            let Some(loc) = self.locate(&id) else {
                continue;
            };
            if let Ok(t) = self.tab_mut(&loc) {
                let p = t.pane_mut(&id).unwrap();
                if let crate::PaneKind::Terminal { command, .. } = &mut p.kind {
                    *command = None;
                }
                p.agent = None;
                p.session_id = None;
                p.invocation = None;
                p.status = None;
                p.status_since = None;
            }
        }
    }

    /// Whether `pane` may close without `--force` (`rade close`, the close
    /// key): an Agent of a Fan-out that has no Winner yet may not — closing
    /// it ends its turn and drops it from the comparison, so it is refused
    /// like closing the Fan-out Tab asks (R15).
    pub fn check_close(&self, pane: &str) -> Result<()> {
        let hit = self.fanouts.iter().find_map(|f| {
            let m = f.members.iter().find(|m| m.pane == pane)?;
            f.winner.is_none().then_some((f, m))
        });
        match hit {
            None => Ok(()),
            Some((f, m)) => Err(Error::usage(format!(
                "{pane} ({}) is an Agent of fan-out '{}': closing it stops its work and \
                 drops it from the comparison. End the Fan-out with `rade fanout discard {}`, \
                 or close only this Agent with `rade close {pane} --force`",
                m.label, f.id, f.id
            ))),
        }
    }

    /// Interrupted Members of a Fan-out (Session Restore cut their turn).
    pub fn interrupted_members(&self, f: &Fanout) -> Vec<String> {
        f.members
            .iter()
            .filter(|m| {
                self.pane(&m.pane)
                    .is_some_and(|p| p.status == Some(AgentStatus::Interrupted))
            })
            .map(|m| m.pane.clone())
            .collect()
    }

    /// The Fan-out whose Tab this is (the Fan-out bar).
    pub fn fanout_of_tab(&self, tab: &str) -> Option<&Fanout> {
        self.fanouts.iter().find(|f| f.tab == tab)
    }
}

/// What "Continue" sends an interrupted Fan-out Agent: works for one that
/// resumed its conversation and for one that had to start fresh (no
/// session id to resume, R11.2).
pub fn continue_prompt(f: &Fanout) -> String {
    format!(
        "Continue from where you left off. If you have no record of this task, here it is:\n\n{}",
        f.prompt
    )
}

/// The `rade fanout status` summary of a Fan-out (without git stats).
pub fn summary(ws: &State, f: &Fanout) -> serde_json::Value {
    let status = |pane: &str| {
        ws.pane(pane)
            .and_then(|p| p.status)
            .map(|s| s.as_str().to_string())
    };
    let members: Vec<serde_json::Value> = f
        .members
        .iter()
        .map(|m| {
            let path = ws
                .source(&f.source)
                .and_then(|p| p.worktrees.iter().find(|w| w.name == m.worktree))
                .map(|w| w.path.clone());
            serde_json::json!({
                "label": m.label, "agent": m.agent, "worktree": m.worktree,
                "branch": m.worktree, "path": path, "pane": m.pane,
                "status": status(&m.pane), "finished": m.finished,
            })
        })
        .collect();
    let judge = f.judge.as_ref().map(|j| {
        serde_json::json!({
            "agent": j.agent, "pane": j.pane, "status": status(&j.pane),
            "finished": j.finished, "read_only": j.read_only, "dir": j.dir,
            "pick": j.pick, "ranking": j.ranking,
        })
    });
    serde_json::json!({
        "fanout": f.id, "source": f.source, "prompt": f.prompt, "base": f.base,
        "tab": f.tab, "phase": f.phase(), "done": f.done(), "total": f.members.len(),
        "interrupted": ws.interrupted_members(f).len(),
        "members": members, "judge": judge, "winner": f.winner,
    })
}

#[path = "fanout_judge.rs"]
mod judge;
pub use judge::*;

#[cfg(test)]
#[path = "fanout_tests.rs"]
mod tests;
