//! What the usage UI shows (R110.2–R110.4, specs A.74, E, F.8–F.10,
//! F.15–F.20): the short limit figures of the `+` menu and the Statusline
//! usage cell, the Usage page's cell texts, and which forecast Toast to
//! raise. GTK-free; `rade-app` draws them.

use super::limits::{self, approx, Forecast, HOT_PERCENT};
use super::{Limit, UsageState, FORMAT_CHANGED};
use crate::account::AgentKind;
use serde::Serialize;

/// A `+` menu row / Statusline detail before any sample (never `0%`).
pub const NO_DATA_YET: &str = "no data yet";

/// A limit label in its short form: `5-hour` → `5h`, `weekly` → `wk`,
/// `Spark 5-hour` → `Spark 5h`, `Opus weekly` → `Opus wk`.
pub fn short(label: &str) -> String {
    label
        .split(' ')
        .map(|w| match w {
            "weekly" => "wk".to_string(),
            "daily" => "1d".to_string(),
            w => match w.strip_suffix("-hour") {
                Some(h) => format!("{h}h"),
                None => match w.strip_suffix("-day") {
                    Some(d) => format!("{d}d"),
                    None => w.to_string(),
                },
            },
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The short figures of an account (spec F.8–F.10, A.74): Claude's session
/// and weekly pair `5h 62% · wk 41%` while one of them is the Tightest
/// limit, else the Tightest limit alone (`Spark 5h 81%`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Brief {
    /// The first limit's short label (`5h`, `Spark 5h`).
    pub label: String,
    /// Its used % (rounded): the meter's fill.
    pub percent: i64,
    /// The second limit (`wk 41%`).
    pub rest: Option<String>,
    /// The Tightest limit is at or above 80 % (`wait` color + icon).
    pub hot: bool,
}

impl Brief {
    /// `5h 62% · wk 41%`.
    pub fn text(&self) -> String {
        match &self.rest {
            Some(r) => format!("{} {}% · {r}", self.label, self.percent),
            None => format!("{} {}%", self.label, self.percent),
        }
    }

    /// The part after the meter in the Statusline cell: `62% · wk 41%`.
    pub fn after_meter(&self) -> String {
        match &self.rest {
            Some(r) => format!("{}% · {r}", self.percent),
            None => format!("{}%", self.percent),
        }
    }
}

fn pct(l: &Limit) -> i64 {
    l.used_percent.unwrap_or(0.0).round() as i64
}

/// The [`Brief`] of an account's latest limits; `None` without a number.
pub fn brief(latest: &[Limit]) -> Option<Brief> {
    let t = limits::tightest(latest)?;
    let hot = t.used_percent.unwrap_or(0.0) >= HOT_PERCENT;
    let find = |id: &str| {
        latest
            .iter()
            .find(|l| l.limit_id == id && l.used_percent.is_some())
    };
    if let (Some(s), Some(w)) = (find("five_hour"), find("seven_day")) {
        if t.limit_id == "five_hour" || t.limit_id == "seven_day" {
            return Some(Brief {
                label: short(&s.label()),
                percent: pct(s),
                rest: Some(format!("{} {}%", short(&w.label()), pct(w))),
                hot,
            });
        }
    }
    Some(Brief {
        label: short(&t.label()),
        percent: pct(t),
        rest: None,
        hot,
    })
}

/// A `+` menu account row's detail (F.8–F.10) and whether it is hot:
/// the [`Brief`], `no data yet`, or the format-changed text.
pub fn menu_detail(state: UsageState, latest: &[Limit]) -> (String, bool) {
    if state == UsageState::FormatChanged {
        return (FORMAT_CHANGED.into(), false);
    }
    match brief(latest) {
        Some(b) => (b.text(), b.hot),
        None => (NO_DATA_YET.into(), false),
    }
}

/// The Statusline usage cell (A.74) of the focused Agent's account.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UsageCell {
    /// `claude 5h` (before the meter).
    pub lead: String,
    /// Meter fill 0–100.
    pub percent: i64,
    /// `62% · wk 41%` (after the meter).
    pub tail: String,
    pub hot: bool,
    /// `Claude · acme-work`.
    pub tooltip: String,
}

impl UsageCell {
    /// The whole cell as one line: `claude 5h 62% · wk 41%`.
    pub fn text(&self) -> String {
        format!("{} {}", self.lead, self.tail)
    }
}

/// The usage cell of an account with figures; `None` (hidden) without.
pub fn status_cell(agent: AgentKind, account: &str, latest: &[Limit]) -> Option<UsageCell> {
    let b = brief(latest)?;
    Some(UsageCell {
        lead: format!("{} {}", agent.as_str(), b.label),
        percent: b.percent,
        tail: b.after_meter(),
        hot: b.hot,
        tooltip: format!("{} · {account}", tooltip_agent(agent)),
    })
}

fn tooltip_agent(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Claude => "Claude",
        AgentKind::Codex => "Codex",
    }
}

