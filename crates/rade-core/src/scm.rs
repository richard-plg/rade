//! Source Control for the Right Panel's Changes page (PRD R21, ADR-0013):
//! `git status --porcelain=v2` parsing, stage / unstage / discard per file
//! and per hunk (one hunk cut from `git diff` and fed to `git apply`),
//! commit, pull, push, `gh pr create`, and the Draft (Generate) prompt and
//! runner. Every operation is a plain `git` / `gh` call the app makes on
//! the owner's click; nothing here runs on its own.

use crate::git;
use crate::rpc::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A Worktree's git state (`git status --porcelain=v2 --branch`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Status {
    /// `None` when detached.
    pub branch: Option<String>,
    /// e.g. `origin/feat-x`.
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// No commit yet.
    pub initial: bool,
    pub files: Vec<Change>,
}

/// The Changes page's lists (R21.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Section {
    Staged,
    Unstaged,
    Untracked,
}

pub const SECTIONS: [Section; 3] = [Section::Staged, Section::Unstaged, Section::Untracked];

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Section::Staged => "Staged Changes",
            Section::Unstaged => "Changes",
            Section::Untracked => "Untracked",
        }
    }
}

/// One changed path: `x` = index status, `y` = working tree status (`.`
/// unchanged, `?` untracked, git's letters otherwise).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Change {
    pub path: String,
    /// A rename's or copy's source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orig: Option<String>,
    pub x: char,
    pub y: char,
}

impl Change {
    pub fn untracked(&self) -> bool {
        self.x == '?'
    }

    pub fn conflicted(&self) -> bool {
        matches!(
            (self.x, self.y),
            ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D')
        )
    }

    pub fn in_section(&self, s: Section) -> bool {
        match s {
            Section::Untracked => self.untracked(),
            Section::Staged => !self.untracked() && !self.conflicted() && self.x != '.',
            Section::Unstaged => !self.untracked() && (self.conflicted() || self.y != '.'),
        }
    }

    /// The letter shown in section `s` (R21.1).
    pub fn letter(&self, s: Section) -> char {
        match s {
            Section::Untracked => 'U',
            Section::Unstaged if self.conflicted() => '!',
            Section::Unstaged => self.y,
            Section::Staged => self.x,
        }
    }
}

impl Status {
    pub fn section(&self, s: Section) -> Vec<&Change> {
        self.files.iter().filter(|c| c.in_section(s)).collect()
    }

    /// The upstream is this branch on a remote (a Worktree branched from
    /// `origin/main` tracks `origin/main`, which is not where it pushes).
    pub fn own_upstream(&self) -> Option<(&str, &str)> {
        let (remote, b) = self.upstream.as_deref()?.split_once('/')?;
        (Some(b) == self.branch.as_deref()).then_some((remote, b))
    }
}

