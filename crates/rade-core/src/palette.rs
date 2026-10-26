//! Command palette (PRD R14, R107.3): what it lists (Source links,
//! Projects, Worktrees, Panes by name, Tabs, unlinked Sources, app and link
//! actions, files of the active Worktree; no absolute paths, Pane ids or
//! General in titles), how a query picks a
//! category (prefix) and ranks items (fuzzy, `nucleo-matcher`), recently
//! used first, and the inline sub-forms ([`crate::palette_form`]). GTK-free:
//! the app renders it and runs the chosen [`Target`].

use crate::linkview::LinkView;
use crate::model::State;
use crate::project::Project;
use crate::PaneKind;
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32String};
use serde::Serialize;
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

/// Item categories, in the order the empty palette lists them (and the
/// tie-break order for equal scores).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Pane,
    Tab,
    /// A Project (R107.3): Enter shows its home.
    Project,
    /// A Source link `<source> · <Project> · <base>` (R107.3).
    Link,
    Worktree,
    Source,
    Action,
    File,
    /// A symbol of the Worktree (`$`, R92): from its language servers.
    Symbol,
}

/// A category filter, chosen by the query's first character (VS Code's
/// `>` for commands, the rest RADE's own) or cycled with Tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Filter {
    All,
    Actions,
    Sources,
    Panes,
    Files,
    Symbols,
}

/// In the order of the scope chips (spec D.8).
pub const FILTERS: [Filter; 6] = [
    Filter::All,
    Filter::Actions,
    Filter::Panes,
    Filter::Sources,
    Filter::Files,
    Filter::Symbols,
];

/// The section a kind's rows are listed under (spec D.10), in list order.
pub const SECTIONS: [&str; 8] = [
    "PROJECTS",
    "SOURCES",
    "WORKTREES",
    "AGENTS",
    "TABS",
    "ACTIONS",
    "FILES",
    "SYMBOLS",
];

/// The section label of `k`'s rows.
pub fn section(k: Kind) -> &'static str {
    match k {
        Kind::Project => "PROJECTS",
        Kind::Link | Kind::Source => "SOURCES",
        Kind::Worktree => "WORKTREES",
        Kind::Pane => "AGENTS",
        Kind::Tab => "TABS",
        Kind::Action => "ACTIONS",
        Kind::File => "FILES",
        Kind::Symbol => "SYMBOLS",
    }
}

fn section_rank(k: Kind) -> usize {
    SECTIONS.iter().position(|s| *s == section(k)).unwrap_or(0)
}

impl Filter {
    pub fn prefix(self) -> Option<char> {
        match self {
            Filter::All => None,
            Filter::Actions => Some('>'),
            Filter::Sources => Some('/'),
            Filter::Panes => Some('@'),
            Filter::Files => Some('#'),
            Filter::Symbols => Some('$'),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Actions => "Actions",
            Filter::Sources => "Sources",
            Filter::Panes => "Agents",
            Filter::Files => "Files",
            Filter::Symbols => "Symbols",
        }
    }

    /// The scope chip's text (spec D.8): `> Actions`, `All`.
    pub fn chip(self) -> String {
        match self.prefix() {
            Some(c) => format!("{c} {}", self.label()),
            None => self.label().to_string(),
        }
    }

    pub fn admits(self, k: Kind) -> bool {
        match self {
            Filter::All => k != Kind::Symbol,
            Filter::Symbols => k == Kind::Symbol,
            Filter::Actions => k == Kind::Action,
            Filter::Sources => matches!(
                k,
                Kind::Source | Kind::Worktree | Kind::Link | Kind::Project
            ),
            Filter::Panes => matches!(k, Kind::Pane | Kind::Tab),
            Filter::Files => k == Kind::File,
        }
    }

    /// The filter `delta` steps along [`FILTERS`], wrapping (Tab / Shift+Tab).
    pub fn cycle(self, delta: i32) -> Filter {
        let i = FILTERS.iter().position(|f| *f == self).unwrap_or(0) as i32;
        FILTERS[(i + delta).rem_euclid(FILTERS.len() as i32) as usize]
    }
}

