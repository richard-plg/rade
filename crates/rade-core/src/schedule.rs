//! Schedules (PRD R136–R137, ADR-0051): a per-Workspace rule that starts an
//! Agent with a prompt on a cron expression or once at a given time, in a
//! Project link's checkout or a folder (Scratch). Here: the saved model, the
//! 5-field cron in local time (DST included), what is due at a check, the
//! Runs history and validation. The app fires the runs (`rade-app`
//! `schedules.rs`).

use crate::agents::AgentStatus;
use crate::rpc::{Error, Result};
use jiff::civil::{Date, DateTime, Time};
use jiff::tz::{AmbiguousOffset, TimeZone};
use jiff::Timestamp;
use serde::{Deserialize, Deserializer, Serialize};
use std::path::{Path, PathBuf};

/// Runs kept per Schedule (R137.4).
pub const MAX_RUNS: usize = 50;
/// A due time older than this at a check is a miss (R136.4).
pub const LATE_SECS: i64 = 120;
/// How far `next_after` looks ahead (`0 0 29 2 mon` may be years away).
const LOOK_DAYS: i64 = 366 * 8;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CatchUp {
    #[default]
    Skip,
    Once,
}

impl CatchUp {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "skip" => Ok(Self::Skip),
            "once" => Ok(Self::Once),
            o => Err(Error::usage(format!(
                "catch-up must be skip|once, not '{o}'"
            ))),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Started,
    Skipped,
    Missed,
    Failed,
}

/// One firing of a Schedule (R137.4).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Run {
    pub n: u32,
    pub at: i64,
    pub outcome: Outcome,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missed: Option<u32>,
    #[serde(default)]
    pub pane: Option<String>,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub status: Option<AgentStatus>,
    /// The Pane a resume opened (R137.6).
    #[serde(default)]
    pub resumed: Option<String>,
}

impl Run {
    pub fn new(n: u32, at: i64, outcome: Outcome, reason: Option<String>) -> Self {
        Self {
            n,
            at,
            outcome,
            reason,
            missed: None,
            pane: None,
            session: None,
            account: None,
            status: None,
            resumed: None,
        }
    }
}

/// A Schedule as saved in `state.json` (R136.1). Exactly one of
/// `project` / `dir` and one of `cron` / `at` is set.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Schedule {
    pub name: String,
    pub workspace: String,
    #[serde(default)]
    pub project: Option<String>,
    /// The link's Source id, resolved at create / edit.
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub dir: Option<PathBuf>,
    #[serde(default)]
    pub cron: Option<String>,
    /// Unix seconds.
    #[serde(default)]
    pub at: Option<i64>,
    pub agent: String,
    /// None = the Workspace's default for the agent at run time.
    #[serde(default)]
    pub account: Option<String>,
    pub prompt: String,
    pub enabled: bool,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub catch_up: CatchUp,
    pub keep_runs: u8,
    pub created: i64,
    /// The end of the last check (R136.4).
    #[serde(default)]
    pub checked: i64,
    /// The last Run number given.
    #[serde(default)]
    pub last_n: u32,
    /// Newest last, at most [`MAX_RUNS`].
    #[serde(default)]
    pub runs: Vec<Run>,
}

/// What a check finds (R136.4).
#[derive(Debug, PartialEq, Eq)]
pub enum Due {
    Nothing,
    /// Start a run now; `missed` > 0 = a catch-up run for that many times.
    Start {
        at: i64,
        missed: u32,
    },
    /// Record one `missed` Run for `count` fire times.
    Missed {
        count: u32,
        latest: i64,
    },
}

impl Schedule {
    pub fn next_after(&self, after: i64, tz: &TimeZone) -> Option<i64> {
        if let Some(at) = self.at {
            return (at > after).then_some(at);
        }
        let cron = Cron::parse(self.cron.as_deref()?).ok()?;
        cron.next_after(after, tz)
    }

    /// The next fire time from `now`, or None when off or done.
    pub fn next(&self, now: i64, tz: &TimeZone) -> Option<i64> {
        if !self.enabled || self.done {
            return None;
        }
        self.next_after(now.max(self.checked), tz)
    }

    /// The next `count` fire times after `now`.
    pub fn next_times(&self, now: i64, count: usize, tz: &TimeZone) -> Vec<i64> {
        let mut out = Vec::new();
        let mut t = now;
        while out.len() < count {
            match self.next_after(t, tz) {
                Some(n) => {
                    out.push(n);
                    t = n;
                }
                None => break,
            }
        }
        out
    }

