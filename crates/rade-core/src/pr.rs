//! PR status of a Worktree (PRD R146, ADR-0053): parse `gh pr list --json`,
//! choose the PR, roll its checks up, the chip / tooltip / CLI / KPI texts,
//! the read schedule (pure over an injected `now`), the transitions that
//! become Activity rows, Notifications and Toasts, and the one runner that
//! calls `gh` through [`crate::proc`]. The poller thread, the widgets and the
//! socket answers live in `rade-app`.
//!
//! Times are a [`Duration`] on any monotonic clock the caller owns (the time
//! since the app started, say); nothing in here reads a clock.

use crate::proc;
use crate::rpc::{Error, ErrorKind, Result};
use crate::scm::Env;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// The `gh pr list` read's limit (R146.1).
pub const READ_LIMIT: Duration = Duration::from_secs(20);
/// Every 60 s for the shown Workspace while the window is active (R146.2).
pub const ACTIVE_EVERY: Duration = Duration::from_secs(60);
/// ... every 20 s while a PR's checks are pending.
pub const PENDING_EVERY: Duration = Duration::from_secs(20);
/// ... otherwise every 10 min.
pub const IDLE_EVERY: Duration = Duration::from_secs(600);
/// A Workspace whose `gh` failed is left alone for 10 min.
pub const DOWN_FOR: Duration = Duration::from_secs(600);

// ---------------------------------------------------------------------------
// gh's JSON
// ---------------------------------------------------------------------------

