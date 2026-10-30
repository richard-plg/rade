//! The redesign's GTK-free decisions (PRD R20, M14): status words for
//! captions, sidebar rows and the Status Bar, the Agent rows under each
//! Worktree, which Panes get the Attention Ring, which newly waiting
//! Agents raise a Toast, and each Agent's glyph (R115).

use crate::agents::AgentStatus;
use crate::model::{State, Worktree};
use crate::notify::{Notification, Origin};
use crate::project::Project;
use std::collections::HashMap;

/// Branches the app reads from git: the dimmed branch of a main checkout,
/// the `→ Project?` suggestions and the one-time freeze (R30).
#[derive(Clone, Debug, Default)]
pub struct Branches {
    /// Current branch per Worktree id.
    pub current: HashMap<String, String>,
    /// The main branch per Source id (a link with no `base`), e.g. `main`.
    pub main: HashMap<String, String>,
}

/// One linked Worktree row of the sidebar: (source id, worktree id).
pub type WtRef = (String, String);

/// One group of the sidebar (R30.1, ADR-0028).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Group {
    /// A Project header and the Worktrees recorded for it.
    Project {
        /// `<workspace>/<project>`: the fold key.
        key: String,
        id: String,
        name: String,
        worktrees: Vec<WtRef>,
        collapsed: bool,
        error: Option<String>,
    },
    /// One row per Source of the Workspace: its main checkout, pinned.
    MainCheckouts { sources: Vec<String> },
    /// Linked Worktrees with no (valid) recorded Project, each with its
    /// suggested Project id.
    NoProject {
        worktrees: Vec<(String, String, Option<String>)>,
    },
    /// Without Projects: every Source with its Worktrees (unchanged, R30.2);
    /// with Projects: General alone, last.
    Sources { sources: Vec<String>, heading: bool },
}

/// The Source chips' text (R30.1): each name without the Workspace's
/// name and a separator in front (`globex-web` in Workspace `globex` →
/// `app`), when that leaves something.
pub fn chip_names(names: &[&str], workspace: &str) -> Vec<String> {
    names
        .iter()
        .map(|n| {
            let ws = workspace.to_lowercase();
            let rest = n
                .to_lowercase()
                .strip_prefix(&ws)
                .filter(|r| r.len() > 1 && r.starts_with(['-', '_', '.']))
                .map(|r| r.len() - 1);
            rest.and_then(|k| n.get(n.len() - k..))
                .unwrap_or(n)
                .to_string()
        })
        .collect()
}

/// The branches Project `p` names for `source` (R28.1), `origin/` kept.
fn base_branches<'a>(p: &'a Project, source: &str, br: &'a Branches) -> Vec<&'a String> {
    let Some(ps) = p.source(source) else {
        return Vec::new();
    };
    match &ps.base {
        None => br.main.get(source).into_iter().collect(),
        Some(b) => vec![b],
    }
}

/// `branch` is `b` (or `b` without `origin/`).
fn same_branch(b: &str, branch: &str) -> bool {
    b == branch || b.strip_prefix("origin/") == Some(branch)
}

/// The Project a Scratch Worktree on `branch` is likely for (R30.1,
/// R132.2): the one listing its Source whose base is the branch, or one of whose `/`-separated parts is the
/// Project id, alone or before `-` / `_` / `.`. Two Projects match → no
/// guess. A hint only: never applied by RADE.
pub fn suggest(source: &str, branch: &str, projects: &[Project], br: &Branches) -> Option<String> {
    let lower = branch.to_lowercase();
    let names_id = |id: &str| {
        let id = id.to_lowercase();
        lower.split('/').any(|part| {
            part.strip_prefix(id.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(['-', '_', '.']))
        })
    };
    let mut hits = projects
        .iter()
        .filter(|p| p.lists(source) && p.error.is_none())
        .filter(|p| {
            base_branches(p, source, br).iter().any(|b| {
                let b = b.strip_prefix("origin/").unwrap_or(b);
                !b.is_empty() && branch == b
            }) || names_id(&p.id)
        });
    let first = hits.next()?;
    hits.next().is_none().then(|| first.id.clone())
}

impl AgentStatus {
    /// The caption's status chip (R20.4).
    pub fn chip(self) -> &'static str {
        match self {
            AgentStatus::Running => "Running",
            AgentStatus::Waiting => "Needs input",
            AgentStatus::Done => "Done",
            AgentStatus::Error => "Failed",
            AgentStatus::Exited => "Exited",
            AgentStatus::Interrupted => "Interrupted",
        }
    }

    /// The same in running text: sidebar rows, Status Bar counts.
    pub fn words(self) -> &'static str {
        match self {
            AgentStatus::Waiting => "needs input",
            AgentStatus::Error => "failed",
            s => s.as_str(),
        }
    }
}

/// `38s`, `2m`, `1h`, `3d`: how long an Agent has been in its status.
pub fn age(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86400),
    }
}

