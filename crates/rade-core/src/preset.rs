//! Agent Presets (PRD R16.1–R16.2): how to launch, prompt, resume, and
//! observe each kind of Agent. Facts: docs/internals/research/agent-integration.md.
//! Built-ins are overridable and extensible in `config.toml` `[agents.<name>]`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// How a prompt given to `rade spawn` reaches the Agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptMode {
    /// On the command line (after `prompt_flag`, if any).
    Arg,
    /// Typed into the Terminal (bracketed paste + Enter) once it settles.
    Type,
}

/// Which hook mechanism `rade setup` installs for this Agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HookKind {
    Claude,
    Codex,
    /// Google Antigravity CLI (replaced Gemini CLI on 2026-05-19).
    Agy,
    Opencode,
    None,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    /// Launch command (run by the shell, so it may carry arguments).
    pub command: String,
    pub prompt: PromptMode,
    /// Flag before the prompt argument (`-i`, `--prompt`); none = positional.
    pub prompt_flag: Option<String>,
    /// Flag that makes the Agent use a RADE-chosen session id (Claude Code).
    /// None = the Agent reports its id from a session-start hook.
    pub session_id_flag: Option<String>,
    /// Resume command; `{session_id}` is replaced (Session Restore, R11.2).
    pub resume: Option<String>,
    pub hooks: HookKind,
    /// Accepts a pasted image file path as an image (ADR-0004).
    pub image_paths: bool,
    /// Bytes to send for Shift+Enter if the kitty keyboard protocol is not
    /// enough (R6.2). Reserved: parsed but not applied in v1 — every built-in
    /// gets Shift+Enter natively from Ghostty (decision in docs/internals/checks/R6.md).
    pub shift_enter: Option<String>,
    /// Environment variables that select the Agent's config (Claude Code
    /// `CLAUDE_CONFIG_DIR`); recorded from a hand-started Agent and set again
    /// when it is resumed (R11.2).
    pub config_env: Vec<String>,
    /// Flags that take a value (`--model opus`), so a recorded invocation's
    /// positional prompt can be told apart from flag values.
    pub value_flags: Vec<String>,
    /// Session/prompt flags dropped from a recorded invocation on resume
    /// (with their value if they are in `value_flags`). `session_id_flag`
    /// and `prompt_flag` are always dropped.
    pub session_flags: Vec<String>,
    /// Flag that picks the model (`--model`, `-m`). Unused since 1.0.0
    /// (R101); the key still loads so old config files keep working.
    pub model_flag: Option<String>,
    /// The user's own icon for this agent (R115.5): an SVG or PNG drawn in
    /// its own colours instead of RADE's glyph; `~/` is expanded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<PathBuf>,
}

/// How the running Agent was really started (R11.2): its argv and the
/// Preset's `config_env` variables that were set. Reported by the
/// session-start hook (`rade status --hook`), saved on the Pane.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Invocation {
    pub argv: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// `[agents.<name>]` in config.toml: every field optional; a new name needs `command`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct PresetOverride {
    pub command: Option<String>,
    pub prompt: Option<PromptMode>,
    pub prompt_flag: Option<String>,
    pub session_id_flag: Option<String>,
    pub resume: Option<String>,
    pub hooks: Option<HookKind>,
    pub image_paths: Option<bool>,
    pub shift_enter: Option<String>,
    pub config_env: Option<Vec<String>>,
    pub value_flags: Option<Vec<String>>,
    pub session_flags: Option<Vec<String>>,
    pub model_flag: Option<String>,
    pub icon: Option<String>,
}

fn words(s: &str) -> Vec<String> {
    s.split_whitespace().map(String::from).collect()
}

fn preset(
    command: &str,
    prompt_flag: Option<&str>,
    session_id_flag: Option<&str>,
    resume: &str,
    hooks: HookKind,
    image_paths: bool,
) -> Preset {
    Preset {
        command: command.into(),
        prompt: PromptMode::Arg,
        prompt_flag: prompt_flag.map(Into::into),
        session_id_flag: session_id_flag.map(Into::into),
        resume: Some(resume.into()),
        hooks,
        image_paths,
        shift_enter: None,
        config_env: vec![],
        value_flags: vec![],
        session_flags: vec![],
        model_flag: None,
        icon: None,
    }
}

