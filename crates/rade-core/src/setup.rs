//! `rade setup` (PRD R16.3–R16.4): install Agent hooks that report Agent
//! Status through `rade status … --hook`. Local files only, no socket.
//! Merges into existing config without removing user entries, backs up the
//! original, is idempotent. Paths honor HOME, XDG_CONFIG_HOME,
//! CLAUDE_CONFIG_DIR (plus every `~/.claude-*` config dir), and CODEX_HOME.
//!
//! Every hook is `[ -z "$RADE_PANE_ID" ] || rade status … --hook`: in a
//! non-RADE session it exits 0 at once; `--hook` makes `rade` silent and
//! exit 0 when RADE is unreachable.

use crate::paths::{home_dir, Env};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use toml_edit::{value, Array, ArrayOfTables, DocumentMut, Item, Table};

pub const AGENTS: [&str; 4] = ["claude", "codex", "agy", "opencode"];

const SKILL: &str = include_str!("../../../skills/rade/SKILL.md");
/// R27: Workspace setup with `rade workspace` (installed next to `rade`).
const WS_SKILL: &str = include_str!("../../../skills/rade-workspace/SKILL.md");
/// R32: Projects and layered config (installed next to `rade`).
const CONFIG_SKILL: &str = include_str!("../../../skills/rade-config/SKILL.md");
/// Skills 0.7.1 installed for the engines removed in 1.0.0 (R101.1): `setup`
/// removes its copies of them.
const REMOVED_SKILLS: [&str; 5] = [
    "rade-task",
    "rade-batch",
    "rade-initiative",
    "rade-design",
    "rade-report",
];

/// Codex turn-complete fallback (no trust gate); the JSON payload is `$1`.
const CODEX_NOTIFY: [&str; 4] = [
    "sh",
    "-c",
    r#"[ -z "$RADE_PANE_ID" ] || rade status done --hook "$1""#,
    "rade-notify",
];

pub const CODEX_TRUST_NOTICE: &str = "Codex runs these hooks only after you trust them once: start `codex`, type /hooks, and trust the RADE entries. Until then only turn-complete is reported (via `notify`).";

const OPENCODE_PLUGIN: &str = r#"// Installed by `rade setup` (RADE): reports Agent Status to RADE.
// No-op outside RADE Terminals (RADE_PANE_ID / RADE_SOCKET unset).
export const Rade = async ({ $ }) => {
  if (!process.env.RADE_PANE_ID || !process.env.RADE_SOCKET) return {};
  const subagents = new Set();
  const status = (sid, ...args) =>
    $`rade status ${args} --hook --agent opencode --session-id ${sid ?? ""}`.quiet().nothrow();
  return {
    event: async ({ event }) => {
      const p = event.properties ?? {};
      const sid = p.sessionID ?? p.info?.id;
      if (event.type === "session.created") {
        if (p.info?.parentID) subagents.add(p.info.id);
        else await status(sid);
        return;
      }
      if (subagents.has(sid)) return;
      if (event.type === "permission.asked" || event.type === "question.asked") await status(sid, "waiting");
      else if (event.type === "session.error") await status(sid, "error");
      else if (event.type === "session.status" && p.status?.type === "busy") await status(sid, "running");
      else if (event.type === "session.status" && p.status?.type === "idle") await status(sid, "done");
    },
  };
};
"#;

/// A file `setup` would write.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub path: PathBuf,
    /// Current content; `None` if the file does not exist.
    pub old: Option<String>,
    pub new: String,
    /// Remove the file instead (a skill `setup` no longer installs, R101.1):
    /// its `.rade-bak-*` copies go too, and its folder if that leaves it empty.
    pub remove: bool,
}

