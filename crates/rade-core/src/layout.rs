//! Layout: the binary split tree of Panes inside one Tab (R1.3).

use crate::agents::AgentStatus;
use crate::model::Tab;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitDir {
    /// New Pane to the right (side by side).
    Right,
    /// New Pane below (stacked).
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pane {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub kind: PaneKind,
    /// Agent Preset name (R16); set at spawn or by a session-start hook.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// The Preset whose program runs in the Terminal's foreground, found
    /// from its process while no hook has said (R115.4): shows the agent's
    /// glyph; never saved, so Session Restore is unchanged.
    #[serde(skip)]
    pub detected: Option<String>,
    /// The Pane that spawned this one (its Parent, R2.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Spawn depth (R2.5): 0 unless spawned from another Pane.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub depth: u32,
    /// Agent Status (R10.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<AgentStatus>,
    /// When `status` last changed (Unix seconds; tooltips, R10.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_since: Option<u64>,
    /// `status` came from an R10.2 fallback rule, not a hook or RADE itself.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub status_heuristic: bool,
    /// When an Agent hook last reported (`rade status`, `rade notify
    /// --status`); `None` = its hooks never reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hook_at: Option<u64>,
    /// Statuses a hook reported for this Pane (bit = `AgentStatus` index);
    /// transient, like the status itself after a restart.
    #[serde(skip)]
    pub hook_statuses: u8,
    /// The Agent's resumable session id (Session Restore, R11.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// A hook reported a turn (running/waiting/done/error), so the session
    /// has a conversation to resume. A never-prompted session cannot be
    /// resumed ("No conversation found").
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub had_turn: bool,
    /// How the Agent was really started (argv + config env), reported by its
    /// session-start hook; Session Restore re-runs it (R11.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation: Option<crate::preset::Invocation>,
    /// The Worktree's `setup` command this Terminal runs before its program
    /// (R12.2); transient: a restored Pane never runs it again.
    #[serde(skip)]
    pub setup: Option<String>,
    /// The Worktree (id) this Pane runs in when it is not its Tab's: a
    /// Fan-out Agent shown in the Fan-out Tab of the main Worktree (R15.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs_in: Option<String>,
    /// The last terminal title (OSC 0/2) its program set, sanitized
    /// ([`crate::title`]); kept so a restored Tab shows it until the
    /// program sets a new one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The Agent Account this Pane runs under (R108.2): its name in the
    /// Workspace's `[[account]]` list; kept by Session Restore.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// The agent of `account` (a claude and a codex account may share a
    /// name, R108.1): what finds the account again once the Pane is no
    /// longer an Agent Pane (a refused restore, R108.7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_agent: Option<crate::account::AgentKind>,
    /// A line the restored Terminal prints before its shell (R108.7: why a
    /// recorded invocation was not run); transient.
    #[serde(skip)]
    pub notice: Option<String>,
    /// The prompt RADE spawned this Agent with (R118.1): the Review
    /// header's task line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PaneKind {
    Terminal {
        cwd: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command: Option<String>,
    },
    /// Read-only view of a file (R7.2); `back` = files it showed before,
    /// newest last (Markdown link navigation, R7.3).
    Viewer {
        path: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line: Option<u32>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        back: Vec<BackEntry>,
        /// Places Back left, newest last (Alt+Right, R84.3).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        forward: Vec<BackEntry>,
    },
    /// A Worktree's diff and Review Comments (R13); `root` = the Worktree.
    Review { root: PathBuf },
    /// A web page (R18); `url` = the page it shows, reloaded on restore;
    /// the owner's Browser Annotations not sent yet (R71).
    Browser {
        url: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        annotations: Vec<crate::annotate::Annotation>,
    },
}

/// One file on a Viewer's Back list, with the line it was shown at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "BackRepr")]
pub struct BackEntry {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// state.json before M6 stored Back entries as bare paths.
#[derive(Deserialize)]
#[serde(untagged)]
enum BackRepr {
    Path(PathBuf),
    Entry {
        path: PathBuf,
        #[serde(default)]
        line: Option<u32>,
    },
}

impl From<BackRepr> for BackEntry {
    fn from(r: BackRepr) -> Self {
        match r {
            BackRepr::Path(path) => Self { path, line: None },
            BackRepr::Entry { path, line } => Self { path, line },
        }
    }
}

impl Pane {
    pub fn new(id: String, cwd: PathBuf, command: Option<String>, name: Option<String>) -> Self {
        Self {
            id,
            name,
            kind: PaneKind::Terminal { cwd, command },
            agent: None,
            detected: None,
            parent: None,
            depth: 0,
            status: None,
            status_since: None,
            status_heuristic: false,
            hook_at: None,
            hook_statuses: 0,
            session_id: None,
            had_turn: false,
            invocation: None,
            setup: None,
            runs_in: None,
            title: None,
            account: None,
            account_agent: None,
            notice: None,
            task: None,
        }
    }

    /// Runs a program RADE launched (Preset or `--cmd`), not the user's shell:
    /// gets the Agent scrollback budget (R2.7) and stays open after exit.
    pub fn is_program(&self) -> bool {
        self.agent.is_some() || self.command().is_some()
    }

    /// A Terminal's cwd; a Viewer's file; a Review's Worktree; empty for a
    /// Browser (see [`Pane::url`]).
    pub fn cwd(&self) -> &PathBuf {
        static NONE: PathBuf = PathBuf::new();
        match &self.kind {
            PaneKind::Terminal { cwd, .. } => cwd,
            PaneKind::Viewer { path, .. } => path,
            PaneKind::Review { root } => root,
            PaneKind::Browser { .. } => &NONE,
        }
    }

    pub fn command(&self) -> Option<&str> {
        match &self.kind {
            PaneKind::Terminal { command, .. } => command.as_deref(),
            _ => None,
        }
    }

    pub fn is_viewer(&self) -> bool {
        matches!(self.kind, PaneKind::Viewer { .. })
    }

    pub fn is_review(&self) -> bool {
        matches!(self.kind, PaneKind::Review { .. })
    }
}

impl Tab {
    /// Every Pane: the Layout's, then the Title Bars'.
    pub fn panes(&self) -> Vec<&Pane> {
        let mut all = self.layout.panes();
        all.extend(self.title_bars.iter());
        all
    }

    pub fn pane(&self, id: &str) -> Option<&Pane> {
        self.panes().into_iter().find(|p| p.id == id)
    }

    pub fn pane_mut(&mut self, id: &str) -> Option<&mut Pane> {
        match self.layout.pane_mut(id) {
            Some(p) => Some(p),
            None => self.title_bars.iter_mut().find(|p| p.id == id),
        }
    }

    pub fn pane_ids(&self) -> Vec<String> {
        self.panes().iter().map(|p| p.id.clone()).collect()
    }

    /// Re-tile the Agent Grid (R2.4) if the Tab is auto-arranged: Parent
    /// left, first [`crate::layout::GRID_TILES`] others tiled, the rest Title Bars.
    pub(crate) fn arrange(&mut self) {
        let Some(parent_id) = self.grid.clone() else {
            return;
        };
        let mut others: Vec<Pane> = self.panes().into_iter().cloned().collect();
        let Some(i) = others.iter().position(|p| p.id == parent_id) else {
            self.grid = None;
            return;
        };
        let parent = others.remove(i);
        let n = others.len().min(crate::layout::GRID_TILES);
        self.title_bars = others.split_off(n);
        self.layout = Layout::grid(parent, others);
    }
}

// A Tab holds a handful of Panes: boxing leaves to save bytes isn't worth
// the churn.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Layout {
    Leaf {
        pane: Pane,
    },
    Split {
        dir: SplitDir,
        /// Share of the space given to `a` (0..1).
        ratio: f64,
        a: Box<Layout>,
        b: Box<Layout>,
    },
}

/// Most Subagents tiled in an Agent Grid (= default `limits.max_children`);
/// only a raised limit leaves extra Subagents as Title Bars (R2.4).
pub const GRID_TILES: usize = 8;

/// Axis-aligned rectangle in the unit square.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

const EPS: f64 = 1e-9;

impl Layout {
    pub fn leaf(pane: Pane) -> Self {
        Layout::Leaf { pane }
    }

    pub(crate) fn split_of(dir: SplitDir, ratio: f64, a: Layout, b: Layout) -> Self {
        Layout::Split {
            dir,
            ratio,
            a: Box::new(a),
            b: Box::new(b),
        }
    }

    /// Agent Grid (R2.4): Parent on the left, up to [`GRID_TILES`] children
    /// tiled on the right: 1 full, 2–3 stacked, 4–8 in two columns (column
    /// by column). Reading order stays parent, children…, so re-arranging is
    /// stable.
    pub fn grid(parent: Pane, children: Vec<Pane>) -> Self {
        use SplitDir::{Down, Right};
        let mut c: Vec<Layout> = children.into_iter().map(Layout::leaf).collect();
        assert!(c.len() <= GRID_TILES, "overflow children are Title Bars");
        let right = match c.len() {
            0 => return Layout::leaf(parent),
            1..=3 => Layout::even(Down, c),
            n => {
                let second = c.split_off(n.div_ceil(2));
                Layout::even(
                    Right,
                    vec![Layout::even(Down, c), Layout::even(Down, second)],
                )
            }
        };
        Layout::split_of(Right, 0.5, Layout::leaf(parent), right)
    }

    /// Siblings tiled evenly (Fan-out Tab, R15.2): up to 3 side by side,
    /// then two rows (4 = 2×2, 5–6 = 3 over 3, 7–8 = 4 over 4), reading
    /// order row by row.
    pub fn tiles(panes: Vec<Pane>) -> Self {
        use SplitDir::{Down, Right};
        let mut c: Vec<Layout> = panes.into_iter().map(Layout::leaf).collect();
        assert!(!c.is_empty(), "at least one Pane");
        if c.len() <= 3 {
            return Layout::even(Right, c);
        }
        let second = c.split_off(c.len().div_ceil(2));
        Layout::even(
            Down,
            vec![Layout::even(Right, c), Layout::even(Right, second)],
        )
    }

    /// `items` side by side along `dir`, equal sizes.
    fn even(dir: SplitDir, mut items: Vec<Layout>) -> Self {
        let last = items.pop().expect("at least one item");
        let mut n = 1.0;
        items.into_iter().rev().fold(last, |acc, x| {
            // The new first item gets 1/(items after it + 1).
            n += 1.0;
            Layout::split_of(dir, 1.0 / n, x, acc)
        })
    }

    /// Panes in reading order (a before b).
    pub fn panes(&self) -> Vec<&Pane> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect<'a>(&'a self, out: &mut Vec<&'a Pane>) {
        match self {
            Layout::Leaf { pane } => out.push(pane),
            Layout::Split { a, b, .. } => {
                a.collect(out);
                b.collect(out);
            }
        }
    }

    pub fn pane(&self, id: &str) -> Option<&Pane> {
        self.panes().into_iter().find(|p| p.id == id)
    }

    pub fn pane_mut(&mut self, id: &str) -> Option<&mut Pane> {
        match self {
            Layout::Leaf { pane } => (pane.id == id).then_some(pane),
            Layout::Split { a, b, .. } => a.pane_mut(id).or_else(|| b.pane_mut(id)),
        }
    }

    pub fn first_pane(&self) -> &Pane {
        match self {
            Layout::Leaf { pane } => pane,
            Layout::Split { a, .. } => a.first_pane(),
        }
    }

    /// Replace the leaf `target` with a split holding it (a) and `new` (b).
    /// Returns false when `target` is not in this Layout.
    pub fn split(&mut self, target: &str, dir: SplitDir, new: Pane) -> bool {
        match self {
            Layout::Leaf { pane } if pane.id == target => {
                let old = Layout::leaf(pane.clone());
                *self = Layout::Split {
                    dir,
                    ratio: 0.5,
                    a: Box::new(old),
                    b: Box::new(Layout::leaf(new)),
                };
                true
            }
            Layout::Leaf { .. } => false,
            Layout::Split { a, b, .. } => {
                a.split(target, dir, new.clone()) || b.split(target, dir, new)
            }
        }
    }

    /// Remove the leaf `id`; its sibling takes the parent's place.
    /// Returns `Err(self)` unchanged if `id` is absent, `Ok(None)` if the
    /// Layout is now empty.
    #[allow(clippy::result_large_err)] // Err returns the tree itself, by value
    pub fn remove(self, id: &str) -> Result<Option<Layout>, Layout> {
        match self {
            Layout::Leaf { ref pane } if pane.id == id => Ok(None),
            leaf @ Layout::Leaf { .. } => Err(leaf),
            Layout::Split { dir, ratio, a, b } => match a.remove(id) {
                Ok(None) => Ok(Some(*b)),
                Ok(Some(a)) => Ok(Some(Layout::Split {
                    dir,
                    ratio,
                    a: Box::new(a),
                    b,
                })),
                Err(a) => match b.remove(id) {
                    Ok(None) => Ok(Some(a)),
                    Ok(Some(b)) => Ok(Some(Layout::Split {
                        dir,
                        ratio,
                        a: Box::new(a),
                        b: Box::new(b),
                    })),
                    Err(b) => Err(Layout::Split {
                        dir,
                        ratio,
                        a: Box::new(a),
                        b: Box::new(b),
                    }),
                },
            },
        }
    }

    /// Set the ratio of the split reached by `path` (false = a, true = b).
    pub fn set_ratio(&mut self, path: &[bool], ratio: f64) -> bool {
        match (self, path.split_first()) {
            (Layout::Split { ratio: r, .. }, None) => {
                *r = ratio.clamp(0.05, 0.95);
                true
            }
            (Layout::Split { a, b, .. }, Some((side, rest))) => {
                if *side {
                    b.set_ratio(rest, ratio)
                } else {
                    a.set_ratio(rest, ratio)
                }
            }
            (Layout::Leaf { .. }, _) => false,
        }
    }

    /// Each Pane's rectangle when the Tab is the unit square.
    pub fn rects(&self) -> Vec<(&str, Rect)> {
        let mut out = Vec::new();
        self.place(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 1.0,
                h: 1.0,
            },
            &mut out,
        );
        out
    }

    fn place<'a>(&'a self, r: Rect, out: &mut Vec<(&'a str, Rect)>) {
        match self {
            Layout::Leaf { pane } => out.push((&pane.id, r)),
            Layout::Split { dir, ratio, a, b } => {
                let (ra, rb) = match dir {
                    SplitDir::Right => {
                        let wa = r.w * ratio;
                        (
                            Rect { w: wa, ..r },
                            Rect {
                                x: r.x + wa,
                                w: r.w - wa,
                                ..r
                            },
                        )
                    }
                    SplitDir::Down => {
                        let ha = r.h * ratio;
                        (
                            Rect { h: ha, ..r },
                            Rect {
                                y: r.y + ha,
                                h: r.h - ha,
                                ..r
                            },
                        )
                    }
                };
                a.place(ra, out);
                b.place(rb, out);
            }
        }
    }

    /// The nearest Pane from `from` in `dir`, by geometry: closest edge
    /// first, then the largest shared border.
    pub fn neighbor(&self, from: &str, dir: Direction) -> Option<String> {
        let rects = self.rects();
        let f = rects.iter().find(|(id, _)| *id == from)?.1;
        rects
            .iter()
            .filter(|(id, _)| *id != from)
            .filter_map(|(id, c)| {
                let (gap, overlap) = match dir {
                    Direction::Left => (f.x - (c.x + c.w), overlap(f.y, f.h, c.y, c.h)),
                    Direction::Right => (c.x - (f.x + f.w), overlap(f.y, f.h, c.y, c.h)),
                    Direction::Up => (f.y - (c.y + c.h), overlap(f.x, f.w, c.x, c.w)),
                    Direction::Down => (c.y - (f.y + f.h), overlap(f.x, f.w, c.x, c.w)),
                };
                (gap > -EPS && overlap > EPS).then_some((id, gap, overlap))
            })
            .min_by(|x, y| {
                x.1.partial_cmp(&y.1)
                    .unwrap()
                    .then(y.2.partial_cmp(&x.2).unwrap())
            })
            .map(|(id, _, _)| id.to_string())
    }

    /// Shape of the tree (Pane ids and split directions, not ratios); the UI
    /// rebuilds a Tab's widgets only when this changes.
    pub fn signature(&self) -> String {
        match self {
            Layout::Leaf { pane } => pane.id.clone(),
            Layout::Split { dir, a, b, .. } => {
                let d = match dir {
                    SplitDir::Right => 'R',
                    SplitDir::Down => 'D',
                };
                format!("{d}({},{})", a.signature(), b.signature())
            }
        }
    }
}

