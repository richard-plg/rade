//! Codex usage (R109.2): `token_count` lines of
//! `$CODEX_HOME/sessions/**/rollout-*.jsonl` carry `rate_limits` for one
//! `limit_id` each (`codex`, `codex_bengalfox` = Spark, `premium`, …) and the
//! session's cumulative `info.total_token_usage`.

use super::{add_tokens, local_day, parse_time, window_name, Limit, ProjectOf, TokenRow, SCRATCH};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Cumulative token counters of one session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    pub input: u64,
    pub cached: u64,
    pub cache_write: u64,
    pub output: u64,
}

/// What a rollout file said before the part being read: kept with the file's
/// offset so a tail can resume mid-file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RolloutState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<Totals>,
}

/// What a chunk of rollout lines held.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rollout {
    /// The newest sample of each limit (by `limit_id` and window), in the
    /// order first seen; credits last.
    pub limits: Vec<Limit>,
    pub tokens: Vec<TokenRow>,
    /// Non-empty lines read.
    pub lines: usize,
    /// Lines that gave a limit or token sample.
    pub samples: usize,
    /// Lines that were not JSON.
    pub broken: usize,
    /// Lines handed to the JSON parser (only those that can hold a sample
    /// or the session's context, R111-e).
    pub parsed: usize,
}

/// A line the parser reads: the top-level or payload `type` it handles.
/// Every other line (conversation, tool output: most of a rollout's bytes)
/// is only checked for being a `{…}` object, never parsed (R111-e).
fn sample_line(line: &str) -> bool {
    line.contains("\"token_count\"")
        || line.contains("\"turn_context\"")
        || line.contains("\"session_meta\"")
}

/// `GPT-5.3-Codex-Spark` → `Spark`; known ids by name; an unknown id
/// without a `limit_name` keeps its raw id (R109.4).
fn group_of(limit_id: &str, limit_name: Option<&str>) -> String {
    match (limit_id, limit_name) {
        ("codex", _) => "Codex".into(),
        ("premium", _) => "Premium".into(),
        (_, Some(n)) if !n.is_empty() => match n.rfind("Codex-") {
            Some(i) if i + 6 < n.len() => n[i + 6..].to_string(),
            _ => n.to_string(),
        },
        (id, _) => id.to_string(),
    }
}

fn upsert(limits: &mut Vec<Limit>, l: Limit) {
    match limits.iter_mut().find(|x| x.key() == l.key()) {
        Some(x) if l.t >= x.t => *x = l,
        Some(_) => {}
        None => limits.push(l),
    }
}

/// The limits of one `rate_limits` object at time `t`.
pub fn rate_limits(rl: &Value, t: i64) -> Vec<Limit> {
    let Some(id) = rl["limit_id"].as_str().filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    let model = rl["limit_name"].as_str().filter(|s| !s.is_empty());
    let group = group_of(id, model);
    let reached = rl["rate_limit_reached_type"].as_str();
    let base = Limit {
        limit_id: id.into(),
        group: group.clone(),
        source: "rollout".into(),
        t,
        model: model.map(str::to_string),
        ..Limit::default()
    };
    let mut out = Vec::new();
    for slot in ["primary", "secondary"] {
        let Some(w) = rl.get(slot).filter(|w| w.is_object()) else {
            continue;
        };
        let minutes = w["window_minutes"].as_u64();
        out.push(Limit {
            name: minutes.map(window_name).unwrap_or_else(|| slot.into()),
            window_minutes: minutes,
            used_percent: w["used_percent"].as_f64(),
            resets_at: super::time_value(&w["resets_at"]),
            blocked: reached == Some(slot),
            ..base.clone()
        });
    }
    if out.is_empty() {
        out.push(Limit {
            name: group,
            ..base.clone()
        });
    }
    // A reached type that names no window: the fullest window blocks.
    if reached.is_some() && !out.iter().any(|l| l.blocked) {
        if let Some(l) = out.iter_mut().max_by(|a, b| {
            a.used_percent
                .unwrap_or(0.0)
                .total_cmp(&b.used_percent.unwrap_or(0.0))
        }) {
            l.blocked = true;
        }
    }
    if let Some(c) = rl.get("credits").filter(|c| c.is_object()) {
        out.push(Limit {
            limit_id: "credits".into(),
            group: "Credits".into(),
            name: "Credits".into(),
            source: "rollout".into(),
            t,
            balance: c["balance"]
                .as_str()
                .map(str::to_string)
                .or_else(|| c["balance"].as_f64().map(|b| b.to_string())),
            unlimited: c["unlimited"].as_bool(),
            ..Limit::default()
        });
    }
    out
}

