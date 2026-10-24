//! Basic actions (PRD R19, M13): which Tabs "Close Other Tabs" / "Close
//! Tabs to the Right" close, Duplicate Tab, Tab addressing for `rade tab`,
//! pausing and resuming a Source, and the Terminal search bar's count.

use crate::agents::AgentStatus;
use crate::fanout::Phase;
use crate::model::{State, GENERAL_ID};
use crate::rpc::{Error, ErrorKind, Result};

/// Which Tabs a close takes (`rade tab close [--others|--right]`, the Tab
/// menu).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseMode {
    /// Just this Tab.
    Only,
    /// Every other Tab of its Worktree.
    Others,
    /// The Tabs after it in its Worktree.
    Right,
}

/// The Tabs a close takes, and the Fan-out Tabs it leaves alone: closing
/// one of those means discarding its Fan-out, which is never done on the
/// side (R15).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CloseSet {
    pub close: Vec<String>,
    /// (tab, fan-out id).
    pub kept: Vec<(String, String)>,
}

/// (source, worktree, tab) ids of a Tab matched by name.
type Hit = (String, String, String);

/// An Agent still at work in Tabs about to close: (pane, name, status).
pub type Busy = (String, String, AgentStatus);

/// What a close from the UI asks first (R19.9): the dialog's heading, body
/// and its Close button. The CLI and the socket never ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClosePrompt {
    pub heading: String,
    pub body: String,
    pub action: String,
}

impl State {
    /// A Tab by id, or by its label or given name (a name resolves in the
    /// caller's Worktree, then its Source, then everywhere; several →
    /// `AMBIGUOUS`), like Pane names.
    pub fn resolve_tab(&self, query: &str, caller: Option<&str>) -> Result<String> {
        if self.locate_tab(query).is_some() {
            return Ok(query.into());
        }
        let here = caller.and_then(|c| self.locate(c));
        let hits: Vec<Hit> = self
            .worktrees()
            .flat_map(|(p, w)| {
                w.tabs
                    .iter()
                    .filter(|t| t.name == query || t.label() == query)
                    .map(move |t| (p.id.clone(), w.id.clone(), t.id.clone()))
            })
            .collect();
        let scopes: [&dyn Fn(&Hit) -> bool; 3] = [
            &|h| {
                here.as_ref()
                    .is_some_and(|l| l.source == h.0 && l.worktree == h.1)
            },
            &|h| here.as_ref().is_some_and(|l| l.source == h.0),
            &|_| true,
        ];
        for in_scope in scopes {
            let ids: Vec<&str> = hits
                .iter()
                .filter(|h| in_scope(h))
                .map(|h| h.2.as_str())
                .collect();
            match ids.len() {
                0 => continue,
                1 => return Ok(ids[0].into()),
                _ => {
                    return Err(Error::new(
                        ErrorKind::Ambiguous,
                        format!("'{query}' matches several tabs: {}", ids.join(", ")),
                    ))
                }
            }
        }
        Err(Error::not_found(format!("no tab '{query}'")))
    }

    /// The Tabs a close of `tab` takes (R19.1). Fan-out Tabs are kept out
    /// of Others / Right; closing one alone is refused (`USAGE`, pointing
    /// at `rade fanout discard`).
    pub fn tab_close_set(&self, tab: &str, mode: CloseMode) -> Result<CloseSet> {
        let (p, w) = self
            .locate_tab(tab)
            .ok_or_else(|| Error::not_found(format!("no tab '{tab}'")))?;
        let tabs = &self.worktree(&p, &w).unwrap().tabs;
        let at = tabs.iter().position(|t| t.id == tab).unwrap();
        let picked: Vec<&str> = match mode {
            CloseMode::Only => {
                if let Some(f) = self.fanout_of_tab(tab) {
                    return Err(Error::usage(format!(
                        "{tab} is the Tab of fan-out '{0}': closing it discards the fan-out \
                         (its Worktrees and branches); run `rade fanout discard {0}`",
                        f.id
                    )));
                }
                vec![tab]
            }
            CloseMode::Others => tabs
                .iter()
                .filter(|t| t.id != tab)
                .map(|t| t.id.as_str())
                .collect(),
            CloseMode::Right => tabs[at + 1..].iter().map(|t| t.id.as_str()).collect(),
        };
        let mut set = CloseSet::default();
        for t in picked {
            match self.fanout_of_tab(t) {
                Some(f) => set.kept.push((t.into(), f.id.clone())),
                None => set.close.push(t.into()),
            }
        }
        Ok(set)
    }

