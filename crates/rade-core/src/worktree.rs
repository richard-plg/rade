//! Worktrees in the model (PRD R12, R1.2): add, find, select, remove,
//! import the ones git already has, and the per-Source `setup` command that
//! runs once in a new Worktree's first Terminal. Git itself runs in the app
//! through [`crate::git`]; these operations only change the model.

use crate::agents::AgentStatus;
use crate::git::GitWorktree;
use crate::layout::Pane;
use crate::model::{State, Tab, Worktree};
use crate::rpc::{Error, Result};
use std::path::Path;

/// Printed once a program Terminal's `setup` finished (the typed-prompt
/// timer waits for it).
pub const SETUP_DONE: &str = "rade setup done";

/// How a Terminal runs the Worktree's `setup` first (R12.2): returns its
/// (startup command, initial input). A shell Terminal gets the setup typed
/// into the user's own shell (keeps Ghostty's shell integration: cwd, OSC
/// 133). A program (Agent, `--cmd`) runs behind a wrapper: print the setup,
/// run it in its own `sh` (its `exit` can't end the Terminal), say how it
/// ended ([`SETUP_DONE`] or a failure), then the program — started either way.
pub fn setup_launch(setup: &str, command: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(command) = command else {
        return (None, Some(format!("{setup}\n")));
    };
    let wrapped = format!(
        "printf '\\033[1mrade setup: %s\\033[0m\\n' {q}; sh -c {q}; s=$?; \
         if [ $s -eq 0 ]; then printf '\\033[2m{SETUP_DONE}\\033[0m\\n'; \
         else printf '\\033[31mrade setup failed (exit %s)\\033[0m\\n' $s; fi\n{command}",
        q = crate::preset::shell_quote(setup)
    );
    (Some(wrapped), None)
}

/// The base a new Worktree records (R99.1): in a Project, the base it was
/// given or its link's, else the Source's main branch `main` it was cut
/// from — never nothing, so a later `rade project link set-base` cannot
/// move its comparison base (R99.2). Outside a Project, `--base` as given.
pub fn recorded_base(
    in_project: bool,
    resolved: Option<&str>,
    main: Option<&str>,
) -> Option<String> {
    match resolved {
        Some(b) => Some(b.to_string()),
        None if in_project => main.map(String::from),
        None => None,
    }
}

impl State {
    /// Register a Worktree `name` at `path` in a Source (by id or name);
    /// `setup` is the Source's setup command to run in its first Terminal.
    /// Returns (source id, worktree id).
    pub fn add_worktree(
        &mut self,
        source: &str,
        name: &str,
        path: &Path,
        setup: Option<String>,
    ) -> Result<(String, String)> {
        let p = self.find_source(source)?;
        if p.is_general() {
            return Err(Error::usage("General has no Worktrees"));
        }
        // `git worktree list` imported it while `git worktree add` was
        // running (R12.4 vs R12.1, M13 race): it is this one; keep it.
        if let Some(w) = p.worktrees.iter().find(|w| crate::git::same(&w.path, path)) {
            let (pid, id) = (p.id.clone(), w.id.clone());
            let w = self.worktree_mut(&pid, &id)?;
            if w.setup.is_none() {
                w.setup = setup.filter(|s| !s.trim().is_empty());
            }
            return Ok((pid, id));
        }
        if p.worktrees.iter().any(|w| w.name == name) {
            return Err(Error::usage(format!(
                "source '{}' already has a worktree '{name}'",
                p.name
            )));
        }
        let pid = p.id.clone();
        let id = self.next("w");
        let wt = Worktree {
            id: id.clone(),
            name: name.into(),
            path: path.into(),
            tabs: Vec::new(),
            active_tab: None,
            setup: setup.filter(|s| !s.trim().is_empty()),
            comments: Vec::new(),
            review: Default::default(),
            project: None,
            base: None,
        };
        self.sources
            .iter_mut()
            .find(|p| p.id == pid)
            .unwrap()
            .worktrees
            .push(wt);
        Ok((pid, id))
    }

    /// Record the Project and base a Worktree was made for (R28.2).
    pub fn set_worktree_origin(
        &mut self,
        source: &str,
        worktree: &str,
        project: Option<String>,
        base: Option<String>,
    ) -> Result<()> {
        let w = self.worktree_mut(source, worktree)?;
        w.project = project;
        w.base = base;
        Ok(())
    }

    /// Move a Worktree to a Project's sidebar group, or to none (R30.4).
    pub fn set_worktree_project(
        &mut self,
        source: &str,
        worktree: &str,
        project: Option<String>,
    ) -> Result<()> {
        self.worktree_mut(source, worktree)?.project = project;
        Ok(())
    }

    /// (source id, worktree id) of a Worktree by id or name; `worktree`
    /// `None` = the Source's active one.
    pub fn find_worktree(&self, source: &str, worktree: Option<&str>) -> Result<(String, String)> {
        let p = self.find_source(source)?;
        let w = match worktree {
            None => p.worktree(&p.active_worktree),
            Some(q) => p
                .worktrees
                .iter()
                .find(|w| w.id == q)
                .or_else(|| p.worktrees.iter().find(|w| w.name == q)),
        };
        w.map(|w| (p.id.clone(), w.id.clone())).ok_or_else(|| {
            Error::not_found(format!(
                "no worktree '{}' in source '{}'",
                worktree.unwrap_or("?"),
                p.name
            ))
        })
    }

