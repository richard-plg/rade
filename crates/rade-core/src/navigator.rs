//! The Navigator tree (PRD R104, ADR-0034 decision 8, spec A.17–A.38):
//! Projects → Source links (name + base) → Worktrees → Agents, then
//! Scratch. Pure: the app passes the shown Workspace's state, its
//! Projects in shown order, the resolved links ([`LinkView`]) and the git
//! facts it gathered off the UI thread, and renders the [`Row`]s.
//!
//! Rules:
//! - a link's Agents: the Agents of the Source's main Worktree whose cwd
//!   is inside the link's checkout; a link on the main checkout also takes
//!   every main-Worktree Agent whose cwd is in no link's other checkout
//!   (main checkouts appear under each link that uses them, R104.1);
//! - a link's Worktrees: the Source's linked Worktrees recorded for the
//!   link's Project (R99);
//! - Scratch: the Workspace root, Sources no Project links, a linked
//!   Source's main checkout that no link uses while it has Panes, and
//!   Worktrees with no recorded Project (each with its `→ <Project>?`);
//! - a parent row carries the worst LED and the waiting count of the
//!   Agents under it only while collapsed (R104.2);
//! - fold state: `State::collapsed_projects` keys `<ws>/<project>`,
//!   `<ws>/<project>/<source>`, `<ws>/<project>/<source>/<worktree id>`.

use crate::agents::AgentStatus;
use crate::design::{suggest, Branches};
use crate::linkview::LinkView;
use crate::model::{Source, State};
use crate::project::Project;
use crate::{Pane, PaneKind};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RowKind {
    Project,
    Link,
    Worktree,
    Agent,
    /// A section label (`Scratch`).
    Section,
    /// A Scratch row: the Workspace root or a Source's main checkout.
    Scratch,
    /// `No Projects yet` (spec A empty state).
    Empty,
}

/// What a click on a row opens.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Target {
    /// The Project's home page (R105).
    Project {
        project: String,
    },
    Link {
        project: String,
        source: String,
    },
    Worktree {
        source: String,
        worktree: String,
    },
    Pane {
        pane: String,
    },
    /// A Source's main Worktree (the Workspace root for its General).
    Scratch {
        source: String,
    },
    None,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Row {
    pub kind: RowKind,
    pub depth: u8,
    /// Fold key (parents) or a stable id.
    pub key: String,
    pub title: String,
    /// Right-aligned mono text: a link's base (its HEAD when drifted), a
    /// Worktree's `↑3 ●4`, an Agent's status words / time / port, a
    /// Scratch row's Pane count.
    pub meta: String,
    /// Agent rows: their status; collapsed parents: the worst below.
    pub led: Option<AgentStatus>,
    /// Agent rows: their agent's glyph, coloured by `led` (R115).
    pub mark: Option<crate::design::AgentMark>,
    /// Agent rows: their Preset (its `icon`, R115.5).
    pub preset: Option<String>,
    /// Collapsed parents: Agents waiting below (shown when > 0).
    pub badge: usize,
    pub drift: bool,
    /// The also-in marker's tooltip (`Also in Billing @ release/2.4`).
    pub also_in: Option<String>,
    /// A Scratch Worktree's suggestion: (project id, name).
    pub suggest: Option<(String, String)>,
    /// `None` = no chevron.
    pub collapsed: Option<bool>,
    /// A Project with no links (title in `fg2`).
    pub dim: bool,
    /// A Project's index in shown order (`proj-1..3` cycle).
    pub color: Option<usize>,
    pub selected: bool,
    /// A Worktree under review: `R2 · 3 open` / `approved` (R119.5).
    pub review: Option<String>,
    pub target: Target,
}

impl Row {
    fn new(kind: RowKind, depth: u8, key: String, title: String, target: Target) -> Row {
        Row {
            kind,
            depth,
            key,
            title,
            meta: String::new(),
            led: None,
            mark: None,
            preset: None,
            badge: 0,
            drift: false,
            also_in: None,
            suggest: None,
            collapsed: None,
            dim: false,
            color: None,
            selected: false,
            review: None,
            target,
        }
    }

