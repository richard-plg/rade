//! Workspaces (PRD R23, ADR-0015): the registry, `<root>/.rade/workspace.toml`,
//! env per Pane and where new Worktrees go (ADR-0014). The model is flat:
//! every Source names its Workspace; the one shown is the active Source's.

use crate::agents::AgentStatus;
use crate::model::{general_id, slug, Source, State, Workspace, DEFAULT_WORKSPACE};
use crate::paths::Flavor;
use crate::rpc::{Error, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// `<root>/.rade/workspace.toml` (R23.2, R25.1). Unknown keys are ignored.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct WorkspaceFile {
    pub name: Option<String>,
    pub color: Option<String>,
    pub env: BTreeMap<String, String>,
    pub clone: CloneConfig,
    pub identity: Identity,
    /// `[[account]]` tables (R108.1), validated by
    /// [`crate::account::accounts`].
    pub account: Vec<crate::account::AccountDecl>,
}

/// `[identity]` (R25.1): who the Workspace is on git and GitHub.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Identity {
    pub git_name: Option<String>,
    pub git_email: Option<String>,
    /// The expected GitHub login (`gh api user`).
    pub github: Option<String>,
    /// A file name in `~/.ssh`.
    pub ssh_key: Option<String>,
    pub sign: Option<Sign>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Sign {
    Ssh,
    None,
}

impl Identity {
    /// Commits are signed with the SSH key (`sign = "ssh"`).
    pub fn signs(&self) -> bool {
        self.sign == Some(Sign::Ssh)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct CloneConfig {
    /// `rade source add repo` clones `<default_owner>/repo` (R24.2).
    pub default_owner: Option<String>,
}

impl WorkspaceFile {
    pub fn parse(text: &str) -> std::result::Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// The file under `root`; none → empty. Read each time it is needed
    /// (no watcher).
    pub fn read(root: &Path) -> std::result::Result<Self, String> {
        let path = root.join(".rade/workspace.toml");
        match std::fs::read_to_string(&path) {
            Ok(t) => Self::parse(&t).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// `[env]` with a leading `~` expanded; `RADE_*` dropped: RADE's own
    /// variables can't be overridden (R23.3).
    pub fn env(&self, home: &Path) -> Vec<(String, String)> {
        self.env
            .iter()
            .filter(|(k, _)| !k.starts_with("RADE_"))
            .map(|(k, v)| (k.clone(), expand_tilde(v, home)))
            .collect()
    }
}

pub(crate) fn expand_tilde(v: &str, home: &Path) -> String {
    if v == "~" {
        home.display().to_string()
    } else if let Some(rest) = v.strip_prefix("~/") {
        home.join(rest).display().to_string()
    } else {
        v.to_string()
    }
}

/// Named colors (libadwaita's palette) for `color = "…"`.
pub const PALETTE: [(&str, &str); 8] = [
    ("blue", "#3584e4"),
    ("green", "#2ec27e"),
    ("yellow", "#e5a50a"),
    ("orange", "#ff7800"),
    ("purple", "#9141ac"),
    ("red", "#e01b24"),
    ("brown", "#986a44"),
    ("gray", "#77767b"),
];

/// A Workspace's color as `#rrggbb`: a palette name or a hex color; else
/// gray for `default` (index 0) and the palette in turn for the others.
pub fn color_hex(color: Option<&str>, index: usize) -> String {
    let c = color.map(|c| c.trim().to_ascii_lowercase());
    if let Some(c) = &c {
        if let Some((_, hex)) = PALETTE.iter().find(|(n, _)| n == c) {
            return hex.to_string();
        }
        if c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit()) {
            return c.clone();
        }
    }
    // Not yellow / orange by default: amber is `waiting` (R20.3).
    const AUTO: [usize; 5] = [0, 1, 4, 5, 6];
    if index == 0 {
        PALETTE[7].1.into()
    } else {
        PALETTE[AUTO[(index - 1) % AUTO.len()]].1.into()
    }
}

/// How a Workspace is shown (switcher, Toasts, desktop notifications).
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub id: String,
    /// `workspace.toml`'s `name`, else the one given at add.
    pub name: String,
    /// `#rrggbb`
    pub color: String,
    /// `Ctrl+Alt+<n>` for the first nine (R23.5).
    pub key: Option<String>,
}

/// The name a new Workspace at `root` gets: `given`, else its
/// `workspace.toml`'s `name`, else the folder's name (R23.1).
pub fn default_name(root: &Path, given: Option<&str>) -> String {
    given
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(String::from)
        .or_else(|| WorkspaceFile::read(root).ok().and_then(|f| f.name))
        .or_else(|| root.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "workspace".into())
}

/// Counts for the switcher and `workspace.list` (R23.5).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    /// Sources, Generals excluded.
    pub sources: usize,
    pub unread: u32,
    /// Agents `waiting`.
    pub waiting: usize,
    /// Worst Agent Status.
    pub status: Option<AgentStatus>,
}

impl State {
    pub fn workspace(&self, id: &str) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    /// By id, else by name.
    pub fn find_workspace(&self, query: &str) -> Result<&Workspace> {
        self.workspace(query)
            .or_else(|| self.workspaces.iter().find(|w| w.name == query))
            .ok_or_else(|| Error::not_found(format!("no workspace '{query}'")))
    }

