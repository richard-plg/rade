//! Review Comments (PRD R13.2): anchored to new-file lines of a Worktree's
//! diff, kept per Worktree in `state.json` until sent or cleared, and
//! composed into one prompt for the Worktree's Agent. Also where the Review
//! Pane opens.

use crate::agents::AgentStatus;
use crate::diff::{DiffLine, LineKind};
use crate::layout::{Pane, PaneKind};
use crate::model::State;
use crate::rpc::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewComment {
    pub id: u64,
    /// Path relative to the Worktree root.
    pub path: String,
    /// New-file line (a removed line: the new line that follows it).
    pub line: u32,
    /// Last line of a multi-line selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    /// The commented code, one entry per line, with its diff sign
    /// (`+`, `-`, ` `).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excerpt: Vec<String>,
    pub text: String,
    /// Where it sits in the file, to find it again when the file changes
    /// (M11). Comments from before M11 have none and are never moved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<Context>,
    /// Its lines could not be found again: it keeps its last place and its
    /// excerpt, and is sent marked as outdated.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub outdated: bool,
    /// The Round it was sent in (R119.1); `None`: a draft.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<u32>,
    /// The owner resolved it (R119.3); only a sent comment.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub resolved: bool,
    /// The sent comment this draft follows up (R119.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<u64>,
}

/// A Review Comment's surroundings in the new file: the commented lines
/// themselves are its excerpt's new-side lines.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    /// Up to [`CONTEXT_LINES`] lines before the commented ones.
    #[serde(default)]
    pub before: Vec<String>,
    /// Up to [`CONTEXT_LINES`] lines after them.
    #[serde(default)]
    pub after: Vec<String>,
    /// The `@@ … @@` header of the hunk it was written in, if known.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hunk: String,
}

pub const CONTEXT_LINES: usize = 2;
/// A fuzzy match (context changed) is trusted only this many lines away.
pub const NEARBY_LINES: usize = 100;

impl ReviewComment {
    /// `path:line` or `path:line-end`.
    pub fn location(&self) -> String {
        match self.end_line {
            Some(e) => format!("{}:{}-{e}", self.path, self.line),
            None => format!("{}:{}", self.path, self.line),
        }
    }

    /// The commented new-file lines: the excerpt without removed lines and
    /// diff signs (none for a comment on removed lines only).
    fn lines(&self) -> Vec<&str> {
        self.excerpt
            .iter()
            .filter(|l| !l.starts_with('-'))
            .map(|l| l.get(1..).unwrap_or(""))
            .collect()
    }

    /// Record its anchor from the file's current text (when written):
    /// [`CONTEXT_LINES`] around its lines, and the hunk header. Only when
    /// its lines are really there, one after the other, from `line` (else
    /// it gets no anchor and stays put: a quote that isn't the file's
    /// lines could never be found again).
    pub fn capture(&mut self, file: &str, hunk: Option<&str>) {
        let lines: Vec<&str> = file.lines().collect();
        let s = self.line as usize - 1;
        let text = self.lines();
        if lines.get(s..s + text.len()) != Some(&text[..]) {
            self.context = None;
            return;
        }
        let mut ctx = context_at(&lines, s, text.len());
        ctx.hunk = hunk.unwrap_or_default().to_string();
        self.context = Some(ctx);
    }

    /// Find its lines again in the file's new text (`None`: the file is
    /// gone): the place whose lines and context match exactly, nearest its
    /// old line; else, within [`NEARBY_LINES`], its lines ignoring
    /// indentation with the most context lines matching (a line too short
    /// to mean anything, or no lines at all, needs some context). Found: it
    /// moves there and its context is refreshed; lost: `outdated` (until
    /// found again). Comments without an anchor stay put. Returns whether
    /// anything changed.
    pub fn relocate(&mut self, file: Option<&str>) -> bool {
        let Some(ctx) = self.context.clone() else {
            return false;
        };
        let was = self.clone();
        let text: Vec<String> = self.lines().into_iter().map(String::from).collect();
        if text.is_empty() && ctx.before.is_empty() && ctx.after.is_empty() {
            return false; // nothing to look for
        }
        let lines: Vec<&str> = file.map(|f| f.lines().collect()).unwrap_or_default();
        let old = self.line as usize - 1;
        match file.and_then(|_| find(&lines, &ctx, &text, old)) {
            Some(s) => {
                let n = text.len();
                self.line = s as u32 + 1;
                self.end_line = (n > 1).then_some((s + n) as u32);
                let mut fresh = context_at(&lines, s, n);
                fresh.hunk = ctx.hunk;
                self.context = Some(fresh);
                self.outdated = false;
            }
            None => self.outdated = true,
        }
        *self != was
    }
}

/// The context around `n` lines starting at 0-based `s`.
fn context_at(lines: &[&str], s: usize, n: usize) -> Context {
    let s = s.min(lines.len());
    let own = |a: usize, b: usize| -> Vec<String> {
        lines[a.min(lines.len())..b.min(lines.len())]
            .iter()
            .map(|l| l.to_string())
            .collect()
    };
    Context {
        before: own(s.saturating_sub(CONTEXT_LINES), s),
        after: own(s + n, s + n + CONTEXT_LINES),
        hunk: String::new(),
    }
}

