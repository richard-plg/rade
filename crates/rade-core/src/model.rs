//! Domain model: Sources → Worktrees → Tabs → Layout of Panes (CONTEXT.md, PRD R1).
//! Pure data and operations; the GTK app renders it, `state.json` stores it.

use crate::layout::{Direction, Layout, Pane, PaneKind, SplitDir};
use crate::notify::Notification;
use crate::rpc::{Error, ErrorKind, PaneInfo, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 2 = M15: Workspaces, Projects renamed Sources (R24.4); 3 = 1.0.0: the
/// removed engines' fields dropped (R101.3, [`crate::store`]).
pub const STATE_VERSION: u32 = 3;
/// The `default` Workspace's General; another Workspace's is
/// `general@<ws>` ([`general_id`]).
pub const GENERAL_ID: &str = "general";
/// The Workspace that always exists, rooted at `$HOME` (R23.1).
pub const DEFAULT_WORKSPACE: &str = "default";
pub const MAIN_WORKTREE: &str = "main";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub version: u32,
    /// Workspaces, `default` first (R23.1). A Source names its own.
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    /// Registered Sources; General is always present and last.
    #[serde(alias = "projects")] // v1 (R24.4)
    pub sources: Vec<Source>,
    #[serde(alias = "active_project")]
    pub active_source: String,
    #[serde(default)]
    next_id: u64,
    /// Newest last (R3).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notifications: Vec<Notification>,
    /// The sidebar's Notification panel is collapsed (R3.2).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub notifications_collapsed: bool,
    /// The right sidebar file tree is shown (R8.1).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub file_tree: bool,
    /// Command palette: recently used items, newest first (R14.2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub palette_recent: Vec<String>,
    /// Fan-outs not yet discarded (R15).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fanouts: Vec<crate::fanout::Fanout>,
    /// Folded Project headers in the sidebar, `<workspace>/<project>` (R30.1).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collapsed_projects: Vec<String>,
    /// Source id → the `.rade.toml` content `rade config trust` accepted
    /// (R29.4, ADR-0017): any other content is untrusted.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub trusted_configs: std::collections::BTreeMap<String, String>,
    /// Which sidebar membership rule this state was frozen for (R30.5,
    /// ADR-0028): below 2 = the one-time freeze of the 0.3.1 branch rule
    /// is still to run. A new state starts at [`SIDEBAR_RULE`].
    #[serde(default)]
    pub sidebar_rule: u32,
    /// Project headers the owner unfolded although they show folded by
    /// default (no Worktree and no work, R30.1), `<workspace>/<project>`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expanded_projects: Vec<String>,
    /// Agent Overview shows no Preview for the keyboard selection
    /// (`Alt+P`, R117.9); hover still shows one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub overview_no_preview: bool,
    /// The account Agents opened later use by default, per
    /// `<workspace>/<agent>` (the forecast Toast's action, R110.4): in
    /// memory only, never saved, never written to `workspace.toml`.
    #[serde(skip)]
    pub next_defaults: std::collections::BTreeMap<String, String>,
}

/// The sidebar membership rule of M26 (R30, ADR-0028): explicit only.
pub const SIDEBAR_RULE: u32 = 2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Source {
    /// Slug of the name; `general` / `general@<ws>` for a General.
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    /// Its Workspace's id (R23).
    #[serde(default = "default_workspace")]
    pub workspace: String,
    pub worktrees: Vec<Worktree>,
    pub active_worktree: String,
    /// Paused (R19.3): its processes are stopped, its Panes kept in the
    /// model and shown as placeholders; not started at launch.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub paused: bool,
    /// Its sidebar rows are folded under it (R1.6, M14).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub collapsed: bool,
}

/// A working identity rooted at a folder (R23); its name, color and env
/// come from `<root>/.rade/workspace.toml` ([`crate::workspace`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    /// Name given at add (`--name`, the toml's, else the folder's); the
    /// toml's `name`, when set, is shown instead.
    pub name: String,
    pub root: PathBuf,
    /// The Source it showed last; a switch shows it again (R23.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_source: Option<String>,
    /// The checkout each Source link resolved to the first time (R98.1),
    /// kept until the link's `base` / `checkout` key changes, the link is
    /// removed or the checkout is gone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub link_checkouts: Vec<crate::checkout::LinkCheckout>,
    /// Project ids in the order the Navigator shows them (R104.4): the
    /// order they were added or first seen; `Alt+1..9` follow it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub project_order: Vec<String>,
}

fn default_workspace() -> String {
    DEFAULT_WORKSPACE.into()
}

/// The General of Workspace `ws` (R23.7).
pub fn general_id(ws: &str) -> String {
    if ws == DEFAULT_WORKSPACE {
        GENERAL_ID.into()
    } else {
        format!("{GENERAL_ID}@{ws}")
    }
}