/// One Agent row under its Worktree in the sidebar (R20.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentRow {
    pub pane: String,
    pub name: String,
    pub status: Option<AgentStatus>,
    /// Dim text on the right: the status in words, or, while running, the
    /// time since it started.
    pub meta: String,
}

impl State {
    /// The Agent Panes (a Preset) of a Worktree's Tabs, in Tab and Layout
    /// order (Title Bars last in their Tab).
    pub fn agent_rows(&self, source: &str, worktree: &str, now: u64) -> Vec<AgentRow> {
        let Some(w) = self.worktree(source, worktree) else {
            return Vec::new();
        };
        w.tabs
            .iter()
            .flat_map(|t| t.panes())
            .filter(|p| p.agent.is_some())
            .map(|p| AgentRow {
                pane: p.id.clone(),
                name: p.name.clone().unwrap_or_else(|| p.id.clone()),
                status: p.status,
                meta: match p.status {
                    Some(AgentStatus::Running) => p
                        .status_since
                        .map_or(String::new(), |s| age(now.saturating_sub(s))),
                    Some(s) => s.words().to_string(),
                    None => String::new(),
                },
            })
            .collect()
    }

    /// Agents per status over the shown Workspace's Sources (R23.4),
    /// worst first, none left out but zeros (the Status Bar, R20.9).
    pub fn status_counts(&self) -> Vec<(AgentStatus, usize)> {
        let mut all = AgentStatus::ALL;
        all.sort_by_key(|s| s.rank());
        let ws = self.active_workspace();
        let shown: Vec<&crate::Pane> = self
            .worktrees()
            .filter(|(s, _)| s.workspace == ws)
            .flat_map(|(_, w)| w.tabs.iter().flat_map(|t| t.panes()))
            .collect();
        all.into_iter()
            .map(|s| (s, shown.iter().filter(|p| p.status == Some(s)).count()))
            .filter(|(_, n)| *n > 0)
            .collect()
    }

    /// Fold or unfold a Source's sidebar rows (R1.6); the new state.
    pub fn toggle_collapsed(&mut self, source: &str) -> crate::rpc::Result<bool> {
        let id = self.find_source(source)?.id.clone();
        let p = self.source_mut(&id).unwrap();
        p.collapsed = !p.collapsed;
        Ok(p.collapsed)
    }

