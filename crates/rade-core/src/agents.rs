//! Agents in the model: spawn placement and limits (PRD R2.3–R2.5), Agent
//! Status (R10.1), Notifications on `waiting` (R10.3), and the
//! conditions `rade wait` resolves on (R10.4).

use crate::config::Limits;
use crate::layout::SplitDir;
use crate::model::State;
use crate::preset::{Invocation, Preset};
use crate::rpc::{Error, ErrorKind, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentStatus {
    Running,
    /// Needs human input.
    Waiting,
    /// Turn finished.
    Done,
    Error,
    /// The process ended.
    Exited,
    /// Its turn was cut off: RADE quit or crashed while it was `running` or
    /// `waiting`, and Session Restore resumed it idle. It needs a prompt
    /// ("continue") to go on; it is not finished.
    Interrupted,
}

impl AgentStatus {
    pub const ALL: [AgentStatus; 6] = [
        AgentStatus::Running,
        AgentStatus::Waiting,
        AgentStatus::Done,
        AgentStatus::Error,
        AgentStatus::Exited,
        AgentStatus::Interrupted,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            AgentStatus::Running => "running",
            AgentStatus::Waiting => "waiting",
            AgentStatus::Done => "done",
            AgentStatus::Error => "error",
            AgentStatus::Exited => "exited",
            AgentStatus::Interrupted => "interrupted",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|x| x.as_str() == s)
            .ok_or_else(|| {
                Error::usage(format!(
                    "unknown status '{s}' (running|waiting|done|error|exited|interrupted)"
                ))
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabChoice {
    New,
    Current,
}

/// A Worktree's Agent Presets: `(source id, worktree id)` → Presets.
pub type PresetsFor<'a> = dyn Fn(&str, &str) -> BTreeMap<String, Preset> + 'a;

/// `pane.spawn` after the app resolved the Preset into a command.
#[derive(Clone, Debug, Default)]
pub struct SpawnSpec {
    /// Source id or name; default: caller's, else the one on screen.
    pub source: Option<String>,
    /// Worktree id or name (`new` is created by the app first).
    pub worktree: Option<String>,
    pub caller: Option<String>,
    pub tab: Option<TabChoice>,
    pub split: Option<SplitDir>,
    pub command: Option<String>,
    pub name: Option<String>,
    pub agent: Option<String>,
    pub session_id: Option<String>,
    pub status: Option<AgentStatus>,
    /// Unix seconds, for `status_since`.
    pub now: u64,
    /// The Agent Account resolved by [`crate::account::resolve_spawn`]
    /// (R108.2); refused unless it is declared in the Workspace of the
    /// Pane being created (R108.3).
    pub account: Option<crate::account::Account>,
}

impl State {
    /// Create a Terminal per the placement algorithm (API.md "Placement"):
    /// `--tab new`, a non-Agent caller, or no caller → new Tab; `--split` /
    /// `--tab current` → split; an Agent caller → its Tab's Agent Grid, or a
    /// split right of it when that Tab was arranged by hand. Limits first
    /// (R2.5). Never changes what is on screen or which Pane has focus,
    /// except an explicit split, which focuses the new Pane like the key does.
    pub fn spawn(&mut self, s: SpawnSpec, limits: &Limits) -> Result<String> {
        let caller = s
            .caller
            .as_deref()
            .and_then(|c| Some((self.pane(c)?.clone(), self.locate(c)?)));
        // The Worktree a caller runs in (a Fan-out Agent's own, R15.2).
        let runs = s.caller.as_deref().and_then(|c| self.pane_worktree(c));
        let (source, mut worktree) = match (&s.source, &runs) {
            (Some(q), _) => {
                let p = self.find_source(q)?;
                (p.id.clone(), p.active_worktree.clone())
            }
            (None, Some((p, w))) => (p.clone(), w.clone()),
            (None, None) => {
                let (p, w) = self.active();
                (p.id.clone(), w.id.clone())
            }
        };
        match s.worktree.as_deref() {
            None => {}
            // The app creates it (git) and passes its name (R12, API.md rule 4).
            Some("new") => return Err(Error::usage("worktree 'new' must be created first")),
            Some(name) => {
                worktree = self
                    .source(&source)
                    .unwrap()
                    .worktrees
                    .iter()
                    .find(|w| w.name == name || w.id == name)
                    .map(|w| w.id.clone())
                    .ok_or_else(|| {
                        Error::not_found(format!("no worktree '{name}' in source '{source}'"))
                    })?
            }
        }
        self.check_limits(s.caller.as_deref(), limits)?;
        self.check_account(&source, s.account.as_ref())?;

        // Beside the caller in its Worktree, else (nothing named) the Pane
        // on screen: a Terminal on a link checkout keeps it (R104.5).
        let beside = match (&runs, &s.source, &s.worktree) {
            (Some((p, w)), _, _) if *p == source && *w == worktree => s.caller.clone(),
            (None, None, None) => self.active_tab().map(|t| t.focused.clone()),
            _ => None,
        };
        let root = self.start_root(&source, &worktree, beside.as_deref())?;
        let mut pane = self.new_pane(&root, s.command, s.name);
        self.take_setup(&source, &worktree, &mut pane);
        pane.agent = s.agent;
        pane.set_account(s.account);
        pane.session_id = s.session_id;
        pane.status = s.status;
        pane.status_since = s.status.map(|_| s.now);
        if let Some((c, _)) = &caller {
            pane.parent = Some(c.id.clone());
            pane.depth = c.depth + 1;
        }
        let id = pane.id.clone();

        let local = caller.as_ref().filter(|_| {
            runs.as_ref()
                .is_some_and(|(p, w)| *p == source && *w == worktree)
        });
        // Shown in the caller's Tab, it still runs in the caller's Worktree.
        if let Some((c, _)) = local {
            pane.runs_in = c.runs_in.clone();
        }
        let anchor = local.map(|(c, _)| c.id.clone()).or_else(|| {
            let w = self.worktree(&source, &worktree)?;
            Some(w.tab(w.active_tab.as_deref()?)?.focused.clone())
        });
        let split = s
            .split
            .or((s.tab == Some(TabChoice::Current)).then_some(SplitDir::Right));
        if s.tab != Some(TabChoice::New) {
            if let (Some(dir), Some(anchor)) = (split, &anchor) {
                return self.split_with(anchor, dir, pane, true);
            }
            if let Some((c, l)) = local.filter(|(c, _)| c.agent.is_some() && split.is_none()) {
                let tab = self.tab_mut(l)?;
                if tab.grid.is_some() || tab.panes().len() == 1 {
                    tab.grid.get_or_insert(c.id.clone());
                    tab.title_bars.push(pane);
                    tab.arrange();
                    return Ok(id);
                }
                return self.split_with(&c.id.clone(), SplitDir::Right, pane, false);
            }
        }
        pane.runs_in = None; // a Tab of its own Worktree
        self.new_tab_with(&source, &worktree, pane)
            .map(|(_, pane)| pane)
    }

    /// Spawn limits for a caller (R2.5): depth and live Subagents.
    pub fn check_limits(&self, caller: Option<&str>, limits: &Limits) -> Result<()> {
        let Some(c) = caller.and_then(|c| self.pane(c)) else {
            return Ok(());
        };
        if c.depth + 1 > limits.max_depth {
            return Err(Error::new(
                ErrorKind::Limit,
                format!(
                    "spawn depth limit: {} is at depth {}, limits.max_depth = {}",
                    c.id, c.depth, limits.max_depth
                ),
            ));
        }
        let live = self.live_children(&c.id);
        if live >= limits.max_children as usize {
            return Err(Error::new(
                ErrorKind::Limit,
                format!(
                    "child limit: {} already has {live} live Subagents, limits.max_children = {}",
                    c.id, limits.max_children
                ),
            ));
        }
        Ok(())
    }

    /// Subagents of `pane` whose process has not exited (R2.5).
    pub fn live_children(&self, pane: &str) -> usize {
        self.all_panes()
            .filter(|p| p.parent.as_deref() == Some(pane) && p.status != Some(AgentStatus::Exited))
            .count()
    }

    pub(crate) fn all_panes(&self) -> impl Iterator<Item = &crate::Pane> {
        self.worktrees()
            .flat_map(|(_, w)| w.tabs.iter().flat_map(|t| t.panes()))
    }

    /// Hook entry point (`rade status`): set status, session id, Preset.
    /// Entering `waiting` raises a Notification (R10.3).
    pub fn set_status(
        &mut self,
        pane: &str,
        status: Option<AgentStatus>,
        session_id: Option<String>,
        agent: Option<String>,
        now: u64,
    ) -> Result<()> {
        let loc = self.locate_or_err(pane)?;
        // A paused Source's Agents were stopped mid-whatever (R19.3): their
        // exit hooks must not overwrite the status Resume brings back.
        let status = status.filter(|_| !self.is_paused(&loc.source));
        let p = self.tab_mut(&loc)?.pane_mut(pane).unwrap();
        let was = p.status;
        // A resumed Agent reporting `done` before any new turn began (a
        // hook firing on session start) has not finished the cut-off turn.
        let status =
            status.filter(|s| !(was == Some(AgentStatus::Interrupted) && *s == AgentStatus::Done));
        if status.is_some() {
            p.status = status;
            p.status_heuristic = false;
            if was != status {
                p.status_since = Some(now);
            }
        }
        if status.is_some_and(|s| s != AgentStatus::Exited) {
            p.had_turn = true;
        }
        // A session-start report (Preset, no status) in a shell whose last
        // hand-started Agent exited (R10.2 command-finished): alive again.
        if status.is_none()
            && agent.is_some()
            && p.command().is_none()
            && p.status == Some(AgentStatus::Exited)
        {
            p.status = None;
            p.status_since = None;
        }
        if session_id.is_some() {
            p.session_id = session_id;
        }
        if agent.is_some() {
            p.agent = agent;
        }
        let name = p.name.clone().unwrap_or_else(|| pane.to_string());
        // Only on a transition: a repeated `done` without a new turn is
        // not news (R3.1, R10.3; done/error: owner decision A, 2026-09-30).
        let text = match status.filter(|s| was != Some(*s)) {
            Some(AgentStatus::Waiting) => Some("needs input"),
            Some(AgentStatus::Done) => Some("finished"),
            Some(AgentStatus::Error) => Some("failed"),
            _ => None,
        };
        if let Some(text) = text {
            self.notify_status(pane, &format!("{name} {text}"), now);
        }
        self.track_fanout(pane, status);
        Ok(())
    }

    /// Record the Agent's real invocation (R11.2; `pane.set_status`).
    pub fn set_invocation(&mut self, pane: &str, invocation: Invocation) -> Result<()> {
        let loc = self.locate_or_err(pane)?;
        self.tab_mut(&loc)?.pane_mut(pane).unwrap().invocation = Some(invocation);
        Ok(())
    }

    /// Session Restore (R11.2): no process survived (ADR-0005), so statuses
    /// are cleared and each Pane gets the command it restarts with
    /// ([`restore_command`]). An Agent that was mid-turn (`running` or
    /// `waiting`, or still `interrupted`) and comes back as an Agent is
    /// `interrupted` (since `now`): resumed idle, its turn not finished.
    pub fn prepare_restore(&mut self, presets: &BTreeMap<String, Preset>, now: u64) {
        self.prepare_restore_with(&|_, _| presets.clone(), now);
    }

    /// [`Self::prepare_restore`] with each Worktree's own Presets (its
    /// config layers, R29.2: `(source id, worktree id)` → Presets), asked
    /// once per Worktree.
    pub fn prepare_restore_with(&mut self, presets: &PresetsFor<'_>, now: u64) {
        let ids: Vec<String> = self.all_panes().map(|p| p.id.clone()).collect();
        self.restore_panes(&ids, presets, now);
        self.restore_fanouts(None);
    }

    /// [`Self::restore_pane`] for each Pane, with the Presets of the
    /// Worktree it runs in.
    pub(crate) fn restore_panes(&mut self, ids: &[String], presets: &PresetsFor<'_>, now: u64) {
        let mut by_worktree: BTreeMap<(String, String), BTreeMap<String, Preset>> = BTreeMap::new();
        for id in ids {
            let Some(key) = self.pane_worktree(id) else {
                continue;
            };
            let p = by_worktree
                .entry(key)
                .or_insert_with_key(|(s, w)| presets(s, w))
                .clone();
            self.restore_pane(id, &p, now);
        }
    }

    /// One Pane as Session Restore leaves it (see [`Self::prepare_restore`]);
    /// pausing a Source does the same to its Panes (R19.3).
    pub(crate) fn restore_pane(&mut self, id: &str, presets: &BTreeMap<String, Preset>, now: u64) {
        let Some(loc) = self.locate(id) else { return };
        let Ok(t) = self.tab_mut(&loc) else { return };
        let p = t.pane_mut(id).unwrap();
        let command = restore_command(p, presets);
        // A hand-started Agent that exited: its shell is all that
        // comes back, so it is no longer an Agent Pane.
        if p.command().is_none() && p.status == Some(AgentStatus::Exited) {
            p.agent = None;
            p.session_id = None;
            p.invocation = None;
            p.had_turn = false;
        }
        let mid_turn = matches!(
            p.status,
            Some(AgentStatus::Running | AgentStatus::Waiting | AgentStatus::Interrupted)
        );
        let since = match p.status {
            Some(AgentStatus::Interrupted) => p.status_since, // restored again
            _ => Some(now),
        };
        p.status = (mid_turn && command.is_some() && p.agent.is_some())
            .then_some(AgentStatus::Interrupted);
        p.status_since = p.status.and(since);
        p.status_heuristic = false;
        p.hook_at = None;
        if let crate::PaneKind::Terminal { command: c, .. } = &mut p.kind {
            *c = command;
        }
    }
}

/// What a restored Pane runs (R11.2). Never replays the launch prompt: an
/// Agent resumes its saved session (if it had a turn), else starts fresh.
/// With a recorded invocation both keep its config env and flags (`clw` =
/// `CLAUDE_CONFIG_DIR=… claude --dangerously-skip-permissions`); without
/// one they use the Preset's resume / launch command. `--cmd` Panes and
/// Panes whose Preset is gone come back as a plain shell in their cwd
/// (arbitrary commands are not re-run).
pub fn restore_command(pane: &crate::Pane, presets: &BTreeMap<String, Preset>) -> Option<String> {
    // An Agent started by hand in a shell that has since exited (R10.2
    // command-finished): the Pane is a shell again.
    if pane.command().is_none() && pane.status == Some(AgentStatus::Exited) {
        return None;
    }
    let p = presets.get(pane.agent.as_deref()?)?;
    let id = pane.session_id.as_deref().filter(|_| pane.had_turn);
    if let Some(cmd) = pane
        .invocation
        .as_ref()
        .and_then(|inv| p.invocation_command(inv, id))
    {
        return Some(cmd);
    }
    id.and_then(|id| p.resume_command(id))
        .or_else(|| Some(p.launch(None, None)))
}

/// `rade wait --until …` (API.md).
#[derive(Clone, Debug)]
pub enum WaitUntil {
    Status(AgentStatus),
    /// No output for `idle_seconds`.
    Idle,
    /// `/regex/` against the screen text (scrollback included).
    Pattern(regex_lite::Regex),
}

impl WaitUntil {
    pub fn parse(s: &str) -> Result<Self> {
        if s == "idle" {
            return Ok(Self::Idle);
        }
        if let Some(re) = s.strip_prefix('/').and_then(|r| r.strip_suffix('/')) {
            return regex_lite::Regex::new(re)
                .map(Self::Pattern)
                .map_err(|e| Error::usage(format!("bad --until regex: {e}")));
        }
        AgentStatus::parse(s).map(Self::Status).map_err(|_| {
            Error::usage(format!(
                "--until must be done|waiting|exited|error|running|interrupted|idle|/regex/, not '{s}'"
            ))
        })
    }

    /// Whether the wait is over; `Some(matched)` if so. A status wait also
    /// ends when the Agent exits or (waiting for `done`) errors, since it
    /// can't reach the target any more; callers check the returned status.
    pub fn check(
        &self,
        status: Option<AgentStatus>,
        text: &str,
        quiet_for: Duration,
        idle_after: Duration,
    ) -> Option<String> {
        match self {
            Self::Status(want) => {
                let s = status?;
                let over = s == *want
                    || s == AgentStatus::Exited
                    || (*want == AgentStatus::Done && s == AgentStatus::Error);
                over.then(|| s.as_str().to_string())
            }
            Self::Idle => (quiet_for >= idle_after).then(|| "idle".into()),
            Self::Pattern(re) => re.find(text).map(|m| m.as_str().to_string()),
        }
    }

    /// Needs the whole screen text each poll (only regex does).
    pub fn needs_text(&self) -> bool {
        matches!(self, Self::Pattern(_))
    }
}

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
