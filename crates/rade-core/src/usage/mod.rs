//! Agent usage (PRD R109, R110, ADR-0035 decisions 5–9): the limits each
//! CLI reports and the tokens its session logs hold, per Agent Account.
//!
//! Read-only and tolerant: unknown fields are ignored, broken lines skipped,
//! an unknown `limit_id` keeps its raw name, and when nothing can be read the
//! answer is [`FORMAT_CHANGED`], never a number.
//!
//! - [`claude`]: the statusline JSON (`rate_limits`) and transcript tokens.
//! - [`codex`]: rollout `token_count` lines (limits per `limit_id`, tokens).
//! - [`tail`]: reading what was appended to a log since last time.
//! - [`store`]: the per-login Usage store file and its 90-day retention.
//! - [`limits`]: Tightest limit, forecast, Next resets, limit events.
//! - [`detailed`]: the opt-in Detailed limits request (Claude).

pub mod claude;
pub mod codex;
pub mod detailed;
pub mod limits;
pub mod report;
pub mod store;
pub mod tail;
pub mod view;

use serde::{Deserialize, Serialize};

/// What the UI shows when a log or JSON had lines but no readable sample.
pub const FORMAT_CHANGED: &str = "no usage data (format changed?)";
/// What the UI shows before any sample exists.
pub const NO_DATA: &str = "no usage data";
/// Tokens of a session whose `cwd` is in no Project.
pub const SCRATCH: &str = "Scratch";

/// One limit of an account at one moment: a dynamic list entry
/// `{account, group, name, window, used %, resets_at, source}` (ADR-0035
/// decision 6), not a fixed "5h + weekly".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Limit {
    /// The CLI's id: `five_hour`, `seven_day`, `spend_limit`,
    /// `seven_day_opus`, … (Claude); `codex`, `codex_bengalfox`, `premium`,
    /// `credits` or an unknown raw id (Codex).
    pub limit_id: String,
    /// Detail group: `Session`, `Weekly`, `Extra usage` (Claude); the
    /// limit's short name for Codex (`Codex`, `Spark`, `Premium`, `Credits`,
    /// or the raw id).
    pub group: String,
    /// Row name within the group: `5-hour`, `weekly`, `All models`, `Opus`,
    /// `Spend limit`; a windowless Codex limit repeats its group.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_minutes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_percent: Option<f64>,
    /// Unix seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<i64>,
    /// `statusline`, `rollout` or `detailed`.
    pub source: String,
    /// When it was sampled (Unix seconds).
    #[serde(default)]
    pub t: i64,
    /// A per-model limit's full model name (`GPT-5.3-Codex-Spark`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Claude's `spend_limit` as the CLI reports it (the only money shown).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<String>,
    /// Codex credits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unlimited: Option<bool>,
    /// The CLI says this limit is the one blocking now.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub blocked: bool,
}

impl Limit {
    /// Identity of a limit across samples: one id can have several windows.
    pub fn key(&self) -> String {
        format!("{}/{}", self.limit_id, self.name)
    }

    /// The short label used for the Tightest limit, Next resets and Activity:
    /// `5-hour`, `weekly`, `Opus weekly`, `Spark 5-hour`, `Premium`.
    pub fn label(&self) -> String {
        match self.group.as_str() {
            "Session" | "Codex" => self.name.clone(),
            "Weekly" if self.name == "All models" => "weekly".into(),
            "Weekly" => format!("{} weekly", self.name),
            "Extra usage" => self.name.to_lowercase(),
            g if g == self.name => self.name.clone(),
            g => format!("{g} {}", self.name),
        }
    }

    /// A limit that applies to one model only (Opus, Sonnet, Spark): the
    /// forecast suggests another model before another account (R110.4).
    pub fn per_model(&self) -> bool {
        self.model.is_some()
            || matches!(
                self.limit_id.as_str(),
                "seven_day_opus" | "seven_day_sonnet" | "model_scoped"
            )
    }
}

/// The name of a window in minutes: `5-hour`, `weekly`, `daily`, `90-minute`.
pub fn window_name(minutes: u64) -> String {
    match minutes {
        10_080 => "weekly".into(),
        1_440 => "daily".into(),
        m if m % 1_440 == 0 => format!("{}-day", m / 1_440),
        m if m % 60 == 0 => format!("{}-hour", m / 60),
        m => format!("{m}-minute"),
    }
}

/// Token counts of one day × Project × model (cache reads kept apart: they
/// are not counted in totals, R109.2).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenRow {
    /// Local `YYYY-MM-DD`.
    pub day: String,
    pub project: String,
    pub model: String,
    pub input: u64,
    pub output: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
}

impl TokenRow {
    /// Input + output + cache creation; cache reads add 0 (they would inflate
    /// the number tenfold, design §10.3).
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_creation
    }

    fn same_bucket(&self, o: &TokenRow) -> bool {
        self.day == o.day && self.project == o.project && self.model == o.model
    }
}

/// Add `row` into `rows`, merging with its day × Project × model bucket.
pub fn add_tokens(rows: &mut Vec<TokenRow>, row: TokenRow) {
    match rows.iter_mut().find(|r| r.same_bucket(&row)) {
        Some(r) => {
            r.input += row.input;
            r.output += row.output;
            r.cache_creation += row.cache_creation;
            r.cache_read += row.cache_read;
        }
        None => rows.push(row),
    }
}

/// Whether a parse found anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UsageState {
    Ok,
    /// Nothing to read yet.
    NoData,
    /// Lines were read but none held a sample: [`FORMAT_CHANGED`].
    FormatChanged,
}

