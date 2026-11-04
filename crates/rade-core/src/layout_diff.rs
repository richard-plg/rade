//! Which of a Tab's widgets the UI keeps when its Layout changes (R142,
//! R142-h). Taking a Terminal out of its container unrealizes its GL area
//! (Ghostty drops and rebuilds its renderer), so a Tab of 40 Panes rebuilt on
//! every spawn costs 40 re-realizes per spawn. The UI instead keeps every
//! split widget whose subtree did not change and moves only the Panes whose
//! place in the tree changed:
//!
//! - [`Shape`]: a Layout's tree without ratios (what the split widgets mirror).
//! - [`reconcile`]: old shape → new shape as [`Step`]s, top down.
//! - [`Content`]: what a Tab shows (the tiled Layout, an Agent Grid with Title
//!   Bars, a Zoomed Pane, one main Pane), and [`Content::same_frame`] = the
//!   outer widgets can stay.
//! - [`FrameKey`]: the parts of a Pane's frame fixed when it is built (header,
//!   Close button); a changed key rebuilds that one frame.

use crate::layout::{Layout, Pane, SplitDir};
use crate::model::Tab;

/// A Layout's tree: Pane ids and split directions, not ratios.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    Leaf(String),
    Split(SplitDir, Box<Shape>, Box<Shape>),
}

impl Shape {
    pub fn of(layout: &Layout) -> Self {
        match layout {
            Layout::Leaf { pane } => Shape::Leaf(pane.id.clone()),
            Layout::Split { dir, a, b, .. } => {
                Shape::Split(*dir, Box::new(Shape::of(a)), Box::new(Shape::of(b)))
            }
        }
    }
}

/// What the UI does with one node of the new Layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// The same subtree as before: its widgets stay as they are (only the
    /// ratios follow the model).
    Keep,
    /// The old split widget stays (same direction); its two children follow
    /// these steps.
    Reuse(Box<Step>, Box<Step>),
    /// A new split widget over two children built by these steps.
    Split(SplitDir, Box<Step>, Box<Step>),
    /// The Pane's frame goes here: moved from elsewhere, or new.
    Place(String),
}

/// The steps from `old` (what the widgets show; `None` = nothing built) to
/// `new`. A subtree equal to the old one at the same place is kept whole; a
/// split in the same direction keeps its widget and recurses; anything else
/// is built, its Panes placed.
pub fn reconcile(old: Option<&Shape>, new: &Shape) -> Step {
    match (old, new) {
        (Some(o), n) if o == n => Step::Keep,
        (Some(Shape::Split(od, oa, ob)), Shape::Split(nd, na, nb)) if od == nd => Step::Reuse(
            Box::new(reconcile(Some(oa), na)),
            Box::new(reconcile(Some(ob), nb)),
        ),
        (_, Shape::Leaf(p)) => Step::Place(p.clone()),
        (_, Shape::Split(d, a, b)) => Step::Split(
            *d,
            Box::new(reconcile(None, a)),
            Box::new(reconcile(None, b)),
        ),
    }
}

impl Step {
    /// The Panes whose frame is (re)parented by these steps.
    pub fn placed(&self) -> Vec<&str> {
        match self {
            Step::Keep => Vec::new(),
            Step::Place(p) => vec![p.as_str()],
            Step::Reuse(a, b) | Step::Split(_, a, b) => {
                let mut v = a.placed();
                v.extend(b.placed());
                v
            }
        }
    }

    /// New split widgets these steps create.
    pub fn new_splits(&self) -> usize {
        match self {
            Step::Keep | Step::Place(_) => 0,
            Step::Reuse(a, b) => a.new_splits() + b.new_splits(),
            Step::Split(_, a, b) => 1 + a.new_splits() + b.new_splits(),
        }
    }
}

/// What a Tab shows, by the rules of `layout_view::rebuild` (R1.7, R2.4,
/// R111.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    /// Zoomed: this Pane alone.
    Zoomed(String),
    /// Large text: the focused Pane, the others as Title Bars under it.
    One { main: String, others: Vec<String> },
    /// The Layout as nested splits.
    Tiled,
    /// An Agent Grid with Title Bars: the Parent left, the tiles and the
    /// Title Bars right (a right split at the top).
    Grid { bars: Vec<String> },
    /// Title Bars under any other Layout.
    Stacked { bars: Vec<String> },
}

impl Content {
    /// `one_pane` = the window shows one main Pane (R111.2).
    pub fn of(tab: &Tab, one_pane: bool) -> Self {
        if let Some(z) = tab.zoomed.as_deref().filter(|z| tab.pane(z).is_some()) {
            return Content::Zoomed(z.to_string());
        }
        let panes = tab.panes();
        let ids: Vec<&str> = panes.iter().map(|p| p.id.as_str()).collect();
        if one_pane && ids.len() > 1 {
            if let Some((main, others)) = crate::chrome::one_main_pane(&ids, Some(&tab.focused)) {
                return Content::One {
                    main: main.to_string(),
                    others: others.into_iter().map(String::from).collect(),
                };
            }
        }
        if tab.title_bars.is_empty() {
            return Content::Tiled;
        }
        let bars = tab.title_bars.iter().map(|p| p.id.clone()).collect();
        match &tab.layout {
            Layout::Split {
                dir: SplitDir::Right,
                ..
            } => Content::Grid { bars },
            _ => Content::Stacked { bars },
        }
    }

    /// The outer widgets built for `self` can show `new` too (only the
    /// tiled subtrees and the Title Bar list differ): a Tiled Tab stays
    /// Tiled, a Grid stays a Grid, Stacked stays Stacked.
    pub fn same_frame(&self, new: &Content) -> bool {
        matches!(
            (self, new),
            (Content::Tiled, Content::Tiled)
                | (Content::Grid { .. }, Content::Grid { .. })
                | (Content::Stacked { .. }, Content::Stacked { .. })
        )
    }
}

/// The parts of a Pane's frame fixed when it is built: a header (R103.3),
/// and in it a Close button when the Tab has 2 or more Panes (R19.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameKey {
    pub header: bool,
    pub close: bool,
}

impl FrameKey {
    /// `panes` = the Tab's Pane count.
    pub fn of(p: &Pane, panes: usize) -> Self {
        let header = crate::chrome::has_header(p, panes);
        FrameKey {
            header,
            close: header && panes >= 2,
        }
    }
}

#[cfg(test)]
#[path = "layout_diff_tests.rs"]
mod tests;
