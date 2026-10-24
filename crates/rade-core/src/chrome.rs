//! Text rules of the window chrome (PRD R103, spec `a-workspace.md`): the
//! titlebar breadcrumb, a Tab's label, a Pane header and the Statusline's
//! cells. GTK-free so they have unit tests; `rade-app` only lays them out.

use crate::agents::AgentStatus;
use crate::layout::{Pane, PaneKind};

/// The titlebar breadcrumb (R103.1): `Project / Source / Worktree ← base ↑n`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Crumbs {
    /// The Project the checkout is shown under, if any.
    pub project: Option<String>,
    pub source: String,
    /// A linked Worktree's name; `None` for the Source's main checkout.
    pub worktree: Option<String>,
    /// `← develop ↑3`: the Worktree's comparison base and commits ahead.
    pub tail: Option<String>,
}

impl Crumbs {
    /// The whole breadcrumb as one line (`debug.ui chrome`).
    pub fn text(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if let Some(p) = &self.project {
            parts.push(p);
        }
        parts.push(&self.source);
        if let Some(w) = &self.worktree {
            parts.push(w);
        }
        let mut s = parts.join(" / ");
        if let Some(t) = &self.tail {
            s.push(' ');
            s.push_str(t);
        }
        s
    }
}

/// The breadcrumb of a checkout. The tail needs a linked Worktree and a
/// base; `↑n` shows only when the Worktree is ahead of it.
pub fn breadcrumb(
    project: Option<&str>,
    source: &str,
    worktree: Option<&str>,
    base: Option<&str>,
    ahead: Option<u32>,
) -> Crumbs {
    let tail = match (worktree, base) {
        (Some(_), Some(b)) => Some(match ahead {
            Some(n) if n > 0 => format!("← {b} ↑{n}"),
            _ => format!("← {b}"),
        }),
        _ => None,
    };
    Crumbs {
        project: project.map(String::from),
        source: source.to_string(),
        worktree: worktree.map(String::from),
        tail,
    }
}

/// A Tab's label (R103.2): the name the user gave, else its (lead) Agent's
/// name, else what a Pane-type Tab shows (`Review`, a Browser's host),
/// else its Worktree's name. Never `Tab <n>`; the Agent count is a widget
/// of its own ([`tab_count`]).
pub fn tab_label(
    user_name: Option<&str>,
    agent: Option<&str>,
    special: Option<&str>,
    worktree: &str,
) -> String {
    fn pick(s: Option<&str>) -> Option<&str> {
        s.map(str::trim).filter(|s| !s.is_empty())
    }
    pick(user_name)
        .or(pick(agent))
        .or(pick(special))
        .unwrap_or(worktree)
        .to_string()
}

/// The count next to a Tab's label: its Agents, shown from 2 on.
pub fn tab_count(agents: usize) -> Option<usize> {
    (agents >= 2).then_some(agents)
}

/// What a Tab with no Agent and no user name shows instead of the Worktree
/// name: `Review` when it holds only Reviews, a Browser's host when it
/// holds only Browsers.
pub fn tab_special(panes: &[&Pane]) -> Option<String> {
    if panes.is_empty() {
        return None;
    }
    if panes.iter().all(|p| p.is_review()) {
        return Some("Review".into());
    }
    let hosts: Vec<String> = panes
        .iter()
        .filter_map(|p| match &p.kind {
            PaneKind::Browser { url, .. } => Some(url_host(url)),
            _ => None,
        })
        .collect();
    (hosts.len() == panes.len()).then(|| hosts[0].clone())
}

/// `http://localhost:5173/x` → `localhost:5173`; anything else as it is.
pub fn url_host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    if host.is_empty() {
        url.to_string()
    } else {
        host.to_string()
    }
}

/// The model an Agent was started with: `--model X`, `--model=X` or
/// `-m X` in its command line.
pub fn model_of(argv: &[String]) -> Option<String> {
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        if let Some(v) = a.strip_prefix("--model=") {
            return (!v.is_empty()).then(|| v.to_string());
        }
        if a == "--model" || a == "-m" {
            return it.next().filter(|v| !v.starts_with('-')).cloned();
        }
    }
    None
}

/// [`model_of`] a Pane: its recorded invocation, else its command.
pub fn pane_model(p: &Pane) -> Option<String> {
    if let Some(inv) = &p.invocation {
        if let Some(m) = model_of(&inv.argv) {
            return Some(m);
        }
    }
    match &p.kind {
        PaneKind::Terminal {
            command: Some(c), ..
        } => model_of(&c.split_whitespace().map(String::from).collect::<Vec<_>>()),
        _ => None,
    }
}

/// A Pane header's parts (R103.3): `name · agent · model · status`, empty
/// parts left out; `waiting` reads `needs input`.
pub fn pane_header(
    name: &str,
    agent: Option<&str>,
    model: Option<&str>,
    status: Option<AgentStatus>,
) -> Vec<String> {
    let mut parts = vec![name.to_string()];
    parts.extend(
        agent
            .filter(|a| !a.is_empty() && *a != name)
            .map(String::from),
    );
    parts.extend(model.filter(|m| !m.is_empty()).map(String::from));
    parts.extend(status.map(|s| s.words().to_string()));
    parts
}

/// Whether Pane `p` of a Tab with `panes` tiled Panes shows a header
/// (R103.3): an Agent, a Subagent or a named Pane always; with 2 or more
/// Panes every Terminal too, for its ✕ (R19.9). Viewers, Reviews and
/// Browsers have headers of their own.
pub fn has_header(p: &Pane, panes: usize) -> bool {
    p.agent_name().is_some()
        || p.parent.is_some()
        || p.name.is_some()
        || (panes >= 2 && matches!(p.kind, PaneKind::Terminal { .. }))
}