    /// The fire times in `(checked, now]` (R136.4); does not move `checked`.
    pub fn due(&self, now: i64, tz: &TimeZone) -> Due {
        if !self.enabled || self.done || now <= self.checked {
            return Due::Nothing;
        }
        let (mut count, mut latest, mut t) = (0u32, 0, self.checked);
        while let Some(n) = self.next_after(t, tz) {
            if n > now {
                break;
            }
            count += 1;
            latest = n;
            t = n;
            if count >= 100_000 {
                break;
            }
        }
        match count {
            0 => Due::Nothing,
            1 if now - latest <= LATE_SECS => Due::Start {
                at: latest,
                missed: 0,
            },
            _ if self.catch_up == CatchUp::Once => Due::Start {
                at: now,
                missed: count,
            },
            _ => Due::Missed { count, latest },
        }
    }

    /// The next Run number.
    pub fn next_n(&mut self) -> u32 {
        self.last_n += 1;
        self.last_n
    }

    /// Record a Run: keep the newest [`MAX_RUNS`]; an `at` Schedule is then
    /// off and done (R136.3).
    pub fn record(&mut self, run: Run) {
        self.runs.push(run);
        let extra = self.runs.len().saturating_sub(MAX_RUNS);
        self.runs.drain(..extra);
        if self.at.is_some() {
            self.done = true;
            self.enabled = false;
        }
    }

    pub fn run(&self, n: u32) -> Option<&Run> {
        self.runs.iter().find(|r| r.n == n)
    }

    pub fn run_mut(&mut self, n: u32) -> Option<&mut Run> {
        self.runs.iter_mut().find(|r| r.n == n)
    }

    /// The newest started Run.
    pub fn last_started(&self) -> Option<&Run> {
        self.runs
            .iter()
            .rev()
            .find(|r| r.outcome == Outcome::Started)
    }

    /// The newest Run's Pane still open and busy (R137.3).
    pub fn busy_pane(
        &self,
        status: impl Fn(&str) -> Option<Option<AgentStatus>>,
    ) -> Option<String> {
        let r = self.last_started()?;
        let pane = r.pane.as_deref()?;
        status(pane).filter(|s| busy(*s)).map(|_| pane.to_string())
    }

    /// Run Panes to close after a run starts (R137.4): the started Runs'
    /// open Panes newest first, past `keep_runs`, that are finished.
    /// `status` = None for a closed Pane, else its Agent Status.
    pub fn panes_to_close(
        &self,
        status: impl Fn(&str) -> Option<Option<AgentStatus>>,
    ) -> Vec<String> {
        self.runs
            .iter()
            .rev()
            .filter_map(|r| r.pane.as_deref())
            .filter_map(|p| status(p).map(|s| (p, s)))
            .skip(self.keep_runs as usize)
            .filter(|(_, s)| !busy(*s))
            .map(|(p, _)| p.to_string())
            .collect()
    }

    /// `{"project","link"}` or the folder.
    pub fn in_scratch(&self) -> bool {
        self.dir.is_some()
    }

    pub fn when_text(&self, now: i64, tz: &TimeZone) -> String {
        match (&self.cron, self.at) {
            (Some(c), _) => cron_text(c),
            (None, Some(at)) => format!("Once, {}", fmt_time(at, now, tz)),
            _ => String::new(),
        }
    }

    /// The schedule object of API.md.
    pub fn to_json(&self, now: i64, tz: &TimeZone, account_now: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "name": self.name, "workspace": self.workspace,
            "project": self.project, "link": self.link, "dir": self.dir,
            "cron": self.cron, "at": self.at, "when_text": self.when_text(now, tz),
            "agent": self.agent, "account": self.account, "account_now": account_now,
            "prompt": self.prompt, "enabled": self.enabled, "done": self.done,
            "catch_up": self.catch_up, "keep_runs": self.keep_runs, "created": self.created,
            "next": self.next(now, tz),
            "last": self.runs.last().map(|r| serde_json::json!({
                "n": r.n, "at": r.at, "outcome": r.outcome,
            })),
        })
    }
}

/// `running`, `waiting` and `interrupted` Agents are not finished (R137.3).
pub fn busy(s: Option<AgentStatus>) -> bool {
    matches!(
        s,
        Some(AgentStatus::Running | AgentStatus::Waiting | AgentStatus::Interrupted)
    )
}