    /// Show a Worktree (sidebar row, R1.1): its Source and its Tabs.
    pub fn select_worktree(&mut self, source: &str, worktree: &str) -> Result<()> {
        let (p, w) = self.find_worktree(source, Some(worktree))?;
        self.worktree_mut(&p, &w)?;
        self.sources
            .iter_mut()
            .find(|x| x.id == p)
            .unwrap()
            .active_worktree = w;
        self.set_active_source(p);
        Ok(())
    }

    /// Take a Worktree out of the model (after `git worktree remove`, or a
    /// prune): its Panes go with it. The main checkout can't be removed.
    pub fn remove_worktree(&mut self, source: &str, worktree: &str) -> Result<Worktree> {
        let (p, w) = self.find_worktree(source, Some(worktree))?;
        let proj = self.sources.iter_mut().find(|x| x.id == p).unwrap();
        let i = proj.worktrees.iter().position(|x| x.id == w).unwrap();
        if i == 0 {
            return Err(Error::usage("the main worktree can't be removed"));
        }
        let removed = proj.worktrees.remove(i);
        if proj.active_worktree == w {
            proj.active_worktree = proj.worktrees[0].id.clone();
        }
        Ok(removed)
    }

    /// Add the Worktrees git lists for a Source that the model lacks
    /// (created outside RADE); skips bare, prunable (folder gone) and known
    /// ones (`same` compares folders). Named after the branch, else the
    /// folder, made unique. Returns the new names.
    pub fn import_worktrees(
        &mut self,
        source: &str,
        listed: &[GitWorktree],
        same: &dyn Fn(&Path, &Path) -> bool,
    ) -> Vec<String> {
        let mut added = Vec::new();
        for g in listed.iter().filter(|g| !g.bare && !g.prunable) {
            let Ok(p) = self.find_source(source) else {
                break;
            };
            if p.is_general()
                || same(&g.path, &p.path)
                || p.worktrees.iter().any(|w| same(&w.path, &g.path))
            {
                continue;
            }
            let base = g
                .branch
                .clone()
                .or_else(|| Some(g.path.file_name()?.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "worktree".into());
            // `new` can't be a Worktree name (`spawn --worktree new`).
            let taken = |n: &str| n == "new" || p.worktrees.iter().any(|w| w.name == n);
            let name = (1..)
                .map(|i| {
                    if i == 1 {
                        base.clone()
                    } else {
                        format!("{base}-{i}")
                    }
                })
                .find(|n| !taken(n))
                .unwrap();
            if self.add_worktree(source, &name, &g.path, None).is_ok() {
                added.push(name);
            }
        }
        added
    }

    /// A new Terminal in this Worktree takes its pending `setup` (R12.2):
    /// only the first one runs it.
    pub(crate) fn take_setup(&mut self, source: &str, worktree: &str, pane: &mut Pane) {
        if let Ok(w) = self.worktree_mut(source, worktree) {
            pane.setup = w.setup.take();
        }
    }

    /// (source, worktree) a Pane runs in: its Tab's, or its `runs_in`
    /// Worktree (a Fan-out Agent, R15.2).
    pub fn pane_worktree(&self, id: &str) -> Option<(String, String)> {
        let loc = self.locate(id)?;
        let runs_in = self.tab(&loc)?.pane(id)?.runs_in.clone();
        let w = runs_in
            .filter(|w| self.worktree(&loc.source, w).is_some())
            .unwrap_or(loc.worktree);
        Some((loc.source, w))
    }

    /// Panes that run in a Worktree: its Tabs' own, plus Fan-out Agents
    /// shown elsewhere that run in it (`runs_in`, R15.2).
    pub fn panes_in(&self, source: &str, worktree: &str) -> Vec<&Pane> {
        self.source(source)
            .into_iter()
            .flat_map(|p| &p.worktrees)
            .flat_map(|w| w.tabs.iter().flat_map(Tab::panes).map(move |x| (w, x)))
            .filter(|(w, x)| x.runs_in.as_deref().unwrap_or(&w.id) == worktree)
            .map(|(_, x)| x)
            .collect()
    }

    /// Worst Agent Status of the Panes running in a Worktree (sidebar row,
    /// R10.3).
    pub fn worktree_status(&self, source: &str, worktree: &str) -> Option<AgentStatus> {
        crate::status::worst(
            self.panes_in(source, worktree)
                .into_iter()
                .map(|p| p.status),
        )
    }

    /// Whether this is the Source's main checkout (its first Worktree).
    pub fn is_main_worktree(&self, source: &str, worktree: &str) -> bool {
        self.source(source)
            .and_then(|p| p.worktrees.first())
            .is_some_and(|w| w.id == worktree)
    }
}

#[cfg(test)]
#[path = "worktree_tests.rs"]
mod tests;