    /// The row as one text (`debug.ui sidebar` `text`, R104-a): the title,
    /// and a link's base after it unless drifted.
    pub fn label(&self) -> String {
        if self.kind == RowKind::Link && !self.drift && !self.meta.is_empty() {
            format!("{} {}", self.title, self.meta)
        } else {
            self.title.clone()
        }
    }

    /// One line of the golden text (R104-e).
    pub fn text(&self) -> String {
        let chev = match self.collapsed {
            Some(true) => "▸ ",
            Some(false) => "▾ ",
            None => "  ",
        };
        let mut s = format!("{}{chev}{}", "  ".repeat(self.depth as usize), self.title);
        if !self.meta.is_empty() {
            s += &format!("  {}", self.meta);
        }
        let mut flags: Vec<String> = Vec::new();
        flags.push(format!("{:?}", self.kind).to_lowercase());
        if let Some(l) = self.led {
            flags.push(format!("led={}", l.as_str()));
        }
        if self.badge > 0 {
            flags.push(format!("badge={}", self.badge));
        }
        if self.drift {
            flags.push("drift".into());
        }
        if let Some(t) = &self.also_in {
            flags.push(format!("also: {t}"));
        }
        if let Some(t) = &self.review {
            flags.push(format!("review: {t}"));
        }
        if let Some((_, n)) = &self.suggest {
            flags.push(format!("→ {n}?"));
        }
        if self.dim {
            flags.push("dim".into());
        }
        if self.selected {
            flags.push("selected".into());
        }
        format!("{s}  [{}]", flags.join(", "))
    }
}

/// What [`build`] reads besides the state.
pub struct Input<'a> {
    /// The shown Workspace's Projects, in shown order ([`ordered`]).
    pub projects: &'a [Project],
    /// Every link of the Workspace (Keep already applied).
    pub links: &'a [LinkView],
    /// `(ahead of its base, changed files)` of a Worktree folder.
    pub worktree_facts: &'a dyn Fn(&Path) -> (usize, usize),
    /// The port a Pane listens on (`:5173` as an Agent's meta).
    pub port: &'a dyn Fn(&str) -> Option<u16>,
    /// Current branches (the Scratch suggestions).
    pub branches: &'a Branches,
    /// `realpath`, to compare a Pane's cwd with checkout folders.
    pub canon: &'a dyn Fn(&Path) -> PathBuf,
    pub home: &'a Path,
    pub selected: Option<&'a Target>,
    pub now: u64,
}

/// The Projects in shown order (R104.4): those in `order` first, as
/// listed, then the rest by id.
pub fn ordered(projects: &[Project], order: &[String]) -> Vec<Project> {
    let rank = |id: &str| order.iter().position(|o| o == id).unwrap_or(usize::MAX);
    let mut v = projects.to_vec();
    v.sort_by(|a, b| rank(&a.id).cmp(&rank(&b.id)).then(a.id.cmp(&b.id)));
    v
}

impl State {
    /// Append Project ids not yet in Workspace `ws`'s shown order (in the
    /// given order); whether anything changed.
    pub fn note_projects(&mut self, ws: &str, ids: &[String]) -> bool {
        let Some(w) = self.workspaces.iter_mut().find(|w| w.id == ws) else {
            return false;
        };
        let mut changed = false;
        for id in ids {
            if !w.project_order.contains(id) {
                w.project_order.push(id.clone());
                changed = true;
            }
        }
        changed
    }

    /// Project ids for `Alt+1..9` (R104.4): the shown Workspace's order.
    pub fn alt_projects(&self, projects: &[Project]) -> Vec<String> {
        let order = self
            .workspace(self.active_workspace())
            .map(|w| w.project_order.clone())
            .unwrap_or_default();
        ordered(projects, &order)
            .into_iter()
            .map(|p| p.id)
            .collect()
    }
}

/// `/home/u/x` → `~/x`.
pub fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(r) if r.as_os_str().is_empty() => "~".into(),
        Ok(r) => format!("~/{}", r.display()),
        Err(_) => path.display().to_string(),
    }
}