/// A parsed 5-field cron expression (R136.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cron {
    minutes: u64,
    hours: u32,
    /// Bits 1–31.
    dom: u32,
    /// Bits 1–12.
    months: u16,
    /// Bits 0–6, Sunday = 0.
    dow: u8,
    dom_any: bool,
    dow_any: bool,
}

const FIELDS: [(&str, u32, u32); 5] = [
    ("minute", 0, 59),
    ("hour", 0, 23),
    ("day-of-month", 1, 31),
    ("month", 1, 12),
    ("day-of-week", 0, 7),
];
const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const DAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];

fn macro_expr(s: &str) -> Option<&'static str> {
    Some(match s {
        "@hourly" => "0 * * * *",
        "@daily" | "@midnight" => "0 0 * * *",
        "@weekly" => "0 0 * * 0",
        "@monthly" => "0 0 1 * *",
        "@yearly" | "@annually" => "0 0 1 1 *",
        _ => return None,
    })
}

impl Cron {
    /// Parse; the error names the field and its text.
    pub fn parse(expr: &str) -> std::result::Result<Self, String> {
        let expr = expr.trim();
        let text = if expr.starts_with('@') {
            macro_expr(&expr.to_ascii_lowercase())
                .ok_or_else(|| format!("unknown cron shorthand '{expr}' (@hourly, @daily, @weekly, @monthly, @yearly)"))?
                .to_string()
        } else {
            expr.to_ascii_lowercase()
        };
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.len() != 5 {
            return Err(format!(
                "cron needs 5 fields (minute hour day-of-month month day-of-week), got {} in '{expr}'",
                parts.len()
            ));
        }
        let mut bits = [0u64; 5];
        for (i, part) in parts.iter().enumerate() {
            bits[i] = field(part, i)
                .map_err(|e| format!("cron field {} '{}': {e}", FIELDS[i].0, part))?;
        }
        let mut dow = bits[4];
        if dow & (1 << 7) != 0 {
            dow = (dow | 1) & !(1 << 7);
        }
        Ok(Self {
            minutes: bits[0],
            hours: bits[1] as u32,
            dom: bits[2] as u32,
            months: bits[3] as u16,
            dow: dow as u8,
            dom_any: parts[2].starts_with('*'),
            dow_any: parts[4].starts_with('*'),
        })
    }

    fn day_matches(&self, d: Date) -> bool {
        if self.months & (1 << d.month()) == 0 {
            return false;
        }
        let dom = self.dom & (1 << d.day()) != 0;
        let dow = self.dow & (1 << d.weekday().to_sunday_zero_offset()) != 0;
        match (self.dom_any, self.dow_any) {
            (true, true) => true,
            (true, false) => dow,
            (false, true) => dom,
            (false, false) => dom || dow,
        }
    }

    /// The first fire time after `after` (Unix s) in `tz` (R136.2): a local
    /// time skipped by DST fires at the gap's end, a repeated one once (the
    /// earlier).
    pub fn next_after(&self, after: i64, tz: &TimeZone) -> Option<i64> {
        let ts = Timestamp::from_second(after).ok()?;
        let start = ts.to_zoned(tz.clone()).datetime();
        let start = start.date().at(start.hour(), start.minute(), 0, 0);
        let mut day = start.date();
        for _ in 0..LOOK_DAYS {
            if self.day_matches(day) {
                for h in 0..24i8 {
                    if self.hours & (1 << h) == 0 {
                        continue;
                    }
                    for m in 0..60i8 {
                        if self.minutes & (1 << m) == 0 {
                            continue;
                        }
                        let dt = day.to_datetime(Time::new(h, m, 0, 0).ok()?);
                        if dt < start {
                            continue;
                        }
                        let t = resolve(dt, tz)?;
                        if t > after {
                            return Some(t);
                        }
                    }
                }
            }
            day = day.tomorrow().ok()?;
        }
        None
    }
}

/// A civil local time → Unix s: a gap → the instant it ends, a fold → the
/// earlier.
fn resolve(dt: DateTime, tz: &TimeZone) -> Option<i64> {
    let amb = tz.to_ambiguous_timestamp(dt);
    match amb.offset() {
        AmbiguousOffset::Unambiguous { offset } => Some(offset.to_timestamp(dt).ok()?.as_second()),
        AmbiguousOffset::Fold { before, .. } => Some(before.to_timestamp(dt).ok()?.as_second()),
        AmbiguousOffset::Gap { after, .. } => {
            // Read with the later offset the time lies before the jump; the
            // next transition is the gap's end.
            let guess = after.to_timestamp(dt).ok()?;
            let t = tz.following(guess).next()?;
            Some(t.timestamp().as_second())
        }
    }
}

