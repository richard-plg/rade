//! The Chromium engine of `rade browser` (PRD R72, ADR-0026): the pure
//! parts of talking Chrome DevTools Protocol over `--remote-debugging-pipe`
//! — message framing, key events for `press`, console lines, finding a
//! Chromium and its command line. The process side lives in the CLI
//! (`rade-cli/src/chromium.rs`).

use crate::browser::KeySpec;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Split complete NUL-terminated messages off the front of `buf` (the
/// pipe transport's framing); a partial message stays in `buf`.
pub fn take_frames(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    while let Some(i) = buf.iter().position(|&b| b == 0) {
        let rest = buf.split_off(i + 1);
        let mut msg = std::mem::replace(buf, rest);
        msg.pop();
        out.push(msg);
    }
    out
}

/// A command as it goes down the pipe: `{"id", "method", "params",
/// "sessionId"?}` + NUL.
pub fn frame(id: u64, method: &str, params: Value, session: Option<&str>) -> Vec<u8> {
    let mut m = json!({"id": id, "method": method, "params": params});
    if let Some(s) = session {
        m["sessionId"] = json!(s);
    }
    let mut b = serde_json::to_vec(&m).expect("serializable");
    b.push(0);
    b
}

/// CDP modifier bits: Alt 1, Ctrl 2, Meta 4, Shift 8.
pub fn modifiers(k: &KeySpec) -> u8 {
    (k.alt as u8) | (k.ctrl as u8) << 1 | (k.meta as u8) << 2 | (k.shift as u8) << 3
}

/// Windows virtual key code of a DOM `key` (Chromium needs it for the
/// default actions of Enter, Tab, Backspace, arrows …).
fn key_code(key: &str) -> u32 {
    match key {
        "Backspace" => 8,
        "Tab" => 9,
        "Enter" => 13,
        "Escape" => 27,
        " " => 32,
        "PageUp" => 33,
        "PageDown" => 34,
        "End" => 35,
        "Home" => 36,
        "ArrowLeft" => 37,
        "ArrowUp" => 38,
        "ArrowRight" => 39,
        "ArrowDown" => 40,
        "Insert" => 45,
        "Delete" => 46,
        f if f.starts_with('F') && f.len() > 1 => f[1..].parse::<u32>().map_or(0, |n| 111 + n),
        c => match c.chars().next() {
            Some(ch) if c.chars().count() == 1 && ch.is_ascii_alphanumeric() => {
                ch.to_ascii_uppercase() as u32
            }
            _ => 0,
        },
    }
}

/// `Input.dispatchKeyEvent` params for one key press (trusted input): a
/// `keyDown` carrying the text it types (none with Ctrl / Alt / Meta, so
/// shortcuts stay shortcuts; Enter types `\r`), then `keyUp`.
pub fn key_events(k: &KeySpec) -> Vec<Value> {
    let m = modifiers(k);
    let plain = !k.ctrl && !k.alt && !k.meta;
    let text = match k.key.as_str() {
        "Enter" if plain => Some("\r".to_string()),
        s if plain && s.chars().count() == 1 => Some(s.to_string()),
        _ => None,
    };
    let vk = key_code(&k.key);
    let mut down = json!({
        "type": if text.is_some() { "keyDown" } else { "rawKeyDown" },
        "key": k.key, "code": k.code, "modifiers": m,
        "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk,
    });
    if let Some(t) = text {
        down["text"] = json!(t);
        down["unmodifiedText"] = json!(t);
    }
    let up = json!({
        "type": "keyUp", "key": k.key, "code": k.code, "modifiers": m,
        "windowsVirtualKeyCode": vk, "nativeVirtualKeyCode": vk,
    });
    vec![down, up]
}

/// An object's `preview` as JSON-ish text (`{"n":1}`, `[1,"x"]`), like
/// the WebKit engine's `JSON.stringify`; `…` when Chromium cut it short.
fn preview_text(p: &Value) -> Option<String> {
    let props = p["properties"].as_array()?;
    let val = |x: &Value| -> String {
        let v = x["value"].as_str().unwrap_or("");
        match x["type"].as_str() {
            Some("string") => Value::String(v.into()).to_string(),
            Some("number" | "boolean") => v.to_string(),
            Some("object") if x["subtype"] == "null" => "null".into(),
            _ => Value::String(v.into()).to_string(),
        }
    };
    let more = if p["overflow"] == true { ",…" } else { "" };
    Some(if p["subtype"] == "array" {
        let items: Vec<String> = props.iter().map(val).collect();
        format!("[{}{more}]", items.join(","))
    } else {
        let items: Vec<String> = props
            .iter()
            .map(|x| {
                format!(
                    "{}:{}",
                    Value::String(x["name"].as_str().unwrap_or("").into()),
                    val(x)
                )
            })
            .collect();
        format!("{{{}{more}}}", items.join(","))
    })
}

/// A `Runtime.RemoteObject` as console text: strings as they are, other
/// values as JSON, plain objects and arrays from their preview, other
/// objects by their description.
fn remote_text(o: &Value) -> String {
    let plain = o["type"] == "object"
        && (o["subtype"].is_null() || o["subtype"] == "array")
        && matches!(o["className"].as_str(), None | Some("Object" | "Array"));
    match (&o["value"], o["type"].as_str()) {
        (Value::String(s), _) => s.clone(),
        (Value::Null, Some("undefined")) => "undefined".into(),
        (v, _) if !v.is_null() => v.to_string(),
        _ if plain && o["preview"].is_object() => preview_text(&o["preview"]).unwrap_or_default(),
        _ => o["description"]
            .as_str()
            .or(o["unserializableValue"].as_str())
            .unwrap_or("")
            .to_string(),
    }
}

/// Where a stack trace starts: (url, 1-based line).
fn top_frame(stack: &Value) -> (Option<String>, u64) {
    let f = &stack["callFrames"][0];
    (
        f["url"]
            .as_str()
            .filter(|u| !u.is_empty())
            .map(String::from),
        f["lineNumber"].as_u64().map_or(0, |l| l + 1),
    )
}

/// `Runtime.consoleAPICalled` → a `rade browser console` message
/// (`{"level", "text", "source", "line"}`, like the WebKit engine's).
pub fn console_message(p: &Value) -> Value {
    let level = match p["type"].as_str().unwrap_or("log") {
        "warning" => "warn",
        "error" | "assert" => "error",
        "info" => "info",
        "debug" | "trace" => "debug",
        _ => "log",
    };
    let text: Vec<String> = p["args"]
        .as_array()
        .into_iter()
        .flatten()
        .map(remote_text)
        .collect();
    let (source, line) = top_frame(&p["stackTrace"]);
    json!({"level": level, "text": text.join(" ").chars().take(4000).collect::<String>(), "source": source, "line": line})
}

/// `Runtime.exceptionThrown` → an `[error] Uncaught …` message.
pub fn exception_message(p: &Value) -> Value {
    let d = &p["exceptionDetails"];
    let what = d["exception"]["description"]
        .as_str()
        .map(String::from)
        .or_else(|| d["exception"].get("value").map(remote_text_of))
        .unwrap_or_else(|| d["text"].as_str().unwrap_or("error").to_string());
    let what = what.strip_prefix("Uncaught ").unwrap_or(&what).to_string();
    let (src, line) = match d["url"].as_str() {
        Some(u) if !u.is_empty() => (
            Some(u.to_string()),
            d["lineNumber"].as_u64().map_or(0, |l| l + 1),
        ),
        _ => top_frame(&d["stackTrace"]),
    };
    let kind = if d["text"]
        .as_str()
        .is_some_and(|t| t.contains("(in promise)"))
    {
        "Unhandled rejection: "
    } else {
        "Uncaught "
    };
    json!({"level": "error", "text": format!("{kind}{what}"), "source": src, "line": line})
}

fn remote_text_of(v: &Value) -> String {
    remote_text(&json!({ "value": v }))
}

/// Where to look for a Chromium, in order (R72.1): the configured path;
/// Playwright's Chromium (`$PLAYWRIGHT_BROWSERS_PATH`, else
/// `~/.cache/ms-playwright`, newest build first; Chrome for Testing
/// inside); then `chromium`, `chromium-browser`, `google-chrome-stable`,
/// `google-chrome`, `chrome` on `PATH`. Only the first that exists is used.
pub fn candidates(
    configured: Option<&Path>,
    home: &Path,
    playwright: Option<&Path>,
    path_env: &str,
    list_dir: &dyn Fn(&Path) -> Vec<String>,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(c) = configured {
        out.push(c.to_path_buf());
    }
    let pw = playwright
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".cache/ms-playwright"));
    let mut builds: Vec<(u32, String)> = list_dir(&pw)
        .into_iter()
        .filter_map(|n| Some((n.strip_prefix("chromium-")?.parse().ok()?, n)))
        .collect();
    builds.sort();
    for (_, b) in builds.iter().rev() {
        for sub in ["chrome-linux64/chrome", "chrome-linux/chrome"] {
            out.push(pw.join(b).join(sub));
        }
    }
    // A name in any PATH dir before the next name.
    for name in [
        "chromium",
        "chromium-browser",
        "google-chrome-stable",
        "google-chrome",
        "chrome",
    ] {
        for dir in path_env.split(':').filter(|d| !d.is_empty()) {
            out.push(Path::new(dir).join(name));
        }
    }
    out
}

