//! Detailed limits (R109.5, ADR-0035 decision 9, design §10.5 option A):
//! Claude's per-model weekly limits exist only behind the undocumented
//! usage endpoint. Off by default, opt-in per account (`detailed_limits =
//! true`); at most one request per [`MIN_INTERVAL_S`] per login; the login
//! token is read from the account's `config_dir` at call time, handed to
//! `curl` on stdin (never argv, env, a log, state or the store) and dropped;
//! an unexpected response turns the option off.
//!
//! Tests never call the real endpoint: `RADE_CLAUDE_USAGE_URL` points at a
//! fake server and is honoured only in test mode (ADR-0021).

use super::{time_value, Limit};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The production endpoint (never used by tests).
pub const ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";
/// The test-mode override.
pub const URL_ENV: &str = "RADE_CLAUDE_USAGE_URL";
/// One request per login per this many seconds.
pub const MIN_INTERVAL_S: i64 = 300;
/// curl's whole-request budget.
const TIMEOUT_S: u64 = 10;

/// The URL to ask: `RADE_CLAUDE_USAGE_URL` in test mode, else [`ENDPOINT`].
pub fn endpoint(env: crate::paths::Env, test_mode: bool) -> String {
    match env(URL_ENV) {
        Some(u) if test_mode => u,
        _ => ENDPOINT.into(),
    }
}

/// The weekly buckets only the endpoint has (the statusline already gives
/// `five_hour`, `seven_day` and the spend limit).
const BUCKETS: [(&str, &str); 5] = [
    ("seven_day_opus", "Opus"),
    ("seven_day_sonnet", "Sonnet"),
    ("seven_day_oauth_apps", "OAuth apps"),
    ("seven_day_cowork", "Cowork"),
    ("seven_day_overage_included", "Overage included"),
];
const KNOWN: [&str; 3] = ["five_hour", "seven_day", "model_scoped"];

/// A bucket `{utilization, resets_at}`: `Ok(None)` for `null`, `Err` for any
/// other shape.
/// A bucket's used % and reset time.
type Bucket = (Option<f64>, Option<i64>);

fn bucket(v: &Value) -> Result<Option<Bucket>, String> {
    match v {
        Value::Null => Ok(None),
        Value::Object(o) if o.contains_key("utilization") => {
            let u = &o["utilization"];
            if !(u.is_number() || u.is_null()) {
                return Err("`utilization` is not a number".into());
            }
            Ok(Some((u.as_f64(), o.get("resets_at").and_then(time_value))))
        }
        _ => Err("a bucket is not {utilization, resets_at}".into()),
    }
}

/// The per-model limits of an endpoint response at time `t`. `Err` (the
/// format changed) when it is not a JSON object holding at least one known
/// bucket in the known shape.
pub fn parse_response(text: &str, t: i64) -> Result<Vec<Limit>, String> {
    let v: Value = serde_json::from_str(text).map_err(|_| "not JSON".to_string())?;
    let o = v.as_object().ok_or("not a JSON object")?;
    let known = BUCKETS
        .iter()
        .map(|(k, _)| *k)
        .chain(KNOWN)
        .filter(|k| o.contains_key(*k))
        .count();
    if known == 0 {
        return Err("no known limit in the response".into());
    }
    let weekly = |id: &str, name: &str, used: Option<f64>, reset: Option<i64>| Limit {
        limit_id: id.into(),
        group: "Weekly".into(),
        name: name.into(),
        window_minutes: Some(10_080),
        used_percent: used,
        resets_at: reset,
        source: "detailed".into(),
        t,
        ..Limit::default()
    };
    for k in KNOWN.iter().take(2) {
        if let Some(b) = o.get(*k) {
            bucket(b)?;
        }
    }
    let mut out = Vec::new();
    for (id, name) in BUCKETS {
        if let Some(b) = o.get(id) {
            if let Some((Some(used), reset)) = bucket(b)? {
                out.push(weekly(id, name, Some(used), reset));
            }
        }
    }
    match o.get("model_scoped") {
        None | Some(Value::Null) => {}
        Some(Value::Array(items)) => {
            for m in items {
                let name = m["display_name"]
                    .as_str()
                    .ok_or("model_scoped without display_name")?;
                if let Some((Some(used), reset)) = bucket(&serde_json::json!({
                    "utilization": m["utilization"], "resets_at": m["resets_at"]
                }))? {
                    if !out.iter().any(|l| l.name == name) {
                        out.push(weekly("model_scoped", name, Some(used), reset));
                    }
                }
            }
        }
        Some(_) => return Err("`model_scoped` is not a list".into()),
    }
    Ok(out)
}

/// The login token, read at call time. No `Debug` / `Display` of the value:
/// it can never be formatted into a message or a log.
pub struct Token(String);

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(…)")
    }
}

impl Drop for Token {
    fn drop(&mut self) {
        // Best effort: overwrite the bytes before they are freed.
        let mut v = std::mem::take(&mut self.0).into_bytes();
        v.iter_mut().for_each(|b| *b = 0);
        std::hint::black_box(&v);
    }
}