fn names(i: usize) -> &'static [&'static str] {
    match i {
        3 => &MONTHS,
        4 => &DAYS,
        _ => &[],
    }
}

fn value(s: &str, i: usize) -> std::result::Result<u32, String> {
    let (_, lo, hi) = FIELDS[i];
    if let Some(p) = names(i).iter().position(|n| *n == s) {
        return Ok(p as u32 + if i == 3 { 1 } else { 0 });
    }
    let v: u32 = s.parse().map_err(|_| format!("'{s}' is not a number"))?;
    if v < lo || v > hi {
        return Err(format!("{v} is out of range {lo}-{hi}"));
    }
    Ok(v)
}

fn field(text: &str, i: usize) -> std::result::Result<u64, String> {
    let (_, lo, hi) = FIELDS[i];
    let mut bits = 0u64;
    for item in text.split(',') {
        if item.is_empty() {
            return Err("empty list item".into());
        }
        let (range, step) = match item.split_once('/') {
            Some((r, s)) => {
                let step: u32 = s
                    .parse()
                    .map_err(|_| format!("step '{s}' is not a number"))?;
                if step == 0 {
                    return Err("step 0".into());
                }
                (r, step)
            }
            None => (item, 1),
        };
        let (a, b) = if range == "*" {
            (lo, hi)
        } else if let Some((a, b)) = range.split_once('-') {
            let (a, b) = (value(a, i)?, value(b, i)?);
            if a > b {
                return Err(format!("range {a}-{b} goes backwards"));
            }
            (a, b)
        } else {
            let a = value(range, i)?;
            (a, if item.contains('/') { hi } else { a })
        };
        let mut v = a;
        while v <= b {
            bits |= 1 << v;
            v += step;
        }
    }
    Ok(bits)
}

/// A short human text for a cron expression (R139.1); the expression itself
/// when no pattern fits.
pub fn cron_text(expr: &str) -> String {
    let e = expr.trim().to_ascii_lowercase();
    let e = macro_expr(&e).map(str::to_string).unwrap_or(e);
    let f: Vec<&str> = e.split_whitespace().collect();
    if f.len() != 5 {
        return expr.to_string();
    }
    let num = |s: &str| s.parse::<u32>().ok();
    let rest_any = |from: usize| f[from..].iter().all(|x| *x == "*");
    let hm = |h: u32, m: u32| format!("{h:02}:{m:02}");
    if let Some(n) = f[0].strip_prefix("*/").and_then(num) {
        if f[1] == "*" && rest_any(2) {
            return format!("Every {n} min");
        }
    }
    if f[0] == "*" && rest_any(1) {
        return "Every minute".into();
    }
    if let Some(m) = num(f[0]) {
        if f[1] == "*" && rest_any(2) {
            return if m == 0 {
                "Hourly".into()
            } else {
                format!("Hourly at :{m:02}")
            };
        }
        if let Some(n) = f[1].strip_prefix("*/").and_then(num) {
            if rest_any(2) {
                return format!("Every {n} h");
            }
        }
        if let Some(h) = num(f[1]) {
            let t = hm(h, m);
            match (f[2], f[3], f[4]) {
                ("*", "*", "*") => return format!("Daily {t}"),
                ("*", "*", "1-5" | "mon-fri") => return format!("Weekdays {t}"),
                ("*", "*", "0,6" | "6,0" | "sat,sun" | "sun,sat") => {
                    return format!("Weekends {t}")
                }
                ("*", "*", d) => {
                    if let Ok(v) = value(d, 4) {
                        let name = [
                            "Sundays",
                            "Mondays",
                            "Tuesdays",
                            "Wednesdays",
                            "Thursdays",
                            "Fridays",
                            "Saturdays",
                        ];
                        return format!("{} {t}", name[(v % 7) as usize]);
                    }
                }
                (d, "*", "*") => {
                    if let Some(d) = num(d) {
                        return format!("Monthly, day {d} {t}");
                    }
                }
                _ => {}
            }
        }
    }
    expr.to_string()
}

