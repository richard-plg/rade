//! Staged Session Restore (R130, ADR-0050): the order in which restored
//! Terminals start, and the pace between Agents, so a launch with many
//! Agent Panes never blocks the window. GTK-free; the app drains it.

use std::collections::VecDeque;

use crate::layout::PaneKind;
use crate::model::State;

/// Wait after starting an Agent before the next Terminal (R130.3): spreads
/// the CPU and memory spike of many agents resuming at once.
pub const AGENT_GAP_MS: u64 = 300;

/// The restored Terminal Panes in start order (R130.2): the focused Pane,
/// the rest of the Tab on screen, the shown Worktree's other Tabs, the
/// shown Workspace (the active Source first), then the other Workspaces in
/// their order. Panes of paused Sources run nothing (R19.3) and are left out.
pub fn order(state: &State) -> Vec<String> {
    let (active_src, active_wt) = state.active();
    let ws = state.active_workspace().to_string();
    let rank_ws = |w: &str| {
        if w == ws {
            0
        } else {
            1 + state
                .workspaces
                .iter()
                .position(|x| x.id == w)
                .unwrap_or(state.workspaces.len())
        }
    };
    // The shown Workspace (its active Source first), then the others.
    let mut sources: Vec<&crate::model::Source> =
        state.sources.iter().filter(|s| !s.paused).collect();
    sources.sort_by_key(|s| (rank_ws(&s.workspace), s.id != active_src.id));

    let mut out: Vec<String> = Vec::new();
    let mut push = |kind: &PaneKind, id: &str| {
        if matches!(kind, PaneKind::Terminal { .. }) && !out.iter().any(|x| x == id) {
            out.push(id.to_string());
        }
    };
    if !active_src.paused {
        if let Some(tab) = state.active_tab() {
            if let Some(p) = tab.pane(&tab.focused) {
                push(&p.kind, &p.id);
            }
            for p in tab.panes() {
                push(&p.kind, &p.id);
            }
        }
        for tab in &active_wt.tabs {
            for p in tab.panes() {
                push(&p.kind, &p.id);
            }
        }
    }
    for s in sources {
        for w in &s.worktrees {
            for tab in &w.tabs {
                for p in tab.panes() {
                    push(&p.kind, &p.id);
                }
            }
        }
    }
    out
}

/// What the app does next with the queue.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Start this Pane's Terminal now.
    Start(String),
    /// Nothing before this many ms.
    Wait(u64),
    Done,
}

/// The Panes still to start, front first, and when the next may start.
#[derive(Debug, Default)]
pub struct Queue {
    pending: VecDeque<String>,
    next_at: u64,
}

impl Queue {
    pub fn new(order: Vec<String>) -> Self {
        Self {
            pending: order.into(),
            next_at: 0,
        }
    }

    pub fn contains(&self, pane: &str) -> bool {
        self.pending.iter().any(|p| p == pane)
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn pending(&self) -> impl Iterator<Item = &str> {
        self.pending.iter().map(String::as_str)
    }

    /// Take `pane` out of the queue: it starts now, out of turn (R130.4:
    /// shown, or a socket call needs it), or it was closed. True when it
    /// was waiting.
    pub fn promote(&mut self, pane: &str) -> bool {
        let before = self.pending.len();
        self.pending.retain(|p| p != pane);
        self.pending.len() != before
    }

    /// The next step at `now` (ms). `is_agent` says whether a Pane runs an
    /// Agent: only Agents are followed by [`AGENT_GAP_MS`].
    pub fn next(&mut self, now: u64, is_agent: impl Fn(&str) -> bool) -> Step {
        if self.pending.is_empty() {
            return Step::Done;
        }
        if now < self.next_at {
            return Step::Wait(self.next_at - now);
        }
        let pane = self.pending.pop_front().expect("not empty");
        if is_agent(&pane) {
            self.next_at = now + AGENT_GAP_MS;
        }
        Step::Start(pane)
    }
}

#[cfg(test)]
#[path = "restore_queue_tests.rs"]
mod tests;
