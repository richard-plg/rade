//! Applying a Fan-out Winner (PRD R15.4) with the `git` CLI. RADE never
//! makes a commit on a branch (ADR-0008): the Winner's result is staged on
//! the Source's current branch (`git merge --squash`) for the user or an
//! Agent to commit, or its Worktree is kept as it is.

use crate::git::{self, command, run};
use crate::rpc::{Error, Result};
use std::path::Path;

/// The Winner's whole result as one commit object that no branch points
/// at: its working tree — commits on its branch, staged and unstaged
/// edits, untracked non-ignored files — on top of its HEAD. Built in a
/// throwaway copy of its index; its own index and files are untouched.
pub fn snapshot(dir: &Path) -> Result<String> {
    let index = dir.join(run(dir, &["rev-parse", "--git-path", "index"])?.trim());
    let tmp = crate::diff::temp_index();
    let _cleanup = crate::diff::Cleanup(&tmp);
    if index.exists() {
        crate::diff::copy_index(&index, &tmp)
            .map_err(|e| Error::internal(format!("index copy: {e}")))?;
    }
    let with_tmp = |args: &[&str]| -> Result<String> {
        let out = command(dir)
            .args(args)
            .env("GIT_INDEX_FILE", &tmp)
            .output()
            .map_err(|e| Error::internal(format!("cannot run git: {e}")))?;
        if !out.status.success() {
            return Err(Error::internal(format!(
                "git {}: {}",
                args[0],
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    with_tmp(&["add", "-A"])?;
    let tree = with_tmp(&["write-tree"])?;
    let head = run(dir, &["rev-parse", "--verify", "HEAD"])?;
    // An object only (no ref, never signed or hooked): a fixed identity
    // works in repositories without user.name.
    let out = command(dir)
        .args(["commit-tree", "--no-gpg-sign", &tree, "-p", head.trim()])
        .args(["-m", "RADE fan-out snapshot"])
        .env("GIT_AUTHOR_NAME", "RADE")
        .env("GIT_AUTHOR_EMAIL", "rade@localhost")
        .env("GIT_COMMITTER_NAME", "RADE")
        .env("GIT_COMMITTER_EMAIL", "rade@localhost")
        .output()
        .map_err(|e| Error::internal(format!("cannot run git: {e}")))?;
    if !out.status.success() {
        return Err(Error::internal(format!(
            "git commit-tree: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The Source's checkout the Winner is staged into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// Checked-out branch; `None` when HEAD is detached.
    pub branch: Option<String>,
    /// Tracked files with staged or unstaged changes (untracked files are
    /// git's to protect: it refuses to overwrite them).
    pub dirty: Vec<String>,
    /// A merge, rebase, cherry-pick or revert is in progress.
    pub busy: bool,
}

pub fn target(repo: &Path) -> Result<Target> {
    git::require_toplevel(repo)?;
    let branch = run(repo, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .ok()
        .map(|b| b.trim().to_string());
    let dirty = git::parse_status_z(&run(
        repo,
        &["status", "--porcelain", "-z", "--untracked-files=no"],
    )?);
    let busy = [
        "MERGE_HEAD",
        "rebase-merge",
        "rebase-apply",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
    ]
    .iter()
    .any(|f| {
        run(repo, &["rev-parse", "--git-path", f]).is_ok_and(|p| repo.join(p.trim()).exists())
    });
    Ok(Target {
        branch,
        dirty,
        busy,
    })
}

/// Refusals before the target is touched: a detached HEAD (no "current
/// branch"), an operation in progress, uncommitted changes (they would mix
/// with the Winner's), or a Winner that changed nothing. Returns the branch.
pub fn check_squash(t: &Target, changed: usize, winner: &str) -> Result<String> {
    let Some(branch) = &t.branch else {
        return Err(Error::usage(
            "the Source's checkout has a detached HEAD; check out a branch first",
        ));
    };
    if t.busy {
        return Err(Error::usage(format!(
            "a merge or rebase is in progress on {branch}; finish or abort it first"
        )));
    }
    if !t.dirty.is_empty() {
        let shown: Vec<&str> = t.dirty.iter().take(5).map(String::as_str).collect();
        return Err(Error::usage(format!(
            "{branch} has uncommitted changes ({}{}); commit or stash them first",
            shown.join(", "),
            if t.dirty.len() > 5 { ", …" } else { "" }
        )));
    }
    if changed == 0 {
        return Err(Error::usage(format!("{winner} changed nothing")));
    }
    Ok(branch.clone())
}

/// `git merge --squash <commit>` in `repo`: the result is staged, not
/// committed. Conflicts are left for the user (returned); any other
/// failure is an error and leaves the checkout as git left it.
pub fn squash(repo: &Path, commit: &str) -> Result<Vec<String>> {
    let out = command(repo)
        // A user's `merge.ff = only` would refuse a diverged squash.
        .args(["-c", "merge.ff=true", "merge", "--squash", commit])
        // A three-way merge wants a committer even though a squash makes no
        // commit; a repository without user.name must still work.
        .env("GIT_COMMITTER_NAME", "RADE")
        .env("GIT_COMMITTER_EMAIL", "rade@localhost")
        .output()
        .map_err(|e| Error::internal(format!("cannot run git: {e}")))?;
    if out.status.success() {
        return Ok(Vec::new());
    }
    let conflicts: Vec<String> = run(repo, &["diff", "--name-only", "-z", "--diff-filter=U"])?
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    if conflicts.is_empty() {
        let msg = String::from_utf8_lossy(&out.stderr);
        let msg = if msg.trim().is_empty() {
            String::from_utf8_lossy(&out.stdout)
        } else {
            msg
        };
        return Err(Error::usage(format!("git merge --squash: {}", msg.trim())));
    }
    Ok(conflicts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::{repo, sh};

    fn t(branch: Option<&str>, dirty: &[&str], busy: bool) -> Target {
        Target {
            branch: branch.map(Into::into),
            dirty: dirty.iter().map(|s| s.to_string()).collect(),
            busy,
        }
    }

    #[test]
    fn r15_4_squash_refuses_detached_busy_dirty_and_empty() {
        assert_eq!(
            check_squash(&t(Some("main"), &[], false), 2, "w").unwrap(),
            "main"
        );
        let e = check_squash(&t(None, &[], false), 2, "w").unwrap_err();
        assert!(e.message.contains("detached"), "{e}");
        let e = check_squash(&t(Some("main"), &[], true), 2, "w").unwrap_err();
        assert!(e.message.contains("in progress"), "{e}");
        let e = check_squash(&t(Some("main"), &["a", "b"], false), 2, "w").unwrap_err();
        assert!(
            e.message.contains("uncommitted") && e.message.contains("a, b"),
            "{e}"
        );
        let e = check_squash(&t(Some("main"), &[], false), 0, "codex-2").unwrap_err();
        assert!(e.message.contains("codex-2 changed nothing"), "{e}");
    }

    /// A Worktree `w` of `r` on branch `fo-1` with a commit, an unstaged
    /// edit and an untracked file.
    fn winner(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let r = repo(name);
        let w = r.with_extension("wt");
        let _ = std::fs::remove_dir_all(&w);
        sh(
            &r,
            &["worktree", "add", "-q", "-b", "fo-1", w.to_str().unwrap()],
        );
        std::fs::write(w.join("c.txt"), "committed\n").unwrap();
        sh(&w, &["add", "c.txt"]);
        sh(&w, &["commit", "-qm", "c"]);
        std::fs::write(w.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(w.join("new.txt"), "untracked\n").unwrap();
        (r, w)
    }

    #[test]
    fn r15_4_snapshot_holds_everything_and_leaves_the_worktree_alone() {
        let (r, w) = winner("snap");
        let before = run(&w, &["status", "--porcelain"]).unwrap();
        let c = snapshot(&w).unwrap();
        assert_eq!(
            run(&w, &["status", "--porcelain"]).unwrap(),
            before,
            "index untouched"
        );
        assert!(
            run(&r, &["branch", "--contains", &c])
                .unwrap()
                .trim()
                .is_empty(),
            "no branch"
        );
        let files = run(&r, &["show", "--name-only", "--format=", &c]).unwrap();
        assert!(
            files.contains("a.txt") && files.contains("new.txt"),
            "{files}"
        );
        assert_eq!(
            run(&r, &["show", &format!("{c}:c.txt")]).unwrap(),
            "committed\n"
        );

        // Squash into main: staged, not committed; HEAD unchanged.
        let head = run(&r, &["rev-parse", "HEAD"]).unwrap();
        assert!(squash(&r, &c).unwrap().is_empty());
        assert_eq!(run(&r, &["rev-parse", "HEAD"]).unwrap(), head, "no commit");
        let staged = run(&r, &["diff", "--cached", "--name-only"]).unwrap();
        assert_eq!(
            staged.lines().collect::<Vec<_>>(),
            ["a.txt", "c.txt", "new.txt"]
        );
        let tg = target(&r).unwrap();
        assert_eq!(tg.branch.as_deref(), Some("main"));
        assert_eq!(tg.dirty.len(), 3, "now dirty: a second squash is refused");
        let _ = std::fs::remove_dir_all(&w);
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn r15_4_squash_conflicts_are_listed_and_detached_is_seen() {
        let (r, w) = winner("conf");
        std::fs::write(r.join("a.txt"), "one\nzwei\nthree\n").unwrap();
        sh(&r, &["commit", "-qam", "main moved"]);
        let c = snapshot(&w).unwrap();
        assert_eq!(squash(&r, &c).unwrap(), ["a.txt"]);
        sh(&r, &["reset", "-q", "--hard"]);
        sh(&r, &["checkout", "-q", "--detach"]);
        assert_eq!(target(&r).unwrap().branch, None);
        let _ = std::fs::remove_dir_all(&w);
        let _ = std::fs::remove_dir_all(&r);
    }
}