/// `Oct 30 09:00`, with the year when it is not `now`'s.
pub fn fmt_time(t: i64, now: i64, tz: &TimeZone) -> String {
    let Ok(ts) = Timestamp::from_second(t) else {
        return t.to_string();
    };
    let z = ts.to_zoned(tz.clone());
    let year_now = Timestamp::from_second(now)
        .map(|n| n.to_zoned(tz.clone()).year())
        .unwrap_or(z.year());
    let mon = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][z.month() as usize - 1];
    let year = if z.year() == year_now {
        String::new()
    } else {
        format!(" {}", z.year())
    };
    format!("{mon} {}{year} {:02}:{:02}", z.day(), z.hour(), z.minute())
}

/// `YYYY-MM-DD HH:MM` local → Unix s (R136.3); a gap time → its end.
pub fn parse_at(text: &str, tz: &TimeZone) -> Result<i64> {
    let bad = || {
        Error::usage(format!(
            "--at must be 'YYYY-MM-DD HH:MM' (local time), not '{text}'"
        ))
    };
    let t = text.trim().replace('T', " ");
    let (d, hm) = t.split_once(' ').ok_or_else(bad)?;
    let date: Date = d.trim().parse().map_err(|_| bad())?;
    let (h, m) = hm.trim().split_once(':').ok_or_else(bad)?;
    let (h, m): (i8, i8) = (h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?);
    let time = Time::new(h, m, 0, 0).map_err(|_| bad())?;
    resolve(date.to_datetime(time), tz).ok_or_else(bad)
}

/// A Schedule name (R136.1).
pub fn check_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 40
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(Error::usage(format!(
            "schedule name '{name}' must be lowercase letters, digits and '-', start with a letter or digit, at most 40 characters"
        )))
    }
}

fn double<'de, D: Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

/// `schedule.add` / `schedule.edit` parameters (API.md); for an edit only
/// the given keys change.
#[derive(Deserialize, Default, Debug, Clone)]
pub struct Spec {
    #[serde(default)]
    pub workspace: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub rename: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default)]
    pub cron: Option<String>,
    /// `YYYY-MM-DD HH:MM` local.
    #[serde(default)]
    pub at: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    /// Absent = unchanged; null = the default at run time.
    #[serde(default, deserialize_with = "double")]
    pub account: Option<Option<String>>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub catch_up: Option<String>,
    #[serde(default)]
    pub keep_runs: Option<u8>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// What validation needs from the app (R136.5).
pub struct Ctx<'a> {
    pub workspace: &'a str,
    /// The Workspace's Projects with their links' Source ids.
    pub projects: &'a [(String, Vec<String>)],
    pub presets: &'a [String],
    /// The Workspace's other Schedule names.
    pub names: &'a [String],
    pub now: i64,
    pub tz: &'a TimeZone,
    /// Resolve `account` for a Preset like `spawn --account` (R108).
    pub account: &'a dyn Fn(&str, &str) -> Result<()>,
}

/// A new Schedule from `schedule.add` (R136.5); nothing is written here.
pub fn create(spec: &Spec, ctx: &Ctx) -> Result<Schedule> {
    let name = spec
        .name
        .clone()
        .ok_or_else(|| Error::usage("a schedule needs a name"))?;
    let base = Schedule {
        name,
        workspace: ctx.workspace.to_string(),
        project: None,
        link: None,
        dir: None,
        cron: None,
        at: None,
        agent: "claude".into(),
        account: None,
        prompt: String::new(),
        enabled: true,
        done: false,
        catch_up: CatchUp::Skip,
        keep_runs: 3,
        created: ctx.now,
        checked: ctx.now,
        last_n: 0,
        runs: Vec::new(),
    };
    if spec.project.is_none() && spec.dir.is_none() {
        return Err(Error::usage(
            "a schedule needs --project <p> or --dir <path>",
        ));
    }
    if spec.cron.is_none() && spec.at.is_none() {
        return Err(Error::usage(
            "a schedule needs --cron \"<expr>\" or --at \"YYYY-MM-DD HH:MM\"",
        ));
    }
    apply(base, spec, ctx, true)
}

/// `old` changed by `spec` (only given keys), validated (R136.5, API.md
/// `schedule.edit`).
pub fn edit(old: &Schedule, spec: &Spec, ctx: &Ctx) -> Result<Schedule> {
    apply(old.clone(), spec, ctx, false)
}