/// Split a query into its filter (a leading prefix character) and the
/// search text (trimmed).
pub fn parse_query(q: &str) -> (Filter, &str) {
    let q = q.trim_start();
    let first = q.chars().next();
    match FILTERS
        .iter()
        .find(|f| f.prefix().is_some() && f.prefix() == first)
    {
        Some(f) => (*f, q[1..].trim()),
        None => (Filter::All, q.trim()),
    }
}

/// `q` with its filter replaced by `f`, keeping the search text.
pub fn with_filter(q: &str, f: Filter) -> String {
    let (_, text) = parse_query(q);
    match f.prefix() {
        Some(c) => format!("{c}{text}"),
        None => text.to_string(),
    }
}

/// What Enter runs or opens.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Target {
    Action { id: String },
    Project { project: String },
    Link { project: String, source: String },
    Source { source: String },
    Worktree { source: String, worktree: String },
    Tab { tab: String },
    Pane { pane: String },
    File { path: PathBuf },
    Symbol { path: PathBuf, line: u32, col: u32 },
}

impl Target {
    /// The selected row's `Enter …` hint words (spec D.15): what Enter
    /// does with it (palette_run).
    pub fn enter_hint(&self) -> &'static str {
        match self {
            Target::Link { .. } => "open terminal",
            Target::Project { .. } | Target::File { .. } => "open",
            Target::Source { .. } | Target::Worktree { .. } | Target::Tab { .. } => "switch",
            Target::Pane { .. } => "focus",
            Target::Symbol { .. } => "go to",
            Target::Action { .. } => "run",
        }
    }

    /// Stable key for the recently-used list.
    pub fn key(&self) -> String {
        match self {
            Target::Action { id } => format!("action:{id}"),
            Target::Project { project } => format!("project:{project}"),
            Target::Link { project, source } => format!("link:{project}/{source}"),
            Target::Source { source } => format!("source:{source}"),
            Target::Worktree { source, worktree } => format!("worktree:{source}/{worktree}"),
            Target::Tab { tab } => format!("tab:{tab}"),
            Target::Pane { pane } => format!("pane:{pane}"),
            Target::File { path } => format!("file:{}", path.display()),
            Target::Symbol { path, line, col } => {
                format!("symbol:{}:{line}:{col}", path.display())
            }
        }
    }
}

/// One palette row.
#[derive(Clone, Debug)]
pub struct Item {
    pub kind: Kind,
    pub target: Target,
    pub title: String,
    pub detail: String,
    /// Keybinding label (`Ctrl+Shift+E`).
    pub keys: Option<String>,
    /// Folded text the query matches: title + detail (files: relative path).
    hay: Utf32String,
    /// Where the title starts in `hay`, in chars (files: after the folder).
    title_at: u32,
}

impl Item {
    pub fn new(
        kind: Kind,
        target: Target,
        title: &str,
        detail: &str,
        keys: Option<String>,
    ) -> Self {
        Self {
            hay: fold(&format!("{title}  {detail}")).as_str().into(),
            title_at: 0,
            kind,
            target,
            title: title.into(),
            detail: detail.into(),
            keys,
        }
    }

    fn title_slice(&self) -> nucleo_matcher::Utf32Str<'_> {
        let end = self.title_at as usize + self.title.chars().count();
        self.hay.slice(self.title_at as usize..end)
    }

    /// A Worktree symbol (R92): matched on its name; the detail is its
    /// kind and place.
    pub fn symbol(root: &Path, s: &crate::code::nav::Item) -> Self {
        let name = s.name.clone().unwrap_or_default();
        let rel = s
            .path
            .strip_prefix(root)
            .unwrap_or(&s.path)
            .display()
            .to_string();
        let detail = match &s.kind {
            Some(k) => format!("{k} · {rel}:{}", s.line),
            None => format!("{rel}:{}", s.line),
        };
        Self {
            hay: fold(&name).as_str().into(),
            title_at: 0,
            kind: Kind::Symbol,
            target: Target::Symbol {
                path: s.path.clone(),
                line: s.line,
                col: s.col,
            },
            title: name,
            detail,
            keys: None,
        }
    }

    /// A file of the Worktree at `root`: matched on its relative path.
    pub fn file(root: &Path, path: PathBuf) -> Self {
        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
        let (dir, name) = match rel.rfind('/') {
            Some(i) => (&rel[..i + 1], &rel[i + 1..]),
            None => ("", &rel[..]),
        };
        Self {
            hay: fold(&rel).as_str().into(),
            title_at: dir.chars().count() as u32,
            kind: Kind::File,
            title: name.into(),
            detail: dir.trim_end_matches('/').into(),
            keys: None,
            target: Target::File { path },
        }
    }
}

