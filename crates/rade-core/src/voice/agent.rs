//! The voice agent (R124, D7, D9): its command line, environment, the turn
//! texts of PLAN §3.5 and a reader for its output streams (Claude Code's
//! stream-json, `codex exec --json`). Nothing here starts a process.

use crate::config::{DraftLanguage, VoiceAgent, VoiceConfig, VoiceTools};
use serde_json::Value;
use std::path::Path;

/// Passed with `--append-system-prompt` (Codex: first part of each turn).
/// `{LANGUAGE}` is replaced by [`system_prompt`].
pub const SYSTEM: &str = r#"You turn a developer's spoken words into a prompt for a coding agent working in this repository.
The words come from speech recognition: expect fillers ("ừ", "à", "uh"), repeats, restarts and
misheard names. Use the CONTEXT and, only when needed, read or search files to find what they mean
(the real file, function or command). Do not change anything.

Write only the prompt, nothing before or after it:
- Line 1: the goal in one sentence.
- Then 0 to 5 lines starting with "- ": where, constraints, how to know it is done.
- Refer to code as `path:line` or `name` in backticks, using real names from the repository.
- Language: {LANGUAGE}.
- Keep the developer's intent and scope. Do not add requirements, opinions or steps they did not ask for.
- Fillers, repeats and false starts go. Keep it about as long as what they meant, never longer.
- If a part cannot be resolved, keep it and mark it [?] "<their words>"."#;

/// The turn RADE sends when a message ended on the step limit with no text.
pub const WRITE_NOW: &str = "Write the prompt now from what you have; use no more tools.";

const REWRITE: &str = "Rewrite the whole prompt with this change.";

/// [`SYSTEM`] with the language line filled in.
pub fn system_prompt(cfg: &VoiceConfig) -> String {
    let lang = match cfg.draft_language {
        DraftLanguage::Same => {
            "the language the developer spoke (keep English technical terms as they are)"
        }
        DraftLanguage::En => "English",
    };
    SYSTEM.replace("{LANGUAGE}", lang)
}

/// `claude -p …` for one voice agent process (D7); `append_prompt` is
/// [`system_prompt`].
pub fn claude_argv(cfg: &VoiceConfig, append_prompt: &str) -> Vec<String> {
    let model = if cfg.model.trim().is_empty() {
        "haiku"
    } else {
        cfg.model.trim()
    };
    let tools = match cfg.tools {
        VoiceTools::Read => "Read,Grep,Glob",
        VoiceTools::None => "",
    };
    let mut v: Vec<String> = [
        "claude",
        "-p",
        "--model",
        model,
        "--tools",
        tools,
        "--max-turns",
        &cfg.max_turns().to_string(),
        "--no-session-persistence",
        "--setting-sources",
        "",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--include-partial-messages",
        "--verbose",
        "--append-system-prompt",
    ]
    .map(String::from)
    .to_vec();
    v.push(append_prompt.to_string());
    v
}

/// `codex exec …`, one process per turn; the prompt goes on stdin.
pub fn codex_argv(cfg: &VoiceConfig) -> Vec<String> {
    let mut v: Vec<String> = [
        "codex",
        "exec",
        "--ephemeral",
        "--sandbox",
        "read-only",
        "--skip-git-repo-check",
        "--json",
    ]
    .map(String::from)
    .to_vec();
    if !cfg.model.trim().is_empty() {
        v.push("-m".into());
        v.push(cfg.model.trim().into());
    }
    v.push("-".into());
    v
}

/// The command to run: `agent_command` through `sh -c`, else the built one.
pub fn command(cfg: &VoiceConfig) -> Vec<String> {
    if !cfg.agent_command.trim().is_empty() {
        return vec!["sh".into(), "-c".into(), cfg.agent_command.clone()];
    }
    match cfg.agent {
        VoiceAgent::Claude => claude_argv(cfg, &system_prompt(cfg)),
        VoiceAgent::Codex => codex_argv(cfg),
    }
}