fn pane_cwd(p: &Pane) -> Option<&Path> {
    match &p.kind {
        PaneKind::Terminal { cwd, .. } => Some(cwd),
        _ => None,
    }
}

fn agent_row(p: &Pane, depth: u8, input: &Input) -> Row {
    // The user's name, else the session's title (R104.7), else the Preset.
    let title = p
        .name
        .clone()
        .or_else(|| p.session_title())
        .or_else(|| p.agent_name().map(String::from))
        .unwrap_or_else(|| "Terminal".into());
    let mut r = Row::new(
        RowKind::Agent,
        depth,
        p.id.clone(),
        title,
        Target::Pane { pane: p.id.clone() },
    );
    r.led = p.status;
    r.mark = p.mark();
    r.preset = p.agent_name().map(String::from);
    r.meta = match p.status {
        Some(AgentStatus::Waiting) | Some(AgentStatus::Error) => {
            p.status.unwrap().words().to_string()
        }
        Some(AgentStatus::Running) => p.status_since.map_or(String::new(), |s| {
            crate::design::age(input.now.saturating_sub(s))
        }),
        _ => match (input.port)(&p.id) {
            Some(port) => format!(":{port}"),
            None => p.agent_name().unwrap_or_default().to_string(),
        },
    };
    r
}

/// Worst status and waiting count of the built rows: Agent rows and, for a
/// child that is itself collapsed, the aggregate it already carries
/// (expanded parents carry none, so nothing counts twice).
fn aggregate(rows: &[Row]) -> (Option<AgentStatus>, usize) {
    let worst = crate::status::worst(rows.iter().map(|r| r.led));
    let waiting = rows
        .iter()
        .map(|r| match r.kind {
            RowKind::Agent => usize::from(r.led == Some(AgentStatus::Waiting)),
            _ => r.badge,
        })
        .sum();
    (worst, waiting)
}

/// Push `parent` and, unless it is collapsed, `children`; a collapsed
/// parent carries their aggregate instead (R104.2).
fn push_folded(out: &mut Vec<Row>, mut parent: Row, children: Vec<Row>, collapsed: bool) {
    if collapsed {
        let (worst, waiting) = aggregate(&children);
        if parent.led.is_none() {
            parent.led = worst;
        }
        parent.badge = waiting;
        out.push(parent);
    } else {
        out.push(parent);
        out.extend(children);
    }
}

fn agents(panes: Vec<&Pane>) -> Vec<&Pane> {
    panes
        .into_iter()
        .filter(|p| p.agent_name().is_some())
        .collect()
}

/// The main Worktree's Panes of `s`, each with its canonical cwd.
fn main_panes<'s>(
    s: &'s Source,
    canon: &dyn Fn(&Path) -> PathBuf,
) -> Vec<(&'s Pane, Option<PathBuf>)> {
    s.worktrees
        .first()
        .into_iter()
        .flat_map(|w| w.tabs.iter().flat_map(|t| t.panes()))
        .map(|p| (p, pane_cwd(p).map(canon)))
        .collect()
}

/// The main-Worktree Panes of Source `s` that belong to link `l`: those
/// whose cwd is inside its checkout; for a link on the main checkout,
/// every one not inside another link's checkout (R104.1). `links` = all
/// links of the Workspace.
pub fn link_panes<'s>(
    s: &'s Source,
    l: &LinkView,
    links: &[LinkView],
    canon: &dyn Fn(&Path) -> PathBuf,
) -> Vec<&'s Pane> {
    let others: Vec<&PathBuf> = links
        .iter()
        .filter(|x| x.source == s.id && x.kind != Some(crate::checkout::Kind::Main))
        .filter_map(|x| x.path.as_ref())
        .collect();
    main_panes(s, canon)
        .into_iter()
        .filter(|(_, cwd)| {
            let inside = |root: &Path| cwd.as_deref().is_some_and(|c| c.starts_with(root));
            match (&l.kind, &l.path) {
                (Some(crate::checkout::Kind::Main), _) => !others.iter().any(|o| inside(o)),
                (_, Some(path)) => inside(path),
                _ => false,
            }
        })
        .map(|(p, _)| p)
        .collect()
}