    /// Name, color and key of every Workspace, in switcher order; reads
    /// each `workspace.toml` (R23.2).
    pub fn workspace_looks(&self) -> Vec<Look> {
        self.workspaces
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let f = WorkspaceFile::read(&w.root).unwrap_or_default();
                Look {
                    id: w.id.clone(),
                    name: f.name.unwrap_or_else(|| w.name.clone()),
                    color: color_hex(f.color.as_deref(), i),
                    key: (i < 9).then(|| format!("Ctrl+Alt+{}", i + 1)),
                }
            })
            .collect()
    }

    /// The shown name of a Pane's Workspace when it is hidden (Toasts and
    /// desktop notifications name it, R23.6); `None` when it is shown.
    pub fn hidden_workspace_name(&self, pane: &str) -> Option<String> {
        if self.pane_visible(pane) {
            return None;
        }
        let ws = self.source(&self.locate(pane)?.source)?.workspace.clone();
        self.workspace_looks()
            .into_iter()
            .find(|l| l.id == ws)
            .map(|l| l.name)
    }

    /// The Workspace a new Source at `path` joins (R24.2): `explicit`, else
    /// the one whose root holds it, else `caller`'s, else the one shown.
    pub fn workspace_for_new_source(
        &self,
        path: &Path,
        explicit: Option<&str>,
        caller: Option<&str>,
    ) -> Result<String> {
        if let Some(q) = explicit {
            return Ok(self.find_workspace(q)?.id.clone());
        }
        let under_root = self
            .workspaces
            .iter()
            .any(|w| w.id != DEFAULT_WORKSPACE && path.starts_with(&w.root));
        let caller_ws = caller
            .and_then(|c| self.locate(c))
            .and_then(|l| self.source(&l.source))
            .map(|s| s.workspace.clone());
        Ok(match caller_ws {
            Some(w) if !under_root => w,
            _ => self.workspace_for_path(path),
        })
    }

    /// The Workspace a Source is in.
    pub fn source_workspace(&self, source: &str) -> Option<&Workspace> {
        self.workspace(&self.source(source)?.workspace)
    }

    /// Register `root` as a Workspace named `name` (R23.1): moves every
    /// Source under it (from a less specific Workspace) into it and adds
    /// its General. Returns its id and the moved Sources.
    pub fn add_workspace(&mut self, root: &Path, name: &str) -> Result<(String, Vec<String>)> {
        if !root.is_absolute() {
            return Err(Error::usage(format!(
                "workspace root must be absolute: {}",
                root.display()
            )));
        }
        if let Some(w) = self.workspaces.iter().find(|w| w.root == root) {
            return Err(Error::usage(format!(
                "{} is already workspace '{}'",
                root.display(),
                w.id
            )));
        }
        let base = slug(name);
        let mut id = base.clone();
        let mut n = 1;
        while self.workspace(&id).is_some() {
            n += 1;
            id = format!("{base}-{n}");
        }
        self.workspaces.push(Workspace {
            id: id.clone(),
            name: name.trim().into(),
            root: root.into(),
            active_source: None,
            link_checkouts: Vec::new(),
            project_order: Vec::new(),
        });
        let roots: BTreeMap<String, PathBuf> = self
            .workspaces
            .iter()
            .map(|w| (w.id.clone(), w.root.clone()))
            .collect();
        let mut moved = Vec::new();
        for s in self.sources.iter_mut().filter(|s| !s.is_general()) {
            let inside_new = s.path.starts_with(root);
            // A Source in a Workspace nested inside the new root stays.
            let more_specific = roots
                .get(&s.workspace)
                .is_some_and(|r| r.starts_with(root) && s.workspace != DEFAULT_WORKSPACE);
            if inside_new && !more_specific {
                s.workspace = id.clone();
                moved.push(s.id.clone());
            }
        }
        let home = self.workspaces[0].root.clone();
        self.ensure_general(&home);
        Ok((id, moved))
    }

    /// Unregister a Workspace (R23.1): not `default`, not while it has
    /// Sources besides its General. Returns its id and its General (the
    /// caller closes its Panes). Files are untouched.
    pub fn remove_workspace(&mut self, query: &str) -> Result<(String, Source)> {
        let id = self.find_workspace(query)?.id.clone();
        if id == DEFAULT_WORKSPACE {
            return Err(Error::usage("the default workspace can't be removed"));
        }
        let left: Vec<&str> = self
            .sources
            .iter()
            .filter(|s| s.workspace == id && !s.is_general())
            .map(|s| s.id.as_str())
            .collect();
        if !left.is_empty() {
            return Err(Error::usage(format!(
                "workspace '{id}' still has sources: {} (remove them first: `rade source remove <id>`)",
                left.join(", ")
            )));
        }
        let shown = self.active_workspace() == id;
        self.workspaces.retain(|w| w.id != id);
        let g = general_id(&id);
        let i = self.sources.iter().position(|s| s.id == g).unwrap();
        let general = self.sources.remove(i);
        if shown {
            self.switch_workspace(DEFAULT_WORKSPACE)?;
        }
        Ok((id, general))
    }

    /// Show a Workspace (R23.5): the Source it showed last (its Worktree,
    /// Tab and focus are kept per Source), else its General.
    pub fn switch_workspace(&mut self, query: &str) -> Result<String> {
        let ws = self.find_workspace(query)?;
        let source = ws
            .active_source
            .clone()
            .filter(|s| self.source(s).is_some_and(|x| x.workspace == ws.id))
            .unwrap_or_else(|| general_id(&ws.id));
        self.select_source(&source)?;
        Ok(source)
    }

    /// The shown Workspace's Sources, in sidebar order (R23.4).
    pub fn visible_sources(&self) -> impl Iterator<Item = &Source> {
        let ws = self.active_workspace().to_string();
        self.sources.iter().filter(move |s| s.workspace == ws)
    }

    /// Whether a Pane's Source is in the shown Workspace.
    pub fn pane_visible(&self, pane: &str) -> bool {
        self.locate(pane)
            .and_then(|l| self.source(&l.source))
            .is_some_and(|s| s.workspace == self.active_workspace())
    }

    pub fn workspace_summary(&self, ws: &str) -> Summary {
        let mut sum = Summary::default();
        let mut statuses = Vec::new();
        for (s, w) in self.worktrees().filter(|(s, _)| s.workspace == ws) {
            if s.active_worktree == w.id && !s.is_general() {
                sum.sources += 1;
            }
            for p in w.tabs.iter().flat_map(|t| t.panes()) {
                sum.unread += self.unread(&p.id);
                if p.status == Some(AgentStatus::Waiting) {
                    sum.waiting += 1;
                }
                statuses.push(p.status);
            }
        }
        sum.status = crate::status::worst(statuses);
        sum
    }

    /// Agents `waiting` in a hidden Workspace: the switcher's `!` (R23.5).
    pub fn hidden_waiting(&self) -> Vec<String> {
        self.waiting_panes()
            .into_iter()
            .filter(|p| !self.pane_visible(p))
            .collect()
    }

    /// Env a new Terminal of `source` gets before RADE's own variables
    /// (R23.3): its Workspace's `[env]`, then `RADE_WORKSPACE`. A broken
    /// `workspace.toml` is an error (the caller reports it and goes on
    /// without `[env]`).
    pub fn env_for(
        &self,
        source: &str,
        home: &Path,
    ) -> std::result::Result<Vec<(String, String)>, String> {
        let ws = self
            .source_workspace(source)
            .ok_or_else(|| format!("no source '{source}'"))?;
        let mut env = WorkspaceFile::read(&ws.root)?.env(home);
        env.push(("RADE_WORKSPACE".into(), ws.id.clone()));
        Ok(env)
    }

    /// The Worktrees folder of Workspace `ws` (R23.8, ADR-0014):
    /// `<root>/.worktrees[-dev]`, or `<data_dir>/worktrees` for `default`.
    pub fn worktrees_folder(&self, ws: &str, data_dir: &Path, flavor: Flavor) -> PathBuf {
        match self.workspace(ws) {
            Some(w) if w.id != DEFAULT_WORKSPACE => w.root.join(match flavor {
                Flavor::Installed => ".worktrees",
                Flavor::Devel => ".worktrees-dev",
            }),
            _ => data_dir.join("worktrees"),
        }
    }

    /// Where a new Worktree `name` of `source` goes (R23.8, ADR-0014):
    /// `<root>/.worktrees[-dev]/<source>/<name>`, or for the `default`
    /// Workspace `<data_dir>/worktrees/<source>/<name>`.
    pub fn worktree_dir(
        &self,
        source: &str,
        name: &str,
        data_dir: &Path,
        flavor: Flavor,
    ) -> PathBuf {
        let ws = self
            .source_workspace(source)
            .map_or(DEFAULT_WORKSPACE.to_string(), |w| w.id.clone());
        self.worktrees_folder(&ws, data_dir, flavor)
            .join(source)
            .join(name)
    }

    /// Where the Project checkout of `project`'s link to `source` goes
    /// (R98.1): `<Worktrees folder>/.projects/<project>/<source>`.
    pub fn project_checkout_dir(
        &self,
        ws: &str,
        project: &str,
        source: &str,
        data_dir: &Path,
        flavor: Flavor,
    ) -> PathBuf {
        crate::checkout::project_checkout_dir(
            &self.worktrees_folder(ws, data_dir, flavor),
            project,
            source,
        )
    }

    /// The ref Review, Changes, the `+/−` counts and Create PR compare the
    /// checkout at `root` with (R99.2): a Worktree's recorded base, else
    /// its link's base; a link's recorded checkout, its link's base;
    /// `None` = the Source's main branch. Reads the Workspace's
    /// `project.toml` files.
    pub fn comparison_base(&self, root: &Path) -> Option<String> {
        let found = self
            .worktrees()
            .find(|(_, w)| crate::git::same(&w.path, root));
        if let Some((s, w)) = found {
            if w.base.is_some() {
                return w.base.clone();
            }
            let link_base = w.project.as_ref().and_then(|pid| {
                let ws = self.workspace(&s.workspace)?;
                crate::project::list(&ws.root)
                    .into_iter()
                    .find(|p| &p.id == pid)?
                    .source(&s.id)?
                    .base
                    .clone()
            });
            if link_base.is_some() {
                return crate::checkout::comparison_base(None, link_base.as_deref());
            }
        }
        self.workspaces
            .iter()
            .flat_map(|w| &w.link_checkouts)
            .find(|r| crate::git::same(&r.path, root))
            .map(|r| r.base.clone())
    }
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