/// Fold for matching: lowercase is left to smart case; accents are
/// dropped char by char (`Tiếng Việt` → `Tieng Viet`, `đ` → `d`), so a
/// query with or without Vietnamese marks finds both and positions line up.
pub fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'đ' => 'd',
            'Đ' => 'D',
            c if c.is_ascii() => c,
            c => c.nfd().next().unwrap_or(c),
        })
        .collect()
}

/// GTK accelerator (`<Ctrl><Shift>Page_Down`) → label (`Ctrl+Shift+PgDn`).
pub fn accel_label(accel: &str) -> String {
    let mut parts = Vec::new();
    let mut rest = accel;
    while let Some(s) = rest.strip_prefix('<') {
        let Some(end) = s.find('>') else { break };
        parts.push(match &s[..end] {
            "Control" | "Primary" => "Ctrl",
            m => m,
        });
        rest = &s[end + 1..];
    }
    let key = match rest {
        "Return" => "Enter".to_string(),
        "Page_Down" => "PgDn".into(),
        "Page_Up" => "PgUp".into(),
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => k.into(),
    };
    let mut s = parts.join("+");
    if !s.is_empty() {
        s.push('+');
    }
    s + &key
}

/// App actions the palette offers: every keybinding action (Source
/// switching is covered by the Source rows, which show `Alt+<n>`) plus
/// actions without a key. `…` = asks for more (an inline form or dialog).
pub const ACTIONS: &[(&str, &str)] = &[
    ("new_tab", "New Tab"),
    ("close_pane", "Close Pane"),
    ("split_right", "Split Right"),
    ("split_down", "Split Down"),
    ("focus_left", "Focus Pane Left"),
    ("focus_right", "Focus Pane Right"),
    ("focus_up", "Focus Pane Up"),
    ("focus_down", "Focus Pane Down"),
    ("zoom", "Toggle Zoom"),
    ("next_tab", "Next Tab"),
    ("prev_tab", "Previous Tab"),
    ("rename_tab", "Rename Tab…"),
    ("duplicate_tab", "Duplicate Tab"),
    ("close_tab", "Close Tab"),
    ("close_other_tabs", "Close Other Tabs"),
    ("close_tabs_right", "Close Tabs to the Right"),
    ("rename_pane", "Rename Pane…"),
    ("find_in_terminal", "Find in Terminal…"),
    ("rename_source", "Rename Source…"),
    ("pause_source", "Pause Source"),
    ("resume_source", "Resume Source"),
    ("add_source", "Add Source…"),
    ("new_project", "New Project…"),
    ("create_workspace", "Create Workspace…"),
    ("jump_unread", "Open Newest Unread Notification"),
    ("toggle_file_tree", "Toggle Right Panel"),
    ("toggle_status_bar", "Toggle Status Bar"),
    ("open_usage", "Agent Usage"),
    ("agent_overview", "Agent Overview"),
    ("voice", "Voice Prompt"),
    ("open_settings", "Settings"),
    ("new_worktree", "New Worktree…"),
    ("open_review", "Review Changes"),
    ("review_since", "Review: Since Last Round"),
    ("open_browser", "Open Browser"),
    ("show_changes", "Show Changes"),
    ("search_worktree", "Search in Worktree"),
    ("scm_commit", "Commit Staged Changes"),
    ("scm_pull", "Pull"),
    ("scm_push", "Push…"),
    ("scm_pr", "Create Pull Request…"),
    ("scm_fetch", "Fetch"),
    ("scm_sync", "Sync Changes…"),
    ("scm_publish", "Publish Branch…"),
    ("scm_undo_commit", "Undo Last Commit"),
    ("scm_view_tree", "Changes: View as Tree"),
    ("scm_view_list", "Changes: View as List"),
    ("spawn_agent", "Spawn Agent…"),
    ("fanout", "Fan-out…"),
    ("mark_all_read", "Mark All Notifications Read"),
    ("clear_notifications", "Clear Notifications"),
];

