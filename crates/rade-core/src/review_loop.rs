//! The Review Loop (PRD R118, R119, ADR-0040): what the Review Pane shows
//! around the diff — whose work it is (the head), the files rail as a tree
//! with lock / generated files last, Viewed marks that reset when a file's
//! diff changes, and the Pane's own key moves over the rows.

use crate::agents::AgentStatus;
use crate::design::AgentMark;
use crate::diff::{FileDiff, Row};
use crate::model::State;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// A Worktree's Review state kept in `state.json` (R118.3, R119).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Loop {
    /// Viewed files: path → [`file_hash`] of its content when marked.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub viewed: BTreeMap<String, u64>,
    /// Rounds, oldest first; at most [`ROUNDS_KEPT`] (R119.1).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rounds: Vec<Round>,
    /// What the Agent said when it asked for a review (R119.6), until the
    /// next Round.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// The files differ from the last Round's snapshot, as an open Review
    /// last saw (never saved): the Navigator chip's `changed after approval`.
    #[serde(skip)]
    pub changed_since: bool,
}

/// One pass of the owner over the Review (R119.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Round {
    pub n: u32,
    /// Unix seconds.
    pub at: u64,
    /// A commit of the Worktree's files then ([`snapshot`]).
    pub snapshot: String,
    /// Approved, else changes were requested.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub approved: bool,
}

/// Rounds kept per Worktree; older ones (and their refs) are dropped.
pub const ROUNDS_KEPT: usize = 20;

/// A Round's state as the header and `review.status` say it (R119.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RoundState {
    ChangesRequested,
    Approved,
    ChangedAfterApproval,
}

impl RoundState {
    pub fn of(r: &Round, changed_since: bool) -> Self {
        match (r.approved, changed_since) {
            (false, _) => Self::ChangesRequested,
            (true, false) => Self::Approved,
            (true, true) => Self::ChangedAfterApproval,
        }
    }

    pub fn words(self) -> &'static str {
        match self {
            Self::ChangesRequested => "changes requested",
            Self::Approved => "approved",
            Self::ChangedAfterApproval => "changed after approval",
        }
    }
}

/// A comment's state (R119.3): facts, not a judgement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentState {
    Draft,
    /// Sent; the Agent's turn after its Round has not ended yet.
    Sent,
    /// Its lines (with their context) are no longer in the file.
    Changed,
    Untouched,
}

impl CommentState {
    pub fn words(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Sent => "sent",
            Self::Changed => "code changed",
            Self::Untouched => "untouched",
        }
    }
}

/// `c`'s state: `turn_ended` = an Agent turn ended after its Round;
/// `file` = the file's text now (`None`: gone).
pub fn comment_state(
    c: &crate::review::ReviewComment,
    turn_ended: bool,
    file: Option<&str>,
) -> CommentState {
    if c.round.is_none() {
        return CommentState::Draft;
    }
    if !turn_ended {
        return CommentState::Sent;
    }
    let mut block: Vec<String> = Vec::new();
    if let Some(ctx) = &c.context {
        block.extend(ctx.before.iter().cloned());
    }
    block.extend(
        c.excerpt
            .iter()
            .filter(|l| !l.starts_with('-'))
            .map(|l| l.get(1..).unwrap_or("").to_string()),
    );
    if let Some(ctx) = &c.context {
        block.extend(ctx.after.iter().cloned());
    }
    let lines: Vec<&str> = file.map(|f| f.lines().collect()).unwrap_or_default();
    let there = !block.is_empty()
        && file.is_some()
        && lines
            .windows(block.len())
            .any(|w| w.iter().zip(&block).all(|(a, b)| *a == b.as_str()));
    if there && !c.outdated {
        CommentState::Untouched
    } else {
        CommentState::Changed
    }
}

/// An item of the Review's *Approve ▾* menu (R119.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Approve {
    AskCommit,
    AskCommitPr,
    CommitMyself,
    CreatePr,
    ChooseWinner,
}