    /// The Project a Worktree is under (R30.1, ADR-0028): its recorded
    /// `project` when that Project lists its Source; main checkouts never.
    pub fn project_of_worktree<'a>(
        &self,
        source: &str,
        w: &Worktree,
        projects: &'a [Project],
    ) -> Option<&'a Project> {
        if self.is_main_worktree(source, &w.id) {
            return None;
        }
        let id = w.project.as_deref()?;
        projects.iter().find(|p| p.id == id && p.lists(source))
    }

    /// The shown Workspace's sidebar groups (R30.1): each Project with its
    /// recorded Worktrees, Main checkouts, No Project, then General; without
    /// Projects one plain group (unchanged, R30.2). `active`: Projects kept
    /// unfolded although empty: an empty Project (no Worktree, not active)
    /// shows folded unless unfolded.
    pub fn sidebar_groups(
        &self,
        projects: &[Project],
        active: &[String],
        br: &Branches,
    ) -> Vec<Group> {
        let ws = self.active_workspace().to_string();
        let visible: Vec<&crate::model::Source> = self.visible_sources().collect();
        if projects.is_empty() {
            return vec![Group::Sources {
                sources: visible.iter().map(|s| s.id.clone()).collect(),
                heading: false,
            }];
        }
        let mut out: Vec<Group> = projects
            .iter()
            .map(|p| {
                let key = format!("{ws}/{}", p.id);
                let worktrees: Vec<WtRef> = visible
                    .iter()
                    .flat_map(|s| s.worktrees.iter().map(move |w| (s, w)))
                    .filter(|(s, w)| {
                        self.project_of_worktree(&s.id, w, projects)
                            .is_some_and(|x| x.id == p.id)
                    })
                    .map(|(s, w)| (s.id.clone(), w.id.clone()))
                    .collect();
                let empty = worktrees.is_empty() && !active.contains(&p.id);
                Group::Project {
                    collapsed: self.collapsed_projects.contains(&key)
                        || (empty && !self.expanded_projects.contains(&key)),
                    key,
                    id: p.id.clone(),
                    name: p.name.clone(),
                    worktrees,
                    error: p.error.clone(),
                }
            })
            .collect();
        let (general, sources): (Vec<&crate::model::Source>, Vec<&crate::model::Source>) =
            visible.iter().partition(|s| s.is_general());
        out.push(Group::MainCheckouts {
            sources: sources.iter().map(|s| s.id.clone()).collect(),
        });
        let free: Vec<(String, String, Option<String>)> = sources
            .iter()
            .flat_map(|s| s.worktrees.iter().map(move |w| (s, w)))
            .filter(|(s, w)| {
                !self.is_main_worktree(&s.id, &w.id)
                    && self.project_of_worktree(&s.id, w, projects).is_none()
            })
            .map(|(s, w)| {
                let hint = br
                    .current
                    .get(&w.id)
                    .and_then(|b| suggest(&s.id, b, projects, br));
                (s.id.clone(), w.id.clone(), hint)
            })
            .collect();
        if !free.is_empty() {
            out.push(Group::NoProject { worktrees: free });
        }
        out.push(Group::Sources {
            sources: general.iter().map(|s| s.id.clone()).collect(),
            heading: false,
        });
        out
    }

    /// Source ids in sidebar order for `Alt+<n>` (R30.2): the Sources as
    /// listed (Main checkouts with Projects), General last.
    pub fn sidebar_order(&self) -> Vec<String> {
        let (general, mut rest): (Vec<String>, Vec<String>) = self
            .visible_sources()
            .map(|s| s.id.clone())
            .partition(|id| crate::model::is_general_id(id));
        rest.extend(general);
        rest
    }

    /// Worst Agent Status in a Project's Worktrees (its folded header's
    /// glyph, R30-c).
    pub fn project_status(&self, project: &str, projects: &[Project]) -> Option<AgentStatus> {
        crate::status::worst(
            self.project_worktrees(project, projects)
                .iter()
                .flat_map(|(s, w)| self.panes_in(s, w))
                .map(|p| p.status),
        )
    }

    /// The shown Workspace's Worktrees recorded for `project` (R30.1).
    pub fn project_worktrees(&self, project: &str, projects: &[Project]) -> Vec<WtRef> {
        self.visible_sources()
            .flat_map(|s| s.worktrees.iter().map(move |w| (s, w)))
            .filter(|(s, w)| {
                self.project_of_worktree(&s.id, w, projects)
                    .is_some_and(|p| p.id == project)
            })
            .map(|(s, w)| (s.id.clone(), w.id.clone()))
            .collect()
    }

    /// Fold or unfold a Project header (`<ws>/<project>`) shown `folded`
    /// now; the new state, kept in `state.json` (R30-b).
    pub fn toggle_project(&mut self, key: &str, folded: bool) -> bool {
        self.collapsed_projects.retain(|k| k != key);
        self.expanded_projects.retain(|k| k != key);
        if folded {
            self.expanded_projects.push(key.into());
        } else {
            self.collapsed_projects.push(key.into());
        }
        !folded
    }

    /// The one-time freeze of the 0.3.1 branch rule (R30.5, ADR-0028): each
    /// linked Worktree with no valid `project` that the old rule showed
    /// under exactly one Project of its Workspace (`projects_of`) gets it
    /// recorded; main checkouts never. Runs once (`sidebar_rule`); returns
    /// what it recorded as (source, worktree name, project).
    pub fn freeze_sidebar(
        &mut self,
        projects_of: &dyn Fn(&str) -> Vec<Project>,
        br: &Branches,
    ) -> Vec<(String, String, String)> {
        if self.sidebar_rule >= crate::model::SIDEBAR_RULE {
            return Vec::new();
        }
        let mut cache: HashMap<String, Vec<Project>> = HashMap::new();
        let mut set = Vec::new();
        for s in &self.sources {
            let projects = cache
                .entry(s.workspace.clone())
                .or_insert_with(|| projects_of(&s.workspace));
            for w in s.worktrees.iter().skip(1) {
                if w.project
                    .as_deref()
                    .is_some_and(|id| projects.iter().any(|p| p.id == id && p.lists(&s.id)))
                {
                    continue;
                }
                let Some(cur) = br.current.get(&w.id) else {
                    continue;
                };
                let under: Vec<&Project> = projects
                    .iter()
                    .filter(|p| {
                        base_branches(p, &s.id, br)
                            .iter()
                            .any(|b| same_branch(b, cur))
                    })
                    .collect();
                if let [p] = under[..] {
                    set.push((s.id.clone(), w.id.clone(), w.name.clone(), p.id.clone()));
                }
            }
        }
        for (s, w, _, p) in &set {
            if let Some(x) = self
                .source_mut(s)
                .and_then(|s| s.worktrees.iter_mut().find(|x| &x.id == w))
            {
                x.project = Some(p.clone());
            }
        }
        self.sidebar_rule = crate::model::SIDEBAR_RULE;
        set.into_iter().map(|(s, _, n, p)| (s, n, p)).collect()
    }

    /// Panes that get the Attention Ring: their Agent is `waiting` (R20.3).
    pub fn waiting_panes(&self) -> Vec<String> {
        self.all_panes()
            .filter(|p| p.status == Some(AgentStatus::Waiting))
            .map(|p| p.id.clone())
            .collect()
    }
}

/// Agents that started waiting since `before` and whose Pane is not on
/// screen: each raises a Toast (R20.9).
pub fn toasts(before: &[String], ws: &State) -> Vec<String> {
    ws.waiting_panes()
        .into_iter()
        .filter(|p| !before.contains(p) && !ws.pane_on_screen(p))
        .collect()
}