/// Keybinding actions the palette lists (not Source switching, not itself).
#[cfg(test)]
pub fn is_listed_binding(action: &str) -> bool {
    // Source and Workspace keys are on their own rows (R14.1, R23.5).
    !action.starts_with("source_")
        && !action.starts_with("workspace_")
        && action != "command_palette"
}

/// Prefix of the per-Preset spawn actions (`spawn:claude`).
pub const SPAWN_PREFIX: &str = "spawn:";

/// Prefix of the "Open localhost:<port>" actions (`browser:http://localhost:3000/`).
pub const BROWSER_PREFIX: &str = "browser:";

/// "Open localhost:<port>" rows (R18.4): (port, where it listens: the Pane
/// name · Source / Worktree).
pub fn port_items(ports: &[(u16, String)]) -> Vec<Item> {
    ports
        .iter()
        .map(|(port, at)| {
            let target = Target::Action {
                id: format!("{BROWSER_PREFIX}http://localhost:{port}/"),
            };
            Item::new(
                Kind::Action,
                target,
                &format!("Open localhost:{port}"),
                at,
                None,
            )
        })
        .collect()
}

fn pane_title(p: &crate::Pane) -> String {
    if let Some(n) = &p.name {
        return n.clone();
    }
    match &p.kind {
        PaneKind::Viewer { path, .. } => path
            .file_name()
            .map_or("Viewer".into(), |n| n.to_string_lossy().into_owned()),
        PaneKind::Review { .. } => "Review".into(),
        PaneKind::Diff { path, .. } => format!(
            "Diff · {}",
            path.rsplit('/').next().unwrap_or(path.as_str())
        ),
        PaneKind::Browser { url, .. } => {
            // The host, or a file's name: never a local path.
            let rest = url.split_once("://").map_or(url.as_str(), |(_, r)| r);
            let host = if url.starts_with("file:") {
                rest.rsplit('/').next().unwrap_or("")
            } else {
                rest.split('/').next().unwrap_or("")
            };
            format!("Browser {host}").trim_end().to_string()
        }
        PaneKind::Terminal { .. } => p.agent.clone().unwrap_or_else(|| "Terminal".into()),
    }
}

/// What the palette knows of the shown Workspace's Projects (R107.3).
pub struct Links<'a> {
    /// In shown order.
    pub projects: &'a [Project],
    /// Every link of the shown Workspace.
    pub links: &'a [LinkView],
    /// A Worktree folder's commits ahead of its base.
    pub ahead: &'a dyn Fn(&Path) -> usize,
    pub canon: &'a dyn Fn(&Path) -> PathBuf,
    pub home: &'a Path,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Everything but files with no Projects known (the Workspace root and
/// paths shown under the State's home).
pub fn model_items(ws: &State, keybindings: &[(String, String)], presets: &[String]) -> Vec<Item> {
    let none = |_: &Path| 0;
    let canon = |p: &Path| p.to_path_buf();
    // The default Workspace is rooted at home.
    let home = ws
        .workspaces
        .first()
        .map(|w| w.root.clone())
        .unwrap_or_default();
    let l = Links {
        projects: &[],
        links: &[],
        ahead: &none,
        canon: &canon,
        home: &home,
    };
    items(ws, keybindings, presets, &l)
}