/// `general` or `general@<ws>` ([`slug`] never makes an `@`).
pub fn is_general_id(id: &str) -> bool {
    id == GENERAL_ID || id.starts_with("general@")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Worktree {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub tabs: Vec<Tab>,
    pub active_tab: Option<String>,
    /// The Source's `setup` command, still to run in this Worktree's first
    /// Terminal (R12.2); taken by that Terminal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<String>,
    /// Review Comments not sent yet (R13.2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<crate::review::ReviewComment>,
    /// Viewed files, Rounds and the Agent's summary (R118, R119).
    #[serde(default, skip_serializing_if = "crate::review_loop::Loop::is_empty")]
    pub review: crate::review_loop::Loop,
    /// The Project it was made for (R28.2): its config layer and sidebar group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// The branch or ref it started from (R28.3): Review compares with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tab {
    pub id: String,
    pub name: String,
    pub layout: Layout,
    /// Pane that has focus inside this Tab.
    pub focused: String,
    /// Zoomed Pane (R1.7); the Layout itself is untouched.
    #[serde(default)]
    pub zoomed: Option<String>,
    /// Subagents beyond the tiled ones, collapsed to Title Bars (R2.4).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub title_bars: Vec<Pane>,
    /// Agent Grid Parent while the Tab is auto-arranged; a manual split or
    /// resize clears it (R2.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<String>,
    /// `name` was given (rename, Fan-out): the label shows it instead of
    /// the Pane title ([`Tab::label`]).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub renamed: bool,
    /// The Viewer focused last: a plain file open goes there (R113.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_viewer: Option<String>,
}

/// Where a Pane lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneLoc {
    pub source: String,
    pub worktree: String,
    pub tab: String,
}

/// Result of closing a Pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Closed {
    pub loc: PaneLoc,
    /// The Pane was the Tab's last, so the Tab closed too.
    pub tab_closed: bool,
}

pub fn slug(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let s = s
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if s.is_empty() {
        "source".into()
    } else {
        s
    }
}

impl State {
    /// Fresh state: only General, rooted at `home`.
    pub fn new(home: &Path) -> Self {
        let mut ws = Self {
            version: STATE_VERSION,
            workspaces: Vec::new(),
            sources: Vec::new(),
            active_source: GENERAL_ID.into(),
            next_id: 0,
            notifications: Vec::new(),
            notifications_collapsed: false,
            file_tree: false,
            palette_recent: Vec::new(),
            fanouts: Vec::new(),
            collapsed_projects: Vec::new(),
            trusted_configs: Default::default(),
            sidebar_rule: SIDEBAR_RULE,
            expanded_projects: Vec::new(),
            overview_no_preview: false,
            next_defaults: Default::default(),
        };
        ws.ensure_general(home);
        ws
    }

    /// Repair loaded state: the `default` Workspace exists first, rooted
    /// at `home`; every Source is in a known Workspace; each Workspace has
    /// its General, rooted at its root, after every other Source (R23.1,
    /// R23.7).
    pub fn ensure_general(&mut self, home: &Path) {
        match self
            .workspaces
            .iter()
            .position(|w| w.id == DEFAULT_WORKSPACE)
        {
            Some(i) => {
                let w = self.workspaces.remove(i);
                self.workspaces.insert(0, w);
            }
            None => self.workspaces.insert(
                0,
                Workspace {
                    id: DEFAULT_WORKSPACE.into(),
                    name: DEFAULT_WORKSPACE.into(),
                    root: home.into(),
                    active_source: None,
                    link_checkouts: Vec::new(),
                    project_order: Vec::new(),
                },
            ),
        }
        self.workspaces[0].root = home.into();
        let known: Vec<String> = self.workspaces.iter().map(|w| w.id.clone()).collect();
        for s in &mut self.sources {
            if !known.contains(&s.workspace) {
                s.workspace = DEFAULT_WORKSPACE.into();
            }
        }
        let mut generals = Vec::new();
        for ws in self.workspaces.clone() {
            let id = general_id(&ws.id);
            let mut g = match self.sources.iter().position(|p| p.id == id) {
                Some(i) => self.sources.remove(i),
                None => {
                    let wt = self.new_worktree(&ws.root);
                    Source {
                        id,
                        name: "General".into(),
                        path: ws.root.clone(),
                        workspace: ws.id.clone(),
                        active_worktree: wt.id.clone(),
                        worktrees: vec![wt],
                        paused: false,
                        collapsed: false,
                    }
                }
            };
            g.workspace = ws.id.clone();
            g.path = ws.root.clone();
            g.paused = false; // General can't be paused
            for wt in &mut g.worktrees {
                wt.path = ws.root.clone();
            }
            generals.push(g);
        }
        // A General whose Workspace is gone goes with it.
        self.sources.retain(|s| !s.is_general());
        self.sources.extend(generals);
        if self.source(&self.active_source.clone()).is_none() {
            self.active_source = GENERAL_ID.into();
        }
        for i in 0..self.workspaces.len() {
            let keep = self.workspaces[i]
                .active_source
                .as_deref()
                .and_then(|a| self.source(a))
                .is_some_and(|s| s.workspace == self.workspaces[i].id);
            if !keep {
                self.workspaces[i].active_source = None;
            }
        }
        // ids must never repeat, even if next_id was lost.
        let max = self
            .all_ids()
            .filter_map(|id| id.split_once('_')?.1.parse::<u64>().ok())
            .max();
        self.next_id = self.next_id.max(max.unwrap_or(0));
        self.repair_notification_ids();
    }