/// Limits and token rows of rollout lines (`state` carries the session's
/// `cwd`, model and last totals across chunks). Broken lines are skipped.
pub fn parse_rollout(
    text: &str,
    state: &mut RolloutState,
    project_of: ProjectOf<'_>,
    utc_offset: i64,
) -> Rollout {
    let mut out = Rollout::default();
    let mut credits: Option<Limit> = None;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        out.lines += 1;
        if !sample_line(line) {
            let t = line.trim();
            if !(t.starts_with('{') && t.ends_with('}')) {
                out.broken += 1;
            }
            continue;
        }
        out.parsed += 1;
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            out.broken += 1;
            continue;
        };
        let p = &v["payload"];
        let t = v["timestamp"].as_str().and_then(parse_time).unwrap_or(0);
        match v["type"].as_str() {
            Some("session_meta") | Some("turn_context") => {
                if let Some(c) = p["cwd"].as_str() {
                    state.cwd = Some(c.to_string());
                }
                if let Some(m) = p["model"].as_str() {
                    state.model = Some(m.to_string());
                }
            }
            Some("event_msg") if p["type"] == "token_count" => {
                let mut sampled = false;
                if p["rate_limits"].is_object() {
                    for l in rate_limits(&p["rate_limits"], t) {
                        sampled = true;
                        if l.limit_id == "credits" {
                            if credits.as_ref().is_none_or(|c| l.t >= c.t) {
                                credits = Some(l);
                            }
                        } else {
                            upsert(&mut out.limits, l);
                        }
                    }
                }
                if let Some(row) = token_delta(&p["info"], state, t, project_of, utc_offset) {
                    sampled = true;
                    add_tokens(&mut out.tokens, row);
                }
                out.samples += usize::from(sampled);
            }
            _ => {}
        }
    }
    out.limits.extend(credits);
    out
}

