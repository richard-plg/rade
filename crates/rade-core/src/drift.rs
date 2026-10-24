//! Base drift (PRD R100, ADR-0034 decision 6): a link's recorded checkout
//! whose HEAD is not the link's base, and the safe actions on it. Pure;
//! the app reads HEAD and the dirty count off the UI thread.

use crate::rpc::{Error, Result};
use serde::Serialize;
use std::path::Path;

/// What a checkout's HEAD is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    Branch(String),
    /// A detached HEAD, its short commit.
    Detached(String),
}

impl Head {
    /// From `git worktree list`: the branch, else the short commit.
    pub fn of(branch: Option<&str>, commit: &str) -> Head {
        match branch {
            Some(b) => Head::Branch(b.into()),
            None => Head::Detached(commit.into()),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Head::Branch(b) | Head::Detached(b) => b,
        }
    }
}

/// An action offered on a drifted checkout (R100.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// `git switch <base>`; only when clean.
    BackToBase,
    /// Hide the warning until HEAD changes again.
    Keep,
    /// Switch back to base, then `git worktree add` the drifted branch on
    /// the link (when clean); when dirty it says to commit first.
    MoveToWorktree,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Drift {
    pub head: String,
    pub base: String,
    pub dirty: usize,
    #[serde(skip)]
    pub actions: Vec<Action>,
}

/// `None` when the checkout is on `base`; a detached HEAD is always drift,
/// even at the base's commit (R100-c), and offers no Move to a Worktree.
pub fn drift(base: &str, head: &Head, dirty: usize) -> Option<Drift> {
    if *head == Head::Branch(base.to_string()) {
        return None;
    }
    let mut actions = Vec::new();
    if dirty == 0 {
        actions.push(Action::BackToBase);
    }
    actions.push(Action::Keep);
    // Only a branch can move: on a detached HEAD a Worktree "of" the short
    // commit would be a new branch cut from the base, without its commits.
    if matches!(head, Head::Branch(_)) {
        actions.push(Action::MoveToWorktree);
    }
    Some(Drift {
        head: head.name().to_string(),
        base: base.to_string(),
        dirty,
        actions,
    })
}

/// `n uncommitted change(s)`.
pub fn changes(n: usize) -> String {
    if n == 1 {
        "1 uncommitted change".into()
    } else {
        format!("{n} uncommitted changes")
    }
}

/// Whether Back to base may run (`rade project link to-base`, R100.2):
/// `Ok(false)` = not drifted, nothing to do; `Ok(true)` = run
/// `git switch <base>`; refused when dirty or when the base is checked out
/// in another checkout (`elsewhere`), naming it.
pub fn to_base(source: &str, d: Option<&Drift>, elsewhere: Option<&Path>) -> Result<bool> {
    let Some(d) = d else { return Ok(false) };
    if d.dirty > 0 {
        return Err(Error::usage(format!(
            "'{source}' has {}: commit them first (RADE never switches a dirty checkout)",
            changes(d.dirty)
        )));
    }
    if let Some(p) = elsewhere {
        return Err(Error::usage(format!(
            "{} is checked out in {}: switch that checkout first",
            d.base,
            p.display()
        )));
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r100_a_clean_drift_offers_three_actions() {
        let d = drift("develop", &Head::Branch("feat/checkout-v2".into()), 0).unwrap();
        assert_eq!(d.head, "feat/checkout-v2");
        assert_eq!(d.base, "develop");
        assert_eq!(d.dirty, 0);
        assert_eq!(
            d.actions,
            [Action::BackToBase, Action::Keep, Action::MoveToWorktree]
        );
        assert_eq!(to_base("shop-api", Some(&d), None), Ok(true));
        let e = to_base("shop-api", Some(&d), Some(Path::new("/w/.worktrees/x"))).unwrap_err();
        assert!(e.message.contains("/w/.worktrees/x"), "{e}");
    }

    #[test]
    fn r100_b_dirty_drift_offers_keep_and_move_only() {
        let d = drift("develop", &Head::Branch("feat/checkout-v2".into()), 3).unwrap();
        assert_eq!(d.dirty, 3);
        assert_eq!(d.actions, [Action::Keep, Action::MoveToWorktree]);
        let e = to_base("shop-api", Some(&d), None).unwrap_err();
        assert_eq!(e.kind, crate::rpc::ErrorKind::Usage);
        assert!(e.message.contains("3 uncommitted changes"), "{e}");
    }

    #[test]
    fn r100_c_detached_at_base_commit_is_drift() {
        assert_eq!(drift("develop", &Head::Branch("develop".into()), 0), None);
        assert_eq!(drift("develop", &Head::Branch("develop".into()), 2), None);
        assert_eq!(to_base("x", None, None), Ok(false), "not drifted: no-op");
        let d = drift("develop", &Head::Detached("1a2b3c4".into()), 0).unwrap();
        assert_eq!(d.head, "1a2b3c4");
        // No branch to move: a Worktree named after the commit would be a
        // new branch cut from the base, leaving the detached commits out.
        assert_eq!(d.actions, [Action::BackToBase, Action::Keep]);
        let d = drift("develop", &Head::Detached("1a2b3c4".into()), 2).unwrap();
        assert_eq!(d.actions, [Action::Keep]);
    }
}