/// Everything but files: Source links, Projects, Worktrees, Panes by name,
/// Tabs, Sources no Project links (the Workspace root as `~/…`), actions
/// (app actions, one `Add <source> to Project…` / `Fan-out on <source>…`
/// per Source, one `Change base of <source> in <Project>…` per link) and
/// one "Spawn <preset>" per Preset. `keybindings` = effective (action,
/// accelerator) pairs. No title or detail holds an absolute path under
/// home, a Pane id or `General`.
pub fn items(
    ws: &State,
    keybindings: &[(String, String)],
    presets: &[String],
    l: &Links,
) -> Vec<Item> {
    let key = |action: &str| {
        keybindings
            .iter()
            .find(|(a, _)| a == action)
            .map(|(_, k)| accel_label(k))
    };
    let tilde = |p: &Path| crate::navigator::tilde(p, l.home);
    let pname = |id: &str| {
        l.projects
            .iter()
            .find(|p| p.id == id)
            .map_or_else(|| id.to_string(), |p| p.name.clone())
    };
    let mut out = Vec::new();
    // Projects and their links (R107.3).
    for p in l.projects {
        let n = l.links.iter().filter(|x| x.project == p.id).count();
        let target = Target::Project {
            project: p.id.clone(),
        };
        let detail = format!("Project · {}", plural(n, "source", "sources"));
        out.push(Item::new(Kind::Project, target, &p.name, &detail, None));
    }
    // Links in the Projects' shown order, then their own order.
    let rank = |id: &str| {
        l.projects
            .iter()
            .position(|p| p.id == id)
            .unwrap_or(usize::MAX)
    };
    let mut ordered: Vec<&LinkView> = l.links.iter().collect();
    ordered.sort_by_key(|x| rank(&x.project));
    for x in ordered.iter().copied() {
        let Some(s) = ws.source(&x.source) else {
            continue;
        };
        let mut agents = crate::navigator::link_panes(s, x, l.links, l.canon)
            .into_iter()
            .filter(|p| p.agent.is_some())
            .count();
        agents += s
            .worktrees
            .iter()
            .skip(1)
            .filter(|w| w.project.as_deref() == Some(x.project.as_str()))
            .flat_map(|w| w.tabs.iter().flat_map(|t| t.panes()))
            .filter(|p| p.agent.is_some())
            .count();
        let mut detail = format!("{} · {}", x.project_name, x.base);
        if agents > 0 {
            detail += &format!(" · {}", plural(agents, "agent", "agents"));
        }
        let target = Target::Link {
            project: x.project.clone(),
            source: x.source.clone(),
        };
        out.push(Item::new(Kind::Link, target, &s.name, &detail, None));
    }
    let linked = |src: &str| l.links.iter().any(|x| x.source == src);
    // Panes and Tabs, placed by Project / Worktree name.
    for (p, w) in ws.worktrees() {
        let main = p.worktrees.first().is_some_and(|m| m.id == w.id);
        let place = if p.is_general() {
            tilde(&p.path)
        } else if main {
            p.name.clone()
        } else {
            match w
                .project
                .as_deref()
                .filter(|id| l.projects.iter().any(|x| x.id == *id))
            {
                Some(id) => format!("{} / {}", pname(id), w.name),
                None => format!("{} / {}", p.name, w.name),
            }
        };
        for t in &w.tabs {
            for x in t.panes() {
                let mut d: Vec<String> = Vec::new();
                d.extend(x.agent.clone());
                d.extend(x.status.map(|s| s.as_str().to_string()));
                d.push(format!("{} · {place}", t.name));
                let target = Target::Pane { pane: x.id.clone() };
                out.push(Item::new(
                    Kind::Pane,
                    target,
                    &pane_title(x),
                    &d.join(" · "),
                    None,
                ));
            }
            let n = t.panes().len();
            let detail = format!("{place} · {n} Pane{}", if n == 1 { "" } else { "s" });
            let target = Target::Tab { tab: t.id.clone() };
            out.push(Item::new(Kind::Tab, target, &t.name, &detail, None));
        }
    }
    // The shown Workspace's Worktrees and unlinked Sources (R23.4);
    // `Alt+<n>` counts the Sources.
    for (i, p) in ws.visible_sources().enumerate() {
        for w in p.worktrees.iter().skip(1) {
            let target = Target::Worktree {
                source: p.id.clone(),
                worktree: w.id.clone(),
            };
            let link = w
                .project
                .as_deref()
                .and_then(|id| l.links.iter().find(|x| x.project == id && x.source == p.id));
            let detail = match link {
                Some(x) => {
                    let base = crate::checkout::comparison_base(w.base.as_deref(), Some(&x.base))
                        .unwrap_or_else(|| x.base.clone());
                    let mut d = format!("{} · {} ← {base}", x.project_name, p.name);
                    let a = (l.ahead)(&w.path);
                    if a > 0 {
                        d += &format!(" · ↑{a}");
                    }
                    d
                }
                None => match &w.base {
                    Some(b) => format!("{} ← {b}", p.name),
                    None => p.name.clone(),
                },
            };
            out.push(Item::new(Kind::Worktree, target, &w.name, &detail, None));
        }
        if linked(&p.id) {
            continue;
        }
        let target = Target::Source {
            source: p.id.clone(),
        };
        let keys = key(&format!("source_{}", i + 1));
        let (title, detail) = if p.is_general() {
            (tilde(&p.path), "Workspace root".to_string())
        } else {
            (p.name.clone(), tilde(&p.path))
        };
        out.push(Item::new(Kind::Source, target, &title, &detail, keys));
    }
    // "Switch Workspace" rows (R23.5), with more than one Workspace.
    // ponytail: the first nine (their Ctrl+Alt+<n> actions); a Workspace
    // target kind if anyone has ten.
    let looks = ws.workspace_looks();
    if looks.len() > 1 {
        for (i, lk) in looks.iter().enumerate().take(9) {
            let id = format!("workspace_{}", i + 1);
            let title = format!("Switch Workspace: {}", lk.name);
            let root = tilde(&ws.workspaces[i].root);
            out.push(Item::new(
                Kind::Action,
                Target::Action { id: id.clone() },
                &title,
                &root,
                key(&id),
            ));
        }
    }
    for (id, title) in ACTIONS {
        let target = Target::Action { id: id.to_string() };
        out.push(Item::new(Kind::Action, target, title, "", key(id)));
    }
    // Link actions (R107.3).
    for p in ws.visible_sources().filter(|p| !p.is_general()) {
        let target = Target::Action {
            id: format!("{LINK_ADD_PREFIX}{}", p.id),
        };
        let title = format!("Add {} to Project…", p.name);
        out.push(Item::new(Kind::Action, target, &title, "", None));
    }
    for x in ordered.iter().copied() {
        let Some(s) = ws.source(&x.source) else {
            continue;
        };
        let target = Target::Action {
            id: format!("{LINK_BASE_PREFIX}{}/{}", x.project, x.source),
        };
        let title = format!("Change base of {} in {}…", s.name, x.project_name);
        out.push(Item::new(Kind::Action, target, &title, &x.base, None));
    }
    for p in ws.visible_sources().filter(|p| !p.is_general()) {
        let target = Target::Action {
            id: format!("{FANOUT_ON_PREFIX}{}", p.id),
        };
        let title = format!("Fan-out on {}…", p.name);
        out.push(Item::new(
            Kind::Action,
            target,
            &title,
            "one prompt, several agents",
            None,
        ));
    }
    for name in presets {
        let target = Target::Action {
            id: format!("{SPAWN_PREFIX}{name}"),
        };
        let title = format!("Spawn {name}");
        out.push(Item::new(
            Kind::Action,
            target,
            &title,
            "in a new Tab",
            None,
        ));
    }
    out
}

