//! Tightest limit, forecast, Next resets and limit events (R110.1, R110.5).

use super::{window_name, Limit};
use serde::{Deserialize, Serialize};

/// The level from which a limit is "hot" (`wait` color, Activity event).
pub const HOT_PERCENT: f64 = 80.0;
/// A sample stamped further than this after now (clock skew) is left out of
/// the forecast fit.
pub const SKEW_S: i64 = 60;
/// Next resets lists the resets of limits used at least this much (or
/// forecast to hit 100 %): the quota worth waiting for.
pub const RESET_WORTH_PERCENT: f64 = 50.0;

/// The account's limit with the highest used % (ties: the earlier reset;
/// no reset counts as latest). Limits without a percentage never count.
pub fn tightest(limits: &[Limit]) -> Option<&Limit> {
    limits
        .iter()
        .filter(|l| l.used_percent.is_some())
        .min_by(|a, b| {
            let (pa, pb) = (a.used_percent.unwrap_or(0.0), b.used_percent.unwrap_or(0.0));
            pb.total_cmp(&pa).then(
                a.resets_at
                    .unwrap_or(i64::MAX)
                    .cmp(&b.resets_at.unwrap_or(i64::MAX)),
            )
        })
}

/// The limits as they stand at `now`: a window whose reset has passed is no
/// longer at its old percentage (a new sample comes only while a session
/// runs on that login), so it counts as 0 %, not blocked, with no reset
/// known yet (R110.1).
pub fn as_of(mut limits: Vec<Limit>, now: i64) -> Vec<Limit> {
    for l in &mut limits {
        if l.resets_at.is_some_and(|r| r <= now) {
            ended(l);
        }
    }
    limits
}

fn ended(l: &mut Limit) {
    l.used_percent = l.used_percent.map(|_| 0.0);
    l.used_usd = l.used_usd.map(|_| 0.0);
    l.resets_at = None;
    l.blocked = false;
}

/// Activity `reset` lines (R110.5) for the resets the clock passed in
/// `(from, to]`, from every stored sample of an account (the newest may
/// already be of the next window). Once per window: a window's `resets_at`
/// jitters, so only the latest time of a cluster counts; a window that was
/// used 0 %, or whose next window was sampled before the reset (then
/// [`limit_events`] said it), gives none.
pub fn reset_events(account: &str, samples: &[Limit], from: i64, to: i64) -> Vec<String> {
    let mut out = Vec::new();
    let mut done: Vec<String> = Vec::new();
    for l in samples {
        let Some(r) = l.resets_at.filter(|&r| r > from && r <= to) else {
            continue;
        };
        let key = l.key();
        if done.contains(&key) {
            continue;
        }
        let same: Vec<&Limit> = samples.iter().filter(|s| s.key() == key).collect();
        let later_in_cluster = same
            .iter()
            .any(|s| s.resets_at.is_some_and(|x| x > r && x <= r + 120));
        let used = same.iter().any(|s| {
            s.resets_at.is_some_and(|x| (r - 120..=r).contains(&x))
                && s.used_percent.unwrap_or(0.0) > 0.0
        });
        let told = same
            .iter()
            .any(|s| s.t < r && s.resets_at.is_some_and(|x| x > r + 120));
        if later_in_cluster || !used || told {
            continue;
        }
        done.push(key);
        out.push(format!("account {account}: {} reset", l.label()));
    }
    out
}

/// Where a limit is heading at the current pace.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Forecast {
    /// Reaches 100 % this many seconds from now, before its reset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hits_100_in_s: Option<u64>,
    /// Does not reach 100 % before the reset; this is where it ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_reset_percent: Option<f64>,
}

impl Forecast {
    /// `hits 100% in ~40m` or `97% at reset`.
    pub fn text(&self) -> String {
        match (self.hits_100_in_s, self.at_reset_percent) {
            (Some(s), _) => format!("hits 100% in {}", approx(s)),
            (_, Some(p)) => format!("{}% at reset", p.round() as i64),
            _ => String::new(),
        }
    }
}

/// `~4m`, `~40m` (5-minute steps under an hour), `~1h 20m` (10-minute steps).
pub fn approx(secs: u64) -> String {
    let m = (secs + 30) / 60;
    if m < 10 {
        format!("~{}m", m.max(1))
    } else if m < 60 {
        let r = (m + 2) / 5 * 5;
        if r >= 60 {
            "~1h".into()
        } else {
            format!("~{r}m")
        }
    } else {
        let r = (m + 5) / 10 * 10;
        match (r / 60, r % 60) {
            (h, 0) => format!("~{h}h"),
            (h, mm) => format!("~{h}h {mm}m"),
        }
    }
}