/// Where `text` (with `ctx` around it) starts in `lines` (see
/// [`ReviewComment::relocate`]).
fn find(lines: &[&str], ctx: &Context, text: &[String], old: usize) -> Option<usize> {
    let n = text.len();
    let last = lines.len().checked_sub(n)?;
    let same = |s: usize, exact: bool| {
        text.iter().enumerate().all(|(i, t)| {
            let l = lines[s + i];
            if exact {
                l == t
            } else {
                l.trim() == t.trim()
            }
        })
    };
    // (context lines matching, all of them matching exactly)
    let around = |s: usize, exact: bool| -> (usize, bool) {
        let b = ctx.before.len();
        let before = ctx.before.iter().enumerate().filter(|(i, c)| {
            (s + i)
                .checked_sub(b)
                .and_then(|k| lines.get(k))
                .is_some_and(|l| if exact { l == c } else { l.trim() == c.trim() })
        });
        let after = ctx.after.iter().enumerate().filter(|(i, c)| {
            lines.get(s + n + i).is_some_and(|l| {
                if exact {
                    *l == c.as_str()
                } else {
                    l.trim() == c.trim()
                }
            })
        });
        let k = before.count() + after.count();
        (k, k == ctx.before.len() + ctx.after.len())
    };
    let dist = |s: usize| s.abs_diff(old);
    let exact = (0..=last)
        .filter(|&s| same(s, true) && around(s, true).1)
        .min_by_key(|&s| dist(s));
    if exact.is_some() {
        return exact;
    }
    // Text that says little (none, blank, `}`) needs context to be trusted.
    let weak = text.iter().all(|t| t.trim().len() < 3);
    let need = if weak {
        (ctx.before.len() + ctx.after.len()).div_ceil(2).max(1)
    } else {
        0
    };
    (old.saturating_sub(NEARBY_LINES)..=(old + NEARBY_LINES).min(last))
        .filter(|&s| same(s, false))
        .map(|s| (around(s, false).0, s))
        .filter(|(k, _)| *k >= need)
        .max_by_key(|&(k, s)| (k, std::cmp::Reverse(dist(s))))
        .map(|(_, s)| s)
}

/// Anchor of selected diff lines: (first line, last line if different,
/// excerpt). New-file numbers where the selection has new lines, else the
/// position of the removed lines in the new file.
pub fn anchor(lines: &[&DiffLine]) -> Option<(u32, Option<u32>, Vec<String>)> {
    let first = lines.first()?;
    let news: Vec<u32> = lines.iter().filter_map(|l| l.new).collect();
    let (a, b) = match (news.first(), news.last()) {
        (Some(a), Some(b)) => (*a, *b),
        _ => (first.at, first.at),
    };
    let excerpt = lines
        .iter()
        .map(|l| {
            let sign = match l.kind {
                LineKind::Added => '+',
                LineKind::Removed => '-',
                LineKind::Context => ' ',
            };
            format!("{sign}{}", l.text)
        })
        .collect();
    Some((a, (b != a).then_some(b), excerpt))
}

/// Excerpt lines shown per comment in the prompt; longer ones end in `…`.
pub const EXCERPT_LINES: usize = 12;

/// One prompt for all comments (R13.2): for each, `path:line`, the quoted
/// code, and the comment. Since M44 the prompt is headed with its Round
/// (R119.1) and a follow-up quotes the comment it follows (from `all`).
pub fn compose(worktree: &str, comments: &[ReviewComment]) -> String {
    compose_round(worktree, None, comments, &[])
}

/// [`compose`] for Round `round` (when known); `all` holds the sent
/// comments follow-ups refer to.
pub fn compose_round(
    worktree: &str,
    round: Option<u32>,
    comments: &[ReviewComment],
    all: &[ReviewComment],
) -> String {
    let head = match round {
        Some(n) => format!("Review round {n}: comments"),
        None => "Review comments".to_string(),
    };
    let s = format!(
        "{head} on the changes in this worktree ({worktree}). \
         Please address each one:\n"
    );
    let replies: Vec<ReviewComment> = comments
        .iter()
        .map(|c| {
            let earlier = c.reply_to.and_then(|id| all.iter().find(|x| x.id == id));
            let mut c = c.clone();
            if let Some(e) = earlier {
                let first = e.text.trim().lines().next().unwrap_or("");
                c.text = format!(
                    "Re: round {}, {}: \"{first}\"\n{}",
                    e.round.unwrap_or(0),
                    e.location(),
                    c.text.trim()
                );
            }
            c
        })
        .collect();
    each(s, &replies)
}

fn each(mut s: String, comments: &[ReviewComment]) -> String {
    for (i, c) in comments.iter().enumerate() {
        let outdated = match c.context.as_ref().map(|x| hunk_scope(&x.hunk)) {
            _ if !c.outdated => String::new(),
            Some(Some(scope)) => {
                format!(" (outdated: the code there changed since; it was in `{scope}`)")
            }
            _ => " (outdated: the code there changed since)".to_string(),
        };
        s += &format!("\n{}. {}{outdated}\n", i + 1, c.location());
        for l in c.excerpt.iter().take(EXCERPT_LINES) {
            s += &format!("   > {l}\n");
        }
        if c.excerpt.len() > EXCERPT_LINES {
            s += "   > …\n";
        }
        for l in c.text.trim().lines() {
            s += &format!("   {l}\n");
        }
    }
    s.trim_end().to_string()
}

