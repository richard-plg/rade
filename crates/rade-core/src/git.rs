//! Git facts and operations through the `git` CLI (R1.6, R12, R13): the
//! checked-out branch, `git worktree` add/list/remove, the Source's default
//! branch, dirty checks. Parsing is pure and unit-tested; the runners are
//! thin wrappers tested against real temp repositories.

use crate::rpc::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Branch name from `<repo>/.git/HEAD` (following a `.git` file for worktrees),
/// a short commit hash when detached, `None` if not a git checkout.
pub fn branch(repo: &Path) -> Option<String> {
    let dot_git = repo.join(".git");
    let git_dir = if dot_git.is_file() {
        let text = std::fs::read_to_string(&dot_git).ok()?;
        let dir = text.strip_prefix("gitdir:")?.trim();
        repo.join(dir)
    } else {
        dot_git
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref: ") {
        Some(r) => Some(r.strip_prefix("refs/heads/").unwrap_or(r).to_string()),
        None => Some(head.chars().take(7).collect()),
    }
}

/// A `git` command in `dir`: C locale (stable messages), never prompts,
/// never takes optional locks (the Agent working there owns the index).
pub fn command(dir: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C")
        .arg(dir)
        .args(["-c", "core.quotePath=false"])
        .env("LC_ALL", "C")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null());
    c
}

/// Run `git args` in `dir`; stdout on success, else an error carrying
/// git's own message (so the UI and CLI can show it).
pub fn run(dir: &Path, args: &[&str]) -> Result<String> {
    let out = command(dir)
        .args(args)
        .output()
        .map_err(|e| Error::internal(format!("cannot run git: {e}")))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(Error::usage(format!(
        "git {}: {}",
        args.first().unwrap_or(&""),
        if msg.is_empty() { "failed" } else { &msg }
    )))
}

pub(crate) fn ok(dir: &Path, args: &[&str]) -> bool {
    command(dir)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Top level of the checkout containing `dir`; `None` outside git.
pub fn toplevel(dir: &Path) -> Option<PathBuf> {
    run(dir, &["rev-parse", "--show-toplevel"])
        .ok()
        .map(|s| PathBuf::from(s.trim()))
}

/// Branch names a Worktree may be called (`git check-ref-format --branch`
/// rules, plus: one line, not `new` — `--worktree new` means "create").
pub fn validate_name(name: &str) -> Result<()> {
    let bad = |why: &str| {
        Err(Error::usage(format!(
            "invalid worktree name '{name}': {why}"
        )))
    };
    if name.is_empty() {
        return bad("empty");
    }
    if name == "new" {
        return bad("'new' is reserved (spawn --worktree new)");
    }
    if name == "@" || name.starts_with('-') {
        return bad("not a valid branch name");
    }
    if let Some(c) = name
        .chars()
        .find(|c| c.is_ascii_control() || " ~^:?*[\\".contains(*c))
    {
        return bad(&format!("contains {c:?}"));
    }
    if name.contains("..") || name.contains("@{") || name.contains("//") {
        return bad("contains '..', '@{' or '//'");
    }
    if name.starts_with('/') || name.ends_with('/') || name.ends_with('.') {
        return bad("starts or ends with '/' or ends with '.'");
    }
    if name
        .split('/')
        .any(|c| c.starts_with('.') || c.ends_with(".lock"))
    {
        return bad("a part starts with '.' or ends with '.lock'");
    }
    Ok(())
}

/// Where RADE puts a Source's Worktree (ADR-0007):
/// `$XDG_DATA_HOME/rade/worktrees/<source>/<name>`.
pub fn worktree_dir(data_dir: &Path, source: &str, name: &str) -> PathBuf {
    data_dir.join("worktrees").join(source).join(name)
}

/// `wt-<agent>-<n>` with the smallest `n` not taken (`spawn --worktree new`
/// without `--name`); `wt-shell-<n>` without an Agent.
pub fn auto_name(agent: Option<&str>, taken: &dyn Fn(&str) -> bool) -> String {
    let a = crate::model::slug(agent.unwrap_or("shell"));
    (1..)
        .map(|n| format!("wt-{a}-{n}"))
        .find(|n| !taken(n))
        .expect("unbounded")
}

/// One entry of `git worktree list --porcelain`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitWorktree {
    pub path: PathBuf,
    pub head: Option<String>,
    /// Short branch name; `None` when detached.
    pub branch: Option<String>,
    pub bare: bool,
    /// `Some(reason)` (maybe empty) when locked.
    pub locked: Option<String>,
    /// Its folder is gone (`git worktree prune` would drop it).
    pub prunable: bool,
}