/// A header's name: the Pane's name, else its Agent, else `Terminal`.
pub fn header_name(p: &Pane) -> &str {
    p.name.as_deref().or(p.agent_name()).unwrap_or("Terminal")
}

/// [`pane_header`] of a Pane, joined with ` · `.
pub fn pane_header_text(p: &Pane) -> String {
    let name = p.name.as_deref().or(p.agent.as_deref()).unwrap_or(&p.id);
    pane_header(name, p.agent.as_deref(), pane_model(p).as_deref(), p.status).join(" · ")
}

/// What a Statusline cell is (its CSS class `status-cell-<kind>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellKind {
    Workspace,
    Project,
    Worktree,
    Counts,
    Ports,
    /// The focused Agent's account usage (R110.3, A.74).
    Usage,
    Identity,
    /// The system monitor (R116): a widget of its own after the cells.
    Monitor,
    Hint,
}

impl CellKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CellKind::Workspace => "ws",
            CellKind::Project => "project",
            CellKind::Worktree => "worktree",
            CellKind::Counts => "counts",
            CellKind::Ports => "ports",
            CellKind::Usage => "usage",
            CellKind::Identity => "identity",
            CellKind::Monitor => "monitor",
            CellKind::Hint => "hint",
        }
    }
}

/// One Statusline cell: its kind and text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub kind: CellKind,
    pub text: String,
}

/// The key hints at the Statusline's right end: the forms that work with a
/// Terminal focused (R17 amendment).
pub const HINTS: [&str; 2] = ["Ctrl ⇧ P commands", "Ctrl ⇧ G next waiting"];

/// What the Statusline shows (R103.5).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusInput {
    pub workspace: String,
    pub project: Option<String>,
    /// The focused checkout's name and, when known, commits ahead / behind
    /// its base.
    pub worktree: Option<(String, Option<(u32, u32)>)>,
    /// Agents per status, in display order.
    pub counts: Vec<(AgentStatus, usize)>,
    pub ports: Vec<u16>,
    /// `git config user.email` of the focused checkout.
    pub identity: Option<String>,
    /// The focused Agent's account usage, `claude 5h 62% · wk 41%`
    /// (`crate::usage::view::status_cell`); `None` hides the cell.
    pub usage: Option<String>,
}

/// One status count as the Statusline words it: `1 waiting`.
pub fn count_text(s: AgentStatus, n: usize) -> String {
    format!("{n} {}", s.as_str())
}

/// The Statusline's cells in order (spec A.77): Workspace (uppercase) ·
/// Project · Worktree `↑a ↓b` · counts · ports · usage (R110.3) ·
/// identity · hints. Empty cells are left out.
pub fn statusline_cells(s: &StatusInput) -> Vec<Cell> {
    let cell = |kind, text: String| Cell { kind, text };
    let mut out = vec![cell(CellKind::Workspace, s.workspace.to_uppercase())];
    if let Some(p) = &s.project {
        out.push(cell(CellKind::Project, p.clone()));
    }
    if let Some((name, ab)) = &s.worktree {
        out.push(cell(
            CellKind::Worktree,
            match ab {
                Some((a, b)) => format!("{name} ↑{a} ↓{b}"),
                None => name.clone(),
            },
        ));
    }
    if !s.counts.is_empty() {
        let t: Vec<String> = s.counts.iter().map(|(st, n)| count_text(*st, *n)).collect();
        out.push(cell(CellKind::Counts, t.join(" · ")));
    }
    if !s.ports.is_empty() {
        let t: Vec<String> = s.ports.iter().map(|p| format!(":{p}")).collect();
        out.push(cell(CellKind::Ports, t.join(" ")));
    }
    if let Some(u) = &s.usage {
        out.push(cell(CellKind::Usage, u.clone()));
    }
    if let Some(i) = s.identity.as_deref().filter(|i| !i.is_empty()) {
        out.push(cell(CellKind::Identity, i.to_string()));
    }
    out.extend(HINTS.iter().map(|h| cell(CellKind::Hint, h.to_string())));
    out
}

/// The Inspector's text tabs (R103.4): page name and title, in order; the
/// Activity icon tab follows them.
pub const INSPECTOR_TABS: [(&str, &str); 3] = [
    ("changes", "Changes"),
    ("files", "Files"),
    ("search", "Search"),
];

/// An Inspector tab's label: `Changes 7`, the title alone for 0.
pub fn inspector_label(title: &str, n: usize) -> String {
    if n == 0 {
        title.to_string()
    } else {
        format!("{title} {n}")
    }
}

/// An empty state (R103.6): one mono line and key hints (keys, words);
/// never a warning icon or a StatusPage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Empty {
    pub line: &'static str,
    pub hints: &'static [(&'static str, &'static str)],
}

