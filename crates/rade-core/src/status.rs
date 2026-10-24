//! Agent Status beyond hooks (PRD R10.2, R10.3): the dot palette, worst-of
//! roll-ups for Tabs and Sources, the fallback rules that set a status when
//! an Agent's hooks can't, and the one-time "Codex hooks not trusted" hint
//! (R16.4). GTK-free; the app feeds it Terminal signals and renders dots.

use crate::agents::AgentStatus;
use crate::layout::Pane;
use crate::model::State;
use crate::model::Tab;
use crate::preset::{HookKind, Preset};
use crate::rpc::Result;
use std::collections::BTreeMap;

/// Fallbacks never replace a status a hook reported this recently.
pub const HOOK_GRACE_SECS: u64 = 10;
/// A Codex turn this old with no hook report since it began → hint (R16.4).
pub const CODEX_HINT_SECS: u64 = 30;
/// Glyph colour for an Agent Pane without a status yet (light, dark).
pub const NO_STATUS_COLOR: (&str, &str) = ("#77767b", "#9a9996");

impl AgentStatus {
    /// Glyph colour (R10.3, R20.1) in the light or dark style: every one has
    /// ≥ 3:1 contrast (WCAG 1.4.11, graphics) on libadwaita's window, view,
    /// sidebar and caption backgrounds of that style (tested; the same
    /// tokens are in the app's stylesheet and `docs/internals/design/contrast.py`).
    /// `running` is a fixed blue, never the accent (owner decision, M14).
    pub fn color(self, dark: bool) -> &'static str {
        let (light, dk) = match self {
            AgentStatus::Running => ("#1c71d8", "#78aeed"),
            AgentStatus::Waiting => ("#a87200", "#f6d32d"),
            AgentStatus::Done => ("#1c8a55", "#57e389"),
            AgentStatus::Error => ("#e01b24", "#ff7b63"),
            AgentStatus::Exited => NO_STATUS_COLOR,
            AgentStatus::Interrupted => ("#9141ac", "#dc8add"),
        };
        if dark {
            dk
        } else {
            light
        }
    }

    /// Worst first: what most needs the user. `error`, `waiting` and
    /// `interrupted` need action; `running` is work in progress, more
    /// telling than the resting `done`; `exited` is gone.
    pub(crate) fn rank(self) -> u8 {
        match self {
            AgentStatus::Error => 0,
            AgentStatus::Waiting => 1,
            AgentStatus::Interrupted => 2,
            AgentStatus::Running => 3,
            AgentStatus::Done => 4,
            AgentStatus::Exited => 5,
        }
    }
}

/// The worst of some statuses ([`AgentStatus::rank`]); `None` if none has one.
pub fn worst(statuses: impl IntoIterator<Item = Option<AgentStatus>>) -> Option<AgentStatus> {
    statuses.into_iter().flatten().min_by_key(|s| s.rank())
}

/// The Pane with the worst status (the longest in it on a tie), for an
/// aggregate dot's tooltip.
pub fn worst_pane<'a>(panes: impl IntoIterator<Item = &'a Pane>) -> Option<&'a Pane> {
    panes
        .into_iter()
        .filter_map(|p| Some((p.status?.rank(), p.status_since.unwrap_or(0), p)))
        .min_by_key(|(rank, since, _)| (*rank, *since))
        .map(|(_, _, p)| p)
}

/// "2 running · 1 done": Agent Panes per status, worst first.
pub fn summary<'a>(panes: impl IntoIterator<Item = &'a Pane>) -> String {
    let mut n = [0usize; AgentStatus::ALL.len()];
    for s in panes.into_iter().filter_map(|p| p.status) {
        n[s.rank() as usize] += 1;
    }
    let mut all = AgentStatus::ALL;
    all.sort_by_key(|s| s.rank());
    all.iter()
        .filter(|s| n[s.rank() as usize] > 0)
        .map(|s| format!("{} {}", n[s.rank() as usize], s.as_str()))
        .collect::<Vec<_>>()
        .join(" · ")
}

impl Tab {
    /// Worst Agent Status of the Tab's Panes (Tab label dot, R10.3).
    pub fn status(&self) -> Option<AgentStatus> {
        worst(self.panes().into_iter().map(|p| p.status))
    }
}

impl Pane {
    /// `status_source` in `rade list` (API.md): `hook` or `heuristic`.
    pub fn status_source(&self) -> Option<&'static str> {
        self.status.map(|_| {
            if self.status_heuristic {
                "heuristic"
            } else {
                "hook"
            }
        })
    }
}

impl HookKind {
    /// Whether this kind of hook reports `s` (installed by `rade setup`).
    pub fn reports(self, s: AgentStatus) -> bool {
        use AgentStatus::*;
        match self {
            HookKind::Claude | HookKind::Opencode => matches!(s, Running | Waiting | Done | Error),
            HookKind::Codex => matches!(s, Running | Waiting | Done),
            HookKind::Agy => matches!(s, Running | Done),
            HookKind::None => false,
        }
    }
}