/// The mark of an agent, drawn as its glyph coloured by its Agent Status
/// (R115, ADR-0038): the agent's logo, a robot for any other Preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMark {
    Claude,
    Codex,
    Antigravity,
    OpenCode,
    /// Any other Preset.
    Other,
}

impl AgentMark {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
            Self::OpenCode => "opencode",
            Self::Other => "other",
        }
    }

    /// Its bundled icon (spec README "Icons"): the agent's own logo
    /// (ADR-0038 amendment 2), Phosphor Bold `robot` for any other.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Claude => "agent-claude-symbolic",
            Self::Codex => "agent-codex-symbolic",
            Self::Antigravity => "agent-antigravity-symbolic",
            Self::OpenCode => "agent-opencode-symbolic",
            Self::Other => "ph-robot-bold-symbolic",
        }
    }

    /// A Preset name or a program (`/usr/bin/claude`, `claude-sonnet`).
    fn of_word(word: &str) -> Option<Self> {
        let w = word.rsplit('/').next().unwrap_or(word).to_ascii_lowercase();
        [
            ("claude", Self::Claude),
            ("codex", Self::Codex),
            ("agy", Self::Antigravity),
            ("antigravity", Self::Antigravity),
            ("opencode", Self::OpenCode),
        ]
        .into_iter()
        .find(|(p, _)| w.starts_with(p))
        .map(|(_, m)| m)
    }
}

impl From<crate::account::AgentKind> for AgentMark {
    fn from(k: crate::account::AgentKind) -> Self {
        match k {
            crate::account::AgentKind::Claude => Self::Claude,
            crate::account::AgentKind::Codex => Self::Codex,
        }
    }
}

/// The glyph of Preset `preset` by its name (R115-a).
pub fn agent_glyph(preset: &str) -> AgentMark {
    AgentMark::of_word(preset).unwrap_or(AgentMark::Other)
}

impl crate::layout::Pane {
    /// Its Agent Preset: set at spawn or by a hook, else the one whose
    /// program runs in its foreground (R115.4).
    pub fn agent_name(&self) -> Option<&str> {
        self.agent.as_deref().or(self.detected.as_deref())
    }

    /// Its Agent's glyph (R115): by the Preset's name, else by the program
    /// its command or recorded invocation runs; `None` for a shell.
    pub fn mark(&self) -> Option<AgentMark> {
        let preset = self.agent_name()?;
        let program = |cmd: &str| {
            cmd.split_whitespace()
                .find(|w| !w.contains('='))
                .and_then(AgentMark::of_word)
        };
        AgentMark::of_word(preset)
            .or_else(|| match &self.kind {
                crate::PaneKind::Terminal {
                    command: Some(c), ..
                } => program(c),
                _ => None,
            })
            .or_else(|| {
                self.invocation
                    .as_ref()
                    .and_then(|i| i.argv.first())
                    .and_then(|a| AgentMark::of_word(a))
            })
            .or(Some(AgentMark::Other))
    }
}