/// Inspector › Changes of a clean checkout.
pub const EMPTY_CHANGES: Empty = Empty {
    line: "No changes against the base",
    hints: &[("Ctrl ⇧ R", "review"), ("Ctrl ⇧ S", "search")],
};
/// Inspector › Changes by Project with every link clean (spec A, empty).
pub const EMPTY_PROJECT_CHANGES: Empty = Empty {
    line: "No changes against each link's base",
    hints: &[("Ctrl ⇧ R", "review"), ("Ctrl ⇧ S", "search")],
};
/// Inspector › Changes of a folder that is not a git repository.
pub const EMPTY_NO_REPO: Empty = Empty {
    line: "Not a git repository",
    hints: &[("Ctrl ⇧ E", "hide")],
};
/// Inspector › Files of an empty folder.
pub const EMPTY_FILES: Empty = Empty {
    line: "Empty folder",
    hints: &[("Ctrl ⇧ T", "shell"), ("Ctrl ⇧ P", "commands")],
};
/// Inspector › Search with no query.
pub const EMPTY_SEARCH: Empty = Empty {
    line: "Type to search this checkout",
    hints: &[("Ctrl ⇧ S", "search"), ("Esc", "back")],
};
/// Inspector › Activity until R107 (M37) fills it.
pub const EMPTY_ACTIVITY: Empty = Empty {
    line: "No activity yet",
    hints: &[("Ctrl ⇧ G", "next waiting")],
};
/// A Tab with no Pane / a Worktree with no Tabs (spec A "Empty state").
pub const EMPTY_TAB: Empty = Empty {
    line: "No terminals here",
    hints: &[("Ctrl ⇧ T", "shell"), ("+", "agent")],
};

/// A rectangle in window coordinates (px).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    /// Whether the two overlap with a positive area (touching edges do not).
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
}

/// Space kept between a lifted Toast and the button under it.
pub const TOAST_GAP: f64 = 8.0;

/// How far (px) a Toast at `toast` moves up so it covers none of `buttons`
/// (R103.6 "a Toast never covers an action"): it rises above each button it
/// overlaps, again until clear, never above `top`.
pub fn toast_lift(toast: &Rect, buttons: &[Rect], top: f64) -> f64 {
    let mut t = *toast;
    // Each pass moves above at least one button: at most one pass each.
    for _ in 0..=buttons.len() {
        let Some(over) = buttons
            .iter()
            .filter(|b| t.intersects(b))
            .map(|b| b.y)
            .reduce(f64::min)
        else {
            break;
        };
        t.y = over - TOAST_GAP - t.h;
        if t.y <= top {
            t.y = top;
            break;
        }
    }
    (toast.y - t.y).max(0.0)
}

/// The identity warning (R107.5): when the focused checkout's `git config
/// user.email` is set and differs from the Workspace's, the tooltip that
/// names both (the cell turns `wait`; Changes shows it as one line).
pub fn identity_warning(workspace: Option<&str>, checkout: Option<&str>) -> Option<String> {
    fn norm(e: Option<&str>) -> Option<&str> {
        e.map(str::trim).filter(|e| !e.is_empty())
    }
    match (norm(workspace), norm(checkout)) {
        (Some(w), Some(c)) if !w.eq_ignore_ascii_case(c) => Some(format!(
            "This checkout commits as {c}, the Workspace's identity is {w}"
        )),
        _ => None,
    }
}

// ---- R111: narrow windows and large text (spec sheets `## 1280 px`,
// `## Text scale 200 %`) ------------------------------------------------------

/// The narrowest window that docks the Inspector beside the center; below it
/// the Inspector floats over the center's right edge (spec A "1280 px").
pub const WIDE_WIDTH: i32 = 1440;
/// The text scale from which the Navigator becomes an icon rail and the
/// center shows one Pane (spec A "Text scale 200 %"; GNOME's steps are 1.0,
/// 1.25, 1.5, 2.0: 1.25 still fits the full layout).
pub const RAIL_TEXT_SCALE: f64 = 1.5;
/// The Navigator (spec A.17) and its icon rail.
pub const NAVIGATOR_WIDTH: i32 = 268;
pub const RAIL_WIDTH: i32 = 48;

/// How the window lays itself out at a width and text scale (R111).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutMode {
    /// The Inspector floats over the center (340 px, hidden by default).
    pub inspector_overlay: bool,
    /// The Navigator is a 48 px icon rail; the full one opens as an overlay.
    pub rail: bool,
    /// The center shows the focused Pane; the others become Title Bars (R2.4).
    pub one_pane: bool,
    /// Large text: table cells and details move to a second line, cards and
    /// grids go to one column, overlays widen.
    pub large_text: bool,
}

impl LayoutMode {
    /// `wide` (the golden layout), `narrow` or `rail` (`debug.ui chrome`).
    pub fn name(&self) -> &'static str {
        if self.rail {
            "rail"
        } else if self.inspector_overlay {
            "narrow"
        } else {
            "wide"
        }
    }
}

/// The layout for a window `width` px wide at `text_scale` (1.0 = 100 %).
/// Nothing changes at 1440 px and scale 1 (the goldens).
pub fn layout_mode(width: i32, text_scale: f64) -> LayoutMode {
    let large = text_scale >= RAIL_TEXT_SCALE;
    LayoutMode {
        inspector_overlay: large || width < WIDE_WIDTH,
        rail: large,
        one_pane: large,
        large_text: large,
    }
}

/// The text scale GTK's `gtk-xft-dpi` stands for (96 dpi × 1024 = 1.0).
pub fn text_scale_of_dpi(xft_dpi: i32) -> f64 {
    if xft_dpi <= 0 {
        return 1.0;
    }
    (f64::from(xft_dpi) / (96.0 * 1024.0) * 100.0).round() / 100.0
}