/// Prefix of the `Add <source> to Project…` actions (`link_add:<source>`).
pub const LINK_ADD_PREFIX: &str = "link_add:";
/// Prefix of `Change base of <source> in <Project>…`
/// (`link_base:<project>/<source>`).
pub const LINK_BASE_PREFIX: &str = "link_base:";
/// Prefix of `Fan-out on <source>…` (`fanout_on:<source>`).
pub const FANOUT_ON_PREFIX: &str = "fanout_on:";

/// Files the palette searches in a Worktree: like the file tree (R8.3,
/// `.gitignore` applied, hidden files kept, `.git` skipped), sorted, at most
/// `cap` (the bool says whether the walk stopped there).
pub fn file_items(root: &Path, cap: usize) -> (Vec<Item>, bool) {
    let mut paths = Vec::new();
    let walk = ignore::WalkBuilder::new(root)
        .hidden(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build();
    for e in walk.filter_map(Result::ok) {
        let file = e
            .file_type()
            .is_some_and(|t| t.is_file() || (t.is_symlink() && e.path().is_file()));
        if file {
            if paths.len() == cap {
                paths.sort();
                return (
                    paths.into_iter().map(|p| Item::file(root, p)).collect(),
                    true,
                );
            }
            paths.push(e.into_path());
        }
    }
    paths.sort();
    (
        paths.into_iter().map(|p| Item::file(root, p)).collect(),
        false,
    )
}

/// How many recently used items are remembered.
pub const RECENT_MAX: usize = 30;

/// Put `key` first in the recently-used list.
pub fn remember(recent: &mut Vec<String>, key: String) {
    recent.retain(|k| *k != key);
    recent.insert(0, key);
    recent.truncate(RECENT_MAX);
}

/// A ranked item: its index in the list, whether the query matched its
/// title alone (those rank above every match that needs the detail line:
/// a Pane named `codex` before Panes running the codex Preset, a file name
/// before a folder name), score, and the title characters the query
/// matched (for bold highlighting).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub index: usize,
    pub title: bool,
    pub score: u32,
    pub positions: Vec<u32>,
}