/// `gh pr list --head <branch> …` (R146.1).
pub fn gh_list_args(branch: &str) -> Vec<String> {
    let mut a: Vec<String> = ["pr", "list", "--head"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    a.push(branch.to_string());
    for s in [
        "--state",
        "all",
        "--limit",
        "5",
        "--json",
        "number,title,url,state,isDraft,baseRefName,headRefOid,reviewDecision,mergeStateStatus,statusCheckRollup",
    ] {
        a.push(s.to_string());
    }
    a
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawCheck {
    #[serde(rename = "__typename")]
    typename: Option<String>,
    name: Option<String>,
    #[serde(rename = "workflowName")]
    workflow_name: Option<String>,
    status: Option<String>,
    conclusion: Option<String>,
    #[serde(rename = "detailsUrl")]
    details_url: Option<String>,
    context: Option<String>,
    state: Option<String>,
    #[serde(rename = "targetUrl")]
    target_url: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawPr {
    number: u64,
    title: Option<String>,
    url: Option<String>,
    state: Option<String>,
    is_draft: bool,
    base_ref_name: Option<String>,
    head_ref_oid: Option<String>,
    review_decision: Option<String>,
    merge_state_status: Option<String>,
    status_check_rollup: Option<Vec<RawCheck>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PrState {
    Open,
    Merged,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Checks {
    Passing,
    Failing,
    Pending,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Review {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

impl Review {
    fn text(self) -> &'static str {
        match self {
            Review::Approved => "approved",
            Review::ChangesRequested => "changes requested",
            Review::ReviewRequired => "review required",
        }
    }
}

/// One entry of `statusCheckRollup`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    /// A `CheckRun`'s name, a `StatusContext`'s context.
    pub name: String,
    /// The workflow of a `CheckRun` ("" for a `StatusContext`).
    pub workflow: String,
    pub outcome: Checks,
    /// The `CheckRun` conclusion or the `StatusContext` state, lower case
    /// (`failure`, `timed_out`, …); the status when not completed.
    pub conclusion: String,
    pub url: String,
}

/// A parsed `gh pr list` entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pr {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: PrState,
    pub draft: bool,
    pub base: String,
    pub head: String,
    pub review: Option<Review>,
    pub merge_state: String,
    pub checks: Vec<Check>,
}

fn check_of(r: RawCheck) -> Check {
    let upper = |o: &Option<String>| o.as_deref().unwrap_or("").trim().to_uppercase();
    if r.typename.as_deref() == Some("StatusContext") || r.context.is_some() {
        let state = upper(&r.state);
        let outcome = match state.as_str() {
            "FAILURE" | "ERROR" => Checks::Failing,
            "PENDING" | "EXPECTED" | "" => Checks::Pending,
            _ => Checks::Passing,
        };
        return Check {
            name: r.context.or(r.name).unwrap_or_default(),
            workflow: String::new(),
            outcome,
            conclusion: state.to_lowercase(),
            url: r.target_url.or(r.details_url).unwrap_or_default(),
        };
    }
    let status = upper(&r.status);
    let conclusion = upper(&r.conclusion);
    let outcome = if (status.is_empty() && conclusion.is_empty())
        || (!status.is_empty() && status != "COMPLETED")
    {
        Checks::Pending
    } else {
        match conclusion.as_str() {
            "FAILURE" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE" => {
                Checks::Failing
            }
            _ => Checks::Passing,
        }
    };
    let shown = if outcome == Checks::Pending {
        status
    } else {
        conclusion
    };
    Check {
        name: r.name.unwrap_or_default(),
        workflow: r.workflow_name.unwrap_or_default(),
        outcome,
        conclusion: shown.to_lowercase(),
        url: r.details_url.or(r.target_url).unwrap_or_default(),
    }
}

fn pr_of(r: RawPr) -> Pr {
    let state = match r.state.as_deref().unwrap_or("").to_uppercase().as_str() {
        "MERGED" => PrState::Merged,
        "CLOSED" => PrState::Closed,
        _ => PrState::Open,
    };
    let review = match r.review_decision.as_deref().unwrap_or("") {
        "APPROVED" => Some(Review::Approved),
        "CHANGES_REQUESTED" => Some(Review::ChangesRequested),
        "REVIEW_REQUIRED" => Some(Review::ReviewRequired),
        _ => None,
    };
    Pr {
        number: r.number,
        title: r.title.unwrap_or_default(),
        url: r.url.unwrap_or_default(),
        state,
        draft: r.is_draft,
        base: r.base_ref_name.unwrap_or_default(),
        head: r.head_ref_oid.unwrap_or_default(),
        review,
        merge_state: r.merge_state_status.unwrap_or_default(),
        checks: r
            .status_check_rollup
            .unwrap_or_default()
            .into_iter()
            .map(check_of)
            .collect(),
    }
}

/// Parse gh's JSON array.
pub fn parse(json: &str) -> Result<Vec<Pr>> {
    let json = json.trim();
    if json.is_empty() {
        return Ok(Vec::new());
    }
    let raw: Vec<RawPr> = serde_json::from_str(json)
        .map_err(|e| Error::internal(format!("unexpected gh pr list output: {e}")))?;
    Ok(raw.into_iter().map(pr_of).collect())
}

/// The PR shown (R146.1): the newest open one (highest number), else the
/// newest merged, else the newest closed.
pub fn choose(prs: &[Pr]) -> Option<&Pr> {
    [PrState::Open, PrState::Merged, PrState::Closed]
        .into_iter()
        .find_map(|s| prs.iter().filter(|p| p.state == s).max_by_key(|p| p.number))
}

/// The checks rollup (R146.1).
pub fn rollup(checks: &[Check]) -> Checks {
    if checks.iter().any(|c| c.outcome == Checks::Failing) {
        Checks::Failing
    } else if checks.iter().any(|c| c.outcome == Checks::Pending) {
        Checks::Pending
    } else if !checks.is_empty() {
        Checks::Passing
    } else {
        Checks::None
    }
}

// ---------------------------------------------------------------------------
// The status the app keeps and answers with
// ---------------------------------------------------------------------------

/// The `pr.status` answer's `pr` (docs/API.md) plus what the prompt needs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PrStatus {
    pub number: u64,
    pub url: String,
    pub title: String,
    pub state: PrState,
    pub draft: bool,
    pub checks: Checks,
    /// Names of the failing checks.
    pub failing: Vec<String>,
    pub review: Option<Review>,
    /// The checks ran on another commit than the Worktree's `HEAD`.
    pub stale: bool,
    /// The PR's head commit (`headRefOid`).
    pub head: String,
    /// Unix seconds of the read.
    pub checked_at: Option<u64>,
    /// The failing checks, for `ci_log`.
    #[serde(skip)]
    pub failed: Vec<Check>,
    /// The Worktree's `HEAD` the status was compared with.
    #[serde(skip)]
    pub local_head: String,
}

impl PrStatus {
    /// `local_head` empty (unknown) is never stale.
    pub fn new(pr: &Pr, local_head: &str, checked_at: Option<u64>) -> Self {
        let failed: Vec<Check> = pr
            .checks
            .iter()
            .filter(|c| c.outcome == Checks::Failing)
            .cloned()
            .collect();
        PrStatus {
            number: pr.number,
            url: pr.url.clone(),
            title: pr.title.clone(),
            state: pr.state,
            draft: pr.draft,
            checks: rollup(&pr.checks),
            failing: failed.iter().map(|c| c.name.clone()).collect(),
            review: pr.review,
            stale: !local_head.is_empty() && !pr.head.is_empty() && pr.head != local_head,
            head: pr.head.clone(),
            checked_at,
            failed,
            local_head: local_head.to_string(),
        }
    }
}

/// parse + choose + status in one step; `None` = no PR for the branch.
pub fn status_from_json(
    json: &str,
    local_head: &str,
    checked_at: Option<u64>,
) -> Result<Option<PrStatus>> {
    let prs = parse(json)?;
    Ok(choose(&prs).map(|p| PrStatus::new(p, local_head, checked_at)))
}

// ---------------------------------------------------------------------------
// Texts (R146.3, R148.1)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Error,
    Warn,
    Fg3,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chip {
    pub text: String,
    pub tone: Tone,
}

/// The chip after a Worktree's name.
pub fn chip(s: &PrStatus) -> Chip {
    let draft = if s.draft && s.state == PrState::Open {
        "draft "
    } else {
        ""
    };
    let n = s.number;
    let (suffix, tone) = match s.state {
        PrState::Merged => (" merged", Tone::Fg3),
        PrState::Closed => (" closed", Tone::Fg3),
        PrState::Open => match s.checks {
            Checks::Passing => (" \u{2713}", Tone::Ok),
            Checks::Failing => (" \u{2717}", Tone::Error),
            Checks::Pending => (" \u{25cf}", Tone::Warn),
            Checks::None => ("", Tone::Fg3),
        },
    };
    Chip {
        text: format!("{draft}#{n}{suffix}"),
        tone,
    }
}

/// The chip of a recorded PR before the first read (R146.5): `fg3`.
pub fn chip_recorded(number: u64) -> Chip {
    Chip {
        text: format!("#{number}"),
        tone: Tone::Fg3,
    }
}

fn state_word(s: &PrStatus) -> String {
    let w = match s.state {
        PrState::Open => "open",
        PrState::Merged => "merged",
        PrState::Closed => "closed",
    };
    if s.draft && s.state == PrState::Open {
        format!("{w} (draft)")
    } else {
        w.to_string()
    }
}

fn checks_text(s: &PrStatus) -> String {
    match s.checks {
        Checks::Failing if !s.failing.is_empty() => {
            format!("checks failing ({})", s.failing.join(", "))
        }
        Checks::Failing => "checks failing".into(),
        Checks::Passing => "checks passing".into(),
        Checks::Pending => "checks pending".into(),
        Checks::None => "no checks".into(),
    }
}

/// The chip's tooltip: title, state, review decision, a stale note.
pub fn tooltip(s: &PrStatus) -> String {
    let mut lines = vec![
        format!("#{} {}", s.number, s.title),
        format!("{} \u{b7} {}", state_word(s), checks_text(s)),
    ];
    if let Some(r) = s.review {
        lines.push(format!("review: {}", r.text()));
    }
    if s.stale {
        lines.push("checks for an older push".into());
    }
    lines.join("\n")
}

/// `#482 open · checks failing (test, lint) · changes requested` (R148.1).
pub fn cli_line(s: &PrStatus) -> String {
    let mut parts = vec![format!("#{} {}", s.number, state_word(s)), checks_text(s)];
    if let Some(r) = s.review {
        parts.push(r.text().to_string());
    }
    if s.stale {
        parts.push("checks for an older push".into());
    }
    parts.join(" \u{b7} ")
}

pub fn cli_line_none(branch: &str) -> String {
    format!("no pull request for {branch}")
}

/// Project home's 4th KPI (R146.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kpi {
    /// Number of open PRs.
    pub value: String,
    pub sub: String,
}

/// `statuses`: the chosen PR of each Worktree of the Project (None = no PR).
/// One open PR → `#482 checks passing|failing|pending`; several →
/// `<k> failing`, else `<k> pending`, else `<k> passing`; none open → "".
pub fn kpi<'a>(statuses: impl IntoIterator<Item = Option<&'a PrStatus>>) -> Kpi {
    let open: Vec<&PrStatus> = statuses
        .into_iter()
        .flatten()
        .filter(|s| s.state == PrState::Open)
        .collect();
    let count = |c| open.iter().filter(|s| s.checks == c).count();
    let sub = match open.len() {
        0 => String::new(),
        1 => {
            let s = open[0];
            let c = match s.checks {
                Checks::Failing => "checks failing".to_string(),
                _ => checks_text(s),
            };
            format!("#{} {c}", s.number)
        }
        _ => {
            let (f, p) = (count(Checks::Failing), count(Checks::Pending));
            if f > 0 {
                format!("{f} failing")
            } else if p > 0 {
                format!("{p} pending")
            } else {
                format!("{} passing", count(Checks::Passing))
            }
        }
    };
    Kpi {
        value: open.len().to_string(),
        sub,
    }
}

