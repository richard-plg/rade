//! What the app makes of a Usage store (R109.3, R110.1, R110.2): which new
//! samples are worth a line and the Activity lines they cause, and the
//! `usage.list` / `usage.get` answers (Tightest limit, tokens per period,
//! limit groups, state) — GTK-free so the app only wires files and threads.

use super::limits::{self, AccountLimits, Forecast};
use super::store::{self, Record};
use super::{local_day, Limit, UsageState};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The token period of `usage.list` / `usage.get` (`period`): Today, 7 days
/// (the default, spec E.5) or 30 days, in local days ending today.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Period {
    Today,
    #[default]
    Week,
    Month,
}

impl Period {
    /// `today`, `7d`, `30d`; `None` (absent) → 7 days; anything else → `Err`.
    pub fn parse(s: Option<&str>) -> Result<Period, String> {
        match s {
            None => Ok(Period::Week),
            Some("today") => Ok(Period::Today),
            Some("7d") => Ok(Period::Week),
            Some("30d") => Ok(Period::Month),
            Some(o) => Err(format!("period must be today, 7d or 30d, not '{o}'")),
        }
    }

    pub fn days(self) -> i64 {
        match self {
            Period::Today => 1,
            Period::Week => 7,
            Period::Month => 30,
        }
    }

    /// The local days of the period, oldest first (`YYYY-MM-DD`).
    pub fn day_list(self, now: i64, utc_offset: i64) -> Vec<String> {
        (0..self.days())
            .rev()
            .map(|k| local_day(now - k * 86_400, utc_offset))
            .collect()
    }
}

/// The samples of one batch (`usage.record`, or one scan) worth a line
/// against what the store holds, with the account stamped on them, and the
/// Activity lines (R110.5) that the batch causes.
pub fn new_samples(
    records: &[Record],
    account: &str,
    batch: Vec<Limit>,
) -> (Vec<Record>, Vec<String>) {
    let before = store::latest(records);
    let keep: Vec<Limit> = batch
        .into_iter()
        .filter(|l| store::worth_recording(before.iter().find(|p| p.key() == l.key()), l))
        .collect();
    let mut after = before.clone();
    for l in &keep {
        match after.iter_mut().find(|x| x.key() == l.key()) {
            Some(x) if l.t >= x.t => *x = l.clone(),
            Some(_) => {}
            None => after.push(l.clone()),
        }
    }
    let events = limits::limit_events(account, &before, &after);
    let recs = keep
        .into_iter()
        .map(|limit| Record::Limit {
            account: Some(account.to_string()),
            limit,
        })
        .collect();
    (recs, events)
}

/// The Project a session's `cwd` belongs to: the longest checkout folder
/// (`(path, Project name)`) that holds it, else `None` (Scratch).
pub fn project_for(cwd: &str, checkouts: &[(PathBuf, String)]) -> Option<String> {
    let cwd = Path::new(cwd);
    checkouts
        .iter()
        .filter(|(p, _)| cwd.starts_with(p))
        .max_by_key(|(p, _)| p.components().count())
        .map(|(_, n)| n.clone())
}

fn token_rows<'a>(
    records: &'a [Record],
    from_day: &'a str,
) -> impl Iterator<Item = &'a super::TokenRow> {
    records.iter().filter_map(move |r| match r {
        Record::Tokens { row, .. } if row.day.as_str() >= from_day => Some(row),
        _ => None,
    })
}

/// The period's first local day.
fn first_day(period: Period, now: i64, utc_offset: i64) -> String {
    local_day(now - (period.days() - 1) * 86_400, utc_offset)
}

/// One account's `usage.list` numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub tightest: Option<Limit>,
    /// Tokens in the period, cache reads excluded.
    pub tokens: u64,
    /// The newest record's time (Unix seconds).
    pub updated: Option<i64>,
    pub state: UsageState,
}

/// The state of an account: `format-changed` when its last scan read lines
/// but no sample (never a stale number then), `ok` with records, else
/// `no-data`.
pub fn state_of(records: &[Record], last_scan: Option<UsageState>) -> UsageState {
    match last_scan {
        Some(UsageState::FormatChanged) => UsageState::FormatChanged,
        _ if records.is_empty() => UsageState::NoData,
        _ => UsageState::Ok,
    }
}

pub fn summary(
    records: &[Record],
    last_scan: Option<UsageState>,
    period: Period,
    now: i64,
    utc_offset: i64,
) -> Summary {
    let state = state_of(records, last_scan);
    let latest = limits::as_of(store::latest(records), now);
    let from = first_day(period, now, utc_offset);
    Summary {
        tightest: (state == UsageState::Ok)
            .then(|| limits::tightest(&latest).cloned())
            .flatten(),
        tokens: token_rows(records, &from).map(|r| r.total()).sum(),
        updated: records.iter().map(Record::t).max(),
        state,
    }
}