/// The best hits and how many items matched in all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ranked {
    pub hits: Vec<Hit>,
    pub total: usize,
}

/// Rank `items` for `query` (prefix + text): the filter's items, fuzzy
/// matched word by word on title + detail (files on their path; no special
/// characters), best first; title matches first, recently used items get a
/// small bonus. An empty text lists recently used items first, then the
/// rest by kind (files only under the `#` filter). `cancelled` is polled
/// while matching; `None` = it returned true.
pub fn rank(
    items: &[Item],
    query: &str,
    recent: &[String],
    limit: usize,
    cancelled: &dyn Fn() -> bool,
) -> Option<Ranked> {
    let (filter, text) = parse_query(query);
    let recency = |it: &Item| {
        let k = it.target.key();
        recent.iter().position(|r| *r == k)
    };
    if text.is_empty() {
        let mut order: Vec<(usize, usize, Kind)> = items
            .iter()
            .enumerate()
            .filter(|(_, it)| filter.admits(it.kind))
            .filter(|(_, it)| it.kind != Kind::File || filter == Filter::Files)
            .map(|(i, it)| (recency(it).unwrap_or(usize::MAX), i, it.kind))
            .collect();
        order.sort_by_key(|&(r, i, k)| (r, if r == usize::MAX { k } else { Kind::Pane }, i));
        let total = order.len();
        order.truncate(limit);
        let hits = order.into_iter().map(|(_, index, _)| Hit {
            index,
            title: false,
            score: 0,
            positions: vec![],
        });
        return Some(Ranked {
            hits: hits.collect(),
            total,
        });
    }
    let pattern = Pattern::new(
        &fold(text),
        CaseMatching::Smart,
        Normalization::Never,
        AtomKind::Fuzzy,
    );
    let mut plain = Matcher::new(Config::DEFAULT);
    let mut paths = Matcher::new(Config::DEFAULT.match_paths());
    let mut hits: Vec<Hit> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        if i % 4096 == 4095 && cancelled() {
            return None;
        }
        if !filter.admits(it.kind) {
            continue;
        }
        let m = if it.kind == Kind::File {
            &mut paths
        } else {
            &mut plain
        };
        if let Some(s) = pattern.score(it.hay.slice(..), m) {
            let t = pattern.score(it.title_slice(), m);
            let bonus = recency(it).map_or(0, |r| (RECENT_MAX - r) as u32);
            hits.push(Hit {
                index: i,
                title: t.is_some(),
                score: t.unwrap_or(s) + bonus,
                positions: vec![],
            });
        }
    }
    let order = |a: &Hit, b: &Hit| {
        let (x, y) = (&items[a.index], &items[b.index]);
        (b.title, b.score)
            .cmp(&(a.title, a.score))
            .then(x.kind.cmp(&y.kind))
            .then(x.title.len().cmp(&y.title.len()))
            .then(a.index.cmp(&b.index))
    };
    let total = hits.len();
    if hits.len() > limit {
        hits.select_nth_unstable_by(limit, order);
        hits.truncate(limit);
    }
    hits.sort_unstable_by(order);
    // Listed by section (spec D.10), best first inside each.
    hits.sort_by_key(|h| section_rank(items[h.index].kind));
    for h in &mut hits {
        let it = &items[h.index];
        let m = if it.kind == Kind::File {
            &mut paths
        } else {
            &mut plain
        };
        let mut idx = Vec::new();
        if !h.title || pattern.indices(it.title_slice(), m, &mut idx).is_none() {
            idx.clear();
            pattern.indices(it.hay.slice(..), m, &mut idx);
            let end = it.title_at + it.title.chars().count() as u32;
            idx.retain(|&p| p >= it.title_at && p < end);
            idx.iter_mut().for_each(|p| *p -= it.title_at);
        }
        idx.sort_unstable();
        idx.dedup();
        h.positions = idx;
    }
    Some(Ranked { hits, total })
}