/// `PR #482 ✓` in the Worktree meta of Project home.
pub fn meta(s: &PrStatus) -> String {
    format!("PR {}", chip(s).text)
}

// ---------------------------------------------------------------------------
// Schedule (R146.2)
// ---------------------------------------------------------------------------

/// What the schedule needs to know about one Worktree.
#[derive(Clone, Copy, Debug, Default)]
pub struct Candidate {
    /// Its branch has an upstream, or a PR is recorded.
    pub eligible: bool,
    /// Its Workspace is shown and the window is active.
    pub shown: bool,
    /// Its PR's checks are pending (last read).
    pub pending: bool,
    pub last_read: Option<Duration>,
    /// Start, a Push / Publish / Sync / Create PR, a `HEAD` change,
    /// `--refresh`.
    pub trigger: bool,
}

/// When the Worktree is next due; `None` = never (not eligible).
pub fn due_at(c: &Candidate, now: Duration) -> Option<Duration> {
    if !c.eligible {
        return None;
    }
    if c.trigger {
        return Some(now);
    }
    let Some(last) = c.last_read else {
        return Some(now);
    };
    let every = match (c.shown, c.pending) {
        (true, true) => PENDING_EVERY,
        (true, false) => ACTIVE_EVERY,
        (false, _) => IDLE_EVERY,
    };
    Some(last + every)
}