/// Parse `git worktree list --porcelain [-z]` (records separated by an
/// empty line; with `-z` every line ends in NUL, so paths may hold `\n`).
pub fn parse_worktree_list(text: &str) -> Vec<GitWorktree> {
    let mut out = Vec::new();
    let mut cur: Option<GitWorktree> = None;
    let lines: Vec<&str> = if text.contains('\0') {
        text.split('\0').collect()
    } else {
        text.lines().collect()
    };
    for line in lines {
        let (key, val) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "worktree" => {
                out.extend(cur.take());
                cur = Some(GitWorktree {
                    path: val.into(),
                    ..Default::default()
                });
            }
            _ => {
                let Some(w) = cur.as_mut() else { continue };
                match key {
                    "HEAD" => w.head = Some(val.into()),
                    "branch" => {
                        w.branch = Some(val.strip_prefix("refs/heads/").unwrap_or(val).into())
                    }
                    "bare" => w.bare = true,
                    "locked" => w.locked = Some(val.into()),
                    "prunable" => w.prunable = true,
                    _ => {}
                }
            }
        }
    }
    out.extend(cur);
    out
}

pub fn list_worktrees(repo: &Path) -> Result<Vec<GitWorktree>> {
    run(repo, &["worktree", "list", "--porcelain", "-z"]).map(|t| parse_worktree_list(&t))
}

/// The branch git has checked out in the Worktree at `dir`; `None` when
/// detached (never a commit hash, unlike [`branch`]).
pub fn worktree_branch(repo: &Path, dir: &Path) -> Option<String> {
    list_worktrees(repo)
        .ok()?
        .into_iter()
        .find(|w| same(&w.path, dir))?
        .branch
}

/// `dir` is the top of a git checkout (a Source inside another repository
/// must not act on that repository).
pub fn require_toplevel(dir: &Path) -> Result<()> {
    match toplevel(dir) {
        Some(top) if same(&top, dir) => Ok(()),
        Some(top) => Err(Error::usage(format!(
            "{} is inside the git repository {}, not its top folder",
            dir.display(),
            top.display()
        ))),
        None => Err(Error::usage(format!(
            "{} is not a git repository",
            dir.display()
        ))),
    }
}

/// The Source's main branch (R13.1): `origin/HEAD`'s target (e.g.
/// `origin/main`), else a local `main`, else `master`.
pub fn choose_default_branch(
    origin_head: Option<&str>,
    has_local: &dyn Fn(&str) -> bool,
) -> Option<String> {
    if let Some(r) = origin_head.map(str::trim).filter(|r| !r.is_empty()) {
        return Some(r.strip_prefix("refs/remotes/").unwrap_or(r).to_string());
    }
    ["main", "master"]
        .into_iter()
        .find(|b| has_local(b))
        .map(Into::into)
}

pub fn default_branch(repo: &Path) -> Option<String> {
    let origin = run(
        repo,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
    )
    .ok();
    choose_default_branch(origin.as_deref(), &|b| has_branch(repo, b))
}

pub fn has_branch(repo: &Path, name: &str) -> bool {
    ok(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{name}"),
        ],
    )
}

/// A branch's commit, `None` without it.
pub fn head(repo: &Path, name: &str) -> Option<String> {
    run(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{name}"),
        ],
    )
    .ok()
    .map(|s| s.trim().to_string())
}

/// Local and remote-tracking branches, for the "base" choice (R12.1).
pub fn branches(repo: &Path) -> Vec<String> {
    run(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes",
        ],
    )
    .unwrap_or_default()
    .lines()
    .filter(|r| !r.ends_with("/HEAD"))
    .map(|r| {
        r.strip_prefix("refs/heads/")
            .or_else(|| r.strip_prefix("refs/remotes/"))
            .unwrap_or(r)
            .to_string()
    })
    .collect()
}

/// The ref a base names (R99.1): `base` itself when it resolves to a commit,
/// else `origin/<base>` when only the remote has it (a Project's base such as
/// `release/2.4`), else `base` unchanged (git then reports it).
pub fn start_point(repo: &Path, base: &str) -> String {
    let resolves = |r: &str| {
        ok(
            repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{r}^{{commit}}"),
            ],
        )
    };
    if resolves(base) {
        return base.to_string();
    }
    let remote = format!("origin/{base}");
    if ok(
        repo,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/remotes/{remote}"),
        ],
    ) {
        return remote;
    }
    base.to_string()
}

/// What `worktree add` did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Added {
    /// A new branch was made (`-b`); false = an existing one checked out.
    pub created_branch: bool,
    /// The base the new branch started at (`None` for an existing branch).
    pub base: Option<String>,
}