/// Built-in Presets. Antigravity (`agy`) has no hook for "needs input", so
/// its `waiting` status must come from the fallbacks (M6).
pub fn builtins() -> BTreeMap<String, Preset> {
    BTreeMap::from([
        (
            "claude".into(),
            Preset {
                config_env: words("CLAUDE_CONFIG_DIR"),
                // `claude --help` 2.1.285: `<value>` and `[value]` options.
                value_flags: words(
                    "--add-dir --agent --agents --allowedTools --allowed-tools \
                     --append-system-prompt --autocompact --betas --client-data-url --cloud \
                     -d --debug --debug-file --disallowedTools --disallowed-tools --effort \
                     --environment --fallback-model --file --from-pr --input-format \
                     --json-schema --max-budget-usd --mcp-config --model -n --name \
                     --output-format --permission-mode --permission-prompts --plugin-dir \
                     --plugin-url --prompt-suggestions --remote-control \
                     --remote-control-session-name-prefix -r --resume --session-id \
                     --setting-sources --settings --system-prompt --system-prompt-snapshot \
                     --teleport --tools -w --worktree",
                ),
                session_flags: words(
                    "--session-id -r --resume -c --continue -p --print --fork-session \
                     --from-pr --teleport",
                ),
                model_flag: Some("--model".into()),
                ..preset(
                    "claude",
                    None,
                    Some("--session-id"),
                    "claude --resume {session_id}",
                    HookKind::Claude,
                    true,
                )
            },
        ),
        (
            "codex".into(),
            // --no-daemon: Codex otherwise runs hooks in a shared app-server
            // daemon that lacks this Pane's RADE_* environment.
            Preset {
                config_env: words("CODEX_HOME"),
                // `codex --help` 0.159.2. `resume <id>` is positional, so
                // it is dropped with the prompt.
                value_flags: words(
                    "-c --config --enable --disable --remote --remote-auth-token-env \
                     -i --image -m --model --local-provider -p --profile -s --sandbox \
                     -C --cd --add-dir -a --ask-for-approval",
                ),
                session_flags: words("--last --all --include-non-interactive -i --image"),
                model_flag: Some("-m".into()),
                ..preset(
                    "codex --no-daemon",
                    None,
                    None,
                    "codex --no-daemon resume {session_id}",
                    HookKind::Codex,
                    true,
                )
            },
        ),
        (
            "agy".into(),
            Preset {
                // No config-dir variable is documented (agy 1.2.14).
                // `agy --help`: Go flags; these take a value.
                value_flags: words(
                    "--add-dir --agent --conversation --effort -i --prompt-interactive \
                     --input-format --json-schema --log-file --mode --model --output-format \
                     -p --print --prompt --print-timeout --source",
                ),
                session_flags: words(
                    "-i --prompt-interactive -p --print --prompt --conversation -c --continue",
                ),
                model_flag: Some("--model".into()),
                ..preset(
                    "agy",
                    Some("-i"),
                    None,
                    "agy --conversation {session_id}",
                    HookKind::Agy,
                    false, // image-path attachment UNVERIFIED
                )
            },
        ),
        (
            "opencode".into(),
            Preset {
                config_env: words("OPENCODE_CONFIG OPENCODE_CONFIG_DIR"),
                // `opencode --help`. `run <message..>` is positional.
                value_flags: words(
                    "--log-level --port --hostname --mdns-domain --cors -m --model \
                     -s --session --prompt --agent --replay-limit",
                ),
                session_flags: words("-s --session -c --continue --fork --prompt"),
                model_flag: Some("-m".into()),
                ..preset(
                    "opencode",
                    Some("--prompt"),
                    None,
                    "opencode --session {session_id}",
                    HookKind::Opencode,
                    true,
                )
            },
        ),
    ])
}