/// Every Pane of Project `project`: its links' main-Worktree Panes and the
/// Panes of its Worktrees, each once.
pub fn project_panes<'s>(
    state: &'s State,
    project: &str,
    links: &[LinkView],
    canon: &dyn Fn(&Path) -> PathBuf,
) -> Vec<&'s Pane> {
    let mut out: Vec<&Pane> = Vec::new();
    for l in links.iter().filter(|l| l.project == project) {
        let Some(s) = state.source(&l.source) else {
            continue;
        };
        for p in link_panes(s, l, links, canon) {
            if !out.iter().any(|x| x.id == p.id) {
                out.push(p);
            }
        }
    }
    for s in &state.sources {
        for w in s.worktrees.iter().skip(1) {
            if w.project.as_deref() == Some(project)
                && links
                    .iter()
                    .any(|l| l.project == project && l.source == s.id)
            {
                out.extend(w.tabs.iter().flat_map(|t| t.panes()));
            }
        }
    }
    out
}

/// The shown Workspace's Navigator rows (R104.1–R104.2).
/// What turns the shown rows `old` into `new` (R111-c): `(start, removed,
/// inserted)` = replace `old[start..start + removed]` by
/// `new[start..start + inserted]`; the common head and tail stay as they
/// are, so a status change rebuilds one row instead of the whole tree.
/// `None` when nothing changed.
pub fn splice<T: PartialEq>(old: &[T], new: &[T]) -> Option<(usize, usize, usize)> {
    if old == new {
        return None;
    }
    let head = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let max_tail = old.len().min(new.len()) - head;
    let tail = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(max_tail)
        .take_while(|(a, b)| a == b)
        .count();
    Some((head, old.len() - head - tail, new.len() - head - tail))
}

pub fn build(state: &State, input: &Input) -> Vec<Row> {
    let ws = state.active_workspace().to_string();
    let collapsed = |key: &str| state.collapsed_projects.iter().any(|k| k == key);
    let sources: Vec<&Source> = state.visible_sources().collect();
    let source = |id: &str| sources.iter().copied().find(|s| s.id == id);
    // Checkouts of links that are not the main checkout, per Source.
    let mut out = Vec::new();
    if input.projects.is_empty() {
        out.push(Row::new(
            RowKind::Empty,
            0,
            "empty".into(),
            "No Projects yet".into(),
            Target::None,
        ));
    }
    for (i, p) in input.projects.iter().enumerate() {
        let pkey = format!("{ws}/{}", p.id);
        let links: Vec<&LinkView> = input.links.iter().filter(|l| l.project == p.id).collect();
        let mut prow = Row::new(
            RowKind::Project,
            0,
            pkey.clone(),
            p.name.clone(),
            Target::Project {
                project: p.id.clone(),
            },
        );
        prow.color = Some(i);
        prow.dim = links.is_empty();
        if let Some(e) = &p.error {
            prow.meta = "broken".into();
            prow.also_in = Some(e.clone());
        }
        let mut children = Vec::new();
        for l in &links {
            let Some(s) = source(&l.source) else { continue };
            let lkey = format!("{pkey}/{}", l.source);
            let mut lrow = Row::new(
                RowKind::Link,
                1,
                lkey.clone(),
                s.name.clone(),
                Target::Link {
                    project: p.id.clone(),
                    source: l.source.clone(),
                },
            );
            match &l.drift {
                Some(d) => {
                    lrow.drift = true;
                    lrow.meta = d.head.clone();
                }
                None => lrow.meta = l.base.clone(),
            }
            lrow.also_in = l.also_in_tooltip();
            let mut lchildren = Vec::new();
            for pane in link_panes(s, l, input.links, input.canon) {
                if pane.agent_name().is_some() {
                    lchildren.push(agent_row(pane, 2, input));
                }
            }
            for w in s.worktrees.iter().skip(1) {
                if w.project.as_deref() != Some(&p.id) {
                    continue;
                }
                let wkey = format!("{lkey}/{}", w.id);
                let mut wrow = Row::new(
                    RowKind::Worktree,
                    2,
                    wkey.clone(),
                    w.name.clone(),
                    Target::Worktree {
                        source: s.id.clone(),
                        worktree: w.id.clone(),
                    },
                );
                let (ahead, changed) = (input.worktree_facts)(&w.path);
                let mut meta = Vec::new();
                if ahead > 0 {
                    meta.push(format!("↑{ahead}"));
                }
                if changed > 0 {
                    meta.push(format!("●{changed}"));
                }
                wrow.meta = meta.join(" ");
                wrow.review = crate::review_loop::chip(w);
                let wchildren: Vec<Row> = agents(w.tabs.iter().flat_map(|t| t.panes()).collect())
                    .into_iter()
                    .map(|x| agent_row(x, 3, input))
                    .collect();
                let folded = collapsed(&wkey);
                if !wchildren.is_empty() {
                    wrow.collapsed = Some(folded);
                }
                push_folded(&mut lchildren, wrow, wchildren, folded);
            }
            let folded = collapsed(&lkey);
            lrow.collapsed = Some(folded);
            push_folded(&mut children, lrow, lchildren, folded);
        }
        let folded = collapsed(&pkey);
        if !links.is_empty() {
            prow.collapsed = Some(folded);
        }
        push_folded(&mut out, prow, children, folded);
    }
    scratch(input, &sources, &mut out);
    if let Some(sel) = input.selected {
        // The focused Pane's Agent row when it is shown (R104.6), else the
        // checkout it runs in; a Project home keeps its Project.
        let focused = state
            .active_tab()
            .map(|t| Target::Pane {
                pane: t.focused.clone(),
            })
            .filter(|_| !matches!(sel, Target::Project { .. }))
            .filter(|f| out.iter().any(|r| &r.target == f));
        let want = focused.as_ref().unwrap_or(sel);
        if let Some(r) = out.iter_mut().find(|r| &r.target == want) {
            r.selected = true;
        }
    }
    out
}