/// `css` with every `font-size: <n>px` multiplied by `scale`. RADE's sheets
/// give font sizes in px, which GTK never scales with the text-scaling factor;
/// the app reloads its sheet through this when the factor changes (R111.2).
pub fn scale_font_px(css: &str, scale: f64) -> String {
    if (scale - 1.0).abs() < f64::EPSILON {
        return css.to_string();
    }
    const KEY: &str = "font-size:";
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find(KEY) {
        let (head, tail) = rest.split_at(i + KEY.len());
        out.push_str(head);
        let ws = tail.len() - tail.trim_start().len();
        out.push_str(&tail[..ws]);
        let t = &tail[ws..];
        let n = t
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(t.len());
        match (t[..n].parse::<f64>(), t[n..].starts_with("px")) {
            (Ok(v), true) => {
                let scaled = (v * scale * 100.0).round() / 100.0;
                out.push_str(&format!("{scaled}px"));
                rest = &t[n + 2..];
            }
            _ => rest = t,
        }
    }
    out.push_str(rest);
    out
}

/// Large text: the rows whose heights the sheets give in px grow to their
/// text's natural height + N px (spec A +8 Navigator rows, D +12 palette
/// rows, F +10 `+` menu rows; `## Text scale 200 %`). GTK's `min-height` is
/// the content box's, so N px of vertical padding gives exactly that once
/// the text outgrows the px minimum.
pub const ROW_GROWTH: &[(&str, i32)] = &[
    (".nav-tree > row", 8),
    (".palette-item", 12),
    (".new-menu-item", 10),
];

/// RADE's sheet at `scale`: px font sizes scaled ([`scale_font_px`]) and,
/// from the rail's text scale, the [`ROW_GROWTH`] padding appended.
pub fn scale_sheet(css: &str, scale: f64) -> String {
    let mut out = scale_font_px(css, scale);
    if scale >= RAIL_TEXT_SCALE {
        out.push_str("\n/* R111.2: rows grow with their text */\n");
        for (sel, n) in ROW_GROWTH {
            let (top, bottom) = (n / 2, n - n / 2);
            out.push_str(&format!(
                "{sel} {{ padding-top: {top}px; padding-bottom: {bottom}px; }}\n"
            ));
        }
    }
    out
}

/// The command field (spec A.11): 440 px, shrinking to 280 px before the
/// breadcrumb ellipsizes (A "1280 px").
pub const COMMAND_FIELD_WIDTH: i32 = 440;
pub const COMMAND_FIELD_MIN: i32 = 280;

/// The command field's width when `free` px are left in the titlebar after
/// every other item at its natural width.
pub fn command_field_width(free: i32) -> i32 {
    free.clamp(COMMAND_FIELD_MIN, COMMAND_FIELD_WIDTH)
}

/// Whether the command field shows its placeholder: only when it fits next
/// to the icon and key caps (A "Text scale 200 %"; otherwise icon + keys).
pub fn command_placeholder_fits(field: i32, chrome: i32, placeholder: i32) -> bool {
    chrome + placeholder <= field
}

/// Whether breadcrumb segment `i` of `n` may ellipsize: the middle ones
/// (the Source), never the Project (first, when there is one) nor the last
/// (the branch / current page).
pub fn crumb_ellipsizes(i: usize, n: usize, has_project: bool) -> bool {
    let first_kept = has_project && i == 0;
    !first_kept && i + 1 < n
}

/// Which Statusline cells show in `avail` px (spec A "1280 px"): cells drop
/// in the order hints (rightmost first), the system monitor (R116.4),
/// ports, identity until the rest fits; Workspace, Project, Worktree,
/// counts and usage never drop.
/// `widths[i]` is cell `i`'s width with its share of the gaps.
pub fn statusline_fit(cells: &[Cell], widths: &[i32], avail: i32) -> Vec<bool> {
    let mut keep = vec![true; cells.len()];
    let mut total: i32 = widths.iter().sum();
    let mut order: Vec<usize> = Vec::new();
    for kind in [
        CellKind::Hint,
        CellKind::Monitor,
        CellKind::Ports,
        CellKind::Identity,
    ] {
        order.extend((0..cells.len()).rev().filter(|&i| cells[i].kind == kind));
    }
    for i in order {
        if total <= avail {
            break;
        }
        keep[i] = false;
        total -= widths.get(i).copied().unwrap_or(0);
    }
    keep
}

/// The focused Pane and the Title Bars under it when the center shows one
/// main Pane (A "Text scale 200 %"): the focused Pane (or the first, when
/// the focus is elsewhere) stays, the others keep their order as bars.
pub fn one_main_pane<'a>(
    panes: &[&'a str],
    focused: Option<&str>,
) -> Option<(&'a str, Vec<&'a str>)> {
    let main = panes
        .iter()
        .copied()
        .find(|p| Some(*p) == focused)
        .or_else(|| panes.first().copied())?;
    Some((main, panes.iter().copied().filter(|p| *p != main).collect()))
}

/// The Usage page's detail card and the gap before it (spec E.4).
pub const USAGE_DETAIL_WIDTH: i32 = 440;
pub const USAGE_GAP: i32 = 16;
/// The narrowest account list beside the detail card (E "1280 px").
pub const USAGE_LIST_MIN: i32 = 560;
/// The Tokens-per-day chart at 100 % (E.33) and at large text.
pub const USAGE_CHART_LARGE: i32 = 160;

/// Whether the detail card goes below the list (full width) on a page whose
/// content is `content` px wide: when the list beside it would be narrower
/// than 560 px (964 − 16 − 440 = 508 at 1280 px), or with large text.
pub fn usage_detail_below(content: i32, mode: LayoutMode) -> bool {
    mode.large_text || content - USAGE_GAP - USAGE_DETAIL_WIDTH < USAGE_LIST_MIN
}