/// The samples of the current window: those after the last reset (a
/// `resets_at` that moved, or a drop in the used %), oldest first. Samples
/// stamped more than [`SKEW_S`] after `now` are left out.
pub fn current_window<L: std::borrow::Borrow<Limit>>(samples: &[L], now: i64) -> Vec<&Limit> {
    let mut v: Vec<&Limit> = samples
        .iter()
        .map(|l| l.borrow())
        .filter(|l| l.used_percent.is_some() && l.t <= now + SKEW_S)
        .collect();
    v.sort_by_key(|l| l.t);
    let Some(last) = v.last() else {
        return v;
    };
    let reset = last.resets_at;
    let same = |a: Option<i64>| match (a, reset) {
        (Some(a), Some(r)) => (a - r).abs() <= 120,
        (None, None) => true,
        _ => false,
    };
    let mut start = v.len() - 1;
    while start > 0 {
        let (prev, next) = (v[start - 1], v[start]);
        let dropped = prev.used_percent.unwrap_or(0.0) > next.used_percent.unwrap_or(0.0) + 0.5;
        if !same(prev.resets_at) || dropped {
            break;
        }
        start -= 1;
    }
    v.split_off(start)
}

/// A linear fit of the current window's samples (R110.1). `None` for fewer
/// than 2 samples or under a minute of data, a flat or falling series, a
/// limit already at 100 %, or a reset already past by the local clock
/// (never a negative time).
pub fn forecast<L: std::borrow::Borrow<Limit>>(samples: &[L], now: i64) -> Option<Forecast> {
    let w = current_window(samples, now);
    let (first, last) = (w.first()?, w.last()?);
    if w.len() < 2 || last.t - first.t < 60 {
        return None;
    }
    let reset = last.resets_at;
    if reset.is_some_and(|r| r <= now) || last.used_percent? >= 100.0 {
        return None;
    }
    let n = w.len() as f64;
    let t0 = first.t as f64;
    let xs: Vec<f64> = w.iter().map(|l| l.t as f64 - t0).collect();
    let ys: Vec<f64> = w.iter().map(|l| l.used_percent.unwrap_or(0.0)).collect();
    let (mx, my) = (xs.iter().sum::<f64>() / n, ys.iter().sum::<f64>() / n);
    let sxx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    let sxy: f64 = xs.iter().zip(&ys).map(|(x, y)| (x - mx) * (y - my)).sum();
    if sxx <= 0.0 {
        return None;
    }
    let slope = sxy / sxx; // % per second
    if slope * 3600.0 < 0.1 {
        return None;
    }
    let at = |t: f64| my + slope * (t - t0 - mx);
    let now_pct = at(now as f64);
    let to_100 = ((100.0 - now_pct) / slope).max(0.0);
    match reset {
        Some(r) if now as f64 + to_100 >= r as f64 => Some(Forecast {
            hits_100_in_s: None,
            at_reset_percent: Some(at(r as f64).clamp(0.0, 100.0)),
        }),
        _ => Some(Forecast {
            hits_100_in_s: Some(to_100.round() as u64),
            at_reset_percent: None,
        }),
    }
}

/// One account's latest limits with each limit's forecast.
#[derive(Clone, Debug, Default)]
pub struct AccountLimits {
    pub account: String,
    pub limits: Vec<(Limit, Option<Forecast>)>,
}

/// One row of Next resets (spec E.22–E.24).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResetRow {
    pub resets_at: i64,
    /// `in 1h 12m`, `Sat 21:00`.
    pub when: String,
    pub account: String,
    /// `5-hour · now 62%`, `weekly ×4 · max 58% Opus`.
    pub what: String,
    /// `forecast: hits 100% in ~40m`, `GPT-5.3-Codex-Spark only`, or empty.
    pub note: String,
    pub limit_id: String,
    pub name: String,
    pub used_percent: f64,
    pub forecast: Option<Forecast>,
}