/// `git init -b <branch>` in `dir`, nothing else: no file added, no
/// commit (R131.2).
pub fn init(dir: &Path, branch: &str) -> Result<()> {
    if branch.is_empty() || branch.starts_with('-') {
        return Err(Error::usage(format!("invalid branch '{branch}'")));
    }
    run(dir, &["init", "-q", "-b", branch]).map(|_| ())
}

/// `git worktree add` (R12.1): a new branch `name` at `base` (default: the
/// Source's main branch, else HEAD), or — when branch `name` exists — that
/// branch checked out (passing `base` then is an error).
pub fn add_worktree(repo: &Path, dir: &Path, name: &str, base: Option<&str>) -> Result<Added> {
    validate_name(name)?;
    require_toplevel(repo)?;
    if base.is_some_and(|b| b.starts_with('-') || b.is_empty()) {
        return Err(Error::usage(format!("invalid base '{}'", base.unwrap())));
    }
    if dir.exists() {
        return Err(Error::usage(format!("{} already exists", dir.display())));
    }
    // A fresh `git init` (R131.3): nothing to cut a branch from.
    if run(repo, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_err()
        && run(
            repo,
            &["for-each-ref", "--count=1", "refs/heads", "refs/remotes"],
        )
        .is_ok_and(|o| o.trim().is_empty())
    {
        return Err(Error::usage(format!(
            "{} has no commits yet: commit once, then try again",
            repo.display()
        )));
    }
    let d = dir.to_string_lossy();
    if has_branch(repo, name) {
        if base.is_some() {
            return Err(Error::usage(format!(
                "branch '{name}' already exists; omit --base to check it out"
            )));
        }
        run(repo, &["worktree", "add", &d, name])?;
        return Ok(Added {
            created_branch: false,
            base: None,
        });
    }
    let base = match base {
        Some(b) => b.to_string(),
        None => default_branch(repo).unwrap_or_else(|| "HEAD".into()),
    };
    // `--no-track`: a branch cut from `origin/<base>` must not push to it.
    let start = start_point(repo, &base);
    run(
        repo,
        &["worktree", "add", "--no-track", "-b", name, &d, &start],
    )?;
    Ok(Added {
        created_branch: true,
        base: Some(base),
    })
}

/// Paths with uncommitted changes or untracked files (R12.3 refusal).
pub fn dirty(dir: &Path) -> Result<Vec<String>> {
    let out = run(
        dir,
        &["status", "--porcelain", "-z", "--untracked-files=all"],
    )?;
    Ok(parse_status_z(&out))
}

/// Paths of `git status --porcelain -z` (`XY path\0`; a rename or copy
/// is followed by its source path, which is skipped).
pub fn parse_status_z(out: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut it = out.split('\0');
    while let Some(e) = it.next() {
        let Some(path) = e.get(3..).filter(|p| !p.is_empty()) else {
            continue;
        };
        paths.push(path.to_string());
        if e.starts_with(['R', 'C']) {
            it.next();
        }
    }
    paths
}

/// `git worktree remove` (`--force` also drops uncommitted changes). A
/// locked Worktree is refused: unlocking is the user's explicit call.
pub fn remove_worktree(repo: &Path, dir: &Path, force: bool) -> Result<()> {
    if let Some(reason) = lock_reason(repo, dir)? {
        return Err(Error::usage(format!(
            "worktree {} is locked{}; run `git worktree unlock` first",
            dir.display(),
            if reason.is_empty() {
                String::new()
            } else {
                format!(" ({reason})")
            }
        )));
    }
    let d = dir.to_string_lossy();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&d);
    run(repo, &args).map(drop)
}

/// `Some(reason)` (maybe empty) when git has the Worktree at `dir` locked.
pub fn lock_reason(repo: &Path, dir: &Path) -> Result<Option<String>> {
    Ok(list_worktrees(repo)?
        .into_iter()
        .find(|w| same(&w.path, dir))
        .and_then(|w| w.locked))
}

/// `git branch -d` (or `-D` when `force`).
pub fn delete_branch(repo: &Path, name: &str, force: bool) -> Result<()> {
    run(repo, &["branch", if force { "-D" } else { "-d" }, name]).map(drop)
}

pub fn prune(repo: &Path) -> Result<()> {
    run(repo, &["worktree", "prune"]).map(drop)
}

/// Same folder, tolerating symlinks and trailing slashes.
pub fn same(a: &Path, b: &Path) -> bool {
    let c = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    a == b || c(a) == c(b)
}

#[cfg(test)]
#[path = "git_tests.rs"]
pub(crate) mod tests;