/// Built-ins merged with config overrides.
pub fn registry(
    overrides: &BTreeMap<String, PresetOverride>,
) -> Result<BTreeMap<String, Preset>, String> {
    let mut all = builtins();
    for (name, o) in overrides {
        let base = match all.remove(name) {
            Some(p) => p,
            None => Preset {
                command: o
                    .command
                    .clone()
                    .ok_or_else(|| format!("[agents.{name}] needs `command`"))?,
                prompt: PromptMode::Arg,
                prompt_flag: None,
                session_id_flag: None,
                resume: None,
                hooks: HookKind::None,
                image_paths: false,
                shift_enter: None,
                config_env: vec![],
                value_flags: vec![],
                session_flags: vec![],
                model_flag: None,
                icon: None,
            },
        };
        let o = o.clone();
        all.insert(
            name.clone(),
            Preset {
                command: o.command.unwrap_or(base.command),
                prompt: o.prompt.unwrap_or(base.prompt),
                prompt_flag: o.prompt_flag.or(base.prompt_flag),
                session_id_flag: o.session_id_flag.or(base.session_id_flag),
                resume: o.resume.or(base.resume),
                hooks: o.hooks.unwrap_or(base.hooks),
                image_paths: o.image_paths.unwrap_or(base.image_paths),
                shift_enter: o.shift_enter.or(base.shift_enter),
                config_env: o.config_env.unwrap_or(base.config_env),
                value_flags: o.value_flags.unwrap_or(base.value_flags),
                session_flags: o.session_flags.unwrap_or(base.session_flags),
                model_flag: o.model_flag.or(base.model_flag),
                icon: o.icon.as_deref().map(expand_home).or(base.icon),
            },
        );
    }
    Ok(all)
}

/// `~/x` → `$HOME/x` (an `icon` path, R115.5).
fn expand_home(p: &str) -> PathBuf {
    match (p.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(p),
    }
}

/// The Preset whose program `argv` runs (R115.4), by the hook's rule:
/// argv[0]'s basename, or an interpreter's first non-flag argument's (not
/// after `-c`), a script extension (`.js`, `.mjs`, `.cjs`, `.py`, `.sh`)
/// ignored. Several Presets on one program: the one named like it. A
/// Preset whose command starts with an interpreter (`bash x.sh`) is never
/// detected: every script would match it.
pub fn detect(presets: &BTreeMap<String, Preset>, argv: &[String]) -> Option<String> {
    let stem = |a: &str| {
        let b = a.rsplit('/').next().unwrap_or(a);
        [".js", ".mjs", ".cjs", ".py", ".sh"]
            .iter()
            .find_map(|x| b.strip_suffix(x))
            .unwrap_or(b)
            .to_string()
    };
    let first = argv.first()?;
    let script = argv
        .iter()
        .skip(1)
        .position(|a| !a.starts_with('-'))
        .map(|i| i + 1)
        .filter(|i| !argv[1..*i].iter().any(|a| a == "-c"))
        .map(|i| stem(&argv[i]));
    const INTERPRETERS: &[&str] = &[
        "sh", "bash", "zsh", "fish", "dash", "env", "node", "nodejs", "bun", "deno", "npx",
        "python", "python3", "uv", "uvx",
    ];
    let runs = |p: &Preset| {
        let bin = stem(p.binary());
        !INTERPRETERS.contains(&bin.as_str())
            && (stem(first) == bin || script.as_deref() == Some(bin.as_str()))
    };
    let hits: Vec<&String> = presets
        .iter()
        .filter(|(_, p)| runs(p))
        .map(|(n, _)| n)
        .collect();
    hits.iter()
        .find(|n| presets[n.as_str()].binary() == n.as_str())
        .or(hits.first())
        .map(|n| n.to_string())
}

