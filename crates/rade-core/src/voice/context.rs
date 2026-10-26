//! What the voice agent is told (R124, D8): the Pane, the tail of its
//! session (transcript, rollout or screen), git facts, the rules files, the
//! checkout's layout and the Viewer's file; plus the vocabulary hint that goes
//! to speech-to-text. Pure functions over text and paths, and one `gather`
//! that runs git.

use crate::git;
use crate::scm::RULES_FILES;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Exchanges kept from a session (a user message and the answer to it).
const EXCHANGES: usize = 6;
const MESSAGE_CHARS: usize = 2_000;
const TAIL_CHARS: usize = 16_000;
const SCREEN_LINES: usize = 120;
const STATUS_LINES: usize = 60;
const STAT_LINES: usize = 40;
const RULES_CHARS: usize = 6_000;
const LAYOUT_CHARS: usize = 3_000;
const VOCABULARY_CHARS: usize = 800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bundle {
    /// `(name, text)` in the order `Pane`, `Session` (or `Screen`), `Git`,
    /// `Rules`, `Layout`, `Viewer`; empty sections are omitted.
    pub sections: Vec<(String, String)>,
    pub vocabulary: String,
}

#[derive(Clone, Debug, Default)]
pub struct GatherInput {
    /// The Pane's cwd.
    pub dir: PathBuf,
    /// `claude` / `codex` (else the screen is the session).
    pub agent: Option<String>,
    pub model: Option<String>,
    pub session_id: Option<String>,
    pub claude_config_dir: PathBuf,
    pub codex_home: PathBuf,
    /// The Pane's screen text, read by the caller on the UI thread.
    pub screen: Option<String>,
    /// Worktree / link / base, as the Navigator names them.
    pub where_label: String,
    /// The same Tab's Viewer: its path and selected line range.
    pub viewer: Option<(PathBuf, Option<(u32, u32)>)>,
}

/// `s` cut to `max` chars, the last one `…` when something was cut.
fn cut(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max - 1).collect();
    t.push('…');
    t
}