impl Notification {
    /// The Agent Status event behind it, for its Inbox glyph (R3.2).
    pub fn status_kind(&self) -> Option<AgentStatus> {
        if !matches!(self.source, Origin::Status | Origin::Merged) {
            return None;
        }
        let first = self.text.split(" — ").next().unwrap_or("");
        [
            (" needs input", AgentStatus::Waiting),
            (" finished", AgentStatus::Done),
            (" failed", AgentStatus::Error),
        ]
        .into_iter()
        .find(|(end, _)| first.ends_with(end))
        .map(|(_, s)| s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn r115_a_each_agent_gets_its_glyph_by_preset_then_command() {
        assert_eq!(agent_glyph("claude-sonnet"), AgentMark::Claude);
        assert_eq!(agent_glyph("codex"), AgentMark::Codex);
        assert_eq!(agent_glyph("agy"), AgentMark::Antigravity);
        assert_eq!(agent_glyph("opencode"), AgentMark::OpenCode);
        assert_eq!(agent_glyph("reviewer"), AgentMark::Other);
        // A Pane: its Preset, else the program it runs, else none (a shell).
        let mut w = State::new(Path::new("/home/u"));
        let (p, wt) = {
            let (p, wt) = w.active();
            (p.id.clone(), wt.id.clone())
        };
        let (_, a) = w.new_tab(&p, &wt, None, None).unwrap();
        assert_eq!(w.pane(&a).unwrap().mark(), None);
        let mut pane = w.pane(&a).unwrap().clone();
        pane.agent = Some("reviewer".into());
        assert_eq!(pane.mark(), Some(AgentMark::Other));
        if let crate::PaneKind::Terminal { command, .. } = &mut pane.kind {
            *command = Some("FOO=1 /usr/bin/codex --model o3".into());
        }
        assert_eq!(pane.mark(), Some(AgentMark::Codex));
        assert_eq!(AgentMark::Codex.icon(), "agent-codex-symbolic");
        assert_eq!(
            AgentMark::from(crate::account::AgentKind::Claude),
            AgentMark::Claude
        );
    }

    fn ws_with_agents() -> (State, String, String, Vec<String>) {
        let mut w = State::new(Path::new("/home/u"));
        let (p, wt) = {
            let (p, wt) = w.active();
            (p.id.clone(), wt.id.clone())
        };
        let (_, a) = w.new_tab(&p, &wt, None, Some("lead".into())).unwrap();
        let b = w
            .split(&a, crate::SplitDir::Right, None, Some("rev".into()))
            .unwrap();
        let shell = w.split(&a, crate::SplitDir::Down, None, None).unwrap();
        for (x, agent) in [(&a, "claude"), (&b, "codex")] {
            w.set_status(x, None, None, Some(agent.into()), 100)
                .unwrap();
        }
        (w, p, wt, vec![a, b, shell])
    }

    #[test]
    fn words_and_chips_name_every_status() {
        assert_eq!(AgentStatus::Waiting.chip(), "Needs input");
        assert_eq!(AgentStatus::Error.chip(), "Failed");
        assert_eq!(AgentStatus::Waiting.words(), "needs input");
        assert_eq!(AgentStatus::Error.words(), "failed");
        assert_eq!(AgentStatus::Running.words(), "running");
        for s in AgentStatus::ALL {
            assert!(!s.chip().is_empty() && !s.words().is_empty());
        }
    }

    #[test]
    fn collapsing_a_source_is_kept_in_state() {
        let (mut w, p, _, _) = ws_with_agents();
        assert!(w.toggle_collapsed(&p).unwrap());
        let back: State = serde_json::from_str(&serde_json::to_string(&w).unwrap()).unwrap();
        assert!(back.source(&p).unwrap().collapsed);
        assert!(!w.toggle_collapsed(&p).unwrap());
        assert!(w.toggle_collapsed("nope").is_err());
    }

    /// The owner's shape (v0.7.1:docs/design/simplify-proposal.md §1): Workspace
    /// globex, 4 Sources (support-bot paused), Projects BEACON, ATLAS, Globex
    /// App Main and Forecast, main checkouts on unrelated branches, two
    /// Worktrees with no Project. Returns (state, projects, branches).
    fn owner() -> (State, Vec<Project>, Branches) {
        let mut w = State::new(Path::new("/home/u"));
        for s in [
            "/w/globex-web",
            "/w/globex-api",
            "/w/globex-billing",
            "/w/support-bot",
        ] {
            w.add_source(Path::new(s), None).unwrap();
        }
        w.source_mut("support-bot").unwrap().paused = true;
        let proj = |id: &str, text: &str| Project::parse(id, Path::new("/f"), text);
        let two = |dev: &str, prod: &str| {
            format!("[[source]]\nid = \"globex-web\"\nbase = {{ {dev}{prod} }}\n[[source]]\nid = \"globex-api\"\nbase = {{ {dev}{prod} }}")
        };
        let projects = vec![
            proj("beacon", &format!("name = \"BEACON\"\nbases = [\"dev\", \"prod\"]\n{}", two("dev = \"feat/dev/beacon\", ", "prod = \"feat/beacon\""))),
            proj("atlas", &format!("name = \"ATLAS\"\nbases = [\"prod\"]\n{}", two("", "prod = \"feat/atlas\""))),
            proj("globex-web-main", &format!("name = \"Globex Web Main\"\nbases = [\"main\"]\n{}", two("", "main = \"master\""))),
            proj("forecast", "name = \"Forecast\"\nbases = [\"main\"]\n[[source]]\nid = \"globex-billing\"\nbase = { main = \"main\" }"),
        ];
        assert!(projects.iter().all(|p| p.error.is_none()), "{projects:?}");
        let mut br = Branches::default();
        for (s, b) in [
            ("globex-web", "feat/atlas"),
            ("globex-api", "fix/labels-order"),
            ("globex-billing", "main"),
            ("support-bot", "main"),
        ] {
            br.current
                .insert(w.source(s).unwrap().worktrees[0].id.clone(), b.into());
        }
        for (src, name, branch, p) in [
            (
                "globex-web",
                "beacon-dev",
                "feat/dev/beacon",
                Some("beacon"),
            ),
            (
                "globex-api",
                "beacon-dev",
                "feat/dev/beacon",
                Some("beacon"),
            ),
            ("globex-web", "atlas-dev", "feat/atlas-dev", Some("atlas")),
            ("globex-api", "atlas-dev", "feat/atlas-dev", Some("atlas")),
            ("globex-api", "master", "master", Some("globex-web-main")),
            ("globex-web", "agent-chat", "feat/beacon-agent-chat", None),
            (
                "globex-api",
                "ocr",
                "fix/ocr-llm-reasoning-effort-400",
                None,
            ),
        ] {
            let (s, id) = w
                .add_worktree(src, name, Path::new(&format!("/w/x/{src}-{name}")), None)
                .unwrap();
            w.set_worktree_origin(&s, &id, p.map(String::from), None)
                .unwrap();
            br.current.insert(id, branch.into());
        }
        (w, projects, br)
    }

    fn names(w: &State, wts: &[WtRef]) -> Vec<String> {
        wts.iter()
            .map(|(s, id)| {
                let wt = w.worktree(s, id).unwrap();
                format!("{s}:{}", wt.name)
            })
            .collect()
    }

    #[test]
    fn r30_owner_shape_groups_by_recorded_project_only() {
        let (w, projects, br) = owner();
        let g = w.sidebar_groups(&projects, &[], &br);
        assert_eq!(g.len(), 7, "{g:#?}");
        let Group::Project {
            id,
            name,
            worktrees,
            collapsed,
            ..
        } = &g[0]
        else {
            panic!()
        };
        assert_eq!(
            (id.as_str(), name.as_str(), *collapsed),
            ("beacon", "BEACON", false)
        );
        assert_eq!(
            names(&w, worktrees),
            ["globex-web:beacon-dev", "globex-api:beacon-dev"]
        );
        // globex-web's main checkout is on feat/atlas: not under ATLAS any more.
        let Group::Project { worktrees, .. } = &g[1] else {
            panic!()
        };
        assert_eq!(
            names(&w, worktrees),
            ["globex-web:atlas-dev", "globex-api:atlas-dev"]
        );
        let Group::Project { worktrees, .. } = &g[2] else {
            panic!()
        };
        assert_eq!(names(&w, worktrees), ["globex-api:master"]);
        // Forecast: nothing recorded, no work → folded ("no work yet").
        assert!(
            matches!(&g[3], Group::Project { worktrees, collapsed: true, .. } if worktrees.is_empty())
        );
        assert_eq!(
            g[4],
            Group::MainCheckouts {
                sources: vec![
                    "globex-web".into(),
                    "globex-api".into(),
                    "globex-billing".into(),
                    "support-bot".into()
                ]
            }
        );
        let Group::NoProject { worktrees } = &g[5] else {
            panic!()
        };
        let free: Vec<(String, Option<String>)> = worktrees
            .iter()
            .map(|(s, id, hint)| {
                (
                    format!("{s}:{}", w.worktree(s, id).unwrap().name),
                    hint.clone(),
                )
            })
            .collect();
        assert_eq!(
            free,
            [
                (
                    "globex-web:agent-chat".to_string(),
                    Some("beacon".to_string())
                ),
                ("globex-api:ocr".to_string(), None)
            ]
        );
        assert_eq!(
            g[6],
            Group::Sources {
                sources: vec![crate::model::general_id("default")],
                heading: false
            }
        );
        // An active Project is not folded by default.
        let g = w.sidebar_groups(&projects, &["forecast".into()], &br);
        assert!(matches!(
            &g[3],
            Group::Project {
                collapsed: false,
                ..
            }
        ));
        // Alt+n: the Sources' order, General last.
        assert_eq!(
            w.sidebar_order(),
            [
                "globex-web",
                "globex-api",
                "globex-billing",
                "support-bot",
                "general"
            ]
        );
    }

    #[test]
    fn r30_f_a_branch_switch_moves_nothing_and_a_move_does() {
        let (mut w, projects, mut br) = owner();
        let before = w.sidebar_groups(&projects, &[], &br);
        // Main checkout to BEACON's base, a free Worktree to ATLAS's.
        let main = w.source("globex-web").unwrap().worktrees[0].id.clone();
        br.current.insert(main, "feat/dev/beacon".into());
        let ocr = w
            .source("globex-api")
            .unwrap()
            .worktrees
            .last()
            .unwrap()
            .id
            .clone();
        br.current.insert(ocr.clone(), "feat/atlas".into());
        let after = w.sidebar_groups(&projects, &[], &br);
        let strip = |g: &[Group]| -> Vec<Group> {
            g.iter()
                .map(|g| match g {
                    Group::NoProject { worktrees } => Group::NoProject {
                        worktrees: worktrees
                            .iter()
                            .map(|(s, w, _)| (s.clone(), w.clone(), None))
                            .collect(),
                    },
                    g => g.clone(),
                })
                .collect()
        };
        assert_eq!(
            strip(&before),
            strip(&after),
            "only suggestions follow branches"
        );
        assert!(
            matches!(&after[5], Group::NoProject { worktrees } if worktrees[1].2.as_deref() == Some("atlas"))
        );
        // Moving it (what a suggestion click does) puts it under ATLAS.
        w.set_worktree_project("globex-api", &ocr, Some("atlas".into()))
            .unwrap();
        let g = w.sidebar_groups(&projects, &[], &br);
        let Group::Project { worktrees, .. } = &g[1] else {
            panic!()
        };
        assert!(worktrees.iter().any(|x| x.1 == ocr));
        assert_eq!(w.project_worktrees("atlas", &projects).len(), 3);
        // A main checkout with a recorded project is still a main checkout.
        let main = w.source("globex-billing").unwrap().worktrees[0].clone();
        assert!(w
            .project_of_worktree("globex-billing", &main, &projects)
            .is_none());
    }

    #[test]
    fn r30_5_freeze_records_the_old_rule_once() {
        let (mut w, projects, mut br) = owner();
        // A 0.5.1 state: nothing recorded on the Worktrees made by hand.
        w.sidebar_rule = 0;
        for s in ["globex-web", "globex-api"] {
            for x in w.source_mut(s).unwrap().worktrees.iter_mut() {
                x.project = None;
            }
        }
        // One under two Projects by branch (dev of BEACON = main of App Main).
        let (s, both) = w
            .add_worktree("globex-web", "shared", Path::new("/w/x/shared"), None)
            .unwrap();
        br.current.insert(both.clone(), "feat/x".into());
        let p2 = vec![
            projects[0].clone(),
            Project::parse(
                "other",
                Path::new("/f"),
                "[[source]]\nid = \"globex-web\"\nbase = \"feat/x\"",
            ),
            Project::parse(
                "other2",
                Path::new("/f"),
                "[[source]]\nid = \"globex-web\"\nbase = \"feat/x\"",
            ),
            projects[1].clone(),
            projects[2].clone(),
            projects[3].clone(),
        ];
        let set = w.freeze_sidebar(
            &|ws| {
                if ws == "default" {
                    p2.clone()
                } else {
                    Vec::new()
                }
            },
            &br,
        );
        let mut got: Vec<String> = set.iter().map(|(s, n, p)| format!("{s}:{n}={p}")).collect();
        got.sort();
        assert_eq!(
            got,
            [
                "globex-api:beacon-dev=beacon",
                "globex-api:master=globex-web-main",
                "globex-web:beacon-dev=beacon"
            ],
            "atlas-dev is on feat/atlas-dev (no base), agent-chat on no base, shared under two"
        );
        assert!(w.worktree(&s, &both).unwrap().project.is_none());
        assert!(
            w.source("globex-web").unwrap().worktrees[0]
                .project
                .is_none(),
            "main checkout"
        );
        assert_eq!(w.sidebar_rule, crate::model::SIDEBAR_RULE);
        // Once only; kept in state.json.
        let back: State = serde_json::from_str(&serde_json::to_string(&w).unwrap()).unwrap();
        assert_eq!(back.sidebar_rule, 2);
        assert!(w.freeze_sidebar(&|_| p2.clone(), &br).is_empty());
        // A new state never freezes.
        let mut fresh = State::new(Path::new("/home/u"));
        assert!(fresh.freeze_sidebar(&|_| p2.clone(), &br).is_empty());
        // An old state.json without the key reads as 0.
        let mut v = serde_json::to_value(&fresh).unwrap();
        v.as_object_mut().unwrap().remove("sidebar_rule");
        let old: State = serde_json::from_value(v).unwrap();
        assert_eq!(old.sidebar_rule, 0);
    }

    #[test]
    fn r30_b_fold_and_unfold_are_kept() {
        let (mut w, projects, br) = owner();
        assert!(w.toggle_project("default/beacon", false));
        assert!(
            !w.toggle_project("default/forecast", true),
            "unfold a folded-by-default one"
        );
        let back: State = serde_json::from_str(&serde_json::to_string(&w).unwrap()).unwrap();
        let g = back.sidebar_groups(&projects, &[], &br);
        assert!(matches!(
            &g[0],
            Group::Project {
                collapsed: true,
                ..
            }
        ));
        assert!(matches!(
            &g[3],
            Group::Project {
                collapsed: false,
                ..
            }
        ));
        assert!(!w.toggle_project("default/beacon", true));
        assert!(!w.collapsed_projects.contains(&"default/beacon".to_string()));
    }

    #[test]
    fn r30_chips_and_suggestions() {
        assert_eq!(
            chip_names(&["globex-web", "globex-api", "support-bot"], "globex"),
            ["web", "api", "support-bot"]
        );
        assert_eq!(chip_names(&["api", "app"], "acme"), ["api", "app"]);
        assert_eq!(
            chip_names(&["x-", "Globex_Web", "globexweb"], "Globex"),
            ["x-", "Web", "globexweb"],
            "never empty, a separator only"
        );
        let (_, projects, br) = owner();
        let s = |src: &str, b: &str| suggest(src, b, &projects, &br);
        assert_eq!(
            s("globex-web", "feat/beacon-agent-chat").as_deref(),
            Some("beacon")
        );
        assert_eq!(s("globex-web", "feat/atlas-dev").as_deref(), Some("atlas"));
        assert_eq!(s("globex-web", "fix/ocr"), None);
        assert_eq!(
            s("globex-billing", "feat/beacon-x"),
            None,
            "BEACON doesn't list it"
        );
        assert_eq!(
            s("globex-api", "master").as_deref(),
            Some("globex-web-main")
        );
    }

    #[test]
    fn r132_a_suggestions_need_a_real_match() {
        let (_, projects, br) = owner();
        let s = |src: &str, b: &str| suggest(src, b, &projects, &br);
        // A base is a whole branch name, not a prefix.
        assert_eq!(s("globex-api", "masterclass"), None);
        assert_eq!(s("globex-api", "master-old"), None);
        // A Project id is a whole part, or a part's word before `-` / `_` / `.`.
        assert_eq!(s("globex-web", "feat/beaconing"), None);
        assert_eq!(s("globex-web", "beacon").as_deref(), Some("beacon"));
        assert_eq!(s("globex-web", "feat/beacon_x").as_deref(), Some("beacon"));
        // Two Projects match: no guess.
        assert_eq!(s("globex-web", "beacon/atlas-x"), None);
    }

    #[test]
    fn r30_c_project_status_is_the_worst_of_its_worktrees() {
        let (mut w, projects, _) = owner();
        assert_eq!(w.project_status("beacon", &projects), None);
        let (s, wt) = w.project_worktrees("beacon", &projects)[0].clone();
        let (_, pane) = w.new_tab(&s, &wt, None, Some("a".into())).unwrap();
        w.set_status(
            &pane,
            Some(AgentStatus::Waiting),
            None,
            Some("claude".into()),
            1,
        )
        .unwrap();
        assert_eq!(
            w.project_status("beacon", &projects),
            Some(AgentStatus::Waiting)
        );
        assert_eq!(w.project_status("atlas", &projects), None);
    }

    #[test]
    fn age_is_short() {
        assert_eq!(age(0), "0s");
        assert_eq!(age(38), "38s");
        assert_eq!(age(150), "2m");
        assert_eq!(age(7200), "2h");
        assert_eq!(age(3 * 86400 + 5), "3d");
    }

    #[test]
    fn agent_rows_list_agents_only_with_status_words_or_age() {
        let (mut w, p, wt, panes) = ws_with_agents();
        let rows = w.agent_rows(&p, &wt, 200);
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["lead", "rev"],
            "the plain shell is no Agent"
        );
        assert!(rows.iter().all(|r| r.meta.is_empty() && r.status.is_none()));
        w.set_status(&panes[0], Some(AgentStatus::Running), None, None, 80)
            .unwrap();
        w.set_status(&panes[1], Some(AgentStatus::Waiting), None, None, 190)
            .unwrap();
        let rows = w.agent_rows(&p, &wt, 200);
        assert_eq!(rows[0].meta, "2m");
        assert_eq!(rows[0].status, Some(AgentStatus::Running));
        assert_eq!(rows[1].meta, "needs input");
        assert_eq!(rows[1].pane, panes[1]);
        assert!(w.agent_rows(&p, "nope", 200).is_empty());
    }

    #[test]
    fn status_counts_worst_first_without_zeros() {
        let (mut w, _, _, panes) = ws_with_agents();
        assert!(w.status_counts().is_empty());
        w.set_status(&panes[0], Some(AgentStatus::Running), None, None, 1)
            .unwrap();
        w.set_status(&panes[1], Some(AgentStatus::Waiting), None, None, 1)
            .unwrap();
        assert_eq!(
            w.status_counts(),
            [(AgentStatus::Waiting, 1), (AgentStatus::Running, 1)]
        );
    }

    #[test]
    fn ring_and_toast_only_for_newly_waiting_off_screen() {
        let (mut w, p, wt, panes) = ws_with_agents();
        w.set_status(&panes[1], Some(AgentStatus::Waiting), None, None, 1)
            .unwrap();
        assert_eq!(w.waiting_panes(), [panes[1].clone()]);
        // On screen (its Tab is shown): ring, no Toast.
        assert!(toasts(&[], &w).is_empty());
        // Another Tab in front: a Toast, once.
        let (t2, _) = w.new_tab(&p, &wt, None, None).unwrap();
        w.select_tab(&t2).unwrap();
        assert_eq!(toasts(&[], &w), [panes[1].clone()]);
        assert!(toasts(&w.waiting_panes(), &w).is_empty());
    }

    #[test]
    fn notification_status_kind_from_status_events_only() {
        let n = |text: &str, source| Notification {
            id: 1,
            pane: "p_1".into(),
            title: None,
            text: text.into(),
            time: 0,
            read: false,
            path: String::new(),
            source,
        };
        assert_eq!(
            n(
                "rev needs input — Claude needs your permission",
                Origin::Merged
            )
            .status_kind(),
            Some(AgentStatus::Waiting)
        );
        assert_eq!(
            n("rev finished", Origin::Status).status_kind(),
            Some(AgentStatus::Done)
        );
        assert_eq!(
            n("rev failed", Origin::Status).status_kind(),
            Some(AgentStatus::Error)
        );
        assert_eq!(n("build finished", Origin::Message).status_kind(), None);
        assert_eq!(n("hello", Origin::Status).status_kind(), None);
    }
}
