//! The Diff Pane's and Quick Diff's pure parts (R122, R123): side-by-side
//! pairing, change moves, gutter marks, and the Pane's placement in the
//! model. GTK-free; the widgets live in `rade-app`.

use crate::diff::{Hunk, LineKind};

/// A side-by-side row: indices into `Hunk::lines`; `None` = filler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair {
    pub left: Option<usize>,
    pub right: Option<usize>,
}

/// Pair a hunk's lines (R122.2): context on both sides; a run of removed
/// lines followed by a run of added lines zipped in order, the longer
/// run's rest against filler; an added run with no removed run before it
/// against filler on the left.
pub fn side_by_side(h: &Hunk) -> Vec<Pair> {
    let mut out = Vec::new();
    let (mut removed, mut added): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
    let flush = |out: &mut Vec<Pair>, removed: &mut Vec<usize>, added: &mut Vec<usize>| {
        for i in 0..removed.len().max(added.len()) {
            out.push(Pair {
                left: removed.get(i).copied(),
                right: added.get(i).copied(),
            });
        }
        removed.clear();
        added.clear();
    };
    for (i, l) in h.lines.iter().enumerate() {
        match l.kind {
            LineKind::Context => {
                flush(&mut out, &mut removed, &mut added);
                out.push(Pair {
                    left: Some(i),
                    right: Some(i),
                });
            }
            // Removed lines after an added run start a new block.
            LineKind::Removed => {
                if !added.is_empty() {
                    flush(&mut out, &mut removed, &mut added);
                }
                removed.push(i);
            }
            LineKind::Added => added.push(i),
        }
    }
    flush(&mut out, &mut removed, &mut added);
    out
}

/// The first row of each run of added / removed rows; `None` = a row that
/// is not a diff line.
pub fn change_starts(kinds: &[Option<LineKind>]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut in_run = false;
    for (i, k) in kinds.iter().enumerate() {
        let changed = matches!(k, Some(LineKind::Added | LineKind::Removed));
        if changed && !in_run {
            out.push(i);
        }
        in_run = changed;
    }
    out
}

/// F7: the first start after `current`, wrapping to the first.
pub fn next_change(starts: &[usize], current: Option<usize>) -> Option<usize> {
    let first = *starts.first()?;
    Some(match current {
        Some(c) => starts.iter().copied().find(|s| *s > c).unwrap_or(first),
        None => first,
    })
}

/// Shift+F7: the last start before `current`, wrapping to the last.
pub fn prev_change(starts: &[usize], current: Option<usize>) -> Option<usize> {
    let last = *starts.last()?;
    Some(match current {
        Some(c) => starts
            .iter()
            .copied()
            .rev()
            .find(|s| *s < c)
            .unwrap_or(last),
        None => last,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    Added,
    Modified,
    Deleted,
}

/// A Quick Diff gutter mark on a 1-based line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    pub line: u32,
    pub kind: MarkKind,
}

/// Marks of a `git diff -U0` (R123.1): per `@@ -a,b +c,d @@` (a missing
/// count is 1) `b == 0` → Added `c..c+d-1`; `d == 0` → Deleted at
/// `max(c + 1, 1)` (git's `c` is the line before the deletion); else
/// Modified `c..c+d-1`. Sorted by line.
pub fn quick_marks(u0: &str) -> Vec<Mark> {
    let mut out = Vec::new();
    for l in u0.lines().filter(|l| l.starts_with("@@ ")) {
        let Some((_, b, c, d)) = crate::scm::hunk_range(l) else {
            continue;
        };
        if b == 0 {
            out.extend((c..c + d).map(|line| Mark {
                line,
                kind: MarkKind::Added,
            }));
        } else if d == 0 {
            out.push(Mark {
                line: (c + 1).max(1),
                kind: MarkKind::Deleted,
            });
        } else {
            out.extend((c..c + d).map(|line| Mark {
                line,
                kind: MarkKind::Modified,
            }));
        }
    }
    out.sort_by_key(|m| m.line);
    out
}

#[cfg(test)]
#[path = "diff_view_tests.rs"]
mod tests;