fn hook_cmd(args: &str) -> String {
    format!(r#"[ -z "$RADE_PANE_ID" ] || rade status {args} --hook"#)
}

fn is_ours(command: &str) -> bool {
    command.contains("rade status") && command.contains("--hook")
}

/// Every Claude Code config dir, so each way the user starts Claude reports
/// status (`claude` and an alias with `CLAUDE_CONFIG_DIR=~/.claude-work`):
/// `$CLAUDE_CONFIG_DIR`, `~/.claude`, and each `~/.claude-*` holding a
/// `settings.json`.
fn claude_dirs(env: Env) -> Vec<PathBuf> {
    let home = home_dir(env);
    let mut extra: Vec<PathBuf> = std::fs::read_dir(&home)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".claude-"))
        .map(|e| e.path())
        .filter(|p| p.join("settings.json").is_file())
        .collect();
    extra.sort();
    let mut dirs: Vec<PathBuf> = env("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .into_iter()
        .collect();
    let ws = workspace_dirs(env, "CLAUDE_CONFIG_DIR");
    for d in std::iter::once(home.join(".claude")).chain(extra).chain(ws) {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    dirs
}

fn codex_dir(env: Env) -> PathBuf {
    env("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir(env).join(".codex"))
}

/// `$CODEX_HOME` (or `~/.codex`) and each Workspace's `CODEX_HOME`.
fn codex_dirs(env: Env) -> Vec<PathBuf> {
    let mut dirs = vec![codex_dir(env)];
    for d in workspace_dirs(env, "CODEX_HOME") {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    dirs
}

/// `var` from the `[env]` of every registered Workspace (R25.5), then the
/// `config_dir` of each of its `[[account]]` tables whose agent uses `var`
/// (R108.5): the roots come from `state.json`, read as plain JSON (no
/// migration, no write).
fn workspace_dirs(env: Env, var: &str) -> Vec<PathBuf> {
    let home = home_dir(env);
    let state: Value = std::fs::read_to_string(crate::paths::state_file(env))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for w in state["workspaces"].as_array().into_iter().flatten() {
        let Some(root) = w["root"].as_str() else {
            continue;
        };
        let file = crate::workspace::WorkspaceFile::read(Path::new(root)).unwrap_or_default();
        for (k, v) in file.env(&home) {
            if k == var && !dirs.contains(&PathBuf::from(&v)) {
                dirs.push(v.into());
            }
        }
        // Each table on its own, so one bad entry doesn't hide the others.
        for a in &file.account {
            let kind = crate::account::AgentKind::parse(&a.agent);
            if kind.is_some_and(|k| k.var() == var) && !a.config_dir.trim().is_empty() {
                let d = PathBuf::from(crate::workspace::expand_tilde(a.config_dir.trim(), &home));
                if !dirs.contains(&d) {
                    dirs.push(d);
                }
            }
        }
    }
    dirs
}

fn xdg_config(env: Env) -> PathBuf {
    env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir(env).join(".config"))
}

/// (event, matcher, `rade status` args) per Agent (research: agent-integration.md).
fn agent_hooks(agent: &str) -> Vec<(&'static str, Option<&'static str>, String)> {
    let a = |event, matcher, args: &str| (event, matcher, hook_cmd(args));
    match agent {
        "claude" => vec![
            a("SessionStart", None, "--agent claude"),
            a("UserPromptSubmit", None, "running"),
            a("Notification", Some("permission_prompt"), "waiting"),
            a("Notification", Some("elicitation_dialog"), "waiting"),
            a("Stop", None, "done"),
            a("StopFailure", None, "error"),
        ],
        "codex" => vec![
            a("SessionStart", None, "--agent codex"),
            a("UserPromptSubmit", None, "running"),
            a("PermissionRequest", None, "waiting"),
            a("Stop", None, "done"),
        ],
        // PreInvocation/Stop read JSON from stdout: print `{}` (no-op).
        // No SessionStart: the first PreInvocation reports the id. No
        // "needs input" event exists; PreToolUse is left alone (it decides
        // permissions).
        "agy" => vec![
            (
                "PreInvocation",
                None,
                hook_cmd("running --agent agy") + "; echo '{}'",
            ),
            // Stop requires a `decision`; anything but "continue" lets it stop.
            (
                "Stop",
                None,
                hook_cmd("done") + "; echo '{\"decision\":\"stop\"}'",
            ),
        ],
        _ => vec![],
    }
}

fn read(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Produces a file's new content from its old content and the Agent name.
type Merge = fn(Option<&str>, &str) -> Result<String, String>;

/// Claude Code's files in one config dir: hooks and the skills.
fn claude_files(d: &Path) -> Vec<(PathBuf, Merge)> {
    vec![
        (d.join("settings.json"), merge_json),
        (d.join("skills/rade/SKILL.md"), |_, _| Ok(SKILL.into())),
        (d.join("skills/rade-workspace/SKILL.md"), |_, _| {
            Ok(WS_SKILL.into())
        }),
        (d.join("skills/rade-config/SKILL.md"), |_, _| {
            Ok(CONFIG_SKILL.into())
        }),
    ]
}

/// Removals of the old skills (`REMOVED_SKILLS`) installed under `skills`.
fn old_skills(skills: &Path) -> Result<Vec<Change>, String> {
    let mut out = Vec::new();
    for name in REMOVED_SKILLS {
        let path = skills.join(name).join("SKILL.md");
        if let Some(old) = read(&path)? {
            out.push(Change {
                path,
                old: Some(old),
                new: String::new(),
                remove: true,
            });
        }
    }
    Ok(out)
}

/// Files that would change for `agents` (unchanged files are left out).
pub fn plan(agents: &[String], env: Env) -> Result<Vec<Change>, String> {
    let mut out = Vec::new();
    for agent in agents {
        let files: Vec<(PathBuf, Merge)> = match agent.as_str() {
            "claude" => {
                for d in claude_dirs(env) {
                    out.extend(changes(claude_files(&d), agent)?);
                    out.extend(old_skills(&d.join("skills"))?);
                }
                continue;
            }
            "codex" => {
                let mut f: Vec<(PathBuf, Merge)> = codex_dirs(env)
                    .into_iter()
                    .map(|d| (d.join("config.toml"), merge_codex as Merge))
                    .collect();
                // Only rc files that exist: never create a shell config.
                for rc in [".zshrc", ".bashrc"] {
                    let path = home_dir(env).join(rc);
                    if path.is_file() {
                        f.push((path, merge_shell_rc));
                    }
                }
                f
            }
            "agy" => vec![
                (home_dir(env).join(".gemini/config/hooks.json"), merge_agy),
                (
                    home_dir(env).join(".gemini/antigravity-cli/skills/rade/SKILL.md"),
                    |_, _| Ok(SKILL.into()),
                ),
                (
                    home_dir(env).join(".gemini/antigravity-cli/skills/rade-workspace/SKILL.md"),
                    |_, _| Ok(WS_SKILL.into()),
                ),
                (
                    home_dir(env).join(".gemini/antigravity-cli/skills/rade-config/SKILL.md"),
                    |_, _| Ok(CONFIG_SKILL.into()),
                ),
            ],
            "opencode" => vec![(xdg_config(env).join("opencode/plugins/rade.js"), |_, _| {
                Ok(OPENCODE_PLUGIN.into())
            })],
            other => {
                return Err(format!(
                    "unknown agent '{other}' (one of {})",
                    AGENTS.join(", ")
                ))
            }
        };
        out.extend(changes(files, agent)?);
        if agent == "agy" {
            out.extend(old_skills(
                &home_dir(env).join(".gemini/antigravity-cli/skills"),
            )?);
        }
    }
    Ok(out)
}

/// The changes that install `agent` (`claude` or `codex`) into one config
/// dir only: a Workspace's own `CLAUDE_CONFIG_DIR` / `CODEX_HOME` (R25.2),
/// and `doctor`'s "hooks installed" check (R26).
pub fn plan_dir(agent: &str, dir: &Path) -> Result<Vec<Change>, String> {
    let files = match agent {
        "claude" => claude_files(dir),
        "codex" => vec![(dir.join("config.toml"), merge_codex as Merge)],
        other => return Err(format!("no config dir for agent '{other}'")),
    };
    let mut out = changes(files, agent)?;
    if agent == "claude" {
        out.extend(old_skills(&dir.join("skills"))?);
    }
    Ok(out)
}

fn changes(files: Vec<(PathBuf, Merge)>, agent: &str) -> Result<Vec<Change>, String> {
    let mut out = Vec::new();
    for (path, merge) in files {
        let old = read(&path)?;
        let new = merge(old.as_deref(), agent)
            .map_err(|e| format!("{}: {e} (left unchanged)", path.display()))?;
        if old.as_deref() != Some(new.as_str()) {
            out.push(Change {
                path,
                old,
                new,
                remove: false,
            });
        }
    }
    Ok(out)
}

/// Write the changes; an existing file is first copied to
/// `<file>.rade-bak-<stamp>`. Returns the backups made. A symlink stays a
/// symlink (its target is written), the file's mode is kept, and the new
/// content is synced before it replaces the old one.
pub fn apply(changes: &[Change], stamp: &str) -> std::io::Result<Vec<PathBuf>> {
    use std::io::Write;
    let mut backups = Vec::new();
    for c in changes {
        if c.remove {
            remove_skill(&c.path)?;
            continue;
        }
        if let Some(dir) = c.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut target = c.path.clone();
        let mut mode = None;
        if c.old.is_some() {
            let mut name = c.path.clone().into_os_string();
            name.push(format!(".rade-bak-{stamp}"));
            std::fs::copy(&c.path, &name)?;
            backups.push(PathBuf::from(name));
            target = c.path.canonicalize()?;
            mode = Some(std::fs::metadata(&target)?.permissions());
        }
        let mut tmp = target.clone().into_os_string();
        tmp.push(".rade-tmp");
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(c.new.as_bytes())?;
        if let Some(m) = mode {
            f.set_permissions(m)?;
        }
        f.sync_all()?;
        std::fs::rename(&tmp, &target)?;
    }
    Ok(backups)
}

/// A removed skill's `SKILL.md`, the `SKILL.md.rade-bak-*` copies earlier
/// runs left beside it, and its folder when nothing else is in it.
fn remove_skill(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let Some(dir) = path.parent() else {
        return Ok(());
    };
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let bak = format!("{name}.rade-bak-");
    for e in std::fs::read_dir(dir)?.flatten() {
        if e.file_name().to_string_lossy().starts_with(&bak) && e.path().is_file() {
            std::fs::remove_file(e.path())?;
        }
    }
    // Not empty (a file of the user's): the folder stays.
    let _ = std::fs::remove_dir(dir);
    Ok(())
}

/// `text` with a block between the marker lines `begin` and `end` holding
/// `body`: an existing block is replaced in place, else one is appended
/// after a blank line. Everything outside the block is kept as is.
pub fn marked_block(text: &str, begin: &str, end: &str, body: &str) -> String {
    let body = if body.is_empty() || body.ends_with('\n') {
        body.to_string()
    } else {
        format!("{body}\n")
    };
    let block = format!("{begin}\n{body}{end}\n");
    if let Some(i) = text.find(&format!("{begin}\n")) {
        if let Some(j) = text[i..].find(end).map(|j| i + j + end.len()) {
            let rest = text[j..].strip_prefix('\n').unwrap_or(&text[j..]);
            return format!("{}{block}{rest}", &text[..i]);
        }
    }
    let sep = match text {
        "" => "",
        t if t.ends_with('\n') => "\n",
        _ => "\n\n",
    };
    format!("{text}{sep}{block}")
}

/// Antigravity `hooks.json`: named groups; RADE owns the `rade` group.
fn merge_agy(text: Option<&str>, _: &str) -> Result<String, String> {
    let mut root: Value = match text.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => serde_json::from_str(t).map_err(|e| format!("invalid JSON: {e}"))?,
        None => json!({}),
    };
    let obj = root
        .as_object_mut()
        .ok_or("top level is not a JSON object")?;
    let mut group = serde_json::Map::new();
    for (event, _, command) in agent_hooks("agy") {
        group.insert(
            event.into(),
            // PreInvocation/Stop take a flat handler list (agy 1.2.14 rejects
            // the `{hooks: [...]}` group form with "must specify 'command'").
            json!([{"type": "command", "command": command, "timeout": 10}]),
        );
    }
    obj.insert("rade".into(), Value::Object(group));
    let mut out = serde_json::to_string_pretty(&root).unwrap();
    out.push('\n');
    Ok(out)
}

/// Claude Code `settings.json`: `hooks.<Event>[] = {matcher?, hooks: [{type, command}]}`.
fn merge_json(text: Option<&str>, agent: &str) -> Result<String, String> {
    let mut root: Value = match text.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => serde_json::from_str(t).map_err(|e| format!("invalid JSON: {e}"))?,
        None => json!({}),
    };
    let obj = root
        .as_object_mut()
        .ok_or("top level is not a JSON object")?;
    let hooks = obj
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("`hooks` is not an object")?;
    // Drop our old entries, keep everything else.
    for groups in hooks.values_mut().filter_map(Value::as_array_mut) {
        groups.retain_mut(|g| {
            let Some(list) = g.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let before = list.len();
            list.retain(|h| !h["command"].as_str().is_some_and(is_ours));
            !(list.is_empty() && before > 0)
        });
    }
    hooks.retain(|_, v| v.as_array().is_none_or(|a| !a.is_empty()));
    for (event, matcher, command) in agent_hooks(agent) {
        let hook = json!({"type": "command", "command": command});
        let mut group = json!({"hooks": [hook]});
        if let Some(m) = matcher {
            group = json!({"matcher": m, "hooks": group["hooks"]});
        }
        hooks
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or(format!("`hooks.{event}` is not an array"))?
            .push(group);
    }
    if agent == "claude" {
        statusline(obj);
    }
    let mut out = serde_json::to_string_pretty(&root).unwrap();
    out.push('\n');
    Ok(out)
}

/// The statusline command RADE sets (R109.1).
pub const STATUSLINE: &str = "rade status --statusline";

/// `s` single-quoted for `sh` (`'` → `'\''`): `sh` parses it back to `s`
/// byte for byte.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The `statusLine.command` RADE sets in place of `previous` (R109.1): the
/// wrapper, with the previous command as `--then`; a command that already is
/// the wrapper is kept as it is (never nested).
pub fn statusline_command(previous: Option<&str>) -> String {
    match previous.filter(|p| !p.trim().is_empty()) {
        Some(p) if p.trim_start().starts_with(STATUSLINE) => p.to_string(),
        Some(p) => format!("{STATUSLINE} --then {}", sh_quote(p)),
        None => STATUSLINE.to_string(),
    }
}

/// Claude Code's `statusLine` set to the RADE wrapper (R109.1): other keys
/// (`padding`) kept; a `statusLine` that is not `{"type": "command"}` is
/// left untouched (see [`statusline_note`]).
fn statusline(obj: &mut serde_json::Map<String, Value>) {
    match obj.get_mut("statusLine") {
        None | Some(Value::Null) => {
            obj.insert(
                "statusLine".into(),
                json!({"type": "command", "command": STATUSLINE}),
            );
        }
        Some(Value::Object(s)) if s.get("type").and_then(Value::as_str) == Some("command") => {
            let new = statusline_command(s.get("command").and_then(Value::as_str));
            s.insert("command".into(), Value::String(new));
        }
        Some(_) => {}
    }
}

/// The note `rade setup` prints for a `settings.json` whose `statusLine` it
/// leaves alone (not a command), else `None`.
pub fn statusline_note(path: &Path, text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    match v.get("statusLine") {
        None | Some(Value::Null) => None,
        Some(Value::Object(s)) if s.get("type").and_then(Value::as_str) == Some("command") => None,
        Some(_) => Some(format!(
            "{}: `statusLine` is not a command, so it was left as it is and Claude Code limits are not recorded for this config dir (R109.1).",
            path.display()
        )),
    }
}

/// Codex `config.toml`: `[[hooks.<Event>]]` with `[[hooks.<Event>.hooks]]`,
/// plus the legacy `notify` fallback if the user has none.
const RC_BEGIN: &str = "# >>> rade: codex --no-daemon in RADE Panes >>>";

/// A `codex` typed by hand in a RADE Pane uses Codex's shared app-server
/// daemon, which runs hooks without the Pane's RADE_* env, so status and the
/// resumable session id never reach RADE. Inside RADE Panes only, make
/// `codex` mean `codex --no-daemon` (unless the flag is already given).
fn merge_shell_rc(text: Option<&str>, _: &str) -> Result<String, String> {
    Ok(marked_block(
        text.unwrap_or_default(),
        RC_BEGIN,
        "# <<< rade <<<",
        "if [ -n \"$RADE_PANE_ID\" ]; then\n\
         \x20 codex() { case \" $* \" in *\" --no-daemon \"*) command codex \"$@\" ;; *) command codex --no-daemon \"$@\" ;; esac; }\n\
         fi\n",
    ))
}

fn merge_codex(text: Option<&str>, _: &str) -> Result<String, String> {
    let mut doc: DocumentMut = text
        .unwrap_or("")
        .parse()
        .map_err(|e| format!("invalid TOML: {e}"))?;
    if !doc.contains_key("notify") {
        doc["notify"] = value(CODEX_NOTIFY.iter().copied().collect::<Array>());
    }
    let hooks = doc
        .entry("hooks")
        .or_insert_with(|| {
            let mut t = Table::new();
            t.set_implicit(true);
            Item::Table(t)
        })
        .as_table_mut()
        .ok_or("`hooks` is not a table")?;
    for (_, groups) in hooks.iter_mut() {
        let Some(groups) = groups.as_array_of_tables_mut() else {
            continue;
        };
        for g in groups.iter_mut() {
            if let Some(list) = g.get_mut("hooks").and_then(Item::as_array_of_tables_mut) {
                list.retain(|h| !h.get("command").and_then(Item::as_str).is_some_and(is_ours));
            }
        }
        groups.retain(|g| {
            g.get("hooks")
                .and_then(Item::as_array_of_tables)
                .is_none_or(|l| !l.is_empty())
        });
    }
    // Remove empty event arrays we emptied.
    let empty: Vec<String> = hooks
        .iter()
        .filter(|(_, v)| v.as_array_of_tables().is_some_and(|a| a.is_empty()))
        .map(|(k, _)| k.to_string())
        .collect();
    for k in empty {
        hooks.remove(&k);
    }
    for (event, matcher, command) in agent_hooks("codex") {
        let mut hook = Table::new();
        hook["type"] = value("command");
        hook["command"] = value(command);
        let mut list = ArrayOfTables::new();
        list.push(hook);
        let mut group = Table::new();
        if let Some(m) = matcher {
            group["matcher"] = value(m);
        }
        group.insert("hooks", Item::ArrayOfTables(list));
        hooks
            .entry(event)
            .or_insert(Item::ArrayOfTables(ArrayOfTables::new()))
            .as_array_of_tables_mut()
            .ok_or(format!("`hooks.{event}` is not an array of tables"))?
            .push(group);
    }
    Ok(doc.to_string())
}

/// Things the user must know after setup.
pub fn notes(agents: &[String], env: Env) -> Vec<String> {
    let mut n = Vec::new();
    if agents.iter().any(|a| a == "codex") {
        n.push(CODEX_TRUST_NOTICE.to_string());
        let cfg = read(&codex_dir(env).join("config.toml")).ok().flatten();
        if cfg.is_some_and(|c| c.contains("notify") && !c.contains("rade-notify")) {
            n.push("Codex: your own `notify` setting was kept, so turn-complete reports rely on the hooks.".into());
        }
    }
    if agents.iter().any(|a| a == "claude") {
        let dirs: Vec<String> = claude_dirs(env)
            .iter()
            .map(|d| d.display().to_string())
            .collect();
        n.push(format!(
            "Claude Code: hooks and the `rade` skill go into each config dir: {}.",
            dirs.join(", ")
        ));
        for d in claude_dirs(env) {
            let p = d.join("settings.json");
            if let Some(note) = read(&p)
                .ok()
                .flatten()
                .and_then(|t| statusline_note(&p, &t))
            {
                n.push(note);
            }
        }
    }
    if agents.iter().any(|a| a == "agy") {
        n.push("Antigravity (agy): reports running/done only; it has no hook for \"needs input\". The `rade` skill is installed under ~/.gemini/antigravity-cli/skills.".into());
    }
    n.push("Codex / OpenCode: add docs/AGENTS-snippet.md (RADE repo) to your AGENTS.md so they can drive other agents.".into());
    n
}

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;