    /// Agents still `running` or `waiting` in these Tabs (the confirmation
    /// before closing several, R19.1).
    pub fn busy_agents(&self, tabs: &[String]) -> Vec<Busy> {
        self.worktrees()
            .flat_map(|(_, w)| &w.tabs)
            .filter(|t| tabs.contains(&t.id))
            .flat_map(|t| t.panes())
            .filter_map(|p| match p.status {
                Some(s @ (AgentStatus::Running | AgentStatus::Waiting)) => Some((
                    p.id.clone(),
                    p.name
                        .clone()
                        .or_else(|| p.agent.clone())
                        .unwrap_or_else(|| p.id.clone()),
                    s,
                )),
                _ => None,
            })
            .collect()
    }

    /// The question before closing Pane `pane` from the UI (R19.9); `None`
    /// for a Viewer, Review or Browser (nothing runs in it: it closes at
    /// once, like its own ✕ / Esc) or an unknown Pane.
    pub fn close_pane_prompt(&self, pane: &str) -> Option<ClosePrompt> {
        let p = self.pane(pane)?;
        if !matches!(p.kind, crate::PaneKind::Terminal { .. }) {
            return None;
        }
        let name = p.name.as_deref().or(p.agent_name());
        let body = match (p.status, name) {
            (Some(AgentStatus::Running | AgentStatus::Waiting), Some(n)) => {
                match p.agent_name().filter(|a| *a != n) {
                    Some(a) => format!("{n} ({a}) is still working and will be stopped."),
                    None => format!("{n} is still working and will be stopped."),
                }
            }
            _ => "Its processes will be stopped.".into(),
        };
        let what = match (name, p.shown_title()) {
            (Some(n), Some(t)) => format!("{n} — {t}"),
            (Some(n), None) => n.to_string(),
            (None, Some(t)) => format!("Terminal — {t}"),
            (None, None) => "Terminal".into(),
        };
        Some(ClosePrompt {
            heading: format!("Close Pane “{what}”?"),
            body,
            action: "Close Pane".into(),
        })
    }

    /// The question before closing the Tabs of `set` from the UI (R19.9):
    /// always asked; names the Agents still working and the Fan-out Tabs
    /// left open.
    pub fn close_tabs_prompt(&self, set: &CloseSet) -> ClosePrompt {
        let busy = self.busy_agents(&set.close);
        let labels: Vec<String> = self
            .worktrees()
            .flat_map(|(_, w)| &w.tabs)
            .filter(|t| set.close.contains(&t.id))
            .map(|t| t.label())
            .collect();
        let mut body = String::new();
        if labels.len() > 1 {
            for l in &labels {
                body += &format!("• {l}\n");
            }
            body += "\n";
        }
        if busy.is_empty() {
            body += if set.close.len() > 1 {
                "Their Panes and processes will be stopped."
            } else {
                "Its Panes and their processes will be stopped."
            };
        } else {
            body += "These Agents are still working and will be stopped:\n";
            for (_, name, status) in &busy {
                body += &format!("\n• {name} — {}", status.as_str());
            }
        }
        if !set.kept.is_empty() {
            let ids: Vec<&str> = set.kept.iter().map(|k| k.1.as_str()).collect();
            body += &format!(
                "\n\nFan-out Tab {} stays open (discard a Fan-out from its bar).",
                ids.join(", ")
            );
        }
        let one = set.close.len() == 1;
        let heading = match self
            .worktrees()
            .flat_map(|(_, w)| &w.tabs)
            .find(|t| one && t.id == set.close[0])
        {
            Some(t) => format!("Close Tab “{}”?", t.label()),
            None => format!("Close {} Tabs?", set.close.len()),
        };
        ClosePrompt {
            heading,
            body,
            action: if one { "Close Tab" } else { "Close Tabs" }.into(),
        }
    }