impl UsageState {
    /// From how many lines were read and how many gave a sample.
    pub fn of(lines: usize, samples: usize) -> UsageState {
        match (lines, samples) {
            (_, s) if s > 0 => UsageState::Ok,
            (0, _) => UsageState::NoData,
            _ => UsageState::FormatChanged,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            UsageState::Ok => "ok",
            UsageState::NoData => "no-data",
            UsageState::FormatChanged => "format-changed",
        }
    }
}

/// Lines read with no sample since the last one, over any number of scans,
/// before a login's logs count as [`UsageState::FormatChanged`] (R109.4):
/// a normal turn writes a few lines before its first sample (a Codex
/// `turn_context`, a Claude user line), a changed format never writes one.
pub const FORMAT_CHANGED_LINES: usize = 100;

/// A login's run of lines without a sample, across scans.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanTrack {
    pub unsampled: usize,
}

impl ScanTrack {
    /// After one scan that read `lines` with `samples` among them: the
    /// login's new state, `None` = keep the one it has.
    pub fn after(&mut self, lines: usize, samples: usize) -> Option<UsageState> {
        if samples > 0 {
            self.unsampled = 0;
            return Some(UsageState::Ok);
        }
        self.unsampled += lines;
        (self.unsampled >= FORMAT_CHANGED_LINES).then_some(UsageState::FormatChanged)
    }
}

/// Which Project a session's `cwd` belongs to (`None` → Scratch).
pub type ProjectOf<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Unix seconds of an RFC 3339 time (`2026-10-23T09:00:00.123Z`,
/// `…+07:00`); fractions are dropped.
pub fn parse_time(s: &str) -> Option<i64> {
    let days = crate::date::days_from_civil(s)?;
    let rest = s.get(10..)?;
    let rest = rest.strip_prefix('T').or_else(|| rest.strip_prefix(' '))?;
    let num = |a: usize, b: usize| rest.get(a..b)?.parse::<i64>().ok();
    let (h, m, sec) = (num(0, 2)?, num(3, 5)?, num(6, 8)?);
    let mut tail = &rest[8..];
    if let Some(t) = tail.strip_prefix('.') {
        tail = t.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match tail {
        "" | "Z" | "z" => 0,
        t if t.len() == 6 && (t.starts_with('+') || t.starts_with('-')) => {
            let o = t[1..3].parse::<i64>().ok()? * 3600 + t[4..6].parse::<i64>().ok()? * 60;
            if t.starts_with('-') {
                -o
            } else {
                o
            }
        }
        _ => return None,
    };
    Some(days * 86_400 + h * 3600 + m * 60 + sec - offset)
}

/// A resets_at value as either Unix seconds or an RFC 3339 string.
pub(crate) fn time_value(v: &serde_json::Value) -> Option<i64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64().map(|f| f as i64),
        serde_json::Value::String(s) => parse_time(s),
        _ => None,
    }
}

/// Local `YYYY-MM-DD` of Unix seconds, `utc_offset` seconds east of UTC.
pub fn local_day(secs: i64, utc_offset: i64) -> String {
    let (y, m, d, ..) = crate::date::civil(secs + utc_offset);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_times() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_time("2026-10-23T09:00:00.863Z"), Some(1_792_746_000));
        assert_eq!(parse_time("2026-10-23T16:00:00+07:00"), Some(1_792_746_000));
        assert_eq!(parse_time("2026-10-23"), None);
        assert_eq!(parse_time("garbage"), None);
        assert_eq!(local_day(1_792_746_000, 0), "2026-10-23");
        assert_eq!(local_day(1_792_746_000, 16 * 3600), "2026-10-24");
    }

    #[test]
    fn window_names_and_labels() {
        assert_eq!(window_name(300), "5-hour");
        assert_eq!(window_name(10_080), "weekly");
        assert_eq!(window_name(2_880), "2-day");
        assert_eq!(window_name(90), "90-minute");
        let l = |g: &str, n: &str| Limit {
            group: g.into(),
            name: n.into(),
            ..Limit::default()
        };
        assert_eq!(l("Session", "5-hour").label(), "5-hour");
        assert_eq!(l("Weekly", "All models").label(), "weekly");
        assert_eq!(l("Weekly", "Opus").label(), "Opus weekly");
        assert_eq!(l("Codex", "weekly").label(), "weekly");
        assert_eq!(l("Spark", "5-hour").label(), "Spark 5-hour");
        assert_eq!(l("Premium", "Premium").label(), "Premium");
        assert_eq!(l("Extra usage", "Spend limit").label(), "spend limit");
    }

    #[test]
    fn a_turn_without_its_sample_yet_is_not_a_format_change() {
        // A Codex turn writes turn_context / user lines before its first
        // token_count; a Claude transcript the user line before the
        // assistant's usage. A poll in between must not hide the numbers.
        let mut t = ScanTrack::default();
        assert_eq!(t.after(40, 3), Some(UsageState::Ok));
        assert_eq!(t.after(4, 0), None);
        assert_eq!(t.after(0, 0), None);
        assert_eq!(t.after(2, 1), Some(UsageState::Ok));
        // Only a long run of lines with no sample at all is one.
        let mut t = ScanTrack::default();
        assert_eq!(t.after(FORMAT_CHANGED_LINES - 1, 0), None);
        assert_eq!(t.after(1, 0), Some(UsageState::FormatChanged));
        assert_eq!(t.after(3, 0), Some(UsageState::FormatChanged));
        assert_eq!(t.after(3, 1), Some(UsageState::Ok));
        assert_eq!(t.after(5, 0), None);
    }

    #[test]
    fn state_of_lines_and_samples() {
        assert_eq!(UsageState::of(0, 0), UsageState::NoData);
        assert_eq!(UsageState::of(5, 0), UsageState::FormatChanged);
        assert_eq!(UsageState::of(5, 1), UsageState::Ok);
    }
}