impl Preset {
    /// Shell command that starts the Agent. The prompt is included only in
    /// `arg` mode; in `type` mode the caller types it after start.
    pub fn launch(&self, prompt: Option<&str>, session_id: Option<&str>) -> String {
        let mut cmd = self.command.clone();
        if let (Some(flag), Some(id)) = (&self.session_id_flag, session_id) {
            cmd += &format!(" {flag} {}", shell_quote(id));
        }
        if let (PromptMode::Arg, Some(p)) = (self.prompt, prompt) {
            if let Some(flag) = &self.prompt_flag {
                cmd += &format!(" {flag}");
            }
            cmd += &format!(" {}", shell_quote(p));
        }
        cmd
    }

    pub fn resume_command(&self, session_id: &str) -> Option<String> {
        self.resume
            .as_ref()
            .map(|r| r.replace("{session_id}", &shell_quote(session_id)))
    }

    /// Its program can be started: the launch command's first word after
    /// `VAR=value` prefixes is an executable file (a path), or one on
    /// `path` (`$PATH`). Fan-out checks this before creating Worktrees.
    pub fn available(&self, path: &str) -> bool {
        use std::os::unix::fs::PermissionsExt;
        let prog = self.program();
        if prog.is_empty() {
            return false;
        }
        let exec = |p: &std::path::Path| {
            p.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        };
        if prog.contains('/') {
            return exec(std::path::Path::new(prog));
        }
        path.split(':')
            .filter(|d| !d.is_empty())
            .any(|d| exec(&std::path::Path::new(d).join(prog)))
    }

    /// Program name the Agent process runs as: the basename of the launch
    /// command's first word after any `VAR=value` prefixes.
    pub fn binary(&self) -> &str {
        let word = self.program();
        word.rsplit('/').next().unwrap_or(word)
    }

    /// The launch command's first word after `VAR=value` prefixes (whose
    /// values may be quoted: `A='x y' prog`).
    fn program(&self) -> &str {
        let mut rest = self.command.trim_start();
        loop {
            let end = word_end(rest);
            let (w, tail) = rest.split_at(end);
            if !is_assignment(w) {
                return w;
            }
            rest = tail.trim_start();
        }
    }

    /// A recorded argv's arguments minus the prompt and session tokens:
    /// flags in `session_flags` (with their value), and every positional
    /// word that is not a flag's value (prompt, `resume <id>`, `run …`).
    /// ponytail: a value-taking flag missing from `value_flags` makes its
    /// value look positional, so it is dropped; a positional that is not a
    /// prompt (opencode's `[source]` dir) is dropped too.
    pub fn kept_args(&self, argv: &[String]) -> Vec<String> {
        let has = |list: &[String], f: &str| list.iter().any(|x| x == f);
        let strip = |f: &str| {
            has(&self.session_flags, f)
                || self.session_id_flag.as_deref() == Some(f)
                || self.prompt_flag.as_deref() == Some(f)
        };
        let mut out = Vec::new();
        let mut args = argv.iter().skip(1).peekable();
        while let Some(a) = args.next() {
            if a == "--" {
                break; // the rest is positional
            }
            if !a.starts_with('-') || a == "-" {
                continue; // positional: prompt or session word
            }
            let (name, inline) = match a.split_once('=') {
                Some((n, _)) if a.starts_with("--") => (n, true),
                _ => (a.as_str(), false),
            };
            let takes = !inline
                && (has(&self.value_flags, name)
                    || self.session_id_flag.as_deref() == Some(name)
                    || self.prompt_flag.as_deref() == Some(name));
            // Optional values (`--resume [id]`) never start with '-'.
            let value = takes
                .then(|| args.next_if(|v| !v.starts_with('-')))
                .flatten();
            if !strip(name) {
                out.push(a.clone());
                out.extend(value.cloned());
            }
        }
        out
    }