    fn all_ids(&self) -> impl Iterator<Item = &str> {
        self.sources.iter().flat_map(|p| {
            p.worktrees.iter().flat_map(|w| {
                std::iter::once(w.id.as_str()).chain(w.tabs.iter().flat_map(|t| {
                    std::iter::once(t.id.as_str())
                        .chain(t.panes().into_iter().map(|p| p.id.as_str()))
                }))
            })
        })
    }

    pub(crate) fn next(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}_{}", self.next_id)
    }

    fn new_worktree(&mut self, path: &Path) -> Worktree {
        Worktree {
            id: self.next("w"),
            name: MAIN_WORKTREE.into(),
            path: path.into(),
            tabs: Vec::new(),
            active_tab: None,
            setup: None,
            comments: Vec::new(),
            review: Default::default(),
            project: None,
            base: None,
        }
    }

    pub(crate) fn new_pane(
        &mut self,
        cwd: &Path,
        command: Option<String>,
        name: Option<String>,
    ) -> Pane {
        Pane::new(self.next("p"), cwd.into(), command, name)
    }

    // ---- lookup -----------------------------------------------------------

    pub fn source(&self, id: &str) -> Option<&Source> {
        self.sources.iter().find(|p| p.id == id)
    }

    pub(crate) fn source_mut(&mut self, id: &str) -> Option<&mut Source> {
        self.sources.iter_mut().find(|p| p.id == id)
    }

    /// Source by id or name.
    pub fn find_source(&self, query: &str) -> Result<&Source> {
        self.sources
            .iter()
            .find(|p| p.id == query)
            .or_else(|| self.sources.iter().find(|p| p.name == query))
            .ok_or_else(|| Error::not_found(format!("no source '{query}'")))
    }

    pub fn active(&self) -> (&Source, &Worktree) {
        let p = self
            .source(&self.active_source)
            .expect("active source exists");
        let w = p
            .worktree(&p.active_worktree)
            .expect("active worktree exists");
        (p, w)
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        let (_, w) = self.active();
        w.tab(w.active_tab.as_deref()?)
    }

    pub fn worktree(&self, source: &str, worktree: &str) -> Option<&Worktree> {
        self.source(source)?.worktree(worktree)
    }

    pub(crate) fn worktree_mut(&mut self, source: &str, worktree: &str) -> Result<&mut Worktree> {
        self.source_mut(source)
            .and_then(|p| p.worktrees.iter_mut().find(|w| w.id == worktree))
            .ok_or_else(|| Error::not_found(format!("no worktree {source}/{worktree}")))
    }

    pub fn worktrees(&self) -> impl Iterator<Item = (&Source, &Worktree)> {
        self.sources
            .iter()
            .flat_map(|p| p.worktrees.iter().map(move |w| (p, w)))
    }

    pub fn locate_tab(&self, tab: &str) -> Option<(String, String)> {
        self.worktrees()
            .find(|(_, w)| w.tab(tab).is_some())
            .map(|(p, w)| (p.id.clone(), w.id.clone()))
    }

    pub fn locate(&self, pane: &str) -> Option<PaneLoc> {
        self.worktrees().find_map(|(p, w)| {
            w.tabs
                .iter()
                .find(|t| t.pane(pane).is_some())
                .map(|t| PaneLoc {
                    source: p.id.clone(),
                    worktree: w.id.clone(),
                    tab: t.id.clone(),
                })
        })
    }

    pub fn pane(&self, id: &str) -> Option<&Pane> {
        let loc = self.locate(id)?;
        self.tab(&loc)?.pane(id)
    }

    pub(crate) fn tab(&self, loc: &PaneLoc) -> Option<&Tab> {
        self.worktree(&loc.source, &loc.worktree)?.tab(&loc.tab)
    }

    pub(crate) fn tab_mut(&mut self, loc: &PaneLoc) -> Result<&mut Tab> {
        self.worktree_mut(&loc.source, &loc.worktree)?
            .tabs
            .iter_mut()
            .find(|t| t.id == loc.tab)
            .ok_or_else(|| Error::not_found(format!("no tab {}", loc.tab)))
    }

    pub(crate) fn locate_or_err(&self, pane: &str) -> Result<PaneLoc> {
        self.locate(pane)
            .ok_or_else(|| Error::not_found(format!("no pane '{pane}'")))
    }

    /// Resolve a Pane id or name (API.md "Addressing"): a name resolves in
    /// the caller's Worktree, then its Source, then globally.
    pub fn resolve_pane(&self, query: &str, caller: Option<&str>) -> Result<String> {
        if self.locate(query).is_some() {
            return Ok(query.into());
        }
        let named: Vec<(PaneLoc, String)> = self
            .worktrees()
            .flat_map(|(p, w)| {
                w.tabs.iter().flat_map(move |t| {
                    t.panes()
                        .into_iter()
                        .filter(|x| x.name.as_deref() == Some(query))
                        .map(move |x| {
                            let loc = PaneLoc {
                                source: p.id.clone(),
                                // where it runs (R15.2)
                                worktree: x.runs_in.clone().unwrap_or_else(|| w.id.clone()),
                                tab: t.id.clone(),
                            };
                            (loc, x.id.clone())
                        })
                })
            })
            .collect();
        // The Worktree the caller runs in (a Fan-out Agent's own, R15.2).
        let caller = caller.and_then(|c| {
            let (source, worktree) = self.pane_worktree(c)?;
            Some(PaneLoc {
                source,
                worktree,
                tab: String::new(),
            })
        });
        let ws_of = |source: &str| self.source(source).map(|s| s.workspace.clone());
        let caller_ws = caller.as_ref().and_then(|c| ws_of(&c.source));
        let scopes: [&dyn Fn(&PaneLoc) -> bool; 4] = [
            &|l| {
                caller
                    .as_ref()
                    .is_some_and(|c| c.source == l.source && c.worktree == l.worktree)
            },
            &|l| caller.as_ref().is_some_and(|c| c.source == l.source),
            // then the caller's Workspace (R23.4)
            &|l| caller_ws.is_some() && ws_of(&l.source) == caller_ws,
            &|_| true,
        ];
        for in_scope in scopes {
            let hits: Vec<&String> = named
                .iter()
                .filter(|(l, _)| in_scope(l))
                .map(|(_, id)| id)
                .collect();
            match hits.len() {
                0 => continue,
                1 => return Ok(hits[0].clone()),
                _ => {
                    let ids: Vec<&str> = hits.iter().map(|s| s.as_str()).collect();
                    return Err(Error::new(
                        ErrorKind::Ambiguous,
                        format!("'{query}' matches several panes: {}", ids.join(", ")),
                    ));
                }
            }
        }
        Err(Error::not_found(format!("no pane '{query}'")))
    }

    pub fn pane_infos(&self) -> Vec<PaneInfo> {
        self.worktrees()
            .flat_map(|(p, w)| {
                w.tabs.iter().flat_map(move |t| {
                    t.panes().into_iter().map(move |x| PaneInfo {
                        pane: x.id.clone(),
                        name: x.name.clone(),
                        workspace: p.workspace.clone(),
                        source: p.id.clone(),
                        worktree: w.name.clone(),
                        tab: t.id.clone(),
                        parent: x.parent.clone(),
                        agent: x.agent.clone(),
                        status: x.status.map(|s| s.as_str().to_string()),
                        status_source: x.status_source().map(Into::into),
                        status_since: x.status.and(x.status_since),
                        unread: self.unread(&x.id),
                        title: x.title.clone(),
                        account: x.account.clone(),
                        cwd: x
                            .url()
                            .map_or_else(|| x.cwd().display().to_string(), Into::into),
                    })
                })
            })
            .collect()
    }

    // ---- Sources (R1.1, R1.5) --------------------------------------------

    /// Register a folder as a Source (placed before General). Never touches files.
    pub fn add_source(&mut self, path: &Path, name: Option<&str>) -> Result<String> {
        self.add_source_in(path, name, None)
    }

    /// [`Self::add_source`] into Workspace `ws` (id or name; `None`: the
    /// one whose root holds `path`, else the one shown, R24.2).
    pub fn add_source_in(
        &mut self,
        path: &Path,
        name: Option<&str>,
        ws: Option<&str>,
    ) -> Result<String> {
        let ws = ws
            .map(|q| self.find_workspace(q).map(|w| w.id.clone()))
            .transpose()?;
        if !path.is_absolute() {
            return Err(Error::usage(format!(
                "source path must be absolute: {}",
                path.display()
            )));
        }
        if let Some(p) = self
            .sources
            .iter()
            .find(|p| !p.is_general() && p.path == path)
        {
            return Err(Error::usage(format!(
                "{} is already source '{}'",
                path.display(),
                p.name
            )));
        }
        let name = name
            .map(str::to_string)
            .or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "root".into());
        let base = slug(&name);
        let mut id = base.clone();
        let mut n = 1;
        while self.source(&id).is_some() {
            n += 1;
            id = format!("{base}-{n}");
        }
        let wt = self.new_worktree(path);
        let workspace = ws.unwrap_or_else(|| self.workspace_for_path(path));
        let source = Source {
            id: id.clone(),
            name,
            path: path.into(),
            workspace,
            active_worktree: wt.id.clone(),
            worktrees: vec![wt],
            paused: false,
            collapsed: false,
        };
        self.insert_source(source);
        Ok(id)
    }

    /// Unregister a Source (by id or name); returns it so the caller can
    /// close its Terminals. General cannot be removed. Files are untouched.
    pub fn remove_source(&mut self, query: &str) -> Result<Source> {
        let id = self.find_source(query)?.id.clone();
        if self.source(&id).unwrap().is_general() {
            return Err(Error::usage("General cannot be removed"));
        }
        let i = self.sources.iter().position(|p| p.id == id).unwrap();
        let removed = self.sources.remove(i);
        self.fanouts.retain(|f| f.source != id);
        if self.active_source == id {
            self.active_source = general_id(&removed.workspace);
        }
        Ok(removed)
    }

    /// Index of the first General: other Sources go before it.
    fn first_general(&self) -> usize {
        self.sources
            .iter()
            .position(Source::is_general)
            .unwrap_or(self.sources.len())
    }

    /// Insert a new Source after the other non-General ones.
    pub(crate) fn insert_source(&mut self, source: Source) {
        let at = self.first_general();
        self.sources.insert(at, source);
    }

    /// The Workspace a new Source at `path` joins: the one whose root
    /// holds it (the most specific; `default`, at `$HOME`, last), else the
    /// one shown (R24.2).
    pub fn workspace_for_path(&self, path: &Path) -> String {
        self.workspaces
            .iter()
            .filter(|w| w.id != DEFAULT_WORKSPACE && path.starts_with(&w.root))
            .max_by_key(|w| w.root.components().count())
            .map(|w| w.id.clone())
            .unwrap_or_else(|| self.active_workspace().to_string())
    }

    /// The Workspace on screen: the active Source's (R23.4).
    pub fn active_workspace(&self) -> &str {
        self.source(&self.active_source)
            .map_or(DEFAULT_WORKSPACE, |s| s.workspace.as_str())
    }

    /// Move a Source to `index` among the non-General Sources.
    pub fn move_source(&mut self, id: &str, index: usize) -> Result<()> {
        if self.source(id).is_some_and(Source::is_general) {
            return Err(Error::usage("General stays last"));
        }
        let i = self
            .sources
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| Error::not_found(format!("no source '{id}'")))?;
        let p = self.sources.remove(i);
        let max = self.first_general();
        self.sources.insert(index.min(max), p);
        Ok(())
    }

    pub fn select_source(&mut self, id: &str) -> Result<()> {
        let id = self.find_source(id)?.id.clone();
        self.set_active_source(id);
        Ok(())
    }

    /// Show a Source; its Workspace remembers it for the next switch
    /// (R23.5).
    pub(crate) fn set_active_source(&mut self, id: String) {
        if let Some(ws) = self.source(&id).map(|s| s.workspace.clone()) {
            if let Some(w) = self.workspaces.iter_mut().find(|w| w.id == ws) {
                w.active_source = Some(id.clone());
            }
        }
        self.active_source = id;
    }

    /// Rename a Source (by id or name): its display name only; the id
    /// (CLI, `RADE_SOURCE`, Worktree folders) never changes (R19.4). The
    /// name must be non-empty and not another Source's name or id.
    pub fn rename_source(&mut self, query: &str, name: &str) -> Result<String> {
        let id = self.find_source(query)?.id.clone();
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::usage("a Source name can't be empty"));
        }
        if let Some(o) = self
            .sources
            .iter()
            .find(|p| p.id != id && (p.name == name || p.id == name))
        {
            return Err(Error::usage(format!(
                "'{name}' is already Source '{}' ({})",
                o.name, o.id
            )));
        }
        self.source_mut(&id).unwrap().name = name.into();
        Ok(id)
    }

    // ---- Tabs and Panes (R1.3, R1.4, R1.7) ---------------------------------

    /// New Tab with one Terminal whose cwd is the Worktree root (R1.4).
    /// Becomes the Worktree's active Tab only if it has none; use
    /// [`Self::select_tab`] to show it.
    pub fn new_tab(
        &mut self,
        source: &str,
        worktree: &str,
        command: Option<String>,
        name: Option<String>,
    ) -> Result<(String, String)> {
        self.check_not_paused(source)?; // before the setup is taken
        let root = self.worktree_mut(source, worktree)?.path.clone();
        let mut pane = self.new_pane(&root, command, name);
        self.take_setup(source, worktree, &mut pane);
        self.new_tab_with(source, worktree, pane)
    }

    /// The Preset whose program now runs in Pane `id`'s foreground (R115.4),
    /// none when it ended; true when that changed.
    pub fn set_detected(&mut self, id: &str, preset: Option<String>) -> bool {
        let Some(loc) = self.locate(id) else {
            return false;
        };
        let Some(p) = self.tab_mut(&loc).ok().and_then(|t| t.pane_mut(id)) else {
            return false;
        };
        if p.detected == preset {
            return false;
        }
        p.detected = preset;
        true
    }

    /// A new Tab next to Pane `beside` (the UI's New Tab): it starts where
    /// a split of `beside` would ([`Self::start_root`]).
    pub fn new_tab_beside(
        &mut self,
        source: &str,
        worktree: &str,
        beside: Option<&str>,
    ) -> Result<(String, String)> {
        self.check_not_paused(source)?; // before the setup is taken
        let root = self.start_root(source, worktree, beside)?;
        let mut pane = self.new_pane(&root, None, None);
        self.take_setup(source, worktree, &mut pane);
        let (tab, pane) = self.new_tab_with(source, worktree, pane)?;
        self.move_after_active(source, worktree, &tab);
        Ok((tab, pane))
    }

    /// Put new Tab `tab` right after its Worktree's active Tab, like a
    /// browser (R19.7: the UI's New Tab and `+` menu); the CLI appends.
    pub fn move_after_active(&mut self, source: &str, worktree: &str, tab: &str) {
        let active = self
            .worktree(source, worktree)
            .and_then(|w| w.active_tab.clone());
        if let Some(a) = active.filter(|a| a != tab) {
            self.move_tab_after(tab, &a);
        }
    }

    /// Move Tab `tab` right after Tab `after` of the same Worktree.
    pub fn move_tab_after(&mut self, tab: &str, after: &str) {
        let Some((s, w)) = self.locate_tab(tab) else {
            return;
        };
        let Ok(wt) = self.worktree_mut(&s, &w) else {
            return;
        };
        if tab == after || wt.tab(after).is_none() {
            return;
        }
        let from = wt.tabs.iter().position(|t| t.id == tab).unwrap();
        let t = wt.tabs.remove(from);
        let at = wt.tabs.iter().position(|t| t.id == after).unwrap() + 1;
        wt.tabs.insert(at, t);
    }

    /// Where a new Terminal next to Pane `beside` starts (R1.4, R104.5):
    /// the Worktree root, not `beside`'s cwd; but when that cwd is inside a
    /// recorded link checkout of the Source (a Project checkout or a path)
    /// other than the Worktree root, that checkout, so a Terminal on a link
    /// stays on it.
    pub(crate) fn start_root(
        &self,
        source: &str,
        worktree: &str,
        beside: Option<&str>,
    ) -> Result<PathBuf> {
        let root = self
            .worktree(source, worktree)
            .ok_or_else(|| Error::not_found(format!("no worktree '{worktree}'")))?
            .path
            .clone();
        let Some(cwd) = beside
            .and_then(|p| self.pane(p))
            .and_then(|p| match &p.kind {
                PaneKind::Terminal { cwd, .. } => Some(cwd.clone()),
                _ => None,
            })
        else {
            return Ok(root);
        };
        let inside = |dir: &Path| {
            cwd.starts_with(dir)
                || matches!(
                    (cwd.canonicalize(), dir.canonicalize()),
                    (Ok(c), Ok(d)) if c.starts_with(&d)
                )
        };
        Ok(self
            .workspaces
            .iter()
            .flat_map(|w| &w.link_checkouts)
            .filter(|r| r.source == source && r.path != root && inside(&r.path))
            .max_by_key(|r| r.path.as_os_str().len())
            .map_or(root, |r| r.path.clone()))
    }

    pub(crate) fn new_tab_with(
        &mut self,
        source: &str,
        worktree: &str,
        pane: Pane,
    ) -> Result<(String, String)> {
        self.check_not_paused(source)?;
        let tab_id = self.next("t");
        let wt = self.worktree_mut(source, worktree)?;
        let tab = Tab {
            id: tab_id.clone(),
            name: format!("Tab {}", wt.tabs.len() + 1),
            focused: pane.id.clone(),
            layout: Layout::leaf(pane.clone()),
            zoomed: None,
            title_bars: Vec::new(),
            grid: None,
            renamed: false,
            last_viewer: None,
        };
        wt.tabs.push(tab);
        wt.active_tab.get_or_insert(tab_id.clone());
        Ok((tab_id, pane.id))
    }

    /// Split `target`; the new Terminal starts in the Worktree root, not the
    /// focused Pane's cwd (R1.4), or in the link checkout `target` is in
    /// ([`Self::start_root`], R104.5). Focus moves to it and Zoom is cleared.
    pub fn split(
        &mut self,
        target: &str,
        dir: SplitDir,
        command: Option<String>,
        name: Option<String>,
    ) -> Result<String> {
        let loc = self.locate_or_err(target)?;
        self.check_not_paused(&loc.source)?; // before the setup is taken
        let root = self.start_root(&loc.source, &loc.worktree, Some(target))?;
        let mut pane = self.new_pane(&root, command, name);
        self.take_setup(&loc.source, &loc.worktree, &mut pane);
        self.split_with(target, dir, pane, true)
    }

    /// Split `target` with `pane` (a Title Bar `target` splits the first tiled
    /// Pane). A split is a manual arrangement: the Tab stops auto-arranging
    /// its Agent Grid (R2.4).
    pub(crate) fn split_with(
        &mut self,
        target: &str,
        dir: SplitDir,
        pane: Pane,
        focus: bool,
    ) -> Result<String> {
        let loc = self.locate_or_err(target)?;
        self.check_not_paused(&loc.source)?;
        let id = pane.id.clone();
        let tab = self.tab_mut(&loc)?;
        let anchor = if tab.layout.pane(target).is_some() {
            target.to_string()
        } else {
            tab.layout.first_pane().id.clone()
        };
        tab.layout.split(&anchor, dir, pane);
        tab.grid = None;
        if focus {
            tab.focused = id.clone();
            tab.zoomed = None;
        }
        Ok(id)
    }

    /// Close a Pane: its sibling takes the space; the last Pane closes the Tab.
    pub fn close_pane(&mut self, id: &str) -> Result<Closed> {
        let loc = self.locate_or_err(id)?;
        // A closed Fan-out Agent can't finish any more (R15).
        self.track_fanout(id, Some(crate::AgentStatus::Exited));
        let tab = self.tab_mut(&loc)?;
        let rest = if let Some(i) = tab.title_bars.iter().position(|p| p.id == id) {
            tab.title_bars.remove(i);
            Ok(Some(tab.layout.clone()))
        } else {
            match tab.layout.clone().remove(id) {
                // The last tiled Pane went: a Title Bar takes its place.
                Ok(None) if !tab.title_bars.is_empty() => {
                    Ok(Some(Layout::leaf(tab.title_bars.remove(0))))
                }
                r => r,
            }
        };
        match rest {
            Ok(Some(rest)) => {
                tab.layout = rest;
                if tab.grid.as_deref() == Some(id) {
                    tab.grid = None; // Parent gone: Subagents stay where they are.
                }
                tab.arrange();
                if tab.focused == id {
                    tab.focused = tab.layout.first_pane().id.clone();
                }
                if tab.zoomed.as_deref() == Some(id) {
                    tab.zoomed = None;
                }
                Ok(Closed {
                    loc,
                    tab_closed: false,
                })
            }
            Ok(None) => {
                self.remove_tab(&loc.source, &loc.worktree, &loc.tab)?;
                Ok(Closed {
                    loc,
                    tab_closed: true,
                })
            }
            Err(_) => unreachable!("located pane must be removable"),
        }
    }

    /// Close a whole Tab; returns the ids of its Panes.
    pub fn close_tab(&mut self, tab: &str) -> Result<Vec<String>> {
        let (p, w) = self
            .locate_tab(tab)
            .ok_or_else(|| Error::not_found(format!("no tab '{tab}'")))?;
        let panes = self.worktree(&p, &w).unwrap().tab(tab).unwrap().pane_ids();
        for id in &panes {
            self.track_fanout(id, Some(crate::AgentStatus::Exited));
        }
        self.remove_tab(&p, &w, tab)?;
        Ok(panes)
    }

    fn remove_tab(&mut self, source: &str, worktree: &str, tab: &str) -> Result<()> {
        let wt = self.worktree_mut(source, worktree)?;
        let i = wt.tabs.iter().position(|t| t.id == tab).unwrap();
        wt.tabs.remove(i);
        if wt.active_tab.as_deref() == Some(tab) {
            // Next Tab, or the previous one if it was last.
            wt.active_tab = wt
                .tabs
                .get(i)
                .or_else(|| wt.tabs.last())
                .map(|t| t.id.clone());
        }
        Ok(())
    }

    pub fn select_tab(&mut self, tab: &str) -> Result<()> {
        let (p, w) = self
            .locate_tab(tab)
            .ok_or_else(|| Error::not_found(format!("no tab '{tab}'")))?;
        self.worktree_mut(&p, &w)?.active_tab = Some(tab.into());
        Ok(())
    }

    /// Select the Tab `delta` steps away in the active Worktree, wrapping.
    pub fn cycle_tab(&mut self, delta: isize) {
        let (p, w) = {
            let (p, w) = self.active();
            (p.id.clone(), w.id.clone())
        };
        let wt = self.worktree_mut(&p, &w).unwrap();
        let n = wt.tabs.len() as isize;
        if n == 0 {
            return;
        }
        let cur = wt
            .tabs
            .iter()
            .position(|t| Some(&t.id) == wt.active_tab.as_ref())
            .unwrap_or(0) as isize;
        let next = (cur + delta).rem_euclid(n) as usize;
        wt.active_tab = Some(wt.tabs[next].id.clone());
    }

    pub fn move_tab(&mut self, tab: &str, index: usize) -> Result<()> {
        let (p, w) = self
            .locate_tab(tab)
            .ok_or_else(|| Error::not_found(format!("no tab '{tab}'")))?;
        let wt = self.worktree_mut(&p, &w)?;
        let i = wt.tabs.iter().position(|t| t.id == tab).unwrap();
        let t = wt.tabs.remove(i);
        let at = index.min(wt.tabs.len());
        wt.tabs.insert(at, t);
        Ok(())
    }

    pub fn rename_tab(&mut self, tab: &str, name: &str) -> Result<()> {
        let (p, w) = self
            .locate_tab(tab)
            .ok_or_else(|| Error::not_found(format!("no tab '{tab}'")))?;
        let wt = self.worktree_mut(&p, &w)?;
        let i = wt.tabs.iter().position(|t| t.id == tab).unwrap();
        let t = &mut wt.tabs[i];
        // An empty name gives the label back to the Pane title.
        t.renamed = !name.trim().is_empty();
        t.name = if t.renamed {
            name.trim().into()
        } else {
            format!("Tab {}", i + 1)
        };
        Ok(())
    }

    /// Record a Pane's terminal title (already sanitized); false if unchanged.
    pub fn set_pane_title(&mut self, id: &str, title: &str) -> bool {
        let Some(loc) = self.locate(id) else {
            return false;
        };
        let Ok(t) = self.tab_mut(&loc) else {
            return false;
        };
        let p = t.pane_mut(id).unwrap();
        if p.title.as_deref() == Some(title) {
            return false;
        }
        p.title = Some(title.into());
        true
    }

    /// Name a Pane (`None` / blank clears it). Names address Panes in the
    /// CLI (API.md "Addressing"), so a name may not look like a Pane id and
    /// may not be taken by another Pane running in the same Worktree (that
    /// lookup would be `AMBIGUOUS`); elsewhere it may repeat (R19.4).
    pub fn rename_pane(&mut self, id: &str, name: Option<&str>) -> Result<()> {
        let loc = self.locate_or_err(id)?;
        let name = name.map(str::trim).filter(|n| !n.is_empty());
        if let Some(n) = name {
            if n.strip_prefix("p_")
                .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
            {
                return Err(Error::usage(format!(
                    "'{n}' looks like a Pane id; pick another name"
                )));
            }
            let here = self.pane_worktree(id);
            let taken = self
                .pane_infos()
                .into_iter()
                .find(|i| {
                    i.pane != id
                        && i.name.as_deref() == Some(n)
                        && self.pane_worktree(&i.pane) == here
                })
                .map(|i| i.pane);
            if let Some(other) = taken {
                return Err(Error::usage(format!(
                    "{other} in this Worktree is already named '{n}'"
                )));
            }
        }
        self.tab_mut(&loc)?.pane_mut(id).unwrap().name = name.map(Into::into);
        Ok(())
    }

    /// Make `id` the focused Pane and show its Source, Worktree, and Tab.
    pub fn focus_pane(&mut self, id: &str) -> Result<()> {
        let loc = self.locate_or_err(id)?;
        let tab = self.tab_mut(&loc)?;
        tab.focused = id.into();
        if tab.pane(id).is_some_and(|p| p.is_viewer()) {
            tab.last_viewer = Some(id.into());
        }
        // A Title Bar Pane is shown by Zooming it (R2.4, R3.3).
        if tab.zoomed.as_ref().is_some_and(|z| z != id) || tab.title_bars.iter().any(|p| p.id == id)
        {
            tab.zoomed = Some(id.into());
        }
        self.mark_read(id);
        self.worktree_mut(&loc.source, &loc.worktree)?.active_tab = Some(loc.tab.clone());
        let p = self.source_mut(&loc.source).unwrap();
        p.active_worktree = loc.worktree;
        self.set_active_source(loc.source);
        Ok(())
    }

    /// Move focus in the active Tab by geometry; returns the new focus.
    pub fn focus_direction(&mut self, dir: Direction) -> Option<String> {
        let tab = self.active_tab()?;
        if tab.zoomed.is_some() {
            return None;
        }
        let next = tab.layout.neighbor(&tab.focused, dir)?;
        self.focus_pane(&next).ok()?;
        Some(next)
    }

    /// Toggle Zoom of the Tab's focused Pane (R1.7).
    pub fn toggle_zoom(&mut self, tab: &str) -> Result<Option<String>> {
        let (p, w) = self
            .locate_tab(tab)
            .ok_or_else(|| Error::not_found(format!("no tab '{tab}'")))?;
        let t = self
            .worktree_mut(&p, &w)?
            .tabs
            .iter_mut()
            .find(|t| t.id == tab)
            .unwrap();
        t.zoomed = match t.zoomed {
            Some(_) => None,
            None => Some(t.focused.clone()),
        };
        Ok(t.zoomed.clone())
    }

    /// Track a Terminal's cwd (OSC 7); returns whether it changed.
    pub fn set_pane_cwd(&mut self, id: &str, new: &Path) -> bool {
        let Some(loc) = self.locate(id) else {
            return false;
        };
        let Ok(tab) = self.tab_mut(&loc) else {
            return false;
        };
        let Some(pane) = tab.pane_mut(id) else {
            return false;
        };
        let PaneKind::Terminal { cwd, .. } = &mut pane.kind else {
            return false;
        };
        if cwd == new {
            return false;
        }
        *cwd = new.into();
        true
    }

    /// `manual` = the user dragged it: the Tab stops auto-arranging (R2.4).
    pub fn set_split_ratio(&mut self, tab: &str, path: &[bool], ratio: f64, manual: bool) -> bool {
        let Some((p, w)) = self.locate_tab(tab) else {
            return false;
        };
        let Ok(wt) = self.worktree_mut(&p, &w) else {
            return false;
        };
        wt.tabs.iter_mut().find(|t| t.id == tab).is_some_and(|t| {
            if manual {
                t.grid = None;
            }
            t.layout.set_ratio(path, ratio)
        })
    }
}

impl Source {
    pub fn worktree(&self, id: &str) -> Option<&Worktree> {
        self.worktrees.iter().find(|w| w.id == id)
    }

    pub fn is_general(&self) -> bool {
        is_general_id(&self.id)
    }
}

impl Worktree {
    pub fn tab(&self, id: &str) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