/// The gap between Project home KPI cards (spec B.6).
pub const KPI_GAP: i32 = 20;

/// The width of each of `n` KPI cards sharing `content` px (B "1280 px":
/// (956 − 20 × 2) / 3 … each card shrinks equally).
pub fn kpi_width(content: i32, n: i32) -> i32 {
    if n <= 0 {
        return content;
    }
    (content - KPI_GAP * (n - 1)) / n
}

/// Whether the KPI cards go one per line (B "Text scale 200 %"): when a card
/// would be narrower than what its value and detail need.
pub fn kpis_one_per_line(content: i32, n: i32, need: i32) -> bool {
    kpi_width(content, n) < need
}

/// An overlay's width (palette, New Project dialog, `+` popover, R111):
/// `base` px, at large text `min(window − 32, cap)` (never narrower than
/// `base`, never wider than the window − 32).
pub fn overlay_width(window: i32, base: i32, cap: i32, mode: LayoutMode) -> i32 {
    let room = (window - 32).max(0);
    if mode.large_text {
        cap.min(room).max(base.min(room))
    } else {
        base.min(room)
    }
}

/// A Toast's widest text at large text (F "Text scale 200 %").
pub fn toast_max_width(window: i32) -> i32 {
    (window - 64).max(0)
}

/// Whether a row's parts (title, detail, hints, … each at natural width,
/// `gap` between them) fit on one line `avail` px wide; if not, the detail
/// moves to a second line (palette, `+` menu, tables at large text).
pub fn fits_one_line(parts: &[i32], gap: i32, avail: i32) -> bool {
    let n = parts.iter().filter(|w| **w > 0).count() as i32;
    parts.iter().sum::<i32>() + gap * (n - 1).max(0) <= avail
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r19_9_terminals_get_a_header_once_a_tab_is_split() {
        let shell = Pane::new("p_1".into(), "/w".into(), None, None);
        assert!(!has_header(&shell, 1));
        assert!(has_header(&shell, 2));
        assert_eq!(header_name(&shell), "Terminal");
        let mut agent = Pane::new("p_2".into(), "/w".into(), Some("codex".into()), None);
        agent.agent = Some("codex".into());
        assert!(has_header(&agent, 1));
        assert_eq!(header_name(&agent), "codex");
        let viewer = Pane::viewer("p_3".into(), "/w/a.md".into(), None);
        assert!(!has_header(&viewer, 2));
    }

    #[test]
    fn identity_mismatch_names_both_emails() {
        let w = identity_warning(Some("richard@acme.io"), Some("other@example.com")).unwrap();
        assert!(
            w.contains("other@example.com") && w.contains("richard@acme.io"),
            "{w}"
        );
        assert_eq!(
            identity_warning(Some("richard@acme.io"), Some("Richard@acme.io")),
            None
        );
        assert_eq!(identity_warning(Some("richard@acme.io"), None), None);
        assert_eq!(identity_warning(None, Some("other@example.com")), None);
        assert_eq!(identity_warning(Some("richard@acme.io"), Some(" ")), None);
    }

    #[test]
    fn r103_b_breadcrumb_of_a_worktree_with_base_and_ahead() {
        let c = breadcrumb(
            Some("Shop"),
            "shop-web",
            Some("checkout-v2"),
            Some("develop"),
            Some(3),
        );
        assert_eq!(c.text(), "Shop / shop-web / checkout-v2 ← develop ↑3");
        assert_eq!(c.tail.as_deref(), Some("← develop ↑3"));
    }

    #[test]
    fn r103_b_breadcrumb_without_project_ahead_or_worktree() {
        assert_eq!(
            breadcrumb(None, "api", Some("fix"), Some("main"), Some(0)).text(),
            "api / fix ← main"
        );
        assert_eq!(
            breadcrumb(Some("Shop"), "shop-web", None, Some("develop"), Some(2)).text(),
            "Shop / shop-web"
        );
        assert_eq!(
            breadcrumb(None, "api", Some("fix"), None, Some(4)).text(),
            "api / fix"
        );
    }

    #[test]
    fn r103_d_tab_label_user_then_agent_then_worktree() {
        assert_eq!(
            tab_label(Some("agents"), Some("planner"), None, "checkout-v2"),
            "agents"
        );
        assert_eq!(
            tab_label(None, Some("planner"), None, "checkout-v2"),
            "planner"
        );
        assert_eq!(tab_label(None, None, None, "checkout-v2"), "checkout-v2");
        assert_eq!(
            tab_label(Some("  "), None, Some("Review"), "checkout-v2"),
            "Review"
        );
        for l in [
            tab_label(None, None, None, "checkout-v2"),
            tab_label(None, Some(""), None, "w"),
        ] {
            assert!(!l.starts_with("Tab "), "{l}");
        }
        assert_eq!(tab_count(1), None);
        assert_eq!(tab_count(2), Some(2));
    }

    #[test]
    fn r103_d_special_labels_for_review_and_browser_tabs() {
        let review = Pane::new("p_1".into(), "/w".into(), None, None);
        let mut review = review;
        review.kind = PaneKind::Review { root: "/w".into() };
        assert_eq!(tab_special(&[&review]).as_deref(), Some("Review"));
        let mut b = Pane::new("p_2".into(), "/w".into(), None, None);
        b.kind = PaneKind::Browser {
            url: "http://localhost:5173/cart?x=1".into(),
            annotations: vec![],
        };
        assert_eq!(tab_special(&[&b]).as_deref(), Some("localhost:5173"));
        let shell = Pane::new("p_3".into(), "/w".into(), None, None);
        assert_eq!(tab_special(&[&shell, &b]), None);
        assert_eq!(tab_special(&[]), None);
    }

    #[test]
    fn r103_e_pane_header_parts_and_model_flags() {
        assert_eq!(
            pane_header(
                "tests",
                Some("codex"),
                Some("gpt-5"),
                Some(AgentStatus::Waiting)
            )
            .join(" · "),
            "tests · codex · gpt-5 · needs input"
        );
        assert_eq!(
            pane_header("planner", Some("claude"), None, Some(AgentStatus::Done)).join(" · "),
            "planner · claude · done"
        );
        assert_eq!(pane_header("sh", None, None, None), ["sh"]);
        let v = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        assert_eq!(model_of(&v("codex -m gpt-5")).as_deref(), Some("gpt-5"));
        assert_eq!(
            model_of(&v("claude --model opus --x")).as_deref(),
            Some("opus")
        );
        assert_eq!(
            model_of(&v("claude --model=sonnet")).as_deref(),
            Some("sonnet")
        );
        assert_eq!(model_of(&v("claude --model")), None);
        assert_eq!(model_of(&v("claude -m --yolo")), None);
        assert_eq!(model_of(&v("claude")), None);
    }

    #[test]
    fn r103_e_pane_header_text_reads_the_pane_command() {
        let mut p = Pane::new(
            "p_4".into(),
            "/w".into(),
            Some("fake-agent.sh codex -m gpt-5".into()),
            Some("tests".into()),
        );
        p.agent = Some("codex".into());
        p.status = Some(AgentStatus::Waiting);
        assert_eq!(pane_header_text(&p), "tests · codex · gpt-5 · needs input");
        p.invocation = Some(crate::preset::Invocation {
            argv: vec!["codex".into(), "--model".into(), "o3".into()],
            env: Default::default(),
        });
        assert_eq!(pane_header_text(&p), "tests · codex · o3 · needs input");
    }

    #[test]
    fn r103_j_statusline_cells_in_order() {
        let s = StatusInput {
            workspace: "acme".into(),
            project: Some("Shop".into()),
            worktree: Some(("checkout-v2".into(), Some((3, 0)))),
            counts: vec![
                (AgentStatus::Waiting, 1),
                (AgentStatus::Running, 1),
                (AgentStatus::Done, 2),
            ],
            ports: vec![5173],
            identity: Some("richard@acme.io".into()),
            usage: Some("claude 5h 62% · wk 41%".into()),
        };
        let cells = statusline_cells(&s);
        let texts: Vec<&str> = cells.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "ACME",
                "Shop",
                "checkout-v2 ↑3 ↓0",
                "1 waiting · 1 running · 2 done",
                ":5173",
                "claude 5h 62% · wk 41%",
                "richard@acme.io",
                "Ctrl ⇧ P commands",
                "Ctrl ⇧ G next waiting",
            ]
        );
        assert_eq!(cells[0].kind, CellKind::Workspace);
        assert_eq!(cells[5].kind, CellKind::Usage);
        assert_eq!(cells[7].kind, CellKind::Hint);
    }

    #[test]
    fn r103_j_statusline_leaves_empty_cells_out() {
        let s = StatusInput {
            workspace: "default".into(),
            worktree: Some(("main".into(), None)),
            ..Default::default()
        };
        let texts: Vec<String> = statusline_cells(&s).into_iter().map(|c| c.text).collect();
        assert_eq!(
            texts,
            [
                "DEFAULT",
                "main",
                "Ctrl ⇧ P commands",
                "Ctrl ⇧ G next waiting"
            ]
        );
    }

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn r103_g_toast_clear_of_buttons_stays() {
        let toast = r(500.0, 800.0, 300.0, 40.0);
        assert_eq!(toast_lift(&toast, &[r(10.0, 800.0, 80.0, 26.0)], 0.0), 0.0);
        assert_eq!(toast_lift(&toast, &[], 0.0), 0.0);
        // Touching edges is not an overlap.
        assert_eq!(toast_lift(&toast, &[r(500.0, 840.0, 80.0, 26.0)], 0.0), 0.0);
    }

    #[test]
    fn r103_g_toast_moves_above_the_button_it_covers() {
        let toast = r(500.0, 800.0, 300.0, 40.0);
        // A Send to Agent button at y 810..836: the toast's bottom goes to
        // 810 − 8 (gap), so it rises 840 − 802 = 38 px.
        let lift = toast_lift(&toast, &[r(700.0, 810.0, 110.0, 26.0)], 0.0);
        assert_eq!(lift, 38.0);
        let moved = r(500.0, 800.0 - lift, 300.0, 40.0);
        assert!(!moved.intersects(&r(700.0, 810.0, 110.0, 26.0)));
    }

    #[test]
    fn r103_g_toast_climbs_past_stacked_buttons() {
        let toast = r(500.0, 800.0, 300.0, 40.0);
        // After clearing the lower button it would land on a second one.
        let buttons = [r(600.0, 820.0, 80.0, 26.0), r(520.0, 760.0, 80.0, 26.0)];
        let lift = toast_lift(&toast, &buttons, 0.0);
        let moved = r(500.0, 800.0 - lift, 300.0, 40.0);
        assert!(buttons.iter().all(|b| !moved.intersects(b)), "{lift}");
        assert_eq!(moved.y + moved.h, 760.0 - TOAST_GAP);
    }

    #[test]
    fn r103_g_toast_never_leaves_the_top() {
        let toast = r(0.0, 100.0, 300.0, 40.0);
        let lift = toast_lift(&toast, &[r(0.0, 50.0, 300.0, 80.0)], 60.0);
        assert_eq!(lift, 40.0, "stops at the top bound 60");
    }

    #[test]
    fn r103_h_empty_states_are_one_line_with_key_hints() {
        for e in [
            EMPTY_CHANGES,
            EMPTY_PROJECT_CHANGES,
            EMPTY_NO_REPO,
            EMPTY_FILES,
            EMPTY_SEARCH,
            EMPTY_ACTIVITY,
            EMPTY_TAB,
        ] {
            assert!(!e.line.contains('\n') && !e.line.is_empty());
            assert!(!e.hints.is_empty(), "{}", e.line);
        }
        assert_eq!(EMPTY_TAB.line, "No terminals here");
        assert_eq!(EMPTY_TAB.hints[0], ("Ctrl ⇧ T", "shell"));
    }

    #[test]
    fn r103_i_inspector_tab_label_counts_from_one() {
        assert_eq!(inspector_label("Changes", 7), "Changes 7");
        assert_eq!(inspector_label("Changes", 0), "Changes");
        assert_eq!(INSPECTOR_TABS.map(|t| t.1), ["Changes", "Files", "Search"]);
    }

    #[test]
    fn r111_a_layout_mode_by_width() {
        let wide = layout_mode(1440, 1.0);
        assert_eq!(
            wide,
            LayoutMode::default(),
            "1440 × 900 at 100 % is the golden layout"
        );
        assert_eq!(wide.name(), "wide");
        assert_eq!(layout_mode(1920, 1.0), LayoutMode::default());
        let narrow = layout_mode(1280, 1.0);
        assert!(narrow.inspector_overlay && !narrow.rail && !narrow.one_pane && !narrow.large_text);
        assert_eq!(narrow.name(), "narrow");
        assert!(layout_mode(1439, 1.25).inspector_overlay);
        assert!(
            !layout_mode(1440, 1.25).inspector_overlay,
            "1.25 keeps the full layout"
        );
    }

    #[test]
    fn r111_b_rail_and_one_pane_from_text_scale_1_5() {
        for (w, s) in [(1440, 2.0), (1280, 1.5), (2560, 2.0)] {
            let m = layout_mode(w, s);
            assert!(
                m.rail && m.one_pane && m.inspector_overlay && m.large_text,
                "{w} {s}"
            );
            assert_eq!(m.name(), "rail");
        }
        assert!(!layout_mode(1440, 1.49).rail);
        assert_eq!(text_scale_of_dpi(96 * 1024), 1.0);
        assert_eq!(text_scale_of_dpi(2 * 96 * 1024), 2.0);
        assert_eq!(text_scale_of_dpi(122880), 1.25);
        assert_eq!(text_scale_of_dpi(0), 1.0);
    }

    #[test]
    fn r111_b_font_sizes_in_px_scale_with_the_text() {
        let css = ".a { font-size: 13px; } .b{font-size:12.5px} .c { font-size: 0.9em; } .d { min-height: 26px; }";
        assert_eq!(
            scale_font_px(css, 2.0),
            ".a { font-size: 26px; } .b{font-size:25px} .c { font-size: 0.9em; } .d { min-height: 26px; }"
        );
        assert_eq!(scale_font_px(css, 1.0), css);
        assert_eq!(
            scale_font_px("x { font-size: 11.5px }", 1.5),
            "x { font-size: 17.25px }"
        );
        assert_eq!(scale_font_px("font-size: px", 2.0), "font-size: px");
    }

    #[test]
    fn r111_b_rows_grow_to_their_text_plus_the_sheets_padding() {
        let css = scale_sheet(".a { font-size: 13px; }", 2.0);
        assert!(css.starts_with(".a { font-size: 26px; }"));
        // A: +8, D: +12, F: +10 (the sheets' `## Text scale 200 %`).
        for (sel, half) in [
            (".nav-tree > row", 4),
            (".palette-item", 6),
            (".new-menu-item", 5),
        ] {
            let rule = format!("{sel} {{ padding-top: {half}px; padding-bottom: {half}px; }}");
            assert!(css.contains(&rule), "{rule} in {css}");
        }
        let big = scale_sheet("", 1.5);
        assert!(big.contains(".palette-item { padding-top: 6px;"));
        // Below the rail's scale the px heights hold the text: unchanged.
        assert_eq!(
            scale_sheet(".a { font-size: 13px; }", 1.0),
            ".a { font-size: 13px; }"
        );
        assert!(!scale_sheet("", 1.25).contains("padding"));
    }

    #[test]
    fn r111_a_command_field_shrinks_to_280_first() {
        assert_eq!(command_field_width(600), 440);
        assert_eq!(command_field_width(440), 440);
        assert_eq!(command_field_width(350), 350);
        assert_eq!(
            command_field_width(100),
            280,
            "then the breadcrumb ellipsizes"
        );
        assert!(command_placeholder_fits(440, 120, 200));
        assert!(
            !command_placeholder_fits(280, 160, 330),
            "icon and key caps only"
        );
        // Shop / shop-web / checkout-v2: only shop-web ellipsizes.
        assert!(!crumb_ellipsizes(0, 3, true));
        assert!(crumb_ellipsizes(1, 3, true));
        assert!(!crumb_ellipsizes(2, 3, true));
        // shop-web / checkout-v2 (no Project): the Source may.
        assert!(crumb_ellipsizes(0, 2, false));
    }

    fn cells_a() -> Vec<Cell> {
        statusline_cells(&StatusInput {
            workspace: "acme".into(),
            project: Some("Shop".into()),
            worktree: Some(("checkout-v2".into(), Some((3, 0)))),
            counts: vec![(AgentStatus::Waiting, 1)],
            ports: vec![5173],
            identity: Some("richard@acme.io".into()),
            usage: Some("claude 5h 62% · wk 41%".into()),
        })
    }

    #[test]
    fn r116_c_the_monitor_drops_after_the_hints_before_ports() {
        let mut cells = cells_a();
        let at = cells.len() - 2;
        cells.insert(
            at,
            Cell {
                kind: CellKind::Monitor,
                text: "cpu  34% · mem  61% · ↓0B ↑0B".into(),
            },
        );
        // ws project worktree counts ports usage identity monitor hint hint
        let widths = vec![60, 60, 120, 140, 70, 180, 140, 200, 130, 140];
        let total: i32 = widths.iter().sum();
        let shown = |avail| {
            statusline_fit(&cells, &widths, avail)
                .iter()
                .zip(&cells)
                .filter(|(k, _)| **k)
                .map(|(_, c)| c.kind.as_str())
                .collect::<Vec<_>>()
                .join(",")
        };
        assert_eq!(
            shown(total - 270),
            "ws,project,worktree,counts,ports,usage,identity,monitor"
        );
        assert_eq!(
            shown(total - 271),
            "ws,project,worktree,counts,ports,usage,identity"
        );
        assert_eq!(
            shown(total - 471),
            "ws,project,worktree,counts,usage,identity"
        );
    }

    #[test]
    fn r111_a_statusline_drops_hints_then_port_then_identity() {
        let cells = cells_a();
        let kinds: Vec<CellKind> = cells.iter().map(|c| c.kind).collect();
        // ws project worktree counts ports usage identity hint hint
        let widths = vec![60, 60, 120, 140, 70, 180, 140, 130, 140];
        let total: i32 = widths.iter().sum();
        let shown = |avail| {
            statusline_fit(&cells, &widths, avail)
                .iter()
                .zip(&kinds)
                .filter(|(k, _)| **k)
                .map(|(_, c)| c.as_str())
                .collect::<Vec<_>>()
                .join(",")
        };
        assert_eq!(
            shown(total),
            "ws,project,worktree,counts,ports,usage,identity,hint,hint"
        );
        assert_eq!(
            shown(total - 1),
            "ws,project,worktree,counts,ports,usage,identity,hint"
        );
        assert_eq!(
            shown(total - 141),
            "ws,project,worktree,counts,ports,usage,identity"
        );
        assert_eq!(
            shown(total - 271),
            "ws,project,worktree,counts,usage,identity"
        );
        assert_eq!(shown(total - 341), "ws,project,worktree,counts,usage");
        // Never Workspace, counts or usage, however narrow.
        assert_eq!(shown(10), "ws,project,worktree,counts,usage");
    }

    #[test]
    fn r111_b_one_main_pane_keeps_the_focused_one() {
        let panes = ["planner", "tests", "ui"];
        assert_eq!(
            one_main_pane(&panes, Some("tests")),
            Some(("tests", vec!["planner", "ui"]))
        );
        assert_eq!(
            one_main_pane(&panes, Some("elsewhere")),
            Some(("planner", vec!["tests", "ui"]))
        );
        assert_eq!(one_main_pane(&["solo"], None), Some(("solo", vec![])));
        assert_eq!(one_main_pane(&[], Some("x")), None);
    }

    #[test]
    fn r111_a_usage_detail_moves_below_a_list_under_560() {
        let wide = layout_mode(1440, 1.0);
        // 1440: 1440 − 268 − 48 = 1124 content → list 668 beside the card.
        assert!(!usage_detail_below(1124, wide));
        // 1280: 964 − 16 − 440 = 508 < 560.
        assert!(usage_detail_below(964, layout_mode(1280, 1.0)));
        assert!(!usage_detail_below(1016, wide), "560 exactly fits");
        assert!(usage_detail_below(1015, wide));
        assert!(
            usage_detail_below(2000, layout_mode(1440, 2.0)),
            "one column at large text"
        );
    }

    #[test]
    fn r111_a_kpi_cards_shrink_equally_and_wrap_at_large_text() {
        assert_eq!(kpi_width(956, 3), 305);
        assert_eq!(kpi_width(1068, 3), 342);
        assert!(!kpis_one_per_line(956, 3, 260));
        assert!(
            kpis_one_per_line(1336, 3, 520),
            "a 2× value + detail no longer fits"
        );
        assert_eq!(kpi_width(500, 0), 500);
    }

    #[test]
    fn r111_b_overlays_widen_at_large_text() {
        let (wide, large) = (layout_mode(1280, 1.0), layout_mode(1440, 2.0));
        assert_eq!(
            overlay_width(1280, 720, 1100, wide),
            720,
            "palette keeps 720 at 1280"
        );
        assert_eq!(overlay_width(1440, 720, 1100, large), 1100);
        assert_eq!(overlay_width(1440, 680, 1000, large), 1000);
        assert_eq!(overlay_width(900, 680, 1000, large), 868, "window − 32");
        assert_eq!(overlay_width(700, 680, 1000, wide), 668);
        assert_eq!(toast_max_width(1440), 1376);
        assert!(fits_one_line(&[200, 150, 80], 10, 450));
        assert!(!fits_one_line(&[200, 150, 80], 10, 449));
        assert!(
            fits_one_line(&[200, 0, 80], 10, 290),
            "an empty detail takes no gap"
        );
    }
}