    /// Duplicate Tab (R19.1): a new Tab right after `tab`, in its Worktree,
    /// with a shell whose cwd is the source's focused Terminal's (the
    /// Worktree root for any other Pane, or an Agent running in another
    /// Worktree). Agents are not copied: a second process on one session
    /// would fork it. Selected; returns (tab, pane).
    pub fn duplicate_tab(&mut self, tab: &str) -> Result<(String, String)> {
        let (p, w) = self
            .locate_tab(tab)
            .ok_or_else(|| Error::not_found(format!("no tab '{tab}'")))?;
        let wt = self.worktree(&p, &w).unwrap();
        let src = wt.tab(tab).unwrap();
        let cwd = src
            .pane(&src.focused)
            .filter(|x| x.runs_in.is_none())
            .filter(|x| matches!(x.kind, crate::PaneKind::Terminal { .. }))
            .map_or_else(|| wt.path.clone(), |x| x.cwd().clone());
        let at = wt.tabs.iter().position(|t| t.id == tab).unwrap();
        let pane = self.new_pane(&cwd, None, None);
        let (new, pane) = self.new_tab_with(&p, &w, pane)?;
        self.move_tab(&new, at + 1)?;
        self.select_tab(&new)?;
        Ok((new, pane))
    }

    pub fn is_paused(&self, source: &str) -> bool {
        self.source(source).is_some_and(|p| p.paused)
    }

    /// A paused Source takes no new Panes (spawn, split, Tab, Fan-out…).
    pub fn check_not_paused(&self, source: &str) -> Result<()> {
        match self.source(source).filter(|p| p.paused) {
            Some(p) => Err(Error::usage(format!(
                "source '{}' is paused: resume it first (`rade source resume {}`)",
                p.name, p.id
            ))),
            None => Ok(()),
        }
    }

    /// Pause a Source (R19.3): its Panes stay (Tabs, Layouts, names) but
    /// become what Session Restore makes of them — each gets the command it
    /// restarts with, an Agent that was mid-turn comes back `interrupted` —
    /// and nothing runs until [`Self::resume_source`]. The app then stops
    /// every process in it. Refused for General, and while one of its
    /// Fan-outs is at work unless `force` (those Agents then come back
    /// interrupted, with the Fan-out bar's Continue). Pausing a paused
    /// Source does nothing. Returns the Source id.
    pub fn pause_source(
        &mut self,
        query: &str,
        presets: &crate::agents::PresetsFor<'_>,
        force: bool,
        now: u64,
    ) -> Result<String> {
        let id = self.find_source(query)?.id.clone();
        if id == GENERAL_ID {
            return Err(Error::usage("General can't be paused"));
        }
        if self.is_paused(&id) {
            return Ok(id);
        }
        if !force {
            if let Some(f) = self
                .fanouts
                .iter()
                .find(|f| f.source == id && matches!(f.phase(), Phase::Running | Phase::Judging))
            {
                return Err(Error::usage(format!(
                    "fan-out '{0}' is at work in source '{1}': pausing cuts its Agents' turns \
                     (they come back interrupted). Pause anyway with \
                     `rade source pause {1} --force`",
                    f.id, id
                )));
            }
        }
        let panes: Vec<String> = self
            .source(&id)
            .unwrap()
            .worktrees
            .iter()
            .flat_map(|w| w.tabs.iter().flat_map(|t| t.pane_ids()))
            .collect();
        self.restore_panes(&panes, presets, now);
        self.restore_fanouts(Some(&id));
        self.source_mut(&id).unwrap().paused = true;
        Ok(id)
    }

    /// Resume a paused Source: its Panes start again with the commands
    /// pausing gave them (Agents resume their sessions, never replaying a
    /// prompt, R11.2). Resuming a running Source does nothing.
    pub fn resume_source(&mut self, query: &str) -> Result<String> {
        let id = self.find_source(query)?.id.clone();
        self.source_mut(&id).unwrap().paused = false;
        Ok(id)
    }
}

/// The search bar's match count, Ghostty's way: `selected/total` with the
/// selected match 1-based (0 when none is selected); empty before the first
/// count arrives. Ghostty sends -1 for "none".
pub fn search_count(total: isize, selected: isize) -> String {
    if total < 0 {
        return String::new();
    }
    let sel = if selected >= 0 { selected + 1 } else { 0 };
    format!("{sel}/{total}")
}

#[cfg(test)]
#[path = "basic_tests.rs"]
mod tests;
