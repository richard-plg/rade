//! `rade version [--check] [--json]` and the unknown-method hint (PRD R127,
//! ADR-0048, D24, D25). Local: works without the app.

use rade_core::config::Config;
use rade_core::paths::{self, Flavor};
use rade_core::rpc::Error;
use rade_core::update::{self, Cache, Channel, Version};
use serde_json::{json, Value};
use std::time::Duration;

const CLI: &str = env!("CARGO_PKG_VERSION");
const TIMEOUT: Duration = Duration::from_secs(2);

/// What `app.info` told: `None` = no app; `(None, None)` = an app older than
/// 1.2.0 (it does not know the method).
type App = Option<(Option<String>, Option<u32>)>;

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn is_unknown_method(e: &Error) -> bool {
    e.message.contains("unknown method '")
}

fn ask_app() -> Result<Value, Error> {
    crate::call_at(
        &paths::socket_path(&paths::process_env),
        "app.info",
        json!({}),
        Some(TIMEOUT),
    )
}

fn app_now() -> App {
    match ask_app() {
        Ok(v) => Some((
            v["version"].as_str().map(str::to_string),
            v["pid"].as_u64().map(|p| p as u32),
        )),
        Err(e) if is_unknown_method(&e) => Some((None, None)),
        Err(_) => None,
    }
}

/// The user's `[update] channel`.
fn channel() -> Channel {
    std::fs::read_to_string(paths::config_file(&paths::process_env))
        .ok()
        .and_then(|t| Config::parse(&t).ok())
        .unwrap_or_default()
        .update
        .channel
        .into()
}

/// The text lines and the `--json` value for a given state.
fn report(app: &App, cache: &Cache, channel: Channel, now: u64) -> (Vec<String>, Value) {
    let current = Version::parse(CLI).expect("CARGO_PKG_VERSION is a version");
    let lines = update::version_lines(
        CLI,
        app.as_ref().map(|(v, p)| (v.as_deref(), *p)),
        cache,
        &current,
        channel,
        now,
    );
    let offer = update::pick(
        &cache.releases,
        &current,
        &current,
        channel,
        cache.skipped.as_deref(),
    );
    let json = json!({
        "cli": CLI,
        "app": app.as_ref().map(|(v, p)| json!({"version": v, "pid": p})),
        "update": {
            "checked_at": cache.checked_at,
            "error": cache.error,
            "latest": offer.as_ref().map(|o| o.release.version.to_string()),
            "kind": offer.as_ref().map(|o| o.kind.as_str()),
            "security": offer.as_ref().is_some_and(|o| o.security),
            "skipped": cache.skipped,
        },
    });
    (lines, json)
}

/// D24: why `--check` sends nothing here, if it does not.
fn check_refused() -> Option<&'static str> {
    if std::env::var("RADE_UPDATE_CHECK").as_deref() == Ok("0") {
        return Some("update checks are off (RADE_UPDATE_CHECK=0)");
    }
    if Flavor::current() == Flavor::Devel && override_url().is_none() {
        return Some("a development build doesn't check for updates");
    }
    None
}

/// `RADE_UPDATE_URL`, honoured only by a Devel binary (D4).
fn override_url() -> Option<String> {
    if Flavor::current() != Flavor::Devel {
        return None;
    }
    std::env::var("RADE_UPDATE_URL")
        .ok()
        .filter(|v| !v.is_empty())
}

/// One request now (D2, ignoring D3's due rule), written to the cache.
fn check(dir: &std::path::Path) {
    let url = override_url();
    let ua = format!("rade/{CLI}");
    let target = url.as_deref().unwrap_or(update::REPO_API);
    let res = match update::curl_get(target, &ua, false, url.is_some()) {
        Ok((200, body)) => update::parse_releases(&body, url.as_deref()),
        Ok((status, body)) => Err(update::check_error(status, &body)),
        Err(e) => Err(e),
    };
    let at = now();
    let _ = Cache::update(dir, |c| {
        c.checked_at = Some(at);
        match res {
            Ok(r) => {
                c.releases = r;
                c.error = None;
            }
            Err(e) => c.error = Some(e),
        }
    });
}

pub fn run(check_now: bool, json: bool) -> Result<Value, Error> {
    let dir = paths::update_dir(&paths::process_env);
    if check_now {
        match check_refused() {
            Some(why) => eprintln!("rade: {why}"),
            None => check(&dir),
        }
    }
    let (lines, value) = report(&app_now(), &Cache::load(&dir), channel(), now());
    if json {
        println!("{value}");
    } else {
        for l in lines {
            println!("{l}");
        }
    }
    Ok(Value::Null)
}

/// D25: the second stderr line after a call failed with `unknown method`,
/// when the app is not the CLI's version (or too old to say).
pub fn app_hint() -> Option<String> {
    let app = match ask_app() {
        Ok(v) => v["version"].as_str().map(str::to_string),
        Err(e) if is_unknown_method(&e) => None,
        Err(_) => return None,
    };
    hint_line(app.as_deref())
}

fn hint_line(app: Option<&str>) -> Option<String> {
    if app == Some(CLI) {
        return None;
    }
    Some(format!(
        "rade: this rade is {CLI}, the running RADE app is {}: restart RADE to finish updating",
        app.unwrap_or("older")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r127_n_version_texts() {
        let empty = Cache::default();
        let (l, j) = report(&None, &empty, Channel::Stable, 5_000);
        assert_eq!(
            l,
            [
                format!("rade {CLI}"),
                "app not running".into(),
                "not checked yet".into()
            ]
        );
        assert_eq!(j["app"], Value::Null);
        assert_eq!(j["update"]["checked_at"], Value::Null);
        assert_eq!(j["update"]["latest"], Value::Null);
        assert_eq!(j["update"]["security"], false);

        let up = Cache {
            checked_at: Some(1_000),
            ..Cache::default()
        };
        let app: App = Some((Some(CLI.into()), Some(42)));
        let (l, j) = report(&app, &up, Channel::Stable, 1_030);
        assert_eq!(l[1], format!("app {CLI} (pid 42)"));
        assert_eq!(l[2], "up to date · checked just now");
        assert_eq!(j["app"], json!({"version": CLI, "pid": 42}));
        assert_eq!(j["update"]["checked_at"], 1_000);

        let old: App = Some((None, None));
        let (l, j) = report(&old, &up, Channel::Stable, 1_000 + 300);
        assert_eq!(
            l[1],
            "app older than 1.2.0 (no app.info) · restart RADE to finish updating"
        );
        assert_eq!(l[2], "up to date · checked 5m ago");
        assert_eq!(j["app"], json!({"version": null, "pid": null}));

        let failed = Cache {
            error: Some("GitHub answered 500".into()),
            ..Cache::default()
        };
        let (l, j) = report(&None, &failed, Channel::Stable, 7_200);
        assert_eq!(l[2], "last check failed: GitHub answered 500 · ");
        assert_eq!(j["update"]["error"], "GitHub answered 500");
    }

    #[test]
    fn r127_n_hint_line() {
        assert_eq!(hint_line(Some(CLI)), None);
        assert_eq!(
            hint_line(Some("1.0.0")).unwrap(),
            format!("rade: this rade is {CLI}, the running RADE app is 1.0.0: restart RADE to finish updating")
        );
        assert!(hint_line(None).unwrap().contains("app is older:"));
        assert!(is_unknown_method(&Error::usage("unknown method 'x.y'")));
        assert!(!is_unknown_method(&Error::usage("nope")));
    }
}