    /// Shell command that restarts a recorded [`Invocation`] (R11.2):
    /// `VAR='v' … 'argv0' <kept args> <resume args with the id>`, or without
    /// the resume args when there is no session to resume.
    pub fn invocation_command(&self, inv: &Invocation, session_id: Option<&str>) -> Option<String> {
        let argv0 = inv.argv.first()?;
        let kept = self.kept_args(&inv.argv);
        let mut words: Vec<String> = inv
            .env
            .iter()
            .filter(|(k, _)| is_name(k))
            .map(|(k, v)| format!("{k}={}", shell_quote(v)))
            .collect();
        words.push(shell_quote(argv0));
        words.extend(kept.iter().map(|a| shell_quote(a)));
        if let (Some(id), Some(template)) = (session_id, &self.resume) {
            // Keep recorded options; skipping a valued template flag must skip
            // its value too, or that value becomes an unintended prompt.
            let mut args = shell_words(template)
                .into_iter()
                .skip_while(|word| is_assignment(word))
                .skip(1)
                .peekable();
            while let Some(word) = args.next() {
                let name = word.split_once('=').map_or(word, |(name, _)| name);
                let duplicate = word.starts_with('-')
                    && kept.iter().any(|arg| {
                        arg.split_once('=').map_or(arg.as_str(), |(name, _)| name) == name
                    });
                if duplicate {
                    if !word.contains('=') && self.value_flags.iter().any(|flag| flag == name) {
                        args.next_if(|value| !value.starts_with('-'));
                    }
                } else {
                    words.push(word.replace("{session_id}", &shell_quote(id)));
                }
            }
        }
        Some(words.join(" "))
    }
}

/// `NAME=value` shell prefix word.
/// A command's shell words, quotes kept (`a 'b c'` → `a`, `'b c'`).
pub fn shell_words(mut s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    loop {
        s = s.trim_start();
        if s.is_empty() {
            return out;
        }
        let (w, rest) = s.split_at(word_end(s));
        out.push(w);
        s = rest;
    }
}

/// Byte length of the shell word at the start of `s`: up to unquoted
/// whitespace, quotes kept.
fn word_end(s: &str) -> usize {
    let mut quote = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (None, '\'' | '"') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, c) if c.is_whitespace() => return i,
            _ => {}
        }
    }
    s.len()
}

pub(crate) fn is_assignment(w: &str) -> bool {
    w.split_once('=').is_some_and(|(k, _)| is_name(k))
}

fn is_name(k: &str) -> bool {
    !k.is_empty()
        && !k.starts_with(|c: char| c.is_ascii_digit())
        && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A one-shot run of an agent CLI (R128.3): Codex `exec`, Claude `-p` /
/// `--print`. `words` is an argv or a command's shell words (quotes and
/// `VAR=value` prefixes allowed); the agent is the first word whose base
/// name is `codex` or `claude` (an interpreter may come before it).
pub fn is_headless<S: AsRef<str>>(words: &[S]) -> bool {
    headless_program(words).is_some()
}

/// The agent of a one-shot run ([`is_headless`]): `codex` or `claude`.
pub fn headless_program<S: AsRef<str>>(words: &[S]) -> Option<&'static str> {
    let words: Vec<&str> = words
        .iter()
        .map(|w| w.as_ref().trim_matches(|c| c == '\'' || c == '"'))
        .collect();
    let base = |w: &str| w.rsplit('/').next().unwrap_or(w).to_string();
    let at = words
        .iter()
        .position(|w| matches!(base(w).as_str(), "codex" | "claude" | "codex.js"))?;
    let rest = &words[at + 1..];
    match base(words[at]).as_str() {
        "codex" | "codex.js" => rest.contains(&"exec").then_some("codex"),
        _ => rest
            .iter()
            .any(|w| *w == "-p" || *w == "--print" || w.starts_with("--print="))
            .then_some("claude"),
    }
}

