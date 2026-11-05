//! CI failures back to the Agent (PRD R147, ADR-0053): which `gh run view`
//! reads a failing PR needs, cleaning and cutting the logs, the prompt Send
//! to Agent offers (also `rade pr failures`), the `pr.failures` answer, and
//! the opt-in auto-send rule (`[scm] ci_failures = "send"`). GitHub Actions
//! jobs only; any other check is named with its URL.

use crate::pr::{run_gh, Check, Checks, PrStatus};
use crate::rpc::Result;
use crate::scm::Env;
use serde::Serialize;
use std::path::Path;
use std::time::{Duration, Instant};

/// One `gh run view --log-failed` call's limit (R147.1).
pub const LOG_LIMIT: Duration = Duration::from_secs(30);
/// All the `gh run view` calls of one answer together (R148.3): later jobs
/// are named with their URL.
pub const TOTAL_LOG_LIMIT: Duration = Duration::from_secs(90);
/// Lines kept per job.
pub const MAX_LINES: usize = 150;
/// Jobs in a prompt.
pub const MAX_JOBS: usize = 6;
/// All logs together.
pub const MAX_BYTES: usize = 24 * 1024;
/// Below this many bytes left a job gets no log.
const MIN_LOG: usize = 64;
/// Failing heads sent in a row without a passing read (R147.4).
pub const MAX_SENDS: u32 = 3;

pub const CLOSING: &str =
    "Find the cause, fix it, run the failing step locally if you can, then commit and push.";

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

/// `(run, job)` of `https://github.com/<o>/<r>/actions/runs/<run>/job/<job>`;
/// a run URL without a job, or anything else, is `None`.
pub fn actions_job(details_url: &str) -> Option<(String, String)> {
    let rest = details_url.trim().strip_prefix("https://github.com/")?;
    let rest = rest.split(['?', '#']).next()?;
    let p: Vec<&str> = rest.split('/').collect();
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    match p.as_slice() {
        [o, r, "actions", "runs", run, "job", job]
            if !o.is_empty() && !r.is_empty() && digits(run) && digits(job) =>
        {
            Some((run.to_string(), job.to_string()))
        }
        _ => None,
    }
}

