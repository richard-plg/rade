//! Source Control for the Right Panel's Changes page (PRD R21, ADR-0013):
//! `git status --porcelain=v2` parsing, stage / unstage / discard per file
//! and per hunk (one hunk cut from `git diff` and fed to `git apply`),
//! commit, pull, push, `gh pr create`, and the Draft (Generate) prompt and
//! runner. Every operation is a plain `git` / `gh` call the app makes on
//! the owner's click; nothing here runs on its own.

use crate::git;
use crate::rpc::{Error, Result};
use serde::{Deserialize, Serialize};
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
    let base = ["diff", "--no-color", "--no-ext-diff", "-M"];
    match s {
        Section::Untracked => run(
            dir,
            &[
                "diff",
                "--no-color",
                "--no-index",
                "--",
                "/dev/null",
                &c.path,
            ],
            None,
            &[1],
        ),
        Section::Staged => {
            let mut a = base.to_vec();
            a.push("--cached");
            run(dir, &with_paths(&a, &paths_of(c)), None, &[])
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
        HunkOp::Stage => &["apply", "--cached", "--whitespace=nowarn", "-"],
        HunkOp::Unstage => &["apply", "--cached", "-R", "--whitespace=nowarn", "-"],
        HunkOp::Discard => &["apply", "-R", "--whitespace=nowarn", "-"],
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
