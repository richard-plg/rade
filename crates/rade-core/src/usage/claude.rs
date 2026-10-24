//! Claude Code usage (R109.1, R109.2): the `rate_limits` of the statusline
//! JSON and the `message.usage` of transcripts under
//! `$CLAUDE_CONFIG_DIR/projects/*/*.jsonl`.

use super::{add_tokens, local_day, parse_time, time_value, Limit, ProjectOf, TokenRow, SCRATCH};
use serde_json::Value;
use std::collections::VecDeque;

/// The statusline limits RADE records (the binary's statusline code builds
/// only these three; the others come from Detailed limits).
const STATUSLINE: [(&str, &str, &str); 3] = [
    ("five_hour", "Session", "5-hour"),
    ("seven_day", "Weekly", "All models"),
    ("spend_limit", "Extra usage", "Spend limit"),
];

/// The limits in a statusline JSON at time `t`. `Ok(vec![])` when the JSON
/// has no `rate_limits` (API key or 3P sessions); `Err` when it is not JSON
/// or `rate_limits` is not an object (format changed). Unknown fields and
/// unknown buckets are ignored.
pub fn parse_statusline(text: &str, t: i64) -> Result<Vec<Limit>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    match v.get("rate_limits") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(rl @ Value::Object(_)) => Ok(statusline_limits(rl, t)),
        Some(_) => Err("`rate_limits` is not an object".into()),
    }
}

/// The three statusline buckets of a `rate_limits` object.
pub fn statusline_limits(rl: &Value, t: i64) -> Vec<Limit> {
    let mut out = Vec::new();
    for (id, group, name) in STATUSLINE {
        let Some(b) = rl.get(id).filter(|b| b.is_object()) else {
            continue;
        };
        let Some(used) = b.get("used_percentage").and_then(Value::as_f64) else {
            continue;
        };
        out.push(Limit {
            limit_id: id.into(),
            group: group.into(),
            name: name.into(),
            window_minutes: match id {
                "five_hour" => Some(300),
                "seven_day" => Some(10_080),
                _ => None,
            },
            used_percent: Some(used),
            resets_at: b.get("resets_at").and_then(time_value),
            source: "statusline".into(),
            t,
            used_usd: b.get("used_usd").and_then(Value::as_f64),
            limit_usd: b.get("limit_usd").and_then(Value::as_f64),
            period: b.get("period").and_then(Value::as_str).map(str::to_string),
            ..Limit::default()
        });
    }
    out
}

/// Per transcript file: the message ids already counted (Claude Code writes
/// one assistant message's usage on several lines). Bounded.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Seen {
    #[serde(default)]
    pub ids: VecDeque<String>,
}

const SEEN_MAX: usize = 256;

impl Seen {
    /// `true` the first time `id` is offered.
    fn first(&mut self, id: &str) -> bool {
        if self.ids.iter().any(|s| s == id) {
            return false;
        }
        if self.ids.len() == SEEN_MAX {
            self.ids.pop_front();
        }
        self.ids.push_back(id.to_string());
        true
    }
}

/// What a chunk of transcript lines held.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transcript {
    pub tokens: Vec<TokenRow>,
    /// Non-empty lines read.
    pub lines: usize,
    /// Assistant lines with a usage (counted or a repeat).
    pub samples: usize,
    /// Lines handed to the JSON parser: only those with a `"usage"` key
    /// (R111-e; user and tool lines are most of a transcript's bytes).
    pub parsed: usize,
}