impl Approve {
    /// The menu for a Worktree with / without a live Agent, in a Fan-out
    /// or not, in R119.4's order.
    pub fn items(live_agent: bool, fanout: bool) -> Vec<Self> {
        let mut v = Vec::new();
        if live_agent {
            v.extend([Self::AskCommit, Self::AskCommitPr]);
        }
        v.extend([Self::CommitMyself, Self::CreatePr]);
        if fanout {
            v.push(Self::ChooseWinner);
        }
        v
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::AskCommit => "Ask Agent to commit",
            Self::AskCommitPr => "Ask Agent to commit and open a PR",
            Self::CommitMyself => "Commit myself…",
            Self::CreatePr => "Create PR…",
            Self::ChooseWinner => "Choose as Winner…",
        }
    }

    /// The prompt an "Ask Agent" item sends.
    pub fn prompt(self, round: u32, base: &str) -> Option<String> {
        let head = format!(
            "Approved (review round {round}). Commit these changes following this repository's conventions."
        );
        match self {
            Self::AskCommit => Some(format!("{head} Do not push.")),
            Self::AskCommitPr => Some(format!(
                "{head} Then push the branch and open a pull request against {base}."
            )),
            _ => None,
        }
    }
}

/// A Worktree's Review chip on its Navigator row (R119.5): `approved`
/// after an approving Round (`R<n> · changed` once the files changed
/// since), `R<n> · <k> open` while sent comments are
/// open, else none.
pub fn chip(w: &crate::model::Worktree) -> Option<String> {
    let last = w.review.rounds.last()?;
    if last.approved {
        // Short: the row's title must keep its room.
        return Some(if w.review.changed_since {
            format!("R{} · changed", last.n)
        } else {
            "approved".into()
        });
    }
    let open = w
        .comments
        .iter()
        .filter(|c| c.round.is_some() && !c.resolved)
        .count();
    (open > 0).then(|| format!("R{} · {open} open", last.n))
}

/// The ref that keeps Round `n`'s snapshot of Worktree `name` alive.
pub fn round_ref(name: &str, n: u32) -> String {
    format!("refs/rade/review/{name}/{n}")
}

/// Snapshot the Worktree at `dir` (working tree incl. untracked files) and
/// keep it under [`round_ref`]; returns the commit.
pub fn snapshot(dir: &Path, name: &str, n: u32) -> crate::rpc::Result<String> {
    let sha = crate::merge::snapshot(dir)?;
    crate::git::run(dir, &["update-ref", &round_ref(name, n), &sha])?;
    Ok(sha)
}

/// Delete the refs of Rounds `ns` (all of the Worktree's when `None`).
pub fn drop_refs(dir: &Path, name: &str, ns: Option<&[u32]>) {
    let refs: Vec<String> = match ns {
        Some(ns) => ns.iter().map(|n| round_ref(name, *n)).collect(),
        None => crate::git::run(
            dir,
            &[
                "for-each-ref",
                "--format=%(refname)",
                &format!("refs/rade/review/{name}/"),
            ],
        )
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect(),
    };
    for r in refs {
        let _ = crate::git::run(dir, &["update-ref", "-d", &r]);
    }
}