fn apply(mut s: Schedule, spec: &Spec, ctx: &Ctx, new: bool) -> Result<Schedule> {
    if let Some(n) = spec
        .rename
        .as_ref()
        .or(if new { spec.name.as_ref() } else { None })
    {
        s.name = n.clone();
    }
    check_name(&s.name)?;
    if ctx.names.contains(&s.name) {
        return Err(Error::usage(format!(
            "schedule '{}' already exists in workspace '{}'",
            s.name, ctx.workspace
        )));
    }
    if spec.project.is_some() && spec.dir.is_some() {
        return Err(Error::usage("give --project or --dir, not both"));
    }
    if spec.cron.is_some() && spec.at.is_some() {
        return Err(Error::usage("give --cron or --at, not both"));
    }
    if let Some(p) = &spec.project {
        s.project = Some(p.clone());
        s.link = spec.link.clone();
        s.dir = None;
    } else if let Some(l) = &spec.link {
        if s.project.is_none() {
            return Err(Error::usage("--link needs --project"));
        }
        s.link = Some(l.clone());
    }
    if let Some(d) = &spec.dir {
        s.dir = Some(PathBuf::from(d));
        s.project = None;
        s.link = None;
    }
    let when_changed = spec.cron.is_some() || spec.at.is_some();
    if let Some(c) = &spec.cron {
        Cron::parse(c).map_err(Error::usage)?;
        s.cron = Some(c.trim().to_string());
        s.at = None;
        s.done = false;
    }
    if let Some(a) = &spec.at {
        let at = parse_at(a, ctx.tz)?;
        if at <= ctx.now {
            return Err(Error::usage(format!("--at '{a}' is in the past")));
        }
        s.at = Some(at);
        s.cron = None;
        if s.done {
            s.done = false;
            s.enabled = true;
        }
    }
    if when_changed {
        s.checked = ctx.now;
    }
    if let Some(a) = &spec.agent {
        s.agent = a.clone();
    }
    if let Some(a) = &spec.account {
        s.account = a.clone().filter(|x| !x.is_empty());
    }
    if let Some(p) = &spec.prompt {
        s.prompt = p.clone();
    }
    if let Some(c) = &spec.catch_up {
        s.catch_up = CatchUp::parse(c)?;
    }
    if let Some(k) = spec.keep_runs {
        if !(1..=20).contains(&k) {
            return Err(Error::usage(format!("--keep-runs must be 1-20, not {k}")));
        }
        s.keep_runs = k;
    }
    if let Some(e) = spec.enabled {
        if e && !s.enabled {
            s.checked = ctx.now;
        }
        s.enabled = e;
    }
    check(&mut s, ctx)?;
    Ok(s)
}

/// The rules of R136.5 on a whole Schedule; fills in a Project's only link.
pub fn check(s: &mut Schedule, ctx: &Ctx) -> Result<()> {
    if s.prompt.trim().is_empty() {
        return Err(Error::usage("the prompt is empty"));
    }
    if let Some(p) = &s.project {
        let Some((_, links)) = ctx.projects.iter().find(|(n, _)| n == p) else {
            return Err(Error::not_found(format!(
                "no project '{p}' in workspace '{}'",
                ctx.workspace
            )));
        };
        match &s.link {
            Some(l) if links.contains(l) => {}
            Some(l) => {
                return Err(Error::not_found(format!(
                    "project '{p}' has no link '{l}' (links: {})",
                    links.join(", ")
                )))
            }
            None => match links.as_slice() {
                [one] => s.link = Some(one.clone()),
                [] => return Err(Error::usage(format!("project '{p}' has no links"))),
                many => {
                    return Err(Error::usage(format!(
                        "project '{p}' has several links: give --link ({})",
                        many.join(", ")
                    )))
                }
            },
        }
    }
    if let Some(d) = &s.dir {
        if !d.is_absolute() {
            return Err(Error::usage(format!(
                "--dir '{}' must be an absolute path",
                d.display()
            )));
        }
        if !Path::new(d).is_dir() {
            return Err(Error::usage(format!(
                "--dir '{}' is not a folder",
                d.display()
            )));
        }
    }
    if !ctx.presets.contains(&s.agent) {
        return Err(Error::usage(format!(
            "unknown agent '{}' (Presets: {})",
            s.agent,
            ctx.presets.join(", ")
        )));
    }
    if let Some(a) = &s.account {
        (ctx.account)(&s.agent, a)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "schedule_tests.rs"]
mod tests;