fn overlap(a: f64, al: f64, b: f64, bl: f64) -> f64 {
    (a + al).min(b + bl) - a.max(b)
}

/// Where keyboard focus goes when the app re-renders (R1.3, R9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusAction {
    /// Grab it for the focused Pane's Terminal.
    Terminal,
    /// Hand it to the focused Pane's Viewer.
    Viewer,
    /// Hand it to the focused Pane's Review.
    Review,
    /// Hand it to the focused Pane's Browser.
    Browser,
    /// Leave it where it is.
    Keep,
}

impl FocusAction {
    /// Decide for the Tab on screen's focused Pane. Grab even when that
    /// Terminal still has focus: re-parenting it (a caption appearing, a
    /// split) makes GTK move focus elsewhere after the next frame unless it
    /// is grabbed again (live bug: typing `clw` with the file tree open lost
    /// the keyboard). Only a text entry the user is typing in keeps focus.
    pub fn decide(focused: Option<&Pane>, text_entry_has_focus: bool) -> Self {
        match focused {
            _ if text_entry_has_focus => FocusAction::Keep,
            Some(p) if p.is_viewer() => FocusAction::Viewer,
            Some(p) if p.is_review() => FocusAction::Review,
            Some(p) if p.is_browser() => FocusAction::Browser,
            Some(_) => FocusAction::Terminal,
            None => FocusAction::Keep,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_returns_to_the_focused_pane_unless_typing_in_an_entry() {
        let t = p("p_1");
        let v = Pane::viewer("p_2".into(), "/a.md".into(), None);
        assert_eq!(FocusAction::decide(Some(&t), false), FocusAction::Terminal);
        assert_eq!(FocusAction::decide(Some(&v), false), FocusAction::Viewer);
        assert_eq!(FocusAction::decide(Some(&t), true), FocusAction::Keep);
        assert_eq!(FocusAction::decide(None, false), FocusAction::Keep);
    }

    fn p(id: &str) -> Pane {
        Pane::new(id.into(), "/w".into(), None, None)
    }

    fn ids(l: &Layout) -> Vec<&str> {
        l.panes().iter().map(|p| p.id.as_str()).collect()
    }

    /// p1 | (p2 / p3)
    fn three() -> Layout {
        let mut l = Layout::leaf(p("p1"));
        assert!(l.split("p1", SplitDir::Right, p("p2")));
        assert!(l.split("p2", SplitDir::Down, p("p3")));
        l
    }

    #[test]
    fn split_replaces_leaf_with_split_and_keeps_order() {
        let l = three();
        assert_eq!(ids(&l), ["p1", "p2", "p3"]);
        assert_eq!(l.signature(), "R(p1,D(p2,p3))");
    }

    #[test]
    fn split_unknown_target_is_noop() {
        let mut l = Layout::leaf(p("p1"));
        assert!(!l.split("nope", SplitDir::Right, p("p2")));
        assert_eq!(ids(&l), ["p1"]);
    }

    #[test]
    fn remove_collapses_split_to_sibling() {
        let l = three().remove("p2").unwrap().unwrap();
        assert_eq!(l.signature(), "R(p1,p3)");
        let l = l.remove("p1").unwrap().unwrap();
        assert_eq!(l.signature(), "p3");
        assert_eq!(l.remove("p3").unwrap(), None);
    }

    #[test]
    fn remove_missing_returns_original() {
        let l = three();
        let back = l.clone().remove("zz").unwrap_err();
        assert_eq!(back, l);
    }

    #[test]
    fn remove_keeps_ratio_of_surviving_split() {
        let mut l = three();
        assert!(l.set_ratio(&[], 0.3));
        let l = l.remove("p3").unwrap().unwrap();
        assert!(matches!(l, Layout::Split { ratio, .. } if (ratio - 0.3).abs() < 1e-9));
    }

    #[test]
    fn rects_follow_ratios() {
        let mut l = three();
        l.set_ratio(&[], 0.25);
        l.set_ratio(&[true], 0.5);
        let r = l.rects();
        assert_eq!(
            r[0],
            (
                "p1",
                Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 0.25,
                    h: 1.0
                }
            )
        );
        assert_eq!(
            r[1],
            (
                "p2",
                Rect {
                    x: 0.25,
                    y: 0.0,
                    w: 0.75,
                    h: 0.5
                }
            )
        );
        assert_eq!(
            r[2],
            (
                "p3",
                Rect {
                    x: 0.25,
                    y: 0.5,
                    w: 0.75,
                    h: 0.5
                }
            )
        );
    }

    #[test]
    fn set_ratio_clamps_and_rejects_leaf_paths() {
        let mut l = three();
        assert!(l.set_ratio(&[], 2.0));
        assert!(matches!(l, Layout::Split { ratio, .. } if ratio == 0.95));
        assert!(!l.set_ratio(&[false], 0.5), "path into a leaf");
    }

    #[test]
    fn neighbor_uses_geometry() {
        let l = three();
        assert_eq!(l.neighbor("p1", Direction::Right).as_deref(), Some("p2"));
        assert_eq!(l.neighbor("p3", Direction::Left).as_deref(), Some("p1"));
        assert_eq!(l.neighbor("p2", Direction::Down).as_deref(), Some("p3"));
        assert_eq!(l.neighbor("p3", Direction::Up).as_deref(), Some("p2"));
        assert_eq!(l.neighbor("p1", Direction::Left), None);
        assert_eq!(l.neighbor("p2", Direction::Up), None);
    }

    #[test]
    fn neighbor_prefers_largest_shared_border() {
        // (p1 / p2) | p3 with p1 taking 70% height: from p3 going left hits p1.
        let mut l = Layout::leaf(p("p1"));
        l.split("p1", SplitDir::Right, p("p3"));
        l.split("p1", SplitDir::Down, p("p2"));
        l.set_ratio(&[false], 0.7);
        assert_eq!(l.neighbor("p3", Direction::Left).as_deref(), Some("p1"));
    }

    #[test]
    fn r2_4_grid_shapes_keep_reading_order() {
        let kids = |n: usize| (1..=n).map(|i| p(&format!("c{i}"))).collect::<Vec<_>>();
        let sig = |n| Layout::grid(p("P"), kids(n)).signature();
        assert_eq!(sig(0), "P");
        assert_eq!(sig(1), "R(P,c1)");
        assert_eq!(sig(2), "R(P,D(c1,c2))");
        assert_eq!(sig(3), "R(P,D(c1,D(c2,c3)))");
        assert_eq!(sig(4), "R(P,R(D(c1,c2),D(c3,c4)))");
        assert_eq!(sig(6), "R(P,R(D(c1,D(c2,c3)),D(c4,D(c5,c6))))");
        assert_eq!(sig(8).matches('c').count(), 8, "8 tiled");
        // Three stacked children get equal heights; parent takes the left half.
        let g = Layout::grid(p("P"), kids(3));
        let r = g.rects();
        assert!((r[0].1.w - 0.5).abs() < 1e-9);
        for (_, rect) in &r[1..] {
            assert!((rect.h - 1.0 / 3.0).abs() < 1e-9, "{rect:?}");
            assert!((rect.x - 0.5).abs() < 1e-9);
        }
    }

    #[test]
    fn r15_2_tiles_siblings_evenly() {
        let ps = |n: usize| (1..=n).map(|i| p(&format!("a{i}"))).collect::<Vec<_>>();
        let sig = |n| Layout::tiles(ps(n)).signature();
        assert_eq!(sig(1), "a1");
        assert_eq!(sig(2), "R(a1,a2)");
        assert_eq!(sig(3), "R(a1,R(a2,a3))");
        assert_eq!(sig(4), "D(R(a1,a2),R(a3,a4))");
        assert_eq!(sig(5), "D(R(a1,R(a2,a3)),R(a4,a5))");
        let t = Layout::tiles(ps(3));
        let r = t.rects();
        for (_, rect) in &r {
            assert!(
                (rect.w - 1.0 / 3.0).abs() < 1e-9 && rect.h == 1.0,
                "{rect:?}"
            );
        }
    }

    #[test]
    fn serde_roundtrip() {
        let l = three();
        let json = serde_json::to_string(&l).unwrap();
        assert!(json.contains(r#""type":"split""#));
        let back: Layout = serde_json::from_str(&json).unwrap();
        assert_eq!(back, l);
    }
}