impl Loop {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// FNV-1a: stable across Rust versions (unlike `DefaultHasher`), since
/// Viewed marks are saved.
fn fnv(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in parts {
        for x in *b {
            h ^= *x as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Files larger than this are known by size and mtime, not read.
const HASH_READ_MAX: u64 = 16 << 20;

/// What a Viewed mark is checked against (R118.3): the file's status, old
/// path and its content in the working tree (none when deleted). The base
/// is fixed for a Worktree, so a new content is a new diff — and unlike
/// the diff text it does not change when whitespace is hidden.
pub fn file_hash(root: &Path, f: &FileDiff) -> u64 {
    let head = [f.status as u8];
    let old = f.old_path.as_deref().unwrap_or("").as_bytes();
    let path = root.join(&f.path);
    let body = match std::fs::metadata(&path) {
        Ok(m) if f.status != 'D' && m.len() > HASH_READ_MAX => {
            let t = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            format!("{}:{t}", m.len()).into_bytes()
        }
        Ok(_) if f.status != 'D' => std::fs::read(&path).unwrap_or_default(),
        _ => Vec::new(),
    };
    fnv(&[&head, f.path.as_bytes(), old, &body])
}

impl Loop {
    /// `path` is viewed at content `hash` ([`file_hash`]).
    pub fn is_viewed(&self, path: &str, hash: u64) -> bool {
        self.viewed.get(path) == Some(&hash)
    }

    /// Mark or unmark a file as viewed.
    pub fn set_viewed(&mut self, path: &str, hash: u64, on: bool) {
        if on {
            self.viewed.insert(path.to_string(), hash);
        } else {
            self.viewed.remove(path);
        }
    }

    /// (viewed, total) over the changed files `(path, hash)`; marks of
    /// files no longer changed are dropped (they would never match again).
    pub fn progress(&mut self, files: &[(&str, u64)]) -> (usize, usize) {
        let live: HashSet<&str> = files.iter().map(|f| f.0).collect();
        self.viewed.retain(|p, _| live.contains(p.as_str()));
        (
            files.iter().filter(|(p, h)| self.is_viewed(p, *h)).count(),
            files.len(),
        )
    }
}

/// The Review header's Agent (R118.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HeadAgent {
    pub pane: String,
    pub name: String,
    pub mark: AgentMark,
    pub status: Option<AgentStatus>,
    /// Other Agents in the Worktree.
    pub more: usize,
}

/// Whose work a Review shows (R118.1).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Head {
    pub agent: Option<HeadAgent>,
    /// The prompt it was spawned with, else its session title.
    pub task: Option<String>,
}

/// One line, at most this many characters (the header ellipsizes too).
const TASK_CHARS: usize = 200;

impl State {
    /// The head of Worktree `worktree`'s Review: its Agent whose status
    /// changed last (not exited), the others counted, and that Agent's task.
    pub fn review_head(&self, source: &str, worktree: &str) -> Head {
        let mut agents: Vec<_> = self
            .panes_in(source, worktree)
            .into_iter()
            .filter(|p| p.agent_name().is_some() && p.status != Some(AgentStatus::Exited))
            .collect();
        agents.sort_by_key(|p| std::cmp::Reverse(p.status_since.unwrap_or(0)));
        let Some(p) = agents.first() else {
            return Head::default();
        };
        let task = p
            .task
            .as_deref()
            .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|t| !t.is_empty())
            .or_else(|| p.session_title())
            .map(|t| match t.char_indices().nth(TASK_CHARS) {
                Some((i, _)) => format!("{}…", &t[..i]),
                None => t,
            });
        Head {
            agent: Some(HeadAgent {
                pane: p.id.clone(),
                name: p
                    .name
                    .clone()
                    .unwrap_or_else(|| p.agent_name().unwrap().to_string()),
                mark: p.mark().unwrap_or(AgentMark::Other),
                status: p.status,
                more: agents.len() - 1,
            }),
            task,
        }
    }
}

impl State {
    /// The Worktree whose Review an Agent's finished turn offers (R119.5):
    /// an Agent Pane's own Worktree when it is not a Source's main one, or
    /// one already in a review (Rounds or comments).
    pub fn review_target(&self, pane: &str) -> Option<(String, String)> {
        self.pane(pane)?.agent_name()?;
        let (p, w) = self.pane_worktree(pane)?;
        let wt = self.worktree(&p, &w)?;
        (wt.name != crate::model::MAIN_WORKTREE
            || !wt.review.rounds.is_empty()
            || !wt.comments.is_empty())
        .then_some((p, w))
    }

    /// Agent Panes whose turn has ended (`done`) and that offer a Review:
    /// the Toast for each newly finished one (R119.5).
    pub fn finished_for_review(&self) -> Vec<String> {
        self.all_panes()
            .filter(|p| p.status == Some(AgentStatus::Done))
            .filter(|p| self.review_target(&p.id).is_some())
            .map(|p| p.id.clone())
            .collect()
    }

    /// The number the next Round of a Worktree gets.
    pub fn next_round(&self, source: &str, worktree: &str) -> u32 {
        self.worktree(source, worktree)
            .and_then(|w| w.review.rounds.last())
            .map_or(1, |r| r.n + 1)
    }

    /// Record Round `round` (R119.1): the drafts get its number (when
    /// changes are requested), the summary ends; returns the numbers of the
    /// Rounds dropped past [`ROUNDS_KEPT`] (their refs go too).
    pub fn record_round(
        &mut self,
        source: &str,
        worktree: &str,
        round: Round,
    ) -> crate::rpc::Result<Vec<u32>> {
        let wt = self.worktree_mut(source, worktree)?;
        if !round.approved {
            for c in wt.comments.iter_mut().filter(|c| c.round.is_none()) {
                c.round = Some(round.n);
            }
        }
        wt.review.summary = None;
        wt.review.rounds.push(round);
        let extra = wt.review.rounds.len().saturating_sub(ROUNDS_KEPT);
        Ok(wt.review.rounds.drain(..extra).map(|r| r.n).collect())
    }