/// Claude's project folder name: every char not `[A-Za-z0-9]` becomes `-`.
fn slug(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// `<config dir>/projects/<slug(cwd)>/<session>.jsonl`, else
/// `projects/*/<session>.jsonl`.
pub fn claude_transcript(config_dir: &Path, cwd: &Path, session: &str) -> Option<PathBuf> {
    if session.is_empty() || session.contains('/') {
        return None;
    }
    let file = format!("{session}.jsonl");
    let projects = config_dir.join("projects");
    let direct = projects.join(slug(cwd)).join(&file);
    if direct.is_file() {
        return Some(direct);
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir(&projects)
        .ok()?
        .flatten()
        .map(|e| e.path().join(&file))
        .filter(|p| p.is_file())
        .collect();
    found.sort();
    found.pop()
}

/// The newest `<codex home>/sessions/**/rollout-*<session>.jsonl`.
pub fn codex_rollout(codex_home: &Path, session: &str) -> Option<PathBuf> {
    if session.is_empty() || session.contains('/') {
        return None;
    }
    let suffix = format!("{session}.jsonl");
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    let mut stack = vec![codex_home.join("sessions")];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("rollout-") && name.ends_with(&suffix) {
                let t = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                if best.as_ref().is_none_or(|(bt, bp)| (t, &p) > (*bt, bp)) {
                    best = Some((t, p));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

/// Turns (user text, assistant text) → the last [`EXCHANGES`] exchanges as a
/// flat list, each message cut, the whole kept under [`TAIL_CHARS`] by
/// dropping the oldest.
fn tail_of(turns: impl IntoIterator<Item = (Role, String)>) -> Vec<(Role, String)> {
    let mut ex: Vec<(String, Vec<String>)> = Vec::new();
    for (role, text) in turns {
        if text.trim().is_empty() {
            continue;
        }
        match role {
            Role::User => ex.push((text, Vec::new())),
            Role::Assistant => match ex.last_mut() {
                Some((_, a)) => a.push(text),
                None => ex.push((String::new(), vec![text])),
            },
        }
    }
    let from = ex.len().saturating_sub(EXCHANGES);
    let mut out = Vec::new();
    for (u, a) in ex.into_iter().skip(from) {
        if !u.is_empty() {
            out.push((Role::User, cut(u.trim(), MESSAGE_CHARS)));
        }
        if !a.is_empty() {
            out.push((Role::Assistant, cut(a.join("\n\n").trim(), MESSAGE_CHARS)));
        }
    }
    let mut total: usize = out.iter().map(|(_, t)| t.chars().count()).sum();
    let mut drop = 0;
    while total > TAIL_CHARS && drop < out.len() {
        total -= out[drop].1.chars().count();
        drop += 1;
    }
    out.drain(..drop);
    out
}

/// The text parts of a message `content` (a string, or a list whose
/// `text` / `Text` parts count). `None` when it has a `tool_result`.
fn content_text(c: &Value) -> Option<String> {
    match c {
        Value::String(s) => Some(s.clone()),
        Value::Array(parts) => {
            let mut out = Vec::new();
            for p in parts {
                match p["type"].as_str() {
                    Some("tool_result") => return None,
                    Some("text" | "Text") => {
                        if let Some(t) = p["text"].as_str() {
                            out.push(t.to_string());
                        }
                    }
                    _ => {}
                }
            }
            Some(out.join("\n"))
        }
        _ => None,
    }
}

/// A Claude transcript's user and assistant texts. Other line types, tool
/// calls and tool results are not turns.
pub fn claude_tail(jsonl: &str) -> Vec<(Role, String)> {
    let turns = jsonl.lines().filter_map(|l| {
        let v: Value = serde_json::from_str(l).ok()?;
        let role = match v["type"].as_str()? {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            _ => return None,
        };
        if v["isMeta"].as_bool() == Some(true) {
            return None;
        }
        Some((role, content_text(&v["message"]["content"])?))
    });
    tail_of(turns.collect::<Vec<_>>())
}

/// A Codex rollout's real words: `item_completed` events of type
/// `UserMessage` / `AgentMessage` (the `response_item` copies also hold
/// injected AGENTS.md and environment texts).
pub fn codex_tail(jsonl: &str) -> Vec<(Role, String)> {
    let turns = jsonl.lines().filter_map(|l| {
        let v: Value = serde_json::from_str(l).ok()?;
        if v["type"].as_str()? != "event_msg" || v["payload"]["type"].as_str()? != "item_completed"
        {
            return None;
        }
        let item = &v["payload"]["item"];
        let role = match item["type"].as_str()? {
            "UserMessage" => Role::User,
            "AgentMessage" => Role::Assistant,
            _ => return None,
        };
        Some((role, content_text(&item["content"])?))
    });
    tail_of(turns.collect::<Vec<_>>())
}

/// The last 120 lines of the screen, trailing blank lines trimmed.
pub fn screen_tail(screen: &str) -> String {
    let lines: Vec<&str> = screen.lines().map(str::trim_end).collect();
    let end = lines
        .iter()
        .rposition(|l| !l.is_empty())
        .map_or(0, |i| i + 1);
    lines[end.saturating_sub(SCREEN_LINES)..end].join("\n")
}

/// `git ls-files` folded to directories of depth ≤ 2 with their file
/// counts (`src/checkout/ (12)`), at most 3,000 chars.
pub fn layout(ls_files: &str) -> String {
    let mut dirs: BTreeMap<String, usize> = BTreeMap::new();
    for f in ls_files.lines().filter(|l| !l.is_empty()) {
        let mut parts: Vec<&str> = f.split('/').collect();
        parts.pop();
        parts.truncate(2);
        let d = if parts.is_empty() {
            "./".to_string()
        } else {
            format!("{}/", parts.join("/"))
        };
        *dirs.entry(d).or_default() += 1;
    }
    let mut out = String::new();
    for (d, n) in dirs {
        let line = format!("{d} ({n})\n");
        if out.chars().count() + line.chars().count() > LAYOUT_CHARS {
            break;
        }
        out.push_str(&line);
    }
    out.trim_end().to_string()
}

/// Identifier-like tokens of `text` in order: `[A-Za-z_][A-Za-z0-9_.]{2,}`
/// that contain `_`, `.`, a digit or an inner capital.
fn identifiers(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        let t = cur.trim_end_matches('.');
        if t.chars().count() >= 3 && identifier_like(t) {
            out.push(t.to_string());
        }
        cur.clear();
    };
    for c in text.chars() {
        let ok = if cur.is_empty() {
            c.is_ascii_alphabetic() || c == '_'
        } else {
            c.is_ascii_alphanumeric() || c == '_' || c == '.'
        };
        if ok {
            cur.push(c);
        } else {
            flush(&mut cur, &mut out);
        }
    }
    flush(&mut cur, &mut out);
    out
}

fn identifier_like(t: &str) -> bool {
    t.chars().enumerate().any(|(i, c)| {
        c == '_' || c == '.' || c.is_ascii_digit() || (i > 0 && c.is_ascii_uppercase())
    })
}

/// Names for speech-to-text and the voice agent: the changed files' stems,
/// then the session's identifiers, newest first, deduplicated, at most 800
/// chars, space-joined.
pub fn vocabulary(tail: &[(Role, String)], changed: &[String]) -> String {
    let mut words: Vec<String> = Vec::new();
    for p in changed {
        let name = p.rsplit('/').next().unwrap_or(p);
        let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
        words.extend(identifiers(stem));
    }
    for (_, t) in tail.iter().rev() {
        let mut ids = identifiers(t);
        ids.reverse();
        words.extend(ids);
    }
    let mut out = String::new();
    let mut seen: Vec<&str> = Vec::new();
    for w in &words {
        if seen.contains(&w.as_str()) {
            continue;
        }
        let add = w.chars().count() + usize::from(!out.is_empty());
        if out.chars().count() + add > VOCABULARY_CHARS {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(w);
        seen.push(w);
    }
    out
}

fn session_text(tail: &[(Role, String)]) -> String {
    tail.iter()
        .map(|(r, t)| match r {
            Role::User => format!("User: {t}"),
            Role::Assistant => format!("Agent: {t}"),
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn first_lines(s: &str, n: usize) -> String {
    s.lines().take(n).collect::<Vec<_>>().join("\n")
}

/// Paths in `git status --short` lines (a rename's new path).
fn changed_paths(status: &str) -> Vec<String> {
    status
        .lines()
        .filter(|l| l.len() > 3)
        .map(|l| {
            let p = &l[3..];
            p.rsplit_once(" -> ")
                .map_or(p, |(_, n)| n)
                .trim_matches('"')
                .to_string()
        })
        .collect()
}

fn git_text(dir: &Path) -> (String, Vec<String>) {
    let run = |args: &[&str]| git::run(dir, args).unwrap_or_default();
    let status = run(&["status", "--short"]);
    let mut out = String::new();
    if let Some(b) = git::branch(dir) {
        out.push_str(&format!("branch: {b}\n"));
    }
    let stat = run(&["diff", "--stat", "HEAD"]);
    let log = run(&["log", "-5", "--format=%s"]);
    for (title, body) in [
        ("git status --short", first_lines(&status, STATUS_LINES)),
        ("git diff --stat HEAD", first_lines(&stat, STAT_LINES)),
        ("recent commits", log.trim().to_string()),
    ] {
        if !body.trim().is_empty() {
            out.push_str(&format!("\n{title}:\n{}\n", body.trim_end()));
        }
    }
    (out.trim().to_string(), changed_paths(&status))
}

fn rules_text(dir: &Path) -> String {
    let root = git::toplevel(dir).unwrap_or_else(|| dir.to_path_buf());
    let mut out = String::new();
    let mut left = RULES_CHARS;
    for f in RULES_FILES {
        if left == 0 {
            break;
        }
        let Ok(t) = std::fs::read_to_string(root.join(f)) else {
            continue;
        };
        let body = cut(t.trim(), left);
        left -= body.chars().count();
        out.push_str(&format!("### {f}\n{body}\n\n"));
    }
    out.trim_end().to_string()
}

/// Everything the voice agent is told; runs git in `input.dir`. Call it off
/// the UI thread.
pub fn gather(input: &GatherInput) -> Bundle {
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut pane = String::new();
    if let Some(a) = &input.agent {
        pane.push_str(&format!("agent: {a}\n"));
    }
    if let Some(m) = &input.model {
        pane.push_str(&format!("model: {m}\n"));
    }
    pane.push_str(&format!("cwd: {}\n", input.dir.display()));
    if !input.where_label.is_empty() {
        pane.push_str(&format!("where: {}\n", input.where_label));
    }
    sections.push(("Pane".into(), pane.trim_end().into()));

    let session = input.session_id.as_deref().unwrap_or("");
    let agent = input.agent.as_deref().unwrap_or("");
    let read = |p: Option<PathBuf>| p.and_then(|p| std::fs::read_to_string(p).ok());
    let tail = if agent.starts_with("claude") {
        read(claude_transcript(
            &input.claude_config_dir,
            &input.dir,
            session,
        ))
        .map(|t| claude_tail(&t))
    } else if agent.starts_with("codex") {
        read(codex_rollout(&input.codex_home, session)).map(|t| codex_tail(&t))
    } else {
        None
    }
    .filter(|t| !t.is_empty());
    match &tail {
        Some(t) => sections.push(("Session".into(), session_text(t))),
        None => {
            if let Some(s) = input.screen.as_deref().map(screen_tail) {
                sections.push(("Screen".into(), s));
            }
        }
    }

    let (git, changed) = git_text(&input.dir);
    sections.push(("Git".into(), git));
    sections.push(("Rules".into(), rules_text(&input.dir)));
    let files = git::run(&input.dir, &["ls-files"]).unwrap_or_default();
    sections.push(("Layout".into(), layout(&files)));
    if let Some((p, lines)) = &input.viewer {
        let mut v = format!("file: {}", p.display());
        if let Some((a, b)) = lines {
            v.push_str(&format!("\nselected lines: {a}-{b}"));
        }
        sections.push(("Viewer".into(), v));
    }
    sections.retain(|(_, t)| !t.trim().is_empty());
    Bundle {
        sections,
        vocabulary: vocabulary(tail.as_deref().unwrap_or(&[]), &changed),
    }
}

/// The `CONTEXT` text: `## <Section>` blocks.
pub fn render(b: &Bundle) -> String {
    b.sections
        .iter()
        .map(|(n, t)| format!("## {n}\n{t}"))
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