/// A Terminal event the R10.2 fallbacks act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    /// OSC 9 / 777 desktop notification or BEL.
    Attention,
    /// Screen unchanged for `idle_seconds`.
    Idle,
    /// OSC 133 command finished: the shell's foreground command ended.
    CommandFinished,
}

impl State {
    /// Worst Agent Status of a Source's Panes (sidebar dot, R1.6).
    pub fn source_status(&self, source: &str) -> Option<AgentStatus> {
        let p = self.source(source)?;
        worst(p.worktrees.iter().flat_map(|w| &w.tabs).map(Tab::status))
    }

    /// An Agent hook reported for `pane` (`pane.set_status`, `notify.post
    /// --status`), with `status` if it gave one.
    pub fn record_hook(&mut self, pane: &str, status: Option<AgentStatus>, now: u64) -> Result<()> {
        let loc = self.locate_or_err(pane)?;
        let p = self.tab_mut(&loc)?.pane_mut(pane).unwrap();
        p.hook_at = Some(now);
        if let Some(s) = status {
            p.hook_statuses |= 1 << s.rank();
        }
        Ok(())
    }

    /// Apply an R10.2 fallback rule to an Agent Pane; returns the status it
    /// set, if any (the transition raises its Notification as usual).
    /// Rules — only Agent Panes (a Preset), only from `running`, never
    /// within [`HOOK_GRACE_SECS`] of a hook report, and only for statuses the
    /// Preset's hooks can't report (a Preset whose hooks never reported
    /// counts as having none; a status a hook did report counts as
    /// reportable even if the Preset declares no such hook):
    /// - Attention (OSC 9/777, BEL) → `waiting` if no `waiting` hook; for a
    ///   Codex Preset → `done` if no `done` hook (its OSC 9 is the turn end).
    /// - Idle → `done` if no `done` hook, else `waiting` if no `waiting` hook
    ///   (a done hook would have fired: it is stuck on a question).
    /// - CommandFinished in a shell Pane (an Agent started by hand) →
    ///   `exited`, from any status.
    pub fn apply_signal(
        &mut self,
        pane: &str,
        signal: Signal,
        presets: &BTreeMap<String, Preset>,
        now: u64,
    ) -> Result<Option<AgentStatus>> {
        let p = self
            .pane(pane)
            .ok_or_else(|| crate::rpc::Error::not_found(format!("no pane {pane}")))?;
        let Some(agent) = &p.agent else {
            return Ok(None);
        };
        let target = if signal == Signal::CommandFinished {
            (p.command().is_none() && p.status != Some(AgentStatus::Exited))
                .then_some(AgentStatus::Exited)
        } else if p.status != Some(AgentStatus::Running)
            || p.hook_at
                .is_some_and(|t| now.saturating_sub(t) < HOOK_GRACE_SECS)
        {
            None
        } else {
            let declared = presets.get(agent).map_or(HookKind::None, |x| x.hooks);
            let hooks = match p.hook_at {
                Some(_) => declared,
                None => HookKind::None,
            };
            let seen = p.hook_statuses;
            let lacks = |s: AgentStatus| !hooks.reports(s) && seen & (1 << s.rank()) == 0;
            match signal {
                // Codex's only OSC 9 is its turn-end reply (research §6).
                Signal::Attention if declared == HookKind::Codex => {
                    lacks(AgentStatus::Done).then_some(AgentStatus::Done)
                }
                Signal::Attention => lacks(AgentStatus::Waiting).then_some(AgentStatus::Waiting),
                _ if lacks(AgentStatus::Done) => Some(AgentStatus::Done),
                _ => lacks(AgentStatus::Waiting).then_some(AgentStatus::Waiting),
            }
        };
        let Some(s) = target else { return Ok(None) };
        self.set_status(pane, Some(s), None, None, now)?;
        let loc = self.locate_or_err(pane)?;
        self.tab_mut(&loc)?.pane_mut(pane).unwrap().status_heuristic = true;
        Ok(Some(s))
    }

    /// A Codex Pane whose current turn has run [`CODEX_HINT_SECS`] with no
    /// hook report since it began: its hooks are likely not trusted yet
    /// (R16.4). Only turns RADE saw start (spawn with a prompt, `send
    /// --enter`) count.
    pub fn codex_untrusted(&self, presets: &BTreeMap<String, Preset>, now: u64) -> Option<String> {
        let mut panes = self
            .worktrees()
            .flat_map(|(_, w)| w.tabs.iter().flat_map(|t| t.panes()));
        panes
            .find(|p| {
                let codex = p
                    .agent
                    .as_deref()
                    .and_then(|a| presets.get(a))
                    .is_some_and(|x| x.hooks == HookKind::Codex);
                codex
                    && p.status == Some(AgentStatus::Running)
                    && p.status_since.is_some_and(|since| {
                        now.saturating_sub(since) >= CODEX_HINT_SECS
                            && p.hook_at.is_none_or(|t| t < since)
                    })
            })
            .map(|p| p.id.clone())
    }
}

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