    /// Resolve a sent comment, or open it again.
    pub fn resolve_comment(
        &mut self,
        source: &str,
        worktree: &str,
        id: u64,
        on: bool,
    ) -> crate::rpc::Result<()> {
        let wt = self.worktree_mut(source, worktree)?;
        let c = wt
            .comments
            .iter_mut()
            .find(|c| c.id == id && c.round.is_some())
            .ok_or_else(|| crate::rpc::Error::not_found(format!("no sent comment {id}")))?;
        c.resolved = on;
        Ok(())
    }

    /// An Agent of the Worktree ended a turn (`done`, `waiting`, `error`)
    /// at or after `at` (R119.3).
    pub fn turn_ended_since(&self, source: &str, worktree: &str, at: u64) -> bool {
        self.panes_in(source, worktree).iter().any(|p| {
            p.agent_name().is_some()
                && matches!(
                    p.status,
                    Some(AgentStatus::Done | AgentStatus::Waiting | AgentStatus::Error)
                )
                // Seconds: a turn that ends in the Round's own second counts.
                && p.status_since.is_some_and(|t| t >= at)
        })
    }

    /// The Agent's summary when it asks for a review (R119.6).
    pub fn set_summary(
        &mut self,
        source: &str,
        worktree: &str,
        text: &str,
    ) -> crate::rpc::Result<()> {
        let t: String = text.trim().chars().take(2000).collect();
        self.worktree_mut(source, worktree)?.review.summary = Some(t).filter(|t| !t.is_empty());
        Ok(())
    }

    /// What an open Review saw: the files differ from the last Round's
    /// snapshot or not. Returns whether it changed.
    pub fn set_changed_since(&mut self, source: &str, worktree: &str, on: bool) -> bool {
        match self.worktree_mut(source, worktree) {
            Ok(w) if w.review.changed_since != on => {
                w.review.changed_since = on;
                true
            }
            _ => false,
        }
    }

    /// Mark or unmark a Worktree's file viewed at content `hash` (R118.3).
    pub fn set_viewed(
        &mut self,
        source: &str,
        worktree: &str,
        path: &str,
        hash: u64,
        on: bool,
    ) -> crate::rpc::Result<()> {
        self.worktree_mut(source, worktree)?
            .review
            .set_viewed(path, hash, on);
        Ok(())
    }

    /// Drop a Worktree's Viewed marks of files not in `files`.
    pub fn prune_viewed(&mut self, source: &str, worktree: &str, files: &[(&str, u64)]) {
        if let Ok(w) = self.worktree_mut(source, worktree) {
            w.review.progress(files);
        }
    }