/// The most overdue due Worktree (ties: first).
pub fn next_due<K: Clone>(items: &[(K, Candidate)], now: Duration) -> Option<K> {
    items
        .iter()
        .filter_map(|(k, c)| due_at(c, now).filter(|t| *t <= now).map(|t| (t, k)))
        .min_by_key(|(t, _)| *t)
        .map(|(_, k)| k.clone())
}

/// A Workspace's `gh` state: one call in flight, and 10 min of silence after
/// a failure.
#[derive(Clone, Debug, Default)]
pub struct Gate {
    in_flight: bool,
    down_until: Option<Duration>,
    message: Option<String>,
}

impl Gate {
    pub fn is_down(&self, now: Duration) -> bool {
        self.down_until.is_some_and(|t| now < t)
    }
    /// gh's first message line while down (`unavailable`).
    pub fn message(&self, now: Duration) -> Option<&str> {
        self.message.as_deref().filter(|_| self.is_down(now))
    }
    pub fn can_start(&self, now: Duration) -> bool {
        !self.in_flight && !self.is_down(now)
    }
    /// Mark a call started; false (and nothing changes) when it may not.
    pub fn start(&mut self, now: Duration) -> bool {
        if !self.can_start(now) {
            return false;
        }
        self.in_flight = true;
        true
    }
    pub fn finish_ok(&mut self) {
        self.in_flight = false;
        self.down_until = None;
        self.message = None;
    }
    /// A failed, timed-out or missing gh: down for [`DOWN_FOR`]. True when
    /// the Workspace was up before: the caller adds the one Activity row.
    pub fn finish_err(&mut self, now: Duration, message: &str) -> bool {
        let was_up = !self.is_down(now);
        self.in_flight = false;
        self.down_until = Some(now + DOWN_FOR);
        self.message = Some(message.lines().next().unwrap_or("").trim().to_string());
        was_up
    }
}

// ---------------------------------------------------------------------------
// Transitions (R146.4)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    ChecksFailed {
        number: u64,
        head: String,
        jobs: Vec<String>,
    },
    ChecksPassed {
        number: u64,
    },
    Merged {
        number: u64,
    },
    ChangesRequested {
        number: u64,
    },
}