/// An account's latest limits as they stand at `now` ([`limits::as_of`]),
/// each with its forecast from its samples in
/// the current window (for Next resets).
pub fn account_limits(account: &str, records: &[Record], now: i64) -> AccountLimits {
    AccountLimits {
        account: account.to_string(),
        limits: limits::as_of(store::latest(records), now)
            .into_iter()
            .map(|l| {
                let f: Option<Forecast> = limits::forecast(&store::series(records, &l.key()), now);
                (l, f)
            })
            .collect(),
    }
}

/// `usage.get`'s tokens: per day of the period (every day listed), per
/// Project and per model (largest first); cache reads excluded.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Tokens {
    pub by_day: Vec<DayTokens>,
    pub by_project: Vec<ProjectTokens>,
    pub by_model: Vec<ModelTokens>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DayTokens {
    pub day: String,
    pub tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ProjectTokens {
    pub project: String,
    pub tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ModelTokens {
    pub model: String,
    pub tokens: u64,
}

fn ranked(m: BTreeMap<String, u64>) -> Vec<(String, u64)> {
    let mut v: Vec<(String, u64)> = m.into_iter().filter(|(_, n)| *n > 0).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

pub fn tokens(records: &[Record], period: Period, now: i64, utc_offset: i64) -> Tokens {
    let days = period.day_list(now, utc_offset);
    let from = days.first().cloned().unwrap_or_default();
    let (mut day, mut project, mut model) = (BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
    for r in token_rows(records, &from) {
        *day.entry(r.day.clone()).or_insert(0) += r.total();
        *project.entry(r.project.clone()).or_insert(0) += r.total();
        *model.entry(r.model.clone()).or_insert(0) += r.total();
    }
    Tokens {
        by_day: days
            .into_iter()
            .map(|d| DayTokens {
                tokens: day.get(&d).copied().unwrap_or(0),
                day: d,
            })
            .collect(),
        by_project: ranked(project)
            .into_iter()
            .map(|(project, tokens)| ProjectTokens { project, tokens })
            .collect(),
        by_model: ranked(model)
            .into_iter()
            .map(|(model, tokens)| ModelTokens { model, tokens })
            .collect(),
    }
}

/// The order of rows within a group (spec E.30): Claude's weekly `All
/// models` first, then Opus, Sonnet, OAuth apps, other models; Codex windows
/// shortest first.
fn row_rank(l: &Limit) -> (u8, u64) {
    let named = match l.limit_id.as_str() {
        "five_hour" | "seven_day" | "spend_limit" => 0,
        "seven_day_opus" => 1,
        "seven_day_sonnet" => 2,
        "seven_day_oauth_apps" => 3,
        _ => 4,
    };
    (named, l.window_minutes.unwrap_or(u64::MAX))
}

/// An account's latest limits grouped for its detail (spec E.27, E.30):
/// `Session`, `Weekly`, `Extra usage`, then Codex groups by `limit_id` in
/// the order first seen, `Credits` last.
pub fn groups(limits: &[Limit]) -> Vec<(String, Vec<Limit>)> {
    let mut out: Vec<(String, Vec<Limit>)> = Vec::new();
    for l in limits {
        match out.iter_mut().find(|(g, _)| *g == l.group) {
            Some((_, v)) => v.push(l.clone()),
            None => out.push((l.group.clone(), vec![l.clone()])),
        }
    }
    let rank = |g: &str| match g {
        "Session" => 0,
        "Weekly" => 1,
        "Extra usage" => 2,
        "Credits" => 4,
        _ => 3,
    };
    out.sort_by_key(|(g, _)| rank(g)); // stable: Codex groups keep their order
    for (_, v) in &mut out {
        v.sort_by_key(row_rank);
    }
    out
}

/// `rade account usage`'s line for one account:
/// `acme  codex  81% Spark 5-hour  resets in 2h 30m`; without a number
/// `oss  codex  no usage data` (or the format-changed text).
pub fn account_line(
    name: &str,
    agent: &str,
    tightest: Option<(&str, f64, Option<i64>)>,
    state: &str,
    now: i64,
    utc_offset: i64,
) -> String {
    match tightest {
        Some((label, used, reset)) => {
            let mut l = format!("{name}  {agent}  {}% {label}", used.round() as i64);
            if let Some(r) = reset.filter(|&r| r > now) {
                l.push_str(&format!(
                    "  resets {}",
                    limits::when_text(r, now, utc_offset)
                ));
            }
            l
        }
        None if state == "format-changed" => format!("{name}  {agent}  {}", super::FORMAT_CHANGED),
        None => format!("{name}  {agent}  {}", super::NO_DATA),
    }
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod tests;