/// The tokens a `token_count`'s cumulative totals added since the last ones
/// (a counter that went down starts a new count). `None` without totals or
/// when nothing changed.
fn token_delta(
    info: &Value,
    state: &mut RolloutState,
    t: i64,
    project_of: ProjectOf<'_>,
    utc_offset: i64,
) -> Option<TokenRow> {
    let u = info.get("total_token_usage").filter(|u| u.is_object())?;
    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    let now = Totals {
        input: n("input_tokens"),
        cached: n("cached_input_tokens"),
        cache_write: n("cache_write_input_tokens"),
        output: n("output_tokens"),
    };
    let last = state.last.unwrap_or_default();
    let went_down = now.input < last.input
        || now.cached < last.cached
        || now.cache_write < last.cache_write
        || now.output < last.output;
    let prev = if went_down { Totals::default() } else { last };
    state.last = Some(now);
    let d = Totals {
        input: now.input - prev.input,
        cached: now.cached - prev.cached,
        cache_write: now.cache_write - prev.cache_write,
        output: now.output - prev.output,
    };
    if d == Totals::default() {
        return None;
    }
    Some(TokenRow {
        day: local_day(t, utc_offset),
        project: state
            .cwd
            .as_deref()
            .and_then(project_of)
            .unwrap_or_else(|| SCRATCH.into()),
        model: state.model.clone().unwrap_or_else(|| "unknown".into()),
        // OpenAI counts cached input inside input_tokens.
        input: d.input.saturating_sub(d.cached),
        output: d.output,
        cache_creation: d.cache_write,
        cache_read: d.cached,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{UsageState, FORMAT_CHANGED};

    const ROLLOUT: &str = include_str!("../../tests/fixtures/usage/rollout-0.163.0.jsonl");

    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn r109_c_rollout_limits_newest_wins() {
        let mut st = RolloutState::default();
        let r = parse_rollout(ROLLOUT, &mut st, &none, 0);
        let rows: Vec<(String, Option<f64>)> = r
            .limits
            .iter()
            .map(|l| (l.label(), l.used_percent))
            .collect();
        assert_eq!(
            rows,
            [
                ("weekly".to_string(), Some(41.0)),
                ("Spark 5-hour".to_string(), Some(81.0)),
                ("Spark weekly".to_string(), Some(33.0)),
                ("Premium".to_string(), None),
                ("Credits".to_string(), None),
            ]
        );
        assert_eq!(r.limits[0].group, "Codex");
        assert_eq!(r.limits[0].window_minutes, Some(10_080));
        let spark = &r.limits[1];
        assert_eq!(spark.limit_id, "codex_bengalfox");
        assert_eq!(spark.model.as_deref(), Some("GPT-5.3-Codex-Spark"));
        assert_eq!(spark.resets_at, Some(1_793_181_600));
        assert!(spark.per_model());
        assert_eq!(r.limits[4].balance.as_deref(), Some("0"));
        assert_eq!(r.limits[4].unlimited, Some(false));
        assert_eq!(r.broken, 0);
        assert_eq!(UsageState::of(r.lines, r.samples), UsageState::Ok);
        // The session's cwd and model are kept for the next chunk.
        assert_eq!(
            st.cwd.as_deref(),
            Some("/home/user/workspaces/acme/shop-web")
        );
        assert_eq!(st.model.as_deref(), Some("gpt-6.1-sol"));
        // Tokens: the last total minus nothing (first count), cache reads apart.
        let tok: u64 = r.tokens.iter().map(TokenRow::total).sum();
        assert_eq!(tok, (616_151 - 541_568) + 46_111);
        assert_eq!(r.tokens[0].cache_read, 541_568);
        assert_eq!(r.tokens[0].project, SCRATCH);
    }

    #[test]
    fn r109_e_broken_lines_unknown_limit_and_no_sample() {
        let text = format!(
            "{ROLLOUT}{{not json\n{{\"timestamp\":\"2026-10-23T10:00:00Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"info\":null,\"rate_limits\":{{\"limit_id\":\"codex_newmodel\",\"limit_name\":null,\"primary\":{{\"used_percent\":12.5,\"window_minutes\":300,\"resets_at\":1793181600}},\"secondary\":null,\"future\":1}}}}}}\n\"truncated\":tr\n"
        );
        let r = parse_rollout(&text, &mut RolloutState::default(), &none, 0);
        assert_eq!(r.broken, 2);
        let new = r
            .limits
            .iter()
            .find(|l| l.limit_id == "codex_newmodel")
            .unwrap();
        assert_eq!(new.group, "codex_newmodel");
        assert_eq!(new.label(), "codex_newmodel 5-hour");
        assert_eq!(new.used_percent, Some(12.5));

        let other = "{\"timestamp\":\"x\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count2\",\"usage\":{\"pct\":50}}}\n{\"type\":\"session_meta\",\"payload\":{}}\n";
        let r = parse_rollout(other, &mut RolloutState::default(), &none, 0);
        assert!(r.limits.is_empty());
        assert!(r.limits.iter().all(|l| l.used_percent.is_none()));
        let state = UsageState::of(r.lines, r.samples);
        assert_eq!(state, UsageState::FormatChanged);
        assert_eq!(FORMAT_CHANGED, "no usage data (format changed?)");
    }

    #[test]
    fn reached_type_marks_the_blocking_window_and_counters_restart() {
        let rl: Value = serde_json::from_str(
            r#"{"limit_id":"codex_bengalfox","limit_name":"GPT-5.3-Codex-Spark","primary":{"used_percent":100.0,"window_minutes":300,"resets_at":1},"secondary":{"used_percent":40.0,"window_minutes":10080,"resets_at":2},"rate_limit_reached_type":"rate_limit"}"#,
        )
        .unwrap();
        let l = rate_limits(&rl, 5);
        assert!(l[0].blocked && !l[1].blocked);
        let mut st = RolloutState {
            last: Some(Totals {
                input: 1_000,
                ..Totals::default()
            }),
            ..RolloutState::default()
        };
        let info: Value = serde_json::from_str(
            r#"{"total_token_usage":{"input_tokens":30,"cached_input_tokens":10,"output_tokens":5}}"#,
        )
        .unwrap();
        let row = token_delta(&info, &mut st, 0, &none, 0).unwrap();
        assert_eq!((row.input, row.output, row.cache_read), (20, 5, 10));
        assert!(token_delta(&info, &mut st, 0, &none, 0).is_none());
    }
}