/// Single-quote for POSIX shells.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Random RFC 4122 v4 UUID (for `--session-id`).
pub fn new_uuid() -> String {
    use std::io::Read;
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .expect("read /dev/urandom");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r115_d_detect_finds_the_preset_a_foreground_program_runs() {
        let argv = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let b = builtins();
        assert_eq!(
            detect(&b, &argv(&["codex", "--yolo"])).as_deref(),
            Some("codex")
        );
        assert_eq!(
            detect(&b, &argv(&["/usr/local/bin/agy"])).as_deref(),
            Some("agy")
        );
        assert_eq!(
            detect(
                &b,
                &argv(&["node", "--x", "/usr/lib/@openai/codex/bin/codex.js"])
            )
            .as_deref(),
            Some("codex")
        );
        assert_eq!(
            detect(&b, &argv(&["bash", "/tmp/bin/opencode"])).as_deref(),
            Some("opencode")
        );
        assert_eq!(detect(&b, &argv(&["bash", "-c", "codex"])), None);
        assert_eq!(detect(&b, &argv(&["vim", "codex.md"])), None);
        // A second Preset on Claude's program: the one named like it wins.
        let o = BTreeMap::from([(
            "reviewer".to_string(),
            PresetOverride {
                command: Some("claude --model opus".into()),
                ..Default::default()
            },
        )]);
        let r = registry(&o).unwrap();
        assert_eq!(detect(&r, &argv(&["claude"])).as_deref(), Some("claude"));
        // A Preset run through an interpreter matches no script.
        let o = BTreeMap::from([(
            "mine".to_string(),
            PresetOverride {
                command: Some("bash ~/bin/agent.sh".into()),
                ..Default::default()
            },
        )]);
        let r = registry(&o).unwrap();
        assert_eq!(detect(&r, &argv(&["bash", "build.sh"])), None);
    }

    #[test]
    fn r115_f_icon_expands_home_and_overrides_a_builtin() {
        let o = BTreeMap::from([(
            "codex".to_string(),
            PresetOverride {
                icon: Some("~/icons/openai.svg".into()),
                ..Default::default()
            },
        )]);
        let r = registry(&o).unwrap();
        let home = std::env::var("HOME").unwrap();
        assert_eq!(
            r["codex"].icon.as_deref(),
            Some(std::path::Path::new(&format!("{home}/icons/openai.svg")))
        );
        assert_eq!(r["claude"].icon, None);
        let t: PresetOverride = toml::from_str("icon = \"/x/claude.png\"").unwrap();
        assert_eq!(t.icon.as_deref(), Some("/x/claude.png"));
    }

    #[test]
    fn r16_1_builtins_cover_four_agents_with_their_strategies() {
        let b = builtins();
        assert_eq!(
            b.keys().collect::<Vec<_>>(),
            ["agy", "claude", "codex", "opencode"]
        );
        // Claude gets a RADE-chosen id; the others report theirs (R16.4).
        assert!(b["claude"].session_id_flag.is_some());
        assert!(["agy", "codex", "opencode"]
            .iter()
            .all(|a| b[*a].session_id_flag.is_none()));
        assert_eq!(b["codex"].hooks, HookKind::Codex);
        assert!(b.values().all(|p| p.resume.is_some()));
    }

    #[test]
    fn launch_builds_quoted_commands() {
        let b = builtins();
        assert_eq!(
            b["claude"].launch(Some("say 'hi'"), Some("u-1")),
            r"claude --session-id 'u-1' 'say '\''hi'\'''"
        );
        assert_eq!(
            b["codex"].launch(Some("x"), Some("ignored")),
            "codex --no-daemon 'x'"
        );
        assert_eq!(b["agy"].launch(Some("x"), Some("ignored")), "agy -i 'x'");
        assert_eq!(
            b["agy"].resume_command("c1").unwrap(),
            "agy --conversation 'c1'"
        );
        assert_eq!(b["opencode"].launch(None, None), "opencode");
        assert_eq!(
            b["codex"].resume_command("abc").unwrap(),
            "codex --no-daemon resume 'abc'"
        );
    }

    #[test]
    fn type_mode_leaves_prompt_out_of_the_command() {
        let mut p = builtins()["codex"].clone();
        p.prompt = PromptMode::Type;
        assert_eq!(p.launch(Some("x"), None), "codex --no-daemon");
    }

    #[test]
    fn r16_2_config_overrides_and_adds_presets() {
        let o: BTreeMap<String, PresetOverride> = toml::from_str(
            "[claude]\ncommand = \"claude --model opus\"\n[fake]\ncommand = \"fake-agent\"\nprompt = \"type\"\n",
        )
        .unwrap();
        let r = registry(&o).unwrap();
        assert_eq!(r["claude"].command, "claude --model opus");
        assert_eq!(
            r["claude"].hooks,
            HookKind::Claude,
            "unset fields keep built-in"
        );
        assert_eq!(r["fake"].prompt, PromptMode::Type);
        assert_eq!(r["fake"].hooks, HookKind::None);
        let bad: BTreeMap<String, PresetOverride> =
            toml::from_str("[nocmd]\nprompt = \"arg\"\n").unwrap();
        assert!(registry(&bad).unwrap_err().contains("nocmd"));
        assert!(toml::from_str::<BTreeMap<String, PresetOverride>>("[x]\nbogus = 1\n").is_err());
    }

    fn argv(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn r11_2_kept_args_drop_prompt_and_session_tokens() {
        let b = builtins();
        let kept = |agent: &str, a: &[&str]| b[agent].kept_args(&argv(a)).join(" ");
        assert_eq!(
            kept(
                "claude",
                &[
                    "claude",
                    "--dangerously-skip-permissions",
                    "--model",
                    "opus",
                    "do x"
                ]
            ),
            "--dangerously-skip-permissions --model opus"
        );
        assert_eq!(kept("claude", &["claude", "--session-id", "u", "p"]), "");
        assert_eq!(
            kept(
                "claude",
                &["claude", "-r", "--verbose", "--resume=abc", "-c", "-p"]
            ),
            "--verbose"
        );
        assert_eq!(
            kept(
                "codex",
                &["codex", "--no-daemon", "--yolo", "-m", "gpt", "p"]
            ),
            "--no-daemon --yolo -m gpt"
        );
        assert_eq!(
            kept(
                "codex",
                &[
                    "codex",
                    "-c",
                    "key=value",
                    "--profile",
                    "work",
                    "resume",
                    "id-1"
                ]
            ),
            "-c key=value --profile work"
        );
        assert_eq!(kept("codex", &["codex", "resume", "--last"]), "");
        assert_eq!(
            kept(
                "opencode",
                &["opencode", "-m", "a/b", "-s", "ses", "--prompt", "hi", "run", "msg"]
            ),
            "-m a/b"
        );
        assert_eq!(
            kept(
                "agy",
                &[
                    "agy",
                    "--dangerously-skip-permissions",
                    "-i",
                    "p",
                    "--conversation",
                    "c"
                ]
            ),
            "--dangerously-skip-permissions"
        );
    }

    #[test]
    fn r11_2_invocation_command_env_prefix_resume_and_dedupe() {
        let b = builtins();
        let inv = Invocation {
            argv: argv(&["claude", "--dangerously-skip-permissions", "it's x"]),
            env: BTreeMap::from([("CLAUDE_CONFIG_DIR".into(), "/h/.claude-work".into())]),
        };
        assert_eq!(
            b["claude"].invocation_command(&inv, Some("u-1")).unwrap(),
            "CLAUDE_CONFIG_DIR='/h/.claude-work' 'claude' '--dangerously-skip-permissions' --resume 'u-1'"
        );
        assert_eq!(
            b["claude"].invocation_command(&inv, None).unwrap(),
            "CLAUDE_CONFIG_DIR='/h/.claude-work' 'claude' '--dangerously-skip-permissions'"
        );
        let codex = Invocation {
            argv: argv(&["codex", "--no-daemon", "--yolo", "-m", "gpt", "p"]),
            env: BTreeMap::from([("bad key;x".into(), "v".into())]),
        };
        assert_eq!(
            b["codex"].invocation_command(&codex, Some("t")).unwrap(),
            "'codex' '--no-daemon' '--yolo' '-m' 'gpt' resume 't'",
            "--no-daemon not repeated; invalid env names skipped"
        );
    }

    #[test]
    fn recorded_resume_drops_duplicate_flags_with_their_template_values() {
        let mut preset = builtins().remove("claude").unwrap();
        preset.resume = Some("claude --model sonnet --effort low --permission-mode acceptEdits --resume {session_id}".into());
        let inv = Invocation {
            argv: argv(&[
                "claude",
                "--model",
                "opus",
                "--effort",
                "high",
                "--permission-mode",
                "plan",
                "old prompt",
            ]),
            env: BTreeMap::new(),
        };
        assert_eq!(
            preset.invocation_command(&inv, Some("sid")).unwrap(),
            "'claude' '--model' 'opus' '--effort' 'high' '--permission-mode' 'plan' --resume 'sid'"
        );
    }

    #[test]
    fn recorded_resume_deduplicates_inline_flags_and_keeps_quoted_template_words() {
        let mut preset = builtins().remove("claude").unwrap();
        preset.resume = Some("claude --model=sonnet --effort low --add-dir '/work/my app' --system-prompt 'read this first' --resume {session_id}".into());
        let inv = Invocation {
            argv: argv(&["claude", "--model", "opus", "--effort=high"]),
            env: BTreeMap::new(),
        };
        assert_eq!(preset.invocation_command(&inv, Some("sid")).unwrap(), "'claude' '--model' 'opus' '--effort=high' --add-dir '/work/my app' --system-prompt 'read this first' --resume 'sid'");
        preset.resume = Some("claude --model 'sonnet model' --resume {session_id}".into());
        assert_eq!(
            preset.invocation_command(&inv, Some("sid")).unwrap(),
            "'claude' '--model' 'opus' '--effort=high' --resume 'sid'"
        );
    }

    #[test]
    fn r16_2_env_prefixed_command_override() {
        let o: BTreeMap<String, PresetOverride> = toml::from_str(
            "[claude]\ncommand = \"CLAUDE_CONFIG_DIR=/home/u/.claude-work claude --dangerously-skip-permissions\"\n",
        )
        .unwrap();
        let p = &registry(&o).unwrap()["claude"];
        assert_eq!(p.binary(), "claude");
        assert_eq!(
            p.launch(Some("do x"), Some("u-1")),
            "CLAUDE_CONFIG_DIR=/home/u/.claude-work claude --dangerously-skip-permissions --session-id 'u-1' 'do x'"
        );
        assert_eq!(builtins()["codex"].binary(), "codex");
        let fake: BTreeMap<String, PresetOverride> = toml::from_str(
            "[fake]\ncommand = \"/t/fake-agent.sh\"\nconfig_env = [\"FAKE_CONFIG_DIR\"]\nvalue_flags = [\"--model\"]\n",
        )
        .unwrap();
        let f = &registry(&fake).unwrap()["fake"];
        assert_eq!(f.binary(), "fake-agent.sh");
        assert_eq!(f.config_env, ["FAKE_CONFIG_DIR"]);
    }

    #[test]
    fn uuid_is_v4_shaped_and_random() {
        let a = new_uuid();
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "4");
        assert_ne!(a, new_uuid());
    }

    #[test]
    fn r128_c_one_shot_runs() {
        let argv = |s: &str| {
            shell_words(s)
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        };
        for yes in [
            "codex exec draw",
            "codex --no-daemon exec --skip-git-repo-check -s workspace-write -C /x 'p'",
            "codex -m x exec",
            "/usr/bin/codex exec",
            "node /usr/lib/node_modules/@openai/codex/bin/codex.js exec hi",
            "'codex' '--no-daemon' 'exec' 'hi'",
            "claude -p hi",
            "CLAUDE_CONFIG_DIR=/x claude --print",
            "claude --print=json",
        ] {
            assert!(is_headless(&argv(yes)), "{yes}");
        }
        for no in [
            "codex",
            "codex resume X",
            "codex --yolo",
            "claude --resume X",
            "claude 'fix exec bug'",
            "zsh -ic 'clv \"Đọc TASK.md\"'",
            "bash ~/bin/agent.sh -p",
            "",
        ] {
            assert!(!is_headless(&argv(no)), "{no}");
        }
    }
}