/// Main-Worktree Panes of `s` no link claims: none while a link uses the
/// main checkout, else those outside every link's checkout.
fn unclaimed<'s>(
    s: &'s Source,
    links: &[LinkView],
    canon: &dyn Fn(&Path) -> PathBuf,
) -> Vec<&'s Pane> {
    let mine = links.iter().filter(|l| l.source == s.id);
    if mine
        .clone()
        .any(|l| l.kind == Some(crate::checkout::Kind::Main))
    {
        return Vec::new();
    }
    let others: Vec<PathBuf> = mine.filter_map(|l| l.path.clone()).collect();
    main_panes(s, canon)
        .into_iter()
        .filter(|(_, cwd)| {
            !others
                .iter()
                .any(|o| cwd.as_deref().is_some_and(|c| c.starts_with(o)))
        })
        .map(|(p, _)| p)
        .collect()
}

/// The Panes "Remove from Scratch…" closes on Source `s`'s Scratch row
/// (R104.5): a linked Source's main-checkout Panes no link claims (closed,
/// the row goes away). None for the Workspace root, a Source no Project
/// links (its row stays: Remove Source…) or a link on the main checkout.
pub fn scratch_panes<'s>(
    s: &'s Source,
    links: &[LinkView],
    canon: &dyn Fn(&Path) -> PathBuf,
) -> Vec<&'s Pane> {
    if s.is_general() || !links.iter().any(|l| l.source == s.id) {
        return Vec::new();
    }
    unclaimed(s, links, canon)
}