/// Token rows of transcript lines, per local day × Project × model. A
/// message id counted before (in `seen`) adds nothing; a line that is not
/// JSON or has no `message.usage` is skipped.
pub fn transcript_tokens(
    text: &str,
    seen: &mut Seen,
    project_of: ProjectOf<'_>,
    utc_offset: i64,
) -> Transcript {
    let mut out = Transcript::default();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        out.lines += 1;
        if !line.contains("\"usage\"") {
            continue;
        }
        out.parsed += 1;
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let msg = &v["message"];
        let Some(u) = msg.get("usage").filter(|u| u.is_object()) else {
            continue;
        };
        out.samples += 1;
        let id = msg["id"]
            .as_str()
            .or_else(|| v["requestId"].as_str())
            .or_else(|| v["uuid"].as_str());
        if let Some(id) = id {
            if !seen.first(id) {
                continue;
            }
        }
        let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
        let t = v["timestamp"].as_str().and_then(parse_time).unwrap_or(0);
        let project = v["cwd"]
            .as_str()
            .and_then(project_of)
            .unwrap_or_else(|| SCRATCH.into());
        add_tokens(
            &mut out.tokens,
            TokenRow {
                day: local_day(t, utc_offset),
                project,
                model: msg["model"].as_str().unwrap_or("unknown").to_string(),
                input: n("input_tokens"),
                output: n("output_tokens"),
                cache_creation: n("cache_creation_input_tokens"),
                cache_read: n("cache_read_input_tokens"),
            },
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUSLINE_JSON: &str =
        include_str!("../../tests/fixtures/usage/statusline-2.1.309.json");
    const TRANSCRIPT: &str = include_str!("../../tests/fixtures/usage/transcript-2.1.309.jsonl");

    #[test]
    fn r109_a_statusline_json_gives_three_limits() {
        let l = parse_statusline(STATUSLINE_JSON, 1_000).unwrap();
        let ids: Vec<&str> = l.iter().map(|x| x.limit_id.as_str()).collect();
        // `future_bucket` (unknown) and `some_new_field` are ignored.
        assert_eq!(ids, ["five_hour", "seven_day", "spend_limit"]);
        assert_eq!(l[0].used_percent, Some(62.0));
        assert_eq!(l[0].resets_at, Some(1_793_180_320));
        assert_eq!(
            (l[0].group.as_str(), l[0].name.as_str()),
            ("Session", "5-hour")
        );
        assert_eq!(l[0].window_minutes, Some(300));
        assert_eq!(l[1].used_percent, Some(41.0));
        assert_eq!(l[1].label(), "weekly");
        assert_eq!((l[2].used_usd, l[2].limit_usd), (Some(12.0), Some(50.0)));
        assert_eq!(l[2].period.as_deref(), Some("month"));
        assert!(l.iter().all(|x| x.source == "statusline" && x.t == 1_000));
    }

    #[test]
    fn statusline_without_or_with_broken_rate_limits() {
        assert_eq!(parse_statusline(r#"{"model":{}}"#, 0), Ok(vec![]));
        assert_eq!(parse_statusline(r#"{"rate_limits":null}"#, 0), Ok(vec![]));
        assert!(parse_statusline(r#"{"rate_limits":[1]}"#, 0).is_err());
        assert!(parse_statusline("not json", 0).is_err());
        // A bucket without a number is skipped, ISO resets are read.
        let l = parse_statusline(
            r#"{"rate_limits":{"five_hour":{"used_percentage":"x"},"seven_day":{"used_percentage":3,"resets_at":"2026-10-23T09:00:00Z"}}}"#,
            0,
        )
        .unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].resets_at, Some(1_792_746_000));
    }

    #[test]
    fn r109_f_transcript_tokens_by_model_and_project() {
        let shop = |cwd: &str| {
            cwd.starts_with("/home/user/workspaces/acme/shop-web")
                .then(|| "Shop".to_string())
        };
        let mut seen = Seen::default();
        let t = transcript_tokens(TRANSCRIPT, &mut seen, &shop, 0);
        assert_eq!(t.lines, 5);
        assert_eq!(t.samples, 4);
        let row = |p: &str, m: &str| {
            t.tokens
                .iter()
                .find(|r| r.project == p && r.model == m)
                .cloned()
                .unwrap()
        };
        // msg_01 is written twice and counted once.
        let opus = row("Shop", "claude-opus-5-5");
        assert_eq!(
            (opus.input, opus.output, opus.cache_creation),
            (100, 200, 50)
        );
        assert_eq!(opus.total(), 350);
        let sonnet = row("Shop", "claude-sonnet-5-5");
        assert_eq!(sonnet.total(), 35);
        let scratch = row(SCRATCH, "claude-opus-5-5");
        assert_eq!(scratch.total(), 6);
        assert_eq!(t.tokens.len(), 3);
        assert!(t.tokens.iter().all(|r| r.day == "2026-10-23"));
        // Cache reads are recorded apart and add 0 to every total.
        assert_eq!(opus.cache_read, 90_000);
        let all: u64 = t.tokens.iter().map(TokenRow::total).sum();
        assert_eq!(all, 391);
        // A second read of the same lines (the same file) counts nothing.
        let again = transcript_tokens(TRANSCRIPT, &mut seen, &shop, 0);
        assert!(again.tokens.is_empty());
    }
}