/// Chromium's command line (R72.1): CDP over the pipe (no port), its own
/// profile, no first-run UI, background services off, headless unless
/// `headed` (on Wayland when `wayland`). `--no-sandbox` like Playwright: Ubuntu's AppArmor blocks the
/// user-namespace sandbox for binaries without a profile (ADR-0026).
pub fn launch_args(profile: &Path, headed: bool, wayland: bool) -> Vec<String> {
    let mut a: Vec<String> = [
        "--remote-debugging-pipe",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-background-networking",
        "--disable-component-update",
        "--disable-sync",
        "--disable-default-apps",
        "--disable-domain-reliability",
        "--disable-client-side-phishing-detection",
        "--disable-breakpad",
        "--no-pings",
        "--disable-features=Translate,OptimizationHints,MediaRouter,DialMediaRouteProvider,\
         AutofillServerCommunication,CertificateTransparencyComponentUpdater",
        "--password-store=basic",
        "--use-mock-keychain",
        "--no-sandbox",
        "--window-size=1280,800",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.push(format!("--user-data-dir={}", profile.display()));
    if headed {
        // RADE is Wayland-only (ADR-0003): the window goes to the same
        // compositor; X11 only when there is no Wayland display.
        a.push(
            if wayland {
                "--ozone-platform=wayland"
            } else {
                "--ozone-platform-hint=auto"
            }
            .into(),
        );
    } else {
        a.push("--headless=new".into());
        a.push("--hide-scrollbars".into());
        a.push("--mute-audio".into());
    }
    a.push("about:blank".into());
    a
}

/// The history entry `back` (-1) / `forward` (+1) moves to, from
/// `Page.getNavigationHistory`: its id, or None at the end.
pub fn history_step(h: &Value, delta: i64) -> Option<u64> {
    let i = h["currentIndex"].as_i64()? + delta;
    let e = h["entries"].as_array()?.get(usize::try_from(i).ok()?)?;
    e["id"].as_u64()
}

#[cfg(test)]
#[path = "cdp_tests.rs"]
mod tests;