/// `gh run view <run> --job <job> --log-failed`.
pub fn log_args(run: &str, job: &str) -> Vec<String> {
    ["run", "view", run, "--job", job, "--log-failed"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// The gh argv for a failed check's log, `None` = name + URL only.
pub fn plan(check: &Check) -> Option<Vec<String>> {
    actions_job(&check.url).map(|(run, job)| log_args(&run, &job))
}

// ---------------------------------------------------------------------------
// Cleaning and cutting
// ---------------------------------------------------------------------------

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match it.peek() {
            Some('[') => {
                it.next();
                for n in it.by_ref() {
                    if ('@'..='~').contains(&n) {
                        break;
                    }
                }
            }
            Some(']') => {
                it.next();
                while let Some(n) = it.next() {
                    if n == '\u{7}' {
                        break;
                    }
                    if n == '\u{1b}' {
                        it.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// `2026-11-05T10:00:00.1234567Z ` at the start of a line.
fn strip_timestamp(s: &str) -> &str {
    let b = s.as_bytes();
    let shape = b"dddd-dd-ddTdd:dd:dd";
    if b.len() < shape.len() {
        return s;
    }
    for (c, k) in b.iter().zip(shape) {
        let ok = if *k == b'd' {
            c.is_ascii_digit()
        } else {
            c == k
        };
        if !ok {
            return s;
        }
    }
    let mut i = shape.len();
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
        i += 1;
    }
    if b.get(i) != Some(&b'Z') {
        return s;
    }
    s[i + 1..].strip_prefix(' ').unwrap_or(&s[i + 1..])
}

/// A `gh run view --log-failed` log without its `<job>\t<step>\t` fields,
/// timestamps, ANSI codes and `##[group]` / `##[endgroup]` lines; an
/// `##[error]` line keeps its text. Trailing blank lines are dropped.
pub fn clean(log: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for raw in log.lines() {
        let mut line = raw.trim_start_matches('\u{feff}');
        if line.matches('\t').count() >= 2 {
            line = line.splitn(3, '\t').nth(2).unwrap_or("");
        }
        let line = strip_ansi(line);
        let line = strip_timestamp(&line);
        if line.starts_with("##[group]") || line.starts_with("##[endgroup]") {
            continue;
        }
        let line = line.strip_prefix("##[error]").unwrap_or(line);
        out.push(line.trim_end().to_string());
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out.join("\n")
}

/// The last `max` lines; a cut line count is stated first.
pub fn tail_lines(text: &str, max: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max {
        return text.to_string();
    }
    let cut = lines.len() - max;
    format!("(first {cut} lines cut)\n{}", lines[cut..].join("\n"))
}

/// Keep the end of `text` within `max` bytes, on a char (and line) boundary,
/// the cut stated first. `text` within `max` is returned as is.
pub fn cut_to_bytes(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let note = format!("(cut to {})", size_text(max));
    let room = max.saturating_sub(note.len() + 1);
    let mut start = text.len() - room.min(text.len());
    while !text.is_char_boundary(start) {
        start += 1;
    }
    // Begin on a whole line when one fits.
    if let Some(nl) = text[start..].find('\n') {
        if start > 0 && !text[..start].ends_with('\n') {
            start += nl + 1;
        }
    }
    format!("{note}\n{}", &text[start..])
}

/// `24 KB`, `9.8 KB`, `40 bytes`.
pub fn size_text(bytes: usize) -> String {
    if bytes % 1024 == 0 && bytes >= 1024 {
        format!("{} KB", bytes / 1024)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} bytes")
    }
}

/// How long the next `gh run view` may take when `spent` of
/// [`TOTAL_LOG_LIMIT`] is used; `None` = no time left, the job is only named.
pub fn log_limit(spent: Duration) -> Option<Duration> {
    let left = TOTAL_LOG_LIMIT.checked_sub(spent)?;
    (!left.is_zero()).then(|| left.min(LOG_LIMIT))
}

// ---------------------------------------------------------------------------
// Jobs, prompt, answer
// ---------------------------------------------------------------------------

/// One failed check in the `pr.failures` answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Job {
    pub workflow: String,
    pub job: String,
    pub conclusion: String,
    pub url: String,
    /// The cleaned, cut tail; `None` for a check without a readable log.
    pub log: Option<String>,
}

impl Job {
    pub fn heading(&self) -> String {
        if self.workflow.is_empty() {
            self.job.clone()
        } else {
            format!("{} / {}", self.workflow, self.job)
        }
    }
}

/// The `pr.failures` answer (the app adds `worktree`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Failures {
    pub number: u64,
    pub head: String,
    pub stale: bool,
    pub jobs: Vec<Job>,
    /// `None` when nothing is failing.
    pub prompt: Option<String>,
}

fn sha7(s: &str) -> String {
    s.chars().take(7).collect()
}

fn fence_for(text: &str) -> String {
    let mut run = 0usize;
    let mut best = 0usize;
    for c in text.chars() {
        if c == '`' {
            run += 1;
            best = best.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat(best.max(2) + 1)
}

/// R147.2. `omitted`: headings of failed jobs beyond [`MAX_JOBS`].
pub fn prompt(status: &PrStatus, jobs: &[Job], omitted: &[String]) -> String {
    prompt_with(status, jobs, omitted, &[])
}

/// [`prompt`] with `notes` (a log left out for time or size) before the
/// closing line.
pub fn prompt_with(
    status: &PrStatus,
    jobs: &[Job],
    omitted: &[String],
    notes: &[String],
) -> String {
    let mut out = String::new();
    if status.stale {
        out.push_str(&format!(
            "These checks ran on an older push ({}); your branch is at {}.\n\n",
            sha7(&status.head),
            sha7(&status.local_head)
        ));
    }
    out.push_str(&format!(
        "CI failed on PR #{} \"{}\" at {}.\n",
        status.number,
        status.title,
        sha7(&status.head)
    ));
    for j in jobs {
        out.push_str(&format!("\n## {}\n", j.heading()));
        match j.log.as_deref().filter(|l| !l.is_empty()) {
            Some(log) => {
                let f = fence_for(log);
                out.push_str(&format!("{f}\n{log}\n{f}\n"));
            }
            None => out.push_str(&format!("{}: {}\n", j.conclusion, j.url)),
        }
    }
    if !omitted.is_empty() {
        let n = omitted.len();
        out.push_str(&format!(
            "\n({n} more failed job{} not shown: {})\n",
            if n == 1 { "" } else { "s" },
            omitted.join(", ")
        ));
    }
    for n in notes {
        out.push_str(&format!("\n{n}\n"));
    }
    out.push_str(&format!("\n{CLOSING}\n"));
    out
}

/// The pure part of `fetch_failures`: `raw_logs` are gh's outputs aligned
/// with `status.failed` (`None` = not an Actions job, or unreadable).
pub fn failures(status: &PrStatus, raw_logs: &[Option<String>]) -> Failures {
    failures_with(status, raw_logs, &[])
}

/// [`failures`]; `out_of_time[i]` = the log of `status.failed[i]` was not
/// asked for because [`TOTAL_LOG_LIMIT`] was used up.
pub fn failures_with(
    status: &PrStatus,
    raw_logs: &[Option<String>],
    out_of_time: &[bool],
) -> Failures {
    let mut late: Vec<String> = Vec::new();
    let mut no_room: Vec<String> = Vec::new();
    let mut jobs: Vec<Job> = Vec::new();
    let mut omitted: Vec<String> = Vec::new();
    let mut used = 0usize;
    for (i, c) in status.failed.iter().enumerate() {
        let mut job = Job {
            workflow: c.workflow.clone(),
            job: c.name.clone(),
            conclusion: c.conclusion.clone(),
            url: c.url.clone(),
            log: None,
        };
        if jobs.len() >= MAX_JOBS {
            omitted.push(job.heading());
            continue;
        }
        let cleaned = raw_logs
            .get(i)
            .and_then(|l| l.as_deref())
            .map(clean)
            .filter(|l| !l.is_empty());
        if out_of_time.get(i).copied().unwrap_or(false) {
            late.push(job.heading());
        } else if let Some(text) = cleaned {
            let text = tail_lines(&text, MAX_LINES);
            let left = MAX_BYTES.saturating_sub(used);
            // Too little room left for a useful tail: name + URL only.
            if left >= MIN_LOG {
                let text = cut_to_bytes(&text, left);
                used += text.len();
                job.log = Some(text);
            } else {
                no_room.push(job.heading());
            }
        }
        jobs.push(job);
    }
    let mut notes: Vec<String> = Vec::new();
    let many = |n: usize| if n == 1 { "" } else { "s" };
    if !late.is_empty() {
        notes.push(format!(
            "(log{} not read, the {} s limit for reading logs was reached, see the URL{}: {})",
            many(late.len()),
            TOTAL_LOG_LIMIT.as_secs(),
            many(late.len()),
            late.join(", ")
        ));
    }
    if !no_room.is_empty() {
        notes.push(format!(
            "(log{} left out, the {} limit for all logs was reached: {})",
            many(no_room.len()),
            size_text(MAX_BYTES),
            no_room.join(", ")
        ));
    }
    let prompt = (!jobs.is_empty()).then(|| prompt_with(status, &jobs, &omitted, &notes));
    Failures {
        number: status.number,
        head: status.head.clone(),
        stale: status.stale,
        jobs,
        prompt,
    }
}

/// Run `gh run view … --log-failed` per failed Actions job (30 s each, 90 s in all) and
/// build the answer. A log that can't be read falls back to name + URL.
/// Not failing → no jobs, no prompt.
pub fn fetch_failures(dir: &Path, gh: &str, status: &PrStatus, env: &Env) -> Result<Failures> {
    if status.checks != Checks::Failing {
        return Ok(failures(
            &PrStatus {
                failed: Vec::new(),
                ..status.clone()
            },
            &[],
        ));
    }
    let mut raw: Vec<Option<String>> = Vec::new();
    let mut late: Vec<bool> = Vec::new();
    let started = Instant::now();
    for (i, c) in status.failed.iter().enumerate() {
        // Jobs past the limits are only named.
        let mut out_of_time = false;
        let log = match plan(c) {
            Some(args) if i < MAX_JOBS => match log_limit(started.elapsed()) {
                Some(limit) => run_gh(dir, gh, &args, env, limit).ok(),
                None => {
                    out_of_time = true;
                    None
                }
            },
            _ => None,
        };
        raw.push(log);
        late.push(out_of_time);
    }
    Ok(failures_with(status, &raw, &late))
}

// ---------------------------------------------------------------------------
// Auto-send (R147.4)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Ask,
    Send,
}

impl Mode {
    /// `[scm] ci_failures`; anything but `send` is `ask`.
    pub fn parse(s: &str) -> Mode {
        if s.trim() == "send" {
            Mode::Send
        } else {
            Mode::Ask
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    Running,
    Waiting,
    Done,
}

/// The PR's last read, as far as auto-send cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Read<'a> {
    Failing {
        head: &'a str,
    },
    Passing,
    /// Pending, none, merged, no PR: nothing changes.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Send the prompt now (the caller then records an Activity row).
    Send,
    /// A failing head waits for the Agent to stop running: ask again later.
    Wait,
    /// The 3rd send did not help: the Notification `CI still failing on
    /// #<n> after 3 tries`, once.
    AskStillFailing,
    Nothing,
}

/// The notice text for [`Action::AskStillFailing`].
pub fn still_failing(number: u64) -> String {
    format!("CI still failing on #{number} after {MAX_SENDS} tries")
}

/// Per-PR state: feed it every read (and again when the Agent's state
/// changes while an action was `Wait`).
#[derive(Clone, Debug, Default)]
pub struct AutoSend {
    sent_head: Option<String>,
    in_a_row: u32,
    noticed: bool,
}

impl AutoSend {
    /// The state kept on the Worktree's `PrRef` (R147.4): what a relaunch
    /// starts from, so a head already sent is not sent again.
    pub fn restore(sent_head: Option<String>, in_a_row: u32) -> Self {
        AutoSend {
            sent_head,
            in_a_row,
            noticed: false,
        }
    }

    pub fn sent_head(&self) -> Option<&str> {
        self.sent_head.as_deref()
    }

    pub fn in_a_row(&self) -> u32 {
        self.in_a_row
    }

    /// `agents`: how many Agents the Worktree has; `agent`: the state of the
    /// one when there is exactly one.
    pub fn step(
        &mut self,
        mode: Mode,
        read: Read,
        agents: usize,
        agent: Option<AgentState>,
    ) -> Action {
        let head = match read {
            Read::Passing => {
                self.in_a_row = 0;
                self.noticed = false;
                return Action::Nothing;
            }
            Read::Other => return Action::Nothing,
            Read::Failing { head } => head,
        };
        // `ask`, and none or several Agents, never send.
        if mode != Mode::Send || agents != 1 {
            return Action::Nothing;
        }
        if self.sent_head.as_deref() == Some(head) {
            return Action::Nothing;
        }
        if self.in_a_row >= MAX_SENDS {
            if self.noticed {
                return Action::Nothing;
            }
            self.noticed = true;
            return Action::AskStillFailing;
        }
        match agent {
            Some(AgentState::Waiting | AgentState::Done) => {
                self.sent_head = Some(head.to_string());
                self.in_a_row += 1;
                Action::Send
            }
            _ => Action::Wait,
        }
    }
}

#[cfg(test)]
#[path = "ci_log_tests.rs"]
mod tests;