    /// Keep the prompt an Agent was spawned with (R118.1).
    pub fn set_task(&mut self, pane: &str, task: &str) -> crate::rpc::Result<()> {
        let loc = self.locate_or_err(pane)?;
        let p = self.tab_mut(&loc)?.pane_mut(pane).unwrap();
        p.task = Some(task.to_string()).filter(|t| !t.trim().is_empty());
        Ok(())
    }
}

/// Lock files and generated output by name (R118.2); `.gitattributes`
/// `linguist-generated` adds more ([`generated_attrs`]).
pub fn generated_by_name(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    const LOCKS: [&str; 7] = [
        "Cargo.lock",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "poetry.lock",
        "uv.lock",
        "go.sum",
    ];
    LOCKS.contains(&name)
        || [".min.js", ".min.css", ".snap", ".map"]
            .iter()
            .any(|ext| name.ends_with(ext))
}

/// Paths of `paths` marked `linguist-generated` (set, not `false`) in the
/// repository at `dir` (`git check-attr`); none on any error.
pub fn generated_attrs(dir: &Path, paths: &[&str]) -> HashSet<String> {
    use std::io::Write;
    use std::process::Stdio;
    let Ok(mut c) = crate::git::command(dir)
        .args(["check-attr", "-z", "--stdin", "linguist-generated"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return HashSet::new();
    };
    let mut stdin = c.stdin.take().unwrap();
    let input: Vec<u8> = paths
        .iter()
        .flat_map(|p| [p.as_bytes(), b"\0"])
        .flatten()
        .copied()
        .collect();
    // check-attr answers as it reads: write from a thread so a long list
    // cannot fill both pipes.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let out = c.wait_with_output().map(|o| o.stdout).unwrap_or_default();
    let _ = writer.join();
    let text = String::from_utf8_lossy(&out);
    let f: Vec<&str> = text.split('\0').collect();
    f.chunks(3)
        .filter(|c| c.len() == 3 && !matches!(c[2], "unspecified" | "unset" | "false"))
        .map(|c| c[0].to_string())
        .collect()
}

/// The diff's files in rail order: as git lists them, lock / generated
/// ones last (both keep git's order). Returns the indices into `files`.
pub fn file_order(files: &[FileDiff], generated: &dyn Fn(&str) -> bool) -> Vec<usize> {
    let (mut main, late): (Vec<usize>, Vec<usize>) =
        (0..files.len()).partition(|&i| !generated(&files[i].path));
    main.extend(late);
    main
}

/// One row of the files rail (R118.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RailRow {
    /// A folder (joined with single-child folders: `src/ui/`); `path`
    /// is its full path with a trailing `/`.
    Folder {
        path: String,
        name: String,
        depth: usize,
        folded: bool,
    },
    /// A changed file: `index` into the diff's files.
    File {
        index: usize,
        name: String,
        depth: usize,
    },
    /// The lock / generated group's header.
    Generated { count: usize, folded: bool },
}

/// The key a folded generated group is kept under in `folded`.
pub const GENERATED: &str = "\0generated";

/// The rail for `order` (from [`file_order`]): a tree of folders in diff
/// order, then the lock / generated group (folded unless `folded` lacks
/// [`GENERATED`] — callers start it folded). A non-empty `filter` lists the
/// matching files flat (path substring, case ignored); `hide` drops files
/// (Hide viewed).
pub fn rail(
    files: &[FileDiff],
    order: &[usize],
    generated: &dyn Fn(&str) -> bool,
    folded: &HashSet<String>,
    filter: &str,
    hide: &dyn Fn(usize) -> bool,
) -> Vec<RailRow> {
    let filter = filter.trim().to_lowercase();
    let shown: Vec<usize> = order
        .iter()
        .copied()
        .filter(|&i| !hide(i))
        .filter(|&i| filter.is_empty() || files[i].path.to_lowercase().contains(&filter))
        .collect();
    let (main, late): (Vec<usize>, Vec<usize>) =
        shown.iter().partition(|&&i| !generated(&files[i].path));
    let mut out = Vec::new();
    if filter.is_empty() {
        tree(files, &main, "", 0, folded, &mut out);
    } else {
        out.extend(main.iter().map(|&i| RailRow::File {
            index: i,
            name: files[i].path.clone(),
            depth: 0,
        }));
    }
    if !late.is_empty() {
        let fold = folded.contains(GENERATED);
        out.push(RailRow::Generated {
            count: late.len(),
            folded: fold,
        });
        if !fold {
            out.extend(late.iter().map(|&i| RailRow::File {
                index: i,
                name: files[i].path.clone(),
                depth: 1,
            }));
        }
    }
    out
}

/// Rows for the files `idx` (all under `prefix`), in their order: a file
/// directly in `prefix`, or a folder holding the next run of files.
fn tree(
    files: &[FileDiff],
    idx: &[usize],
    prefix: &str,
    depth: usize,
    folded: &HashSet<String>,
    out: &mut Vec<RailRow>,
) {
    let rest = |i: usize| &files[i].path[prefix.len()..];
    let mut k = 0;
    while k < idx.len() {
        let r = rest(idx[k]);
        let Some((dir, _)) = r.split_once('/') else {
            out.push(RailRow::File {
                index: idx[k],
                name: r.to_string(),
                depth,
            });
            k += 1;
            continue;
        };
        // git lists a folder's files together.
        let head = format!("{dir}/");
        let end = k + idx[k..]
            .iter()
            .take_while(|&&i| rest(i).starts_with(&head))
            .count();
        let group = &idx[k..end];
        // Join single-child folders: while every file sits under one
        // deeper folder and none directly here.
        let mut name = head.clone();
        loop {
            let sub: Vec<Option<&str>> = group
                .iter()
                .map(|&i| rest(i)[name.len()..].split_once('/').map(|(d, _)| d))
                .collect();
            match sub.first() {
                Some(Some(d)) if sub.iter().all(|s| *s == Some(*d)) => {
                    name = format!("{name}{d}/");
                }
                _ => break,
            }
        }
        let path = format!("{prefix}{name}");
        let fold = folded.contains(&path);
        out.push(RailRow::Folder {
            path: path.clone(),
            name: name.clone(),
            depth,
            folded: fold,
        });
        if !fold {
            tree(files, group, &path, depth + 1, folded, out);
        }
        k = end;
    }
}

/// A commit of the branch since its base (the header's `n commits ▾`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Commit {
    pub sha: String,
    pub subject: String,
    /// Committer time, Unix seconds.
    pub at: u64,
}