/// Whether the thing a recently-used key names still exists (actions and
/// files are kept: an action always exists, a file is checked on open).
pub fn key_exists(ws: &State, key: &str) -> bool {
    let Some((kind, id)) = key.split_once(':') else {
        return false;
    };
    match kind {
        "pane" => ws.locate(id).is_some(),
        "tab" => ws.locate_tab(id).is_some(),
        "source" => ws.source(id).is_some(),
        "link" => id
            .split_once('/')
            .is_some_and(|(_, s)| ws.source(s).is_some()),
        "worktree" => id
            .split_once('/')
            .is_some_and(|(p, w)| ws.worktree(p, w).is_some()),
        _ => true,
    }
}

/// Drop recently used keys whose Pane / Tab / Worktree / Source is gone.
pub fn prune_recent(ws: &State, recent: &mut Vec<String>) {
    recent.retain(|k| key_exists(ws, k));
}

/// What the palette knows about the active Worktree's files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Files {
    /// No file search here (General is `$HOME`).
    Off,
    /// The background walk is still running.
    Indexing,
    /// Listed: how many, and whether the walk stopped at the cap.
    Listed(usize, bool),
}

/// The line under the results: `3 results · 1,001 files`, `60 of 812`.
pub fn status_line(shown: usize, total: usize, query: &str, files: Files) -> String {
    let (f, text) = parse_query(query);
    let what = match (shown, total) {
        (0, _) => "No matches".to_string(),
        (n, t) if t > n => format!("{n} of {t} results"),
        (1, _) => "1 result".into(),
        (n, _) => format!("{n} results"),
    };
    let about_files = f == Filter::Files || (f == Filter::All && !text.is_empty());
    let files = match files {
        _ if !about_files => String::new(),
        Files::Off => " · no file search in General".into(),
        Files::Indexing => " · indexing files…".into(),
        Files::Listed(n, true) => format!(" · first {n} files searched"),
        Files::Listed(n, false) => format!(" · {n} files"),
    };
    what + &files
}

/// The row a navigation key selects in a list of `n` rows: Up/Down wrap,
/// Page Up/Down move `page` rows and stop at the ends.
pub fn step(selected: u32, n: u32, key: &str, page: u32) -> Option<u32> {
    if n == 0 {
        return None;
    }
    let s = selected.min(n - 1);
    Some(match key {
        "up" => (s + n - 1) % n,
        "down" => (s + 1) % n,
        "page_up" => s.saturating_sub(page),
        "page_down" => (s + page).min(n - 1),
        _ => return None,
    })
}

/// Every keybinding action the palette should offer (tests check that
/// [`ACTIONS`] covers them).
#[cfg(test)]
pub fn bound_actions() -> impl Iterator<Item = &'static str> {
    crate::config::DEFAULT_KEYBINDINGS
        .iter()
        .map(|(a, _)| *a)
        .filter(|a| is_listed_binding(a))
}

#[cfg(test)]
#[path = "palette_tests.rs"]
mod tests;