/// A meter's fill in px: `width × percent / 100`, rounded, within 0..=width
/// (E.16: 62 % of 56 px → 35 px; A.74: 62 % of 36 px → 22 px).
pub fn meter_fill(width: u32, percent: f64) -> u32 {
    ((width as f64 * percent.clamp(0.0, 100.0) / 100.0).round() as u32).min(width)
}

/// A token count: `12.4M`, `950k`, `0`.
pub fn tokens_text(n: u64) -> String {
    match n {
        0 => "0".into(),
        n if n < 1_000 => n.to_string(),
        n if n < 100_000 => format!("{:.1}k", n as f64 / 1_000.0),
        n if n < 1_000_000 => format!("{}k", (n as f64 / 1_000.0).round() as u64),
        n => format!("{:.1}M", n as f64 / 1_000_000.0),
    }
}

/// The Updated cell (E.17): `40s`, `5m`, `2h`, `3d`; `—` without data.
pub fn updated_text(updated: Option<i64>, now: i64) -> String {
    match updated {
        Some(t) => crate::design::age(now.saturating_sub(t).max(0) as u64),
        None => "—".into(),
    }
}

/// The table head's token column for a period (E.10): `TOKENS 7D`.
pub fn tokens_head(period: super::report::Period) -> &'static str {
    match period {
        super::report::Period::Today => "TOKENS TODAY",
        super::report::Period::Week => "TOKENS 7D",
        super::report::Period::Month => "TOKENS 30D",
    }
}

/// The weekday letter of a local `YYYY-MM-DD` (E.32: `F S S M T W T`).
pub fn day_letter(day: &str) -> String {
    match crate::date::days_from_civil(day) {
        Some(d) => ["T", "F", "S", "S", "M", "T", "W"][d.rem_euclid(7) as usize].into(),
        None => "?".into(),
    }
}

/// A model's short name for the footer: `claude-opus-4-5` → `opus`.
pub fn model_short(model: &str) -> String {
    let m = model.to_lowercase();
    for k in ["opus", "sonnet", "haiku"] {
        if m.contains(k) {
            return k.into();
        }
    }
    model.to_string()
}

/// The footer's model split (E.33): `opus 78% · sonnet 22%` (shares of the
/// period's tokens, models with the same short name together, largest
/// first, at most 3).
pub fn model_split(by_model: &[(String, u64)]) -> String {
    let total: u64 = by_model.iter().map(|m| m.1).sum();
    if total == 0 {
        return String::new();
    }
    let mut merged: Vec<(String, u64)> = Vec::new();
    for (m, n) in by_model {
        let s = model_short(m);
        match merged.iter_mut().find(|x| x.0 == s) {
            Some(x) => x.1 += n,
            None => merged.push((s, *n)),
        }
    }
    merged.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    merged
        .iter()
        .take(3)
        .map(|(m, n)| format!("{m} {}%", (*n as f64 * 100.0 / total as f64).round() as i64))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// A detail limit row's reset column (E.28, E.30): `1h 12m`, `Mon 09:00`;
/// the spend limit's `of $50 /mo` (E.31).
pub fn reset_text(l: &Limit, now: i64, utc_offset: i64) -> String {
    if let Some(cap) = l.limit_usd {
        let per = match l.period.as_deref() {
            Some("month") | Some("monthly") => " /mo",
            Some("week") | Some("weekly") => " /wk",
            Some("day") | Some("daily") => " /day",
            _ => "",
        };
        return format!("of {}{per}", dollars(cap));
    }
    match l.resets_at.filter(|&r| r > now) {
        Some(r) => {
            let w = limits::when_text(r, now, utc_offset);
            w.strip_prefix("in ").map(str::to_string).unwrap_or(w)
        }
        None => String::new(),
    }
}

/// `$12`, `$12.50`.
pub fn dollars(v: f64) -> String {
    if (v - v.round()).abs() < 0.005 {
        format!("${}", v.round() as i64)
    } else {
        format!("${v:.2}")
    }
}

/// A detail limit row's value (E.28): `62%`; the spend limit `$12`
/// (E.31); Codex credits their balance; `—` without a number.
pub fn value_text(l: &Limit) -> String {
    if let Some(u) = l.used_usd {
        return dollars(u);
    }
    if let Some(b) = &l.balance {
        return b.clone();
    }
    match l.used_percent {
        Some(p) => format!("{}%", p.round() as i64),
        None => "—".into(),
    }
}

/// The detail's sub-line (E.26): `Claude Code · Max 20x · ~/.claude-work`
/// (no plan: left out).
pub fn detail_sub(agent: AgentKind, plan: Option<&str>, config_dir: &str, home: &str) -> String {
    let dir = match config_dir.strip_prefix(home) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with('/')) => {
            format!("~{rest}")
        }
        _ => config_dir.to_string(),
    };
    let mut parts = vec![agent.label().to_string()];
    if let Some(p) = plan.filter(|p| !p.is_empty()) {
        parts.push(p.to_string());
    }
    parts.push(dir);
    parts.join(" · ")
}