/// The Scratch section (R104.1): the Workspace root, unlinked Sources,
/// unused main checkouts with Panes, Worktrees with no Project.
fn scratch(input: &Input, sources: &[&Source], out: &mut Vec<Row>) {
    let linked = |sid: &str| input.links.iter().any(|l| l.source == sid);
    let valid = |s: &Source, project: Option<&str>| {
        project.is_some_and(|id| input.projects.iter().any(|p| p.id == id && p.lists(&s.id)))
    };
    // The one Project linking the Source, when there is exactly one.
    let only_project = |sid: &str| {
        let mut it = input
            .projects
            .iter()
            .filter(|p| p.error.is_none() && p.lists(sid));
        match (it.next(), it.next()) {
            (Some(p), None) => Some(p.id.clone()),
            _ => None,
        }
    };
    out.push(Row::new(
        RowKind::Section,
        0,
        "scratch".into(),
        "Scratch".into(),
        Target::None,
    ));
    let main_row = |s: &Source, title: String, panes: Vec<&Pane>, out: &mut Vec<Row>| {
        let mut r = Row::new(
            RowKind::Scratch,
            0,
            format!("scratch/{}", s.id),
            title,
            Target::Scratch {
                source: s.id.clone(),
            },
        );
        if !panes.is_empty() {
            r.meta = panes.len().to_string();
        }
        out.push(r);
        out.extend(agents(panes).into_iter().map(|p| agent_row(p, 1, input)));
    };
    for s in sources.iter().filter(|s| s.is_general()) {
        main_row(
            s,
            tilde(&s.path, input.home),
            unclaimed(s, input.links, input.canon),
            out,
        );
    }
    for s in sources.iter().filter(|s| !s.is_general()) {
        let panes = unclaimed(s, input.links, input.canon);
        if !linked(&s.id) || !panes.is_empty() {
            main_row(s, s.name.clone(), panes, out);
        }
    }
    for s in sources.iter().filter(|s| !s.is_general()) {
        for w in s.worktrees.iter().skip(1) {
            if valid(s, w.project.as_deref()) {
                continue;
            }
            let mut r = Row::new(
                RowKind::Worktree,
                0,
                format!("scratch/{}/{}", s.id, w.id),
                w.name.clone(),
                Target::Worktree {
                    source: s.id.clone(),
                    worktree: w.id.clone(),
                },
            );
            r.review = crate::review_loop::chip(w);
            r.suggest = input
                .branches
                .current
                .get(&w.id)
                .and_then(|b| suggest(&s.id, b, input.projects, input.branches))
                .or_else(|| only_project(&s.id))
                .and_then(|id| {
                    input
                        .projects
                        .iter()
                        .find(|p| p.id == id)
                        .map(|p| (p.id.clone(), p.name.clone()))
                });
            if r.suggest.is_none() {
                r.meta = s.name.clone();
            }
            out.push(r);
            out.extend(
                agents(w.tabs.iter().flat_map(|t| t.panes()).collect())
                    .into_iter()
                    .map(|p| agent_row(p, 1, input)),
            );
        }
    }
}

/// The row a window state selects: Project home shown → its Project;
/// a linked Worktree → it; a main checkout → the link whose (not main)
/// checkout holds the focused Pane's cwd, else the first link (shown
/// order) on the main checkout, else its Scratch row (R104.5).
pub fn selection(
    state: &State,
    links: &[LinkView],
    home: Option<&str>,
    canon: &dyn Fn(&Path) -> PathBuf,
) -> Option<Target> {
    if let Some(p) = home {
        return Some(Target::Project { project: p.into() });
    }
    let (s, w) = state.active();
    if !state.is_main_worktree(&s.id, &w.id) {
        return Some(Target::Worktree {
            source: s.id.clone(),
            worktree: w.id.clone(),
        });
    }
    let cwd = state
        .active_tab()
        .and_then(|t| t.panes().into_iter().find(|p| p.id == t.focused))
        .and_then(pane_cwd)
        .map(canon);
    let mine = || links.iter().filter(|l| l.source == s.id);
    let held = mine().find(|l| {
        l.kind != Some(crate::checkout::Kind::Main)
            && l.path
                .as_ref()
                .zip(cwd.as_ref())
                .is_some_and(|(p, c)| c.starts_with(p))
    });
    Some(
        held.or_else(|| mine().find(|l| l.kind == Some(crate::checkout::Kind::Main)))
            .map_or(
                Target::Scratch {
                    source: s.id.clone(),
                },
                |l| Target::Link {
                    project: l.project.clone(),
                    source: l.source.clone(),
                },
            ),
    )
}

/// The rows as the golden text (R104-e), one line each.
pub fn text(rows: &[Row]) -> String {
    rows.iter().map(|r| r.text() + "\n").collect()
}

#[cfg(test)]
#[path = "navigator_tests.rs"]
pub(crate) mod tests;