/// The account's OAuth access token from `<config_dir>/.credentials.json`
/// (`claudeAiOauth.accessToken`); `None` without a login.
pub fn read_token(config_dir: &Path) -> Option<Token> {
    let text = std::fs::read_to_string(config_dir.join(".credentials.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v["claudeAiOauth"]["accessToken"]
        .as_str()
        .filter(|s| !s.is_empty() && !s.contains(['\n', '\r', '"', '\\']))
        .map(|s| Token(s.to_string()))
}

/// What a fetch answered: the HTTP status and body.
pub type Fetched = Result<(u16, String), String>;

/// GET `url` with the token through `curl`; the header goes on curl's stdin
/// (`-K -`), so it is in no argv or environment. Errors never hold the token.
pub fn curl_fetch(url: &str, token: &Token) -> Fetched {
    if url.contains(['\n', '\r', '"']) {
        return Err("bad usage URL".into());
    }
    let mut child = Command::new("curl")
        .args([
            "-sS",
            "--noproxy",
            "127.0.0.1,localhost,::1",
            "--max-time",
            &TIMEOUT_S.to_string(),
            "-K",
            "-",
            "-w",
            "\n%{http_code}",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    {
        let mut stdin = child.stdin.take().ok_or("curl: no stdin")?;
        let mut cfg = format!(
            "url = \"{url}\"\nheader = \"Authorization: Bearer {}\"\nheader = \"anthropic-beta: oauth-2025-04-20\"\nheader = \"User-Agent: rade\"\n",
            token.0
        )
        .into_bytes();
        let w = stdin.write_all(&cfg);
        cfg.iter_mut().for_each(|b| *b = 0);
        w.map_err(|e| format!("curl: {e}"))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).replace(&token.0, "…");
        return Err(err.trim().chars().take(200).collect());
    }
    let (body, code) = stdout.rsplit_once('\n').unwrap_or(("", stdout.as_str()));
    let code = code
        .trim()
        .parse::<u16>()
        .map_err(|_| "curl: no HTTP status".to_string())?;
    Ok((code, body.to_string()))
}

/// The outcome of one Detailed limits refresh.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Asked less than [`MIN_INTERVAL_S`] ago: nothing sent.
    NotDue,
    /// No login token in the config dir (nothing sent; the option stays).
    NoLogin,
    Limits(Vec<Limit>),
    /// A transient failure (network, 429, 5xx): the option stays on.
    Failed(String),
    /// An unexpected response: turn the option off and say so (R109.5).
    Disable(String),
}

/// When each login was last asked (per canonical `config_dir`).
#[derive(Debug, Default)]
pub struct Gate {
    last: HashMap<PathBuf, i64>,
}

impl Gate {
    /// `true` (and noted) when `dir` was not asked in the last
    /// [`MIN_INTERVAL_S`].
    pub fn take(&mut self, dir: &Path, now: i64) -> bool {
        let key = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        match self.last.get(&key) {
            Some(&t) if now - t < MIN_INTERVAL_S && now >= t => false,
            _ => {
                self.last.insert(key, now);
                true
            }
        }
    }
}

/// One refresh for an account with Detailed limits on: gate, read the token,
/// fetch (injected: [`curl_fetch`] in the app, never the real endpoint in
/// tests), parse.
pub fn refresh(
    config_dir: &Path,
    url: &str,
    gate: &mut Gate,
    now: i64,
    fetch: &dyn Fn(&str, &Token) -> Fetched,
) -> Outcome {
    if !gate.take(config_dir, now) {
        return Outcome::NotDue;
    }
    let Some(token) = read_token(config_dir) else {
        return Outcome::NoLogin;
    };
    let answer = fetch(url, &token);
    drop(token);
    match answer {
        Err(e) => Outcome::Failed(e),
        Ok((200, body)) => match parse_response(&body, now) {
            Ok(l) => Outcome::Limits(l),
            Err(e) => Outcome::Disable(format!("unexpected response: {e}")),
        },
        Ok((code, _)) if code == 429 || code >= 500 => Outcome::Failed(format!("HTTP {code}")),
        Ok((code, _)) => Outcome::Disable(format!("unexpected response: HTTP {code}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    const FAKE_TOKEN: &str = "sk-ant-oat01-FAKE-TEST-TOKEN-0123456789";
    const GOOD: &str = r#"{"five_hour":{"utilization":62.0,"resets_at":"2026-10-23T15:12:00+00:00"},"seven_day":{"utilization":41.0,"resets_at":"2026-10-26T09:00:00+00:00"},"seven_day_oauth_apps":{"utilization":3.0,"resets_at":"2026-10-26T09:00:00+00:00"},"seven_day_opus":{"utilization":58.0,"resets_at":"2026-10-26T09:00:00+00:00"},"seven_day_sonnet":{"utilization":22.0,"resets_at":"2026-10-26T09:00:00+00:00"},"seven_day_cowork":null,"extra_usage":{"is_enabled":false},"new_field":1}"#;

    /// A local fake of the endpoint: answers `bodies` in turn (the last one
    /// repeats), records each request's Authorization header.
    fn fake_server(bodies: Vec<&'static str>) -> (String, Arc<Mutex<Vec<String>>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/api/oauth/usage", l.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        std::thread::spawn(move || {
            for (i, stream) in l.incoming().enumerate() {
                let Ok(mut stream) = stream else { break };
                let mut r = BufReader::new(stream.try_clone().unwrap());
                let mut auth = String::new();
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.strip_prefix("Authorization: ") {
                        auth = v.trim().to_string();
                    }
                }
                s.lock().unwrap().push(auth);
                let body = bodies[i.min(bodies.len() - 1)];
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.read(&mut [0u8; 1]);
            }
        });
        (url, seen)
    }

    fn login(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rade-detailed-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join(".credentials.json"),
            format!(r#"{{"claudeAiOauth":{{"accessToken":"{FAKE_TOKEN}","refreshToken":"r","expiresAt":1}}}}"#),
        )
        .unwrap();
        d
    }

    #[test]
    fn r109_h_detailed_limits_gate_parse_and_disable() {
        let (url, seen) = fake_server(vec![GOOD, r#"{"utilization_v2":{"buckets":[]}}"#]);
        let dir = login("h");
        let mut gate = Gate::default();
        let now = 1_792_764_000;
        let fetch = |u: &str, t: &Token| curl_fetch(u, t);
        // 3 refreshes within 5 minutes: 1 request.
        let first = refresh(&dir, &url, &mut gate, now, &fetch);
        let Outcome::Limits(l) = first else {
            panic!("{first:?}")
        };
        assert_eq!(
            refresh(&dir, &url, &mut gate, now + 60, &fetch),
            Outcome::NotDue
        );
        assert_eq!(
            refresh(&dir, &url, &mut gate, now + 299, &fetch),
            Outcome::NotDue
        );
        assert_eq!(seen.lock().unwrap().len(), 1);
        assert_eq!(seen.lock().unwrap()[0], format!("Bearer {FAKE_TOKEN}"));
        let get = |n: &str| l.iter().find(|x| x.name == n).and_then(|x| x.used_percent);
        assert_eq!(get("Opus"), Some(58.0));
        assert_eq!(get("Sonnet"), Some(22.0));
        assert_eq!(get("OAuth apps"), Some(3.0));
        assert_eq!(get("Cowork"), None);
        assert!(l
            .iter()
            .all(|x| x.source == "detailed" && x.group == "Weekly"));
        assert_eq!(l[0].resets_at, Some(1_793_005_200));
        // Nothing derived from the answer holds the token.
        assert!(!format!("{l:?}").contains(FAKE_TOKEN));
        assert!(!serde_json::to_string(&l).unwrap().contains(FAKE_TOKEN));

        // The next answer has an unexpected format: turn the option off.
        let out = refresh(&dir, &url, &mut gate, now + 301, &fetch);
        let Outcome::Disable(msg) = out else {
            panic!("{out:?}")
        };
        assert!(msg.contains("unexpected response") && !msg.contains(FAKE_TOKEN));
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[test]
    fn no_login_url_override_only_in_test_mode_and_http_codes() {
        let empty = std::env::temp_dir().join(format!("rade-detailed-{}-none", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        let mut gate = Gate::default();
        let never = |_: &str, _: &Token| -> Fetched { panic!("must not fetch") };
        assert_eq!(
            refresh(&empty, "http://x", &mut gate, 0, &never),
            Outcome::NoLogin
        );

        let env = |k: &str| (k == URL_ENV).then(|| "http://127.0.0.1:9/u".to_string());
        assert_eq!(endpoint(&env, true), "http://127.0.0.1:9/u");
        assert_eq!(endpoint(&env, false), ENDPOINT);
        assert_eq!(endpoint(&|_: &str| None, true), ENDPOINT);

        let dir = login("codes");
        let answer = |code: u16| move |_: &str, _: &Token| -> Fetched { Ok((code, "{}".into())) };
        let mut g = Gate::default();
        assert!(matches!(
            refresh(&dir, "u", &mut g, 0, &answer(503)),
            Outcome::Failed(_)
        ));
        assert!(matches!(
            refresh(&dir, "u", &mut g, 400, &answer(401)),
            Outcome::Disable(_)
        ));
        let net = |_: &str, _: &Token| -> Fetched { Err("connection refused".into()) };
        assert!(matches!(
            refresh(&dir, "u", &mut g, 800, &net),
            Outcome::Failed(_)
        ));
        assert_eq!(format!("{:?}", read_token(&dir).unwrap()), "Token(…)");
    }

    #[test]
    fn response_shapes() {
        assert!(parse_response("[]", 0).is_err());
        assert!(parse_response("{}", 0).is_err());
        assert!(parse_response(r#"{"seven_day_opus":5}"#, 0).is_err());
        assert!(parse_response(r#"{"five_hour":{"pct":5}}"#, 0).is_err());
        let l = parse_response(
            r#"{"five_hour":null,"model_scoped":[{"display_name":"Fable","utilization":12,"resets_at":null}]}"#,
            0,
        )
        .unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(
            (l[0].limit_id.as_str(), l[0].name.as_str()),
            ("model_scoped", "Fable")
        );
        assert!(l[0].per_model());
    }
}