/// How data arrives for an account with none (spec E empty state).
pub fn how_data_arrives(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Claude => "claude: run a session with the RADE statusline",
        AgentKind::Codex => "codex: run a session",
    }
}

// ---- Forecast Toast (R110.4, spec F.15–F.20) --------------------------------

/// One account of the shown Workspace with its latest limits and their
/// forecasts.
#[derive(Clone, Debug, Default)]
pub struct AccountUsage {
    pub agent: Option<AgentKind>,
    pub name: String,
    pub limits: Vec<(Limit, Option<Forecast>)>,
}

/// The forecast Toast to raise.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ForecastToast {
    /// Once per limit per window: `<agent>/<account>/<limit key>@<resets_at>`.
    pub key: String,
    pub agent: AgentKind,
    pub account: String,
    /// The bold part (`acme-work`) and the rest of the title.
    pub name: String,
    pub rest: String,
    /// `62% now · resets in 1h 12m · acme-team is at 8%`.
    pub sub: String,
    /// The other account the action offers (`Next agents use acme-team`).
    pub other: Option<String>,
    /// A per-model limit (Opus, Spark): another model is suggested first.
    pub per_model: bool,
}

impl ForecastToast {
    /// The whole title: `acme-work will hit its 5-hour limit in ~40m at this pace`.
    pub fn title(&self) -> String {
        format!("{}{}", self.name, self.rest)
    }

    /// The action button's label.
    pub fn action(&self) -> Option<String> {
        self.other.as_ref().map(|o| format!("Next agents use {o}"))
    }

    /// The Activity line it is recorded as (R110.5 style).
    pub fn activity_line(&self) -> String {
        format!("account {}: {}", self.account, self.rest.trim_start())
    }
}

fn tightest_of(a: &AccountUsage) -> Option<f64> {
    let latest: Vec<Limit> = a.limits.iter().map(|(l, _)| l.clone()).collect();
    limits::tightest(&latest).and_then(|l| l.used_percent)
}

/// The forecast Toast to show now, if any: the limit (of any account of
/// `accounts`) forecast to hit 100 % soonest before its reset, whose
/// key is not in `shown` (at most one Toast per limit per window). The
/// other account = the same agent's account with the lowest Tightest
/// limit under 80 % (ties: by name); its figure is the same limit's when
/// it has it, else its Tightest limit's.
pub fn forecast_toast(
    accounts: &[AccountUsage],
    shown: &[String],
    now: i64,
    utc_offset: i64,
) -> Option<ForecastToast> {
    let mut best: Option<(u64, &AccountUsage, &Limit, String)> = None;
    for a in accounts {
        let Some(agent) = a.agent else { continue };
        for (l, f) in &a.limits {
            let Some(secs) = f.and_then(|f| f.hits_100_in_s) else {
                continue;
            };
            let key = format!(
                "{}/{}/{}@{}",
                agent.as_str(),
                a.name,
                l.key(),
                l.resets_at.unwrap_or(0)
            );
            if shown.contains(&key) {
                continue;
            }
            if best.as_ref().is_none_or(|b| secs < b.0) {
                best = Some((secs, a, l, key));
            }
        }
    }
    let (secs, a, l, key) = best?;
    let agent = a.agent?;
    let other = accounts
        .iter()
        .filter(|o| o.agent == Some(agent) && o.name != a.name)
        .filter_map(|o| tightest_of(o).map(|t| (t, o)))
        .filter(|(t, _)| *t < HOT_PERCENT)
        .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.name.cmp(&y.1.name)))
        .map(|(t, o)| {
            let same = o
                .limits
                .iter()
                .find(|(x, _)| x.key() == l.key())
                .and_then(|(x, _)| x.used_percent)
                .unwrap_or(t);
            (o.name.clone(), same.round() as i64)
        });
    let mut sub = vec![format!("{}% now", pct(l))];
    if let Some(r) = l.resets_at.filter(|&r| r > now) {
        sub.push(format!("resets {}", limits::when_text(r, now, utc_offset)));
    }
    let per_model = l.per_model();
    if per_model {
        sub.push(match &l.model {
            Some(m) => format!("only {m} is limited: another model is not"),
            None => "another model is not limited".into(),
        });
    }
    if let Some((n, p)) = &other {
        sub.push(format!("{n} is at {p}%"));
    }
    Some(ForecastToast {
        key,
        agent,
        account: a.name.clone(),
        name: a.name.clone(),
        rest: format!(
            " will hit its {} limit in {} at this pace",
            l.label(),
            approx(secs)
        ),
        sub: sub.join(" · "),
        other: other.map(|o| o.0),
        per_model,
    })
}

#[cfg(test)]
#[path = "view_tests.rs"]
mod tests;