const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `in 45m`, `in 1h 12m` within a day; `Sat 21:00` within a week; else
/// `Nov 7 09:00` (local time, `utc_offset` seconds east of UTC).
pub fn when_text(at: i64, now: i64, utc_offset: i64) -> String {
    let d = at - now;
    if d < 86_400 {
        let m = ((d.max(0) + 30) / 60).max(1);
        return match (m / 60, m % 60) {
            (0, mm) => format!("in {mm}m"),
            (h, 0) => format!("in {h}h"),
            (h, mm) => format!("in {h}h {mm}m"),
        };
    }
    let local = at + utc_offset;
    let (_, mo, day, h, mi, _) = crate::date::civil(local);
    if d < 7 * 86_400 {
        let wd = (local.div_euclid(86_400) + 4).rem_euclid(7) as usize;
        format!("{} {h:02}:{mi:02}", DAYS[wd])
    } else {
        format!("{} {day} {h:02}:{mi:02}", MONTHS[(mo - 1) as usize])
    }
}

/// Upcoming resets in time order (R110.1): the limits of each account that
/// reset at one time make one row; a row is listed when one of its limits is
/// used at least [`RESET_WORTH_PERCENT`] or forecast to hit 100 %. At most
/// `max` rows.
pub fn next_resets(
    accounts: &[AccountLimits],
    now: i64,
    utc_offset: i64,
    max: usize,
) -> Vec<ResetRow> {
    let mut rows = Vec::new();
    for a in accounts {
        let mut times: Vec<i64> = a
            .limits
            .iter()
            .filter_map(|(l, _)| l.resets_at.filter(|&r| r > now && l.used_percent.is_some()))
            .collect();
        times.sort();
        times.dedup();
        for at in times {
            let group: Vec<&(Limit, Option<Forecast>)> = a
                .limits
                .iter()
                .filter(|(l, _)| l.resets_at == Some(at) && l.used_percent.is_some())
                .collect();
            let Some((top, fc)) = group.iter().copied().max_by(|x, y| {
                x.0.used_percent
                    .unwrap_or(0.0)
                    .total_cmp(&y.0.used_percent.unwrap_or(0.0))
            }) else {
                continue;
            };
            let used = top.used_percent.unwrap_or(0.0);
            let hits = fc.is_some_and(|f| f.hits_100_in_s.is_some());
            if used < RESET_WORTH_PERCENT && !hits {
                continue;
            }
            let pct = used.round() as i64;
            let what = if group.len() == 1 {
                format!("{} · now {pct}%", top.label())
            } else {
                let window = top
                    .window_minutes
                    .map(window_name)
                    .unwrap_or_else(|| top.label());
                format!("{window} ×{} · max {pct}% {}", group.len(), top.name)
            };
            let note = match fc {
                Some(f) if group.len() == 1 || hits => format!("forecast: {}", f.text()),
                _ if group.len() == 1 => top
                    .model
                    .as_ref()
                    .map(|m| format!("{m} only"))
                    .unwrap_or_default(),
                _ => String::new(),
            };
            rows.push(ResetRow {
                resets_at: at,
                when: when_text(at, now, utc_offset),
                account: a.account.clone(),
                what,
                note,
                limit_id: top.limit_id.clone(),
                name: top.label(),
                used_percent: used,
                forecast: *fc,
            });
        }
    }
    rows.sort_by(|a, b| {
        a.resets_at
            .cmp(&b.resets_at)
            .then(a.account.cmp(&b.account))
    });
    rows.truncate(max);
    rows
}

/// Activity lines (R110.5) for what changed between two snapshots of one
/// account's latest limits: `account acme: Spark 5-hour over 80%`,
/// `… blocked`, `… reset`. A previous window that had ended by the new
/// sample's time counts as 0 % and not blocked (its `reset` line came from
/// the clock, [`reset_events`]).
pub fn limit_events(account: &str, before: &[Limit], after: &[Limit]) -> Vec<String> {
    let mut out = Vec::new();
    for n in after {
        let Some(p) = before.iter().find(|p| p.key() == n.key()) else {
            continue;
        };
        let mut p = p.clone();
        if p.resets_at.is_some_and(|r| r <= n.t) {
            ended(&mut p);
        }
        let (pu, nu) = (p.used_percent.unwrap_or(0.0), n.used_percent.unwrap_or(0.0));
        let line = |what: &str| format!("account {account}: {} {what}", n.label());
        let moved = matches!((p.resets_at, n.resets_at), (Some(a), Some(b)) if b > a + 120);
        if moved && nu < pu {
            out.push(line("reset"));
            continue;
        }
        if pu < HOT_PERCENT && nu >= HOT_PERCENT {
            out.push(line("over 80%"));
        }
        if !p.blocked && n.blocked {
            out.push(line("blocked"));
        }
    }
    out
}

#[cfg(test)]
#[path = "limits_tests.rs"]
mod tests;