/// The function/scope git names after a hunk header's second `@@`.
fn hunk_scope(header: &str) -> Option<&str> {
    let rest = header.strip_prefix("@@")?;
    let scope = rest[rest.find("@@")? + 2..].trim();
    (!scope.is_empty()).then_some(scope)
}

impl Pane {
    pub fn review(id: String, root: PathBuf) -> Self {
        let mut p = Pane::new(id, PathBuf::new(), None, None);
        p.kind = PaneKind::Review { root };
        p
    }
}

impl State {
    pub fn comments(&self, source: &str, worktree: &str) -> Result<&[ReviewComment]> {
        let (p, w) = self.find_worktree(source, Some(worktree))?;
        Ok(&self.worktree(&p, &w).unwrap().comments)
    }

    /// Add a Review Comment; returns its id (unique in the Worktree).
    pub fn add_comment(
        &mut self,
        source: &str,
        worktree: &str,
        mut c: ReviewComment,
    ) -> Result<u64> {
        if c.text.trim().is_empty() {
            return Err(Error::usage("empty comment"));
        }
        let (p, w) = self.find_worktree(source, Some(worktree))?;
        let wt = self.worktree_mut(&p, &w)?;
        c.id = wt.comments.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        let id = c.id;
        wt.comments.push(c);
        Ok(id)
    }

    /// Edit a comment's text; empty text deletes it.
    pub fn edit_comment(
        &mut self,
        source: &str,
        worktree: &str,
        id: u64,
        text: &str,
    ) -> Result<()> {
        let (p, w) = self.find_worktree(source, Some(worktree))?;
        let wt = self.worktree_mut(&p, &w)?;
        let i = wt
            .comments
            .iter()
            .position(|c| c.id == id)
            .ok_or_else(|| Error::not_found(format!("no comment {id}")))?;
        if text.trim().is_empty() {
            wt.comments.remove(i);
        } else {
            wt.comments[i].text = text.into();
        }
        Ok(())
    }

    /// Re-anchor a Worktree's comments to its files as they are now
    /// ([`ReviewComment::relocate`]); `read` gives a file's text by its
    /// Worktree-relative path (each read once, only for anchored comments).
    /// Returns whether any comment changed.
    pub fn reanchor_comments(
        &mut self,
        source: &str,
        worktree: &str,
        read: &dyn Fn(&str) -> Option<String>,
    ) -> Result<bool> {
        let (p, w) = self.find_worktree(source, Some(worktree))?;
        let wt = self.worktree_mut(&p, &w)?;
        let mut files: std::collections::HashMap<String, Option<String>> = Default::default();
        let mut changed = false;
        for c in wt.comments.iter_mut().filter(|c| c.context.is_some()) {
            let text = files.entry(c.path.clone()).or_insert_with(|| read(&c.path));
            changed |= c.relocate(text.as_deref());
        }
        Ok(changed)
    }

    /// Remove and return the draft comments (Discard…); sent ones stay.
    pub fn take_comments(&mut self, source: &str, worktree: &str) -> Result<Vec<ReviewComment>> {
        let (p, w) = self.find_worktree(source, Some(worktree))?;
        let wt = self.worktree_mut(&p, &w)?;
        let (drafts, sent) = std::mem::take(&mut wt.comments)
            .into_iter()
            .partition(|c| c.round.is_none());
        wt.comments = sent;
        Ok(drafts)
    }

    /// Agent Panes of a Worktree that can take a prompt (not `exited`).
    pub fn agent_panes(&self, source: &str, worktree: &str) -> Vec<&Pane> {
        self.panes_in(source, worktree)
            .into_iter()
            .filter(|p| p.agent.is_some() && p.status != Some(AgentStatus::Exited))
            .collect()
    }

    /// Show a Worktree's Review Pane (R13.1): the one in its active Tab, else
    /// a new one split right of that Tab's focused Pane (so the Agent stays
    /// visible), else in a new Tab. Shows and focuses it.
    pub fn open_review(&mut self, source: &str, worktree: &str) -> Result<String> {
        let (p, w) = self.find_worktree(source, Some(worktree))?;
        let wt = self.worktree(&p, &w).unwrap();
        let root = wt.path.clone();
        // M14: full width, in its own Tab; one per Worktree.
        let existing = wt
            .tabs
            .iter()
            .flat_map(|t| t.panes())
            .find(|x| x.is_review())
            .map(|x| x.id.clone());
        let id = match existing {
            Some(id) => id,
            None => {
                let pane = Pane::review(self.next("p"), root);
                self.new_tab_with(&p, &w, pane)?.1
            }
        };
        self.focus_pane(&id)?;
        Ok(id)
    }
}

#[cfg(test)]
#[path = "review_tests.rs"]
mod tests;