/// Parse `git status --porcelain=v2 -z --branch`.
pub fn parse_status(out: &[u8]) -> Status {
    let mut st = Status::default();
    let mut recs = out
        .split(|b| *b == 0)
        .filter(|r| !r.is_empty())
        .map(|r| String::from_utf8_lossy(r).into_owned());
    let xy = |s: &str| {
        let mut c = s.chars();
        (c.next().unwrap_or('.'), c.next().unwrap_or('.'))
    };
    while let Some(rec) = recs.next() {
        if let Some(h) = rec.strip_prefix("# ") {
            let (k, v) = h.split_once(' ').unwrap_or((h, ""));
            match k {
                "branch.oid" => st.initial = v == "(initial)",
                "branch.head" => st.branch = (v != "(detached)").then(|| v.to_string()),
                "branch.upstream" => st.upstream = Some(v.to_string()),
                "branch.ab" => {
                    for n in v.split_whitespace() {
                        let val = n[1..].parse().unwrap_or(0);
                        match n.as_bytes()[0] {
                            b'+' => st.ahead = val,
                            _ => st.behind = val,
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        // Fields before the path: 1 → 8, 2 → 9 (+ orig record), u → 10.
        let (n, kind) = match rec.as_bytes()[0] {
            b'1' => (8, 1),
            b'2' => (9, 2),
            b'u' => (10, 3),
            b'?' => {
                st.files.push(Change {
                    path: rec[2..].to_string(),
                    orig: None,
                    x: '?',
                    y: '?',
                });
                continue;
            }
            _ => continue,
        };
        let f: Vec<&str> = rec.splitn(n + 1, ' ').collect();
        let (Some(code), Some(path)) = (f.get(1), f.get(n)) else {
            continue;
        };
        let (x, y) = xy(code);
        st.files.push(Change {
            path: path.to_string(),
            orig: if kind == 2 { recs.next() } else { None },
            x,
            y,
        });
    }
    st
}

/// A git command for pathspec arguments: names like `:x` or `a*` are paths.
fn git_cmd(dir: &Path) -> Command {
    let mut c = git::command(dir);
    c.env("GIT_LITERAL_PATHSPECS", "1");
    c
}

/// Run git with `input` on stdin; stdout, else git's message as the error.
/// `ok_codes`: exit codes that count as success (`diff --no-index` → 1).
fn run(dir: &Path, args: &[&str], input: Option<&[u8]>, ok_codes: &[i32]) -> Result<String> {
    run_env(dir, &[], args, input, ok_codes)
}

/// The Workspace env (R23.3) the network commands run with: push, pull,
/// `gh pr create`, the Draft command.
pub type Env = [(String, String)];

fn run_env(
    dir: &Path,
    env: &Env,
    args: &[&str],
    input: Option<&[u8]>,
    ok_codes: &[i32],
) -> Result<String> {
    let mut c = git_cmd(dir);
    c.envs(env.iter().map(|(k, v)| (k, v)));
    c.args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c
        .spawn()
        .map_err(|e| Error::internal(format!("cannot run git: {e}")))?;
    if let (Some(i), Some(mut stdin)) = (input, child.stdin.take()) {
        let _ = stdin.write_all(i);
    }
    let out = child
        .wait_with_output()
        .map_err(|e| Error::internal(format!("git: {e}")))?;
    if out.status.success() || out.status.code().is_some_and(|c| ok_codes.contains(&c)) {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let msg = if msg.is_empty() {
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    } else {
        msg
    };
    Err(Error::usage(format!(
        "git {}: {}",
        args.first().unwrap_or(&""),
        if msg.is_empty() { "failed" } else { &msg }
    )))
}

/// The Worktree's status (R21.1); an error outside a repository.
pub fn status(dir: &Path) -> Result<Status> {
    let out = git::command(dir)
        .args([
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
        ])
        .output()
        .map_err(|e| Error::internal(format!("cannot run git: {e}")))?;
    if !out.status.success() {
        return Err(Error::usage(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok(parse_status(&out.stdout))
}

fn with_paths<'a>(args: &[&'a str], paths: &'a [String]) -> Vec<&'a str> {
    let mut v = args.to_vec();
    v.push("--");
    v.extend(paths.iter().map(String::as_str));
    v
}

/// The paths a change touches (a rename's source too).
pub fn paths_of(c: &Change) -> Vec<String> {
    let mut v = vec![c.path.clone()];
    v.extend(c.orig.clone());
    v
}

/// Run `args` on `paths`, given on stdin (Stage All of 50k files would
/// overflow the command line).
fn on_paths(dir: &Path, args: &[&str], paths: &[String]) -> Result<()> {
    let mut a = args.to_vec();
    a.extend(["--pathspec-from-file=-", "--pathspec-file-nul"]);
    run(dir, &a, Some(paths.join("\0").as_bytes()), &[]).map(drop)
}

pub fn stage(dir: &Path, paths: &[String]) -> Result<()> {
    on_paths(dir, &["add", "-A"], paths)
}

pub fn unstage(dir: &Path, paths: &[String], initial: bool) -> Result<()> {
    let args: &[&str] = if initial {
        &["rm", "--cached", "-r", "-q"]
    } else {
        &["restore", "--staged"]
    };
    on_paths(dir, args, paths)
}

/// Discard a change in the working tree (R21.2): an untracked file is
/// deleted, a tracked one restored from the index. The caller asked first.
pub fn discard(dir: &Path, c: &Change) -> Result<()> {
    if c.untracked() {
        let p = dir.join(&c.path);
        // Never outside the Worktree (git paths are relative and clean).
        if c.path.split('/').any(|s| s == "..") {
            return Err(Error::usage(format!("refusing to delete {}", c.path)));
        }
        return std::fs::remove_file(&p)
            .map_err(|e| Error::usage(format!("delete {}: {e}", c.path)));
    }
    on_paths(dir, &["restore", "--worktree"], &paths_of(c))
}

/// The diff a row shows (R21.2): staged = index vs `HEAD`, otherwise
/// working tree vs index; an untracked file against nothing.
pub fn file_diff(dir: &Path, c: &Change, s: Section) -> Result<String> {
    file_diff_ctx(dir, c, s, None)
}

/// [`file_diff`] with `context` lines around each hunk (None = git's 3,
/// Some(n) = `-U<n>`).
pub fn file_diff_ctx(dir: &Path, c: &Change, s: Section, context: Option<u32>) -> Result<String> {
    let u = context.map(|n| format!("-U{n}"));
    let mut base = vec!["diff", "--no-color", "--no-ext-diff", "-M"];
    base.extend(u.as_deref());
    match s {
        Section::Untracked => {
            let mut a = vec!["diff", "--no-color", "--no-index"];
            a.extend(u.as_deref());
            a.extend(["--", "/dev/null", &c.path]);
            run(dir, &a, None, &[1])
        }
        Section::Staged => {
            base.push("--cached");
            run(dir, &with_paths(&base, &paths_of(c)), None, &[])
        }
        Section::Unstaged => run(dir, &with_paths(&base, &paths_of(c)), None, &[]),
    }
}

/// One file of a `git diff`: its header lines (`diff --git` … `+++`) and
/// each hunk's text (from its `@@` line, `\ No newline` markers kept), in
/// the order [`crate::diff::parse`] numbers them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilePatch {
    pub header: String,
    pub hunks: Vec<String>,
}

pub fn split_patch(raw: &str) -> Vec<FilePatch> {
    let mut out: Vec<FilePatch> = Vec::new();
    let mut in_hunk = false;
    for line in raw.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            out.push(FilePatch {
                header: line.to_string(),
                hunks: Vec::new(),
            });
            in_hunk = false;
            continue;
        }
        let Some(f) = out.last_mut() else { continue };
        if line.starts_with("@@ ") {
            f.hunks.push(line.to_string());
            in_hunk = true;
        } else if in_hunk {
            f.hunks.last_mut().unwrap().push_str(line);
        } else {
            f.header.push_str(line);
        }
    }
    out
}

/// A patch holding hunk `i` of `f` alone.
pub fn hunk_patch(f: &FilePatch, i: usize) -> Option<String> {
    Some(format!("{}{}", f.header, f.hunks.get(i)?))
}

/// Hunks are staged one by one only in a modified text file (R21.3).
pub fn hunks_apply(f: &crate::diff::FileDiff) -> bool {
    f.status == 'M' && !f.binary
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HunkOp {
    Stage,
    Unstage,
    Discard,
}

/// Apply one hunk's patch (R21.3): stage = `apply --cached`, unstage =
/// `--cached -R` (patch from `diff --cached`), discard = `-R` (patch from
/// the unstaged diff).
pub fn apply_hunk(dir: &Path, patch: &str, op: HunkOp) -> Result<()> {
    let args: &[&str] = match op {
        HunkOp::Stage => &["apply", "--cached", "--recount", "--whitespace=nowarn", "-"],
        HunkOp::Unstage => &[
            "apply",
            "--cached",
            "-R",
            "--recount",
            "--whitespace=nowarn",
            "-",
        ],
        HunkOp::Discard => &["apply", "-R", "--recount", "--whitespace=nowarn", "-"],
    };
    run(dir, args, Some(patch.as_bytes()), &[]).map(drop)
}

/// Commit the staged changes with `message` (R21.4); returns `<hash> <subject>`.
pub fn commit(dir: &Path, message: &str) -> Result<String> {
    if message.trim().is_empty() {
        return Err(Error::usage("the commit message is empty"));
    }
    run(
        dir,
        &["commit", "-q", "-F", "-"],
        Some(message.as_bytes()),
        &[],
    )?;
    Ok(run(dir, &["log", "-1", "--format=%h %s"], None, &[])?
        .trim()
        .to_string())
}

/// `git pull --ff-only` (R21.5, ADR-0013).
pub fn pull(dir: &Path, env: &Env) -> Result<()> {
    run_env(dir, env, &["pull", "--ff-only", "-q"], None, &[]).map(drop)
}

/// What Push will do (shown in its confirmation, R21.5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PushPlan {
    pub remote: String,
    pub branch: String,
    /// `-u`: no upstream of its own yet.
    pub set_upstream: bool,
    /// Commits the remote doesn't have.
    pub commits: u32,
}

/// The remote to push a branch without an upstream to: `origin`, else the
/// only one.
pub fn choose_remote(remotes: &[String]) -> Option<String> {
    if remotes.iter().any(|r| r == "origin") {
        return Some("origin".into());
    }
    match remotes {
        [one] => Some(one.clone()),
        _ => None,
    }
}

pub fn push_plan(dir: &Path, st: &Status) -> Result<PushPlan> {
    let branch = st
        .branch
        .clone()
        .ok_or_else(|| Error::usage("HEAD is detached: check out a branch to push"))?;
    if let Some((remote, _)) = st.own_upstream() {
        return Ok(PushPlan {
            remote: remote.to_string(),
            branch,
            set_upstream: false,
            commits: st.ahead,
        });
    }
    let remotes: Vec<String> = run(dir, &["remote"], None, &[])?
        .lines()
        .map(String::from)
        .collect();
    let remote = choose_remote(&remotes).ok_or_else(|| {
        Error::usage(if remotes.is_empty() {
            "this repository has no remote to push to".to_string()
        } else {
            format!(
                "several remotes and none is 'origin': {}",
                remotes.join(", ")
            )
        })
    })?;
    let commits = run(
        dir,
        &[
            "rev-list",
            "--count",
            "HEAD",
            "--not",
            &format!("--remotes={remote}"),
        ],
        None,
        &[],
    )
    .ok()
    .and_then(|s| s.trim().parse().ok())
    .unwrap_or(0);
    Ok(PushPlan {
        remote,
        branch,
        set_upstream: true,
        commits,
    })
}

pub fn push(dir: &Path, plan: &PushPlan, env: &Env) -> Result<()> {
    let mut args = vec!["push", "-q"];
    if plan.set_upstream {
        args.push("-u");
    }
    args.push(&plan.remote);
    let refspec = format!("HEAD:refs/heads/{}", plan.branch);
    args.push(&refspec);
    run_env(dir, env, &args, None, &[]).map(drop)
}

/// What a card needs, read in one go off the UI thread (R120, R121).
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub status: Status,
    /// `git diff --cached --numstat -M`: path → (added, removed).
    pub staged: HashMap<String, (usize, usize)>,
    /// `git diff --numstat -M`.
    pub unstaged: HashMap<String, (usize, usize)>,
    /// Line count of each untracked file (< 4 MiB, no NUL; else 0).
    pub untracked_lines: HashMap<String, usize>,
    /// `(short, subject)` of HEAD; `None` before the first commit.
    pub head: Option<(String, String)>,
    /// `git branch -r --contains HEAD` is not empty.
    pub head_pushed: bool,
    /// Commits ahead of `base` (`base..HEAD`, else `origin/base..HEAD`).
    pub ahead_base: usize,
}

fn count_lines(b: &[u8]) -> usize {
    let n = b.iter().filter(|c| **c == b'\n').count();
    if b.last().is_some_and(|c| *c != b'\n') {
        n + 1
    } else {
        n
    }
}

pub fn snapshot(dir: &Path, base: &str) -> Result<Snapshot> {
    let status = status(dir)?;
    let numstat = |args: &[&str]| {
        git::run(dir, args)
            .map(|o| crate::home::numstat(&o))
            .unwrap_or_default()
    };
    let staged = numstat(&["diff", "--cached", "--numstat", "-M"]);
    let unstaged = numstat(&["diff", "--numstat", "-M"]);
    let untracked_lines = status
        .files
        .iter()
        .filter(|c| c.untracked())
        .map(|c| {
            let n = std::fs::read(dir.join(&c.path))
                .ok()
                .filter(|b| b.len() < 4 << 20 && !b.contains(&0))
                .map_or(0, |b| count_lines(&b));
            (c.path.clone(), n)
        })
        .collect();
    let head = git::run(dir, &["log", "-1", "--format=%h%x00%s"])
        .ok()
        .and_then(|o| {
            let (h, s) = o.trim_end_matches('\n').split_once('\0')?;
            Some((h.to_string(), s.to_string()))
        });
    let head_pushed = head.is_some() && head_pushed(dir);
    let ahead_base = git::run(dir, &["rev-list", "--count", &format!("{base}..HEAD")])
        .ok()
        .or_else(|| {
            git::run(
                dir,
                &["rev-list", "--count", &format!("origin/{base}..HEAD")],
            )
            .ok()
        })
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    Ok(Snapshot {
        status,
        staged,
        unstaged,
        untracked_lines,
        head,
        head_pushed,
        ahead_base,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitKind {
    Normal,
    Amend,
    All,
}

/// HEAD is on a remote branch (`git branch -r --contains HEAD`).
pub fn head_pushed(dir: &Path) -> bool {
    git::run(dir, &["branch", "-r", "--contains", "HEAD"])
        .map(|o| !o.trim().is_empty())
        .unwrap_or(false)
}

fn refuse_pushed(dir: &Path) -> Result<()> {
    if head_pushed(dir) {
        let short = git::run(dir, &["log", "-1", "--format=%h"])
            .unwrap_or_default()
            .trim()
            .to_string();
        return Err(Error::usage(format!(
            "{short} is already pushed; RADE does not force-push"
        )));
    }
    Ok(())
}

/// Commit variants (R121.2); returns `<short> <subject>` like [`commit`].
/// `All` stages every change first; `Amend` with an empty message keeps
/// the last one and is refused when HEAD is pushed.
pub fn commit_kind(dir: &Path, message: &str, kind: CommitKind) -> Result<String> {
    match kind {
        CommitKind::Normal => commit(dir, message),
        CommitKind::All => {
            if message.trim().is_empty() {
                return Err(Error::usage("the commit message is empty"));
            }
            let paths: Vec<String> = status(dir)?.files.iter().flat_map(paths_of).collect();
            if !paths.is_empty() {
                on_paths(dir, &["add", "-A"], &paths)?;
            }
            commit(dir, message)
        }
        CommitKind::Amend => {
            refuse_pushed(dir)?;
            if message.trim().is_empty() {
                run(dir, &["commit", "--amend", "--no-edit", "-q"], None, &[])?;
            } else {
                run(
                    dir,
                    &["commit", "--amend", "-q", "-F", "-"],
                    Some(message.as_bytes()),
                    &[],
                )?;
            }
            Ok(run(dir, &["log", "-1", "--format=%h %s"], None, &[])?
                .trim()
                .to_string())
        }
    }
}

/// Undo the last commit (R121.2), keeping its changes staged; returns the
/// undone full message. Refused when HEAD is pushed.
pub fn undo_last_commit(dir: &Path) -> Result<String> {
    let msg = git::run(dir, &["log", "-1", "--format=%B"])
        .map_err(|_| Error::usage("there is no commit to undo"))?;
    refuse_pushed(dir)?;
    if git::run(dir, &["rev-parse", "--verify", "-q", "HEAD~1"]).is_ok() {
        run(dir, &["reset", "--soft", "HEAD~1"], None, &[])?;
    } else {
        run(dir, &["update-ref", "-d", "HEAD"], None, &[])?;
    }
    Ok(msg.trim().to_string())
}

/// The last `n` commit messages, newest first.
pub fn recent_messages(dir: &Path, n: usize) -> Vec<String> {
    git::run(dir, &["log", "-n", &n.to_string(), "--format=%B%x00"])
        .map(|o| {
            o.split('\0')
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// The remote Fetch / Sync use: the own upstream's, else `origin` or the
/// only one.
pub fn fetch_remote(dir: &Path, st: &Status) -> Result<String> {
    if let Some((remote, _)) = st.own_upstream() {
        return Ok(remote.to_string());
    }
    let remotes: Vec<String> = run(dir, &["remote"], None, &[])?
        .lines()
        .map(String::from)
        .collect();
    choose_remote(&remotes).ok_or_else(|| {
        Error::usage(if remotes.is_empty() {
            "this repository has no remote to fetch from".to_string()
        } else {
            format!(
                "several remotes and none is 'origin': {}",
                remotes.join(", ")
            )
        })
    })
}

/// `git fetch --quiet <remote>` (R121.4).
pub fn fetch(dir: &Path, remote: &str, env: &Env) -> Result<()> {
    run_env(dir, env, &["fetch", "--quiet", remote], None, &[]).map(drop)
}

/// Sync (R121.3): pull `--ff-only` when behind, then push when ahead (the
/// plan is recomputed after the pull). A failed pull pushes nothing.
pub fn sync(dir: &Path, env: &Env) -> Result<PushPlan> {
    let st = status(dir)?;
    if st.behind > 0 {
        pull(dir, env)?;
    }
    let st = status(dir)?;
    let plan = push_plan(dir, &st)?;
    if plan.commits > 0 {
        push(dir, &plan, env)?;
    }
    Ok(plan)
}

fn refuse_dotdot(path: &str) -> Result<()> {
    if path.split('/').any(|s| s == "..") {
        return Err(Error::usage(format!("refusing to delete {path}")));
    }
    Ok(())
}

/// Discard many (R120.4): tracked files get their unstaged changes
/// restored (renames: both paths); untracked ones go through `trash`.
/// Returns the untracked paths whose trash failed with the error text.
pub fn discard_many(
    dir: &Path,
    tracked: &[Change],
    untracked: &[String],
    trash: &dyn Fn(&Path) -> std::io::Result<()>,
) -> Result<Vec<(String, String)>> {
    for p in untracked {
        refuse_dotdot(p)?;
    }
    let paths: Vec<String> = tracked.iter().flat_map(paths_of).collect();
    if !paths.is_empty() {
        on_paths(dir, &["restore", "--worktree"], &paths)?;
    }
    let mut failed = Vec::new();
    for p in untracked {
        if let Err(e) = trash(&dir.join(p)) {
            failed.push((p.clone(), e.to_string()));
        }
    }
    Ok(failed)
}

/// Permanently delete untracked files (the caller asked twice).
pub fn delete_untracked(dir: &Path, paths: &[String]) -> Result<()> {
    for p in paths {
        refuse_dotdot(p)?;
    }
    for p in paths {
        std::fs::remove_file(dir.join(p)).map_err(|e| Error::usage(format!("delete {p}: {e}")))?;
    }
    Ok(())
}

/// Append `/<path>` to `<dir>/.gitignore` (R120.5).
pub fn gitignore_add(dir: &Path, path: &str) -> Result<()> {
    refuse_dotdot(path)?;
    let f = dir.join(".gitignore");
    let mut text = std::fs::read_to_string(&f).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("/{path}\n"));
    std::fs::write(&f, text).map_err(|e| Error::usage(format!(".gitignore: {e}")))
}

/// `@@ -a[,b] +c[,d] @@` → (a, b, c, d); a missing count is 1.
fn hunk_range(h: &str) -> Option<(u32, u32, u32, u32)> {
    let mut it = h.split_whitespace();
    it.next()?;
    let one = |s: &str, sign: char| -> Option<(u32, u32)> {
        let s = s.strip_prefix(sign)?;
        match s.split_once(',') {
            Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
            None => Some((s.parse().ok()?, 1)),
        }
    };
    let (a, b) = one(it.next()?, '-')?;
    let (c, d) = one(it.next()?, '+')?;
    Some((a, b, c, d))
}

/// Patch of the selected lines of `f` (R122.3). `selected` = (hunk index,
/// index into `crate::diff::Hunk::lines`). `reverse` = false for Stage,
/// true for Unstage and Discard. None when nothing is selected.
pub fn lines_patch(f: &FilePatch, selected: &[(usize, usize)], reverse: bool) -> Option<String> {
    let mut out = String::new();
    for (hi, hunk) in f.hunks.iter().enumerate() {
        let mut lines = hunk.split_inclusive('\n');
        let header = lines.next()?;
        let (old_start, _, new_start, _) = hunk_range(header)?;
        let mut body = String::new();
        let (mut old_n, mut new_n) = (0u32, 0u32);
        let (mut idx, mut any, mut prev_kept) = (0usize, false, false);
        for line in lines {
            let first = line.as_bytes().first().copied();
            if first == Some(b'\\') {
                if prev_kept {
                    body.push_str(line);
                }
                continue;
            }
            let sel = selected.contains(&(hi, idx));
            idx += 1;
            let rest = line.get(1..).unwrap_or("");
            let kind = match first {
                Some(b'+') | Some(b'-') => first.unwrap(),
                _ => b' ',
            };
            let keep_as = match (kind, reverse) {
                (b' ', _) => Some(b' '),
                (b'-', false) | (b'+', true) => Some(if sel { kind } else { b' ' }),
                _ => sel.then_some(kind),
            };
            prev_kept = keep_as.is_some();
            let Some(k) = keep_as else { continue };
            if sel && kind != b' ' {
                any = true;
            }
            match k {
                b' ' => {
                    old_n += 1;
                    new_n += 1;
                }
                b'-' => old_n += 1,
                _ => new_n += 1,
            }
            if first == Some(b' ') || first == Some(b'+') || first == Some(b'-') {
                body.push(k as char);
                body.push_str(rest);
            } else {
                // An empty context line.
                body.push_str(line);
            }
        }
        if any {
            out.push_str(&format!(
                "@@ -{old_start},{old_n} +{new_start},{new_n} @@\n{body}"
            ));
        }
    }
    (!out.is_empty()).then(|| format!("{}{out}", f.header))
}

/// Quick Diff (R123): the `-U0` diff of `file` against the index; None
/// when the file is not in a work tree, untracked or ignored.
pub fn quick_diff(file: &Path) -> Result<Option<String>> {
    quick_diff_with(file, "-U0")
}

fn quick_diff_with(file: &Path, context: &str) -> Result<Option<String>> {
    let Some(parent) = file.parent() else {
        return Ok(None);
    };
    let Ok(top) = git::run(parent, &["rev-parse", "--show-toplevel"]) else {
        return Ok(None);
    };
    let top = std::path::PathBuf::from(top.trim());
    let Ok(abs) = file.canonicalize() else {
        return Ok(None);
    };
    let Ok(rel) = abs.strip_prefix(&top) else {
        return Ok(None);
    };
    let rel = rel.to_string_lossy().into_owned();
    if run(
        &top,
        &["ls-files", "--error-unmatch", "--", &rel],
        None,
        &[],
    )
    .is_err()
    {
        return Ok(None);
    }
    let out = run(
        &top,
        &["diff", context, "--no-color", "--no-ext-diff", "--", &rel],
        None,
        &[],
    )?;
    Ok(Some(out))
}

/// The hunk of `file`'s `-U3` diff against the index whose new range holds
/// `line` (or a pure deletion at `line`): its file patch and hunk index.
pub fn hunk_at_line(file: &Path, line: u32) -> Result<Option<(FilePatch, usize)>> {
    let Some(raw) = quick_diff_with(file, "-U3")? else {
        return Ok(None);
    };
    let Some(f) = split_patch(&raw).into_iter().next() else {
        return Ok(None);
    };
    let found = f.hunks.iter().position(|h| {
        hunk_range(h.lines().next().unwrap_or("")).is_some_and(|(_, _, c, d)| {
            if d == 0 {
                (c + 1).max(1) == line
            } else {
                (c..c + d).contains(&line)
            }
        })
    });
    Ok(found.map(|i| (f, i)))
}

/// The PR base offered by default: the Source's main branch without its
/// remote (`origin/main` → `main`).
pub fn pr_base(default_branch: Option<&str>) -> String {
    let b = default_branch.unwrap_or("main");
    b.split_once('/')
        .filter(|(r, _)| *r == "origin" || *r == "upstream")
        .map_or(b, |(_, b)| b)
        .to_string()
}

/// Create the PR with `gh` (R21.6); the branch is already pushed. Returns
/// gh's last output line (the PR URL).
pub fn create_pr(
    dir: &Path,
    gh: &str,
    branch: &str,
    base: &str,
    title: &str,
    body: &str,
    env: &Env,
) -> Result<String> {
    if title.trim().is_empty() {
        return Err(Error::usage("the pull request needs a title"));
    }
    let out = Command::new(gh)
        .args(["pr", "create", "--head", branch, "--base", base])
        .args(["--title", title.trim(), "--body", body])
        .current_dir(dir)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| {
            Error::usage(if e.kind() == std::io::ErrorKind::NotFound {
                "GitHub CLI (gh) is not installed: see https://cli.github.com, then `gh auth login`"
                    .to_string()
            } else {
                format!("cannot run gh: {e}")
            })
        })?;
    let text = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        let msg = if err.is_empty() { text.trim() } else { err };
        return Err(Error::usage(
            msg.split_whitespace().collect::<Vec<_>>().join(" "),
        ));
    }
    Ok(text.lines().last().unwrap_or("").trim().to_string())
}

// ---- Drafts (Generate, R21.7) ----------------------------------------------

/// `[scm] draft_command`'s default: Claude Code headless, no tools (it only
/// writes text; ADR-0013).
pub const DEFAULT_DRAFT_COMMAND: &str = "claude -p --tools \"\" --no-session-persistence";
pub const DRAFT_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_DIFF: usize = 60_000;
const MAX_RULES: usize = 8_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraftKind {
    Commit,
    Pr,
}

/// What the Draft prompt is built from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DraftInput {
    pub diff: String,
    /// (file name, contents) of the repository's rules files.
    pub rules: Vec<(String, String)>,
    /// Recent commit subjects (style examples).
    pub log: Vec<String>,
    /// PR: the branch, its base and its commits' subjects.
    pub branch: Option<String>,
    pub base: Option<String>,
    pub commits: Vec<String>,
}

/// `s` cut to at most `max` bytes (on a char boundary); whether it was cut.
pub(crate) fn cut(s: &str, max: usize) -> (&str, bool) {
    if s.len() <= max {
        return (s, false);
    }
    let mut i = max;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    (&s[..i], true)
}

pub fn draft_prompt(kind: DraftKind, i: &DraftInput) -> String {
    let mut p = String::new();
    match kind {
        DraftKind::Commit => p.push_str(
            "Write a git commit message for the staged changes below. Follow this \
             repository's commit conventions (its rules files and recent commit subjects \
             below). Reply with the commit message only: a subject line, then optionally a \
             blank line and a short body. No preamble, no code fences, no quotes.\n",
        ),
        DraftKind::Pr => p.push_str(&format!(
            "Write a pull request title and description for merging branch `{}` into `{}`. \
             Follow this repository's conventions and its pull request template if there is \
             one. Reply with the title on the first line, a blank line, then the description \
             in Markdown. No preamble, no code fences around the whole reply.\n",
            i.branch.as_deref().unwrap_or("HEAD"),
            i.base.as_deref().unwrap_or("main"),
        )),
    }
    for (name, text) in &i.rules {
        let (t, was_cut) = cut(text, MAX_RULES);
        p.push_str(&format!("\n## {name}\n\n{}\n", t.trim_end()));
        if was_cut {
            p.push_str("[… cut]\n");
        }
    }
    if !i.log.is_empty() {
        p.push_str("\n## Recent commit subjects\n\n");
        for l in &i.log {
            p.push_str(&format!("- {l}\n"));
        }
    }
    if !i.commits.is_empty() {
        p.push_str("\n## Commits on this branch\n\n");
        for l in &i.commits {
            p.push_str(&format!("- {l}\n"));
        }
    }
    let (d, was_cut) = cut(&i.diff, MAX_DIFF);
    p.push_str(match kind {
        DraftKind::Commit => "\n## Staged diff\n\n",
        DraftKind::Pr => "\n## Diff against the base\n\n",
    });
    p.push_str("```diff\n");
    p.push_str(d.trim_end());
    p.push_str("\n```\n");
    if was_cut {
        p.push_str(&format!(
            "(The diff was cut at {MAX_DIFF} bytes of {}.)\n",
            i.diff.len()
        ));
    }
    p
}

/// The Draft command's reply → (message or PR title, PR body).
pub fn parse_draft(kind: DraftKind, out: &str) -> (String, String) {
    let mut t = out.trim();
    // A reply wrapped in one code fence.
    if t.starts_with("```") && t.ends_with("```") && t.len() > 6 {
        t = t[3..t.len() - 3].trim();
        if let Some((first, rest)) = t.split_once('\n') {
            if !first.contains(' ') {
                t = rest.trim(); // the fence's language tag
            }
        }
    }
    match kind {
        DraftKind::Commit => (t.to_string(), String::new()),
        DraftKind::Pr => {
            let (title, body) = t.split_once('\n').unwrap_or((t, ""));
            let title = title.trim().trim_start_matches('#').trim();
            let title = title.strip_prefix("Title:").unwrap_or(title).trim();
            (title.to_string(), body.trim().to_string())
        }
    }
}

const RULES_FILES: &[&str] = &[
    "CLAUDE.md",
    "AGENTS.md",
    "CONTRIBUTING.md",
    "docs/CONTRIBUTING.md",
    ".github/CONTRIBUTING.md",
];
const PR_TEMPLATES: &[&str] = &[
    ".github/pull_request_template.md",
    ".github/PULL_REQUEST_TEMPLATE.md",
    "docs/pull_request_template.md",
    "pull_request_template.md",
];

/// Gather the Draft's input in the Worktree at `dir` (R21.7). PR: `base`
/// is the ref the branch is compared with (`origin/main`, `main`).
pub fn gather_draft(dir: &Path, kind: DraftKind, base: Option<&str>) -> Result<DraftInput> {
    let mut i = DraftInput::default();
    let mut files: Vec<&str> = RULES_FILES.to_vec();
    if kind == DraftKind::Pr {
        files.extend(PR_TEMPLATES);
    }
    for f in files {
        if let Ok(t) = std::fs::read_to_string(dir.join(f)) {
            i.rules.push((f.to_string(), t));
        }
    }
    i.log = run(dir, &["log", "-10", "--format=%s"], None, &[])
        .map(|s| s.lines().map(String::from).collect())
        .unwrap_or_default();
    match kind {
        DraftKind::Commit => {
            i.diff = run(
                dir,
                &["diff", "--cached", "--no-color", "--no-ext-diff", "-M"],
                None,
                &[],
            )?;
            if i.diff.trim().is_empty() {
                return Err(Error::usage("nothing is staged: stage changes first"));
            }
        }
        DraftKind::Pr => {
            let base = base.unwrap_or("main");
            i.branch = git::branch(dir);
            i.base = Some(pr_base(Some(base)));
            i.diff = run(
                dir,
                &[
                    "diff",
                    "--no-color",
                    "--no-ext-diff",
                    "-M",
                    &format!("{base}...HEAD"),
                ],
                None,
                &[],
            )?;
            i.commits = run(
                dir,
                &["log", "--format=%s", &format!("{base}..HEAD")],
                None,
                &[],
            )
            .map(|s| s.lines().map(String::from).collect())
            .unwrap_or_default();
            if i.commits.is_empty() && i.diff.trim().is_empty() {
                return Err(Error::usage(format!(
                    "no commits on this branch beyond {base}"
                )));
            }
        }
    }
    Ok(i)
}

/// Run the Draft command (`sh -c`) in `dir` with `prompt` on stdin; its
/// stdout. Killed (its whole process group) at `timeout` or when `cancel`
/// is set.
pub fn run_draft(
    command: &str,
    dir: &Path,
    prompt: &str,
    timeout: Duration,
    cancel: &AtomicBool,
    env: &Env,
) -> Result<String> {
    use std::os::unix::process::CommandExt;
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(dir)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::internal(format!("cannot run the draft command: {e}")))?;
    let pgid = child.id() as i32;
    let mut stdin = child.stdin.take().unwrap();
    let prompt = prompt.to_string();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(prompt.as_bytes());
    });
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });
    let mut stderr = child.stderr.take().unwrap();
    let ereader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child
            .try_wait()
            .map_err(|e| Error::internal(e.to_string()))?
        {
            break s;
        }
        let stop = if cancel.load(Ordering::Relaxed) {
            Some("cancelled")
        } else if start.elapsed() > timeout {
            Some("timed out")
        } else {
            None
        };
        if let Some(why) = stop {
            crate::process::signal_group(pgid, crate::process::SIGKILL);
            let _ = child.wait();
            return Err(Error::usage(format!("the draft command {why}")));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let _ = writer.join();
    let out = reader.join().unwrap_or_default();
    let err = ereader.join().unwrap_or_default();
    if !status.success() {
        let tail: String = err
            .trim()
            .lines()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .join(" / ");
        return Err(Error::usage(format!(
            "the draft command failed ({status}){}",
            if tail.is_empty() {
                String::new()
            } else {
                format!(": {tail}")
            }
        )));
    }
    if out.trim().is_empty() {
        return Err(Error::usage("the draft command printed nothing"));
    }
    Ok(out)
}

#[cfg(test)]
#[path = "scm_tests.rs"]
mod tests;