/// The Pane's env without any `RADE_*` key, so `rade status --hook` is a
/// no-op in the voice agent and no Pane's status changes.
pub fn env_for(base: &[(String, String)]) -> Vec<(String, String)> {
    base.iter()
        .filter(|(k, _)| !k.starts_with("RADE_"))
        .cloned()
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turn<'a> {
    /// The first message of a process: the context, then the words;
    /// `draft` when an earlier process left one.
    First {
        context: &'a str,
        draft: Option<&'a str>,
    },
    /// A revision in the same process; `draft` as it is in the text box.
    Revise { draft: &'a str },
}

/// The user message of a turn (Claude, §3.5).
pub fn turn_message(turn: Turn, words: &str) -> String {
    match turn {
        Turn::First { context, draft } => {
            let draft = draft.map_or(String::new(), |d| format!("CURRENT DRAFT\n{d}\n\n"));
            format!("CONTEXT\n{context}\n\n{draft}SPEECH\n{words}")
        }
        Turn::Revise { draft } => {
            format!("REVISE\nCURRENT DRAFT\n{draft}\n\nSPEECH\n{words}\n\n{REWRITE}")
        }
    }
}

/// Codex's stdin: every turn starts with the system text and the context; a
/// revision adds the current draft.
pub fn codex_prompt(system: &str, context: &str, draft: Option<&str>, words: &str) -> String {
    let mut s = format!(
        "{system}\n\n{}",
        turn_message(Turn::First { context, draft }, words)
    );
    if draft.is_some() {
        s.push_str("\n\n");
        s.push_str(REWRITE);
    }
    s
}

/// One stream-json user line for Claude's stdin (no trailing newline).
pub fn claude_stdin_line(text: &str) -> String {
    serde_json::json!({"type": "user", "message": {"role": "user", "content": text}}).to_string()
}

#[derive(Clone, Debug, PartialEq)]
pub enum AgentEvent {
    /// Streamed draft text.
    Delta(String),
    /// A tool use, as the activity line shows it.
    Tool {
        label: String,
    },
    /// The turn's text (code fence stripped), replacing the streamed text.
    Final {
        text: String,
    },
    /// The message ended on the step limit with no text (`error_max_turns`).
    MaxTurns,
    Error {
        text: String,
    },
}

/// A text wrapped in one code fence, unwrapped.
pub fn strip_fence(s: &str) -> String {
    let t = s.trim();
    if t.len() > 6 && t.starts_with("```") && t.ends_with("```") {
        let inner = &t[3..t.len() - 3];
        // The opener's language tag is the rest of its line.
        return match inner.split_once('\n') {
            Some((first, rest)) if !first.contains(' ') => rest.trim().to_string(),
            _ => inner.trim().to_string(),
        };
    }
    t.to_string()
}

/// `path` relative to `cwd` when it is inside it.
fn relative(path: &str, cwd: &Path) -> String {
    Path::new(path)
        .strip_prefix(cwd)
        .map_or_else(|_| path.to_string(), |p| p.display().to_string())
}

/// The activity line of a Claude tool use (D9).
pub fn activity(tool: &str, input: &Value, cwd: &Path) -> String {
    let s = |k: &str| input[k].as_str().unwrap_or("");
    match tool {
        "Read" if !s("file_path").is_empty() => format!("Read {}", relative(s("file_path"), cwd)),
        "Grep" if !s("pattern").is_empty() => format!("Searched “{}”", s("pattern")),
        "Glob" if !s("pattern").is_empty() => format!("Listed {}", s("pattern")),
        _ => tool.to_string(),
    }
}

/// Events of one line of Claude's stream-json output. Text deltas come from
/// `stream_event`s, tool uses from the complete `assistant` messages (their
/// `input` is whole there), the end from `result`.
pub fn parse_claude_line(line: &str, cwd: &Path) -> Vec<AgentEvent> {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    match v["type"].as_str() {
        Some("stream_event") => {
            let d = &v["event"]["delta"];
            match (v["event"]["type"].as_str(), d["type"].as_str()) {
                (Some("content_block_delta"), Some("text_delta")) => d["text"]
                    .as_str()
                    .filter(|t| !t.is_empty())
                    .map(|t| vec![AgentEvent::Delta(t.into())])
                    .unwrap_or_default(),
                _ => Vec::new(),
            }
        }
        Some("assistant") => v["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| p["type"] == "tool_use")
            .map(|p| AgentEvent::Tool {
                label: activity(p["name"].as_str().unwrap_or("Tool"), &p["input"], cwd),
            })
            .collect(),
        Some("result") => {
            let text = v["result"].as_str().unwrap_or("");
            if v["subtype"] == "error_max_turns" {
                vec![AgentEvent::MaxTurns]
            } else if v["is_error"].as_bool() == Some(true) {
                let t = if text.is_empty() {
                    v["subtype"].as_str().unwrap_or("error")
                } else {
                    text
                };
                vec![AgentEvent::Error { text: t.into() }]
            } else {
                vec![AgentEvent::Final {
                    text: strip_fence(text),
                }]
            }
        }
        _ => Vec::new(),
    }
}

/// `/usr/bin/zsh -lc 'cat x'` → `cat x`.
fn unwrap_shell(command: &str) -> String {
    let c = command.trim();
    let mut words = c.splitn(3, ' ');
    let (Some(shell), Some(flag), Some(rest)) = (words.next(), words.next(), words.next()) else {
        return c.to_string();
    };
    let is_shell = shell
        .rsplit('/')
        .next()
        .is_some_and(|s| matches!(s, "sh" | "bash" | "zsh" | "dash" | "fish"));
    if !is_shell || !(flag == "-c" || flag == "-lc" || flag == "-ic") {
        return c.to_string();
    }
    let rest = rest.trim();
    if let Some(q) = rest.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
        return q.replace("'\\''", "'");
    }
    if let Some(q) = rest.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        return q.replace("\\\"", "\"");
    }
    rest.to_string()
}

/// Events of one line of `codex exec --json`. `last` holds the newest
/// `agent_message` text; `turn.completed` answers with it. Interim
/// messages are not streamed: Codex has no text deltas.
pub fn parse_codex_line(line: &str, last: &mut Option<String>) -> Vec<AgentEvent> {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    let item = &v["item"];
    match v["type"].as_str() {
        Some("item.started") if item["type"] == "command_execution" => {
            vec![AgentEvent::Tool {
                label: format!(
                    "Ran {}",
                    unwrap_shell(item["command"].as_str().unwrap_or(""))
                ),
            }]
        }
        Some("item.completed") if item["type"] == "agent_message" => {
            *last = item["text"].as_str().map(String::from);
            Vec::new()
        }
        Some("turn.completed") => vec![AgentEvent::Final {
            text: strip_fence(last.take().as_deref().unwrap_or("")),
        }],
        Some("turn.failed") => vec![AgentEvent::Error {
            text: v["error"]["message"]
                .as_str()
                .unwrap_or("turn failed")
                .into(),
        }],
        Some("error") => vec![AgentEvent::Error {
            text: v["message"].as_str().unwrap_or("error").into(),
        }],
        _ => Vec::new(),
    }
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