impl Event {
    /// The Activity row.
    pub fn activity(&self) -> String {
        match self {
            Event::ChecksFailed { number, jobs, .. } => {
                let n = jobs.len();
                format!(
                    "#{number} checks failed ({n} job{})",
                    if n == 1 { "" } else { "s" }
                )
            }
            Event::ChecksPassed { number } => format!("#{number} checks passed"),
            Event::Merged { number } => format!("#{number} merged"),
            Event::ChangesRequested { number } => format!("#{number} changes requested"),
        }
    }
    /// (title, body) of the Notification on the Worktree's Agent Pane (else
    /// its first Pane); `ChecksFailed` carries the action Send to Agent.
    pub fn notification(&self) -> Option<(String, String)> {
        match self {
            Event::ChecksFailed { number, jobs, .. } => {
                Some((format!("#{number} checks failed"), jobs.join(", ")))
            }
            Event::ChangesRequested { number } => Some((
                format!("#{number} changes requested"),
                "A reviewer asked for changes".into(),
            )),
            _ => None,
        }
    }
    /// The Toast (with Remove Worktree…).
    pub fn toast(&self) -> Option<String> {
        match self {
            Event::Merged { number } => Some(format!("#{number} merged")),
            _ => None,
        }
    }
    pub fn offers_send_to_agent(&self) -> bool {
        matches!(self, Event::ChecksFailed { .. })
    }
}

struct Seen {
    number: u64,
    checks: Checks,
    state: PrState,
    review: Option<Review>,
    failed_head: Option<String>,
}

/// Compares each read with the last one of this run. The first read of a
/// Worktree (or of a new PR number) is silent.
#[derive(Default)]
pub struct Tracker {
    last: HashMap<String, Seen>,
}

impl Tracker {
    /// `key` identifies the Worktree. `None` = no PR (forgets it).
    pub fn feed(&mut self, key: &str, s: Option<&PrStatus>) -> Vec<Event> {
        let Some(s) = s else {
            self.last.remove(key);
            return Vec::new();
        };
        let failed_now = s.checks == Checks::Failing;
        let mut seen = match self.last.remove(key) {
            Some(p) if p.number == s.number => p,
            _ => {
                self.last.insert(
                    key.to_string(),
                    Seen {
                        number: s.number,
                        checks: s.checks,
                        state: s.state,
                        review: s.review,
                        failed_head: failed_now.then(|| s.head.clone()),
                    },
                );
                return Vec::new();
            }
        };
        let mut ev = Vec::new();
        if failed_now && seen.failed_head.as_deref() != Some(s.head.as_str()) {
            seen.failed_head = Some(s.head.clone());
            ev.push(Event::ChecksFailed {
                number: s.number,
                head: s.head.clone(),
                jobs: s.failing.clone(),
            });
        }
        if s.checks == Checks::Passing && seen.checks != Checks::Passing {
            ev.push(Event::ChecksPassed { number: s.number });
        }
        if s.state == PrState::Merged && seen.state != PrState::Merged {
            ev.push(Event::Merged { number: s.number });
        }
        if s.review == Some(Review::ChangesRequested)
            && seen.review != Some(Review::ChangesRequested)
        {
            ev.push(Event::ChangesRequested { number: s.number });
        }
        seen.checks = s.checks;
        seen.state = s.state;
        seen.review = s.review;
        self.last.insert(key.to_string(), seen);
        ev
    }
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

/// Run `gh <args>` in `dir` with the Workspace env, off the UI thread.
/// Missing gh → a usage error with the install hint; over `limit` →
/// `Timeout`; a non-zero exit → gh's first stderr line.
pub fn run_gh(dir: &Path, gh: &str, args: &[String], env: &Env, limit: Duration) -> Result<String> {
    let out = proc::output(
        Command::new(gh)
            .args(args)
            .current_dir(dir)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .env("GH_PROMPT_DISABLED", "1")
            .env("NO_COLOR", "1"),
        limit,
    )
    .map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Error::usage(
            "GitHub CLI (gh) is not installed: see https://cli.github.com, then `gh auth login`",
        ),
        std::io::ErrorKind::TimedOut => Error::new(
            ErrorKind::Timeout,
            format!(
                "gh {} did not finish within {} s",
                args.iter().take(2).cloned().collect::<Vec<_>>().join(" "),
                limit.as_secs()
            ),
        ),
        _ => Error::usage(format!("cannot run gh: {e}")),
    })?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let text = String::from_utf8_lossy(&out.stdout);
        let first = |s: &str| {
            s.lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .map(str::to_string)
        };
        let msg = first(&err)
            .or_else(|| first(&text))
            .unwrap_or_else(|| format!("gh exited with {}", out.status));
        return Err(Error::usage(msg));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The PR list of `branch` (R146.1), 20 s limit.
pub fn read(dir: &Path, gh: &str, branch: &str, env: &Env) -> Result<Vec<Pr>> {
    parse(&run_gh(dir, gh, &gh_list_args(branch), env, READ_LIMIT)?)
}

#[cfg(test)]
#[path = "pr_tests.rs"]
mod tests;