/// At most this many commits are listed.
pub const COMMITS_MAX: usize = 50;

/// `git log <base>..HEAD`, newest first (none on any error).
pub fn commits(dir: &Path, base: &str) -> Vec<Commit> {
    let range = format!("{base}..HEAD");
    let max = format!("-{COMMITS_MAX}");
    crate::git::run(dir, &["log", &max, "--format=%h%x00%ct%x00%s", &range])
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut f = l.splitn(3, '\0');
            Some(Commit {
                sha: f.next()?.to_string(),
                at: f.next()?.parse().ok()?,
                subject: f.next()?.to_string(),
            })
        })
        .collect()
}

/// The Review Pane's own moves (R118.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    NextHunk,
    PrevHunk,
    NextFile,
    PrevFile,
    NextComment,
    PrevComment,
}

/// The row `m` lands on from row `from` (`None`: before the first row), or
/// `None` when there is nothing further that way (no wrapping).
pub fn step(rows: &[Row], from: Option<usize>, m: Move) -> Option<usize> {
    let hit = |r: &Row| match m {
        Move::NextHunk | Move::PrevHunk => matches!(r, Row::Hunk(..)),
        Move::NextFile | Move::PrevFile => matches!(r, Row::File(_)),
        Move::NextComment | Move::PrevComment => matches!(r, Row::Comment(_)),
    };
    let forward = matches!(m, Move::NextHunk | Move::NextFile | Move::NextComment);
    if forward {
        let start = from.map_or(0, |f| f + 1);
        (start..rows.len()).find(|&i| hit(&rows[i]))
    } else {
        let end = from?;
        (0..end).rev().find(|&i| hit(&rows[i]))
    }
}

/// Where a comment card goes (R118.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    pub id: u64,
    pub path: String,
    /// Its last new-file line.
    pub line: u32,
    /// Outdated, or its line is not in the diff: at the top of its file.
    pub outdated: bool,
}

/// `rows` with comment cards under their last line (ids in order) and the
/// composer after row `composer_after`. A card whose line is not shown
/// (folded hunk, line gone, outdated) goes right under its file's header;
/// a folded file shows none.
pub fn place_cards(
    rows: &[Row],
    files: &[FileDiff],
    cards: &[Card],
    composer_after: Option<usize>,
) -> Vec<Row> {
    let shown_line = |fi: usize, line: u32| {
        rows.iter().any(|r| match *r {
            Row::Line(f, h, l) => f == fi && files[f].hunks[h].lines[l].new == Some(line),
            _ => false,
        })
    };
    let file_of = |path: &str| files.iter().position(|f| f.path == path);
    let mut out = Vec::with_capacity(rows.len() + cards.len() + 1);
    for (i, r) in rows.iter().enumerate() {
        out.push(*r);
        match *r {
            Row::File(fi) => {
                let folded =
                    !matches!(rows.get(i + 1), Some(Row::Hunk(f, _) | Row::Binary(f)) if *f == fi);
                if !folded {
                    out.extend(
                        cards
                            .iter()
                            .filter(|c| file_of(&c.path) == Some(fi))
                            .filter(|c| c.outdated || !shown_line(fi, c.line))
                            .map(|c| Row::Comment(c.id)),
                    );
                }
            }
            Row::Line(fi, h, l) => {
                if let Some(n) = files[fi].hunks[h].lines[l].new {
                    out.extend(
                        cards
                            .iter()
                            .filter(|c| !c.outdated && c.line == n && file_of(&c.path) == Some(fi))
                            .map(|c| Row::Comment(c.id)),
                    );
                }
            }
            _ => {}
        }
        if composer_after == Some(i) {
            out.push(Row::Composer);
        }
    }
    out
}

#[cfg(test)]
#[path = "review_loop_tests.rs"]
mod tests;
